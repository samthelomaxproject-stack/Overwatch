use tauri::Manager;
use std::sync::{Arc, Mutex};

// Store location data
#[derive(Default, Clone)]
struct LocationData {
    lat: f64,
    lon: f64,
    accuracy: f64,
    has_fix: bool,
}

static LOCATION: Mutex<LocationData> = Mutex::new(LocationData {
    lat: 0.0,
    lon: 0.0,
    accuracy: 0.0,
    has_fix: false,
});

#[cfg(target_os = "macos")]
mod macos_location {
    use super::*;
    use objc::{class, msg_send, sel, sel_impl};
    use objc::runtime::Object;
    use std::ffi::c_void;
    
    pub fn request_location_permission() {
        unsafe {
            let cls = class!(CLLocationManager);
            let manager: *mut Object = msg_send![cls, alloc];
            let manager: *mut Object = msg_send![manager, init];
            
            // Set delegate to handle location updates
            let delegate = create_location_delegate();
            let _: () = msg_send![manager, setDelegate: delegate];
            
            // Request when-in-use authorization
            let _: () = msg_send![manager, requestWhenInUseAuthorization];
            
            // Start updating location
            let _: () = msg_send![manager, startUpdatingLocation];
        }
    }
    
    unsafe fn create_location_delegate() -> *mut Object {
        // Simple delegate that just stores location
        // In a real implementation, you'd create a proper delegate class
        std::ptr::null_mut()
    }
    
    pub fn get_authorization_status() -> String {
        unsafe {
            let cls = class!(CLLocationManager);
            let status: i32 = msg_send![cls, authorizationStatus];
            
            match status {
                0 => "not_determined".to_string(),
                1 => "restricted".to_string(),
                2 => "denied".to_string(),
                3 => "authorized_always".to_string(),
                4 => "authorized_when_in_use".to_string(),
                _ => "unknown".to_string(),
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod macos_location {
    use super::*;
    pub fn request_location_permission() {}
    pub fn get_authorization_status() -> String {
        "not_macos".to_string()
    }
}

#[tauri::command]
fn get_location_status() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        Ok(macos_location::get_authorization_status())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Not on macOS".to_string())
    }
}

#[tauri::command]
fn request_location() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        macos_location::request_location_permission();
        Ok("Location requested".to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Not on macOS".to_string())
    }
}

pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            // Request location permission on startup
            #[cfg(target_os = "macos")]
            {
                macos_location::request_location_permission();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_location_status,
            request_location
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}