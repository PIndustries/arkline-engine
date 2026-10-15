//! Loopback-only authenticated development transport for one shared editor world.
//! This deliberately is not the production game listener or a LAN service.
use crate::collaborative_editor::{
    CollaborationRole, CollaborativeEditorHost, CollaborativeEditorSnapshot, EditorSessionId,
    MAX_COLLABORATIVE_EDITOR_SESSIONS,
};
use anyhow::{ensure, Context, Result};
use arkline_protocol::{
    EditorEventGraph, EditorSceneState, ExteriorDefinition, ExteriorDefinitionState, Request,
    Response, RoomLayoutDocument, RoomLayoutState,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_ACCEPTED_WORKERS: usize = MAX_COLLABORATIVE_EDITOR_SESSIONS + 8;
const REQUEST_BURST: f64 = 120.0;
const REQUESTS_PER_SECOND: f64 = 60.0;
const IO_DEADLINE: Duration = Duration::from_secs(30);
const CLIENT_IO_DEADLINE: Duration = Duration::from_secs(5);
const ACTOR_REPLY_DEADLINE: Duration = Duration::from_secs(5);
const ACTOR_QUEUE: usize = 128;
const EDITOR_TICK: Duration = Duration::from_millis(50);

/// Host-generated bearer secrets. Fields intentionally stay private and Debug
/// never reveals the values; call the explicit accessors only to share them.
#[derive(Clone, PartialEq, Eq)]
pub struct DevelopmentEditorJoinSecrets {
    editor: String,
    viewer: String,
}
impl DevelopmentEditorJoinSecrets {
    pub fn injected(editor: impl Into<String>, viewer: impl Into<String>) -> Self {
        Self {
            editor: editor.into(),
            viewer: viewer.into(),
        }
    }
    fn random() -> Self {
        fn secret() -> String {
            let mut bytes = [0u8; 32];
            OsRng.fill_bytes(&mut bytes);
            hex::encode(bytes)
        }
        Self {
            editor: secret(),
            viewer: secret(),
        }
    }
    pub fn editor_secret(&self) -> &str {
        &self.editor
    }
    pub fn viewer_secret(&self) -> &str {
        &self.viewer
    }
}
impl fmt::Debug for DevelopmentEditorJoinSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DevelopmentEditorJoinSecrets")
            .field("editor", &"[REDACTED]")
            .field("viewer", &"[REDACTED]")
            .finish()
    }
}

pub struct DevelopmentEditorConfig {
    pub bind_addr: SocketAddr,
    pub directory: PathBuf,
    pub host_connect: Request,
    pub join_secrets: Option<DevelopmentEditorJoinSecrets>,
}
impl DevelopmentEditorConfig {
    pub fn new(directory: impl Into<PathBuf>, host_connect: Request) -> Self {
        Self {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 7779)),
            directory: directory.into(),
            host_connect,
            join_secrets: None,
        }
    }
    pub fn with_bind_addr(mut self, bind_addr: SocketAddr) -> Self {
        self.bind_addr = bind_addr;
        self
    }
    pub fn with_join_secrets(mut self, secrets: DevelopmentEditorJoinSecrets) -> Self {
        self.join_secrets = Some(secrets);
        self
    }
}

/// Shared host handle. The simulation and every socket request are serialized
/// on one actor thread. Dropping this value requests shutdown without joining a
/// worker, so it is safe to drop from a UI thread.
pub struct DevelopmentEditorHost {
    address: SocketAddr,
    secrets: DevelopmentEditorJoinSecrets,
    host_response: Response,
    host_session: EditorSessionId,
    actor: SyncSender<ActorCommand>,
    stop: Arc<AtomicBool>,
    shutdown_complete: AtomicBool,
    completion: Mutex<Receiver<()>>,
}
impl fmt::Debug for DevelopmentEditorHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DevelopmentEditorHost")
            .field("address", &self.address)
            .field("secrets", &"[REDACTED]")
            .field("host_response", &"[REDACTED]")
            .finish()
    }
}
impl DevelopmentEditorHost {
    pub fn bind(config: DevelopmentEditorConfig) -> Result<Self> {
        ensure!(
            config.bind_addr.ip().is_loopback(),
            "development editor transport binds to loopback only"
        );
        ensure!(
            matches!(&config.host_connect, Request::Connect { .. }),
            "host_connect must be a Connect request"
        );
        let listener =
            TcpListener::bind(config.bind_addr).context("bind editor loopback listener")?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let secrets = config
            .join_secrets
            .unwrap_or_else(DevelopmentEditorJoinSecrets::random);
        ensure!(
            !secrets.editor.is_empty()
                && !secrets.viewer.is_empty()
                && secrets.editor.len() <= 128
                && secrets.viewer.len() <= 128
                && !constant_time_equal(secrets.editor.as_bytes(), secrets.viewer.as_bytes()),
            "editor and viewer join secrets must be distinct nonempty values of at most 128 bytes"
        );

        let mut authority = CollaborativeEditorHost::open(&config.directory)?;
        let (host_session, host_response) =
            authority.join(CollaborationRole::Host, config.host_connect)?;
        let (actor, receiver) = mpsc::sync_channel(ACTOR_QUEUE);
        let stop = Arc::new(AtomicBool::new(false));
        let actor_stop = stop.clone();
        let actor_secrets = secrets.clone();
        let (completion_tx, completion_rx) = mpsc::channel();
        thread::Builder::new()
            .name("arkline-editor-authority".into())
            .spawn(move || {
                actor_loop(
                    authority,
                    host_session,
                    actor_secrets,
                    receiver,
                    actor_stop,
                    completion_tx,
                )
            })
            .context("start editor authority actor")?;

        let worker_count = Arc::new(AtomicUsize::new(0));
        let accept_stop = stop.clone();
        let accept_actor = actor.clone();
        let acceptor = thread::Builder::new()
            .name("arkline-editor-loopback".into())
            .spawn(move || {
                accept_loop(listener, accept_actor, accept_stop, worker_count);
            });
        if let Err(error) = acceptor {
            stop.store(true, Ordering::Release);
            return Err(error).context("start editor loopback listener");
        }

        Ok(Self {
            address,
            secrets,
            host_response,
            host_session,
            actor,
            stop,
            shutdown_complete: AtomicBool::new(false),
            completion: Mutex::new(completion_rx),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.address
    }

    /// Explicit secret export point. Never log or format this value.
    pub fn expose_join_secrets(&self) -> &DevelopmentEditorJoinSecrets {
        &self.secrets
    }

    /// Includes the Connect response/token for the host's existing credential store.
    pub fn host_connect_response(&self) -> Response {
        self.host_response.clone()
    }

    /// Stop the authority and wait for its world/database to be dropped. Call only
    /// from a worker thread; UI-thread Drop remains nonblocking.
    pub fn shutdown_and_wait_from_worker(&self) -> Result<()> {
        self.stop.store(true, Ordering::Release);
        let _ = self.actor.try_send(ActorCommand::Shutdown);
        if self.shutdown_complete.load(Ordering::Acquire) {
            return Ok(());
        }
        let receiver = self
            .completion
            .lock()
            .map_err(|_| anyhow::anyhow!("shutdown signal poisoned"))?;
        if self.shutdown_complete.load(Ordering::Acquire) {
            return Ok(());
        }
        receiver
            .recv()
            .context("editor authority stopped before shutdown completion signal")?;
        self.shutdown_complete.store(true, Ordering::Release);
        Ok(())
    }

    pub fn snapshot(&self) -> Result<CollaborativeEditorSnapshot> {
        expect_snapshot(actor_call(
            &self.actor,
            Some(self.host_session),
            ActorAction::Snapshot,
        )?)
    }
    pub fn game(&self, request: Request) -> Result<Response> {
        expect_game(actor_call(
            &self.actor,
            Some(self.host_session),
            ActorAction::Game(request),
        )?)
    }
    pub fn clock(&self, action: ClockAction) -> Result<()> {
        expect_accepted(actor_call(
            &self.actor,
            Some(self.host_session),
            ActorAction::Clock(action),
        )?)
    }
    pub fn replace_graph(
        &self,
        expected_revision: u64,
        graph: EditorEventGraph,
        clear_pending: bool,
    ) -> Result<()> {
        self.graph(GraphAction::Replace {
            expected_revision,
            graph,
            clear_pending,
        })
    }
    pub fn trigger_graph(&self, node_id: u64) -> Result<()> {
        self.graph(GraphAction::Trigger { node_id })
    }
    pub fn step_graph(&self) -> Result<()> {
        self.graph(GraphAction::Step)
    }
    pub fn clear_graph(&self) -> Result<()> {
        self.graph(GraphAction::Clear)
    }
    pub fn set_scene_state(&self, state: EditorSceneState) -> Result<()> {
        expect_accepted(actor_call(
            &self.actor,
            Some(self.host_session),
            ActorAction::SceneState(state),
        )?)
    }
    pub fn replace_room_layout(
        &self,
        expected_revision: u64,
        document: RoomLayoutDocument,
    ) -> Result<RoomLayoutState> {
        match actor_call(
            &self.actor,
            Some(self.host_session),
            ActorAction::RoomLayoutReplace {
                expected_revision,
                document,
            },
        )? {
            ActorOutput::RoomLayout(state) => Ok(state),
            _ => Err(anyhow::anyhow!("unexpected editor authority response")),
        }
    }
    pub fn replace_exterior(
        &self,
        expected_revision: u64,
        document: ExteriorDefinition,
    ) -> Result<ExteriorDefinitionState> {
        match actor_call(
            &self.actor,
            Some(self.host_session),
            ActorAction::ExteriorReplace {
                expected_revision,
                document,
            },
        )? {
            ActorOutput::Exterior(state) => Ok(state),
            _ => Err(anyhow::anyhow!("unexpected editor authority response")),
        }
    }
    pub fn set_exterior_enabled(&self, enabled: bool, expected_revision: u64) -> Result<()> {
        expect_accepted(actor_call(
            &self.actor,
            Some(self.host_session),
            ActorAction::ExteriorEnabled {
                enabled,
                expected_revision,
            },
        )?)
    }
    fn graph(&self, action: GraphAction) -> Result<()> {
        expect_accepted(actor_call(
            &self.actor,
            Some(self.host_session),
            ActorAction::Graph(action),
        )?)
    }
}
impl Drop for DevelopmentEditorHost {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.actor.try_send(ActorCommand::Shutdown);
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ClockAction {
    Play,
    Pause,
    Step { delta_ms: u64 },
}

/// Synchronous newline-JSON client intended for the embedded worker adapter.
pub struct DevelopmentEditorClient {
    stream: TcpStream,
    role: CollaborationRole,
    closed: bool,
}
impl fmt::Debug for DevelopmentEditorClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DevelopmentEditorClient")
            .field("role", &self.role)
            .field("secret", &"[REDACTED]")
            .finish()
    }
}
impl DevelopmentEditorClient {
    /// `connect` is required for host/editor secrets and forbidden for viewers.
    /// The server selects the role from the secret; the client cannot request one.
    pub fn connect(
        address: SocketAddr,
        secret: &str,
        connect: Option<Request>,
    ) -> Result<(Self, Response)> {
        ensure!(
            address.ip().is_loopback(),
            "development editor client connects to loopback only"
        );
        let mut stream = TcpStream::connect_timeout(&address, CLIENT_IO_DEADLINE)
            .context("connect to editor loopback host")?;
        stream.set_nodelay(true)?;
        let reply = client_exchange(
            &mut stream,
            &WireRequest::Join {
                secret: secret.to_owned(),
                connect,
            },
        )?;
        let (role, response) = match reply {
            WireReply::Joined { role, response } => (role, response),
            other => return Err(anyhow::anyhow!(reply_error(other))),
        };
        Ok((
            Self {
                stream,
                role,
                closed: false,
            },
            response,
        ))
    }
    pub fn role(&self) -> CollaborationRole {
        self.role
    }
    pub fn snapshot(&mut self) -> Result<CollaborativeEditorSnapshot> {
        match self.exchange(WireRequest::Snapshot)? {
            WireReply::State { snapshot } => Ok(snapshot),
            reply => Err(self.unexpected_reply(reply)),
        }
    }
    pub fn game(&mut self, request: Request) -> Result<Response> {
        match self.exchange(WireRequest::Game { request })? {
            WireReply::Game { response } => Ok(response),
            reply => Err(self.unexpected_reply(reply)),
        }
    }
    pub fn replace_graph(
        &mut self,
        expected_revision: u64,
        graph: EditorEventGraph,
        clear_pending: bool,
    ) -> Result<()> {
        self.graph(WireRequest::GraphReplace {
            expected_revision,
            graph,
            clear_pending,
        })
    }
    pub fn trigger_graph(&mut self, node_id: u64) -> Result<()> {
        self.graph(WireRequest::GraphTrigger { node_id })
    }
    pub fn step_graph(&mut self) -> Result<()> {
        self.graph(WireRequest::GraphStep)
    }
    pub fn clear_graph(&mut self) -> Result<()> {
        self.graph(WireRequest::GraphClear)
    }
    pub fn set_scene_state(&mut self, state: EditorSceneState) -> Result<()> {
        self.graph(WireRequest::SceneState { state })
    }
    pub fn replace_room_layout(
        &mut self,
        expected_revision: u64,
        document: RoomLayoutDocument,
    ) -> Result<RoomLayoutState> {
        match self.exchange(WireRequest::RoomLayoutReplace {
            expected_revision,
            document,
        })? {
            WireReply::RoomLayout { state } => Ok(state),
            reply => Err(self.unexpected_reply(reply)),
        }
    }
    pub fn replace_exterior(
        &mut self,
        expected_revision: u64,
        document: ExteriorDefinition,
    ) -> Result<ExteriorDefinitionState> {
        match self.exchange(WireRequest::ExteriorReplace {
            expected_revision,
            document,
        })? {
            WireReply::Exterior { state } => Ok(state),
            reply => Err(self.unexpected_reply(reply)),
        }
    }
    pub fn set_exterior_enabled(&mut self, enabled: bool, expected_revision: u64) -> Result<()> {
        self.graph(WireRequest::ExteriorEnabled {
            enabled,
            expected_revision,
        })
    }
    pub fn leave(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        match self.exchange(WireRequest::Leave)? {
            WireReply::Accepted => {
                self.closed = true;
                Ok(())
            }
            reply => Err(self.unexpected_reply(reply)),
        }
    }
    fn graph(&mut self, command: WireRequest) -> Result<()> {
        match self.exchange(command)? {
            WireReply::Accepted => Ok(()),
            reply => Err(self.unexpected_reply(reply)),
        }
    }
    fn exchange(&mut self, request: WireRequest) -> Result<WireReply> {
        ensure!(!self.closed, "editor client has left");
        match client_exchange(&mut self.stream, &request) {
            Ok(WireReply::Error { message }) => {
                if remote_error_is_uncertain(&message) {
                    self.poison();
                }
                Err(anyhow::anyhow!(message))
            }
            Ok(reply) => Ok(reply),
            Err(error) => {
                self.poison();
                Err(error)
            }
        }
    }
    fn unexpected_reply(&mut self, reply: WireReply) -> anyhow::Error {
        self.poison();
        anyhow::anyhow!(reply_error(reply))
    }
    fn poison(&mut self) {
        self.closed = true;
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireRequest {
    Join {
        secret: String,
        #[serde(default)]
        connect: Option<Request>,
    },
    Snapshot,
    Game {
        request: Request,
    },
    Clock {
        action: ClockAction,
    },
    GraphReplace {
        expected_revision: u64,
        graph: EditorEventGraph,
        clear_pending: bool,
    },
    GraphTrigger {
        node_id: u64,
    },
    GraphStep,
    GraphClear,
    SceneState {
        state: EditorSceneState,
    },
    RoomLayoutReplace {
        expected_revision: u64,
        document: RoomLayoutDocument,
    },
    ExteriorReplace {
        expected_revision: u64,
        document: ExteriorDefinition,
    },
    ExteriorEnabled {
        enabled: bool,
        expected_revision: u64,
    },
    Leave,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireReply {
    Joined {
        role: CollaborationRole,
        response: Response,
    },
    State {
        snapshot: CollaborativeEditorSnapshot,
    },
    Game {
        response: Response,
    },
    Accepted,
    RoomLayout {
        state: RoomLayoutState,
    },
    Exterior {
        state: ExteriorDefinitionState,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone)]
enum GraphAction {
    Replace {
        expected_revision: u64,
        graph: EditorEventGraph,
        clear_pending: bool,
    },
    Trigger {
        node_id: u64,
    },
    Step,
    Clear,
}
#[derive(Clone)]
enum ActorAction {
    Join {
        secret: String,
        connect: Option<Request>,
    },
    Snapshot,
    Game(Request),
    Clock(ClockAction),
    Graph(GraphAction),
    SceneState(EditorSceneState),
    RoomLayoutReplace {
        expected_revision: u64,
        document: RoomLayoutDocument,
    },
    ExteriorReplace {
        expected_revision: u64,
        document: ExteriorDefinition,
    },
    ExteriorEnabled {
        enabled: bool,
        expected_revision: u64,
    },
    Leave,
}
enum ActorOutput {
    Joined(EditorSessionId, CollaborationRole, Response),
    Snapshot(CollaborativeEditorSnapshot),
    Game(Response),
    Accepted,
    RoomLayout(RoomLayoutState),
    Exterior(ExteriorDefinitionState),
}
enum ActorCommand {
    Call {
        session: Option<EditorSessionId>,
        action: ActorAction,
        deadline: Instant,
        cancelled: Arc<AtomicBool>,
        reply: SyncSender<std::result::Result<ActorOutput, String>>,
    },
    CleanupLeave(EditorSessionId),
    Shutdown,
}

fn actor_call(
    actor: &SyncSender<ActorCommand>,
    session: Option<EditorSessionId>,
    action: ActorAction,
) -> Result<ActorOutput> {
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let deadline = Instant::now() + ACTOR_REPLY_DEADLINE;
    let cancelled = Arc::new(AtomicBool::new(false));
    actor
        .try_send(ActorCommand::Call {
            session,
            action,
            deadline,
            cancelled: cancelled.clone(),
            reply: reply_tx,
        })
        .map_err(|err| match err {
            TrySendError::Full(_) => anyhow::anyhow!("editor authority is busy"),
            TrySendError::Disconnected(_) => anyhow::anyhow!("editor authority stopped"),
        })?;
    match reply_rx.recv_timeout(ACTOR_REPLY_DEADLINE) {
        Ok(result) => result.map_err(anyhow::Error::msg),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            cancelled.store(true, Ordering::Release);
            Err(anyhow::anyhow!(
                "editor authority response deadline; command outcome is unknown"
            ))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(anyhow::anyhow!(
            "editor authority stopped before responding"
        )),
    }
}

fn expect_snapshot(output: ActorOutput) -> Result<CollaborativeEditorSnapshot> {
    match output {
        ActorOutput::Snapshot(snapshot) => Ok(snapshot),
        _ => Err(anyhow::anyhow!("unexpected editor authority response")),
    }
}
fn expect_game(output: ActorOutput) -> Result<Response> {
    match output {
        ActorOutput::Game(response) => Ok(response),
        _ => Err(anyhow::anyhow!("unexpected editor authority response")),
    }
}
fn expect_accepted(output: ActorOutput) -> Result<()> {
    match output {
        ActorOutput::Accepted => Ok(()),
        _ => Err(anyhow::anyhow!("unexpected editor authority response")),
    }
}

fn actor_loop(
    mut authority: CollaborativeEditorHost,
    host_session: EditorSessionId,
    secrets: DevelopmentEditorJoinSecrets,
    receiver: Receiver<ActorCommand>,
    stop: Arc<AtomicBool>,
    completion: mpsc::Sender<()>,
) {
    let mut last_advance = Instant::now();
    let mut next_tick = last_advance + EDITOR_TICK;
    while !stop.load(Ordering::Acquire) {
        let timeout = next_tick.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(timeout) {
            Ok(ActorCommand::Shutdown) => break,
            Ok(ActorCommand::Call {
                session,
                action,
                deadline,
                cancelled,
                reply,
            }) => {
                let result = if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
                    Err("editor authority request cancelled before dispatch".into())
                } else {
                    dispatch(&mut authority, host_session, &secrets, session, action)
                };
                if let Err(mpsc::TrySendError::Disconnected(Ok(ActorOutput::Joined(id, _, _)))) =
                    reply.try_send(result)
                {
                    // A join that completed after its caller timed out must not
                    // leave an unreachable pilot/session in the shared world.
                    let _ = authority.leave(id);
                }
            }
            Ok(ActorCommand::CleanupLeave(id)) => {
                let _ = authority.leave(id);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // Advance by real monotonic elapsed time independently of request traffic.
        // A delayed actor catches up in bounded chunks, then schedules the next tick.
        let now = Instant::now();
        if now >= next_tick {
            let elapsed_ms = now.duration_since(last_advance).as_millis().min(250) as u64;
            if elapsed_ms > 0 {
                let _ = authority.advance(host_session, elapsed_ms);
            }
            last_advance = now;
            next_tick = now + EDITOR_TICK;
        }
    }
    drop(authority);
    let _ = completion.send(());
}

fn dispatch(
    authority: &mut CollaborativeEditorHost,
    host_session: EditorSessionId,
    secrets: &DevelopmentEditorJoinSecrets,
    session: Option<EditorSessionId>,
    action: ActorAction,
) -> std::result::Result<ActorOutput, String> {
    let result = (|| -> Result<ActorOutput> {
        match action {
            ActorAction::Join { secret, connect } => {
                let role = if constant_time_equal(secret.as_bytes(), secrets.editor.as_bytes()) {
                    CollaborationRole::Editor
                } else if constant_time_equal(secret.as_bytes(), secrets.viewer.as_bytes()) {
                    CollaborationRole::Viewer
                } else {
                    anyhow::bail!("invalid editor join secret")
                };
                let request = match (role, connect) {
                    (CollaborationRole::Editor, Some(request @ Request::Connect { .. })) => request,
                    (CollaborationRole::Viewer, None) => Request::Snapshot,
                    (CollaborationRole::Editor, _) => anyhow::bail!("editor join requires Connect"),
                    (CollaborationRole::Viewer, _) => {
                        anyhow::bail!("viewer join accepts no game session")
                    }
                    _ => unreachable!(),
                };
                let (id, response) = authority.join(role, request)?;
                Ok(ActorOutput::Joined(id, role, response))
            }
            ActorAction::Snapshot => {
                let id = session.context("session required")?;
                Ok(ActorOutput::Snapshot(authority.snapshot(id)?))
            }
            ActorAction::Game(request) => {
                let id = session.context("session required")?;
                if authority.is_paused()
                    && !matches!(
                        &request,
                        Request::Snapshot | Request::ExportLedger | Request::AuthCapabilities
                    )
                {
                    anyhow::bail!("game requests are unavailable while the editor clock is paused");
                }
                Ok(ActorOutput::Game(authority.request(id, request)?))
            }
            ActorAction::Clock(action) => {
                let id = session.context("session required")?;
                ensure!(id == host_session, "host session required");
                match action {
                    ClockAction::Play => authority.play(id)?,
                    ClockAction::Pause => authority.pause(id)?,
                    ClockAction::Step { delta_ms } => authority.step(id, delta_ms)?,
                }
                Ok(ActorOutput::Accepted)
            }
            ActorAction::Graph(action) => {
                let id = session.context("session required")?;
                match action {
                    GraphAction::Replace {
                        expected_revision,
                        graph,
                        clear_pending,
                    } => authority.replace_graph(id, expected_revision, graph, clear_pending)?,
                    GraphAction::Trigger { node_id } => authority.trigger_graph(id, node_id)?,
                    GraphAction::Step => authority.step_graph(id)?,
                    GraphAction::Clear => authority.clear_graph(id)?,
                }
                Ok(ActorOutput::Accepted)
            }
            ActorAction::SceneState(state) => {
                authority.set_scene_state(session.context("session required")?, state)?;
                Ok(ActorOutput::Accepted)
            }
            ActorAction::RoomLayoutReplace {
                expected_revision,
                document,
            } => {
                let id = session.context("session required")?;
                Ok(ActorOutput::RoomLayout(authority.room_layout_replace(
                    id,
                    expected_revision,
                    document,
                )?))
            }
            ActorAction::ExteriorReplace {
                expected_revision,
                document,
            } => Ok(ActorOutput::Exterior(authority.exterior_replace(
                session.context("session required")?,
                expected_revision,
                document,
            )?)),
            ActorAction::ExteriorEnabled {
                enabled,
                expected_revision,
            } => {
                authority.exterior_set_enabled(
                    session.context("session required")?,
                    enabled,
                    expected_revision,
                )?;
                Ok(ActorOutput::Accepted)
            }
            ActorAction::Leave => {
                let id = session.context("session required")?;
                ensure!(
                    id != host_session,
                    "the local host session cannot leave remotely"
                );
                authority.leave(id)?;
                Ok(ActorOutput::Accepted)
            }
        }
    })();
    result.map_err(|error| error.to_string())
}

fn accept_loop(
    listener: TcpListener,
    actor: SyncSender<ActorCommand>,
    stop: Arc<AtomicBool>,
    workers: Arc<AtomicUsize>,
) {
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, peer)) => {
                if !peer.ip().is_loopback() || !reserve_worker(&workers) {
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                    continue;
                }
                let actor = actor.clone();
                let worker_guard = workers.clone();
                if thread::Builder::new()
                    .name("arkline-editor-peer".into())
                    .spawn(move || {
                        let _guard = WorkerGuard(worker_guard);
                        let _ = serve_peer(stream, actor);
                    })
                    .is_err()
                {
                    workers.fetch_sub(1, Ordering::AcqRel);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => thread::sleep(Duration::from_millis(25)),
        }
    }
}
fn reserve_worker(workers: &AtomicUsize) -> bool {
    workers
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            (current < MAX_ACCEPTED_WORKERS).then_some(current + 1)
        })
        .is_ok()
}
struct WorkerGuard(Arc<AtomicUsize>);
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct RequestRate {
    tokens: f64,
    last_refill: Instant,
}
impl RequestRate {
    fn new() -> Self {
        Self {
            tokens: REQUEST_BURST,
            last_refill: Instant::now(),
        }
    }
    fn take(&mut self) -> bool {
        let now = Instant::now();
        self.tokens = (self.tokens
            + now.duration_since(self.last_refill).as_secs_f64() * REQUESTS_PER_SECOND)
            .min(REQUEST_BURST);
        self.last_refill = now;
        if self.tokens < 1.0 {
            false
        } else {
            self.tokens -= 1.0;
            true
        }
    }
}

fn serve_peer(mut stream: TcpStream, actor: SyncSender<ActorCommand>) -> Result<()> {
    // Darwin may preserve O_NONBLOCK from the listening socket on accepted
    // streams. Deadline-based reads/writes below rely on blocking I/O plus
    // socket timeouts, so normalize every accepted peer explicitly.
    stream.set_nonblocking(false)?;
    stream.set_nodelay(true)?;
    let first = read_line_deadline(&mut stream, Instant::now() + IO_DEADLINE)?
        .context("peer closed before join")?;
    let request: WireRequest = serde_json::from_slice(&first).context("invalid editor request")?;
    let WireRequest::Join { secret, connect } = request else {
        write_line_deadline(
            &mut stream,
            &WireReply::Error {
                message: "first request must join".into(),
            },
            Instant::now() + IO_DEADLINE,
        )?;
        return Ok(());
    };
    let output = match actor_call(&actor, None, ActorAction::Join { secret, connect }) {
        Ok(output) => output,
        Err(error) => {
            write_line_deadline(
                &mut stream,
                &WireReply::Error {
                    message: error.to_string(),
                },
                Instant::now() + IO_DEADLINE,
            )?;
            return Ok(());
        }
    };
    let ActorOutput::Joined(id, role, response) = output else {
        return Err(anyhow::anyhow!("unexpected join response"));
    };
    let result = serve_joined_peer(&mut stream, &actor, id, role, response);
    if let Err(error) = &result {
        // Keep transport failures observable to the loopback client. This is
        // especially useful for bounded-frame/serialization failures, which
        // otherwise look like an unexplained peer disconnect.
        let _ = write_line_deadline(
            &mut stream,
            &WireReply::Error {
                message: format!("editor transport failed: {error}"),
            },
            Instant::now() + CLIENT_IO_DEADLINE,
        );
    }
    // Always clean up after admission, including failed join-response writes or
    // any later socket error, so disconnected sessions cannot leak authority slots.
    // This is a background peer worker. Blocking here is deliberate: a full
    // actor queue must delay cleanup rather than silently leak the session.
    let _ = actor.send(ActorCommand::CleanupLeave(id));
    result
}

fn serve_joined_peer(
    stream: &mut TcpStream,
    actor: &SyncSender<ActorCommand>,
    id: EditorSessionId,
    role: CollaborationRole,
    response: Response,
) -> Result<()> {
    write_line_deadline(
        stream,
        &WireReply::Joined { role, response },
        Instant::now() + IO_DEADLINE,
    )?;

    let mut request_rate = RequestRate::new();
    loop {
        let line = match read_line_deadline(stream, Instant::now() + IO_DEADLINE) {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(error) => return Err(error).context("read editor peer request"),
        };
        if !request_rate.take() {
            let _ = write_line_deadline(
                stream,
                &WireReply::Error {
                    message: "editor request rate exceeded".into(),
                },
                Instant::now() + IO_DEADLINE,
            );
            break;
        }
        let request: WireRequest = match serde_json::from_slice(&line) {
            Ok(request) => request,
            Err(error) => {
                write_line_deadline(
                    stream,
                    &WireReply::Error {
                        message: format!("invalid editor request: {error}"),
                    },
                    Instant::now() + IO_DEADLINE,
                )?;
                break;
            }
        };
        let (action, is_leave) = match request {
            WireRequest::Join { .. } => {
                write_line_deadline(
                    stream,
                    &WireReply::Error {
                        message: "connection is already joined".into(),
                    },
                    Instant::now() + IO_DEADLINE,
                )?;
                break;
            }
            WireRequest::Snapshot => (ActorAction::Snapshot, false),
            WireRequest::Game { request } => (ActorAction::Game(request), false),
            WireRequest::Clock { action } => (ActorAction::Clock(action), false),
            WireRequest::GraphReplace {
                expected_revision,
                graph,
                clear_pending,
            } => (
                ActorAction::Graph(GraphAction::Replace {
                    expected_revision,
                    graph,
                    clear_pending,
                }),
                false,
            ),
            WireRequest::GraphTrigger { node_id } => {
                (ActorAction::Graph(GraphAction::Trigger { node_id }), false)
            }
            WireRequest::GraphStep => (ActorAction::Graph(GraphAction::Step), false),
            WireRequest::GraphClear => (ActorAction::Graph(GraphAction::Clear), false),
            WireRequest::SceneState { state } => (ActorAction::SceneState(state), false),
            WireRequest::RoomLayoutReplace {
                expected_revision,
                document,
            } => (
                ActorAction::RoomLayoutReplace {
                    expected_revision,
                    document,
                },
                false,
            ),
            WireRequest::ExteriorReplace {
                expected_revision,
                document,
            } => (
                ActorAction::ExteriorReplace {
                    expected_revision,
                    document,
                },
                false,
            ),
            WireRequest::ExteriorEnabled {
                enabled,
                expected_revision,
            } => (
                ActorAction::ExteriorEnabled {
                    enabled,
                    expected_revision,
                },
                false,
            ),
            WireRequest::Leave => (ActorAction::Leave, true),
        };
        let output = match actor_call(actor, Some(id), action) {
            Ok(output) => output,
            Err(error) => {
                write_line_deadline(
                    stream,
                    &WireReply::Error {
                        message: error.to_string(),
                    },
                    Instant::now() + IO_DEADLINE,
                )?;
                if is_leave {
                    break;
                }
                continue;
            }
        };
        let reply = match output {
            ActorOutput::Snapshot(snapshot) => WireReply::State { snapshot },
            ActorOutput::Game(response) => WireReply::Game { response },
            ActorOutput::Accepted => WireReply::Accepted,
            ActorOutput::RoomLayout(state) => WireReply::RoomLayout { state },
            ActorOutput::Exterior(state) => WireReply::Exterior { state },
            ActorOutput::Joined(..) => WireReply::Error {
                message: "unexpected join response".into(),
            },
        };
        write_line_deadline(stream, &reply, Instant::now() + IO_DEADLINE)?;
        if is_leave {
            break;
        }
    }
    Ok(())
}

fn client_exchange(stream: &mut TcpStream, request: &WireRequest) -> Result<WireReply> {
    client_exchange_with_timeout(stream, request, CLIENT_IO_DEADLINE)
}
fn client_exchange_with_timeout(
    stream: &mut TcpStream,
    request: &WireRequest,
    timeout: Duration,
) -> Result<WireReply> {
    let result = (|| {
        let deadline = Instant::now() + timeout;
        write_line_deadline(stream, request, deadline)?;
        let line = read_line_deadline(stream, Instant::now() + timeout)?
            .context("editor host closed the connection")?;
        serde_json::from_slice(&line).context("invalid editor host response")
    })();
    if result.is_err() {
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }
    result
}
fn reply_error(reply: WireReply) -> String {
    match reply {
        WireReply::Error { message } => message,
        _ => "unexpected editor host response".into(),
    }
}
fn remote_error_is_uncertain(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    (message.contains("outcome") && message.contains("unknown"))
        || message.starts_with("editor transport failed:")
        || message.contains("authority stopped")
}

fn read_line_deadline(stream: &mut TcpStream, deadline: Instant) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "editor read deadline"))?;
        stream.set_read_timeout(Some(remaining))?;
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            if line.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete editor request",
            ));
        }
        if let Some(newline) = chunk[..read].iter().position(|byte| *byte == b'\n') {
            if line.len().saturating_add(newline) > MAX_FRAME_BYTES || newline + 1 != read {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "oversized or pipelined editor frame",
                ));
            }
            line.extend_from_slice(&chunk[..newline]);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(Some(line));
        }
        if line.len().saturating_add(read) > MAX_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "editor frame too large",
            ));
        }
        line.extend_from_slice(&chunk[..read]);
    }
}

struct BoundedFrame(Vec<u8>);
impl Write for BoundedFrame {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_FRAME_BYTES {
            return Err(io::Error::other("editor frame too large"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn write_line_deadline<T: Serialize>(
    stream: &mut TcpStream,
    value: &T,
    deadline: Instant,
) -> io::Result<()> {
    let mut frame = BoundedFrame(Vec::new());
    serde_json::to_writer(&mut frame, value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    frame.0.push(b'\n');
    let mut remaining = frame.0.as_slice();
    while !remaining.is_empty() {
        let timeout = deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "editor write deadline"))?;
        stream.set_write_timeout(Some(timeout))?;
        let written = stream.write(remaining)?;
        if written == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "editor peer closed",
            ));
        }
        remaining = &remaining[written..];
    }
    Ok(())
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use arkline_protocol::{default_pioneer_room_layout, PROTOCOL_VERSION};

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "arkline-editor-transport-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ))
    }
    fn connect(pilot: &str) -> Request {
        Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            pilot: pilot.into(),
            resume_token: None,
            supports_room_controls: true,
            supports_fleet_hangar: true,
            supports_pioneer_interior: true,
        }
    }

    #[test]
    fn exterior_paused_apply_and_owned_switches_agree_across_two_clients_and_viewer() {
        let directory = root();
        let secrets = DevelopmentEditorJoinSecrets::injected("exterior-editor", "exterior-viewer");
        let host = DevelopmentEditorHost::bind(
            DevelopmentEditorConfig::new(&directory, connect("ExteriorHost"))
                .with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
                .with_join_secrets(secrets.clone()),
        )
        .unwrap();
        let (mut editor, _) = DevelopmentEditorClient::connect(
            host.local_addr(),
            secrets.editor_secret(),
            Some(connect("ExteriorEditor")),
        )
        .unwrap();
        let (mut viewer, _) =
            DevelopmentEditorClient::connect(host.local_addr(), secrets.viewer_secret(), None)
                .unwrap();
        let initial_time = host.snapshot().unwrap().simulation_time_ms;
        let mut document = arkline_protocol::default_pioneer_exterior();
        document.fixtures[3].pulse.as_mut().unwrap().on_ms = 50;
        document.fixtures[3].pulse.as_mut().unwrap().phase_ms = (1000 - initial_time % 1000) % 1000;
        let accepted = editor.replace_exterior(0, document.clone()).unwrap();
        assert_eq!(accepted.revision, 1);
        let before = host.snapshot().unwrap();
        assert!(before.paused);
        assert_eq!(before.simulation_time_ms, initial_time);
        assert!(editor.replace_exterior(0, document.clone()).is_err());
        assert!(viewer.replace_exterior(1, document.clone()).is_err());
        assert!(viewer.set_exterior_enabled(false, 1).is_err());
        assert!(host.set_exterior_enabled(false, 0).is_err());
        host.set_exterior_enabled(false, 1).unwrap();
        let shared = host.snapshot().unwrap().snapshot.exterior.unwrap();
        assert_eq!(
            shared,
            editor.snapshot().unwrap().snapshot.exterior.unwrap()
        );
        assert_eq!(
            shared,
            viewer.snapshot().unwrap().snapshot.exterior.unwrap()
        );
        assert!(
            !shared
                .vessels
                .iter()
                .find(|v| v.pilot == "ExteriorHost")
                .unwrap()
                .powered
        );
        assert!(
            shared
                .vessels
                .iter()
                .find(|v| v.pilot == "ExteriorEditor")
                .unwrap()
                .powered
        );
        document.hull_asset_sha256 = "0".repeat(64);
        assert!(editor.replace_exterior(1, document).is_err());
        host.clock(ClockAction::Step { delta_ms: 50 }).unwrap();
        let after = viewer.snapshot().unwrap();
        assert!(after.paused);
        assert_eq!(after.simulation_time_ms, initial_time + 50);
        assert_eq!(
            after.snapshot.exterior,
            editor.snapshot().unwrap().snapshot.exterior
        );
        let state = after.snapshot.exterior.unwrap();
        let vessel = state
            .vessels
            .iter()
            .find(|v| v.pilot == "ExteriorEditor")
            .unwrap();
        assert_eq!(
            state.definition.document.fixtures[3].output(vessel, after.simulation_time_ms),
            0.0
        );
        editor.leave().unwrap();
        viewer.leave().unwrap();
        drop(editor);
        drop(viewer);
        host.shutdown_and_wait_from_worker().unwrap();
        drop(host);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn two_loopback_peers_share_clock_graph_and_role_authority() {
        let directory = root();
        let secrets = DevelopmentEditorJoinSecrets::injected("editor-test", "viewer-test");
        let host = DevelopmentEditorHost::bind(
            DevelopmentEditorConfig::new(directory.clone(), connect("TransportHost"))
                .with_bind_addr(SocketAddr::from(([127, 0, 0, 1], 0)))
                .with_join_secrets(secrets.clone()),
        )
        .unwrap();
        assert!(format!("{secrets:?}").contains("REDACTED"));
        assert!(!format!("{secrets:?}").contains("editor-test"));
        assert!(host.host_connect_response().resume_token.is_some());

        let (mut editor, editor_connect) = DevelopmentEditorClient::connect(
            host.local_addr(),
            secrets.editor_secret(),
            Some(connect("TransportEditor")),
        )
        .unwrap();
        assert!(editor_connect.resume_token.is_some());
        assert_eq!(editor.role(), CollaborationRole::Editor);
        let (mut viewer, _) =
            DevelopmentEditorClient::connect(host.local_addr(), secrets.viewer_secret(), None)
                .unwrap();
        assert_eq!(viewer.role(), CollaborationRole::Viewer);
        assert_eq!(viewer.snapshot().unwrap().snapshot.pilots.len(), 2);

        let scene_time = host.snapshot().unwrap().simulation_time_ms;
        editor
            .set_scene_state(EditorSceneState::InteriorDeck2)
            .unwrap();
        host.set_scene_state(EditorSceneState::StationHangar)
            .unwrap();
        let observed = viewer.snapshot().unwrap();
        assert_eq!(observed.simulation_time_ms, scene_time);
        let editor_avatar = observed
            .snapshot
            .pilots
            .iter()
            .find(|pilot| pilot.name == "TransportEditor")
            .unwrap()
            .station_avatar
            .as_ref()
            .unwrap();
        assert!(editor_avatar.onboard_interior);
        assert_eq!(editor_avatar.interior_deck, 2);
        assert!(
            !observed
                .snapshot
                .pilots
                .iter()
                .find(|pilot| pilot.name == "TransportHost")
                .unwrap()
                .station_avatar
                .as_ref()
                .unwrap()
                .onboard_interior
        );
        assert!(viewer.set_scene_state(EditorSceneState::Cockpit).is_err());
        assert_eq!(
            serde_json::to_value(viewer.snapshot().unwrap().snapshot).unwrap(),
            serde_json::to_value(observed.snapshot).unwrap()
        );
        host.set_scene_state(EditorSceneState::Cockpit).unwrap();
        editor.set_scene_state(EditorSceneState::Cockpit).unwrap();

        assert!(host.game(Request::Undock).is_err());
        assert!(host.game(Request::Snapshot).unwrap().snapshot.is_some());
        host.clock(ClockAction::Step { delta_ms: 50 }).unwrap();
        let editor_time = editor.snapshot().unwrap().simulation_time_ms;
        let viewer_time = viewer.snapshot().unwrap().simulation_time_ms;
        assert_eq!(editor_time, viewer_time);

        editor
            .replace_graph(0, EditorEventGraph::default(), false)
            .unwrap();
        assert_eq!(viewer.snapshot().unwrap().graph.revision, 1);
        assert!(viewer
            .replace_graph(0, EditorEventGraph::default(), true)
            .is_err());
        assert!(viewer.game(Request::Undock).is_err());

        let mut first_layout = default_pioneer_room_layout();
        first_layout.rooms[0].name.push_str(" shared");
        let first_state = editor.replace_room_layout(0, first_layout).unwrap();
        assert_eq!(first_state.revision, 1);
        assert_eq!(
            editor
                .snapshot()
                .unwrap()
                .assets
                .unwrap()
                .pioneer_room_layout,
            Some(first_state.clone())
        );
        assert!(editor
            .replace_room_layout(0, first_state.document.clone())
            .is_err());
        assert!(viewer
            .replace_room_layout(1, first_state.document.clone())
            .is_err());

        // Host-authorized geometry edits remain available while the shared
        // simulation is running; a second peer observes the same revision.
        host.clock(ClockAction::Play).unwrap();
        let mut second_layout = first_state.document.clone();
        second_layout.rooms[1].name.push_str(" running");
        let second_state = host.replace_room_layout(1, second_layout).unwrap();
        assert_eq!(second_state.revision, 2);
        assert_eq!(
            editor
                .snapshot()
                .unwrap()
                .assets
                .unwrap()
                .pioneer_room_layout,
            Some(second_state)
        );
        host.clock(ClockAction::Pause).unwrap();
        assert!(DevelopmentEditorClient::connect(
            host.local_addr(),
            "wrong-secret",
            Some(connect("RejectedPilot")),
        )
        .is_err());

        let (mut traffic_client, _) =
            DevelopmentEditorClient::connect(host.local_addr(), secrets.viewer_secret(), None)
                .unwrap();
        let polling = Arc::new(AtomicBool::new(true));
        let polling_thread = polling.clone();
        let traffic = thread::spawn(move || {
            while polling_thread.load(Ordering::Acquire) {
                if traffic_client.snapshot().is_err() {
                    break;
                }
            }
        });
        let before_busy_play = host.snapshot().unwrap().simulation_time_ms;
        host.clock(ClockAction::Play).unwrap();
        thread::sleep(Duration::from_millis(240));
        polling.store(false, Ordering::Release);
        traffic.join().unwrap();
        host.clock(ClockAction::Pause).unwrap();
        assert!(host.snapshot().unwrap().simulation_time_ms >= before_busy_play + 100);
        assert!(host.snapshot().unwrap().simulation_time_ms > editor_time);
        assert_eq!(
            editor.snapshot().unwrap().simulation_time_ms,
            viewer.snapshot().unwrap().simulation_time_ms
        );
        editor.leave().unwrap();
        viewer.leave().unwrap();
        drop(editor);
        drop(viewer);
        host.shutdown_and_wait_from_worker().unwrap();
        drop(host);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cancelled_calls_do_not_dispatch_and_abandoned_join_is_cleaned_up() {
        let directory = root();
        let mut authority = CollaborativeEditorHost::open(&directory).unwrap();
        let (host_session, _) = authority
            .join(CollaborationRole::Host, connect("CancellationHost"))
            .unwrap();
        let secrets = DevelopmentEditorJoinSecrets::injected("editor-cancel", "viewer-cancel");
        let (actor, receiver) = mpsc::sync_channel(ACTOR_QUEUE);
        let stop = Arc::new(AtomicBool::new(false));
        let actor_stop = stop.clone();
        let (completion_tx, completion_rx) = mpsc::channel();
        let thread = thread::spawn(move || {
            actor_loop(
                authority,
                host_session,
                secrets.clone(),
                receiver,
                actor_stop,
                completion_tx,
            );
        });

        let cancelled = Arc::new(AtomicBool::new(true));
        let (reply, response) = mpsc::sync_channel(1);
        assert!(actor
            .send(ActorCommand::Call {
                session: None,
                action: ActorAction::Join {
                    secret: "editor-cancel".into(),
                    connect: Some(connect("CancelledPilot")),
                },
                deadline: Instant::now() + ACTOR_REPLY_DEADLINE,
                cancelled,
                reply,
            })
            .is_ok());
        assert!(response
            .recv_timeout(ACTOR_REPLY_DEADLINE)
            .unwrap()
            .is_err());

        let (reply, response) = mpsc::sync_channel(1);
        assert!(actor
            .send(ActorCommand::Call {
                session: Some(host_session),
                action: ActorAction::Snapshot,
                deadline: Instant::now() + ACTOR_REPLY_DEADLINE,
                cancelled: Arc::new(AtomicBool::new(false)),
                reply,
            })
            .is_ok());
        let ActorOutput::Snapshot(snapshot) = response
            .recv_timeout(ACTOR_REPLY_DEADLINE)
            .unwrap()
            .unwrap_or_else(|error| panic!("authority response failed: {error}"))
        else {
            panic!("expected initial snapshot");
        };
        assert_eq!(snapshot.snapshot.pilots.len(), 1);

        let (reply, abandoned_response) = mpsc::sync_channel(1);
        assert!(actor
            .send(ActorCommand::Call {
                session: None,
                action: ActorAction::Join {
                    secret: "editor-cancel".into(),
                    connect: Some(connect("AbandonedPilot")),
                },
                deadline: Instant::now() + ACTOR_REPLY_DEADLINE,
                cancelled: Arc::new(AtomicBool::new(false)),
                reply,
            })
            .is_ok());
        drop(abandoned_response);

        let (reply, response) = mpsc::sync_channel(1);
        assert!(actor
            .send(ActorCommand::Call {
                session: Some(host_session),
                action: ActorAction::Snapshot,
                deadline: Instant::now() + ACTOR_REPLY_DEADLINE,
                cancelled: Arc::new(AtomicBool::new(false)),
                reply,
            })
            .is_ok());
        let ActorOutput::Snapshot(snapshot) = response
            .recv_timeout(ACTOR_REPLY_DEADLINE)
            .unwrap()
            .unwrap_or_else(|error| panic!("authority response failed: {error}"))
        else {
            panic!("expected snapshot after abandoned join");
        };
        // Disconnect preserves the abandoned pilot as offline persistent state;
        // only its collaboration session should be removed.
        assert_eq!(snapshot.snapshot.pilots.len(), 2);
        assert_eq!(
            snapshot.snapshot.online_pilots,
            vec!["CancellationHost".to_owned()]
        );

        // If the abandoned join leaked its session, this would reach the cap
        // one viewer early. All 31 remaining slots must still be available.
        for _ in 0..MAX_COLLABORATIVE_EDITOR_SESSIONS - 1 {
            let output = actor_call(
                &actor,
                None,
                ActorAction::Join {
                    secret: "viewer-cancel".into(),
                    connect: None,
                },
            )
            .unwrap();
            assert!(matches!(
                output,
                ActorOutput::Joined(_, CollaborationRole::Viewer, _)
            ));
        }
        assert!(actor_call(
            &actor,
            None,
            ActorAction::Join {
                secret: "viewer-cancel".into(),
                connect: None,
            },
        )
        .is_err());

        assert!(actor.send(ActorCommand::Shutdown).is_ok());
        completion_rx.recv().unwrap();
        thread.join().unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn malformed_or_late_rpc_reply_poisons_client_connection() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            read_line_deadline(&mut peer, Instant::now() + Duration::from_secs(1))
                .unwrap()
                .unwrap();
            thread::sleep(Duration::from_millis(40));
            peer.write_all(b"not-json\n").unwrap();
            let _ = peer.read_to_end(&mut request);
        });
        let stream = TcpStream::connect(address).unwrap();
        let mut client = DevelopmentEditorClient {
            stream,
            role: CollaborationRole::Editor,
            closed: false,
        };
        assert!(client.snapshot().is_err());
        assert!(client.closed);
        assert!(client.snapshot().unwrap_err().to_string().contains("left"));
        server.join().unwrap();

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            read_line_deadline(&mut peer, Instant::now() + Duration::from_secs(1))
                .unwrap()
                .unwrap();
            thread::sleep(Duration::from_millis(40));
            let _ = peer.write_all(b"{\"type\":\"error\",\"message\":\"late\"}\n");
            let mut request = Vec::new();
            let _ = peer.read_to_end(&mut request);
        });
        let mut stream = TcpStream::connect(address).unwrap();
        assert!(client_exchange_with_timeout(
            &mut stream,
            &WireRequest::Snapshot,
            Duration::from_millis(10),
        )
        .is_err());
        assert!(client_exchange_with_timeout(
            &mut stream,
            &WireRequest::Snapshot,
            Duration::from_millis(10),
        )
        .is_err());
        server.join().unwrap();
    }
}
