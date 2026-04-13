use serde_json::Value;
use ssh2::Session;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

use super::{
    connect_ssh_session, run_remote_command, server_profile_path, validate_warp_config,
    warp_status_from_config, LocalWarpProfileStatus, RemoteWarpConfig, SavedServerProfile,
    WGCF_VERSION,
};

fn local_warp_profile_path(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;

    Ok(local_data.join("warp_profile.json"))
}

fn load_local_warp_config_sync(app: &AppHandle) -> Result<Option<RemoteWarpConfig>, String> {
    let profile_path = local_warp_profile_path(app)?;
    if !profile_path.exists() {
        return Ok(None);
    }

    let contents = std::fs::read_to_string(&profile_path).map_err(|e| e.to_string())?;
    let config = serde_json::from_str::<RemoteWarpConfig>(&contents)
        .map_err(|e| format!("Failed to parse local WARP profile: {}", e))?;
    validate_warp_config(&config)?;

    Ok(Some(config))
}

fn save_local_warp_config_sync(app: &AppHandle, config: &RemoteWarpConfig) -> Result<(), String> {
    validate_warp_config(config)?;

    let profile_path = local_warp_profile_path(app)?;
    if let Some(parent) = profile_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let json = serde_json::to_vec_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(profile_path, json).map_err(|e| e.to_string())
}

pub(crate) fn clear_local_warp_profile_sync(app: &AppHandle) -> Result<(), String> {
    let profile_path = local_warp_profile_path(app)?;

    match std::fs::remove_file(profile_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn json_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn json_u16(value: Option<&Value>) -> Option<u16> {
    value
        .and_then(|item| item.as_u64().or_else(|| item.as_str()?.parse::<u64>().ok()))
        .and_then(|value| u16::try_from(value).ok())
}

fn parse_endpoint(endpoint_line: &str) -> Result<(String, u16), String> {
    let trimmed = endpoint_line.trim();
    let (host, port) = trimmed
        .rsplit_once(':')
        .ok_or_else(|| format!("Invalid WARP endpoint: {}", trimmed))?;
    let port = port
        .parse::<u16>()
        .map_err(|e| format!("Invalid WARP endpoint port '{}': {}", port, e))?;

    Ok((host.trim().to_string(), port))
}

fn parse_compact_warp_json(value: &Value) -> Option<RemoteWarpConfig> {
    Some(RemoteWarpConfig {
        private_key: json_string(value.get("private_key"))?,
        address_v4: json_string(value.get("address_v4"))?,
        address_v6: json_string(value.get("address_v6")).unwrap_or_default(),
        endpoint: json_string(value.get("endpoint"))?,
        endpoint_port: json_u16(value.get("endpoint_port"))?,
        peer_public_key: json_string(value.get("peer_public_key"))?,
    })
}

fn parse_wireguard_outbound_json(value: &Value) -> Option<RemoteWarpConfig> {
    let local_addresses = value.get("local_address")?.as_array()?;
    let address_v4 = local_addresses
        .iter()
        .filter_map(Value::as_str)
        .find(|item| item.contains('.'))?
        .trim()
        .to_string();
    let address_v6 = local_addresses
        .iter()
        .filter_map(Value::as_str)
        .find(|item| item.contains(':'))
        .map(str::trim)
        .unwrap_or_default()
        .to_string();

    Some(RemoteWarpConfig {
        private_key: json_string(value.get("private_key"))?,
        address_v4,
        address_v6,
        endpoint: json_string(value.get("server"))?,
        endpoint_port: json_u16(value.get("server_port"))?,
        peer_public_key: json_string(value.get("peer_public_key"))?,
    })
}

fn parse_outbound_from_singbox_config(value: &Value) -> Option<RemoteWarpConfig> {
    let outbounds = value.get("outbounds")?.as_array()?;
    let outbound = outbounds.iter().find(|item| {
        item.get("type").and_then(Value::as_str) == Some("wireguard")
            && item.get("tag").and_then(Value::as_str) == Some("warp")
    })?;

    parse_wireguard_outbound_json(outbound)
}

fn parse_wgcf_profile_text(profile_text: &str) -> Result<RemoteWarpConfig, String> {
    let mut private_key = None;
    let mut address_line = None;
    let mut peer_public_key = None;
    let mut endpoint_line = None;
    let mut in_peer_section = false;

    for line in profile_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        if trimmed.eq_ignore_ascii_case("[peer]") {
            in_peer_section = true;
            continue;
        }

        if trimmed.starts_with('[') {
            in_peer_section = false;
            continue;
        }

        let Some((raw_key, raw_value)) = trimmed.split_once('=') else {
            continue;
        };

        let key = raw_key.trim();
        let value = raw_value.trim();
        if value.is_empty() {
            continue;
        }

        if !in_peer_section && key.eq_ignore_ascii_case("PrivateKey") {
            private_key = Some(value.to_string());
        } else if !in_peer_section && key.eq_ignore_ascii_case("Address") {
            address_line = Some(value.to_string());
        } else if in_peer_section && key.eq_ignore_ascii_case("PublicKey") {
            peer_public_key = Some(value.to_string());
        } else if in_peer_section && key.eq_ignore_ascii_case("Endpoint") {
            endpoint_line = Some(value.to_string());
        }
    }

    let address_line =
        address_line.ok_or_else(|| "WARP profile is missing Address.".to_string())?;
    let mut address_parts = address_line
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let address_v4 = address_parts
        .find(|value| value.contains('.'))
        .ok_or_else(|| "WARP profile is missing IPv4 Address.".to_string())?
        .to_string();
    let address_v6 = address_line
        .split(',')
        .map(str::trim)
        .find(|value| value.contains(':'))
        .unwrap_or_default()
        .to_string();
    let endpoint_line =
        endpoint_line.ok_or_else(|| "WARP profile is missing Endpoint.".to_string())?;
    let (endpoint, endpoint_port) = parse_endpoint(&endpoint_line)?;

    Ok(RemoteWarpConfig {
        private_key: private_key
            .ok_or_else(|| "WARP profile is missing PrivateKey.".to_string())?,
        address_v4,
        address_v6,
        endpoint,
        endpoint_port,
        peer_public_key: peer_public_key
            .ok_or_else(|| "WARP profile is missing Peer PublicKey.".to_string())?,
    })
}

fn parse_local_warp_profile(profile_text: &str) -> Result<RemoteWarpConfig, String> {
    let trimmed = profile_text.trim();
    if trimmed.is_empty() {
        return Err("Paste a WARP profile first.".to_string());
    }

    if trimmed.starts_with('{') {
        let value = serde_json::from_str::<Value>(trimmed)
            .map_err(|e| format!("Failed to parse WARP profile JSON: {}", e))?;
        let parsed = parse_compact_warp_json(&value)
            .or_else(|| parse_wireguard_outbound_json(&value))
            .or_else(|| parse_outbound_from_singbox_config(&value))
            .ok_or_else(|| {
                "WARP JSON is unsupported. Paste either the compact RKN warp.json, a wireguard outbound object, or a sing-box config with a warp outbound."
                    .to_string()
            })?;
        validate_warp_config(&parsed)?;
        return Ok(parsed);
    }

    let parsed = parse_wgcf_profile_text(trimmed)?;
    validate_warp_config(&parsed)?;
    Ok(parsed)
}

fn load_remote_warp_config(sess: &Session) -> Result<Option<RemoteWarpConfig>, String> {
    let command = r#"bash -lc '
WARP_JSON="/opt/rkn/warp.json"
if [ -f "$WARP_JSON" ]; then
  cat "$WARP_JSON"
fi
'"#;

    let (stdout, exit_status) = run_remote_command(sess, command)?;
    if exit_status != 0 {
        return Err(format!(
            "Failed to read remote WARP identity. Output: {}",
            stdout.trim()
        ));
    }

    if stdout.trim().is_empty() {
        return Ok(None);
    }

    let config = serde_json::from_str::<RemoteWarpConfig>(stdout.trim())
        .map_err(|e| format!("Failed to parse remote WARP JSON: {}", e))?;
    validate_warp_config(&config)?;

    Ok(Some(config))
}

fn upload_remote_warp_config(
    sess: &Session,
    config: &RemoteWarpConfig,
) -> Result<RemoteWarpConfig, String> {
    validate_warp_config(config)?;
    let config_json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    let command = format!(
        r#"bash -lc '
mkdir -p /opt/rkn
cat <<'"'"'EOF'"'"' > /opt/rkn/warp.json
{config_json}
EOF
cat /opt/rkn/warp.json
'"#,
        config_json = config_json
    );
    let (stdout, exit_status) = run_remote_command(sess, &command)?;
    if exit_status != 0 {
        return Err(format!(
            "Failed to upload remote WARP profile. Output: {}",
            stdout.trim()
        ));
    }

    let uploaded = serde_json::from_str::<RemoteWarpConfig>(stdout.trim())
        .map_err(|e| format!("Failed to parse uploaded WARP JSON: {}", e))?;
    validate_warp_config(&uploaded)?;

    Ok(uploaded)
}

pub(crate) fn ensure_remote_warp_config(
    app: &AppHandle,
    sess: &Session,
) -> Result<RemoteWarpConfig, String> {
    let _ = app.emit(
        "tunnel-log",
        "[SSH:WARP] Ensuring remote Cloudflare WARP identity...".to_string(),
    );

    if let Some(local_import) = load_local_warp_config_sync(app)? {
        let _ = app.emit(
            "tunnel-log",
            "[SSH:WARP] Uploading the locally imported WARP profile to the remote server."
                .to_string(),
        );
        let uploaded = upload_remote_warp_config(sess, &local_import)?;
        let _ = app.emit(
            "tunnel-log",
            format!(
                "[SSH:WARP] Using imported WARP endpoint {}:{}.",
                uploaded.endpoint, uploaded.endpoint_port
            ),
        );
        return Ok(uploaded);
    }

    if let Some(existing) = load_remote_warp_config(sess)? {
        let _ = app.emit(
            "tunnel-log",
            "[SSH:WARP] Reusing the existing remote WARP identity.".to_string(),
        );
        let _ = app.emit(
            "tunnel-log",
            format!(
                "[SSH:WARP] Using Cloudflare endpoint {}:{}.",
                existing.endpoint, existing.endpoint_port
            ),
        );
        return Ok(existing);
    }

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
        let message =
            "Automatic remote WARP bootstrap failed. Import a personal WARP profile in Server Access and redeploy.";
        let _ = app.emit("tunnel-log", format!("[SSH:WARP] {}", message));
        return Err(format!("{} [Automatic remote WARP bootstrap failed. Import a personal WARP profile in Server Access and redeploy.]", message));
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

pub fn get_local_warp_profile_status(app: AppHandle) -> Result<LocalWarpProfileStatus, String> {
    let config = load_local_warp_config_sync(&app)?;
    Ok(warp_status_from_config(config.as_ref()))
}

pub fn import_local_warp_profile(
    app: AppHandle,
    profile_text: String,
) -> Result<LocalWarpProfileStatus, String> {
    let config = parse_local_warp_profile(&profile_text)?;
    save_local_warp_config_sync(&app, &config)?;
    Ok(warp_status_from_config(Some(&config)))
}

pub fn bootstrap_local_warp_profile(app: AppHandle) -> Result<LocalWarpProfileStatus, String> {
    let profile = load_saved_server_profile(app.clone())?.ok_or_else(|| {
        "Saved server profile not found. Deploy or attach a server first.".to_string()
    })?;

    bootstrap_local_warp_profile_from_profile(app, profile)
}

pub fn bootstrap_local_warp_profile_from_credentials(
    app: AppHandle,
    host: String,
    user: String,
    password: String,
) -> Result<LocalWarpProfileStatus, String> {
    bootstrap_local_warp_profile_from_profile(
        app,
        SavedServerProfile {
            host,
            user,
            password,
        },
    )
}

fn bootstrap_local_warp_profile_from_profile(
    app: AppHandle,
    profile: SavedServerProfile,
) -> Result<LocalWarpProfileStatus, String> {
    let warp = tauri::async_runtime::block_on(tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || -> Result<RemoteWarpConfig, String> {
            let sess = connect_ssh_session(&app, &profile.host, &profile.user, &profile.password)?;
            ensure_remote_warp_config(&app, &sess)
        }
    }))
    .map_err(|error| error.to_string())??;

    save_local_warp_config_sync(&app, &warp)?;

    let _ = app.emit(
        "tunnel-log",
        format!(
            "[SYSTEM] Local WARP profile created from the current server and saved on this Mac ({}:{}).",
            warp.endpoint, warp.endpoint_port
        ),
    );

    Ok(warp_status_from_config(Some(&warp)))
}

pub fn clear_local_warp_profile(app: AppHandle) -> Result<(), String> {
    clear_local_warp_profile_sync(&app)
}
