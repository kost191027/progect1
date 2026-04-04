use std::sync::Mutex;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_shell::ShellExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::{sleep, Duration};

mod generator;
mod geodata;
mod ssh;

struct AppState {
    /// PID процесса sing-box, запущенного root-правами через osascript
    singbox_pid: Mutex<Option<u32>>,
    network_fingerprint: Mutex<Option<String>>,
    recovery_in_progress: Mutex<bool>,
    proxy_failure_count: Mutex<u8>,
    kill_switch_engaged: Mutex<bool>,
}

fn emit_tunnel_state(app: &AppHandle, is_running: bool) {
    let _ = app.emit("tunnel-state", is_running);
}

fn emit_guard_state(app: &AppHandle, state: &str) {
    let _ = app.emit("tunnel-guard-state", state.to_string());
}

fn process_exists(pid: u32) -> bool {
    std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .output()
        .map(|output| {
            output.status.success() && !String::from_utf8_lossy(&output.stdout).trim().is_empty()
        })
        .unwrap_or(false)
}

fn escape_applescript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn run_admin_command(script: &str) -> Result<std::process::Output, String> {
    let osascript_arg = format!(
        "do shell script \"{}\" with administrator privileges",
        escape_applescript(script)
    );

    std::process::Command::new("osascript")
        .args(["-e", &osascript_arg])
        .output()
        .map_err(|e| format!("Failed to execute osascript: {}", e))
}

fn terminate_root_process(pid: u32) -> Result<(), String> {
    let kill_cmd = format!(
        "kill {} >/dev/null 2>&1 || true\nsleep 1\nkill -9 {} >/dev/null 2>&1 || true",
        pid, pid
    );

    let output = run_admin_command(&kill_cmd)?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn recent_log_tail(log_path: &str, max_lines: usize) -> String {
    let Ok(contents) = std::fs::read_to_string(log_path) else {
        return String::new();
    };

    let mut lines = contents
        .lines()
        .rev()
        .take(max_lines)
        .map(str::to_string)
        .collect::<Vec<_>>();
    lines.reverse();
    lines.join("\n")
}

fn current_network_fingerprint() -> Option<String> {
    let output = std::process::Command::new("ifconfig")
        .arg("-u")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut blocks = Vec::new();
    let mut current_header: Option<String> = None;
    let mut current_status: Option<String> = None;
    let mut current_ipv4: Option<String> = None;

    let flush_block = |blocks: &mut Vec<String>,
                       header: &mut Option<String>,
                       status: &mut Option<String>,
                       ipv4: &mut Option<String>| {
        if let Some(iface) = header.take() {
            if iface.starts_with("lo0") || iface.starts_with("utun") {
                *status = None;
                *ipv4 = None;
                return;
            }

            let status_value = status.take().unwrap_or_else(|| "unknown".to_string());
            let ipv4_value = ipv4.take().unwrap_or_else(|| "no-ipv4".to_string());
            blocks.push(format!("{}|{}|{}", iface, status_value, ipv4_value));
        }
    };

    for line in stdout.lines() {
        if !line.starts_with('\t') && line.contains(':') {
            flush_block(
                &mut blocks,
                &mut current_header,
                &mut current_status,
                &mut current_ipv4,
            );
            current_header = line
                .split(':')
                .next()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            continue;
        }

        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("status:") {
            current_status = Some(value.trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("inet ") {
            let ipv4 = value.split_whitespace().next().unwrap_or_default().trim();
            if !ipv4.is_empty() {
                current_ipv4 = Some(ipv4.to_string());
            }
        }
    }

    flush_block(
        &mut blocks,
        &mut current_header,
        &mut current_status,
        &mut current_ipv4,
    );

    if blocks.is_empty() {
        None
    } else {
        Some(blocks.join(";"))
    }
}

fn set_network_fingerprint(state: &AppState, fingerprint: Option<String>) {
    let mut guard = state.network_fingerprint.lock().unwrap();
    *guard = fingerprint;
}

fn get_network_fingerprint(state: &AppState) -> Option<String> {
    state.network_fingerprint.lock().unwrap().clone()
}

fn try_begin_recovery(state: &AppState) -> bool {
    let mut guard = state.recovery_in_progress.lock().unwrap();
    if *guard {
        false
    } else {
        *guard = true;
        true
    }
}

fn finish_recovery(state: &AppState) {
    let mut guard = state.recovery_in_progress.lock().unwrap();
    *guard = false;
}

fn reset_guard_state(state: &AppState) {
    *state.proxy_failure_count.lock().unwrap() = 0;
    *state.kill_switch_engaged.lock().unwrap() = false;
}

fn register_proxy_failure(app: &AppHandle, state: &AppState) {
    let mut failure_count = state.proxy_failure_count.lock().unwrap();
    *failure_count = failure_count.saturating_add(1);

    if *failure_count < 3 {
        return;
    }

    drop(failure_count);

    let mut engaged = state.kill_switch_engaged.lock().unwrap();
    if *engaged {
        return;
    }

    *engaged = true;
    let _ = app.emit(
        "tunnel-log",
        "[GUARD] Proxy path is degraded. Kill-switch remains engaged for non-direct traffic."
            .to_string(),
    );
    emit_guard_state(app, "engaged");
}

fn classify_proxy_failure(line: &str) -> bool {
    let lower = line.to_lowercase();

    lower.contains("outbound/vless[proxy]")
        && (lower.contains("handshake failure")
            || lower.contains("connection refused")
            || lower.contains("timed out")
            || lower.contains("timeout")
            || lower.contains("network is unreachable")
            || lower.contains("no route to host")
            || lower.contains("eof"))
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

async fn launch_tunnel_process(app: &AppHandle, announce_prompt: bool) -> Result<u32, String> {
    let singbox_path = resolve_singbox_path()?;

    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let config_path = local_data.join("client_config.json");

    if !config_path.exists() {
        return Err("Client config not found. Please deploy a server first.".to_string());
    }

    let config_str = config_path.to_string_lossy().to_string();
    let log_path = "/tmp/rkn-tun.log";

    if announce_prompt {
        let _ = app.emit(
            "tunnel-log",
            "[SYSTEM] Requesting administrator privileges...".to_string(),
        );
    }

    let shell_cmd = format!(
        "{} run -c '{}' > {} 2>&1 & echo $!",
        singbox_path, config_str, log_path
    );

    let osascript_arg = format!(
        "do shell script \"{}\" with administrator privileges",
        escape_applescript(&shell_cmd)
    );

    let output = app
        .shell()
        .command("osascript")
        .args(["-e", &osascript_arg])
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
    pid_str
        .parse()
        .map_err(|_| format!("Failed to parse PID from: '{}'", pid_str))
}

async fn verify_tunnel_start(
    app: &AppHandle,
    state: &AppState,
    pid: u32,
    log_path: &str,
) -> Result<(), String> {
    {
        let mut guard = state.singbox_pid.lock().unwrap();
        *guard = Some(pid);
    }

    sleep(Duration::from_millis(1200)).await;

    if !process_exists(pid) {
        {
            let mut guard = state.singbox_pid.lock().unwrap();
            if guard.as_ref() == Some(&pid) {
                *guard = None;
            }
        }

        set_network_fingerprint(state, None);
        emit_tunnel_state(app, false);

        let log_tail = recent_log_tail(log_path, 20);
        let details = if log_tail.is_empty() {
            "No startup logs captured.".to_string()
        } else {
            format!("Recent logs:\n{}", log_tail)
        };

        return Err(format!("Core process exited during startup. {}", details));
    }

    set_network_fingerprint(state, current_network_fingerprint());
    reset_guard_state(state);
    emit_tunnel_state(app, true);
    emit_guard_state(app, "active");

    Ok(())
}

fn spawn_log_reader(app: AppHandle, pid: u32, log_path: &'static str) {
    tauri::async_runtime::spawn(async move {
        sleep(Duration::from_millis(500)).await;

        let file = match tokio::fs::File::open(log_path).await {
            Ok(f) => f,
            Err(e) => {
                let _ = app.emit(
                    "tunnel-log",
                    format!("[WARN] Could not open log file: {}", e),
                );
                return;
            }
        };

        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        loop {
            let current_pid = {
                let state = app.state::<AppState>();
                let current_pid = *state.singbox_pid.lock().unwrap();
                current_pid
            };

            if current_pid != Some(pid) {
                break;
            }

            match lines.next_line().await {
                Ok(Some(line)) => {
                    if !line.trim().is_empty() {
                        let _ = app.emit("tunnel-log", format!("[CORE] {}", line));
                        if classify_proxy_failure(&line) {
                            let state = app.state::<AppState>();
                            register_proxy_failure(&app, &state);
                        }
                    }
                }
                Ok(None) => {
                    sleep(Duration::from_millis(300)).await;
                }
                Err(_) => break,
            }
        }
    });
}

fn spawn_process_exit_monitor(app: AppHandle, pid: u32) {
    tauri::async_runtime::spawn(async move {
        loop {
            sleep(Duration::from_secs(2)).await;

            let current_pid = {
                let state = app.state::<AppState>();
                let current_pid = *state.singbox_pid.lock().unwrap();
                current_pid
            };

            if current_pid != Some(pid) {
                break;
            }

            if !process_exists(pid) {
                {
                    let state = app.state::<AppState>();
                    let mut guard = state.singbox_pid.lock().unwrap();
                    if guard.as_ref() == Some(&pid) {
                        *guard = None;
                    }
                    set_network_fingerprint(&state, None);
                    finish_recovery(&state);
                    reset_guard_state(&state);
                }

                let _ = app.emit(
                    "tunnel-log",
                    "[SYSTEM] Core process exited. Tunnel is no longer active.".to_string(),
                );
                emit_tunnel_state(&app, false);
                emit_guard_state(&app, "inactive");
                break;
            }
        }
    });
}

fn spawn_network_recovery_monitor(app: AppHandle, pid: u32) {
    tauri::async_runtime::spawn(async move {
        loop {
            sleep(Duration::from_secs(5)).await;

            let current_pid = {
                let state = app.state::<AppState>();
                let current_pid = *state.singbox_pid.lock().unwrap();
                current_pid
            };

            if current_pid != Some(pid) {
                break;
            }

            let current_fingerprint = current_network_fingerprint();
            let fingerprint_changed = {
                let state = app.state::<AppState>();
                let previous = get_network_fingerprint(&state);
                current_fingerprint.is_some() && current_fingerprint != previous
            };

            if !fingerprint_changed {
                continue;
            }

            let state = app.state::<AppState>();
            if !try_begin_recovery(&state) {
                continue;
            }

            if let Some(fingerprint) = current_fingerprint.clone() {
                set_network_fingerprint(&state, Some(fingerprint));
            }

            let _ = app.emit(
                "tunnel-log",
                "[SYSTEM] Network change detected. Reinitializing tunnel...".to_string(),
            );

            let _ = terminate_root_process(pid);
            {
                let mut guard = state.singbox_pid.lock().unwrap();
                if guard.as_ref() == Some(&pid) {
                    *guard = None;
                }
            }

            match launch_tunnel_process(&app, false).await {
                Ok(new_pid) => {
                    let _ = app.emit(
                        "tunnel-log",
                        format!("[SYSTEM] Tunnel recovered with new PID {}.", new_pid),
                    );

                    if verify_tunnel_start(&app, &state, new_pid, "/tmp/rkn-tun.log")
                        .await
                        .is_ok()
                    {
                        spawn_log_reader(app.clone(), new_pid, "/tmp/rkn-tun.log");
                        spawn_process_exit_monitor(app.clone(), new_pid);
                        spawn_network_recovery_monitor(app.clone(), new_pid);
                    } else {
                        let _ = app.emit(
                            "tunnel-log",
                            "[WARN] Tunnel recovery failed during startup verification."
                                .to_string(),
                        );
                        emit_tunnel_state(&app, false);
                        emit_guard_state(&app, "inactive");
                    }
                }
                Err(err) => {
                    let _ = app.emit(
                        "tunnel-log",
                        format!("[WARN] Tunnel recovery failed: {}", err),
                    );
                    emit_tunnel_state(&app, false);
                    emit_guard_state(&app, "inactive");
                }
            }

            let state = app.state::<AppState>();
            finish_recovery(&state);
            break;
        }
    });
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
    let pid = launch_tunnel_process(&app, true).await?;

    let _ = app.emit(
        "tunnel-log",
        format!("[SYSTEM] Core process started with PID {} (root)", pid),
    );

    verify_tunnel_start(&app, &state, pid, "/tmp/rkn-tun.log").await?;
    spawn_log_reader(app.clone(), pid, "/tmp/rkn-tun.log");
    spawn_process_exit_monitor(app.clone(), pid);
    spawn_network_recovery_monitor(app.clone(), pid);

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

            if terminate_root_process(pid).is_ok() {
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
            set_network_fingerprint(&state, None);
            finish_recovery(&state);
            reset_guard_state(&state);
            emit_tunnel_state(&app, false);
            emit_guard_state(&app, "inactive");

            Ok(())
        }
        None => {
            let _ = app.emit(
                "tunnel-log",
                "[SYSTEM] No active tunnel to stop.".to_string(),
            );
            set_network_fingerprint(&state, None);
            finish_recovery(&state);
            reset_guard_state(&state);
            emit_tunnel_state(&app, false);
            emit_guard_state(&app, "inactive");
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
            network_fingerprint: Mutex::new(None),
            recovery_in_progress: Mutex::new(false),
            proxy_failure_count: Mutex::new(0),
            kill_switch_engaged: Mutex::new(false),
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
                            let _ = terminate_root_process(pid);
                            let _ = std::fs::remove_file("/tmp/rkn-tun.log");
                        }
                        reset_guard_state(&state);
                        emit_tunnel_state(app, false);
                        emit_guard_state(app, "inactive");
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
