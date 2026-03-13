import os
from datetime import date, timedelta
from typing import Optional

import requests

ACLED_URL = "https://api.acleddata.com/acled/read"
DEFAULT_EVENT_TYPES = [
    "Battles",
    "Explosions/Remote violence",
    "Strategic developments",
    "Violence against civilians",
]


def fetch_acled(days: int = 7, country: Optional[str] = None):
    email = os.getenv("ACLED_EMAIL", "")
    key = os.getenv("ACLED_KEY", "")
    if not email or not key:
        raise RuntimeError("ACLED_EMAIL/ACLED_KEY not configured")

    end = date.today()
    start = end - timedelta(days=days)

    params = {
        "key": key,
        "email": email,
        "terms": "accept",
        "event_date": f"{start}|{end}",
        "event_date_where": "BETWEEN",
        "event_type": "|".join(DEFAULT_EVENT_TYPES),
        "event_type_where": "IN",
        "limit": 5000,
    }
    if country:
        params["country"] = country
        params["country_where"] = "="

    r = requests.get(ACLED_URL, params=params, timeout=30)
    r.raise_for_status()
    payload = r.json()
    return payload.get("data", [])
