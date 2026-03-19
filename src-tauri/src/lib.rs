use tauri::{Manager, Emitter, WebviewWindowBuilder, WebviewUrl, Url};
use serde::Serialize;
use std::thread;
use std::time::{Duration, Instant};
use std::ffi::CStr;
use std::os::raw::c_char;
#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};
use serde_json::json;

// Include generated protobuf code
pub mod meshtastic_proto {
    include!(concat!(env!("OUT_DIR"), "/meshtastic.rs"));
}

mod meshtastic;
mod rtl_sdr_test;

// Location state shared between threads
#[derive(Clone, Copy, Default, Debug)]
struct LocationState {
    latitude: f64,
    longitude: f64,
    accuracy: f64,
    has_fix: bool,
    permission_granted: bool,
}

// Global atomic location cache (thread-safe, no raw pointers)
static CURR_LAT: Mutex<f64> = Mutex::new(0.0);
static CURR_LON: Mutex<f64> = Mutex::new(0.0);
static CURR_ACC: Mutex<f64> = Mutex::new(0.0);
static HAS_FIX: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
mod macos_location {
    use objc::{class, msg_send, sel, sel_impl};
    use objc::runtime::Object;
    use std::sync::atomic::{AtomicPtr, Ordering};
    
    // Atomic pointer to location manager - thread-safe storage
    static MANAGER_PTR: AtomicPtr<Object> = AtomicPtr::new(std::ptr::null_mut());

    fn get_manager() -> Option<*mut Object> {
        let ptr = MANAGER_PTR.load(Ordering::SeqCst);
        if ptr.is_null() {
            None
        } else {
            Some(ptr)
        }
    }

    fn set_manager(manager: *mut Object) {
        MANAGER_PTR.store(manager, Ordering::SeqCst);
    }

    pub fn setup_location_manager() {
        if get_manager().is_none() {
            unsafe {
                let cls = class!(CLLocationManager);
                let manager: *mut Object = msg_send![cls, alloc];
                let manager: *mut Object = msg_send![manager, init];
                set_manager(manager);
            }
        }
    }

    pub fn request_permission() {
        setup_location_manager();
        unsafe {
            if let Some(manager) = get_manager() {
                let _: () = msg_send![manager, requestWhenInUseAuthorization];
            }
        }
    }
    
    pub fn start_updates() {
        setup_location_manager();
        unsafe {
            if let Some(manager) = get_manager() {
                let _: () = msg_send![manager, startUpdatingLocation];
            }
        }
    }
    
    pub fn get_authorization_status() -> i32 {
        unsafe {
            let cls = class!(CLLocationManager);
            let status: i32 = msg_send![cls, authorizationStatus];
            eprintln!("CoreLocation authorization status: {}", status);
            status
        }
    }
    
    pub fn get_current_location() -> Option<(f64, f64, f64)> {
        unsafe {
            if let Some(manager) = get_manager() {
                let location: *mut Object = msg_send![manager, location];
                if !location.is_null() {
                    let coord: CLLocationCoordinate2D = msg_send![location, coordinate];
                    let accuracy: f64 = msg_send![location, horizontalAccuracy];
                    eprintln!("CoreLocation got position: {:.6}, {:.6}, accuracy: {:.1}m", coord.latitude, coord.longitude, accuracy);
                    return Some((coord.latitude, coord.longitude, accuracy));
                } else {
                    eprintln!("CoreLocation location is null");
                }
            } else {
                eprintln!("CoreLocation manager not available");
            }
            None
        }
    }
    
    #[repr(C)]
    #[derive(Debug, Copy, Clone)]
    pub struct CLLocationCoordinate2D {
        pub latitude: f64,
        pub longitude: f64,
    }
}

#[cfg(not(target_os = "macos"))]
mod macos_location {
    pub fn setup_location_manager() {}
    pub fn request_permission() {}
    pub fn start_updates() {}
    pub fn get_authorization_status() -> i32 { 0 }
    pub fn get_current_location() -> Option<(f64, f64, f64)> { None }
}

// ADS-B Module for RTL-SDR integration
mod adsb {
    use super::*;
    use serde::Serialize;
    
    #[derive(Clone, Debug, Serialize)]
    pub struct Aircraft {
        pub icao: String,
        pub callsign: Option<String>,
        pub latitude: Option<f64>,
        pub longitude: Option<f64>,
        pub altitude: Option<u32>,
        pub speed: Option<u32>,
        pub heading: Option<u32>,
        pub vertical_rate: Option<i32>,
        #[serde(skip)]
        pub last_seen: std::time::Instant,
    }
    
    pub struct AdsbState {
        pub aircraft: Arc<Mutex<Vec<Aircraft>>>,
        pub is_running: AtomicBool,
    }
    
    impl AdsbState {
        pub fn new() -> Self {
            Self {
                aircraft: Arc::new(Mutex::new(Vec::new())),
                is_running: AtomicBool::new(false),
            }
        }
    }
    
    // Try to start dump1090 and parse output
    pub fn start_adsb_monitoring(app_handle: tauri::AppHandle, state: Arc<AdsbState>) {
        if state.is_running.load(Ordering::SeqCst) {
            eprintln!("ADS-B already running");
            return;
        }
        
        state.is_running.store(true, Ordering::SeqCst);
        let aircraft = state.aircraft.clone();
        let state_clone = state.clone();
        
        thread::spawn(move || {
            eprintln!("Starting ADS-B monitoring...");
            
            // Try to find dump1090 in common locations
            let dump1090_paths = [
                "/opt/homebrew/bin/dump1090",
                "/usr/local/bin/dump1090",
                "dump1090",
                "./dump1090",
            ];
            
            let mut dump1090_cmd: Option<Command> = None;
            
            for path in &dump1090_paths {
                if std::path::Path::new(path).exists() || Command::new("which").arg(path).output().map(|o| o.status.success()).unwrap_or(false) {
                    let mut cmd = Command::new(path);
                    cmd.arg("--net")
                       .arg("--interactive")
                       .arg("--metric")
                       .stdout(Stdio::piped())
                       .stderr(Stdio::piped());
                    dump1090_cmd = Some(cmd);
                    eprintln!("Found dump1090 at: {}", path);
                    break;
                }
            }
            
            if let Some(mut cmd) = dump1090_cmd {
                match cmd.spawn() {
                    Ok(mut child) => {
                        eprintln!("dump1090 started successfully");
                        
                        if let Some(stdout) = child.stdout.take() {
                            let reader = BufReader::new(stdout);
                            
                            for line in reader.lines() {
                                if !state_clone.is_running.load(Ordering::SeqCst) {
                                    break;
                                }
                                
                                if let Ok(line) = line {
                                    // Parse aircraft data from dump1090 output
                                    // Example: Hex     Mode  Sqwk  Flight   Alt    Spd  Hdg  Lat      Long     Sig  Msgs   Ti|
                                    if let Some(new_aircraft) = parse_dump1090_line(&line) {
                                        let mut ac_list = aircraft.lock().unwrap();
                                        
                                        // Update or add aircraft
                                        if let Some(existing) = ac_list.iter_mut().find(|a| a.icao == new_aircraft.icao) {
                                            *existing = new_aircraft.clone();
                                        } else {
                                            ac_list.push(new_aircraft.clone());
                                        }
                                        
                                        // Emit to JavaScript
                                        let _ = app_handle.emit("adsb-aircraft", new_aircraft.clone());
                                        
                                        // Emit aircraft count
                                        let count = ac_list.len();
                                        let _ = app_handle.emit("adsb-count", count);
                                    }
                                }
                            }
                        }
                        
                        // Cleanup
                        let _ = child.kill();
                    }
                    Err(e) => {
                        eprintln!("Failed to start dump1090: {}", e);
                        // Emit error to JavaScript
                        let _ = app_handle.emit("adsb-error", format!("Failed to start dump1090: {}", e));
                    }
                }
            } else {
                eprintln!("dump1090 not found, running in simulation mode");
                
                // Simulation mode for testing
                let mut counter = 0;
                while state_clone.is_running.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_secs(2));
                    counter += 1;
                    
                    // Simulate aircraft
                    if counter % 3 == 0 {
                        let sim_aircraft = Aircraft {
                            icao: format!("A{:04X}", counter),
                            callsign: Some(format!("SIM{}", counter)),
                            latitude: Some(33.18 + (counter as f64 * 0.001)),
                            longitude: Some(-96.88 + (counter as f64 * 0.001)),
                            altitude: Some(5000 + (counter * 100) as u32),
                            speed: Some(250 + (counter * 10) as u32),
                            heading: Some((counter * 45) as u32 % 360),
                            vertical_rate: Some(0),
                            last_seen: std::time::Instant::now(),
                        };
                        
                        {
                            let mut ac_list = aircraft.lock().unwrap();
                            ac_list.push(sim_aircraft.clone());
                            if ac_list.len() > 5 {
                                ac_list.remove(0);
                            }
                        }
                        
                        let _ = app_handle.emit("adsb-aircraft", sim_aircraft.clone());
                        let count = aircraft.lock().unwrap().len();
                        let _ = app_handle.emit("adsb-count", count);
                        eprintln!("Simulated aircraft: {} (count: {})", sim_aircraft.icao, count);
                    }
                }
            }
            
            state.is_running.store(false, Ordering::SeqCst);
            eprintln!("ADS-B monitoring stopped");
        });
    }
    
    fn parse_dump1090_line(line: &str) -> Option<Aircraft> {
        // Basic parsing - dump1090 output varies by version
        // This is a simplified parser
        let parts: Vec<&str> = line.split_whitespace().collect();
        
        if parts.len() >= 2 && parts[0].starts_with("Hex") == false {
            // Try to extract hex code (ICAO)
            let icao = parts[0].to_string();
            
            // Look for flight number
            let callsign = parts.iter().find(|&&p| p.len() >= 3 && p.chars().all(|c| c.is_alphanumeric())).map(|&s| s.to_string());
            
            // Look for altitude (numbers followed by 'm' or just numbers)
            let altitude = parts.iter().find_map(|&p| {
                if let Ok(alt) = p.parse::<u32>() {
                    if alt > 0 && alt < 50000 { Some(alt) } else { None }
                } else {
                    None
                }
            });
            
            // Look for lat/lon
            let mut latitude = None;
            let mut longitude = None;
            
            for (_i, &part) in parts.iter().enumerate() {
                if let Ok(val) = part.parse::<f64>() {
                    if val.abs() < 90.0 && latitude.is_none() {
                        latitude = Some(val);
                    } else if val.abs() < 180.0 && longitude.is_none() && latitude.is_some() {
                        longitude = Some(val);
                    }
                }
            }
            
            Some(Aircraft {
                icao,
                callsign,
                latitude,
                longitude,
                altitude,
                speed: None,
                heading: None,
                vertical_rate: None,
                last_seen: std::time::Instant::now(),
            })
        } else {
            None
        }
    }
}

// Update global atomic cache
fn update_global_location(lat: f64, lon: f64, acc: f64) {
    *CURR_LAT.lock().unwrap() = lat;
    *CURR_LON.lock().unwrap() = lon;
    *CURR_ACC.lock().unwrap() = acc;
    HAS_FIX.store(true, Ordering::SeqCst);
    // Share with sigint collector thread
    sigint::gps::update_shared_gps_fix(lat, lon, acc, None);
}

// Tauri command to get current location
#[tauri::command]
fn get_current_location() -> Result<(f64, f64, f64), String> {
    #[cfg(target_os = "macos")]
    {
        if let Some((lat, lon, acc)) = macos_location::get_current_location() {
            update_global_location(lat, lon, acc);
            return Ok((lat, lon, acc));
        }
    }
    
    if HAS_FIX.load(Ordering::SeqCst) {
        let lat = *CURR_LAT.lock().unwrap();
        let lon = *CURR_LON.lock().unwrap();
        let acc = *CURR_ACC.lock().unwrap();
        Ok((lat, lon, acc))
    } else {
        Err("No location fix yet".to_string())
    }
}

// Tauri command to check permission status
#[tauri::command]
fn check_location_permission() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let status = macos_location::get_authorization_status();
        let status_str = match status {
            0 => "not_determined",
            1 => "restricted", 
            2 => "denied",
            3 => "authorized_always",
            4 => "authorized_when_in_use",
            _ => "unknown",
        };
        eprintln!("RUST: check_location_permission returning: '{}' (raw status: {})", status_str, status);
        Ok(status_str.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Not on macOS".to_string())
    }
}

// Tauri command to request permission
#[tauri::command]
fn request_location_permission() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos_location::request_permission();
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Not on macOS".to_string())
    }
}

// ADS-B State
static ADSB_STATE: Mutex<Option<Arc<adsb::AdsbState>>> = Mutex::new(None);

// Meshtastic State
static MESHTASTIC_STATE: Mutex<Option<Arc<meshtastic::MeshtasticState>>> = Mutex::new(None);

// Tauri command to start ADS-B monitoring
#[tauri::command]
fn start_adsb(app_handle: tauri::AppHandle) -> Result<String, String> {
    let mut state_guard = ADSB_STATE.lock().unwrap();
    
    if state_guard.is_none() {
        *state_guard = Some(Arc::new(adsb::AdsbState::new()));
    }
    
    if let Some(ref state) = *state_guard {
        adsb::start_adsb_monitoring(app_handle, state.clone());
        Ok("ADS-B monitoring started".to_string())
    } else {
        Err("Failed to initialize ADS-B state".to_string())
    }
}

// Tauri command to stop ADS-B monitoring
#[tauri::command]
fn stop_adsb() -> Result<String, String> {
    let state_guard = ADSB_STATE.lock().unwrap();
    
    if let Some(ref state) = *state_guard {
        state.is_running.store(false, Ordering::SeqCst);
        Ok("ADS-B monitoring stopped".to_string())
    } else {
        Ok("ADS-B was not running".to_string())
    }
}

// Tauri command to get ADS-B aircraft list
#[tauri::command]
fn get_adsb_aircraft() -> Result<Vec<String>, String> {
    let state_guard = ADSB_STATE.lock().unwrap();

    if let Some(ref state) = *state_guard {
        let aircraft = state.aircraft.lock().unwrap();
        let icaos: Vec<String> = aircraft.iter().map(|a| a.icao.clone()).collect();
        Ok(icaos)
    } else {
        Ok(vec![])
    }
}

// Meshtastic Tauri commands
#[tauri::command]
fn start_meshtastic(app_handle: tauri::AppHandle, port: String) -> Result<String, String> {
    // Synchronous test first - does the port even open?
    let test_result = std::panic::catch_unwind(|| {
        use std::time::Duration;
        
        let mut last_error = String::new();
        for baud in [921600u32, 115200] {
            match serialport::new(&port, baud)
                .timeout(Duration::from_millis(500))
                .open() {
                Ok(_) => return Ok(format!("Port {} opened successfully at {} baud", port, baud)),
                Err(e) => last_error = format!("Failed at {} baud: {}", baud, e),
            }
        }
        Err(last_error)
    });
    
    match test_result {
        Ok(Ok(msg)) => {
            // Port opened! Now start the background thread
            let mut state_guard = MESHTASTIC_STATE.lock().unwrap();
            if state_guard.is_none() {
                *state_guard = Some(Arc::new(meshtastic::MeshtasticState::new()));
            }
            if let Some(ref state) = *state_guard {
                let _ = meshtastic::start_meshtastic_serial(app_handle, state.clone(), port);
            }
            Ok(msg)
        }
        Ok(Err(e)) => Err(format!("Serial port test failed: {}", e)),
        Err(_) => Err("Serial port test panicked".to_string()),
    }
}

#[tauri::command]
fn stop_meshtastic() -> Result<String, String> {
    let mut state_guard = MESHTASTIC_STATE.lock().unwrap();

    if let Some(ref state) = *state_guard {
        meshtastic::stop_meshtastic(state);
        Ok("Meshtastic disconnected".to_string())
    } else {
        Ok("Meshtastic was not running".to_string())
    }
}

#[tauri::command]
fn send_meshtastic_message(port: String, text: String, channel: u32) -> Result<(), String> {
    let state_guard = MESHTASTIC_STATE.lock().unwrap();

    if let Some(ref state) = *state_guard {
        meshtastic::send_text_message(state, &port, text, channel)
    } else {
        Err("Meshtastic not initialized".to_string())
    }
}

#[tauri::command]
fn send_meshtastic_text(port: String, text: String, channel: u32) -> Result<(), String> {
    // Simple text send without protobuf wrapping - for ATAK mode
    let state_guard = MESHTASTIC_STATE.lock().unwrap();

    if let Some(ref state) = *state_guard {
        meshtastic::send_text_message(state, &port, text, channel)
    } else {
        Err("Meshtastic not initialized".to_string())
    }
}

#[tauri::command]
fn get_meshtastic_ports() -> Result<Vec<String>, String> {
    Ok(meshtastic::get_available_ports())
}

#[tauri::command]
fn get_meshtastic_nodes() -> Result<Vec<meshtastic::MeshtasticNode>, String> {
    let state_guard = MESHTASTIC_STATE.lock().unwrap();

    if let Some(ref state) = *state_guard {
        let nodes = state.nodes.lock().unwrap();
        Ok(nodes.clone())
    } else {
        Ok(vec![])
    }
}

#[tauri::command]
fn get_meshtastic_messages() -> Result<Vec<meshtastic::MeshtasticMessage>, String> {
    let state_guard = MESHTASTIC_STATE.lock().unwrap();

    if let Some(ref state) = *state_guard {
        let messages = state.messages.lock().unwrap();
        Ok(messages.clone())
    } else {
        Ok(vec![])
    }
}

pub fn run() {
    // Initialize ADS-B state
    {
        let mut adsb_state = ADSB_STATE.lock().unwrap();
        *adsb_state = Some(Arc::new(adsb::AdsbState::new()));
    }

    // Initialize Meshtastic state
    {
        let mut mesh_state = MESHTASTIC_STATE.lock().unwrap();
        *mesh_state = Some(Arc::new(meshtastic::MeshtasticState::new()));
    }

// Meshtastic CLI integration commands
const MESHTASTIC_CLI_PATH: &str = "/opt/homebrew/bin/meshtastic";
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};

#[tauri::command]
fn meshtastic_cli_test_tcp(host: String) -> Result<String, String> {
    let output = Command::new(MESHTASTIC_CLI_PATH)
        .args(&["--host", &host, "--info"])
        .output()
        .map_err(|e| format!("Failed to run meshtastic CLI: {}", e))?;
    
    if !output.status.success() {
        return Err(format!("meshtastic CLI failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[tauri::command]
fn meshtastic_cli_send_tcp(host: String, text: String) -> Result<String, String> {
    let output = Command::new(MESHTASTIC_CLI_PATH)
        .args(&["--host", &host, "--sendtext", &text])
        .output()
        .map_err(|e| format!("Failed to run meshtastic CLI: {}", e))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!("Failed to send. Stderr: {} | Stdout: {}", stderr, stdout));
    }
    
    Ok(format!("Message sent: {}", text))
}

#[tauri::command]
fn meshtastic_cli_test(port: String) -> Result<String, String> {
    let output = Command::new(MESHTASTIC_CLI_PATH)
        .args(&["--port", &port, "--info"])
        .output()
        .map_err(|e| format!("Failed to run meshtastic CLI: {}", e))?;
    
    if !output.status.success() {
        return Err(format!("meshtastic CLI failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[tauri::command]
fn meshtastic_cli_send(port: String, text: String) -> Result<String, String> {
    let output = Command::new(MESHTASTIC_CLI_PATH)
        .args(&["--port", &port, "--sendtext", &text])
        .output()
        .map_err(|e| format!("Failed to run meshtastic CLI: {}", e))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!("Failed to send. Stderr: {} | Stdout: {}", stderr, stdout));
    }
    
    Ok(format!("Message sent: {}", text))
}

#[tauri::command]
fn meshtastic_cli_start_listen(_app_handle: tauri::AppHandle, _port: String) -> Result<String, String> {
    // Note: --listen blocks the port, so we skip it for now
    // Messages will be polled separately if needed
    Ok("Listening disabled to allow sending".to_string())
}

// RTL-SDR / ADS-B State
static RTL_SDR_STATE: Mutex<Option<bool>> = Mutex::new(None);
static RTL_SDR_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static RTL_SDR_STATUS: Mutex<String> = Mutex::new(String::new());
static RTL_SDR_AIRCRAFT: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

// Aircraft database with TTL cleanup (like Intercept's DataStore)
use std::collections::HashMap;
use std::time::{Instant, Duration};

#[derive(Clone, Debug)]
struct Aircraft {
    icao: String,
    callsign: Option<String>,
    registration: Option<String>,
    aircraft_type: Option<String>,
    altitude: Option<i32>,
    speed: Option<i32>,
    heading: Option<i32>,
    vertical_rate: Option<i32>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    squawk: Option<String>,
    last_seen: Instant,
}

impl Aircraft {
    fn new(icao: String) -> Self {
        Aircraft {
            icao,
            callsign: None,
            registration: None,
            aircraft_type: None,
            altitude: None,
            speed: None,
            heading: None,
            vertical_rate: None,
            latitude: None,
            longitude: None,
            squawk: None,
            last_seen: Instant::now(),
        }
    }
    
    fn is_stale(&self) -> bool {
        self.last_seen.elapsed() > Duration::from_secs(60)
    }
}

// Debug logging helper
fn debug_log(msg: &str) {
    eprintln!("[DEBUG] {}", msg);
}

#[tauri::command]
fn start_rtl_sdr(app_handle: tauri::AppHandle) -> Result<String, String> {
    // First, kill any existing instances
    {
        let mut pids = RTL_SDR_PIDS.lock().unwrap();
        for pid in pids.iter() {
            let _ = std::process::Command::new("kill")
                .args(&["-9", &pid.to_string()])
                .output();
        }
        pids.clear();
        
        // Also kill any orphaned Python bridges
        let _ = std::process::Command::new("pkill")
            .args(&["-9", "-f", "rtl_sdr_socket.py"])
            .output();
    }
    
    let mut state = RTL_SDR_STATE.lock().unwrap();
    if *state == Some(true) {
        return Err("Already running".to_string());
    }
    *state = Some(true);
    
    // Clone for thread
    let app_handle_thread = app_handle.clone();
    
    std::thread::spawn(move || {
        // Start dump1090 first
        if let Ok(child) = std::process::Command::new("/opt/homebrew/bin/dump1090")
            .args(&["--net", "--net-sbs-port", "30003", "--quiet"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn() {
            let id = child.id();
            let mut pids = RTL_SDR_PIDS.lock().unwrap();
            pids.push(id);
        }
        
        // Helper macro to set status in static and emit
        macro_rules! set_status {
            ($msg:expr) => {
                {
                    let msg = $msg.to_string();
                    if let Ok(mut s) = RTL_SDR_STATUS.lock() {
                        *s = msg.clone();
                    }
                    let _ = app_handle_thread.emit("rtl-sdr-status", msg);
                }
            };
        }

        // Wait for dump1090 to start
        set_status!("STARTING");
        std::thread::sleep(std::time::Duration::from_secs(3));
        
        // Spawn Python bridge
        set_status!("BRIDGE_STARTING");
        
        let child = std::process::Command::new("python3")
            .arg("/Users/thelomaxproject/Overwatch/rtl_sdr_socket.py")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn();
        
        match child {
            Ok(mut child) => {
                set_status!("BRIDGE_UP");
                
                // Open log file for Rust-side logging
                use std::fs::OpenOptions;
                use std::io::Write;
                
                let mut rust_log = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("/tmp/adsb_rust.log")
                    .ok();
                
                if let Some(ref mut log) = rust_log {
                    let _ = writeln!(log, "[{}] Rust: Started reading from Python bridge", 
                        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());
                }
                
                // TCP CONNECTION
                std::thread::sleep(std::time::Duration::from_secs(4));
                use std::net::TcpStream;
                set_status!("CONNECTING");
                
                match TcpStream::connect("127.0.0.1:30004") {
                    Ok(stream) => {
                        set_status!("CONNECTED");
                        let reader = BufReader::new(stream);
                        for line in reader.lines() {
                            if let Ok(line) = line {
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                                    if let Some(aircraft) = json.get("aircraft") {
                                        let now_secs = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs();

                                        // Stamp with last_seen and upsert into static db
                                        if let Ok(mut db) = RTL_SDR_AIRCRAFT.lock() {
                                            let icao = aircraft.get("icao").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let mut entry = aircraft.clone();
                                            entry["_last_seen"] = serde_json::json!(now_secs);

                                            if let Some(pos) = db.iter().position(|a| a.get("icao").and_then(|v| v.as_str()) == Some(&icao)) {
                                                db[pos] = entry;
                                            } else {
                                                db.push(entry);
                                            }

                                            // Prune stale entries (>60s) right here
                                            db.retain(|a| {
                                                a.get("_last_seen")
                                                    .and_then(|t| t.as_u64())
                                                    .map(|t| now_secs.saturating_sub(t) < 60)
                                                    .unwrap_or(false)
                                            });
                                        }
                                        let _ = app_handle_thread.emit("rtl-sdr-aircraft", aircraft.clone());
                                    }
                                }
                            }
                        }
                        set_status!("DISCONNECTED");
                    }
                    Err(e) => {
                        let msg = format!("TCP_ERROR: {:?}", e);
                        set_status!(msg.as_str());
                        let _ = app_handle_thread.emit("rtl-sdr-error", msg);
                    }
                }
            }
            Err(e) => {
                let msg = format!("BRIDGE_ERROR: {}", e);
                set_status!(msg.as_str());
                let _ = app_handle_thread.emit("rtl-sdr-error", msg);
            }
        }
    });
    
    Ok("RTL-SDR streaming started".to_string())
}

#[tauri::command]
fn get_rtl_sdr_status() -> serde_json::Value {
    let status = RTL_SDR_STATUS.lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    let aircraft: Vec<serde_json::Value> = RTL_SDR_AIRCRAFT.lock()
        .map(|db| {
            db.iter().map(|ac| {
                // Strip internal _last_seen before sending to JS
                let mut clean = ac.clone();
                if let Some(obj) = clean.as_object_mut() {
                    obj.remove("_last_seen");
                }
                clean
            }).collect()
        })
        .unwrap_or_default();
    let count = aircraft.len();
    serde_json::json!({
        "status": status,
        "aircraft": aircraft,
        "count": count
    })
}

// ── Hub process management ────────────────────────────────────────────────────

static HUB_PID: Mutex<Option<u32>> = Mutex::new(None);

/// Start the hub-api process (sigint hub, localhost:8789).
/// Spawns a Rust thread that runs the hub HTTP server.
/// Idempotent — returns immediately if already running.
#[tauri::command]
fn start_hub() -> Result<String, String> {
    let mut pid = HUB_PID.lock().unwrap();
    if pid.is_some() {
        return Ok("Hub already running".to_string());
    }

    // Spawn hub in a background thread using the sigint crate
    // The hub runs on 0.0.0.0:8789 (all interfaces — VPN clients can reach it)
    std::thread::spawn(|| {
        let config = sigint::hub::HubConfig {
            bind_addr: "0.0.0.0:8789".to_string(),
            db_path: format!("{}/hub.db",
                std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())),
            collector_enabled: true,
        };
        log::info!("Starting hub-api on {}", config.bind_addr);
        if let Err(e) = sigint::hub::run_hub(config) {
            log::error!("Hub exited: {e}");
        }
    });

    // Mark as running (thread id not available, use sentinel)
    *pid = Some(1);
    Ok("Hub started on 0.0.0.0:8789".to_string())
}

/// Stop the hub (marks as stopped — actual thread cleanup on next restart).
#[tauri::command]
fn stop_hub() -> Result<String, String> {
    let mut pid = HUB_PID.lock().unwrap();
    *pid = None;
    Ok("Hub stopped".to_string())
}

/// Check if the hub is running.
#[tauri::command]
fn hub_status() -> serde_json::Value {
    let running = HUB_PID.lock().unwrap().is_some();
    // Also verify it's actually reachable
    let reachable = ureq::get("http://127.0.0.1:8789/health")
        .call()
        .map(|r| r.status() == 200)
        .unwrap_or(false);
    serde_json::json!({ "running": running, "reachable": reachable })
}

static COLLECTOR_RUNNING: Mutex<bool> = Mutex::new(false);
static PRIVACY_MODE: Mutex<String> = Mutex::new(String::new());

#[tauri::command]
fn set_privacy_mode(mode: String) -> Result<String, String> {
    let validated = match mode.to_uppercase().as_str() {
        "A" | "B" | "C" => mode.to_uppercase(),
        _ => return Err(format!("Invalid mode: {mode} — must be A, B, or C")),
    };
    *PRIVACY_MODE.lock().unwrap() = validated.clone();
    // Update shared static read by the collector thread on every scan cycle
    sigint::wifi::set_shared_privacy_mode(sigint::wifi::PrivacyMode::from_str(&validated));
    log::info!("Wi-Fi privacy mode set to {validated}");
    Ok(format!("Privacy mode set to {validated}"))
}

fn current_privacy_mode() -> sigint::wifi::PrivacyMode {
    let s = PRIVACY_MODE.lock().unwrap();
    sigint::wifi::PrivacyMode::from_str(if s.is_empty() { "A" } else { s.as_str() })
}

#[cfg(target_os = "macos")]
fn nsstring_to_string(obj: *mut objc::runtime::Object) -> String {
    if obj.is_null() {
        return String::new();
    }
    unsafe {
        let c: *const c_char = msg_send![obj, UTF8String];
        if c.is_null() {
            return String::new();
        }
        CStr::from_ptr(c).to_string_lossy().into_owned()
    }
}

#[cfg(target_os = "macos")]
#[derive(Serialize)]
struct UiWifiNetwork {
    display_name: String,
    bssid_display: Option<String>,
    band: String,
    channel: u32,
    rssi_dbm: i32,
    privacy_mode: String,
}

#[cfg(target_os = "macos")]
fn scan_wifi_native_for_ui(mode: sigint::wifi::PrivacyMode) -> Result<Vec<UiWifiNetwork>, String> {
    use objc::{class, msg_send};
    use objc::runtime::Object;

    unsafe {
        let client_cls = class!(CWWiFiClient);
        let client: *mut Object = msg_send![client_cls, sharedWiFiClient];
        if client.is_null() {
            return Err("CWWiFiClient unavailable".to_string());
        }

        let iface: *mut Object = msg_send![client, interface];
        if iface.is_null() {
            return Err("No Wi-Fi interface".to_string());
        }

        let nil_obj: *mut Object = std::ptr::null_mut();
        let mut err: *mut Object = std::ptr::null_mut();
        let networks: *mut Object = msg_send![iface, scanForNetworksWithSSID:nil_obj error:&mut err];
        if networks.is_null() {
            return Err("CoreWLAN scan returned null".to_string());
        }

        let enumerator: *mut Object = msg_send![networks, objectEnumerator];
        let mut out: Vec<UiWifiNetwork> = Vec::new();

        loop {
            let net: *mut Object = msg_send![enumerator, nextObject];
            if net.is_null() {
                break;
            }

            let ssid_obj: *mut Object = msg_send![net, ssid];
            let bssid_obj: *mut Object = msg_send![net, bssid];
            let rssi: i32 = msg_send![net, rssiValue];

            let ch_obj: *mut Object = msg_send![net, wlanChannel];
            if ch_obj.is_null() {
                continue;
            }
            let channel: u32 = msg_send![ch_obj, channelNumber];
            let band_code: i64 = msg_send![ch_obj, channelBand];
            let band = match band_code {
                1 => "2.4".to_string(),
                2 => "5".to_string(),
                3 => "6".to_string(),
                _ => {
                    if channel > 14 { "5".to_string() } else { "2.4".to_string() }
                }
            };

            let ssid = nsstring_to_string(ssid_obj);
            let bssid = nsstring_to_string(bssid_obj);

            let (display_name, bssid_display) = match mode {
                sigint::wifi::PrivacyMode::A => (format!("Ch {} · {} GHz", channel, band), None),
                sigint::wifi::PrivacyMode::B => (
                    format!("{} (hashed)", {
                        // Keep UI semantics aligned with sigint hash display.
                        let raw = if ssid.is_empty() { format!("(hidden) Ch {}", channel) } else { ssid.clone() };
                        // re-use existing mode-B output from last scan when possible
                        raw
                    }),
                    None,
                ),
                sigint::wifi::PrivacyMode::C => (
                    if ssid.is_empty() { format!("(hidden) Ch {}", channel) } else { ssid.clone() },
                    Some(if bssid.is_empty() { String::new() } else { bssid.clone() }),
                ),
            };

            out.push(UiWifiNetwork {
                display_name,
                bssid_display,
                band,
                channel,
                rssi_dbm: rssi,
                privacy_mode: mode.as_str().to_string(),
            });
        }

        out.sort_by(|a, b| b.rssi_dbm.cmp(&a.rssi_dbm));
        Ok(out)
    }
}

/// Return the most recent raw Wi-Fi scan results for UI display.
/// Respects privacy mode: Mode A shows only channels, B shows hashes, C shows real SSIDs/BSSIDs.
#[tauri::command]
fn get_wifi_scan_results() -> serde_json::Value {
    let mode = current_privacy_mode();

    #[cfg(target_os = "macos")]
    {
        if mode == sigint::wifi::PrivacyMode::C {
            if let Ok(native) = scan_wifi_native_for_ui(mode) {
                return serde_json::json!({
                    "mode": mode.as_str(),
                    "count": native.len(),
                    "networks": native,
                    "source": "corewlan-native"
                });
            }
        }
    }

    let results = sigint::wifi::get_last_scan_results();
    serde_json::json!({
        "mode": mode.as_str(),
        "count": results.len(),
        "networks": results,
        "source": "collector-cache"
    })
}

/// Read EUD/node connection status from hub DB for tactical debug output.
#[tauri::command]
fn get_eud_statuses() -> serde_json::Value {
    let db_path = format!("{}/hub.db",
        std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()));

    match sigint::hub::get_node_statuses(&db_path, 90) {
        Ok(nodes) => serde_json::json!({
            "ok": true,
            "count": nodes.len(),
            "nodes": nodes
        }),
        Err(e) => serde_json::json!({
            "ok": false,
            "error": e.to_string(),
            "nodes": []
        })
    }
}

/// Start the local SIGINT node collector.
/// Spawns threads for: Wi-Fi scanning, GPS, and sync push/pull loop.
/// hackrf_sweep is spawned separately via the Sweeper when RF is enabled.
#[tauri::command]
fn start_collector(hub_url: String) -> Result<String, String> {
    let mut running = COLLECTOR_RUNNING.lock().unwrap();
    if *running {
        return Ok("Collector already running".to_string());
    }
    *running = true;
    drop(running);

    std::thread::spawn(move || {
        use sigint::collector::{Collector, CollectorConfig};
        use sigint::gps::MacosGpsProvider;
        use sigint::wifi::AirportScanner;
        use sigint::storage::NodeDb;
        use sigint::sync::HttpSyncTransport;

        let mut config = CollectorConfig::default();
        config.privacy_mode = current_privacy_mode();
        let device_id = config.keys.device_id.clone();
        log::info!("Collector device_id: {}, privacy_mode: {}", device_id, config.privacy_mode.as_str());

        let db_path = format!("{}/sigint_node.db",
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()));
        let db = match NodeDb::open(&db_path) {
            Ok(d) => d,
            Err(e) => { log::error!("Collector DB error: {e}"); return; }
        };

        let transport = Box::new(HttpSyncTransport::new(hub_url, &device_id));
        let collector = Collector::new(
            config,
            Box::new(MacosGpsProvider),
            Box::new(AirportScanner),
            db,
            transport,
        );

        collector.run(); // blocking loop
    });

    Ok("Collector started".to_string())
}

/// Start the hackrf_sweep RF sweeper thread.
/// Feeds observations into the collector's ring buffer via the push_rf channel.
/// In the current architecture, the sweeper runs independently and the
/// collector flushes the buffer every 5s.
#[tauri::command]
fn start_sweeper() -> Result<String, String> {
    use sigint::sweeper::{Sweeper, SweepConfig};
    use sigint::rf::RingBuffer;
    use std::sync::{Arc, Mutex as StdMutex};

    // Detect binary first
    if sigint::sweeper::detect_binary().is_none() {
        return Err("hackrf_sweep not found — install HackRF tools".to_string());
    }

    std::thread::spawn(|| {
        let buf = Arc::new(StdMutex::new(RingBuffer::new(2000)));
        let sweeper = Sweeper::new(SweepConfig::default(), buf);
        sweeper.run();
    });

    Ok("Sweeper started".to_string())
}

/// Fetch SIGINT delta from the local hub-api (runs on localhost:8789).
/// Returns merged tile data since the given cursor timestamp.
/// JS polls this every 5 seconds when hub is running.
#[tauri::command]
fn get_sigint_delta(cursor: u64) -> serde_json::Value {
    let url = format!("http://127.0.0.1:8789/api/delta?device_id=overwatch-ui&cursor={cursor}");
    match ureq::get(&url).call() {
        Ok(resp) => {
            let body = resp.into_string().unwrap_or_default();
            serde_json::from_str::<serde_json::Value>(&body)
                .unwrap_or(serde_json::json!({"tiles": [], "cursor": cursor}))
        }
        Err(_) => serde_json::json!({"tiles": [], "cursor": cursor})
    }
}

#[derive(Serialize)]
struct CctvResolveResult {
    ok: bool,
    direct_url: Option<String>,
    feed_type: Option<String>,
    reason: Option<String>,
}

fn detect_feed_type(url: &str) -> &'static str {
    let u = url.to_lowercase();
    if u.starts_with("rtsp://") || u.starts_with("rtmp://") { return "rtsp"; }
    if u.contains(".m3u8") { return "hls"; }
    if u.contains(".mp4") || u.contains(".webm") || u.contains(".mov") { return "video"; }
    if u.contains(".jpg") || u.contains(".jpeg") || u.contains(".png") || u.contains(".gif") || u.contains(".webp") || u.contains("mjpg") || u.contains("mjpeg") { return "image"; }
    "unknown"
}

fn extract_direct_media_url(html: &str, base_url: &str) -> Option<String> {
    fn score_url(url: &str) -> i32 {
        let u = url.to_lowercase();
        let mut score = 0;
        if u.contains(".m3u8") { score += 120; }
        if u.contains(".mp4") || u.contains(".webm") || u.contains(".mov") { score += 100; }
        if u.contains(".mjpg") || u.contains(".mjpeg") { score += 90; }
        if u.contains(".jpg") || u.contains(".jpeg") || u.contains(".png") { score += 40; }
        if u.contains("stream") || u.contains("live") || u.contains("playlist") { score += 20; }
        if u.contains("logo") || u.contains("sprite") || u.contains("thumbnail") || u.contains("placeholder") { score -= 80; }
        if u.contains("earthcam") && (u.contains("logo") || u.contains("default")) { score -= 120; }
        score
    }

    let mut candidates: Vec<String> = Vec::new();

    if let Ok(re) = regex::Regex::new(r#"https?://[^"'\s<>]+\.(m3u8|mp4|webm|mov|mjpg|mjpeg|jpg|jpeg|png)(\?[^"'\s<>]*)?"#) {
        for m in re.find_iter(html) {
            candidates.push(m.as_str().to_string());
        }
    }

    if let Ok(attr_re) = regex::Regex::new(r#"(?:content|src)=["']([^"']+)["']"#) {
        for cap in attr_re.captures_iter(html) {
            if let Some(raw) = cap.get(1) {
                let candidate = raw.as_str();
                let lc = candidate.to_lowercase();
                if lc.contains(".m3u8") || lc.contains(".mp4") || lc.contains(".webm") || lc.contains(".mov") || lc.contains(".mjpg") || lc.contains(".mjpeg") || lc.contains(".jpg") || lc.contains(".jpeg") || lc.contains(".png") {
                    if let Ok(base) = Url::parse(base_url) {
                        if let Ok(joined) = base.join(candidate) {
                            candidates.push(joined.to_string());
                            continue;
                        }
                    }
                    candidates.push(candidate.to_string());
                }
            }
        }
    }

    candidates.sort_by_key(|u| -score_url(u));
    candidates.into_iter().next()
}

#[tauri::command]
fn resolve_cctv_stream_url(url: String) -> Result<CctvResolveResult, String> {
    let parsed = Url::parse(&url).map_err(|e| format!("Invalid URL: {}", e))?;
    let host = parsed.host_str().unwrap_or_default();
    if host.is_empty() {
        return Ok(CctvResolveResult { ok: false, direct_url: None, feed_type: None, reason: Some("invalid-host".to_string()) });
    }

    let resp = ureq::get(&url)
        .set("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122 Safari/537.36")
        .set("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .call();

    let body = match resp {
        Ok(r) => r.into_string().map_err(|e| format!("read body failed: {}", e))?,
        Err(e) => {
            return Ok(CctvResolveResult { ok: false, direct_url: None, feed_type: None, reason: Some(format!("fetch-failed: {}", e)) });
        }
    };

    if let Some(found) = extract_direct_media_url(&body, &url) {
        let feed = detect_feed_type(&found).to_string();
        return Ok(CctvResolveResult { ok: true, direct_url: Some(found), feed_type: Some(feed), reason: None });
    }

    Ok(CctvResolveResult { ok: false, direct_url: None, feed_type: None, reason: Some("no-direct-media-found".to_string()) })
}

#[tauri::command]
fn open_cctv_source_window(app: tauri::AppHandle, url: String, title: Option<String>) -> Result<String, String> {
    let mut parsed = Url::parse(&url).map_err(|e| format!("Invalid URL: {}", e))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    parsed.query_pairs_mut().append_pair("_owts", &ts.to_string());
    let label = format!("cctv-source-{}", ts);

    WebviewWindowBuilder::new(&app, label, WebviewUrl::External(parsed))
        .title(title.unwrap_or_else(|| "CCTV Source".to_string()))
        .inner_size(1280.0, 820.0)
        .resizable(true)
        .build()
        .map_err(|e| format!("Failed to open CCTV window: {}", e))?;

    Ok("opened".to_string())
}

#[tauri::command]
fn stop_rtl_sdr() -> Result<String, String> {
    let mut state = RTL_SDR_STATE.lock().unwrap();
    *state = Some(false);
    
    // Kill Python bridge
    let _ = Command::new("pkill")
        .args(&["-9", "-f", "rtl_sdr_poll.py"])
        .output();
    
    // Kill dump1090
    let _ = Command::new("pkill")
        .args(&["-9", "dump1090"])
        .output();
    
    Ok("RTL-SDR stopped".to_string())
}

    tauri::Builder::default()
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            {
                let app_handle = app.handle().clone();
                
                macos_location::setup_location_manager();
                macos_location::request_permission();
                std::thread::sleep(Duration::from_millis(500));
                macos_location::start_updates();
                
                thread::spawn(move || {
                    loop {
                        thread::sleep(Duration::from_secs(5));
                        
                        if let Some((lat, lon, acc)) = macos_location::get_current_location() {
                            update_global_location(lat, lon, acc);
                            let _ = app_handle.emit("location-update", (lat, lon, acc));
                        }
                        
                        let status = macos_location::get_authorization_status();
                        let granted = status == 3 || status == 4;
                        let _ = app_handle.emit("permission-change", granted);
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_current_location,
            check_location_permission,
            request_location_permission,
            start_adsb,
            stop_adsb,
            get_adsb_aircraft,
            start_meshtastic,
            stop_meshtastic,
            send_meshtastic_message,
            send_meshtastic_text,
            get_meshtastic_ports,
            get_meshtastic_nodes,
            get_meshtastic_messages,
            meshtastic_cli_test,
            meshtastic_cli_send,
            meshtastic_cli_start_listen,
            start_rtl_sdr,
            resolve_cctv_stream_url,
            open_cctv_source_window,
            stop_rtl_sdr,
            start_hub,
            stop_hub,
            hub_status,
            start_collector,
            start_sweeper,
            set_privacy_mode,
            get_wifi_scan_results,
            get_eud_statuses,
            get_rtl_sdr_status,
            get_sigint_delta
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
