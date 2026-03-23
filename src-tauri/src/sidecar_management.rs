// OSINT Sidecar process management
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use ureq;

static SIDE_CAR_PID: Mutex<Option<u32>> = Mutex::new(None);
static SIDE_CAR_RUNNING: AtomicBool = AtomicBool::new(false);

/// Ensure required Python packages are installed (idempotent check).
fn ensure_python_deps(sidecar_path: &std::path::Path) -> Result<(), String> {
    let req_file = sidecar_path.join("requirements.txt");
    if !req_file.exists() {
        return Err(format!("requirements.txt not found at {}", req_file.display()));
    }

    // Quick check: can we import uvicorn?
    let check = Command::new("python3")
        .args(&["-c", "import uvicorn, fastapi"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    if check.is_ok() && check.unwrap().success() {
        log::info!("Python dependencies already installed");
        return Ok(());
    }

    // Need to install - run pip install
    log::info!("Installing Python sidecar dependencies...");
    let install = Command::new("python3")
        .args(&["-m", "pip", "install", "--quiet", "-r", req_file.to_string_lossy().as_ref()])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status();

    match install {
        Ok(status) if status.success() => {
            log::info!("Dependencies installed successfully");
            Ok(())
        }
        Ok(status) => Err(format!("pip install failed with code {}", status)),
        Err(e) => Err(format!("Failed to run pip: {}", e)),
    }
}

/// Start the osint_hub sidecar (FastAPI service on 127.0.0.1:8790).
/// Uses system Python and auto-installs dependencies on first launch.
/// Idempotent — returns immediately if already running.
#[tauri::command]
pub fn start_sidecar() -> Result<String, String> {
    // Fast check without lock
    if SIDE_CAR_RUNNING.load(Ordering::SeqCst) {
        return Ok("Sidecar already running".to_string());
    }

    let mut pid_guard = SIDE_CAR_PID.lock().unwrap();
    if pid_guard.is_some() {
        SIDE_CAR_RUNNING.store(true, Ordering::SeqCst);
        return Ok("Sidecar already running".to_string());
    }

    // Determine paths relative to the running executable
    let exe_path = std::env::current_exe()
        .map_err(|e| format!("Cannot determine executable path: {e}"))?;
    let base_dir = exe_path.parent()
        .ok_or("Cannot find executable parent directory")?;

    // The sidecar directory should be in Resources/_up_/osint_hub (Tauri 2.x) or Resources/osint_hub
    let possible_paths = [
        base_dir.join("Resources").join("_up_").join("osint_hub"),
        base_dir.join("Resources").join("osint_hub"),
    ];
    
    let sidecar_dir = possible_paths.iter()
        .find(|p| p.exists())
        .ok_or_else(|| format!("Sidecar directory not found in any expected location. Checked: {:?}", possible_paths))?;

    // Ensure Python dependencies are installed (first-run setup)
    ensure_python_deps(sidecar_dir)?;

    // Spawn the sidecar in a background thread
    let sidecar_path = sidecar_dir.clone();
    std::thread::spawn(move || {
        let mut cmd = Command::new("python3");
        cmd.args(&["-m", "uvicorn", "app.main:app", "--host", "127.0.0.1", "--port", "8790"]);
        cmd.current_dir(&sidecar_path);
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
        cmd.stdin(Stdio::null());

        log::info!("Starting osint sidecar: python3 -m uvicorn app.main:app --host 127.0.0.1 --port 8790");
        match cmd.spawn() {
            Ok(mut child) => {
                let pid = child.id();
                log::info!("Sidecar spawned with PID {}", pid);
                {
                    let mut pid_guard = SIDE_CAR_PID.lock().unwrap();
                    *pid_guard = Some(pid);
                    SIDE_CAR_RUNNING.store(true, Ordering::SeqCst);
                }
                let _ = child.wait();
                log::info!("Sidecar (PID {}) exited", pid);
                SIDE_CAR_RUNNING.store(false, Ordering::SeqCst);
                *SIDE_CAR_PID.lock().unwrap() = None;
            }
            Err(e) => {
                log::error!("Failed to spawn sidecar: {e}");
                SIDE_CAR_RUNNING.store(false, Ordering::SeqCst);
            }
        }
    });

    Ok("Sidecar starting on 127.0.0.1:8790".to_string())
}

/// Stop the osint sidecar (terminates the process).
#[tauri::command]
pub fn stop_sidecar() -> Result<String, String> {
    let pid = {
        let mut pid_guard = SIDE_CAR_PID.lock().unwrap();
        if let Some(pid_val) = *pid_guard {
            *pid_guard = None;
            SIDE_CAR_RUNNING.store(false, Ordering::SeqCst);
            pid_val
        } else {
            return Ok("Sidecar not running".to_string());
        }
    };

    // Try to kill the process
    let _ = Command::new("kill")
        .args(&["-TERM", &pid.to_string()])
        .output();

    // Give it a moment, then force kill if still alive
    std::thread::sleep(std::time::Duration::from_millis(500));
    let _ = Command::new("kill")
        .args(&["-KILL", &pid.to_string()])
        .output();

    Ok("Sidecar stopped".to_string())
}

/// Check if the sidecar is running and reachable.
#[tauri::command]
pub fn sidecar_status() -> serde_json::Value {
    let running = SIDE_CAR_RUNNING.load(Ordering::SeqCst);
    let reachable = ureq::get("http://127.0.0.1:8790/health")
        .call()
        .map(|r| r.status() == 200)
        .unwrap_or(false);
    serde_json::json!({
        "running": running,
        "reachable": reachable
    })
}
