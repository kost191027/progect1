mod deploy;
mod invite;
mod status;
mod warp;

// ── Crate-visible helpers used by lib.rs ────────────────────────────────────

pub(crate) use invite::clear_issued_invites;
pub(crate) use status::ensure_local_transport_is_current;
pub(crate) use warp::clear_local_warp_profile_sync;

// ── Shared types (used across submodules and by generator.rs) ───────────────

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ssh2::Session;
use std::io::ErrorKind;
use std::io::Read;
use std::net::SocketAddr;
use std::net::TcpStream;
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

// ── Constants ───────────────────────────────────────────────────────────────

pub(crate) const PRIMARY_EXTERNAL_PORT: u16 = 4433;
pub(crate) const EXTERNAL_PORT_CANDIDATES: [u16; 5] = [4433, 443, 5443, 7443, 9443];
pub(crate) const INTERNAL_SS_PORT_CANDIDATES: [u16; 5] = [14433, 15433, 16433, 17433, 18433];
pub(crate) const PINNED_SING_BOX_IMAGE: &str = "ghcr.io/sagernet/sing-box:v1.10.7";
pub(crate) const WGCF_VERSION: &str = "2.2.29";
pub(crate) const CONTAINER_PREFIXES: [&str; 5] = [
    "sys-networkd",
    "mdns-relay",
    "core-authd",
    "netdiag-agent",
    "kernel-events",
];
pub(crate) const LEGACY_CONTAINER_NAME: &str = "sys-network-helper";
pub(crate) const SSH_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const SSH_SESSION_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const REMOTE_DEPLOY_STALL_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const REMOTE_DEPLOY_POLL_INTERVAL: Duration = Duration::from_millis(250);
pub(crate) const MAX_FALLBACK_COVER_DOMAINS: usize = 4;

// ── Shared types ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) struct RemoteDeployTarget {
    pub(crate) external_port: u16,
    pub(crate) internal_ss_port: u16,
    pub(crate) container_name: String,
    pub(crate) reusing_existing_instance: bool,
    pub(crate) migrating_to_primary_port: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedServerProfile {
    pub host: String,
    pub user: String,
    pub password: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BackendAppRole {
    Master,
    Subordinate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedBackendAppRole {
    role: BackendAppRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RemoteTransportBootstrap {
    pub(crate) external_port: u16,
    #[serde(default = "default_internal_ss_port")]
    pub(crate) internal_ss_port: u16,
    #[serde(default = "default_remote_routing_mode")]
    pub(crate) routing_mode: String,
    pub(crate) cover_domain: String,
    #[serde(default)]
    pub(crate) fallback_cover_domains: Vec<String>,
    pub(crate) shadow_pass: String,
    pub(crate) ss_password: String,
    #[serde(default)]
    pub(crate) ss_server_password: String,
    #[serde(default)]
    pub(crate) issued_invites: Vec<RemoteInviteRecord>,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalClientTransportState {
    pub(crate) cover_domain: String,
    pub(crate) shadow_pass: String,
    pub(crate) ss_password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RemoteWarpConfig {
    pub(crate) private_key: String,
    pub(crate) address_v4: String,
    pub(crate) address_v6: String,
    pub(crate) endpoint: String,
    pub(crate) endpoint_port: u16,
    pub(crate) peer_public_key: String,
}

fn default_remote_routing_mode() -> String {
    "warp".to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalWarpProfileStatus {
    has_profile: bool,
    endpoint: Option<String>,
    endpoint_port: Option<u16>,
    address_v4: Option<String>,
    address_v6: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RemoteInviteRecord {
    pub(crate) id: String,
    pub(crate) shadow_pass: String,
    pub(crate) ss_user_password: String,
    pub(crate) generated_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransportStateSnapshot {
    current_cover_domain: Option<String>,
    available_cover_domains: Vec<String>,
    local_cover_domain: Option<String>,
    requires_redeploy: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalInstallationState {
    has_saved_server_profile: bool,
    has_client_config: bool,
}

fn default_internal_ss_port() -> u16 {
    crate::generator::INTERNAL_SS_PORT
}

// ── Remote mutation lock ────────────────────────────────────────────────────

fn remote_mutation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn acquire_remote_mutation_lock() -> Result<MutexGuard<'static, ()>, String> {
    remote_mutation_lock()
        .lock()
        .map_err(|_| "Remote operation lock is unavailable right now. Try again.".to_string())
}

// ── SSH connection ──────────────────────────────────────────────────────────

pub(crate) fn emit_ssh_stage(app: &AppHandle, stage: &str, message: impl Into<String>) {
    let _ = app.emit("tunnel-log", format!("[SSH:{}] {}", stage, message.into()));
}

fn resolve_ssh_socket_addrs(host: &str) -> Result<Vec<SocketAddr>, String> {
    let address = format!("{}:22", host);
    let resolved = address
        .to_socket_addrs()
        .map_err(|e| format!("Failed to resolve {}: {}", address, e))?
        .collect::<Vec<_>>();

    if resolved.is_empty() {
        return Err(format!("No SSH addresses resolved for {}", host));
    }

    Ok(resolved)
}

fn connect_tcp_with_timeout(addrs: &[SocketAddr]) -> Result<TcpStream, String> {
    let mut last_error = None;

    for addr in addrs {
        match TcpStream::connect_timeout(addr, SSH_CONNECT_TIMEOUT) {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                let _ = stream.set_read_timeout(Some(SSH_SESSION_TIMEOUT));
                let _ = stream.set_write_timeout(Some(SSH_SESSION_TIMEOUT));
                return Ok(stream);
            }
            Err(err) => {
                last_error = Some(format!("{} ({})", addr, err));
            }
        }
    }

    Err(format!(
        "Failed to connect to SSH on all resolved addresses within {:?}. Last error: {}",
        SSH_CONNECT_TIMEOUT,
        last_error.unwrap_or_else(|| "no addresses attempted".to_string())
    ))
}

pub(crate) fn connect_ssh_session(
    app: &AppHandle,
    host: &str,
    user: &str,
    pass: &str,
) -> Result<Session, String> {
    emit_ssh_stage(app, "RESOLVE", format!("Resolving {}...", host));
    let addrs = resolve_ssh_socket_addrs(host)?;
    emit_ssh_stage(
        app,
        "CONNECT",
        format!(
            "Trying {} resolved SSH address(es) with {:?} timeout...",
            addrs.len(),
            SSH_CONNECT_TIMEOUT
        ),
    );

    let tcp = connect_tcp_with_timeout(&addrs)?;
    emit_ssh_stage(app, "HANDSHAKE", "TCP connected. Starting SSH handshake...");

    let mut sess = Session::new().map_err(|e| e.to_string())?;
    sess.set_timeout(SSH_SESSION_TIMEOUT.as_millis() as u32);
    sess.set_tcp_stream(tcp);
    sess.handshake()
        .map_err(|e| format!("SSH handshake failed: {}", e))?;

    emit_ssh_stage(
        app,
        "AUTH",
        format!("Authenticating with password as {}...", user),
    );
    sess.userauth_password(user, pass)
        .map_err(|e| format!("Auth failed: {}", e))?;

    if !sess.authenticated() {
        return Err("Authentication failed".to_string());
    }

    Ok(sess)
}

// ── Remote command execution ────────────────────────────────────────────────

pub(crate) fn run_remote_command(sess: &Session, command: &str) -> Result<(String, i32), String> {
    let mut channel = sess.channel_session().map_err(|e| e.to_string())?;
    channel.exec(command).map_err(|e| e.to_string())?;

    let mut stdout = String::new();
    channel
        .read_to_string(&mut stdout)
        .map_err(|e| e.to_string())?;

    channel.wait_close().map_err(|e| e.to_string())?;
    let exit_status = channel.exit_status().map_err(|e| e.to_string())?;

    Ok((stdout, exit_status))
}

pub(crate) fn stream_remote_deploy_output(
    app: &AppHandle,
    sess: &Session,
    channel: &mut ssh2::Channel,
) -> Result<(), String> {
    sess.set_blocking(false);

    let mut buffer = [0; 1024];
    let mut last_progress = Instant::now();

    loop {
        match channel.read(&mut buffer) {
            Ok(0) => {
                if channel.eof() {
                    break;
                }

                if last_progress.elapsed() >= REMOTE_DEPLOY_STALL_TIMEOUT {
                    let _ = channel.close();
                    sess.set_blocking(true);
                    return Err(format!(
                        "Remote deploy stalled: no output for more than {:?}. The server may still be finishing container startup. Please try Update once more.",
                        REMOTE_DEPLOY_STALL_TIMEOUT
                    ));
                }

                thread::sleep(REMOTE_DEPLOY_POLL_INTERVAL);
            }
            Ok(n) => {
                last_progress = Instant::now();
                let output = String::from_utf8_lossy(&buffer[..n]);
                for line in output.lines() {
                    if !line.trim().is_empty() {
                        let _ = app.emit("tunnel-log", format!("[SERVER] {}", line));
                    }
                }
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                if last_progress.elapsed() >= REMOTE_DEPLOY_STALL_TIMEOUT {
                    let _ = channel.close();
                    sess.set_blocking(true);
                    return Err(format!(
                        "Remote deploy stalled: no output for more than {:?}. The server may still be finishing container startup. Please try Update once more.",
                        REMOTE_DEPLOY_STALL_TIMEOUT
                    ));
                }

                thread::sleep(REMOTE_DEPLOY_POLL_INTERVAL);
            }
            Err(err) => {
                sess.set_blocking(true);
                return Err(format!("Remote deploy stream error: {}", err));
            }
        }
    }

    sess.set_blocking(true);
    Ok(())
}

// ── Server profile persistence ──────────────────────────────────────────────

pub(crate) fn server_profile_path(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    Ok(local_data.join("server_profile.json"))
}

fn server_profile_archive_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    Ok(local_data.join("server_profiles"))
}

fn sanitize_profile_key_segment(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|char| match char {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => char,
            _ => '_',
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

fn archived_server_profile_path(
    app: &AppHandle,
    profile: &SavedServerProfile,
) -> Result<PathBuf, String> {
    let archive_dir = server_profile_archive_dir(app)?;
    let host = sanitize_profile_key_segment(&profile.host);
    let user = sanitize_profile_key_segment(&profile.user);
    Ok(archive_dir.join(format!("{host}__{user}.json")))
}

fn backend_app_role_path(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    Ok(local_data.join("app_role.json"))
}

pub(crate) fn save_server_profile(
    app: &AppHandle,
    profile: &SavedServerProfile,
) -> Result<(), String> {
    let profile_path = server_profile_path(app)?;
    let archived_profile_path = archived_server_profile_path(app, profile)?;

    if let Some(parent) = profile_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if let Some(parent) = archived_profile_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let profile_json = serde_json::to_vec_pretty(profile).map_err(|e| e.to_string())?;
    std::fs::write(&profile_path, &profile_json).map_err(|e| e.to_string())?;
    std::fs::write(archived_profile_path, profile_json).map_err(|e| e.to_string())
}

pub(crate) fn remove_saved_server_profile(app: &AppHandle) -> Result<(), String> {
    let profile_path = server_profile_path(app)?;

    match std::fs::remove_file(profile_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn load_backend_app_role(app: &AppHandle) -> Result<BackendAppRole, String> {
    let role_path = backend_app_role_path(app)?;

    if !role_path.exists() {
        return Ok(BackendAppRole::Master);
    }

    let role_json = std::fs::read_to_string(role_path).map_err(|e| e.to_string())?;
    let persisted = serde_json::from_str::<PersistedBackendAppRole>(&role_json)
        .map_err(|e| format!("Failed to parse backend app role: {}", e))?;

    Ok(persisted.role)
}

pub(crate) fn save_backend_app_role(app: &AppHandle, role: BackendAppRole) -> Result<(), String> {
    let role_path = backend_app_role_path(app)?;

    if let Some(parent) = role_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let role_json =
        serde_json::to_vec_pretty(&PersistedBackendAppRole { role }).map_err(|e| e.to_string())?;
    std::fs::write(role_path, role_json).map_err(|e| e.to_string())
}

pub(crate) fn clear_backend_app_role(app: &AppHandle) -> Result<(), String> {
    let role_path = backend_app_role_path(app)?;

    match std::fs::remove_file(role_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn ensure_master_role(app: &AppHandle, action: &str) -> Result<(), String> {
    if load_backend_app_role(app)? == BackendAppRole::Subordinate {
        return Err(format!(
            "This app is currently linked as a subordinate installation. Reset local data or switch back to a master server profile before trying to {} here.",
            action
        ));
    }

    Ok(())
}

// ── Client config persistence ───────────────────────────────────────────────

pub(crate) fn local_client_config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    Ok(local_data.join("client_config.json"))
}

pub(crate) fn load_local_client_transport_state(
    app: &AppHandle,
) -> Result<Option<LocalClientTransportState>, String> {
    let client_config_path = local_client_config_path(app)?;

    if !client_config_path.exists() {
        return Ok(None);
    }

    let config_json = std::fs::read_to_string(&client_config_path).map_err(|e| e.to_string())?;
    let parsed = serde_json::from_str::<Value>(&config_json)
        .map_err(|e| format!("Failed to parse local client config JSON: {}", e))?;
    let outbounds = parsed
        .get("outbounds")
        .and_then(Value::as_array)
        .ok_or_else(|| "Local client config is missing outbounds array".to_string())?;

    let shadowtls_outbound = outbounds
        .iter()
        .find(|outbound| outbound.get("type").and_then(Value::as_str) == Some("shadowtls"))
        .ok_or_else(|| "Local client config is missing shadowtls outbound".to_string())?;
    let shadowsocks_outbound = outbounds
        .iter()
        .find(|outbound| {
            outbound.get("type").and_then(Value::as_str) == Some("shadowsocks")
                && outbound.get("tag").and_then(Value::as_str) == Some("proxy")
        })
        .ok_or_else(|| "Local client config is missing proxy shadowsocks outbound".to_string())?;

    let cover_domain = shadowtls_outbound
        .get("tls")
        .and_then(|tls| tls.get("server_name"))
        .and_then(Value::as_str)
        .ok_or_else(|| "Local client config is missing ShadowTLS server_name".to_string())?
        .to_string();
    let shadow_pass = shadowtls_outbound
        .get("password")
        .and_then(Value::as_str)
        .ok_or_else(|| "Local client config is missing ShadowTLS password".to_string())?
        .to_string();
    let ss_password = shadowsocks_outbound
        .get("password")
        .and_then(Value::as_str)
        .ok_or_else(|| "Local client config is missing Shadowsocks password".to_string())?
        .to_string();

    Ok(Some(LocalClientTransportState {
        cover_domain,
        shadow_pass,
        ss_password,
    }))
}

// ── Remote bootstrap & container helpers ────────────────────────────────────

pub(crate) fn load_remote_transport_bootstrap(
    sess: &Session,
) -> Result<Option<RemoteTransportBootstrap>, String> {
    let command = r#"bash -lc '
CONFIG_DIR="/opt/rkn"
BOOTSTRAP_FILE="$CONFIG_DIR/bootstrap.json"
ACTIVE_CONFIG="$CONFIG_DIR/config.json"

if [ -f "$BOOTSTRAP_FILE" ]; then
  echo "__BOOTSTRAP__"
  cat "$BOOTSTRAP_FILE"
  exit 0
fi

if [ -f "$ACTIVE_CONFIG" ]; then
  echo "__CONFIG__"
  cat "$ACTIVE_CONFIG"
  exit 0
fi

exit 0
'"#;

    let (stdout, exit_status) = run_remote_command(sess, command)?;
    if exit_status != 0 {
        return Err(format!(
            "Failed to read remote transport bootstrap. Output: {}",
            stdout.trim()
        ));
    }

    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if let Some(json_payload) = trimmed.strip_prefix("__BOOTSTRAP__") {
        let bootstrap = serde_json::from_str::<RemoteTransportBootstrap>(json_payload.trim())
            .map_err(|e| format!("Failed to parse remote bootstrap JSON: {}", e))?;
        return Ok(Some(bootstrap));
    }

    if let Some(config_payload) = trimmed.strip_prefix("__CONFIG__") {
        let bootstrap = parse_remote_bootstrap_from_server_config(config_payload.trim())?;
        return Ok(Some(bootstrap));
    }

    Ok(None)
}

fn parse_remote_bootstrap_from_server_config(
    config_json: &str,
) -> Result<RemoteTransportBootstrap, String> {
    let parsed = serde_json::from_str::<Value>(config_json)
        .map_err(|e| format!("Failed to parse remote server config JSON: {}", e))?;
    let inbounds = parsed
        .get("inbounds")
        .and_then(Value::as_array)
        .ok_or_else(|| "Remote server config is missing inbounds array".to_string())?;

    let shadowtls_inbound = inbounds
        .iter()
        .find(|inbound| inbound.get("type").and_then(Value::as_str) == Some("shadowtls"))
        .ok_or_else(|| "Remote server config is missing shadowtls inbound".to_string())?;
    let shadowsocks_inbound = inbounds
        .iter()
        .find(|inbound| inbound.get("type").and_then(Value::as_str) == Some("shadowsocks"))
        .ok_or_else(|| "Remote server config is missing shadowsocks inbound".to_string())?;

    let external_port = shadowtls_inbound
        .get("listen_port")
        .and_then(Value::as_u64)
        .ok_or_else(|| "Remote server config is missing shadowtls listen_port".to_string())?
        as u16;
    let cover_domain = shadowtls_inbound
        .get("handshake")
        .and_then(|handshake| handshake.get("server"))
        .and_then(Value::as_str)
        .ok_or_else(|| "Remote server config is missing ShadowTLS cover domain".to_string())?
        .to_string();
    let fallback_cover_domains = shadowtls_inbound
        .get("handshake_for_server_name")
        .and_then(Value::as_object)
        .map(|mappings| mappings.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let shadow_pass = shadowtls_inbound
        .get("users")
        .and_then(Value::as_array)
        .and_then(|users| users.first())
        .and_then(|user| user.get("password"))
        .and_then(Value::as_str)
        .ok_or_else(|| "Remote server config is missing ShadowTLS password".to_string())?
        .to_string();
    let ss_password = shadowsocks_inbound
        .get("password")
        .and_then(Value::as_str)
        .ok_or_else(|| "Remote server config is missing Shadowsocks password".to_string())?
        .to_string();
    let internal_ss_port = shadowsocks_inbound
        .get("listen_port")
        .and_then(Value::as_u64)
        .map(|port| port as u16)
        .unwrap_or_else(default_internal_ss_port);
    let routing_mode = if remote_runtime_uses_warp_from_config(&parsed) {
        "warp"
    } else {
        "direct"
    };

    Ok(RemoteTransportBootstrap {
        external_port,
        internal_ss_port,
        routing_mode: routing_mode.to_string(),
        cover_domain,
        fallback_cover_domains,
        shadow_pass,
        ss_password,
        ss_server_password: String::new(),
        issued_invites: Vec::new(),
    })
}

pub(crate) fn load_remote_container_name(sess: &Session) -> Result<Option<String>, String> {
    let command = format!(
        r#"bash -lc '
CONFIG_DIR="/opt/rkn"
ACTIVE_CONTAINER_FILE="$CONFIG_DIR/container_name"
CONTAINER_NAME=""

if [ -f "$ACTIVE_CONTAINER_FILE" ]; then
  CONTAINER_NAME="$(cat "$ACTIVE_CONTAINER_FILE" 2>/dev/null || true)"
fi

if [ -z "$CONTAINER_NAME" ] && docker inspect "{legacy_container_name}" >/dev/null 2>&1; then
  CONTAINER_NAME="{legacy_container_name}"
fi

if [ -n "$CONTAINER_NAME" ] && docker inspect "$CONTAINER_NAME" >/dev/null 2>&1; then
  echo "$CONTAINER_NAME"
fi
'"#,
        legacy_container_name = LEGACY_CONTAINER_NAME
    );

    let (stdout, exit_status) = run_remote_command(sess, &command)?;
    if exit_status != 0 {
        return Err(format!(
            "Failed to read active remote container name. Output: {}",
            stdout.trim()
        ));
    }

    let container_name = stdout.trim();
    if container_name.is_empty() {
        Ok(None)
    } else {
        Ok(Some(container_name.to_string()))
    }
}

pub(crate) fn load_remote_container_image(
    sess: &Session,
    container_name: &str,
) -> Result<Option<String>, String> {
    let command = format!(
        r#"bash -lc '
if docker inspect "{container_name}" >/dev/null 2>&1; then
  docker inspect -f "{{{{.Config.Image}}}}" "{container_name}" 2>/dev/null || true
fi
'"#,
        container_name = container_name
    );

    let (stdout, exit_status) = run_remote_command(sess, &command)?;
    if exit_status != 0 {
        return Err(format!(
            "Failed to read remote container image for {}. Output: {}",
            container_name,
            stdout.trim()
        ));
    }

    let image = stdout.trim();
    if image.is_empty() {
        Ok(None)
    } else {
        Ok(Some(image.to_string()))
    }
}

pub(crate) fn remote_runtime_uses_warp(sess: &Session) -> Result<bool, String> {
    let command = r#"bash -lc '
CONFIG_DIR="/opt/rkn"
ACTIVE_CONFIG="$CONFIG_DIR/config.json"

if [ ! -f "$ACTIVE_CONFIG" ]; then
  echo "enabled=false"
  exit 0
fi

if grep -q '"tag"[[:space:]]*:[[:space:]]*"warp"' "$ACTIVE_CONFIG" \
  && { grep -q '"final"[[:space:]]*:[[:space:]]*"warp"' "$ACTIVE_CONFIG" \
    || grep -q '"outbound"[[:space:]]*:[[:space:]]*"warp"' "$ACTIVE_CONFIG"; }; then
  echo "enabled=true"
else
  echo "enabled=false"
fi
'"#;

    let (stdout, exit_status) = run_remote_command(sess, command)?;
    if exit_status != 0 {
        return Err(format!(
            "Failed to detect whether remote runtime already uses WARP. Output: {}",
            stdout.trim()
        ));
    }

    Ok(stdout.lines().any(|line| line.trim() == "enabled=true"))
}

pub(crate) fn remote_runtime_uses_warp_from_config(parsed: &Value) -> bool {
    parsed
        .get("outbounds")
        .and_then(Value::as_array)
        .map(|outbounds| {
            outbounds.iter().any(|outbound| {
                outbound.get("tag").and_then(Value::as_str) == Some("warp")
                    && outbound.get("type").and_then(Value::as_str) == Some("wireguard")
            })
        })
        .unwrap_or(false)
        && parsed
            .get("route")
            .and_then(Value::as_object)
            .map(|route| {
                route.get("final").and_then(Value::as_str) == Some("warp")
                    || route
                        .get("rules")
                        .and_then(Value::as_array)
                        .map(|rules| {
                            rules.iter().any(|rule| {
                                rule.get("outbound").and_then(Value::as_str) == Some("warp")
                            })
                        })
                        .unwrap_or(false)
            })
            .unwrap_or(false)
}

pub(crate) fn build_container_name(short_id: &str) -> String {
    let seed = short_id
        .get(0..2)
        .and_then(|prefix| u8::from_str_radix(prefix, 16).ok())
        .unwrap_or(0);
    let prefix = CONTAINER_PREFIXES[(seed as usize) % CONTAINER_PREFIXES.len()];
    let suffix = short_id.get(0..6).unwrap_or(short_id);

    format!("{}-{}", prefix, suffix)
}

pub(crate) fn snapshot_for_cover_domain(cover_domain: impl Into<String>) -> TransportStateSnapshot {
    let cover_domain = cover_domain.into();

    TransportStateSnapshot {
        current_cover_domain: Some(cover_domain.clone()),
        available_cover_domains: crate::generator::available_cover_domains(),
        local_cover_domain: Some(cover_domain),
        requires_redeploy: false,
    }
}

// ── Cached transport bootstrap (for instant invite generation) ─────────────

fn cached_transport_bootstrap_path(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    Ok(local_data.join("cached_bootstrap.json"))
}

pub(crate) fn save_cached_transport_bootstrap(
    app: &AppHandle,
    bootstrap: &RemoteTransportBootstrap,
) -> Result<(), String> {
    let path = cached_transport_bootstrap_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec_pretty(bootstrap).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

pub(crate) fn load_cached_transport_bootstrap(
    app: &AppHandle,
) -> Result<Option<RemoteTransportBootstrap>, String> {
    let path = cached_transport_bootstrap_path(app)?;
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let bootstrap = serde_json::from_str::<RemoteTransportBootstrap>(&contents)
        .map_err(|e| format!("Failed to parse cached bootstrap: {}", e))?;
    Ok(Some(bootstrap))
}

pub(crate) fn clear_cached_transport_bootstrap(app: &AppHandle) -> Result<(), String> {
    let path = cached_transport_bootstrap_path(app)?;
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn ensure_local_client_rule_sets_sync(
    app: &AppHandle,
) -> Result<Vec<crate::geodata::LocalRuleSetAsset>, String> {
    tauri::async_runtime::block_on(crate::geodata::ensure_local_client_rule_sets(app))
}

pub(crate) fn monitored_port_pattern() -> String {
    EXTERNAL_PORT_CANDIDATES
        .iter()
        .copied()
        .chain(INTERNAL_SS_PORT_CANDIDATES.iter().copied())
        .map(|port| port.to_string())
        .collect::<Vec<_>>()
        .join("|")
}

pub(crate) fn build_rotated_cover_domain_history(
    current_cover_domain: &str,
    existing_fallback_cover_domains: &[String],
    new_cover_domain: &str,
) -> Vec<String> {
    let mut fallback_cover_domains = Vec::new();

    let push_unique = |domains: &mut Vec<String>, domain: &str| {
        if domain != new_cover_domain && !domains.iter().any(|item| item == domain) {
            domains.push(domain.to_string());
        }
    };

    push_unique(&mut fallback_cover_domains, current_cover_domain);
    for domain in existing_fallback_cover_domains {
        push_unique(&mut fallback_cover_domains, domain);
    }

    fallback_cover_domains.truncate(MAX_FALLBACK_COVER_DOMAINS);
    fallback_cover_domains
}

// ── Warp config helpers (used in warp.rs, also by deploy.rs) ────────────────

pub(crate) fn validate_warp_config(config: &RemoteWarpConfig) -> Result<(), String> {
    if config.private_key.trim().is_empty()
        || config.address_v4.trim().is_empty()
        || config.endpoint.trim().is_empty()
        || config.peer_public_key.trim().is_empty()
        || config.endpoint_port == 0
    {
        return Err("WARP profile is incomplete.".to_string());
    }

    Ok(())
}

pub(crate) fn warp_status_from_config(config: Option<&RemoteWarpConfig>) -> LocalWarpProfileStatus {
    LocalWarpProfileStatus {
        has_profile: config.is_some(),
        endpoint: config.map(|item| item.endpoint.clone()),
        endpoint_port: config.map(|item| item.endpoint_port),
        address_v4: config.map(|item| item.address_v4.clone()),
        address_v6: config
            .filter(|item| !item.address_v6.trim().is_empty())
            .map(|item| item.address_v6.clone()),
    }
}

// ── Public Tauri command facade ─────────────────────────────────────────────

#[tauri::command]
pub async fn deploy_server(
    app: AppHandle,
    host: String,
    user: String,
    pass: String,
) -> Result<TransportStateSnapshot, String> {
    deploy::deploy_server(app, host, user, pass).await
}

#[tauri::command]
pub fn get_local_installation_state(app: AppHandle) -> Result<LocalInstallationState, String> {
    invite::get_local_installation_state(app)
}

#[tauri::command]
pub fn list_issued_invite_links(app: AppHandle) -> Result<Vec<invite::IssuedInviteLink>, String> {
    invite::list_issued_invite_links(app)
}

#[tauri::command]
pub async fn delete_issued_invite_link(app: AppHandle, invite_id: String) -> Result<(), String> {
    invite::delete_issued_invite_link(app, invite_id).await
}

#[tauri::command]
pub async fn generate_invite_link(
    app: AppHandle,
) -> Result<invite::GeneratedInviteLinkResult, String> {
    invite::generate_invite_link(app).await
}

#[tauri::command]
pub async fn import_invite_link(
    app: AppHandle,
    invite_link: String,
) -> Result<invite::InviteImportResult, String> {
    invite::import_invite_link(app, invite_link).await
}

#[tauri::command]
pub fn load_saved_server_profile(app: AppHandle) -> Result<Option<SavedServerProfile>, String> {
    warp::load_saved_server_profile(app)
}

#[tauri::command]
pub fn get_local_warp_profile_status(app: AppHandle) -> Result<LocalWarpProfileStatus, String> {
    warp::get_local_warp_profile_status(app)
}

#[tauri::command]
pub fn import_local_warp_profile(
    app: AppHandle,
    profile_text: String,
) -> Result<LocalWarpProfileStatus, String> {
    warp::import_local_warp_profile(app, profile_text)
}

#[tauri::command]
pub fn bootstrap_local_warp_profile(app: AppHandle) -> Result<LocalWarpProfileStatus, String> {
    warp::bootstrap_local_warp_profile(app)
}

#[tauri::command]
pub fn bootstrap_local_warp_profile_from_credentials(
    app: AppHandle,
    host: String,
    user: String,
    password: String,
) -> Result<LocalWarpProfileStatus, String> {
    warp::bootstrap_local_warp_profile_from_credentials(app, host, user, password)
}

#[tauri::command]
pub fn clear_local_warp_profile(app: AppHandle) -> Result<(), String> {
    warp::clear_local_warp_profile(app)
}

#[tauri::command]
pub async fn get_transport_state_snapshot(
    app: AppHandle,
) -> Result<TransportStateSnapshot, String> {
    status::get_transport_state_snapshot(app).await
}

#[tauri::command]
pub async fn check_server_status(app: AppHandle) -> Result<String, String> {
    status::check_server_status(app).await
}

#[tauri::command]
pub async fn rotate_sni(app: AppHandle, target_domain: Option<String>) -> Result<String, String> {
    status::rotate_sni(app, target_domain).await
}
