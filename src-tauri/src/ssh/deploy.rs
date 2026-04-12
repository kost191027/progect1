use serde_json::json;
use ssh2::Session;
use std::io::Write;
use tauri::{AppHandle, Emitter, Manager};

use super::warp::ensure_remote_warp_config;
use super::{
    acquire_remote_mutation_lock, build_container_name, connect_ssh_session, emit_ssh_stage,
    ensure_local_client_rule_sets_sync, load_remote_container_image, load_remote_container_name,
    load_remote_transport_bootstrap, remote_runtime_uses_warp, run_remote_command,
    save_server_profile, snapshot_for_cover_domain, stream_remote_deploy_output,
    RemoteDeployTarget, SavedServerProfile, TransportStateSnapshot, EXTERNAL_PORT_CANDIDATES,
    INTERNAL_SS_PORT_CANDIDATES, LEGACY_CONTAINER_NAME, PINNED_SING_BOX_IMAGE,
    PRIMARY_EXTERNAL_PORT,
};

fn compose_multi_user_ss_password(server_password: &str, user_password: &str) -> String {
    format!("{}:{}", server_password, user_password)
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
    let internal_candidates = INTERNAL_SS_PORT_CANDIDATES
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
SELECTED_PORT=""
INTERNAL_PORT=""

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
      SELECTED_PORT="$PRIMARY_PORT"
      echo "port=$PRIMARY_PORT"
      echo "container=$PREVIOUS_CONTAINER"
      echo "reuse=true"
      echo "migrate_primary=true"
    else
      for candidate in {candidates}; do
      if [ "$candidate" = "$CURRENT_PORT" ]; then
        SELECTED_PORT="$CURRENT_PORT"
        echo "port=$CURRENT_PORT"
        echo "container=$PREVIOUS_CONTAINER"
        echo "reuse=true"
        echo "migrate_primary=false"
        break
      fi
      done
    fi
  fi
fi

if [ -z "$SELECTED_PORT" ]; then
  for port in {candidates}; do
    if ! ss -Htanl "( sport = :$port )" | grep -q .; then
      SELECTED_PORT="$port"
      echo "port=$port"
      echo "container={generated_container_name}"
      echo "reuse=false"
      echo "migrate_primary=false"
      break
    fi
  done
fi

for port in {internal_candidates}; do
  if ! ss -Htanl "( sport = :$port )" | grep -q .; then
    INTERNAL_PORT="$port"
    break
  fi
done

if [ -z "$SELECTED_PORT" ] || [ -z "$INTERNAL_PORT" ]; then
  exit 1
fi

echo "internal_port=$INTERNAL_PORT"
exit 0
'"#,
        legacy_container_name = LEGACY_CONTAINER_NAME,
        candidates = candidates,
        internal_candidates = internal_candidates,
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
    let mut selected_internal_port = None;
    let mut selected_container_name = None;
    let mut reusing_existing_instance = false;
    let mut migrating_to_primary_port = false;

    for line in stdout.lines() {
        if let Some(value) = line.trim().strip_prefix("port=") {
            selected_port = value.trim().parse::<u16>().ok();
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
    let internal_ss_port = selected_internal_port.ok_or_else(|| {
        format!(
            "Failed to parse remote selected internal SS port from output: {}",
            stdout
        )
    })?;
    let container_name = selected_container_name.unwrap_or_else(|| build_container_name(short_id));

    Ok(RemoteDeployTarget {
        external_port,
        internal_ss_port,
        container_name,
        reusing_existing_instance,
        migrating_to_primary_port,
    })
}

pub(crate) fn validate_remote_runtime(
    sess: &Session,
    container_name: &str,
    external_port: u16,
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

echo "$socket_dump" | grep -Eq ":{internal_ss_port}\b" || {{
  echo "[error] internal Shadowsocks port {internal_ss_port} is not listening"
  echo "[docker_logs]"
  docker logs --tail 40 "{container_name}" 2>&1 || true
  exit 1
}}
'"#,
        container_name = container_name,
        external_port = external_port,
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

pub async fn deploy_server(
    app: AppHandle,
    host: String,
    user: String,
    pass: String,
) -> Result<TransportStateSnapshot, String> {
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
            let _mutation_guard = acquire_remote_mutation_lock()?;
            let _ = app.emit(
                "tunnel-log",
                format!("--- [SSH] Connecting to {}:22 ---", host),
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

            if let Some(remote_image) = load_remote_container_image(&sess, &container_name)? {
                if remote_image != PINNED_SING_BOX_IMAGE {
                    let _ = app.emit(
                        "tunnel-log",
                        format!(
                            "[SSH WARN] Existing RKN runtime uses server image {} but this build pins {}. Falling back to a fresh deploy to migrate the server runtime.",
                            remote_image, PINNED_SING_BOX_IMAGE
                        ),
                    );
                    return Ok(None);
                }
            }

            if !remote_runtime_uses_warp(&sess)? {
                let _ = app.emit(
                    "tunnel-log",
                    "[SYSTEM] Existing RKN runtime still uses the previous server routing. Refreshing it now so this server matches the current WARP-backed transport.".to_string(),
                );
                return Ok(None);
            }

            if let Err(error) =
                validate_remote_runtime(
                    &sess,
                    &container_name,
                    remote_bootstrap.external_port,
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

            let local_rule_sets = ensure_local_client_rule_sets_sync(&app)?;
            let client_cfg = crate::generator::build_client_config(
                &host,
                &remote_bootstrap.shadow_pass,
                &remote_bootstrap.ss_password,
                remote_bootstrap.external_port,
                &remote_bootstrap.cover_domain,
                &local_rule_sets,
            );

            std::fs::create_dir_all(&local_data).map_err(|e| e.to_string())?;
            let client_cfg_path = local_data.join("client_config.json");
            std::fs::write(&client_cfg_path, &client_cfg).map_err(|e| e.to_string())?;
            save_server_profile(&app, &attach_saved_profile)?;
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

            Ok(Some(remote_bootstrap))
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
    let shadow_pass = crate::generator::generate_shadowtls_password(&app)
        .await
        .map_err(|e| format!("ShadowTLS password error: {}", e))?;
    let ss_server_password = crate::generator::generate_ss_password(&app)
        .await
        .map_err(|e| format!("Shadowsocks server password error: {}", e))?;
    let ss_user_password = crate::generator::generate_ss_password(&app)
        .await
        .map_err(|e| format!("Shadowsocks user password error: {}", e))?;
    let ss_password = compose_multi_user_ss_password(&ss_server_password, &ss_user_password);

    let _ = app.emit(
        "tunnel-log",
        format!(
            "[SYSTEM] Transport stack: ShadowTLS v3 + Shadowsocks-2022. ShadowTLS secret length: {} chars.",
            shadow_pass.len()
        ),
    );

    let deploy_app = app.clone();
    let deploy_snapshot = tauri::async_runtime::spawn_blocking(move || {
        let _mutation_guard = acquire_remote_mutation_lock()?;
        let _ = deploy_app.emit(
            "tunnel-log",
            format!("--- [SSH] Connecting to {}:22 ---", host),
        );

        let sess = connect_ssh_session(&deploy_app, &host, &user, &pass)?;
        emit_ssh_stage(&deploy_app, "AUTH", "Authenticated successfully.");
        emit_ssh_stage(&deploy_app, "PREFLIGHT", "Running remote pre-flight checks for deploy target...");

        let deploy_target = select_remote_deploy_target(&sess, &short_id)?;
        let external_port = deploy_target.external_port;
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

        let _ = deploy_app.emit(
            "tunnel-log",
            format!(
                "[SSH] Selected pinned image {} and container name {}.",
                PINNED_SING_BOX_IMAGE, container_name
            ),
        );

        let deploy_script = include_str!("../../scripts/deploy.sh");
        let cover_domain = crate::generator::select_cover_domain(&short_id);
        let _ = deploy_app.emit(
            "tunnel-log",
            format!("[SSH] ShadowTLS cover domain: {}", cover_domain),
        );
        let warp_config = ensure_remote_warp_config(&deploy_app, &sess)?;
        let server_cfg = crate::generator::build_server_config_with_invites(
            crate::generator::ServerConfigParams {
                master_shadow_pass: &shadow_pass,
                ss_server_password: &ss_server_password,
                master_ss_user_password: &ss_user_password,
                external_port,
                internal_ss_port,
                cover_domain,
                fallback_cover_domains: &[],
                issued_invites: &[],
                warp: &warp_config,
            },
        );
        let local_rule_sets = ensure_local_client_rule_sets_sync(&deploy_app)?;
        let client_cfg = crate::generator::build_client_config(
            &host,
            &shadow_pass,
            &ss_password,
            external_port,
            cover_domain,
            &local_rule_sets,
        );
        let bootstrap_cfg = json!({
            "external_port": external_port,
            "internal_ss_port": internal_ss_port,
            "cover_domain": cover_domain,
            "fallback_cover_domains": [],
            "shadow_pass": shadow_pass,
            "ss_password": ss_password,
            "ss_server_password": ss_server_password,
            "issued_invites": []
        })
        .to_string();

        emit_ssh_stage(
            &deploy_app,
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

cat << 'BOOTSTRAPEOF' > /opt/rkn/bootstrap.candidate.json
{}
BOOTSTRAPEOF

{}
"#,
            PINNED_SING_BOX_IMAGE, container_name, server_cfg, bootstrap_cfg, deploy_script
        );

        emit_ssh_stage(&deploy_app, "UPLOAD", "Uploading generated config and deploy script...");
        let mut channel = sess.channel_session().map_err(|e| e.to_string())?;
        emit_ssh_stage(&deploy_app, "DEPLOY", "Executing remote fast-deploy script...");
        channel.exec("bash -s 2>&1").map_err(|e| e.to_string())?;

        channel
            .write_all(injected_script.as_bytes())
            .map_err(|e| e.to_string())?;
        channel.send_eof().map_err(|e| e.to_string())?;
        stream_remote_deploy_output(&deploy_app, &sess, &mut channel)?;

        channel
            .wait_close()
            .map_err(|e| format!("Failed to wait for remote deploy close: {}", e))?;
        let exit_status = channel
            .exit_status()
            .map_err(|e| format!("Failed to read remote deploy exit status: {}", e))?;

        if exit_status != 0 {
            let _ = deploy_app.emit(
                "tunnel-log",
                format!("[SSH ERROR] Deployment failed with code: {}", exit_status),
            );
            return Err(format!("Deployment script exited with {}", exit_status));
        }

        emit_ssh_stage(
            &deploy_app,
            "VALIDATE",
            format!(
                "Remote deploy finished. Validating container {} and ports...",
                container_name
            ),
        );
        validate_remote_runtime(&sess, &container_name, external_port, internal_ss_port)?;

        emit_ssh_stage(
            &deploy_app,
            "VALIDATE",
            format!(
                "Remote runtime looks healthy. Verifying external port {} from this client...",
                external_port
            ),
        );
        verify_external_port_reachable(&host, external_port)?;

        emit_ssh_stage(
            &deploy_app,
            "VALIDATE",
            format!("External port {} is reachable from this client.", external_port),
        );

        std::fs::create_dir_all(&local_data).map_err(|e| e.to_string())?;
        let client_cfg_path = local_data.join("client_config.json");
        std::fs::write(&client_cfg_path, &client_cfg).map_err(|e| e.to_string())?;
        save_server_profile(&deploy_app, &saved_profile)?;
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
