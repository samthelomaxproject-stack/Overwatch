"""
Social OSINT Ingestion - minimal public sources feeding Conflict layer.
First-pass sources: Telegram RSS, Reddit JSON, no auth required.
"""
import hashlib
import json
import re
import time
from datetime import datetime, timezone, timedelta
from typing import Dict, List, Optional, Tuple
from urllib.parse import urlencode

import feedparser
import requests

from . import conflict_events, geocode


# ========== CONFIGURATION ==========

SOCIAL_SOURCES = [
    # Public Telegram channels with RSS feeds
    {
        "name": "Intel Slava Z",
        "type": "telegram_rss",
        "url": "https://rsshub.app/telegram/channel/intelslava",
        "event_type": "conflict",
        "region": "ukraine"
    },
    {
        "name": "Liveuamap",
        "type": "telegram_rss", 
        "url": "https://rsshub.app/telegram/channel/liveuamap",
        "event_type": "conflict",
        "region": "global"
    },
    # Reddit conflict-related subreddits via JSON API
    {
        "name": "r/UkrainianConflict",
        "type": "reddit_json",
        "url": "https://www.reddit.com/r/UkrainianConflict/new.json",
        "event_type": "conflict",
        "region": "ukraine"
    },
    {
        "name": "r/syriancivilwar",
        "type": "reddit_json",
        "url": "https://www.reddit.com/r/syriancivilwar/new.json",
        "event_type": "conflict",
        "region": "syria"
    },
]

# Base confidence scores
CONFIDENCE_BASE_SOCIAL = 0.35
CONFIDENCE_BOOST_DETAILED_LOCATION = 0.10
CONFIDENCE_BOOST_MULTIPLE_SOURCES = 0.15
CONFIDENCE_PENALTY_VAGUE = -0.10


# ========== HELPERS ==========

def calculate_confidence(text: str, location: Optional[str], similar_count: int = 1) -> float:
    """Simple deterministic confidence calculation for social sources."""
    score = CONFIDENCE_BASE_SOCIAL
    
    # Boost for detailed location
    if location and len(location) > 10:
        score += CONFIDENCE_BOOST_DETAILED_LOCATION
    
    # Boost for multiple similar reports
    if similar_count > 1:
        score += CONFIDENCE_BOOST_MULTIPLE_SOURCES * min(similar_count - 1, 3)
    
    # Penalty for vague text
    if text and len(text) < 100:
        score += CONFIDENCE_PENALTY_VAGUE
    
    return min(max(score, 0.0), 1.0)


def extract_location_from_text(text: str) -> Optional[str]:
    """Extract location mentions from text (very simple pattern matching)."""
    # Look for common location patterns
    patterns = [
        r'\b(?:in|near|at)\s+([A-Z][a-z]+(?:\s+[A-Z][a-z]+){0,2})\b',
        r'\b([A-Z][a-z]+,\s*[A-Z][a-z]+)\b',
    ]
    
    for pattern in patterns:
        match = re.search(pattern, text)
        if match:
            return match.group(1).strip()
    
    return None


def normalize_social_event(item: Dict, source_config: Dict) -> Optional[Dict]:
    """Normalize social item to conflict event format."""
    # Extract basic fields
    title = item.get("title", "").strip()
    text = item.get("text", item.get("summary", "")).strip()
    
    if not title or not text:
        return None
    
    # Extract location
    location_text = extract_location_from_text(title + " " + text)
    
    # Try geocoding if we have a location mention
    lat, lon = None, None
    if location_text:
        try:
            lat, lon = geocode.geocode_location(location_text)
        except:
            pass
    
    # Fallback: Use region-based default coords if no geocoding succeeded
    # This ensures social events show on map even without explicit location
    if not lat and not lon:
        region = source_config.get("region", "")
        region_defaults = {
            "ukraine": (49.0, 32.0),  # Central Ukraine
            "syria": (35.0, 38.0),    # Central Syria
            "global": None             # Skip if no region match
        }
        coords = region_defaults.get(region)
        if coords:
            lat, lon = coords
            location_text = location_text or f"{region.title()} region"
    
    # Skip events without any location (even regional fallback)
    if not lat or not lon:
        return None
    
    # Calculate confidence
    confidence = calculate_confidence(text, location_text)
    
    # Build normalized event
    event = {
        "title": title[:500],
        "summary": text[:2000],
        "source_type": "social",
        "source_name": source_config["name"],
        "source_url": item.get("link", item.get("url", "")),
        "published_at": item.get("published", datetime.now(timezone.utc).isoformat()),
        "event_type": source_config.get("event_type", "other"),
        "location_name": location_text,
        "lat": lat,
        "lon": lon,
        "raw_json": json.dumps({
            "confidence_score": confidence,
            "verification_status": "unverified",
            "source_platform": source_config["type"],
            "region": source_config.get("region"),
        })
    }
    
    return event


# ========== INGEST FUNCTIONS ==========

def ingest_telegram_rss(source: Dict) -> Dict[str, int]:
    """Ingest Telegram channel via RSS feed."""
    try:
        feed = feedparser.parse(source["url"])
        
        if feed.bozo:
            return {"error": "Feed parse error", "new": 0}
        
        new_count = 0
        
        for entry in feed.entries[:50]:  # Limit to 50 most recent
            item = {
                "title": entry.get("title", ""),
                "text": entry.get("summary", entry.get("description", "")),
                "link": entry.get("link", ""),
                "published": entry.get("published", entry.get("updated", ""))
            }
            
            normalized = normalize_social_event(item, source)
            
            if normalized:
                result = conflict_events.upsert_event(normalized)
                if result["inserted"]:
                    new_count += 1
        
        return {"ok": True, "new": new_count, "source": source["name"]}
    
    except Exception as e:
        return {"ok": False, "error": str(e), "source": source["name"]}


def ingest_reddit_json(source: Dict) -> Dict[str, int]:
    """Ingest Reddit subreddit via public JSON API."""
    try:
        headers = {"User-Agent": "Overwatch/0.2.0 (OSINT Hub)"}
        response = requests.get(source["url"], headers=headers, timeout=15)
        response.raise_for_status()
        
        data = response.json()
        posts = data.get("data", {}).get("children", [])
        
        new_count = 0
        
        for post_wrapper in posts[:50]:
            post = post_wrapper.get("data", {})
            
            item = {
                "title": post.get("title", ""),
                "text": post.get("selftext", "")[:2000],
                "link": f"https://reddit.com{post.get('permalink', '')}",
                "published": datetime.fromtimestamp(post.get("created_utc", 0), tz=timezone.utc).isoformat()
            }
            
            normalized = normalize_social_event(item, source)
            
            if normalized:
                result = conflict_events.upsert_event(normalized)
                if result["inserted"]:
                    new_count += 1
        
        return {"ok": True, "new": new_count, "source": source["name"]}
    
    except Exception as e:
        return {"ok": False, "error": str(e), "source": source["name"]}


def ingest_all_social() -> Dict:
    """Ingest all configured social sources."""
    results = []
    total_new = 0
    
    for source in SOCIAL_SOURCES:
        if source["type"] == "telegram_rss":
            result = ingest_telegram_rss(source)
        elif source["type"] == "reddit_json":
            result = ingest_reddit_json(source)
        else:
            result = {"ok": False, "error": f"Unknown type: {source['type']}", "source": source["name"]}
        
        results.append(result)
        
        if result.get("ok"):
            total_new += result.get("new", 0)
        
        # Rate limiting between sources
        time.sleep(1)
    
    return {
        "ok": True,
        "total_new": total_new,
        "sources": results,
        "ingested_at": datetime.now(timezone.utc).isoformat()
    }
