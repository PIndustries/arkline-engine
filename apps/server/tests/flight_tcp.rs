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
            world_layout_version: 1,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Navigator".into(),
            resume_token: token,
        })
    }
}
#[test]
fn commanded_flight_stops_and_survives_process_restart_without_economic_changes() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("arkline-flight-tcp-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("world.sqlite");
    let host = Host::start(&path);
    let mut client = Client::new(&host.address);
    let token = client.connect(None).resume_token.unwrap();
    let before = client.request(Request::Undock).snapshot.unwrap();
    let initial = before.pilots[0].flight.as_ref().unwrap().position;
    client.request(Request::Navigate {
        mode: NavigationMode::FlyTo {
            position: [
                initial[0] + 4000.0,
                initial[1] + 1000.0,
                initial[2] + 2000.0,
            ],
        },
    });
    thread::sleep(Duration::from_millis(1600));
    let moving = client.request(Request::Snapshot).snapshot.unwrap();
    let moved = moving.pilots[0].flight.as_ref().unwrap();
    assert!(
        moved
            .position
            .iter()
            .zip(initial)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt()
            > 10.0,
        "real server tick must advance position"
    );
    assert!(moved.velocity.iter().map(|v| v * v).sum::<f64>().sqrt() > 0.0);
    client.request(Request::Navigate {
        mode: NavigationMode::Stop,
    });
    // Let authoritative braking finish without issuing more navigation commands.
    let speed = moved.velocity.iter().map(|v| v * v).sum::<f64>().sqrt();
    let stop_seconds =
        speed / ship_braking_acceleration(moving.pilots[0].ship_class, moving.pilots[0].ore) + 2.0;
    let deadline = std::time::Instant::now() + Duration::from_secs_f64(stop_seconds);
    let stopped = loop {
        let snapshot = client.request(Request::Snapshot).snapshot.unwrap();
        if snapshot.pilots[0]
            .flight
            .as_ref()
            .unwrap()
            .velocity
            .iter()
            .all(|v| v.abs() < 0.1)
        {
            break snapshot;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "authoritative braking exceeded physical deadline"
        );
        thread::sleep(Duration::from_millis(100));
    };
    let position = stopped.pilots[0].flight.as_ref().unwrap().position;
    assert!(stopped.pilots[0]
        .flight
        .as_ref()
        .unwrap()
        .velocity
        .iter()
        .all(|v| v.abs() < 0.1));
    assert_eq!(
        stopped.ledger_hash, before.ledger_hash,
        "navigation must not mint economic events"
    );
    drop(client);
    drop(host);
    let host = Host::start(&path);
    let mut client = Client::new(&host.address);
    let restored = client.connect(Some(token)).snapshot.unwrap();
    assert_eq!(
        restored.pilots[0].flight.as_ref().unwrap().position,
        position
    );
    assert_eq!(restored.pilots[0].ore, stopped.pilots[0].ore);
    assert_eq!(restored.pilots[0].credits, stopped.pilots[0].credits);
    assert_eq!(restored.ledger_hash, before.ledger_hash);
    drop(client);
    drop(host);
    fs::remove_dir_all(directory).unwrap();
}
