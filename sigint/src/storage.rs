use rusqlite::{Connection, params};
use crate::error::Error;
use crate::wire::{RfAggregate, ChannelHotness};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS tile_aggregates (
    tile_id       TEXT    NOT NULL,
    time_bucket   INTEGER NOT NULL,
    sensor_type   TEXT    NOT NULL,
    dimension     TEXT    NOT NULL,
    mean_val      REAL    NOT NULL,
    max_val       REAL    NOT NULL,
    sample_count  INTEGER NOT NULL,
    confidence    REAL    NOT NULL,
    last_seen_utc INTEGER NOT NULL,
    synced        INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (tile_id, time_bucket, sensor_type, dimension)
);

CREATE TABLE IF NOT EXISTS sync_state (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS device_keys (
    device_id  TEXT PRIMARY KEY,
    public_key TEXT NOT NULL,
    secret_key TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
"#;

/// A row from tile_aggregates (used for sync batching).
#[derive(Debug, Clone)]
pub struct TileRow {
    pub tile_id: String,
    pub time_bucket: u64,
    pub sensor_type: String,
    pub dimension: String,
    pub mean_val: f64,
    pub max_val: f64,
    pub sample_count: u32,
    pub confidence: f64,
    pub last_seen_utc: u64,
}

/// Node-side SQLite database handle.
pub struct NodeDb {
    conn: Connection,
}

impl NodeDb {
    /// Open (or create) the node database at the given path.
    pub fn open(path: &str) -> Result<Self, Error> {
        let conn = Connection::open(path)?;
        // Enable WAL for concurrent reads
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self, Error> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    // ── RF aggregate upsert ───────────────────────────────────────────────────

    /// Upsert an RF aggregate into tile_aggregates.
    /// dimension = "rf:{freq_start_hz}-{freq_end_hz}"
    pub fn upsert_rf_aggregate(
        &mut self,
        tile_id: &str,
        time_bucket: u64,
        agg: &RfAggregate,
        last_seen_utc: u64,
    ) -> Result<(), Error> {
        let dimension = format!("rf:{}-{}", agg.freq_start_hz, agg.freq_end_hz);
        self.conn.execute(
            r#"INSERT INTO tile_aggregates
               (tile_id, time_bucket, sensor_type, dimension, mean_val, max_val,
                sample_count, confidence, last_seen_utc, synced)
               VALUES (?1, ?2, 'rf', ?3, ?4, ?5, ?6, ?7, ?8, 0)
               ON CONFLICT(tile_id, time_bucket, sensor_type, dimension) DO UPDATE SET
                 mean_val      = (mean_val * sample_count + excluded.mean_val * excluded.sample_count)
                                 / (sample_count + excluded.sample_count),
                 max_val       = MAX(max_val, excluded.max_val),
                 sample_count  = sample_count + excluded.sample_count,
                 confidence    = (confidence + excluded.confidence) / 2.0,
                 last_seen_utc = MAX(last_seen_utc, excluded.last_seen_utc),
                 synced        = 0"#,
            params![
                tile_id,
                time_bucket as i64,
                dimension,
                agg.mean_power_dbm,
                agg.max_power_dbm,
                agg.sample_count as i64,
                agg.confidence,
                last_seen_utc as i64,
            ],
        )?;
        Ok(())
    }

    // ── Wi-Fi channel hotness upsert ──────────────────────────────────────────

    /// Upsert a channel hotness entry.
    /// dimension = "wifi:{band}:{channel}"
    pub fn upsert_channel_hotness(
        &mut self,
        tile_id: &str,
        time_bucket: u64,
        ch: &ChannelHotness,
        last_seen_utc: u64,
    ) -> Result<(), Error> {
        let dimension = format!("wifi:{}:{}", ch.band, ch.channel);
        self.conn.execute(
            r#"INSERT INTO tile_aggregates
               (tile_id, time_bucket, sensor_type, dimension, mean_val, max_val,
                sample_count, confidence, last_seen_utc, synced)
               VALUES (?1, ?2, 'wifi_channel', ?3, ?4, ?5, ?6, ?7, ?8, 0)
               ON CONFLICT(tile_id, time_bucket, sensor_type, dimension) DO UPDATE SET
                 mean_val      = (mean_val * sample_count + excluded.mean_val * excluded.sample_count)
                                 / (sample_count + excluded.sample_count),
                 max_val       = MAX(max_val, excluded.max_val),
                 sample_count  = sample_count + excluded.sample_count,
                 confidence    = (confidence + excluded.confidence) / 2.0,
                 last_seen_utc = MAX(last_seen_utc, excluded.last_seen_utc),
                 synced        = 0"#,
            params![
                tile_id,
                time_bucket as i64,
                dimension,
                ch.mean_rssi_dbm,
                ch.max_rssi_dbm,
                ch.count as i64,
                ch.confidence,
                last_seen_utc as i64,
            ],
        )?;
        Ok(())
    }

    // ── Sync cursor ───────────────────────────────────────────────────────────

    pub fn get_sync_state(&self, key: &str) -> Result<Option<String>, Error> {
        let mut stmt = self.conn.prepare("SELECT value FROM sync_state WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        Ok(rows.next()?.map(|row| row.get::<_, String>(0).unwrap_or_default()))
    }

    pub fn set_sync_state(&mut self, key: &str, value: &str) -> Result<(), Error> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sync_state (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    // ── Pending sync ──────────────────────────────────────────────────────────

    /// Return all rows not yet synced to the hub.
    pub fn get_pending_sync(&self) -> Result<Vec<TileRow>, Error> {
        let mut stmt = self.conn.prepare(
            r#"SELECT tile_id, time_bucket, sensor_type, dimension,
                      mean_val, max_val, sample_count, confidence, last_seen_utc
               FROM tile_aggregates WHERE synced = 0
               ORDER BY time_bucket ASC, tile_id ASC
               LIMIT 500"#,
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(TileRow {
                tile_id: row.get(0)?,
                time_bucket: row.get::<_, i64>(1)? as u64,
                sensor_type: row.get(2)?,
                dimension: row.get(3)?,
                mean_val: row.get(4)?,
                max_val: row.get(5)?,
                sample_count: row.get::<_, i64>(6)? as u32,
                confidence: row.get(7)?,
                last_seen_utc: row.get::<_, i64>(8)? as u64,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Error::Sqlite)
    }

    /// Mark a batch of rows as synced.
    pub fn mark_synced(&mut self, rows: &[TileRow]) -> Result<(), Error> {
        let tx = self.conn.transaction()?;
        for row in rows {
            tx.execute(
                r#"UPDATE tile_aggregates SET synced = 1
                   WHERE tile_id = ?1 AND time_bucket = ?2
                     AND sensor_type = ?3 AND dimension = ?4"#,
                params![row.tile_id, row.time_bucket as i64, row.sensor_type, row.dimension],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{RfAggregate, ChannelHotness};

    fn make_db() -> NodeDb {
        NodeDb::open_in_memory().unwrap()
    }

    #[test]
    fn schema_creates_tables() {
        let db = make_db();
        // If we can run get_pending_sync without error, schema is good
        let rows = db.get_pending_sync().unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn upsert_rf_aggregate() {
        let mut db = make_db();
        let agg = RfAggregate {
            freq_start_hz: 2_400_000_000,
            freq_end_hz: 2_500_000_000,
            mean_power_dbm: -70.0,
            max_power_dbm: -60.0,
            sample_count: 5,
            confidence: 0.8,
        };
        db.upsert_rf_aggregate("tile_001", 1740000060, &agg, 1740000090).unwrap();
        let rows = db.get_pending_sync().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tile_id, "tile_001");
        assert_eq!(rows[0].sensor_type, "rf");
    }

    #[test]
    fn upsert_channel_hotness() {
        let mut db = make_db();
        let ch = ChannelHotness {
            band: "2.4".to_string(),
            channel: 6,
            count: 3,
            mean_rssi_dbm: -65.0,
            max_rssi_dbm: -55.0,
            confidence: 0.7,
        };
        db.upsert_channel_hotness("tile_002", 1740000060, &ch, 1740000090).unwrap();
        let rows = db.get_pending_sync().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sensor_type, "wifi_channel");
        assert!(rows[0].dimension.contains("6"));
    }

    #[test]
    fn mark_synced_clears_pending() {
        let mut db = make_db();
        let agg = RfAggregate {
            freq_start_hz: 900_000_000,
            freq_end_hz: 928_000_000,
            mean_power_dbm: -80.0,
            max_power_dbm: -75.0,
            sample_count: 2,
            confidence: 0.5,
        };
        db.upsert_rf_aggregate("tile_003", 1740000060, &agg, 1740000090).unwrap();
        let rows = db.get_pending_sync().unwrap();
        assert_eq!(rows.len(), 1);
        db.mark_synced(&rows).unwrap();
        let rows2 = db.get_pending_sync().unwrap();
        assert!(rows2.is_empty());
    }

    #[test]
    fn sync_state_roundtrip() {
        let mut db = make_db();
        db.set_sync_state("last_push_ts", "1740000000").unwrap();
        let val = db.get_sync_state("last_push_ts").unwrap();
        assert_eq!(val.as_deref(), Some("1740000000"));
    }

    #[test]
    fn rf_upsert_merges_sample_count() {
        let mut db = make_db();
        let agg1 = RfAggregate {
            freq_start_hz: 2_400_000_000,
            freq_end_hz: 2_500_000_000,
            mean_power_dbm: -70.0,
            max_power_dbm: -60.0,
            sample_count: 5,
            confidence: 0.8,
        };
        let agg2 = RfAggregate {
            freq_start_hz: 2_400_000_000,
            freq_end_hz: 2_500_000_000,
            mean_power_dbm: -68.0,
            max_power_dbm: -55.0,
            sample_count: 3,
            confidence: 0.9,
        };
        db.upsert_rf_aggregate("tile_001", 1740000060, &agg1, 1000).unwrap();
        db.upsert_rf_aggregate("tile_001", 1740000060, &agg2, 1001).unwrap();
        let rows = db.get_pending_sync().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sample_count, 8); // 5 + 3
        assert!((rows[0].max_val - (-55.0)).abs() < 1e-6);
    }
}
