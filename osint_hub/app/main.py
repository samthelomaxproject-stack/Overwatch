import os
import threading
import time
from datetime import datetime, timedelta, timezone
from typing import Dict, List, Optional, Tuple

from dotenv import load_dotenv
from fastapi import FastAPI, HTTPException, Query

from .acled import fetch_acled
from .db import get_conn, init_db
from .shodan import (
    discover_shodan,
    get_shodan_events,
    get_shodan_meta,
    get_categories,
    get_detail,
    seed_mock_findings,
    clear_mock_findings,
    scheduler_enabled,
    scheduler_interval_sec,
)

load_dotenv()
app = FastAPI(title="Overwatch OSINT Conflict API", version="0.2.0")

CACHE_TTL_SECONDS = int(os.getenv("CACHE_TTL_SECONDS", "300"))
AUTO_INGEST_ENABLED = os.getenv("ACLED_AUTO_INGEST", "true").lower() in ("1", "true", "yes")
AUTO_INGEST_INTERVAL_MIN = int(os.getenv("ACLED_AUTO_INGEST_INTERVAL_MIN", "30"))
AUTO_INGEST_DAYS = int(os.getenv("ACLED_AUTO_INGEST_DAYS", "7"))

_cache: Dict[str, Tuple[float, list]] = {}
_cache_lock = threading.Lock()
_last_ingest_meta = {"at": None, "count": 0, "ok": None, "error": None}
_ingest_thread_started = False
_shodan_thread_started = False
_last_shodan_meta = {"at": None, "fetched": 0, "ok": None, "error": None}
_shodan_refresh_lock = threading.Lock()
_shodan_refresh_inflight = set()


@app.on_event("startup")
def startup():
    global _ingest_thread_started, _shodan_thread_started
    init_db()
    if AUTO_INGEST_ENABLED and not _ingest_thread_started:
        t = threading.Thread(target=_ingest_loop, daemon=True)
        t.start()
        _ingest_thread_started = True
    if scheduler_enabled() and not _shodan_thread_started:
        t = threading.Thread(target=_shodan_loop, daemon=True)
        t.start()
        _shodan_thread_started = True


def _clear_cache():
    with _cache_lock:
        _cache.clear()


def _get_cache(key: str):
    with _cache_lock:
        item = _cache.get(key)
        if not item:
            return None
        ts, payload = item
        if (time.time() - ts) > CACHE_TTL_SECONDS:
            _cache.pop(key, None)
            return None
        return payload


def _set_cache(key: str, payload: list):
    with _cache_lock:
        _cache[key] = (time.time(), payload)


def _ingest_once(days: int = AUTO_INGEST_DAYS, country: Optional[str] = None):
    events = fetch_acled(days=days, country=country)
    upsert_events(events)
    _clear_cache()
    _last_ingest_meta.update({
        "at": datetime.now(timezone.utc).isoformat(),
        "count": len(events),
        "ok": True,
        "error": None,
    })
    return len(events)


def _ingest_loop():
    while True:
        try:
            _ingest_once(days=AUTO_INGEST_DAYS)
        except Exception as e:
            _last_ingest_meta.update({
                "at": datetime.now(timezone.utc).isoformat(),
                "count": 0,
                "ok": False,
                "error": str(e),
            })
        time.sleep(max(5, AUTO_INGEST_INTERVAL_MIN) * 60)


def _shodan_discover_once(force_refresh: bool = False):
    try:
        result = discover_shodan(bbox=None, categories=None, force_refresh=force_refresh)
        _last_shodan_meta.update({
            "at": datetime.now(timezone.utc).isoformat(),
            "fetched": int(result.get("fetched", 0)),
            "ok": bool(result.get("ok", False)),
            "error": result.get("reason") if not result.get("ok", False) else None,
        })
        return result
    except Exception as e:
        _last_shodan_meta.update({
            "at": datetime.now(timezone.utc).isoformat(),
            "fetched": 0,
            "ok": False,
            "error": str(e),
        })
        raise


def _shodan_loop():
    while True:
        try:
            _shodan_discover_once(force_refresh=False)
        except Exception:
            pass
        time.sleep(max(30, scheduler_interval_sec()))


def _queue_shodan_refresh(bbox: Optional[str], categories: Optional[List[str]], force: bool = False) -> bool:
    key = f"{bbox or 'global'}|{','.join(sorted(categories or []))}|{int(force)}"
    with _shodan_refresh_lock:
        if key in _shodan_refresh_inflight:
            return False
        _shodan_refresh_inflight.add(key)

    def _run():
        try:
            discover_shodan(bbox=bbox, categories=categories, force_refresh=force)
        finally:
            with _shodan_refresh_lock:
                _shodan_refresh_inflight.discard(key)

    threading.Thread(target=_run, daemon=True).start()
    return True


def upsert_events(events: List[dict]):
    with get_conn() as conn:
        for ev in events:
            external_id = str(ev.get("event_id_cnty") or ev.get("event_id_no_cnty") or "")
            if not external_id:
                external_id = f"acled:{ev.get('event_date')}:{ev.get('latitude')}:{ev.get('longitude')}:{ev.get('event_type')}"

            conn.execute(
                """
                INSERT INTO conflict_events (
                  external_id, source_system, event_date, country, admin1, location,
                  latitude, longitude, event_type, sub_event_type, actor1, actor2,
                  fatalities, notes, source_scale, confidence_score, updated_at
                ) VALUES (?, 'acled', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
                ON CONFLICT(external_id) DO UPDATE SET
                  event_date=excluded.event_date,
                  country=excluded.country,
                  admin1=excluded.admin1,
                  location=excluded.location,
                  latitude=excluded.latitude,
                  longitude=excluded.longitude,
                  event_type=excluded.event_type,
                  sub_event_type=excluded.sub_event_type,
                  actor1=excluded.actor1,
                  actor2=excluded.actor2,
                  fatalities=excluded.fatalities,
                  notes=excluded.notes,
                  source_scale=excluded.source_scale,
                  confidence_score=excluded.confidence_score,
                  updated_at=CURRENT_TIMESTAMP
                """,
                (
                    external_id,
                    ev.get("event_date") or "",
                    ev.get("country") or "",
                    ev.get("admin1") or "",
                    ev.get("location") or "",
                    float(ev.get("latitude") or 0),
                    float(ev.get("longitude") or 0),
                    ev.get("event_type") or "",
                    ev.get("sub_event_type") or "",
                    ev.get("actor1") or "",
                    ev.get("actor2") or "",
                    int(ev.get("fatalities") or 0),
                    ev.get("notes") or "",
                    ev.get("source_scale") or "",
                    0.75,
                ),
            )

            row = conn.execute("SELECT id FROM conflict_events WHERE external_id=?", (external_id,)).fetchone()
            if row:
                event_id = row["id"]
                conn.execute("DELETE FROM event_sources WHERE event_id=?", (event_id,))
                src_raw = ev.get("source") or ""
                named_sources = [s.strip() for s in src_raw.split(";") if s.strip()]
                for i, sname in enumerate(named_sources):
                    conn.execute(
                        "INSERT INTO event_sources (event_id, source_name, source_type, is_primary) VALUES (?, ?, 'acled_named', ?)",
                        (event_id, sname, 1 if i == 0 else 0),
                    )
        conn.commit()


def _query_events(window: str, country: Optional[str], event_types: Optional[str], limit: int, date_from: Optional[str] = None, date_to: Optional[str] = None):
    cache_key = f"w={window}|c={country or ''}|t={event_types or ''}|l={limit}|df={date_from or ''}|dt={date_to or ''}"
    cached = _get_cache(cache_key)
    if cached is not None:
        return cached

    where = []
    vals: List[object] = []

    if date_from:
        where.append("date(event_date) >= date(?)")
        vals.append(date_from)
    if date_to:
        where.append("date(event_date) <= date(?)")
        vals.append(date_to)

    if not date_from and not date_to:
        days = {"1d": 1, "7d": 7, "30d": 30}[window]
        cutoff = (datetime.now(timezone.utc) - timedelta(days=days)).date().isoformat()
        where.append("date(event_date) >= date(?)")
        vals.append(cutoff)

    if country:
        where.append("country = ?")
        vals.append(country)

    if event_types:
        types = [x.strip() for x in event_types.split(",") if x.strip()]
        if types:
            where.append("event_type IN ({})".format(",".join(["?"] * len(types))))
            vals.extend(types)

    sql = f"""
      SELECT * FROM conflict_events
      WHERE {' AND '.join(where)}
      ORDER BY event_date DESC
      LIMIT ?
    """
    vals.append(limit)

    with get_conn() as conn:
        rows = conn.execute(sql, vals).fetchall()
        out = []
        for r in rows:
            src = conn.execute(
                "SELECT source_name, source_url FROM event_sources WHERE event_id=? ORDER BY is_primary DESC, id ASC",
                (r["id"],),
            ).fetchall()
            out.append({
                "id": r["id"],
                "external_id": r["external_id"],
                "event_date": r["event_date"],
                "country": r["country"],
                "admin1": r["admin1"],
                "location": r["location"],
                "latitude": r["latitude"],
                "longitude": r["longitude"],
                "event_type": r["event_type"],
                "sub_event_type": r["sub_event_type"],
                "actor1": r["actor1"],
                "actor2": r["actor2"],
                "fatalities": r["fatalities"],
                "notes": r["notes"],
                "source_scale": r["source_scale"],
                "confidence": r["confidence_score"],
                "updated_at": r["updated_at"],
                "sources": [{"name": s["source_name"], "url": s["source_url"]} for s in src],
            })

    _set_cache(cache_key, out)
    return out


@app.get("/health")
def health():
    return {
        "ok": True,
        "service": "overwatch-osint",
        "version": "0.2.0",
        "auto_ingest": {
            "enabled": AUTO_INGEST_ENABLED,
            "interval_min": AUTO_INGEST_INTERVAL_MIN,
            "days": AUTO_INGEST_DAYS,
            "last": _last_ingest_meta,
        },
        "shodan": {
            "scheduler_enabled": scheduler_enabled(),
            "scheduler_interval_sec": scheduler_interval_sec(),
            "last": _last_shodan_meta,
        },
    }


@app.post("/api/ingest/acled")
def ingest_acled(days: int = Query(7, ge=1, le=30), country: Optional[str] = None):
    try:
        count = _ingest_once(days=days, country=country)
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))
    return {"ok": True, "ingested": count}


@app.get("/api/events")
def get_events(
    window: str = Query("1d", pattern="^(1d|7d|30d)$"),
    country: Optional[str] = None,
    event_types: Optional[str] = None,
    date_from: Optional[str] = None,
    date_to: Optional[str] = None,
    limit: int = Query(1000, ge=1, le=5000),
):
    try:
        return _query_events(window=window, country=country, event_types=event_types, limit=limit, date_from=date_from, date_to=date_to)
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


@app.get("/api/events/since")
def get_events_since(
    since: str,
    country: Optional[str] = None,
    limit: int = Query(500, ge=1, le=5000),
):
    with get_conn() as conn:
        where = ["datetime(updated_at) > datetime(?)"]
        vals: List[object] = [since]
        if country:
            where.append("country = ?")
            vals.append(country)
        sql = f"""
            SELECT * FROM conflict_events
            WHERE {' AND '.join(where)}
            ORDER BY datetime(updated_at) DESC
            LIMIT ?
        """
        vals.append(limit)
        rows = conn.execute(sql, vals).fetchall()
        return [dict(r) for r in rows]


@app.get("/api/alerts/high-impact")
def high_impact_alerts(
    window: str = Query("1d", pattern="^(1d|7d|30d)$"),
    min_fatalities: int = Query(10, ge=1, le=10000),
    country: Optional[str] = None,
    limit: int = Query(200, ge=1, le=1000),
):
    events = _query_events(window=window, country=country, event_types=None, limit=5000)
    filtered = [e for e in events if int(e.get("fatalities") or 0) >= min_fatalities]
    return filtered[:limit]


@app.get("/api/meta")
def meta():
    with get_conn() as conn:
        countries = [r[0] for r in conn.execute("SELECT DISTINCT country FROM conflict_events WHERE country <> '' ORDER BY country ASC").fetchall()]
        event_types = [r[0] for r in conn.execute("SELECT DISTINCT event_type FROM conflict_events WHERE event_type <> '' ORDER BY event_type ASC").fetchall()]
    return {
        "countries": countries,
        "event_types": event_types,
        "presets": {
            "ukraine": ["Ukraine"],
            "middle-east": ["Israel", "Palestine", "Lebanon", "Syria", "Iraq", "Yemen", "Iran"],
            "africa-hotspots": ["Sudan", "DR Congo", "Somalia", "Mali", "Burkina Faso", "Niger", "Nigeria", "Ethiopia", "Mozambique"],
        },
    }


@app.get("/api/shodan/events")
def shodan_events(
    bbox: Optional[str] = None,
    category: Optional[str] = None,
    since: Optional[str] = None,
    stale: Optional[int] = Query(None, ge=1, le=720),
    country: Optional[str] = None,
    limit: int = Query(500, ge=1, le=5000),
):
    cat_list = [c.strip() for c in (category or "").split(",") if c.strip()]
    configured = bool(os.getenv("SHODAN_API_KEY", "").strip())
    if not configured:
        return {
            "items": [],
            "meta": {
                "source": "hub_shodan_cache",
                "generated_at": datetime.now(timezone.utc).isoformat(),
                "configured": False,
                "stale": False,
                "queued_refresh": False,
                "region_keys": [],
            },
        }

    queued = False
    if stale is not None:
        # stale hint triggers background refresh only; response remains cache-first.
        queued = _queue_shodan_refresh(bbox=bbox, categories=cat_list or None, force=False)

    items = get_shodan_events(bbox=bbox, categories=cat_list or None, since=since, country=country, limit=limit)
    region_keys = sorted(list({str(i.get("region_key") or "") for i in items if i.get("region_key")}))
    return {
        "items": items,
        "meta": {
            "source": "hub_shodan_cache",
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "configured": True,
            "stale": False,
            "queued_refresh": queued,
            "region_keys": region_keys,
        },
    }


@app.get("/api/shodan/events/since")
def shodan_events_since(
    since: str,
    category: Optional[str] = None,
    bbox: Optional[str] = None,
    country: Optional[str] = None,
    limit: int = Query(500, ge=1, le=5000),
):
    cat_list = [c.strip() for c in (category or "").split(",") if c.strip()]
    items = get_shodan_events(bbox=bbox, categories=cat_list or None, since=since, country=country, limit=limit)
    return {"items": items, "meta": {"source": "hub_shodan_cache", "generated_at": datetime.now(timezone.utc).isoformat()}}


@app.get("/api/shodan/meta")
def shodan_meta():
    return get_shodan_meta()


@app.post("/api/shodan/ingest")
def shodan_ingest(
    bbox: Optional[str] = None,
    category: Optional[str] = None,
    force: bool = Query(False),
):
    cat_list = [c.strip() for c in (category or "").split(",") if c.strip()]
    result = discover_shodan(bbox=bbox, categories=cat_list or None, force_refresh=force)
    _last_shodan_meta.update({
        "at": datetime.now(timezone.utc).isoformat(),
        "fetched": int(result.get("fetched", 0)),
        "ok": bool(result.get("ok", False)),
        "error": result.get("reason") if not result.get("ok", False) else None,
    })
    return result


@app.post("/api/shodan/refresh-region")
def shodan_refresh_region(
    bbox: Optional[str] = None,
    category: Optional[str] = None,
    force: bool = Query(False),
):
    cat_list = [c.strip() for c in (category or "").split(",") if c.strip()]
    queued = _queue_shodan_refresh(bbox=bbox, categories=cat_list or None, force=force)
    return {
        "accepted": True,
        "queued": queued,
        "force": force,
        "bbox": bbox,
        "categories": cat_list,
        "generated_at": datetime.now(timezone.utc).isoformat(),
    }


@app.get("/api/shodan/detail/{item_id}")
def shodan_detail(item_id: str):
    row = get_detail(item_id)
    if not row:
        raise HTTPException(status_code=404, detail="not found")
    return row


@app.get("/api/shodan/categories")
def shodan_categories():
    return {"items": get_categories()}


@app.post("/api/shodan/mock-seed")
def shodan_mock_seed():
    # verification-only helper; does not touch live discovery logic.
    return seed_mock_findings()


@app.post("/api/shodan/mock-clear")
def shodan_mock_clear():
    # cleanup helper to remove only mock records.
    return clear_mock_findings()
