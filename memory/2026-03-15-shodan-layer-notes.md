# 2026-03-15 — Shodan OSINT Layer (Hub-First) Implementation Notes

User directive:
- Implement SHODAN as a new OSINT map layer with hub-first architecture.
- Hub/sidecar is the only component talking to Shodan API.
- APK/web clients only consume normalized hub markers.
- Preserve existing layers/architecture with minimal targeted changes.
- Optimize for cache-first behavior to reduce Shodan credit usage.

Initial implementation started:
- Added Shodan schema tables in `osint_hub/app/db.py`:
  - `shodan_findings`
  - `shodan_region_cache`
- Added `osint_hub/app/shodan.py` for:
  - discovery
  - normalization
  - SQLite upsert
  - region/category freshness checks
  - marker serving
- Integrated in `osint_hub/app/main.py`:
  - scheduler support
  - `/api/shodan/markers`
  - `/api/shodan/refresh`
  - health metadata for Shodan scheduler state
- Added Shodan env vars to `osint_hub/.env.example`.

Next integration steps (in-progress):
- Hub webui map toggle + render pipeline for `layerVisible.shodan`.
- APK map toggle + render pipeline consuming hub-only Shodan endpoint.
- Keep API key strictly in sidecar env.
