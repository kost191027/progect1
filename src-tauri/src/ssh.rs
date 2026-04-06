use serde::{Deserialize, Serialize};
use ssh2::Session;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Write;
use std::net::SocketAddr;
use std::net::TcpStream;
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};

const PRIMARY_EXTERNAL_PORT: u16 = 4433;
const EXTERNAL_PORT_CANDIDATES: [u16; 5] = [4433, 443, 5443, 7443, 9443];
const PINNED_SING_BOX_IMAGE: &str = "ghcr.io/sagernet/sing-box:v1.13.5";
const CONTAINER_PREFIXES: [&str; 5] = [
    "sys-networkd",
    "mdns-relay",
    "core-authd",
    "netdiag-agent",
    "kernel-events",
];
const LEGACY_CONTAINER_NAME: &str = "sys-network-helper";
const SSH_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REMOTE_DEPLOY_STALL_TIMEOUT: Duration = Duration::from_secs(60);
const REMOTE_DEPLOY_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug)]
struct RemoteDeployTarget {
    external_port: u16,
    container_name: String,
    reusing_existing_instance: bool,
    migrating_to_primary_port: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SavedServerProfile {
    pub host: String,
    pub user: String,
    pub password: String,
}

fn emit_ssh_stage(app: &AppHandle, stage: &str, message: impl Into<String>) {
    let _ = app.emit("tunnel-log", format!("[SSH:{}] {}", stage, message.into()));
}

fn server_profile_path(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;

    Ok(local_data.join("server_profile.json"))
}

fn save_server_profile(app: &AppHandle, profile: &SavedServerProfile) -> Result<(), String> {
    let profile_path = server_profile_path(app)?;

    if let Some(parent) = profile_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let profile_json = serde_json::to_vec_pretty(profile).map_err(|e| e.to_string())?;
    std::fs::write(profile_path, profile_json).map_err(|e| e.to_string())
}

fn verify_external_port_reachable(host: &str, external_port: u16) -> Result<(), String> {
    let address = format!("{}:{}", host, external_port);
    let mut resolved_addrs = address
        .to_socket_addrs()
        .map_err(|e| format!("Failed to resolve {}: {}", address, e))?;

    let timeout = Duration::from_secs(5);
    let mut last_error = None;

    for socket_addr in resolved_addrs.by_ref() {
        match TcpStream::connect_timeout(&socket_addr, timeout) {
            Ok(stream) => {
                let _ = stream.shutdown(std::net::Shutdown::Both);
                return Ok(());
            }
            Err(err) => {
                last_error = Some(format!("{} ({})", socket_addr, err));
            }
        }
    }

    Err(format!(
        "External port {} is not reachable from this client after deploy. Last error: {}. Check provider firewall/security group and chosen port.",
        external_port,
        last_error.unwrap_or_else(|| "no resolved addresses".to_string())
    ))
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

fn connect_ssh_session(
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

fn build_container_name(short_id: &str) -> String {
    let seed = short_id
        .get(0..2)
        .and_then(|prefix| u8::from_str_radix(prefix, 16).ok())
        .unwrap_or(0);
    let prefix = CONTAINER_PREFIXES[(seed as usize) % CONTAINER_PREFIXES.len()];
    let suffix = short_id.get(0..6).unwrap_or(short_id);

    format!("{}-{}", prefix, suffix)
}

fn run_remote_command(sess: &Session, command: &str) -> Result<(String, i32), String> {
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

fn validate_remote_runtime(
    sess: &Session,
    container_name: &str,
    external_port: u16,
) -> Result<(), String> {
    let command = format!(
        r#"bash -lc '
docker inspect -f "{{{{.State.Running}}}}" "{container_name}" | grep -q true &&
ss -ltn | grep -q ":{external_port} " &&
ss -ltn | grep -q "127.0.0.1:{internal_ss_port} "
'"#,
        container_name = container_name,
        external_port = external_port,
        internal_ss_port = crate::generator::INTERNAL_SS_PORT
    );

    let (stdout, exit_status) = run_remote_command(sess, &command)?;

    if exit_status == 0 {
        Ok(())
    } else {
        Err(format!(
            "Remote runtime validation failed for container {} on port {}. Output: {}",
            container_name,
            external_port,
            stdout.trim()
        ))
    }
}

fn stream_remote_deploy_output(
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
                        "Remote deploy stalled: no output for more than {:?}",
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
                        "Remote deploy stalled: no output for more than {:?}",
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

fn select_remote_deploy_target(
    sess: &Session,
    short_id: &str,
) -> Result<RemoteDeployTarget, String> {
    let candidates = EXTERNAL_PORT_CANDIDATES
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    let generated_container_name = build_container_name(short_id);

    let command = format!(
        r#"bash -lc '
CONFIG_DIR="/opt/rkn"
ACTIVE_CONTAINER_FILE="$CONFIG_DIR/container_name"
PREVIOUS_CONTAINER=""
PRIMARY_PORT="{primary_port}"

if command -v docker >/dev/null 2>&1; then
  if [ -f "$ACTIVE_CONTAINER_FILE" ]; then
    PREVIOUS_CONTAINER="$(cat "$ACTIVE_CONTAINER_FILE" 2>/dev/null || true)"
  fi

  if [ -z "$PREVIOUS_CONTAINER" ] && docker inspect "{legacy_container_name}" >/dev/null 2>&1; then
    PREVIOUS_CONTAINER="{legacy_container_name}"
  fi

  if [ -n "$PREVIOUS_CONTAINER" ] && docker inspect "$PREVIOUS_CONTAINER" >/dev/null 2>&1; then
    CURRENT_PORT="$(grep -m1 '"'"'"listen_port"'"'"' "$CONFIG_DIR/config.json" | sed -E '"'"'s/[^0-9]*([0-9]+).*/\1/'"'"' || true)"
    if [ "$CURRENT_PORT" != "$PRIMARY_PORT" ] && ! ss -Htanl "( sport = :$PRIMARY_PORT )" | grep -q .; then
      echo "port=$PRIMARY_PORT"
      echo "container=$PREVIOUS_CONTAINER"
      echo "reuse=true"
      echo "migrate_primary=true"
      exit 0
    fi

    for candidate in {candidates}; do
      if [ "$candidate" = "$CURRENT_PORT" ]; then
        echo "port=$CURRENT_PORT"
        echo "container=$PREVIOUS_CONTAINER"
        echo "reuse=true"
        echo "migrate_primary=false"
        exit 0
      fi
    done
  fi
fi

for port in {candidates}; do
  if ! ss -Htanl "( sport = :$port )" | grep -q .; then
    echo "port=$port"
    echo "container={generated_container_name}"
    echo "reuse=false"
    echo "migrate_primary=false"
    exit 0
  fi
done

exit 1
'"#,
        legacy_container_name = LEGACY_CONTAINER_NAME,
        candidates = candidates,
        generated_container_name = generated_container_name,
        primary_port = PRIMARY_EXTERNAL_PORT
    );

    let (stdout, exit_status) = run_remote_command(sess, &command)?;

    if exit_status != 0 {
        return Err(format!(
            "No free external ports found in candidate list: {}",
            candidates
        ));
    }

    let mut selected_port = None;
    let mut selected_container_name = None;
    let mut reusing_existing_instance = false;
    let mut migrating_to_primary_port = false;

    for line in stdout.lines() {
        if let Some(value) = line.trim().strip_prefix("port=") {
            selected_port = value.trim().parse::<u16>().ok();
        } else if let Some(value) = line.trim().strip_prefix("container=") {
            if !value.trim().is_empty() {
                selected_container_name = Some(value.trim().to_string());
            }
        } else if let Some(value) = line.trim().strip_prefix("reuse=") {
            reusing_existing_instance = value.trim() == "true";
        } else if let Some(value) = line.trim().strip_prefix("migrate_primary=") {
            migrating_to_primary_port = value.trim() == "true";
        }
    }

    let external_port = selected_port.ok_or_else(|| {
        format!(
            "Failed to parse remote selected port from output: {}",
            stdout
        )
    })?;
    let container_name = selected_container_name.unwrap_or_else(|| build_container_name(short_id));

    Ok(RemoteDeployTarget {
        external_port,
        container_name,
        reusing_existing_instance,
        migrating_to_primary_port,
    })
}

#[tauri::command]
pub async fn deploy_server(
    app: AppHandle,
    host: String,
    user: String,
    pass: String,
) -> Result<(), String> {
    let _ = app.emit(
        "tunnel-log",
        "[SYSTEM] Generating transport credentials via sing-box...".to_string(),
    );

    // 1. Generate transport secrets asynchronously using local sing-box
    let short_id = crate::generator::generate_short_id(&app)
        .await
        .map_err(|e| format!("Transport secret error: {}", e))?;
    let shadow_pass = crate::generator::generate_shadowtls_password(&app)
        .await
        .map_err(|e| format!("ShadowTLS password error: {}", e))?;
    let ss_password = crate::generator::generate_ss_password(&app)
        .await
        .map_err(|e| format!("Shadowsocks password error: {}", e))?;

    let _ = app.emit(
        "tunnel-log",
        format!(
            "[SYSTEM] Transport stack: ShadowTLS v3 + Shadowsocks-2022. ShadowTLS secret length: {} chars.",
            shadow_pass.len()
        ),
    );

    // Получаем путь к AppData
    let local_data = app
        .path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());
    let saved_profile = SavedServerProfile {
        host: host.clone(),
        user: user.clone(),
        password: pass.clone(),
    };

    // 4. Perform SSH upload and deployment (Blocking)
    tauri::async_runtime::spawn_blocking(move || {
        let _ = app.emit(
            "tunnel-log",
            format!("--- [SSH] Connecting to {}:22 ---", host),
        );

        let sess = connect_ssh_session(&app, &host, &user, &pass)?;
        emit_ssh_stage(&app, "AUTH", "Authenticated successfully.");
        emit_ssh_stage(&app, "PREFLIGHT", "Running remote pre-flight checks for deploy target...");

        let deploy_target = select_remote_deploy_target(&sess, &short_id)?;
        let external_port = deploy_target.external_port;
        let container_name = deploy_target.container_name;

        if deploy_target.reusing_existing_instance {
            let message = if deploy_target.migrating_to_primary_port {
                format!(
                    "[SSH] Existing RKN instance detected. Preferred port {} is free, migrating container {} back to external port {}.",
                    PRIMARY_EXTERNAL_PORT, container_name, PRIMARY_EXTERNAL_PORT
                )
            } else {
                format!(
                    "[SSH] Existing RKN instance detected. Reusing container {} and external port {}.",
                    container_name, external_port
                )
            };
            let _ = app.emit("tunnel-log", message);
        } else if external_port == PRIMARY_EXTERNAL_PORT {
            let _ = app.emit(
                "tunnel-log",
                format!(
                    "[SSH] Preferred external port {} is available on remote host.",
                    PRIMARY_EXTERNAL_PORT
                ),
            );
        } else {
            let _ = app.emit(
                "tunnel-log",
                format!(
                    "[SSH WARN] Preferred external port {} is busy. Falling back to external port {}.",
                    PRIMARY_EXTERNAL_PORT, external_port
                ),
            );
        }

        let _ = app.emit(
            "tunnel-log",
            format!(
                "[SSH] Selected pinned image {} and container name {}.",
                PINNED_SING_BOX_IMAGE, container_name
            ),
        );

        let deploy_script = include_str!("../scripts/deploy.sh");
        let cover_domain = crate::generator::select_cover_domain(&short_id);
        let _ = app.emit(
            "tunnel-log",
            format!("[SSH] ShadowTLS cover domain: {}", cover_domain),
        );
        let server_cfg =
            crate::generator::build_server_config(&host, &shadow_pass, &ss_password, external_port, cover_domain);
        let client_cfg =
            crate::generator::build_client_config(&host, &shadow_pass, &ss_password, external_port, cover_domain);

        emit_ssh_stage(
            &app,
            "DEPLOY",
            format!("Deploying transport on external port {}...", external_port),
        );

        let injected_script = format!(
            r#"#!/bin/bash
mkdir -p /opt/rkn
export RKN_IMAGE='{}'
export RKN_CONTAINER_NAME='{}'
cat << 'CONFIGEOF' > /opt/rkn/config.candidate.json
{}
CONFIGEOF

{}
"#,
            PINNED_SING_BOX_IMAGE, container_name, server_cfg, deploy_script
        );

        emit_ssh_stage(&app, "UPLOAD", "Uploading generated config and deploy script...");
        let mut channel = sess.channel_session().map_err(|e| e.to_string())?;
        emit_ssh_stage(&app, "DEPLOY", "Executing remote fast-deploy script...");
        channel.exec("bash -s 2>&1").map_err(|e| e.to_string())?;

        channel
            .write_all(injected_script.as_bytes())
            .map_err(|e| e.to_string())?;
        channel.send_eof().map_err(|e| e.to_string())?;
        stream_remote_deploy_output(&app, &sess, &mut channel)?;

        channel
            .wait_close()
            .map_err(|e| format!("Failed to wait for remote deploy close: {}", e))?;
        let exit_status = channel
            .exit_status()
            .map_err(|e| format!("Failed to read remote deploy exit status: {}", e))?;

        if exit_status != 0 {
            let _ = app.emit(
                "tunnel-log",
                format!("[SSH ERROR] Deployment failed with code: {}", exit_status),
            );
            return Err(format!("Deployment script exited with {}", exit_status));
        }

        emit_ssh_stage(
            &app,
            "VALIDATE",
            format!(
                "Remote deploy finished. Validating container {} and ports...",
                container_name
            ),
        );
        validate_remote_runtime(&sess, &container_name, external_port)?;

        emit_ssh_stage(
            &app,
            "VALIDATE",
            format!(
                "Remote runtime looks healthy. Verifying external port {} from this client...",
                external_port
            ),
        );
        verify_external_port_reachable(&host, external_port)?;

        emit_ssh_stage(
            &app,
            "VALIDATE",
            format!("External port {} is reachable from this client.", external_port),
        );

        std::fs::create_dir_all(&local_data).map_err(|e| e.to_string())?;
        let client_cfg_path = local_data.join("client_config.json");
        std::fs::write(&client_cfg_path, &client_cfg).map_err(|e| e.to_string())?;
        save_server_profile(&app, &saved_profile)?;

        let _ = app.emit(
            "tunnel-log",
            format!(
                "[SYSTEM] Client config safely generated at: {:?}",
                client_cfg_path
            ),
        );
        let _ = app.emit(
            "tunnel-log",
            format!(
                "[SSH] Deployment finished successfully! External port: {}",
                external_port
            ),
        );
        let _ = app.emit(
            "tunnel-log",
            "[SYSTEM] Server credentials saved locally for next launch.".to_string(),
        );
        Ok(())
    })
    .await
    .unwrap()
}

#[tauri::command]
pub fn load_saved_server_profile(app: AppHandle) -> Result<Option<SavedServerProfile>, String> {
    let profile_path = server_profile_path(&app)?;

    if !profile_path.exists() {
        return Ok(None);
    }

    let profile_json = std::fs::read_to_string(profile_path).map_err(|e| e.to_string())?;
    let profile = serde_json::from_str::<SavedServerProfile>(&profile_json)
        .map_err(|e| format!("Failed to parse saved server profile: {}", e))?;

    Ok(Some(profile))
}

#[tauri::command]
pub async fn check_server_status(app: AppHandle) -> Result<String, String> {
    let profile = load_saved_server_profile(app.clone())?
        .ok_or_else(|| "Saved server profile not found. Deploy once first.".to_string())?;

    tauri::async_runtime::spawn_blocking(move || {
        let _ = app.emit(
            "tunnel-log",
            format!("--- [SSH] Checking server status on {}:22 ---", profile.host),
        );

        let sess = connect_ssh_session(&app, &profile.host, &profile.user, &profile.password)?;
        emit_ssh_stage(&app, "STATUS", "Collecting docker runtime diagnostics...");

        let command = r#"bash -lc '
CONFIG_DIR="/opt/rkn"
ACTIVE_CONTAINER_FILE="$CONFIG_DIR/container_name"
CONTAINER_NAME="$(cat "$ACTIVE_CONTAINER_FILE" 2>/dev/null || true)"

echo "[STATUS] host=$(hostname)"
echo "[STATUS] container_name=${CONTAINER_NAME:-<missing>}"
echo "[STATUS] docker_ps"
docker ps -a --format "table {{.Names}}\t{{.Image}}\t{{.Status}}" | sed -n "1,10p"

if [ -n "$CONTAINER_NAME" ] && docker inspect "$CONTAINER_NAME" >/dev/null 2>&1; then
  echo "[STATUS] docker_inspect"
  docker inspect -f "running={{.State.Running}} pid={{.State.Pid}} started={{.State.StartedAt}}" "$CONTAINER_NAME"
  echo "[STATUS] listening_sockets"
  ss -ltnup | grep -E ":(443|4433|5443|7443|9443|14433)\b" || true
  echo "[STATUS] active_config"
  cat "$CONFIG_DIR/config.json"
  echo "[STATUS] docker_logs"
  docker logs --tail 120 "$CONTAINER_NAME" 2>&1
else
  echo "[STATUS] active container missing or not inspectable"
  echo "[STATUS] active_config"
  cat "$CONFIG_DIR/config.json" 2>/dev/null || true
fi
'"#;

        let (stdout, exit_status) = run_remote_command(&sess, command)?;
        for line in stdout.lines() {
            if !line.trim().is_empty() {
                let _ = app.emit("tunnel-log", format!("[SERVER STATUS] {}", line));
            }
        }

        if exit_status == 0 {
            Ok(stdout)
        } else {
            Err(format!(
                "Server status command failed with code {}. Output: {}",
                exit_status, stdout
            ))
        }
    })
    .await
    .unwrap()
}

/// Rotate ShadowTLS cover domain: generates new credentials, re-deploys
/// the server container with a fresh cover domain, and saves a new client config.
#[tauri::command]
pub async fn rotate_sni(app: AppHandle) -> Result<String, String> {
    let profile = load_saved_server_profile(app.clone())?
        .ok_or_else(|| "Saved server profile not found. Deploy once first.".to_string())?;

    let _ = app.emit(
        "tunnel-log",
        "[SYSTEM] Rotating ShadowTLS cover domain...".to_string(),
    );

    let short_id = crate::generator::generate_short_id(&app)
        .await
        .map_err(|e| format!("Short ID generation error: {}", e))?;
    let shadow_pass = crate::generator::generate_shadowtls_password(&app)
        .await
        .map_err(|e| format!("ShadowTLS password error: {}", e))?;
    let ss_password = crate::generator::generate_ss_password(&app)
        .await
        .map_err(|e| format!("Shadowsocks password error: {}", e))?;

    let cover_domain = crate::generator::select_cover_domain(&short_id);
    let _ = app.emit(
        "tunnel-log",
        format!("[SYSTEM] New cover domain: {}", cover_domain),
    );

    let local_data = app
        .path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());

    let result_domain = cover_domain.to_string();

    tauri::async_runtime::spawn_blocking(move || {
        let sess = connect_ssh_session(&app, &profile.host, &profile.user, &profile.password)?;

        let deploy_target = select_remote_deploy_target(&sess, &short_id)?;
        let external_port = deploy_target.external_port;
        let container_name = deploy_target.container_name;

        let deploy_script = include_str!("../scripts/deploy.sh");
        let server_cfg = crate::generator::build_server_config(
            &profile.host,
            &shadow_pass,
            &ss_password,
            external_port,
            cover_domain,
        );
        let client_cfg = crate::generator::build_client_config(
            &profile.host,
            &shadow_pass,
            &ss_password,
            external_port,
            cover_domain,
        );

        let injected_script = format!(
            r#"#!/bin/bash
mkdir -p /opt/rkn
export RKN_IMAGE='{}'
export RKN_CONTAINER_NAME='{}'
cat << 'CONFIGEOF' > /opt/rkn/config.candidate.json
{}
CONFIGEOF

{}
"#,
            PINNED_SING_BOX_IMAGE, container_name, server_cfg, deploy_script
        );

        emit_ssh_stage(&app, "ROTATE", "Deploying new cover domain to server...");
        let mut channel = sess.channel_session().map_err(|e| e.to_string())?;
        channel.exec("bash -s 2>&1").map_err(|e| e.to_string())?;
        channel
            .write_all(injected_script.as_bytes())
            .map_err(|e| e.to_string())?;
        channel.send_eof().map_err(|e| e.to_string())?;
        stream_remote_deploy_output(&app, &sess, &mut channel)?;

        channel.wait_close().map_err(|e| e.to_string())?;
        let exit_status = channel.exit_status().map_err(|e| e.to_string())?;

        if exit_status != 0 {
            return Err(format!(
                "SNI rotation deploy failed with code {}",
                exit_status
            ));
        }

        validate_remote_runtime(&sess, &container_name, external_port)?;

        std::fs::create_dir_all(&local_data).map_err(|e| e.to_string())?;
        let client_cfg_path = local_data.join("client_config.json");
        std::fs::write(&client_cfg_path, &client_cfg).map_err(|e| e.to_string())?;

        let _ = app.emit(
            "tunnel-log",
            format!(
                "[SYSTEM] SNI rotated to {}. Restart tunnel to apply.",
                cover_domain
            ),
        );

        Ok(result_domain)
    })
    .await
    .unwrap()
}
