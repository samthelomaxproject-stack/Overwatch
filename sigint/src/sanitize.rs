/// Anti-poisoning: input validation, clamping, and rate limiting.
///
/// All values entering the aggregation pipeline pass through here.
/// Rejects implausible data and clamps to documented safe ranges.
/// Per-node rate limiting prevents a single source flooding a tile.
///
/// Design goals:
/// - Documented constants, all configurable
/// - Pure functions — no I/O, easy to unit test
/// - Conservative defaults: reject unclear inputs rather than accept them
use std::collections::HashMap;
use crate::wire::{RfAggregate, ChannelHotness};

// ── RF Sanity Bounds ──────────────────────────────────────────────────────────

/// RF power floors and ceilings (dBm).
/// Real-world SDR range: −120 (noise floor) to −10 (near transmitter).
/// We clamp to a slightly narrower range to exclude obvious instrument errors.
pub const RF_MIN_POWER_DBM: f64 = -120.0;
pub const RF_MAX_POWER_DBM: f64 = 0.0;

/// Minimum plausible frequency (Hz): 10 MHz
pub const RF_MIN_FREQ_HZ: u64 = 10_000_000;
/// Maximum plausible frequency (Hz): 6 GHz (HackRF max)
pub const RF_MAX_FREQ_HZ: u64 = 6_000_000_000;

// ── Wi-Fi RSSI Bounds ─────────────────────────────────────────────────────────

/// Wi-Fi RSSI floor (dBm): −100 is the ITU noise floor for 2.4 GHz.
pub const WIFI_MIN_RSSI_DBM: i32 = -100;
/// Wi-Fi RSSI ceiling (dBm): −10 is ~1m from AP. Anything higher = instrument error.
pub const WIFI_MAX_RSSI_DBM: i32 = -10;

// ── GPS Sanity ────────────────────────────────────────────────────────────────

/// Maximum accepted GPS horizontal accuracy (metres). Fixes worse than this
/// produce 0 GPS confidence in the confidence formula, but we still reject
/// coordinates that are clearly corrupt.
pub const GPS_MAX_ACCURACY_M: f64 = 500.0;

/// Maximum plausible speed (m/s): ~300 m/s ≈ 1080 km/h (fast fixed-wing).
/// Anything above this is almost certainly a GPS glitch.
pub const GPS_MAX_SPEED_MPS: f64 = 300.0;

// ── Per-node Rate Limiting ────────────────────────────────────────────────────

/// Maximum RF observations a single node may contribute to one
/// (tile_id, time_bucket, freq_band) within a 60-second bucket.
pub const RATE_LIMIT_RF_PER_TILE: u32 = 200;

/// Maximum Wi-Fi channel observations per (tile_id, time_bucket, channel).
pub const RATE_LIMIT_WIFI_PER_TILE: u32 = 20;

// ── Validation functions ──────────────────────────────────────────────────────

/// Validate and clamp an RF aggregate. Returns `None` if the aggregate is
/// fundamentally implausible and should be rejected outright.
pub fn sanitize_rf(agg: &RfAggregate) -> Option<RfAggregate> {
    // Reject bad frequency ranges
    if agg.freq_start_hz < RF_MIN_FREQ_HZ || agg.freq_end_hz > RF_MAX_FREQ_HZ {
        log::warn!("Rejected RF agg: freq out of range ({}-{})", agg.freq_start_hz, agg.freq_end_hz);
        return None;
    }
    if agg.freq_start_hz >= agg.freq_end_hz {
        log::warn!("Rejected RF agg: freq_start >= freq_end");
        return None;
    }
    // Reject zero samples
    if agg.sample_count == 0 {
        return None;
    }
    // Reject if mean > max (instrument error)
    if agg.mean_power_dbm > agg.max_power_dbm + 0.1 {
        log::warn!("Rejected RF agg: mean > max ({} > {})", agg.mean_power_dbm, agg.max_power_dbm);
        return None;
    }

    Some(RfAggregate {
        freq_start_hz: agg.freq_start_hz,
        freq_end_hz: agg.freq_end_hz,
        mean_power_dbm: agg.mean_power_dbm.clamp(RF_MIN_POWER_DBM, RF_MAX_POWER_DBM),
        max_power_dbm: agg.max_power_dbm.clamp(RF_MIN_POWER_DBM, RF_MAX_POWER_DBM),
        sample_count: agg.sample_count,
        confidence: agg.confidence.clamp(0.0, 1.0),
    })
}

/// Validate and clamp a channel hotness entry.
pub fn sanitize_wifi(ch: &ChannelHotness) -> Option<ChannelHotness> {
    if ch.count == 0 { return None; }
    if ch.channel == 0 { return None; }
    if !["2.4", "5", "6"].contains(&ch.band.as_str()) {
        log::warn!("Rejected Wi-Fi: unknown band '{}'", ch.band);
        return None;
    }
    // Reject if mean > max
    if ch.mean_rssi_dbm > ch.max_rssi_dbm + 0.1 {
        log::warn!("Rejected Wi-Fi: mean_rssi > max_rssi");
        return None;
    }

    Some(ChannelHotness {
        band: ch.band.clone(),
        channel: ch.channel,
        count: ch.count,
        mean_rssi_dbm: ch.mean_rssi_dbm.clamp(WIFI_MIN_RSSI_DBM as f64, WIFI_MAX_RSSI_DBM as f64),
        max_rssi_dbm: ch.max_rssi_dbm.clamp(WIFI_MIN_RSSI_DBM as f64, WIFI_MAX_RSSI_DBM as f64),
        confidence: ch.confidence.clamp(0.0, 1.0),
    })
}

/// Validate a GPS fix before using it for tile lookup.
pub fn validate_gps(lat: f64, lon: f64, accuracy_m: f64, speed_mps: Option<f64>) -> bool {
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        log::warn!("Invalid GPS coords: {lat},{lon}");
        return false;
    }
    if accuracy_m > GPS_MAX_ACCURACY_M {
        log::debug!("GPS accuracy too poor: {accuracy_m}m > {GPS_MAX_ACCURACY_M}m");
        return false;
    }
    if let Some(speed) = speed_mps {
        if speed > GPS_MAX_SPEED_MPS {
            log::warn!("GPS speed implausible: {speed} m/s");
            return false;
        }
    }
    true
}

// ── Per-node Rate Limiter ─────────────────────────────────────────────────────

/// In-memory per-node contribution counter for a single time bucket.
/// Keyed by (device_id, tile_id, time_bucket, dimension).
/// Reset when the time_bucket rolls over.
#[derive(Debug, Default)]
pub struct RateLimiter {
    rf_counts: HashMap<String, u32>,
    wifi_counts: HashMap<String, u32>,
    current_bucket: u64,
}

impl RateLimiter {
    pub fn new() -> Self { Self::default() }

    fn maybe_reset(&mut self, time_bucket: u64) {
        if time_bucket != self.current_bucket {
            self.rf_counts.clear();
            self.wifi_counts.clear();
            self.current_bucket = time_bucket;
        }
    }

    /// Returns true if the RF contribution is within rate limit.
    pub fn allow_rf(&mut self, device_id: &str, tile_id: &str, time_bucket: u64, freq_dim: &str) -> bool {
        self.maybe_reset(time_bucket);
        let key = format!("{device_id}:{tile_id}:{freq_dim}");
        let count = self.rf_counts.entry(key).or_insert(0);
        if *count >= RATE_LIMIT_RF_PER_TILE {
            log::warn!("Rate limit: RF {device_id}:{tile_id}:{freq_dim} > {RATE_LIMIT_RF_PER_TILE}");
            return false;
        }
        *count += 1;
        true
    }

    /// Returns true if the Wi-Fi contribution is within rate limit.
    pub fn allow_wifi(&mut self, device_id: &str, tile_id: &str, time_bucket: u64, band: &str, channel: u32) -> bool {
        self.maybe_reset(time_bucket);
        let key = format!("{device_id}:{tile_id}:{band}:{channel}");
        let count = self.wifi_counts.entry(key).or_insert(0);
        if *count >= RATE_LIMIT_WIFI_PER_TILE {
            log::warn!("Rate limit: Wi-Fi {device_id}:{tile_id}:{band}:{channel} > {RATE_LIMIT_WIFI_PER_TILE}");
            return false;
        }
        *count += 1;
        true
    }
}

// ── Time Decay ────────────────────────────────────────────────────────────────

/// Default half-life for RF heat display (seconds).
/// After this time, displayed intensity drops to 50% of observed.
pub const RF_DECAY_HALF_LIFE_SECS: f64 = 300.0;   // 5 minutes

/// Default half-life for Wi-Fi heat display.
pub const WIFI_DECAY_HALF_LIFE_SECS: f64 = 120.0; // 2 minutes

/// Compute an exponential decay multiplier for a tile observation.
///
/// `age_secs` = now − last_seen_utc
/// `half_life` = seconds until intensity halves
///
/// Returns a multiplier in (0, 1]. Apply to mean/max before rendering.
///
/// Formula: e^(-ln(2) / half_life * age_secs)
pub fn decay_factor(age_secs: f64, half_life: f64) -> f64 {
    if age_secs <= 0.0 || half_life <= 0.0 { return 1.0; }
    let exponent = -(std::f64::consts::LN_2 / half_life) * age_secs;
    exponent.exp().clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{RfAggregate, ChannelHotness};

    fn rf(mean: f64, max: f64, start: u64, end: u64) -> RfAggregate {
        RfAggregate { freq_start_hz: start, freq_end_hz: end,
            mean_power_dbm: mean, max_power_dbm: max, sample_count: 5, confidence: 0.8 }
    }

    fn wifi(band: &str, channel: u32, mean: f64, max: f64) -> ChannelHotness {
        ChannelHotness { band: band.to_string(), channel, count: 3,
            mean_rssi_dbm: mean, max_rssi_dbm: max, confidence: 0.7 }
    }

    // ── RF sanitization ──
    #[test] fn rf_valid_passes() { assert!(sanitize_rf(&rf(-72.0, -60.0, 2_400_000_000, 2_500_000_000)).is_some()); }
    #[test] fn rf_clamped_below_floor() {
        let a = sanitize_rf(&rf(-200.0, -100.0, 2_400_000_000, 2_500_000_000)).unwrap();
        assert!((a.mean_power_dbm - RF_MIN_POWER_DBM).abs() < 1e-9);
    }
    #[test] fn rf_clamped_above_ceiling() {
        let a = sanitize_rf(&rf(10.0, 20.0, 2_400_000_000, 2_500_000_000)).unwrap();
        assert!((a.max_power_dbm - RF_MAX_POWER_DBM).abs() < 1e-9);
    }
    #[test] fn rf_bad_freq_rejected() { assert!(sanitize_rf(&rf(-70.0, -60.0, 100, 200)).is_none()); }
    #[test] fn rf_inverted_freq_rejected() { assert!(sanitize_rf(&rf(-70.0, -60.0, 2_500_000_000, 2_400_000_000)).is_none()); }
    #[test] fn rf_mean_gt_max_rejected() { assert!(sanitize_rf(&rf(-50.0, -70.0, 2_400_000_000, 2_500_000_000)).is_none()); }
    #[test] fn rf_zero_samples_rejected() {
        let mut a = rf(-70.0, -60.0, 2_400_000_000, 2_500_000_000);
        a.sample_count = 0;
        assert!(sanitize_rf(&a).is_none());
    }

    // ── Wi-Fi sanitization ──
    #[test] fn wifi_valid_passes() { assert!(sanitize_wifi(&wifi("2.4", 6, -65.0, -55.0)).is_some()); }
    #[test] fn wifi_rssi_clamped() {
        let w = sanitize_wifi(&wifi("5", 36, -150.0, -5.0)).unwrap();
        assert!((w.mean_rssi_dbm - WIFI_MIN_RSSI_DBM as f64).abs() < 1e-9);
        assert!((w.max_rssi_dbm - WIFI_MAX_RSSI_DBM as f64).abs() < 1e-9);
    }
    #[test] fn wifi_bad_band_rejected() { assert!(sanitize_wifi(&wifi("3.0", 1, -65.0, -55.0)).is_none()); }
    #[test] fn wifi_zero_channel_rejected() { assert!(sanitize_wifi(&wifi("2.4", 0, -65.0, -55.0)).is_none()); }
    #[test] fn wifi_mean_gt_max_rejected() { assert!(sanitize_wifi(&wifi("2.4", 6, -40.0, -70.0)).is_none()); }

    // ── GPS validation ──
    #[test] fn gps_valid() { assert!(validate_gps(33.18, -96.88, 5.0, Some(0.0))); }
    #[test] fn gps_bad_lat() { assert!(!validate_gps(91.0, 0.0, 5.0, None)); }
    #[test] fn gps_bad_lon() { assert!(!validate_gps(0.0, 181.0, 5.0, None)); }
    #[test] fn gps_poor_accuracy() { assert!(!validate_gps(33.0, -96.0, 600.0, None)); }
    #[test] fn gps_implausible_speed() { assert!(!validate_gps(33.0, -96.0, 5.0, Some(400.0))); }

    // ── Rate limiter ──
    #[test]
    fn rate_limiter_allows_up_to_limit() {
        let mut rl = RateLimiter::new();
        for _ in 0..RATE_LIMIT_RF_PER_TILE {
            assert!(rl.allow_rf("dev", "tile", 60, "2400-2500"));
        }
        assert!(!rl.allow_rf("dev", "tile", 60, "2400-2500"));
    }

    #[test]
    fn rate_limiter_resets_on_new_bucket() {
        let mut rl = RateLimiter::new();
        for _ in 0..RATE_LIMIT_RF_PER_TILE {
            rl.allow_rf("dev", "tile", 60, "2400-2500");
        }
        // New time bucket → reset
        assert!(rl.allow_rf("dev", "tile", 120, "2400-2500"));
    }

    // ── Decay ──
    #[test] fn decay_at_zero() { assert!((decay_factor(0.0, 300.0) - 1.0).abs() < 1e-9); }
    #[test] fn decay_at_half_life() { assert!((decay_factor(300.0, 300.0) - 0.5).abs() < 0.001); }
    #[test] fn decay_at_two_half_lives() { assert!((decay_factor(600.0, 300.0) - 0.25).abs() < 0.001); }
    #[test] fn decay_clamped_to_zero_not_negative() { assert!(decay_factor(1_000_000.0, 1.0) >= 0.0); }
}
