use serde_json::json;
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

#[derive(Debug)]
pub struct RealityKeys {
    pub private_key: String,
    pub public_key: String,
}

pub async fn generate_reality_keypair(app: &AppHandle) -> Result<RealityKeys, String> {
    let sidecar = app
        .shell()
        .sidecar("sing-box")
        .map_err(|e| e.to_string())?
        .args(["generate", "reality-keypair"]);

    let output = sidecar.output().await.map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    let mut private_key = String::new();
    let mut public_key = String::new();

    for line in stdout.lines() {
        if line.starts_with("PrivateKey: ") {
            private_key = line.replace("PrivateKey: ", "").trim().to_string();
        } else if line.starts_with("PublicKey: ") {
            public_key = line.replace("PublicKey: ", "").trim().to_string();
        }
    }

    if private_key.is_empty() || public_key.is_empty() {
        return Err("Failed to parse reality keypair from sing-box".to_string());
    }

    Ok(RealityKeys {
        private_key,
        public_key,
    })
}

pub async fn generate_short_id(app: &AppHandle) -> Result<String, String> {
    let sidecar = app
        .shell()
        .sidecar("sing-box")
        .map_err(|e| e.to_string())?
        .args(["generate", "short-id"]);

    let output = sidecar.output().await.map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub async fn generate_uuid(app: &AppHandle) -> Result<String, String> {
    let sidecar = app
        .shell()
        .sidecar("sing-box")
        .map_err(|e| e.to_string())?
        .args(["generate", "uuid"]);

    let output = sidecar.output().await.map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn build_server_config(
    keys: &RealityKeys,
    short_id: &str,
    uuid: &str,
    shadow_pass: &str,
) -> String {
    let config = json!({
      "log": {
        "disabled": false,
        "level": "info",
        "timestamp": true
      },
      "inbounds": [
        {
          "type": "shadowtls",
          "tag": "in-stls",
          "listen": "::",
          "listen_port": 443,
          "version": 3,
          "users": [
            {
              "password": shadow_pass
            }
          ],
          "handshake": {
            "server": "104.21.35.210",
            "server_port": 443
          },
          "detour": "in-reality"
        },
        {
          "type": "vless",
          "tag": "in-reality",
          "listen": "127.0.0.1",
          "listen_port": 8443,
          "users": [
            {
              "name": "rkn-user",
              "uuid": uuid,
              "flow": "xtls-rprx-vision"
            }
          ],
          "tls": {
            "enabled": true,
            "server_name": "www.microsoft.com",
            "reality": {
              "enabled": true,
              "handshake": {
                "server": "104.21.35.210",
                "server_port": 443
              },
              "private_key": keys.private_key,
              "short_id": [short_id]
            }
          }
        }
      ],
      "outbounds": [
        {
          "type": "direct",
          "tag": "direct"
        }
      ]
    });

    serde_json::to_string_pretty(&config).unwrap()
}

pub fn build_client_config(
    server_ip: &str,
    keys: &RealityKeys,
    short_id: &str,
    uuid: &str,
    shadow_pass: &str,
) -> String {
    let config = json!({
      "log": {
        "level": "info",
        "timestamp": true
      },
      "inbounds": [
        {
          "type": "tun",
          "tag": "tun-in",
          "interface_name": "utun-rkn",
          "inet4_address": "172.19.0.1/30",
          "inet6_address": "fdfe:dcba:9876::1/126",
          "auto_route": true,
          "strict_route": true,
          "stack": "system",
          "mtu": 1280,
          "sniff": true
        }
      ],
      "outbounds": [
        {
          "type": "vless",
          "tag": "proxy",
          "server": server_ip,
          "server_port": 443,
          "uuid": uuid,
          "flow": "xtls-rprx-vision",
          "tls": {
            "enabled": true,
            "server_name": "www.microsoft.com",
            "utls": {
              "enabled": true,
              "fingerprint": "chrome"
            },
            "reality": {
              "enabled": true,
              "public_key": keys.public_key,
              "short_id": short_id
            }
          },
          "detour": "shadowtls-out"
        },
        {
          "type": "shadowtls",
          "tag": "shadowtls-out",
          "server": server_ip,
          "server_port": 443,
          "version": 3,
          "password": shadow_pass,
          "tls": {
            "enabled": true,
            "server_name": "104.21.35.210",
            "utls": {
              "enabled": true,
              "fingerprint": "chrome"
            }
          }
        },
        {
          "type": "direct",
          "tag": "direct"
        }
      ],
      "route": {
        "rules": [
          {
            "inbound": "tun-in",
            "action": "route",
            "outbound": "proxy"
          }
        ],
        "auto_detect_interface": true
      }
    });

    serde_json::to_string_pretty(&config).unwrap()
}
