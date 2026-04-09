use std::sync::Mutex;
use std::{fs, path::PathBuf};
use tauri::image::Image;
use tauri::menu::{MenuBuilder, MenuItem, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent, Wry};
use tauri_plugin_shell::ShellExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::{sleep, Duration};

mod generator;
mod geodata;
mod ssh;

const TRAY_ICON: Image<'_> = tauri::include_image!("./icons/tray-icon.png");

struct AppState {
    /// PID процесса sing-box, запущенного root-правами через osascript
    singbox_pid: Mutex<Option<u32>>,
    network_fingerprint: Mutex<Option<String>>,
    recovery_in_progress: Mutex<bool>,
    proxy_failure_count: Mutex<u8>,
    kill_switch_engaged: Mutex<bool>,
    tray_toggle_item: Mutex<Option<MenuItem<Wry>>>,
}

fn tunnel_pid_path(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    Ok(local_data.join("active_tunnel_pid"))
}

fn save_tunnel_pid(app: &AppHandle, pid: u32) -> Result<(), String> {
    let pid_path = tunnel_pid_path(app)?;

    if let Some(parent) = pid_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    fs::write(pid_path, pid.to_string()).map_err(|e| e.to_string())
}

fn load_saved_tunnel_pid(app: &AppHandle) -> Result<Option<u32>, String> {
    let pid_path = tunnel_pid_path(app)?;

    if !pid_path.exists() {
        return Ok(None);
    }

    let pid_raw = fs::read_to_string(pid_path).map_err(|e| e.to_string())?;
    let pid = pid_raw
        .trim()
        .parse::<u32>()
        .map_err(|e| format!("Failed to parse saved tunnel PID: {}", e))?;

    Ok(Some(pid))
}

fn clear_saved_tunnel_pid(app: &AppHandle) {
    if let Ok(pid_path) = tunnel_pid_path(app) {
        let _ = fs::remove_file(pid_path);
    }
}

fn emit_tunnel_state(app: &AppHandle, is_running: bool) {
    let _ = app.emit("tunnel-state", is_running);
}

fn emit_guard_state(app: &AppHandle, state: &str) {
    let _ = app.emit("tunnel-guard-state", state.to_string());
}

fn emit_screen_navigation(app: &AppHandle, screen: &str) {
    let _ = app.emit("navigate-screen", screen.to_string());
}

fn client_config_exists(app: &AppHandle) -> bool {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("client_config.json").exists())
        .unwrap_or(false)
}

pub(crate) fn refresh_tray_toggle_item(app: &AppHandle) {
    let state = app.state::<AppState>();
    let maybe_item = state.tray_toggle_item.lock().unwrap().clone();

    let Some(item) = maybe_item else {
        return;
    };

    let is_configured = client_config_exists(app);
    let is_running = state.singbox_pid.lock().unwrap().is_some();
    let label = if is_running {
        "Stop Tunnel"
    } else {
        "Start Tunnel"
    };

    let _ = item.set_text(label);
    let _ = item.set_enabled(is_configured);
}

fn show_main_window(app: &AppHandle, screen: Option<&str>) {
    if let Some(screen) = screen {
        emit_screen_navigation(app, screen);
    }

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
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

    lower.contains("outbound/shadowsocks[proxy]")
        && (lower.contains("context deadline exceeded")
            || lower.contains("connection refused")
            || lower.contains("i/o timeout")
            || lower.contains("network is unreachable")
            || lower.contains("no route to host")
            || lower.contains("connection reset"))
}

fn current_singbox_target_triple() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        (os, arch) => Err(format!(
            "Unsupported platform for sing-box sidecar resolution: {} / {}",
            os, arch
        )),
    }
}

/// Находит абсолютный путь до sidecar-бинарника `sing-box`
fn resolve_singbox_path() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("Cannot resolve binary directory")?;
    let target_triple = current_singbox_target_triple()?;

    let mut candidates = vec![
        format!("sing-box-{}", target_triple),
        "sing-box".to_string(),
    ];

    if cfg!(target_os = "windows") {
        candidates = vec![
            format!("sing-box-{}.exe", target_triple),
            "sing-box.exe".to_string(),
            format!("sing-box-{}", target_triple),
            "sing-box".to_string(),
        ];
    }

    for candidate in candidates {
        let sidecar_path = dir.join(&candidate);
        if sidecar_path.exists() {
            return Ok(sidecar_path.to_string_lossy().to_string());
        }
    }

    // Fallback: system PATH
    if cfg!(target_os = "windows") {
        Ok("sing-box.exe".to_string())
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
        "'{}' run -c '{}' > {} 2>&1 & echo $!",
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
        clear_saved_tunnel_pid(app);
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
    save_tunnel_pid(app, pid)?;
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
                    clear_saved_tunnel_pid(&app);
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
            if let Some(fingerprint) = current_fingerprint.clone() {
                set_network_fingerprint(&state, Some(fingerprint));
            }

            let _ = app.emit(
                "tunnel-log",
                "[SYSTEM] Network change detected. Keeping tunnel active and updating the network context.".to_string(),
            );
        }
    });
}

async fn start_tunnel_inner(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let guard = state.singbox_pid.lock().unwrap();
        if guard.is_some() {
            return Err("Tunnel is already running".to_string());
        }
    }

    ssh::ensure_local_transport_is_current(&app).await?;

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
    refresh_tray_toggle_item(&app);

    Ok(())
}

async fn stop_tunnel_inner(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
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
            clear_saved_tunnel_pid(&app);
            set_network_fingerprint(&state, None);
            finish_recovery(&state);
            reset_guard_state(&state);
            emit_tunnel_state(&app, false);
            emit_guard_state(&app, "inactive");
            refresh_tray_toggle_item(&app);

            Ok(())
        }
        None => {
            let _ = app.emit(
                "tunnel-log",
                "[SYSTEM] No active tunnel to stop.".to_string(),
            );
            set_network_fingerprint(&state, None);
            clear_saved_tunnel_pid(&app);
            finish_recovery(&state);
            reset_guard_state(&state);
            emit_tunnel_state(&app, false);
            emit_guard_state(&app, "inactive");
            refresh_tray_toggle_item(&app);
            Ok(())
        }
    }
}

#[tauri::command]
async fn reset_local_data(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let is_running = state.singbox_pid.lock().unwrap().is_some();

    if is_running {
        stop_tunnel_inner(app.clone()).await?;
    }

    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let files_to_remove = [
        local_data.join("client_config.json"),
        local_data.join("server_profile.json"),
        local_data.join("active_tunnel_pid"),
    ];

    for path in files_to_remove {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Failed to remove local file {}: {}",
                    path.display(),
                    error
                ));
            }
        }
    }

    let _ = fs::remove_file("/tmp/rkn-tun.log");
    set_network_fingerprint(&state, None);
    finish_recovery(&state);
    reset_guard_state(&state);
    emit_tunnel_state(&app, false);
    emit_guard_state(&app, "inactive");
    refresh_tray_toggle_item(&app);
    let _ = app.emit(
        "tunnel-log",
        "[SYSTEM] Local server profile and client config were removed from this Mac.".to_string(),
    );

    Ok(())
}

pub(crate) async fn restart_tunnel_if_running(
    app: &AppHandle,
    reason: &str,
) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let is_running = state.singbox_pid.lock().unwrap().is_some();
    if !is_running {
        return Ok(false);
    }

    let _ = app.emit("tunnel-log", format!("[SYSTEM] {}", reason));
    stop_tunnel_inner(app.clone()).await?;
    sleep(Duration::from_millis(500)).await;
    start_tunnel_inner(app.clone()).await?;

    Ok(true)
}

#[tauri::command]
async fn start_tunnel(app: AppHandle) -> Result<(), String> {
    start_tunnel_inner(app).await
}

#[tauri::command]
async fn stop_tunnel(app: AppHandle) -> Result<(), String> {
    stop_tunnel_inner(app).await
}

#[tauri::command]
async fn restore_tunnel_session(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<u32>, String> {
    {
        let current_pid = *state.singbox_pid.lock().unwrap();
        if current_pid.is_some() {
            return Ok(current_pid);
        }
    }

    let Some(saved_pid) = load_saved_tunnel_pid(&app)? else {
        emit_tunnel_state(&app, false);
        emit_guard_state(&app, "inactive");
        return Ok(None);
    };

    if !process_exists(saved_pid) {
        clear_saved_tunnel_pid(&app);
        emit_tunnel_state(&app, false);
        emit_guard_state(&app, "inactive");
        return Ok(None);
    }

    {
        let mut guard = state.singbox_pid.lock().unwrap();
        *guard = Some(saved_pid);
    }

    set_network_fingerprint(&state, current_network_fingerprint());
    reset_guard_state(&state);
    emit_tunnel_state(&app, true);
    emit_guard_state(&app, "active");
    spawn_log_reader(app.clone(), saved_pid, "/tmp/rkn-tun.log");
    spawn_process_exit_monitor(app.clone(), saved_pid);
    spawn_network_recovery_monitor(app.clone(), saved_pid);
    refresh_tray_toggle_item(&app);

    Ok(Some(saved_pid))
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
            tray_toggle_item: Mutex::new(None),
        })
        .setup(|app| {
            // --- System Tray (живёт в менюбаре macOS) ---
            let app_handle = app.app_handle().clone();
            let toggle_item = MenuItemBuilder::with_id("toggle_tunnel", "Start Tunnel")
                .enabled(client_config_exists(&app_handle))
                .build(app)?;
            let settings_item = MenuItemBuilder::with_id("open_settings", "Settings").build(app)?;
            let info_item = MenuItemBuilder::with_id("open_info", "Info").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

            {
                let state = app.state::<AppState>();
                *state.tray_toggle_item.lock().unwrap() = Some(toggle_item.clone());
            }

            let menu = MenuBuilder::new(app)
                .item(&toggle_item)
                .item(&settings_item)
                .item(&info_item)
                .separator()
                .item(&quit_item)
                .build()?;

            let _tray = TrayIconBuilder::new()
                .icon(TRAY_ICON)
                .icon_as_template(false)
                .tooltip("RKN — Recursive Kinetic Network")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "toggle_tunnel" => {
                        let app_handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let is_running = {
                                let state = app_handle.state::<AppState>();
                                let is_running = state.singbox_pid.lock().unwrap().is_some();
                                is_running
                            };

                            let result = if is_running {
                                stop_tunnel_inner(app_handle.clone()).await
                            } else {
                                start_tunnel_inner(app_handle.clone()).await
                            };

                            if let Err(error) = result {
                                let _ = app_handle.emit(
                                    "tunnel-log",
                                    format!("[ERROR] tray tunnel action failed: {}", error),
                                );
                            }
                        });
                    }
                    "open_settings" => {
                        show_main_window(app, Some("settings"));
                    }
                    "open_info" => {
                        show_main_window(app, Some("info"));
                    }
                    "quit" => {
                        // Убиваем sing-box процесс перед выходом
                        let state = app.state::<AppState>();
                        if let Some(pid) = state.singbox_pid.lock().unwrap().take() {
                            let _ = terminate_root_process(pid);
                            let _ = std::fs::remove_file("/tmp/rkn-tun.log");
                        }
                        clear_saved_tunnel_pid(app);
                        reset_guard_state(&state);
                        emit_tunnel_state(app, false);
                        emit_guard_state(app, "inactive");
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            refresh_tray_toggle_item(&app_handle);

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
            reset_local_data,
            restore_tunnel_session,
            ssh::deploy_server,
            ssh::get_transport_state_snapshot,
            ssh::load_saved_server_profile,
            ssh::check_server_status,
            ssh::rotate_sni
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let RunEvent::Reopen { .. } = event {
                show_main_window(app, None);
            }
        });
}
