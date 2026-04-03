use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

struct AppState {
    singbox_child: Mutex<Option<tauri_plugin_shell::process::CommandChild>>,
}

#[tauri::command]
fn start_vpn(
    app: AppHandle,
    state: State<'_, AppState>,
    config_path: String,
) -> Result<(), String> {
    let mut child_guard = state.singbox_child.lock().unwrap();
    if child_guard.is_some() {
        return Err("VPN is already running".to_string());
    }

    let sidecar = app
        .shell()
        .sidecar("sing-box")
        .map_err(|e| e.to_string())?
        .args(["run", "-c", &config_path]);

    let (mut rx, child) = sidecar.spawn().map_err(|e| e.to_string())?;

    *child_guard = Some(child);

    // Read stdout/stderr asynchronously
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(line) => {
                    let _ =
                        app_handle.emit("tunnel-log", String::from_utf8_lossy(&line).into_owned());
                }
                CommandEvent::Stderr(line) => {
                    let _ =
                        app_handle.emit("tunnel-log", String::from_utf8_lossy(&line).into_owned());
                }
                _ => {}
            }
        }
        let _ = app_handle.emit("tunnel-log", "Process terminated.".to_string());

        // When process ends, clear the lock if we can safely do it.
        // Here we just let it be, stop_vpn will clear it. Or we can just log.
    });

    Ok(())
}

#[tauri::command]
fn stop_vpn(state: State<'_, AppState>) -> Result<(), String> {
    let mut child_guard = state.singbox_child.lock().unwrap();
    if let Some(child) = child_guard.take() {
        let _ = child.kill();
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            singbox_child: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![start_vpn, stop_vpn])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
