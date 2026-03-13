# Overwatch OSINT Conflict Module (Initial Scaffold)

This is the initial backend scaffold for global conflict event mapping in Overwatch.

## Stack (scaffold)
- FastAPI (Python)
- SQLite (replaceable with Postgres/PostGIS)
- ACLED API ingest (free myACLED token/account)

## Endpoints
- `GET /health`
- `GET /api/events?window=1d|7d|30d&country=&event_types=`
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

## Notes
- This is intentionally additive and does not replace existing Overwatch hub APIs yet.
- Next phase: migrate storage to Postgres, integrate with existing hub service/routes, and connect webui + Android consumers.
