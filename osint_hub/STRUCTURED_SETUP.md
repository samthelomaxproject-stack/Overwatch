# Structured Sources Setup Guide

Quick setup for ReliefWeb and FIRMS.

## USGS Earthquakes ✅

**No setup required** - works out of the box.

```bash
curl -X POST http://127.0.0.1:8790/api/structured/ingest/usgs
```

---

## ReliefWeb 🔐

**Requires:** Approved appname (free, no API key)

### Step 1: Request Appname

Visit: https://apidoc.reliefweb.int/parameters#appname

Fill out form with:
- **Application Name:** Overwatch OSINT Hub
- **URL:** https://github.com/samthelomaxproject-stack/Overwatch
- **Purpose:** Humanitarian intelligence / conflict monitoring
- **Contact Email:** (your email)

**Approval time:** Usually 24-48 hours

### Step 2: Configure Environment Variable

Once approved, add to your environment:

```bash
export RELIEFWEB_APPNAME=your_approved_name
```

Or in your `.env` file:
```bash
RELIEFWEB_APPNAME=your_approved_name
```

### Step 3: Restart Hub

```bash
pkill -f "uvicorn.*8790"
cd /path/to/Overwatch/osint_hub
python3 -m uvicorn app.main:app --host 127.0.0.1 --port 8790
```

### Step 4: Test

```bash
# Check status
curl http://127.0.0.1:8790/api/structured/sources | jq '.sources[] | select(.type == "reliefweb")'
# Should show "enabled": true

# Ingest
curl -X POST http://127.0.0.1:8790/api/structured/ingest/reliefweb
```

---

## NASA FIRMS 🛰️

**Requires:** FIRMS API key (free registration)

### Step 1: Register for API Key

Visit: https://firms.modaps.eosdis.nasa.gov/api/

1. Click "Request API Key"
2. Fill out form (name, email, purpose)
3. Check email for API key (instant)

### Step 2: Configure Environment Variables

```bash
export FIRMS_ENABLED=true
export FIRMS_API_KEY=your_key_here
```

Or in your `.env` file:
```bash
FIRMS_ENABLED=true
FIRMS_API_KEY=your_api_key
```

**Alternative:** If you have a MAP key instead:
```bash
FIRMS_ENABLED=true
FIRMS_MAP_KEY=your_map_key
```

### Step 3: Restart Hub

```bash
pkill -f "uvicorn.*8790"
cd /path/to/Overwatch/osint_hub
python3 -m uvicorn app.main:app --host 127.0.0.1 --port 8790
```

### Step 4: Test

```bash
# Check status
curl http://127.0.0.1:8790/api/structured/sources | jq '.sources[] | select(.type == "firms")'
# Should show "enabled": true

# Ingest (last 24 hours)
curl -X POST 'http://127.0.0.1:8790/api/structured/ingest/firms?days_back=1'
```

---

## Verification

### Check All Sources

```bash
curl http://127.0.0.1:8790/api/structured/sources | jq '.sources[] | {name, type, enabled}'
```

Expected output:
```json
{
  "name": "ReliefWeb",
  "type": "reliefweb",
  "enabled": true
}
{
  "name": "USGS Earthquakes",
  "type": "usgs",
  "enabled": true
}
{
  "name": "NASA FIRMS",
  "type": "firms",
  "enabled": true
}
```

### Ingest All Structured Sources

```bash
curl -X POST http://127.0.0.1:8790/api/structured/ingest
```

Expected response:
```json
{
  "ok": true,
  "total_new": 150,
  "sources": [
    {"ok": true, "source": "reliefweb", "new": 45},
    {"ok": true, "source": "usgs", "new": 53},
    {"ok": true, "source": "firms", "new": 52}
  ]
}
```

### View Events on Map

1. Open Overwatch app
2. Enable Conflict layer
3. Look for purple markers (structured sources)
4. Click marker → verify source badge:
   - 🏛️ ReliefWeb
   - 🌍 USGS
   - 🛰️ FIRMS

---

## Troubleshooting

### ReliefWeb: "requires approved appname"

**Symptom:**
```json
{
  "ok": false,
  "error": "ReliefWeb requires approved appname..."
}
```

**Fix:** Request appname approval (see Step 1 above)

### FIRMS: "FIRMS disabled"

**Symptom:**
```json
{
  "ok": false,
  "error": "FIRMS ingestion disabled (FIRMS_ENABLED=false)"
}
```

**Fix:** Set `FIRMS_ENABLED=true` and `FIRMS_API_KEY`

### FIRMS: "API key not configured"

**Symptom:**
```json
{
  "ok": false,
  "error": "FIRMS API key not configured..."
}
```

**Fix:** Set `FIRMS_API_KEY=your_key`

### Events Not Showing on Map

**Check:**
1. Events were actually ingested: `curl .../api/conflict/events?source_type=reliefweb`
2. Events have coordinates: Check `lat` and `lon` in response
3. Time window includes events: Try `?window=month`
4. Conflict layer is enabled in UI

---

## Rate Limits & Best Practices

### USGS
- **Limit:** None published
- **Recommendation:** Poll every 15-30 minutes
- **Data freshness:** Real-time (< 1 minute delay)

### ReliefWeb
- **Limit:** Not strictly published
- **Recommendation:** Poll every 1-4 hours (reports are curated, not real-time)
- **Data freshness:** Hours to days

### FIRMS
- **Limit:** Varies by plan (check your API key tier)
- **Recommendation:** Poll every 30-60 minutes
- **Data freshness:** Near real-time (~3 hour latency)

---

## Production Deployment

For production, add to your systemd service or docker-compose:

```yaml
environment:
  - RELIEFWEB_APPNAME=your_approved_name
  - FIRMS_ENABLED=true
  - FIRMS_API_KEY=your_key
```

Or in `.env` file:
```bash
# Structured OSINT Sources
RELIEFWEB_APPNAME=overwatch_osint_prod
FIRMS_ENABLED=true
FIRMS_API_KEY=abc123def456...
```

**Security:** Never commit `.env` to git. Use secrets management in production.
