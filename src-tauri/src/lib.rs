use std::sync::Mutex;
use tauri::Manager;

#[cfg(target_os = "macos")]
mod macos_location {
    use objc::{class, msg_send, sel, sel_impl};
    use objc::runtime::{Object, BOOL, YES};
    
    pub fn request_location_permission() {
        unsafe {
            let cls = class!(CLLocationManager);
            let manager: *mut Object = msg_send![cls, alloc];
            let manager: *mut Object = msg_send![manager, init];
            
            // Request when-in-use authorization
            let _: () = msg_send![manager, requestWhenInUseAuthorization];
            
            // Also request always for background updates
            let _: () = msg_send![manager, requestAlwaysAuthorization];
        }
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
    pub fn request_location_permission() {}
    pub fn get_authorization_status() -> String {
        "not_macos".to_string()
    }
}

#[tauri::command]
fn request_macos_location_permission() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        macos_location::request_location_permission();
        Ok("Permission requested".to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Not on macOS".to_string())
    }
}

#[tauri::command]
fn get_macos_location_status() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        Ok(macos_location::get_authorization_status())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Not on macOS".to_string())
    }
}

pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            request_macos_location_permission,
            get_macos_location_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}