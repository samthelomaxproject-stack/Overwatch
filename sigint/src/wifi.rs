use std::collections::HashMap;
use std::process::Command;
use crate::error::Error;
use crate::wire::ChannelHotness;

/// Raw Wi-Fi network as seen by the OS scanner.
#[derive(Debug, Clone)]
pub struct WifiNetwork {
    pub bssid: String,
    pub ssid: String,
    /// "2.4" | "5" | "6"
    pub band: String,
    pub channel: u32,
    pub frequency_mhz: u32,
    pub rssi_dbm: i32,
}

impl WifiNetwork {
    pub fn band_from_frequency(freq_mhz: u32) -> &'static str {
        if freq_mhz >= 5925 {
            "6"
        } else if freq_mhz >= 5000 {
            "5"
        } else {
            "2.4"
        }
    }
}

/// Privacy Mode A: strip BSSID and SSID, keep only channel + RSSI.
#[derive(Debug, Clone)]
pub struct ChannelObservation {
    pub band: String,
    pub channel: u32,
    pub rssi_dbm: i32,
}

/// Apply Privacy Mode A — default and required.
/// Strips all identifying information (BSSID, SSID).
pub fn apply_privacy_mode_a(networks: Vec<WifiNetwork>) -> Vec<ChannelObservation> {
    networks
        .into_iter()
        .map(|n| ChannelObservation {
            band: n.band,
            channel: n.channel,
            rssi_dbm: n.rssi_dbm,
        })
        .collect()
}

// ── Scanner trait ─────────────────────────────────────────────────────────────

pub trait WifiScanner: Send + Sync {
    fn scan(&self) -> Result<Vec<WifiNetwork>, Error>;
}

// ── macOS CoreWLAN scanner (via scan_wifi.swift) ─────────────────────────────
//
// The `airport` CLI was removed in macOS 15. We use a bundled Swift script
// that calls CoreWLAN directly.
// Output format per line: SSID|BSSID|RSSI|CHANNEL|BAND

pub struct AirportScanner;

const SCAN_WIFI_PATHS: &[&str] = &[
    "/Applications/Overwatch.app/Contents/Resources/scan_wifi.swift",
    "/Users/thelomaxproject/Overwatch/scan_wifi.swift", // dev fallback
];

fn find_scan_wifi() -> Option<&'static str> {
    SCAN_WIFI_PATHS.iter().copied().find(|p| std::path::Path::new(p).exists())
}

impl WifiScanner for AirportScanner {
    fn scan(&self) -> Result<Vec<WifiNetwork>, Error> {
        let script = find_scan_wifi()
            .ok_or_else(|| Error::ScannerUnavailable(
                "scan_wifi.swift not found".to_string()
            ))?;

        let output = Command::new("swift")
            .arg(script)
            .output()
            .map_err(Error::Io)?;

        if output.stdout.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::ScannerUnavailable(
                format!("scan_wifi: {stderr}")
            ));
        }

        let text = String::from_utf8_lossy(&output.stdout);
        parse_corewlan_output(&text)
    }
}

/// Parse pipe-delimited output from scan_wifi.swift:
/// `SSID|BSSID|RSSI|CHANNEL|BAND`
fn parse_corewlan_output(text: &str) -> Result<Vec<WifiNetwork>, Error> {
    let mut networks = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("ERROR") { continue; }
        let parts: Vec<&str> = line.splitn(5, '|').collect();
        if parts.len() < 5 { continue; }
        let ssid = parts[0].to_string();
        let bssid = parts[1].to_string();
        let rssi_dbm: i32 = parts[2].parse().unwrap_or(-100);
        let channel: u32 = parts[3].parse().unwrap_or(0);
        if channel == 0 { continue; }
        let band = parts[4].to_string();
        let (_, frequency_mhz) = channel_to_band_freq(channel);
        networks.push(WifiNetwork { ssid, bssid, band, channel, frequency_mhz, rssi_dbm });
    }
    Ok(networks)
}

fn channel_to_band_freq(channel: u32) -> (&'static str, u32) {
    if channel <= 13 {
        let freq = 2407 + channel * 5;
        ("2.4", freq)
    } else if channel == 14 {
        ("2.4", 2484)
    } else if (36..=177).contains(&channel) {
        let freq = 5000 + channel * 5;
        ("5", freq)
    } else {
        // 6 GHz channels (Wi-Fi 6E)
        let freq = 5950 + channel * 5;
        ("6", freq)
    }
}

/// Linux iw/nmcli stub — to be implemented for Linux hub deployment.
pub struct LinuxWifiScanner;

impl WifiScanner for LinuxWifiScanner {
    fn scan(&self) -> Result<Vec<WifiNetwork>, Error> {
        // Phase 2: spawn 'iw dev <iface> scan' or 'nmcli dev wifi list'
        Err(Error::ScannerUnavailable(
            "Linux Wi-Fi scanner not yet implemented (Phase 2)".to_string(),
        ))
    }
}

// ── Wi-Fi tile bucket ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WifiBucketKey {
    tile_id: String,
    time_bucket: u64,
    band: String,
    channel: u32,
}

#[derive(Debug)]
struct WifiBucketEntry {
    count: u32,
    rssi_sum: f64,
    max_rssi_dbm: f64,
    confidence_sum: f64,
}

impl WifiBucketEntry {
    fn new(rssi: i32, confidence: f64) -> Self {
        Self {
            count: 1,
            rssi_sum: rssi as f64,
            max_rssi_dbm: rssi as f64,
            confidence_sum: confidence,
        }
    }

    fn update(&mut self, rssi: i32, confidence: f64) {
        self.count += 1;
        self.rssi_sum += rssi as f64;
        self.max_rssi_dbm = self.max_rssi_dbm.max(rssi as f64);
        self.confidence_sum += confidence;
    }

    fn mean_rssi(&self) -> f64 {
        self.rssi_sum / self.count as f64
    }

    fn mean_confidence(&self) -> f64 {
        self.confidence_sum / self.count as f64
    }
}

/// In-memory Wi-Fi tile bucket store.
pub struct WifiTileBucket {
    buckets: HashMap<WifiBucketKey, WifiBucketEntry>,
}

impl WifiTileBucket {
    pub fn new() -> Self {
        Self {
            buckets: HashMap::new(),
        }
    }

    /// Upsert a channel observation.
    pub fn upsert(
        &mut self,
        tile_id: &str,
        time_bucket: u64,
        obs: &ChannelObservation,
        confidence: f64,
    ) {
        let key = WifiBucketKey {
            tile_id: tile_id.to_string(),
            time_bucket,
            band: obs.band.clone(),
            channel: obs.channel,
        };

        match self.buckets.get_mut(&key) {
            Some(entry) => entry.update(obs.rssi_dbm, confidence),
            None => {
                self.buckets
                    .insert(key, WifiBucketEntry::new(obs.rssi_dbm, confidence));
            }
        }
    }

    /// Drain buckets for a given tile + time_bucket into ChannelHotness wire structs.
    pub fn drain_tile(&mut self, tile_id: &str, time_bucket: u64) -> Vec<ChannelHotness> {
        let keys: Vec<_> = self
            .buckets
            .keys()
            .filter(|k| k.tile_id == tile_id && k.time_bucket == time_bucket)
            .cloned()
            .collect();

        keys.into_iter()
            .filter_map(|k| {
                self.buckets.remove(&k).map(|e| ChannelHotness {
                    band: k.band,
                    channel: k.channel,
                    count: e.count,
                    mean_rssi_dbm: e.mean_rssi(),
                    max_rssi_dbm: e.max_rssi_dbm,
                    confidence: e.mean_confidence(),
                })
            })
            .collect()
    }
}

impl Default for WifiTileBucket {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_mode_a_strips_identifiers() {
        let nets = vec![WifiNetwork {
            bssid: "aa:bb:cc:dd:ee:ff".to_string(),
            ssid: "MyNetwork".to_string(),
            band: "2.4".to_string(),
            channel: 6,
            frequency_mhz: 2437,
            rssi_dbm: -65,
        }];
        let obs = apply_privacy_mode_a(nets);
        assert_eq!(obs.len(), 1);
        assert_eq!(obs[0].channel, 6);
        assert_eq!(obs[0].rssi_dbm, -65);
        // No BSSID or SSID on ChannelObservation
    }

    #[test]
    fn channel_to_band_2_4() {
        let (band, freq) = channel_to_band_freq(6);
        assert_eq!(band, "2.4");
        assert_eq!(freq, 2437);
    }

    #[test]
    fn channel_to_band_5() {
        let (band, freq) = channel_to_band_freq(36);
        assert_eq!(band, "5");
        assert_eq!(freq, 5180);
    }

    #[test]
    fn wifi_tile_bucket_upsert_and_drain() {
        let mut bucket = WifiTileBucket::new();
        let obs1 = ChannelObservation { band: "2.4".to_string(), channel: 6, rssi_dbm: -70 };
        let obs2 = ChannelObservation { band: "2.4".to_string(), channel: 6, rssi_dbm: -60 };
        bucket.upsert("tile_a", 60, &obs1, 0.8);
        bucket.upsert("tile_a", 60, &obs2, 0.8);

        let hotness = bucket.drain_tile("tile_a", 60);
        assert_eq!(hotness.len(), 1);
        assert_eq!(hotness[0].count, 2);
        assert!((hotness[0].mean_rssi_dbm - (-65.0)).abs() < 1e-6);
        assert!((hotness[0].max_rssi_dbm - (-60.0)).abs() < 1e-6);
    }

    #[test]
    fn corewlan_output_parse() {
        // Simulate scan_wifi.swift pipe-delimited output
        let text = "HomeNetwork|aa:bb:cc:dd:ee:ff|-55|6|2.4\nOtherNet||−72|36|5\n";
        let nets = parse_corewlan_output(text).unwrap();
        assert!(!nets.is_empty());
        assert_eq!(nets[0].ssid, "HomeNetwork");
        assert_eq!(nets[0].rssi_dbm, -55);
        assert_eq!(nets[0].channel, 6);
        assert_eq!(nets[0].band, "2.4");
    }

    #[test]
    fn corewlan_skips_bad_lines() {
        let text = "SSID|BSSID|-65|6|2.4\n\nERROR: something\nbad line\n";
        let nets = parse_corewlan_output(text).unwrap();
        assert_eq!(nets.len(), 1);
    }

    #[test]
    fn band_from_frequency() {
        assert_eq!(WifiNetwork::band_from_frequency(2437), "2.4");
        assert_eq!(WifiNetwork::band_from_frequency(5180), "5");
        assert_eq!(WifiNetwork::band_from_frequency(6000), "6");
    }
}
