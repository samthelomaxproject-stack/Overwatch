"""
Minimal geocoding via Nominatim (OpenStreetMap).
No API key required. Rate-limited to 1 req/sec per OSM policy.
"""
import time
from typing import Optional, Tuple

import requests


_last_geocode_time = 0
_geocode_cache = {}


def geocode_location(location: str) -> Optional[Tuple[float, float]]:
    """
    Geocode location string to (lat, lon).
    Returns None if not found or rate-limited.
    """
    global _last_geocode_time
    
    if not location or len(location) < 3:
        return None
    
    # Check cache
    cache_key = location.lower().strip()
    if cache_key in _geocode_cache:
        return _geocode_cache[cache_key]
    
    # Rate limiting: 1 req/sec per Nominatim policy
    now = time.time()
    if now - _last_geocode_time < 1.0:
        time.sleep(1.0 - (now - _last_geocode_time))
    
    try:
        params = {
            "q": location,
            "format": "json",
            "limit": 1
        }
        
        headers = {
            "User-Agent": "Overwatch/0.2.0 (OSINT Hub; Social geocoding)"
        }
        
        response = requests.get(
            "https://nominatim.openstreetmap.org/search",
            params=params,
            headers=headers,
            timeout=10
        )
        
        _last_geocode_time = time.time()
        
        if response.status_code == 200:
            results = response.json()
            if results:
                lat = float(results[0]["lat"])
                lon = float(results[0]["lon"])
                _geocode_cache[cache_key] = (lat, lon)
                return (lat, lon)
    
    except Exception:
        pass
    
    return None
