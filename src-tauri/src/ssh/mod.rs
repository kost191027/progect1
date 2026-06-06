mod deploy;
mod invite;
mod status;
mod warp;

// ── Crate-visible helpers used by lib.rs ────────────────────────────────────

pub(crate) use invite::{clear_imported_invites, clear_issued_invites};
#[cfg(target_os = "android")]
pub(crate) use status::ensure_local_transport_is_current;
#[cfg(not(target_os = "android"))]
pub(crate) use status::ensure_local_transport_is_current_quiet;
pub(crate) use warp::clear_local_warp_profile_sync;

// ── Shared types (used across submodules and by generator.rs) ───────────────

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ssh2::Session;
use std::io::{ErrorKind, Read, Write};
use std::net::ToSocketAddrs;
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

// ── Constants ───────────────────────────────────────────────────────────────

pub(crate) const PRIMARY_EXTERNAL_PORT: u16 = 4433;
pub(crate) const EXTERNAL_PORT_CANDIDATES: [u16; 5] = [4433, 443, 5443, 7443, 9443];
pub(crate) const VLESS_EXTERNAL_PORT_CANDIDATES: [u16; 5] = [8443, 8444, 2087, 2096, 2053];
pub(crate) const INTERNAL_SS_PORT_CANDIDATES: [u16; 5] = [14433, 15433, 16433, 17433, 18433];
pub(crate) const PINNED_WARP_SING_BOX_IMAGE: &str = "ghcr.io/sagernet/sing-box:v1.10.7";
pub(crate) const PINNED_DIRECT_SING_BOX_IMAGE: &str = "ghcr.io/sagernet/sing-box:v1.13.5";
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
pub(crate) const SSH_SESSION_TIMEOUT: Duration = Duration::from_secs(20);
pub(crate) const SSH_SESSION_ATTEMPTS: usize = 3;
pub(crate) const SSH_RETRY_BACKOFF: Duration = Duration::from_millis(750);
const SSH_PORT_CANDIDATES: [u16; 2] = [22, 2222];
pub(crate) const REMOTE_DEPLOY_STALL_TIMEOUT: Duration = Duration::from_secs(180);
pub(crate) const REMOTE_DEPLOY_POLL_INTERVAL: Duration = Duration::from_millis(250);
pub(crate) const MAX_FALLBACK_COVER_DOMAINS: usize = 4;

pub(crate) fn pinned_sing_box_image_for_routing_mode(routing_mode: &str) -> &'static str {
    if routing_mode == "warp" {
        PINNED_WARP_SING_BOX_IMAGE
    } else {
        PINNED_DIRECT_SING_BOX_IMAGE
    }
}

// ── Shared types ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) struct RemoteDeployTarget {
    pub(crate) external_port: u16,
    pub(crate) vless_external_port: u16,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredServerProfile {
    id: String,
    host: String,
    user: String,
    password: String,
    saved_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SavedServerProfileEntry {
    id: String,
    host: String,
    user: String,
    saved_at: u64,
    is_active: bool,
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
    #[serde(default)]
    pub(crate) vless_external_port: u16,
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
    pub(crate) vless_uuid: String,
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
    pub(crate) vless_uuid: String,
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
    #[serde(default)]
    pub(crate) vless_uuid: String,
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

fn parse_ssh_target(host: &str) -> (String, Vec<u16>) {
    if let Some((hostname, port)) = host.rsplit_once(':') {
        if !hostname.is_empty() && !hostname.contains(':') {
            if let Ok(port) = port.parse::<u16>() {
                return (hostname.to_string(), vec![port]);
            }
        }
    }

    (host.to_string(), SSH_PORT_CANDIDATES.to_vec())
}

fn resolve_ssh_socket_addrs(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let address = format!("{}:{}", host, port);
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

fn read_remote_ssh_banner(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    let mut collected = Vec::new();
    let mut line = Vec::new();

    loop {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).map_err(|e| {
            format!(
                "Failed to read remote SSH banner before client hello: {}",
                e
            )
        })?;
        collected.push(byte[0]);
        line.push(byte[0]);

        if collected.len() > 8192 {
            return Err("Remote SSH banner exceeded the safety limit".to_string());
        }

        if byte[0] == b'\n' {
            if line.starts_with(b"SSH-") {
                return Ok(collected);
            }
            line.clear();
        }
    }
}

fn proxy_bidirectional(mut left: TcpStream, mut right: TcpStream) {
    let Ok(mut left_read) = left.try_clone() else {
        return;
    };
    let Ok(mut right_write) = right.try_clone() else {
        return;
    };

    let join = thread::spawn(move || {
        let _ = std::io::copy(&mut left_read, &mut right_write);
        let _ = right_write.shutdown(Shutdown::Write);
    });

    let _ = std::io::copy(&mut right, &mut left);
    let _ = left.shutdown(Shutdown::Write);
    let _ = join.join();
}

fn connect_tcp_with_banner_first_proxy(addrs: &[SocketAddr]) -> Result<TcpStream, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| format!("Failed to bind local SSH banner-first proxy: {}", e))?;
    listener
        .set_nonblocking(false)
        .map_err(|e| format!("Failed to configure local SSH banner-first proxy: {}", e))?;
    let local_addr = listener
        .local_addr()
        .map_err(|e| format!("Failed to read local SSH banner-first proxy address: {}", e))?;
    let remote_addrs = addrs.to_vec();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

    thread::spawn(move || {
        let (mut local_stream, _) = match listener.accept() {
            Ok(accepted) => accepted,
            Err(error) => {
                let _ = ready_tx.send(Err(format!(
                    "Failed to accept local SSH banner-first proxy client: {}",
                    error
                )));
                return;
            }
        };
        let _ = local_stream.set_nodelay(true);
        let _ = local_stream.set_read_timeout(Some(SSH_SESSION_TIMEOUT));
        let _ = local_stream.set_write_timeout(Some(SSH_SESSION_TIMEOUT));

        let mut remote_stream = match connect_tcp_with_timeout(&remote_addrs) {
            Ok(stream) => stream,
            Err(error) => {
                let _ = ready_tx.send(Err(error));
                return;
            }
        };

        let banner = match read_remote_ssh_banner(&mut remote_stream) {
            Ok(banner) => banner,
            Err(error) => {
                let _ = ready_tx.send(Err(error));
                return;
            }
        };

        if let Err(error) = local_stream.write_all(&banner) {
            let _ = ready_tx.send(Err(format!(
                "Failed to forward remote SSH banner to libssh2: {}",
                error
            )));
            return;
        }
        let _ = ready_tx.send(Ok(()));
        proxy_bidirectional(local_stream, remote_stream);
    });

    let stream = TcpStream::connect_timeout(&local_addr, SSH_CONNECT_TIMEOUT).map_err(|e| {
        format!(
            "Failed to connect to local SSH banner-first proxy at {}: {}",
            local_addr, e
        )
    })?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(SSH_SESSION_TIMEOUT));
    let _ = stream.set_write_timeout(Some(SSH_SESSION_TIMEOUT));

    match ready_rx.recv_timeout(SSH_SESSION_TIMEOUT) {
        Ok(Ok(())) => Ok(stream),
        Ok(Err(error)) => Err(error),
        Err(error) => Err(format!(
            "Timed out waiting for SSH banner-first proxy readiness: {}",
            error
        )),
    }
}

pub(crate) fn connect_ssh_session(
    app: &AppHandle,
    host: &str,
    user: &str,
    pass: &str,
) -> Result<Session, String> {
    connect_ssh_session_inner(app, host, user, pass, true)
}

pub(crate) fn connect_ssh_session_quiet(
    app: &AppHandle,
    host: &str,
    user: &str,
    pass: &str,
) -> Result<Session, String> {
    connect_ssh_session_inner(app, host, user, pass, false)
}

fn maybe_emit_ssh_stage(
    app: &AppHandle,
    emit_stages: bool,
    stage: &str,
    message: impl Into<String>,
) {
    if emit_stages {
        emit_ssh_stage(app, stage, message);
    }
}

struct SshAttemptContext<'a> {
    app: &'a AppHandle,
    user: &'a str,
    pass: &'a str,
    port: u16,
    attempt: usize,
    mode: &'a str,
    emit_stages: bool,
}

#[derive(Debug)]
enum SshAttemptError {
    Auth(String),
    Other(String),
}

impl SshAttemptError {
    fn into_message(self) -> String {
        match self {
            Self::Auth(message) | Self::Other(message) => message,
        }
    }
}

fn complete_ssh_session_from_stream(
    tcp: TcpStream,
    ctx: SshAttemptContext<'_>,
) -> Result<Session, SshAttemptError> {
    maybe_emit_ssh_stage(
        ctx.app,
        ctx.emit_stages,
        "HANDSHAKE",
        format!(
            "TCP connected on port {} via {}. Starting SSH handshake (attempt {}/{}, {:?} timeout)...",
            ctx.port, ctx.mode, ctx.attempt, SSH_SESSION_ATTEMPTS, SSH_SESSION_TIMEOUT
        ),
    );

    let mut sess = Session::new().map_err(|e| SshAttemptError::Other(e.to_string()))?;
    sess.set_timeout(SSH_SESSION_TIMEOUT.as_millis() as u32);
    sess.set_tcp_stream(tcp);
    sess.handshake().map_err(|error| {
        SshAttemptError::Other(format!(
            "SSH handshake failed on port {} via {}: {}",
            ctx.port, ctx.mode, error
        ))
    })?;

    maybe_emit_ssh_stage(
        ctx.app,
        ctx.emit_stages,
        "AUTH",
        format!(
            "Authenticating with password as {} on port {} via {} (attempt {}/{})...",
            ctx.user, ctx.port, ctx.mode, ctx.attempt, SSH_SESSION_ATTEMPTS
        ),
    );

    sess.userauth_password(ctx.user, ctx.pass)
        .map_err(|error| {
            SshAttemptError::Auth(format!(
                "SSH authentication failed for user '{}' on port {} via {}. Check the SSH username/password saved in Server Access. Details: {}",
                ctx.user, ctx.port, ctx.mode, error
            ))
        })?;

    if !sess.authenticated() {
        return Err(SshAttemptError::Auth(format!(
            "SSH authentication failed for user '{}' on port {} via {}. Check the SSH username/password saved in Server Access.",
            ctx.user, ctx.port, ctx.mode
        )));
    }

    Ok(sess)
}

fn connect_ssh_session_inner(
    app: &AppHandle,
    host: &str,
    user: &str,
    pass: &str,
    emit_stages: bool,
) -> Result<Session, String> {
    let (ssh_host, ssh_ports) = parse_ssh_target(host);
    maybe_emit_ssh_stage(
        app,
        emit_stages,
        "RESOLVE",
        format!("Resolving {}...", ssh_host),
    );
    let mut resolved_targets = Vec::new();
    for port in ssh_ports {
        resolved_targets.push((port, resolve_ssh_socket_addrs(&ssh_host, port)?));
    }

    let resolved_count = resolved_targets
        .iter()
        .map(|(_, addrs)| addrs.len())
        .sum::<usize>();
    let port_list = resolved_targets
        .iter()
        .map(|(port, _)| port.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    maybe_emit_ssh_stage(
        app,
        emit_stages,
        "CONNECT",
        format!(
            "Trying {} resolved SSH address(es) on port(s) {} with {:?} timeout...",
            resolved_count, port_list, SSH_CONNECT_TIMEOUT
        ),
    );

    let mut last_error = None;

    for attempt in 1..=SSH_SESSION_ATTEMPTS {
        for (port, addrs) in &resolved_targets {
            let tcp = match connect_tcp_with_timeout(addrs) {
                Ok(stream) => stream,
                Err(error) => {
                    last_error = Some(format!(
                        "SSH TCP connect failed on port {}: {}",
                        port, error
                    ));
                    continue;
                }
            };

            match complete_ssh_session_from_stream(
                tcp,
                SshAttemptContext {
                    app,
                    user,
                    pass,
                    port: *port,
                    attempt,
                    mode: "direct",
                    emit_stages,
                },
            ) {
                Ok(sess) => return Ok(sess),
                Err(SshAttemptError::Auth(error)) => {
                    maybe_emit_ssh_stage(app, emit_stages, "AUTH", error.clone());
                    return Err(error);
                }
                Err(SshAttemptError::Other(error)) => {
                    maybe_emit_ssh_stage(
                        app,
                        emit_stages,
                        "RETRY",
                        format!(
                            "{}. Trying SSH banner-first fallback on the same endpoint...",
                            error
                        ),
                    );
                }
            }

            let tcp = match connect_tcp_with_banner_first_proxy(addrs) {
                Ok(stream) => stream,
                Err(error) => {
                    last_error = Some(format!(
                        "SSH banner-first fallback failed on port {}: {}",
                        port, error
                    ));
                    maybe_emit_ssh_stage(
                        app,
                        emit_stages,
                        "RETRY",
                        format!(
                            "SSH banner-first fallback failed on port {}. Trying the next SSH endpoint...",
                            port
                        ),
                    );
                    continue;
                }
            };

            match complete_ssh_session_from_stream(
                tcp,
                SshAttemptContext {
                    app,
                    user,
                    pass,
                    port: *port,
                    attempt,
                    mode: "banner-first fallback",
                    emit_stages,
                },
            ) {
                Ok(sess) => return Ok(sess),
                Err(SshAttemptError::Auth(error)) => {
                    maybe_emit_ssh_stage(app, emit_stages, "AUTH", error.clone());
                    return Err(error);
                }
                Err(error) => {
                    let error = error.into_message();
                    last_error = Some(error.clone());
                    maybe_emit_ssh_stage(
                        app,
                        emit_stages,
                        "RETRY",
                        format!("{}. Trying the next SSH endpoint...", error),
                    );
                }
            }
        }

        if attempt < SSH_SESSION_ATTEMPTS {
            maybe_emit_ssh_stage(
                app,
                emit_stages,
                "RETRY",
                format!(
                    "SSH attempt {}/{} failed on all configured endpoints. Retrying shortly...",
                    attempt, SSH_SESSION_ATTEMPTS
                ),
            );
            thread::sleep(SSH_RETRY_BACKOFF);
        }
    }

    Err(last_error.unwrap_or_else(|| "SSH session failed to start.".to_string()))
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
                    if should_emit_remote_deploy_line(line) {
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

fn should_emit_remote_deploy_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }

    if trimmed.contains("Digest:")
        || trimmed.contains("Pulling from")
        || trimmed.contains("Status: Image is up to date")
        || trimmed == PINNED_WARP_SING_BOX_IMAGE
        || trimmed == PINNED_DIRECT_SING_BOX_IMAGE
    {
        return false;
    }

    trimmed.starts_with("[INFO]")
        || trimmed.starts_with("[WARN]")
        || trimmed.starts_with("[ERROR]")
        || trimmed.starts_with("[SUCCESS]")
        || trimmed.contains("Container is UP")
}

// ── Server profile persistence ──────────────────────────────────────────────

pub(crate) fn server_profile_path(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    Ok(local_data.join("server_profile.json"))
}

fn saved_server_profiles_path(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    Ok(local_data.join("saved_server_profiles.json"))
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

fn saved_server_profile_id(profile: &SavedServerProfile) -> String {
    format!(
        "{}__{}",
        sanitize_profile_key_segment(&profile.host),
        sanitize_profile_key_segment(&profile.user)
    )
}

fn load_saved_server_profile_records(app: &AppHandle) -> Result<Vec<StoredServerProfile>, String> {
    let profiles_path = saved_server_profiles_path(app)?;
    if !profiles_path.exists() {
        return Ok(Vec::new());
    }

    let contents = std::fs::read_to_string(&profiles_path).map_err(|e| e.to_string())?;
    serde_json::from_str::<Vec<StoredServerProfile>>(&contents)
        .map_err(|e| format!("Failed to parse saved server profiles JSON: {}", e))
}

fn save_saved_server_profile_records(
    app: &AppHandle,
    profiles: &[StoredServerProfile],
) -> Result<(), String> {
    let profiles_path = saved_server_profiles_path(app)?;
    if let Some(parent) = profiles_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let json = serde_json::to_vec_pretty(profiles).map_err(|e| e.to_string())?;
    std::fs::write(profiles_path, json).map_err(|e| e.to_string())
}

fn upsert_saved_server_profile_record(
    app: &AppHandle,
    profile: &SavedServerProfile,
) -> Result<(), String> {
    let mut profiles = load_saved_server_profile_records(app)?;
    let id = saved_server_profile_id(profile);
    let saved_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    profiles.retain(|record| record.id != id);
    profiles.insert(
        0,
        StoredServerProfile {
            id,
            host: profile.host.clone(),
            user: profile.user.clone(),
            password: profile.password.clone(),
            saved_at,
        },
    );
    save_saved_server_profile_records(app, &profiles)
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
    std::fs::write(archived_profile_path, profile_json).map_err(|e| e.to_string())?;
    upsert_saved_server_profile_record(app, profile)
}

pub(crate) fn remove_saved_server_profile(app: &AppHandle) -> Result<(), String> {
    let profile_path = server_profile_path(app)?;

    match std::fs::remove_file(profile_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn clear_saved_server_profiles(app: &AppHandle) -> Result<(), String> {
    let profiles_path = saved_server_profiles_path(app)?;

    match std::fs::remove_file(profiles_path) {
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
    let vless_uuid = outbounds
        .iter()
        .find(|outbound| {
            outbound.get("type").and_then(Value::as_str) == Some("vless")
                && outbound.get("tag").and_then(Value::as_str) == Some("vless-proxy")
        })
        .and_then(|outbound| outbound.get("uuid"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

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
        vless_uuid,
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
    let vless_inbound = inbounds
        .iter()
        .find(|inbound| inbound.get("type").and_then(Value::as_str) == Some("vless"));

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
    let vless_external_port = vless_inbound
        .and_then(|inbound| inbound.get("listen_port"))
        .and_then(Value::as_u64)
        .map(|port| port as u16)
        .unwrap_or(0);
    let vless_uuid = vless_inbound
        .and_then(|inbound| inbound.get("users"))
        .and_then(Value::as_array)
        .and_then(|users| users.first())
        .and_then(|user| user.get("uuid"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
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
        vless_external_port,
        vless_uuid,
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
pub async fn regenerate_invite_vless_link(
    app: AppHandle,
    invite_id: String,
) -> Result<invite::RegeneratedVlessInviteLinkResult, String> {
    invite::regenerate_invite_vless_link(app, invite_id).await
}

#[tauri::command]
pub async fn import_invite_link(
    app: AppHandle,
    invite_link: String,
) -> Result<invite::InviteImportResult, String> {
    invite::import_invite_link(app, invite_link).await
}

#[tauri::command]
pub fn list_imported_invite_profiles(
    app: AppHandle,
) -> Result<Vec<invite::ImportedInviteProfile>, String> {
    invite::list_imported_invite_profiles(app)
}

#[tauri::command]
pub async fn activate_imported_invite_profile(
    app: AppHandle,
    profile_id: String,
) -> Result<invite::InviteImportResult, String> {
    invite::activate_imported_invite_profile(app, profile_id).await
}

#[tauri::command]
pub fn delete_imported_invite_profile(app: AppHandle, profile_id: String) -> Result<(), String> {
    invite::delete_imported_invite_profile(app, profile_id)
}

#[tauri::command]
pub fn list_saved_server_profiles(app: AppHandle) -> Result<Vec<SavedServerProfileEntry>, String> {
    let active = warp::load_saved_server_profile(app.clone())?;
    let active_id = active.as_ref().map(saved_server_profile_id);
    let mut records = load_saved_server_profile_records(&app)?;

    if let (Some(active_profile), Some(active_id)) = (active.as_ref(), active_id.as_ref()) {
        if !records.iter().any(|record| record.id == *active_id) {
            records.insert(
                0,
                StoredServerProfile {
                    id: active_id.clone(),
                    host: active_profile.host.clone(),
                    user: active_profile.user.clone(),
                    password: active_profile.password.clone(),
                    saved_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|e| e.to_string())?
                        .as_secs(),
                },
            );
            save_saved_server_profile_records(&app, &records)?;
        }
    }

    Ok(records
        .into_iter()
        .map(|record| SavedServerProfileEntry {
            is_active: active_id.as_deref() == Some(record.id.as_str()),
            id: record.id,
            host: record.host,
            user: record.user,
            saved_at: record.saved_at,
        })
        .collect())
}

#[tauri::command]
pub fn add_saved_server_profile(
    app: AppHandle,
    host: String,
    user: String,
    password: String,
) -> Result<SavedServerProfile, String> {
    if host.trim().is_empty() || user.trim().is_empty() || password.trim().is_empty() {
        return Err(
            "Server IP, login, and password are required before saving a server.".to_string(),
        );
    }

    let profile = SavedServerProfile {
        host: host.trim().to_string(),
        user: user.trim().to_string(),
        password,
    };
    upsert_saved_server_profile_record(&app, &profile)?;
    save_backend_app_role(&app, BackendAppRole::Master)?;
    Ok(profile)
}

#[tauri::command]
pub fn activate_saved_server_profile(
    app: AppHandle,
    profile_id: String,
) -> Result<SavedServerProfile, String> {
    let records = load_saved_server_profile_records(&app)?;
    let Some(record) = records.into_iter().find(|record| record.id == profile_id) else {
        return Err("Saved server profile not found.".to_string());
    };
    let profile = SavedServerProfile {
        host: record.host,
        user: record.user,
        password: record.password,
    };
    save_server_profile(&app, &profile)?;
    save_backend_app_role(&app, BackendAppRole::Master)?;
    Ok(profile)
}

#[tauri::command]
pub fn delete_saved_server_profile(app: AppHandle, profile_id: String) -> Result<(), String> {
    let mut records = load_saved_server_profile_records(&app)?;
    let original_len = records.len();
    records.retain(|record| record.id != profile_id);
    if records.len() == original_len {
        return Err("Saved server profile not found.".to_string());
    }
    save_saved_server_profile_records(&app, &records)?;

    if warp::load_saved_server_profile(app.clone())?
        .as_ref()
        .map(saved_server_profile_id)
        .as_deref()
        == Some(profile_id.as_str())
    {
        remove_saved_server_profile(&app)?;
    }

    Ok(())
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
