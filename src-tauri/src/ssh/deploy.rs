use serde_json::json;
use ssh2::Session;
use std::io::Write;
use tauri::{AppHandle, Emitter, Manager};

use super::invite::resolve_master_ss_transport;
use super::warp::{
    ensure_remote_warp_config, has_local_warp_profile_sync, load_remote_warp_config,
};
use super::{
    acquire_remote_mutation_lock, build_container_name, connect_ssh_session, emit_ssh_stage,
    ensure_local_client_rule_sets_sync, ensure_master_role, load_remote_container_image,
    load_remote_container_name, load_remote_transport_bootstrap,
    pinned_sing_box_image_for_routing_mode, remote_runtime_uses_warp, run_remote_command,
    save_backend_app_role, save_cached_transport_bootstrap, save_server_profile,
    snapshot_for_cover_domain, stream_remote_deploy_output, BackendAppRole, RemoteDeployTarget,
    RemoteTransportBootstrap, SavedServerProfile, TransportStateSnapshot, EXTERNAL_PORT_CANDIDATES,
    INTERNAL_SS_PORT_CANDIDATES, LEGACY_CONTAINER_NAME, PRIMARY_EXTERNAL_PORT,
    VLESS_EXTERNAL_PORT_CANDIDATES,
};

const TRANSPORT_SMOKE_TARGETS: &[&str] = &[
    "https://www.gstatic.com/generate_204",
    "https://cp.cloudflare.com/generate_204",
    "https://www.apple.com/library/test/success.html",
];

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

fn compose_multi_user_ss_password(server_password: &str, user_password: &str) -> String {
    format!("{}:{}", server_password, user_password)
}

fn backup_local_file_if_exists(path: &std::path::Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }

    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let backup_path = path.with_extension(format!("bak.{}", millis));
    std::fs::copy(path, &backup_path).map_err(|e| {
        format!(
            "Failed to create local backup {} from {}: {}",
            backup_path.display(),
            path.display(),
            e
        )
    })?;

    Ok(())
}

pub(crate) struct RemoteDeployExecution<'a> {
    pub(crate) container_name: &'a str,
    pub(crate) external_port: u16,
    pub(crate) vless_external_port: u16,
    pub(crate) internal_ss_port: u16,
    pub(crate) sing_box_image: &'a str,
    pub(crate) server_cfg: &'a str,
    pub(crate) bootstrap_cfg: &'a str,
}

pub(crate) fn select_remote_deploy_target(
    sess: &Session,
    short_id: &str,
) -> Result<RemoteDeployTarget, String> {
    let candidates = EXTERNAL_PORT_CANDIDATES
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    let vless_candidates = VLESS_EXTERNAL_PORT_CANDIDATES
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    let internal_candidates = INTERNAL_SS_PORT_CANDIDATES
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    let generated_container_name = build_container_name(short_id);

    let script = format!(
        r#"
CONFIG_DIR="/opt/rkn"
ACTIVE_CONTAINER_FILE="$CONFIG_DIR/container_name"
ACTIVE_CONFIG="$CONFIG_DIR/config.json"
ACTIVE_BOOTSTRAP="$CONFIG_DIR/bootstrap.json"
PREVIOUS_CONTAINER=""
SELECTED_PORT=""
SELECTED_VLESS_PORT=""
INTERNAL_PORT=""
RKN_MANAGED_LABEL="com.freedom.rkn.managed=true"

is_tcp_port_listening() {{
  port="$1"
  if command -v ss >/dev/null 2>&1; then
    ss -Htanl 2>/dev/null | awk '"'"'{{print $4}}'"'"' | grep -Eq "(^|:)$port$"
    return $?
  fi
  if command -v netstat >/dev/null 2>&1; then
    netstat -tnl 2>/dev/null | awk '"'"'{{print $4}}'"'"' | grep -Eq "(^|[.:])$port$"
    return $?
  fi
  return 1
}}

if command -v docker >/dev/null 2>&1; then
  if [ -f "$ACTIVE_CONTAINER_FILE" ]; then
    PREVIOUS_CONTAINER="$(cat "$ACTIVE_CONTAINER_FILE" 2>/dev/null || true)"
  fi

  if [ -z "$PREVIOUS_CONTAINER" ] && docker inspect "{legacy_container_name}" >/dev/null 2>&1; then
    PREVIOUS_CONTAINER="{legacy_container_name}"
  fi

  if [ -z "$PREVIOUS_CONTAINER" ]; then
    PREVIOUS_CONTAINER="$(docker ps -a --filter "label=$RKN_MANAGED_LABEL" --format '"'"'{{{{.Names}}}}'"'"' 2>/dev/null | head -n 1 || true)"
  fi

  if [ -n "$PREVIOUS_CONTAINER" ] && docker inspect "$PREVIOUS_CONTAINER" >/dev/null 2>&1; then
    CURRENT_PORT=""
    CURRENT_INTERNAL_PORT=""
    if [ -f "$ACTIVE_CONFIG" ] && command -v jq >/dev/null 2>&1; then
      CURRENT_PORT="$(jq -r '"'"'[.inbounds[]? | select(.type=="shadowtls") | .listen_port][0] // empty'"'"' "$ACTIVE_CONFIG" 2>/dev/null || true)"
      CURRENT_VLESS_PORT="$(jq -r '"'"'[.inbounds[]? | select(.type=="vless") | .listen_port][0] // empty'"'"' "$ACTIVE_CONFIG" 2>/dev/null || true)"
      CURRENT_INTERNAL_PORT="$(jq -r '"'"'[.inbounds[]? | select(.type=="shadowsocks") | .listen_port][0] // empty'"'"' "$ACTIVE_CONFIG" 2>/dev/null || true)"
    fi
    if [ -z "$CURRENT_PORT" ] && [ -f "$ACTIVE_CONFIG" ]; then
      CURRENT_PORT="$(grep '"'"'"listen_port"'"' "$ACTIVE_CONFIG" | sed -n '1p' | sed -E '"'"'s/[^0-9]*([0-9]+).*/\1/'"'"' || true)"
    fi
    if [ -z "$CURRENT_VLESS_PORT" ] && [ -f "$ACTIVE_BOOTSTRAP" ]; then
      CURRENT_VLESS_PORT="$(grep '"'"'"vless_external_port"'"' "$ACTIVE_BOOTSTRAP" | sed -E '"'"'s/[^0-9]*([0-9]+).*/\1/'"'"' || true)"
    fi
    if [ -z "$CURRENT_INTERNAL_PORT" ] && [ -f "$ACTIVE_CONFIG" ]; then
      if [ -n "$CURRENT_VLESS_PORT" ]; then
        CURRENT_INTERNAL_PORT="$(grep '"'"'"listen_port"'"' "$ACTIVE_CONFIG" | sed -n '3p' | sed -E '"'"'s/[^0-9]*([0-9]+).*/\1/'"'"' || true)"
      else
        CURRENT_INTERNAL_PORT="$(grep '"'"'"listen_port"'"' "$ACTIVE_CONFIG" | sed -n '2p' | sed -E '"'"'s/[^0-9]*([0-9]+).*/\1/'"'"' || true)"
      fi
    fi
    if [ -z "$CURRENT_INTERNAL_PORT" ] && [ -f "$ACTIVE_BOOTSTRAP" ]; then
      CURRENT_INTERNAL_PORT="$(grep '"'"'"internal_ss_port"'"' "$ACTIVE_BOOTSTRAP" | sed -E '"'"'s/[^0-9]*([0-9]+).*/\1/'"'"' || true)"
    fi
    if [ -n "$CURRENT_PORT" ]; then
      SELECTED_PORT="$CURRENT_PORT"
      echo "port=$CURRENT_PORT"
      echo "container=$PREVIOUS_CONTAINER"
      echo "reuse=true"
      echo "migrate_primary=false"
      if [ -n "$CURRENT_VLESS_PORT" ]; then
        SELECTED_VLESS_PORT="$CURRENT_VLESS_PORT"
        echo "vless_port=$CURRENT_VLESS_PORT"
      fi
      if [ -n "$CURRENT_INTERNAL_PORT" ]; then
        INTERNAL_PORT="$CURRENT_INTERNAL_PORT"
        echo "internal_port=$CURRENT_INTERNAL_PORT"
      fi
    fi
  fi
fi

if [ -z "$SELECTED_PORT" ]; then
  for port in {candidates}; do
    if ! is_tcp_port_listening "$port"; then
      SELECTED_PORT="$port"
      echo "port=$port"
      echo "container={generated_container_name}"
      echo "reuse=false"
      echo "migrate_primary=false"
      break
    fi
  done
fi

if [ -z "$INTERNAL_PORT" ]; then
  for port in {internal_candidates}; do
    if ! is_tcp_port_listening "$port"; then
      INTERNAL_PORT="$port"
      break
    fi
  done
  if [ -n "$INTERNAL_PORT" ]; then
    echo "internal_port=$INTERNAL_PORT"
  fi
fi

if [ -z "$SELECTED_VLESS_PORT" ]; then
  for port in {vless_candidates}; do
    if [ "$port" = "$SELECTED_PORT" ]; then
      continue
    fi
    if ! is_tcp_port_listening "$port"; then
      SELECTED_VLESS_PORT="$port"
      echo "vless_port=$port"
      break
    fi
  done
fi

if [ -z "$SELECTED_PORT" ] || [ -z "$SELECTED_VLESS_PORT" ] || [ -z "$INTERNAL_PORT" ]; then
  if [ -z "$SELECTED_PORT" ]; then
    echo "missing_external_port=true"
  fi
  if [ -z "$SELECTED_VLESS_PORT" ]; then
    echo "missing_vless_external_port=true"
  fi
  if [ -z "$INTERNAL_PORT" ]; then
    echo "missing_internal_port=true"
  fi
  if [ -n "$PREVIOUS_CONTAINER" ]; then
    echo "previous_container=$PREVIOUS_CONTAINER"
  fi
  if [ -f "$ACTIVE_CONFIG" ]; then
    echo "active_config_present=true"
  fi
  exit 1
fi
exit 0
"#,
        legacy_container_name = LEGACY_CONTAINER_NAME,
        candidates = candidates,
        vless_candidates = vless_candidates,
        internal_candidates = internal_candidates,
        generated_container_name = generated_container_name
    );
    let command = format!("bash -lc {}", shell_single_quote(&script));

    let (stdout, exit_status) = run_remote_command(sess, &command)?;

    if exit_status != 0 {
        if let Ok(target) = select_remote_deploy_target_from_socket_dump(sess, short_id) {
            return Ok(target);
        }

        return Err(format!(
            "No deploy target could be selected. External candidates: {}; internal candidates: {}. Output: {}",
            candidates,
            internal_candidates,
            stdout.trim()
        ));
    }

    let mut selected_port = None;
    let mut selected_vless_port = None;
    let mut selected_internal_port = None;
    let mut selected_container_name = None;
    let mut reusing_existing_instance = false;
    let mut migrating_to_primary_port = false;

    for line in stdout.lines() {
        if let Some(value) = line.trim().strip_prefix("port=") {
            selected_port = value.trim().parse::<u16>().ok();
        } else if let Some(value) = line.trim().strip_prefix("vless_port=") {
            selected_vless_port = value.trim().parse::<u16>().ok();
        } else if let Some(value) = line.trim().strip_prefix("internal_port=") {
            selected_internal_port = value.trim().parse::<u16>().ok();
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
    let vless_external_port = selected_vless_port.ok_or_else(|| {
        format!(
            "Failed to parse remote selected VLESS port from output: {}",
            stdout
        )
    })?;
    let internal_ss_port = selected_internal_port.ok_or_else(|| {
        format!(
            "Failed to parse remote selected internal SS port from output: {}",
            stdout
        )
    })?;
    let container_name = selected_container_name.unwrap_or_else(|| build_container_name(short_id));

    Ok(RemoteDeployTarget {
        external_port,
        vless_external_port,
        internal_ss_port,
        container_name,
        reusing_existing_instance,
        migrating_to_primary_port,
    })
}

fn select_remote_deploy_target_from_socket_dump(
    sess: &Session,
    short_id: &str,
) -> Result<RemoteDeployTarget, String> {
    let command = r#"bash -lc '
if command -v ss >/dev/null 2>&1; then
  ss -Htanl 2>/dev/null || true
elif command -v netstat >/dev/null 2>&1; then
  netstat -tnl 2>/dev/null || true
fi
'"#;
    let (stdout, _exit_status) = run_remote_command(sess, command)?;
    let external_port = EXTERNAL_PORT_CANDIDATES
        .iter()
        .copied()
        .find(|port| !socket_dump_contains_port(&stdout, *port))
        .ok_or_else(|| {
            format!(
                "No free external ports found in fallback socket dump. Candidates: {}. Output: {}",
                EXTERNAL_PORT_CANDIDATES
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
                stdout.trim()
            )
        })?;
    let internal_ss_port = INTERNAL_SS_PORT_CANDIDATES
        .iter()
        .copied()
        .find(|port| !socket_dump_contains_port(&stdout, *port))
        .ok_or_else(|| {
            format!(
                "No free internal Shadowsocks ports found in fallback socket dump. Candidates: {}. Output: {}",
                INTERNAL_SS_PORT_CANDIDATES
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
                stdout.trim()
            )
        })?;
    let vless_external_port = VLESS_EXTERNAL_PORT_CANDIDATES
        .iter()
        .copied()
        .find(|port| *port != external_port && !socket_dump_contains_port(&stdout, *port))
        .ok_or_else(|| {
            format!(
                "No free VLESS external ports found in fallback socket dump. Candidates: {}. Output: {}",
                VLESS_EXTERNAL_PORT_CANDIDATES
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
                stdout.trim()
            )
        })?;

    Ok(RemoteDeployTarget {
        external_port,
        vless_external_port,
        internal_ss_port,
        container_name: build_container_name(short_id),
        reusing_existing_instance: false,
        migrating_to_primary_port: false,
    })
}

fn socket_dump_contains_port(socket_dump: &str, port: u16) -> bool {
    let colon_port = format!(":{}", port);
    let dot_port = format!(".{}", port);
    socket_dump.split_whitespace().any(|field| {
        let address = field
            .trim_matches(|c| c == '[' || c == ']')
            .trim_end_matches(',');
        address.ends_with(&colon_port) || address.ends_with(&dot_port)
    })
}

pub(crate) fn validate_remote_runtime(
    sess: &Session,
    container_name: &str,
    external_port: u16,
    vless_external_port: u16,
    internal_ss_port: u16,
) -> Result<(), String> {
    let command = format!(
        r#"bash -lc '
set -e

running="$(docker inspect -f "{{{{.State.Running}}}}" "{container_name}" 2>/dev/null || echo false)"
if [ "$running" != "true" ]; then
  echo "container_running=$running"
  echo "[docker_inspect]"
  docker inspect "{container_name}" 2>&1 || true
  echo "[docker_logs]"
  docker logs --tail 40 "{container_name}" 2>&1 || true
  exit 1
fi

socket_dump="$(ss -ltnup 2>&1 || true)"
echo "[ss]"
echo "$socket_dump"

echo "$socket_dump" | grep -Eq ":{external_port}\b" || {{
  echo "[error] external port {external_port} is not listening"
  echo "[docker_logs]"
  docker logs --tail 40 "{container_name}" 2>&1 || true
  exit 1
}}

echo "$socket_dump" | grep -Eq ":{vless_external_port}\b" || {{
  echo "[error] VLESS external port {vless_external_port} is not listening"
  echo "[docker_logs]"
  docker logs --tail 40 "{container_name}" 2>&1 || true
  exit 1
}}

echo "$socket_dump" | grep -Eq ":{internal_ss_port}\b" || {{
  echo "[error] internal Shadowsocks port {internal_ss_port} is not listening"
  echo "[docker_logs]"
  docker logs --tail 40 "{container_name}" 2>&1 || true
  exit 1
}}
'"#,
        container_name = container_name,
        external_port = external_port,
        vless_external_port = vless_external_port,
        internal_ss_port = internal_ss_port
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

pub(crate) fn summarize_runtime_validation_error(error: &str) -> String {
    if let Some(summary) = error.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with("[error]") || trimmed.starts_with("container_running=") {
            Some(trimmed.to_string())
        } else if trimmed.contains(r#"unknown field "local_address""#) {
            Some(
                "server image/config mismatch: WARP local_address requires the pinned sing-box v1.10.7 runtime"
                    .to_string(),
            )
        } else if trimmed.contains("bind: address already in use") {
            Some("internal Shadowsocks port is already in use on the host".to_string())
        } else {
            None
        }
    }) {
        return summary;
    }

    "remote runtime validation failed".to_string()
}

pub(crate) fn verify_external_port_reachable(host: &str, external_port: u16) -> Result<(), String> {
    use std::net::ToSocketAddrs;

    let address = format!("{}:{}", host, external_port);
    let mut resolved_addrs = address
        .to_socket_addrs()
        .map_err(|e| format!("Failed to resolve {}: {}", address, e))?;

    let timeout = std::time::Duration::from_secs(5);
    let mut last_error = None;

    for socket_addr in resolved_addrs.by_ref() {
        match std::net::TcpStream::connect_timeout(&socket_addr, timeout) {
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

fn summarize_transport_smoke_error(error: &str) -> String {
    let lower = error.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("context deadline exceeded") {
        "transport handshake timed out from this client".to_string()
    } else if lower.contains("hmac mismatch") || lower.contains("verify failed") {
        "ShadowTLS secret or cover-domain handshake was rejected by the server".to_string()
    } else if lower.contains("connection refused") {
        "transport port is reachable by TCP scan but refused the real proxy session".to_string()
    } else if lower.contains("eof") {
        "transport connection closed during handshake".to_string()
    } else {
        "client transport smoke-check failed".to_string()
    }
}

#[cfg(not(target_os = "android"))]
fn client_config_has_outbound_tag(client_cfg: &str, tag: &str) -> Result<bool, String> {
    let parsed = serde_json::from_str::<serde_json::Value>(client_cfg).map_err(|e| {
        format!(
            "Failed to parse generated client config for smoke-check outbound audit: {}",
            e
        )
    })?;

    Ok(parsed
        .get("outbounds")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|outbounds| {
            outbounds.iter().any(|outbound| {
                outbound.get("tag").and_then(serde_json::Value::as_str) == Some(tag)
            })
        }))
}

#[cfg(not(target_os = "android"))]
fn build_client_transport_smoke_config(client_cfg: &str) -> Result<String, String> {
    let parsed = serde_json::from_str::<serde_json::Value>(client_cfg).map_err(|e| {
        format!(
            "Failed to parse generated client config for smoke-check: {}",
            e
        )
    })?;
    let outbounds = parsed
        .get("outbounds")
        .cloned()
        .ok_or_else(|| "Generated client config has no outbounds for smoke-check".to_string())?;

    serde_json::to_string_pretty(&json!({
        "log": {
            "level": "warn"
        },
        "outbounds": outbounds
    }))
    .map_err(|e| format!("Failed to serialize transport smoke-check config: {}", e))
}

#[cfg(not(target_os = "android"))]
fn validate_local_client_config(
    app: &AppHandle,
    local_data: &std::path::Path,
    client_cfg: &str,
) -> Result<(), String> {
    std::fs::create_dir_all(local_data).map_err(|e| e.to_string())?;
    let check_cfg_path = local_data.join("client_config.candidate.check.json");
    std::fs::write(&check_cfg_path, client_cfg).map_err(|e| {
        format!(
            "Failed to write client config candidate {}: {}",
            check_cfg_path.display(),
            e
        )
    })?;

    let singbox_path = crate::resolve_singbox_path(app)?;
    let output = std::process::Command::new(&singbox_path)
        .args([
            "--disable-color",
            "check",
            "-c",
            check_cfg_path.to_string_lossy().as_ref(),
        ])
        .output()
        .map_err(|e| {
            format!(
                "Failed to start bundled sing-box client config validation with {}: {}",
                singbox_path, e
            )
        });

    let _ = std::fs::remove_file(&check_cfg_path);

    let output = output?;
    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!(
        "Generated client config failed bundled sing-box validation. stdout: {} stderr: {}",
        stdout.trim(),
        stderr.trim()
    ))
}

#[cfg(target_os = "android")]
fn validate_local_client_config(
    _app: &AppHandle,
    _local_data: &std::path::Path,
    _client_cfg: &str,
) -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "android"))]
fn run_client_transport_smoke_check(
    app: &AppHandle,
    local_data: &std::path::Path,
    client_cfg: &str,
) -> Result<(), String> {
    let smoke_cfg = build_client_transport_smoke_config(client_cfg)?;
    std::fs::create_dir_all(local_data).map_err(|e| e.to_string())?;
    let smoke_cfg_path = local_data.join("client_transport_smoke.json");
    std::fs::write(&smoke_cfg_path, smoke_cfg).map_err(|e| {
        format!(
            "Failed to write transport smoke-check config {}: {}",
            smoke_cfg_path.display(),
            e
        )
    })?;

    let singbox_path = crate::resolve_singbox_path(app)?;
    let mut required_outbounds = vec![("ShadowTLS/Shadowsocks", "proxy")];
    if client_config_has_outbound_tag(client_cfg, "vless-proxy")? {
        required_outbounds.push(("VLESS", "vless-proxy"));
    }

    let mut errors = Vec::new();
    for (label, outbound_tag) in required_outbounds {
        match run_client_transport_smoke_check_for_outbound(
            &singbox_path,
            &smoke_cfg_path,
            label,
            outbound_tag,
        ) {
            Ok(()) => {}
            Err(error) => errors.push(error),
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(not(target_os = "android"))]
fn run_client_transport_smoke_check_for_outbound(
    singbox_path: &str,
    smoke_cfg_path: &std::path::Path,
    label: &str,
    outbound_tag: &str,
) -> Result<(), String> {
    let mut errors = Vec::new();
    for target_url in TRANSPORT_SMOKE_TARGETS {
        match run_client_transport_smoke_check_once(
            singbox_path,
            smoke_cfg_path,
            outbound_tag,
            target_url,
        ) {
            Ok(()) => return Ok(()),
            Err(error) => errors.push(format!("{} => {}", target_url, error)),
        }
    }

    Err(format!(
        "{} smoke-check failed for outbound `{}`: all targets failed: {}",
        label,
        outbound_tag,
        errors.join("; ")
    ))
}

#[cfg(not(target_os = "android"))]
fn run_client_transport_smoke_check_once(
    singbox_path: &str,
    smoke_cfg_path: &std::path::Path,
    outbound_tag: &str,
    target_url: &str,
) -> Result<(), String> {
    let mut child = std::process::Command::new(singbox_path)
        .args([
            "--disable-color",
            "tools",
            "fetch",
            "-c",
            smoke_cfg_path.to_string_lossy().as_ref(),
            "-o",
            outbound_tag,
            target_url,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "Failed to start local sing-box transport smoke-check with {}: {}",
                singbox_path, e
            )
        })?;

    let started = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(8);
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("Failed to poll transport smoke-check: {}", e))?
        {
            let output = child
                .wait_with_output()
                .map_err(|e| format!("Failed to collect transport smoke-check output: {}", e))?;
            if status.success() {
                return Ok(());
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "{}. stdout: {} stderr: {}",
                summarize_transport_smoke_error(&format!("{} {}", stdout, stderr)),
                stdout.trim(),
                stderr.trim()
            ));
        }

        if started.elapsed() >= timeout {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .map_err(|e| format!("Failed to stop timed-out transport smoke-check: {}", e))?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "Transport smoke-check timed out after {:?}. stdout: {} stderr: {}",
                timeout,
                stdout.trim(),
                stderr.trim()
            ));
        }

        std::thread::sleep(std::time::Duration::from_millis(150));
    }
}

#[cfg(target_os = "android")]
fn run_client_transport_smoke_check(
    _app: &AppHandle,
    _local_data: &std::path::Path,
    _client_cfg: &str,
) -> Result<(), String> {
    Ok(())
}

pub(crate) fn execute_remote_deploy(
    sess: &Session,
    app: &AppHandle,
    execution: &RemoteDeployExecution<'_>,
) -> Result<(), String> {
    let deploy_script = include_str!("../../scripts/deploy.sh")
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let injected_script = format!(
        r#"#!/bin/bash
mkdir -p /opt/rkn
export RKN_IMAGE='{}'
export RKN_CONTAINER_NAME='{}'
STAMP="$(date +%s)"
if [ -f /opt/rkn/config.json ]; then
  cp /opt/rkn/config.json "/opt/rkn/config.json.bak.${{STAMP}}"
fi
if [ -f /opt/rkn/bootstrap.json ]; then
  cp /opt/rkn/bootstrap.json "/opt/rkn/bootstrap.json.bak.${{STAMP}}"
fi
cat << 'CONFIGEOF' > /opt/rkn/config.candidate.json
{}
CONFIGEOF

cat << 'BOOTSTRAPEOF' > /opt/rkn/bootstrap.candidate.json
{}
BOOTSTRAPEOF

{}
"#,
        execution.sing_box_image,
        execution.container_name,
        execution.server_cfg,
        execution.bootstrap_cfg,
        deploy_script
    )
    .replace("\r\n", "\n")
    .replace('\r', "\n");

    emit_ssh_stage(
        app,
        "UPLOAD",
        "Uploading generated config and deploy script...",
    );
    let mut channel = sess.channel_session().map_err(|e| e.to_string())?;
    emit_ssh_stage(app, "DEPLOY", "Executing remote fast-deploy script...");
    channel.exec("bash -s 2>&1").map_err(|e| e.to_string())?;

    channel
        .write_all(injected_script.as_bytes())
        .map_err(|e| e.to_string())?;
    channel.send_eof().map_err(|e| e.to_string())?;
    stream_remote_deploy_output(app, sess, &mut channel)?;

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
        app,
        "VALIDATE",
        format!(
            "Remote deploy finished. Validating container {} and ports...",
            execution.container_name
        ),
    );
    validate_remote_runtime(
        sess,
        execution.container_name,
        execution.external_port,
        execution.vless_external_port,
        execution.internal_ss_port,
    )?;

    Ok(())
}

pub async fn deploy_server(
    app: AppHandle,
    host: String,
    user: String,
    pass: String,
) -> Result<TransportStateSnapshot, String> {
    ensure_master_role(&app, "deploy this server")?;

    let local_data = app
        .path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());
    let local_client_config_present = local_data.join("client_config.json").exists();
    let saved_profile = SavedServerProfile {
        host: host.clone(),
        user: user.clone(),
        password: pass.clone(),
    };
    let attach_saved_profile = saved_profile.clone();

    let attach_result = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        let host = host.clone();
        let user = user.clone();
        let pass = pass.clone();
        let local_data = local_data.clone();
        move || -> Result<Option<super::RemoteTransportBootstrap>, String> {
            if let Ok(Some(existing_profile)) = super::load_saved_server_profile(app.clone()) {
                if existing_profile.host != host {
                    let _ = app.emit(
                        "tunnel-log",
                        format!(
                            "[WARN] Deploy target switched from saved server {} to {}. This deploy will update the local active profile for this device only.",
                            existing_profile.host, host
                        ),
                    );
                }
            }
            let _mutation_guard = acquire_remote_mutation_lock()?;
            let _ = app.emit(
                "tunnel-log",
                format!("--- [SSH] Connecting to {} (ports 22/2222) ---", host),
            );

            let sess = connect_ssh_session(&app, &host, &user, &pass)?;
            emit_ssh_stage(&app, "AUTH", "Authenticated successfully.");

            emit_ssh_stage(
                &app,
                "PREFLIGHT",
                "Checking whether this server already has an active RKN transport...",
            );

            let remote_bootstrap = load_remote_transport_bootstrap(&sess)?;
            let Some(remote_bootstrap) = remote_bootstrap else {
                let _ = app.emit(
                    "tunnel-log",
                    "[SSH] No existing transport bootstrap found on remote server. Proceeding with a fresh deploy.".to_string(),
                );
                return Ok(None);
            };

            let Some(container_name) = load_remote_container_name(&sess)? else {
                let _ = app.emit(
                    "tunnel-log",
                    "[SSH] Existing config was found, but no active RKN container is currently available. Proceeding with a fresh deploy.".to_string(),
                );
                return Ok(None);
            };

            let _ = app.emit(
                "tunnel-log",
                "[SSH] Existing RKN transport detected on this server. Reusing it instead of rotating transport credentials.".to_string(),
            );
            let local_message = if local_client_config_present {
                "[SSH] Local client config already exists on this device. Refreshing it from the active remote transport."
            } else {
                "[SSH] Attach to existing server mode is active for this device."
            };
            let _ = app.emit("tunnel-log", local_message.to_string());
            let _ = app.emit(
                "tunnel-log",
                format!(
                    "[SSH] Reusing remote container {} and external port {} without reinstalling the server stack.",
                    container_name, remote_bootstrap.external_port
                ),
            );
            let _ = app.emit(
                "tunnel-log",
                format!(
                    "[SSH] Active remote cover domain: {}",
                    remote_bootstrap.cover_domain
                ),
            );

            let runtime_uses_warp = remote_runtime_uses_warp(&sess)?;
            let effective_routing_mode = if remote_bootstrap.routing_mode == "warp" || runtime_uses_warp {
                "warp"
            } else {
                "direct"
            };
            let has_local_warp_profile = has_local_warp_profile_sync(&app)?;
            let has_remote_warp_profile = load_remote_warp_config(&sess)?.is_some();
            if effective_routing_mode == "direct"
                && (has_local_warp_profile || has_remote_warp_profile)
            {
                let reason = if has_remote_warp_profile {
                    "a remote WARP profile already exists"
                } else {
                    "a local WARP profile is present"
                };
                let _ = app.emit(
                    "tunnel-log",
                    format!(
                        "[SSH:WARP] Existing runtime is direct, but {}. Falling back to a repair deploy so the server config is rebuilt with WARP egress.",
                        reason
                    ),
                );
                return Ok(None);
            }
            let expected_image = pinned_sing_box_image_for_routing_mode(effective_routing_mode);

            if let Some(remote_image) = load_remote_container_image(&sess, &container_name)? {
                if remote_image != expected_image {
                    let _ = app.emit(
                        "tunnel-log",
                        format!(
                            "[SSH WARN] Existing RKN runtime uses server image {} but this build pins {}. Falling back to a fresh deploy to migrate the server runtime.",
                            remote_image, expected_image
                        ),
                    );
                    return Ok(None);
                }
            }

            if remote_bootstrap.routing_mode != effective_routing_mode {
                let _ = app.emit(
                    "tunnel-log",
                    format!(
                        "[SYSTEM] Existing RKN runtime is currently using {} routing. Preserving that server mode for this device instead of forcing a transport migration.",
                        effective_routing_mode
                    ),
                );
            }

            if remote_bootstrap.vless_external_port == 0 || remote_bootstrap.vless_uuid.is_empty() {
                let _ = app.emit(
                    "tunnel-log",
                    "[SSH] Existing RKN runtime does not include VLESS yet. Falling back to Deploy/Update so both ShadowTLS and VLESS are provisioned."
                        .to_string(),
                );
                return Ok(None);
            }

            if let Err(error) =
                validate_remote_runtime(
                    &sess,
                    &container_name,
                    remote_bootstrap.external_port,
                    remote_bootstrap.vless_external_port,
                    remote_bootstrap.internal_ss_port,
                )
            {
                let summary = summarize_runtime_validation_error(&error);
                let _ = app.emit(
                    "tunnel-log",
                    format!(
                        "[SSH WARN] Existing transport metadata was found, but the active runtime is unhealthy. Falling back to a fresh deploy. {}.",
                        summary
                    ),
                );
                return Ok(None);
            }

            if let Err(error) = verify_external_port_reachable(&host, remote_bootstrap.external_port)
            {
                let _ = app.emit(
                    "tunnel-log",
                    format!(
                        "[SSH WARN] Existing transport metadata was found, but the external port is not reachable from this client. Falling back to a fresh deploy. Details: {}",
                        error
                    ),
                );
                return Ok(None);
            }
            if let Err(error) =
                verify_external_port_reachable(&host, remote_bootstrap.vless_external_port)
            {
                let _ = app.emit(
                    "tunnel-log",
                    format!(
                        "[SSH WARN] Existing VLESS metadata was found, but the VLESS port is not reachable from this client. Falling back to a fresh deploy. Details: {}",
                        error
                    ),
                );
                return Ok(None);
            }

            let effective_bootstrap = super::RemoteTransportBootstrap {
                routing_mode: effective_routing_mode.to_string(),
                ..remote_bootstrap.clone()
            };
            let local_rule_sets = ensure_local_client_rule_sets_sync(&app)?;
            let client_cfg =
                crate::generator::build_client_config(crate::generator::ClientConfigParams {
                    server_ip: &host,
                    shadow_pass: &effective_bootstrap.shadow_pass,
                    ss_password: &effective_bootstrap.ss_password,
                    vless_uuid: &effective_bootstrap.vless_uuid,
                    external_port: effective_bootstrap.external_port,
                    vless_external_port: effective_bootstrap.vless_external_port,
                    cover_domain: &effective_bootstrap.cover_domain,
                    local_rule_sets: &local_rule_sets,
                });

            emit_ssh_stage(
                &app,
                "VALIDATE",
                "Checking generated client config with bundled sing-box before saving it...",
            );
            validate_local_client_config(&app, &local_data, &client_cfg)?;

            emit_ssh_stage(
                &app,
                "VALIDATE",
                "Running local ShadowTLS/VLESS transport smoke-check before accepting the existing server...",
            );
            if let Err(error) = run_client_transport_smoke_check(&app, &local_data, &client_cfg) {
                let _ = app.emit(
                    "tunnel-log",
                    format!(
                        "[SSH] Advisory transport smoke-check for the existing server failed, but runtime and port validation passed. Saving the refreshed client config anyway. Details: {}",
                        error
                    ),
                );
            }

            std::fs::create_dir_all(&local_data).map_err(|e| e.to_string())?;
            let client_cfg_path = local_data.join("client_config.json");
            backup_local_file_if_exists(&client_cfg_path)?;
            backup_local_file_if_exists(&super::server_profile_path(&app)?)?;
            std::fs::write(&client_cfg_path, &client_cfg).map_err(|e| e.to_string())?;
            save_server_profile(&app, &attach_saved_profile)?;
            save_backend_app_role(&app, BackendAppRole::Master)?;
            let _ = save_cached_transport_bootstrap(&app, &effective_bootstrap);
            crate::refresh_tray_toggle_item(&app);

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
                    "[SSH] Attach to existing server completed successfully. External port: {}",
                    remote_bootstrap.external_port
                ),
            );
            let _ = app.emit(
                "tunnel-log",
                "[SYSTEM] Server credentials saved locally for next launch.".to_string(),
            );

            Ok(Some(effective_bootstrap))
        }
    })
    .await
    .unwrap()?;

    if let Some(remote_bootstrap) = attach_result {
        crate::restart_tunnel_if_running(
            &app,
            "Tunnel config changed after attaching to the existing server. Restarting core to apply the updated client config.",
        )
        .await?;
        return Ok(snapshot_for_cover_domain(remote_bootstrap.cover_domain));
    }

    let _ = app.emit(
        "tunnel-log",
        "[SYSTEM] Generating transport credentials via sing-box...".to_string(),
    );

    let short_id = crate::generator::generate_short_id(&app)
        .await
        .map_err(|e| format!("Transport secret error: {}", e))?;

    let _ = app.emit(
        "tunnel-log",
        "[SYSTEM] Preparing the transport stack for this server deploy...".to_string(),
    );

    let deploy_app = app.clone();
    let deploy_snapshot = tauri::async_runtime::spawn_blocking(move || -> Result<TransportStateSnapshot, String> {
        let _maintenance_guard = crate::begin_remote_transport_maintenance(
            &deploy_app,
            "server deploy/update is changing the active RKN container.",
        );
        let _mutation_guard = acquire_remote_mutation_lock()?;
        let _ = deploy_app.emit(
            "tunnel-log",
            format!("--- [SSH] Connecting to {} (ports 22/2222) ---", host),
        );

        let sess = connect_ssh_session(&deploy_app, &host, &user, &pass)?;
        emit_ssh_stage(&deploy_app, "AUTH", "Authenticated successfully.");
        emit_ssh_stage(&deploy_app, "PREFLIGHT", "Running remote pre-flight checks for deploy target...");

        let deploy_target = select_remote_deploy_target(&sess, &short_id)?;
        let external_port = deploy_target.external_port;
        let vless_external_port = deploy_target.vless_external_port;
        let internal_ss_port = deploy_target.internal_ss_port;
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
            let _ = deploy_app.emit("tunnel-log", message);
        } else if external_port == PRIMARY_EXTERNAL_PORT {
            let _ = deploy_app.emit(
                "tunnel-log",
                format!(
                    "[SSH] Preferred external port {} is available on remote host.",
                    PRIMARY_EXTERNAL_PORT
                ),
            );
        } else {
            let _ = deploy_app.emit(
                "tunnel-log",
                format!(
                    "[SSH WARN] Preferred external port {} is busy. Falling back to external port {}.",
                    PRIMARY_EXTERNAL_PORT, external_port
                ),
            );
        }

        let existing_bootstrap = load_remote_transport_bootstrap(&sess)?;
        let has_local_warp_profile = has_local_warp_profile_sync(&deploy_app)?;
        let has_remote_warp_profile = load_remote_warp_config(&sess)?.is_some();
        let (shadow_pass, vless_uuid, ss_server_password, master_ss_user_password, ss_password, routing_mode, cover_domain, fallback_cover_domains, issued_invites) =
            if let Some(existing_bootstrap) = existing_bootstrap.clone() {
                let _ = deploy_app.emit(
                    "tunnel-log",
                    "[SYSTEM] Existing transport bootstrap found on this server. Preserving transport credentials and cover domain for multi-device compatibility while refreshing the runtime.".to_string(),
                );
                let (ss_server_password, master_ss_user_password, master_combined_password) =
                    resolve_master_ss_transport(&deploy_app, &existing_bootstrap)?;
                let routing_mode = if existing_bootstrap.routing_mode == "warp"
                    || has_local_warp_profile
                    || has_remote_warp_profile
                {
                    if existing_bootstrap.routing_mode != "warp" {
                        let _ = deploy_app.emit(
                            "tunnel-log",
                            "[SSH:WARP] WARP profile is present. Migrating this existing server runtime from direct egress to WARP egress.".to_string(),
                        );
                    }
                    "warp".to_string()
                } else {
                    existing_bootstrap.routing_mode.clone()
                };
                let vless_uuid = if existing_bootstrap.vless_uuid.trim().is_empty() {
                    tauri::async_runtime::block_on(crate::generator::generate_vless_uuid(
                        &deploy_app,
                    ))
                    .map_err(|e| format!("VLESS UUID error: {}", e))?
                } else {
                    existing_bootstrap.vless_uuid.clone()
                };
                (
                    existing_bootstrap.shadow_pass.clone(),
                    vless_uuid,
                    ss_server_password,
                    master_ss_user_password,
                    master_combined_password,
                    routing_mode,
                    existing_bootstrap.cover_domain.clone(),
                    existing_bootstrap.fallback_cover_domains.clone(),
                    existing_bootstrap.issued_invites.clone(),
                )
            } else {
                let default_routing_mode = if has_local_warp_profile || has_remote_warp_profile {
                    "warp"
                } else {
                    "direct"
                };
                let _ = deploy_app.emit(
                    "tunnel-log",
                    format!(
                        "[SYSTEM] No existing transport bootstrap found. Defaulting this fresh server deploy to {} egress for this device.",
                        default_routing_mode
                    ),
                );
                let shadow_pass = tauri::async_runtime::block_on(
                    crate::generator::generate_shadowtls_password(&deploy_app),
                )
                    .map_err(|e| format!("ShadowTLS password error: {}", e))?;
                let ss_server_password = tauri::async_runtime::block_on(
                    crate::generator::generate_ss_password(&deploy_app),
                )
                    .map_err(|e| format!("Shadowsocks server password error: {}", e))?;
                let ss_user_password = tauri::async_runtime::block_on(
                    crate::generator::generate_ss_password(&deploy_app),
                )
                    .map_err(|e| format!("Shadowsocks user password error: {}", e))?;
                let ss_password = compose_multi_user_ss_password(&ss_server_password, &ss_user_password);
                let vless_uuid = tauri::async_runtime::block_on(
                    crate::generator::generate_vless_uuid(&deploy_app),
                )
                    .map_err(|e| format!("VLESS UUID error: {}", e))?;
                (
                    shadow_pass,
                    vless_uuid,
                    ss_server_password,
                    ss_user_password,
                    ss_password,
                    default_routing_mode.to_string(),
                    crate::generator::select_cover_domain(&short_id).to_string(),
                    Vec::new(),
                    Vec::new(),
                )
            };
        let sing_box_image = pinned_sing_box_image_for_routing_mode(&routing_mode);
        let _ = deploy_app.emit(
            "tunnel-log",
            format!(
                "[SSH] Selected pinned image {} and container name {}.",
                sing_box_image, container_name
            ),
        );
        let _ = deploy_app.emit(
            "tunnel-log",
            format!(
                "[SYSTEM] Transport stack: ShadowTLS v3 + Shadowsocks-2022. ShadowTLS secret length: {} chars.",
                shadow_pass.len()
            ),
        );
        let _ = deploy_app.emit(
            "tunnel-log",
            format!(
                "[SYSTEM] VLESS fallback transport will listen on external port {}.",
                vless_external_port
            ),
        );
        let _ = deploy_app.emit(
            "tunnel-log",
            format!("[SSH] ShadowTLS cover domain: {}", cover_domain),
        );
        let warp_config = if routing_mode == "warp" {
            Some(ensure_remote_warp_config(&deploy_app, &sess)?)
        } else {
            let _ = deploy_app.emit(
                "tunnel-log",
                "[SSH] Existing server runtime is configured for direct egress. Preserving that mode and skipping WARP provisioning.".to_string(),
            );
            None
        };
        let local_rule_sets = ensure_local_client_rule_sets_sync(&deploy_app)?;
        let server_cfg = crate::generator::build_server_config_with_invites(
            crate::generator::ServerConfigParams {
                master_shadow_pass: &shadow_pass,
                master_vless_uuid: &vless_uuid,
                ss_server_password: &ss_server_password,
                master_ss_user_password: &master_ss_user_password,
                external_port,
                vless_external_port,
                internal_ss_port,
                routing_mode: &routing_mode,
                cover_domain: &cover_domain,
                fallback_cover_domains: &fallback_cover_domains,
                issued_invites: &issued_invites,
                warp: warp_config.as_ref(),
            },
        );
        let client_cfg =
            crate::generator::build_client_config(crate::generator::ClientConfigParams {
                server_ip: &host,
                shadow_pass: &shadow_pass,
                ss_password: &ss_password,
                vless_uuid: &vless_uuid,
                external_port,
                vless_external_port,
                cover_domain: &cover_domain,
                local_rule_sets: &local_rule_sets,
            });
        let bootstrap_cfg = json!({
            "external_port": external_port,
            "vless_external_port": vless_external_port,
            "internal_ss_port": internal_ss_port,
            "routing_mode": routing_mode,
            "cover_domain": cover_domain,
            "fallback_cover_domains": fallback_cover_domains,
            "shadow_pass": shadow_pass,
            "ss_password": ss_password,
            "vless_uuid": vless_uuid,
            "ss_server_password": ss_server_password,
            "issued_invites": issued_invites
        })
        .to_string();

        emit_ssh_stage(
            &deploy_app,
            "DEPLOY",
            format!(
                "Deploying ShadowTLS transport on external port {} with cover domain {}...",
                external_port, cover_domain
            ),
        );
        execute_remote_deploy(
            &sess,
            &deploy_app,
            &RemoteDeployExecution {
                container_name: &container_name,
                external_port,
                vless_external_port,
                internal_ss_port,
                sing_box_image,
                server_cfg: &server_cfg,
                bootstrap_cfg: &bootstrap_cfg,
            },
        )?;

        emit_ssh_stage(
            &deploy_app,
            "VALIDATE",
            format!(
                "Remote runtime looks healthy. Verifying external port {} from this client...",
                external_port
            ),
        );
        verify_external_port_reachable(&host, external_port)?;
        verify_external_port_reachable(&host, vless_external_port)?;

        emit_ssh_stage(
            &deploy_app,
            "VALIDATE",
            "Checking generated client config with bundled sing-box before saving it...",
        );
        validate_local_client_config(&deploy_app, &local_data, &client_cfg)?;

        emit_ssh_stage(
            &deploy_app,
            "VALIDATE",
            "Running local ShadowTLS/VLESS transport smoke-check through the generated transport outbounds...",
        );
        if let Err(error) = run_client_transport_smoke_check(&deploy_app, &local_data, &client_cfg) {
            let _ = deploy_app.emit(
                "tunnel-log",
                format!(
                    "[SSH] Advisory transport smoke-check failed, but the remote runtime is healthy and the client config will still be saved. Start the tunnel to verify the live path. Details: {}",
                    error
                ),
            );
        } else {
            emit_ssh_stage(
                &deploy_app,
                "VALIDATE",
                "Client transport smoke-check passed for all provisioned transport outbounds. Handshake is accepted.",
            );
        }

        std::fs::create_dir_all(&local_data).map_err(|e| e.to_string())?;
        let client_cfg_path = local_data.join("client_config.json");
        backup_local_file_if_exists(&client_cfg_path)?;
        backup_local_file_if_exists(&super::server_profile_path(&deploy_app)?)?;
        std::fs::write(&client_cfg_path, &client_cfg).map_err(|e| e.to_string())?;
        save_server_profile(&deploy_app, &saved_profile)?;
        save_backend_app_role(&deploy_app, BackendAppRole::Master)?;
        let fresh_bootstrap = RemoteTransportBootstrap {
            external_port,
            vless_external_port,
            internal_ss_port,
            routing_mode,
            cover_domain: cover_domain.to_string(),
            fallback_cover_domains,
            shadow_pass: shadow_pass.clone(),
            ss_password: ss_password.clone(),
            vless_uuid: vless_uuid.clone(),
            ss_server_password: ss_server_password.clone(),
            issued_invites,
        };
        let _ = save_cached_transport_bootstrap(&deploy_app, &fresh_bootstrap);
        crate::refresh_tray_toggle_item(&deploy_app);

        let _ = deploy_app.emit(
            "tunnel-log",
            format!(
                "[SYSTEM] Client config safely generated at: {:?}",
                client_cfg_path
            ),
        );
        let _ = deploy_app.emit(
            "tunnel-log",
            format!(
                "[SSH] Deployment finished successfully! External port: {}",
                external_port
            ),
        );
        let _ = deploy_app.emit(
            "tunnel-log",
            "[SYSTEM] Server credentials saved locally for next launch.".to_string(),
        );
        Ok(snapshot_for_cover_domain(cover_domain))
    })
    .await
    .unwrap()?;

    crate::restart_tunnel_if_running(
        &app,
        "Tunnel config changed after deploy. Restarting core to apply the updated client config.",
    )
    .await?;

    Ok(deploy_snapshot)
}
