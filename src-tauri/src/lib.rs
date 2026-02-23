use tauri::{Manager, Emitter};
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

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

pub fn run() {
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
            request_location_permission
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
