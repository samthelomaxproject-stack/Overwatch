# Social OSINT Pipeline

Minimal public social media ingestion feeding the Conflict layer.

## Overview

Social OSINT provides **real-time, lower-confidence** intelligence from public social platforms. Events are marked as `unverified` and display with warning badges in the UI.

## Architecture

- **Hub-only ingestion**: Runs on OSINT Hub (port 8790)
- **Storage**: SQLite `conflict_events` table (same as RSS/GDELT)
- **Source type**: `"social"`
- **Default confidence**: 0.35 (vs 0.75+ for RSS/GDELT)
- **Verification status**: `"unverified"` by default

## Supported Sources

### Reddit (JSON API)
- No authentication required
- Public subreddits only
- `.json` endpoint (e.g., `/r/UkrainianConflict/new.json`)

### Telegram (RSS via RSShub)
- Public channels with RSS feeds
- Currently disabled (feed parsing issues)
- Future: Direct API integration

## Configuration

Edit `osint_hub/app/social_ingest.py`:

```python
SOCIAL_SOURCES = [
    {
        "name": "r/UkrainianConflict",
        "type": "reddit_json",
        "url": "https://www.reddit.com/r/UkrainianConflict/new.json",
        "event_type": "conflict",
        "region": "ukraine"
    },
    # Add more sources...
]
```

## Confidence Model

```python
BASE_CONFIDENCE = 0.35

# Boosts:
+ 0.10  # Detailed location (>10 chars)
+ 0.15  # Multiple similar reports (per additional source, max 3)

# Penalties:
- 0.10  # Vague text (<100 chars)
```

## Geocoding

1. **Extract location** from title/text (regex patterns)
2. **Geocode** via Nominatim (OpenStreetMap)
3. **Fallback** to regional coordinates if geocoding fails:
   - Ukraine: 49.0°N, 32.0°E
   - Syria: 35.0°N, 38.0°E
   - Global: Skip event

## API Endpoints

### Trigger Ingestion
```bash
curl -X POST http://127.0.0.1:8790/api/social/ingest
```

Returns:
```json
{
  "ok": true,
  "total_new": 7,
  "sources": [
    {"ok": true, "new": 3, "source": "r/UkrainianConflict"},
    {"ok": true, "new": 4, "source": "r/syriancivilwar"}
  ],
  "ingested_at": "2026-03-26T06:01:55Z"
}
```

### List Sources
```bash
curl http://127.0.0.1:8790/api/social/sources
```

## Event Format

Social events follow the same schema as RSS/GDELT but include:

```json
{
  "id": 186,
  "title": "...",
  "summary": "...",
  "source_type": "social",
  "source_name": "r/syriancivilwar",
  "source_url": "https://reddit.com/...",
  "published_at": "2026-03-25T21:05:20+00:00",
  "event_type": "conflict",
  "location": "Syria region",
  "lat": 35.0,
  "lon": 38.0,
  "confidence_score": 0.35,
  "verification_status": "unverified"
}
```

## UI Integration

The Conflict layer automatically handles social sources:

- **Map markers**: Same as RSS/GDELT
- **Popup badge**: `⚠️ Social OSINT - unverified (35%)`
- **Styling**: Orange warning color

## Rate Limiting

- **Nominatim geocoding**: 1 req/sec (OSM policy)
- **Inter-source delay**: 1 second
- **Reddit API**: No key required, respects User-Agent policy

## Future Enhancements

### Near-term
- [ ] Telegram direct API integration
- [ ] Twitter/X public timeline scraping
- [ ] Cross-source correlation for verification_status upgrades

### Long-term
- [ ] Multi-source deduplication
- [ ] Automated verification via RSS/GDELT correlation
- [ ] Sentiment analysis for event classification
- [ ] Image hash matching for duplicate detection

## Maintenance

### Clear old social events
```bash
sqlite3 ~/.config/overwatch/conflict_events.db \
  "DELETE FROM conflict_events WHERE source_type='social' AND published_at < datetime('now', '-7 days')"
```

### Add new Reddit source
Edit `SOCIAL_SOURCES` in `social_ingest.py`, restart hub.

### Disable social ingestion
Remove `POST /api/social/ingest` calls from any automation/cron.

## Security Notes

- **Public data only**: No authentication, no login-dependent platforms
- **User-Agent compliance**: Identifies as `Overwatch/0.2.0 (OSINT Hub)`
- **Rate limiting**: Respects API policies (Nominatim, Reddit)
- **No PII collection**: Events are public, pre-published content only
