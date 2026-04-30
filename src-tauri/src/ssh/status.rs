use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use super::deploy::{execute_remote_deploy, RemoteDeployExecution};
use super::invite::resolve_master_ss_transport;
use super::warp::{ensure_remote_warp_config, load_saved_server_profile};
use super::{
    acquire_remote_mutation_lock, build_rotated_cover_domain_history, connect_ssh_session,
    connect_ssh_session_quiet, emit_ssh_stage, ensure_local_client_rule_sets_sync,
    ensure_master_role, load_local_client_transport_state, load_remote_container_name,
    load_remote_transport_bootstrap, monitored_port_pattern, run_remote_command,
    save_cached_transport_bootstrap, LocalClientTransportState, RemoteTransportBootstrap,
    TransportStateSnapshot,
};

fn local_transport_requires_redeploy(
    local_state: &LocalClientTransportState,
    remote_bootstrap: &RemoteTransportBootstrap,
) -> bool {
    crate::generator::is_legacy_cover_domain_requiring_refresh(&local_state.cover_domain)
        || crate::generator::is_legacy_cover_domain_requiring_refresh(
            &remote_bootstrap.cover_domain,
        )
        || local_state.cover_domain != remote_bootstrap.cover_domain
        || local_state.shadow_pass != remote_bootstrap.shadow_pass
        || local_state.ss_password != remote_bootstrap.ss_password
}

fn load_transport_state_snapshot_sync(
    app: &AppHandle,
    emit_ssh_logs: bool,
) -> Result<TransportStateSnapshot, String> {
    let available_cover_domains = crate::generator::available_cover_domains();
    let local_state = load_local_client_transport_state(app)?;

    let Some(profile) = load_saved_server_profile(app.clone())? else {
        return Ok(TransportStateSnapshot {
            current_cover_domain: None,
            available_cover_domains,
            local_cover_domain: local_state.map(|state| state.cover_domain),
            requires_redeploy: false,
        });
    };

    let sess = if emit_ssh_logs {
        connect_ssh_session(app, &profile.host, &profile.user, &profile.password)?
    } else {
        connect_ssh_session_quiet(app, &profile.host, &profile.user, &profile.password)?
    };
    let remote_bootstrap = load_remote_transport_bootstrap(&sess)?;

    let Some(remote_bootstrap) = remote_bootstrap else {
        return Ok(TransportStateSnapshot {
            current_cover_domain: None,
            available_cover_domains,
            local_cover_domain: local_state.map(|state| state.cover_domain),
            requires_redeploy: false,
        });
    };

    let requires_redeploy = local_state
        .as_ref()
        .map(|state| local_transport_requires_redeploy(state, &remote_bootstrap))
        .unwrap_or(false);

    Ok(TransportStateSnapshot {
        current_cover_domain: Some(remote_bootstrap.cover_domain),
        available_cover_domains,
        local_cover_domain: local_state.map(|state| state.cover_domain),
        requires_redeploy,
    })
}

#[cfg(target_os = "android")]
pub(crate) async fn ensure_local_transport_is_current(app: &AppHandle) -> Result<(), String> {
    let check_app = app.clone();
    let snapshot_result = tauri::async_runtime::spawn_blocking(move || {
        load_transport_state_snapshot_sync(&check_app, true)
    })
    .await
    .unwrap();

    ensure_transport_snapshot_current(app, snapshot_result, true)
}

pub(crate) async fn ensure_local_transport_is_current_quiet(app: &AppHandle) -> Result<(), String> {
    let check_app = app.clone();
    let snapshot_result = tauri::async_runtime::spawn_blocking(move || {
        load_transport_state_snapshot_sync(&check_app, false)
    })
    .await
    .unwrap();

    ensure_transport_snapshot_current(app, snapshot_result, false)
}

fn ensure_transport_snapshot_current(
    app: &AppHandle,
    snapshot_result: Result<TransportStateSnapshot, String>,
    emit_warning: bool,
) -> Result<(), String> {
    let snapshot = match snapshot_result {
        Ok(snapshot) => snapshot,
        Err(error) => {
            if emit_warning {
                let _ = app.emit(
                    "tunnel-log",
                    format!(
                        "[WARN] Unable to verify whether this device is in sync with the remote transport: {}",
                        error
                    ),
                );
            }
            return Ok(());
        }
    };

    if !snapshot.requires_redeploy {
        return Ok(());
    }

    let remote_cover_domain = snapshot
        .current_cover_domain
        .unwrap_or_else(|| "unknown".to_string());
    let local_cover_domain = snapshot
        .local_cover_domain
        .unwrap_or_else(|| "unknown".to_string());

    if crate::generator::is_legacy_cover_domain_requiring_refresh(&remote_cover_domain)
        || crate::generator::is_legacy_cover_domain_requiring_refresh(&local_cover_domain)
    {
        let _ = app.emit(
            "tunnel-log",
            format!(
                "[SYSTEM] This device still uses the legacy cover domain {}. Run Deploy/Update once to migrate the transport to a currently supported domain before starting the tunnel.",
                if crate::generator::is_legacy_cover_domain_requiring_refresh(&remote_cover_domain) {
                    remote_cover_domain.clone()
                } else {
                    local_cover_domain.clone()
                }
            ),
        );

        return Err(
            "The current cover domain is no longer supported. Run Deploy/Update before starting the tunnel."
                .to_string(),
        );
    }

    let _ = app.emit(
        "tunnel-log",
        format!(
            "[SYSTEM] Remote cover domain is {} but this device still has {}. Run Deploy/Update on this device before starting the tunnel.",
            remote_cover_domain, local_cover_domain
        ),
    );

    Err(
        "Remote transport changed on another client. Run Deploy/Update on this device before starting the tunnel."
            .to_string(),
    )
}

fn summarize_server_status_output(stdout: &str) -> Vec<String> {
    let mut summary = Vec::new();
    let running = stdout
        .lines()
        .find(|line| line.trim_start().starts_with("running="))
        .map(|line| line.contains("running=true"))
        .unwrap_or(false);
    let warp_enabled = stdout.contains(r#""tag": "warp""#)
        && (stdout.contains(r#""final": "warp""#) || stdout.contains(r#""outbound": "warp""#));
    let fatal_count = stdout.matches("FATAL").count();
    let hmac_mismatch_count = stdout
        .matches("client hello verify failed: hmac mismatch")
        .count();
    let unexpected_session_count = stdout
        .matches("client hello verify failed: unexpected session id length")
        .count();
    let unexpected_eof_count = stdout
        .matches("read client handshake: unexpected EOF")
        .count();
    let handshake_response_count = stdout.matches("received handshake response").count();
    let wireguard_retry_count = stdout
        .matches("retrying handshake because we stopped hearing back after 15 seconds")
        .count();
    let known_service_matches = [
        ("docker sing-box", stdout.matches("sing-box").count()),
        ("xray", stdout.matches("xray").count()),
        ("nginx", stdout.matches("nginx").count()),
        ("tailscaled", stdout.matches("tailscaled").count()),
        ("hysteria", stdout.matches("hysteria").count()),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .map(|(name, _)| name)
    .collect::<Vec<_>>();

    if running {
        summary.push("Runtime health: container is running.".to_string());
    } else {
        summary.push(
            "Runtime health: container health could not be confirmed from this snapshot."
                .to_string(),
        );
    }

    if warp_enabled {
        summary.push("WARP routing: enabled in the active server config.".to_string());
    } else {
        summary.push("WARP routing: not detected in the active server config.".to_string());
    }

    if handshake_response_count > 0 {
        summary.push(format!(
            "WARP peer health: received {} successful WireGuard handshake response(s) in the recent log window.",
            handshake_response_count
        ));
    }

    if wireguard_retry_count > 0 && handshake_response_count > 0 {
        summary.push(
            "WARP peer keepalive: retries are present, but the peer still answers. This is usually acceptable while traffic is flowing."
                .to_string(),
        );
    }

    if fatal_count > 0 {
        summary.push(format!(
            "Recent server log severity: {} fatal event(s) detected in the current status window.",
            fatal_count
        ));
    }

    let transport_noise_count =
        hmac_mismatch_count + unexpected_session_count + unexpected_eof_count;
    if transport_noise_count > 0 && fatal_count == 0 {
        summary.push(format!(
            "ShadowTLS noise: {} external handshake mismatch / scan event(s) seen recently. These warnings are usually background internet noise unless the client itself is failing.",
            transport_noise_count
        ));
    }

    if !known_service_matches.is_empty() {
        summary.push(format!(
            "Coexistence snapshot: detected other network services alongside RKN: {}.",
            known_service_matches.join(", ")
        ));
    }

    if summary.is_empty() {
        summary.push("Server status summary is unavailable for this response.".to_string());
    }

    summary
}

pub async fn get_transport_state_snapshot(
    app: AppHandle,
) -> Result<TransportStateSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || load_transport_state_snapshot_sync(&app, true))
        .await
        .unwrap()
}

pub async fn check_server_status(app: AppHandle) -> Result<String, String> {
    let profile = load_saved_server_profile(app.clone())?
        .ok_or_else(|| "Saved server profile not found. Deploy once first.".to_string())?;
    let monitored_ports = monitored_port_pattern();

    tauri::async_runtime::spawn_blocking(move || {
        let _ = app.emit(
            "tunnel-log",
            format!("--- [SSH] Checking server status on {}:22 ---", profile.host),
        );

        let sess = connect_ssh_session(&app, &profile.host, &profile.user, &profile.password)?;
        emit_ssh_stage(&app, "STATUS", "Collecting docker runtime diagnostics...");

        let command = format!(
            r#"bash -lc '
CONFIG_DIR="/opt/rkn"
ACTIVE_CONTAINER_FILE="$CONFIG_DIR/container_name"
CONTAINER_NAME="$(cat "$ACTIVE_CONTAINER_FILE" 2>/dev/null || true)"

echo "[STATUS] host=$(hostname)"
echo "[STATUS] container_name=${{CONTAINER_NAME:-<missing>}}"
echo "[STATUS] docker_ps"
docker ps -a --format "table {{.Names}}\t{{.Image}}\t{{.Status}}" | sed -n "1,10p"
echo "[STATUS] coexistence_processes"
ps -eo comm= | grep -E "^(sing-box|xray|nginx|tailscaled|hysteria|wgcf)$" | sort | uniq || true

if [ -n "$CONTAINER_NAME" ] && docker inspect "$CONTAINER_NAME" >/dev/null 2>&1; then
  echo "[STATUS] docker_inspect"
  docker inspect -f "running={{.State.Running}} pid={{.State.Pid}} started={{.State.StartedAt}}" "$CONTAINER_NAME"
  echo "[STATUS] listening_sockets"
  ss -ltnup | grep -E ":({monitored_ports})\b" || true
  echo "[STATUS] active_config"
  cat "$CONFIG_DIR/config.json"
  echo "[STATUS] docker_logs"
  docker logs --tail 120 "$CONTAINER_NAME" 2>&1
else
  echo "[STATUS] active container missing or not inspectable"
  echo "[STATUS] active_config"
  cat "$CONFIG_DIR/config.json" 2>/dev/null || true
fi
'"#,
            monitored_ports = monitored_ports
        );

        let (stdout, exit_status) = run_remote_command(&sess, &command)?;
        for line in summarize_server_status_output(&stdout) {
            let _ = app.emit("tunnel-log", format!("[SYSTEM] {}", line));
        }
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

pub async fn rotate_sni(app: AppHandle, target_domain: Option<String>) -> Result<String, String> {
    ensure_master_role(&app, "rotate the cover domain")?;

    let profile = load_saved_server_profile(app.clone())?
        .ok_or_else(|| "Saved server profile not found. Deploy once first.".to_string())?;

    let _ = app.emit(
        "tunnel-log",
        "[SYSTEM] Rotating ShadowTLS cover domain...".to_string(),
    );

    let local_data = app
        .path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());

    let rotate_app = app.clone();
    let result_domain = tauri::async_runtime::spawn_blocking(move || {
        let _mutation_guard = acquire_remote_mutation_lock()?;
        let sess = connect_ssh_session(
            &rotate_app,
            &profile.host,
            &profile.user,
            &profile.password,
        )?;
        let remote_bootstrap = load_remote_transport_bootstrap(&sess)?.ok_or_else(|| {
            "Remote transport bootstrap not found. Deploy the server first.".to_string()
        })?;
        let container_name = load_remote_container_name(&sess)?.ok_or_else(|| {
            "Remote RKN container is not active. Deploy the server first.".to_string()
        })?;

        let mut occupied_cover_domains = remote_bootstrap.fallback_cover_domains.clone();
        occupied_cover_domains.push(remote_bootstrap.cover_domain.clone());
        let cover_domain = if let Some(target_domain) = target_domain.as_deref() {
            if !crate::generator::is_supported_cover_domain(target_domain) {
                return Err(format!(
                    "Unsupported cover domain selected for rotation: {}",
                    target_domain
                ));
            }

            if target_domain == remote_bootstrap.cover_domain {
                let _ = rotate_app.emit(
                    "tunnel-log",
                    format!(
                        "[SYSTEM] Selected cover domain {} is already active. Nothing to rotate.",
                        target_domain
                    ),
                );
                return Ok(target_domain.to_string());
            }

            target_domain
        } else {
            crate::generator::select_next_cover_domain(
                &remote_bootstrap.cover_domain,
                &occupied_cover_domains,
            )
        };
        let fallback_cover_domains = build_rotated_cover_domain_history(
            &remote_bootstrap.cover_domain,
            &remote_bootstrap.fallback_cover_domains,
            cover_domain,
        );

        let _ = rotate_app.emit(
            "tunnel-log",
            "[SYSTEM] Preserving existing transport passwords for multi-device compatibility."
                .to_string(),
        );
        let _ = rotate_app.emit(
            "tunnel-log",
            format!(
                "[SYSTEM] Current cover domain: {}",
                remote_bootstrap.cover_domain
            ),
        );
        let _ = rotate_app.emit(
            "tunnel-log",
            format!("[SYSTEM] New cover domain: {}", cover_domain),
        );
        if !fallback_cover_domains.is_empty() {
            let _ = rotate_app.emit(
                "tunnel-log",
                format!(
                    "[SYSTEM] Previous cover domains kept on the server for staged rollover: {}",
                    fallback_cover_domains.join(", ")
                ),
            );
        }

        let (ss_server_password, master_ss_user_password, master_combined_password) =
            resolve_master_ss_transport(&rotate_app, &remote_bootstrap)?;
        let warp_config = if remote_bootstrap.routing_mode == "warp" {
            Some(ensure_remote_warp_config(&rotate_app, &sess)?)
        } else {
            let _ = rotate_app.emit(
                "tunnel-log",
                "[SYSTEM] Remote server currently uses direct egress. Preserving that mode during cover-domain rotation.".to_string(),
            );
            None
        };
        let server_cfg = crate::generator::build_server_config_with_invites(
            crate::generator::ServerConfigParams {
                master_shadow_pass: &remote_bootstrap.shadow_pass,
                ss_server_password: &ss_server_password,
                master_ss_user_password: &master_ss_user_password,
                external_port: remote_bootstrap.external_port,
                internal_ss_port: remote_bootstrap.internal_ss_port,
                routing_mode: &remote_bootstrap.routing_mode,
                cover_domain,
                fallback_cover_domains: &fallback_cover_domains,
                issued_invites: &remote_bootstrap.issued_invites,
                warp: warp_config.as_ref(),
            },
        );
        let local_rule_sets = ensure_local_client_rule_sets_sync(&rotate_app)?;
        let client_cfg = crate::generator::build_client_config(
            &profile.host,
            &remote_bootstrap.shadow_pass,
            &master_combined_password,
            remote_bootstrap.external_port,
            cover_domain,
            &local_rule_sets,
        );
        let bootstrap_cfg = json!({
            "external_port": remote_bootstrap.external_port,
            "internal_ss_port": remote_bootstrap.internal_ss_port,
            "routing_mode": remote_bootstrap.routing_mode,
            "cover_domain": cover_domain,
            "fallback_cover_domains": fallback_cover_domains,
            "shadow_pass": remote_bootstrap.shadow_pass,
            "ss_password": master_combined_password,
            "ss_server_password": ss_server_password,
            "issued_invites": remote_bootstrap.issued_invites
        })
        .to_string();

        emit_ssh_stage(
            &rotate_app,
            "ROTATE",
            "Deploying new cover domain to server...",
        );
        execute_remote_deploy(
            &sess,
            &rotate_app,
            &RemoteDeployExecution {
                container_name: &container_name,
                external_port: remote_bootstrap.external_port,
                internal_ss_port: remote_bootstrap.internal_ss_port,
                server_cfg: &server_cfg,
                bootstrap_cfg: &bootstrap_cfg,
            },
        )?;

        std::fs::create_dir_all(&local_data).map_err(|e| e.to_string())?;
        let client_cfg_path = local_data.join("client_config.json");
        std::fs::write(&client_cfg_path, &client_cfg).map_err(|e| e.to_string())?;

        let rotated_bootstrap = super::RemoteTransportBootstrap {
            external_port: remote_bootstrap.external_port,
            internal_ss_port: remote_bootstrap.internal_ss_port,
            routing_mode: remote_bootstrap.routing_mode,
            cover_domain: cover_domain.to_string(),
            fallback_cover_domains,
            shadow_pass: remote_bootstrap.shadow_pass,
            ss_password: master_combined_password,
            ss_server_password,
            issued_invites: remote_bootstrap.issued_invites,
        };
        let _ = save_cached_transport_bootstrap(&rotate_app, &rotated_bootstrap);

        let _ = rotate_app.emit(
            "tunnel-log",
            format!(
                "[SYSTEM] SNI rotated to {}. This device will restart its tunnel automatically; other devices need Deploy/Attach once to refresh their local config.",
                cover_domain
            ),
        );

        Ok(cover_domain.to_string())
    })
    .await
    .unwrap()?;

    crate::restart_tunnel_if_running(
        &app,
        "Tunnel config changed after SNI rotation. Restarting core to apply the updated client config.",
    )
    .await?;

    Ok(result_domain)
}
