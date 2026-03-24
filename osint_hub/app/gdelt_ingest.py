"""
GDELT ingestion for OSINT events pipeline.
Uses GDELT 2.0 Event Database (free, no API key needed).
"""
import os
import requests
from datetime import datetime, timezone, timedelta
from typing import Dict, List

from .events import upsert_event, generate_fingerprint
from .db import get_conn


GDELT_BASE = "https://api.gdeltproject.org/api/v2/doc/doc"


def _gdelt_to_event_type(gdelt_themes: List[str]) -> str:
    """Map GDELT themes to our event types."""
    themes_str = " ".join(gdelt_themes).lower()
    
    if any(w in themes_str for w in ["military", "war", "armed_conflict", "terrorism"]):
        return "conflict"
    if any(w in themes_str for w in ["protest", "demonstration"]):
        return "protest"
    if any(w in themes_str for w in ["strike", "labor"]):
        return "strike"
    if any(w in themes_str for w in ["military_deployment", "troop_movement"]):
        return "military_activity"
    if any(w in themes_str for w in ["disaster", "earthquake", "flood", "fire"]):
        return "disaster"
    if any(w in themes_str for w in ["security", "police", "arrest"]):
        return "security_incident"
    
    return "other"


def ingest_gdelt(hours_back: int = 24, max_results: int = 100) -> Dict:
    """Ingest recent GDELT events."""
    started = datetime.now(timezone.utc).isoformat()
    
    new_count = 0
    updated_count = 0
    duplicate_count = 0
    error = None
    
    try:
        # GDELT Doc API for news articles with geo
        # Using recent timespan mode
        query_terms = "protest OR attack OR military OR disaster OR strike OR conflict"
        
        resp = requests.get(
            GDELT_BASE,
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
                    # GDELT format: YYYYMMDDTHHmmssZ
                    dt = datetime.strptime(seendate, "%Y%m%dT%H%M%SZ")
                    published = dt.replace(tzinfo=timezone.utc).isoformat()
                except:
                    pass
            
            # Get location if available
            lat = article.get("lat")
            lon = article.get("lon")
            geocode_conf = 0.8 if (lat and lon) else 0.0
            
            # GDELT themes
            themes = article.get("themes", [])
            event_type = _gdelt_to_event_type(themes)
            
            # Summary
            summary = article.get("snippet", title)[:200]
            
            # Confidence
            confidence = 0.6  # GDELT base
            if lat and lon:
                confidence += 0.2
            if event_type != "other":
                confidence += 0.1
            confidence = min(1.0, confidence)
            
            event = {
                "id": f"gdelt_{generate_fingerprint(title, url)}",
                "external_id": url,
                "source_type": "gdelt",
                "source_name": source,
                "source_url": url,
                "title": title,
                "raw_text": summary,
                "summary": summary,
                "published_at": published,
                "ingested_at": started,
                "event_type": event_type,
                "lat": float(lat) if lat else None,
                "lon": float(lon) if lon else None,
                "geocode_confidence": geocode_conf,
                "classification_confidence": 0.7 if event_type != "other" else 0.4,
                "confidence_score": confidence,
                "ai_tags": ",".join(themes[:5]) if themes else None,
                "status": "active"
            }
            
            result = upsert_event(event)
            new_count += result["inserted"]
            updated_count += result["updated"]
        
        status = "ok"
        
    except Exception as e:
        status = "error"
        error = str(e)
    
    finished = datetime.now(timezone.utc).isoformat()
    
    # Log ingest run
    with get_conn() as conn:
        conn.execute("""
            INSERT INTO osint_ingest_runs (
                source_type, source_name, started_at, finished_at, status,
                new_count, updated_count, duplicate_count, error
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        """, ("gdelt", "GDELT_API", started, finished, status, new_count, updated_count, duplicate_count, error))
        conn.commit()
    
    return {
        "ok": status == "ok",
        "source": "GDELT",
        "new": new_count,
        "updated": updated_count,
        "duplicates": duplicate_count,
        "error": error
    }
