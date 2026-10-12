//! Owns only processes started by this editor. Network addresses cannot be supplied by users.
use std::{
    collections::VecDeque,
    io::Read,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender, SyncSender},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Configuration {
    pub project: PathBuf,
    pub binaries: PathBuf,
}
#[derive(Debug)]
pub enum Action {
    Start(Configuration),
    Launch(String),
    Stop,
    Shutdown,
}
#[derive(Debug)]
pub enum Event {
    Starting,
    Ready(SocketAddr),
    ClientCount(usize),
    Stopped,
    Error(String),
}

pub struct Runtime {
    tx: Sender<Action>,
    pub events: Receiver<Event>,
    pub logs: Receiver<String>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Default for Runtime {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        let (out, events) = mpsc::channel();
        let (log_out, logs) = mpsc::sync_channel(256);
        let worker = thread::spawn(move || run(rx, out, log_out));
        Self {
            tx,
            events,
            logs,
            worker: Some(worker),
        }
    }
}
impl Runtime {
    pub fn send(&self, action: Action) {
        let _ = self.tx.send(action);
    }
    pub fn shutdown(&mut self) {
        let _ = self.tx.send(Action::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn emit(out: &Sender<Event>, event: Event) {
    let _ = out.send(event);
}
fn binary(dir: &Path, name: &str) -> PathBuf {
    dir.join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    })
}
fn ready(line: &str) -> Option<SocketAddr> {
    let address = line
        .strip_prefix("Arkline server listening on ")?
        .trim()
        .parse::<SocketAddr>()
        .ok()?;
    (address.ip() == IpAddr::from([127, 0, 0, 1]) && address.port() != 0).then_some(address)
}
fn pilot_valid(pilot: &str) -> bool {
    !pilot.is_empty()
        && pilot.len() <= 24
        && pilot
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
fn prepare(config: &Configuration) -> Result<(PathBuf, PathBuf), String> {
    if config.project.as_os_str().is_empty() {
        return Err("Choose a project directory.".into());
    }
    std::fs::create_dir_all(&config.project).map_err(|e| format!("Cannot create project: {e}"))?;
    let project = config
        .project
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project: {e}"))?;
    if project.parent().is_none() {
        return Err("Choose a project folder, not the filesystem root.".into());
    }
    let data = project.join(".arkline-editor");
    std::fs::create_dir_all(data.join("client"))
        .map_err(|e| format!("Cannot create local data: {e}"))?;
    for name in ["arkline-server", "arkline-client"] {
        if !binary(&config.binaries, name).is_file() {
            return Err(format!(
                "Missing {name} in {}. Build the server and client first.",
                config.binaries.display()
            ));
        }
    }
    Ok((project, data))
}
// Fixed chunks and capped lines keep arbitrary process output from growing memory.
fn lines(
    mut input: impl Read + Send + 'static,
    tx: SyncSender<(u64, bool, String)>,
    generation: u64,
    server: bool,
) {
    thread::spawn(move || {
        let mut buf = [0u8; 1024];
        let mut line = Vec::new();
        loop {
            let Ok(n) = input.read(&mut buf) else {
                break;
            };
            if n == 0 {
                break;
            }
            for &b in &buf[..n] {
                if b == b'\n' {
                    let value = String::from_utf8_lossy(&line).into_owned();
                    let _ = tx.try_send((generation, server, value));
                    line.clear();
                } else if line.len() < 4096 {
                    line.push(b);
                }
            }
        }
    });
}
struct Session {
    server: Child,
    clients: Vec<Child>,
    client_binary: PathBuf,
    project: PathBuf,
    data: PathBuf,
    address: Option<SocketAddr>,
    started: Instant,
    generation: u64,
}
impl Session {
    fn start(
        config: Configuration,
        generation: u64,
        logs: &SyncSender<(u64, bool, String)>,
    ) -> Result<Self, String> {
        let (project, data) = prepare(&config)?;
        let binaries = config
            .binaries
            .canonicalize()
            .map_err(|e| format!("Cannot resolve binary directory: {e}"))?;
        let mut server = Command::new(binary(&binaries, "arkline-server"))
            .args(["--bind", "127.0.0.1:0", "--local-development"])
            .arg(&data)
            .current_dir(&project)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Cannot start local server: {e}"))?;
        lines(
            server.stdout.take().unwrap(),
            logs.clone(),
            generation,
            true,
        );
        lines(
            server.stderr.take().unwrap(),
            logs.clone(),
            generation,
            false,
        );
        Ok(Self {
            server,
            clients: vec![],
            client_binary: binary(&binaries, "arkline-client"),
            project,
            data,
            address: None,
            started: Instant::now(),
            generation,
        })
    }
    fn launch(
        &mut self,
        pilot: String,
        logs: &SyncSender<(u64, bool, String)>,
    ) -> Result<(), String> {
        if self.clients.len() >= 8 {
            return Err("This editor supports up to eight concurrent test clients.".into());
        }
        if !pilot_valid(&pilot) {
            return Err("Pilot: use 1–24 ASCII letters, numbers, '_' or '-'.".into());
        }
        let addr = self.address.ok_or("Wait for local server readiness.")?;
        let mut child = Command::new(&self.client_binary)
            .args([
                "--remote-only",
                "--server",
                &addr.to_string(),
                "--pilot",
                &pilot,
                "--data-dir",
            ])
            .arg(self.data.join("client"))
            .arg("--credential-scope")
            .arg(format!("editor:{}", self.project.display()))
            .current_dir(&self.project)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Cannot launch client: {e}"))?;
        lines(
            child.stderr.take().unwrap(),
            logs.clone(),
            self.generation,
            false,
        );
        self.clients.push(child);
        Ok(())
    }
}
fn stop(child: &mut Child) {
    match child.try_wait() {
        Ok(Some(_)) => {}
        _ => {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        for child in &mut self.clients {
            stop(child);
        }
        stop(&mut self.server);
    }
}
fn run(rx: Receiver<Action>, out: Sender<Event>, log_out: SyncSender<String>) {
    let (log_tx, logs) = mpsc::sync_channel(256);
    let mut session: Option<Session> = None;
    let mut generation = 0u64;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Action::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Action::Stop) => {
                session = None;
                emit(&out, Event::ClientCount(0));
                emit(&out, Event::Stopped);
            }
            Ok(Action::Start(config)) => {
                if session.is_some() {
                    emit(
                        &out,
                        Event::Error(
                            "Stop this editor's local session before starting another.".into(),
                        ),
                    );
                    continue;
                }
                generation += 1;
                emit(&out, Event::Starting);
                match Session::start(config, generation, &log_tx) {
                    Ok(s) => session = Some(s),
                    Err(e) => {
                        emit(&out, Event::Stopped);
                        emit(&out, Event::Error(e));
                    }
                }
            }
            Ok(Action::Launch(pilot)) => {
                if let Some(s) = session.as_mut() {
                    match s.launch(pilot, &log_tx) {
                        Ok(()) => emit(&out, Event::ClientCount(s.clients.len())),
                        Err(e) => emit(&out, Event::Error(e)),
                    }
                } else {
                    emit(&out, Event::Error("Start a local server first.".into()));
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        for _ in 0..128 {
            let Ok((id, server, line)) = logs.try_recv() else {
                break;
            };
            let Some(s) = session.as_mut().filter(|s| s.generation == id) else {
                continue;
            };
            if server && s.address.is_none() {
                if let Some(addr) = ready(&line) {
                    s.address = Some(addr);
                    emit(&out, Event::Ready(addr));
                }
            }
            let _ = log_out.try_send(line);
        }
        let failure = session.as_mut().and_then(|s| {
            match s.server.try_wait() {
                Ok(Some(status)) => return Some(format!("Local server exited: {status}")),
                Err(e) => return Some(format!("Cannot inspect server: {e}")),
                _ => {}
            }
            if s.address.is_none() && s.started.elapsed() > Duration::from_secs(15) {
                return Some(
                    "Local server did not become ready within 15 seconds; stopped.".into(),
                );
            }
            let before = s.clients.len();
            s.clients
                .retain_mut(|c| !matches!(c.try_wait(), Ok(Some(_))));
            if before != s.clients.len() {
                emit(&out, Event::ClientCount(s.clients.len()));
            }
            None
        });
        if let Some(error) = failure {
            session = None;
            emit(&out, Event::ClientCount(0));
            emit(&out, Event::Stopped);
            emit(&out, Event::Error(error));
        }
    }
    drop(session);
}
pub fn append_log(logs: &mut VecDeque<String>, line: String) {
    logs.push_back(line);
    while logs.len() > 200 {
        logs.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readiness_only_accepts_exact_numeric_loopback_and_assigned_port() {
        assert_eq!(
            ready("Arkline server listening on 127.0.0.1:17777")
                .unwrap()
                .port(),
            17777
        );
        for s in [
            "Arkline server listening on 0.0.0.0:42",
            "Arkline server listening on 127.0.0.1:0",
            "Arkline server listening on localhost:42",
            "Arkline server listening on 192.168.1.1:42",
            "unrelated 127.0.0.1:42",
        ] {
            assert!(ready(s).is_none());
        }
    }
    #[test]
    fn pilot_validation_and_log_retention_are_bounded() {
        assert!(pilot_valid("Editor_02-Test"));
        assert!(!pilot_valid(""));
        assert!(!pilot_valid("a b"));
        assert!(!pilot_valid(&"x".repeat(25)));
        let mut logs = VecDeque::new();
        for i in 0..1000 {
            append_log(&mut logs, i.to_string());
        }
        assert_eq!(logs.len(), 200);
        assert_eq!(logs[0], "800");
        let (tx, rx) = mpsc::sync_channel(4);
        lines(
            std::io::Cursor::new(format!("{}\n", "x".repeat(20000)).into_bytes()),
            tx,
            1,
            true,
        );
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap().2.len(),
            4096
        );
    }
    #[test]
    #[ignore = "Explicit real local server smoke; set ARKLINE_EDITOR_BIN_DIR, no GUI or public network"]
    fn actual_server_readiness_authentication_and_cleanup() {
        use std::io::{BufRead, Write};
        let binaries = PathBuf::from(
            std::env::var_os("ARKLINE_EDITOR_BIN_DIR").expect("set binary directory"),
        );
        let project = std::env::temp_dir().join(format!(
            "arkline-editor-real-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (tx, rx) = mpsc::sync_channel(256);
        let mut session = Session::start(
            Configuration {
                project: project.clone(),
                binaries,
            },
            1,
            &tx,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if let Ok((_, true, line)) = rx.recv_timeout(Duration::from_millis(100)) {
                if let Some(addr) = ready(&line) {
                    session.address = Some(addr);
                    break;
                }
            }
        }
        let mut stream = std::net::TcpStream::connect_timeout(
            &session.address.expect("actual readiness"),
            Duration::from_secs(2),
        )
        .unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream.write_all(b"{\"command\":\"connect\",\"protocol_version\":1,\"pilot\":\"EditorSmoke\",\"resume_token\":null}\n").unwrap();
        let mut response = String::new();
        std::io::BufReader::new(stream)
            .take(1_048_576)
            .read_line(&mut response)
            .unwrap();
        assert!(
            response.contains("\"ok\":true"),
            "local authenticated response was not successful"
        );
        assert!(response.contains("\"snapshot\":"));
        assert!(session.data.join("world.sqlite").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&session.data)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        stop(&mut session.server);
        assert!(session.server.try_wait().unwrap().is_some());
        drop(session);
        std::fs::remove_dir_all(project).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn isolated_session_uses_explicit_local_mode_stable_scope_and_cleans_only_owned_children() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "arkline-editor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bins = root.join("bin with spaces");
        std::fs::create_dir_all(&bins).unwrap();
        for (name,script) in [("arkline-server","#!/bin/sh\nprintf '%s\\n' \"$@\" > server-args\nprintf 'Arkline server listening on 127.0.0.1:12345\\n'\nexec sleep 60\n"),("arkline-client","#!/bin/sh\nprintf '%s\\n' \"$@\" > client-args\nexec sleep 60\n")] {let path=bins.join(name);std::fs::write(&path,script).unwrap();std::fs::set_permissions(path,std::fs::Permissions::from_mode(0o700)).unwrap();}
        let project = root.join("project with spaces");
        let config = Configuration {
            project: project.clone(),
            binaries: bins,
        };
        let (tx, rx) = mpsc::sync_channel(8);
        let mut unrelated = Command::new("sleep").arg("60").spawn().unwrap();
        let mut session = Session::start(config.clone(), 42, &tx).unwrap();
        let (generation, server, line) = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(generation, 42);
        assert!(server);
        session.address = ready(&line);
        session.launch("EditorPilot".into(), &tx).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !project.join("client-args").exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let args = std::fs::read_to_string(project.join("server-args")).unwrap();
        assert!(args.contains("--local-development\n"));
        assert!(!args.contains("--db"));
        assert!(args.contains("127.0.0.1:0"));
        let args = std::fs::read_to_string(project.join("client-args")).unwrap();
        assert!(args.starts_with("--remote-only\n--server\n127.0.0.1:12345\n"));
        assert!(args.contains(&format!(
            "--credential-scope\neditor:{}\n",
            project.canonicalize().unwrap().display()
        )));
        stop(&mut session.clients[0]);
        stop(&mut session.server);
        assert!(session.clients[0].try_wait().unwrap().is_some());
        assert!(session.server.try_wait().unwrap().is_some());
        drop(session);
        assert!(unrelated.try_wait().unwrap().is_none());
        stop(&mut unrelated);
        let (resolved, data) = prepare(&config).unwrap();
        assert_eq!(resolved, project.canonicalize().unwrap());
        assert_eq!(data, resolved.join(".arkline-editor"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
