"""
Conflict Events - Source-aware persistent storage for Conflict layer.
Supports GDELT + operator-configured RSS feeds with 30-day retention.
"""
import hashlib
import json
import sqlite3
from datetime import datetime, timezone, timedelta
from typing import Dict, List, Optional

from .db import get_conn


def init_conflict_db():
    """Initialize conflict events tables."""
    with get_conn() as conn:
        conn.executescript("""
            CREATE TABLE IF NOT EXISTS conflict_feeds (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                url TEXT NOT NULL UNIQUE,
                category TEXT DEFAULT 'general',
                enabled INTEGER DEFAULT 1,
                created_at TEXT NOT NULL,
                last_checked_at TEXT,
                last_success_at TEXT,
                last_error TEXT
            );
            
            CREATE INDEX IF NOT EXISTS idx_feeds_enabled ON conflict_feeds(enabled);
            
            CREATE TABLE IF NOT EXISTS conflict_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                dedupe_key TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                summary TEXT,
                source_type TEXT NOT NULL,
                source_name TEXT,
                source_url TEXT,
                published_at TEXT,
                ingested_at TEXT NOT NULL,
                event_type TEXT,
                location_name TEXT,
                lat REAL,
                lon REAL,
                raw_json TEXT
            );
            
            CREATE INDEX IF NOT EXISTS idx_events_dedupe ON conflict_events(dedupe_key);
            CREATE INDEX IF NOT EXISTS idx_events_published ON conflict_events(published_at DESC);
            CREATE INDEX IF NOT EXISTS idx_events_ingested ON conflict_events(ingested_at DESC);
            CREATE INDEX IF NOT EXISTS idx_events_geo ON conflict_events(lat, lon) WHERE lat IS NOT NULL;
        """)
        conn.commit()


def generate_dedupe_key(source_name: str, source_url: str, title: str, published_at: Optional[str]) -> str:
    """Generate deterministic dedupe key."""
    parts = [
        source_name.lower().strip(),
        source_url.lower().strip(),
        title.lower().strip(),
        published_at or ""
    ]
    return hashlib.sha256("|".join(parts).encode()).hexdigest()[:16]


def seed_default_feeds():
    """Seed default conflict-relevant RSS feeds."""
    default_feeds = [
        {"name": "BBC World News", "url": "https://feeds.bbci.co.uk/news/world/rss.xml", "category": "news"},
        {"name": "New York Times World", "url": "https://rss.nytimes.com/services/xml/rss/nyt/World.xml", "category": "news"},
        {"name": "Al Jazeera", "url": "https://www.aljazeera.com/xml/rss/all.xml", "category": "news"},
        {"name": "Reuters World", "url": "https://www.reutersagency.com/feed/?taxonomy=best-topics&post_type=best", "category": "news"},
    ]
    
    now = datetime.now(timezone.utc).isoformat()
    with get_conn() as conn:
        for feed in default_feeds:
            try:
                conn.execute("""
                    INSERT INTO conflict_feeds (name, url, category, enabled, created_at)
                    VALUES (?, ?, ?, 1, ?)
                """, (feed["name"], feed["url"], feed["category"], now))
            except sqlite3.IntegrityError:
                pass  # Already exists
        conn.commit()


def list_feeds() -> List[Dict]:
    """Get all configured feeds."""
    with get_conn() as conn:
        rows = conn.execute("""
            SELECT id, name, url, category, enabled, created_at, 
                   last_checked_at, last_success_at, last_error
            FROM conflict_feeds
            ORDER BY name
        """).fetchall()
    
    return [{
        "id": r[0],
        "name": r[1],
        "url": r[2],
        "category": r[3],
        "enabled": bool(r[4]),
        "created_at": r[5],
        "last_checked_at": r[6],
        "last_success_at": r[7],
        "last_error": r[8]
    } for r in rows]


def add_feed(name: str, url: str, category: str = "general") -> Dict:
    """Add new RSS feed."""
    now = datetime.now(timezone.utc).isoformat()
    with get_conn() as conn:
        cursor = conn.execute("""
            INSERT INTO conflict_feeds (name, url, category, enabled, created_at)
            VALUES (?, ?, ?, 1, ?)
        """, (name, url, category, now))
        conn.commit()
        feed_id = cursor.lastrowid
    
    return {"id": feed_id, "name": name, "url": url, "category": category, "enabled": True}


def update_feed(feed_id: int, enabled: Optional[bool] = None, name: Optional[str] = None) -> bool:
    """Update feed settings."""
    updates = []
    values = []
    
    if enabled is not None:
        updates.append("enabled = ?")
        values.append(1 if enabled else 0)
    if name is not None:
        updates.append("name = ?")
        values.append(name)
    
    if not updates:
        return False
    
    values.append(feed_id)
    with get_conn() as conn:
        conn.execute(f"UPDATE conflict_feeds SET {', '.join(updates)} WHERE id = ?", values)
        conn.commit()
    
    return True


def delete_feed(feed_id: int) -> bool:
    """Delete feed."""
    with get_conn() as conn:
        conn.execute("DELETE FROM conflict_feeds WHERE id = ?", (feed_id,))
        conn.commit()
    return True


def upsert_event(event: Dict) -> Dict[str, int]:
    """Insert conflict event, skip if duplicate."""
    now = datetime.now(timezone.utc).isoformat()
    
    dedupe_key = generate_dedupe_key(
        event.get("source_name", ""),
        event.get("source_url", ""),
        event["title"],
        event.get("published_at")
    )
    
    with get_conn() as conn:
        # Check for existing
        existing = conn.execute("SELECT id FROM conflict_events WHERE dedupe_key = ?", (dedupe_key,)).fetchone()
        
        if existing:
            return {"inserted": 0, "duplicate": 1}
        
        conn.execute("""
            INSERT INTO conflict_events (
                dedupe_key, title, summary, source_type, source_name, source_url,
                published_at, ingested_at, event_type, location_name, lat, lon, raw_json
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """, (
            dedupe_key,
            event["title"],
            event.get("summary"),
            event["source_type"],
            event.get("source_name"),
            event.get("source_url"),
            event.get("published_at"),
            now,
            event.get("event_type", "other"),
            event.get("location_name"),
            event.get("lat"),
            event.get("lon"),
            event.get("raw_json")
        ))
        conn.commit()
    
    return {"inserted": 1, "duplicate": 0}


def get_events(window: str = "week", limit: int = 500) -> List[Dict]:
    """Get conflict events for time window."""
    now = datetime.now(timezone.utc)
    
    if window == "day":
        cutoff = now - timedelta(days=1)
    elif window == "month":
        cutoff = now - timedelta(days=30)
    else:  # week
        cutoff = now - timedelta(days=7)
    
    cutoff_str = cutoff.isoformat()
    
    with get_conn() as conn:
        rows = conn.execute("""
            SELECT id, title, summary, source_type, source_name, source_url,
                   published_at, event_type, location_name, lat, lon
            FROM conflict_events
            WHERE published_at >= ? AND lat IS NOT NULL AND lon IS NOT NULL
            ORDER BY published_at DESC
            LIMIT ?
        """, (cutoff_str, limit)).fetchall()
    
    return [{
        "id": r[0],
        "title": r[1],
        "summary": r[2],
        "source_type": r[3],
        "source_name": r[4],
        "source_url": r[5],
        "published_at": r[6],
        "event_type": r[7] or "other",
        "location": r[8],
        "lat": r[9],
        "lon": r[10]
    } for r in rows]


def prune_old_events() -> int:
    """Delete events older than 30 days."""
    cutoff = (datetime.now(timezone.utc) - timedelta(days=30)).isoformat()
    
    with get_conn() as conn:
        cursor = conn.execute("DELETE FROM conflict_events WHERE published_at < ?", (cutoff,))
        conn.commit()
        deleted = cursor.rowcount
    
    return deleted
