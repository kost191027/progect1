#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::io::Write;
use std::sync::Mutex;
use std::{fs, path::PathBuf};
use tauri::image::Image;
use tauri::menu::{MenuBuilder, MenuItem, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
#[cfg(target_os = "windows")]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
#[cfg(target_os = "macos")]
use tauri::RunEvent;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent, Wry};
#[cfg(not(target_os = "windows"))]
use tauri_plugin_shell::ShellExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::{sleep, Duration};

mod generator;
mod geodata;
#[path = "ssh/mod.rs"]
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
    #[cfg(target_os = "windows")]
    windows_tray_notice_shown: Mutex<bool>,
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

#[cfg(not(target_os = "windows"))]
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

#[cfg(not(target_os = "windows"))]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

#[cfg(not(target_os = "windows"))]
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

    #[cfg(not(target_os = "windows"))]
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

#[cfg(target_os = "windows")]
fn trim_utf8_bom(value: &str) -> &str {
    value.strip_prefix('\u{feff}').unwrap_or(value)
}

/// Extract the VPN server IP from the client config's outbound section.
#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
fn build_windows_runtime_client_config(raw_config: &str, log_path: &str) -> Result<String, String> {
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
                // Windows 8 compatibility profile:
                // - do not force a fixed adapter name on older systems
                // - strict_route: false — sing-box uses Windows Filtering
                //   Platform (WFP) for strict routing, but the required WFP
                //   calls crash on Windows 8.  auto_route alone captures
                //   traffic via 0.0.0.0/1 + 128.0.0.0/1 routes.
                // - gvisor stack — the ONLY stack that works on Windows 8.
                //   Both "system" and "mixed" stacks use native OS TCP APIs
                //   that are unavailable on Windows 8, causing sing-box to
                //   crash immediately after TUN initialization.
                object.remove("interface_name");
                object.insert("strict_route".to_string(), serde_json::json!(false));
                object.insert("stack".to_string(), serde_json::json!("gvisor"));
            }
        }
    }

    if let Some(route) = cfg.get_mut("route").and_then(|value| value.as_object_mut()) {
        // auto_detect_interface MUST stay true so that the "direct" outbound
        // binds to the physical network adapter instead of the TUN. Without it,
        // traffic to the VPN server goes through the TUN → routing loop.
        route.insert("auto_detect_interface".to_string(), serde_json::json!(true));
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

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("Clipboard read is not implemented for this platform yet.".to_string())
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

#[cfg(target_os = "windows")]
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

async fn launch_tunnel_process(app: &AppHandle, announce_prompt: bool) -> Result<u32, String> {
    let singbox_path = resolve_singbox_path()?;

    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let config_path = local_data.join("client_config.json");

    if !config_path.exists() {
        return Err("Client config not found. Please deploy a server first.".to_string());
    }

    let log_path = tunnel_log_path();
    let _ = std::fs::remove_file(log_path);

    #[cfg(target_os = "windows")]
    {
        let _ = app.emit(
            "tunnel-log",
            "[SYSTEM] Running local sing-box preflight...".to_string(),
        );

        let win_config_path = local_data.join("client_config_win.json");
        let raw = std::fs::read_to_string(&config_path).map_err(|e| {
            format!(
                "Failed to read base client config {}: {}",
                config_path.display(),
                e
            )
        })?;
        let runtime_cfg = build_windows_runtime_client_config(&raw, log_path)?;
        std::fs::write(&win_config_path, runtime_cfg).map_err(|e| {
            format!(
                "Failed to write Windows runtime client config {}: {}",
                win_config_path.display(),
                e
            )
        })?;
        let config_str = win_config_path.to_string_lossy().to_string();

        run_windows_singbox_preflight(&singbox_path, &config_str)?;

        if announce_prompt {
            let _ = app.emit(
                "tunnel-log",
                "[SYSTEM] Requesting administrator privileges...".to_string(),
            );
        }

        launch_tunnel_process_windows(app, &singbox_path, &config_str, log_path).await
    }

    #[cfg(not(target_os = "windows"))]
    {
        let config_str = config_path.to_string_lossy().to_string();

        if announce_prompt {
            let _ = app.emit(
                "tunnel-log",
                "[SYSTEM] Requesting administrator privileges...".to_string(),
            );
        }

        let shell_cmd = format!(
            "{} run -c {} > {} 2>&1 & echo $!",
            shell_single_quote(&singbox_path),
            shell_single_quote(&config_str),
            shell_single_quote(log_path),
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
}

async fn restart_tunnel_process(app: &AppHandle, old_pid: u32) -> Result<u32, String> {
    let singbox_path = resolve_singbox_path()?;
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
        let runtime_cfg = build_windows_runtime_client_config(&raw, log_path)?;
        std::fs::write(&win_config_path, runtime_cfg).map_err(|e| {
            format!(
                "Failed to write Windows runtime client config {}: {}",
                win_config_path.display(),
                e
            )
        })?;
        let config_str = win_config_path.to_string_lossy().to_string();
        launch_tunnel_process_windows(app, &singbox_path, &config_str, log_path).await
    }

    #[cfg(not(target_os = "windows"))]
    {
        let config_str = config_path.to_string_lossy().to_string();
        let shell_cmd = format!(
            "kill {old_pid} >/dev/null 2>&1 || true\nsleep 1\nkill -9 {old_pid} >/dev/null 2>&1 || true\n{} run -c {} > {} 2>&1 & echo $!",
            shell_single_quote(&singbox_path),
            shell_single_quote(&config_str),
            shell_single_quote(log_path),
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
        #[cfg(target_os = "windows")]
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

            let current_fingerprint = current_network_fingerprint();

            #[cfg(target_os = "windows")]
            {
                let elapsed = last_tick.elapsed();
                last_tick = std::time::Instant::now();

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
                let _ = app.emit(
                    "tunnel-log",
                    "[SYSTEM] Network change detected. Keeping tunnel active and updating the network context.".to_string(),
                );
            }
        }
    });
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

    ssh::ensure_local_transport_is_current(&app).await?;
    crate::geodata::ensure_local_client_rule_sets(&app).await?;

    let _ = app.emit("tunnel-log", "[SYSTEM] Resolving core binary path...");
    let pid = launch_tunnel_process(&app, true).await?;

    let _ = app.emit(
        "tunnel-log",
        format!("[SYSTEM] Core process started with PID {} (root)", pid),
    );

    verify_tunnel_start(&app, &state, pid, tunnel_log_path()).await?;
    spawn_log_reader(app.clone(), pid, tunnel_log_path());
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

            if terminate_root_process(Some(&app), pid).is_ok() {
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

            let _ = std::fs::remove_file(tunnel_log_path());
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
        local_data.join("client_config_win.json"),
        local_data.join("server_profile.json"),
        local_data.join("active_tunnel_pid"),
        local_data.join("elevated_singbox_bootstrap.err"),
        local_data.join("elevated_singbox_bootstrap.ps1"),
        local_data.join("elevated_singbox.pid"),
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
    {
        let mut guard = state.singbox_pid.lock().unwrap();
        *guard = None;
    }

    let new_pid = match restart_tunnel_process(app, old_pid).await {
        Ok(pid) => pid,
        Err(error) => {
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
            kill_switch_engaged: Mutex::new(false),
            tray_toggle_item: Mutex::new(None),
            #[cfg(target_os = "windows")]
            windows_tray_notice_shown: Mutex::new(false),
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

            Ok(())
        })
        // --- Закрытие окна → скрытие (туннель продолжает работать) ---
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                #[cfg(target_os = "windows")]
                maybe_announce_windows_tray_behavior(&window.app_handle());
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            start_tunnel,
            stop_tunnel,
            reset_local_data,
            restore_tunnel_session,
            write_clipboard_text,
            read_clipboard_text,
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
            #[cfg(target_os = "macos")]
            if let RunEvent::Reopen { .. } = event {
                show_main_window(app, None);
            }

            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}
