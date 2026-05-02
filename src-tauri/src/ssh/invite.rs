use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

use super::deploy::{execute_remote_deploy, RemoteDeployExecution};
use super::warp::{
    clear_local_warp_profile_sync, ensure_remote_warp_config, load_saved_server_profile,
};
use super::{
    acquire_remote_mutation_lock, clear_cached_transport_bootstrap, connect_ssh_session,
    ensure_local_client_rule_sets_sync, ensure_master_role, load_cached_transport_bootstrap,
    load_remote_container_name, load_remote_transport_bootstrap, local_client_config_path,
    pinned_sing_box_image_for_routing_mode, remove_saved_server_profile, save_backend_app_role,
    save_cached_transport_bootstrap, BackendAppRole, LocalInstallationState, RemoteInviteRecord,
    RemoteTransportBootstrap,
};

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
    #[serde(default)]
    shadow_pass: String,
    #[serde(default)]
    ss_user_password: String,
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
struct InviteRemoteSyncEvent {
    invite_id: String,
    status: String,
    message: String,
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

fn invite_sync_revision() -> &'static AtomicU64 {
    static REVISION: OnceLock<AtomicU64> = OnceLock::new();
    REVISION.get_or_init(|| AtomicU64::new(0))
}

#[derive(Default)]
struct InviteSyncCoordinator {
    running: bool,
}

fn invite_sync_coordinator() -> &'static Mutex<InviteSyncCoordinator> {
    static COORDINATOR: OnceLock<Mutex<InviteSyncCoordinator>> = OnceLock::new();
    COORDINATOR.get_or_init(|| Mutex::new(InviteSyncCoordinator::default()))
}

const INVITE_REMOTE_SYNC_DEBOUNCE: Duration = Duration::from_secs(8);

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

pub(crate) fn resolve_master_ss_transport(
    app: &AppHandle,
    remote_bootstrap: &RemoteTransportBootstrap,
) -> Result<(String, String, String), String> {
    if !remote_bootstrap.ss_server_password.is_empty() {
        let master_user_password = if let Some((_, master_user_password)) =
            split_multi_user_ss_password(&remote_bootstrap.ss_password)
        {
            master_user_password
        } else {
            let _ = app.emit(
                    "tunnel-log",
                    "[SSH WARN] Remote bootstrap has a server Shadowsocks password but the master client password is still in the legacy single-password format. Migrating this device to a multi-user Shadowsocks credential while preserving the server password."
                        .to_string(),
                );
            tauri::async_runtime::block_on(crate::generator::generate_ss_password(app))?
        };
        let master_combined_password = compose_multi_user_ss_password(
            &remote_bootstrap.ss_server_password,
            &master_user_password,
        );

        return Ok((
            remote_bootstrap.ss_server_password.clone(),
            master_user_password,
            master_combined_password,
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

pub fn get_local_installation_state(app: AppHandle) -> Result<LocalInstallationState, String> {
    let profile_path = super::server_profile_path(&app)?;
    let client_config_path = local_client_config_path(&app)?;

    Ok(LocalInstallationState {
        has_saved_server_profile: profile_path.exists(),
        has_client_config: client_config_path.exists(),
    })
}

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

pub async fn delete_issued_invite_link(app: AppHandle, invite_id: String) -> Result<(), String> {
    let mut records = load_issued_invite_records(&app)?;
    let original_len = records.len();
    records.retain(|record| record.id != invite_id);

    if records.len() == original_len {
        return Err("Invite link not found in the local master list.".to_string());
    }

    if records.is_empty() {
        clear_issued_invites(&app)?;
    } else {
        save_issued_invite_records(&app, &records)?;
    }

    schedule_invite_remote_sync(
        &app,
        "Please wait while the previous invite is removed from the server.",
    );

    Ok(())
}

fn build_remote_invites_from_records(
    records: &[StoredInviteLinkRecord],
    remote_bootstrap: &RemoteTransportBootstrap,
) -> Result<Vec<RemoteInviteRecord>, String> {
    records
        .iter()
        .map(|record| {
            if !record.shadow_pass.trim().is_empty() && !record.ss_user_password.trim().is_empty() {
                return Ok(RemoteInviteRecord {
                    id: record.id.clone(),
                    shadow_pass: record.shadow_pass.clone(),
                    ss_user_password: record.ss_user_password.clone(),
                    generated_at: record.generated_at,
                });
            }

            if let Some(existing) = remote_bootstrap
                .issued_invites
                .iter()
                .find(|invite| invite.id == record.id)
            {
                return Ok(existing.clone());
            }

            Err(format!(
                "Invite {} is missing server-side secrets. Recreate this invite link before syncing again.",
                record.id
            ))
        })
        .collect()
}

fn sync_invites_remote_from_local_records(app: &AppHandle) -> Result<(), String> {
    let _mutation_guard = acquire_remote_mutation_lock()?;
    let profile = load_saved_server_profile(app.clone())?
        .ok_or_else(|| "Saved server profile not found. Deploy once first.".to_string())?;
    let records = load_issued_invite_records(app)?;

    let sess = connect_ssh_session(app, &profile.host, &profile.user, &profile.password)?;
    let remote_bootstrap = load_remote_transport_bootstrap(&sess)?.ok_or_else(|| {
        "Remote transport bootstrap not found. Deploy the server first.".to_string()
    })?;
    let container_name = load_remote_container_name(&sess)?.ok_or_else(|| {
        "Remote RKN container is not active. Deploy the server first.".to_string()
    })?;

    let synced_invites = build_remote_invites_from_records(&records, &remote_bootstrap)?;
    let (ss_server_password, master_ss_user_password, master_combined_password) =
        resolve_master_ss_transport(app, &remote_bootstrap)?;
    let warp_config = if remote_bootstrap.routing_mode == "warp" {
        Some(ensure_remote_warp_config(app, &sess)?)
    } else {
        None
    };
    let server_cfg =
        crate::generator::build_server_config_with_invites(crate::generator::ServerConfigParams {
            master_shadow_pass: &remote_bootstrap.shadow_pass,
            ss_server_password: &ss_server_password,
            master_ss_user_password: &master_ss_user_password,
            external_port: remote_bootstrap.external_port,
            internal_ss_port: remote_bootstrap.internal_ss_port,
            routing_mode: &remote_bootstrap.routing_mode,
            cover_domain: &remote_bootstrap.cover_domain,
            fallback_cover_domains: &remote_bootstrap.fallback_cover_domains,
            issued_invites: &synced_invites,
            warp: warp_config.as_ref(),
        });
    let bootstrap_cfg = json!({
        "external_port": remote_bootstrap.external_port,
        "internal_ss_port": remote_bootstrap.internal_ss_port,
        "routing_mode": remote_bootstrap.routing_mode,
        "cover_domain": remote_bootstrap.cover_domain,
        "fallback_cover_domains": remote_bootstrap.fallback_cover_domains,
        "shadow_pass": remote_bootstrap.shadow_pass,
        "ss_password": master_combined_password,
        "ss_server_password": ss_server_password,
        "issued_invites": synced_invites
    })
    .to_string();
    execute_remote_deploy(
        &sess,
        app,
        &RemoteDeployExecution {
            container_name: &container_name,
            external_port: remote_bootstrap.external_port,
            internal_ss_port: remote_bootstrap.internal_ss_port,
            sing_box_image: pinned_sing_box_image_for_routing_mode(&remote_bootstrap.routing_mode),
            server_cfg: &server_cfg,
            bootstrap_cfg: &bootstrap_cfg,
        },
    )
    .map_err(|error| {
        if let Some(code) = error
            .strip_prefix("Deployment script exited with ")
            .and_then(|value| value.trim().parse::<i32>().ok())
        {
            format!("Invite sync deploy failed with code {}", code)
        } else {
            error
        }
    })?;

    Ok(())
}

fn schedule_invite_remote_sync(app: &AppHandle, message: &str) {
    invite_sync_revision().fetch_add(1, Ordering::SeqCst);
    let _ = app.emit(
        "invite-remote-sync",
        InviteRemoteSyncEvent {
            invite_id: "invite-batch".to_string(),
            status: "started".to_string(),
            message: message.to_string(),
        },
    );

    let mut coordinator = match invite_sync_coordinator().lock() {
        Ok(guard) => guard,
        Err(_) => {
            let _ = app.emit(
                "invite-remote-sync",
                InviteRemoteSyncEvent {
                    invite_id: "invite-batch".to_string(),
                    status: "failed".to_string(),
                    message:
                        "Invite changes were saved locally, but the sync coordinator is unavailable right now."
                            .to_string(),
                },
            );
            return;
        }
    };

    if coordinator.running {
        return;
    }

    coordinator.running = true;
    drop(coordinator);

    let sync_app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let stable_revision = invite_sync_revision().load(Ordering::SeqCst);
            thread::sleep(INVITE_REMOTE_SYNC_DEBOUNCE);

            if invite_sync_revision().load(Ordering::SeqCst) != stable_revision {
                continue;
            }

            match tauri::async_runtime::spawn_blocking({
                let sync_app = sync_app.clone();
                move || sync_invites_remote_from_local_records(&sync_app)
            })
            .await
            .map_err(|error| error.to_string())
            .and_then(|result| result)
            {
                Ok(()) => {
                    let _ = sync_app.emit(
                        "invite-remote-sync",
                        InviteRemoteSyncEvent {
                            invite_id: "invite-batch".to_string(),
                            status: "completed".to_string(),
                            message: "Invite changes finished syncing on the server.".to_string(),
                        },
                    );
                }
                Err(error) => {
                    let _ = sync_app.emit(
                        "invite-remote-sync",
                        InviteRemoteSyncEvent {
                            invite_id: "invite-batch".to_string(),
                            status: "failed".to_string(),
                            message: format!(
                                "Invite changes were saved locally, but the server sync still needs attention: {}",
                                error
                            ),
                        },
                    );
                    let _ = sync_app.emit(
                        "tunnel-log",
                        format!(
                            "[WARN] Invite changes were saved locally, but the background server sync needs attention: {}",
                            error
                        ),
                    );
                }
            }

            if invite_sync_revision().load(Ordering::SeqCst) != stable_revision {
                continue;
            }

            let mut coordinator = match invite_sync_coordinator().lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };

            if invite_sync_revision().load(Ordering::SeqCst) == stable_revision {
                coordinator.running = false;
                break;
            }
        }
    });
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

fn resolve_bootstrap_for_invite(
    app: &AppHandle,
) -> Result<(RemoteTransportBootstrap, String), String> {
    let profile = load_saved_server_profile(app.clone())?
        .ok_or_else(|| "Saved server profile not found. Deploy once first.".to_string())?;

    if let Some(cached) = load_cached_transport_bootstrap(app)? {
        if !cached.cover_domain.is_empty()
            && !cached.ss_password.is_empty()
            && !cached.shadow_pass.is_empty()
            && cached.external_port > 0
        {
            return Ok((cached, profile.host));
        }
    }

    let sess = connect_ssh_session(app, &profile.host, &profile.user, &profile.password)?;
    let remote_bootstrap = load_remote_transport_bootstrap(&sess)?.ok_or_else(|| {
        "Remote transport bootstrap not found. Deploy the server first.".to_string()
    })?;
    let _ = save_cached_transport_bootstrap(app, &remote_bootstrap);

    Ok((remote_bootstrap, profile.host))
}

pub async fn generate_invite_link(app: AppHandle) -> Result<GeneratedInviteLinkResult, String> {
    ensure_master_role(&app, "generate invite links")?;

    let invite_shadow_pass = crate::generator::generate_shadowtls_password(&app).await?;
    let invite_ss_user_password = crate::generator::generate_ss_password(&app).await?;

    let invite_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(
        move || -> Result<GeneratedInviteLinkResult, String> {
            let (bootstrap, host) = resolve_bootstrap_for_invite(&invite_app)?;
            let generated_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_secs();
            let invite_id = issue_invite_id()?;
            let (ss_server_password, _, _) = resolve_master_ss_transport(&invite_app, &bootstrap)?;
            let invite_ss_password =
                compose_multi_user_ss_password(&ss_server_password, &invite_ss_user_password);

            let payload = InviteLinkPayload {
                version: 1,
                invite_id: invite_id.clone(),
                host: host.clone(),
                external_port: bootstrap.external_port,
                cover_domain: bootstrap.cover_domain.clone(),
                shadow_pass: invite_shadow_pass.clone(),
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
                    host,
                    cover_domain: bootstrap.cover_domain.clone(),
                    generated_at,
                    shadow_pass: invite_shadow_pass,
                    ss_user_password: invite_ss_user_password,
                },
            );
            save_issued_invite_records(&invite_app, &records)?;

            Ok(GeneratedInviteLinkResult { link })
        },
    )
    .await
    .unwrap()?;

    schedule_invite_remote_sync(
        &app,
        "Applying the latest invite changes on the server in the background.",
    );

    Ok(result)
}

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
            clear_local_warp_profile_sync(&app)?;
            clear_cached_transport_bootstrap(&app)?;
            save_backend_app_role(&app, BackendAppRole::Subordinate)?;
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
