# Overwatch OSINT Module (Conflict + Shodan)

This sidecar is the **hub-first OSINT backend** for conflict and Shodan layers.

## Stack (scaffold)
- FastAPI (Python)
- SQLite (replaceable with Postgres/PostGIS)
- ACLED API ingest (free myACLED token/account)

## Endpoints
- `GET /health`
- `GET /api/meta` (countries/event types + presets)
- `GET /api/events?window=1d|7d|30d&country=&event_types=&date_from=YYYY-MM-DD&date_to=YYYY-MM-DD`
- `GET /api/events/since?since=ISO8601&country=`
- `GET /api/alerts/high-impact?window=1d|min_fatalities=10&country=`
- `POST /api/ingest/acled?days=7&country=` (manual ingest trigger)
- `GET /api/shodan/events?bbox=minLon,minLat,maxLon,maxLat&category=sdr,camera&since=&limit=500&stale=`
- `GET /api/shodan/events/since?since=ISO8601&bbox=&category=`
- `GET /api/shodan/meta`
- `POST /api/shodan/ingest?bbox=&category=&force=false`
- `POST /api/shodan/refresh-region?bbox=&category=sdr,satcom&force=false`
- `GET /api/shodan/detail/{id}`
- `GET /api/shodan/categories`
- `POST /api/shodan/mock-seed` (verification-only mock data)
- `POST /api/shodan/mock-clear` (remove only `source=mock_shodan` rows)

## Quick start
```bash
cd osint_hub
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
cp .env.example .env
# edit .env and set ACLED_USERNAME + ACLED_PASSWORD (+ optional SHODAN_API_KEY)
uvicorn app.main:app --reload --port 8790
```

## ACLED auth guideline (current)
- Use exact OAuth password-grant flow per ACLED docs for each ingest cycle:
  - `POST https://acleddata.com/oauth/token`
  - form fields: `username`, `password`, `grant_type=password`, `client_id=acled`
- Use returned `access_token` as `Authorization: Bearer <token>` for `/api/acled/read` requests.

## Notes
- Hub-first rule: only this sidecar talks to Shodan API; web/APK consume normalized hub endpoints only.
- Cache-first discovery minimizes Shodan credit usage (region/category TTL + SQLite findings cache).
- This is intentionally additive and does not replace existing Overwatch hub APIs yet.
- Next phase: migrate storage to Postgres, integrate with existing hub service/routes, and connect webui + Android consumers.
