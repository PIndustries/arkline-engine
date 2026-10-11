use arkline_protocol::{Request, Response};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tungstenite::{client::IntoClientRequest, Message, WebSocket};
struct Host {
    child: Child,
    _dir: std::path::PathBuf,
    ws: String,
}
impl Host {
    fn new(cap: usize) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "arkline-ws-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_arkline-server"))
            .args([
                "--bind",
                "127.0.0.1:0",
                "--ws-bind",
                "127.0.0.1:0",
                "--ws-max-pilots",
                &cap.to_string(),
                "--db",
            ])
            .arg(dir.join("world.sqlite"))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        line.clear();
        out.read_line(&mut line).unwrap();
        let ws = line
            .split_whitespace()
            .nth(4)
            .expect("WS readiness")
            .to_string();
        Self {
            child,
            _dir: dir,
            ws,
        }
    }
    fn connect(
        &self,
        origin: Option<&str>,
        path: &str,
    ) -> Result<
        WebSocket<TcpStream>,
        Box<tungstenite::HandshakeError<tungstenite::ClientHandshake<TcpStream>>>,
    > {
        let mut request = format!("ws://{}{path}", self.ws)
            .into_client_request()
            .unwrap();
        if let Some(origin) = origin {
            request
                .headers_mut()
                .insert("Origin", origin.parse().unwrap());
        }
        let stream = TcpStream::connect(&self.ws).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        tungstenite::client(request, stream)
            .map(|(ws, _)| ws)
            .map_err(Box::new)
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self._dir);
    }
}
fn ask(ws: &mut WebSocket<TcpStream>, request: Request) -> Response {
    ws.send(Message::text(serde_json::to_string(&request).unwrap()))
        .unwrap();
    loop {
        match ws.read().unwrap() {
            Message::Text(s) => return serde_json::from_str(&s).unwrap(),
            Message::Ping(_) => ws.flush().unwrap(),
            m => panic!("unexpected response kind {m:?}"),
        }
    }
}
fn login(ws: &mut WebSocket<TcpStream>, name: &str, token: Option<String>) -> Response {
    ask(
        ws,
        Request::Connect {
            supports_pioneer_interior: true,
            protocol_version: arkline_protocol::PROTOCOL_VERSION,
            pilot: name.into(),
            resume_token: token,
        },
    )
}
#[test]
fn origins_health_auth_and_registration_bounds() {
    let h = Host::new(1);
    assert!(h.connect(Some("https://hostile.example"), "/ws").is_err());
    assert!(h.connect(None, "/other").is_err());
    let mut ws = h
        .connect(Some("https://play.arkline.network"), "/ws")
        .unwrap();
    assert!(!ask(&mut ws, Request::Undock).ok);
    let snapshot = ask(&mut ws, Request::Snapshot);
    assert!(snapshot.ok);
    assert!(snapshot.assets.is_none());
    assert!(snapshot.snapshot.unwrap().pilots.is_empty());
    let made = login(&mut ws, "PublicPilot", None);
    assert!(made.ok);
    let token = made.resume_token.unwrap();
    let denied = login(&mut ws, "OtherPilot", None);
    assert!(!denied.ok);
    assert!(login(&mut ws, "PublicPilot", Some(token)).ok);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let mut s = TcpStream::connect(&h.ws).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        s.write_all(b"GET /health HTTP/1.1\r\nHost: local\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        s.read_to_string(&mut response).unwrap();
        if response.starts_with("HTTP/1.1 200") {
            assert!(response.contains("\"simulation_ready\":true"));
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(100));
    }
}
#[test]
fn presence_cleanup_and_oversize_disconnect() {
    let h = Host::new(10);
    let mut a = h.connect(None, "/ws").unwrap();
    let token = login(&mut a, "SharedPilot", None).resume_token.unwrap();
    let mut b = h
        .connect(Some("https://test.arkline.network"), "/ws")
        .unwrap();
    assert!(login(&mut b, "SharedPilot", Some(token)).ok);
    drop(a);
    let mut observer = h.connect(None, "/ws").unwrap();
    assert!(ask(&mut observer, Request::Snapshot)
        .snapshot
        .unwrap()
        .online_pilots
        .contains(&"SharedPilot".into()));
    b.send(Message::text("x".repeat(16385))).unwrap();
    assert!(b.read().is_err());
    drop(b);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let s = ask(&mut observer, Request::Snapshot).snapshot.unwrap();
        if s.online_pilots.is_empty() {
            assert_eq!(s.pilots.len(), 1);
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
}
#[test]
fn ping_bursts_are_bounded_and_binary_is_rejected() {
    let h = Host::new(10);
    let mut ws = h.connect(None, "/ws").unwrap();
    for _ in 0..100 {
        if ws.write(Message::Ping(vec![1].into())).is_err() {
            break;
        }
    }
    let _ = ws.flush();
    let mut bounded = false;
    for _ in 0..110 {
        match ws.read() {
            Ok(Message::Text(s)) => {
                let r: Response = serde_json::from_str(&s).unwrap();
                assert!(!r.ok);
                bounded = true;
                break;
            }
            Err(_) | Ok(Message::Close(_)) => {
                bounded = true;
                break;
            }
            _ => {}
        }
    }
    assert!(bounded);
    let mut ws = h.connect(None, "/ws").unwrap();
    ws.send(Message::Binary(vec![0].into())).unwrap();
    match ws.read().unwrap() {
        Message::Text(s) => assert!(!serde_json::from_str::<Response>(&s).unwrap().ok),
        _ => panic!("expected rejection"),
    };
}

#[test]
fn fragmented_text_is_one_request_and_large_http_headers_are_closed() {
    use tungstenite::protocol::frame::{
        coding::{Data, OpCode},
        Frame,
    };
    let h = Host::new(10);
    let mut ws = h.connect(None, "/ws").unwrap();
    let json = serde_json::to_vec(&Request::Snapshot).unwrap();
    let split = json.len() / 2;
    ws.send(Message::Frame(Frame::message(
        json[..split].to_vec(),
        OpCode::Data(Data::Text),
        false,
    )))
    .unwrap();
    ws.send(Message::Frame(Frame::message(
        json[split..].to_vec(),
        OpCode::Data(Data::Continue),
        true,
    )))
    .unwrap();
    let reply = ws.read().unwrap();
    assert!(
        serde_json::from_str::<Response>(reply.to_text().unwrap())
            .unwrap()
            .ok
    );
    let mut raw = TcpStream::connect(&h.ws).unwrap();
    raw.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    raw.write_all(
        format!(
            "GET /ws HTTP/1.1\r\nHost: local\r\nX-Large: {}",
            "x".repeat(17000)
        )
        .as_bytes(),
    )
    .unwrap();
    let mut byte = [0u8; 1];
    match raw.read(&mut byte) {
        Ok(0) => {}
        Err(e) => assert_ne!(e.kind(), std::io::ErrorKind::TimedOut),
        other => panic!("expected oversized handshake closure: {other:?}"),
    }
    assert!(h.connect(None, "/ws").is_ok());
}
