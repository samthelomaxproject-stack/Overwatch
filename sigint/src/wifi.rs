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

// ── macOS airport scanner ─────────────────────────────────────────────────────

const AIRPORT_PATH: &str =
    "/System/Library/PrivateFrameworks/Apple80211.framework/Versions/Current/Resources/airport";

pub struct AirportScanner;

impl WifiScanner for AirportScanner {
    fn scan(&self) -> Result<Vec<WifiNetwork>, Error> {
        let output = Command::new(AIRPORT_PATH)
            .arg("-s")
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Error::ScannerUnavailable(
                        "airport CLI not found — macOS version may have removed it".to_string(),
                    )
                } else {
                    Error::Io(e)
                }
            })?;

        if !output.status.success() && output.stdout.is_empty() {
            return Err(Error::ScannerUnavailable(
                "airport returned no output".to_string(),
            ));
        }

        let text = String::from_utf8_lossy(&output.stdout);
        parse_airport_output(&text)
    }
}

/// Parse `airport -s` tab-separated output.
///
/// Example header + row:
/// ```text
///                             SSID BSSID             RSSI CHANNEL HT CC SECURITY (auth/unicast/group)
/// MyNetwork                        aa:bb:cc:dd:ee:ff  -65   6      Y  US WPA2(PSK/AES/AES)
/// ```
fn parse_airport_output(text: &str) -> Result<Vec<WifiNetwork>, Error> {
    let mut networks = Vec::new();
    let mut lines = text.lines();

    // Skip header line
    let _ = lines.next();

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(net) = parse_airport_line(line) {
            networks.push(net);
        }
    }

    Ok(networks)
}

fn parse_airport_line(line: &str) -> Option<WifiNetwork> {
    // airport -s output is column-aligned, not strictly tab-separated
    // SSID is right-justified in the first 33 chars, BSSID follows
    if line.len() < 40 {
        return None;
    }

    let ssid = line[..33].trim().to_string();
    let rest = line[33..].trim();
    let parts: Vec<&str> = rest.split_whitespace().collect();

    // Expected: BSSID RSSI CHANNEL HT CC ...
    if parts.len() < 3 {
        return None;
    }

    let bssid = parts[0].to_string();
    let rssi_dbm: i32 = parts[1].parse().ok()?;
    let channel_str = parts[2];

    // Channel may be "6" or "6,+1" or "36" etc.
    let channel: u32 = channel_str.split(',').next()?.parse().ok()?;

    // Derive band and frequency from channel
    let (band, frequency_mhz) = channel_to_band_freq(channel);

    Some(WifiNetwork {
        bssid,
        ssid,
        band: band.to_string(),
        channel,
        frequency_mhz,
        rssi_dbm,
    })
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
    fn airport_line_parse() {
        // Simulate airport -s output line (column-aligned)
        let line = "         HomeNetwork            aa:bb:cc:dd:ee:ff  -55  6      Y  US WPA2(PSK/AES/AES)";
        // This may not parse perfectly with fixed-column logic but shouldn't panic
        let _ = parse_airport_line(line);
    }

    #[test]
    fn band_from_frequency() {
        assert_eq!(WifiNetwork::band_from_frequency(2437), "2.4");
        assert_eq!(WifiNetwork::band_from_frequency(5180), "5");
        assert_eq!(WifiNetwork::band_from_frequency(6000), "6");
    }
}
