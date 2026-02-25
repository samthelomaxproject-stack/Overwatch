/// A GPS fix with position, accuracy, and optional motion data.
#[derive(Debug, Clone)]
pub struct GpsFix {
    pub lat: f64,
    pub lon: f64,
    /// Horizontal accuracy (metres)
    pub accuracy_m: f64,
    pub altitude_m: Option<f64>,
    /// Ground speed (m/s)
    pub speed_mps: Option<f64>,
    pub timestamp_utc: u64,
}

impl GpsFix {
    /// Derive the H3 tile_id string for this fix at a given resolution (0–15).
    /// Returns the H3 cell index as a hex string.
    pub fn tile_id(&self, resolution: u8) -> String {
        use h3o::{CellIndex, LatLng, Resolution};
        let res = Resolution::try_from(resolution).unwrap_or(Resolution::Ten);
        let ll = LatLng::new(self.lat, self.lon)
            .expect("lat/lon out of range");
        let cell: CellIndex = ll.to_cell(res);
        format!("{cell}")
    }

    /// Time bucket: floor to nearest 60-second boundary
    pub fn time_bucket(&self) -> u64 {
        (self.timestamp_utc / 60) * 60
    }
}

/// Pluggable GPS provider interface.
pub trait GpsProvider: Send + Sync {
    fn current_fix(&self) -> Option<GpsFix>;
}

/// Stub implementation — returns a fixed position.
/// Used for unit tests and offline development.
pub struct StubGpsProvider {
    pub fix: Option<GpsFix>,
}

impl StubGpsProvider {
    pub fn with_fix(lat: f64, lon: f64, accuracy_m: f64) -> Self {
        Self {
            fix: Some(GpsFix {
                lat,
                lon,
                accuracy_m,
                altitude_m: None,
                speed_mps: Some(0.0),
                timestamp_utc: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            }),
        }
    }

    pub fn unavailable() -> Self {
        Self { fix: None }
    }
}

impl GpsProvider for StubGpsProvider {
    fn current_fix(&self) -> Option<GpsFix> {
        self.fix.clone()
    }
}

/// macOS CoreLocation provider.
/// Hooks into the existing macos_location module in the Tauri backend.
/// In standalone (non-Tauri) use, reads from a shared memory location
/// updated by the Tauri process (Phase 2 implementation).
pub struct MacosGpsProvider;

impl GpsProvider for MacosGpsProvider {
    fn current_fix(&self) -> Option<GpsFix> {
        // Phase 2: IPC to macos_location module or shared state
        // For now, returns None (no fix available without Tauri context)
        log::debug!("MacosGpsProvider: Phase 2 IPC not yet implemented");
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_returns_fix() {
        let gps = StubGpsProvider::with_fix(33.18, -96.88, 5.0);
        let fix = gps.current_fix().unwrap();
        assert!((fix.lat - 33.18).abs() < 1e-9);
        assert!((fix.accuracy_m - 5.0).abs() < 1e-9);
    }

    #[test]
    fn tile_id_returns_h3_hex() {
        let fix = GpsFix {
            lat: 33.18, lon: -96.88, accuracy_m: 5.0,
            altitude_m: None, speed_mps: None,
            timestamp_utc: 1740000000,
        };
        let tile = fix.tile_id(10);
        // H3 cell ids are 15-char hex strings
        assert_eq!(tile.len(), 15, "H3 cell id should be 15 chars, got: {tile}");
        // Must be valid hex
        assert!(u64::from_str_radix(&tile, 16).is_ok(), "Not valid hex: {tile}");
    }

    #[test]
    fn stub_unavailable_returns_none() {
        let gps = StubGpsProvider::unavailable();
        assert!(gps.current_fix().is_none());
    }

    #[test]
    fn time_bucket_rounds_down() {
        let fix = GpsFix {
            lat: 0.0, lon: 0.0, accuracy_m: 5.0,
            altitude_m: None, speed_mps: None,
            timestamp_utc: 1740000090, // 90s into a minute
        };
        assert_eq!(fix.time_bucket(), 1740000060);
    }
}
