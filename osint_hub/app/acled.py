import os
from datetime import date, timedelta
from typing import Dict, Optional

import requests

ACLED_URL = "https://acleddata.com/api/acled/read"
ACLED_OAUTH_URL = "https://acleddata.com/oauth/token"
DEFAULT_EVENT_TYPES = [
    "Battles",
    "Explosions/Remote violence",
    "Strategic developments",
    "Violence against civilians",
]

_token_cache: Dict[str, Optional[str]] = {
    "access_token": None,
    "refresh_token": None,
}


def _oauth_with_password() -> Dict[str, str]:
    username = os.getenv("ACLED_USERNAME", "")
    password = os.getenv("ACLED_PASSWORD", "")
    if not username or not password:
        raise RuntimeError("ACLED_USERNAME/ACLED_PASSWORD not configured")

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
    return {
        "access_token": payload.get("access_token", ""),
        "refresh_token": payload.get("refresh_token", ""),
    }


def _oauth_refresh(refresh_token: str) -> Dict[str, str]:
    r = requests.post(
        ACLED_OAUTH_URL,
        headers={"Content-Type": "application/x-www-form-urlencoded"},
        data={
            "refresh_token": refresh_token,
            "grant_type": "refresh_token",
            "client_id": "acled",
        },
        timeout=30,
    )
    r.raise_for_status()
    payload = r.json()
    return {
        "access_token": payload.get("access_token", ""),
        "refresh_token": payload.get("refresh_token", refresh_token),
    }


def _get_access_token() -> str:
    if _token_cache.get("access_token"):
        return _token_cache["access_token"] or ""

    env_access = os.getenv("ACLED_ACCESS_TOKEN", "")
    env_refresh = os.getenv("ACLED_REFRESH_TOKEN", "")

    if env_access:
        _token_cache["access_token"] = env_access
        _token_cache["refresh_token"] = env_refresh or None
        return env_access

    tokens = _oauth_with_password()
    _token_cache.update(tokens)
    return tokens["access_token"]


def _authed_get(url: str, params: dict) -> requests.Response:
    token = _get_access_token()
    r = requests.get(
        url,
        params=params,
        headers={"Authorization": f"Bearer {token}"},
        timeout=45,
    )
    if r.status_code == 401 and _token_cache.get("refresh_token"):
        # Attempt refresh once
        refreshed = _oauth_refresh(_token_cache["refresh_token"] or "")
        _token_cache.update(refreshed)
        r = requests.get(
            url,
            params=params,
            headers={"Authorization": f"Bearer {_token_cache['access_token']}"},
            timeout=45,
        )
    r.raise_for_status()
    return r


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

    r = _authed_get(ACLED_URL, params)
    payload = r.json()
    return payload.get("data", [])
