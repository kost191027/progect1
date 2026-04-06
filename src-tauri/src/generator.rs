use serde_json::json;
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use crate::geodata::{CURATED_RU_DOMAIN_SUFFIXES, DIRECT_ROUTE_RULE_SET_TAGS, REMOTE_RULE_SETS};

pub const INTERNAL_SS_PORT: u16 = 14433;

/// Cover domains for ShadowTLS handshake — high-traffic TLS sites that DPI
/// expects to see on any network. Rotated per deploy to diversify fingerprint.
const COVER_DOMAINS: &[&str] = &[
    "www.microsoft.com",
    "www.apple.com",
    "www.googleapis.com",
    "cdn.cloudflare.com",
    "www.amazon.com",
];

/// Pick a cover domain deterministically from the short_id hex seed.
pub fn select_cover_domain(short_id: &str) -> &'static str {
    let seed = short_id
        .get(0..2)
        .and_then(|h| u8::from_str_radix(h, 16).ok())
        .unwrap_or(0) as usize;
    COVER_DOMAINS[seed % COVER_DOMAINS.len()]
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

pub async fn generate_short_id(app: &AppHandle) -> Result<String, String> {
    let short_id = run_singbox_generate(app, &["generate", "rand", "8", "--hex"]).await?;

    if !is_hex_string(&short_id) {
        return Err(format!("Generated short_id is not valid hex: {}", short_id));
    }

    Ok(short_id.to_ascii_lowercase())
}

pub async fn generate_shadowtls_password(app: &AppHandle) -> Result<String, String> {
    let password = run_singbox_generate(app, &["generate", "rand", "16", "--hex"]).await?;

    if !is_hex_string(&password) {
        return Err(format!(
            "Generated ShadowTLS password is not valid hex: {}",
            password
        ));
    }

    if password.len() < 32 {
        return Err(format!(
            "Generated ShadowTLS password is unexpectedly short ({} chars): {}",
            password.len(),
            password
        ));
    }

    Ok(password.to_ascii_lowercase())
}

pub async fn generate_ss_password(app: &AppHandle) -> Result<String, String> {
    run_singbox_generate(app, &["generate", "rand", "16", "--base64"]).await
}

pub fn build_server_config(
    _server_ip: &str,
    shadow_pass: &str,
    ss_password: &str,
    external_port: u16,
    cover_domain: &str,
) -> String {
    // Always bind to 0.0.0.0 — on NAT VPS the public IP is not assigned
    // to any local interface, so bind(public_ip) fails with EADDRNOTAVAIL.
    let listen_host = "0.0.0.0";

    let config = json!({
      "log": {
        "disabled": false,
        "level": "debug",
        "timestamp": true
      },
      "inbounds": [
        {
          "type": "shadowtls",
          "tag": "in-stls",
          "listen": listen_host,
          "listen_port": external_port,
          "version": 3,
          "users": [
            {
              "name": "default",
              "password": shadow_pass
            }
          ],
          "handshake": {
            "server": cover_domain,
            "server_port": 443
          },
          "detour": "ss-in"
        },
        {
          "type": "shadowsocks",
          "tag": "ss-in",
          "listen": "127.0.0.1",
          "listen_port": INTERNAL_SS_PORT,
          "method": "2022-blake3-aes-128-gcm",
          "password": ss_password,
          "multiplex": {
            "enabled": true
          }
        }
      ],
      "outbounds": [
        {
          "type": "direct",
          "tag": "direct"
        }
      ],
      "route": {
        "rules": [
          {
            "outbound": "direct"
          }
        ]
      }
    });

    serde_json::to_string_pretty(&config).unwrap()
}

pub fn build_client_config(
    server_ip: &str,
    shadow_pass: &str,
    ss_password: &str,
    external_port: u16,
    cover_domain: &str,
) -> String {
    let mut tun_inbound = json!({
      "type": "tun",
      "tag": "tun-in",
      "address": ["172.19.0.1/30"],
      "auto_route": true,
      "strict_route": true,
      "stack": "system"
    });

    if !cfg!(target_os = "macos") {
        tun_inbound["interface_name"] = json!("tun0");
    }

    let config = json!({
      "log": {
        "level": "info"
      },

      "dns": {
        "servers": [
          {
            "tag": "proxy-dns",
            "type": "udp",
            "server": "1.1.1.1",
            "detour": "proxy"
          },
          {
            "tag": "direct-dns",
            "type": "udp",
            "server": "77.88.8.8",
            "detour": "direct"
          }
        ],
        "rules": [
          {
            "rule_set": DIRECT_ROUTE_RULE_SET_TAGS,
            "server": "direct-dns"
          },
          {
            "domain_suffix": CURATED_RU_DOMAIN_SUFFIXES,
            "server": "direct-dns"
          }
        ],
        "final": "proxy-dns"
      },

      "inbounds": [
        tun_inbound
      ],

      "outbounds": [
        {
          "type": "shadowsocks",
          "tag": "proxy",
          "server": server_ip,
          "server_port": external_port,
          "method": "2022-blake3-aes-128-gcm",
          "password": ss_password,
          "udp_over_tcp": true,
          "multiplex": {
            "enabled": true
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

      "route": {
        "rules": [
          {
            "action": "sniff"
          },
          {
            "protocol": "dns",
            "action": "hijack-dns"
          },
          {
            "ip_cidr": [format!("{}/32", server_ip)],
            "action": "route",
            "outbound": "direct"
          },
          {
            "domain_suffix": CURATED_RU_DOMAIN_SUFFIXES,
            "action": "route",
            "outbound": "direct"
          },
          {
            "rule_set": DIRECT_ROUTE_RULE_SET_TAGS,
            "action": "route",
            "outbound": "direct"
          }
        ],
        "final": "proxy",
        "auto_detect_interface": true,
        "rule_set": REMOTE_RULE_SETS
        .iter()
        .filter(|rule_set| DIRECT_ROUTE_RULE_SET_TAGS.contains(&rule_set.tag))
        .map(|rule_set| {
          json!({
            "tag": rule_set.tag,
            "type": "remote",
            "format": "binary",
            "url": rule_set.url,
            "download_detour": "direct"
          })
        })
        .collect::<Vec<_>>()
      }
    });

    serde_json::to_string_pretty(&config).unwrap()
}
