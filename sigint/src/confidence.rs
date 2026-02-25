/// Configuration for confidence scoring.
/// All thresholds are configurable — defaults match the design spec.
#[derive(Debug, Clone)]
pub struct ConfidenceConfig {
    /// GPS accuracy above this = confidence 0 (metres)
    pub max_gps_accuracy_m: f64,
    /// Samples needed for full sample_factor
    pub min_samples_full: u32,
    /// Dwell time for full dwell_factor (seconds)
    pub min_dwell_secs: f64,
    /// Speed above this = confidence 0 (m/s, ~65 mph)
    pub max_speed_mps: f64,
}

impl Default for ConfidenceConfig {
    fn default() -> Self {
        Self {
            max_gps_accuracy_m: 20.0,
            min_samples_full: 10,
            min_dwell_secs: 30.0,
            max_speed_mps: 30.0,
        }
    }
}

/// Compute confidence ∈ [0.0, 1.0].
///
/// Formula (all factors clamped to [0,1], product is the result):
/// ```text
/// gps_factor    = clamp(1 − accuracy_m / max_gps_accuracy_m, 0, 1)
/// sample_factor = min(sample_count / min_samples_full, 1.0)
/// dwell_factor  = min(dwell_secs   / min_dwell_secs,   1.0)
/// speed_factor  = clamp(1 − speed_mps / max_speed_mps, 0, 1)
/// confidence    = gps_factor × sample_factor × dwell_factor × speed_factor
/// ```
pub fn compute(
    gps_accuracy_m: f64,
    sample_count: u32,
    dwell_secs: f64,
    speed_mps: f64,
    config: &ConfidenceConfig,
) -> f64 {
    let gps_factor = (1.0 - gps_accuracy_m / config.max_gps_accuracy_m).clamp(0.0, 1.0);
    let sample_factor = ((sample_count as f64) / (config.min_samples_full as f64)).min(1.0);
    let dwell_factor = (dwell_secs / config.min_dwell_secs).min(1.0);
    let speed_factor = (1.0 - speed_mps / config.max_speed_mps).clamp(0.0, 1.0);

    gps_factor * sample_factor * dwell_factor * speed_factor
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ConfidenceConfig {
        ConfidenceConfig::default()
    }

    #[test]
    fn perfect_conditions() {
        // Accurate GPS, enough samples, full dwell, stationary → 1.0
        let c = compute(0.0, 10, 30.0, 0.0, &cfg());
        assert!((c - 1.0).abs() < 1e-9, "expected 1.0, got {c}");
    }

    #[test]
    fn zero_accuracy_perfect() {
        // 0m accuracy = GPS factor 1.0
        let c = compute(0.0, 10, 30.0, 0.0, &cfg());
        assert!((c - 1.0).abs() < 1e-9);
    }

    #[test]
    fn max_gps_accuracy_zero_confidence() {
        // Exactly at the limit → gps_factor = 0 → confidence = 0
        let c = compute(20.0, 10, 30.0, 0.0, &cfg());
        assert!((c - 0.0).abs() < 1e-9, "expected 0.0, got {c}");
    }

    #[test]
    fn beyond_max_gps_accuracy_still_zero() {
        // Worse than max → clamped to 0
        let c = compute(100.0, 10, 30.0, 0.0, &cfg());
        assert!((c - 0.0).abs() < 1e-9);
    }

    #[test]
    fn max_speed_zero_confidence() {
        // At or above max_speed_mps → speed_factor = 0 → confidence = 0
        let c = compute(0.0, 10, 30.0, 30.0, &cfg());
        assert!((c - 0.0).abs() < 1e-9, "expected 0.0, got {c}");
    }

    #[test]
    fn beyond_max_speed_clamped() {
        let c = compute(0.0, 10, 30.0, 100.0, &cfg());
        assert!((c - 0.0).abs() < 1e-9);
    }

    #[test]
    fn single_sample_reduces_confidence() {
        // 1/10 samples → sample_factor = 0.1
        let c = compute(0.0, 1, 30.0, 0.0, &cfg());
        assert!((c - 0.1).abs() < 1e-9, "expected 0.1, got {c}");
    }

    #[test]
    fn full_dwell() {
        // Exactly at min_dwell_secs → dwell_factor = 1.0
        let c = compute(0.0, 10, 30.0, 0.0, &cfg());
        assert!((c - 1.0).abs() < 1e-9);
    }

    #[test]
    fn half_dwell() {
        // 15/30 → dwell_factor = 0.5
        let c = compute(0.0, 10, 15.0, 0.0, &cfg());
        assert!((c - 0.5).abs() < 1e-9, "expected 0.5, got {c}");
    }

    #[test]
    fn all_zeros_is_zero() {
        // gps_factor = 1 (accuracy=0), sample=0/10=0 → product = 0
        let c = compute(0.0, 0, 0.0, 0.0, &cfg());
        assert!((c - 0.0).abs() < 1e-9);
    }

    #[test]
    fn custom_config() {
        let cfg = ConfidenceConfig {
            max_gps_accuracy_m: 10.0,
            min_samples_full: 5,
            min_dwell_secs: 10.0,
            max_speed_mps: 10.0,
        };
        // 5m accuracy on 10m scale → gps_factor = 0.5
        // 5 samples, full → sample_factor = 1.0
        // 10s dwell, full → dwell_factor = 1.0
        // 5 m/s on 10 m/s scale → speed_factor = 0.5
        let c = compute(5.0, 5, 10.0, 5.0, &cfg);
        assert!((c - 0.25).abs() < 1e-9, "expected 0.25, got {c}");
    }
}
