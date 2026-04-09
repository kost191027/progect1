use serde_json::{json, Value};
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use crate::geodata::{CURATED_RU_DOMAIN_SUFFIXES, DIRECT_ROUTE_RULE_SET_TAGS, REMOTE_RULE_SETS};

pub const INTERNAL_SS_PORT: u16 = 14433;

/// ShadowTLS cover domains.
///
/// Stable domains used for first deploys.
///
/// We avoid staying Apple-only here because some routes can start failing on
/// `captive.apple.com` / `publicassets.cdn-apple.com` even when the generated
/// client and server configs still match. The primary pool therefore mixes the
/// upstream ShadowTLS TLS1.3-compatible domains with a smaller Apple subset.
const PRIMARY_COVER_DOMAINS: &[&str] = &[
    "feishu.cn",
    "coding.net",
    "cloud.tencent.com",
    "weather-data.apple.com",
    "publicassets.cdn-apple.com",
];

/// Additional ShadowTLS-friendly domains used only for SNI rotation.
///
/// `feishu.cn`, `coding.net`, `upyun.com`, `douyin.com`, `toutiao.com`,
/// `publicassets.cdn-apple.com` and `weather-data.apple.com` come from the
/// upstream ShadowTLS TLS 1.3 compatibility list. `cloud.tencent.com` and
/// `captive.apple.com` are kept because they are used in the upstream
/// ShadowTLS examples and have worked for us on some paths.
const ROTATION_COVER_DOMAINS: &[&str] = &[
    "feishu.cn",
    "coding.net",
    "cloud.tencent.com",
    "weather-data.apple.com",
    "publicassets.cdn-apple.com",
    "upyun.com",
    "douyin.com",
    "toutiao.com",
    "mp.weixin.qq.com",
    "captive.apple.com",
];

pub fn available_cover_domains() -> Vec<String> {
    let mut domains = Vec::new();

    for domain in PRIMARY_COVER_DOMAINS
        .iter()
        .chain(ROTATION_COVER_DOMAINS.iter())
        .copied()
    {
        if !domains.iter().any(|item| item == domain) {
            domains.push(domain.to_string());
        }
    }

    domains
}

pub fn is_supported_cover_domain(domain: &str) -> bool {
    PRIMARY_COVER_DOMAINS.contains(&domain) || ROTATION_COVER_DOMAINS.contains(&domain)
}

pub fn select_cover_domain(short_id: &str) -> &'static str {
    let seed = short_id
        .get(0..2)
        .and_then(|h| u8::from_str_radix(h, 16).ok())
        .unwrap_or(0) as usize;
    PRIMARY_COVER_DOMAINS[seed % PRIMARY_COVER_DOMAINS.len()]
}

pub fn select_next_cover_domain(
    current_cover_domain: &str,
    occupied_cover_domains: &[String],
) -> &'static str {
    if let Some(candidate) = ROTATION_COVER_DOMAINS.iter().copied().find(|candidate| {
        *candidate != current_cover_domain
            && !occupied_cover_domains
                .iter()
                .any(|domain| domain == candidate)
    }) {
        return candidate;
    }

    let current_index = ROTATION_COVER_DOMAINS
        .iter()
        .position(|candidate| *candidate == current_cover_domain)
        .unwrap_or(0);

    for offset in 1..=ROTATION_COVER_DOMAINS.len() {
        let candidate =
            ROTATION_COVER_DOMAINS[(current_index + offset) % ROTATION_COVER_DOMAINS.len()];
        if candidate != current_cover_domain {
            return candidate;
        }
    }

    ROTATION_COVER_DOMAINS[0]
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
    build_server_config_with_fallbacks(
        _server_ip,
        shadow_pass,
        ss_password,
        external_port,
        cover_domain,
        &[],
    )
}

pub fn build_server_config_with_fallbacks(
    _server_ip: &str,
    shadow_pass: &str,
    ss_password: &str,
    external_port: u16,
    cover_domain: &str,
    fallback_cover_domains: &[String],
) -> String {
    // Always bind to 0.0.0.0 — on NAT VPS the public IP is not assigned
    // to any local interface, so bind(public_ip) fails with EADDRNOTAVAIL.
    let listen_host = "0.0.0.0";

    let mut shadowtls_inbound = json!({
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
    });

    let mut handshake_for_server_name = serde_json::Map::new();
    for domain in fallback_cover_domains {
        if domain == cover_domain {
            continue;
        }

        handshake_for_server_name.insert(
            domain.clone(),
            json!({
                "server": domain,
                "server_port": 443
            }),
        );
    }

    if !handshake_for_server_name.is_empty() {
        shadowtls_inbound
            .as_object_mut()
            .expect("shadowtls inbound must be an object")
            .insert(
                "handshake_for_server_name".to_string(),
                Value::Object(handshake_for_server_name),
            );
    }

    let config = json!({
      "log": {
        "disabled": false,
        "level": "debug",
        "timestamp": true
      },
      "inbounds": [
        shadowtls_inbound,
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
            "server": "1.1.1.1"
          },
          {
            "tag": "direct-dns",
            "type": "udp",
            "server": "77.88.8.8"
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
        "default_domain_resolver": "direct-dns",
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
