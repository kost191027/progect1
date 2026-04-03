use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

const GEOIP_URL: &str = "https://github.com/SagerNet/sing-geoip/releases/latest/download/geoip.db";
const GEOSITE_URL: &str =
    "https://github.com/SagerNet/sing-geosite/releases/latest/download/geosite.db";

/// Возвращает путь к папке геоданных внутри AppLocalData
fn geodata_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let dir = base.join("geodata");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Скачивает один файл по URL и сохраняет в указанный путь.
/// Возвращает размер файла в байтах.
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

/// Проверяет наличие geoip.db и geosite.db.
/// Если файлов нет — скачивает их.
/// Вызывается перед стартом тоннеля.
pub async fn ensure_geodata(app: &AppHandle) -> Result<(), String> {
    let dir = geodata_dir(app)?;

    let geoip_path = dir.join("geoip.db");
    let geosite_path = dir.join("geosite.db");

    if geoip_path.exists() && geosite_path.exists() {
        let _ = app.emit(
            "tunnel-log",
            "[GEODATA] Routing databases found locally.".to_string(),
        );
        return Ok(());
    }

    let _ = app.emit(
        "tunnel-log",
        "[GEODATA] Downloading fresh GeoIP & GeoSite databases...".to_string(),
    );

    if !geoip_path.exists() {
        let size = download_file(GEOIP_URL, &geoip_path).await?;
        let _ = app.emit(
            "tunnel-log",
            format!(
                "[GEODATA] geoip.db downloaded ({:.1} MB)",
                size as f64 / 1_048_576.0
            ),
        );
    }

    if !geosite_path.exists() {
        let size = download_file(GEOSITE_URL, &geosite_path).await?;
        let _ = app.emit(
            "tunnel-log",
            format!(
                "[GEODATA] geosite.db downloaded ({:.1} MB)",
                size as f64 / 1_048_576.0
            ),
        );
    }

    let _ = app.emit(
        "tunnel-log",
        "[GEODATA] Routing databases ready.".to_string(),
    );

    Ok(())
}
