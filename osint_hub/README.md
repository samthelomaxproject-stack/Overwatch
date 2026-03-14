# Overwatch OSINT Conflict Module (Initial Scaffold)

This is the initial backend scaffold for global conflict event mapping in Overwatch.

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

## Quick start
```bash
cd osint_hub
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
cp .env.example .env
# edit .env and set ACLED_EMAIL + ACLED_KEY
uvicorn app.main:app --reload --port 8790
```

## ACLED auth guideline (current)
- Use OAuth per ACLED docs:
  - `POST https://acleddata.com/oauth/token`
  - form fields: `username`, `password`, `grant_type=password`, `client_id=acled`
- Use returned `access_token` as `Authorization: Bearer <token>` for `/api/acled/read` requests.
- Refresh with `grant_type=refresh_token` + `client_id=acled` when needed.

## Notes
- This is intentionally additive and does not replace existing Overwatch hub APIs yet.
- Next phase: migrate storage to Postgres, integrate with existing hub service/routes, and connect webui + Android consumers.
