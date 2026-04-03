use std::sync::Mutex;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_shell::ShellExt;
use tokio::io::{AsyncBufReadExt, BufReader};

mod generator;
#[allow(dead_code)]
mod geodata;
mod ssh;

struct AppState {
    /// PID процесса sing-box, запущенного root-правами через osascript
    singbox_pid: Mutex<Option<u32>>,
}

/// Находит абсолютный путь до sidecar-бинарника `sing-box`
fn resolve_singbox_path() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;

    let dir = exe.parent().ok_or("Cannot resolve binary directory")?;

    let arch_suffix = if cfg!(target_arch = "x86_64") {
        "x86_64-apple-darwin"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64-apple-darwin"
    } else {
        return Err("Unsupported architecture".to_string());
    };

    let sidecar_name = format!("sing-box-{}", arch_suffix);
    let sidecar_path = dir.join(&sidecar_name);

    if sidecar_path.exists() {
        Ok(sidecar_path.to_string_lossy().to_string())
    } else {
        Ok("sing-box".to_string())
    }
}

#[tauri::command]
async fn start_tunnel(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    {
        let guard = state.singbox_pid.lock().unwrap();
        if guard.is_some() {
            return Err("Tunnel is already running".to_string());
        }
    }

    let _ = app.emit("tunnel-log", "[SYSTEM] Resolving core binary path...");

    let singbox_path = resolve_singbox_path()?;

    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let config_path = local_data.join("client_config.json");

    if !config_path.exists() {
        return Err("Client config not found. Please deploy a server first.".to_string());
    }

    let config_str = config_path.to_string_lossy().to_string();

    let _ = app.emit(
        "tunnel-log",
        "[SYSTEM] Requesting administrator privileges...".to_string(),
    );

    let log_path = "/tmp/rkn-tun.log";
    let shell_cmd = format!(
        "{} run -c '{}' > {} 2>&1 & echo $!",
        singbox_path, config_str, log_path
    );

    let escaped_cmd = shell_cmd.replace('\\', "\\\\").replace('"', "\\\"");

    let osascript_arg = format!(
        "do shell script \"{}\" with administrator privileges",
        escaped_cmd
    );

    let sidecar = app
        .shell()
        .command("osascript")
        .args(["-e", &osascript_arg]);

    let output = sidecar
        .output()
        .await
        .map_err(|e| format!("Failed to execute osascript: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("User canceled") || stderr.contains("-128") {
            let _ = app.emit(
                "tunnel-log",
                "[SYSTEM] Administrator access was cancelled by user.",
            );
            return Err("User cancelled admin prompt".to_string());
        }
        return Err(format!("osascript error: {}", stderr));
    }

    let pid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let pid: u32 = pid_str
        .parse()
        .map_err(|_| format!("Failed to parse PID from: '{}'", pid_str))?;

    let _ = app.emit(
        "tunnel-log",
        format!("[SYSTEM] Core process started with PID {} (root)", pid),
    );

    {
        let mut guard = state.singbox_pid.lock().unwrap();
        *guard = Some(pid);
    }

    // Асинхронное чтение логов (tail -f)
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let file = match tokio::fs::File::open(log_path).await {
            Ok(f) => f,
            Err(e) => {
                let _ = app_clone.emit(
                    "tunnel-log",
                    format!("[WARN] Could not open log file: {}", e),
                );
                return;
            }
        };

        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if !line.trim().is_empty() {
                        let _ = app_clone.emit("tunnel-log", format!("[CORE] {}", line));
                    }
                }
                Ok(None) => {
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
                Err(_) => break,
            }
        }
    });

    let _ = app.emit(
        "tunnel-log",
        "[SYSTEM] TUN adapter initialized. Routing active.".to_string(),
    );

    Ok(())
}

#[tauri::command]
async fn stop_tunnel(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let pid = {
        let mut guard = state.singbox_pid.lock().unwrap();
        guard.take()
    };

    match pid {
        Some(pid) => {
            let _ = app.emit(
                "tunnel-log",
                format!("[SYSTEM] Stopping core process (PID {})...", pid),
            );

            let kill_cmd = format!("kill -9 {}", pid);
            let osascript_arg = format!(
                "do shell script \"{}\" with administrator privileges",
                kill_cmd
            );

            let output = app
                .shell()
                .command("osascript")
                .args(["-e", &osascript_arg])
                .output()
                .await
                .map_err(|e| e.to_string())?;

            if output.status.success() {
                let _ = app.emit(
                    "tunnel-log",
                    "[SYSTEM] Core process terminated. Routing disabled.".to_string(),
                );
            } else {
                let _ = app.emit(
                    "tunnel-log",
                    "[WARN] Process may have already exited.".to_string(),
                );
            }

            let _ = std::fs::remove_file("/tmp/rkn-tun.log");

            Ok(())
        }
        None => {
            let _ = app.emit(
                "tunnel-log",
                "[SYSTEM] No active tunnel to stop.".to_string(),
            );
            Ok(())
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            singbox_pid: Mutex::new(None),
        })
        .setup(|app| {
            // --- System Tray (живёт в менюбаре macOS) ---
            let show_item = MenuItemBuilder::with_id("show", "Show RKN").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

            let menu = MenuBuilder::new(app)
                .item(&show_item)
                .separator()
                .item(&quit_item)
                .build()?;

            let _tray = TrayIconBuilder::new()
                .tooltip("RKN — Recursive Kinetic Network")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        // Убиваем sing-box процесс перед выходом
                        let state = app.state::<AppState>();
                        if let Some(pid) = state.singbox_pid.lock().unwrap().take() {
                            let _ = std::process::Command::new("kill")
                                .args(["-9", &pid.to_string()])
                                .output();
                            let _ = std::fs::remove_file("/tmp/rkn-tun.log");
                        }
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        // --- Закрытие окна → скрытие (туннель продолжает работать) ---
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Не закрываем, а прячем окно
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            start_tunnel,
            stop_tunnel,
            ssh::deploy_server
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
