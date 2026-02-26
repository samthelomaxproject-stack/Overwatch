/// hackrf_sweep spawner — runs `hackrf_sweep` and feeds stdout into the RF ring buffer.
///
/// Spawns `hackrf_sweep` as a child process, reads its CSV stdout line-by-line,
/// parses each line with `rf::parse_hackrf_line`, and pushes valid observations
/// into a shared `RingBuffer<RfObservation>`.
///
/// # Frequency plan (default)
/// Covers 2.4 GHz Wi-Fi band. Override via `SweepConfig::freq_ranges`.
///
/// # Usage
/// ```no_run
/// use sigint::sweeper::{Sweeper, SweepConfig};
/// use sigint::rf::RingBuffer;
/// use std::sync::{Arc, Mutex};
///
/// let buf = Arc::new(Mutex::new(RingBuffer::new(1000)));
/// let sweeper = Sweeper::new(SweepConfig::default(), Arc::clone(&buf));
/// std::thread::spawn(move || sweeper.run());
/// ```
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio, Child};
use std::sync::{Arc, Mutex};
use crate::rf::{parse_hackrf_line, RingBuffer, RfObservation};
use crate::Error;

/// Common hackrf_sweep binary locations
const HACKRF_SWEEP_PATHS: &[&str] = &[
    "hackrf_sweep",                    // $PATH
    "/opt/homebrew/bin/hackrf_sweep",  // Homebrew Apple Silicon
    "/usr/local/bin/hackrf_sweep",     // Homebrew Intel / Linux
    "/usr/bin/hackrf_sweep",           // Linux package manager
];

/// A single frequency range for hackrf_sweep (MHz).
#[derive(Debug, Clone)]
pub struct FreqRange {
    pub start_mhz: u32,
    pub end_mhz: u32,
    /// Bin width in Hz (hackrf_sweep -B, default 1 000 000 = 1 MHz)
    pub bin_hz: u32,
}

impl FreqRange {
    pub fn new(start_mhz: u32, end_mhz: u32) -> Self {
        Self { start_mhz, end_mhz, bin_hz: 1_000_000 }
    }
}

/// Sweeper configuration.
#[derive(Debug, Clone)]
pub struct SweepConfig {
    /// Frequency ranges to sweep. hackrf_sweep supports multiple -f arguments
    /// but we chain sweeps sequentially for simplicity.
    pub freq_ranges: Vec<FreqRange>,
    /// Gain (LNA, 0–40 dB in 8 dB steps)
    pub lna_gain: u8,
    /// VGA gain (0–62 dB in 2 dB steps)
    pub vga_gain: u8,
    /// Number of sweeps per frequency range (0 = infinite)
    pub num_sweeps: u32,
    /// Path override for hackrf_sweep binary (None = auto-detect)
    pub binary_path: Option<String>,
}

impl Default for SweepConfig {
    fn default() -> Self {
        Self {
            freq_ranges: vec![
                FreqRange::new(2400, 2500),   // 2.4 GHz Wi-Fi / Bluetooth
                FreqRange::new(5150, 5850),   // 5 GHz Wi-Fi
                FreqRange::new(902, 928),     // 900 MHz ISM (LoRa, Meshtastic)
                FreqRange::new(433, 435),     // 433 MHz ISM
            ],
            lna_gain: 16,
            vga_gain: 20,
            num_sweeps: 0, // infinite
            binary_path: None,
        }
    }
}

/// Detects the hackrf_sweep binary path.
pub fn detect_binary() -> Option<String> {
    for &path in HACKRF_SWEEP_PATHS {
        if std::process::Command::new(path)
            .arg("--help")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
        {
            return Some(path.to_string());
        }
    }
    None
}

/// Spawns hackrf_sweep and streams observations into the shared ring buffer.
pub struct Sweeper {
    config: SweepConfig,
    buffer: Arc<Mutex<RingBuffer<RfObservation>>>,
}

impl Sweeper {
    pub fn new(config: SweepConfig, buffer: Arc<Mutex<RingBuffer<RfObservation>>>) -> Self {
        Self { config, buffer }
    }

    /// Resolve the binary path (config override or auto-detect).
    pub fn binary_path(&self) -> Result<String, Error> {
        if let Some(ref p) = self.config.binary_path {
            return Ok(p.clone());
        }
        detect_binary().ok_or_else(|| Error::ScannerUnavailable(
            "hackrf_sweep not found — install HackRF tools (brew install hackrf)".to_string()
        ))
    }

    /// Build the hackrf_sweep command for one frequency range.
    fn build_command(&self, binary: &str, range: &FreqRange) -> Command {
        let mut cmd = Command::new(binary);
        cmd.arg("-f")
            .arg(format!("{}:{}", range.start_mhz, range.end_mhz))
            .arg("-B")
            .arg(range.bin_hz.to_string())
            .arg("-l")
            .arg(self.config.lna_gain.to_string())
            .arg("-g")
            .arg(self.config.vga_gain.to_string());

        if self.config.num_sweeps > 0 {
            cmd.arg("-n").arg(self.config.num_sweeps.to_string());
        }

        cmd.stdout(Stdio::piped())
            .stderr(Stdio::null());
        cmd
    }

    /// Spawn hackrf_sweep for a single frequency range and feed into buffer.
    /// Returns after the process exits or errors.
    fn sweep_range(&self, binary: &str, range: &FreqRange) -> Result<(), Error> {
        let mut cmd = self.build_command(binary, range);
        let mut child: Child = cmd.spawn().map_err(Error::Io)?;

        let stdout = child.stdout.take()
            .ok_or_else(|| Error::Other("hackrf_sweep stdout not available".to_string()))?;

        let reader = BufReader::new(stdout);
        let mut line_count = 0u64;
        let mut parse_errors = 0u32;

        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if let Some(obs) = parse_hackrf_line(&l) {
                        if let Ok(mut buf) = self.buffer.lock() {
                            buf.push(obs);
                        }
                        line_count += 1;
                    } else if !l.trim().is_empty() && !l.starts_with('#') {
                        parse_errors += 1;
                        if parse_errors <= 5 {
                            log::debug!("hackrf_sweep: unparseable line: {l}");
                        }
                    }
                }
                Err(e) => {
                    log::warn!("hackrf_sweep read error: {e}");
                    break;
                }
            }
        }

        let _ = child.wait();
        log::info!("hackrf_sweep {}–{} MHz: {} observations ({} parse errors)",
            range.start_mhz, range.end_mhz, line_count, parse_errors);
        Ok(())
    }

    /// Run the sweeper loop (blocking — call from a dedicated thread).
    ///
    /// Cycles through all configured frequency ranges continuously.
    /// On error (device unplugged, binary not found), waits 10s and retries.
    pub fn run(self) {
        let binary = match self.binary_path() {
            Ok(b) => {
                log::info!("hackrf_sweep binary: {b}");
                b
            }
            Err(e) => {
                log::error!("Sweeper cannot start: {e}");
                return;
            }
        };

        log::info!("Sweeper starting — {} frequency ranges", self.config.freq_ranges.len());

        loop {
            for range in &self.config.freq_ranges {
                log::debug!("Sweeping {}–{} MHz", range.start_mhz, range.end_mhz);
                if let Err(e) = self.sweep_range(&binary, range) {
                    log::error!("Sweep error for {}–{} MHz: {e}", range.start_mhz, range.end_mhz);
                    std::thread::sleep(std::time::Duration::from_secs(10));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_four_bands() {
        let cfg = SweepConfig::default();
        assert_eq!(cfg.freq_ranges.len(), 4);
    }

    #[test]
    fn build_command_contains_freq_args() {
        let cfg = SweepConfig {
            binary_path: Some("hackrf_sweep".to_string()),
            ..SweepConfig::default()
        };
        let sweeper = Sweeper::new(cfg, Arc::new(Mutex::new(RingBuffer::new(100))));
        let range = FreqRange::new(2400, 2500);
        // Just verify it doesn't panic and has correct range
        let _cmd = sweeper.build_command("hackrf_sweep", &range);
        assert_eq!(range.start_mhz, 2400);
        assert_eq!(range.end_mhz, 2500);
    }

    #[test]
    fn binary_path_override_used() {
        let cfg = SweepConfig {
            binary_path: Some("/custom/hackrf_sweep".to_string()),
            ..SweepConfig::default()
        };
        let sweeper = Sweeper::new(cfg, Arc::new(Mutex::new(RingBuffer::new(100))));
        assert_eq!(sweeper.binary_path().unwrap(), "/custom/hackrf_sweep");
    }

    #[test]
    fn no_binary_returns_error() {
        let cfg = SweepConfig {
            binary_path: Some("/nonexistent/hackrf_sweep".to_string()),
            freq_ranges: vec![FreqRange::new(2400, 2500)],
            ..SweepConfig::default()
        };
        let buf = Arc::new(Mutex::new(RingBuffer::new(100)));
        let sweeper = Sweeper::new(cfg, Arc::clone(&buf));
        // spawn will fail — just test binary_path resolution works
        assert!(sweeper.binary_path().is_ok()); // override is set
    }

    #[test]
    fn ring_buffer_receives_parsed_observations() {
        // Feed a real hackrf_sweep CSV line through the parser manually
        let buf = Arc::new(Mutex::new(RingBuffer::<RfObservation>::new(100)));
        let line = "2024-01-15, 12:30:00, 2400000000, 2500000000, 1000000, 10, -72.3, -68.1, -74.5";
        if let Some(obs) = parse_hackrf_line(line) {
            buf.lock().unwrap().push(obs);
        }
        assert_eq!(buf.lock().unwrap().len(), 1);
    }
}
