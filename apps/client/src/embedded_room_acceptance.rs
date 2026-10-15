// Separate-process acceptance harness for the local shared room-layout authority.
//
// Include this file as a child module of `embedded::tests`. The helper tests
// are ignored during ordinary runs and are started only by the parent test.
use super::*;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const CHILD_TIMEOUT: Duration = Duration::from_secs(30);
const PARENT_TIMEOUT: Duration = Duration::from_secs(70);
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const HOST_COORDINATE_FILE: &str = "host-coordination.json";

#[derive(serde::Deserialize, serde::Serialize)]
struct HostCoordinate {
    address: SocketAddr,
    secret: String,
}

struct AcceptanceDirectory(PathBuf);

impl AcceptanceDirectory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "arkline-room-process-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock is after epoch")
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create isolated acceptance directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .expect("restrict acceptance directory permissions");
        }
        Self(path)
    }

    fn marker(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for AcceptanceDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct ChildProcesses(Vec<Child>);

impl ChildProcesses {
    fn spawn(&mut self, role: &str, directory: &Path) {
        let executable = std::env::current_exe().expect("test executable path");
        let child = Command::new(executable)
            .args([
                "--exact",
                match role {
                    "host" => "embedded::tests::room_acceptance::process_host_helper",
                    "guest" => "embedded::tests::room_acceptance::process_guest_helper",
                    _ => panic!("unknown acceptance child role"),
                },
                "--ignored",
                "--nocapture",
            ])
            .env("ARKLINE_ROOM_ACCEPTANCE_DIR", directory)
            .env("ARKLINE_ROOM_ACCEPTANCE_ROLE", role)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start isolated acceptance child process");
        self.0.push(child);
    }

    fn wait_all(&mut self, deadline: Instant) {
        let mut remaining = self.0.len();
        while remaining > 0 && Instant::now() < deadline {
            remaining = 0;
            for child in &mut self.0 {
                match child.try_wait().expect("inspect acceptance child") {
                    Some(status) => assert!(status.success(), "acceptance child failed"),
                    None => remaining += 1,
                }
            }
            if remaining > 0 {
                std::thread::sleep(POLL_INTERVAL);
            }
        }
        assert_eq!(remaining, 0, "acceptance child process timed out");
    }

    fn wait_for_file(&mut self, path: &Path, deadline: Instant) {
        while Instant::now() < deadline {
            if path.is_file() {
                return;
            }
            for child in &mut self.0 {
                if child
                    .try_wait()
                    .expect("inspect acceptance child while waiting")
                    .is_some()
                {
                    panic!("acceptance child exited before coordination marker");
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        panic!("acceptance child did not publish its coordination marker");
    }
}

impl Drop for ChildProcesses {
    fn drop(&mut self) {
        for child in &mut self.0 {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[test]
fn independent_processes_share_room_revisions_and_host_clock() {
    let directory = AcceptanceDirectory::new();
    let mut children = ChildProcesses(Vec::new());
    let deadline = Instant::now() + PARENT_TIMEOUT;

    children.spawn("host", &directory.0);
    children.wait_for_file(&directory.marker(HOST_COORDINATE_FILE), deadline);
    children.spawn("guest", &directory.0);
    children.wait_for_file(&directory.marker("guest-done"), deadline);
    children.wait_all(deadline);
}

#[test]
#[ignore = "spawned by the independent-process acceptance test"]
fn process_host_helper() {
    let directory = child_directory("host");
    let (mut client, _) = debug_fixture();
    client.data_dir = directory.join("host-project");
    client
        .switch_session(
            EmbeddedSessionMode::Host {
                bind_addr: "127.0.0.1:0".parse().expect("loopback address"),
            },
            "RoomProcessHost".into(),
        )
        .expect("start local shared host");
    wait_for_client(&mut client, |client| {
        client.ready() && client.session_info().is_some()
    });
    let session = client.session_info().expect("host session details");
    let coordinate = HostCoordinate {
        address: session.address.expect("host address"),
        secret: session.editor_secret.clone().expect("host editor secret"),
    };
    write_private_json(&directory.join(HOST_COORDINATE_FILE), &coordinate);

    wait_for_file(
        &directory.join("guest-ready"),
        Instant::now() + CHILD_TIMEOUT,
    );
    wait_for_client(&mut client, |client| {
        client.snapshot().is_some_and(|snapshot| {
            snapshot
                .pilots
                .iter()
                .any(|pilot| pilot.name == "RoomProcessGuest")
        })
    });
    let names = client
        .snapshot()
        .expect("host snapshot")
        .pilots
        .into_iter()
        .map(|pilot| pilot.name)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        names,
        ["RoomProcessGuest", "RoomProcessHost"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );

    let before = client
        .snapshot()
        .expect("snapshot before host clock run")
        .server_time_ms;
    client.play();
    wait_for_client(&mut client, |client| {
        client
            .snapshot()
            .is_some_and(|snapshot| snapshot.server_time_ms > before)
    });
    client.pause();
    wait_for_client(&mut client, |client| client.paused());
    let stopped_at = client
        .snapshot()
        .expect("host snapshot after pause")
        .server_time_ms;
    write_marker(
        &directory.join("host-clock-stopped"),
        &stopped_at.to_string(),
    );
    wait_for_file(
        &directory.join("guest-clock-seen"),
        Instant::now() + CHILD_TIMEOUT,
    );

    let initial = live_layout(&client);
    assert_eq!(initial.revision, 0);
    initial
        .document
        .validate_live()
        .expect("initial live geometry");
    let initial_geometry = initial.document.obstacles(1);
    assert!(
        !initial_geometry.is_empty(),
        "layout projects to deck geometry"
    );
    let mut first_edit = initial.document;
    let room = first_edit
        .rooms
        .iter_mut()
        .find(|room| room.source_room_id.as_deref() == Some("D1-P2"))
        .expect("non-airlock room source");
    room.clear_lwh_m[0] = 7.0;
    let command = client
        .room_layout_replace(0, first_edit)
        .expect("submit host room layout revision");
    wait_for_client(&mut client, |client| {
        client
            .room_layout_ack()
            .is_some_and(|ack| ack.id == command && ack.result.is_ok())
            && live_layout(client).revision == 1
    });
    let projected = live_layout(&client);
    projected
        .document
        .validate_live()
        .expect("accepted live geometry remains valid");
    let projected_geometry = projected.document.obstacles(1);
    assert!(
        initial_geometry
            .iter()
            .zip(&projected_geometry)
            .any(|(before, after)| before.min != after.min || before.max != after.max),
        "accepted room edits change the shared obstacle projection"
    );
    write_marker(&directory.join("host-revision-one"), "ready");
    wait_for_file(
        &directory.join("guest-revision-two"),
        Instant::now() + CHILD_TIMEOUT,
    );
    wait_for_client(&mut client, |client| live_layout(client).revision == 2);

    let second = live_layout(&client);
    assert_eq!(second.revision, 2);
    let accepted_name = room_name(&second.document, "D1-P2");
    let mut stale = second.document;
    stale
        .rooms
        .iter_mut()
        .find(|room| room.source_room_id.as_deref() == Some("D1-P2"))
        .expect("non-airlock room source")
        .name = "STALE PROCESS WRITE".into();
    let stale_command = client
        .room_layout_replace(0, stale)
        .expect("queue stale revision for authority rejection");
    wait_for_client(&mut client, |client| {
        client
            .room_layout_ack()
            .is_some_and(|ack| ack.id == stale_command && ack.result.is_err())
            && live_layout(client).revision == 2
    });
    let final_state = live_layout(&client);
    assert_eq!(final_state.revision, 2);
    assert_eq!(room_name(&final_state.document, "D1-P2"), accepted_name);
    write_marker(&directory.join("host-done"), "ready");
    wait_for_file(
        &directory.join("guest-done"),
        Instant::now() + CHILD_TIMEOUT,
    );
    finish_client(&mut client);
}

#[test]
#[ignore = "spawned by the independent-process acceptance test"]
fn process_guest_helper() {
    let directory = child_directory("guest");
    let coordinate = read_coordinate(&directory.join(HOST_COORDINATE_FILE));
    let (mut client, _) = debug_fixture();
    client.data_dir = directory.join("guest-project");
    client
        .switch_session(
            EmbeddedSessionMode::Join {
                address: coordinate.address,
                secret: coordinate.secret,
            },
            "RoomProcessGuest".into(),
        )
        .expect("join host in a separate project process");
    wait_for_client(&mut client, |client| {
        client.ready()
            && client.snapshot().is_some_and(|snapshot| {
                snapshot
                    .pilots
                    .iter()
                    .any(|pilot| pilot.name == "RoomProcessHost")
                    && snapshot
                        .pilots
                        .iter()
                        .any(|pilot| pilot.name == "RoomProcessGuest")
            })
            && live_layout(client).revision == 0
    });
    assert!(!client.can_control_clock());
    let guest_start = client
        .snapshot()
        .expect("guest initial snapshot")
        .server_time_ms;
    write_marker(&directory.join("guest-ready"), "ready");

    wait_for_file(
        &directory.join("host-clock-stopped"),
        Instant::now() + CHILD_TIMEOUT,
    );
    let host_time: u64 = read_marker(&directory.join("host-clock-stopped"))
        .parse()
        .expect("host clock marker");
    wait_for_client(&mut client, |client| {
        client.paused()
            && client
                .snapshot()
                .is_some_and(|snapshot| snapshot.server_time_ms == host_time)
    });
    assert!(host_time > guest_start);
    write_marker(&directory.join("guest-clock-seen"), "ready");

    wait_for_file(
        &directory.join("host-revision-one"),
        Instant::now() + CHILD_TIMEOUT,
    );
    wait_for_client(&mut client, |client| live_layout(client).revision == 1);
    let mut guest_edit = live_layout(&client).document;
    let room = guest_edit
        .rooms
        .iter_mut()
        .find(|room| room.source_room_id.as_deref() == Some("D1-P2"))
        .expect("non-airlock room source");
    room.name = "PROCESS GUEST ROOM NAME".into();
    let command = client
        .room_layout_replace(1, guest_edit)
        .expect("submit guest room layout revision");
    wait_for_client(&mut client, |client| {
        client
            .room_layout_ack()
            .is_some_and(|ack| ack.id == command && ack.result.is_ok())
            && live_layout(client).revision == 2
    });
    write_marker(&directory.join("guest-revision-two"), "ready");
    wait_for_file(&directory.join("host-done"), Instant::now() + CHILD_TIMEOUT);
    wait_for_client(&mut client, |client| {
        live_layout(client).revision == 2
            && room_name(&live_layout(client).document, "D1-P2") == "PROCESS GUEST ROOM NAME"
    });
    let final_state = live_layout(&client);
    assert_eq!(final_state.revision, 2);
    assert_eq!(
        room_name(&final_state.document, "D1-P2"),
        "PROCESS GUEST ROOM NAME"
    );
    write_marker(&directory.join("guest-done"), "ready");
    finish_client(&mut client);
}

fn child_directory(expected_role: &str) -> PathBuf {
    assert!(
        std::env::var("ARKLINE_ROOM_ACCEPTANCE_ROLE").is_ok_and(|role| role == expected_role),
        "process helper role"
    );
    PathBuf::from(
        std::env::var_os("ARKLINE_ROOM_ACCEPTANCE_DIR")
            .expect("acceptance directory environment variable"),
    )
}

fn wait_for_client(
    client: &mut EmbeddedClient,
    mut predicate: impl FnMut(&EmbeddedClient) -> bool,
) {
    let deadline = Instant::now() + CHILD_TIMEOUT;
    while Instant::now() < deadline {
        client.poll();
        assert!(
            !matches!(client.status(), EmbeddedStatus::Error(_)),
            "editor worker reported an error"
        );
        if predicate(client) {
            return;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    panic!("editor process did not reach the expected shared state");
}

fn live_layout(client: &EmbeddedClient) -> RoomLayoutState {
    client
        .assets()
        .and_then(|assets| assets.pioneer_room_layout)
        .expect("shared editor asset snapshot includes a room layout")
}

fn room_name(document: &RoomLayoutDocument, source_id: &str) -> String {
    document
        .rooms
        .iter()
        .find(|room| room.source_room_id.as_deref() == Some(source_id))
        .expect("room source ID")
        .name
        .clone()
}

fn finish_client(client: &mut EmbeddedClient) {
    let _ = client.clock.send(ClockCommand::Shutdown);
    client.cockpit.updates = None;
    if let Some(worker) = client.worker.take() {
        assert!(worker.join().is_ok(), "editor worker shut down cleanly");
    }
}

fn wait_for_file(path: &Path, deadline: Instant) {
    while Instant::now() < deadline {
        if path.is_file() {
            return;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    panic!("acceptance process did not publish its coordination marker");
}

fn write_marker(path: &Path, contents: &str) {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    write_private_bytes(&temporary, contents.as_bytes());
    fs::rename(&temporary, path).expect("publish process coordination marker");
}

fn read_marker(path: &Path) -> String {
    fs::read_to_string(path).expect("read process coordination marker")
}

fn write_private_json(path: &Path, value: &impl serde::Serialize) {
    let bytes = serde_json::to_vec(value).expect("serialize private host coordination");
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    write_private_bytes(&temporary, &bytes);
    fs::rename(&temporary, path).expect("publish private host coordination");
}

fn write_private_bytes(path: &Path, bytes: &[u8]) {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .expect("create private coordination file");
    file.write_all(bytes)
        .expect("write private coordination file");
    file.sync_all().expect("sync private coordination file");
}

fn read_coordinate(path: &Path) -> HostCoordinate {
    let bytes = fs::read(path).expect("read private host coordination file");
    serde_json::from_slice(&bytes).expect("parse private host coordination file")
}
