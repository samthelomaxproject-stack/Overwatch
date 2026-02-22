use tauri::Manager;
use std::sync::Mutex;

// Global location state
static LOCATION_STATE: Mutex<LocationState> = Mutex::new(LocationState {
    latitude: 0.0,
    longitude: 0.0,
    accuracy: 0.0,
    has_fix: false,
    permission_granted: false,
});

#[derive(Clone, Copy)]
struct LocationState {
    latitude: f64,
    longitude: f64,
    accuracy: f64,
    has_fix: bool,
    permission_granted: bool,
}

#[cfg(target_os = "macos")]
mod macos_location {
    use objc::{class, msg_send, sel, sel_impl};
    use objc::runtime::Object;
    
    pub fn request_permission() {
        unsafe {
            let cls = class!(CLLocationManager);
            let manager: *mut Object = msg_send![cls, alloc];
            let manager: *mut Object = msg_send![manager, init];
            let _: () = msg_send![manager, requestWhenInUseAuthorization];
        }
    }
    
    pub fn start_updates() {
        unsafe {
            let cls = class!(CLLocationManager);
            let manager: *mut Object = msg_send![cls, alloc];
            let manager: *mut Object = msg_send![manager, init];
            let _: () = msg_send![manager, startUpdatingLocation];
        }
    }
    
    pub fn get_authorization_status() -> i32 {
        unsafe {
            let cls = class!(CLLocationManager);
            let status: i32 = msg_send![cls, authorizationStatus];
            status
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod macos_location {
    pub fn request_permission() {}
    pub fn start_updates() {}
    pub fn get_authorization_status() -> i32 { 0 }
}

// Tauri command to get current location
#[tauri::command]
fn get_current_location() -> Result<(f64, f64, f64), String> {
    let state = LOCATION_STATE.lock().unwrap();
    if state.has_fix {
        Ok((state.latitude, state.longitude, state.accuracy))
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
        .setup(|_app| {
            #[cfg(target_os = "macos")]
            {
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    macos_location::request_permission();
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    macos_location::start_updates();
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