//! One request in flight. Failed mutations are never retried automatically.
use anyhow::{bail, Context, Result};
use arkline_protocol::{Request, Response, PROTOCOL_VERSION};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};
const LIMIT: usize = 16 * 1024 * 1024;
pub enum Transport {
    Tcp(BufReader<DeadlineSocket>),
    Ws(Box<WebSocket<MaybeTlsStream<DeadlineSocket>>>),
}
pub struct DeadlineSocket {
    socket: TcpStream,
    deadline: Instant,
}
impl DeadlineSocket {
    fn new(socket: TcpStream) -> Self {
        Self {
            socket,
            deadline: Instant::now() + Duration::from_secs(4),
        }
    }
    fn reset(&mut self) {
        self.deadline = Instant::now() + Duration::from_secs(4);
    }
    fn remaining(&self) -> std::io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|v| !v.is_zero())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut, "response deadline"))
    }
}
impl Read for DeadlineSocket {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.socket.set_read_timeout(Some(self.remaining()?))?;
        self.socket.read(bytes)
    }
}
impl Write for DeadlineSocket {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.socket.set_write_timeout(Some(self.remaining()?))?;
        self.socket.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.socket.flush()
    }
}
fn bounded_line(reader: &mut impl BufRead) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((LIMIT + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if bytes.len() > LIMIT {
        bail!("response exceeds size limit");
    }
    if bytes.last() != Some(&b'\n') {
        bail!("server closed an incomplete response");
    }
    Ok(bytes)
}
fn connect_socket(host: &str, port: u16, local_only: bool) -> Result<TcpStream> {
    use std::net::ToSocketAddrs;
    let addresses: Vec<_> = (host, port).to_socket_addrs()?.collect();
    if addresses.is_empty() {
        bail!("endpoint has no address");
    }
    if local_only && addresses.iter().any(|a| !a.ip().is_loopback()) {
        bail!("plaintext transport is restricted to loopback; use wss:// for remote relays");
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    let mut last = None;
    for address in addresses {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            break;
        }
        match TcpStream::connect_timeout(&address, left) {
            Ok(tcp) => {
                tcp.set_read_timeout(Some(Duration::from_secs(4)))?;
                tcp.set_write_timeout(Some(Duration::from_secs(4)))?;
                return Ok(tcp);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last.context("connection deadline expired")?.into())
}
impl Transport {
    pub fn open(endpoint: &str) -> Result<Self> {
        if endpoint.starts_with("wss://") || endpoint.starts_with("ws://") {
            use tungstenite::client::IntoClientRequest;
            let request = endpoint.into_client_request()?;
            let host = request.uri().host().context("endpoint has no host")?;
            let secure = request.uri().scheme_str() == Some("wss");
            let port = request
                .uri()
                .port_u16()
                .unwrap_or(if secure { 443 } else { 80 });
            let tcp = connect_socket(host.trim_matches(['[', ']']), port, !secure)?;
            let config = tungstenite::protocol::WebSocketConfig::default()
                .max_message_size(Some(LIMIT))
                .max_frame_size(Some(LIMIT));
            // Set socket deadlines before TLS and HTTP handshakes. Do not follow
            // redirects with saved credentials or silently downgrade transport.
            let roots =
                rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let tls = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_no_client_auth();
            let (ws, _) = tungstenite::client_tls_with_config(
                request,
                DeadlineSocket::new(tcp),
                Some(config),
                Some(tungstenite::Connector::Rustls(std::sync::Arc::new(tls))),
            )?;
            Ok(Self::Ws(Box::new(ws)))
        } else {
            let (host, port) = endpoint
                .rsplit_once(':')
                .context("TCP endpoint needs HOST:PORT")?;
            let tcp = connect_socket(
                host.trim_matches(['[', ']']),
                port.parse().context("invalid port")?,
                true,
            )?;
            Ok(Self::Tcp(BufReader::new(DeadlineSocket::new(tcp))))
        }
    }
    pub fn request(&mut self, request: &Request) -> Result<Response> {
        let json = serde_json::to_string(request)?;
        let bytes = match self {
            Self::Tcp(t) => {
                t.get_mut().reset();
                t.get_mut().write_all(json.as_bytes())?;
                t.get_mut().write_all(b"\n")?;
                bounded_line(t)?
            }
            Self::Ws(ws) => {
                match ws.get_mut() {
                    MaybeTlsStream::Plain(t) => t.reset(),
                    MaybeTlsStream::Rustls(t) => t.sock.reset(),
                    _ => bail!("unsupported TLS transport"),
                }
                ws.send(Message::Text(json.into()))?;
                let mut result = None;
                for _ in 0..32 {
                    match ws.read()? {
                        Message::Text(t) => {
                            result = Some(t.as_bytes().to_vec());
                            break;
                        }
                        Message::Ping(_) => ws.flush()?,
                        Message::Pong(_) => {}
                        _ => bail!("unexpected WebSocket response"),
                    }
                }
                result.context("too many control frames")?
            }
        };
        serde_json::from_slice(&bytes).context("invalid server response")
    }
}
pub struct Login {
    pub endpoint: String,
    pub pilot: String,
    pub path: PathBuf,
    pending_token: Mutex<Option<String>>,
}
impl Login {
    pub fn new(endpoint: String, pilot: String, path: Option<PathBuf>) -> Result<Self> {
        if pilot.is_empty()
            || pilot.len() > 24
            || !pilot
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            bail!("pilot must be 1-24 letters, digits, _ or -");
        }
        let digest = hex::encode(Sha256::digest(endpoint.as_bytes()));
        let path = path.unwrap_or_else(|| {
            PathBuf::from(format!(
                "data/tui-{}-{pilot}.credentials.json",
                &digest[..24]
            ))
        });
        Ok(Self {
            endpoint,
            pilot,
            path,
            pending_token: Mutex::new(None),
        })
    }
    pub fn connect(&self) -> Result<(Transport, Response)> {
        // Keep an issued token alive across failed saves and explicit reconnects.
        // Serialize connects so this Login cannot register twice concurrently.
        let mut pending = self
            .pending_token
            .lock()
            .map_err(|_| anyhow::anyhow!("credential state unavailable"))?;
        if let Some(token) = pending.as_ref() {
            self.persist(token, None).context("Credentials remain in memory. Keep this client open, resolve the storage conflict, then press R to retry saving and resume")?;
            *pending = None;
        }
        let token = match fs::read(&self.path) {
            Ok(bytes) => {
                let saved: super::Credentials = serde_json::from_slice(&bytes)?;
                if saved.server != self.endpoint || saved.pilot != self.pilot {
                    bail!("credential endpoint/pilot mismatch");
                }
                Some(saved.resume_token)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(e)
                    .context("credential storage preflight failed; no new identity requested")
            }
        };
        // Probe the actual destination directory before creating a remote identity.
        // The staged file is private and never replaces an existing credential.
        let staged = if token.is_none() {
            Some(StagedCredential::new(&self.path)?)
        } else {
            None
        };
        let mut transport = Transport::open(&self.endpoint)?;
        let mut response = transport.request(&Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            supports_docked_traversal_v2: false,
            supports_room_controls: false,
            world_layout_version: 0,
            supports_fleet_hangar: false,
            supports_pioneer_interior: false,
            pilot: self.pilot.clone(),
            resume_token: token,
        })?;
        if !response.ok {
            bail!("{}", response.message);
        }
        if let Some(token) = response.resume_token.take() {
            *pending = Some(token);
            self.persist(pending.as_ref().unwrap(), staged).context("Credentials remain in memory. Keep this client open, resolve the storage conflict, then press R to retry saving and resume")?;
            *pending = None;
        }
        Ok((transport, response))
    }
    fn persist(&self, token: &str, staged: Option<StagedCredential>) -> Result<()> {
        if let Ok(bytes) = fs::read(&self.path) {
            let saved: super::Credentials = serde_json::from_slice(&bytes)
                .context("existing credentials cannot be replaced")?;
            if saved.server == self.endpoint
                && saved.pilot == self.pilot
                && saved.resume_token == token
            {
                return sync_credential_directory(&self.path);
            }
            bail!("existing credentials cannot be replaced");
        }
        let mut staged = match staged {
            Some(v) => v,
            None => StagedCredential::new(&self.path)?,
        };
        let saved = super::Credentials {
            server: self.endpoint.clone(),
            pilot: self.pilot.clone(),
            resume_token: token.to_owned(),
        };
        staged.file.write_all(&serde_json::to_vec(&saved)?)?;
        staged.file.sync_all()?;
        // A same-directory hard link publishes the complete file atomically and
        // fails if any competing process created the destination meanwhile.
        fs::hard_link(&staged.path, &self.path)
            .context("cannot publish credentials without replacing another file")?;
        sync_credential_directory(&self.path)
    }
}
fn sync_credential_directory(path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        fs::File::open(parent)?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
struct StagedCredential {
    path: PathBuf,
    file: fs::File,
}
impl StagedCredential {
    fn new(destination: &std::path::Path) -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        fs::create_dir_all(parent)
            .context("credential storage preflight failed; no new identity requested")?;
        for _ in 0..100 {
            let path = parent.join(format!(
                ".arkline-credential-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => {
                    let staged = Self { path, file };
                    staged.file.sync_all().context(
                        "credential storage preflight failed; no new identity requested",
                    )?;
                    return Ok(staged);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(e)
                        .context("credential storage preflight failed; no new identity requested")
                }
            }
        }
        bail!("credential storage preflight failed; no new identity requested")
    }
}
impl Drop for StagedCredential {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn test_directory(label: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "arkline-credential-test-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }
    #[test]
    fn failed_preflight_never_contacts_registration_server() {
        let dir = test_directory("preflight");
        let blocked = dir.join("not-a-directory");
        fs::write(&blocked, b"unchanged").unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let login = Login::new(
            listener.local_addr().unwrap().to_string(),
            "Pilot".into(),
            Some(blocked.join("credentials.json")),
        )
        .unwrap();
        assert!(login
            .connect()
            .err()
            .unwrap()
            .to_string()
            .contains("preflight"));
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert_eq!(fs::read(blocked).unwrap(), b"unchanged");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn save_conflict_retains_token_and_retry_resumes_without_overwrite() {
        let dir = test_directory("retry");
        let path = dir.join("credentials.json");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap().to_string();
        let competing_path = path.clone();
        let server = std::thread::spawn(move || {
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let request: Request = serde_json::from_slice(
                    &bounded_line(&mut BufReader::new(stream.try_clone().unwrap())).unwrap(),
                )
                .unwrap();
                match request {
                    Request::Connect { resume_token, .. } => {
                        if attempt == 0 {
                            assert!(resume_token.is_none());
                        } else {
                            assert_eq!(resume_token.as_deref(), Some("test-issued-token"));
                        }
                    }
                    _ => panic!("expected connect"),
                }
                if attempt == 0 {
                    fs::write(&competing_path, b"other process file").unwrap();
                }
                let response = Response {
                    auth: None,
                    physical_cargo: None,
                    ok: true,
                    message: "connected".into(),
                    resume_token: (attempt == 0).then(|| "test-issued-token".into()),
                    assets: None,
                    snapshot: None,
                    ledger: None,
                };
                writeln!(stream, "{}", serde_json::to_string(&response).unwrap()).unwrap();
            }
        });
        let login = Login::new(endpoint, "Pilot".into(), Some(path.clone())).unwrap();
        let failure = login.connect().err().unwrap().to_string();
        assert!(failure.contains("remain in memory"));
        assert!(!failure.contains("test-issued-token"));
        assert_eq!(fs::read(&path).unwrap(), b"other process file");
        // Repeated failures never reconnect or overwrite the conflicting file.
        assert!(login.connect().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"other process file");
        fs::remove_file(&path).unwrap();
        let (_, response) = login.connect().unwrap();
        assert!(response.resume_token.is_none());
        assert!(login.pending_token.lock().unwrap().is_none());
        let saved: super::super::Credentials =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.resume_token, "test-issued-token");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        server.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn endpoint_credentials_are_distinct() {
        let a = Login::new("wss://relay.example/ws".into(), "Pilot".into(), None).unwrap();
        let b = Login::new("wss://relay-example/ws".into(), "Pilot".into(), None).unwrap();
        assert_ne!(a.path, b.path);
        assert!(Login::new("x".into(), "../bad".into(), None).is_err());
    }
    #[test]
    fn truncated_json_is_not_a_response() {
        assert!(bounded_line(&mut std::io::Cursor::new(b"{}")).is_err());
    }
    #[test]
    fn reads_fragmented_response_without_consuming_next() {
        let mut read =
            BufReader::with_capacity(2, std::io::Cursor::new(b"{\"ok\":true}\nsecond\n"));
        assert_eq!(bounded_line(&mut read).unwrap(), b"{\"ok\":true}\n");
        assert_eq!(bounded_line(&mut read).unwrap(), b"second\n");
    }
    #[test]
    fn remote_plaintext_is_rejected_before_connection() {
        assert!(Transport::open("192.0.2.1:7777")
            .err()
            .unwrap()
            .to_string()
            .contains("loopback"));
        assert!(Transport::open("ws://192.0.2.1:7778/ws")
            .err()
            .unwrap()
            .to_string()
            .contains("loopback"));
    }
    #[test]
    fn websocket_handshake_has_a_socket_deadline() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let holder = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_secs(6));
        });
        let start = std::time::Instant::now();
        assert!(Transport::open(&format!("ws://{addr}/ws")).is_err());
        assert!(start.elapsed() < Duration::from_secs(6));
        holder.join().unwrap();
    }
    #[test]
    #[ignore = "requires an isolated local game server; set ARKLINE_TUI_QA_SERVER"]
    fn live_mining_delivery_and_resume() {
        let endpoint = std::env::var("ARKLINE_TUI_QA_SERVER").unwrap();
        assert!(
            endpoint.starts_with("127.0.0.1:"),
            "isolated local world only"
        );
        let pilot = format!("TuiQA{}", std::process::id());
        let path = std::env::temp_dir().join(format!(
            "arkline-{pilot}-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        let login = Login::new(endpoint, pilot.clone(), Some(path.clone())).unwrap();
        let (mut wire, first) = login.connect().unwrap();
        let credits = first
            .snapshot
            .unwrap()
            .pilots
            .iter()
            .find(|p| p.name == pilot)
            .unwrap()
            .credits;
        assert!(
            wire.request(&Request::StartResourceJob {
                target_units: 50,
                pilot: None
            })
            .unwrap()
            .ok
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(100);
        let final_state = loop {
            let r = wire.request(&Request::Snapshot).unwrap();
            let s = r.snapshot.as_ref().unwrap();
            if s.resource_jobs.iter().any(|j| {
                j.pilot == pilot && j.phase == arkline_protocol::ResourceJobPhase::Completed
            }) {
                break r;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "mining delivery timed out"
            );
            std::thread::sleep(Duration::from_millis(200));
        };
        let stored: u64 = final_state
            .assets
            .as_ref()
            .unwrap()
            .storage
            .iter()
            .map(|v| v.ore)
            .sum();
        assert!(stored >= 50);
        let p = final_state
            .snapshot
            .as_ref()
            .unwrap()
            .pilots
            .iter()
            .find(|p| p.name == pilot)
            .unwrap();
        assert_eq!(p.credits, credits);
        assert!(matches!(p.location, arkline_protocol::Location::Docked));
        drop(wire);
        std::thread::sleep(Duration::from_millis(100));
        let (_wire, resumed) = login.connect().unwrap();
        assert_eq!(
            resumed
                .assets
                .unwrap()
                .storage
                .iter()
                .map(|v| v.ore)
                .sum::<u64>(),
            stored
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_file(path).unwrap();
    }
}
