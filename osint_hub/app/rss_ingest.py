"""
RSS/News feed ingestion for OSINT events pipeline.
"""
import os
import re
import feedparser
import requests
from datetime import datetime, timezone
from typing import Dict, List, Optional

from .events import upsert_event, generate_fingerprint
from .geocoding import extract_location_text, geocode_text
from .db import get_conn


def _event_relevant(title: str, description: str) -> bool:
    """Basic keyword check for event relevance."""
    text = (title + " " + description).lower()
    keywords = [
        "attack", "killed", "dead", "bombing", "explosion", "protest",
        "strike", "riot", "military", "troops", "deployed", "conflict",
        "war", "battle", "shooting", "incident", "security", "police",
        "army", "navy", "air force", "drone", "missile", "earthquake",
        "flood", "disaster", "emergency", "evacuation", "casualties"
    ]
    return any(kw in text for kw in keywords)


def _classify_event_type(title: str, description: str) -> str:
    """Rule-based event type classification."""
    text = (title + " " + description).lower()
    
    if any(w in text for w in ["attack", "bombing", "shooting", "killed", "dead", "battle", "war"]):
        return "conflict"
    if any(w in text for w in ["protest", "demonstrat", "rally", "march"]):
        return "protest"
    if any(w in text for w in ["strike", "walkout", "labor"]):
        return "strike"
    if any(w in text for w in ["military", "troops", "deployed", "forces", "army", "navy"]):
        return "military_activity"
    if any(w in text for w in ["earthquake", "flood", "hurricane", "disaster", "wildfire"]):
        return "disaster"
    if any(w in text for w in ["incident", "security", "police", "arrest"]):
        return "security_incident"
    
    return "other"


def ingest_rss_feed(feed_url: str, source_name: Optional[str] = None) -> Dict:
    """Ingest a single RSS feed."""
    started = datetime.now(timezone.utc).isoformat()
    source_name = source_name or feed_url.split('/')[2] if '/' in feed_url else feed_url
    
    new_count = 0
    updated_count = 0
    duplicate_count = 0
    error = None
    
    try:
        # Fetch feed
        resp = requests.get(feed_url, timeout=30, headers={"User-Agent": "Overwatch/0.2 OSINT Hub"})
        resp.raise_for_status()
        
        feed = feedparser.parse(resp.content)
        
        for entry in feed.entries[:50]:  # Limit per feed
            title = entry.get("title", "").strip()
            if not title:
                continue
            
            description = entry.get("description", "") or entry.get("summary", "")
            link = entry.get("link", "")
            
            # Check if event-relevant
            if not _event_relevant(title, description):
                duplicate_count += 1
                continue
            
            # Extract published time
            published = None
            if hasattr(entry, "published_parsed") and entry.published_parsed:
                try:
                    published = datetime(*entry.published_parsed[:6], tzinfo=timezone.utc).isoformat()
                except:
                    pass
            
            # Extract location and geocode
            location_text = extract_location_text(title + " " + description)
            geo_result = geocode_text(location_text) if location_text else None
            
            # Classify event type
            event_type = _classify_event_type(title, description)
            
            # Generate summary (fallback to title)
            summary = description[:200] if description else title
            
            # Calculate confidence
            confidence = 0.5  # Base
            if geo_result and geo_result.get("confidence", 0) > 0.7:
                confidence += 0.2
            if event_type != "other":
                confidence += 0.1
            if len(description) > 100:
                confidence += 0.1
            confidence = min(1.0, confidence)
            
            event = {
                "id": f"rss_{generate_fingerprint(title, link)}",
                "external_id": link,
                "source_type": "rss",
                "source_name": source_name,
                "source_url": link,
                "title": title,
                "raw_text": description,
                "summary": summary,
                "published_at": published,
                "ingested_at": started,
                "event_type": event_type,
                "country": geo_result.get("country") if geo_result else None,
                "admin1": geo_result.get("admin1") if geo_result else None,
                "city": geo_result.get("city") if geo_result else None,
                "lat": geo_result.get("lat") if geo_result else None,
                "lon": geo_result.get("lon") if geo_result else None,
                "geocode_confidence": geo_result.get("confidence", 0.0) if geo_result else 0.0,
                "classification_confidence": 0.7 if event_type != "other" else 0.3,
                "confidence_score": confidence,
                "status": "active"
            }
            
            result = upsert_event(event)
            new_count += result["inserted"]
            updated_count += result["updated"]
        
        status = "ok"
        
        # Update feed state
        with get_conn() as conn:
            conn.execute("""
                INSERT INTO osint_feed_state (source_name, last_polled_at, last_success_at)
                VALUES (?, ?, ?)
                ON CONFLICT(source_name) DO UPDATE SET
                    last_polled_at=excluded.last_polled_at,
                    last_success_at=excluded.last_success_at
            """, (source_name, started, started))
            conn.commit()
        
    except Exception as e:
        status = "error"
        error = str(e)
        
        # Update feed state with error
        with get_conn() as conn:
            conn.execute("""
                INSERT INTO osint_feed_state (source_name, last_polled_at, last_error)
                VALUES (?, ?, ?)
                ON CONFLICT(source_name) DO UPDATE SET
                    last_polled_at=excluded.last_polled_at,
                    last_error=excluded.last_error
            """, (source_name, started, error))
            conn.commit()
    
    finished = datetime.now(timezone.utc).isoformat()
    
    # Log ingest run
    with get_conn() as conn:
        conn.execute("""
            INSERT INTO osint_ingest_runs (
                source_type, source_name, started_at, finished_at, status,
                new_count, updated_count, duplicate_count, error
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        """, ("rss", source_name, started, finished, status, new_count, updated_count, duplicate_count, error))
        conn.commit()
    
    return {
        "ok": status == "ok",
        "source": source_name,
        "new": new_count,
        "updated": updated_count,
        "duplicates": duplicate_count,
        "error": error
    }


def ingest_all_rss_feeds() -> Dict:
    """Ingest all configured RSS feeds."""
    feed_list = os.getenv("RSS_FEED_LIST", "").strip()
    if not feed_list:
        return {"ok": False, "reason": "no_feeds_configured", "feeds": []}
    
    feeds = [f.strip() for f in feed_list.split(";") if f.strip()]
    results = []
    
    for feed_url in feeds:
        result = ingest_rss_feed(feed_url)
        results.append(result)
    
    total_new = sum(r.get("new", 0) for r in results)
    total_updated = sum(r.get("updated", 0) for r in results)
    
    return {
        "ok": True,
        "feeds": len(feeds),
        "results": results,
        "total_new": total_new,
        "total_updated": total_updated
    }
