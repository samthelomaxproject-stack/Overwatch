use tauri::{Manager, Emitter};
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};
use serde_json::json;

// Include generated protobuf code
pub mod meshtastic_proto {
    include!(concat!(env!("OUT_DIR"), "/meshtastic.rs"));
}

mod meshtastic;

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

// Debug logging helper
fn debug_log(msg: &str) {
    eprintln!("[DEBUG] {}", msg);
}

#[tauri::command]
fn start_rtl_sdr(app_handle: tauri::AppHandle) -> Result<String, String> {
    let mut state = RTL_SDR_STATE.lock().unwrap();
    if *state == Some(true) {
        return Err("RTL-SDR already running".to_string());
    }
    
    *state = Some(true);
    
    // Start dump1090 in net-only mode (no ncurses)
    thread::spawn(move || {
        let mut child = Command::new("/opt/homebrew/bin/dump1090")
            .args(&["--net", "--net-sbs-port", "30003", "--quiet"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to start dump1090");
        
        // Give dump1090 time to start
        thread::sleep(Duration::from_secs(2));
        
        let _ = app_handle.emit("rtl-sdr-status", "RTL-SDR scanning 1090 MHz - waiting for aircraft...");
        
        // Connect to SBS output port
        use std::net::TcpStream;
        use std::io::Read;
        use std::time::Duration;
        
        let mut retry_count = 0;
        let stream = loop {
            match TcpStream::connect_timeout(&"127.0.0.1:30003".parse().unwrap(), Duration::from_secs(2)) {
                Ok(s) => break s,
                Err(e) => {
                    retry_count += 1;
                    let _ = app_handle.emit("rtl-sdr-status", format!("Connection attempt {}: {}", retry_count, e));
                    if retry_count > 10 {
                        let _ = app_handle.emit("rtl-sdr-status", "Failed to connect to dump1090 after 10 retries");
                        return;
                    }
                    thread::sleep(Duration::from_millis(500));
                }
            }
        };
        
        let _ = app_handle.emit("rtl-sdr-status", "Connected - receiving aircraft data");
        
        // Read SBS format messages
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    // Parse SBS format: MSG,3,1,1,ICAO,1,2025/02/23,12:00:00.000,2025/02/23,12:00:00.000,Callsign,Alt,Speed,Track,Lat,Lon,...
                    let parts: Vec<&str> = line.trim().split(',').collect();
                    if parts.len() >= 15 && parts[0] == "MSG" {
                        let icao = parts[4].to_string();
                        let callsign = parts[10].trim().to_string();
                        let alt = parts[11].parse::<i32>().unwrap_or(0);
                        let speed = parts[12].parse::<i32>().unwrap_or(0);
                        let track = parts[13].parse::<i32>().unwrap_or(0);
                        let lat = parts[14].parse::<f64>().unwrap_or(0.0);
                        let lon = parts[15].parse::<f64>().unwrap_or(0.0);
                        
                        let aircraft = json!({
                            "icao": icao,
                            "callsign": callsign,
                            "altitude": alt,
                            "speed": speed,
                            "heading": track,
                            "latitude": lat,
                            "longitude": lon
                        });
                        
                        let _ = app_handle.emit("rtl-sdr-aircraft", aircraft);
                    }
                }
                Err(e) => {
                    let _ = app_handle.emit("rtl-sdr-error", format!("Read error: {}", e));
                    break;
                }
            }
        }
        
        let _ = child.wait();
    });
    
    Ok("RTL-SDR started - scanning 1090 MHz for aircraft".to_string())
}

#[tauri::command]
fn stop_rtl_sdr() -> Result<String, String> {
    let mut state = RTL_SDR_STATE.lock().unwrap();
    *state = Some(false);
    
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
            stop_rtl_sdr
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
