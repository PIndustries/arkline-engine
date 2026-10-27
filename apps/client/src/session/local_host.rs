//! Lifetime ownership and loopback startup for a locally spawned host.

#[cfg(not(target_arch = "wasm32"))]
use super::*;

/// Only the process that spawned a local host owns its lifetime.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct LocalServer(Child);
#[cfg(target_arch = "wasm32")]
pub(crate) struct LocalServer;
#[cfg(not(target_arch = "wasm32"))]
impl Drop for LocalServer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn start_local_server(dir: &std::path::Path) -> Result<(LocalServer, String), String> {
    fs::create_dir_all(dir).map_err(|e| format!("Cannot create preview storage: {e}"))?;
    let executable = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("Cannot locate client directory")?
        .join(if cfg!(windows) {
            "arkline-server.exe"
        } else {
            "arkline-server"
        });
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("server.log"))
        .map_err(|e| e.to_string())?;
    let child = Command::new(executable)
        .arg("--bind")
        .arg("127.0.0.1:0")
        .arg("--db")
        .arg(dir.join("world.sqlite"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|e| format!("Cannot start bundled universe server: {e}"))?;
    let mut guard = LocalServer(child);
    let stdout = guard
        .0
        .stdout
        .take()
        .ok_or("Cannot read local server startup")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(address) = line.strip_prefix("Arkline server listening on ") {
                let _ = tx.send(address.to_string());
            }
        }
    });
    let address = rx.recv_timeout(Duration::from_secs(10)).map_err(|_| {
        format!(
            "Local universe could not start. See {}",
            dir.join("server.log").display()
        )
    })?;
    let socket: std::net::SocketAddr = address
        .parse()
        .map_err(|_| "Invalid local server address")?;
    if !socket.ip().is_loopback() || socket.port() == 0 {
        return Err("Local server did not bind safely".into());
    }
    Ok((guard, address))
}
