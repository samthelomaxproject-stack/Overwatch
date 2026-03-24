"""
Lightweight geocoding for OSINT events.
Uses Nominatim (OpenStreetMap) with rate limiting.
"""
import re
import time
import requests
from typing import Dict, Optional

# Rate limit for Nominatim: 1 request per second
_last_geocode_time = 0
_geocode_cache = {}


def extract_location_text(text: str) -> Optional[str]:
    """Extract likely location mentions from text."""
    # Look for common patterns: "in City, Country" or "City, State"
    patterns = [
        r'\bin\s+([A-Z][a-z]+(?:\s+[A-Z][a-z]+)?(?:,\s*[A-Z][a-z]+)?)',
        r'\bat\s+([A-Z][a-z]+(?:\s+[A-Z][a-z]+)?)',
        r'([A-Z][a-z]+,\s*[A-Z]{2,})',  # City, Country Code
    ]
    
    for pattern in patterns:
        match = re.search(pattern, text)
        if match:
            return match.group(1).strip()
    
    return None


def geocode_text(location: str) -> Optional[Dict]:
    """Geocode location text using Nominatim with rate limiting and confidence scoring."""
    global _last_geocode_time, _geocode_cache
    
    if not location or len(location) < 3:
        return None
    
    # Check cache
    cache_key = location.lower().strip()
    if cache_key in _geocode_cache:
        return _geocode_cache[cache_key]
    
    # Rate limit
    now = time.time()
    if now - _last_geocode_time < 1.0:
        time.sleep(1.0 - (now - _last_geocode_time))
    _last_geocode_time = time.time()
    
    try:
        resp = requests.get(
            "https://nominatim.openstreetmap.org/search",
            params={
                "q": location,
                "format": "json",
                "limit": 1,
                "addressdetails": 1
            },
            headers={"User-Agent": "Overwatch/0.2 OSINT Hub"},
            timeout=5
        )
        
        if not resp.ok or not resp.json():
            return None
        
        result = resp.json()[0]
        address = result.get("address", {})
        
        # Determine confidence based on place_type
        place_type = result.get("type", "")
        confidence = 0.5
        if place_type in ["city", "town", "village"]:
            confidence = 0.9
        elif place_type in ["state", "region", "county"]:
            confidence = 0.7
        elif place_type in ["country"]:
            confidence = 0.5
        
        geo = {
            "lat": float(result["lat"]),
            "lon": float(result["lon"]),
            "city": address.get("city") or address.get("town") or address.get("village"),
            "admin1": address.get("state") or address.get("region"),
            "country": address.get("country"),
            "confidence": confidence
        }
        
        _geocode_cache[cache_key] = geo
        return geo
        
    except Exception:
        return None
