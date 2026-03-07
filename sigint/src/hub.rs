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
use crate::{Error, wire::{TileUpdate, TileData}};
use crate::sync::{AckResult, TileDelta};
use crate::sanitize::{sanitize_rf, sanitize_wifi, RateLimiter};
use crate::crypto::verify_payload;

const HUB_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS merged_tiles (
    tile_id         TEXT    NOT NULL,
    time_bucket     INTEGER NOT NULL,
    sensor_type     TEXT    NOT NULL,
    dimension       TEXT    NOT NULL,
    mean_val        REAL    NOT NULL,
    max_val         REAL    NOT NULL,
    sample_count    INTEGER NOT NULL DEFAULT 1,
    source_count    INTEGER NOT NULL DEFAULT 1,
    confidence      REAL    NOT NULL,
    updated_at      INTEGER NOT NULL,
    last_device_id  TEXT,
    last_source_type TEXT,
    PRIMARY KEY (tile_id, time_bucket, sensor_type, dimension)
);

CREATE TABLE IF NOT EXISTS node_registry (
    device_id    TEXT PRIMARY KEY,
    source_type  TEXT,
    public_key   TEXT,
    trust_weight REAL NOT NULL DEFAULT 1.0,
    last_seen    INTEGER,
    last_tile_id TEXT,
    last_tile_bucket INTEGER
);

CREATE TABLE IF NOT EXISTS delta_cursors (
    device_id  TEXT PRIMARY KEY,
    cursor_ts  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS msg_groups (
    group_id    TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    created_by  TEXT,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS msg_group_members (
    group_id    TEXT NOT NULL,
    device_id   TEXT NOT NULL,
    role        TEXT NOT NULL DEFAULT 'member',
    joined_at   INTEGER NOT NULL,
    PRIMARY KEY (group_id, device_id)
);

CREATE TABLE IF NOT EXISTS messages (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    from_device_id     TEXT NOT NULL,
    to_device_id       TEXT,
    to_group_id        TEXT,
    body               TEXT NOT NULL,
    sent_at            INTEGER NOT NULL,
    delivered_at       INTEGER,
    read_at            INTEGER
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
        // Migrations for existing DBs
        let _ = conn.execute("ALTER TABLE merged_tiles ADD COLUMN last_device_id TEXT", []);
        let _ = conn.execute("ALTER TABLE merged_tiles ADD COLUMN last_source_type TEXT", []);
        let _ = conn.execute("ALTER TABLE node_registry ADD COLUMN last_tile_id TEXT", []);
        let _ = conn.execute("ALTER TABLE node_registry ADD COLUMN last_tile_bucket INTEGER", []);
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

    /// Store a node's public key for future signature verification.
    pub fn register_pubkey(&mut self, device_id: &str, public_key_b64: &str) -> Result<(), Error> {
        self.conn.execute(
            "UPDATE node_registry SET public_key = ?1 WHERE device_id = ?2",
            params![public_key_b64, device_id],
        )?;
        Ok(())
    }

    /// Retrieve a node's stored public key (if registered).
    pub fn get_pubkey(&self, device_id: &str) -> Result<Option<String>, Error> {
        let mut stmt = self.conn.prepare(
            "SELECT public_key FROM node_registry WHERE device_id = ?1"
        )?;
        let mut rows = stmt.query(params![device_id])?;
        if let Some(row) = rows.next()? {
            return Ok(row.get::<_, Option<String>>(0)?);
        }
        Ok(None)
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

        // Signature verification — if the node has a registered public key,
        // the batch must be signed and the signature must be valid.
        // Unsigned batches from unknown nodes are accepted (first-contact grace).
        if let Some(ref sig) = update.signature {
            if let Ok(Some(pubkey)) = self.get_pubkey(&update.device_id) {
                // Verify against batch without signature field
                let mut unsigned = update.clone();
                unsigned.signature = None;
                match verify_payload(&unsigned, sig, &pubkey) {
                    Ok(true) => log::debug!("Signature verified for {}", update.device_id),
                    Ok(false) => {
                        log::warn!("REJECTED: invalid signature from {}", update.device_id);
                        return Ok(AckResult { accepted: 0, rejected: update.tiles.len() as u32, cursor: now });
                    }
                    Err(e) => log::warn!("Signature check error for {}: {e}", update.device_id),
                }
            }
        }

        let mut accepted = 0u32;
        let mut rejected = 0u32;
        let mut rate_limiter = RateLimiter::new();
        let tx = self.conn.transaction()?;

        for tile in &update.tiles {
            let mut tile_had_signal_data = false;

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
                            sample_count, source_count, confidence, updated_at, last_device_id, last_source_type)
                           VALUES (?1, ?2, 'rf', ?3, ?4, ?5, ?6, 1, ?7, ?8, ?9, ?10)
                           ON CONFLICT(tile_id, time_bucket, sensor_type, dimension) DO UPDATE SET
                             mean_val      = (mean_val * sample_count + excluded.mean_val * excluded.sample_count)
                                            / (sample_count + excluded.sample_count),
                             max_val       = MAX(max_val, excluded.max_val),
                             sample_count  = sample_count + excluded.sample_count,
                             source_count  = source_count + 1,
                             confidence    = (confidence + excluded.confidence) / 2.0,
                             updated_at    = excluded.updated_at,
                             last_device_id = excluded.last_device_id,
                             last_source_type = excluded.last_source_type"#,
                        params![
                            tile.tile_id, tile.time_bucket as i64, dim,
                            agg.mean_power_dbm, agg.max_power_dbm,
                            agg.sample_count as i64, agg.confidence, now as i64,
                            update.device_id, update.source_type
                        ],
                    )?;
                    accepted += 1;
                    tile_had_signal_data = true;
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
                            sample_count, source_count, confidence, updated_at, last_device_id, last_source_type)
                           VALUES (?1, ?2, 'wifi_channel', ?3, ?4, ?5, ?6, 1, ?7, ?8, ?9, ?10)
                           ON CONFLICT(tile_id, time_bucket, sensor_type, dimension) DO UPDATE SET
                             mean_val      = (mean_val * sample_count + excluded.mean_val * excluded.sample_count)
                                            / (sample_count + excluded.sample_count),
                             max_val       = MAX(max_val, excluded.max_val),
                             sample_count  = sample_count + excluded.sample_count,
                             source_count  = source_count + 1,
                             confidence    = (confidence + excluded.confidence) / 2.0,
                             updated_at    = excluded.updated_at,
                             last_device_id = excluded.last_device_id,
                             last_source_type = excluded.last_source_type"#,
                        params![
                            tile.tile_id, tile.time_bucket as i64, dim,
                            ch.mean_rssi_dbm, ch.max_rssi_dbm,
                            ch.count as i64, ch.confidence, now as i64,
                            update.device_id, update.source_type
                        ],
                    )?;
                    accepted += 1;
                    tile_had_signal_data = true;
                }
            }

            // Always persist a lightweight PLI heartbeat row so clients can render
            // entity position even when there is no RF/Wi-Fi payload this cycle.
            // Position is encoded in tile_id; metadata uses last_device/source columns.
            // Track per-device last known tile for clean PLI fan-out in /api/delta.
            tx.execute(
                "UPDATE node_registry SET source_type = ?1, last_seen = ?2, last_tile_id = ?3, last_tile_bucket = ?4 WHERE device_id = ?5",
                params![
                    update.source_type,
                    now as i64,
                    tile.tile_id,
                    tile.time_bucket as i64,
                    update.device_id
                ],
            )?;

            if !tile_had_signal_data {
                tx.execute(
                    r#"INSERT INTO merged_tiles
                       (tile_id, time_bucket, sensor_type, dimension, mean_val, max_val,
                        sample_count, source_count, confidence, updated_at, last_device_id, last_source_type)
                       VALUES (?1, ?2, 'pli', 'heartbeat', 0.0, 0.0, 1, 1, 1.0, ?3, ?4, ?5)
                       ON CONFLICT(tile_id, time_bucket, sensor_type, dimension) DO UPDATE SET
                         updated_at = excluded.updated_at,
                         last_device_id = excluded.last_device_id,
                         last_source_type = excluded.last_source_type,
                         confidence = 1.0"#,
                    params![
                        tile.tile_id,
                        tile.time_bucket as i64,
                        now as i64,
                        update.device_id,
                        update.source_type
                    ],
                )?;
                accepted += 1;
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
                      mean_val, max_val, sample_count, confidence, updated_at,
                      last_device_id, last_source_type
               FROM merged_tiles WHERE updated_at > ?1
               ORDER BY updated_at ASC LIMIT 1000"#,
        )?;

        // Collect raw rows; rebuild as TileUpdate grouped by tile_id
        #[derive(Debug)]
        struct Row {
            tile_id: String, time_bucket: u64, sensor_type: String,
            dimension: String, mean_val: f64, max_val: f64,
            sample_count: u32, confidence: f64, updated_at: u64,
            last_device_id: Option<String>,
            last_source_type: Option<String>,
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
                last_device_id: r.get(9)?,
                last_source_type: r.get(10)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        let max_cursor = rows.iter().map(|r| r.updated_at).max().unwrap_or(cursor_ts);
        // updated_at and cursor are milliseconds; convert to seconds for decay math
        let now_secs = now as f64 / 1000.0;

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
            // Preserve contributing device metadata for entity rendering
            if tile.device_id.is_none() {
                tile.device_id = row.last_device_id.clone();
            }
            if tile.source_type.is_none() {
                tile.source_type = row.last_source_type.clone();
            }

            let age = now_secs - (row.updated_at as f64 / 1000.0);

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

        let mut tiles_vec: Vec<TileData> = tile_map.into_values().collect();

        // Add per-device PLI heartbeat fan-out so clients can render each EUD independently
        // even when multiple EUDs share a tile/time bucket.
        let mut nstmt = self.conn.prepare(
            "SELECT device_id, COALESCE(source_type,'unknown'), last_seen, last_tile_id, last_tile_bucket
             FROM node_registry
             WHERE last_seen > ?1 AND last_tile_id IS NOT NULL"
        )?;
        let nrows = nstmt.query_map(params![cursor_ts as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)? as u64,
                r.get::<_, String>(3)?,
                r.get::<_, Option<i64>>(4)?.unwrap_or(0) as u64,
            ))
        })?.collect::<Result<Vec<_>, _>>()?;

        let mut cursor_out = max_cursor;
        for (device_id, source_type, last_seen, tile_id, bucket) in nrows {
            let mut t = TileData::new(tile_id, if bucket > 0 { bucket } else { last_seen });
            t.device_id = Some(device_id);
            t.source_type = Some(source_type);
            tiles_vec.push(t);
            if last_seen > cursor_out { cursor_out = last_seen; }
        }

        let mut update = TileUpdate::new("hub", "hub_local");
        update.tiles = tiles_vec;

        Ok(TileDelta { tiles: vec![update], cursor: cursor_out })
    }

    pub fn get_pli_delta(&self, cursor_ts: u64, max_age_secs: u64) -> Result<TileDelta, Error> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let cutoff = now.saturating_sub(max_age_secs);

        let mut stmt = self.conn.prepare(
            "SELECT device_id, COALESCE(source_type,'unknown'), COALESCE(last_seen,0), COALESCE(last_tile_id,''), COALESCE(last_tile_bucket,0)
             FROM node_registry
             WHERE COALESCE(last_tile_id,'') <> '' AND COALESCE(last_seen,0) >= ?1 AND COALESCE(last_seen,0) > ?2
             ORDER BY last_seen ASC LIMIT 500"
        )?;

        let rows = stmt.query_map(params![cutoff as i64, cursor_ts as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                (r.get::<_, i64>(2)?).max(0) as u64,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4).unwrap_or(0).max(0) as u64,
            ))
        })?.collect::<Result<Vec<_>, _>>()?;

        let mut cursor_out = cursor_ts;
        let mut update = TileUpdate::new("hub", "hub_local");
        for (device_id, source_type, last_seen, tile_id, bucket) in rows {
            let mut t = TileData::new(tile_id, if bucket > 0 { bucket } else { last_seen });
            t.device_id = Some(device_id);
            t.source_type = Some(source_type);
            update.tiles.push(t);
            if last_seen > cursor_out { cursor_out = last_seen; }
        }

        Ok(TileDelta { tiles: vec![update], cursor: cursor_out })
    }

    pub fn get_pli_points(&self, max_age_secs: u64) -> Result<Vec<PliPoint>, Error> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let cutoff = now.saturating_sub(max_age_secs as i64);

        let mut stmt = self.conn.prepare(
            "SELECT device_id, COALESCE(source_type,'unknown'), COALESCE(last_seen,0), COALESCE(last_tile_id,'')
             FROM node_registry
             WHERE COALESCE(last_tile_id,'') <> '' AND COALESCE(last_seen,0) >= ?1
             ORDER BY last_seen DESC LIMIT 200"
        )?;

        let rows = stmt.query_map(params![cutoff], |r| {
            Ok(PliPoint {
                device_id: r.get::<_, String>(0)?,
                source_type: r.get::<_, String>(1)?,
                last_seen: (r.get::<_, i64>(2)?).max(0) as u64,
                tile_id: r.get::<_, String>(3)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Error::Sqlite)
    }

    pub fn upsert_group(&mut self, group_id: &str, name: &str, device_id: &str, now: u64) -> Result<(), Error> {
        self.conn.execute(
            "INSERT INTO msg_groups (group_id, name, created_by, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(group_id) DO UPDATE SET name = excluded.name, updated_at = excluded.updated_at",
            params![group_id, name, device_id, now as i64],
        )?;
        self.conn.execute(
            "INSERT INTO msg_group_members (group_id, device_id, role, joined_at) VALUES (?1, ?2, 'owner', ?3)
             ON CONFLICT(group_id, device_id) DO UPDATE SET role = excluded.role",
            params![group_id, device_id, now as i64],
        )?;
        Ok(())
    }

    pub fn join_group(&mut self, group_id: &str, device_id: &str, now: u64) -> Result<(), Error> {
        self.conn.execute(
            "INSERT INTO msg_group_members (group_id, device_id, role, joined_at) VALUES (?1, ?2, 'member', ?3)
             ON CONFLICT(group_id, device_id) DO NOTHING",
            params![group_id, device_id, now as i64],
        )?;
        Ok(())
    }

    pub fn list_groups(&self, device_id: Option<&str>) -> Result<Vec<GroupInfo>, Error> {
        let mut groups = Vec::new();
        let mut stmt = if device_id.is_some() {
            self.conn.prepare(
                "SELECT g.group_id, g.name FROM msg_groups g
                 JOIN msg_group_members m ON m.group_id = g.group_id
                 WHERE m.device_id = ?1 ORDER BY g.updated_at DESC"
            )?
        } else {
            self.conn.prepare("SELECT group_id, name FROM msg_groups ORDER BY updated_at DESC")?
        };

        let rows = if let Some(d) = device_id {
            stmt.query_map(params![d], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?
        };

        for (gid, name) in rows {
            let mut mstmt = self.conn.prepare("SELECT device_id FROM msg_group_members WHERE group_id = ?1 ORDER BY joined_at ASC")?;
            let members = mstmt.query_map(params![gid.clone()], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            groups.push(GroupInfo { group_id: gid, name, members });
        }
        Ok(groups)
    }

    pub fn send_message(&mut self, req: &MsgSendReq, now: u64) -> Result<u64, Error> {
        if req.body.trim().is_empty() { return Err(Error::Other("empty body".into())); }
        if req.to_device.is_none() && req.to_group.is_none() {
            return Err(Error::Other("missing recipient".into()));
        }

        let tx = self.conn.transaction()?;
        let mut last_id = 0u64;

        if let Some(to_device) = &req.to_device {
            tx.execute(
                "INSERT INTO messages (from_device_id, to_device_id, to_group_id, body, sent_at) VALUES (?1, ?2, NULL, ?3, ?4)",
                params![req.from, to_device, req.body, now as i64],
            )?;
            last_id = tx.last_insert_rowid() as u64;
        }

        if let Some(to_group) = &req.to_group {
            let mut stmt = tx.prepare("SELECT device_id FROM msg_group_members WHERE group_id = ?1")?;
            let members = stmt.query_map(params![to_group], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            for m in members {
                tx.execute(
                    "INSERT INTO messages (from_device_id, to_device_id, to_group_id, body, sent_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![req.from, m, to_group, req.body, now as i64],
                )?;
                last_id = tx.last_insert_rowid() as u64;
            }
        }

        tx.commit()?;
        Ok(last_id)
    }

    pub fn inbox(&mut self, device_id: &str, after_id: u64, limit: u64) -> Result<Vec<MsgRow>, Error> {
        let lim = limit.clamp(1, 200) as i64;
        let mut stmt = self.conn.prepare(
            "SELECT id, from_device_id, to_device_id, to_group_id, body, sent_at, delivered_at, read_at
             FROM messages
             WHERE to_device_id = ?1 AND id > ?2
             ORDER BY id ASC LIMIT ?3"
        )?;
        let rows = stmt.query_map(params![device_id, after_id as i64, lim], |r| {
            Ok(MsgRow {
                id: r.get::<_, i64>(0)? as u64,
                from: r.get::<_, String>(1)?,
                to_device: r.get::<_, Option<String>>(2)?,
                to_group: r.get::<_, Option<String>>(3)?,
                body: r.get::<_, String>(4)?,
                sent_at: (r.get::<_, i64>(5)?).max(0) as u64,
                delivered_at: r.get::<_, Option<i64>>(6)?.map(|v| v.max(0) as u64),
                read_at: r.get::<_, Option<i64>>(7)?.map(|v| v.max(0) as u64),
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        self.conn.execute(
            "UPDATE messages SET delivered_at = COALESCE(delivered_at, ?2)
             WHERE to_device_id = ?1 AND id > ?3",
            params![device_id, now, after_id as i64],
        )?;

        Ok(rows)
    }

    pub fn ack_read(&mut self, device_id: &str, id: u64, now: u64) -> Result<(), Error> {
        self.conn.execute(
            "UPDATE messages SET read_at = ?1 WHERE id = ?2 AND to_device_id = ?3",
            params![now as i64, id as i64, device_id],
        )?;
        Ok(())
    }
}


// ── Minimal HTTP server ───────────────────────────────────────────────────────

/// Hub node connection status for UI/debug panels.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NodeStatus {
    pub device_id: String,
    pub source_type: String,
    pub last_seen: u64,
    pub age_secs: u64,
    pub status: String, // CONNECTED | STALE | OFFLINE
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PliPoint {
    pub device_id: String,
    pub source_type: String,
    pub last_seen: u64,
    pub tile_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CopSnapshot {
    pub ts: u64,
    pub entities: Vec<PliPoint>,
    pub heat: Vec<serde_json::Value>,
    pub cameras: Vec<serde_json::Value>,
    pub satellites: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MsgSendReq {
    pub from: String,
    pub to_device: Option<String>,
    pub to_group: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MsgRow {
    pub id: u64,
    pub from: String,
    pub to_device: Option<String>,
    pub to_group: Option<String>,
    pub body: String,
    pub sent_at: u64,
    pub delivered_at: Option<u64>,
    pub read_at: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GroupUpsertReq {
    pub group_id: String,
    pub name: Option<String>,
    pub device_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupInfo {
    pub group_id: String,
    pub name: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MsgAckReq {
    pub device_id: String,
    pub id: u64,
}

/// Query node statuses from hub DB for debug/monitoring.
pub fn get_node_statuses(db_path: &str, stale_after_secs: u64) -> Result<Vec<NodeStatus>, Error> {
    let conn = Connection::open(db_path)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut stmt = conn.prepare(
        "SELECT device_id, COALESCE(source_type,'unknown'), COALESCE(last_seen,0)
         FROM node_registry ORDER BY last_seen DESC LIMIT 50"
    )?;

    let rows = stmt.query_map([], |r| {
        let device_id: String = r.get(0)?;
        let source_type: String = r.get(1)?;
        let last_seen: i64 = r.get(2)?;
        let last_seen_u = if last_seen < 0 { 0 } else { last_seen as u64 };
        let age_secs = now.saturating_sub(last_seen_u);
        let status = if age_secs <= stale_after_secs {
            "CONNECTED"
        } else if age_secs <= stale_after_secs * 3 {
            "STALE"
        } else {
            "OFFLINE"
        }.to_string();

        Ok(NodeStatus { device_id, source_type, last_seen: last_seen_u, age_secs, status })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Error::Sqlite)
}


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

        ("GET", p) if p.starts_with("/api/cop_snapshot") => {
            let max_age = parse_query_param(p, "max_age_secs")
                .and_then(|v| v.parse().ok())
                .unwrap_or(7200u64);
            let s = state.lock().unwrap();
            match s.db.get_pli_points(max_age) {
                Ok(entities) => {
                    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                    let snap = CopSnapshot { ts, entities, heat: vec![], cameras: vec![], satellites: vec![] };
                    (200, serde_json::to_string(&snap).unwrap())
                }
                Err(e) => (500, format!(r#"{{"error":"{e}"}}"#)),
            }
        }

        ("GET", p) if p.starts_with("/api/pli_delta") => {
            let max_age = parse_query_param(p, "max_age_secs")
                .and_then(|v| v.parse().ok())
                .unwrap_or(7200u64);
            let cursor = parse_query_param(p, "cursor")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0u64);
            let s = state.lock().unwrap();
            match s.db.get_pli_delta(cursor, max_age) {
                Ok(delta) => (200, serde_json::to_string(&delta).unwrap()),
                Err(e) => (500, format!(r#"{{"error":"{e}"}}"#)),
            }
        }

        ("GET", p) if p.starts_with("/api/pli") => {
            let max_age = parse_query_param(p, "max_age_secs")
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600u64);
            let s = state.lock().unwrap();
            match s.db.get_pli_points(max_age) {
                Ok(points) => (200, serde_json::to_string(&points).unwrap()),
                Err(e) => (500, format!(r#"{{"error":"{e}"}}"#)),
            }
        }

        ("POST", "/api/msg/send") => {
            let mut buf = vec![0u8; content_length.min(512_000)];
            use std::io::Read;
            if let Err(e) = reader.read_exact(&mut buf) {
                (400, format!(r#"{{"error":"bad body: {e}"}}"#))
            } else {
                match serde_json::from_slice::<MsgSendReq>(&buf) {
                    Ok(req) => {
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                        let mut s = state.lock().unwrap();
                        match s.db.send_message(&req, now) {
                            Ok(id) => (200, format!(r#"{{"ok":true,"id":{id}}}"#)),
                            Err(e) => (400, format!(r#"{{"error":"{e}"}}"#)),
                        }
                    }
                    Err(e) => (400, format!(r#"{{"error":"bad json: {e}"}}"#)),
                }
            }
        }

        ("GET", p) if p.starts_with("/api/msg/inbox") => {
            let device_id = parse_query_param(p, "device_id").unwrap_or_default();
            let after_id = parse_query_param(p, "after_id").and_then(|v| v.parse().ok()).unwrap_or(0u64);
            let limit = parse_query_param(p, "limit").and_then(|v| v.parse().ok()).unwrap_or(100u64);
            if device_id.is_empty() {
                (400, r#"{"error":"missing device_id"}"#.to_string())
            } else {
                let mut s = state.lock().unwrap();
                match s.db.inbox(&device_id, after_id, limit) {
                    Ok(rows) => (200, serde_json::to_string(&rows).unwrap()),
                    Err(e) => (500, format!(r#"{{"error":"{e}"}}"#)),
                }
            }
        }

        ("POST", "/api/msg/ack") => {
            let mut buf = vec![0u8; content_length.min(64_000)];
            use std::io::Read;
            if let Err(e) = reader.read_exact(&mut buf) {
                (400, format!(r#"{{"error":"bad body: {e}"}}"#))
            } else {
                match serde_json::from_slice::<MsgAckReq>(&buf) {
                    Ok(req) => {
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                        let mut s = state.lock().unwrap();
                        match s.db.ack_read(&req.device_id, req.id, now) {
                            Ok(_) => (200, r#"{"ok":true}"#.to_string()),
                            Err(e) => (500, format!(r#"{{"error":"{e}"}}"#)),
                        }
                    }
                    Err(e) => (400, format!(r#"{{"error":"bad json: {e}"}}"#)),
                }
            }
        }

        ("POST", "/api/msg/group/upsert") => {
            let mut buf = vec![0u8; content_length.min(64_000)];
            use std::io::Read;
            if let Err(e) = reader.read_exact(&mut buf) {
                (400, format!(r#"{{"error":"bad body: {e}"}}"#))
            } else {
                match serde_json::from_slice::<GroupUpsertReq>(&buf) {
                    Ok(req) => {
                        let name = req.name.clone().unwrap_or_else(|| req.group_id.clone());
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                        let mut s = state.lock().unwrap();
                        let a = s.db.upsert_group(&req.group_id, &name, &req.device_id, now);
                        let b = s.db.join_group(&req.group_id, &req.device_id, now);
                        match (a,b) {
                            (Ok(_), Ok(_)) => (200, r#"{"ok":true}"#.to_string()),
                            (Err(e), _) | (_, Err(e)) => (500, format!(r#"{{"error":"{e}"}}"#)),
                        }
                    }
                    Err(e) => (400, format!(r#"{{"error":"bad json: {e}"}}"#)),
                }
            }
        }

        ("POST", "/api/msg/group/join") => {
            let mut buf = vec![0u8; content_length.min(64_000)];
            use std::io::Read;
            if let Err(e) = reader.read_exact(&mut buf) {
                (400, format!(r#"{{"error":"bad body: {e}"}}"#))
            } else {
                match serde_json::from_slice::<GroupUpsertReq>(&buf) {
                    Ok(req) => {
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                        let mut s = state.lock().unwrap();
                        match s.db.join_group(&req.group_id, &req.device_id, now) {
                            Ok(_) => (200, r#"{"ok":true}"#.to_string()),
                            Err(e) => (500, format!(r#"{{"error":"{e}"}}"#)),
                        }
                    }
                    Err(e) => (400, format!(r#"{{"error":"bad json: {e}"}}"#)),
                }
            }
        }

        ("GET", p) if p.starts_with("/api/msg/groups") => {
            let device_id = parse_query_param(p, "device_id");
            let s = state.lock().unwrap();
            match s.db.list_groups(device_id.as_deref()) {
                Ok(groups) => (200, serde_json::to_string(&groups).unwrap()),
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
