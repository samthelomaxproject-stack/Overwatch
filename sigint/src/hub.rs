/// Hub aggregation server.
///
/// Receives signed tile batches from nodes, verifies sanity, merges into
/// hub SQLite DB, and serves delta queries.
///
/// Runs as a simple HTTP server on a configurable port (default 8789).
/// Uses a minimal hand-rolled HTTP handler — no Tokio, no async, just
/// std::net::TcpListener + threads. Suitable for VPN/LAN with low concurrency.
///
/// # Usage (standalone binary, Phase 2)
/// ```bash
/// HUB_PORT=8789 HUB_DB=/var/lib/overwatch/hub.db ./hub-api
/// ```
///
/// # Hub DB schema (hub.db)
/// ```sql
/// CREATE TABLE merged_tiles (
///     tile_id TEXT, time_bucket INTEGER, sensor_type TEXT, dimension TEXT,
///     mean_val REAL, max_val REAL, sample_count INTEGER, source_count INTEGER,
///     confidence REAL, updated_at INTEGER,
///     PRIMARY KEY (tile_id, time_bucket, sensor_type, dimension)
/// );
/// CREATE TABLE node_registry (
///     device_id TEXT PRIMARY KEY, source_type TEXT,
///     trust_weight REAL DEFAULT 1.0, last_seen INTEGER
/// );
/// CREATE TABLE delta_cursors (
///     device_id TEXT PRIMARY KEY, cursor_ts INTEGER DEFAULT 0
/// );
/// ```
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use crate::{Error, wire::TileUpdate};
use crate::sync::{AckResult, TileDelta};
use crate::sanitize::{sanitize_rf, sanitize_wifi, RateLimiter};

const HUB_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS merged_tiles (
    tile_id       TEXT    NOT NULL,
    time_bucket   INTEGER NOT NULL,
    sensor_type   TEXT    NOT NULL,
    dimension     TEXT    NOT NULL,
    mean_val      REAL    NOT NULL,
    max_val       REAL    NOT NULL,
    sample_count  INTEGER NOT NULL DEFAULT 1,
    source_count  INTEGER NOT NULL DEFAULT 1,
    confidence    REAL    NOT NULL,
    updated_at    INTEGER NOT NULL,
    PRIMARY KEY (tile_id, time_bucket, sensor_type, dimension)
);

CREATE TABLE IF NOT EXISTS node_registry (
    device_id    TEXT PRIMARY KEY,
    source_type  TEXT,
    trust_weight REAL NOT NULL DEFAULT 1.0,
    last_seen    INTEGER
);

CREATE TABLE IF NOT EXISTS delta_cursors (
    device_id  TEXT PRIMARY KEY,
    cursor_ts  INTEGER NOT NULL DEFAULT 0
);
"#;

// ── Hub DB ────────────────────────────────────────────────────────────────────

pub struct HubDb {
    conn: Connection,
}

impl HubDb {
    pub fn open(path: &str) -> Result<Self, Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(HUB_SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> Result<Self, Error> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(HUB_SCHEMA)?;
        Ok(Self { conn })
    }

    /// Register or update a node.
    pub fn upsert_node(&mut self, device_id: &str, source_type: &str, now: u64) -> Result<(), Error> {
        self.conn.execute(
            "INSERT OR REPLACE INTO node_registry (device_id, source_type, trust_weight, last_seen)
             VALUES (?1, ?2, 1.0, ?3)",
            params![device_id, source_type, now as i64],
        )?;
        Ok(())
    }

    /// Merge a TileUpdate batch into merged_tiles.
    /// Sanitizes all inputs before merging. Tracks per-node rate limits.
    /// Uses confidence-weighted mean merge strategy.
    pub fn merge_update(&mut self, update: &TileUpdate) -> Result<AckResult, Error> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.upsert_node(&update.device_id, &update.source_type, now)?;

        let mut accepted = 0u32;
        let mut rejected = 0u32;
        let mut rate_limiter = RateLimiter::new();
        let tx = self.conn.transaction()?;

        for tile in &update.tiles {
            // Merge RF aggregates
            if let Some(rf_list) = &tile.rf {
                for agg in rf_list {
                    // Sanitize
                    let agg = match sanitize_rf(agg) {
                        Some(a) => a,
                        None => { rejected += 1; continue; }
                    };
                    let dim = format!("rf:{}-{}", agg.freq_start_hz, agg.freq_end_hz);
                    // Rate limit
                    if !rate_limiter.allow_rf(&update.device_id, &tile.tile_id, tile.time_bucket, &dim) {
                        rejected += 1;
                        continue;
                    }
                    tx.execute(
                        r#"INSERT INTO merged_tiles
                           (tile_id, time_bucket, sensor_type, dimension, mean_val, max_val,
                            sample_count, source_count, confidence, updated_at)
                           VALUES (?1, ?2, 'rf', ?3, ?4, ?5, ?6, 1, ?7, ?8)
                           ON CONFLICT(tile_id, time_bucket, sensor_type, dimension) DO UPDATE SET
                             mean_val     = (mean_val * sample_count + excluded.mean_val * excluded.sample_count)
                                           / (sample_count + excluded.sample_count),
                             max_val      = MAX(max_val, excluded.max_val),
                             sample_count = sample_count + excluded.sample_count,
                             source_count = source_count + 1,
                             confidence   = (confidence + excluded.confidence) / 2.0,
                             updated_at   = excluded.updated_at"#,
                        params![
                            tile.tile_id, tile.time_bucket as i64, dim,
                            agg.mean_power_dbm, agg.max_power_dbm,
                            agg.sample_count as i64, agg.confidence, now as i64
                        ],
                    )?;
                    accepted += 1;
                }
            }

            // Merge Wi-Fi channel hotness
            if let Some(wifi) = &tile.wifi {
                for ch in &wifi.channel_hotness {
                    // Sanitize
                    let ch = match sanitize_wifi(ch) {
                        Some(c) => c,
                        None => { rejected += 1; continue; }
                    };
                    // Rate limit
                    if !rate_limiter.allow_wifi(&update.device_id, &tile.tile_id, tile.time_bucket, &ch.band, ch.channel) {
                        rejected += 1;
                        continue;
                    }
                    let dim = format!("wifi:{}:{}", ch.band, ch.channel);
                    tx.execute(
                        r#"INSERT INTO merged_tiles
                           (tile_id, time_bucket, sensor_type, dimension, mean_val, max_val,
                            sample_count, source_count, confidence, updated_at)
                           VALUES (?1, ?2, 'wifi_channel', ?3, ?4, ?5, ?6, 1, ?7, ?8)
                           ON CONFLICT(tile_id, time_bucket, sensor_type, dimension) DO UPDATE SET
                             mean_val     = (mean_val * sample_count + excluded.mean_val * excluded.sample_count)
                                           / (sample_count + excluded.sample_count),
                             max_val      = MAX(max_val, excluded.max_val),
                             sample_count = sample_count + excluded.sample_count,
                             source_count = source_count + 1,
                             confidence   = (confidence + excluded.confidence) / 2.0,
                             updated_at   = excluded.updated_at"#,
                        params![
                            tile.tile_id, tile.time_bucket as i64, dim,
                            ch.mean_rssi_dbm, ch.max_rssi_dbm,
                            ch.count as i64, ch.confidence, now as i64
                        ],
                    )?;
                    accepted += 1;
                }
            }
        }

        tx.commit()?;
        Ok(AckResult { accepted, rejected, cursor: now })
    }

    /// Return tiles updated since cursor_ts for a given device.
    /// Values are time-decayed before sending so renderers show fading heat.
    pub fn get_delta(&self, _device_id: &str, cursor_ts: u64) -> Result<TileDelta, Error> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut stmt = self.conn.prepare(
            r#"SELECT tile_id, time_bucket, sensor_type, dimension,
                      mean_val, max_val, sample_count, confidence, updated_at
               FROM merged_tiles WHERE updated_at > ?1
               ORDER BY updated_at ASC LIMIT 1000"#,
        )?;

        // Collect raw rows; rebuild as TileUpdate grouped by tile_id
        #[derive(Debug)]
        struct Row {
            tile_id: String, time_bucket: u64, sensor_type: String,
            dimension: String, mean_val: f64, max_val: f64,
            sample_count: u32, confidence: f64, updated_at: u64,
        }

        let rows = stmt.query_map(params![cursor_ts as i64], |r| {
            Ok(Row {
                tile_id:      r.get(0)?,
                time_bucket:  r.get::<_, i64>(1)? as u64,
                sensor_type:  r.get(2)?,
                dimension:    r.get(3)?,
                mean_val:     r.get(4)?,
                max_val:      r.get(5)?,
                sample_count: r.get::<_, i64>(6)? as u32,
                confidence:   r.get(7)?,
                updated_at:   r.get::<_, i64>(8)? as u64,
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        let max_cursor = rows.iter().map(|r| r.updated_at).max().unwrap_or(cursor_ts);
        let now_secs = now as f64;

        // Build TileUpdate with time-decayed values
        use crate::wire::{TileData, RfAggregate, WifiData, ChannelHotness};
        use crate::sanitize::{decay_factor, RF_DECAY_HALF_LIFE_SECS, WIFI_DECAY_HALF_LIFE_SECS};
        use std::collections::HashMap;

        let mut tile_map: HashMap<(String, u64), TileData> = HashMap::new();

        for row in &rows {
            let key = (row.tile_id.clone(), row.time_bucket);
            let tile = tile_map.entry(key).or_insert_with(|| {
                TileData::new(row.tile_id.clone(), row.time_bucket)
            });

            let age = now_secs - row.updated_at as f64;

            match row.sensor_type.as_str() {
                "rf" => {
                    let decay = decay_factor(age, RF_DECAY_HALF_LIFE_SECS);
                    let parts: Vec<&str> = row.dimension.trim_start_matches("rf:").split('-').collect();
                    if parts.len() == 2 {
                        if let (Ok(start), Ok(end)) = (parts[0].parse(), parts[1].parse()) {
                            let agg = RfAggregate {
                                freq_start_hz: start, freq_end_hz: end,
                                // Apply decay: power in dBm is logarithmic, so we decay confidence
                                // and scale mean linearly (approximation adequate for display)
                                mean_power_dbm: row.mean_val,
                                max_power_dbm: row.max_val,
                                sample_count: row.sample_count,
                                // Decay expressed through confidence so renderer adjusts opacity
                                confidence: (row.confidence * decay).clamp(0.0, 1.0),
                            };
                            tile.rf.get_or_insert_with(Vec::new).push(agg);
                        }
                    }
                }
                "wifi_channel" => {
                    // dimension = "wifi:{band}:{channel}"
                    let rest = row.dimension.trim_start_matches("wifi:");
                    let mut parts = rest.splitn(2, ':');
                    if let (Some(band), Some(ch_str)) = (parts.next(), parts.next()) {
                        if let Ok(channel) = ch_str.parse() {
                            let wifi_decay = decay_factor(age, WIFI_DECAY_HALF_LIFE_SECS);
                            let ch = ChannelHotness {
                                band: band.to_string(), channel,
                                count: row.sample_count,
                                mean_rssi_dbm: row.mean_val, max_rssi_dbm: row.max_val,
                                confidence: (row.confidence * wifi_decay).clamp(0.0, 1.0),
                            };
                            tile.wifi.get_or_insert(WifiData { channel_hotness: vec![] })
                                .channel_hotness.push(ch);
                        }
                    }
                }
                _ => {}
            }
        }

        let tiles_vec: Vec<TileData> = tile_map.into_values().collect();
        let mut update = TileUpdate::new("hub", "hub_local");
        update.tiles = tiles_vec;

        Ok(TileDelta { tiles: vec![update], cursor: max_cursor })
    }
}

// ── Minimal HTTP server ───────────────────────────────────────────────────────

/// Hub API server configuration.
#[derive(Debug, Clone)]
pub struct HubConfig {
    pub bind_addr: String,
    pub db_path: String,
    /// If false, hub runs in aggregation-only mode (no local collector)
    pub collector_enabled: bool,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:8789".to_string(),
            db_path: "/tmp/hub.db".to_string(),
            collector_enabled: true,
        }
    }
}

/// Shared hub state across request handler threads.
struct HubState {
    db: HubDb,
}

/// Start the hub API server (blocking).
/// Spawns one thread per connection.
pub fn run_hub(config: HubConfig) -> Result<(), Error> {
    let db = HubDb::open(&config.db_path)?;
    let state = Arc::new(Mutex::new(HubState { db }));

    let listener = TcpListener::bind(&config.bind_addr)
        .map_err(|e| Error::Other(format!("Failed to bind {}: {e}", config.bind_addr)))?;

    log::info!("Hub API listening on {}", config.bind_addr);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    if let Err(e) = handle_connection(stream, state) {
                        log::error!("Connection error: {e}");
                    }
                });
            }
            Err(e) => log::error!("Accept error: {e}"),
        }
    }
    Ok(())
}

fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<HubState>>) -> Result<(), Error> {
    let mut reader = BufReader::new(stream.try_clone().map_err(Error::Io)?);

    // Read request line
    let mut request_line = String::new();
    reader.read_line(&mut request_line).map_err(Error::Io)?;
    let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }
    let method = parts[0];
    let path = parts[1];

    // Read headers
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).map_err(Error::Io)?;
        let line = line.trim().to_lowercase();
        if line.is_empty() { break; }
        if line.starts_with("content-length:") {
            content_length = line[15..].trim().parse().unwrap_or(0);
        }
    }

    // Route
    let (status, body) = match (method, path) {
        ("GET", "/health") => {
            (200, r#"{"status":"ok"}"#.to_string())
        }

        ("POST", "/api/push") => {
            let mut buf = vec![0u8; content_length.min(1_048_576)];
            use std::io::Read;
            reader.read_exact(&mut buf).map_err(Error::Io)?;

            match serde_json::from_slice::<TileUpdate>(&buf) {
                Ok(update) => {
                    let mut s = state.lock().unwrap();
                    match s.db.merge_update(&update) {
                        Ok(ack) => (200, serde_json::to_string(&ack).unwrap()),
                        Err(e) => (500, format!(r#"{{"error":"{e}"}}"#)),
                    }
                }
                Err(e) => (400, format!(r#"{{"error":"bad json: {e}"}}"#)),
            }
        }

        ("GET", p) if p.starts_with("/api/delta") => {
            let cursor = parse_query_param(p, "cursor")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0u64);
            let device_id = parse_query_param(p, "device_id").unwrap_or_default();

            let s = state.lock().unwrap();
            match s.db.get_delta(&device_id, cursor) {
                Ok(delta) => (200, serde_json::to_string(&delta).unwrap()),
                Err(e) => (500, format!(r#"{{"error":"{e}"}}"#)),
            }
        }

        _ => (404, r#"{"error":"not found"}"#.to_string()),
    };

    let response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{body}",
        len = body.len()
    );
    stream.write_all(response.as_bytes()).map_err(Error::Io)?;
    Ok(())
}

fn parse_query_param(path: &str, key: &str) -> Option<String> {
    let query = path.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        if kv.next()? == key {
            return Some(kv.next().unwrap_or("").to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{TileData, RfAggregate};

    fn make_db() -> HubDb {
        HubDb::open_in_memory().unwrap()
    }

    #[test]
    fn schema_creates_tables() {
        let db = make_db();
        let delta = db.get_delta("test", 0).unwrap();
        assert!(delta.tiles.is_empty() || delta.tiles[0].tiles.is_empty());
    }

    #[test]
    fn merge_rf_update() {
        let mut db = make_db();
        let mut update = TileUpdate::new("node-001", "entity");
        let mut tile = TileData::new("8a2a1072b59ffff", 1740000060);
        tile.rf = Some(vec![RfAggregate {
            freq_start_hz: 2_400_000_000,
            freq_end_hz: 2_500_000_000,
            mean_power_dbm: -72.0,
            max_power_dbm: -60.0,
            sample_count: 5,
            confidence: 0.8,
        }]);
        update.tiles = vec![tile];

        let ack = db.merge_update(&update).unwrap();
        assert_eq!(ack.accepted, 1);
    }

    #[test]
    fn merge_two_nodes_same_tile() {
        let mut db = make_db();

        for (device, mean) in [("node-a", -70.0f64), ("node-b", -80.0)] {
            let mut update = TileUpdate::new(device, "entity");
            let mut tile = TileData::new("8a2a1072b59ffff", 1740000060);
            tile.rf = Some(vec![RfAggregate {
                freq_start_hz: 2_400_000_000, freq_end_hz: 2_500_000_000,
                mean_power_dbm: mean, max_power_dbm: mean + 10.0,
                sample_count: 3, confidence: 0.7,
            }]);
            update.tiles = vec![tile];
            db.merge_update(&update).unwrap();
        }

        // Both nodes contributed, source_count should be 2
        let mut stmt = db.conn.prepare(
            "SELECT source_count FROM merged_tiles WHERE sensor_type='rf'"
        ).unwrap();
        let count: i64 = stmt.query_row([], |r| r.get(0)).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn delta_returns_recent_tiles() {
        let mut db = make_db();
        let mut update = TileUpdate::new("node-001", "entity");
        let mut tile = TileData::new("8a2a1072b59ffff", 1740000060);
        tile.rf = Some(vec![RfAggregate {
            freq_start_hz: 900_000_000, freq_end_hz: 928_000_000,
            mean_power_dbm: -85.0, max_power_dbm: -80.0,
            sample_count: 2, confidence: 0.5,
        }]);
        update.tiles = vec![tile];
        db.merge_update(&update).unwrap();

        let delta = db.get_delta("node-001", 0).unwrap();
        assert!(!delta.tiles.is_empty());
        assert!(delta.cursor > 0);
    }

    #[test]
    fn parse_query_param_works() {
        assert_eq!(
            parse_query_param("/api/delta?device_id=abc&cursor=1000", "cursor"),
            Some("1000".to_string())
        );
        assert_eq!(
            parse_query_param("/api/delta?device_id=abc&cursor=1000", "device_id"),
            Some("abc".to_string())
        );
        assert_eq!(
            parse_query_param("/api/delta?device_id=abc", "cursor"),
            None
        );
    }
}
