#[cfg(target_os = "android")]
use base64::engine::general_purpose::STANDARD;
#[cfg(target_os = "android")]
use base64::Engine;
use serde_json::{json, Value};
use tauri::AppHandle;
#[cfg(not(target_os = "android"))]
use tauri_plugin_shell::ShellExt;

use crate::geodata::{
    LocalRuleSetAsset, CURATED_RU_DOMAIN_SUFFIXES, DIRECT_ROUTE_RULE_SET_TAGS, GOOGLE_RULE_SET_TAG,
    PROXY_PRIORITY_DOMAIN_SUFFIXES,
};

pub const INTERNAL_SS_PORT: u16 = 14433;

/// ShadowTLS cover domains.
///
/// Stable domains used for first deploys.
///
/// The set is intentionally conservative: only domains from the upstream
/// ShadowTLS TLS 1.3 compatibility list that have not recently produced
/// certificate mismatches in our real client tests stay in the primary pool.
const PRIMARY_COVER_DOMAINS: &[&str] = &[
    "weather-data.apple.com",
    "www.apple.com",
    "www.microsoft.com",
    "speed.cloudflare.com",
];

/// Additional ShadowTLS-friendly domains used only for SNI rotation.
///
/// These candidates come from the upstream ShadowTLS TLS 1.3 compatibility
/// list. Domains that timed out, returned mismatched certificates, or proved
/// unstable on real client paths are treated as legacy and excluded from new
/// selections.
const ROTATION_COVER_DOMAINS: &[&str] = &[
    "weather-data.apple.com",
    "www.apple.com",
    "www.microsoft.com",
    "www.cloudflare.com",
    "speed.cloudflare.com",
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

pub fn is_legacy_cover_domain_requiring_refresh(domain: &str) -> bool {
    matches!(
        domain,
        "cloud.tencent.com"
            | "publicassets.cdn-apple.com"
            | "coding.net"
            | "feishu.cn"
            | "upyun.com"
            | "mp.weixin.qq.com"
            | "douyin.com"
            | "toutiao.com"
            | "sns-video-hw.xhscdn.com"
            | "sns-img-qc.xhscdn.com"
            | "sns-video-qn.xhscdn.com"
            | "p9-dy.byteimg.com"
            | "p6-dy.byteimg.com"
            | "v6-dy-y.ixigua.com"
            | "hls3-akm.douyucdn.cn"
            | "captive.apple.com"
    )
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
    #[cfg(target_os = "android")]
    {
        let singbox_path = crate::resolve_singbox_path(app)?;
        let singbox_dir = std::path::Path::new(&singbox_path)
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let output = std::process::Command::new(&singbox_path)
            .current_dir(singbox_dir)
            .args(args)
            .output()
            .map_err(|e| format!("Failed to launch Android sing-box generator: {}", e))?;

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

        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }

    #[cfg(not(target_os = "android"))]
    {
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
}

#[cfg(target_os = "android")]
fn random_bytes(len: usize) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0_u8; len];
    getrandom::fill(&mut bytes)
        .map_err(|e| format!("Failed to generate Android transport secret bytes: {}", e))?;
    Ok(bytes)
}

fn is_hex_string(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

pub async fn generate_short_id(app: &AppHandle) -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        let _ = app;
        let bytes = random_bytes(8)?;
        Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    #[cfg(not(target_os = "android"))]
    {
        let short_id = run_singbox_generate(app, &["generate", "rand", "8", "--hex"]).await?;

        if !is_hex_string(&short_id) {
            return Err(format!("Generated short_id is not valid hex: {}", short_id));
        }

        Ok(short_id.to_ascii_lowercase())
    }
}

pub async fn generate_shadowtls_password(app: &AppHandle) -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        let _ = app;
        let bytes = random_bytes(16)?;
        Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    #[cfg(not(target_os = "android"))]
    {
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
}

pub async fn generate_ss_password(app: &AppHandle) -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        let _ = app;
        let bytes = random_bytes(16)?;
        Ok(STANDARD.encode(bytes))
    }

    #[cfg(not(target_os = "android"))]
    {
        run_singbox_generate(app, &["generate", "rand", "16", "--base64"]).await
    }
}

pub struct ServerConfigParams<'a> {
    pub master_shadow_pass: &'a str,
    pub ss_server_password: &'a str,
    pub master_ss_user_password: &'a str,
    pub external_port: u16,
    pub internal_ss_port: u16,
    pub routing_mode: &'a str,
    pub cover_domain: &'a str,
    pub fallback_cover_domains: &'a [String],
    pub issued_invites: &'a [crate::ssh::RemoteInviteRecord],
    pub warp: Option<&'a crate::ssh::RemoteWarpConfig>,
}

pub fn build_server_config_with_invites(params: ServerConfigParams<'_>) -> String {
    let listen_host = "0.0.0.0";

    let mut shadowtls_users = vec![json!({
      "name": "master",
      "password": params.master_shadow_pass
    })];
    shadowtls_users.extend(params.issued_invites.iter().map(|invite| {
        json!({
          "name": invite.id,
          "password": invite.shadow_pass
        })
    }));

    let mut shadowsocks_users = vec![json!({
      "name": "master",
      "password": params.master_ss_user_password
    })];
    shadowsocks_users.extend(params.issued_invites.iter().map(|invite| {
        json!({
          "name": invite.id,
          "password": invite.ss_user_password
        })
    }));

    let mut shadowtls_inbound = json!({
      "type": "shadowtls",
      "tag": "in-stls",
      "listen": listen_host,
      "listen_port": params.external_port,
      "version": 3,
      "users": shadowtls_users,
      "handshake": {
        "server": params.cover_domain,
        "server_port": 443
      },
      "detour": "ss-in"
    });

    let mut handshake_for_server_name = serde_json::Map::new();
    for domain in params.fallback_cover_domains {
        if domain == params.cover_domain {
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

    let outbounds = if params.routing_mode == "warp" {
        let warp = params
            .warp
            .expect("warp config must be present when routing_mode is warp");
        let mut warp_local_addresses = vec![warp.address_v4.clone()];
        if !warp.address_v6.trim().is_empty() {
            warp_local_addresses.push(warp.address_v6.clone());
        }

        json!([
          {
            "type": "direct",
            "tag": "direct"
          },
          {
            "type": "wireguard",
            "tag": "warp",
            "local_address": warp_local_addresses,
            "private_key": warp.private_key,
            "server": warp.endpoint,
            "server_port": warp.endpoint_port,
            "peer_public_key": warp.peer_public_key,
            "mtu": 1280
          }
        ])
    } else {
        json!([
          {
            "type": "direct",
            "tag": "direct"
          }
        ])
    };

    let final_outbound = if params.routing_mode == "warp" {
        "warp"
    } else {
        "direct"
    };

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
          "listen_port": params.internal_ss_port,
          "method": "2022-blake3-aes-128-gcm",
          "password": params.ss_server_password,
          "users": shadowsocks_users
        }
      ],
      "outbounds": outbounds,
      "route": {
        "rules": [
          {
            "outbound": final_outbound
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
    local_rule_sets: &[LocalRuleSetAsset],
) -> String {
    let local_rule_set_entries = local_rule_sets
        .iter()
        .map(|rule_set| {
            json!({
              "tag": rule_set.tag,
              "type": "local",
              "format": "binary",
              "path": rule_set.path.to_string_lossy().to_string()
            })
        })
        .collect::<Vec<_>>();
    let available_direct_rule_tags = local_rule_sets
        .iter()
        .filter(|rule_set| DIRECT_ROUTE_RULE_SET_TAGS.contains(&rule_set.tag))
        .map(|rule_set| rule_set.tag)
        .collect::<Vec<_>>();
    let available_direct_dns_rule_tags = available_direct_rule_tags
        .iter()
        .copied()
        .filter(|tag| !tag.starts_with("geoip-"))
        .collect::<Vec<_>>();
    let google_rule_set_available = local_rule_sets
        .iter()
        .any(|rule_set| rule_set.tag == GOOGLE_RULE_SET_TAG);

    let mut tun_inbound = json!({
      "type": "tun",
      "tag": "tun-in",
      "address": ["172.19.0.1/30"],
      "auto_route": true,
      "strict_route": true,
      "stack": "system",
      "endpoint_independent_nat": true
    });

    if !cfg!(target_os = "macos") {
        tun_inbound["interface_name"] = json!("tun0");
    }

    let mut config = json!({
      "log": {
        "level": "info"
      },

      "dns": {
        "servers": [
          {
            "type": "fakeip",
            "tag": "fakeip-dns",
            "inet4_range": "198.18.0.0/15",
            "inet6_range": "fc00::/18"
          },
          {
            "type": "udp",
            "tag": "remote-dns",
            "server": "8.8.8.8",
            "server_port": 53,
            "detour": "proxy"
          },
          {
            "type": "local",
            "tag": "local-dns",
            "prefer_go": true
          }
        ],
        "rules": [
          {
            "domain_suffix": PROXY_PRIORITY_DOMAIN_SUFFIXES,
            "server": "fakeip-dns"
          },
          {
            "domain_suffix": CURATED_RU_DOMAIN_SUFFIXES,
            "server": "local-dns"
          }
        ],
        "final": "remote-dns",
        "strategy": "ipv4_only"
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
            "inbound": "tun-in",
            "action": "sniff",
            "timeout": "1s"
          },
          {
            "inbound": "tun-in",
            "protocol": "dns",
            "action": "hijack-dns"
          },
          {
            "network": "udp",
            "port": 443,
            "action": "reject",
            "method": "default"
          },
          {
            "domain_suffix": PROXY_PRIORITY_DOMAIN_SUFFIXES,
            "action": "route",
            "outbound": "proxy"
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
          }
        ],
        "final": "proxy",
        "default_domain_resolver": "remote-dns",
        "auto_detect_interface": true,
        "rule_set": local_rule_set_entries
      }
    });

    if google_rule_set_available {
        config["dns"]["rules"]
            .as_array_mut()
            .expect("dns rules must be an array")
            .insert(
                0,
                json!({
                  "rule_set": [GOOGLE_RULE_SET_TAG],
                  "server": "fakeip-dns"
                }),
            );

        config["route"]["rules"]
            .as_array_mut()
            .expect("route rules must be an array")
            .insert(
                3,
                json!({
                  "rule_set": [GOOGLE_RULE_SET_TAG],
                  "action": "route",
                  "outbound": "proxy"
                }),
            );
    }

    if !available_direct_dns_rule_tags.is_empty() {
        config["dns"]["rules"]
            .as_array_mut()
            .expect("dns rules must be an array")
            .push(json!({
              "rule_set": available_direct_dns_rule_tags,
              "server": "local-dns"
            }));
    }

    if !available_direct_rule_tags.is_empty() {
        config["route"]["rules"]
            .as_array_mut()
            .expect("route rules must be an array")
            .push(json!({
              "rule_set": available_direct_rule_tags,
              "action": "route",
              "outbound": "direct"
            }));
    }

    serde_json::to_string_pretty(&config).unwrap()
}
