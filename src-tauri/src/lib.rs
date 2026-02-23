use tauri::{Manager, Emitter};
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};

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
    let mut state_guard = MESHTASTIC_STATE.lock().unwrap();

    if state_guard.is_none() {
        *state_guard = Some(Arc::new(meshtastic::MeshtasticState::new()));
    }

    if let Some(ref state) = *state_guard {
        meshtastic::start_meshtastic_serial(app_handle, state.clone(), port)
    } else {
        Err("Failed to initialize Meshtastic state".to_string())
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
            get_meshtastic_ports,
            get_meshtastic_nodes,
            get_meshtastic_messages
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
