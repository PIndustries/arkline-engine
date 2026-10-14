//! Worker adapter for a loopback collaborative editor session.
//!
//! This module is intentionally kept off the render thread. Network exchanges,
//! the shared SQLite authority, and graceful host shutdown all run here.
use super::*;
use arkline_protocol::{EditorEventGraph, Request, Response};
use arkline_server::collaborative_transport::{
    ClockAction, DevelopmentEditorClient, DevelopmentEditorConfig, DevelopmentEditorHost,
};
use std::{
    fs,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError},
    time::Duration,
};

enum Peer {
    Host(Box<DevelopmentEditorHost>),
    Join(DevelopmentEditorClient),
}

struct OpenedPeer {
    peer: Peer,
    connect_response: Response,
    info: EmbeddedSessionInfo,
    secret: Option<String>,
    credential_path: PathBuf,
}

impl Peer {
    fn set_scene_state(&mut self, state: EditorSceneState) -> Result<(), String> {
        match self {
            Self::Host(host) => host
                .set_scene_state(state)
                .map_err(|error| error.to_string()),
            Self::Join(client) => client
                .set_scene_state(state)
                .map_err(|error| error.to_string()),
        }
    }
    fn replace_room_layout(
        &mut self,
        revision: u64,
        document: RoomLayoutDocument,
    ) -> Result<(), String> {
        match self {
            Self::Host(host) => host
                .replace_room_layout(revision, document)
                .map(|_| ())
                .map_err(|e| e.to_string()),
            Self::Join(client) => client
                .replace_room_layout(revision, document)
                .map(|_| ())
                .map_err(|e| e.to_string()),
        }
    }
    fn is_host(&self) -> bool {
        matches!(self, Self::Host(_))
    }

    fn snapshot(
        &mut self,
    ) -> Result<arkline_server::collaborative_editor::CollaborativeEditorSnapshot, String> {
        match self {
            Self::Host(host) => host.snapshot().map_err(|error| error.to_string()),
            Self::Join(client) => client.snapshot().map_err(|error| error.to_string()),
        }
    }

    fn game(&mut self, request: Request) -> Result<Response, String> {
        match self {
            Self::Host(host) => host.game(request).map_err(|error| error.to_string()),
            Self::Join(client) => client.game(request).map_err(|error| error.to_string()),
        }
    }

    fn clock(&self, action: ClockAction) -> Result<(), String> {
        match self {
            Self::Host(host) => host.clock(action).map_err(|error| error.to_string()),
            Self::Join(_) => Err("The shared simulation clock is controlled by the host.".into()),
        }
    }

    fn replace_graph(
        &mut self,
        revision: u64,
        graph: EditorEventGraph,
        clear_pending: bool,
    ) -> Result<(), String> {
        match self {
            Self::Host(host) => host
                .replace_graph(revision, graph, clear_pending)
                .map_err(|error| error.to_string()),
            Self::Join(client) => client
                .replace_graph(revision, graph, clear_pending)
                .map_err(|error| error.to_string()),
        }
    }

    fn trigger_graph(&mut self, node_id: u64) -> Result<(), String> {
        match self {
            Self::Host(host) => host
                .trigger_graph(node_id)
                .map_err(|error| error.to_string()),
            Self::Join(client) => client
                .trigger_graph(node_id)
                .map_err(|error| error.to_string()),
        }
    }

    fn step_graph(&mut self) -> Result<(), String> {
        match self {
            Self::Host(host) => host.step_graph().map_err(|error| error.to_string()),
            Self::Join(client) => client.step_graph().map_err(|error| error.to_string()),
        }
    }

    fn clear_graph(&mut self) -> Result<(), String> {
        match self {
            Self::Host(host) => host.clear_graph().map_err(|error| error.to_string()),
            Self::Join(client) => client.clear_graph().map_err(|error| error.to_string()),
        }
    }

    fn close(self) -> Result<(), String> {
        match self {
            Self::Host(host) => host
                .shutdown_and_wait_from_worker()
                .map_err(|error| error.to_string()),
            Self::Join(mut client) => {
                let _ = client.leave();
                Ok(())
            }
        }
    }
}

#[allow(clippy::too_many_arguments)] // Fixed worker contract owned by embedded.rs.
pub(super) fn run(
    dir: PathBuf,
    pilot: String,
    mode: EmbeddedSessionMode,
    clock: Receiver<ClockCommand>,
    requests: Receiver<Request>,
    updates: SyncSender<Update>,
    controls: super::super::pilot_input::Mailbox,
    outputs: (Sender<EmbeddedStatus>, SyncSender<EmbeddedEventDebugState>),
    info: Sender<EmbeddedSessionInfo>,
) -> Result<(), String> {
    let (states, graph_states) = outputs;
    let OpenedPeer {
        mut peer,
        mut connect_response,
        info: session_info,
        secret,
        credential_path,
    } = open_peer(&dir, &pilot, mode)?;
    if let Err(error) = info.send(session_info) {
        let _ = peer.close();
        return Err(format!(
            "Shared editor session details receiver stopped: {error}"
        ));
    }
    if !connect_response.ok {
        let _ = peer.close();
        return Err(connect_response.message);
    }
    if let Some(token) = connect_response.resume_token.take() {
        if let Err(error) = save_token(&credential_path, &token) {
            let _ = peer.close();
            return Err(redact(error, secret.as_deref()));
        }
    }
    if let Err(error) = updates.send(Update::Ready) {
        let _ = peer.close();
        return Err(error.to_string());
    }
    if let Err(error) = updates.send(Update::Response(Box::new(connect_response), false)) {
        let _ = peer.close();
        return Err(error.to_string());
    }

    let result = run_loop(
        &mut peer,
        clock,
        requests,
        updates,
        controls,
        &states,
        graph_states,
        secret.as_deref(),
    );
    let close_result = peer.close();
    let _ = states.send(EmbeddedStatus::Stopped);
    match (result, close_result) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn open_peer(
    dir: &std::path::Path,
    pilot: &str,
    mode: EmbeddedSessionMode,
) -> Result<OpenedPeer, String> {
    match mode {
        EmbeddedSessionMode::Local => {
            Err("Local sessions use the embedded local-world worker.".into())
        }
        EmbeddedSessionMode::Host { bind_addr } => {
            let credential = credential_path(dir, "embedded-editor", pilot)?;
            let token = read_token(&credential)?;
            let connect = WorkerIdentity::default().request("embedded-editor", pilot, token)?;
            let host = DevelopmentEditorHost::bind(
                DevelopmentEditorConfig::new(dir.to_path_buf(), connect).with_bind_addr(bind_addr),
            )
            .map_err(|error| error.to_string())?;
            let address = host.local_addr();
            let secret = host.expose_join_secrets().editor_secret().to_owned();
            let info = EmbeddedSessionInfo {
                label: format!("Shared editor host · {address}"),
                address: Some(address),
                editor_secret: Some(secret.clone()),
                is_host: true,
            };
            let response = host.host_connect_response();
            Ok(OpenedPeer {
                peer: Peer::Host(Box::new(host)),
                connect_response: response,
                info,
                secret: Some(secret),
                credential_path: credential,
            })
        }
        EmbeddedSessionMode::Join { address, secret } => {
            let credential_domain = format!("shared-editor:{address}");
            let credential = credential_path(dir, &credential_domain, pilot)?;
            let token = read_token(&credential)?;
            let connect = WorkerIdentity::default().request(&credential_domain, pilot, token)?;
            let (client, response) =
                DevelopmentEditorClient::connect(address, &secret, Some(connect))
                    .map_err(|error| redact(error, Some(&secret)))?;
            let info = EmbeddedSessionInfo {
                label: format!("Joined editor · {address}"),
                address: Some(address),
                editor_secret: None,
                is_host: false,
            };
            Ok(OpenedPeer {
                peer: Peer::Join(client),
                connect_response: response,
                info,
                secret: Some(secret),
                credential_path: credential,
            })
        }
    }
}

fn credential_path(dir: &std::path::Path, domain: &str, pilot: &str) -> Result<PathBuf, String> {
    let credentials = dir.join("client");
    fs::create_dir_all(&credentials)
        .map_err(|error| format!("Cannot create shared editor credential directory: {error}"))?;
    Ok(super::super::credentials_path(&credentials, domain, pilot))
}

fn read_token(path: &std::path::Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(token) => Ok(Some(token)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "Cannot read saved shared editor credential: {error}"
        )),
    }
}

fn redact(error: impl ToString, secret: Option<&str>) -> String {
    let mut message = error.to_string();
    if let Some(secret) = secret.filter(|secret| !secret.is_empty()) {
        message = message.replace(secret, "[redacted]");
    }
    message
}

#[allow(clippy::too_many_arguments)] // Keep the existing embedded worker channel boundary explicit.
fn run_loop(
    peer: &mut Peer,
    clock: Receiver<ClockCommand>,
    requests: Receiver<Request>,
    updates: SyncSender<Update>,
    controls: super::super::pilot_input::Mailbox,
    states: &Sender<EmbeddedStatus>,
    graph_states: SyncSender<EmbeddedEventDebugState>,
    secret: Option<&str>,
) -> Result<(), String> {
    let mut stopped = false;
    let mut shared_paused = true;
    let mut last_status = EmbeddedStatus::Loading;
    let mut graph_ack: Option<EmbeddedEventAck> = None;
    let mut room_ack: Option<EmbeddedEventAck> = None;
    let mut scene_ack: Option<EmbeddedEventAck> = None;
    let mut consecutive_busy_snapshots = 0_u8;
    let mut busy_notice_sent = false;

    loop {
        match clock.recv_timeout(Duration::from_millis(50)) {
            Ok(ClockCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(ClockCommand::SceneState(id, state)) => {
                controls.clear();
                let result = if stopped {
                    Err(
                        "This editor's controls are suspended; resume before changing scene state"
                            .into(),
                    )
                } else {
                    peer.set_scene_state(state)
                };
                scene_ack = Some(EmbeddedEventAck {
                    id,
                    result: result.map_err(|error| redact(error, secret)),
                });
            }
            Ok(ClockCommand::RoomLayout(id, revision, document)) => {
                let result = if stopped {
                    Err("This editor's controls are suspended; resume before applying a room layout".into())
                } else {
                    peer.replace_room_layout(revision, document)
                };
                room_ack = Some(EmbeddedEventAck {
                    id,
                    result: result.map_err(|e| redact(e, secret)),
                });
            }
            Ok(ClockCommand::Play) => {
                let result = if peer.is_host() {
                    peer.clock(ClockAction::Play)
                } else if stopped {
                    Ok(())
                } else {
                    Err("Joined editors cannot start or resume the shared clock; only the host controls it.".into())
                };
                match result {
                    Ok(()) => {
                        stopped = false;
                        if peer.is_host() {
                            shared_paused = false;
                        }
                        controls.clear();
                    }
                    Err(error) => send_error(&updates, redact(error, secret))?,
                }
            }
            Ok(ClockCommand::Pause) => {
                let result = if peer.is_host() {
                    peer.clock(ClockAction::Pause)
                } else {
                    Err(
                        "Joined editors cannot pause the shared clock; only the host controls it."
                            .into(),
                    )
                };
                match result {
                    Ok(()) => {
                        controls.clear();
                        shared_paused = true;
                    }
                    Err(error) => send_error(&updates, redact(error, secret))?,
                }
            }
            Ok(ClockCommand::Step) => {
                let result = if stopped {
                    Err("This editor session is stopped. Press Play before stepping.".into())
                } else if peer.is_host() {
                    peer.clock(ClockAction::Step { delta_ms: 50 })
                } else {
                    Err(
                        "Joined editors cannot step the shared clock; only the host controls it."
                            .into(),
                    )
                };
                if let Err(error) = result {
                    send_error(&updates, redact(error, secret))?;
                }
            }
            Ok(ClockCommand::Stop) => {
                controls.clear();
                let result = if peer.is_host() {
                    peer.clock(ClockAction::Pause)
                } else {
                    Ok(())
                };
                match result {
                    Ok(()) => {
                        stopped = true;
                        shared_paused = true;
                    }
                    Err(error) => send_error(&updates, redact(error, secret))?,
                }
            }
            Ok(ClockCommand::Graph(id, command)) => {
                let result = if stopped {
                    Err("This editor session is stopped. Press Play before editing the event graph.".into())
                } else {
                    match command {
                        GraphCommand::Replace {
                            revision,
                            graph,
                            clear_pending,
                        } => peer.replace_graph(revision, graph, clear_pending),
                        GraphCommand::Trigger(node) => peer.trigger_graph(node),
                        GraphCommand::Step => peer.step_graph(),
                        GraphCommand::Clear => peer.clear_graph(),
                    }
                };
                graph_ack = Some(EmbeddedEventAck {
                    id,
                    result: result.map_err(|error| redact(error, secret)),
                });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        for request in requests.try_iter().take(32) {
            let response = if stopped {
                Response::error(
                    "This editor session is stopped. Press Play to resume local control.",
                )
            } else if shared_paused && !super::permitted_while_paused(&request) {
                Response::error(
                    "The shared simulation is paused. Ask the host to press Play before operating the ship.",
                )
            } else {
                match peer.game(request) {
                    Ok(response) => response,
                    Err(error) => Response::error(redact(error, secret)),
                }
            };
            if updates
                .send(Update::Response(Box::new(response), true))
                .is_err()
            {
                return Ok(());
            }
        }

        if stopped || shared_paused {
            controls.clear();
        } else if let Some(request) = controls.take() {
            // Input is transient and never replayed after pause/stop/reconnect.
            let _ = peer.game(request);
        }

        let shared = match peer.snapshot() {
            Ok(shared) => {
                consecutive_busy_snapshots = 0;
                busy_notice_sent = false;
                shared
            }
            Err(error) if is_authority_busy(&error) => {
                consecutive_busy_snapshots = consecutive_busy_snapshots.saturating_add(1);
                if consecutive_busy_snapshots >= 20 && !busy_notice_sent {
                    let notice = Update::Response(
                        Box::new(Response::error(
                            "Shared editor authority is busy; snapshot polling will keep waiting.",
                        )),
                        false,
                    );
                    let _ = updates.try_send(notice);
                    busy_notice_sent = true;
                }
                continue;
            }
            Err(error) => return Err(redact(error, secret)),
        };
        shared_paused = shared.paused;
        let status = if stopped {
            EmbeddedStatus::Stopped
        } else if shared.paused {
            EmbeddedStatus::Paused
        } else {
            EmbeddedStatus::Running
        };
        if status != last_status {
            let _ = states.send(status.clone());
            last_status = status;
        }

        // The collaborative polling view includes both gameplay state and
        // assets, so this stays one bounded authority request per tick.
        let response = Response {
            ok: true,
            message: String::new(),
            snapshot: Some(shared.snapshot),
            assets: shared.assets,
            auth: None,
            resume_token: None,
            ledger: None,
        };
        let _ = updates.try_send(Update::Response(Box::new(response), false));

        let graph_state = EmbeddedEventDebugState {
            debug: shared.graph,
            ack: graph_ack.clone(),
            room_ack: room_ack.clone(),
            scene_ack: scene_ack.clone(),
        };
        match graph_states.try_send(graph_state) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => break,
        }
    }
    Ok(())
}

fn send_error(updates: &SyncSender<Update>, message: String) -> Result<(), String> {
    updates
        .send(Update::Response(Box::new(Response::error(message)), false))
        .map_err(|error| error.to_string())
}

fn is_authority_busy(error: &str) -> bool {
    error == "editor authority is busy"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time after epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "arkline-shared-editor-{}-{stamp}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create isolated editor directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn host_and_join_share_state_but_only_host_controls_clock() {
        let directory = TestDirectory::new();
        let OpenedPeer {
            peer: mut host,
            connect_response: host_response,
            info,
            ..
        } = open_peer(
            &directory.0,
            "HostPilot",
            EmbeddedSessionMode::Host {
                bind_addr: "127.0.0.1:0".parse().expect("loopback socket address"),
            },
        )
        .expect("start shared editor host");
        assert!(host_response.ok, "host pilot should connect");
        let address = info.address.expect("host address");
        let secret = info.editor_secret.expect("host join secret");

        let OpenedPeer {
            peer: mut join,
            connect_response: join_response,
            ..
        } = open_peer(
            &directory.0,
            "GuestPilot",
            EmbeddedSessionMode::Join { address, secret },
        )
        .expect("join shared editor host");
        assert!(join_response.ok, "joined pilot should connect");

        let host_snapshot = host.snapshot().expect("read host snapshot");
        let join_snapshot = join.snapshot().expect("read joined snapshot");
        let host_pilots = host_snapshot
            .snapshot
            .pilots
            .iter()
            .map(|pilot| pilot.name.as_str())
            .collect::<Vec<_>>();
        let joined_pilots = join_snapshot
            .snapshot
            .pilots
            .iter()
            .map(|pilot| pilot.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(host_pilots, joined_pilots);
        assert!(host_pilots.contains(&"HostPilot"));
        assert!(host_pilots.contains(&"GuestPilot"));
        assert!(host_snapshot.paused);
        assert!(join.clock(ClockAction::Pause).is_err());

        host.clock(ClockAction::Step { delta_ms: 50 })
            .expect("host can step shared clock");
        let stepped_host = host.snapshot().expect("read stepped host state");
        let stepped_join = join.snapshot().expect("read stepped joined state");
        assert_eq!(
            stepped_host.simulation_time_ms,
            host_snapshot.simulation_time_ms + 50
        );
        assert_eq!(
            stepped_host.simulation_time_ms,
            stepped_join.simulation_time_ms
        );

        join.close().expect("leave shared session");
        host.close().expect("stop shared authority cleanly");
    }

    #[test]
    fn only_explicit_busy_snapshot_error_is_retryable() {
        assert!(is_authority_busy("editor authority is busy"));
        assert!(!is_authority_busy("editor authority stopped"));
        assert!(!is_authority_busy(
            "editor authority response deadline; command outcome is unknown"
        ));
    }
}
