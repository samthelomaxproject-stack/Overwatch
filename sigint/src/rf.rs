use std::collections::{HashMap, VecDeque};
use crate::wire::RfAggregate;

/// A single decoded line from hackrf_sweep stdout.
///
/// hackrf_sweep CSV format:
/// `date, time, hz_low, hz_high, hz_bin_width, num_samples, dBm, dBm, ...`
#[derive(Debug, Clone)]
pub struct RfObservation {
    pub timestamp_utc: u64,
    pub freq_start_hz: u64,
    pub freq_end_hz: u64,
    pub bin_hz: u64,
    /// Power readings per bin (dBm)
    pub bins: Vec<f64>,
}

impl RfObservation {
    pub fn mean_power_dbm(&self) -> Option<f64> {
        if self.bins.is_empty() {
            return None;
        }
        Some(self.bins.iter().sum::<f64>() / self.bins.len() as f64)
    }

    pub fn max_power_dbm(&self) -> Option<f64> {
        self.bins.iter().copied().reduce(f64::max)
    }
}

/// Parse one line of hackrf_sweep CSV output into an RfObservation.
/// Returns None if the line is malformed or a comment.
///
/// Expected format:
/// `2024-01-01, 12:00:00, 2400000000, 2500000000, 1000000, 10, -72.3, -68.1, ...`
pub fn parse_hackrf_line(line: &str) -> Option<RfObservation> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    let parts: Vec<&str> = line.split(',').map(str::trim).collect();
    if parts.len() < 7 {
        return None;
    }

    // Parse date + time → Unix timestamp (best-effort; fall back to now)
    let timestamp_utc = parse_datetime_utc(parts[0], parts[1]);

    let freq_start_hz: u64 = parts[2].parse().ok()?;
    let freq_end_hz: u64 = parts[3].parse().ok()?;
    let bin_hz: u64 = parts[4].parse().ok()?;
    // parts[5] is num_samples — we don't use it directly

    let bins: Vec<f64> = parts[6..]
        .iter()
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();

    if bins.is_empty() {
        return None;
    }

    Some(RfObservation {
        timestamp_utc,
        freq_start_hz,
        freq_end_hz,
        bin_hz,
        bins,
    })
}

fn parse_datetime_utc(date: &str, time: &str) -> u64 {
    // Try to parse "YYYY-MM-DD HH:MM:SS" as UTC
    let dt_str = format!("{} {}", date.trim(), time.trim());
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&dt_str, "%Y-%m-%d %H:%M:%S") {
        dt.and_utc().timestamp() as u64
    } else {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

// ── Ring Buffer ───────────────────────────────────────────────────────────────

/// Fixed-capacity ring buffer. Oldest entries are dropped when full.
pub struct RingBuffer<T> {
    capacity: usize,
    items: VecDeque<T>,
}

impl<T> RingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            items: VecDeque::with_capacity(capacity),
        }
    }

    pub fn push(&mut self, item: T) {
        if self.items.len() == self.capacity {
            self.items.pop_front();
        }
        self.items.push_back(item);
    }

    pub fn drain_all(&mut self) -> Vec<T> {
        self.items.drain(..).collect()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

// ── Flush window → aggregates ─────────────────────────────────────────────────

/// Drain a batch of observations (5-second flush window) into RfAggregates.
/// Groups by (freq_start_hz, freq_end_hz) and computes mean/max.
pub fn flush_to_aggregates(observations: &[RfObservation], confidence: f64) -> Vec<RfAggregate> {
    // key: (freq_start, freq_end)
    let mut groups: HashMap<(u64, u64), (f64, f64, u32)> = HashMap::new();

    for obs in observations {
        let key = (obs.freq_start_hz, obs.freq_end_hz);
        let mean = match obs.mean_power_dbm() {
            Some(v) => v,
            None => continue,
        };
        let max = obs.max_power_dbm().unwrap_or(mean);

        let entry = groups.entry(key).or_insert((0.0, f64::NEG_INFINITY, 0));
        // Running mean via accumulator (simple sum, divide at end)
        entry.0 += mean;
        entry.1 = entry.1.max(max);
        entry.2 += 1;
    }

    groups
        .into_iter()
        .map(|((freq_start_hz, freq_end_hz), (sum_mean, max_power_dbm, count))| RfAggregate {
            freq_start_hz,
            freq_end_hz,
            mean_power_dbm: sum_mean / count as f64,
            max_power_dbm,
            sample_count: count,
            confidence,
        })
        .collect()
}

// ── Tile Bucket ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RfBucketKey {
    tile_id: String,
    time_bucket: u64,
    freq_start_hz: u64,
    freq_end_hz: u64,
}

/// Aggregated RF state per (tile, time_bucket, freq_band).
/// Uses Welford's online algorithm for numerically stable running mean.
#[derive(Debug)]
struct RfBucketEntry {
    /// Welford: number of samples
    n: u32,
    /// Welford: running mean
    mean: f64,
    /// Welford: sum of squared deviations (M2)
    m2: f64,
    max_power_dbm: f64,
    last_seen: u64,
    confidence_sum: f64,
}

impl RfBucketEntry {
    fn new(power: f64, ts: u64, confidence: f64) -> Self {
        Self {
            n: 1,
            mean: power,
            m2: 0.0,
            max_power_dbm: power,
            last_seen: ts,
            confidence_sum: confidence,
        }
    }

    /// Welford update
    fn update(&mut self, power: f64, ts: u64, confidence: f64) {
        self.n += 1;
        let delta = power - self.mean;
        self.mean += delta / self.n as f64;
        let delta2 = power - self.mean;
        self.m2 += delta * delta2;
        self.max_power_dbm = self.max_power_dbm.max(power);
        self.last_seen = self.last_seen.max(ts);
        self.confidence_sum += confidence;
    }

    fn mean_confidence(&self) -> f64 {
        self.confidence_sum / self.n as f64
    }
}

/// In-memory RF tile bucket store.
pub struct RfTileBucket {
    buckets: HashMap<RfBucketKey, RfBucketEntry>,
}

impl RfTileBucket {
    pub fn new() -> Self {
        Self {
            buckets: HashMap::new(),
        }
    }

    /// Upsert an observation into the bucket for (tile_id, time_bucket, freq band).
    pub fn upsert(
        &mut self,
        tile_id: &str,
        time_bucket: u64,
        obs: &RfObservation,
        confidence: f64,
    ) {
        let mean = match obs.mean_power_dbm() {
            Some(v) => v,
            None => return,
        };
        let max = obs.max_power_dbm().unwrap_or(mean);
        let key = RfBucketKey {
            tile_id: tile_id.to_string(),
            time_bucket,
            freq_start_hz: obs.freq_start_hz,
            freq_end_hz: obs.freq_end_hz,
        };

        match self.buckets.get_mut(&key) {
            Some(entry) => entry.update(max, obs.timestamp_utc, confidence),
            None => {
                self.buckets
                    .insert(key, RfBucketEntry::new(mean, obs.timestamp_utc, confidence));
            }
        }
    }

    /// Drain all buckets as RfAggregates for a given tile + time_bucket.
    pub fn drain_tile(
        &mut self,
        tile_id: &str,
        time_bucket: u64,
    ) -> Vec<RfAggregate> {
        let keys: Vec<_> = self
            .buckets
            .keys()
            .filter(|k| k.tile_id == tile_id && k.time_bucket == time_bucket)
            .cloned()
            .collect();

        keys.into_iter()
            .filter_map(|k| {
                self.buckets.remove(&k).map(|e| RfAggregate {
                    freq_start_hz: k.freq_start_hz,
                    freq_end_hz: k.freq_end_hz,
                    mean_power_dbm: e.mean,
                    max_power_dbm: e.max_power_dbm,
                    sample_count: e.n,
                    confidence: e.mean_confidence(),
                })
            })
            .collect()
    }
}

impl Default for RfTileBucket {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_line() {
        let line = "2024-01-15, 12:30:00, 2400000000, 2500000000, 1000000, 10, -72.3, -68.1, -74.5";
        let obs = parse_hackrf_line(line).unwrap();
        assert_eq!(obs.freq_start_hz, 2_400_000_000);
        assert_eq!(obs.freq_end_hz, 2_500_000_000);
        assert_eq!(obs.bin_hz, 1_000_000);
        assert_eq!(obs.bins.len(), 3);
        assert!((obs.bins[0] - (-72.3)).abs() < 1e-9);
    }

    #[test]
    fn parse_empty_line_returns_none() {
        assert!(parse_hackrf_line("").is_none());
        assert!(parse_hackrf_line("   ").is_none());
        assert!(parse_hackrf_line("# comment").is_none());
    }

    #[test]
    fn parse_too_few_fields_returns_none() {
        assert!(parse_hackrf_line("2024-01-15, 12:30:00, 100").is_none());
    }

    #[test]
    fn ring_buffer_drops_oldest() {
        let mut buf: RingBuffer<u32> = RingBuffer::new(3);
        buf.push(1);
        buf.push(2);
        buf.push(3);
        buf.push(4); // drops 1
        let items = buf.drain_all();
        assert_eq!(items, vec![2, 3, 4]);
    }

    #[test]
    fn ring_buffer_drain_empties() {
        let mut buf: RingBuffer<u32> = RingBuffer::new(10);
        buf.push(1);
        buf.push(2);
        let _ = buf.drain_all();
        assert!(buf.is_empty());
    }

    #[test]
    fn flush_to_aggregates_groups_by_band() {
        let obs1 = RfObservation {
            timestamp_utc: 1000,
            freq_start_hz: 2_400_000_000,
            freq_end_hz: 2_500_000_000,
            bin_hz: 1_000_000,
            bins: vec![-70.0, -72.0],
        };
        let obs2 = RfObservation {
            timestamp_utc: 1001,
            freq_start_hz: 2_400_000_000,
            freq_end_hz: 2_500_000_000,
            bin_hz: 1_000_000,
            bins: vec![-68.0, -65.0],
        };
        let aggs = flush_to_aggregates(&[obs1, obs2], 0.8);
        assert_eq!(aggs.len(), 1);
        assert!(aggs[0].max_power_dbm <= -65.0 + 1e-6);
        assert_eq!(aggs[0].sample_count, 2);
    }

    #[test]
    fn tile_bucket_welford_mean() {
        let mut bucket = RfTileBucket::new();
        let obs = RfObservation {
            timestamp_utc: 1000,
            freq_start_hz: 2_400_000_000,
            freq_end_hz: 2_500_000_000,
            bin_hz: 1_000_000,
            bins: vec![-70.0],
        };
        bucket.upsert("test_tile", 60, &obs, 0.9);
        bucket.upsert("test_tile", 60, &obs, 0.9);
        let aggs = bucket.drain_tile("test_tile", 60);
        assert_eq!(aggs.len(), 1);
        assert_eq!(aggs[0].sample_count, 2);
        assert!((aggs[0].mean_power_dbm - (-70.0)).abs() < 1e-6);
    }
}
