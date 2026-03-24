"""
OSINT Events - Normalized event model for RSS, GDELT, and other sources.
Hub-first architecture: ingestion runs on hub, clients consume normalized events.
"""
import hashlib
import sqlite3
from datetime import datetime, timezone
from typing import Dict, List, Optional

from .db import get_conn

# Event type classification
EVENT_TYPES = {
    "conflict", "protest", "strike", "security_incident", 
    "military_activity", "disaster", "other"
}

def init_events_db():
    """Initialize OSINT events tables."""
    with get_conn() as conn:
        conn.executescript("""
            CREATE TABLE IF NOT EXISTS osint_events (
                id TEXT PRIMARY KEY,
                external_id TEXT,
                source_type TEXT NOT NULL,
                source_name TEXT,
                source_url TEXT,
                title TEXT NOT NULL,
                raw_text TEXT,
                summary TEXT,
                published_at TEXT,
                ingested_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                event_type TEXT,
                country TEXT,
                admin1 TEXT,
                city TEXT,
                lat REAL,
                lon REAL,
                geocode_confidence REAL DEFAULT 0.0,
                classification_confidence REAL DEFAULT 0.0,
                confidence_score REAL DEFAULT 0.0,
                ai_tags TEXT,
                status TEXT DEFAULT 'active',
                duplicate_of TEXT,
                fingerprint TEXT,
                raw_payload TEXT
            );
            
            CREATE INDEX IF NOT EXISTS idx_events_published ON osint_events(published_at DESC);
            CREATE INDEX IF NOT EXISTS idx_events_type ON osint_events(event_type);
            CREATE INDEX IF NOT EXISTS idx_events_geo ON osint_events(lat, lon) WHERE lat IS NOT NULL;
            CREATE INDEX IF NOT EXISTS idx_events_fingerprint ON osint_events(fingerprint);
            CREATE INDEX IF NOT EXISTS idx_events_status ON osint_events(status);
            
            CREATE TABLE IF NOT EXISTS osint_event_sources (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_id TEXT NOT NULL,
                source_name TEXT,
                source_url TEXT,
                source_type TEXT,
                published_at TEXT,
                snippet TEXT,
                FOREIGN KEY (event_id) REFERENCES osint_events(id)
            );
            
            CREATE INDEX IF NOT EXISTS idx_event_sources_event ON osint_event_sources(event_id);
            
            CREATE TABLE IF NOT EXISTS osint_ingest_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_type TEXT NOT NULL,
                source_name TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                status TEXT,
                new_count INTEGER DEFAULT 0,
                updated_count INTEGER DEFAULT 0,
                duplicate_count INTEGER DEFAULT 0,
                error TEXT
            );
            
            CREATE INDEX IF NOT EXISTS idx_ingest_runs_time ON osint_ingest_runs(started_at DESC);
            
            CREATE TABLE IF NOT EXISTS osint_feed_state (
                source_name TEXT PRIMARY KEY,
                last_polled_at TEXT,
                last_success_at TEXT,
                etag TEXT,
                last_modified TEXT,
                last_error TEXT
            );
        """)
        conn.commit()


def generate_fingerprint(title: str, url: Optional[str] = None) -> str:
    """Generate fingerprint for deduplication."""
    normalized = title.lower().strip()
    if url:
        normalized += url.lower().strip()
    return hashlib.sha256(normalized.encode()).hexdigest()[:16]


def upsert_event(event: Dict) -> Dict[str, int]:
    """Insert or update an OSINT event. Returns counts."""
    now = datetime.now(timezone.utc).isoformat()
    event_id = event.get("id") or f"evt_{generate_fingerprint(event['title'], event.get('source_url'))}"
    fingerprint = generate_fingerprint(event["title"], event.get("source_url"))
    
    with get_conn() as conn:
        # Check for existing
        existing = conn.execute("SELECT id FROM osint_events WHERE id=? OR fingerprint=?", 
                                (event_id, fingerprint)).fetchone()
        
        is_new = not existing
        
        conn.execute("""
            INSERT INTO osint_events (
                id, external_id, source_type, source_name, source_url,
                title, raw_text, summary, published_at, ingested_at, updated_at,
                event_type, country, admin1, city, lat, lon,
                geocode_confidence, classification_confidence, confidence_score,
                ai_tags, status, fingerprint, raw_payload
            ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
            ON CONFLICT(id) DO UPDATE SET
                updated_at=excluded.updated_at,
                summary=excluded.summary,
                event_type=excluded.event_type,
                lat=excluded.lat,
                lon=excluded.lon,
                geocode_confidence=excluded.geocode_confidence,
                classification_confidence=excluded.classification_confidence,
                confidence_score=excluded.confidence_score,
                status=excluded.status
        """, (
            event_id,
            event.get("external_id"),
            event["source_type"],
            event.get("source_name"),
            event.get("source_url"),
            event["title"],
            event.get("raw_text"),
            event.get("summary"),
            event.get("published_at"),
            event.get("ingested_at", now),
            now,
            event.get("event_type", "other"),
            event.get("country"),
            event.get("admin1"),
            event.get("city"),
            event.get("lat"),
            event.get("lon"),
            event.get("geocode_confidence", 0.0),
            event.get("classification_confidence", 0.0),
            event.get("confidence_score", 0.0),
            event.get("ai_tags"),
            event.get("status", "active"),
            fingerprint,
            event.get("raw_payload")
        ))
        conn.commit()
        
    return {"inserted": 1 if is_new else 0, "updated": 0 if is_new else 1}


def get_events(
    limit: int = 100,
    since: Optional[str] = None,
    event_type: Optional[str] = None,
    min_confidence: float = 0.0,
    bbox: Optional[str] = None
) -> List[Dict]:
    """Get normalized map-ready events."""
    where = ["status='active'", "lat IS NOT NULL", "lon IS NOT NULL"]
    vals = []
    
    if since:
        where.append("datetime(updated_at) > datetime(?)")
        vals.append(since)
    
    if event_type and event_type in EVENT_TYPES:
        where.append("event_type=?")
        vals.append(event_type)
    
    if min_confidence > 0:
        where.append("confidence_score >= ?")
        vals.append(min_confidence)
    
    if bbox:
        try:
            parts = [float(x.strip()) for x in bbox.split(",")]
            if len(parts) == 4:
                min_lon, min_lat, max_lon, max_lat = parts
                where.append("lon BETWEEN ? AND ?")
                vals.extend([min_lon, max_lon])
                where.append("lat BETWEEN ? AND ?")
                vals.extend([min_lat, max_lat])
        except:
            pass
    
    sql = f"""
        SELECT * FROM osint_events
        WHERE {' AND '.join(where)}
        ORDER BY datetime(published_at) DESC
        LIMIT ?
    """
    vals.append(limit)
    
    with get_conn() as conn:
        rows = conn.execute(sql, vals).fetchall()
    
    items = []
    for r in rows:
        # Map to frontend format
        icon_color = {
            "conflict": "#ef4444",
            "protest": "#f59e0b",
            "strike": "#eab308",
            "security_incident": "#dc2626",
            "military_activity": "#7c2d12",
            "disaster": "#b91c1c",
            "other": "#6b7280"
        }.get(r["event_type"], "#6b7280")
        
        items.append({
            "id": r["id"],
            "type": "osint_event",
            "lat": r["lat"],
            "lon": r["lon"],
            "event_type": r["event_type"],
            "title": r["title"],
            "summary": r["summary"] or r["title"],
            "source_type": r["source_type"],
            "source_name": r["source_name"],
            "source_url": r["source_url"],
            "published_at": r["published_at"],
            "confidence": r["confidence_score"],
            "country": r["country"],
            "city": r["city"],
            "style": {
                "icon": "divIcon",
                "color": icon_color
            }
        })
    
    return items


def get_events_meta() -> Dict:
    """Get event statistics."""
    with get_conn() as conn:
        total = conn.execute("SELECT COUNT(*) FROM osint_events WHERE status='active'").fetchone()[0]
        geolocated = conn.execute("SELECT COUNT(*) FROM osint_events WHERE status='active' AND lat IS NOT NULL").fetchone()[0]
        
        by_type = {}
        for row in conn.execute("SELECT event_type, COUNT(*) FROM osint_events WHERE status='active' GROUP BY event_type"):
            by_type[row[0] or "other"] = row[1]
        
        by_source = {}
        for row in conn.execute("SELECT source_type, COUNT(*) FROM osint_events WHERE status='active' GROUP BY source_type"):
            by_source[row[0]] = row[1]
        
        last_rss = conn.execute("SELECT MAX(finished_at) FROM osint_ingest_runs WHERE source_type='rss' AND status='ok'").fetchone()[0]
        last_gdelt = conn.execute("SELECT MAX(finished_at) FROM osint_ingest_runs WHERE source_type='gdelt' AND status='ok'").fetchone()[0]
    
    return {
        "total_events": total,
        "geolocated_events": geolocated,
        "by_type": by_type,
        "by_source": by_source,
        "last_rss_ingest": last_rss,
        "last_gdelt_ingest": last_gdelt
    }
