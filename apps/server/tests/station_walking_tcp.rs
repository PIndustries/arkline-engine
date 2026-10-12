//! Flight acceptance through the same TCP boundary used by the cockpit.
use arkline_protocol::*;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::TcpStream,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
struct Host {
    process: Child,
    address: String,
}
impl Host {
    fn start(path: &Path) -> Self {
        let mut process = Command::new(env!("CARGO_BIN_EXE_arkline-server"))
            .args(["--bind", "127.0.0.1:0", "--db", path.to_str().unwrap()])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(process.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let address = line
            .trim()
            .strip_prefix("Arkline server listening on ")
            .unwrap()
            .to_string();
        Self { process, address }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
struct Client(BufReader<TcpStream>);
impl Client {
    fn new(address: &str) -> Self {
        let socket = TcpStream::connect(address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        Self(BufReader::new(socket))
    }
    fn request(&mut self, request: Request) -> Response {
        serde_json::to_writer(self.0.get_mut(), &request).unwrap();
        self.0.get_mut().write_all(b"\n").unwrap();
        let mut line = String::new();
        self.0.read_line(&mut line).unwrap();
        let response: Response = serde_json::from_str(&line).unwrap();
        assert!(response.ok, "{}", response.message);
        response
    }
    fn connect(&mut self, token: Option<String>) -> Response {
        self.request(Request::Connect {
            supports_room_controls: false,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Navigator".into(),
            resume_token: token,
        })
    }
}
#[test]
fn real_socket_walk_expiry_and_boarding_preserve_ship_economy() {
    let directory = std::env::temp_dir().join(format!(
        "arkline-walk-tcp-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&directory).unwrap();
    let host = Host::start(&directory.join("world.sqlite"));
    let mut client = Client::new(&host.address);
    let login = client.connect(None);
    let initial = client.request(Request::DisembarkShip).snapshot.unwrap();
    let spawn = initial.pilots[0].station_avatar.as_ref().unwrap().position;
    client.request(Request::StationWalk {
        forward: 1.,
        strafe: 0.,
        turn: 0.,
    });
    thread::sleep(Duration::from_millis(800));
    let moved = client.request(Request::Snapshot).snapshot.unwrap();
    let avatar = moved.pilots[0].station_avatar.as_ref().unwrap();
    assert!(avatar.position[0] - spawn[0] > 1.0 && avatar.position[0] - spawn[0] <= 2.01);
    assert!(!avatar.moving);
    assert_eq!(moved.pilots[0].credits, initial.pilots[0].credits);
    assert_eq!(moved.ledger_hash, initial.ledger_hash);
    drop(client);
    let mut client = Client::new(&host.address);
    client.connect(login.resume_token);
    let boarded = client.request(Request::EnterShip).snapshot.unwrap();
    assert!(boarded.pilots[0].station_avatar.is_none());
    client.request(Request::Undock);
    drop(client);
    drop(host);
    fs::remove_dir_all(directory).unwrap();
}
