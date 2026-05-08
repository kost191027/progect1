#[cfg(target_os = "android")]
use jni::objects::{JObject, JValue};
#[cfg(target_os = "android")]
use jni::JavaVM;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::io::Write;
#[cfg(any(target_os = "windows", target_os = "android"))]
use std::process::Stdio;
use std::sync::Mutex;
use std::{fs, path::PathBuf};
#[cfg(desktop)]
use tauri::image::Image;
#[cfg(desktop)]
use tauri::menu::{MenuBuilder, MenuItem, MenuItemBuilder};
#[cfg(desktop)]
use tauri::tray::TrayIconBuilder;
#[cfg(all(desktop, target_os = "windows"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
#[cfg(all(desktop, target_os = "macos"))]
use tauri::RunEvent;
use tauri::{AppHandle, Emitter, Manager, State};
#[cfg(desktop)]
use tauri::{WindowEvent, Wry};
#[cfg(not(any(target_os = "windows", target_os = "android")))]
use tauri_plugin_shell::ShellExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::{sleep, Duration};

mod generator;
mod geodata;
#[path = "ssh/mod.rs"]
mod ssh;

#[cfg(desktop)]
const TRAY_ICON: Image<'_> = tauri::include_image!("./icons/tray-icon.png");
struct AppState {
    /// PID процесса sing-box, запущенного root-правами через osascript
    singbox_pid: Mutex<Option<u32>>,
    network_fingerprint: Mutex<Option<String>>,
    recovery_in_progress: Mutex<bool>,
    proxy_failure_count: Mutex<u8>,
    proxy_failure_window_started: Mutex<Option<std::time::Instant>>,
    kill_switch_engaged: Mutex<bool>,
    #[cfg(desktop)]
    tray_toggle_item: Mutex<Option<MenuItem<Wry>>>,
    #[cfg(target_os = "windows")]
    windows_tray_notice_shown: Mutex<bool>,
}

#[cfg(target_os = "android")]
const ANDROID_NATIVE_BACKEND_SENTINEL_PID: u32 = u32::MAX - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum WindowsRuntimeMode {
    Tun,
    Compatibility,
}

#[derive(Debug, Clone, serde::Serialize)]
struct WindowsRuntimeModeStatus {
    mode: WindowsRuntimeMode,
    is_windows: bool,
    supports_compatibility_mode: bool,
}

fn tunnel_pid_path(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    Ok(local_data.join("active_tunnel_pid"))
}

#[cfg(target_os = "android")]
fn android_service_pid_path() -> Result<PathBuf, String> {
    Ok(android_files_dir_path()?.join("active_tunnel_pid"))
}

#[cfg(target_os = "windows")]
fn windows_runtime_mode_path(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    Ok(local_data.join("windows_runtime_mode.json"))
}

fn load_windows_runtime_mode(app: &AppHandle) -> Result<WindowsRuntimeMode, String> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Ok(WindowsRuntimeMode::Tun)
    }

    #[cfg(target_os = "windows")]
    {
        let mode_path = windows_runtime_mode_path(app)?;
        if !mode_path.exists() {
            return Ok(WindowsRuntimeMode::Tun);
        }

        let raw = fs::read_to_string(&mode_path).map_err(|e| {
            format!(
                "Failed to read Windows runtime mode {}: {}",
                mode_path.display(),
                e
            )
        })?;

        serde_json::from_str::<WindowsRuntimeMode>(&raw).map_err(|e| {
            format!(
                "Failed to parse Windows runtime mode {}: {}",
                mode_path.display(),
                e
            )
        })
    }
}

fn save_windows_runtime_mode(app: &AppHandle, mode: WindowsRuntimeMode) -> Result<(), String> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, mode);
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        let mode_path = windows_runtime_mode_path(app)?;

        if let Some(parent) = mode_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let payload = serde_json::to_string_pretty(&mode).map_err(|e| e.to_string())?;
        fs::write(&mode_path, payload).map_err(|e| {
            format!(
                "Failed to save Windows runtime mode {}: {}",
                mode_path.display(),
                e
            )
        })
    }
}

fn save_tunnel_pid(app: &AppHandle, pid: u32) -> Result<(), String> {
    let pid_path = tunnel_pid_path(app)?;

    if let Some(parent) = pid_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    fs::write(&pid_path, pid.to_string()).map_err(|e| e.to_string())?;

    #[cfg(target_os = "android")]
    {
        let service_pid_path = android_service_pid_path()?;
        if let Some(parent) = service_pid_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(service_pid_path, pid.to_string()).map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn load_saved_tunnel_pid(app: &AppHandle) -> Result<Option<u32>, String> {
    let pid_path = tunnel_pid_path(app)?;

    if !pid_path.exists() {
        #[cfg(target_os = "android")]
        {
            let service_pid_path = android_service_pid_path()?;
            if !service_pid_path.exists() {
                return Ok(None);
            }

            let pid_raw = fs::read_to_string(service_pid_path).map_err(|e| e.to_string())?;
            let pid = pid_raw
                .trim()
                .parse::<u32>()
                .map_err(|e| format!("Failed to parse saved tunnel PID: {}", e))?;

            return Ok(Some(pid));
        }

        #[cfg(not(target_os = "android"))]
        {
            return Ok(None);
        }
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

    #[cfg(target_os = "android")]
    if let Ok(pid_path) = android_service_pid_path() {
        let _ = fs::remove_file(pid_path);
    }
}

fn emit_tunnel_state(app: &AppHandle, is_running: bool) {
    let _ = app.emit("tunnel-state", is_running);
}

fn emit_guard_state(app: &AppHandle, state: &str) {
    let _ = app.emit("tunnel-guard-state", state.to_string());
}

#[cfg(desktop)]
fn emit_screen_navigation(app: &AppHandle, screen: &str) {
    let _ = app.emit("navigate-screen", screen.to_string());
}

#[cfg(desktop)]
fn client_config_exists(app: &AppHandle) -> bool {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("client_config.json").exists())
        .unwrap_or(false)
}

fn local_client_config_requires_refresh(app: &AppHandle) -> Result<bool, String> {
    let config_path = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("client_config.json");

    if !config_path.exists() {
        return Ok(false);
    }

    let contents = fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
    let config: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|e| format!("Invalid client config JSON: {}", e))?;

    let dns = config.get("dns").and_then(|value| value.as_object());
    if let Some(dns) = dns {
        if dns.contains_key("fakeip") {
            return Ok(true);
        }

        let has_legacy_server_format = dns
            .get("servers")
            .and_then(|value| value.as_array())
            .map(|servers| {
                servers.iter().any(|server| {
                    server
                        .as_object()
                        .map(|server| !server.contains_key("type"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);

        if has_legacy_server_format {
            return Ok(true);
        }
    }

    Ok(false)
}

#[cfg(desktop)]
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

#[cfg(not(desktop))]
pub(crate) fn refresh_tray_toggle_item(_app: &AppHandle) {}

#[cfg(desktop)]
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

#[cfg(desktop)]
fn quit_application(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Some(pid) = state.singbox_pid.lock().unwrap().take() {
        let _ = terminate_root_process(None, pid);
        let _ = std::fs::remove_file(tunnel_log_path());
    }

    clear_saved_tunnel_pid(app);
    set_network_fingerprint(&state, None);
    finish_recovery(&state);
    reset_guard_state(&state);
    emit_tunnel_state(app, false);
    emit_guard_state(app, "inactive");
    app.exit(0);
}

#[cfg(target_os = "windows")]
fn maybe_announce_windows_tray_behavior(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut shown = state.windows_tray_notice_shown.lock().unwrap();
    if *shown {
        return;
    }

    *shown = true;
    let _ = app.emit(
        "tunnel-log",
        "[SYSTEM] Window hidden to the tray. RKN is still running in the background. Use the tray icon to reopen it or choose Quit there to exit completely.".to_string(),
    );
}

/// Create a `Command` that will not flash a console window on Windows.
///
/// GUI applications on Windows inherit no console, so spawning a console
/// subsystem process (powershell, tasklist, ipconfig …) via the default
/// `Command` allocates a brand-new visible console window every time.
/// Passing `CREATE_NO_WINDOW` (0x08000000) suppresses this.
#[cfg(target_os = "windows")]
fn windowless_command(program: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let mut cmd = std::process::Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

fn process_exists(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        let output = windowless_command("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output();
        output
            .map(|o| {
                o.status.success() && String::from_utf8_lossy(&o.stdout).contains(&pid.to_string())
            })
            .unwrap_or(false)
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "pid="])
            .output()
            .map(|output| {
                output.status.success()
                    && !String::from_utf8_lossy(&output.stdout).trim().is_empty()
            })
            .unwrap_or(false)
    }
}

#[cfg(not(any(target_os = "windows", target_os = "android")))]
fn escape_applescript(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\r' => {}
            _ => escaped.push(ch),
        }
    }

    escaped
}

#[cfg(not(any(target_os = "windows", target_os = "android")))]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

#[cfg(not(any(target_os = "windows", target_os = "android")))]
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

#[allow(unused_variables)]
fn terminate_root_process(app: Option<&AppHandle>, pid: u32) -> Result<(), String> {
    if !process_exists(pid) {
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let local_app_dir = app
            .and_then(|a| a.path().app_local_data_dir().ok())
            .unwrap_or_else(|| {
                // Best effort fallback
                let mut fallback =
                    std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default());
                fallback.push("com.freedom.rkn");
                fallback
            });

        let signal_path = local_app_dir.join("elevated_singbox.stop");
        let _ = std::fs::write(&signal_path, "stop");

        // Wait a bit for the supervisor to act
        std::thread::sleep(std::time::Duration::from_millis(600));

        // 3. Fallback: try taskkill (might fail due to permissions, but the stop signal or parent process exit should work)
        let output = windowless_command("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output()
            .map_err(|e| format!("Failed to execute taskkill: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            // Ignore taskkill errors if the process is already gone
            if !process_exists(pid) {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(target_os = "android")]
        {
            let output = std::process::Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .output()
                .map_err(|e| format!("Failed to execute Android kill: {}", e))?;

            if output.status.success() || !process_exists(pid) {
                return Ok(());
            }

            let force_output = std::process::Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .output()
                .map_err(|e| format!("Failed to execute Android force kill: {}", e))?;

            if force_output.status.success() || !process_exists(pid) {
                return Ok(());
            }

            return Err(String::from_utf8_lossy(&force_output.stderr)
                .trim()
                .to_string());
        }

        #[cfg(not(target_os = "android"))]
        {
            let _ = std::process::Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .output();
            std::thread::sleep(std::time::Duration::from_millis(300));
            if !process_exists(pid) {
                return Ok(());
            }
        }

        #[cfg(not(target_os = "android"))]
        let kill_cmd = format!(
            "kill {} >/dev/null 2>&1 || true\nsleep 1\nkill -9 {} >/dev/null 2>&1 || true",
            pid, pid
        );

        #[cfg(not(target_os = "android"))]
        let output = run_admin_command(&kill_cmd)?;

        #[cfg(not(target_os = "android"))]
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }
}

#[cfg(target_os = "windows")]
fn clear_windows_system_proxy() -> Result<(), String> {
    let script = r#"
      $ErrorActionPreference = 'Stop'
      $path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
      Set-ItemProperty -Path $path -Name ProxyEnable -Value 0
      Remove-ItemProperty -Path $path -Name ProxyServer -ErrorAction SilentlyContinue
      Remove-ItemProperty -Path $path -Name ProxyOverride -ErrorAction SilentlyContinue

      try {
        $signature = '[DllImport("wininet.dll")] public static extern bool InternetSetOption(int hInternet, int dwOption, IntPtr lpBuffer, int dwBufferLength);'
        Add-Type -MemberDefinition $signature -Name WinINet -Namespace RknProxy -ErrorAction SilentlyContinue | Out-Null
        [RknProxy.WinINet]::InternetSetOption(0, 39, [IntPtr]::Zero, 0) | Out-Null
        [RknProxy.WinINet]::InternetSetOption(0, 37, [IntPtr]::Zero, 0) | Out-Null
      } catch {}
    "#;

    let output = windowless_command("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
        .map_err(|e| format!("Failed to clear Windows system proxy: {}", e))?;

    let _ = windowless_command("netsh")
        .args(["winhttp", "reset", "proxy"])
        .output();

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn tunnel_log_path() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        // Use the current user's temp dir (e.g. C:\Users\<user>\AppData\Local\Temp)
        // instead of C:\Windows\Temp. Files created by an elevated process under
        // C:\Windows\Temp get restrictive ACLs that prevent the non-elevated app
        // from reading them back (Access Denied / os error 5).
        // The user's %TEMP% is readable by both elevated and non-elevated processes
        // of the same user.
        static LOG_PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        LOG_PATH.get_or_init(|| {
            std::env::temp_dir()
                .join("rkn-tun.log")
                .to_string_lossy()
                .into_owned()
        })
    }

    #[cfg(target_os = "android")]
    {
        static LOG_PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        LOG_PATH.get_or_init(|| {
            android_files_dir_path()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("rkn-tun.log")
                .to_string_lossy()
                .into_owned()
        })
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "android")))]
    {
        "/tmp/rkn-tun.log"
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

#[cfg(target_os = "android")]
fn is_android_native_backend_pid(pid: u32) -> bool {
    pid == ANDROID_NATIVE_BACKEND_SENTINEL_PID
}

#[cfg(not(target_os = "android"))]
fn is_android_native_backend_pid(_pid: u32) -> bool {
    false
}

#[cfg(target_os = "android")]
fn classify_android_startup_blocker(log_tail: &str) -> Option<String> {
    let lower = log_tail.to_lowercase();

    if lower.contains("open /dev/tun: permission denied") {
        return Some(
            "Android core reached the real TUN startup path, but the current mobile runtime still lacks the platform-specific handoff from the VpnService-owned TUN interface into a sing-box-supported Android runtime path. The standalone CLI sidecar cannot open /dev/tun directly inside the app process."
                .to_string(),
        );
    }

    if lower.contains("netlink socket in android is banned by google") {
        return Some(
            "Android runtime still tried to initialize a banned netlink-based network monitor. This mobile config must stay Android-specific and avoid desktop route monitoring fields."
                .to_string(),
        );
    }

    None
}

#[cfg(target_os = "windows")]
fn trim_utf8_bom(value: &str) -> &str {
    value.strip_prefix('\u{feff}').unwrap_or(value)
}

#[cfg(target_os = "android")]
const ANDROID_PROXY_FALLBACK_MODE: bool = false;

#[cfg(target_os = "android")]
fn build_android_runtime_client_config(raw_config: &str, log_path: &str) -> Result<String, String> {
    if ANDROID_PROXY_FALLBACK_MODE {
        return build_android_proxy_runtime_client_config(raw_config, log_path);
    }

    let mut cfg = serde_json::from_str::<serde_json::Value>(raw_config).map_err(|e| {
        format!(
            "Failed to parse generated client config for Android runtime: {}",
            e
        )
    })?;
    let server_ip = extract_server_ip_from_config(&cfg).ok_or_else(|| {
        "Android runtime config could not determine the upstream server IP from the generated client config.".to_string()
    })?;

    cfg["log"]["output"] = serde_json::json!(log_path);
    cfg["log"]["level"] = serde_json::json!("warn");
    cfg["log"]["timestamp"] = serde_json::json!(true);

    if let Some(inbounds) = cfg
        .get_mut("inbounds")
        .and_then(|value| value.as_array_mut())
    {
        for inbound in inbounds {
            let is_tun = inbound
                .get("type")
                .and_then(|value| value.as_str())
                .map(|value| value == "tun")
                .unwrap_or(false);

            if !is_tun {
                continue;
            }

            if let Some(object) = inbound.as_object_mut() {
                object.remove("interface_name");
                object.remove("gso");
                // Newer libbox/sing-box rejects legacy sniff fields on inbounds.
                // Android relies on route rule actions for sniffing instead.
                object.remove("sniff");
                object.remove("sniff_override_destination");
                // Android's VpnService-backed TUN does not implement strict_route,
                // and the system stack is what keeps emitting forbidden
                // "bind forwarder to interface" warnings for our current smoke path.
                // Use the fully userspace gVisor stack here to avoid privileged
                // interface-bound forwarders while we stabilize the mobile tunnel.
                object.insert("strict_route".to_string(), serde_json::json!(false));
                object.insert("stack".to_string(), serde_json::json!("gvisor"));
                object.insert("mtu".to_string(), serde_json::json!(1280));
            }
        }
    }

    if let Some(outbounds) = cfg
        .get_mut("outbounds")
        .and_then(|value| value.as_array_mut())
    {
        for outbound in outbounds {
            let outbound_type = outbound
                .get("type")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let outbound_tag = outbound
                .get("tag")
                .and_then(|value| value.as_str())
                .unwrap_or_default();

            if outbound_type == "shadowsocks" && outbound_tag == "proxy" {
                if let Some(object) = outbound.as_object_mut() {
                    object.remove("multiplex");
                }
            }
        }
    }

    if let Some(route) = cfg.get_mut("route").and_then(|value| value.as_object_mut()) {
        route.insert(
            "auto_detect_interface".to_string(),
            serde_json::json!(false),
        );
        route.remove("default_interface");
        route.remove("override_android_vpn");
        route.remove("default_network_strategy");
        route.remove("network_strategy");
        route.insert(
            "default_domain_resolver".to_string(),
            serde_json::json!("remote-dns"),
        );

        let mut direct_rule_set_tags = Vec::<String>::new();
        let mut google_rule_set_available = false;

        if let Some(rule_sets) = route.get("rule_set").and_then(|value| value.as_array()) {
            for rule_set in rule_sets {
                let tag = rule_set
                    .get("tag")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let rule_type = rule_set
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let path = rule_set
                    .get("path")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();

                if rule_type != "local" {
                    return Err(format!(
                        "Android runtime config requires local rule-set entries, but found type='{}' in route.rule_set.",
                        rule_type
                    ));
                }

                if path.is_empty() || !std::path::Path::new(path).is_absolute() {
                    return Err(format!(
                        "Android runtime config requires absolute rule-set paths, but found '{}'.",
                        path
                    ));
                }

                validate_android_rule_set_file(path)?;

                if tag == crate::geodata::GOOGLE_RULE_SET_TAG {
                    google_rule_set_available = true;
                }

                if crate::geodata::DIRECT_ROUTE_RULE_SET_TAGS.contains(&tag) {
                    direct_rule_set_tags.push(tag.to_string());
                }
            }
        }

        let mut route_rules = vec![
            serde_json::json!({
                "inbound": "tun-in",
                "action": "sniff",
                "timeout": "1s"
            }),
            serde_json::json!({
                "inbound": "tun-in",
                "protocol": "dns",
                "action": "hijack-dns"
            }),
            serde_json::json!({
                "ip_cidr": ["172.19.0.2/32"],
                "port": 53,
                "action": "hijack-dns"
            }),
            serde_json::json!({
                "network": "udp",
                "port": 443,
                "action": "reject",
                "method": "default"
            }),
        ];

        if google_rule_set_available {
            route_rules.push(serde_json::json!({
                "rule_set": [crate::geodata::GOOGLE_RULE_SET_TAG],
                "action": "route",
                "outbound": "proxy"
            }));
        }

        route_rules.extend([
            serde_json::json!({
                "domain_suffix": crate::geodata::PROXY_PRIORITY_DOMAIN_SUFFIXES,
                "action": "route",
                "outbound": "proxy"
            }),
            serde_json::json!({
                "ip_cidr": [format!("{}/32", server_ip)],
                "action": "route",
                "outbound": "direct"
            }),
            serde_json::json!({
                "domain_suffix": crate::geodata::CURATED_RU_DOMAIN_SUFFIXES,
                "action": "route",
                "outbound": "direct"
            }),
        ]);

        if !direct_rule_set_tags.is_empty() {
            route_rules.push(serde_json::json!({
                "rule_set": direct_rule_set_tags,
                "action": "route",
                "outbound": "direct"
            }));
        }

        route.insert("rules".to_string(), serde_json::json!(route_rules));
        route.insert("final".to_string(), serde_json::json!("proxy"));
    }

    if let Some(dns) = cfg.get_mut("dns").and_then(|value| value.as_object_mut()) {
        dns.insert(
            "servers".to_string(),
            serde_json::json!([
                {
                    "type": "tcp",
                    "tag": "remote-dns",
                    "server": "8.8.8.8",
                    "server_port": 53,
                    "detour": "proxy"
                },
                {
                    "type": "local",
                    "tag": "local-dns",
                    "prefer_go": true
                }
            ]),
        );
        let mut dns_rules = vec![
            serde_json::json!({
                "domain_suffix": crate::geodata::PROXY_PRIORITY_DOMAIN_SUFFIXES,
                "server": "remote-dns"
            }),
            serde_json::json!({
                "domain_suffix": crate::geodata::CURATED_RU_DOMAIN_SUFFIXES,
                "server": "local-dns"
            }),
        ];

        let route_rule_sets = cfg
            .get("route")
            .and_then(|value| value.get("rule_set"))
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let mut direct_dns_rule_set_tags = Vec::<String>::new();
        let mut google_rule_set_available = false;
        for rule_set in route_rule_sets {
            let tag = rule_set
                .get("tag")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if tag == crate::geodata::GOOGLE_RULE_SET_TAG {
                google_rule_set_available = true;
            }
            if crate::geodata::DIRECT_ROUTE_RULE_SET_TAGS.contains(&tag)
                && !tag.starts_with("geoip-")
            {
                direct_dns_rule_set_tags.push(tag.to_string());
            }
        }

        if google_rule_set_available {
            dns_rules.insert(
                0,
                serde_json::json!({
                    "rule_set": [crate::geodata::GOOGLE_RULE_SET_TAG],
                    "server": "remote-dns"
                }),
            );
        }

        if !direct_dns_rule_set_tags.is_empty() {
            dns_rules.push(serde_json::json!({
                "rule_set": direct_dns_rule_set_tags,
                "server": "local-dns"
            }));
        }

        dns.insert("rules".to_string(), serde_json::json!(dns_rules));
        dns.insert("final".to_string(), serde_json::json!("remote-dns"));
        dns.insert("strategy".to_string(), serde_json::json!("ipv4_only"));
    }

    serde_json::to_string_pretty(&cfg)
        .map_err(|e| format!("Failed to serialize Android runtime client config: {}", e))
}

#[cfg(target_os = "android")]
fn build_android_proxy_runtime_client_config(
    raw_config: &str,
    log_path: &str,
) -> Result<String, String> {
    let mut cfg = serde_json::from_str::<serde_json::Value>(raw_config).map_err(|e| {
        format!(
            "Failed to parse generated client config for Android proxy fallback runtime: {}",
            e
        )
    })?;
    let server_ip = extract_server_ip_from_config(&cfg).ok_or_else(|| {
        "Android proxy fallback config could not determine the upstream server IP from the generated client config.".to_string()
    })?;

    cfg["log"]["output"] = serde_json::json!(log_path);
    cfg["log"]["level"] = serde_json::json!("info");
    cfg["log"]["timestamp"] = serde_json::json!(true);

    cfg["inbounds"] = serde_json::json!([
        {
            "type": "mixed",
            "tag": "mixed-in",
            "listen": "127.0.0.1",
            "listen_port": 2080,
            "set_system_proxy": true
        }
    ]);

    if let Some(outbounds) = cfg
        .get_mut("outbounds")
        .and_then(|value| value.as_array_mut())
    {
        for outbound in outbounds {
            let outbound_type = outbound
                .get("type")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let outbound_tag = outbound
                .get("tag")
                .and_then(|value| value.as_str())
                .unwrap_or_default();

            if outbound_type == "shadowsocks" && outbound_tag == "proxy" {
                if let Some(object) = outbound.as_object_mut() {
                    object.remove("udp_over_tcp");
                }
            }
        }
    }

    if let Some(route) = cfg.get_mut("route").and_then(|value| value.as_object_mut()) {
        route.insert(
            "auto_detect_interface".to_string(),
            serde_json::json!(false),
        );
        route.remove("default_interface");
        route.remove("override_android_vpn");
        route.remove("default_network_strategy");
        route.remove("network_strategy");
        route.insert(
            "default_domain_resolver".to_string(),
            serde_json::json!("remote-dns"),
        );
        route.insert(
            "rules".to_string(),
            serde_json::json!([
                {
                    "ip_cidr": [format!("{}/32", server_ip)],
                    "action": "route",
                    "outbound": "direct"
                }
            ]),
        );
        route.remove("rule_set");
        route.insert("final".to_string(), serde_json::json!("proxy"));
    }

    if let Some(dns) = cfg.get_mut("dns").and_then(|value| value.as_object_mut()) {
        dns.insert(
            "servers".to_string(),
            serde_json::json!([
                {
                    "type": "https",
                    "tag": "remote-dns",
                    "server": "8.8.8.8",
                    "server_port": 443,
                    "path": "/dns-query",
                    "detour": "proxy",
                    "tls": {
                        "enabled": true,
                        "server_name": "dns.google"
                    }
                },
                {
                    "type": "local",
                    "tag": "local-dns",
                    "prefer_go": true
                }
            ]),
        );
        dns.insert("rules".to_string(), serde_json::json!([]));
        dns.insert("final".to_string(), serde_json::json!("remote-dns"));
        dns.insert("strategy".to_string(), serde_json::json!("ipv4_only"));
    }

    serde_json::to_string_pretty(&cfg).map_err(|e| {
        format!(
            "Failed to serialize Android proxy fallback runtime client config: {}",
            e
        )
    })
}

#[cfg(target_os = "android")]
fn validate_android_rule_set_file(path: &str) -> Result<(), String> {
    let mut magic = [0_u8; 4];
    let mut file = std::fs::File::open(path).map_err(|e| {
        format!(
            "Android runtime config could not open local rule-set '{}': {}",
            path, e
        )
    })?;

    use std::io::Read;
    file.read_exact(&mut magic).map_err(|e| {
        format!(
            "Android runtime config could not read the SRS header from '{}': {}",
            path, e
        )
    })?;

    if &magic[..3] != b"SRS" {
        return Err(format!(
            "Android runtime config requires valid .srs rule-set files, but '{}' does not start with the expected SRS header.",
            path
        ));
    }

    Ok(())
}

#[cfg(target_os = "android")]
fn build_android_handoff_backend_config(runtime_config: &str) -> Result<String, String> {
    let mut cfg = serde_json::from_str::<serde_json::Value>(runtime_config).map_err(|e| {
        format!(
            "Failed to parse Android runtime config while building handoff backend payload: {}",
            e
        )
    })?;

    if let Some(inbounds) = cfg
        .get_mut("inbounds")
        .and_then(|value| value.as_array_mut())
    {
        inbounds.retain(|inbound| {
            inbound
                .get("type")
                .and_then(|value| value.as_str())
                .map(|value| value != "tun")
                .unwrap_or(true)
        });
    }

    serde_json::to_string_pretty(&cfg).map_err(|e| {
        format!(
            "Failed to serialize Android handoff backend config payload: {}",
            e
        )
    })
}

#[cfg(target_os = "android")]
enum AndroidRuntimeLaunchPlan {
    TunHandoffRequired {
        tun_fd: i32,
        config_path: String,
        log_path: String,
    },
    ProxyOnly {
        config_path: String,
        log_path: String,
    },
}

#[cfg(target_os = "android")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AndroidRuntimeContextSnapshot {
    backend_hint: String,
    session_id: String,
    tun_fd: i32,
    tun_state: String,
    tun_address: String,
    tun_prefix_length: i32,
    tun_route: String,
    tun_mtu: i32,
    config_path: String,
    backend_config_path: String,
    log_path: String,
    protect_api_available: bool,
    backend_session_state: String,
    backend_session_id: String,
    backend_session_context_path: String,
    backend_session_config_path: String,
    backend_session_log_path: String,
    consumer_tag: String,
    consumer_claim_state: String,
    consumer_claim_path: String,
    consumer_launch_state: String,
    consumer_launch_path: String,
    consumer_launch_runtime: String,
    consumer_launch_selection: String,
    consumer_launch_summary: String,
    consumer_session_dir: String,
    tun_fd_ownership: String,
}

#[cfg(target_os = "android")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AndroidBackendConsumerClaimSnapshot {
    session_id: String,
    consumer_tag: String,
    claim_state: String,
    tun_fd: i32,
    tun_state: String,
    tun_address: String,
    tun_prefix_length: i32,
    tun_route: String,
    tun_mtu: i32,
    context_path: String,
    backend_config_path: String,
    log_path: String,
}

#[cfg(target_os = "android")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AndroidNativeBackendLaunchSnapshot {
    session_id: String,
    consumer_tag: String,
    launch_state: String,
    detail: String,
    claim_path: String,
    launch_bundle_path: String,
    status_path: String,
    runtime_name: String,
    runtime_selection: String,
    backend_config_summary: String,
}

#[cfg(target_os = "android")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AndroidNativeBackendLaunchBundle {
    session_id: String,
    consumer_tag: String,
    backend_hint: String,
    tun_fd: i32,
    tun_state: String,
    tun_address: String,
    tun_prefix_length: i32,
    tun_route: String,
    tun_mtu: i32,
    config_path: String,
    backend_config_path: String,
    context_path: String,
    claim_path: String,
    log_path: String,
    session_dir: String,
    runtime_log_path: String,
    runtime_status_path: String,
    tun_fd_ownership: String,
    protect_api_available: bool,
}

#[cfg(target_os = "android")]
fn load_android_native_backend_launch_status(
    path: &str,
) -> Result<Option<AndroidNativeBackendLaunchSnapshot>, String> {
    let path = std::path::Path::new(path);
    if !path.exists() {
        return Ok(None);
    }

    let payload = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "Failed to read Android native backend status {}: {}",
            path.display(),
            e
        )
    })?;
    let value = serde_json::from_str::<serde_json::Value>(&payload).map_err(|e| {
        format!(
            "Failed to parse Android native backend status {}: {}",
            path.display(),
            e
        )
    })?;

    Ok(Some(AndroidNativeBackendLaunchSnapshot {
        session_id: value
            .get("session_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        consumer_tag: value
            .get("consumer_tag")
            .and_then(|value| value.as_str())
            .unwrap_or("rkn_android_native_backend_seam")
            .to_string(),
        launch_state: value
            .get("launch_state")
            .and_then(|value| value.as_str())
            .or_else(|| value.get("phase").and_then(|value| value.as_str()))
            .unwrap_or("unknown")
            .to_string(),
        detail: value
            .get("detail")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        claim_path: value
            .get("claim_path")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        launch_bundle_path: value
            .get("launch_bundle_path")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        status_path: value
            .get("status_path")
            .and_then(|value| value.as_str())
            .unwrap_or(path.to_string_lossy().as_ref())
            .to_string(),
        runtime_name: value
            .get("runtime_name")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
            .to_string(),
        runtime_selection: value
            .get("runtime_selection")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        backend_config_summary: value
            .get("backend_config_summary")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
    }))
}

fn extract_server_ip_from_config(cfg: &serde_json::Value) -> Option<String> {
    let outbounds = cfg.get("outbounds")?.as_array()?;
    for outbound in outbounds {
        let outbound_type = outbound.get("type")?.as_str()?;
        // The ShadowTLS outbound has the real server IP
        if outbound_type == "shadowtls" || outbound_type == "shadowsocks" {
            if let Some(server) = outbound.get("server").and_then(|v| v.as_str()) {
                // Only return actual IPs, not hostnames
                if server.parse::<std::net::Ipv4Addr>().is_ok() {
                    return Some(server.to_string());
                }
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn macos_server_route_prelude(server_ip: Option<&str>) -> String {
    let Some(server_ip) = server_ip else {
        return String::new();
    };

    format!(
        "SERVER_IP={}; \
         PHYSICAL_ROUTE=$(netstat -rn -f inet | awk '$1==\"default\" && $2 !~ /^link#/ && $4 !~ /^utun/ {{print $2\" \"$4; exit}}'); \
         PHYSICAL_GW=${{PHYSICAL_ROUTE%% *}}; \
         if [ -n \"$PHYSICAL_GW\" ] && [ \"$PHYSICAL_GW\" != \"$PHYSICAL_ROUTE\" ]; then \
           /sbin/route -n delete -host \"$SERVER_IP\" >/dev/null 2>&1 || true; \
           /sbin/route -n add -host \"$SERVER_IP\" \"$PHYSICAL_GW\" >/dev/null 2>&1 || true; \
         fi; ",
        shell_single_quote(server_ip)
    )
}

#[cfg(not(target_os = "macos"))]
fn macos_server_route_prelude(_server_ip: Option<&str>) -> String {
    String::new()
}

#[cfg(not(target_os = "android"))]
fn build_desktop_runtime_client_config(raw_config: &str) -> Result<String, String> {
    let mut cfg = serde_json::from_str::<serde_json::Value>(raw_config).map_err(|e| {
        format!(
            "Failed to parse generated client config for desktop runtime: {}",
            e
        )
    })?;

    cfg["log"]["level"] = serde_json::json!("warn");
    cfg["log"]["timestamp"] = serde_json::json!(true);

    if let Some(inbounds) = cfg
        .get_mut("inbounds")
        .and_then(|value| value.as_array_mut())
    {
        for inbound in inbounds {
            let is_tun = inbound
                .get("type")
                .and_then(|value| value.as_str())
                .map(|value| value == "tun")
                .unwrap_or(false);

            if !is_tun {
                continue;
            }

            if let Some(object) = inbound.as_object_mut() {
                object.remove("sniff");
                object.remove("sniff_override_destination");
            }
        }
    }

    if let Some(outbounds) = cfg
        .get_mut("outbounds")
        .and_then(|value| value.as_array_mut())
    {
        for outbound in outbounds {
            let outbound_type = outbound
                .get("type")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let outbound_tag = outbound
                .get("tag")
                .and_then(|value| value.as_str())
                .unwrap_or_default();

            if outbound_type == "shadowsocks" && outbound_tag == "proxy" {
                if let Some(object) = outbound.as_object_mut() {
                    object.remove("multiplex");
                }
            }
        }
    }

    serde_json::to_string_pretty(&cfg)
        .map_err(|e| format!("Failed to serialize desktop runtime client config: {}", e))
}

#[cfg(target_os = "windows")]
fn build_windows_runtime_client_config(
    raw_config: &str,
    log_path: &str,
    mode: WindowsRuntimeMode,
) -> Result<String, String> {
    let mut cfg = serde_json::from_str::<serde_json::Value>(raw_config).map_err(|e| {
        format!(
            "Failed to parse generated client config for Windows runtime: {}",
            e
        )
    })?;

    cfg["log"]["output"] = serde_json::json!(log_path);
    cfg["log"]["level"] = serde_json::json!("info");
    cfg["log"]["timestamp"] = serde_json::json!(true);

    // Extract server IP before mutably borrowing inbounds
    let server_ip = extract_server_ip_from_config(&cfg);

    match mode {
        WindowsRuntimeMode::Tun => {
            if let Some(inbounds) = cfg
                .get_mut("inbounds")
                .and_then(|value| value.as_array_mut())
            {
                for inbound in inbounds {
                    let is_tun = inbound
                        .get("type")
                        .and_then(|value| value.as_str())
                        .map(|value| value == "tun")
                        .unwrap_or(false);

                    if !is_tun {
                        continue;
                    }

                    if let Some(object) = inbound.as_object_mut() {
                        // Windows TUN profile:
                        // - use a dedicated Windows TUN subnet
                        // - do not force a fixed adapter name
                        // - strict_route is relaxed for older Windows installs
                        // - use the native system stack on Windows
                        object.insert("address".to_string(), serde_json::json!(["172.18.0.1/30"]));
                        object.remove("interface_name");
                        object.insert("strict_route".to_string(), serde_json::json!(false));
                        object.insert("stack".to_string(), serde_json::json!("system"));
                    }
                }
            }

            if let Some(route) = cfg.get_mut("route").and_then(|value| value.as_object_mut()) {
                // auto_detect_interface MUST stay true so that the "direct"
                // outbound binds to the physical adapter instead of the TUN.
                route.insert("auto_detect_interface".to_string(), serde_json::json!(true));
            }

            // Windows DNS fix for TUN mode: route A/AAAA queries to fakeip,
            // keep everything else on local DNS, and avoid unsupported
            // default_domain_resolver fields in this runtime config.
            if let Some(dns) = cfg.get_mut("dns").and_then(|v| v.as_object_mut()) {
                if let Some(rules) = dns.get_mut("rules").and_then(|v| v.as_array_mut()) {
                    rules.push(serde_json::json!({
                        "query_type": ["A", "AAAA"],
                        "server": "fakeip-dns"
                    }));
                }
                dns.insert("final".to_string(), serde_json::json!("local-dns"));
            }
        }
        WindowsRuntimeMode::Compatibility => {
            cfg["inbounds"] = serde_json::json!([
                {
                    "type": "mixed",
                    "tag": "mixed-in",
                    "listen": "127.0.0.1",
                    "listen_port": 2080,
                    "set_system_proxy": true
                }
            ]);

            if let Some(dns) = cfg.get_mut("dns").and_then(|value| value.as_object_mut()) {
                if let Some(servers) = dns
                    .get_mut("servers")
                    .and_then(|value| value.as_array_mut())
                {
                    servers.retain(|server| {
                        server
                            .get("tag")
                            .and_then(|value| value.as_str())
                            .map(|tag| tag != "fakeip-dns")
                            .unwrap_or(true)
                    });

                    let mut remote_dns_found = false;
                    for server in servers.iter_mut() {
                        let tag = server
                            .get("tag")
                            .and_then(|value| value.as_str())
                            .unwrap_or_default();

                        if tag == "remote-dns" {
                            remote_dns_found = true;
                            if let Some(object) = server.as_object_mut() {
                                object.insert("type".to_string(), serde_json::json!("https"));
                                object.insert("server".to_string(), serde_json::json!("8.8.8.8"));
                                object.insert("server_port".to_string(), serde_json::json!(443));
                                object.insert("path".to_string(), serde_json::json!("/dns-query"));
                                object.insert("detour".to_string(), serde_json::json!("proxy"));
                                object.insert(
                                    "tls".to_string(),
                                    serde_json::json!({
                                        "enabled": true,
                                        "server_name": "dns.google"
                                    }),
                                );
                                object.remove("address_resolver");
                                object.remove("address_strategy");
                                object.remove("domain_resolver");
                                object.remove("domain_strategy");
                            }
                        }
                    }

                    if !remote_dns_found {
                        servers.push(serde_json::json!({
                            "type": "https",
                            "tag": "remote-dns",
                            "server": "8.8.8.8",
                            "server_port": 443,
                            "path": "/dns-query",
                            "detour": "proxy",
                            "tls": {
                                "enabled": true,
                                "server_name": "dns.google"
                            }
                        }));
                    }
                }

                if let Some(rules) = dns.get_mut("rules").and_then(|value| value.as_array_mut()) {
                    rules.retain(|rule| {
                        rule.get("server")
                            .and_then(|value| value.as_str())
                            .map(|server| server != "fakeip-dns")
                            .unwrap_or(true)
                    });
                }

                dns.insert("final".to_string(), serde_json::json!("remote-dns"));
                dns.insert("strategy".to_string(), serde_json::json!("ipv4_only"));
            }

            if let Some(route) = cfg.get_mut("route").and_then(|value| value.as_object_mut()) {
                route.remove("auto_detect_interface");
                route.insert(
                    "default_domain_resolver".to_string(),
                    serde_json::json!({
                        "server": "remote-dns",
                        "strategy": "ipv4_only"
                    }),
                );

                if let Some(rules) = route
                    .get_mut("rules")
                    .and_then(|value| value.as_array_mut())
                {
                    rules.retain(|rule| {
                        let action = rule.get("action").and_then(|value| value.as_str());
                        let protocol = rule.get("protocol").and_then(|value| value.as_str());
                        let network = rule.get("network").and_then(|value| value.as_str());
                        let port = rule.get("port").and_then(|value| value.as_u64());

                        if action == Some("hijack-dns") || protocol == Some("dns") {
                            return false;
                        }

                        if action == Some("reject") && network == Some("udp") && port == Some(443) {
                            return false;
                        }

                        true
                    });
                }
            }
        }
    }

    serde_json::to_string_pretty(&cfg)
        .map_err(|e| format!("Failed to serialize Windows runtime client config: {}", e))
}

#[tauri::command]
fn write_clipboard_text(text: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let mut child = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to launch pbcopy: {}", e))?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| format!("Failed to write clipboard text: {}", e))?;
        }

        let status = child
            .wait()
            .map_err(|e| format!("Failed to wait for pbcopy: {}", e))?;

        if status.success() {
            Ok(())
        } else {
            Err("pbcopy exited with a non-zero status".to_string())
        }
    }

    #[cfg(target_os = "windows")]
    {
        let mut child = windowless_command("cmd")
            .args(["/C", "clip"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to launch clip: {}", e))?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| format!("Failed to write clipboard text: {}", e))?;
        }

        let status = child
            .wait()
            .map_err(|e| format!("Failed to wait for clip: {}", e))?;

        if status.success() {
            Ok(())
        } else {
            Err("clip exited with a non-zero status".to_string())
        }
    }

    #[cfg(target_os = "android")]
    {
        with_android_activity(|env, activity| {
            let class_loader = env
                .call_method(
                    &activity,
                    "getClassLoader",
                    "()Ljava/lang/ClassLoader;",
                    &[],
                )
                .map_err(|e| format!("Failed to access Android app class loader: {}", e))?
                .l()
                .map_err(|e| format!("Failed to decode Android app class loader: {}", e))?;
            let class_name = env
                .new_string("com.freedom.rkn.AndroidVpnBridge")
                .map_err(|e| format!("Failed to allocate Android bridge class name: {}", e))?;
            let bridge = env
                .call_method(
                    &class_loader,
                    "loadClass",
                    "(Ljava/lang/String;)Ljava/lang/Class;",
                    &[JValue::Object(&JObject::from(class_name))],
                )
                .map_err(|e| format!("Failed to load Android clipboard bridge class: {}", e))?
                .l()
                .map_err(|e| format!("Failed to decode Android clipboard bridge class: {}", e))?;
            let bridge = jni::objects::JClass::from(bridge);
            let java_text = env
                .new_string(text)
                .map_err(|e| format!("Failed to allocate Android clipboard text: {}", e))?;

            env.call_static_method(
                bridge,
                "writeClipboardText",
                "(Landroid/content/Context;Ljava/lang/String;)V",
                &[
                    JValue::Object(&activity),
                    JValue::Object(&JObject::from(java_text)),
                ],
            )
            .map_err(|e| format!("Failed to write Android clipboard text: {}", e))?;

            Ok(())
        })
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
    {
        let _ = text;
        Err("Clipboard write is not implemented for this platform yet.".to_string())
    }
}

#[tauri::command]
fn read_clipboard_text() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("pbpaste")
            .output()
            .map_err(|e| format!("Failed to launch pbpaste: {}", e))?;

        if !output.status.success() {
            return Err("pbpaste exited with a non-zero status".to_string());
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    #[cfg(target_os = "windows")]
    {
        let output = windowless_command("powershell")
            .args(["-NoProfile", "-Command", "Get-Clipboard -Raw"])
            .output()
            .map_err(|e| format!("Failed to launch PowerShell clipboard reader: {}", e))?;

        if !output.status.success() {
            return Err("PowerShell clipboard read exited with a non-zero status".to_string());
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    #[cfg(target_os = "android")]
    {
        with_android_activity(|env, activity| {
            let class_loader = env
                .call_method(
                    &activity,
                    "getClassLoader",
                    "()Ljava/lang/ClassLoader;",
                    &[],
                )
                .map_err(|e| format!("Failed to access Android app class loader: {}", e))?
                .l()
                .map_err(|e| format!("Failed to decode Android app class loader: {}", e))?;
            let class_name = env
                .new_string("com.freedom.rkn.AndroidVpnBridge")
                .map_err(|e| format!("Failed to allocate Android bridge class name: {}", e))?;
            let bridge = env
                .call_method(
                    &class_loader,
                    "loadClass",
                    "(Ljava/lang/String;)Ljava/lang/Class;",
                    &[JValue::Object(&JObject::from(class_name))],
                )
                .map_err(|e| format!("Failed to load Android clipboard bridge class: {}", e))?
                .l()
                .map_err(|e| format!("Failed to decode Android clipboard bridge class: {}", e))?;
            let bridge = jni::objects::JClass::from(bridge);
            let value = env
                .call_static_method(
                    bridge,
                    "readClipboardText",
                    "(Landroid/content/Context;)Ljava/lang/String;",
                    &[JValue::Object(&activity)],
                )
                .map_err(|e| format!("Failed to read Android clipboard text: {}", e))?
                .l()
                .map_err(|e| format!("Failed to decode Android clipboard text: {}", e))?;
            let java_string = jni::objects::JString::from(value);
            let resolved = env
                .get_string(&java_string)
                .map_err(|e| format!("Failed to unwrap Android clipboard text: {}", e))?
                .to_string_lossy()
                .into_owned();

            Ok(resolved)
        })
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
    {
        Err("Clipboard read is not implemented for this platform yet.".to_string())
    }
}

#[tauri::command]
fn get_tunnel_log_tail(max_lines: Option<usize>) -> Result<Vec<String>, String> {
    let max_lines = max_lines.unwrap_or(200).clamp(1, 1000);
    let log_path = tunnel_log_path();
    let Ok(contents) = std::fs::read_to_string(log_path) else {
        return Ok(Vec::new());
    };

    let mut lines = contents.lines().map(str::to_string).collect::<Vec<_>>();
    if lines.len() > max_lines {
        lines = lines.split_off(lines.len() - max_lines);
    }

    Ok(lines)
}

#[tauri::command]
fn get_android_vpn_permission_status() -> Result<bool, String> {
    #[cfg(target_os = "android")]
    {
        android_vpn_permission_granted()
    }

    #[cfg(not(target_os = "android"))]
    {
        Ok(false)
    }
}

fn current_network_fingerprint() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        if let Some(fingerprint) = current_network_fingerprint_windows_powershell() {
            return Some(fingerprint);
        }

        current_network_fingerprint_windows_ipconfig()
    }

    #[cfg(not(target_os = "windows"))]
    {
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
                let normalized_iface = iface.to_lowercase();
                if normalized_iface.starts_with("lo")
                    || normalized_iface.starts_with("utun")
                    || normalized_iface.starts_with("awdl")
                    || normalized_iface.starts_with("llw")
                    || normalized_iface.starts_with("bridge")
                    || normalized_iface.starts_with("gif")
                    || normalized_iface.starts_with("stf")
                {
                    *status = None;
                    *ipv4 = None;
                    return;
                }

                let status_value = status.take().unwrap_or_else(|| "unknown".to_string());
                if status_value != "active" {
                    *ipv4 = None;
                    return;
                }

                let Some(ipv4_value) = ipv4.take() else {
                    return;
                };
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
}

#[cfg(target_os = "windows")]
fn current_network_fingerprint_windows_powershell() -> Option<String> {
    let script = r#"
      $ErrorActionPreference = 'Stop'
      $items = @(
        Get-NetIPConfiguration |
          Where-Object {
            $_.NetAdapter -and
            $_.NetAdapter.Status -eq 'Up' -and
            $_.IPv4Address -and
            $_.IPv4DefaultGateway -and
            $_.NetAdapter.InterfaceDescription -notmatch 'Loopback|Wintun|TAP|VPN|Virtual|vEthernet|Bluetooth|Npcap' -and
            $_.InterfaceAlias -notmatch 'Loopback|isatap|Teredo|Wintun|TAP|VPN|vEthernet|Bluetooth|Npcap|tun'
          } |
          Select-Object `
            @{Name='alias';Expression={$_.InterfaceAlias}}, `
            @{Name='index';Expression={$_.InterfaceIndex}}, `
            @{Name='ipv4';Expression={($_.IPv4Address | Select-Object -ExpandProperty IPAddress -First 1)}}, `
            @{Name='gateway';Expression={($_.IPv4DefaultGateway | Select-Object -ExpandProperty NextHop -First 1)}}, `
            @{Name='dns';Expression={@(($_.DNSServer.ServerAddresses)) -join ','}} |
          Sort-Object index, alias
      )
      $items | ConvertTo-Json -Compress
    "#;

    let output = windowless_command("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    parse_windows_network_fingerprint_json(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(target_os = "windows")]
fn current_network_fingerprint_windows_ipconfig() -> Option<String> {
    let output = windowless_command("ipconfig").output().ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut blocks = Vec::new();
    let mut current_adapter: Option<String> = None;
    let mut current_ipv4: Option<String> = None;
    let mut current_gateway: Option<String> = None;

    for line in stdout.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        if !line.starts_with(' ') && line.contains(':') {
            if let Some(adapter) = current_adapter.take() {
                if let Some(ip) = current_ipv4.take() {
                    if let Some(gw) = current_gateway.take() {
                        let lower = adapter.to_lowercase();
                        if !lower.contains("loopback")
                            && !lower.contains("isatap")
                            && !lower.contains("teredo")
                            && !lower.contains("wintun")
                            && !lower.contains("vethernet")
                        {
                            blocks.push(format!("{}|{}|{}", adapter, ip, gw));
                        }
                    }
                }
            }

            current_adapter = line
                .trim_end()
                .strip_suffix(':')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            current_ipv4 = None;
            current_gateway = None;
            continue;
        }

        if trimmed.contains("IPv4") && trimmed.contains(':') {
            if let Some(ip) = trimmed.split(':').last() {
                let ip = ip.trim();
                if !ip.is_empty() {
                    current_ipv4 = Some(ip.to_string());
                }
            }
        }

        if trimmed.contains("Default Gateway") && trimmed.contains(':') {
            if let Some(gw) = trimmed.split(':').last() {
                let gw = gw.trim();
                if !gw.is_empty() {
                    current_gateway = Some(gw.to_string());
                }
            }
        }
    }

    if let Some(adapter) = current_adapter.take() {
        if let Some(ip) = current_ipv4.take() {
            if let Some(gw) = current_gateway.take() {
                let lower = adapter.to_lowercase();
                if !lower.contains("loopback")
                    && !lower.contains("isatap")
                    && !lower.contains("teredo")
                    && !lower.contains("wintun")
                    && !lower.contains("vethernet")
                {
                    blocks.push(format!("{}|{}|{}", adapter, ip, gw));
                }
            }
        }
    }

    if blocks.is_empty() {
        None
    } else {
        Some(blocks.join(";"))
    }
}

#[cfg(target_os = "windows")]
fn parse_windows_network_fingerprint_json(stdout: &str) -> Option<String> {
    let parsed = serde_json::from_str::<serde_json::Value>(stdout.trim()).ok()?;
    let items = parsed.as_array()?;
    let mut blocks = Vec::new();

    for item in items {
        let alias = item
            .get("alias")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let index = item.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
        let ipv4 = item
            .get("ipv4")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let gateway = item
            .get("gateway")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let dns = item
            .get("dns")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if alias.is_empty() || ipv4.is_empty() {
            continue;
        }

        blocks.push(format!("{index}|{alias}|{ipv4}|{gateway}|{dns}"));
    }

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

fn begin_recovery(state: &AppState) -> bool {
    let mut guard = state.recovery_in_progress.lock().unwrap();
    if *guard {
        return false;
    }

    *guard = true;
    true
}

fn reset_guard_state(state: &AppState) {
    *state.proxy_failure_count.lock().unwrap() = 0;
    *state.proxy_failure_window_started.lock().unwrap() = None;
    *state.kill_switch_engaged.lock().unwrap() = false;
}

fn release_guard_after_quiet_period(app: &AppHandle, state: &AppState) {
    const PROXY_GUARD_QUIET_RELEASE: Duration = Duration::from_secs(90);

    let should_release = {
        let engaged = *state.kill_switch_engaged.lock().unwrap();
        if !engaged {
            false
        } else {
            state
                .proxy_failure_window_started
                .lock()
                .unwrap()
                .map(|started| started.elapsed() >= PROXY_GUARD_QUIET_RELEASE)
                .unwrap_or(false)
        }
    };

    if !should_release {
        return;
    }

    reset_guard_state(state);
    emit_guard_state(app, "active");
    let _ = app.emit(
        "tunnel-log",
        "[GUARD] Proxy path has been quiet since the degraded burst. Clearing the runtime guard state; the tunnel remains active.",
    );
}

fn register_proxy_failure(app: &AppHandle, state: &AppState) {
    const PROXY_FAILURE_RECOVERY_THRESHOLD: u8 = 8;
    const PROXY_FAILURE_WINDOW: Duration = Duration::from_secs(45);

    {
        let mut window = state.proxy_failure_window_started.lock().unwrap();
        match *window {
            Some(started) if started.elapsed() <= PROXY_FAILURE_WINDOW => {}
            _ => {
                *window = Some(std::time::Instant::now());
                *state.proxy_failure_count.lock().unwrap() = 0;
                *state.kill_switch_engaged.lock().unwrap() = false;
            }
        }
    }

    let mut failure_count = state.proxy_failure_count.lock().unwrap();
    *failure_count = failure_count.saturating_add(1);

    if *failure_count < PROXY_FAILURE_RECOVERY_THRESHOLD {
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

    #[cfg(target_os = "windows")]
    {
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = app_handle.emit(
                "tunnel-log",
                "[SYSTEM] Proxy transport is failing repeatedly on Windows. Stopping the tunnel to release system routing and restore the normal network path.".to_string(),
            );
            let _ = stop_tunnel_inner(app_handle.clone()).await;
        });
    }

    #[cfg(target_os = "macos")]
    {
        let _ = app.emit(
            "tunnel-log",
            "[SYSTEM] Proxy transport is failing repeatedly. Automatic privileged restart is suppressed on macOS to avoid repeated administrator prompts; toggle the tunnel manually if traffic does not recover."
                .to_string(),
        );
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        if !begin_recovery(state) {
            return;
        }

        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            let result = restart_tunnel_if_running(
                &app_handle,
                "[SYSTEM] Proxy transport is failing repeatedly. Restarting the tunnel to refresh the upstream session.",
            )
            .await;

            if let Err(error) = result {
                let state = app_handle.state::<AppState>();
                finish_recovery(&state);
                let _ = app_handle.emit(
                    "tunnel-log",
                    format!("[ERROR] Proxy transport recovery failed: {}", error),
                );
            }
        });
    }
}

fn classify_proxy_failure(line: &str) -> bool {
    let lower = line.to_lowercase();

    lower.contains("outbound/shadowsocks[proxy]")
        && (lower.contains("context deadline exceeded")
            || lower.contains("connection refused")
            || lower.contains("i/o timeout")
            || lower.contains("network is unreachable")
            || lower.contains("no route to host")
            || lower.contains("connection reset")
            || lower.contains("failed to verify certificate")
            || lower.contains("x509:"))
}

fn classify_outdated_subordinate_config(line: &str) -> bool {
    line.to_lowercase().contains("traffic hijacked")
}

#[cfg(target_os = "android")]
fn classify_noisy_android_core_info(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("inbound/tun[tun-in]: inbound connection")
        || lower.contains("outbound/direct[direct]: outbound connection")
}

#[cfg(not(target_os = "android"))]
fn classify_noisy_android_core_info(_line: &str) -> bool {
    false
}

fn current_singbox_target_triple() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        ("android", "aarch64") => Ok("aarch64-linux-android"),
        (os, arch) => Err(format!(
            "Unsupported platform for sing-box sidecar resolution: {} / {}",
            os, arch
        )),
    }
}

/// Находит абсолютный путь до sidecar-бинарника `sing-box`
pub(crate) fn resolve_singbox_path(app: &AppHandle) -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        return resolve_android_singbox_path(app);
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = app;

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

        if cfg!(target_os = "windows") {
            Ok("sing-box.exe".to_string())
        } else {
            Ok("sing-box".to_string())
        }
    }
}

#[cfg(target_os = "android")]
fn resolve_android_singbox_path(app: &AppHandle) -> Result<String, String> {
    use std::os::unix::fs::PermissionsExt;

    let native_library_dir = with_android_activity(|env, activity| {
        let class_loader = env
            .call_method(
                &activity,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .map_err(|e| format!("Failed to access Android app class loader: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android app class loader: {}", e))?;
        let class_name = env
            .new_string("com.freedom.rkn.AndroidVpnBridge")
            .map_err(|e| format!("Failed to allocate Android bridge class name: {}", e))?;
        let bridge = env
            .call_method(
                &class_loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&JObject::from(class_name))],
            )
            .map_err(|e| format!("Failed to load Android VPN bridge class: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android VPN bridge class: {}", e))?;
        let bridge = jni::objects::JClass::from(bridge);
        let value = env
            .call_static_method(
                bridge,
                "getNativeLibraryDir",
                "(Landroid/content/Context;)Ljava/lang/String;",
                &[JValue::Object(&activity)],
            )
            .map_err(|e| format!("Failed to resolve Android native library dir: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android native library dir: {}", e))?;
        let java_string = jni::objects::JString::from(value);
        let resolved = env
            .get_string(&java_string)
            .map_err(|e| format!("Failed to read Android native library dir: {}", e))?
            .to_string_lossy()
            .into_owned();
        Ok(resolved)
    })?;

    if !native_library_dir.trim().is_empty() {
        for candidate in ["libsingbox.so", "sing-box"] {
            let path = std::path::Path::new(&native_library_dir).join(candidate);
            if path.exists() {
                let mut perms = std::fs::metadata(&path)
                    .map_err(|e| {
                        format!(
                            "Failed to stat Android native sidecar {}: {}",
                            path.display(),
                            e
                        )
                    })?
                    .permissions();
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(&path, perms);
                return Ok(path.to_string_lossy().to_string());
            }
        }

        return Err(format!(
            "Android native sidecar was not found in nativeLibraryDir {}. The APK did not expose an executable jniLib for sing-box. Rebuild with extracted native libs enabled.",
            native_library_dir
        ));
    }

    let _ = app;
    Err(
        "Android nativeLibraryDir is empty, so RKN cannot locate an executable sing-box sidecar on this device."
            .to_string(),
    )
}

#[cfg(target_os = "windows")]
fn summarize_windows_command_failure(prefix: &str, output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !stderr.is_empty() && !stdout.is_empty() {
        format!("{prefix}: {stderr} | {stdout}")
    } else if !stderr.is_empty() {
        format!("{prefix}: {stderr}")
    } else if !stdout.is_empty() {
        format!("{prefix}: {stdout}")
    } else {
        format!(
            "{prefix}: process exited with code {:?}",
            output.status.code()
        )
    }
}

#[cfg(target_os = "windows")]
fn run_windows_singbox_preflight(singbox_path: &str, config_path: &str) -> Result<(), String> {
    let singbox_dir = std::path::Path::new(singbox_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    let version_output = windowless_command(singbox_path)
        .current_dir(singbox_dir)
        .arg("version")
        .output()
        .map_err(|e| format!("Failed to launch sing-box preflight: {}", e))?;

    if !version_output.status.success() {
        return Err(summarize_windows_command_failure(
            "sing-box preflight failed before elevation",
            &version_output,
        ));
    }

    let check_output = windowless_command(singbox_path)
        .current_dir(singbox_dir)
        .args(["check", "-c", config_path])
        .output()
        .map_err(|e| format!("Failed to run sing-box config preflight: {}", e))?;

    if !check_output.status.success() {
        return Err(summarize_windows_command_failure(
            "sing-box config preflight failed",
            &check_output,
        ));
    }

    Ok(())
}

/// Windows: launches sing-box as an elevated process via PowerShell UAC prompt.
///
/// The flow:
/// 1. Build a PowerShell inner command that starts sing-box, redirects output to
///    the log file, and writes the PID to a temp file.
/// 2. Launch that command with `Start-Process -Verb RunAs` which triggers the
///    native Windows UAC dialog.
/// 3. Poll for the PID file (the elevated process writes it asynchronously).
#[cfg(target_os = "windows")]
async fn launch_tunnel_process_windows(
    app: &AppHandle,
    singbox_path: &str,
    config_str: &str,
    log_path: &str,
) -> Result<u32, String> {
    // --- Pre-flight diagnostics ---

    // 1. Check that wintun.dll is accessible (required for TUN mode)
    let singbox_dir = std::path::Path::new(singbox_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let wintun_path = singbox_dir.join("wintun.dll");
    if !wintun_path.exists() {
        // Also check the resource directory (Tauri places bundled resources there)
        let resource_wintun = app.path().resource_dir().ok().map(|d| d.join("wintun.dll"));
        let found = resource_wintun.as_ref().map_or(false, |p| p.exists());
        if !found {
            let _ = app.emit(
                "tunnel-log",
                "[ERROR] wintun.dll not found. TUN mode requires the Wintun driver.",
            );
            return Err(
                "wintun.dll not found next to sing-box. Reinstall the application.".to_string(),
            );
        }
        // Copy wintun.dll from resource dir to sing-box directory so it can find it
        if let Some(src) = resource_wintun {
            let _ = std::fs::copy(&src, &wintun_path);
        }
    }

    // 2. Ensure libcronet.dll is next to sing-box (runtime dependency)
    let cronet_path = singbox_dir.join("libcronet.dll");
    if !cronet_path.exists() {
        let resource_cronet = app
            .path()
            .resource_dir()
            .ok()
            .map(|d| d.join("libcronet.dll"));
        if let Some(src) = resource_cronet.filter(|p| p.exists()) {
            let _ = std::fs::copy(&src, &cronet_path);
        }
    }

    // 3. Check that sing-box binary exists and is not blocked
    if !std::path::Path::new(singbox_path).exists() {
        return Err(format!(
            "sing-box binary not found at {}. Reinstall the application.",
            singbox_path
        ));
    }

    let local_data = app
        .path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());
    let pid_file = local_data.join("elevated_singbox.pid");
    let bootstrap_script = local_data.join("elevated_singbox_bootstrap.ps1");
    let bootstrap_err = local_data.join("elevated_singbox_bootstrap.err");
    let _ = std::fs::remove_file(&pid_file);
    let _ = std::fs::remove_file(&bootstrap_err);

    let pid_file_str = pid_file.to_string_lossy().to_string();
    let bootstrap_err_str = bootstrap_err.to_string_lossy().to_string();

    let stop_signal_file = local_data.join("elevated_singbox.stop");
    let stop_signal_file_str = stop_signal_file.to_string_lossy().to_string();
    let _ = std::fs::remove_file(&stop_signal_file);

    let bootstrap_script_body = format!(
        r#"$ErrorActionPreference = 'Stop'
try {{
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = '{singbox}'
  $psi.WorkingDirectory = '{workdir}'
  $psi.Arguments = 'run -c "{config}"'
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $true
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true

  $p = New-Object System.Diagnostics.Process
  $p.StartInfo = $psi
  [void]$p.Start()
  [System.IO.File]::WriteAllText('{pidfile}', $p.Id.ToString(), [System.Text.Encoding]::ASCII)

  # Check if it crashed within the first 8 seconds
  if ($p.WaitForExit(8000)) {{
    $logTail = ''
    $stdoutTail = ''
    $stderrTail = ''
    try {{
      $stdoutTail = $p.StandardOutput.ReadToEnd()
    }} catch {{}}
    try {{
      $stderrTail = $p.StandardError.ReadToEnd()
    }} catch {{}}
    if (Test-Path '{log_path}') {{
      try {{
        $logTail = (Get-Content -Path '{log_path}' -Tail 20 | Out-String)
      }} catch {{}}
    }}
    $message = "sing-box exited during bootstrap with code $($p.ExitCode)."
    if (-not [string]::IsNullOrWhiteSpace($stderrTail)) {{
      $message = $message + [Environment]::NewLine + "stderr:" + [Environment]::NewLine + $stderrTail.Trim()
    }}
    if (-not [string]::IsNullOrWhiteSpace($stdoutTail)) {{
      $message = $message + [Environment]::NewLine + "stdout:" + [Environment]::NewLine + $stdoutTail.Trim()
    }}
    if (-not [string]::IsNullOrWhiteSpace($logTail)) {{
      $message = $message + [Environment]::NewLine + "log tail:" + [Environment]::NewLine + $logTail.Trim()
    }}
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText('{bootstrap_err}', $message, $utf8NoBom)
    exit 1
  }}

  # Supervisor loop: kill sing-box if we receive a stop signal or if parent GUI process dies
  $parentPid = {parent_pid}
  while ($true) {{
    if (-not (Get-Process -Id $parentPid -ErrorAction SilentlyContinue)) {{
      try {{ $p.Kill() }} catch {{}}
      exit 0
    }}
    if (Test-Path '{stop_signal_file}') {{
      try {{ $p.Kill() }} catch {{}}
      Remove-Item '{stop_signal_file}' -Force -ErrorAction SilentlyContinue
      exit 0
    }}
    if ($p.HasExited) {{
      exit 0
    }}
    Start-Sleep -Milliseconds 500
  }}
}} catch {{
  $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllText('{bootstrap_err}', ($_ | Out-String), $utf8NoBom)
  exit 1
}}"#,
        singbox = singbox_path.replace('\'', "''"),
        workdir = singbox_dir.to_string_lossy().replace('\'', "''"),
        config = config_str.replace('\'', "''"),
        log_path = log_path.replace('\'', "''"),
        pidfile = pid_file_str.replace('\'', "''"),
        bootstrap_err = bootstrap_err_str.replace('\'', "''"),
        parent_pid = std::process::id(),
        stop_signal_file = stop_signal_file_str.replace('\'', "''"),
    );

    // Write with UTF-8 BOM so PowerShell reads Cyrillic paths correctly
    // on systems where the default codepage is not UTF-8.
    let mut bom_content = Vec::with_capacity(3 + bootstrap_script_body.len());
    bom_content.extend_from_slice(b"\xEF\xBB\xBF");
    bom_content.extend_from_slice(bootstrap_script_body.as_bytes());
    std::fs::write(&bootstrap_script, bom_content)
        .map_err(|e| format!("Failed to write Windows tunnel bootstrap script: {}", e))?;

    // Outer command: launch elevated PowerShell with the script file and wait for the
    // short bootstrap wrapper to finish. The wrapper starts sing-box, writes its PID,
    // then exits immediately.
    //
    // IMPORTANT: build the bootstrap path via $env:LOCALAPPDATA to avoid Cyrillic
    // characters on the command line (Cyrillic usernames cause encoding issues in
    // the non-Unicode PowerShell pipeline on older Windows systems).
    let bootstrap_filename = bootstrap_script
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let launch_output = windowless_command("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "$bf = Join-Path $env:LOCALAPPDATA 'com.freedom.rkn\\{}'; Start-Process powershell -Verb RunAs -WindowStyle Hidden -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',$bf)",
                bootstrap_filename
            ),
        ])
        .output()
        .map_err(|e| format!("Failed to launch elevated PowerShell: {}", e))?;

    if !launch_output.status.success() {
        let stderr = String::from_utf8_lossy(&launch_output.stderr)
            .trim()
            .to_string();
        if stderr.contains("canceled")
            || stderr.contains("cancelled")
            || stderr.contains("0x80004005")
        {
            let _ = app.emit(
                "tunnel-log",
                "[SYSTEM] Administrator access was cancelled by user.",
            );
            return Err("User cancelled admin prompt".to_string());
        }
        let bootstrap_hint = std::fs::read_to_string(&bootstrap_err)
            .map(|value| trim_utf8_bom(&value).to_string())
            .unwrap_or_default();
        if !bootstrap_hint.trim().is_empty() {
            return Err(format!(
                "PowerShell elevation error: {} {}",
                stderr,
                bootstrap_hint.trim()
            ));
        }
        return Err(format!("PowerShell elevation error: {}", stderr));
    }

    // Poll for the PID file written by the elevated process. On weaker Windows
    // machines the elevated bootstrap can take noticeably longer after the UAC
    // confirmation, so keep the wait budget generous. We do not block on the
    // outer PowerShell wrapper itself because older Windows installs can hang
    // there even after the UAC prompt is accepted.
    for _ in 0..60 {
        sleep(Duration::from_millis(500)).await;
        if let Ok(contents) = std::fs::read_to_string(&pid_file) {
            let trimmed = contents.trim();
            if let Ok(pid) = trimmed.parse::<u32>() {
                let _ = std::fs::remove_file(&pid_file);
                return Ok(pid);
            }
        }

        if bootstrap_err.exists() {
            break;
        }
    }

    // Check for common failure reasons and emit diagnostics.
    // sing-box writes its own log via config log.output; read last lines for clues.
    let err_hint = recent_log_tail(log_path, 10);

    let bootstrap_hint = std::fs::read_to_string(&bootstrap_err)
        .ok()
        .and_then(|s| {
            let trimmed = trim_utf8_bom(&s).trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .unwrap_or_default();

    let combined_hint = if !bootstrap_hint.is_empty() {
        format!("{} {}", bootstrap_hint, err_hint)
    } else {
        err_hint.clone()
    };

    let diagnostic = if combined_hint.contains("wintun") || combined_hint.contains("Wintun") {
        "Wintun driver failed to initialize. It may be blocked by antivirus software."
    } else if combined_hint.contains("Access is denied") || combined_hint.contains("access denied")
    {
        "Access denied — administrator privileges are required. Check UAC settings."
    } else if combined_hint.contains("antivirus") || combined_hint.contains("blocked") {
        "sing-box may be blocked by antivirus software. Add an exception and retry."
    } else if !combined_hint.is_empty() {
        &combined_hint
    } else {
        "Timed out waiting for elevated sing-box to start. Check UAC settings and antivirus."
    };

    let _ = app.emit("tunnel-log", format!("[ERROR] {}", diagnostic));
    Err(diagnostic.to_string())
}

#[cfg(target_os = "windows")]
async fn launch_tunnel_process_windows_compatibility(
    app: &AppHandle,
    singbox_path: &str,
    config_str: &str,
    log_path: &str,
) -> Result<u32, String> {
    let _ = clear_windows_system_proxy();

    let singbox_dir = std::path::Path::new(singbox_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    let cronet_path = singbox_dir.join("libcronet.dll");
    if !cronet_path.exists() {
        let resource_cronet = app
            .path()
            .resource_dir()
            .ok()
            .map(|d| d.join("libcronet.dll"));
        if let Some(src) = resource_cronet.filter(|p| p.exists()) {
            let _ = std::fs::copy(&src, &cronet_path);
        }
    }

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(log_path)
        .map_err(|e| format!("Failed to open Windows compatibility log file: {}", e))?;
    let stderr_file = log_file.try_clone().map_err(|e| {
        format!(
            "Failed to duplicate Windows compatibility log handle: {}",
            e
        )
    })?;

    let child = windowless_command(singbox_path)
        .current_dir(singbox_dir)
        .args(["run", "-c", config_str])
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .map_err(|e| {
            format!(
                "Failed to launch sing-box in Windows compatibility mode: {}",
                e
            )
        })?;

    Ok(child.id())
}

#[cfg(target_os = "android")]
fn with_android_activity<F, T>(f: F) -> Result<T, String>
where
    F: FnOnce(&mut jni::JNIEnv<'_>, JObject<'_>) -> Result<T, String>,
{
    let android_context = ndk_context::android_context();
    let vm_ptr = android_context.vm();
    let activity_ptr = android_context.context();

    if vm_ptr.is_null() || activity_ptr.is_null() {
        return Err("Android runtime context is unavailable.".to_string());
    }

    let vm = unsafe { JavaVM::from_raw(vm_ptr.cast()) }
        .map_err(|e| format!("Failed to access Android JavaVM: {}", e))?;
    let result = {
        let mut env = vm
            .attach_current_thread()
            .map_err(|e| format!("Failed to attach Android thread to JVM: {}", e))?;
        let activity = unsafe { JObject::from_raw(activity_ptr.cast()) };
        f(&mut env, activity)
    };
    std::mem::forget(vm);
    result
}

#[cfg(target_os = "android")]
fn android_files_dir_path() -> Result<PathBuf, String> {
    with_android_activity(|env, activity| {
        let files_dir = env
            .call_method(&activity, "getFilesDir", "()Ljava/io/File;", &[])
            .map_err(|e| format!("Failed to access Android filesDir: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android filesDir handle: {}", e))?;
        let absolute_path = env
            .call_method(&files_dir, "getAbsolutePath", "()Ljava/lang/String;", &[])
            .map_err(|e| format!("Failed to resolve Android filesDir path: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android filesDir path: {}", e))?;
        let java_string = jni::objects::JString::from(absolute_path);
        let resolved = env
            .get_string(&java_string)
            .map_err(|e| format!("Failed to read Android filesDir path: {}", e))?
            .to_string_lossy()
            .into_owned();

        Ok(PathBuf::from(resolved))
    })
}

#[cfg(target_os = "android")]
fn android_vpn_permission_granted() -> Result<bool, String> {
    with_android_activity(|env, activity| {
        let class_loader = env
            .call_method(
                &activity,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .map_err(|e| format!("Failed to access Android app class loader: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android app class loader: {}", e))?;
        let class_name = env
            .new_string("com.freedom.rkn.AndroidVpnBridge")
            .map_err(|e| format!("Failed to allocate Android bridge class name: {}", e))?;
        let bridge = env
            .call_method(
                &class_loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&JObject::from(class_name))],
            )
            .map_err(|e| format!("Failed to load Android VPN bridge class: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android VPN bridge class: {}", e))?;
        let bridge = jni::objects::JClass::from(bridge);
        let granted = env
            .call_static_method(
                bridge,
                "isVpnPermissionGranted",
                "(Landroid/content/Context;)Z",
                &[JValue::Object(&activity)],
            )
            .map_err(|e| format!("Failed to query Android VPN permission state: {}", e))?
            .z()
            .map_err(|e| format!("Failed to decode Android VPN permission state: {}", e))?;

        Ok(granted)
    })
}

#[cfg(target_os = "android")]
fn request_android_vpn_permission() -> Result<bool, String> {
    with_android_activity(|env, activity| {
        let class_loader = env
            .call_method(
                &activity,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .map_err(|e| format!("Failed to access Android app class loader: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android app class loader: {}", e))?;
        let class_name = env
            .new_string("com.freedom.rkn.AndroidVpnBridge")
            .map_err(|e| format!("Failed to allocate Android bridge class name: {}", e))?;
        let bridge = env
            .call_method(
                &class_loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&JObject::from(class_name))],
            )
            .map_err(|e| format!("Failed to load Android VPN bridge class: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android VPN bridge class: {}", e))?;
        let bridge = jni::objects::JClass::from(bridge);
        let already_granted = env
            .call_static_method(
                bridge,
                "requestVpnPermission",
                "(Landroid/app/Activity;)Z",
                &[JValue::Object(&activity)],
            )
            .map_err(|e| format!("Failed to request Android VPN permission: {}", e))?
            .z()
            .map_err(|e| format!("Failed to decode Android VPN permission result: {}", e))?;

        Ok(already_granted)
    })
}

#[cfg(target_os = "android")]
fn start_android_tunnel_service() -> Result<(), String> {
    with_android_activity(|env, activity| {
        let class_loader = env
            .call_method(
                &activity,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .map_err(|e| format!("Failed to access Android app class loader: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android app class loader: {}", e))?;
        let class_name = env
            .new_string("com.freedom.rkn.AndroidVpnBridge")
            .map_err(|e| format!("Failed to allocate Android bridge class name: {}", e))?;
        let bridge = env
            .call_method(
                &class_loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&JObject::from(class_name))],
            )
            .map_err(|e| format!("Failed to load Android VPN bridge class: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android VPN bridge class: {}", e))?;
        let bridge = jni::objects::JClass::from(bridge);
        env.call_static_method(
            bridge,
            "startTunnelService",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .map_err(|e| format!("Failed to start Android tunnel service: {}", e))?;

        Ok(())
    })
}

#[cfg(target_os = "android")]
fn stop_android_tunnel_service() -> Result<(), String> {
    with_android_activity(|env, activity| {
        let class_loader = env
            .call_method(
                &activity,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .map_err(|e| format!("Failed to access Android app class loader: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android app class loader: {}", e))?;
        let class_name = env
            .new_string("com.freedom.rkn.AndroidVpnBridge")
            .map_err(|e| format!("Failed to allocate Android bridge class name: {}", e))?;
        let bridge = env
            .call_method(
                &class_loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&JObject::from(class_name))],
            )
            .map_err(|e| format!("Failed to load Android VPN bridge class: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android VPN bridge class: {}", e))?;
        let bridge = jni::objects::JClass::from(bridge);
        env.call_static_method(
            bridge,
            "stopTunnelService",
            "(Landroid/content/Context;)V",
            &[JValue::Object(&activity)],
        )
        .map_err(|e| format!("Failed to stop Android tunnel service: {}", e))?;

        Ok(())
    })
}

#[cfg(target_os = "android")]
fn android_tun_interface_ready() -> Result<bool, String> {
    with_android_activity(|env, activity| {
        let class_loader = env
            .call_method(
                &activity,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .map_err(|e| format!("Failed to access Android app class loader: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android app class loader: {}", e))?;
        let class_name = env
            .new_string("com.freedom.rkn.AndroidVpnBridge")
            .map_err(|e| format!("Failed to allocate Android bridge class name: {}", e))?;
        let bridge = env
            .call_method(
                &class_loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&JObject::from(class_name))],
            )
            .map_err(|e| format!("Failed to load Android VPN bridge class: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android VPN bridge class: {}", e))?;
        let bridge = jni::objects::JClass::from(bridge);
        let ready = env
            .call_static_method(
                bridge,
                "isTunnelInterfaceReady",
                "(Landroid/content/Context;)Z",
                &[JValue::Object(&activity)],
            )
            .map_err(|e| format!("Failed to query Android tunnel interface state: {}", e))?
            .z()
            .map_err(|e| format!("Failed to decode Android tunnel interface state: {}", e))?;

        Ok(ready)
    })
}

#[cfg(target_os = "android")]
fn android_bridge_string(method: &str) -> Result<String, String> {
    with_android_activity(|env, activity| {
        let class_loader = env
            .call_method(
                &activity,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .map_err(|e| format!("Failed to access Android app class loader: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android app class loader: {}", e))?;
        let class_name = env
            .new_string("com.freedom.rkn.AndroidVpnBridge")
            .map_err(|e| format!("Failed to allocate Android bridge class name: {}", e))?;
        let bridge = env
            .call_method(
                &class_loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&JObject::from(class_name))],
            )
            .map_err(|e| format!("Failed to load Android VPN bridge class: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android VPN bridge class: {}", e))?;
        let bridge = jni::objects::JClass::from(bridge);
        let value = env
            .call_static_method(
                bridge,
                method,
                "(Landroid/content/Context;)Ljava/lang/String;",
                &[JValue::Object(&activity)],
            )
            .map_err(|e| {
                format!(
                    "Failed to query Android bridge string via {}: {}",
                    method, e
                )
            })?
            .l()
            .map_err(|e| {
                format!(
                    "Failed to decode Android bridge string via {}: {}",
                    method, e
                )
            })?;
        let java_string = jni::objects::JString::from(value);
        let resolved = env
            .get_string(&java_string)
            .map_err(|e| format!("Failed to read Android bridge string via {}: {}", method, e))?
            .to_string_lossy()
            .into_owned();

        Ok(resolved)
    })
}

#[cfg(target_os = "android")]
fn android_bridge_int(method: &str) -> Result<i32, String> {
    with_android_activity(|env, activity| {
        let class_loader = env
            .call_method(
                &activity,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .map_err(|e| format!("Failed to access Android app class loader: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android app class loader: {}", e))?;
        let class_name = env
            .new_string("com.freedom.rkn.AndroidVpnBridge")
            .map_err(|e| format!("Failed to allocate Android bridge class name: {}", e))?;
        let bridge = env
            .call_method(
                &class_loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&JObject::from(class_name))],
            )
            .map_err(|e| format!("Failed to load Android VPN bridge class: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android VPN bridge class: {}", e))?;
        let bridge = jni::objects::JClass::from(bridge);
        let value = env
            .call_static_method(
                bridge,
                method,
                "(Landroid/content/Context;)I",
                &[JValue::Object(&activity)],
            )
            .map_err(|e| format!("Failed to query Android bridge int via {}: {}", method, e))?
            .i()
            .map_err(|e| format!("Failed to decode Android bridge int via {}: {}", method, e))?;

        Ok(value)
    })
}

#[cfg(target_os = "android")]
fn android_bridge_plain_string(method: &str) -> Result<String, String> {
    android_bridge_string(method)
}

#[cfg(target_os = "android")]
fn android_tunnel_debug_state() -> Result<String, String> {
    android_bridge_string("getTunnelDebugState")
}

#[cfg(target_os = "android")]
fn android_peek_tun_fd() -> Result<i32, String> {
    android_bridge_int("peekTunnelFd")
}

#[cfg(target_os = "android")]
fn android_tun_address() -> Result<String, String> {
    android_bridge_string("getTunnelAddress")
}

#[cfg(target_os = "android")]
fn android_tun_prefix_length() -> Result<i32, String> {
    android_bridge_int("getTunnelPrefixLength")
}

#[cfg(target_os = "android")]
fn android_tun_route() -> Result<String, String> {
    android_bridge_string("getTunnelRoute")
}

#[cfg(target_os = "android")]
fn android_tun_mtu() -> Result<i32, String> {
    android_bridge_int("getTunnelMtu")
}

#[cfg(target_os = "android")]
fn android_register_backend_handoff_session(
    session_id: &str,
    context_path: &str,
    backend_config_path: &str,
    log_path: &str,
    tun_fd: i32,
) -> Result<String, String> {
    with_android_activity(|env, activity| {
        let class_loader = env
            .call_method(
                &activity,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .map_err(|e| format!("Failed to access Android app class loader: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android app class loader: {}", e))?;
        let class_name = env
            .new_string("com.freedom.rkn.AndroidVpnBridge")
            .map_err(|e| format!("Failed to allocate Android bridge class name: {}", e))?;
        let bridge = env
            .call_method(
                &class_loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&JObject::from(class_name))],
            )
            .map_err(|e| format!("Failed to load Android VPN bridge class: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android VPN bridge class: {}", e))?;
        let bridge = jni::objects::JClass::from(bridge);
        let java_context_path = env
            .new_string(context_path)
            .map_err(|e| format!("Failed to allocate Android backend context path: {}", e))?;
        let java_session_id = env
            .new_string(session_id)
            .map_err(|e| format!("Failed to allocate Android backend session id: {}", e))?;
        let java_backend_config_path = env
            .new_string(backend_config_path)
            .map_err(|e| format!("Failed to allocate Android backend config path: {}", e))?;
        let java_log_path = env
            .new_string(log_path)
            .map_err(|e| format!("Failed to allocate Android backend log path: {}", e))?;
        let value = env
            .call_static_method(
                bridge,
                "registerBackendHandoffSession",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;I)Ljava/lang/String;",
                &[
                    JValue::Object(&activity),
                    JValue::Object(&JObject::from(java_session_id)),
                    JValue::Object(&JObject::from(java_context_path)),
                    JValue::Object(&JObject::from(java_backend_config_path)),
                    JValue::Object(&JObject::from(java_log_path)),
                    JValue::Int(tun_fd),
                ],
            )
            .map_err(|e| format!("Failed to register Android backend handoff session: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android backend handoff session result: {}", e))?;
        let java_string = jni::objects::JString::from(value);
        let resolved = env
            .get_string(&java_string)
            .map_err(|e| {
                format!(
                    "Failed to read Android backend handoff session result: {}",
                    e
                )
            })?
            .to_string_lossy()
            .into_owned();

        Ok(resolved)
    })
}

#[cfg(target_os = "android")]
fn android_backend_handoff_state() -> Result<String, String> {
    android_bridge_string("getBackendHandoffState")
}

#[cfg(target_os = "android")]
fn android_backend_handoff_session_id() -> Result<String, String> {
    android_bridge_plain_string("getBackendHandoffSessionId")
}

#[cfg(target_os = "android")]
fn android_native_backend_status_path() -> Result<String, String> {
    android_bridge_plain_string("getNativeBackendStatusPath")
}

#[cfg(target_os = "android")]
fn android_native_backend_status_state() -> Result<String, String> {
    android_bridge_plain_string("getNativeBackendStatusState")
}

#[cfg(target_os = "android")]
fn android_backend_state_is_pending(state: &str) -> bool {
    let normalized = state.trim().to_ascii_lowercase();
    normalized.starts_with("launching")
        || normalized.starts_with("starting")
        || normalized.starts_with("pending")
}

#[cfg(target_os = "android")]
fn android_backend_state_is_ready(state: &str) -> bool {
    state.trim().to_ascii_lowercase().starts_with("ready")
}

#[cfg(target_os = "android")]
fn android_backend_state_is_stopped(state: &str) -> bool {
    let normalized = state.trim().to_ascii_lowercase();
    normalized.starts_with("idle")
        || normalized.starts_with("stopped")
        || normalized.starts_with("failed")
        || normalized.starts_with("cancelled")
        || normalized.starts_with("missing")
        || normalized.starts_with("unknown")
}

#[cfg(target_os = "android")]
fn android_claim_backend_handoff_session(
    session_id: &str,
    consumer_tag: &str,
) -> Result<String, String> {
    with_android_activity(|env, activity| {
        let class_loader = env
            .call_method(
                &activity,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .map_err(|e| format!("Failed to access Android app class loader: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android app class loader: {}", e))?;
        let class_name = env
            .new_string("com.freedom.rkn.AndroidVpnBridge")
            .map_err(|e| format!("Failed to allocate Android bridge class name: {}", e))?;
        let bridge = env
            .call_method(
                &class_loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&JObject::from(class_name))],
            )
            .map_err(|e| format!("Failed to load Android VPN bridge class: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android VPN bridge class: {}", e))?;
        let bridge = jni::objects::JClass::from(bridge);
        let java_session_id = env
            .new_string(session_id)
            .map_err(|e| format!("Failed to allocate Android claim session id: {}", e))?;
        let java_consumer_tag = env
            .new_string(consumer_tag)
            .map_err(|e| format!("Failed to allocate Android claim consumer tag: {}", e))?;
        let value = env
            .call_static_method(
                bridge,
                "claimBackendHandoffSession",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                &[
                    JValue::Object(&activity),
                    JValue::Object(&JObject::from(java_session_id)),
                    JValue::Object(&JObject::from(java_consumer_tag)),
                ],
            )
            .map_err(|e| format!("Failed to claim Android backend handoff session: {}", e))?
            .l()
            .map_err(|e| {
                format!(
                    "Failed to decode Android backend handoff claim result: {}",
                    e
                )
            })?;
        let java_string = jni::objects::JString::from(value);
        let resolved = env
            .get_string(&java_string)
            .map_err(|e| format!("Failed to read Android backend handoff claim result: {}", e))?
            .to_string_lossy()
            .into_owned();

        Ok(resolved)
    })
}

#[cfg(target_os = "android")]
fn android_update_backend_handoff_session_state(
    session_id: &str,
    consumer_tag: &str,
    phase: &str,
    detail: &str,
) -> Result<String, String> {
    with_android_activity(|env, activity| {
        let class_loader = env
            .call_method(
                &activity,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .map_err(|e| format!("Failed to access Android app class loader: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android app class loader: {}", e))?;
        let class_name = env
            .new_string("com.freedom.rkn.AndroidVpnBridge")
            .map_err(|e| format!("Failed to allocate Android bridge class name: {}", e))?;
        let bridge = env
            .call_method(
                &class_loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&JObject::from(class_name))],
            )
            .map_err(|e| format!("Failed to load Android VPN bridge class: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android VPN bridge class: {}", e))?;
        let bridge = jni::objects::JClass::from(bridge);
        let java_session_id = env
            .new_string(session_id)
            .map_err(|e| format!("Failed to allocate Android update session id: {}", e))?;
        let java_consumer_tag = env
            .new_string(consumer_tag)
            .map_err(|e| format!("Failed to allocate Android update consumer tag: {}", e))?;
        let java_phase = env
            .new_string(phase)
            .map_err(|e| format!("Failed to allocate Android update phase: {}", e))?;
        let java_detail = env
            .new_string(detail)
            .map_err(|e| format!("Failed to allocate Android update detail: {}", e))?;
        let value = env
            .call_static_method(
                bridge,
                "updateBackendHandoffSessionState",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                &[
                    JValue::Object(&activity),
                    JValue::Object(&JObject::from(java_session_id)),
                    JValue::Object(&JObject::from(java_consumer_tag)),
                    JValue::Object(&JObject::from(java_phase)),
                    JValue::Object(&JObject::from(java_detail)),
                ],
            )
            .map_err(|e| format!("Failed to update Android backend handoff session state: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android backend handoff state update result: {}", e))?;
        let java_string = jni::objects::JString::from(value);
        let resolved = env
            .get_string(&java_string)
            .map_err(|e| {
                format!(
                    "Failed to read Android backend handoff state update result: {}",
                    e
                )
            })?
            .to_string_lossy()
            .into_owned();

        Ok(resolved)
    })
}

#[cfg(target_os = "android")]
fn android_start_native_backend_seam(claim_path: &str) -> Result<String, String> {
    with_android_activity(|env, activity| {
        let class_loader = env
            .call_method(
                &activity,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .map_err(|e| format!("Failed to access Android app class loader: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android app class loader: {}", e))?;
        let class_name = env
            .new_string("com.freedom.rkn.AndroidVpnBridge")
            .map_err(|e| format!("Failed to allocate Android bridge class name: {}", e))?;
        let bridge = env
            .call_method(
                &class_loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&JObject::from(class_name))],
            )
            .map_err(|e| format!("Failed to load Android VPN bridge class: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android VPN bridge class: {}", e))?;
        let bridge = jni::objects::JClass::from(bridge);
        let java_claim_path = env
            .new_string(claim_path)
            .map_err(|e| format!("Failed to allocate Android seam claim path: {}", e))?;
        let value = env
            .call_static_method(
                bridge,
                "startNativeBackendSeam",
                "(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;",
                &[
                    JValue::Object(&activity),
                    JValue::Object(&JObject::from(java_claim_path)),
                ],
            )
            .map_err(|e| format!("Failed to start Android native backend seam: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android native backend seam result: {}", e))?;
        let java_string = jni::objects::JString::from(value);
        let resolved = env
            .get_string(&java_string)
            .map_err(|e| format!("Failed to read Android native backend seam result: {}", e))?
            .to_string_lossy()
            .into_owned();

        Ok(resolved)
    })
}

#[cfg(target_os = "android")]
#[allow(dead_code)]
fn android_protect_socket_fd(fd: i32) -> Result<bool, String> {
    with_android_activity(|env, activity| {
        let class_loader = env
            .call_method(
                &activity,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .map_err(|e| format!("Failed to access Android app class loader: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android app class loader: {}", e))?;
        let class_name = env
            .new_string("com.freedom.rkn.AndroidVpnBridge")
            .map_err(|e| format!("Failed to allocate Android bridge class name: {}", e))?;
        let bridge = env
            .call_method(
                &class_loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&JObject::from(class_name))],
            )
            .map_err(|e| format!("Failed to load Android VPN bridge class: {}", e))?
            .l()
            .map_err(|e| format!("Failed to decode Android VPN bridge class: {}", e))?;
        let bridge = jni::objects::JClass::from(bridge);
        let protected = env
            .call_static_method(
                bridge,
                "protectSocketFd",
                "(Landroid/content/Context;I)Z",
                &[JValue::Object(&activity), JValue::Int(fd)],
            )
            .map_err(|e| format!("Failed to protect Android socket fd: {}", e))?
            .z()
            .map_err(|e| format!("Failed to decode Android socket protect result: {}", e))?;

        Ok(protected)
    })
}

#[cfg(target_os = "android")]
#[allow(dead_code)]
async fn wait_for_android_tun_interface(app: &AppHandle) -> Result<String, String> {
    for _ in 0..20 {
        match android_tun_interface_ready() {
            Ok(true) => {
                return android_tunnel_debug_state();
            }
            Ok(false) => {}
            Err(error) => {
                let _ = app.emit(
                    "tunnel-log",
                    format!("[WARN] Android TUN interface check failed: {}", error),
                );
            }
        }

        sleep(Duration::from_millis(150)).await;
    }

    let state = android_tunnel_debug_state().unwrap_or_else(|_| "unknown".to_string());
    Err(format!(
        "Android VPN service started, but it did not expose a ready TUN interface in time. Last state: {}",
        state
    ))
}

#[cfg(target_os = "android")]
fn android_runtime_uses_tun_inbound(raw_config: &str) -> Result<bool, String> {
    let parsed = serde_json::from_str::<serde_json::Value>(raw_config).map_err(|e| {
        format!(
            "Failed to parse Android runtime config while checking TUN handoff readiness: {}",
            e
        )
    })?;

    let uses_tun = parsed
        .get("inbounds")
        .and_then(|value| value.as_array())
        .map(|inbounds| {
            inbounds.iter().any(|inbound| {
                inbound
                    .get("type")
                    .and_then(|value| value.as_str())
                    .map(|value| value == "tun")
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    Ok(uses_tun)
}

#[cfg(target_os = "android")]
fn android_runtime_context_path(local_data: &std::path::Path) -> std::path::PathBuf {
    local_data.join("android_runtime_context.json")
}

#[cfg(target_os = "android")]
fn android_backend_consumer_claim_path(local_data: &std::path::Path) -> std::path::PathBuf {
    local_data.join("android_backend_consumer_claim.json")
}

#[cfg(target_os = "android")]
fn android_native_backend_launch_bundle_path(local_data: &std::path::Path) -> std::path::PathBuf {
    local_data.join("android_native_backend_launch.json")
}

#[cfg(target_os = "android")]
fn android_native_backend_session_root(local_data: &std::path::Path) -> std::path::PathBuf {
    local_data.join("android_native_backend")
}

#[cfg(target_os = "android")]
fn android_native_backend_session_dir(
    local_data: &std::path::Path,
    session_id: &str,
) -> std::path::PathBuf {
    android_native_backend_session_root(local_data).join(session_id)
}

#[cfg(target_os = "android")]
fn android_runtime_launch_uses_tun(app: &AppHandle) -> Result<bool, String> {
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let config_path = local_data.join("client_config_android.json");
    let raw = std::fs::read_to_string(&config_path).map_err(|e| {
        format!(
            "Failed to read Android runtime config {} while checking launch mode: {}",
            config_path.display(),
            e
        )
    })?;
    android_runtime_uses_tun_inbound(&raw)
}

#[cfg(target_os = "android")]
fn create_android_handoff_session_id(tun_fd: i32) -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("android-handoff-{}-{}", millis, tun_fd)
}

#[cfg(target_os = "android")]
fn persist_android_runtime_context(
    local_data: &std::path::Path,
    snapshot: &AndroidRuntimeContextSnapshot,
) -> Result<std::path::PathBuf, String> {
    let context_path = android_runtime_context_path(local_data);
    let payload = serde_json::to_string_pretty(snapshot)
        .map_err(|e| format!("Failed to serialize Android runtime context: {}", e))?;
    std::fs::write(&context_path, payload).map_err(|e| {
        format!(
            "Failed to write Android runtime context {}: {}",
            context_path.display(),
            e
        )
    })?;
    Ok(context_path)
}

#[cfg(target_os = "android")]
fn persist_android_backend_consumer_claim(
    local_data: &std::path::Path,
    snapshot: &AndroidBackendConsumerClaimSnapshot,
) -> Result<std::path::PathBuf, String> {
    let claim_path = android_backend_consumer_claim_path(local_data);
    let payload = serde_json::to_string_pretty(snapshot)
        .map_err(|e| format!("Failed to serialize Android backend consumer claim: {}", e))?;
    std::fs::write(&claim_path, payload).map_err(|e| {
        format!(
            "Failed to write Android backend consumer claim {}: {}",
            claim_path.display(),
            e
        )
    })?;
    Ok(claim_path)
}

#[cfg(target_os = "android")]
fn persist_android_native_backend_launch_bundle(
    local_data: &std::path::Path,
    snapshot: &AndroidNativeBackendLaunchBundle,
) -> Result<std::path::PathBuf, String> {
    let launch_bundle_path = android_native_backend_launch_bundle_path(local_data);
    if let Some(parent) = launch_bundle_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Failed to create Android native backend launch bundle directory {}: {}",
                parent.display(),
                e
            )
        })?;
    }
    let payload = serde_json::to_string_pretty(snapshot).map_err(|e| {
        format!(
            "Failed to serialize Android native backend launch bundle: {}",
            e
        )
    })?;
    std::fs::write(&launch_bundle_path, payload).map_err(|e| {
        format!(
            "Failed to write Android native backend launch bundle {}: {}",
            launch_bundle_path.display(),
            e
        )
    })?;
    Ok(launch_bundle_path)
}

#[cfg(target_os = "android")]
fn load_android_runtime_context(
    local_data: &std::path::Path,
) -> Result<Option<AndroidRuntimeContextSnapshot>, String> {
    let context_path = android_runtime_context_path(local_data);
    if !context_path.exists() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(&context_path).map_err(|e| {
        format!(
            "Failed to read Android runtime context {}: {}",
            context_path.display(),
            e
        )
    })?;

    let snapshot = serde_json::from_str::<AndroidRuntimeContextSnapshot>(&raw).map_err(|e| {
        format!(
            "Failed to parse Android runtime context {}: {}",
            context_path.display(),
            e
        )
    })?;

    Ok(Some(snapshot))
}

#[cfg(target_os = "android")]
fn prepare_android_backend_consumer_handoff_inner(
    app: Option<&AppHandle>,
    local_data: &std::path::Path,
    consumer_tag: &str,
) -> Result<AndroidBackendConsumerClaimSnapshot, String> {
    let mut runtime_context = load_android_runtime_context(local_data)?
        .ok_or_else(|| "Android runtime handoff context is not available yet.".to_string())?;

    if runtime_context.backend_hint != "android_native_handoff_required"
        && runtime_context.backend_hint != "android_native_proxy_fallback"
    {
        return Err(format!(
            "Android runtime context is not in a supported Android backend mode. Current backend hint: {}",
            runtime_context.backend_hint
        ));
    }

    let claim_state = if runtime_context.backend_hint == "android_native_handoff_required" {
        android_claim_backend_handoff_session(&runtime_context.session_id, consumer_tag)?
    } else {
        "not-required(proxy-only)".to_string()
    };

    runtime_context.backend_session_state = claim_state.clone();
    runtime_context.consumer_tag = consumer_tag.to_string();
    runtime_context.consumer_claim_state = claim_state.clone();

    let claim_snapshot = AndroidBackendConsumerClaimSnapshot {
        session_id: runtime_context.session_id.clone(),
        consumer_tag: consumer_tag.to_string(),
        claim_state: claim_state.clone(),
        tun_fd: runtime_context.tun_fd,
        tun_state: runtime_context.tun_state.clone(),
        tun_address: runtime_context.tun_address.clone(),
        tun_prefix_length: runtime_context.tun_prefix_length,
        tun_route: runtime_context.tun_route.clone(),
        tun_mtu: runtime_context.tun_mtu,
        context_path: runtime_context.backend_session_context_path.clone(),
        backend_config_path: runtime_context.backend_config_path.clone(),
        log_path: runtime_context.log_path.clone(),
    };
    let claim_path = persist_android_backend_consumer_claim(local_data, &claim_snapshot)?;
    runtime_context.consumer_claim_path = claim_path.to_string_lossy().to_string();
    let _ = persist_android_runtime_context(local_data, &runtime_context)?;

    if let Some(app) = app {
        let _ = app.emit(
            "tunnel-log",
            format!(
                "[SYSTEM] Android backend consumer launch prepared: session={}, consumer={}, claim_state={}, claim_path={}",
                claim_snapshot.session_id,
                claim_snapshot.consumer_tag,
                claim_snapshot.claim_state,
                claim_path.display(),
            ),
        );
    }

    Ok(claim_snapshot)
}

#[cfg(target_os = "android")]
async fn start_android_native_backend_consumer_seam_inner(
    app: Option<&AppHandle>,
    local_data: &std::path::Path,
    consumer_tag: &str,
) -> Result<AndroidNativeBackendLaunchSnapshot, String> {
    let mut runtime_context = load_android_runtime_context(local_data)?
        .ok_or_else(|| "Android runtime handoff context is not available yet.".to_string())?;

    let claim_path = if runtime_context.consumer_claim_path.is_empty() {
        let claim_snapshot =
            prepare_android_backend_consumer_handoff_inner(app, local_data, consumer_tag)?;
        let claim_path = android_backend_consumer_claim_path(local_data);
        runtime_context.consumer_tag = claim_snapshot.consumer_tag;
        runtime_context.consumer_claim_state = claim_snapshot.claim_state;
        runtime_context.consumer_claim_path = claim_path.to_string_lossy().to_string();
        let _ = persist_android_runtime_context(local_data, &runtime_context)?;
        claim_path
    } else {
        std::path::PathBuf::from(&runtime_context.consumer_claim_path)
    };

    let session_dir = android_native_backend_session_dir(local_data, &runtime_context.session_id);
    std::fs::create_dir_all(&session_dir).map_err(|e| {
        format!(
            "Failed to create Android native backend session directory {}: {}",
            session_dir.display(),
            e
        )
    })?;
    let runtime_log_path = session_dir.join("runtime.log");
    let runtime_status_path = session_dir.join("status.json");
    let launch_bundle = AndroidNativeBackendLaunchBundle {
        session_id: runtime_context.session_id.clone(),
        consumer_tag: runtime_context.consumer_tag.clone(),
        backend_hint: runtime_context.backend_hint.clone(),
        tun_fd: runtime_context.tun_fd,
        tun_state: runtime_context.tun_state.clone(),
        tun_address: runtime_context.tun_address.clone(),
        tun_prefix_length: runtime_context.tun_prefix_length,
        tun_route: runtime_context.tun_route.clone(),
        tun_mtu: runtime_context.tun_mtu,
        config_path: runtime_context.config_path.clone(),
        backend_config_path: runtime_context.backend_config_path.clone(),
        context_path: runtime_context.backend_session_context_path.clone(),
        claim_path: claim_path.to_string_lossy().to_string(),
        log_path: runtime_context.log_path.clone(),
        session_dir: session_dir.to_string_lossy().to_string(),
        runtime_log_path: runtime_log_path.to_string_lossy().to_string(),
        runtime_status_path: runtime_status_path.to_string_lossy().to_string(),
        tun_fd_ownership: "caller_retains_original".to_string(),
        protect_api_available: runtime_context.protect_api_available,
    };
    let launch_bundle_path =
        persist_android_native_backend_launch_bundle(local_data, &launch_bundle)?;

    let launch_bundle_path_string = launch_bundle_path.to_string_lossy().to_string();
    let raw = tokio::time::timeout(Duration::from_secs(15), async move {
        tauri::async_runtime::spawn_blocking(move || {
            android_start_native_backend_seam(&launch_bundle_path_string)
        })
        .await
        .map_err(|error| format!("Android native backend seam task join failed: {}", error))?
    })
    .await
    .map_err(|_| {
        "Android native backend seam timed out after 15 seconds while waiting for libbox launch."
            .to_string()
    })??;
    let payload = serde_json::from_str::<serde_json::Value>(&raw).map_err(|e| {
        format!(
            "Failed to parse Android native backend seam launch result: {}",
            e
        )
    })?;

    let launch_state = payload
        .get("launch_state")
        .and_then(|value| value.as_str())
        .or_else(|| payload.get("phase").and_then(|value| value.as_str()))
        .unwrap_or("unknown")
        .to_string();
    let detail = payload
        .get("detail")
        .and_then(|value| value.as_str())
        .unwrap_or("Android native backend seam did not return a detail string.")
        .to_string();
    let runtime_name = payload
        .get("runtime_name")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .to_string();
    let runtime_selection = payload
        .get("runtime_selection")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let backend_config_summary = payload
        .get("backend_config_summary")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let status_path = payload
        .get("status_path")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
        .or_else(|| android_native_backend_status_path().ok())
        .unwrap_or_default();

    runtime_context.consumer_launch_state = launch_state.clone();
    runtime_context.consumer_launch_path = status_path.clone();
    runtime_context.consumer_launch_runtime = runtime_name.clone();
    runtime_context.consumer_launch_selection = runtime_selection.clone();
    runtime_context.consumer_launch_summary = backend_config_summary.clone();
    runtime_context.consumer_session_dir = session_dir.to_string_lossy().to_string();
    runtime_context.tun_fd_ownership = launch_bundle.tun_fd_ownership.clone();
    let _ = persist_android_runtime_context(local_data, &runtime_context)?;

    if let Some(app) = app {
        let _ = app.emit(
            "tunnel-log",
            format!(
                "[SYSTEM] Android native backend seam processed the launch bundle: state={}, bundle_path={}, status_path={}",
                launch_state,
                launch_bundle_path.display(),
                status_path
            ),
        );
    }

    if android_native_backend_launch_state_is_pending(&launch_state) {
        for _ in 0..120 {
            sleep(Duration::from_millis(300)).await;

            if let Some(updated) = load_android_native_backend_launch_status(&status_path)? {
                if !android_native_backend_launch_state_is_pending(&updated.launch_state) {
                    return Ok(updated);
                }
            }
        }

        return Err(format!(
            "Android native backend stayed in a pending launch state for too long. status_path={}, runtime={}, detail={}",
            status_path, runtime_name, detail
        ));
    }

    Ok(AndroidNativeBackendLaunchSnapshot {
        session_id: runtime_context.session_id,
        consumer_tag: runtime_context.consumer_tag,
        launch_state,
        detail,
        claim_path: claim_path.to_string_lossy().to_string(),
        launch_bundle_path: launch_bundle_path.to_string_lossy().to_string(),
        status_path,
        runtime_name,
        runtime_selection,
        backend_config_summary,
    })
}

#[cfg(target_os = "android")]
fn android_native_backend_launch_state_is_pending(state: &str) -> bool {
    android_backend_state_is_pending(state)
}

#[cfg(target_os = "android")]
#[allow(dead_code)]
fn summarize_android_command_failure(prefix: &str, output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !stderr.is_empty() && !stdout.is_empty() {
        format!("{prefix}: {stderr} | {stdout}")
    } else if !stderr.is_empty() {
        format!("{prefix}: {stderr}")
    } else if !stdout.is_empty() {
        format!("{prefix}: {stdout}")
    } else {
        format!(
            "{prefix}: process exited with code {:?}",
            output.status.code()
        )
    }
}

#[cfg(target_os = "android")]
fn ensure_android_config_has_no_local_proxy_inbounds(config_path: &str) -> Result<(), String> {
    if ANDROID_PROXY_FALLBACK_MODE {
        return Ok(());
    }

    let raw = std::fs::read_to_string(config_path).map_err(|e| {
        format!(
            "Failed to read Android client config for localhost proxy audit: {}",
            e
        )
    })?;
    let parsed = serde_json::from_str::<serde_json::Value>(&raw).map_err(|e| {
        format!(
            "Failed to parse Android client config for localhost proxy audit: {}",
            e
        )
    })?;

    let Some(inbounds) = parsed.get("inbounds").and_then(|value| value.as_array()) else {
        return Ok(());
    };

    for inbound in inbounds {
        let inbound_type = inbound
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let listen = inbound
            .get("listen")
            .and_then(|value| value.as_str())
            .unwrap_or_default();

        let is_local_proxy_inbound = matches!(inbound_type, "socks" | "mixed" | "http");
        let is_localhost_listener = matches!(listen, "127.0.0.1" | "::1" | "localhost");

        if is_local_proxy_inbound || is_localhost_listener {
            let listen_port = inbound
                .get("listen_port")
                .and_then(|value| value.as_u64())
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            return Err(format!(
                "Android security policy blocked a localhost proxy inbound (type='{}', listen='{}', port={}). RKN mobile currently permits only VPN/TUN-style inbounds to avoid localhost proxy leaks across apps.",
                inbound_type,
                if listen.is_empty() { "default" } else { listen },
                listen_port
            ));
        }
    }

    Ok(())
}

#[cfg(target_os = "android")]
#[allow(dead_code)]
fn run_android_singbox_preflight(singbox_path: &str, config_path: &str) -> Result<(), String> {
    ensure_android_config_has_no_local_proxy_inbounds(config_path)?;

    let singbox_dir = std::path::Path::new(singbox_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    let version_output = std::process::Command::new(singbox_path)
        .current_dir(singbox_dir)
        .arg("version")
        .output()
        .map_err(|e| format!("Failed to launch Android sing-box preflight: {}", e))?;

    if !version_output.status.success() {
        return Err(summarize_android_command_failure(
            "Android sing-box preflight failed",
            &version_output,
        ));
    }

    let check_output = std::process::Command::new(singbox_path)
        .current_dir(singbox_dir)
        .args(["check", "-c", config_path])
        .output()
        .map_err(|e| format!("Failed to run Android sing-box config preflight: {}", e))?;

    if !check_output.status.success() {
        return Err(summarize_android_command_failure(
            "Android sing-box config preflight failed",
            &check_output,
        ));
    }

    Ok(())
}

#[cfg(target_os = "android")]
fn prepare_android_runtime_launch(
    app: &AppHandle,
    local_data: &std::path::Path,
    config_path: &std::path::Path,
    log_path: &str,
    announce_prompt: bool,
) -> Result<AndroidRuntimeLaunchPlan, String> {
    let raw = std::fs::read_to_string(config_path).map_err(|e| {
        format!(
            "Failed to read base client config {}: {}",
            config_path.display(),
            e
        )
    })?;
    let android_config_path = local_data.join("client_config_android.json");
    let runtime_cfg = build_android_runtime_client_config(&raw, log_path)?;
    std::fs::write(&android_config_path, &runtime_cfg).map_err(|e| {
        format!(
            "Failed to write Android runtime client config {}: {}",
            android_config_path.display(),
            e
        )
    })?;
    let runtime_config_path = android_config_path.to_string_lossy().to_string();
    let android_backend_config_path = local_data.join("client_config_android_backend.json");
    let backend_cfg = build_android_handoff_backend_config(&runtime_cfg)?;
    std::fs::write(&android_backend_config_path, &backend_cfg).map_err(|e| {
        format!(
            "Failed to write Android handoff backend config {}: {}",
            android_backend_config_path.display(),
            e
        )
    })?;
    let backend_config_path = android_backend_config_path.to_string_lossy().to_string();

    if announce_prompt {
        let _ = app.emit(
            "tunnel-log",
            "[SYSTEM] Starting Android runtime negotiation without desktop elevation..."
                .to_string(),
        );
    }

    if android_runtime_uses_tun_inbound(&runtime_cfg)? {
        let tun_fd = android_peek_tun_fd().unwrap_or(-1);
        let session_id = create_android_handoff_session_id(tun_fd);
        let tun_state = android_tunnel_debug_state().unwrap_or_else(|_| "unknown".to_string());
        let tun_address = android_tun_address().unwrap_or_else(|_| "unknown".to_string());
        let tun_prefix_length = android_tun_prefix_length().unwrap_or(-1);
        let tun_route = android_tun_route().unwrap_or_else(|_| "unknown".to_string());
        let tun_mtu = android_tun_mtu().unwrap_or(-1);
        let placeholder_snapshot = AndroidRuntimeContextSnapshot {
            backend_hint: "android_native_handoff_required".to_string(),
            session_id: session_id.clone(),
            tun_fd,
            tun_state: tun_state.clone(),
            tun_address: tun_address.clone(),
            tun_prefix_length,
            tun_route: tun_route.clone(),
            tun_mtu,
            config_path: runtime_config_path.clone(),
            backend_config_path: backend_config_path.clone(),
            log_path: log_path.to_string(),
            protect_api_available: true,
            backend_session_state: "pending-registration".to_string(),
            backend_session_id: session_id.clone(),
            backend_session_context_path: String::new(),
            backend_session_config_path: backend_config_path.clone(),
            backend_session_log_path: log_path.to_string(),
            consumer_tag: String::new(),
            consumer_claim_state: "idle".to_string(),
            consumer_claim_path: String::new(),
            consumer_launch_state: "idle".to_string(),
            consumer_launch_path: String::new(),
            consumer_launch_runtime: String::new(),
            consumer_launch_selection: String::new(),
            consumer_launch_summary: String::new(),
            consumer_session_dir: String::new(),
            tun_fd_ownership: "caller_retains_original".to_string(),
        };
        let context_path = persist_android_runtime_context(local_data, &placeholder_snapshot)?;
        let backend_session_state = android_register_backend_handoff_session(
            &session_id,
            &context_path.to_string_lossy(),
            &backend_config_path,
            log_path,
            tun_fd,
        )
        .unwrap_or_else(|error| format!("registration_failed({})", error));
        let snapshot = AndroidRuntimeContextSnapshot {
            backend_session_state: backend_session_state.clone(),
            backend_session_id: android_backend_handoff_session_id()
                .unwrap_or_else(|_| session_id.clone()),
            backend_session_context_path: context_path.to_string_lossy().to_string(),
            ..placeholder_snapshot
        };
        let _ = persist_android_runtime_context(local_data, &snapshot)?;
        let _ = app.emit(
            "tunnel-log",
            format!(
                "[SYSTEM] Android TUN handoff checkpoint reached: session={}, VpnService owns fd={}, state={}, addr={}/{}, route={}, mtu={}, config={}, backend_config={}, log={}, context={}, backend_session={}. The next backend for 6A.4.1 must consume this Android-owned interface instead of launching the standalone CLI path.",
                session_id,
                tun_fd,
                tun_state,
                tun_address,
                tun_prefix_length,
                tun_route,
                tun_mtu,
                runtime_config_path,
                backend_config_path,
                log_path,
                context_path.display(),
                backend_session_state,
            ),
        );

        return Ok(AndroidRuntimeLaunchPlan::TunHandoffRequired {
            tun_fd,
            config_path: runtime_config_path,
            log_path: log_path.to_string(),
        });
    }

    let session_id = create_android_handoff_session_id(-1);
    let placeholder_snapshot = AndroidRuntimeContextSnapshot {
        backend_hint: "android_native_proxy_fallback".to_string(),
        session_id: session_id.clone(),
        tun_fd: -1,
        tun_state: "proxy-only".to_string(),
        tun_address: "n/a".to_string(),
        tun_prefix_length: -1,
        tun_route: "n/a".to_string(),
        tun_mtu: -1,
        config_path: runtime_config_path.clone(),
        backend_config_path: backend_config_path.clone(),
        log_path: log_path.to_string(),
        protect_api_available: false,
        backend_session_state: "not-required(proxy-only)".to_string(),
        backend_session_id: session_id.clone(),
        backend_session_context_path: String::new(),
        backend_session_config_path: backend_config_path.clone(),
        backend_session_log_path: log_path.to_string(),
        consumer_tag: String::new(),
        consumer_claim_state: "idle".to_string(),
        consumer_claim_path: String::new(),
        consumer_launch_state: "idle".to_string(),
        consumer_launch_path: String::new(),
        consumer_launch_runtime: String::new(),
        consumer_launch_selection: String::new(),
        consumer_launch_summary: String::new(),
        consumer_session_dir: String::new(),
        tun_fd_ownership: "proxy-only(no-vpn-fd)".to_string(),
    };
    let context_path = persist_android_runtime_context(local_data, &placeholder_snapshot)?;
    let snapshot = AndroidRuntimeContextSnapshot {
        backend_session_context_path: context_path.to_string_lossy().to_string(),
        ..placeholder_snapshot
    };
    let _ = persist_android_runtime_context(local_data, &snapshot)?;
    let _ = app.emit(
        "tunnel-log",
        format!(
            "[SYSTEM] Android proxy fallback prepared: session={}, config={}, backend_config={}, context={}. Starting the native backend without VpnService/TUN handoff.",
            session_id,
            runtime_config_path,
            backend_config_path,
            context_path.display(),
        ),
    );

    Ok(AndroidRuntimeLaunchPlan::ProxyOnly {
        config_path: runtime_config_path,
        log_path: log_path.to_string(),
    })
}

#[cfg(target_os = "android")]
#[allow(dead_code)]
async fn launch_tunnel_process_android(
    _app: &AppHandle,
    singbox_path: &str,
    config_str: &str,
    log_path: &str,
) -> Result<u32, String> {
    let singbox_dir = std::path::Path::new(singbox_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(log_path)
        .map_err(|e| format!("Failed to open Android sidecar log file: {}", e))?;
    let stderr_file = log_file
        .try_clone()
        .map_err(|e| format!("Failed to duplicate Android sidecar log handle: {}", e))?;

    let child = std::process::Command::new(singbox_path)
        .current_dir(singbox_dir)
        .args(["run", "-c", config_str])
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .map_err(|e| format!("Failed to launch Android sing-box sidecar: {}", e))?;

    Ok(child.id())
}

async fn launch_tunnel_process(app: &AppHandle, announce_prompt: bool) -> Result<u32, String> {
    #[cfg(not(target_os = "android"))]
    let singbox_path = resolve_singbox_path(app)?;

    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let config_path = local_data.join("client_config.json");

    if !config_path.exists() {
        return Err("Client config not found. Please deploy a server first.".to_string());
    }

    let log_path = tunnel_log_path();
    let _ = std::fs::remove_file(log_path);

    #[cfg(target_os = "windows")]
    {
        let runtime_mode = load_windows_runtime_mode(app)?;
        let _ = app.emit(
            "tunnel-log",
            format!(
                "[SYSTEM] Running local sing-box preflight for Windows {} mode...",
                match runtime_mode {
                    WindowsRuntimeMode::Tun => "TUN",
                    WindowsRuntimeMode::Compatibility => "compatibility",
                }
            ),
        );

        let win_config_path = local_data.join("client_config_win.json");
        let raw = std::fs::read_to_string(&config_path).map_err(|e| {
            format!(
                "Failed to read base client config {}: {}",
                config_path.display(),
                e
            )
        })?;
        let runtime_cfg = build_windows_runtime_client_config(&raw, log_path, runtime_mode)?;
        std::fs::write(&win_config_path, runtime_cfg).map_err(|e| {
            format!(
                "Failed to write Windows runtime client config {}: {}",
                win_config_path.display(),
                e
            )
        })?;
        let config_str = win_config_path.to_string_lossy().to_string();

        run_windows_singbox_preflight(&singbox_path, &config_str)?;

        match runtime_mode {
            WindowsRuntimeMode::Tun => {
                if announce_prompt {
                    let _ = app.emit(
                        "tunnel-log",
                        "[SYSTEM] Requesting administrator privileges...".to_string(),
                    );
                }

                launch_tunnel_process_windows(app, &singbox_path, &config_str, log_path).await
            }
            WindowsRuntimeMode::Compatibility => {
                if announce_prompt {
                    let _ = app.emit(
                        "tunnel-log",
                        "[SYSTEM] Starting Windows compatibility mode without TUN elevation..."
                            .to_string(),
                    );
                }

                launch_tunnel_process_windows_compatibility(
                    app,
                    &singbox_path,
                    &config_str,
                    log_path,
                )
                .await
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(target_os = "android")]
        {
            match prepare_android_runtime_launch(
                app,
                &local_data,
                &config_path,
                log_path,
                announce_prompt,
            )? {
                AndroidRuntimeLaunchPlan::TunHandoffRequired {
                    tun_fd,
                    config_path,
                    log_path,
                } => {
                    let consumer_claim = prepare_android_backend_consumer_handoff_inner(
                        Some(app),
                        &local_data,
                        "rkn_android_native_backend_seam",
                    )
                    .ok();
                    let consumer_launch = start_android_native_backend_consumer_seam_inner(
                        Some(app),
                        &local_data,
                        "rkn_android_native_backend_seam",
                    )
                    .await;
                    match consumer_launch {
                        Ok(launch) if launch.launch_state.starts_with("ready") => {
                            let _ = app.emit(
                                "tunnel-log",
                                format!(
                                    "[SYSTEM] Android native backend is ready: runtime={}, state={}, status_path={}",
                                    launch.runtime_name, launch.launch_state, launch.status_path
                                ),
                            );
                            return Ok(ANDROID_NATIVE_BACKEND_SENTINEL_PID);
                        }
                        Ok(launch) => {
                            return Err(format!(
                                "Android native backend launch failed after TUN handoff. VpnService fd={}, config={}, log={}. Consumer handoff{}; seam_state={}, runtime={}, detail={}, status_path={}",
                                tun_fd,
                                config_path,
                                log_path,
                                consumer_claim
                                    .as_ref()
                                    .map(|claim| format!(", session={}, claim_state={}, backend_config={}", claim.session_id, claim.claim_state, claim.backend_config_path))
                                    .unwrap_or_default(),
                                launch.launch_state,
                                launch.runtime_name,
                                launch.detail,
                                launch.status_path
                            ));
                        }
                        Err(error) => {
                            return Err(format!(
                                "Android native backend seam crashed after TUN handoff. VpnService fd={}, config={}, log={}. Consumer handoff{}; seam_error={}",
                                tun_fd,
                                config_path,
                                log_path,
                                consumer_claim
                                    .as_ref()
                                    .map(|claim| format!(", session={}, claim_state={}, backend_config={}", claim.session_id, claim.claim_state, claim.backend_config_path))
                                    .unwrap_or_default(),
                                error
                            ));
                        }
                    }
                }
                AndroidRuntimeLaunchPlan::ProxyOnly {
                    config_path,
                    log_path,
                } => {
                    let consumer_launch = start_android_native_backend_consumer_seam_inner(
                        Some(app),
                        &local_data,
                        "rkn_android_native_backend_seam",
                    )
                    .await;
                    match consumer_launch {
                        Ok(launch) if launch.launch_state.starts_with("ready") => {
                            let _ = app.emit(
                                "tunnel-log",
                                format!(
                                    "[SYSTEM] Android native backend proxy fallback is ready: runtime={}, state={}, status_path={}",
                                    launch.runtime_name, launch.launch_state, launch.status_path
                                ),
                            );
                            return Ok(ANDROID_NATIVE_BACKEND_SENTINEL_PID);
                        }
                        Ok(launch) => {
                            return Err(format!(
                                "Android native backend proxy fallback launch failed. Config={}, log={}; seam_state={}, runtime={}, detail={}, status_path={}",
                                config_path,
                                log_path,
                                launch.launch_state,
                                launch.runtime_name,
                                launch.detail,
                                launch.status_path
                            ));
                        }
                        Err(error) => {
                            return Err(format!(
                                "Android native backend proxy fallback seam crashed. Config={}, log={}; seam_error={}",
                                config_path, log_path, error
                            ));
                        }
                    }
                }
            }
        }

        #[cfg(not(target_os = "android"))]
        let (config_str, server_ip) = {
            let desktop_config_path = local_data.join("client_config_desktop.json");
            let raw = std::fs::read_to_string(&config_path).map_err(|e| {
                format!(
                    "Failed to read base client config {}: {}",
                    config_path.display(),
                    e
                )
            })?;
            let parsed = serde_json::from_str::<serde_json::Value>(&raw).map_err(|e| {
                format!(
                    "Failed to parse base client config {} before desktop startup: {}",
                    config_path.display(),
                    e
                )
            })?;
            let server_ip = extract_server_ip_from_config(&parsed);
            let runtime_cfg = build_desktop_runtime_client_config(&raw)?;
            std::fs::write(&desktop_config_path, runtime_cfg).map_err(|e| {
                format!(
                    "Failed to write desktop runtime client config {}: {}",
                    desktop_config_path.display(),
                    e
                )
            })?;
            (desktop_config_path.to_string_lossy().to_string(), server_ip)
        };

        #[cfg(not(target_os = "android"))]
        if announce_prompt {
            let _ = app.emit(
                "tunnel-log",
                "[SYSTEM] Requesting administrator privileges...".to_string(),
            );
        }

        #[cfg(not(target_os = "android"))]
        let route_prelude = macos_server_route_prelude(server_ip.as_deref());

        #[cfg(not(target_os = "android"))]
        let shell_cmd = format!(
            "{}{} run -c {} > {} 2>&1 & echo $!",
            route_prelude,
            shell_single_quote(&singbox_path),
            shell_single_quote(&config_str),
            shell_single_quote(log_path),
        );

        #[cfg(not(target_os = "android"))]
        let osascript_arg = format!(
            "do shell script \"{}\" with administrator privileges",
            escape_applescript(&shell_cmd)
        );

        #[cfg(not(target_os = "android"))]
        let output = app
            .shell()
            .command("osascript")
            .args(["-e", &osascript_arg])
            .output()
            .await
            .map_err(|e| format!("Failed to execute osascript: {}", e))?;

        #[cfg(not(target_os = "android"))]
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

        #[cfg(not(target_os = "android"))]
        let pid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        #[cfg(not(target_os = "android"))]
        pid_str
            .parse()
            .map_err(|_| format!("Failed to parse PID from: '{}'", pid_str))
    }
}

async fn restart_tunnel_process(app: &AppHandle, old_pid: u32) -> Result<u32, String> {
    #[cfg(not(target_os = "android"))]
    let singbox_path = resolve_singbox_path(app)?;
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let config_path = local_data.join("client_config.json");

    if !config_path.exists() {
        return Err("Client config not found. Please deploy a server first.".to_string());
    }

    let log_path = tunnel_log_path();
    let _ = std::fs::remove_file(log_path);

    let _ = app.emit(
        "tunnel-log",
        "[SYSTEM] Requesting administrator privileges to restart the tunnel...".to_string(),
    );

    #[cfg(target_os = "windows")]
    {
        let runtime_mode = load_windows_runtime_mode(app)?;
        let _ = terminate_root_process(Some(app), old_pid);
        sleep(Duration::from_secs(1)).await;
        let win_config_path = local_data.join("client_config_win.json");
        let raw = std::fs::read_to_string(&config_path).map_err(|e| {
            format!(
                "Failed to read base client config {}: {}",
                config_path.display(),
                e
            )
        })?;
        let runtime_cfg = build_windows_runtime_client_config(&raw, log_path, runtime_mode)?;
        std::fs::write(&win_config_path, runtime_cfg).map_err(|e| {
            format!(
                "Failed to write Windows runtime client config {}: {}",
                win_config_path.display(),
                e
            )
        })?;
        let config_str = win_config_path.to_string_lossy().to_string();
        match runtime_mode {
            WindowsRuntimeMode::Tun => {
                launch_tunnel_process_windows(app, &singbox_path, &config_str, log_path).await
            }
            WindowsRuntimeMode::Compatibility => {
                launch_tunnel_process_windows_compatibility(
                    app,
                    &singbox_path,
                    &config_str,
                    log_path,
                )
                .await
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(target_os = "android")]
        {
            if is_android_native_backend_pid(old_pid) {
                let _ = stop_android_tunnel_service();
            } else {
                let _ = terminate_root_process(Some(app), old_pid);
            }
            sleep(Duration::from_secs(1)).await;

            if !ANDROID_PROXY_FALLBACK_MODE {
                if !android_vpn_permission_granted()? {
                    return Err(
                        "Android VPN permission is required before restarting protection."
                            .to_string(),
                    );
                }

                start_android_tunnel_service()?;
                let _ = app.emit(
                    "tunnel-log",
                    "[SYSTEM] Android VPN service anchor restarted for tunnel recovery."
                        .to_string(),
                );
            }

            match prepare_android_runtime_launch(app, &local_data, &config_path, log_path, false)? {
                AndroidRuntimeLaunchPlan::TunHandoffRequired {
                    tun_fd,
                    config_path,
                    log_path,
                } => {
                    let consumer_claim = prepare_android_backend_consumer_handoff_inner(
                        Some(app),
                        &local_data,
                        "rkn_android_native_backend_seam",
                    )
                    .ok();
                    let consumer_launch = start_android_native_backend_consumer_seam_inner(
                        Some(app),
                        &local_data,
                        "rkn_android_native_backend_seam",
                    )
                    .await;
                    match consumer_launch {
                        Ok(launch) if launch.launch_state.starts_with("ready") => {
                            let _ = app.emit(
                                "tunnel-log",
                                format!(
                                    "[SYSTEM] Android native backend restarted successfully: runtime={}, state={}, status_path={}",
                                    launch.runtime_name, launch.launch_state, launch.status_path
                                ),
                            );
                            return Ok(ANDROID_NATIVE_BACKEND_SENTINEL_PID);
                        }
                        Ok(launch) => {
                            return Err(format!(
                                "Android native backend restart failed after TUN handoff. VpnService fd={}, config={}, log={}. Consumer handoff{}; seam_state={}, runtime={}, detail={}, status_path={}",
                                tun_fd,
                                config_path,
                                log_path,
                                consumer_claim
                                    .as_ref()
                                    .map(|claim| format!(", session={}, claim_state={}, backend_config={}", claim.session_id, claim.claim_state, claim.backend_config_path))
                                    .unwrap_or_default(),
                                launch.launch_state,
                                launch.runtime_name,
                                launch.detail,
                                launch.status_path
                            ));
                        }
                        Err(error) => {
                            return Err(format!(
                                "Android native backend seam crashed during restart after TUN handoff. VpnService fd={}, config={}, log={}. Consumer handoff{}; seam_error={}",
                                tun_fd,
                                config_path,
                                log_path,
                                consumer_claim
                                    .as_ref()
                                    .map(|claim| format!(", session={}, claim_state={}, backend_config={}", claim.session_id, claim.claim_state, claim.backend_config_path))
                                    .unwrap_or_default(),
                                error
                            ));
                        }
                    }
                }
                AndroidRuntimeLaunchPlan::ProxyOnly {
                    config_path,
                    log_path,
                } => {
                    let consumer_launch = start_android_native_backend_consumer_seam_inner(
                        Some(app),
                        &local_data,
                        "rkn_android_native_backend_seam",
                    )
                    .await;
                    match consumer_launch {
                        Ok(launch) if launch.launch_state.starts_with("ready") => {
                            let _ = app.emit(
                                "tunnel-log",
                                format!(
                                    "[SYSTEM] Android native backend proxy fallback restarted successfully: runtime={}, state={}, status_path={}",
                                    launch.runtime_name, launch.launch_state, launch.status_path
                                ),
                            );
                            return Ok(ANDROID_NATIVE_BACKEND_SENTINEL_PID);
                        }
                        Ok(launch) => {
                            return Err(format!(
                                "Android native backend proxy fallback restart failed. Config={}, log={}; seam_state={}, runtime={}, detail={}, status_path={}",
                                config_path,
                                log_path,
                                launch.launch_state,
                                launch.runtime_name,
                                launch.detail,
                                launch.status_path
                            ));
                        }
                        Err(error) => {
                            return Err(format!(
                                "Android native backend proxy fallback seam crashed during restart. Config={}, log={}; seam_error={}",
                                config_path, log_path, error
                            ));
                        }
                    }
                }
            }
        }

        #[cfg(not(target_os = "android"))]
        let (config_str, server_ip) = {
            let desktop_config_path = local_data.join("client_config_desktop.json");
            let raw = std::fs::read_to_string(&config_path).map_err(|e| {
                format!(
                    "Failed to read base client config {}: {}",
                    config_path.display(),
                    e
                )
            })?;
            let parsed = serde_json::from_str::<serde_json::Value>(&raw).map_err(|e| {
                format!(
                    "Failed to parse base client config {} before desktop restart: {}",
                    config_path.display(),
                    e
                )
            })?;
            let server_ip = extract_server_ip_from_config(&parsed);
            let runtime_cfg = build_desktop_runtime_client_config(&raw)?;
            std::fs::write(&desktop_config_path, runtime_cfg).map_err(|e| {
                format!(
                    "Failed to write desktop runtime client config {}: {}",
                    desktop_config_path.display(),
                    e
                )
            })?;
            (desktop_config_path.to_string_lossy().to_string(), server_ip)
        };

        #[cfg(not(target_os = "android"))]
        let route_prelude = macos_server_route_prelude(server_ip.as_deref());

        #[cfg(not(target_os = "android"))]
        let shell_cmd = format!(
            "kill {old_pid} >/dev/null 2>&1 || true\nsleep 1\nkill -9 {old_pid} >/dev/null 2>&1 || true\n{}{} run -c {} > {} 2>&1 & echo $!",
            route_prelude,
            shell_single_quote(&singbox_path),
            shell_single_quote(&config_str),
            shell_single_quote(log_path),
        );

        #[cfg(not(target_os = "android"))]
        let osascript_arg = format!(
            "do shell script \"{}\" with administrator privileges",
            escape_applescript(&shell_cmd)
        );

        #[cfg(not(target_os = "android"))]
        let output = app
            .shell()
            .command("osascript")
            .args(["-e", &osascript_arg])
            .output()
            .await
            .map_err(|e| format!("Failed to execute osascript: {}", e))?;

        #[cfg(not(target_os = "android"))]
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

        #[cfg(not(target_os = "android"))]
        let pid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        #[cfg(not(target_os = "android"))]
        pid_str
            .parse()
            .map_err(|_| format!("Failed to parse PID from: '{}'", pid_str))
    }
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

    #[cfg(target_os = "android")]
    {
        if is_android_native_backend_pid(pid) {
            let mut backend_state = "unknown".to_string();
            let mut tun_ready = false;
            let tun_required = android_runtime_launch_uses_tun(app).unwrap_or(false);

            for _ in 0..25 {
                backend_state =
                    android_native_backend_status_state().unwrap_or_else(|_| "unknown".to_string());
                tun_ready = android_tun_interface_ready().unwrap_or(false);

                if android_backend_state_is_ready(&backend_state) && (!tun_required || tun_ready) {
                    break;
                }

                if !android_backend_state_is_pending(&backend_state) {
                    break;
                }

                sleep(Duration::from_millis(400)).await;
            }

            if !android_backend_state_is_ready(&backend_state) || (tun_required && !tun_ready) {
                if tun_required {
                    let _ = stop_android_tunnel_service();
                }

                {
                    let mut guard = state.singbox_pid.lock().unwrap();
                    if guard.as_ref() == Some(&pid) {
                        *guard = None;
                    }
                }

                set_network_fingerprint(state, None);
                clear_saved_tunnel_pid(app);
                emit_tunnel_state(app, false);
                emit_guard_state(app, "inactive");

                let log_tail = recent_log_tail(log_path, 20);
                let details = if log_tail.is_empty() {
                    format!(
                        "Android native backend did not stay ready during startup. Backend state: {}, tun_required={}, tun_ready={}",
                        backend_state, tun_required, tun_ready
                    )
                } else {
                    format!(
                        "Android native backend did not stay ready during startup. Backend state: {}, tun_required={}, tun_ready={}\nRecent logs:\n{}",
                        backend_state, tun_required, tun_ready, log_tail
                    )
                };

                return Err(details);
            }

            set_network_fingerprint(state, current_network_fingerprint());
            reset_guard_state(state);
            save_tunnel_pid(app, pid)?;
            emit_tunnel_state(app, true);
            emit_guard_state(app, "active");
            return Ok(());
        }

        let log_tail = recent_log_tail(log_path, 20);
        if let Some(blocker) = classify_android_startup_blocker(&log_tail) {
            let _ = terminate_root_process(None, pid);
            let _ = stop_android_tunnel_service();

            {
                let mut guard = state.singbox_pid.lock().unwrap();
                if guard.as_ref() == Some(&pid) {
                    *guard = None;
                }
            }

            set_network_fingerprint(state, None);
            clear_saved_tunnel_pid(app);
            emit_tunnel_state(app, false);
            emit_guard_state(app, "inactive");

            let details = if log_tail.is_empty() {
                blocker
            } else {
                format!("{}\nRecent logs:\n{}", blocker, log_tail)
            };

            return Err(format!("Android tunnel startup blocked. {}", details));
        }
    }

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
        #[cfg(target_os = "windows")]
        let bootstrap_hint = app
            .path()
            .app_local_data_dir()
            .ok()
            .and_then(|dir| {
                std::fs::read_to_string(dir.join("elevated_singbox_bootstrap.err")).ok()
            })
            .map(|value| trim_utf8_bom(&value).trim().to_string())
            .filter(|value| !value.is_empty());

        #[cfg(not(target_os = "windows"))]
        let bootstrap_hint: Option<String> = None;

        let details = if let Some(hint) = bootstrap_hint {
            if log_tail.is_empty() {
                hint
            } else {
                format!("{}\nRecent logs:\n{}", hint, log_tail)
            }
        } else if log_tail.is_empty() {
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
        let mut suppressed_noisy_lines = 0usize;

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
                        if classify_noisy_android_core_info(&line) {
                            suppressed_noisy_lines += 1;
                            if suppressed_noisy_lines.is_multiple_of(100) {
                                let _ = app.emit(
                                    "tunnel-log",
                                    format!(
                                        "[CORE] [suppressed {} repetitive tun/direct info lines in the live UI; full details remain in the file log]",
                                        suppressed_noisy_lines
                                    ),
                                );
                            }
                            continue;
                        }

                        if suppressed_noisy_lines > 0 {
                            let _ = app.emit(
                                "tunnel-log",
                                format!(
                                    "[CORE] [resuming detailed live logs after suppressing {} repetitive tun/direct info lines]",
                                    suppressed_noisy_lines
                                ),
                            );
                            suppressed_noisy_lines = 0;
                        }

                        let _ = app.emit("tunnel-log", format!("[CORE] {}", line));
                        if classify_outdated_subordinate_config(&line) {
                            let _ = app.emit(
                                "subordinate-config-outdated",
                                "The subordinate tunnel config is outdated and must be refreshed."
                                    .to_string(),
                            );
                        }
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

            #[cfg(target_os = "android")]
            if is_android_native_backend_pid(pid) {
                let backend_state =
                    android_native_backend_status_state().unwrap_or_else(|_| "unknown".to_string());
                let tun_ready = android_tun_interface_ready().unwrap_or(false);
                if backend_state.starts_with("ready") && tun_ready {
                    continue;
                }

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

                let log_tail = recent_log_tail(tunnel_log_path(), 20);
                let details = if log_tail.is_empty() {
                    format!("Backend state: {}, tun_ready={}", backend_state, tun_ready)
                } else {
                    format!(
                        "Backend state: {}, tun_ready={}\nRecent logs:\n{}",
                        backend_state, tun_ready, log_tail
                    )
                };

                let _ = app.emit(
                    "tunnel-log",
                    format!(
                        "[SYSTEM] Android native backend stopped. Tunnel is no longer active. {}",
                        details
                    ),
                );
                let _ = stop_android_tunnel_service();
                emit_tunnel_state(&app, false);
                emit_guard_state(&app, "inactive");
                break;
            }

            if !process_exists(pid) {
                #[cfg(target_os = "windows")]
                {
                    // Old Windows systems can occasionally return a transient
                    // false negative from the task inspection path right after
                    // UAC/elevation handoff. Re-check once before declaring the
                    // tunnel dead.
                    sleep(Duration::from_millis(700)).await;
                    if process_exists(pid) {
                        continue;
                    }
                }

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

                let log_tail = recent_log_tail(tunnel_log_path(), 20);
                #[cfg(target_os = "windows")]
                let bootstrap_hint = app
                    .path()
                    .app_local_data_dir()
                    .ok()
                    .and_then(|dir| {
                        std::fs::read_to_string(dir.join("elevated_singbox_bootstrap.err")).ok()
                    })
                    .map(|value| trim_utf8_bom(&value).trim().to_string())
                    .filter(|value| !value.is_empty());

                #[cfg(not(target_os = "windows"))]
                let bootstrap_hint: Option<String> = None;

                let details = if let Some(hint) = bootstrap_hint {
                    if log_tail.is_empty() {
                        hint
                    } else {
                        format!("{}\nRecent logs:\n{}", hint, log_tail)
                    }
                } else if log_tail.is_empty() {
                    "No exit details captured.".to_string()
                } else {
                    format!("Recent logs:\n{}", log_tail)
                };

                let _ = app.emit(
                    "tunnel-log",
                    format!(
                        "[SYSTEM] Core process exited. Tunnel is no longer active. {}",
                        details
                    ),
                );
                #[cfg(target_os = "windows")]
                let _ = clear_windows_system_proxy();
                #[cfg(target_os = "android")]
                let _ = stop_android_tunnel_service();
                emit_tunnel_state(&app, false);
                emit_guard_state(&app, "inactive");
                break;
            }
        }
    });
}

fn spawn_network_recovery_monitor(app: AppHandle, pid: u32) {
    tauri::async_runtime::spawn(async move {
        let mut last_tick = std::time::Instant::now();
        #[cfg(target_os = "windows")]
        let mut adapter_missing = false;

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

            {
                let state = app.state::<AppState>();
                release_guard_after_quiet_period(&app, &state);
            }

            let elapsed = last_tick.elapsed();
            last_tick = std::time::Instant::now();
            let current_fingerprint = current_network_fingerprint();

            #[cfg(target_os = "windows")]
            {
                if elapsed >= Duration::from_secs(20) {
                    let state = app.state::<AppState>();
                    if begin_recovery(&state) {
                        if let Some(fingerprint) = current_fingerprint.clone() {
                            set_network_fingerprint(&state, Some(fingerprint));
                        } else {
                            set_network_fingerprint(&state, None);
                        }

                        reset_guard_state(&state);
                        emit_guard_state(&app, "active");
                        let _ = app.emit(
                            "tunnel-log",
                            "[SYSTEM] Windows resume detected after sleep or suspend. Keeping the tunnel active and reinitializing the recovery state for the current network context.".to_string(),
                        );
                        finish_recovery(&state);
                    }
                }

                if current_fingerprint.is_none() {
                    if !adapter_missing {
                        adapter_missing = true;
                        let state = app.state::<AppState>();
                        if begin_recovery(&state) {
                            set_network_fingerprint(&state, None);
                            let _ = app.emit(
                                "tunnel-log",
                                "[SYSTEM] Windows network adapter is temporarily unavailable. Waiting for the adapter to come back before refreshing the tunnel recovery state.".to_string(),
                            );
                            finish_recovery(&state);
                        }
                    }
                    continue;
                }

                if adapter_missing {
                    adapter_missing = false;
                    let state = app.state::<AppState>();
                    if begin_recovery(&state) {
                        if let Some(fingerprint) = current_fingerprint.clone() {
                            set_network_fingerprint(&state, Some(fingerprint));
                        }
                        reset_guard_state(&state);
                        emit_guard_state(&app, "active");
                        let _ = app.emit(
                            "tunnel-log",
                            "[SYSTEM] Windows network adapter reconnected. Keeping the tunnel active and refreshing the recovery state.".to_string(),
                        );
                        finish_recovery(&state);
                    }
                    continue;
                }
            }

            #[cfg(not(target_os = "windows"))]
            if elapsed >= Duration::from_secs(20) {
                let should_recover = {
                    let state = app.state::<AppState>();
                    begin_recovery(&state)
                };

                if should_recover {
                    let result = restart_tunnel_if_running(
                        &app,
                        "[SYSTEM] Resume detected after sleep or app suspension. Restarting the tunnel to refresh stale transport sessions.",
                    )
                    .await;

                    if let Err(error) = result {
                        let _ = app.emit(
                            "tunnel-log",
                            format!("[ERROR] Tunnel recovery after resume failed: {}", error),
                        );
                    }
                    break;
                }
            }

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

            #[cfg(target_os = "windows")]
            {
                if begin_recovery(&state) {
                    let was_engaged = *state.kill_switch_engaged.lock().unwrap();
                    let previous_failures = *state.proxy_failure_count.lock().unwrap();
                    reset_guard_state(&state);

                    if was_engaged || previous_failures > 0 {
                        emit_guard_state(&app, "active");
                    }

                    let _ = app.emit(
                        "tunnel-log",
                        "[SYSTEM] Windows network change detected. Keeping the tunnel active and refreshing the recovery state for the new adapter.".to_string(),
                    );
                    finish_recovery(&state);
                }
            }

            #[cfg(not(target_os = "windows"))]
            {
                let should_recover = {
                    let state = app.state::<AppState>();
                    begin_recovery(&state)
                };

                if should_recover {
                    let result = restart_tunnel_if_running(
                        &app,
                        "[SYSTEM] Network change detected. Restarting the tunnel to bind the transport to the new network context.",
                    )
                    .await;

                    if let Err(error) = result {
                        let _ = app.emit(
                            "tunnel-log",
                            format!(
                                "[ERROR] Tunnel recovery after network change failed: {}",
                                error
                            ),
                        );
                    }
                    break;
                }
            }
        }
    });
}

#[cfg(not(target_os = "android"))]
fn spawn_post_start_transport_sync_check(app: AppHandle, pid: u32) {
    tauri::async_runtime::spawn(async move {
        sleep(Duration::from_millis(500)).await;

        if let Err(error) = ssh::ensure_local_transport_is_current_quiet(&app).await {
            let still_current_tunnel = {
                let state = app.state::<AppState>();
                let current_pid = *state.singbox_pid.lock().unwrap();
                current_pid == Some(pid)
            };
            if !still_current_tunnel {
                return;
            }

            let _ = app.emit(
                "tunnel-log",
                format!(
                    "[ERROR] Remote transport verification failed after startup: {}",
                    error
                ),
            );
            let _ = stop_tunnel_inner(app.clone()).await;
        }
    });
}

#[cfg(target_os = "android")]
async fn wait_for_android_backend_shutdown(app: &AppHandle) {
    for _ in 0..15 {
        let backend_state =
            android_native_backend_status_state().unwrap_or_else(|_| "unknown".to_string());
        let tun_ready = android_tun_interface_ready().unwrap_or(false);

        if android_backend_state_is_stopped(&backend_state) && !tun_ready {
            return;
        }

        sleep(Duration::from_millis(200)).await;
    }

    let _ = app.emit(
        "tunnel-log",
        "[WARN] Android backend stop did not fully settle before the timeout window expired."
            .to_string(),
    );
}

#[cfg(target_os = "android")]
fn clear_android_native_backend_runtime_artifacts(app: &AppHandle) {
    let Ok(local_data) = app.path().app_local_data_dir() else {
        return;
    };

    if let Ok(path) = android_native_backend_status_path() {
        let _ = std::fs::remove_file(path);
    }

    let mut snapshot = match load_android_runtime_context(&local_data) {
        Ok(Some(snapshot)) => snapshot,
        _ => return,
    };

    if !snapshot.consumer_launch_path.is_empty() {
        let _ = std::fs::remove_file(&snapshot.consumer_launch_path);
    }

    snapshot.consumer_launch_state = "idle".to_string();
    snapshot.consumer_launch_path = String::new();
    snapshot.consumer_launch_runtime = String::new();
    snapshot.consumer_launch_selection = String::new();
    snapshot.consumer_launch_summary = String::new();
    snapshot.consumer_claim_state = "idle".to_string();
    snapshot.consumer_claim_path = String::new();
    snapshot.consumer_tag = String::new();
    snapshot.consumer_session_dir = String::new();
    snapshot.backend_session_state = "idle".to_string();
    snapshot.backend_session_id = String::new();
    snapshot.backend_session_context_path = String::new();
    snapshot.backend_session_config_path = snapshot.backend_config_path.clone();
    snapshot.backend_session_log_path = snapshot.log_path.clone();
    snapshot.tun_fd = -1;
    snapshot.tun_state = "idle".to_string();
    let _ = persist_android_runtime_context(&local_data, &snapshot);
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    use super::parse_windows_network_fingerprint_json;
    use super::{escape_applescript, shell_single_quote};

    #[test]
    fn escape_applescript_preserves_shell_special_chars_inside_string_literal() {
        let input = r#"/tmp/test's "path" $(whoami) `id` \ still-here"#;
        let escaped = escape_applescript(input);

        assert!(escaped.contains("test's"));
        assert!(escaped.contains("\\\"path\\\""));
        assert!(escaped.contains("$(whoami)"));
        assert!(escaped.contains("`id`"));
        assert!(escaped.contains("\\\\ still-here"));
    }

    #[test]
    fn shell_single_quote_safely_quotes_special_path() {
        let input = r#"/tmp/test's "path" $(whoami) `id`"#;
        let quoted = shell_single_quote(input);

        assert_eq!(quoted, r#"'/tmp/test'"'"'s "path" $(whoami) `id`'"#);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn parse_windows_network_fingerprint_json_builds_stable_fingerprint() {
        let sample = r#"[{"alias":"Wi-Fi","index":17,"ipv4":"192.168.1.25","gateway":"192.168.1.1","dns":"1.1.1.1,8.8.8.8"},{"alias":"Ethernet","index":7,"ipv4":"10.0.0.14","gateway":"10.0.0.1","dns":"10.0.0.1"}]"#;
        let parsed = parse_windows_network_fingerprint_json(sample);

        assert_eq!(
            parsed.as_deref(),
            Some("17|Wi-Fi|192.168.1.25|192.168.1.1|1.1.1.1,8.8.8.8;7|Ethernet|10.0.0.14|10.0.0.1|10.0.0.1")
        );
    }
}

async fn start_tunnel_inner(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let guard = state.singbox_pid.lock().unwrap();
        if guard.is_some() {
            return Err("Tunnel is already running".to_string());
        }
    }

    if local_client_config_requires_refresh(&app)? {
        let _ = app.emit(
            "tunnel-log",
            "[SYSTEM] Local client config uses an outdated DNS/FakeIP format. Run Update/Deploy once to regenerate it for the current core.".to_string(),
        );
        return Err(
            "Local client config is outdated. Run Update/Deploy before starting the tunnel."
                .to_string(),
        );
    }

    #[cfg(target_os = "android")]
    ssh::ensure_local_transport_is_current(&app).await?;
    crate::geodata::ensure_local_client_rule_sets(&app).await?;

    #[cfg(target_os = "android")]
    {
        clear_android_native_backend_runtime_artifacts(&app);

        if ANDROID_PROXY_FALLBACK_MODE {
            let _ = app.emit(
                "tunnel-log",
                "[SYSTEM] Android proxy fallback mode is active. Starting the local core without a VpnService anchor."
                    .to_string(),
            );
        } else {
            if !android_vpn_permission_granted()? {
                let _ = app.emit(
                    "tunnel-log",
                    "[SYSTEM] Android VPN permission is required before protection can start. Opening the system prompt now.".to_string(),
                );

                if !request_android_vpn_permission()? {
                    return Err(
                        "Android VPN permission requested. Approve it in the system dialog, then tap Start Protection again."
                            .to_string(),
                    );
                }
            }

            start_android_tunnel_service()?;
            let _ = app.emit(
                "tunnel-log",
                "[SYSTEM] Android VPN service anchor is active. Starting the local core next."
                    .to_string(),
            );
            let _ = app.emit(
                "tunnel-log",
                "[SYSTEM] Android VpnService foreground anchor is ready. The native backend will establish the real TUN interface during handoff."
                    .to_string(),
            );
        }
    }

    let _ = app.emit("tunnel-log", "[SYSTEM] Resolving core binary path...");
    let pid = match launch_tunnel_process(&app, true).await {
        Ok(pid) => pid,
        Err(error) => {
            #[cfg(target_os = "android")]
            let _ = stop_android_tunnel_service();
            return Err(error);
        }
    };

    #[cfg(target_os = "windows")]
    let runtime_mode = load_windows_runtime_mode(&app)?;

    let _ = app.emit(
        "tunnel-log",
        if cfg!(target_os = "android") && is_android_native_backend_pid(pid) {
            "[SYSTEM] Android native backend session started inside the app process.".to_string()
        } else {
            format!("[SYSTEM] Core process started with PID {}.", pid)
        },
    );

    verify_tunnel_start(&app, &state, pid, tunnel_log_path())
        .await
        .inspect_err(|_| {
            #[cfg(target_os = "android")]
            let _ = stop_android_tunnel_service();
        })?;
    spawn_log_reader(app.clone(), pid, tunnel_log_path());
    spawn_process_exit_monitor(app.clone(), pid);
    spawn_network_recovery_monitor(app.clone(), pid);
    #[cfg(not(target_os = "android"))]
    spawn_post_start_transport_sync_check(app.clone(), pid);

    #[cfg(target_os = "windows")]
    {
        let message = match runtime_mode {
            WindowsRuntimeMode::Tun => "[SYSTEM] TUN adapter initialized. Routing active.",
            WindowsRuntimeMode::Compatibility => {
                "[SYSTEM] Windows compatibility mode is active. System proxy routing is enabled."
            }
        };
        let _ = app.emit("tunnel-log", message.to_string());
    }

    #[cfg(not(target_os = "windows"))]
    {
        #[cfg(target_os = "android")]
        let _ = app.emit("tunnel-log", {
            if ANDROID_PROXY_FALLBACK_MODE {
                "[SYSTEM] Android proxy fallback runtime is active. The local core is running without VpnService/TUN.".to_string()
            } else {
                "[SYSTEM] Android protection runtime is active. The VPN service anchor and local core are now running."
                    .to_string()
            }
        });

        #[cfg(not(target_os = "android"))]
        let _ = app.emit(
            "tunnel-log",
            "[SYSTEM] TUN adapter initialized. Routing active.".to_string(),
        );
    }
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
                if cfg!(target_os = "android") && is_android_native_backend_pid(pid) {
                    "[SYSTEM] Stopping Android native backend...".to_string()
                } else {
                    format!("[SYSTEM] Stopping core process (PID {})...", pid)
                },
            );

            if cfg!(target_os = "android") && is_android_native_backend_pid(pid) {
                #[cfg(target_os = "android")]
                {
                    let _ = stop_android_tunnel_service();
                    wait_for_android_backend_shutdown(&app).await;
                    clear_android_native_backend_runtime_artifacts(&app);
                }
                let _ = app.emit(
                    "tunnel-log",
                    "[SYSTEM] Android native backend terminated. Routing disabled.".to_string(),
                );
            } else if terminate_root_process(Some(&app), pid).is_ok() {
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

            #[cfg(not(target_os = "android"))]
            let _ = std::fs::remove_file(tunnel_log_path());
            clear_saved_tunnel_pid(&app);
            set_network_fingerprint(&state, None);
            finish_recovery(&state);
            reset_guard_state(&state);
            #[cfg(target_os = "windows")]
            let _ = clear_windows_system_proxy();
            #[cfg(target_os = "android")]
            if !is_android_native_backend_pid(pid) {
                let _ = stop_android_tunnel_service();
                wait_for_android_backend_shutdown(&app).await;
                clear_android_native_backend_runtime_artifacts(&app);
            }
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
            #[cfg(target_os = "windows")]
            let _ = clear_windows_system_proxy();
            #[cfg(target_os = "android")]
            {
                let _ = stop_android_tunnel_service();
                wait_for_android_backend_shutdown(&app).await;
                clear_android_native_backend_runtime_artifacts(&app);
            }
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
        local_data.join("client_config_win.json"),
        local_data.join("server_profile.json"),
        local_data.join("active_tunnel_pid"),
        local_data.join("elevated_singbox_bootstrap.err"),
        local_data.join("elevated_singbox_bootstrap.ps1"),
        local_data.join("elevated_singbox.pid"),
        local_data.join("windows_runtime_mode.json"),
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

    ssh::clear_issued_invites(&app)?;
    ssh::clear_local_warp_profile_sync(&app)?;
    ssh::clear_cached_transport_bootstrap(&app)?;
    ssh::clear_backend_app_role(&app)?;

    let _ = fs::remove_file(tunnel_log_path());
    set_network_fingerprint(&state, None);
    finish_recovery(&state);
    reset_guard_state(&state);
    emit_tunnel_state(&app, false);
    emit_guard_state(&app, "inactive");
    refresh_tray_toggle_item(&app);
    let _ = app.emit(
        "tunnel-log",
        "[SYSTEM] Local server profile, client config, and imported WARP profile were removed from this Mac.".to_string(),
    );

    Ok(())
}

#[tauri::command]
async fn get_windows_runtime_mode(app: AppHandle) -> Result<WindowsRuntimeModeStatus, String> {
    Ok(WindowsRuntimeModeStatus {
        mode: load_windows_runtime_mode(&app)?,
        is_windows: cfg!(target_os = "windows"),
        supports_compatibility_mode: cfg!(target_os = "windows"),
    })
}

#[tauri::command]
async fn get_android_runtime_context(app: AppHandle) -> Result<Option<serde_json::Value>, String> {
    #[cfg(target_os = "android")]
    {
        let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
        let snapshot = load_android_runtime_context(&local_data)?.map(|mut snapshot| {
            if let Ok(live_backend_state) = android_backend_handoff_state() {
                snapshot.backend_session_state = live_backend_state;
            }
            if let Ok(live_backend_session_id) = android_backend_handoff_session_id() {
                if !live_backend_session_id.is_empty() {
                    snapshot.backend_session_id = live_backend_session_id;
                }
            }
            if let Ok(live_consumer_launch_state) = android_native_backend_status_state() {
                snapshot.consumer_launch_state = live_consumer_launch_state;
            }
            if let Ok(live_consumer_launch_path) = android_native_backend_status_path() {
                snapshot.consumer_launch_path = live_consumer_launch_path;
            }
            snapshot
        });
        let value = serde_json::to_value(snapshot)
            .map_err(|e| format!("Failed to encode Android runtime context: {}", e))?;
        Ok(Some(value))
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(None)
    }
}

#[tauri::command]
async fn claim_android_backend_session(
    session_id: String,
    consumer_tag: String,
) -> Result<Option<String>, String> {
    #[cfg(target_os = "android")]
    {
        let state = android_claim_backend_handoff_session(&session_id, &consumer_tag)?;
        Ok(Some(state))
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = session_id;
        let _ = consumer_tag;
        Ok(None)
    }
}

#[tauri::command]
async fn prepare_android_backend_consumer_handoff(
    app: AppHandle,
    consumer_tag: String,
) -> Result<Option<serde_json::Value>, String> {
    #[cfg(target_os = "android")]
    {
        let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
        let claim_snapshot =
            prepare_android_backend_consumer_handoff_inner(Some(&app), &local_data, &consumer_tag)?;
        let claim_path = android_backend_consumer_claim_path(&local_data);
        let response = serde_json::json!({
            "session_id": claim_snapshot.session_id,
            "consumer_tag": claim_snapshot.consumer_tag,
            "claim_state": claim_snapshot.claim_state,
            "claim_path": claim_path.to_string_lossy().to_string(),
            "context_path": claim_snapshot.context_path,
            "backend_config_path": claim_snapshot.backend_config_path,
            "log_path": claim_snapshot.log_path,
            "tun_fd": claim_snapshot.tun_fd,
            "tun_state": claim_snapshot.tun_state,
            "tun_address": claim_snapshot.tun_address,
            "tun_prefix_length": claim_snapshot.tun_prefix_length,
            "tun_route": claim_snapshot.tun_route,
            "tun_mtu": claim_snapshot.tun_mtu,
        });

        Ok(Some(response))
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        let _ = consumer_tag;
        Ok(None)
    }
}

#[tauri::command]
async fn start_android_native_backend_consumer_seam(
    app: AppHandle,
    consumer_tag: String,
) -> Result<Option<serde_json::Value>, String> {
    #[cfg(target_os = "android")]
    {
        let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
        let launch_snapshot = start_android_native_backend_consumer_seam_inner(
            Some(&app),
            &local_data,
            &consumer_tag,
        )
        .await?;
        let response = serde_json::json!({
            "session_id": launch_snapshot.session_id,
            "consumer_tag": launch_snapshot.consumer_tag,
            "launch_state": launch_snapshot.launch_state,
            "detail": launch_snapshot.detail,
            "claim_path": launch_snapshot.claim_path,
            "launch_bundle_path": launch_snapshot.launch_bundle_path,
            "status_path": launch_snapshot.status_path,
            "runtime_name": launch_snapshot.runtime_name,
            "runtime_selection": launch_snapshot.runtime_selection,
            "backend_config_summary": launch_snapshot.backend_config_summary,
        });
        Ok(Some(response))
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        let _ = consumer_tag;
        Ok(None)
    }
}

#[tauri::command]
async fn update_android_backend_consumer_state(
    session_id: String,
    consumer_tag: String,
    phase: String,
    detail: String,
) -> Result<Option<String>, String> {
    #[cfg(target_os = "android")]
    {
        let state = android_update_backend_handoff_session_state(
            &session_id,
            &consumer_tag,
            &phase,
            &detail,
        )?;
        Ok(Some(state))
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = session_id;
        let _ = consumer_tag;
        let _ = phase;
        let _ = detail;
        Ok(None)
    }
}

#[tauri::command]
async fn set_windows_runtime_mode(
    app: AppHandle,
    mode: WindowsRuntimeMode,
) -> Result<WindowsRuntimeModeStatus, String> {
    if !cfg!(target_os = "windows") {
        return Ok(WindowsRuntimeModeStatus {
            mode: WindowsRuntimeMode::Tun,
            is_windows: false,
            supports_compatibility_mode: false,
        });
    }

    save_windows_runtime_mode(&app, mode)?;

    let _ = app.emit(
        "tunnel-log",
        match mode {
            WindowsRuntimeMode::Tun => {
                "[SYSTEM] Windows runtime mode switched to TUN mode.".to_string()
            }
            WindowsRuntimeMode::Compatibility => {
                "[SYSTEM] Windows runtime mode switched to Compatibility Mode (system proxy, no TUN).".to_string()
            }
        },
    );

    Ok(WindowsRuntimeModeStatus {
        mode,
        is_windows: true,
        supports_compatibility_mode: true,
    })
}

pub(crate) async fn restart_tunnel_if_running(
    app: &AppHandle,
    reason: &str,
) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let old_pid = *state.singbox_pid.lock().unwrap();
    let Some(old_pid) = old_pid else {
        return Ok(false);
    };

    let _ = app.emit("tunnel-log", format!("[SYSTEM] {}", reason));

    let new_pid = match restart_tunnel_process(app, old_pid).await {
        Ok(pid) => pid,
        Err(error) => {
            let old_runtime_still_alive = {
                #[cfg(target_os = "android")]
                {
                    if is_android_native_backend_pid(old_pid) {
                        android_native_backend_status_state()
                            .map(|state| android_backend_state_is_ready(&state))
                            .unwrap_or(false)
                    } else {
                        process_exists(old_pid)
                    }
                }

                #[cfg(not(target_os = "android"))]
                {
                    process_exists(old_pid)
                }
            };

            if old_runtime_still_alive {
                {
                    let mut guard = state.singbox_pid.lock().unwrap();
                    *guard = Some(old_pid);
                }
                let _ = save_tunnel_pid(app, old_pid);
                finish_recovery(&state);
                emit_tunnel_state(app, true);
                refresh_tray_toggle_item(app);
                let _ = app.emit(
                    "tunnel-log",
                    "[SYSTEM] Tunnel restart did not complete, but the previous runtime is still alive. Keeping it tracked instead of leaving an orphaned tunnel state.",
                );
                return Err(error);
            }

            set_network_fingerprint(&state, None);
            clear_saved_tunnel_pid(app);
            finish_recovery(&state);
            reset_guard_state(&state);
            emit_tunnel_state(app, false);
            emit_guard_state(app, "inactive");
            refresh_tray_toggle_item(app);
            return Err(error);
        }
    };

    verify_tunnel_start(app, &state, new_pid, tunnel_log_path()).await?;
    spawn_log_reader(app.clone(), new_pid, tunnel_log_path());
    spawn_process_exit_monitor(app.clone(), new_pid);
    spawn_network_recovery_monitor(app.clone(), new_pid);
    finish_recovery(&state);
    let _ = app.emit(
        "tunnel-log",
        "[SYSTEM] Tunnel restarted with the updated local configuration.".to_string(),
    );
    refresh_tray_toggle_item(app);

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

    #[cfg(target_os = "android")]
    if is_android_native_backend_pid(saved_pid) {
        let backend_state =
            android_native_backend_status_state().unwrap_or_else(|_| "unknown".to_string());
        let tun_ready = android_tun_interface_ready().unwrap_or(false);
        if !backend_state.starts_with("ready") || !tun_ready {
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
        spawn_log_reader(app.clone(), saved_pid, tunnel_log_path());
        spawn_process_exit_monitor(app.clone(), saved_pid);
        spawn_network_recovery_monitor(app.clone(), saved_pid);
        refresh_tray_toggle_item(&app);

        return Ok(Some(saved_pid));
    }

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
    spawn_log_reader(app.clone(), saved_pid, tunnel_log_path());
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
            proxy_failure_window_started: Mutex::new(None),
            kill_switch_engaged: Mutex::new(false),
            #[cfg(desktop)]
            tray_toggle_item: Mutex::new(None),
            #[cfg(target_os = "windows")]
            windows_tray_notice_shown: Mutex::new(false),
        })
        .setup(|app| {
            #[cfg(not(desktop))]
            let _ = app;

            #[cfg(desktop)]
            {
                let app_handle = app.app_handle().clone();
                let toggle_item = MenuItemBuilder::with_id("toggle_tunnel", "Start Tunnel")
                    .enabled(client_config_exists(&app_handle))
                    .build(app)?;
                let settings_item =
                    MenuItemBuilder::with_id("open_settings", "Settings").build(app)?;
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
                    .on_tray_icon_event(|tray, event| {
                        #[cfg(target_os = "windows")]
                        {
                            let app = tray.app_handle();
                            match event {
                                TrayIconEvent::Click {
                                    button: MouseButton::Left,
                                    button_state: MouseButtonState::Up,
                                    ..
                                }
                                | TrayIconEvent::DoubleClick {
                                    button: MouseButton::Left,
                                    ..
                                } => {
                                    show_main_window(&app, None);
                                }
                                _ => {}
                            }
                        }

                        #[cfg(not(target_os = "windows"))]
                        {
                            let _ = (tray, event);
                        }
                    })
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
                            quit_application(app);
                        }
                        _ => {}
                    })
                    .build(app)?;

                refresh_tray_toggle_item(&app_handle);
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            #[cfg(desktop)]
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                #[cfg(target_os = "windows")]
                maybe_announce_windows_tray_behavior(&window.app_handle());
                api.prevent_close();
            }

            #[cfg(not(desktop))]
            {
                let _ = (window, event);
            }
        })
        .invoke_handler(tauri::generate_handler![
            start_tunnel,
            stop_tunnel,
            get_windows_runtime_mode,
            get_android_runtime_context,
            claim_android_backend_session,
            prepare_android_backend_consumer_handoff,
            start_android_native_backend_consumer_seam,
            update_android_backend_consumer_state,
            set_windows_runtime_mode,
            reset_local_data,
            restore_tunnel_session,
            get_tunnel_log_tail,
            write_clipboard_text,
            read_clipboard_text,
            get_android_vpn_permission_status,
            ssh::deploy_server,
            ssh::generate_invite_link,
            ssh::import_invite_link,
            ssh::get_local_installation_state,
            ssh::list_issued_invite_links,
            ssh::delete_issued_invite_link,
            ssh::get_transport_state_snapshot,
            ssh::load_saved_server_profile,
            ssh::get_local_warp_profile_status,
            ssh::import_local_warp_profile,
            ssh::bootstrap_local_warp_profile,
            ssh::bootstrap_local_warp_profile_from_credentials,
            ssh::clear_local_warp_profile,
            ssh::check_server_status,
            ssh::rotate_sni
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            #[cfg(all(desktop, target_os = "macos"))]
            if let RunEvent::Reopen { .. } = event {
                show_main_window(app, None);
            }

            #[cfg(not(all(desktop, target_os = "macos")))]
            let _ = (app, event);
        });
}
