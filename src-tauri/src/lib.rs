use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_shell::ShellExt;
use tokio::io::{AsyncBufReadExt, BufReader};

mod generator;
mod geodata;
mod ssh;

struct AppState {
    /// PID процесса sing-box, запущенного root-правами через osascript
    singbox_pid: Mutex<Option<u32>>,
}

/// Находит абсолютный путь до sidecar-бинарника `sing-box`
fn resolve_singbox_path() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;

    // В dev-режиме: target/debug/progect-1 -> ищем рядом
    // В production: MacOS/progect-1 -> ищем рядом
    let dir = exe.parent().ok_or("Cannot resolve binary directory")?;

    // Tauri sidecar лежит рядом с бинарником как sing-box-{arch}
    // Определяем суффикс архитектуры
    let arch_suffix = if cfg!(target_arch = "x86_64") {
        "x86_64-apple-darwin"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64-apple-darwin"
    } else {
        return Err("Unsupported architecture".to_string());
    };

    let sidecar_name = format!("sing-box-{}", arch_suffix);
    let sidecar_path = dir.join(&sidecar_name);

    if sidecar_path.exists() {
        Ok(sidecar_path.to_string_lossy().to_string())
    } else {
        // Fallback: просто попробуем sing-box в PATH
        Ok("sing-box".to_string())
    }
}

#[tauri::command]
async fn start_tunnel(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    // Проверяем, не запущен ли уже
    {
        let guard = state.singbox_pid.lock().unwrap();
        if guard.is_some() {
            return Err("Tunnel is already running".to_string());
        }
    }

    let _ = app.emit("tunnel-log", "[SYSTEM] Resolving core binary path...");

    // Скачиваем/проверяем геоданные перед запуском
    geodata::ensure_geodata(&app)
        .await
        .map_err(|e| format!("Geodata error: {}", e))?;

    let singbox_path = resolve_singbox_path()?;

    // Получаем путь к конфигу из AppLocalData
    let local_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let config_path = local_data.join("client_config.json");

    if !config_path.exists() {
        return Err("Client config not found. Please deploy a server first.".to_string());
    }

    let config_str = config_path.to_string_lossy().to_string();

    let _ = app.emit(
        "tunnel-log",
        "[SYSTEM] Requesting administrator privileges...".to_string(),
    );

    // Формируем shell-команду для запуска sing-box от рута
    // sing-box пишет логи в файл, а мы будем их читать асинхронно
    let log_path = "/tmp/rkn-tun.log";
    let shell_cmd = format!(
        "{} run -c '{}' > {} 2>&1 & echo $!",
        singbox_path, config_str, log_path
    );

    // Экранируем одинарные кавычки для AppleScript
    let escaped_cmd = shell_cmd.replace('\\', "\\\\").replace('"', "\\\"");

    let osascript_arg = format!(
        "do shell script \"{}\" with administrator privileges",
        escaped_cmd
    );

    // Запускаем osascript через Tauri shell (вызовет нативное окно macOS «Введите пароль»)
    let sidecar = app
        .shell()
        .command("osascript")
        .args(["-e", &osascript_arg]);

    let output = sidecar
        .output()
        .await
        .map_err(|e| format!("Failed to execute osascript: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Если юзер нажал "Cancel" — это не ошибка, а отмена
        if stderr.contains("User canceled") || stderr.contains("-128") {
            let _ = app.emit(
                "tunnel-log",
                "[SYSTEM] Administrator access was cancelled by user.",
            );
            return Err("User cancelled admin prompt".to_string());
        }
        return Err(format!("osascript error: {}", stderr));
    }

    // Ответ osascript — PID root-процесса sing-box
    let pid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let pid: u32 = pid_str
        .parse()
        .map_err(|_| format!("Failed to parse PID from: '{}'", pid_str))?;

    let _ = app.emit(
        "tunnel-log",
        format!("[SYSTEM] Core process started with PID {} (root)", pid),
    );

    // Сохраняем PID
    {
        let mut guard = state.singbox_pid.lock().unwrap();
        *guard = Some(pid);
    }

    // Запускаем асинхронное чтение логов (аналог tail -f)
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        // Даём sing-box секунду на создание файла
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let file = match tokio::fs::File::open(log_path).await {
            Ok(f) => f,
            Err(e) => {
                let _ = app_clone.emit(
                    "tunnel-log",
                    format!("[WARN] Could not open log file: {}", e),
                );
                return;
            }
        };

        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if !line.trim().is_empty() {
                        let _ = app_clone.emit("tunnel-log", format!("[CORE] {}", line));
                    }
                }
                Ok(None) => {
                    // EOF — файл не вырос, ждём немного и перечитаем
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
                Err(_) => break,
            }
        }
    });

    let _ = app.emit(
        "tunnel-log",
        "[SYSTEM] TUN adapter initialized. Routing active.".to_string(),
    );

    Ok(())
}

#[tauri::command]
async fn stop_tunnel(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let pid = {
        let mut guard = state.singbox_pid.lock().unwrap();
        guard.take()
    };

    match pid {
        Some(pid) => {
            let _ = app.emit(
                "tunnel-log",
                format!("[SYSTEM] Stopping core process (PID {})...", pid),
            );

            // Убиваем root-процесс через osascript
            let kill_cmd = format!("kill -9 {}", pid);
            let osascript_arg = format!(
                "do shell script \"{}\" with administrator privileges",
                kill_cmd
            );

            let output = app
                .shell()
                .command("osascript")
                .args(["-e", &osascript_arg])
                .output()
                .await
                .map_err(|e| e.to_string())?;

            if output.status.success() {
                let _ = app.emit(
                    "tunnel-log",
                    "[SYSTEM] Core process terminated. Routing disabled.".to_string(),
                );
            } else {
                // Если не требуются права (процесс уже умер), просто логируем
                let _ = app.emit(
                    "tunnel-log",
                    "[WARN] Process may have already exited.".to_string(),
                );
            }

            // Чистим лог-файл
            let _ = std::fs::remove_file("/tmp/rkn-tun.log");

            Ok(())
        }
        None => {
            let _ = app.emit(
                "tunnel-log",
                "[SYSTEM] No active tunnel to stop.".to_string(),
            );
            Ok(())
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            singbox_pid: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            start_tunnel,
            stop_tunnel,
            ssh::deploy_server
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
