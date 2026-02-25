use serde::{Deserialize, Serialize};

/// Top-level batch pushed from node → hub
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileUpdate {
    pub schema_version: u32,
    pub device_id: String,
    /// entity | drone | handheld | hub_local | unknown
    pub source_type: String,
    pub timestamp_utc: u64,
    pub tiles: Vec<TileData>,
}

impl TileUpdate {
    pub fn new(device_id: impl Into<String>, source_type: impl Into<String>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            schema_version: 1,
            device_id: device_id.into(),
            source_type: source_type.into(),
            timestamp_utc: now,
            tiles: Vec::new(),
        }
    }
}

/// Per-tile aggregate data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileData {
    /// H3 res-10 cell id (hex string)
    pub tile_id: String,
    /// floor(timestamp_utc / 60) * 60
    pub time_bucket: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rf: Option<Vec<RfAggregate>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wifi: Option<WifiData>,
}

impl TileData {
    pub fn new(tile_id: impl Into<String>, timestamp_utc: u64) -> Self {
        Self {
            tile_id: tile_id.into(),
            time_bucket: (timestamp_utc / 60) * 60,
            rf: None,
            wifi: None,
        }
    }
}

/// RF aggregate for one frequency band in one tile/time bucket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RfAggregate {
    pub freq_start_hz: u64,
    pub freq_end_hz: u64,
    pub mean_power_dbm: f64,
    pub max_power_dbm: f64,
    pub sample_count: u32,
    /// 0.0 – 1.0
    pub confidence: f64,
}

/// Wi-Fi data for one tile/time bucket (Mode A: channel hotness only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WifiData {
    pub channel_hotness: Vec<ChannelHotness>,
}

/// Aggregated channel density — no BSSID/SSID in Mode A
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelHotness {
    /// "2.4" | "5" | "6"
    pub band: String,
    pub channel: u32,
    pub count: u32,
    pub mean_rssi_dbm: f64,
    pub max_rssi_dbm: f64,
    /// 0.0 – 1.0
    pub confidence: f64,
}
