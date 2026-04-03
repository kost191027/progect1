use ssh2::Session;
use std::io::Read;
use std::io::Write;
use std::net::TcpStream;
use tauri::{AppHandle, Emitter};

#[tauri::command]
pub async fn deploy_server(
    app: AppHandle,
    host: String,
    user: String,
    pass: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _ = app.emit("tunnel-log", format!("--- [SSH] Connecting to {}:22 ---", host));

        let tcp = TcpStream::connect(format!("{}:22", host)).map_err(|e| format!("Failed to connect: {}", e))?;
        let mut sess = Session::new().unwrap();
        sess.set_tcp_stream(tcp);
        sess.handshake().map_err(|e| format!("SSH handshake failed: {}", e))?;

        sess.userauth_password(&user, &pass).map_err(|e| format!("Auth failed: {}", e))?;

        if !sess.authenticated() {
            return Err("Authentication failed".to_string());
        }

        let _ = app.emit("tunnel-log", "[SSH] Authenticated successfully.".to_string());

        let deploy_script = include_str!("../scripts/deploy.sh");

        let mut channel = sess.channel_session().map_err(|e| e.to_string())?;
        
        let _ = app.emit("tunnel-log", "[SSH] Executing remote fast-deploy script...".to_string());
        
        channel.exec("bash -s").map_err(|e| e.to_string())?;
        
        channel.write_all(deploy_script.as_bytes()).map_err(|e| e.to_string())?;
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
            let _ = app.emit("tunnel-log", "[SSH] Deployment finished successfully!".to_string());
            Ok(())
        } else {
            let _ = app.emit("tunnel-log", format!("[SSH ERROR] Deployment failed with code: {}", exit_status));
            Err(format!("Deployment script exited with {}", exit_status))
        }
    })
    .await
    .unwrap()
}
