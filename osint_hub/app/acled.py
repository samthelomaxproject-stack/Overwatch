import os
from datetime import date, timedelta
from typing import Optional

import requests

ACLED_URL = "https://acleddata.com/api/acled/read"
ACLED_OAUTH_URL = "https://acleddata.com/oauth/token"
DEFAULT_EVENT_TYPES = [
    "Battles",
    "Explosions/Remote violence",
    "Strategic developments",
    "Violence against civilians",
]


def _request_access_token() -> str:
    username = os.getenv("ACLED_USERNAME", "")
    password = os.getenv("ACLED_PASSWORD", "")
    if not username or not password:
        raise RuntimeError("ACLED_USERNAME/ACLED_PASSWORD not configured")

    # Exact ACLED docs-style OAuth password grant request.
    r = requests.post(
        ACLED_OAUTH_URL,
        headers={"Content-Type": "application/x-www-form-urlencoded"},
        data={
            "username": username,
            "password": password,
            "grant_type": "password",
            "client_id": "acled",
        },
        timeout=30,
    )
    r.raise_for_status()
    payload = r.json()
    token = payload.get("access_token", "")
    if not token:
        raise RuntimeError("ACLED OAuth succeeded but no access_token returned")
    return token


def fetch_acled(days: int = 7, country: Optional[str] = None):
    end = date.today()
    start = end - timedelta(days=days)

    params = {
        "event_date": f"{start}|{end}",
        "event_date_where": "BETWEEN",
        "event_type": "|".join(DEFAULT_EVENT_TYPES),
        "event_type_where": "IN",
        "limit": 5000,
    }
    if country:
        params["country"] = country
        params["country_where"] = "="

    token = _request_access_token()
    r = requests.get(
        ACLED_URL,
        params=params,
        headers={"Authorization": f"Bearer {token}"},
        timeout=45,
    )
    r.raise_for_status()
    payload = r.json()
    return payload.get("data", [])
