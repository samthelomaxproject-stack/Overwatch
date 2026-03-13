from datetime import datetime, timedelta, timezone
from typing import List, Optional

from dotenv import load_dotenv
from fastapi import FastAPI, HTTPException, Query

from .acled import fetch_acled
from .db import get_conn, init_db

load_dotenv()
app = FastAPI(title="Overwatch OSINT Conflict API", version="0.1.0")


@app.on_event("startup")
def startup():
    init_db()


def upsert_events(events: List[dict]):
    with get_conn() as conn:
        for ev in events:
            external_id = str(ev.get("event_id_cnty") or ev.get("event_id_no_cnty") or "")
            if not external_id:
                # fallback deterministic key
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


@app.get("/health")
def health():
    return {"ok": True, "service": "overwatch-osint", "version": "0.1.0"}


@app.post("/api/ingest/acled")
def ingest_acled(days: int = Query(7, ge=1, le=30), country: Optional[str] = None):
    try:
        events = fetch_acled(days=days, country=country)
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))
    upsert_events(events)
    return {"ok": True, "ingested": len(events)}


@app.get("/api/events")
def get_events(
    window: str = Query("7d", pattern="^(1d|7d|30d)$"),
    country: Optional[str] = None,
    event_types: Optional[str] = None,
    limit: int = Query(1000, ge=1, le=5000),
):
    days = {"1d": 1, "7d": 7, "30d": 30}[window]
    cutoff = (datetime.now(timezone.utc) - timedelta(days=days)).date().isoformat()

    where = ["date(event_date) >= date(?)"]
    vals = [cutoff]

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
                "sources": [{"name": s["source_name"], "url": s["source_url"]} for s in src],
            })
        return out
