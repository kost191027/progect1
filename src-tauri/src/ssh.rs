use ssh2::Session;
use std::io::Read;
use std::io::Write;
use std::net::TcpStream;
use tauri::{AppHandle, Emitter, Manager};

const EXTERNAL_PORT_CANDIDATES: [u16; 4] = [443, 5443, 7443, 9443];

fn run_remote_command(sess: &Session, command: &str) -> Result<(String, i32), String> {
    let mut channel = sess.channel_session().map_err(|e| e.to_string())?;
    channel.exec(command).map_err(|e| e.to_string())?;

    let mut stdout = String::new();
    channel
        .read_to_string(&mut stdout)
        .map_err(|e| e.to_string())?;

    channel.wait_close().map_err(|e| e.to_string())?;
    let exit_status = channel.exit_status().map_err(|e| e.to_string())?;

    Ok((stdout, exit_status))
}

fn select_remote_external_port(sess: &Session) -> Result<u16, String> {
    let candidates = EXTERNAL_PORT_CANDIDATES
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(" ");

    let command = format!(
        "bash -lc 'for port in {}; do if ! ss -Htanl \"( sport = :$port )\" | grep -q .; then echo \"$port\"; exit 0; fi; done; exit 1'",
        candidates
    );

    let (stdout, exit_status) = run_remote_command(sess, &command)?;

    if exit_status != 0 {
        return Err(format!(
            "No free external ports found in candidate list: {}",
            candidates
        ));
    }

    let selected_port = stdout
        .lines()
        .find_map(|line| line.trim().parse::<u16>().ok())
        .ok_or_else(|| {
            format!(
                "Failed to parse remote selected port from output: {}",
                stdout
            )
        })?;

    Ok(selected_port)
}

#[tauri::command]
pub async fn deploy_server(
    app: AppHandle,
    host: String,
    user: String,
    pass: String,
) -> Result<(), String> {
    let _ = app.emit(
        "tunnel-log",
        "[SYSTEM] Generating unique crypto-keys via sing-box...".to_string(),
    );

    // 1. Generate keys asynchronously using local sing-box
    let reality_keys = crate::generator::generate_reality_keypair(&app)
        .await
        .map_err(|e| format!("Key generation error: {}", e))?;
    let short_id = crate::generator::generate_short_id(&app)
        .await
        .map_err(|e| format!("Short-ID error: {}", e))?;
    let uuid = crate::generator::generate_uuid(&app)
        .await
        .map_err(|e| format!("UUID error: {}", e))?;
    let shadow_pass = crate::generator::generate_short_id(&app)
        .await
        .unwrap_or_else(|_| "shadow_secure_pass".to_string());

    // Получаем путь к AppData
    let local_data = app
        .path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());

    // 4. Perform SSH upload and deployment (Blocking)
    tauri::async_runtime::spawn_blocking(move || {
        let _ = app.emit(
            "tunnel-log",
            format!("--- [SSH] Connecting to {}:22 ---", host),
        );

        let tcp = TcpStream::connect(format!("{}:22", host))
            .map_err(|e| format!("Failed to connect: {}", e))?;
        let mut sess = Session::new().unwrap();
        sess.set_tcp_stream(tcp);
        sess.handshake()
            .map_err(|e| format!("SSH handshake failed: {}", e))?;

        sess.userauth_password(&user, &pass)
            .map_err(|e| format!("Auth failed: {}", e))?;

        if !sess.authenticated() {
            return Err("Authentication failed".to_string());
        }

        let _ = app.emit(
            "tunnel-log",
            "[SSH] Authenticated successfully.".to_string(),
        );

        let _ = app.emit(
            "tunnel-log",
            "[SSH] Running remote pre-flight checks for external port...".to_string(),
        );

        let external_port = select_remote_external_port(&sess)?;
        if external_port == 443 {
            let _ = app.emit(
                "tunnel-log",
                "[SSH] Port 443 is available on remote host.".to_string(),
            );
        } else {
            let _ = app.emit(
                "tunnel-log",
                format!(
                    "[SSH WARN] Port 443 is busy. Falling back to external port {}.",
                    external_port
                ),
            );
        }

        let server_cfg = crate::generator::build_server_config(
            &reality_keys,
            &short_id,
            &uuid,
            &shadow_pass,
            external_port,
        );
        let client_cfg = crate::generator::build_client_config(
            &host,
            &reality_keys,
            &short_id,
            &uuid,
            &shadow_pass,
            external_port,
        );

        let deploy_script = include_str!("../scripts/deploy.sh");

        // Dynamically inject the generated server JSON directly before the bash script starts
        let injected_script = format!(
            r#"#!/bin/bash
mkdir -p /opt/rkn
cat << 'CONFIGEOF' > /opt/rkn/config.json
{}
CONFIGEOF

{}
"#,
            server_cfg, deploy_script
        );

        let mut channel = sess.channel_session().map_err(|e| e.to_string())?;

        let _ = app.emit(
            "tunnel-log",
            "[SSH] Executing remote fast-deploy script...".to_string(),
        );

        channel.exec("bash -s").map_err(|e| e.to_string())?;

        channel
            .write_all(injected_script.as_bytes())
            .map_err(|e| e.to_string())?;
        channel.send_eof().map_err(|e| e.to_string())?;

        let mut buffer = [0; 512];
        loop {
            match channel.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    let output = String::from_utf8_lossy(&buffer[..n]);
                    // Clean line breaks for nicer UI
                    for line in output.lines() {
                        if !line.trim().is_empty() {
                            let _ = app.emit("tunnel-log", format!("[SERVER] {}", line));
                        }
                    }
                }
                Err(_) => break,
            }
        }

        channel.wait_close().unwrap();
        let exit_status = channel.exit_status().unwrap();

        if exit_status == 0 {
            std::fs::create_dir_all(&local_data).map_err(|e| e.to_string())?;
            let client_cfg_path = local_data.join("client_config.json");
            std::fs::write(&client_cfg_path, &client_cfg).map_err(|e| e.to_string())?;

            let _ = app.emit(
                "tunnel-log",
                format!(
                    "[SYSTEM] Client config safely generated at: {:?}",
                    client_cfg_path
                ),
            );
            let _ = app.emit(
                "tunnel-log",
                format!(
                    "[SSH] Deployment finished successfully! External port: {}",
                    external_port
                ),
            );
            Ok(())
        } else {
            let _ = app.emit(
                "tunnel-log",
                format!("[SSH ERROR] Deployment failed with code: {}", exit_status),
            );
            Err(format!("Deployment script exited with {}", exit_status))
        }
    })
    .await
    .unwrap()
}
