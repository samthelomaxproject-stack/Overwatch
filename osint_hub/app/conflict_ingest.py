"""
Conflict event ingestion from RSS and GDELT.
"""
import os
import re
import feedparser
import requests
from datetime import datetime, timezone
from typing import Dict, List

from .conflict_events import upsert_event, prune_old_events, list_feeds
from .geocoding import extract_location_text, geocode_text
from .db import get_conn


def _is_conflict_relevant(title: str, description: str) -> bool:
    """Check if article is conflict-relevant."""
    text = (title + " " + description).lower()
    keywords = [
        "attack", "killed", "dead", "bombing", "explosion", "protest",
        "strike", "riot", "military", "troops", "deployed", "conflict",
        "war", "battle", "shooting", "incident", "security", "police",
        "army", "navy", "air force", "drone", "missile", "earthquake",
        "flood", "disaster", "emergency", "evacuation", "casualties",
        "violence", "clashes", "fighting", "invasion", "rebel", "militia"
    ]
    return any(kw in text for kw in keywords)


def _classify_event(title: str, description: str) -> str:
    """Simple rule-based classification."""
    text = (title + " " + description).lower()
    
    if any(w in text for w in ["attack", "bombing", "shooting", "killed", "battle", "war", "invasion"]):
        return "conflict"
    if any(w in text for w in ["protest", "demonstrat", "rally", "march"]):
        return "protest"
    if any(w in text for w in ["strike", "walkout"]):
        return "strike"
    if any(w in text for w in ["military", "troops", "deployed", "forces"]):
        return "military_activity"
    if any(w in text for w in ["earthquake", "flood", "hurricane", "disaster", "wildfire"]):
        return "disaster"
    if any(w in text for w in ["incident", "security", "police"]):
        return "security_incident"
    
    return "other"


def ingest_rss_feed(feed_id: int, feed_name: str, feed_url: str) -> Dict:
    """Ingest single RSS feed."""
    started = datetime.now(timezone.utc).isoformat()
    
    new_count = 0
    duplicate_count = 0
    error = None
    
    try:
        resp = requests.get(feed_url, timeout=30, headers={"User-Agent": "Overwatch/0.2 Conflict Monitor"})
        resp.raise_for_status()
        
        feed = feedparser.parse(resp.content)
        
        for entry in feed.entries[:50]:
            title = entry.get("title", "").strip()
            if not title:
                continue
            
            description = entry.get("description", "") or entry.get("summary", "")
            link = entry.get("link", "")
            
            if not _is_conflict_relevant(title, description):
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
            
            event_type = _classify_event(title, description)
            summary = description[:250] if description else title
            
            event = {
                "title": title,
                "summary": summary,
                "source_type": "rss",
                "source_name": feed_name,
                "source_url": link,
                "published_at": published or started,
                "event_type": event_type,
                "location_name": location_text,
                "lat": geo_result.get("lat") if geo_result else None,
                "lon": geo_result.get("lon") if geo_result else None,
                "raw_json": None
            }
            
            result = upsert_event(event)
            new_count += result["inserted"]
            duplicate_count += result["duplicate"]
        
        status = "ok"
        
        # Update feed state
        with get_conn() as conn:
            conn.execute("""
                UPDATE conflict_feeds 
                SET last_checked_at = ?, last_success_at = ?, last_error = NULL
                WHERE id = ?
            """, (started, started, feed_id))
            conn.commit()
        
    except Exception as e:
        status = "error"
        error = str(e)
        
        with get_conn() as conn:
            conn.execute("""
                UPDATE conflict_feeds 
                SET last_checked_at = ?, last_error = ?
                WHERE id = ?
            """, (started, error, feed_id))
            conn.commit()
    
    return {
        "ok": status == "ok",
        "feed": feed_name,
        "new": new_count,
        "duplicates": duplicate_count,
        "error": error
    }


def ingest_gdelt(hours_back: int = 24, max_results: int = 100) -> Dict:
    """Ingest GDELT conflict events."""
    started = datetime.now(timezone.utc).isoformat()
    
    new_count = 0
    duplicate_count = 0
    error = None
    
    try:
        query_terms = "conflict OR attack OR protest OR military OR violence"
        
        resp = requests.get(
            "https://api.gdeltproject.org/api/v2/doc/doc",
            params={
                "query": query_terms,
                "mode": "timelinevol",
                "format": "json",
                "maxrecords": max_results,
                "timespan": f"{hours_back}h"
            },
            timeout=30
        )
        
        if not resp.ok:
            raise Exception(f"GDELT API error: {resp.status_code}")
        
        data = resp.json()
        articles = data.get("articles", [])
        
        for article in articles[:max_results]:
            title = article.get("title", "").strip()
            if not title:
                continue
            
            url = article.get("url", "")
            seendate = article.get("seendate", "")
            source = article.get("domain", "GDELT")
            
            # Parse published time
            published = None
            if seendate:
                try:
                    dt = datetime.strptime(seendate, "%Y%m%dT%H%M%SZ")
                    published = dt.replace(tzinfo=timezone.utc).isoformat()
                except:
                    pass
            
            lat = article.get("lat")
            lon = article.get("lon")
            
            themes = article.get("themes", [])
            event_type = "conflict"  # GDELT query is conflict-focused
            
            summary = article.get("snippet", title)[:250]
            
            event = {
                "title": title,
                "summary": summary,
                "source_type": "gdelt",
                "source_name": source,
                "source_url": url,
                "published_at": published or started,
                "event_type": event_type,
                "location_name": None,
                "lat": float(lat) if lat else None,
                "lon": float(lon) if lon else None,
                "raw_json": None
            }
            
            result = upsert_event(event)
            new_count += result["inserted"]
            duplicate_count += result["duplicate"]
        
        status = "ok"
        
    except Exception as e:
        status = "error"
        error = str(e)
    
    return {
        "ok": status == "ok",
        "source": "GDELT",
        "new": new_count,
        "duplicates": duplicate_count,
        "error": error
    }


def refresh_all_conflict_events() -> Dict:
    """Refresh all enabled feeds + GDELT, then prune old events."""
    feeds = list_feeds()
    enabled_feeds = [f for f in feeds if f["enabled"]]
    
    rss_results = []
    for feed in enabled_feeds:
        result = ingest_rss_feed(feed["id"], feed["name"], feed["url"])
        rss_results.append(result)
    
    gdelt_result = ingest_gdelt()
    
    pruned = prune_old_events()
    
    total_new = sum(r.get("new", 0) for r in rss_results) + gdelt_result.get("new", 0)
    total_duplicates = sum(r.get("duplicates", 0) for r in rss_results) + gdelt_result.get("duplicates", 0)
    
    return {
        "ok": True,
        "feeds_checked": len(enabled_feeds),
        "rss_results": rss_results,
        "gdelt_result": gdelt_result,
        "total_new": total_new,
        "total_duplicates": total_duplicates,
        "pruned": pruned
    }
