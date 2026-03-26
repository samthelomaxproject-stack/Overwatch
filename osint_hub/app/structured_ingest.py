"""
Structured source ingestion: ReliefWeb, USGS, NASA FIRMS.
High-confidence data sources with standardized formats.
"""
import json
import os
import time
from datetime import datetime, timezone, timedelta
from typing import Dict, List, Optional

import requests

from . import conflict_events


# ========== CONFIGURATION ==========

RELIEFWEB_API_URL = "https://api.reliefweb.int/v1/reports"
USGS_EARTHQUAKE_URL = "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/2.5_day.geojson"
FIRMS_ENABLED = os.getenv("FIRMS_ENABLED", "false").lower() in ("1", "true", "yes")
FIRMS_API_KEY = os.getenv("FIRMS_API_KEY", "")
FIRMS_MAP_KEY = os.getenv("FIRMS_MAP_KEY", "")  # Alternative FIRMS access method

# Confidence scores
CONFIDENCE_RELIEFWEB = 0.85
CONFIDENCE_USGS = 0.95
CONFIDENCE_FIRMS = 0.80


# ========== RELIEFWEB ==========

def ingest_reliefweb(days_back: int = 7, limit: int = 50) -> Dict:
    """
    Ingest humanitarian reports from ReliefWeb API.
    
    API Docs: https://apidoc.reliefweb.int/
    Requires approved appname: https://apidoc.reliefweb.int/parameters#appname
    Set RELIEFWEB_APPNAME environment variable after approval.
    """
    reliefweb_appname = os.getenv("RELIEFWEB_APPNAME", "")
    
    if not reliefweb_appname:
        return {
            "ok": False,
            "source": "reliefweb",
            "error": "ReliefWeb requires approved appname. Request at https://apidoc.reliefweb.int/parameters#appname and set RELIEFWEB_APPNAME env var.",
            "new": 0
        }
    try:
        # ReliefWeb requires approved appname + User-Agent for access
        params = {
            "appname": reliefweb_appname,
            "limit": limit,
            "profile": "full",
            "sort[]": "date:desc"
        }
        
        headers = {
            "User-Agent": "Overwatch-OSINT-Hub/0.2.0 (github.com/samthelomaxproject-stack/Overwatch)"
        }
        
        response = requests.get(
            RELIEFWEB_API_URL,
            params=params,
            headers=headers,
            timeout=30
        )
        response.raise_for_status()
        
        data = response.json()
        reports = data.get("data", [])
        
        new_count = 0
        
        for report in reports:
            fields = report.get("fields", {})
            
            # Extract location
            country = None
            if fields.get("primary_country"):
                country = fields["primary_country"][0].get("name")
            elif fields.get("country"):
                country = fields["country"][0].get("name")
            
            # Try geocoding country name
            lat, lon = None, None
            if country:
                try:
                    from . import geocode
                    coords = geocode.geocode_location(country)
                    if coords:
                        lat, lon = coords
                except:
                    pass
            
            # Skip if no location
            if not lat or not lon:
                continue
            
            # Classify event type (simple keyword-based)
            title = fields.get("title", "")
            text = (title + " " + fields.get("body-html", "")).lower()
            
            event_type = "humanitarian_incident"
            if any(kw in text for kw in ["conflict", "attack", "fighting", "war"]):
                event_type = "conflict"
            elif any(kw in text for kw in ["earthquake", "flood", "disaster", "cyclone", "hurricane"]):
                event_type = "disaster"
            
            # Build event
            event = {
                "title": title[:500],
                "summary": fields.get("body-html", "")[:2000],
                "source_type": "reliefweb",
                "source_name": "ReliefWeb",
                "source_url": f"https://reliefweb.int{fields.get('url_alias', '')}",
                "published_at": fields.get("date", {}).get("created", datetime.now(timezone.utc).isoformat()),
                "event_type": event_type,
                "location_name": country,
                "lat": lat,
                "lon": lon,
                "raw_json": json.dumps({
                    "confidence_score": CONFIDENCE_RELIEFWEB,
                    "verification_status": "verified",
                    "source_platform": "reliefweb_api",
                    "reliefweb_id": report.get("id")
                })
            }
            
            result = conflict_events.upsert_event(event)
            if result["inserted"]:
                new_count += 1
        
        return {
            "ok": True,
            "source": "reliefweb",
            "new": new_count,
            "fetched": len(reports),
            "ingested_at": datetime.now(timezone.utc).isoformat()
        }
    
    except Exception as e:
        return {
            "ok": False,
            "source": "reliefweb",
            "error": str(e),
            "new": 0
        }


# ========== USGS EARTHQUAKES ==========

def ingest_usgs_earthquakes() -> Dict:
    """
    Ingest recent earthquakes from USGS GeoJSON feed.
    
    Feed: All M2.5+ earthquakes in the last day
    Docs: https://earthquake.usgs.gov/earthquakes/feed/v1.0/geojson.php
    """
    try:
        response = requests.get(USGS_EARTHQUAKE_URL, timeout=30)
        response.raise_for_status()
        
        data = response.json()
        features = data.get("features", [])
        
        new_count = 0
        
        for feature in features:
            props = feature.get("properties", {})
            geom = feature.get("geometry", {})
            coords = geom.get("coordinates", [])
            
            if len(coords) < 2:
                continue
            
            lon, lat = coords[0], coords[1]
            
            # Extract data
            magnitude = props.get("mag", 0)
            place = props.get("place", "Unknown location")
            timestamp_ms = props.get("time", 0)
            timestamp = datetime.fromtimestamp(timestamp_ms / 1000, tz=timezone.utc).isoformat()
            
            # Build title
            title = f"Magnitude {magnitude} earthquake - {place}"
            
            # Build event
            event = {
                "title": title[:500],
                "summary": f"USGS detected magnitude {magnitude} earthquake at {place}. Depth: {coords[2] if len(coords) > 2 else 'unknown'}km.",
                "source_type": "usgs",
                "source_name": "USGS Earthquake Feed",
                "source_url": props.get("url", ""),
                "published_at": timestamp,
                "event_type": "disaster",
                "location_name": place,
                "lat": lat,
                "lon": lon,
                "raw_json": json.dumps({
                    "confidence_score": CONFIDENCE_USGS,
                    "verification_status": "verified",
                    "source_platform": "usgs_geojson",
                    "magnitude": magnitude,
                    "depth_km": coords[2] if len(coords) > 2 else None,
                    "usgs_id": props.get("id"),
                    "alert": props.get("alert"),
                    "tsunami": props.get("tsunami")
                })
            }
            
            result = conflict_events.upsert_event(event)
            if result["inserted"]:
                new_count += 1
        
        return {
            "ok": True,
            "source": "usgs",
            "new": new_count,
            "fetched": len(features),
            "ingested_at": datetime.now(timezone.utc).isoformat()
        }
    
    except Exception as e:
        return {
            "ok": False,
            "source": "usgs",
            "error": str(e),
            "new": 0
        }


# ========== NASA FIRMS ==========

def ingest_nasa_firms(days_back: int = 1, region: str = "world") -> Dict:
    """
    Ingest fire detections from NASA FIRMS.
    
    Requires FIRMS_API_KEY or FIRMS_MAP_KEY environment variable.
    Docs: https://firms.modaps.eosdis.nasa.gov/api/
    
    Note: FIRMS API requires registration. If not configured, fails soft.
    """
    if not FIRMS_ENABLED:
        return {
            "ok": False,
            "source": "firms",
            "error": "FIRMS ingestion disabled (FIRMS_ENABLED=false)",
            "new": 0
        }
    
    if not FIRMS_API_KEY and not FIRMS_MAP_KEY:
        return {
            "ok": False,
            "source": "firms",
            "error": "FIRMS API key not configured (FIRMS_API_KEY or FIRMS_MAP_KEY required)",
            "new": 0
        }
    
    try:
        # FIRMS Active Fire Data - MODIS/VIIRS
        # Using Area CSV endpoint for simplicity
        key = FIRMS_API_KEY or FIRMS_MAP_KEY
        url = f"https://firms.modaps.eosdis.nasa.gov/api/area/csv/{key}/VIIRS_SNPP_NRT/world/{days_back}"
        
        response = requests.get(url, timeout=30)
        response.raise_for_status()
        
        lines = response.text.strip().split("\n")
        if len(lines) < 2:
            return {"ok": True, "source": "firms", "new": 0, "fetched": 0}
        
        # Parse CSV header
        header = lines[0].split(",")
        rows = [line.split(",") for line in lines[1:]]
        
        new_count = 0
        
        for row in rows[:500]:  # Limit to 500 most recent
            if len(row) < len(header):
                continue
            
            data = dict(zip(header, row))
            
            try:
                lat = float(data.get("latitude", 0))
                lon = float(data.get("longitude", 0))
                brightness = float(data.get("bright_ti4", 0))
                confidence = data.get("confidence", "nominal")
                acq_date = data.get("acq_date", "")
                acq_time = data.get("acq_time", "0000")
                
                # Parse timestamp
                if acq_date and acq_time:
                    timestamp_str = f"{acq_date} {acq_time.zfill(4)[:2]}:{acq_time.zfill(4)[2:]}:00"
                    timestamp = datetime.strptime(timestamp_str, "%Y-%m-%d %H:%M:%S").replace(tzinfo=timezone.utc).isoformat()
                else:
                    timestamp = datetime.now(timezone.utc).isoformat()
                
                # Confidence mapping
                conf_score = CONFIDENCE_FIRMS
                if confidence == "high":
                    conf_score = 0.90
                elif confidence == "low":
                    conf_score = 0.70
                
                # Build title
                title = f"Fire detection - {lat:.3f}, {lon:.3f} (brightness: {brightness}K)"
                
                # Build event
                event = {
                    "title": title[:500],
                    "summary": f"NASA FIRMS detected thermal anomaly with {confidence} confidence. Brightness: {brightness}K.",
                    "source_type": "firms",
                    "source_name": "NASA FIRMS",
                    "source_url": f"https://firms.modaps.eosdis.nasa.gov/map/#d:24hrs;@{lon},{lat},12z",
                    "published_at": timestamp,
                    "event_type": "fire_activity",
                    "location_name": f"{lat:.3f}, {lon:.3f}",
                    "lat": lat,
                    "lon": lon,
                    "raw_json": json.dumps({
                        "confidence_score": conf_score,
                        "verification_status": "verified",
                        "source_platform": "firms_viirs",
                        "brightness_k": brightness,
                        "confidence_level": confidence,
                        "scan": data.get("scan"),
                        "track": data.get("track")
                    })
                }
                
                result = conflict_events.upsert_event(event)
                if result["inserted"]:
                    new_count += 1
            
            except (ValueError, KeyError):
                continue
        
        return {
            "ok": True,
            "source": "firms",
            "new": new_count,
            "fetched": len(rows),
            "ingested_at": datetime.now(timezone.utc).isoformat()
        }
    
    except Exception as e:
        return {
            "ok": False,
            "source": "firms",
            "error": str(e),
            "new": 0
        }


# ========== COMBINED INGEST ==========

def ingest_all_structured() -> Dict:
    """Ingest all structured sources."""
    results = []
    total_new = 0
    
    # ReliefWeb
    rw_result = ingest_reliefweb()
    results.append(rw_result)
    if rw_result.get("ok"):
        total_new += rw_result.get("new", 0)
    time.sleep(1)  # Rate limiting
    
    # USGS
    usgs_result = ingest_usgs_earthquakes()
    results.append(usgs_result)
    if usgs_result.get("ok"):
        total_new += usgs_result.get("new", 0)
    time.sleep(1)
    
    # FIRMS (if enabled)
    if FIRMS_ENABLED:
        firms_result = ingest_nasa_firms()
        results.append(firms_result)
        if firms_result.get("ok"):
            total_new += firms_result.get("new", 0)
    
    return {
        "ok": True,
        "total_new": total_new,
        "sources": results,
        "ingested_at": datetime.now(timezone.utc).isoformat()
    }
