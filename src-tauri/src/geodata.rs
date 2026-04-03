use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

/// Каждый rule-set — отдельный .srs файл из GitHub
const RULE_SETS: &[(&str, &str)] = &[
    (
        "geoip-ru.srs",
        "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-ru.srs",
    ),
    (
        "geosite-ru.srs",
        "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-ru.srs",
    ),
    (
        "geosite-category-ads-all.srs",
        "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-ads-all.srs",
    ),
];

/// Возвращает путь к папке геоданных внутри AppLocalData
pub fn geodata_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let dir = base.join("geodata");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Скачивает один файл по URL и сохраняет в указанный путь.
async fn download_file(url: &str, dest: &PathBuf) -> Result<u64, String> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| format!("HTTP error: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Download failed with status: {}",
            response.status()
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Read error: {}", e))?;

    let size = bytes.len() as u64;
    std::fs::write(dest, &bytes).map_err(|e| format!("Write error: {}", e))?;

    Ok(size)
}

/// Проверяет наличие всех rule-set файлов.
/// Если файлов нет — скачивает их.
pub async fn ensure_geodata(app: &AppHandle) -> Result<(), String> {
    let dir = geodata_dir(app)?;

    // Проверяем, все ли файлы на месте
    let all_exist = RULE_SETS.iter().all(|(name, _)| dir.join(name).exists());

    if all_exist {
        let _ = app.emit(
            "tunnel-log",
            "[GEODATA] All rule-set files found locally.".to_string(),
        );
        return Ok(());
    }

    let _ = app.emit(
        "tunnel-log",
        "[GEODATA] Downloading rule-set databases...".to_string(),
    );

    for (name, url) in RULE_SETS {
        let path = dir.join(name);
        if !path.exists() {
            let size = download_file(url, &path).await?;
            let _ = app.emit(
                "tunnel-log",
                format!(
                    "[GEODATA] {} downloaded ({:.1} KB)",
                    name,
                    size as f64 / 1024.0
                ),
            );
        }
    }

    let _ = app.emit(
        "tunnel-log",
        "[GEODATA] All rule-set databases ready.".to_string(),
    );

    Ok(())
}
