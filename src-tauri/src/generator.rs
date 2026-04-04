use serde_json::json;
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use crate::geodata::{
    ADS_RULE_SET_TAG, CURATED_RU_DOMAIN_SUFFIXES, DIRECT_ROUTE_RULE_SET_TAGS,
    DNS_DIRECT_RULE_SET_TAGS, REMOTE_RULE_SETS,
};

#[derive(Debug)]
pub struct RealityKeys {
    pub private_key: String,
    pub public_key: String,
}

async fn run_singbox_generate(app: &AppHandle, args: &[&str]) -> Result<String, String> {
    let sidecar = app
        .shell()
        .sidecar("sing-box")
        .map_err(|e| e.to_string())?
        .args(args);

    let output = sidecar.output().await.map_err(|e| e.to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let message = if !stderr.is_empty() { stderr } else { stdout };
        return Err(format!(
            "sing-box generate {} failed: {}",
            args.join(" "),
            message
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn is_hex_string(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

pub async fn generate_reality_keypair(app: &AppHandle) -> Result<RealityKeys, String> {
    let stdout = run_singbox_generate(app, &["generate", "reality-keypair"]).await?;

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
    let short_id = run_singbox_generate(app, &["generate", "rand", "8", "--hex"]).await?;

    if !is_hex_string(&short_id) {
        return Err(format!("Generated short_id is not valid hex: {}", short_id));
    }

    Ok(short_id.to_ascii_lowercase())
}

pub async fn generate_uuid(app: &AppHandle) -> Result<String, String> {
    run_singbox_generate(app, &["generate", "uuid"]).await
}

pub fn build_server_config(
    keys: &RealityKeys,
    short_id: &str,
    uuid: &str,
    shadow_pass: &str,
    external_port: u16,
) -> String {
    let cover_domain = "www.microsoft.com";

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
          "listen_port": external_port,
          "version": 3,
          "users": [
            {
              "password": shadow_pass
            }
          ],
          "handshake": {
            "server": cover_domain,
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
            "server_name": cover_domain,
            "reality": {
              "enabled": true,
              "handshake": {
                "server": cover_domain,
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
    external_port: u16,
) -> String {
    let cover_domain = "www.microsoft.com";

    let mut tun_inbound = json!({
      "type": "tun",
      "tag": "tun-in",
      "address": [
        "172.19.0.1/30",
        "fdfe:dcba:9876::1/126"
      ],
      "auto_route": true,
      "strict_route": true,
      "stack": "system",
      "mtu": 1280
    });

    if !cfg!(target_os = "macos") {
        tun_inbound["interface_name"] = json!("tun0");
    }

    let config = json!({
      "log": {
        "level": "info",
        "timestamp": true
      },

      // --- DNS: sing-box 1.12+ новый формат серверов ---
      "dns": {
        "servers": [
          {
            "type": "https",
            "tag": "dns-remote",
            "server": "1.1.1.1",
            "server_port": 443,
            "domain_resolver": "dns-bootstrap"
          },
          {
            "type": "https",
            "tag": "dns-direct",
            "server": "dns.google",
            "server_port": 443,
            "domain_resolver": "dns-bootstrap"
          },
          {
            "type": "udp",
            "tag": "dns-bootstrap",
            "server": "8.8.8.8"
          }
        ],
        "strategy": "prefer_ipv4",
        "rules": [
          {
            "rule_set": ADS_RULE_SET_TAG,
            "action": "reject"
          },
          {
            "domain_suffix": CURATED_RU_DOMAIN_SUFFIXES,
            "server": "dns-direct"
          },
          {
            "rule_set": DNS_DIRECT_RULE_SET_TAGS,
            "server": "dns-direct"
          }
        ],
        "final": "dns-remote",
        "independent_cache": true
      },

      // --- TUN Inbound: перехват всего системного трафика ---
      "inbounds": [
        tun_inbound
      ],

      // --- Outbounds ---
      "outbounds": [
        {
          "type": "vless",
          "tag": "proxy",
          "server": server_ip,
          "server_port": 8443,
          "uuid": uuid,
          "flow": "xtls-rprx-vision",
          "tls": {
            "enabled": true,
            "server_name": cover_domain,
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
          "server_port": external_port,
          "version": 3,
          "password": shadow_pass,
          "tls": {
            "enabled": true,
            "server_name": cover_domain,
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

      // --- Route: Умная маршрутизация (sing-box 1.11+ actions) ---
      "route": {
        "rules": [
          // Sniff: определяет протокол трафика
          {
            "action": "sniff"
          },
          // DNS-запросы → перехватываем и отправляем в DNS-модуль
          {
            "protocol": "dns",
            "action": "hijack-dns"
          },
          // Критичные российские домены — напрямую, даже если их нет в rule-set
          {
            "domain_suffix": CURATED_RU_DOMAIN_SUFFIXES,
            "action": "route",
            "outbound": "direct"
          },
          // Российские сайты и IP — напрямую (Split-Tunneling)
          {
            "rule_set": DIRECT_ROUTE_RULE_SET_TAGS,
            "action": "route",
            "outbound": "direct"
          },
          // Реклама — блочим
          {
            "rule_set": ADS_RULE_SET_TAG,
            "action": "reject"
          },
          // Приватные сети (192.168.x.x, 10.x.x.x) — напрямую
          {
            "ip_is_private": true,
            "action": "route",
            "outbound": "direct"
          }
        ],
        // Всё остальное — через зашифрованный туннель
        "final": "proxy",
        "auto_detect_interface": true,
        "default_domain_resolver": {
          "server": "dns-bootstrap",
          "strategy": "prefer_ipv4"
        },

        // GeoIP / GeoSite базы — sing-box скачает сам (remote rule-set)
        "rule_set": REMOTE_RULE_SETS.iter().map(|rule_set| {
          json!({
            "tag": rule_set.tag,
            "type": "remote",
            "format": "binary",
            "url": rule_set.url,
            "download_detour": "direct"
          })
        }).collect::<Vec<_>>()
      }
    });

    serde_json::to_string_pretty(&config).unwrap()
}
