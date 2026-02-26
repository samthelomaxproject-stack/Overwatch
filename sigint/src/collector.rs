/// Node collector — runs the full collection pipeline in a background thread.
///
/// Responsibilities:
/// 1. Poll GPS provider for current fix
/// 2. Drain RF ring buffer every 5s → aggregate → upsert into NodeDb
/// 3. Run Wi-Fi scan every 30s → apply Mode A → upsert into NodeDb
/// 4. Every 30s → build TileUpdate batch from pending DB rows → push to hub
/// 5. Every 30s → pull delta from hub → merge into local DB
///
/// `HUB_COLLECTOR_ENABLED=false` → collector still runs on vehicles/drones;
/// hub simply runs hub-api only with no local collector.
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::{
    Error,
    confidence::{self, ConfidenceConfig},
    crypto::{DeviceKeys, sign_payload},
    gps::GpsProvider,
    rf::{RingBuffer, RfObservation, flush_to_aggregates},
    wifi::{WifiScanner, apply_privacy, PrivacyMode},
    storage::{NodeDb, TileRow},
    sync::{SyncTransport, SyncCursor},
    wire::{TileUpdate, TileData, WifiData},
};

/// Configuration for the node collector.
#[derive(Debug, Clone)]
pub struct CollectorConfig {
    /// H3 resolution for cell lookup (default 10)
    pub h3_resolution: u8,
    /// RF ring buffer capacity
    pub rf_buffer_capacity: usize,
    /// RF flush interval (seconds)
    pub rf_flush_interval_secs: u64,
    /// Wi-Fi scan interval (seconds)
    pub wifi_scan_interval_secs: u64,
    /// Sync push/pull interval (seconds)
    pub sync_interval_secs: u64,
    /// Confidence scoring config
    pub confidence: ConfidenceConfig,
    /// Node keypair — device_id is derived from the public key
    pub keys: DeviceKeys,
    /// Source type for TileUpdates
    pub source_type: String,
    /// Wi-Fi privacy mode (default A)
    pub privacy_mode: PrivacyMode,
}

impl Default for CollectorConfig {
    fn default() -> Self {
        let keys = DeviceKeys::generate();
        Self {
            h3_resolution: 10,
            rf_buffer_capacity: 1000,
            rf_flush_interval_secs: 5,
            wifi_scan_interval_secs: 30,
            sync_interval_secs: 30,
            confidence: ConfidenceConfig::default(),
            keys,
            source_type: "entity".to_string(),
            privacy_mode: PrivacyMode::A,
        }
    }
}

/// Running collector state, shared across the pipeline.
pub struct Collector {
    config: CollectorConfig,
    gps: Box<dyn GpsProvider>,
    rf_buffer: RingBuffer<RfObservation>,
    wifi_scanner: Box<dyn WifiScanner>,
    db: NodeDb,
    transport: Box<dyn SyncTransport>,
}

impl Collector {
    pub fn new(
        config: CollectorConfig,
        gps: Box<dyn GpsProvider>,
        wifi_scanner: Box<dyn WifiScanner>,
        db: NodeDb,
        transport: Box<dyn SyncTransport>,
    ) -> Self {
        let cap = config.rf_buffer_capacity;
        Self { config, gps, rf_buffer: RingBuffer::new(cap), wifi_scanner, db, transport }
    }

    /// Push a raw RF observation into the ring buffer.
    /// Called from the hackrf_sweep reader thread.
    pub fn push_rf(&mut self, obs: RfObservation) {
        self.rf_buffer.push(obs);
    }

    /// Flush the RF buffer, aggregate, and upsert into NodeDb.
    /// Call every `rf_flush_interval_secs`.
    pub fn flush_rf(&mut self) {
        let fix = match self.gps.current_fix() {
            Some(f) => f,
            None => {
                log::debug!("RF flush: no GPS fix");
                return;
            }
        };

        let observations = self.rf_buffer.drain_all();
        if observations.is_empty() { return; }

        let tile_id = fix.tile_id(self.config.h3_resolution);
        let time_bucket = fix.time_bucket();

        let conf = confidence::compute(
            fix.accuracy_m,
            observations.len() as u32,
            self.config.rf_flush_interval_secs as f64,
            fix.speed_mps.unwrap_or(0.0),
            &self.config.confidence,
        );

        let aggregates = flush_to_aggregates(&observations, conf);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        for agg in &aggregates {
            if let Err(e) = self.db.upsert_rf_aggregate(&tile_id, time_bucket, agg, now) {
                log::error!("Failed to upsert RF aggregate: {e}");
            }
        }

        log::debug!("RF flush: {tile_id} → {} aggregates (conf={conf:.2})", aggregates.len());
    }

    /// Run a Wi-Fi scan and upsert Mode-A channel observations into NodeDb.
    /// Call every `wifi_scan_interval_secs`.
    pub fn scan_wifi(&mut self) {
        let fix = match self.gps.current_fix() {
            Some(f) => f,
            None => {
                log::debug!("Wi-Fi scan: no GPS fix");
                return;
            }
        };

        let networks = match self.wifi_scanner.scan() {
            Ok(n) => n,
            Err(e) => {
                log::warn!("Wi-Fi scan failed: {e}");
                return;
            }
        };

        let tile_id = fix.tile_id(self.config.h3_resolution);
        let time_bucket = fix.time_bucket();

        // Read shared mode every cycle so UI changes take effect immediately
        let mode = crate::wifi::get_shared_privacy_mode();
        let observations = apply_privacy(networks, mode);

        let conf = confidence::compute(
            fix.accuracy_m,
            observations.len() as u32,
            self.config.wifi_scan_interval_secs as f64,
            fix.speed_mps.unwrap_or(0.0),
            &self.config.confidence,
        );

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        for obs in &observations {
            let ch = crate::wire::ChannelHotness {
                band: obs.band.clone(),
                channel: obs.channel,
                count: 1,
                mean_rssi_dbm: obs.rssi_dbm as f64,
                max_rssi_dbm: obs.rssi_dbm as f64,
                confidence: conf,
            };
            if let Err(e) = self.db.upsert_channel_hotness(&tile_id, time_bucket, &ch, now) {
                log::error!("Failed to upsert channel hotness: {e}");
            }
        }

        log::debug!("Wi-Fi scan: {tile_id} → {} channels (conf={conf:.2})", observations.len());
    }

    /// Build a TileUpdate batch from pending DB rows and push to hub.
    pub fn sync_push(&mut self) -> Result<(), Error> {
        let pending = self.db.get_pending_sync()?;
        if pending.is_empty() { return Ok(()); }

        let mut batch = self.build_tile_update(&pending);

        // Sign the batch (signature covers all fields except signature itself)
        match sign_payload(&batch, &self.config.keys) {
            Ok(sig) => { batch.signature = Some(sig); }
            Err(e) => log::warn!("Failed to sign batch: {e} — sending unsigned"),
        }

        let ack = self.transport.push(&batch)?;
        log::info!("Sync push: {} accepted, {} rejected (signed={})",
            ack.accepted, ack.rejected, batch.signature.is_some());

        self.db.mark_synced(&pending)?;
        Ok(())
    }

    /// Pull delta from hub and store the cursor.
    pub fn sync_pull(&mut self) -> Result<(), Error> {
        let cursor_str = self.db.get_sync_state("last_pull_cursor")?
            .unwrap_or_else(|| "0".to_string());
        let cursor_ts: u64 = cursor_str.parse().unwrap_or(0);

        let delta = self.transport.pull(&SyncCursor { timestamp: cursor_ts })?;

        if delta.cursor > cursor_ts {
            self.db.set_sync_state("last_pull_cursor", &delta.cursor.to_string())?;
            log::debug!("Sync pull: cursor advanced to {}", delta.cursor);
        }

        Ok(())
    }

    fn build_tile_update(&self, rows: &[TileRow]) -> TileUpdate {
        use std::collections::HashMap;
        use crate::wire::{RfAggregate, ChannelHotness, WifiData};

        let mut tile_map: HashMap<(String, u64), TileData> = HashMap::new();

        for row in rows {
            let key = (row.tile_id.clone(), row.time_bucket);
            let tile = tile_map.entry(key.clone()).or_insert_with(|| {
                TileData::new(row.tile_id.clone(), row.time_bucket)
            });

            match row.sensor_type.as_str() {
                "rf" => {
                    // dimension = "rf:{start}-{end}"
                    let rest = row.dimension.trim_start_matches("rf:");
                    let mut parts = rest.splitn(2, '-');
                    if let (Some(s), Some(e)) = (parts.next(), parts.next()) {
                        if let (Ok(start), Ok(end)) = (s.parse::<u64>(), e.parse::<u64>()) {
                            tile.rf.get_or_insert_with(Vec::new).push(RfAggregate {
                                freq_start_hz: start, freq_end_hz: end,
                                mean_power_dbm: row.mean_val, max_power_dbm: row.max_val,
                                sample_count: row.sample_count, confidence: row.confidence,
                            });
                        }
                    }
                }
                "wifi_channel" => {
                    // dimension = "wifi:{band}:{channel}"
                    let rest = row.dimension.trim_start_matches("wifi:");
                    let mut parts = rest.splitn(2, ':');
                    if let (Some(band), Some(ch_str)) = (parts.next(), parts.next()) {
                        if let Ok(channel) = ch_str.parse() {
                            tile.wifi.get_or_insert(WifiData { channel_hotness: vec![] })
                                .channel_hotness.push(ChannelHotness {
                                    band: band.to_string(), channel,
                                    count: row.sample_count,
                                    mean_rssi_dbm: row.mean_val, max_rssi_dbm: row.max_val,
                                    confidence: row.confidence,
                                });
                        }
                    }
                }
                _ => {}
            }
        }

        let mut update = TileUpdate::new(&self.config.keys.device_id, &self.config.source_type);
        update.tiles = tile_map.into_values().collect();
        update
    }

    /// Run the collector loop (blocking). Intended to be called from a thread.
    pub fn run(mut self) {
        let mut last_rf_flush = Instant::now();
        let mut last_wifi_scan = Instant::now();
        let mut last_sync = Instant::now();

        let rf_interval = Duration::from_secs(self.config.rf_flush_interval_secs);
        let wifi_interval = Duration::from_secs(self.config.wifi_scan_interval_secs);
        let sync_interval = Duration::from_secs(self.config.sync_interval_secs);

        log::info!("Collector started (device_id={})", self.config.keys.device_id);

        loop {
            std::thread::sleep(Duration::from_millis(500));

            if last_rf_flush.elapsed() >= rf_interval {
                self.flush_rf();
                last_rf_flush = Instant::now();
            }

            if last_wifi_scan.elapsed() >= wifi_interval {
                self.scan_wifi();
                last_wifi_scan = Instant::now();
            }

            if last_sync.elapsed() >= sync_interval {
                if let Err(e) = self.sync_push() {
                    log::error!("Sync push failed: {e}");
                }
                if let Err(e) = self.sync_pull() {
                    log::error!("Sync pull failed: {e}");
                }
                last_sync = Instant::now();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        gps::StubGpsProvider,
        wifi::WifiNetwork,
        sync::NullSyncTransport,
        storage::NodeDb,
    };

    struct StubWifiScanner(Vec<WifiNetwork>);
    impl WifiScanner for StubWifiScanner {
        fn scan(&self) -> Result<Vec<WifiNetwork>, Error> { Ok(self.0.clone()) }
    }

    fn make_collector(wifi: Vec<WifiNetwork>) -> Collector {
        let gps = Box::new(StubGpsProvider::with_fix(33.18, -96.88, 5.0));
        let scanner = Box::new(StubWifiScanner(wifi));
        let db = NodeDb::open_in_memory().unwrap();
        let transport = Box::new(NullSyncTransport);
        Collector::new(CollectorConfig::default(), gps, scanner, db, transport)
    }

    #[test]
    fn flush_rf_with_observations() {
        let mut c = make_collector(vec![]);
        c.push_rf(crate::rf::RfObservation {
            timestamp_utc: 1740000000,
            freq_start_hz: 2_400_000_000,
            freq_end_hz: 2_500_000_000,
            bin_hz: 1_000_000,
            bins: vec![-70.0, -72.0],
        });
        c.flush_rf();
        let rows = c.db.get_pending_sync().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sensor_type, "rf");
    }

    #[test]
    fn scan_wifi_stores_channel_hotness() {
        let networks = vec![WifiNetwork {
            bssid: "aa:bb:cc:dd:ee:ff".to_string(),
            ssid: "TestNet".to_string(),
            band: "2.4".to_string(),
            channel: 6,
            frequency_mhz: 2437,
            rssi_dbm: -65,
        }];
        let mut c = make_collector(networks);
        c.scan_wifi();
        let rows = c.db.get_pending_sync().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sensor_type, "wifi_channel");
    }

    #[test]
    fn build_tile_update_roundtrip() {
        let mut c = make_collector(vec![]);
        c.push_rf(crate::rf::RfObservation {
            timestamp_utc: 1740000000,
            freq_start_hz: 900_000_000,
            freq_end_hz: 928_000_000,
            bin_hz: 1_000_000,
            bins: vec![-80.0],
        });
        c.flush_rf();
        let rows = c.db.get_pending_sync().unwrap();
        assert!(!rows.is_empty());
        let update = c.build_tile_update(&rows);
        assert!(!update.tiles.is_empty());
        let tile = &update.tiles[0];
        assert!(tile.rf.is_some());
    }

    #[test]
    fn sync_push_marks_rows_synced() {
        let mut c = make_collector(vec![]);
        c.push_rf(crate::rf::RfObservation {
            timestamp_utc: 1740000000,
            freq_start_hz: 433_000_000,
            freq_end_hz: 435_000_000,
            bin_hz: 100_000,
            bins: vec![-90.0],
        });
        c.flush_rf();
        assert!(!c.db.get_pending_sync().unwrap().is_empty());
        c.sync_push().unwrap();
        assert!(c.db.get_pending_sync().unwrap().is_empty());
    }
}
