use std::path::PathBuf;
use std::time::Duration;

use reqwest::Client;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Copy)]
pub struct RemoteRuleSet {
    pub tag: &'static str,
    pub url: &'static str,
}

#[derive(Clone)]
pub struct LocalRuleSetAsset {
    pub tag: &'static str,
    pub path: PathBuf,
}

pub const ADS_RULE_SET_TAG: &str = "geosite-category-ads-all";
pub const GOOGLE_RULE_SET_TAG: &str = "geosite-google";

pub const DIRECT_ROUTE_RULE_SET_TAGS: &[&str] = &[
    "geoip-ru",
    "geosite-category-gov-ru",
    "geosite-yandex",
    "geosite-vk",
];

pub const CURATED_RU_DOMAIN_SUFFIXES: &[&str] = &[
    "2gis.ru",
    "alfabank.ru",
    "avito.ru",
    "cdek.ru",
    "gosuslugi.ru",
    "kinopoisk.ru",
    "mail.ru",
    "mos.ru",
    "nalog.gov.ru",
    "ok.ru",
    "ozon.ru",
    "pochta.ru",
    "rambler.ru",
    "sberbank.ru",
    "tbank.ru",
    "tinkoff.ru",
    "vtb.ru",
    "wildberries.ru",
];

pub const PROXY_PRIORITY_DOMAIN_SUFFIXES: &[&str] = &[
    "youtube.com",
    "youtu.be",
    "youtubei.googleapis.com",
    "ytimg.com",
    "googlevideo.com",
    "ggpht.com",
    "google.com",
    "gstatic.com",
    "googleapis.com",
    "googleusercontent.com",
    "withgoogle.com",
    "gemini.google.com",
    "ai.google.dev",
];

pub const REMOTE_RULE_SETS: &[RemoteRuleSet] = &[
    RemoteRuleSet {
        tag: "geoip-ru",
        url: "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-ru.srs",
    },
    RemoteRuleSet {
        tag: "geosite-category-gov-ru",
        url: "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-gov-ru.srs",
    },
    RemoteRuleSet {
        tag: "geosite-yandex",
        url: "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-yandex.srs",
    },
    RemoteRuleSet {
        tag: "geosite-vk",
        url: "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-vk.srs",
    },
    RemoteRuleSet {
        tag: ADS_RULE_SET_TAG,
        url: "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-ads-all.srs",
    },
    RemoteRuleSet {
        tag: GOOGLE_RULE_SET_TAG,
        url: "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-google.srs",
    },
];

const LOCAL_CLIENT_RULE_SET_TAGS: &[&str] = &[
    "geoip-ru",
    "geosite-category-gov-ru",
    "geosite-yandex",
    "geosite-vk",
    GOOGLE_RULE_SET_TAG,
];

fn local_rule_set_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("rule-set");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn local_rule_set_path(app: &AppHandle, tag: &str) -> Result<PathBuf, String> {
    Ok(local_rule_set_dir(app)?.join(format!("{tag}.srs")))
}

#[cfg(target_os = "android")]
fn bundled_rule_set_bytes(tag: &str) -> Option<&'static [u8]> {
    match tag {
        "geoip-ru" => Some(include_bytes!("../assets/rule-set/geoip-ru.srs")),
        "geosite-category-gov-ru" => Some(include_bytes!(
            "../assets/rule-set/geosite-category-gov-ru.srs"
        )),
        "geosite-yandex" => Some(include_bytes!("../assets/rule-set/geosite-yandex.srs")),
        "geosite-vk" => Some(include_bytes!("../assets/rule-set/geosite-vk.srs")),
        GOOGLE_RULE_SET_TAG => Some(include_bytes!("../assets/rule-set/geosite-google.srs")),
        _ => None,
    }
}

fn write_rule_set_file(path: &PathBuf, bytes: &[u8]) -> Result<(), String> {
    let temp_path = path.with_extension("tmp");
    std::fs::write(&temp_path, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&temp_path, path).map_err(|e| e.to_string())?;
    Ok(())
}

async fn download_rule_set_file(url: &str, path: &PathBuf) -> Result<(), String> {
    #[cfg(target_os = "android")]
    let timeout = Duration::from_secs(8);
    #[cfg(not(target_os = "android"))]
    let timeout = Duration::from_secs(20);

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(timeout)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    write_rule_set_file(path, &bytes)?;
    Ok(())
}

pub async fn ensure_local_client_rule_sets(
    app: &AppHandle,
) -> Result<Vec<LocalRuleSetAsset>, String> {
    let mut assets = Vec::new();

    for tag in LOCAL_CLIENT_RULE_SET_TAGS {
        let Some(rule_set) = REMOTE_RULE_SETS
            .iter()
            .find(|rule_set| rule_set.tag == *tag)
        else {
            continue;
        };

        let path = local_rule_set_path(app, tag)?;
        #[cfg(target_os = "android")]
        if let Some(bytes) = bundled_rule_set_bytes(rule_set.tag) {
            let _ = app.emit(
                "tunnel-log",
                format!(
                    "[SYSTEM] Restoring bundled local rule-set {} for Android runtime...",
                    rule_set.tag
                ),
            );

            match write_rule_set_file(&path, bytes) {
                Ok(()) => {
                    let _ = app.emit(
                        "tunnel-log",
                        format!(
                            "[SYSTEM] Bundled local rule-set {} restored at {}.",
                            rule_set.tag,
                            path.display()
                        ),
                    );
                }
                Err(error) => {
                    let _ = app.emit(
                        "tunnel-log",
                        format!(
                            "[WARN] Failed to restore bundled local rule-set {}: {}. Falling back to suffix-only rules for now.",
                            rule_set.tag, error
                        ),
                    );
                    continue;
                }
            }

            assets.push(LocalRuleSetAsset {
                tag: rule_set.tag,
                path,
            });
            continue;
        }

        if !path.exists() {
            let _ = app.emit(
                "tunnel-log",
                format!("[SYSTEM] Downloading local rule-set {}...", rule_set.tag),
            );

            match download_rule_set_file(rule_set.url, &path).await {
                Ok(()) => {
                    let _ = app.emit(
                        "tunnel-log",
                        format!(
                            "[SYSTEM] Local rule-set {} saved at {}.",
                            rule_set.tag,
                            path.display()
                        ),
                    );
                }
                Err(error) => {
                    let _ = app.emit(
                        "tunnel-log",
                        format!(
                            "[WARN] Failed to download local rule-set {}: {}. Falling back to suffix-only rules for now.",
                            rule_set.tag, error
                        ),
                    );
                    continue;
                }
            }
        }

        assets.push(LocalRuleSetAsset {
            tag: rule_set.tag,
            path,
        });
    }

    Ok(assets)
}
