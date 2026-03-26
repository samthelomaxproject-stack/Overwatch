# Structured OSINT Sources

High-confidence structured data feeds: ReliefWeb, USGS, NASA FIRMS.

## Overview

Structured sources provide verified, high-confidence intelligence from authoritative organizations. Unlike Social OSINT, these sources are pre-verified and machine-parseable.

**Source Tiers:**
- **USGS**: 0.95 confidence - Verified seismic data
- **ReliefWeb**: 0.85 confidence - Humanitarian reports
- **NASA FIRMS**: 0.80 confidence - Satellite fire detections

## Sources

### 1. USGS Earthquakes ✅ WORKING

**API:** https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/2.5_day.geojson  
**Status:** Operational  
**Confidence:** 0.95  
**Event Type:** `disaster`

**Data Extracted:**
- Magnitude (e.g., M4.5)
- Location (place name)
- Coordinates (lat/lon)
- Depth
- Timestamp
- Alert level
- Tsunami potential

**Example Event:**
```json
{
  "title": "Magnitude 4.5 earthquake - 12km NE of San Francisco, CA",
  "source_type": "usgs",
  "event_type": "disaster",
  "confidence_score": 0.95,
  "verification_status": "verified"
}
```

### 2. ReliefWeb 🚧 SCAFFOLDED

**API:** https://api.reliefweb.int/v1/reports  
**Status:** API access issues (403 Forbidden)  
**Confidence:** 0.85 (when working)  
**Event Types:** `conflict`, `disaster`, `humanitarian_incident`

**Planned Features:**
- Humanitarian situation reports
- Disaster response updates
- Conflict impact assessments
- Country-level geocoding

**Current State:**
- Module implemented
- Endpoint available: `POST /api/structured/ingest/reliefweb`
- Fails soft if API unavailable
- Needs API access resolution

### 3. NASA FIRMS 🔐 REQUIRES API KEY

**API:** https://firms.modaps.eosdis.nasa.gov/api/  
**Status:** Disabled by default  
**Confidence:** 0.70-0.90 (based on detection confidence)  
**Event Type:** `fire_activity`, `disaster`

**Configuration:**
```bash
FIRMS_ENABLED=true
FIRMS_API_KEY=your_key_here
# OR
FIRMS_MAP_KEY=your_map_key_here
```

**Register:** https://firms.modaps.eosdis.nasa.gov/api/

**Data Extracted:**
- Fire detection coordinates
- Brightness temperature (Kelvin)
- Confidence level (low/nominal/high)
- Detection timestamp
- Scan/track parameters

## API Endpoints

### Ingest All Structured Sources
```bash
curl -X POST http://127.0.0.1:8790/api/structured/ingest
```

Returns:
```json
{
  "ok": true,
  "total_new": 53,
  "sources": [
    {"ok": true, "source": "usgs", "new": 53},
    {"ok": false, "source": "reliefweb", "error": "403 Forbidden"},
    {"ok": false, "source": "firms", "error": "FIRMS disabled"}
  ]
}
```

### Individual Source Ingestion

**USGS:**
```bash
curl -X POST http://127.0.0.1:8790/api/structured/ingest/usgs
```

**ReliefWeb:**
```bash
curl -X POST 'http://127.0.0.1:8790/api/structured/ingest/reliefweb?days_back=7'
```

**FIRMS:**
```bash
curl -X POST 'http://127.0.0.1:8790/api/structured/ingest/firms?days_back=1'
```

### List Sources
```bash
curl http://127.0.0.1:8790/api/structured/sources
```

### Filter by Source Type
```bash
# Only USGS earthquakes
curl 'http://127.0.0.1:8790/api/conflict/events?window=day&source_type=usgs'

# All structured sources
curl 'http://127.0.0.1:8790/api/conflict/events?window=week' | jq '.items | map(select(.source_type | IN("usgs","reliefweb","firms")))'
```

## UI Integration

### Visual Distinction

**Marker Colors:**
- 🔵 Blue: RSS/GDELT (standard)
- 🟠 Orange: Social OSINT
- 🟣 **Purple: Structured sources (USGS/ReliefWeb/FIRMS)**

**Opacity:**
- 1.0 for verified structured sources
- 0.9 for corroborated social
- 0.7 for unverified social

### Source Badges in Popups

```
🌍 USGS Verified (95%)
🏛️ ReliefWeb (85%)
🛰️ NASA FIRMS (80%)
```

## Data Flow

```
USGS API → structured_ingest.py
           ↓
    Normalize to conflict event format
           ↓
    conflict_events.upsert_event()
           ↓
    SQLite conflict_events table
           ↓
    /api/conflict/events
           ↓
    Conflict layer (purple markers)
```

## Deduplication

Structured sources use the same deduplication logic as RSS/GDELT:
- Title similarity (>70%)
- Time proximity (±6 hours)
- Location proximity (<100km)

**No duplicates created** if similar event already exists.

## Event Types

New event types added:
- `fire_activity` - NASA FIRMS fire detections
- `humanitarian_incident` - ReliefWeb humanitarian reports

Existing types reused:
- `disaster` - USGS earthquakes, natural disasters
- `conflict` - Armed conflict events
- `military_activity`, `protest`, `strike`, `security_incident`, `other`

## Confidence Breakdown

| Source | Base Score | Reasoning |
|--------|-----------|-----------|
| USGS | 0.95 | Seismic instruments, global network, verified |
| ReliefWeb | 0.85 | UN-coordinated reports, curated content |
| FIRMS | 0.80 | Satellite detection, confidence-adjusted |
| RSS/GDELT | 0.75 | News aggregation, moderate verification |
| Social (corroborated) | 0.50 | Matches existing verified event |
| Social (unverified) | 0.35 | Single unverified source |

## Configuration

### Environment Variables

```bash
# ReliefWeb (currently disabled due to API access)
# No config needed - fails soft

# NASA FIRMS (disabled by default)
FIRMS_ENABLED=true
FIRMS_API_KEY=your_firms_api_key
FIRMS_MAP_KEY=alternative_map_key  # Alternative access method

# General structured ingestion
STRUCTURED_AUTO_INGEST=false  # Future: Auto-ingest on schedule
```

## Testing

### Verify USGS Ingestion
```bash
# Ingest
curl -X POST http://127.0.0.1:8790/api/structured/ingest/usgs

# Check count
curl 'http://127.0.0.1:8790/api/conflict/events?window=day&source_type=usgs' | jq '.count'
# Expected: ~50-100 earthquakes M2.5+ per day

# View sample
curl 'http://127.0.0.1:8790/api/conflict/events?window=day&source_type=usgs' | jq '.items[0]'
```

### Verify UI Rendering
1. Enable Conflict layer
2. Check for purple markers (structured sources)
3. Click marker → should show purple source badge
4. Verify "🌍 USGS Verified (95%)" badge appears

## Troubleshooting

### ReliefWeb 403 Forbidden
- **Status:** Known issue, fail-soft implemented
- **Impact:** ReliefWeb events don't ingest, other sources unaffected
- **Fix:** Investigate API access requirements (may need registration)

### FIRMS Not Ingesting
- **Check:** `FIRMS_ENABLED=true` in environment
- **Check:** `FIRMS_API_KEY` or `FIRMS_MAP_KEY` set
- **Register:** https://firms.modaps.eosdis.nasa.gov/api/

### No Structured Events Showing
- **Verify ingestion:** `curl -X POST .../api/structured/ingest`
- **Check database:** `sqlite3 ~/.config/overwatch/conflict_events.db "SELECT COUNT(*) FROM conflict_events WHERE source_type='usgs'"`
- **Check time window:** Structured events may be outside day/week range

## Future Enhancements

- [ ] Fix ReliefWeb API access
- [ ] FIRMS API key registration automation
- [ ] Auto-ingest scheduling
- [ ] Additional structured sources:
  - [ ] GDACS (Global Disaster Alert System)
  - [ ] ACAPS (Assessment Capacities Project)
  - [ ] OCHA (UN Humanitarian Office)
- [ ] Enhanced metadata extraction
- [ ] Source-specific confidence tuning
