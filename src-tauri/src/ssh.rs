use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use tauri::{AppHandle, Emitter, Manager};

const PRIMARY_EXTERNAL_PORT: u16 = 4433;
const EXTERNAL_PORT_CANDIDATES: [u16; 5] = [4433, 443, 5443, 7443, 9443];
const INTERNAL_SS_PORT_CANDIDATES: [u16; 5] = [14433, 15433, 16433, 17433, 18433];
const PINNED_SING_BOX_IMAGE: &str = "ghcr.io/sagernet/sing-box:v1.10.7";
const WGCF_VERSION: &str = "2.2.29";
const BUNDLED_FALLBACK_WARP_ADDRESS_V4: &str = "172.16.0.2/32";
const BUNDLED_FALLBACK_WARP_ADDRESS_V6: &str = "2606:4700:110:84d0:bc95:602b:71f:611e/128";
const BUNDLED_FALLBACK_WARP_PRIVATE_KEY: &str = "QJFlY7Xqqmpd110buQYhO3kPns9aj4ddLTTUHyXFRWc=";
const BUNDLED_FALLBACK_WARP_ENDPOINT: &str = "162.159.192.1";
const BUNDLED_FALLBACK_WARP_ENDPOINT_PORT: u16 = 500;
const BUNDLED_FALLBACK_WARP_PEER_PUBLIC_KEY: &str = "bmXOC+F1FxEMF9dyiK2H5/1SUtzH0JuVo51h2wPfgyo=";
const CONTAINER_PREFIXES: [&str; 5] = [
    "sys-networkd",
    "mdns-relay",
    "core-authd",
    "netdiag-agent",
    "kernel-events",
];
const LEGACY_CONTAINER_NAME: &str = "sys-network-helper";
const SSH_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const SSH_SESSION_TIMEOUT: Duration = Duration::from_secs(60);
const REMOTE_DEPLOY_STALL_TIMEOUT: Duration = Duration::from_secs(60);
const REMOTE_DEPLOY_POLL_INTERVAL: Duration = Duration::from_millis(250);
const MAX_FALLBACK_COVER_DOMAINS: usize = 4;

#[derive(Debug)]
struct RemoteDeployTarget {
    external_port: u16,
    internal_ss_port: u16,
    container_name: String,
    reusing_existing_instance: bool,
    migrating_to_primary_port: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedServerProfile {
    pub host: String,
    pub user: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemoteTransportBootstrap {
    external_port: u16,
    #[serde(default = "default_internal_ss_port")]
    internal_ss_port: u16,
    cover_domain: String,
    #[serde(default)]
    fallback_cover_domains: Vec<String>,
    shadow_pass: String,
    ss_password: String,
    #[serde(default)]
    ss_server_password: String,
    #[serde(default)]
    issued_invites: Vec<RemoteInviteRecord>,
}

#[derive(Debug, Clone)]
struct LocalClientTransportState {
    cover_domain: String,
    shadow_pass: String,
    ss_password: String,
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

fn default_internal_ss_port() -> u16 {
    crate::generator::INTERNAL_SS_PORT
}

fn snapshot_for_cover_domain(cover_domain: impl Into<String>) -> TransportStateSnapshot {
    let cover_domain = cover_domain.into();

    TransportStateSnapshot {
        current_cover_domain: Some(cover_domain.clone()),
        available_cover_domains: crate::generator::available_cover_domains(),
        local_cover_domain: Some(cover_domain),
        requires_redeploy: false,
    }
}

fn ensure_local_client_rule_sets_sync(
    app: &AppHandle,
) -> Result<Vec<crate::geodata::LocalRuleSetAsset>, String> {
    tauri::async_runtime::block_on(crate::geodata::ensure_local_client_rule_sets(app))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InviteLinkPayload {
    version: u8,
    invite_id: String,
    host: String,
    external_port: u16,
    cover_domain: String,
    shadow_pass: String,
    ss_password: String,
    generated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredInviteLinkRecord {
    id: String,
    link: String,
    host: String,
    cover_domain: String,
    generated_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IssuedInviteLink {
    id: String,
    link: String,
    host: String,
    cover_domain: String,
    generated_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InviteImportResult {
    host: String,
    cover_domain: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeneratedInviteLinkResult {
    link: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalInstallationState {
    has_saved_server_profile: bool,
    has_client_config: bool,
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

fn remove_saved_server_profile(app: &AppHandle) -> Result<(), String> {
    let profile_path = server_profile_path(app)?;

    match std::fs::remove_file(profile_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn issued_invites_path(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;

    Ok(local_data.join("issued_invites.json"))
}

fn load_issued_invite_records(app: &AppHandle) -> Result<Vec<StoredInviteLinkRecord>, String> {
    let invites_path = issued_invites_path(app)?;

    if !invites_path.exists() {
        return Ok(Vec::new());
    }

    let contents = std::fs::read_to_string(&invites_path).map_err(|e| e.to_string())?;
    serde_json::from_str::<Vec<StoredInviteLinkRecord>>(&contents)
        .map_err(|e| format!("Failed to parse issued invites JSON: {}", e))
}

fn save_issued_invite_records(
    app: &AppHandle,
    invites: &[StoredInviteLinkRecord],
) -> Result<(), String> {
    let invites_path = issued_invites_path(app)?;

    if let Some(parent) = invites_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let json = serde_json::to_vec_pretty(invites).map_err(|e| e.to_string())?;
    std::fs::write(invites_path, json).map_err(|e| e.to_string())
}

pub(crate) fn clear_issued_invites(app: &AppHandle) -> Result<(), String> {
    let invites_path = issued_invites_path(app)?;

    match std::fs::remove_file(invites_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn issue_invite_id() -> Result<String, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();

    Ok(format!("invite-{:x}", timestamp))
}

fn compose_multi_user_ss_password(server_password: &str, user_password: &str) -> String {
    format!("{}:{}", server_password, user_password)
}

fn split_multi_user_ss_password(combined_password: &str) -> Option<(String, String)> {
    let (server_password, user_password) = combined_password.split_once(':')?;
    if server_password.is_empty() || user_password.is_empty() {
        return None;
    }

    Some((server_password.to_string(), user_password.to_string()))
}

fn resolve_master_ss_transport(
    app: &AppHandle,
    remote_bootstrap: &RemoteTransportBootstrap,
) -> Result<(String, String, String), String> {
    if !remote_bootstrap.ss_server_password.is_empty() {
        let (_, master_user_password) = split_multi_user_ss_password(&remote_bootstrap.ss_password)
            .ok_or_else(|| {
                "Remote bootstrap is missing the master multi-user Shadowsocks password."
                    .to_string()
            })?;

        return Ok((
            remote_bootstrap.ss_server_password.clone(),
            master_user_password,
            remote_bootstrap.ss_password.clone(),
        ));
    }

    let ss_server_password =
        tauri::async_runtime::block_on(crate::generator::generate_ss_password(app))?;
    let master_ss_user_password =
        tauri::async_runtime::block_on(crate::generator::generate_ss_password(app))?;
    let master_combined_password =
        compose_multi_user_ss_password(&ss_server_password, &master_ss_user_password);

    Ok((
        ss_server_password,
        master_ss_user_password,
        master_combined_password,
    ))
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

    Ok(RemoteTransportBootstrap {
        external_port,
        internal_ss_port,
        cover_domain,
        fallback_cover_domains,
        shadow_pass,
        ss_password,
        ss_server_password: String::new(),
        issued_invites: Vec::new(),
    })
}

fn build_rotated_cover_domain_history(
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

fn local_client_config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    Ok(local_data.join("client_config.json"))
}

fn load_local_client_transport_state(
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

#[tauri::command]
pub fn get_local_installation_state(app: AppHandle) -> Result<LocalInstallationState, String> {
    let profile_path = server_profile_path(&app)?;
    let client_config_path = local_client_config_path(&app)?;

    Ok(LocalInstallationState {
        has_saved_server_profile: profile_path.exists(),
        has_client_config: client_config_path.exists(),
    })
}

#[tauri::command]
pub fn list_issued_invite_links(app: AppHandle) -> Result<Vec<IssuedInviteLink>, String> {
    let records = load_issued_invite_records(&app)?;

    Ok(records
        .into_iter()
        .map(|record| IssuedInviteLink {
            id: record.id,
            link: record.link,
            host: record.host,
            cover_domain: record.cover_domain,
            generated_at: record.generated_at,
        })
        .collect())
}

#[tauri::command]
pub async fn delete_issued_invite_link(app: AppHandle, invite_id: String) -> Result<(), String> {
    let profile = load_saved_server_profile(app.clone())?
        .ok_or_else(|| "Saved server profile not found. Deploy once first.".to_string())?;

    let delete_result = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || -> Result<(), String> {
            let sess = connect_ssh_session(&app, &profile.host, &profile.user, &profile.password)?;
            let remote_bootstrap = load_remote_transport_bootstrap(&sess)?.ok_or_else(|| {
                "Remote transport bootstrap not found. Deploy the server first.".to_string()
            })?;
            let container_name = load_remote_container_name(&sess)?.ok_or_else(|| {
                "Remote RKN container is not active. Deploy the server first.".to_string()
            })?;

            let original_len = remote_bootstrap.issued_invites.len();
            let remaining_invites = remote_bootstrap
                .issued_invites
                .clone()
                .into_iter()
                .filter(|invite| invite.id != invite_id)
                .collect::<Vec<_>>();

            if remaining_invites.len() == original_len {
                return Err("Invite link not found in the active server configuration.".to_string());
            }

            let (ss_server_password, master_ss_user_password, master_combined_password) =
                resolve_master_ss_transport(&app, &remote_bootstrap)?;
            let warp_config = ensure_remote_warp_config(&app, &sess)?;
            let server_cfg = crate::generator::build_server_config_with_invites(
                crate::generator::ServerConfigParams {
                    master_shadow_pass: &remote_bootstrap.shadow_pass,
                    ss_server_password: &ss_server_password,
                    master_ss_user_password: &master_ss_user_password,
                    external_port: remote_bootstrap.external_port,
                    internal_ss_port: remote_bootstrap.internal_ss_port,
                    cover_domain: &remote_bootstrap.cover_domain,
                    fallback_cover_domains: &remote_bootstrap.fallback_cover_domains,
                    issued_invites: &remaining_invites,
                    warp: &warp_config,
                },
            );
            let bootstrap_cfg = json!({
                "external_port": remote_bootstrap.external_port,
                "internal_ss_port": remote_bootstrap.internal_ss_port,
                "cover_domain": remote_bootstrap.cover_domain,
                "fallback_cover_domains": remote_bootstrap.fallback_cover_domains,
                "shadow_pass": remote_bootstrap.shadow_pass,
                "ss_password": master_combined_password,
                "ss_server_password": ss_server_password,
                "issued_invites": remaining_invites
            })
            .to_string();
            let deploy_script = include_str!("../scripts/deploy.sh");
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
                    "Invite revoke deploy failed with code {}",
                    exit_status
                ));
            }

            validate_remote_runtime(
                &sess,
                &container_name,
                remote_bootstrap.external_port,
                remote_bootstrap.internal_ss_port,
            )?;

            let mut records = load_issued_invite_records(&app)?;
            records.retain(|record| record.id != invite_id);
            if records.is_empty() {
                clear_issued_invites(&app)?;
            } else {
                save_issued_invite_records(&app, &records)?;
            }

            Ok(())
        }
    })
    .await
    .unwrap();

    delete_result
}

fn local_transport_requires_redeploy(
    local_state: &LocalClientTransportState,
    remote_bootstrap: &RemoteTransportBootstrap,
) -> bool {
    local_state.cover_domain != remote_bootstrap.cover_domain
        || local_state.shadow_pass != remote_bootstrap.shadow_pass
        || local_state.ss_password != remote_bootstrap.ss_password
}

fn load_transport_state_snapshot_sync(app: &AppHandle) -> Result<TransportStateSnapshot, String> {
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

    let sess = connect_ssh_session(app, &profile.host, &profile.user, &profile.password)?;
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

pub(crate) async fn ensure_local_transport_is_current(app: &AppHandle) -> Result<(), String> {
    let check_app = app.clone();
    let snapshot_result = tauri::async_runtime::spawn_blocking(move || {
        load_transport_state_snapshot_sync(&check_app)
    })
    .await
    .unwrap();

    let snapshot = match snapshot_result {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let _ = app.emit(
                "tunnel-log",
                format!(
                    "[WARN] Unable to verify whether this device is in sync with the remote transport before starting the tunnel: {}",
                    error
                ),
            );
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

fn load_remote_container_name(sess: &Session) -> Result<Option<String>, String> {
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

fn load_remote_container_image(
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

fn remote_runtime_uses_warp(sess: &Session) -> Result<bool, String> {
    let command = r#"bash -lc '
CONFIG_DIR="/opt/rkn"
ACTIVE_CONFIG="$CONFIG_DIR/config.json"

if [ ! -f "$ACTIVE_CONFIG" ]; then
  echo "enabled=false"
  exit 0
fi

if grep -q '"tag"[[:space:]]*:[[:space:]]*"warp"' "$ACTIVE_CONFIG" \
  && grep -q '"final"[[:space:]]*:[[:space:]]*"warp"' "$ACTIVE_CONFIG"; then
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

fn bundled_shadow2_warp_config() -> Result<RemoteWarpConfig, String> {
    Ok(RemoteWarpConfig {
        private_key: BUNDLED_FALLBACK_WARP_PRIVATE_KEY.to_string(),
        address_v4: BUNDLED_FALLBACK_WARP_ADDRESS_V4.to_string(),
        address_v6: BUNDLED_FALLBACK_WARP_ADDRESS_V6.to_string(),
        endpoint: BUNDLED_FALLBACK_WARP_ENDPOINT.to_string(),
        endpoint_port: BUNDLED_FALLBACK_WARP_ENDPOINT_PORT,
        peer_public_key: BUNDLED_FALLBACK_WARP_PEER_PUBLIC_KEY.to_string(),
    })
}

fn ensure_remote_warp_config(app: &AppHandle, sess: &Session) -> Result<RemoteWarpConfig, String> {
    let _ = app.emit(
        "tunnel-log",
        "[SSH:WARP] Ensuring remote Cloudflare WARP identity...".to_string(),
    );

    let command = format!(
        r#"bash -lc '
set -e

CONFIG_DIR="/opt/rkn"
WARP_DIR="$CONFIG_DIR/warp"
WARP_JSON="$CONFIG_DIR/warp.json"
WGCF_BIN="/usr/local/bin/wgcf"
WGCF_VERSION="{wgcf_version}"
PRIMARY_ENDPOINT_HOST="162.159.192.1"
PRIMARY_ENDPOINT_PORT="500"

trap 'echo "__ERROR__ shell failed near line $LINENO"' ERR

mkdir -p "$WARP_DIR"

if [ -f "$WARP_JSON" ]; then
  echo "__STATUS__ existing"
  cat "$WARP_JSON"
  exit 0
fi

run_with_timeout() {{
  if command -v timeout >/dev/null 2>&1; then
    timeout "$@"
  else
    shift
    "$@"
  fi
}}

if [ ! -x "$WGCF_BIN" ]; then
  echo "__STEP__ download-wgcf"
  ARCH="$(uname -m)"
  case "$ARCH" in
    x86_64|amd64) WGCF_ARCH="amd64" ;;
    aarch64|arm64) WGCF_ARCH="arm64" ;;
    *)
      echo "__ERROR__ unsupported architecture: $ARCH"
      exit 1
      ;;
  esac

  WGCF_URL="https://github.com/ViRb3/wgcf/releases/download/v$WGCF_VERSION/wgcf_${{WGCF_VERSION}}_linux_${{WGCF_ARCH}}"
  run_with_timeout 45s curl --connect-timeout 10 --max-time 40 -fsSL "$WGCF_URL" -o "$WGCF_BIN"
  chmod +x "$WGCF_BIN"
fi

cd "$WARP_DIR"
rm -f wgcf-account.toml wgcf-profile.conf

echo "__STEP__ register-wgcf"
if ! run_with_timeout 45s "$WGCF_BIN" register --accept-tos >/dev/null 2>&1; then
  printf "y\n" | run_with_timeout 45s "$WGCF_BIN" register >/dev/null 2>&1
fi

echo "__STEP__ generate-wgcf"
run_with_timeout 45s "$WGCF_BIN" generate >/dev/null 2>&1

echo "__STEP__ parse-wgcf"
set +e

if [ ! -f wgcf-profile.conf ]; then
  echo "__ERROR__ missing wgcf-profile.conf"
  echo "__DEBUG__"
  ls -la "$WARP_DIR" 2>/dev/null || true
  exit 1
fi

PRIVATE_KEY="$(awk -F'=' '/^PrivateKey[[:space:]]*=/{{
  gsub(/[[:space:]\r]/, "", $2); print $2; exit
}}' wgcf-profile.conf || true)"
ADDRESS_LINE="$(awk -F'=' '/^Address[[:space:]]*=/{{
  gsub(/[[:space:]\r]/, "", $2); print $2
}}' wgcf-profile.conf | paste -sd',' - || true)"
PEER_PUBLIC_KEY="$(awk -F'=' 'BEGIN{{peer=0}} /^\[Peer\]$/{{peer=1; next}} peer && /^PublicKey[[:space:]]*=/{{
  gsub(/[[:space:]\r]/, "", $2); print $2; exit
}}' wgcf-profile.conf || true)"
ENDPOINT_LINE="$(awk -F'=' 'BEGIN{{peer=0}} /^\[Peer\]$/{{peer=1; next}} peer && /^Endpoint[[:space:]]*=/{{
  gsub(/[[:space:]\r]/, "", $2); print $2; exit
}}' wgcf-profile.conf || true)"

IFS=',' read -r ADDRESS_V4 ADDRESS_V6 _ <<< "$ADDRESS_LINE" || true
ADDRESS_V4="${{ADDRESS_V4:-}}"
ADDRESS_V6="${{ADDRESS_V6:-}}"

if [[ "$ENDPOINT_LINE" == *:* ]]; then
  ENDPOINT_HOST="${{ENDPOINT_LINE%:*}}"
  ENDPOINT_PORT="${{ENDPOINT_LINE##*:}}"
else
  ENDPOINT_HOST=""
  ENDPOINT_PORT=""
fi

if [ -z "$ENDPOINT_HOST" ] || [ -z "$ENDPOINT_PORT" ] || [ "$ENDPOINT_HOST" = "$ENDPOINT_LINE" ]; then
  ENDPOINT_HOST="$PRIMARY_ENDPOINT_HOST"
  ENDPOINT_PORT="$PRIMARY_ENDPOINT_PORT"
fi

if [ -z "$PRIVATE_KEY" ] || [ -z "$ADDRESS_V4" ] || [ -z "$PEER_PUBLIC_KEY" ]; then
  echo "__ERROR__ failed to parse wgcf-profile.conf"
  echo "__DEBUG__"
  sed -n '1,120p' wgcf-profile.conf 2>/dev/null || true
  exit 1
fi

set -e

cat > "$WARP_JSON" <<EOF
{{
  "private_key": "$PRIVATE_KEY",
  "address_v4": "$ADDRESS_V4",
  "address_v6": "$ADDRESS_V6",
  "endpoint": "$ENDPOINT_HOST",
  "endpoint_port": $ENDPOINT_PORT,
  "peer_public_key": "$PEER_PUBLIC_KEY"
}}
EOF

echo "__STATUS__ created"
cat "$WARP_JSON"
'"#,
        wgcf_version = WGCF_VERSION
    );

    let (stdout, exit_status) = run_remote_command(sess, &command)?;
    if exit_status != 0 {
        let fallback = bundled_shadow2_warp_config()?;
        let _ = app.emit(
            "tunnel-log",
            "[SSH:WARP] Remote wgcf bootstrap failed. Falling back to the bundled working WARP profile derived from the validated shadow2 setup."
                .to_string(),
        );

        let fallback_json = serde_json::to_string_pretty(&fallback).map_err(|e| e.to_string())?;
        let upload_command = format!(
            r#"bash -lc '
mkdir -p /opt/rkn
cat <<'"'"'EOF'"'"' > /opt/rkn/warp.json
{fallback_json}
EOF
cat /opt/rkn/warp.json
'"#,
            fallback_json = fallback_json
        );
        let (fallback_stdout, fallback_status) = run_remote_command(sess, &upload_command)?;
        if fallback_status != 0 {
            let step_summary = stdout
                .lines()
                .filter(|line| line.trim().starts_with("__STEP__"))
                .collect::<Vec<_>>()
                .join(" | ");
            return Err(format!(
                "Failed to provision remote WARP identity. Output: {}{}",
                stdout.trim(),
                if step_summary.is_empty() {
                    String::new()
                } else {
                    format!(" [steps: {}]", step_summary)
                }
            ));
        }

        let uploaded = serde_json::from_str::<RemoteWarpConfig>(fallback_stdout.trim())
            .map_err(|e| format!("Failed to parse uploaded fallback WARP JSON: {}", e))?;

        let _ = app.emit(
            "tunnel-log",
            format!(
                "[SSH:WARP] Using bundled fallback WARP endpoint {}:{}.",
                uploaded.endpoint, uploaded.endpoint_port
            ),
        );

        return Ok(uploaded);
    }

    let mut lines = stdout.lines();
    let status_line = lines.next().unwrap_or_default().trim().to_string();
    let json_payload = lines.collect::<Vec<_>>().join("\n");
    let warp = serde_json::from_str::<RemoteWarpConfig>(json_payload.trim()).map_err(|e| {
        format!(
            "Failed to parse remote WARP bootstrap JSON: {}. Output: {}",
            e,
            stdout.trim()
        )
    })?;

    let human_status = if status_line == "__STATUS__ existing" {
        "Reusing existing remote WARP identity."
    } else {
        "Created a fresh remote WARP identity."
    };
    let _ = app.emit("tunnel-log", format!("[SSH:WARP] {}", human_status));
    let _ = app.emit(
        "tunnel-log",
        format!(
            "[SSH:WARP] Using Cloudflare endpoint {}:{}.",
            warp.endpoint, warp.endpoint_port
        ),
    );

    Ok(warp)
}

fn load_remote_transport_bootstrap(
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

fn extract_invite_payload_segment(invite_link: &str) -> &str {
    let trimmed = invite_link.trim();

    if let Some(payload) = trimmed.strip_prefix("rkn://invite/") {
        return payload.trim();
    }

    if let Some(payload) = trimmed.strip_prefix("rkn-invite:") {
        return payload.trim();
    }

    if let Some(payload) = trimmed.strip_prefix("rkn://invite?data=") {
        return payload.trim();
    }

    if let Some(index) = trimmed.find("data=") {
        return trimmed[(index + 5)..].trim();
    }

    trimmed
}

fn parse_invite_link_payload(invite_link: &str) -> Result<InviteLinkPayload, String> {
    let payload_segment = extract_invite_payload_segment(invite_link);
    if payload_segment.is_empty() {
        return Err("Invite link is empty.".to_string());
    }

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_segment)
        .map_err(|e| format!("Failed to decode invite link payload: {}", e))?;
    let payload = serde_json::from_slice::<InviteLinkPayload>(&payload_bytes)
        .map_err(|e| format!("Failed to parse invite link payload JSON: {}", e))?;

    if payload.version != 1 {
        return Err(format!(
            "Unsupported invite link version: {}",
            payload.version
        ));
    }

    if payload.host.trim().is_empty()
        || payload.cover_domain.trim().is_empty()
        || payload.shadow_pass.trim().is_empty()
        || payload.ss_password.trim().is_empty()
    {
        return Err("Invite link payload is incomplete.".to_string());
    }

    Ok(payload)
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

fn summarize_runtime_validation_error(error: &str) -> String {
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

#[tauri::command]
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
        move || -> Result<Option<RemoteTransportBootstrap>, String> {
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
                    "[SSH WARN] Existing RKN runtime is still missing server-side WARP egress. Falling back to a fresh deploy to migrate the server routing.".to_string(),
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

        let deploy_script = include_str!("../scripts/deploy.sh");
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
pub async fn get_transport_state_snapshot(
    app: AppHandle,
) -> Result<TransportStateSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || load_transport_state_snapshot_sync(&app))
        .await
        .unwrap()
}

#[tauri::command]
pub async fn generate_invite_link(app: AppHandle) -> Result<GeneratedInviteLinkResult, String> {
    let profile = load_saved_server_profile(app.clone())?
        .ok_or_else(|| "Saved server profile not found. Deploy once first.".to_string())?;
    let local_data = app
        .path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());
    let invite_app = app.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        let sess =
            connect_ssh_session(&invite_app, &profile.host, &profile.user, &profile.password)?;
        let remote_bootstrap = load_remote_transport_bootstrap(&sess)?.ok_or_else(|| {
            "Remote transport bootstrap not found. Deploy the server first.".to_string()
        })?;
        let container_name = load_remote_container_name(&sess)?.ok_or_else(|| {
            "Remote RKN container is not active. Deploy the server first.".to_string()
        })?;
        let generated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        let invite_id = issue_invite_id()?;
        let invite_shadow_pass = tauri::async_runtime::block_on(
            crate::generator::generate_shadowtls_password(&invite_app),
        )?;
        let invite_ss_user_password =
            tauri::async_runtime::block_on(crate::generator::generate_ss_password(&invite_app))?;
        let (ss_server_password, master_ss_user_password, master_combined_password) =
            resolve_master_ss_transport(&invite_app, &remote_bootstrap)?;
        let invite_ss_password =
            compose_multi_user_ss_password(&ss_server_password, &invite_ss_user_password);
        let invite_host = profile.host.clone();
        let invite_cover_domain = remote_bootstrap.cover_domain.clone();
        let warp_config = ensure_remote_warp_config(&invite_app, &sess)?;
        let mut updated_invites = remote_bootstrap.issued_invites.clone();
        updated_invites.insert(
            0,
            RemoteInviteRecord {
                id: invite_id.clone(),
                shadow_pass: invite_shadow_pass.clone(),
                ss_user_password: invite_ss_user_password,
                generated_at,
            },
        );

        let server_cfg = crate::generator::build_server_config_with_invites(
            crate::generator::ServerConfigParams {
                master_shadow_pass: &remote_bootstrap.shadow_pass,
                ss_server_password: &ss_server_password,
                master_ss_user_password: &master_ss_user_password,
                external_port: remote_bootstrap.external_port,
                internal_ss_port: remote_bootstrap.internal_ss_port,
                cover_domain: &remote_bootstrap.cover_domain,
                fallback_cover_domains: &remote_bootstrap.fallback_cover_domains,
                issued_invites: &updated_invites,
                warp: &warp_config,
            },
        );
        let bootstrap_cfg = json!({
            "external_port": remote_bootstrap.external_port,
            "internal_ss_port": remote_bootstrap.internal_ss_port,
            "cover_domain": remote_bootstrap.cover_domain,
            "fallback_cover_domains": remote_bootstrap.fallback_cover_domains,
            "shadow_pass": remote_bootstrap.shadow_pass,
            "ss_password": master_combined_password,
            "ss_server_password": ss_server_password,
            "issued_invites": updated_invites
        })
        .to_string();
        let deploy_script = include_str!("../scripts/deploy.sh");
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

        let mut channel = sess.channel_session().map_err(|e| e.to_string())?;
        channel.exec("bash -s 2>&1").map_err(|e| e.to_string())?;
        channel
            .write_all(injected_script.as_bytes())
            .map_err(|e| e.to_string())?;
        channel.send_eof().map_err(|e| e.to_string())?;
        stream_remote_deploy_output(&invite_app, &sess, &mut channel)?;
        channel.wait_close().map_err(|e| e.to_string())?;
        let exit_status = channel.exit_status().map_err(|e| e.to_string())?;

        if exit_status != 0 {
            return Err(format!(
                "Invite link issuance deploy failed with code {}",
                exit_status
            ));
        }

        validate_remote_runtime(
            &sess,
            &container_name,
            remote_bootstrap.external_port,
            remote_bootstrap.internal_ss_port,
        )?;

        std::fs::create_dir_all(&local_data).map_err(|e| e.to_string())?;
        let local_rule_sets = ensure_local_client_rule_sets_sync(&invite_app)?;
        let master_client_cfg = crate::generator::build_client_config(
            &profile.host,
            &remote_bootstrap.shadow_pass,
            &master_combined_password,
            remote_bootstrap.external_port,
            &remote_bootstrap.cover_domain,
            &local_rule_sets,
        );
        let client_cfg_path = local_data.join("client_config.json");
        std::fs::write(&client_cfg_path, &master_client_cfg).map_err(|e| e.to_string())?;

        let payload = InviteLinkPayload {
            version: 1,
            invite_id: invite_id.clone(),
            host: invite_host.clone(),
            external_port: remote_bootstrap.external_port,
            cover_domain: invite_cover_domain.clone(),
            shadow_pass: invite_shadow_pass,
            ss_password: invite_ss_password,
            generated_at,
        };
        let payload_json =
            serde_json::to_vec(&payload).map_err(|e| format!("Invite payload error: {}", e))?;
        let encoded = URL_SAFE_NO_PAD.encode(payload_json);
        let link = format!("rkn://invite/{}", encoded);

        let mut records = load_issued_invite_records(&invite_app)?;
        records.insert(
            0,
            StoredInviteLinkRecord {
                id: invite_id,
                link: link.clone(),
                host: invite_host,
                cover_domain: invite_cover_domain,
                generated_at,
            },
        );
        save_issued_invite_records(&invite_app, &records)?;

        Ok(GeneratedInviteLinkResult { link })
    })
    .await
    .unwrap()?;

    crate::restart_tunnel_if_running(
        &app,
        "Master transport was refreshed while issuing an invite link. Restarting core to keep the local config in sync.",
    )
    .await?;

    Ok(result)
}

#[tauri::command]
pub async fn import_invite_link(
    app: AppHandle,
    invite_link: String,
) -> Result<InviteImportResult, String> {
    if load_saved_server_profile(app.clone())?.is_some() {
        return Err(
            "This app already has master access for a server. Reset local data before importing an invite link here."
                .to_string(),
        );
    }

    let local_data = app
        .path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());

    let result = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || -> Result<InviteImportResult, String> {
            let payload = parse_invite_link_payload(&invite_link)?;
            let local_rule_sets = ensure_local_client_rule_sets_sync(&app)?;
            let client_cfg = crate::generator::build_client_config(
                &payload.host,
                &payload.shadow_pass,
                &payload.ss_password,
                payload.external_port,
                &payload.cover_domain,
                &local_rule_sets,
            );

            std::fs::create_dir_all(&local_data).map_err(|e| e.to_string())?;
            let client_cfg_path = local_data.join("client_config.json");
            std::fs::write(&client_cfg_path, &client_cfg).map_err(|e| e.to_string())?;
            remove_saved_server_profile(&app)?;
            clear_issued_invites(&app)?;
            crate::refresh_tray_toggle_item(&app);

            let _ = app.emit(
                "tunnel-log",
                format!(
                    "[SYSTEM] Invite link imported successfully. Client config updated at: {:?}",
                    client_cfg_path
                ),
            );

            Ok(InviteImportResult {
                host: payload.host,
                cover_domain: payload.cover_domain,
            })
        }
    })
    .await
    .unwrap()?;

    crate::restart_tunnel_if_running(
        &app,
        "Tunnel config changed after importing an invite link. Restarting core to apply the updated client config.",
    )
    .await?;

    Ok(result)
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

/// Rotate only the active ShadowTLS cover domain while keeping transport
/// credentials stable. Clients still need a refreshed config plus a tunnel
/// restart to begin using the new SNI.
#[tauri::command]
pub async fn rotate_sni(app: AppHandle, target_domain: Option<String>) -> Result<String, String> {
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

        let deploy_script = include_str!("../scripts/deploy.sh");
        let (ss_server_password, master_ss_user_password, master_combined_password) =
            resolve_master_ss_transport(&rotate_app, &remote_bootstrap)?;
        let warp_config = ensure_remote_warp_config(&rotate_app, &sess)?;
        let server_cfg = crate::generator::build_server_config_with_invites(
            crate::generator::ServerConfigParams {
                master_shadow_pass: &remote_bootstrap.shadow_pass,
                ss_server_password: &ss_server_password,
                master_ss_user_password: &master_ss_user_password,
                external_port: remote_bootstrap.external_port,
                internal_ss_port: remote_bootstrap.internal_ss_port,
                cover_domain,
                fallback_cover_domains: &fallback_cover_domains,
                issued_invites: &remote_bootstrap.issued_invites,
                warp: &warp_config,
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
            "cover_domain": cover_domain,
            "fallback_cover_domains": fallback_cover_domains,
            "shadow_pass": remote_bootstrap.shadow_pass,
            "ss_password": master_combined_password,
            "ss_server_password": ss_server_password,
            "issued_invites": remote_bootstrap.issued_invites
        })
        .to_string();

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

        emit_ssh_stage(
            &rotate_app,
            "ROTATE",
            "Deploying new cover domain to server...",
        );
        let mut channel = sess.channel_session().map_err(|e| e.to_string())?;
        channel.exec("bash -s 2>&1").map_err(|e| e.to_string())?;
        channel
            .write_all(injected_script.as_bytes())
            .map_err(|e| e.to_string())?;
        channel.send_eof().map_err(|e| e.to_string())?;
        stream_remote_deploy_output(&rotate_app, &sess, &mut channel)?;

        channel.wait_close().map_err(|e| e.to_string())?;
        let exit_status = channel.exit_status().map_err(|e| e.to_string())?;

        if exit_status != 0 {
            return Err(format!(
                "SNI rotation deploy failed with code {}",
                exit_status
            ));
        }

        validate_remote_runtime(
            &sess,
            &container_name,
            remote_bootstrap.external_port,
            remote_bootstrap.internal_ss_port,
        )?;

        std::fs::create_dir_all(&local_data).map_err(|e| e.to_string())?;
        let client_cfg_path = local_data.join("client_config.json");
        std::fs::write(&client_cfg_path, &client_cfg).map_err(|e| e.to_string())?;

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
