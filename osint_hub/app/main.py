import os
import threading
import time
from datetime import datetime, timedelta, timezone
from typing import Dict, List, Optional, Tuple

from dotenv import load_dotenv
from fastapi import FastAPI, HTTPException, Query

from .acled import fetch_acled
from . import events, rss_ingest, gdelt_ingest
from . import conflict_events, conflict_ingest

load_dotenv()  # Load from .env in current directory (bundled)

# Also load user-specific overrides if present (e.g., SHODAN_API_KEY)
# This allows secrets to persist across app updates without modifying the bundle.
user_env = os.path.expanduser("~/.config/overwatch/.env")
if os.path.exists(user_env):
    load_dotenv(user_env, override=True)
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


@app.on_event("startup")
def startup():
    global _ingest_thread_started, _shodan_thread_started
    # init_db()  # Schema already exists, skip to avoid mismatch errors
    events.init_events_db()
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
    category: Optional[str] = None,
    since: Optional[str] = None,
    country: Optional[str] = None,
    limit: int = Query(100, ge=1, le=100000),
):
    # Cache-first endpoint. Never triggers live Shodan API.
    cat_list = [c.strip() for c in (category or "").split(",") if c.strip()]
    configured = bool(os.getenv("SHODAN_API_KEY", "").strip())

    items = get_shodan_events(bbox=None, categories=cat_list or None, since=since, country=country, limit=limit)
    region_keys = sorted(list({str(i.get("region_key") or "") for i in items if i.get("region_key")}))
    return {
        "items": items,
        "meta": {
            "source": "hub_shodan_cache",
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "configured": configured,
            "region_keys": region_keys,
            "cache_only": True,
        },
    }


@app.get("/api/shodan/events/since")
def shodan_events_since(
    since: str,
    category: Optional[str] = None,
    country: Optional[str] = None,
    limit: int = Query(100, ge=1, le=200),
):
    cat_list = [c.strip() for c in (category or "").split(",") if c.strip()]
    items = get_shodan_events(bbox=None, categories=cat_list or None, since=since, country=country, limit=limit)
    return {"items": items, "meta": {"source": "hub_shodan_cache", "generated_at": datetime.now(timezone.utc).isoformat(), "cache_only": True}}


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
    category: Optional[str] = None,
    force: bool = Query(False),
):
    cat_list = [c.strip() for c in (category or "").split(",") if c.strip()]
    result = discover_shodan(bbox=None, categories=cat_list or None, force_refresh=force)
    if result.get("status") == "budget_exceeded":
        return {
            "status": "budget_exceeded",
            "message": "Shodan query skipped due to credit limits",
            "meta": result,
        }
    return {
        "accepted": True,
        "force": force,
        "categories": cat_list,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "meta": result,
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


# ========== OSINT Events Endpoints ==========

@app.get("/api/osint/events")
def osint_events_get(
    limit: int = Query(100, ge=1, le=10000),
    since: Optional[str] = None,
    event_type: Optional[str] = None,
    min_confidence: float = Query(0.0, ge=0.0, le=1.0),
    bbox: Optional[str] = None
):
    """Get normalized OSINT events from RSS/GDELT sources."""
    items = events.get_events(
        limit=limit,
        since=since,
        event_type=event_type,
        min_confidence=min_confidence,
        bbox=bbox
    )
    return {"items": items, "count": len(items)}


@app.get("/api/osint/events/meta")
def osint_events_meta():
    """Get OSINT events metadata and statistics."""
    return events.get_events_meta()


@app.post("/api/osint/ingest/rss")
def osint_ingest_rss():
    """Manually trigger RSS feed ingestion."""
    return rss_ingest.ingest_all_rss_feeds()


@app.post("/api/osint/ingest/gdelt")
def osint_ingest_gdelt(
    hours_back: int = Query(24, ge=1, le=168),
    max_results: int = Query(100, ge=1, le=500)
):
    """Manually trigger GDELT ingestion."""
    return gdelt_ingest.ingest_gdelt(hours_back=hours_back, max_results=max_results)


@app.post("/api/osint/ingest/all")
def osint_ingest_all():
    """Trigger both RSS and GDELT ingestion."""
    rss_result = rss_ingest.ingest_all_rss_feeds()
    gdelt_result = gdelt_ingest.ingest_gdelt()
    
    return {
        "ok": True,
        "rss": rss_result,
        "gdelt": gdelt_result,
        "total_new": rss_result.get("total_new", 0) + gdelt_result.get("new", 0)
    }


# ========== Conflict Events Endpoints ==========

@app.post("/api/conflict/refresh")
def conflict_refresh():
    """Manually refresh all conflict events (RSS + GDELT)."""
    return conflict_ingest.refresh_all_conflict_events()


@app.get("/api/conflict/events")
def conflict_events_get(window: str = Query("week", pattern="^(day|week|month)$")):
    """Get conflict events for time window."""
    events_list = conflict_events.get_events(window=window, limit=None)
    return {"items": events_list, "count": len(events_list), "window": window}


@app.get("/api/conflict/feeds")
def conflict_feeds_list():
    """List all RSS feeds."""
    return {"feeds": conflict_events.list_feeds()}


@app.post("/api/conflict/feeds")
def conflict_feeds_create(name: str, url: str, category: str = "general"):
    """Add new RSS feed."""
    return conflict_events.add_feed(name, url, category)


@app.patch("/api/conflict/feeds/{feed_id}")
def conflict_feeds_update(feed_id: int, enabled: Optional[bool] = None, name: Optional[str] = None):
    """Update feed settings."""
    success = conflict_events.update_feed(feed_id, enabled=enabled, name=name)
    if not success:
        raise HTTPException(404, "Feed not found")
    return {"ok": True}


@app.delete("/api/conflict/feeds/{feed_id}")
def conflict_feeds_delete(feed_id: int):
    """Delete feed."""
    success = conflict_events.delete_feed(feed_id)
    return {"ok": success}


@app.post("/api/conflict/feeds/{feed_id}/test")
def conflict_feeds_test(feed_id: int):
    """Test fetch a single feed."""
    feeds = conflict_events.list_feeds()
    feed = next((f for f in feeds if f["id"] == feed_id), None)
    if not feed:
        raise HTTPException(404, "Feed not found")
    
    return conflict_ingest.ingest_rss_feed(feed["id"], feed["name"], feed["url"])
