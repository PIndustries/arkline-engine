//! The editor owns the window; a dedicated worker owns the local authority and SQLite.
use super::*;
use arkline_protocol::{
    EditorEventGraph, EditorGraphDebug, EditorSceneState, ExteriorDefinition, RoomLayoutDocument,
};
use arkline_server::editor::EditorWorld;
use eframe::App;
use glam::Vec3;
#[path = "embedded_shared.rs"]
mod shared;

#[derive(Clone)]
pub enum EmbeddedSessionMode {
    Local,
    Host {
        bind_addr: std::net::SocketAddr,
    },
    Join {
        address: std::net::SocketAddr,
        secret: String,
    },
}
impl EmbeddedSessionMode {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Local => Ok(()),
            Self::Host { bind_addr } if bind_addr.ip().is_loopback() => Ok(()),
            Self::Join { address, secret } if address.ip().is_loopback()
                && !secret.is_empty() && secret.len() <= 128 => Ok(()),
            _ => Err("Use a loopback address and a valid editor secret. Remote development requires an SSH tunnel.".into()),
        }
    }
}
/// Selects local world storage; a joined session always uses its host's layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EmbeddedRealm {
    #[default]
    Project,
    OrbitalPreview,
}
impl EmbeddedRealm {
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "Project world",
            Self::OrbitalPreview => "Orbital station preview",
        }
    }
}
/// Runtime-only connection details. Credentials must never be logged or saved in project files.
pub struct EmbeddedSessionInfo {
    pub label: String,
    pub address: Option<std::net::SocketAddr>,
    pub editor_secret: Option<String>,
    pub is_host: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmbeddedStatus {
    Loading,
    Paused,
    Running,
    Stopped,
    Error(String),
}
/// Local renderer modes only; selecting one never changes server embodiment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddedView {
    Cockpit,
    External,
    Tactical,
    Tactical3d,
    Hangar,
    FirstPerson,
    ThirdPerson,
}
impl EmbeddedView {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cockpit => "Cockpit",
            Self::External => "3D External",
            Self::Tactical => "2D Tactical",
            Self::Tactical3d => "3D Tactical",
            Self::Hangar => "Hangar inspection",
            Self::FirstPerson => "On foot · first person",
            Self::ThirdPerson => "On foot · third person",
        }
    }
}
#[derive(Clone, Debug)]
pub struct ViewTransition {
    pub sequence: u64,
    pub from: Option<EmbeddedView>,
    pub to: Option<EmbeddedView>,
    pub reason: String,
    pub simulation_time_ms: Option<u64>,
    /// Monotonic host elapsed time since this embedded client was created.
    pub observed_at_ms: u64,
}
#[derive(Clone, Debug)]
pub struct EmbeddedViewState {
    pub current: Option<EmbeddedView>,
    pub available: Vec<EmbeddedView>,
    pub wireframe: bool,
    pub viewed_pilot: String,
    pub scene: String,
    pub transitions: Vec<ViewTransition>,
}
#[derive(Default)]
struct ViewDiagnostics {
    previous: Option<EmbeddedView>,
    sequence: u64,
    started: Option<Instant>,
    events: VecDeque<ViewTransition>,
    wireframe: bool,
}
#[derive(Clone, Debug)]
pub struct EmbeddedEventAck {
    pub id: u64,
    pub result: Result<(), String>,
}
#[derive(Clone, Debug)]
pub struct EmbeddedEventDebugState {
    pub debug: EditorGraphDebug,
    pub ack: Option<EmbeddedEventAck>,
    pub room_ack: Option<EmbeddedEventAck>,
    pub scene_ack: Option<EmbeddedEventAck>,
    pub exterior_ack: Option<EmbeddedEventAck>,
}
enum GraphCommand {
    Replace {
        revision: u64,
        graph: EditorEventGraph,
        clear_pending: bool,
    },
    Trigger(u64),
    Step,
    Clear,
}
enum ExteriorCommand {
    Replace(u64, ExteriorDefinition),
    Enable(bool, u64),
}
const STOP_REQUEST_NOTICE: &str = "Stop requested; waiting for local authority";

enum ClockCommand {
    Graph(u64, GraphCommand),
    RoomLayout(u64, u64, RoomLayoutDocument),
    SceneState(u64, EditorSceneState),
    Exterior(u64, ExteriorCommand),
    Play,
    Pause,
    Step,
    Stop,
    Shutdown,
}

pub struct EmbeddedClient {
    project_data_dir: PathBuf,
    data_dir: PathBuf,
    realm: EmbeddedRealm,
    mode: EmbeddedSessionMode,
    session_info: Option<EmbeddedSessionInfo>,
    session_updates: Receiver<EmbeddedSessionInfo>,
    worker: Option<std::thread::JoinHandle<()>>,
    cockpit: Cockpit,
    clock: mpsc::SyncSender<ClockCommand>,
    graph_states: Receiver<EmbeddedEventDebugState>,
    graph_state: Option<EmbeddedEventDebugState>,
    graph_pending: Option<u64>,
    graph_sequence: u64,
    room_pending: Option<u64>,
    room_ack: Option<EmbeddedEventAck>,
    room_sequence: u64,
    scene_pending: Option<(u64, EditorSceneState)>,
    scene_ack: Option<EmbeddedEventAck>,
    scene_sequence: u64,
    exterior_pending: Option<u64>,
    exterior_ack: Option<EmbeddedEventAck>,
    exterior_sequence: u64,
    session_generation: u64,
    states: Receiver<EmbeddedStatus>,
    state: EmbeddedStatus,
    destroyed: bool,
    views: ViewDiagnostics,
    free_camera: Option<(Vec3, f32, f32)>,
    free_camera_enabled: bool,
    free_camera_scene_hangar: Option<bool>,
    free_camera_saved_view: Option<ViewMode>,
    free_camera_input_time: Option<f64>,
}
impl EmbeddedClient {
    pub fn new(cc: &eframe::CreationContext<'_>, data_dir: PathBuf, pilot: String) -> Self {
        let mut cockpit = Cockpit::new(
            cc,
            "Embedded local world".into(),
            data_dir.join("client"),
            Some("embedded-editor".into()),
        );
        cockpit.embedded = true;
        cockpit.embedded_paused = true;
        cockpit.focus_on_first_frame = false;
        cockpit.boot.visible = false;
        cockpit.pilot = pilot.trim().to_owned();
        cockpit.connecting = true;
        cockpit.notice = "Opening local world…".into();
        cockpit.pilot_input.disable_capture();
        let connection = spawn_worker(
            data_dir.clone(),
            cockpit.pilot.clone(),
            EmbeddedSessionMode::Local,
            cockpit.controls.clone(),
            None,
        );
        cockpit.commands = Some(connection.commands);
        cockpit.updates = Some(connection.updates);
        Self {
            project_data_dir: data_dir.clone(),
            data_dir,
            realm: EmbeddedRealm::Project,
            mode: EmbeddedSessionMode::Local,
            session_info: None,
            session_updates: connection.info,
            worker: Some(connection.worker),
            cockpit,
            clock: connection.clock,
            graph_states: connection.graph_states,
            graph_state: None,
            graph_pending: None,
            graph_sequence: 0,
            room_pending: None,
            room_ack: None,
            room_sequence: 0,
            scene_pending: None,
            scene_ack: None,
            scene_sequence: 0,
            exterior_pending: None,
            exterior_ack: None,
            exterior_sequence: 0,
            session_generation: 0,
            states: connection.states,
            state: EmbeddedStatus::Loading,
            destroyed: false,
            views: ViewDiagnostics {
                started: Some(Instant::now()),
                ..Default::default()
            },
            free_camera: None,
            free_camera_enabled: false,
            free_camera_scene_hangar: None,
            free_camera_saved_view: None,
            free_camera_input_time: None,
        }
    }
    /// Drain authority updates while another editor workspace owns the viewport.
    /// This never renders, consumes egui input, or starts a new connection.
    pub fn graph_debug(&self) -> Option<&EmbeddedEventDebugState> {
        self.graph_state.as_ref()
    }
    pub fn session_generation(&self) -> u64 {
        self.session_generation
    }
    pub fn room_layout_ack(&self) -> Option<&EmbeddedEventAck> {
        self.room_ack.as_ref()
    }
    /// Confirmed own-pilot embodiment, read from the authority snapshot.
    pub fn scene_state(&self) -> Option<EditorSceneState> {
        let pilot = self
            .cockpit
            .snapshot
            .as_ref()?
            .pilots
            .iter()
            .find(|pilot| pilot.name == self.cockpit.pilot)?;
        if pilot.location != Location::Docked {
            return None;
        }
        Some(match pilot.station_avatar.as_ref() {
            None => EditorSceneState::Cockpit,
            Some(avatar) if !avatar.onboard_interior => EditorSceneState::StationHangar,
            Some(avatar) if avatar.interior_deck == 2 => EditorSceneState::InteriorDeck2,
            Some(_) => EditorSceneState::InteriorDeck1,
        })
    }
    pub fn scene_state_ack(&self) -> Option<&EmbeddedEventAck> {
        self.scene_ack.as_ref()
    }
    pub fn scene_state_pending(&self) -> bool {
        self.scene_pending.is_some()
    }
    /// Explicit sandbox mutation of this authenticated editor pilot only.
    /// The worker acknowledges authoritative placement before the UI reports it.
    pub fn set_scene_state(&mut self, state: EditorSceneState) -> Result<u64, String> {
        if !self.ready() {
            return Err("Editor authority is not ready".into());
        }
        if self.scene_pending.is_some() {
            return Err("Waiting for the previous scene state acknowledgement".into());
        }
        let id = self
            .scene_sequence
            .checked_add(1)
            .ok_or("Scene command sequence exhausted")?;
        self.clock
            .try_send(ClockCommand::SceneState(id, state))
            .map_err(|_| "Editor command queue is full or closed".to_string())?;
        self.scene_sequence = id;
        self.scene_pending = Some((id, state));
        self.scene_ack = None;
        // Stop captured input before placement so held controls cannot move the
        // avatar immediately after its authoritative walk/jump reset.
        self.cockpit.controls.clear();
        self.cockpit.pilot_input.disable_capture();
        Ok(id)
    }
    /// Queue an explicit, revision-checked edit of the live development interior.
    /// Rejections never install a local render-only layout or replay a mutation.
    pub fn room_layout_replace(
        &mut self,
        revision: u64,
        document: RoomLayoutDocument,
    ) -> Result<u64, String> {
        document.validate_live()?;
        if !self.ready() {
            return Err("Editor authority is not ready".into());
        }
        if self.room_pending.is_some() {
            return Err("Waiting for the previous room layout acknowledgement".into());
        }
        let id = self
            .room_sequence
            .checked_add(1)
            .ok_or("Room command sequence exhausted")?;
        self.clock
            .try_send(ClockCommand::RoomLayout(id, revision, document))
            .map_err(|_| "Editor command queue is full or closed".to_string())?;
        self.room_sequence = id;
        self.room_pending = Some(id);
        self.room_ack = None;
        Ok(id)
    }
    pub fn exterior_ack(&self) -> Option<&EmbeddedEventAck> {
        self.exterior_ack.as_ref()
    }
    pub fn exterior_replace(
        &mut self,
        revision: u64,
        document: ExteriorDefinition,
    ) -> Result<u64, String> {
        document.validate()?;
        self.queue_exterior(ExteriorCommand::Replace(revision, document))
    }
    pub fn exterior_set_enabled(&mut self, enabled: bool, revision: u64) -> Result<u64, String> {
        self.queue_exterior(ExteriorCommand::Enable(enabled, revision))
    }
    fn queue_exterior(&mut self, command: ExteriorCommand) -> Result<u64, String> {
        if !self.ready() {
            return Err("Editor authority is not ready".into());
        }
        if self.snapshot().is_none_or(|snapshot| {
            snapshot
                .exterior
                .as_ref()
                .is_none_or(|world| world.definition.document.validate().is_err())
        }) {
            return Err(
                "This authority does not advertise compatible powered exterior support".into(),
            );
        }
        if self.exterior_pending.is_some() {
            return Err("Waiting for the previous exterior acknowledgement".into());
        }
        let id = self
            .exterior_sequence
            .checked_add(1)
            .ok_or("Exterior command sequence exhausted")?;
        self.clock
            .try_send(ClockCommand::Exterior(id, command))
            .map_err(|_| "Editor command queue is full or closed".to_string())?;
        self.exterior_sequence = id;
        self.exterior_pending = Some(id);
        self.exterior_ack = None;
        Ok(id)
    }
    pub fn graph_replace(
        &mut self,
        revision: u64,
        graph: EditorEventGraph,
        clear_pending: bool,
    ) -> Result<u64, String> {
        graph.validate()?;
        self.queue_graph(GraphCommand::Replace {
            revision,
            graph,
            clear_pending,
        })
    }
    pub fn graph_trigger(&mut self, node_id: u64) -> Result<u64, String> {
        self.queue_graph(GraphCommand::Trigger(node_id))
    }
    /// Executes one ready graph node, without advancing simulation time.
    pub fn graph_step(&mut self) -> Result<u64, String> {
        self.queue_graph(GraphCommand::Step)
    }
    pub fn graph_clear(&mut self) -> Result<u64, String> {
        self.queue_graph(GraphCommand::Clear)
    }
    fn queue_graph(&mut self, command: GraphCommand) -> Result<u64, String> {
        if !self.ready() {
            return Err("Local authority is not ready".into());
        }
        if self.graph_pending.is_some() {
            return Err("Waiting for the previous graph command acknowledgement".into());
        }
        let id = self
            .graph_sequence
            .checked_add(1)
            .ok_or("Graph command sequence exhausted")?;
        self.clock
            .try_send(ClockCommand::Graph(id, command))
            .map_err(|_| "Local command queue is full or closed".to_string())?;
        self.graph_sequence = id;
        self.graph_pending = Some(id);
        Ok(id)
    }
    pub fn poll(&mut self) {
        for info in self.session_updates.try_iter() {
            self.session_info = Some(info);
        }
        let mut stop_feedback_confirmed = false;
        for state in self.states.try_iter() {
            stop_feedback_confirmed |= matches!(state, EmbeddedStatus::Stopped);
            self.state = state;
        }
        let mut applied_scene = None;
        for graph in self.graph_states.try_iter() {
            if let Some(ack) = graph
                .scene_ack
                .as_ref()
                .filter(|ack| Some(ack.id) == self.scene_pending.map(|(id, _)| id))
            {
                let requested = self.scene_pending.take().map(|(_, state)| state);
                if ack.result.is_ok() {
                    applied_scene = requested;
                }
                self.scene_ack = Some(ack.clone());
            }
            if let Some(ack) = graph
                .room_ack
                .as_ref()
                .filter(|ack| Some(ack.id) == self.room_pending)
            {
                self.room_pending = None;
                self.room_ack = Some(ack.clone());
            }
            if let Some(ack) = graph
                .exterior_ack
                .as_ref()
                .filter(|ack| Some(ack.id) == self.exterior_pending)
            {
                self.exterior_pending = None;
                self.exterior_ack = Some(ack.clone());
            }
            if graph
                .ack
                .as_ref()
                .is_some_and(|a| Some(a.id) == self.graph_pending)
            {
                self.graph_pending = None;
            }
            self.graph_state = Some(graph);
        }
        self.cockpit.embedded_paused = self.state != EmbeddedStatus::Running;
        self.cockpit.embedded_input = false;
        self.cockpit.poll();
        // This exact notice belongs to the editor clock request. Never clear
        // gameplay feedback or a worker error that arrived with confirmation.
        if self.cockpit.notice == STOP_REQUEST_NOTICE {
            if let EmbeddedStatus::Error(error) = &self.state {
                self.cockpit.notice = format!(
                    "Stop outcome unknown; inspect the confirmed state after reconnecting. {error}"
                );
            } else if stop_feedback_confirmed {
                self.cockpit.notice.clear();
            } else if (!self.cockpit.connected && !self.cockpit.connecting)
                || self
                    .worker
                    .as_ref()
                    .is_some_and(|worker| worker.is_finished())
            {
                self.cockpit.notice =
                    "Stop outcome unknown; reconnect and inspect the confirmed state.".into();
            }
        }
        if self.exterior_pending.is_some()
            && (matches!(
                self.state,
                EmbeddedStatus::Error(_) | EmbeddedStatus::Stopped
            ) || !self.cockpit.connected
                || self
                    .worker
                    .as_ref()
                    .is_some_and(|worker| worker.is_finished()))
        {
            let id = self
                .exterior_pending
                .take()
                .expect("pending exterior checked");
            self.exterior_ack = Some(EmbeddedEventAck { id, result: Err("Exterior outcome unknown; reconnect and inspect accepted state. The command was not replayed.".into()) });
        }
        if self.scene_pending.is_some()
            && (matches!(
                self.state,
                EmbeddedStatus::Error(_) | EmbeddedStatus::Stopped
            ) || !self.cockpit.connected
                || self
                    .worker
                    .as_ref()
                    .is_some_and(|worker| worker.is_finished()))
        {
            let (id, _) = self.scene_pending.take().expect("pending scene checked");
            self.scene_ack = Some(EmbeddedEventAck {
                id,
                result: Err("Scene state outcome unknown; reconnect and inspect the confirmed state. The command was not replayed.".into()),
            });
        }
        if let Some(scene) = applied_scene {
            self.set_free_camera(false);
            self.cockpit.view_mode = ViewMode::Cockpit;
            if scene != EditorSceneState::Cockpit {
                self.cockpit.hangar_view.on_foot_view = flight3d::hangar::OnFootView::FirstPerson;
            }
        }
        self.observe_view("Authoritative state or observed client view changed");
        if self.cockpit.embedded_paused {
            self.cockpit.frozen_time = self.cockpit.snapshot.as_ref().map(|s| s.server_time_ms);
        }
    }
    /// Poll a hidden viewport without treating workspace absence as a rendered frame.
    pub fn poll_hidden(&mut self) {
        self.poll();
        self.cockpit.frame_intervals.clear();
        self.cockpit.frame_observed = Instant::now();
    }
    pub fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.poll();
        let in_hangar = self.cockpit.in_station_hangar();
        if self.free_camera_enabled && self.free_camera_scene_hangar != Some(in_hangar) {
            // Camera coordinates are scene-local; let the new scene render once before reseeding.
            self.free_camera = None;
            self.free_camera_scene_hangar = Some(in_hangar);
        }
        self.update_editor_free_camera(ctx);
        self.cockpit
            .flight_view
            .set_editor_free_camera(self.free_camera_enabled, self.free_camera);
        self.cockpit
            .hangar_view
            .set_editor_free_camera(self.free_camera_enabled, self.free_camera);
        self.cockpit.embedded_input = gameplay_input_allowed(
            ctx.pointer_latest_pos()
                .is_some_and(|p| ctx.available_rect().contains(p)),
            self.free_camera_enabled,
        );
        self.cockpit.update(ctx, frame);
        if self.free_camera_enabled && self.free_camera.is_none() {
            self.free_camera = self.active_scene_camera_pose();
        }
        self.observe_view("In-game view control changed");
        // Paused editor time never advances because of UI rendering.
        if self.cockpit.embedded_paused {
            self.cockpit.frozen_time = self.cockpit.snapshot.as_ref().map(|s| s.server_time_ms);
        }
    }
    /// Enable a renderer-only free camera. It never sends pilot or authority commands.
    pub fn set_free_camera(&mut self, enabled: bool) {
        if enabled && !self.free_camera_enabled {
            self.free_camera_enabled = true;
            self.free_camera_scene_hangar = Some(self.cockpit.in_station_hangar());
            if self.cockpit.view_mode == ViewMode::Tactical {
                self.free_camera_saved_view = Some(self.cockpit.view_mode);
                self.cockpit.view_mode = ViewMode::External;
            }
            // Begin exactly at the rendered camera in whichever scene is active.
            self.free_camera = self.active_scene_camera_pose();
        } else if !enabled && self.free_camera_enabled {
            self.free_camera_enabled = false;
            self.free_camera = None;
            self.free_camera_scene_hangar = None;
            if let Some(view) = self.free_camera_saved_view.take() {
                self.cockpit.view_mode = view;
            }
        }
        self.cockpit
            .flight_view
            .set_editor_free_camera(self.free_camera_enabled, self.free_camera);
        self.cockpit
            .hangar_view
            .set_editor_free_camera(self.free_camera_enabled, self.free_camera);
    }
    pub fn free_camera_active(&self) -> bool {
        self.free_camera_enabled
    }
    fn active_scene_camera_pose(&self) -> Option<(Vec3, f32, f32)> {
        if self.cockpit.in_station_hangar() {
            self.cockpit.hangar_view.editor_camera_pose()
        } else {
            self.cockpit.flight_view.editor_camera_pose()
        }
    }
    fn update_editor_free_camera(&mut self, ctx: &egui::Context) {
        // egui may repeat a layout pass with the same input. Move once per
        // input frame, including the frame that seeds a new scene camera.
        let time = ctx.input(|i| i.time);
        if self.free_camera_input_time == Some(time) {
            return;
        }
        self.free_camera_input_time = Some(time);
        let Some((mut position, mut yaw, mut pitch)) = self.free_camera else {
            return;
        };
        // This is render time, never simulation time, so it continues while paused.
        let dt = ctx.input(|i| i.stable_dt).clamp(0.0, 0.1);
        let viewport = ctx.available_rect();
        let wants_keyboard = ctx.wants_keyboard_input();
        // Hangar geometry uses meters; the space renderer uses 100 m units.
        let meters_per_scene_unit = if self.cockpit.in_station_hangar() {
            1.0
        } else {
            100.0
        };
        let in_viewport = ctx.input(|i| {
            i.focused
                && !wants_keyboard
                && i.pointer.hover_pos().is_some_and(|p| viewport.contains(p))
        });
        ctx.input(|i| {
            if in_viewport && i.pointer.button_down(egui::PointerButton::Secondary) {
                let delta = i.pointer.delta();
                yaw += delta.x * 0.004;
                pitch = (pitch - delta.y * 0.004).clamp(-1.52, 1.52);
            }
            let forward = Vec3::new(
                yaw.cos() * pitch.cos(),
                pitch.sin(),
                yaw.sin() * pitch.cos(),
            );
            let right = Vec3::new(-yaw.sin(), 0.0, yaw.cos());
            if !in_viewport {
                return;
            }
            let mut movement = Vec3::ZERO;
            movement += forward
                * (i.key_down(egui::Key::W) as i32 - i.key_down(egui::Key::S) as i32) as f32;
            movement +=
                right * (i.key_down(egui::Key::D) as i32 - i.key_down(egui::Key::A) as i32) as f32;
            movement.y +=
                (i.key_down(egui::Key::E) as i32 - i.key_down(egui::Key::Q) as i32) as f32;
            movement.y +=
                (i.key_down(egui::Key::Space) as i32 - i.key_down(egui::Key::C) as i32) as f32;
            let speed = if i.modifiers.shift { 100.0 } else { 15.0 } / meters_per_scene_unit;
            if movement.length_squared() > 0.0 {
                position += movement.normalize() * speed * dt;
            }
        });
        self.free_camera = Some((position, yaw, pitch));
    }
    fn queue_clock(&mut self, command: ClockCommand) -> bool {
        if self.clock.try_send(command).is_err() {
            self.cockpit.notice =
                "Local command queue is full or closed; retry the control.".into();
            false
        } else {
            true
        }
    }
    pub fn session_mode(&self) -> &EmbeddedSessionMode {
        &self.mode
    }
    pub fn session_info(&self) -> Option<&EmbeddedSessionInfo> {
        self.session_info.as_ref()
    }
    pub fn realm(&self) -> EmbeddedRealm {
        self.realm
    }
    /// Confirmed authority layout, independent of the selected local storage.
    pub fn scene_layout_revision(&self) -> Option<SceneLayoutRevision> {
        self.cockpit
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot_world_layout(snapshot).ok())
            .map(|layout| layout.revision)
    }
    pub fn can_control_clock(&self) -> bool {
        !matches!(self.mode, EmbeddedSessionMode::Join { .. })
    }
    /// Replace the authority worker without blocking the UI or reusing old input/response queues.
    pub fn switch_session(
        &mut self,
        mode: EmbeddedSessionMode,
        pilot: String,
    ) -> Result<(), String> {
        self.switch_session_to(mode, pilot, self.data_dir.clone(), self.realm)
    }
    /// Open a separate persistent realm without moving or replacing the project world.
    pub fn switch_session_in_realm(
        &mut self,
        mode: EmbeddedSessionMode,
        pilot: String,
        realm: EmbeddedRealm,
    ) -> Result<(), String> {
        let data_dir = self.realm_directory(realm)?;
        self.switch_session_to(mode, pilot, data_dir, realm)
    }
    fn realm_directory(&self, realm: EmbeddedRealm) -> Result<PathBuf, String> {
        if self.project_data_dir.as_os_str().is_empty() {
            return Err("The original project world directory is unavailable".into());
        }
        let mut path = self.project_data_dir.clone();
        validate_realm_directory(&path)?;
        if realm == EmbeddedRealm::OrbitalPreview {
            for name in ["realms", "orbital-preview-v1"] {
                path.push(name);
                validate_realm_directory(&path)?;
            }
            if path.exists() {
                let project = fs::canonicalize(&self.project_data_dir)
                    .map_err(|error| format!("Cannot resolve project world directory: {error}"))?;
                let preview = fs::canonicalize(&path)
                    .map_err(|error| format!("Cannot resolve preview world directory: {error}"))?;
                if preview == project || !preview.starts_with(&project) {
                    return Err(
                        "The preview world must stay inside its own project subfolder".into(),
                    );
                }
            }
        }
        Ok(path)
    }
    fn switch_session_to(
        &mut self,
        mode: EmbeddedSessionMode,
        pilot: String,
        data_dir: PathBuf,
        realm: EmbeddedRealm,
    ) -> Result<(), String> {
        mode.validate()?;
        let pilot = pilot.trim().to_owned();
        if pilot.is_empty()
            || pilot.len() > 24
            || !pilot
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err("Pilot must use 1 to 24 letters, numbers, underscores or hyphens".into());
        }
        if self.state == EmbeddedStatus::Loading {
            return Err("Wait for the current connection attempt to finish".into());
        }
        // An exited worker has a disconnected queue; it is still safe to replace.
        if matches!(
            self.clock.try_send(ClockCommand::Shutdown),
            Err(mpsc::TrySendError::Full(_))
        ) {
            return Err("Previous worker is busy; retry the session change".into());
        }
        self.set_free_camera(false);
        self.cockpit.controls.clear();
        self.cockpit.controls = pilot_input::Mailbox::default();
        self.cockpit.pilot_input.disable_capture();
        self.cockpit.invalidate_ai_session();
        self.cockpit.remote_ship = None;
        self.cockpit.manual_remote = None;
        self.cockpit.resource_pilot = None;
        self.cockpit.motion.clear();
        self.cockpit.effects.clear();
        self.cockpit.feed.clear();
        self.cockpit.sound_scene.reset();
        self.cockpit.timeline_origin = None;
        self.cockpit.snapshot = None;
        self.cockpit.assets = None;
        self.cockpit.audit_updates = None;
        self.cockpit.audit_result = None;
        self.cockpit.request_pending = false;
        self.cockpit.frozen_time = None;
        self.cockpit.connected = false;
        self.cockpit.connecting = true;
        self.cockpit.notice = "Switching editor session…".into();
        self.cockpit.pilot = pilot.clone();
        self.cockpit.selected_object = SelectedObject::Station;
        self.cockpit.mining_target = None;
        self.cockpit.selected_drone = None;
        self.cockpit.drone_attack_target = None;
        let connection = spawn_worker(
            data_dir.clone(),
            pilot,
            mode.clone(),
            self.cockpit.controls.clone(),
            self.worker.take(),
        );
        self.cockpit.commands = Some(connection.commands);
        self.cockpit.updates = Some(connection.updates);
        self.clock = connection.clock;
        self.states = connection.states;
        self.graph_states = connection.graph_states;
        self.graph_state = None;
        self.graph_pending = None;
        self.room_pending = None;
        self.room_ack = None;
        self.scene_pending = None;
        self.scene_ack = None;
        self.exterior_pending = None;
        self.exterior_ack = None;
        self.session_generation = self.session_generation.saturating_add(1);
        self.session_info = None;
        self.session_updates = connection.info;
        self.worker = Some(connection.worker);
        self.data_dir = data_dir;
        self.realm = realm;
        self.mode = mode;
        self.state = EmbeddedStatus::Loading;
        Ok(())
    }
    pub fn play(&mut self) {
        self.queue_clock(ClockCommand::Play);
    }
    pub fn pause(&mut self) {
        self.queue_clock(ClockCommand::Pause);
    }
    /// Advances one 50 ms simulation tick, separately from graph node stepping.
    pub fn step(&mut self) {
        self.queue_clock(ClockCommand::Step);
    }
    pub fn stop(&mut self) {
        if self.queue_clock(ClockCommand::Stop) {
            self.cockpit.notice = STOP_REQUEST_NOTICE.into();
        }
    }
    pub fn on_exit(&mut self) {
        if self.destroyed {
            return;
        }
        self.destroyed = true;
        let _ = self.clock.try_send(ClockCommand::Shutdown);
        self.cockpit.flight_view.destroy();
        self.cockpit.hangar_view.destroy();
    }
    pub fn status(&self) -> &EmbeddedStatus {
        &self.state
    }
    pub fn ready(&self) -> bool {
        matches!(self.state, EmbeddedStatus::Paused | EmbeddedStatus::Running)
    }
    pub fn paused(&self) -> bool {
        self.state == EmbeddedStatus::Paused
    }
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.cockpit.snapshot.clone()
    }
    pub fn assets(&self) -> Option<AssetsView> {
        self.cockpit.assets.clone()
    }
    pub fn pilot(&self) -> &str {
        &self.cockpit.pilot
    }
    fn current_view(&self) -> Option<EmbeddedView> {
        let pilot = self.cockpit.own()?;
        if self.cockpit.in_station_hangar() {
            return Some(if pilot.station_avatar.is_some() {
                match self.cockpit.hangar_view.on_foot_view {
                    flight3d::hangar::OnFootView::FirstPerson => EmbeddedView::FirstPerson,
                    flight3d::hangar::OnFootView::ThirdPerson => EmbeddedView::ThirdPerson,
                }
            } else {
                EmbeddedView::Hangar
            });
        }
        Some(match self.cockpit.view_mode {
            ViewMode::Cockpit => EmbeddedView::Cockpit,
            ViewMode::External => EmbeddedView::External,
            ViewMode::Tactical => EmbeddedView::Tactical,
            ViewMode::Tactical3d => EmbeddedView::Tactical3d,
        })
    }
    fn available_views(&self) -> Vec<EmbeddedView> {
        if !self.ready() || !self.cockpit.connected {
            return vec![];
        }
        match self.current_view() {
            None => vec![],
            Some(EmbeddedView::Hangar) => vec![EmbeddedView::Hangar],
            Some(EmbeddedView::FirstPerson | EmbeddedView::ThirdPerson) => {
                vec![EmbeddedView::FirstPerson, EmbeddedView::ThirdPerson]
            }
            _ => vec![
                EmbeddedView::Cockpit,
                EmbeddedView::External,
                EmbeddedView::Tactical,
                EmbeddedView::Tactical3d,
            ],
        }
    }
    fn observe_view(&mut self, reason: &str) {
        let current = self.current_view();
        if current == self.views.previous {
            return;
        }
        self.views.sequence = self.views.sequence.saturating_add(1);
        self.views.events.push_back(ViewTransition {
            sequence: self.views.sequence,
            from: self.views.previous,
            to: current,
            reason: reason.into(),
            simulation_time_ms: self.cockpit.snapshot.as_ref().map(|s| s.server_time_ms),
            observed_at_ms: self
                .views
                .started
                .get_or_insert_with(Instant::now)
                .elapsed()
                .as_millis()
                .min(u64::MAX as u128) as u64,
        });
        self.views.previous = current;
        while self.views.events.len() > 32 {
            self.views.events.pop_front();
        }
    }
    pub fn debug_view_state(&self) -> EmbeddedViewState {
        let scene = match self.cockpit.own() {
            None => "No authoritative pilot snapshot".into(),
            Some(p) if self.cockpit.in_station_hangar() => match p.station_avatar {
                Some(a) if a.onboard_interior => {
                    format!("Pioneer interior · deck {}", a.interior_deck)
                }
                Some(_) => "Station · on foot".into(),
                None => "Station · aboard docked ship".into(),
            },
            Some(p) => format!("System {} · {:?}", p.system_id, p.location),
        };
        EmbeddedViewState {
            current: self.current_view(),
            available: self.available_views(),
            wireframe: self.views.wireframe,
            viewed_pilot: self.cockpit.controlled_name().into(),
            scene,
            transitions: self.views.events.iter().cloned().collect(),
        }
    }
    pub fn set_debug_view(&mut self, view: EmbeddedView) -> Result<(), String> {
        if !self.available_views().contains(&view) {
            return Err("View unavailable for the current confirmed pilot state. Camera controls cannot dock, board, or disembark.".into());
        }
        self.observe_view("Initial confirmed view");
        match view {
            EmbeddedView::Cockpit => self.cockpit.view_mode = ViewMode::Cockpit,
            EmbeddedView::External => self.cockpit.view_mode = ViewMode::External,
            EmbeddedView::Tactical => self.cockpit.view_mode = ViewMode::Tactical,
            EmbeddedView::Tactical3d => self.cockpit.view_mode = ViewMode::Tactical3d,
            EmbeddedView::FirstPerson => {
                self.cockpit.hangar_view.on_foot_view = flight3d::hangar::OnFootView::FirstPerson
            }
            EmbeddedView::ThirdPerson => {
                self.cockpit.hangar_view.on_foot_view = flight3d::hangar::OnFootView::ThirdPerson
            }
            EmbeddedView::Hangar => {}
        }
        self.observe_view("Editor requested local view change");
        Ok(())
    }
    pub fn set_debug_wireframe(&mut self, enabled: bool) {
        self.views.wireframe = enabled;
        self.cockpit.hangar_view.set_debug_wireframe(enabled);
        self.cockpit.flight_view.set_debug_wireframe(enabled);
    }
    pub fn debug_render_metrics(&self) -> crate::DebugRenderMetrics {
        if self.cockpit.in_station_hangar() {
            self.cockpit.hangar_view.debug_render_metrics()
        } else if self.cockpit.view_mode == ViewMode::Tactical {
            Default::default()
        } else {
            self.cockpit.flight_view.debug_render_metrics()
        }
    }
    pub fn set_material_preview(&mut self, preview: Option<EmbeddedMaterialPreview>) {
        self.cockpit.flight_view.set_material_preview(preview);
        self.cockpit
            .hangar_view
            .set_material_preview(preview.map(EmbeddedMaterialPreview::validated));
    }
    pub fn set_spatial_material(
        &mut self,
        material: Option<std::sync::Arc<crate::MaterialRevision>>,
    ) {
        self.cockpit
            .flight_view
            .set_spatial_material(material.clone());
        self.cockpit.hangar_view.set_spatial_material(material);
    }
    pub fn material_program_status(&self) -> crate::MaterialProgramStatus {
        if self.cockpit.in_station_hangar() {
            self.cockpit.hangar_view.material_program_status()
        } else {
            self.cockpit.flight_view.spatial_material_status()
        }
    }
    pub fn send_command(&mut self, request: Request) {
        self.cockpit.send(request);
    }
    /// Read-only view of the same gate used by typed equipment requests.
    pub fn equipment_controls_ready(&self) -> bool {
        self.state == EmbeddedStatus::Running && self.cockpit.can_operate()
    }
    /// Existing session feedback, not a correlated command acknowledgement.
    pub fn command_notice(&self) -> &str {
        &self.cockpit.notice
    }
}
fn validate_realm_directory(path: &std::path::Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err("World directories must be folders, not symlinks or files".into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Cannot inspect world directory: {error}")),
    }
}
fn gameplay_input_allowed(pointer_in_viewport: bool, free_camera: bool) -> bool {
    pointer_in_viewport && !free_camera
}
impl Drop for EmbeddedClient {
    fn drop(&mut self) {
        let _ = self.clock.try_send(ClockCommand::Shutdown);
        // Never wait for database writes on the render thread.
        self.on_exit();
    }
}
struct WorkerConnection {
    commands: Sender<Request>,
    updates: Receiver<Update>,
    clock: mpsc::SyncSender<ClockCommand>,
    states: Receiver<EmbeddedStatus>,
    graph_states: Receiver<EmbeddedEventDebugState>,
    info: Receiver<EmbeddedSessionInfo>,
    worker: std::thread::JoinHandle<()>,
}
fn spawn_worker(
    dir: PathBuf,
    pilot: String,
    mode: EmbeddedSessionMode,
    controls: pilot_input::Mailbox,
    previous: Option<std::thread::JoinHandle<()>>,
) -> WorkerConnection {
    let (clock, clock_rx) = mpsc::sync_channel(16);
    let (graph_tx, graph_states) = mpsc::sync_channel(1);
    let (status_tx, states) = mpsc::channel();
    let (commands, requests) = mpsc::channel();
    let (updates, receiver) = mpsc::sync_channel(64);
    let (info_tx, info) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        // Database ownership is transferred only after the old worker exits.
        // This wait is on a background thread, never the renderer.
        if let Some(previous) = previous {
            let _ = previous.join();
        }
        let result = match mode {
            EmbeddedSessionMode::Local => {
                let _ = info_tx.send(EmbeddedSessionInfo {
                    label: "Local world".into(),
                    address: None,
                    editor_secret: None,
                    is_host: true,
                });
                run(
                    dir,
                    pilot,
                    clock_rx,
                    requests,
                    updates.clone(),
                    controls,
                    (status_tx.clone(), graph_tx),
                )
            }
            mode => shared::run(
                dir,
                pilot,
                mode,
                clock_rx,
                requests,
                updates.clone(),
                controls,
                (status_tx.clone(), graph_tx),
                info_tx,
            ),
        };
        if let Err(error) = result {
            let _ = updates.send(Update::Failure(error.clone()));
            let _ = status_tx.send(EmbeddedStatus::Error(error));
        }
    });
    WorkerConnection {
        commands,
        updates: receiver,
        clock,
        states,
        graph_states,
        info,
        worker,
    }
}
fn permitted_while_paused(request: &Request) -> bool {
    matches!(request, Request::Snapshot | Request::ExportLedger)
}
fn run(
    dir: PathBuf,
    pilot: String,
    clock: Receiver<ClockCommand>,
    requests: Receiver<Request>,
    updates: mpsc::SyncSender<Update>,
    controls: pilot_input::Mailbox,
    outputs: (
        Sender<EmbeddedStatus>,
        mpsc::SyncSender<EmbeddedEventDebugState>,
    ),
) -> Result<(), String> {
    let (states, graph_states) = outputs;
    let mut world = EditorWorld::open(&dir).map_err(|e| e.to_string())?;
    let credentials = dir.join("client");
    fs::create_dir_all(&credentials).map_err(|e| e.to_string())?;
    let path = credentials_path(&credentials, "embedded-editor", &pilot);
    let token = match fs::read_to_string(&path) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    let mut session = None;
    let mut response = world.handle(
        &mut session,
        WorkerIdentity::default().request("embedded-editor", &pilot, token)?,
    );
    if !response.ok {
        return Err(response.message);
    }
    if let Some(token) = response.resume_token.take() {
        save_token(&path, &token)?;
    }
    updates.send(Update::Ready).map_err(|e| e.to_string())?;
    updates
        .send(Update::Response(Box::new(response), false))
        .map_err(|e| e.to_string())?;
    let _ = states.send(EmbeddedStatus::Paused);
    let mut previous = Instant::now();
    let mut graph_ack = None;
    let mut room_ack = None;
    let mut scene_ack = None;
    let mut exterior_ack = None;
    let mut pending_graph_state = Some(EmbeddedEventDebugState {
        debug: world.graph_debug(),
        ack: None,
        room_ack: None,
        scene_ack: None,
        exterior_ack: None,
    });
    let mut last_status = EmbeddedStatus::Paused;
    let mut snapshot_dirty = false;
    loop {
        match clock.recv_timeout(Duration::from_millis(50)) {
            Ok(ClockCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(ClockCommand::SceneState(id, state)) => {
                controls.clear();
                let result = world
                    .set_scene_state(&session, state)
                    .map_err(|error| error.to_string());
                scene_ack = Some(EmbeddedEventAck { id, result });
                snapshot_dirty = true;
            }
            Ok(ClockCommand::Exterior(id, command)) => {
                let result = if session.is_none() {
                    Err("Local world is stopped".into())
                } else {
                    match command {
                        ExteriorCommand::Replace(revision, document) => {
                            world.exterior_replace(revision, document).map(|_| ())
                        }
                        ExteriorCommand::Enable(enabled, revision) => {
                            world.exterior_set_enabled(&mut session, enabled, revision)
                        }
                    }
                    .map_err(|error| error.to_string())
                };
                exterior_ack = Some(EmbeddedEventAck { id, result });
                snapshot_dirty = true;
            }
            Ok(ClockCommand::RoomLayout(id, revision, document)) => {
                let result = if session.is_none() {
                    Err("Local world is stopped".into())
                } else {
                    world
                        .room_layout_replace(revision, document)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                };
                room_ack = Some(EmbeddedEventAck { id, result });
                snapshot_dirty = true;
            }
            Ok(ClockCommand::Graph(id, command)) => {
                let result = if session.is_none() {
                    Err("Local world is stopped".into())
                } else {
                    match command {
                        GraphCommand::Replace {
                            revision,
                            graph,
                            clear_pending,
                        } => world.graph_replace(revision, graph, clear_pending),
                        GraphCommand::Trigger(node) => world.graph_trigger(node),
                        GraphCommand::Step => world.graph_step(),
                        GraphCommand::Clear => world.graph_clear(),
                    }
                    .map_err(|e| e.to_string())
                };
                graph_ack = Some(EmbeddedEventAck { id, result });
                snapshot_dirty = true;
            }
            Ok(ClockCommand::Stop) => {
                world.pause();
                controls.clear();
                world.disconnect(&mut session);
                snapshot_dirty = true;
            }
            Ok(ClockCommand::Play) => {
                if session.is_none() {
                    let token = fs::read_to_string(&path).map_err(|e| e.to_string())?;
                    let response = world.handle(
                        &mut session,
                        WorkerIdentity::default().request(
                            "embedded-editor",
                            &pilot,
                            Some(token),
                        )?,
                    );
                    if !response.ok {
                        return Err(response.message);
                    }
                }
                world.play();
                previous = Instant::now();
                snapshot_dirty = true;
            }
            Ok(ClockCommand::Pause) => {
                world.pause();
                controls.clear();
                snapshot_dirty = true;
            }
            Ok(ClockCommand::Step) => {
                let result = if session.is_none() {
                    Err("Local world is stopped; press Play before stepping".to_string())
                } else {
                    world.step(50).map_err(|e| e.to_string())
                };
                if let Err(error) = result {
                    // A stale UI may queue Play then Step before receiving Running.
                    // Invalid controls are visible rejections, not transport failures.
                    updates
                        .send(Update::Response(Box::new(Response::error(error)), false))
                        .map_err(|e| e.to_string())?;
                }
                snapshot_dirty = true;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let before_time = world.time();
        let now = Instant::now();
        world
            .advance(now.duration_since(previous).as_millis().min(250) as u64)
            .map_err(|e| e.to_string())?;
        previous = now;
        snapshot_dirty |= before_time != world.time();
        for request in requests.try_iter().take(32) {
            snapshot_dirty = true;
            let response = if world.is_paused() && !permitted_while_paused(&request) {
                Response::error("Simulation paused. Press Play to operate the ship.")
            } else {
                world.handle(&mut session, request)
            };
            if updates
                .send(Update::Response(Box::new(response), true))
                .is_err()
            {
                world.disconnect(&mut session);
                return Ok(());
            }
        }
        if let Some(request) = controls.take() {
            if !world.is_paused() {
                let _ = world.handle(&mut session, request);
                snapshot_dirty = true;
            }
        }
        let status = if session.is_none() {
            EmbeddedStatus::Stopped
        } else if world.is_paused() {
            EmbeddedStatus::Paused
        } else {
            EmbeddedStatus::Running
        };
        if status != last_status {
            snapshot_dirty = true;
            let _ = states.send(status.clone());
            last_status = status;
        }
        if snapshot_dirty {
            pending_graph_state = Some(EmbeddedEventDebugState {
                debug: world.graph_debug(),
                ack: graph_ack.clone(),
                room_ack: room_ack.clone(),
                scene_ack: scene_ack.clone(),
                exterior_ack: exterior_ack.clone(),
            });
        }
        if let Some(debug) = pending_graph_state.take() {
            match graph_states.try_send(debug) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(debug)) => pending_graph_state = Some(debug),
                Err(mpsc::TrySendError::Disconnected(_)) => break,
            }
        }
        if session.is_none() {
            snapshot_dirty = false;
            continue;
        }
        if !snapshot_dirty {
            continue;
        }

        let response = world.handle(&mut session, Request::Snapshot);
        match updates.try_send(Update::Response(Box::new(response), false)) {
            Ok(()) => snapshot_dirty = false,
            Err(mpsc::TrySendError::Full(_)) => {}
            Err(mpsc::TrySendError::Disconnected(_)) => break,
        }
    }
    world.disconnect(&mut session);
    let _ = states.send(EmbeddedStatus::Stopped);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    mod room_acceptance {
        include!("embedded_room_acceptance.rs");
    }

    #[test]
    fn exterior_commands_reject_unsupported_advertised_definitions() {
        for bad_hash in [false, true] {
            let (mut client, _) = debug_fixture();
            let (clock, commands) = mpsc::sync_channel(4);
            client.clock = clock;
            let mut document = arkline_protocol::default_pioneer_exterior();
            if bad_hash {
                document.hull_asset_sha256 = "0".repeat(64);
            } else {
                document.schema_version += 1;
            }
            client.cockpit.snapshot.as_mut().unwrap().exterior =
                Some(arkline_protocol::ExteriorWorldState {
                    definition: arkline_protocol::ExteriorDefinitionState {
                        revision: 0,
                        document,
                    },
                    vessels: vec![],
                });
            assert!(client
                .exterior_set_enabled(false, 0)
                .unwrap_err()
                .contains("support"));
            assert!(commands.try_recv().is_err());
            assert!(client.exterior_pending.is_none());
            assert_eq!(client.exterior_sequence, 0);
            client
                .cockpit
                .snapshot
                .as_mut()
                .unwrap()
                .exterior
                .as_mut()
                .unwrap()
                .definition
                .document = arkline_protocol::default_pioneer_exterior();
            assert_eq!(client.exterior_set_enabled(false, 0), Ok(1));
            assert!(matches!(
                commands.try_recv().unwrap(),
                ClockCommand::Exterior(1, ExteriorCommand::Enable(false, 0))
            ));
        }
    }

    #[test]
    fn exterior_commands_require_advertised_support_and_do_not_replay_lost_acknowledgements() {
        let (mut client, _) = debug_fixture();
        client.cockpit.snapshot.as_mut().unwrap().exterior = None;
        assert!(client
            .exterior_set_enabled(false, 0)
            .unwrap_err()
            .contains("advertise"));
        assert!(client.exterior_pending.is_none());
        // A worker/connection loss leaves an explicit unknown result, never a replay.
        client.exterior_pending = Some(7);
        client.state = EmbeddedStatus::Stopped;
        client.poll();
        assert!(client.exterior_pending.is_none());
        let ack = client.exterior_ack().unwrap();
        assert_eq!(ack.id, 7);
        assert!(ack.result.as_ref().unwrap_err().contains("not replayed"));
    }

    #[test]
    fn exterior_local_and_shared_workers_ack_paused_edits_and_preserve_clock() {
        fn wait(client: &mut EmbeddedClient, predicate: impl Fn(&EmbeddedClient) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(8);
            while Instant::now() < deadline {
                client.poll();
                if predicate(client) {
                    return;
                }
                assert!(
                    !matches!(client.status(), EmbeddedStatus::Error(_)),
                    "worker failed"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!(
                "exterior acknowledgement/snapshot did not arrive: {:?}",
                client.exterior_ack()
            );
        }
        fn finish(client: &mut EmbeddedClient) {
            let _ = client.clock.try_send(ClockCommand::Shutdown);
            client.cockpit.updates = None;
            if let Some(worker) = client.worker.take() {
                worker.join().unwrap();
            }
        }
        fn set_off(client: &mut EmbeddedClient) {
            let revision = client
                .snapshot()
                .unwrap()
                .exterior
                .as_ref()
                .unwrap()
                .definition
                .revision;
            let time = client.snapshot().unwrap().server_time_ms;
            let id = client.exterior_set_enabled(false, revision).unwrap();
            assert!(client.exterior_set_enabled(true, revision).is_err());
            wait(client, |client| {
                client.exterior_ack().is_some_and(|ack| ack.id == id)
                    && client
                        .snapshot()
                        .unwrap()
                        .exterior
                        .as_ref()
                        .unwrap()
                        .vessels
                        .iter()
                        .any(|vessel| vessel.pilot == client.pilot() && !vessel.enabled)
            });
            assert!(client.exterior_ack().unwrap().result.is_ok());
            assert_eq!(client.snapshot().unwrap().server_time_ms, time);
            assert_eq!(client.status(), &EmbeddedStatus::Paused);
        }
        let directory = std::env::temp_dir().join(format!(
            "arkline-exterior-bridge-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (mut local, _) = debug_fixture();
        local.data_dir = directory.join("local");
        local
            .switch_session(EmbeddedSessionMode::Local, "ExteriorLocal".into())
            .unwrap();
        wait(&mut local, |client| {
            client.ready() && client.snapshot().is_some_and(|s| s.exterior.is_some())
        });
        set_off(&mut local);
        let mut document = local
            .snapshot()
            .unwrap()
            .exterior
            .as_ref()
            .unwrap()
            .definition
            .document
            .clone();
        document.fixtures[0].emission *= 0.5;
        let id = local.exterior_replace(0, document.clone()).unwrap();
        wait(&mut local, |client| {
            client.exterior_ack().is_some_and(|ack| ack.id == id)
                && client
                    .snapshot()
                    .unwrap()
                    .exterior
                    .as_ref()
                    .unwrap()
                    .definition
                    .revision
                    == 1
        });
        assert!(local.exterior_ack().unwrap().result.is_ok());
        let id = local.exterior_replace(0, document).unwrap();
        wait(&mut local, |client| {
            client.exterior_ack().is_some_and(|ack| ack.id == id)
        });
        assert!(local.exterior_ack().unwrap().result.is_err());
        assert_eq!(
            local
                .snapshot()
                .unwrap()
                .exterior
                .as_ref()
                .unwrap()
                .definition
                .revision,
            1
        );
        finish(&mut local);

        let (mut host, _) = debug_fixture();
        host.data_dir = directory.join("host");
        host.switch_session(
            EmbeddedSessionMode::Host {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
            },
            "ExteriorHost".into(),
        )
        .unwrap();
        wait(&mut host, |client| {
            client.ready() && client.session_info().is_some()
        });
        let info = host.session_info().unwrap();
        let mode = EmbeddedSessionMode::Join {
            address: info.address.unwrap(),
            secret: info.editor_secret.clone().unwrap(),
        };
        let (mut guest, _) = debug_fixture();
        guest.data_dir = directory.join("guest");
        guest.switch_session(mode, "ExteriorGuest".into()).unwrap();
        wait(&mut guest, |client| {
            client.ready() && client.snapshot().is_some_and(|s| s.exterior.is_some())
        });
        set_off(&mut guest);
        wait(&mut host, |client| {
            client
                .snapshot()
                .unwrap()
                .exterior
                .as_ref()
                .unwrap()
                .vessels
                .iter()
                .any(|vessel| vessel.pilot == "ExteriorGuest" && !vessel.enabled)
        });
        assert!(host
            .snapshot()
            .unwrap()
            .exterior
            .as_ref()
            .unwrap()
            .vessels
            .iter()
            .any(|vessel| vessel.pilot == "ExteriorHost" && vessel.enabled));
        let time = host.snapshot().unwrap().server_time_ms;
        host.step();
        wait(&mut host, |client| {
            client.snapshot().unwrap().server_time_ms == time + 50
        });
        wait(&mut guest, |client| {
            client.snapshot().unwrap().server_time_ms == time + 50
        });
        finish(&mut guest);
        finish(&mut host);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn session_switch_rejects_nonlocal_transport_without_losing_the_current_world() {
        let (mut client, _) = debug_fixture();
        let original = client.snapshot().unwrap().server_time_ms;
        assert!(client
            .switch_session(
                EmbeddedSessionMode::Host {
                    bind_addr: "0.0.0.0:7779".parse().unwrap()
                },
                "LocalPilot".into()
            )
            .is_err());
        assert_eq!(client.snapshot().unwrap().server_time_ms, original);
        assert!(client.can_control_clock());
    }

    #[test]
    fn realm_switch_round_trip_preserves_project_world_and_credentials() {
        fn wait(client: &mut EmbeddedClient, predicate: impl Fn(&EmbeddedClient) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(8);
            while Instant::now() < deadline {
                client.poll();
                if predicate(client) {
                    return;
                }
                if let EmbeddedStatus::Error(error) = client.status() {
                    panic!("realm worker failed: {error}");
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!("realm worker did not reach the expected state");
        }
        let directory = std::env::temp_dir().join(format!(
            "arkline-realm-switch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = directory.join("project");
        let pilot = "RealmPreservedPilot";
        let mut world =
            EditorWorld::open_with_creation_scene(&project, SceneLayoutRevision::LegacyV1).unwrap();
        let mut session = None;
        let response = world.handle(
            &mut session,
            WorkerIdentity::default()
                .request("embedded-editor", pilot, None)
                .unwrap(),
        );
        assert!(response.ok);
        let token = response.resume_token.unwrap();
        fs::create_dir_all(project.join("client")).unwrap();
        let project_credential =
            credentials_path(&project.join("client"), "embedded-editor", pilot);
        save_token(&project_credential, &token).unwrap();
        world.step(250).unwrap();
        let original_time = world.time();
        let original_pilot = world
            .handle(&mut session, Request::Snapshot)
            .snapshot
            .unwrap()
            .pilots
            .into_iter()
            .find(|view| view.name == pilot)
            .unwrap();
        drop(world);
        fs::write(project.join("keep-project.txt"), b"preserved project data").unwrap();

        let (mut client, _) = debug_fixture();
        client.project_data_dir = project.clone();
        client.data_dir = project.clone();
        client
            .switch_session_in_realm(
                EmbeddedSessionMode::Local,
                pilot.into(),
                EmbeddedRealm::Project,
            )
            .unwrap();
        wait(&mut client, |client| {
            client.ready() && client.scene_layout_revision() == Some(SceneLayoutRevision::LegacyV1)
        });
        assert_eq!(client.snapshot().unwrap().server_time_ms, original_time);

        client
            .switch_session_in_realm(
                EmbeddedSessionMode::Host {
                    bind_addr: "127.0.0.1:0".parse().unwrap(),
                },
                pilot.into(),
                EmbeddedRealm::OrbitalPreview,
            )
            .unwrap();
        wait(&mut client, |client| {
            client.ready()
                && client.session_info().is_some()
                && client.scene_layout_revision() == Some(SceneLayoutRevision::OrbitalV1)
        });
        let preview = project.join("realms/orbital-preview-v1");
        assert_eq!(client.data_dir, preview);
        assert_eq!(client.project_data_dir, project);
        assert_eq!(client.realm(), EmbeddedRealm::OrbitalPreview);
        let preview_credential =
            credentials_path(&preview.join("client"), "embedded-editor", pilot);
        let preview_token = fs::read_to_string(&preview_credential).unwrap();
        assert_ne!(preview_token, token);
        let preview_time = client.snapshot().unwrap().server_time_ms;
        client.step();
        wait(&mut client, |client| {
            client.snapshot().unwrap().server_time_ms == preview_time + 50
        });

        // The original API continues using the current realm, rather than
        // silently resetting callers to the project directory.
        client
            .switch_session(EmbeddedSessionMode::Local, pilot.into())
            .unwrap();
        wait(&mut client, |client| {
            client.ready() && client.scene_layout_revision() == Some(SceneLayoutRevision::OrbitalV1)
        });
        assert_eq!(client.data_dir, preview);
        assert_eq!(client.snapshot().unwrap().server_time_ms, preview_time + 50);

        client
            .switch_session_in_realm(
                EmbeddedSessionMode::Local,
                pilot.into(),
                EmbeddedRealm::Project,
            )
            .unwrap();
        wait(&mut client, |client| {
            client.ready() && client.scene_layout_revision() == Some(SceneLayoutRevision::LegacyV1)
        });
        assert_eq!(client.data_dir, project);
        assert_eq!(client.realm(), EmbeddedRealm::Project);
        let restored = client.snapshot().unwrap();
        assert_eq!(restored.server_time_ms, original_time);
        assert_eq!(
            restored.pilots.iter().find(|view| view.name == pilot),
            Some(&original_pilot)
        );
        assert_eq!(fs::read_to_string(project_credential).unwrap(), token);
        assert_eq!(
            fs::read_to_string(preview_credential).unwrap(),
            preview_token
        );
        assert_eq!(
            fs::read(project.join("keep-project.txt")).unwrap(),
            b"preserved project data"
        );
        let _ = client.clock.try_send(ClockCommand::Shutdown);
        client.cockpit.updates = None;
        client.worker.take().unwrap().join().unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejected_realm_switch_keeps_path_realm_and_current_snapshot() {
        let (mut client, _) = debug_fixture();
        client.project_data_dir = std::env::temp_dir().join("arkline-realm-rejected-project");
        client.data_dir = client.project_data_dir.clone();
        let original_time = client.snapshot().unwrap().server_time_ms;
        let (clock, commands) = mpsc::sync_channel(1);
        client.clock = clock;
        assert!(client.clock.try_send(ClockCommand::Play).is_ok());
        for (mode, pilot) in [
            (EmbeddedSessionMode::Local, "ValidPilot"),
            (EmbeddedSessionMode::Local, "Invalid pilot"),
            (
                EmbeddedSessionMode::Host {
                    bind_addr: "0.0.0.0:7779".parse().unwrap(),
                },
                "ValidPilot",
            ),
        ] {
            assert!(client
                .switch_session_in_realm(mode, pilot.into(), EmbeddedRealm::OrbitalPreview)
                .is_err());
            assert_eq!(client.data_dir, client.project_data_dir);
            assert_eq!(client.realm(), EmbeddedRealm::Project);
            assert_eq!(client.snapshot().unwrap().server_time_ms, original_time);
        }
        assert!(matches!(commands.try_recv().unwrap(), ClockCommand::Play));
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn held_world_lock_failure_can_retry_without_losing_state_or_authority() {
        fn wait(client: &mut EmbeddedClient, predicate: impl Fn(&EmbeddedClient) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(8);
            while Instant::now() < deadline {
                client.poll();
                if predicate(client) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!(
                "lock recovery did not reach expected state: {:?}",
                client.status()
            );
        }

        struct Directory(PathBuf);
        impl Drop for Directory {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let directory = Directory(std::env::temp_dir().join(format!(
            "arkline-held-lock-recovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
        let project = directory.0.join("project");
        let pilot = "LockRecoveryPilot";
        let mut owner =
            EditorWorld::open_with_creation_scene(&project, SceneLayoutRevision::LegacyV1).unwrap();
        let mut session = None;
        let connected = owner.handle(
            &mut session,
            WorkerIdentity::default()
                .request("embedded-editor", pilot, None)
                .unwrap(),
        );
        assert!(connected.ok);
        let token = connected.resume_token.unwrap();
        fs::create_dir_all(project.join("client")).unwrap();
        let credential = credentials_path(&project.join("client"), "embedded-editor", pilot);
        save_token(&credential, &token).unwrap();
        let credential_bytes = fs::read(&credential).unwrap();
        assert!(
            owner
                .handle(
                    &mut session,
                    Request::CreateCorporation {
                        name: "LockRecoveryCorp".into(),
                    }
                )
                .ok
        );
        owner.step(150).unwrap();
        let baseline = owner.handle(&mut session, Request::Snapshot);
        let snapshot = serde_json::to_value(baseline.snapshot.as_ref().unwrap()).unwrap();
        let assets = serde_json::to_value(baseline.assets.as_ref().unwrap()).unwrap();
        assert!(baseline.assets.unwrap().corporation.is_some());
        let original_time = owner.time();
        let sentinel = project.join("keep-project.rs");
        fs::write(&sentinel, b"saved project source\n").unwrap();
        let lock = project.join("world.sqlite.lock");
        assert!(lock.exists());

        let (mut client, _) = debug_fixture();
        client.project_data_dir = project.clone();
        client.data_dir = project.clone();
        for generation in 1..=2 {
            client
                .switch_session_in_realm(
                    EmbeddedSessionMode::Local,
                    pilot.into(),
                    EmbeddedRealm::Project,
                )
                .unwrap();
            assert_eq!(client.status(), &EmbeddedStatus::Loading);
            assert!(client
                .switch_session(EmbeddedSessionMode::Local, pilot.into())
                .is_err());
            assert_eq!(client.session_generation(), generation);
            wait(&mut client, |client| {
                matches!(client.status(), EmbeddedStatus::Error(_))
            });
            let EmbeddedStatus::Error(error) = client.status() else {
                unreachable!()
            };
            assert!(error.contains("already owned"), "{error}");
            assert!(!client.ready());
            assert!(client.snapshot().is_none());
            assert!(client.assets().is_none());
            assert!(!client.equipment_controls_ready());
            assert!(client.graph_clear().is_err());
            assert!(client
                .set_scene_state(EditorSceneState::InteriorDeck1)
                .is_err());
            assert_eq!(client.realm(), EmbeddedRealm::Project);
            assert_eq!(client.data_dir, project);
            assert_eq!(fs::read(&credential).unwrap(), credential_bytes);
            assert_eq!(fs::read(&sentinel).unwrap(), b"saved project source\n");
            let unchanged = owner.handle(&mut session, Request::Snapshot);
            assert_eq!(
                serde_json::to_value(unchanged.snapshot.unwrap()).unwrap(),
                snapshot
            );
            assert_eq!(
                serde_json::to_value(unchanged.assets.unwrap()).unwrap(),
                assets
            );
        }

        drop(owner);
        // The sidecar is deliberately retained. Recovery uses the OS lifetime
        // lock rather than deleting a file or creating a replacement authority.
        assert!(lock.exists());
        client
            .switch_session_in_realm(
                EmbeddedSessionMode::Local,
                pilot.into(),
                EmbeddedRealm::Project,
            )
            .unwrap();
        wait(&mut client, |client| {
            client.ready() && client.snapshot().is_some() && client.assets().is_some()
        });
        assert!(client.paused());
        assert_eq!(client.session_generation(), 3);
        assert_eq!(
            client.scene_layout_revision(),
            Some(SceneLayoutRevision::LegacyV1)
        );
        assert_eq!(
            serde_json::to_value(client.snapshot().unwrap()).unwrap(),
            snapshot
        );
        assert_eq!(
            serde_json::to_value(client.assets().unwrap()).unwrap(),
            assets
        );
        assert_eq!(fs::read(&credential).unwrap(), credential_bytes);
        let competing = EditorWorld::open(&project)
            .err()
            .expect("recovered worker must own the world exclusively");
        assert!(competing.to_string().contains("already owned"));

        client.step();
        wait(&mut client, |client| {
            client
                .snapshot()
                .is_some_and(|s| s.server_time_ms == original_time + 50)
        });
        let stepped = client.snapshot().unwrap();
        assert_eq!(stepped.pilots.len(), 1);
        assert_eq!(stepped.online_pilots, vec![pilot.to_owned()]);
        let stepped = serde_json::to_value(stepped).unwrap();
        let stepped_assets = serde_json::to_value(client.assets().unwrap()).unwrap();
        client.clock.try_send(ClockCommand::Shutdown).unwrap();
        client.cockpit.updates = None;
        wait(&mut client, |client| {
            client.worker.as_ref().unwrap().is_finished()
        });
        client.worker.take().unwrap().join().unwrap();

        let mut reopened = EditorWorld::open(&project).expect("worker shutdown releases ownership");
        let mut session = None;
        assert!(
            reopened
                .handle(
                    &mut session,
                    WorkerIdentity::default()
                        .request("embedded-editor", pilot, Some(token))
                        .unwrap()
                )
                .ok
        );
        let persisted = reopened.handle(&mut session, Request::Snapshot);
        assert_eq!(
            serde_json::to_value(persisted.snapshot.unwrap()).unwrap(),
            stepped
        );
        assert_eq!(
            serde_json::to_value(persisted.assets.unwrap()).unwrap(),
            stepped_assets
        );
        assert_eq!(fs::read(&credential).unwrap(), credential_bytes);
        assert_eq!(fs::read(&sentinel).unwrap(), b"saved project source\n");
        assert!(lock.exists());
    }

    #[test]
    fn failed_preview_open_can_return_to_preserved_project_world() {
        fn wait(client: &mut EmbeddedClient, predicate: impl Fn(&EmbeddedClient) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(8);
            while Instant::now() < deadline {
                client.poll();
                if predicate(client) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!(
                "realm recovery did not reach expected state: {:?}",
                client.status()
            );
        }
        let directory = std::env::temp_dir().join(format!(
            "arkline-realm-recovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = directory.join("project");
        let pilot = "RealmRecoveryPilot";
        let mut world =
            EditorWorld::open_with_creation_scene(&project, SceneLayoutRevision::LegacyV1).unwrap();
        let mut session = None;
        let response = world.handle(
            &mut session,
            WorkerIdentity::default()
                .request("embedded-editor", pilot, None)
                .unwrap(),
        );
        assert!(response.ok);
        let token = response.resume_token.unwrap();
        fs::create_dir_all(project.join("client")).unwrap();
        let credential = credentials_path(&project.join("client"), "embedded-editor", pilot);
        save_token(&credential, &token).unwrap();
        world.step(150).unwrap();
        let original_time = world.time();
        let original_pilot = world
            .handle(&mut session, Request::Snapshot)
            .snapshot
            .unwrap()
            .pilots
            .into_iter()
            .find(|view| view.name == pilot)
            .unwrap();
        drop(world);
        let sentinel = project.join("keep-project.txt");
        fs::write(&sentinel, b"project recovery data").unwrap();
        let preview = project.join("realms/orbital-preview-v1");
        fs::create_dir_all(&preview).unwrap();
        fs::write(
            preview.join("world.sqlite"),
            b"deliberately corrupt database",
        )
        .unwrap();

        let (mut client, _) = debug_fixture();
        client.project_data_dir = project.clone();
        client.data_dir = project.clone();
        client
            .switch_session_in_realm(
                EmbeddedSessionMode::Local,
                pilot.into(),
                EmbeddedRealm::Project,
            )
            .unwrap();
        wait(&mut client, |client| {
            client.ready() && client.scene_layout_revision() == Some(SceneLayoutRevision::LegacyV1)
        });
        client
            .switch_session_in_realm(
                EmbeddedSessionMode::Local,
                pilot.into(),
                EmbeddedRealm::OrbitalPreview,
            )
            .unwrap();
        wait(&mut client, |client| {
            matches!(client.status(), EmbeddedStatus::Error(_))
        });
        assert_eq!(client.realm(), EmbeddedRealm::OrbitalPreview);
        assert_eq!(client.data_dir, preview);
        assert!(!client.ready());
        assert_eq!(client.scene_layout_revision(), None);
        assert_eq!(fs::read_to_string(&credential).unwrap(), token);
        assert_eq!(fs::read(&sentinel).unwrap(), b"project recovery data");

        client
            .switch_session_in_realm(
                EmbeddedSessionMode::Local,
                pilot.into(),
                EmbeddedRealm::Project,
            )
            .unwrap();
        wait(&mut client, |client| {
            client.ready() && client.scene_layout_revision() == Some(SceneLayoutRevision::LegacyV1)
        });
        let restored = client.snapshot().unwrap();
        assert_eq!(client.data_dir, project);
        assert_eq!(client.realm(), EmbeddedRealm::Project);
        assert_eq!(restored.server_time_ms, original_time);
        assert_eq!(
            restored.pilots.iter().find(|view| view.name == pilot),
            Some(&original_pilot)
        );
        assert_eq!(fs::read_to_string(credential).unwrap(), token);
        assert_eq!(fs::read(sentinel).unwrap(), b"project recovery data");
        let _ = client.clock.try_send(ClockCommand::Shutdown);
        client.cockpit.updates = None;
        client.worker.take().unwrap().join().unwrap();
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn realm_directories_use_fixed_project_children_and_reject_unsafe_existing_paths() {
        let directory = std::env::temp_dir().join(format!(
            "arkline-realm-paths-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let (mut client, _) = debug_fixture();
        client.project_data_dir = directory.clone();
        assert_eq!(
            client.realm_directory(EmbeddedRealm::Project).unwrap(),
            directory
        );
        assert_eq!(
            client
                .realm_directory(EmbeddedRealm::OrbitalPreview)
                .unwrap()
                .strip_prefix(&directory)
                .unwrap(),
            std::path::Path::new("realms/orbital-preview-v1")
        );
        fs::write(directory.join("realms"), b"not a folder").unwrap();
        assert!(client
            .realm_directory(EmbeddedRealm::OrbitalPreview)
            .is_err());
        fs::remove_file(directory.join("realms")).unwrap();
        #[cfg(unix)]
        {
            let outside = directory.with_extension("outside");
            fs::create_dir(&outside).unwrap();
            std::os::unix::fs::symlink(&outside, directory.join("realms")).unwrap();
            assert!(client
                .realm_directory(EmbeddedRealm::OrbitalPreview)
                .is_err());
            assert!(!outside.join("orbital-preview-v1").exists());
            fs::remove_file(directory.join("realms")).unwrap();
            fs::remove_dir(outside).unwrap();
        }
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn embedded_host_join_switches_share_world_and_return_to_preserved_local_world() {
        let directory = std::env::temp_dir().join(format!(
            "arkline-session-switch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fn wait(client: &mut EmbeddedClient, predicate: impl Fn(&EmbeddedClient) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(8);
            while Instant::now() < deadline {
                client.poll();
                if predicate(client) {
                    return;
                }
                if let EmbeddedStatus::Error(error) = client.status() {
                    panic!("session error: {error}");
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!("editor session did not reach expected state");
        }
        fn finish(client: &mut EmbeddedClient) {
            let _ = client.clock.try_send(ClockCommand::Shutdown);
            client.cockpit.updates = None;
            if let Some(worker) = client.worker.take() {
                worker.join().unwrap();
            }
        }
        let (mut host, _) = debug_fixture();
        host.data_dir = directory.join("host");
        host.switch_session(
            EmbeddedSessionMode::Host {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
            },
            "EditorHostQA".into(),
        )
        .unwrap();
        wait(&mut host, |c| c.ready() && c.session_info().is_some());
        let info = host.session_info().unwrap();
        let address = info.address.unwrap();
        let secret = info.editor_secret.clone().unwrap();
        let (mut guest, _) = debug_fixture();
        guest.data_dir = directory.join("guest");
        guest
            .switch_session(
                EmbeddedSessionMode::Join { address, secret },
                "EditorGuestQA".into(),
            )
            .unwrap();
        wait(&mut guest, |c| {
            c.ready() && c.snapshot().is_some_and(|s| s.pilots.len() == 2)
        });
        assert!(!guest.can_control_clock());
        host.play();
        wait(&mut guest, |c| c.status() == &EmbeddedStatus::Running);
        guest.stop();
        wait(&mut guest, |c| c.status() == &EmbeddedStatus::Stopped);
        wait(&mut host, |c| c.status() == &EmbeddedStatus::Running);
        guest.play();
        wait(&mut guest, |c| c.status() == &EmbeddedStatus::Running);
        finish(&mut guest);
        host.switch_session(EmbeddedSessionMode::Local, "EditorHostQA".into())
            .unwrap();
        wait(&mut host, |c| {
            c.ready()
                && c.snapshot()
                    .is_some_and(|s| s.pilots.iter().any(|p| p.name == "EditorHostQA"))
        });
        assert!(host.can_control_clock());
        assert_eq!(host.status(), &EmbeddedStatus::Paused);
        finish(&mut host);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn shared_rejoin_and_host_restart_preserve_accepted_rooms_credentials_and_single_authority() {
        #[track_caller]
        fn wait(client: &mut EmbeddedClient, predicate: impl Fn(&EmbeddedClient) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(8);
            while Instant::now() < deadline {
                client.poll();
                if predicate(client) {
                    return;
                }
                assert!(
                    !matches!(client.status(), EmbeddedStatus::Error(_)),
                    "unexpected session failure: {:?}",
                    client.status()
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!(
                "shared recovery did not reach expected state: {:?}, time {:?}, room {:?}, ack {:?}",
                client.status(),
                client.snapshot().map(|s| s.server_time_ms),
                client.assets().and_then(|a| a.pioneer_room_layout).map(|r| r.revision),
                client.room_layout_ack(),
            );
        }
        fn finish(client: &mut EmbeddedClient) {
            client.clock.try_send(ClockCommand::Shutdown).unwrap();
            client.cockpit.updates = None;
            wait(client, |client| {
                client.worker.as_ref().unwrap().is_finished()
            });
            client.worker.take().unwrap().join().unwrap();
        }
        fn room(client: &EmbeddedClient) -> arkline_protocol::RoomLayoutState {
            client.assets().unwrap().pioneer_room_layout.unwrap()
        }
        fn identities(client: &EmbeddedClient) {
            let snapshot = client.snapshot().unwrap();
            let mut names = snapshot
                .pilots
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>();
            names.sort_unstable();
            assert_eq!(names, ["RecoveryGuest", "RecoveryHost"]);
            let mut online = snapshot.online_pilots.clone();
            online.sort_unstable();
            assert_eq!(online, ["RecoveryGuest", "RecoveryHost"]);
        }
        struct TestClient(EmbeddedClient);
        impl std::ops::Deref for TestClient {
            type Target = EmbeddedClient;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
        impl std::ops::DerefMut for TestClient {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }
        impl Drop for TestClient {
            fn drop(&mut self) {
                let (closed, _) = mpsc::sync_channel(1);
                drop(std::mem::replace(&mut self.0.clock, closed));
                self.0.cockpit.updates = None;
                if let Some(worker) = self.0.worker.take() {
                    let _ = worker.join();
                }
            }
        }
        struct Directory(PathBuf);
        impl Drop for Directory {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let directory = Directory(std::env::temp_dir().join(format!(
            "arkline-shared-recovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        )));
        let host_dir = directory.0.join("host");
        let guest_dir = directory.0.join("guest");
        let (host, _) = debug_fixture();
        let mut host = TestClient(host);
        host.data_dir = host_dir.clone();
        host.switch_session(
            EmbeddedSessionMode::Host {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
            },
            "RecoveryHost".into(),
        )
        .unwrap();
        wait(&mut host, |c| {
            c.ready() && c.session_info().is_some() && c.assets().is_some()
        });
        let info = host.session_info().unwrap();
        let address = info.address.unwrap();
        let old_secret = info.editor_secret.clone().unwrap();
        let (guest, _) = debug_fixture();
        let mut guest = TestClient(guest);
        guest.data_dir = guest_dir.clone();
        guest
            .switch_session(
                EmbeddedSessionMode::Join {
                    address,
                    secret: old_secret.clone(),
                },
                "RecoveryGuest".into(),
            )
            .unwrap();
        wait(&mut guest, |c| c.ready() && c.assets().is_some());
        let mut accepted = room(&guest).document;
        accepted.rooms[0].name = "Accepted before restart".into();
        let id = guest.room_layout_replace(0, accepted).unwrap();
        wait(&mut guest, |c| {
            c.room_layout_ack().is_some_and(|a| a.id == id) && room(c).revision == 1
        });
        assert!(guest.room_layout_ack().unwrap().result.is_ok());
        let accepted = room(&guest);
        let stepped_time = guest.snapshot().unwrap().server_time_ms + 50;
        host.step();
        wait(&mut host, |c| {
            c.snapshot()
                .is_some_and(|s| s.server_time_ms == stepped_time)
        });
        wait(&mut guest, |c| {
            c.snapshot()
                .is_some_and(|s| s.server_time_ms == stepped_time)
        });
        identities(&host);
        identities(&guest);
        let accepted_assets = serde_json::to_value(guest.assets().unwrap()).unwrap();
        let ledger = guest.snapshot().unwrap().ledger_hash.clone();
        let host_credential =
            credentials_path(&host_dir.join("client"), "embedded-editor", "RecoveryHost");
        let guest_domain = format!("shared-editor:{address}");
        let guest_credential =
            credentials_path(&guest_dir.join("client"), &guest_domain, "RecoveryGuest");
        let host_token = fs::read(&host_credential).unwrap();
        let guest_token = fs::read(&guest_credential).unwrap();
        let mut drafts = Vec::new();
        for project in [&host_dir, &guest_dir] {
            fs::create_dir_all(project.join("Assets")).unwrap();
            let path = project.join("Assets/Pioneer.rooms.json");
            let mut draft = accepted.document.clone();
            draft.rooms[0].name = "Saved draft, not applied".into();
            let bytes = serde_json::to_vec_pretty(&draft).unwrap();
            fs::write(&path, &bytes).unwrap();
            drafts.push((path, bytes));
        }
        let preserved = || {
            assert!(
                fs::read(&host_credential).unwrap() == host_token,
                "host credential changed"
            );
            assert!(
                fs::read(&guest_credential).unwrap() == guest_token,
                "guest credential changed"
            );
            for (path, bytes) in &drafts {
                assert!(fs::read(path).unwrap() == *bytes, "saved draft changed");
            }
        };

        // A real Leave and a fresh worker must reuse the existing pilot claim.
        finish(&mut guest);
        wait(&mut host, |c| {
            c.snapshot()
                .is_some_and(|s| s.online_pilots == ["RecoveryHost"])
        });
        guest
            .switch_session(
                EmbeddedSessionMode::Join {
                    address,
                    secret: old_secret.clone(),
                },
                "RecoveryGuest".into(),
            )
            .unwrap();
        wait(&mut guest, |c| c.ready() && c.assets().is_some());
        identities(&guest);
        assert_eq!(room(&guest), accepted);
        assert_eq!(
            serde_json::to_value(guest.assets().unwrap()).unwrap(),
            accepted_assets
        );
        preserved();

        // Stop the actual host authority while the guest is still attached.
        finish(&mut host);
        wait(&mut guest, |c| {
            matches!(c.status(), EmbeddedStatus::Error(_))
        });
        assert!(!guest.ready() && !guest.can_control_clock());
        let mut unsubmitted = accepted.document.clone();
        unsubmitted.rooms[0].name = "Must never be replayed".into();
        assert!(guest.room_layout_replace(1, unsubmitted).is_err());
        preserved();
        // Shutdown waits for database release; the detached acceptor may need
        // one more scheduling turn to release the same endpoint. Retry only
        // that bounded listener-startup failure, never a world/auth failure.
        let reopen_deadline = Instant::now() + Duration::from_secs(8);
        loop {
            host.switch_session(
                EmbeddedSessionMode::Host { bind_addr: address },
                "RecoveryHost".into(),
            )
            .unwrap();
            wait(&mut host, |c| {
                matches!(c.status(), EmbeddedStatus::Error(_))
                    || (c.ready() && c.session_info().is_some() && c.assets().is_some())
            });
            if host.ready() {
                break;
            }
            let EmbeddedStatus::Error(error) = host.status() else {
                unreachable!()
            };
            assert!(error.contains("bind editor loopback listener"), "{error}");
            assert!(
                Instant::now() < reopen_deadline,
                "listener did not release the endpoint"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(host.paused());
        assert_eq!(host.session_info().unwrap().address, Some(address));
        assert_eq!(host.snapshot().unwrap().server_time_ms, stepped_time);
        assert_eq!(room(&host), accepted);
        let secret = host.session_info().unwrap().editor_secret.clone().unwrap();
        let connect = || {
            WorkerIdentity::default()
                .request(&guest_domain, "RecoveryGuest", None)
                .unwrap()
        };
        use arkline_server::collaborative_transport::DevelopmentEditorClient;
        let resumed = WorkerIdentity::default()
            .request(
                &guest_domain,
                "RecoveryGuest",
                Some(String::from_utf8(guest_token.clone()).unwrap()),
            )
            .unwrap();
        let Err(old_secret_error) =
            DevelopmentEditorClient::connect(address, &old_secret, Some(resumed))
        else {
            panic!("old host secret must not admit a valid saved pilot credential");
        };
        assert!(old_secret_error
            .to_string()
            .contains("invalid editor join secret"));
        let Err(duplicate_error) =
            DevelopmentEditorClient::connect(address, &secret, Some(connect()))
        else {
            panic!("existing pilot requires its saved credential");
        };
        assert!(duplicate_error
            .to_string()
            .contains("resume token required"));
        let competing = EditorWorld::open(&host_dir)
            .err()
            .expect("host must retain exclusive world ownership");
        assert!(competing.to_string().contains("already owned"));
        guest
            .switch_session(
                EmbeddedSessionMode::Join { address, secret },
                "RecoveryGuest".into(),
            )
            .unwrap();
        wait(&mut guest, |c| c.ready() && c.assets().is_some());
        wait(&mut host, |c| {
            c.snapshot().is_some_and(|s| s.online_pilots.len() == 2)
        });
        identities(&guest);
        identities(&host);
        assert_eq!(room(&guest), accepted);
        assert_eq!(
            serde_json::to_value(guest.assets().unwrap()).unwrap(),
            accepted_assets
        );
        assert_eq!(guest.snapshot().unwrap().server_time_ms, stepped_time);
        assert_eq!(guest.snapshot().unwrap().ledger_hash, ledger);
        assert!(guest.paused() && !guest.can_control_clock());
        assert!(guest.room_pending.is_none() && guest.room_layout_ack().is_none());
        preserved();
        host.step();
        wait(&mut host, |c| {
            c.snapshot()
                .is_some_and(|s| s.server_time_ms == stepped_time + 50)
        });
        wait(&mut guest, |c| {
            c.snapshot()
                .is_some_and(|s| s.server_time_ms == stepped_time + 50)
        });
        assert_eq!(room(&guest), accepted);
        finish(&mut guest);
        finish(&mut host);
        assert!(
            EditorWorld::open(&host_dir).is_ok(),
            "final shutdown releases authority"
        );
        preserved();
    }

    #[test]
    fn embedded_scene_state_local_and_shared_workers_ack_confirmed_placement() {
        fn wait(client: &mut EmbeddedClient, predicate: impl Fn(&EmbeddedClient) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(8);
            while Instant::now() < deadline {
                client.poll();
                if predicate(client) {
                    return;
                }
                assert!(
                    !matches!(client.status(), EmbeddedStatus::Error(_)),
                    "worker failed"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!(
                "editor scene state did not become confirmed: {:?}",
                client.scene_state_ack()
            );
        }
        fn finish(client: &mut EmbeddedClient) {
            let _ = client.clock.try_send(ClockCommand::Shutdown);
            client.cockpit.updates = None;
            if let Some(worker) = client.worker.take() {
                worker.join().unwrap();
            }
        }
        let directory = std::env::temp_dir().join(format!(
            "arkline-scene-bridge-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (mut local, _) = debug_fixture();
        local.data_dir = directory.join("local");
        local
            .switch_session(EmbeddedSessionMode::Local, "SceneLocal".into())
            .unwrap();
        wait(&mut local, |client| client.ready());
        let time = local.snapshot().unwrap().server_time_ms;
        let id = local
            .set_scene_state(EditorSceneState::InteriorDeck2)
            .unwrap();
        assert!(local.scene_state_pending());
        assert!(local.set_scene_state(EditorSceneState::Cockpit).is_err());
        wait(&mut local, |client| {
            client.scene_state_ack().is_some_and(|ack| ack.id == id)
                && client.scene_state() == Some(EditorSceneState::InteriorDeck2)
        });
        assert!(local.scene_state_ack().unwrap().result.is_ok());
        assert_eq!(local.snapshot().unwrap().server_time_ms, time);
        assert_eq!(
            local.debug_view_state().current,
            Some(EmbeddedView::FirstPerson)
        );
        let id = local.set_scene_state(EditorSceneState::Cockpit).unwrap();
        wait(&mut local, |client| {
            client.scene_state_ack().is_some_and(|ack| ack.id == id)
                && client.scene_state() == Some(EditorSceneState::Cockpit)
        });
        assert_eq!(
            local.debug_view_state().current,
            Some(EmbeddedView::Cockpit)
        );
        local.set_debug_view(EmbeddedView::External).unwrap();
        assert_eq!(
            local.debug_view_state().current,
            Some(EmbeddedView::External)
        );
        assert_eq!(local.snapshot().unwrap().server_time_ms, time);
        local.play();
        wait(&mut local, |client| {
            client.status() == &EmbeddedStatus::Running
        });
        let id = local
            .set_scene_state(EditorSceneState::StationHangar)
            .unwrap();
        wait(&mut local, |client| {
            client.scene_state_ack().is_some_and(|ack| ack.id == id)
                && client.scene_state() == Some(EditorSceneState::StationHangar)
        });
        assert_eq!(local.status(), &EmbeddedStatus::Running);
        finish(&mut local);

        let (mut host, _) = debug_fixture();
        host.data_dir = directory.join("host");
        host.switch_session(
            EmbeddedSessionMode::Host {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
            },
            "SceneBridgeHost".into(),
        )
        .unwrap();
        wait(&mut host, |client| {
            client.ready() && client.session_info().is_some()
        });
        let info = host.session_info().unwrap();
        let (address, secret) = (info.address.unwrap(), info.editor_secret.clone().unwrap());
        let (mut guest, _) = debug_fixture();
        guest.data_dir = directory.join("guest");
        guest
            .switch_session(
                EmbeddedSessionMode::Join { address, secret },
                "SceneBridgeGuest".into(),
            )
            .unwrap();
        wait(&mut guest, |client| client.ready());
        let id = guest
            .set_scene_state(EditorSceneState::InteriorDeck1)
            .unwrap();
        wait(&mut guest, |client| {
            client.scene_state_ack().is_some_and(|ack| ack.id == id)
                && client.scene_state() == Some(EditorSceneState::InteriorDeck1)
        });
        assert!(guest.scene_state_ack().unwrap().result.is_ok());
        wait(&mut host, |client| {
            client.snapshot().is_some_and(|snapshot| {
                snapshot
                    .pilots
                    .iter()
                    .find(|pilot| pilot.name == "SceneBridgeGuest")
                    .is_some_and(|pilot| {
                        pilot.station_avatar.as_ref().is_some_and(|avatar| {
                            avatar.onboard_interior && avatar.interior_deck == 1
                        })
                    })
            })
        });
        assert_eq!(host.scene_state(), Some(EditorSceneState::Cockpit));
        host.play();
        wait(&mut guest, |client| {
            client.status() == &EmbeddedStatus::Running
        });
        let id = guest
            .set_scene_state(EditorSceneState::InteriorDeck2)
            .unwrap();
        wait(&mut guest, |client| {
            client.scene_state_ack().is_some_and(|ack| ack.id == id)
                && client.scene_state() == Some(EditorSceneState::InteriorDeck2)
        });
        assert_eq!(guest.status(), &EmbeddedStatus::Running);
        finish(&mut guest);
        finish(&mut host);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn free_camera_suppresses_gameplay_input_even_over_viewport() {
        assert!(!gameplay_input_allowed(true, true));
        assert!(!gameplay_input_allowed(false, true));
        assert!(gameplay_input_allowed(true, false));
    }
    #[test]
    fn free_camera_temporarily_uses_3d_and_restores_tactical_view() {
        let (mut client, _) = debug_fixture();
        client.cockpit.view_mode = ViewMode::Tactical;
        client.set_free_camera(true);
        assert_eq!(client.cockpit.view_mode, ViewMode::External);
        assert!(client.free_camera_active());
        client.set_free_camera(false);
        assert_eq!(client.cockpit.view_mode, ViewMode::Tactical);
        assert!(!client.free_camera_active());
    }
    #[test]
    fn embedded_free_camera_moves_without_issuing_station_gameplay_commands() {
        let (mut client, requests) = debug_fixture();
        client.state = EmbeddedStatus::Running;
        let pilot = &mut client.cockpit.snapshot.as_mut().unwrap().pilots[0];
        pilot.station_avatar = Some(StationAvatar {
            onboard_interior: false,
            interior_deck: 1,
            jump_started_ms: None,
            seated: false,
            position: arkline_protocol::hangar_layout(pilot.ship_class).boarding_point,
            heading: 0.0,
            moving: false,
            updated_at_ms: 0,
        });
        let ctx = egui::Context::default();
        client.set_free_camera(true);
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 650.0),
                )),
                time: Some(1.0),
                focused: true,
                ..Default::default()
            },
            |ctx| client.update(ctx, &mut eframe::Frame::_new_kittest()),
        );
        let before = client.free_camera.unwrap().0;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 650.0),
                )),
                time: Some(1.05),
                focused: true,
                events: vec![
                    egui::Event::PointerMoved(egui::pos2(450.0, 325.0)),
                    egui::Event::Key {
                        key: egui::Key::W,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::Key {
                        key: egui::Key::Space,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ctx| client.update(ctx, &mut eframe::Frame::_new_kittest()),
        );
        assert!(client.free_camera.unwrap().0.distance(before) > 0.0);
        assert!(!client.cockpit.embedded_input);
        assert!(requests.try_recv().is_err());

        let mut transition_poses = Vec::new();
        let pilot = &mut client.cockpit.snapshot.as_mut().unwrap().pilots[0];
        pilot.location = Location::Belt;
        pilot.station_avatar = None;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 650.0),
                )),
                time: Some(1.1),
                focused: true,
                ..Default::default()
            },
            |ctx| {
                client.update(ctx, &mut eframe::Frame::_new_kittest());
                transition_poses.push(client.free_camera.unwrap());
                // A repeated UI pass must not apply held movement again.
                client.update_editor_free_camera(ctx);
                assert_eq!(client.free_camera, transition_poses.last().copied());
            },
        );
        assert_eq!(client.free_camera_scene_hangar, Some(false));
        assert!(transition_poses
            .iter()
            .all(|pose| *pose == transition_poses[0]));
        assert_ne!(client.free_camera.unwrap().0, before);
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn embedded_free_camera_input_has_expected_axes_and_ignores_outside_or_repeated_input() {
        fn moved_position(key: egui::Key) -> Vec3 {
            let (mut client, _) = debug_fixture();
            client.free_camera = Some((Vec3::ZERO, 0.0, 0.0));
            let ctx = egui::Context::default();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 650.0),
                    )),
                    time: Some(1.0),
                    focused: true,
                    events: vec![
                        egui::Event::PointerMoved(egui::pos2(450.0, 325.0)),
                        egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ctx| client.update_editor_free_camera(ctx),
            );
            client.free_camera.unwrap().0
        }

        // At yaw and pitch zero, forward is +X, right is +Z, and vertical
        // movement follows the world Y axis.
        for (key, axis, sign) in [
            (egui::Key::W, 0, 1.0),
            (egui::Key::S, 0, -1.0),
            (egui::Key::D, 2, 1.0),
            (egui::Key::A, 2, -1.0),
            (egui::Key::E, 1, 1.0),
            (egui::Key::Space, 1, 1.0),
            (egui::Key::Q, 1, -1.0),
            (egui::Key::C, 1, -1.0),
        ] {
            let position = moved_position(key);
            let component = [position.x, position.y, position.z][axis];
            assert!(
                component * sign > 0.0,
                "{key:?} should move along axis {axis} with sign {sign}, got {position:?}"
            );
            for (other_axis, value) in [position.x, position.y, position.z].into_iter().enumerate()
            {
                if other_axis != axis {
                    assert_eq!(value, 0.0, "{key:?} changed an unintended axis");
                }
            }
        }

        let (mut client, _) = debug_fixture();
        client.free_camera = Some((Vec3::ZERO, 0.0, 0.0));
        let ctx = egui::Context::default();
        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 650.0),
            )),
            time: Some(2.0),
            focused: true,
            events: vec![
                egui::Event::PointerMoved(egui::pos2(950.0, 325.0)),
                egui::Event::Key {
                    key: egui::Key::W,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        let _ = ctx.run(raw_input, |ctx| client.update_editor_free_camera(ctx));
        assert_eq!(client.free_camera.unwrap().0, Vec3::ZERO);

        // Calling the updater twice with the same egui input time applies
        // the held movement only once.
        let repeated_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 650.0),
            )),
            time: Some(3.0),
            focused: true,
            events: vec![
                egui::Event::PointerMoved(egui::pos2(450.0, 325.0)),
                egui::Event::Key {
                    key: egui::Key::W,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        let _ = ctx.run(repeated_input, |ctx| {
            client.update_editor_free_camera(ctx);
            let once = client.free_camera.unwrap().0;
            client.update_editor_free_camera(ctx);
            assert_eq!(client.free_camera.unwrap().0, once);
        });

        let (mut client, _) = debug_fixture();
        client.free_camera = Some((Vec3::ZERO, 0.0, 0.0));
        let ctx = egui::Context::default();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 650.0),
                )),
                time: Some(4.0),
                focused: true,
                events: vec![egui::Event::PointerMoved(egui::pos2(450.0, 325.0))],
                ..Default::default()
            },
            |ctx| client.update_editor_free_camera(ctx),
        );
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 650.0),
                )),
                time: Some(4.1),
                focused: true,
                events: vec![
                    egui::Event::PointerMoved(egui::pos2(460.0, 315.0)),
                    egui::Event::PointerButton {
                        pos: egui::pos2(460.0, 315.0),
                        button: egui::PointerButton::Secondary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ctx| client.update_editor_free_camera(ctx),
        );
        let (_, yaw, pitch) = client.free_camera.unwrap();
        assert!(
            yaw > 0.0,
            "positive horizontal mouse delta should turn right"
        );
        assert!(pitch > 0.0, "upward mouse delta should pitch upward");
    }

    fn debug_fixture() -> (EmbeddedClient, Receiver<Request>) {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.embedded = true;
        cockpit.connected = true;
        cockpit.snapshot.as_mut().unwrap().pilots[0].location = Location::Station;
        cockpit.snapshot.as_mut().unwrap().pilots[0].station_avatar = None;
        let (commands, requests) = mpsc::channel();
        cockpit.commands = Some(commands);
        let (clock, _) = mpsc::sync_channel(16);
        let (_, states) = mpsc::channel();
        (
            EmbeddedClient {
                project_data_dir: PathBuf::new(),
                data_dir: PathBuf::new(),
                realm: EmbeddedRealm::Project,
                mode: EmbeddedSessionMode::Local,
                session_info: None,
                session_updates: mpsc::channel().1,
                worker: None,
                cockpit,
                clock,
                graph_states: mpsc::sync_channel(1).1,
                graph_state: None,
                graph_pending: None,
                graph_sequence: 0,
                room_pending: None,
                room_ack: None,
                room_sequence: 0,
                scene_pending: None,
                scene_ack: None,
                scene_sequence: 0,
                exterior_pending: None,
                exterior_ack: None,
                exterior_sequence: 0,
                session_generation: 0,
                states,
                state: EmbeddedStatus::Paused,
                destroyed: false,
                views: ViewDiagnostics {
                    started: Some(Instant::now()),
                    ..Default::default()
                },
                free_camera: None,
                free_camera_enabled: false,
                free_camera_scene_hangar: None,
                free_camera_saved_view: None,
                free_camera_input_time: None,
            },
            requests,
        )
    }
    #[test]
    fn confirmed_stop_resume_feedback_clears_only_its_own_waiting_notice() {
        let (mut client, gameplay_requests) = debug_fixture();
        client.state = EmbeddedStatus::Running;
        let (clock, commands) = mpsc::sync_channel(4);
        client.clock = clock;
        let (states, receiver) = mpsc::channel();
        client.states = receiver;
        client.stop();
        assert!(matches!(commands.try_recv().unwrap(), ClockCommand::Stop));
        client.poll();
        assert_eq!(
            client.cockpit.notice, STOP_REQUEST_NOTICE,
            "queueing is not confirmation"
        );
        // A Running report can already be in flight when Stop is requested.
        // It confirms neither receipt nor acceptance of the Stop command.
        states.send(EmbeddedStatus::Running).unwrap();
        client.poll();
        assert_eq!(client.cockpit.notice, STOP_REQUEST_NOTICE);
        states.send(EmbeddedStatus::Stopped).unwrap();
        client.poll();
        assert_eq!(client.status(), &EmbeddedStatus::Stopped);
        assert!(client.cockpit.notice.is_empty());
        client.play();
        assert!(matches!(commands.try_recv().unwrap(), ClockCommand::Play));
        states.send(EmbeddedStatus::Running).unwrap();
        client.poll();
        assert_eq!(client.status(), &EmbeddedStatus::Running);
        assert!(client.cockpit.notice.is_empty());
        client.stop();
        assert!(matches!(commands.try_recv().unwrap(), ClockCommand::Stop));
        client.play();
        assert!(matches!(commands.try_recv().unwrap(), ClockCommand::Play));
        states.send(EmbeddedStatus::Stopped).unwrap();
        states.send(EmbeddedStatus::Running).unwrap();
        client.poll();
        assert_eq!(client.status(), &EmbeddedStatus::Running);
        assert!(
            client.cockpit.notice.is_empty(),
            "Stop and Resume confirmations may arrive in the same poll"
        );
        client.cockpit.notice = "Airlock is pressure-interlocked".into();
        states.send(EmbeddedStatus::Stopped).unwrap();
        client.poll();
        assert_eq!(client.cockpit.notice, "Airlock is pressure-interlocked");
        client.cockpit.notice = "Real authority failure".into();
        states
            .send(EmbeddedStatus::Error("Real authority failure".into()))
            .unwrap();
        client.poll();
        assert_eq!(client.cockpit.notice, "Real authority failure");
        client.cockpit.notice = STOP_REQUEST_NOTICE.into();
        client.poll();
        assert!(client.cockpit.notice.contains("outcome unknown"));
        assert!(!client.cockpit.notice.contains("waiting"));
        assert!(gameplay_requests.try_recv().is_err());
        assert!(
            commands.try_recv().is_err(),
            "feedback must not retry clock commands"
        );
    }

    #[test]
    fn embedded_stop_resume_clears_notice_and_preserves_world_and_authored_graph() {
        fn wait(client: &mut EmbeddedClient, predicate: impl Fn(&EmbeddedClient) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(8);
            while Instant::now() < deadline {
                client.poll();
                if predicate(client) {
                    return;
                }
                assert!(
                    !matches!(client.status(), EmbeddedStatus::Error(_)),
                    "worker failed"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!("clock transition was not confirmed: {:?}", client.status());
        }
        let (mut client, _) = debug_fixture();
        let directory = std::env::temp_dir().join(format!(
            "arkline-stop-feedback-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        client.data_dir = directory.clone();
        client
            .switch_session(EmbeddedSessionMode::Local, "StopFeedbackPilot".into())
            .unwrap();
        wait(&mut client, |client| client.ready());
        let scene_id = client
            .set_scene_state(EditorSceneState::InteriorDeck2)
            .unwrap();
        wait(&mut client, |client| {
            client
                .scene_state_ack()
                .is_some_and(|ack| ack.id == scene_id)
                && client.scene_state() == Some(EditorSceneState::InteriorDeck2)
        });
        let authored = EditorEventGraph {
            nodes: vec![arkline_protocol::EditorGraphNode {
                id: 1,
                label: "Retained development graph".into(),
                position: [40.0, 80.0],
                operation: arkline_protocol::EditorNodeOp::Manual,
                breakpoint: false,
            }],
            ..Default::default()
        };
        let id = client.graph_replace(0, authored.clone(), false).unwrap();
        wait(&mut client, |client| {
            client.graph_debug().is_some_and(|debug| {
                debug
                    .ack
                    .as_ref()
                    .is_some_and(|ack| ack.id == id && ack.result.is_ok())
            })
        });
        let before = client.snapshot().unwrap();
        let pilot_before = before
            .pilots
            .iter()
            .find(|pilot| pilot.name == "StopFeedbackPilot")
            .unwrap()
            .clone();
        client.stop();
        wait(&mut client, |client| {
            client.status() == &EmbeddedStatus::Stopped
        });
        assert!(client.cockpit.notice.is_empty());
        assert_eq!(
            client.snapshot().unwrap().server_time_ms,
            before.server_time_ms
        );
        client.play();
        wait(&mut client, |client| {
            client.status() == &EmbeddedStatus::Running
        });
        assert!(!client.cockpit.notice.contains("Stop requested"));
        client.pause();
        wait(&mut client, |client| client.paused());
        let after = client.snapshot().unwrap();
        let pilot_after = after
            .pilots
            .iter()
            .find(|pilot| pilot.name == "StopFeedbackPilot")
            .unwrap();
        assert_eq!(pilot_after.ship_class, pilot_before.ship_class);
        assert_eq!(pilot_after.ore, pilot_before.ore);
        assert_eq!(pilot_after.credits, pilot_before.credits);
        assert_eq!(
            pilot_after.station_avatar.as_ref().unwrap().position,
            pilot_before.station_avatar.as_ref().unwrap().position
        );
        assert_eq!(client.scene_state(), Some(EditorSceneState::InteriorDeck2));
        assert_eq!(
            serde_json::to_value(&client.graph_debug().unwrap().debug.graph).unwrap(),
            serde_json::to_value(authored).unwrap()
        );
        let _ = client.clock.try_send(ClockCommand::Shutdown);
        client.cockpit.updates = None;
        client.worker.take().unwrap().join().unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn scene_state_lost_ack_cancels_pending_without_replay_or_late_camera_reset() {
        fn accepted(id: u64) -> EmbeddedEventDebugState {
            EmbeddedEventDebugState {
                debug: EditorGraphDebug {
                    revision: 0,
                    graph: EditorEventGraph::default(),
                    pending: 0,
                    pending_nodes: vec![],
                    breakpoint_node: None,
                    trace: vec![],
                    paused: true,
                },
                ack: None,
                room_ack: None,
                scene_ack: Some(EmbeddedEventAck { id, result: Ok(()) }),
                exterior_ack: None,
            }
        }
        for status in [
            Some(EmbeddedStatus::Error("worker failed".into())),
            Some(EmbeddedStatus::Stopped),
            None,
        ] {
            let (mut client, gameplay_requests) = debug_fixture();
            let (clock, commands) = mpsc::sync_channel(4);
            client.clock = clock;
            let (debug_tx, debug_rx) = mpsc::sync_channel(4);
            client.graph_states = debug_rx;
            let (states, receiver) = mpsc::channel();
            client.states = receiver;
            let (updates, receiver) = mpsc::channel();
            client.cockpit.updates = Some(receiver);
            client.cockpit.view_mode = ViewMode::Tactical;
            client.set_free_camera(true);
            let before = serde_json::to_value(client.snapshot()).unwrap();
            let id = client
                .set_scene_state(EditorSceneState::InteriorDeck2)
                .unwrap();
            assert!(
                matches!(commands.try_recv().unwrap(), ClockCommand::SceneState(sent, EditorSceneState::InteriorDeck2) if sent == id)
            );
            if let Some(status) = status {
                states.send(status).unwrap();
            } else {
                updates
                    .send(Update::Reconnecting("transport disconnected".into()))
                    .unwrap();
            }
            client.poll();
            assert!(!client.scene_state_pending());
            let failure = client
                .scene_state_ack()
                .unwrap()
                .result
                .as_ref()
                .unwrap_err()
                .clone();
            assert!(failure.contains("outcome unknown") && failure.contains("not replayed"));
            assert_eq!(serde_json::to_value(client.snapshot()).unwrap(), before);
            debug_tx.send(accepted(id)).unwrap();
            client.poll();
            assert_eq!(client.scene_state_ack().unwrap().result, Err(failure));
            assert!(
                client.free_camera_active(),
                "late ack must not reset the camera"
            );
            assert_eq!(client.cockpit.view_mode, ViewMode::External);
            assert!(
                commands.try_recv().is_err(),
                "failed command must not be replayed"
            );
            assert!(gameplay_requests.try_recv().is_err());
        }

        // Switching authority generations drops the old acknowledgement queue
        // and preserves command IDs, so stale success cannot affect the new pilot.
        let (mut client, gameplay_requests) = debug_fixture();
        let directory = std::env::temp_dir().join(format!(
            "arkline-scene-generation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        client.data_dir = directory.clone();
        let (clock, commands) = mpsc::sync_channel(4);
        client.clock = clock;
        let (old_debug, receiver) = mpsc::sync_channel(4);
        client.graph_states = receiver;
        let id = client
            .set_scene_state(EditorSceneState::InteriorDeck2)
            .unwrap();
        assert!(
            matches!(commands.try_recv().unwrap(), ClockCommand::SceneState(sent, _) if sent == id)
        );
        let generation = client.session_generation();
        client
            .switch_session(EmbeddedSessionMode::Local, "SceneFreshGeneration".into())
            .unwrap();
        assert_eq!(client.session_generation(), generation + 1);
        assert!(!client.scene_state_pending());
        assert!(client.scene_state_ack().is_none());
        assert_eq!(client.scene_sequence, id);
        assert!(old_debug.send(accepted(id)).is_err());
        assert!(matches!(
            commands.try_recv().unwrap(),
            ClockCommand::Shutdown
        ));
        assert!(commands.try_recv().is_err());
        assert!(gameplay_requests.try_recv().is_err());
        let deadline = Instant::now() + Duration::from_secs(8);
        while !client.ready() && Instant::now() < deadline {
            client.poll();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(client.ready());
        assert_eq!(
            client.scene_state(),
            Some(EditorSceneState::Cockpit),
            "previous placement was not replayed in the new generation"
        );
        let _ = client.clock.try_send(ClockCommand::Shutdown);
        client.cockpit.updates = None;
        client.worker.take().unwrap().join().unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn graph_commands_are_bounded_acknowledged_and_do_not_optimistically_pause() {
        let (mut client, _) = debug_fixture();
        let (clock, commands) = mpsc::sync_channel(1);
        client.clock = clock;
        let (debug_tx, debug_rx) = mpsc::sync_channel(1);
        client.graph_states = debug_rx;
        client.state = EmbeddedStatus::Running;
        let id = client.graph_step().unwrap();
        assert_eq!(client.status(), &EmbeddedStatus::Running);
        assert!(client.graph_clear().is_err());
        assert!(matches!(
            commands.try_recv().unwrap(),
            ClockCommand::Graph(_, GraphCommand::Step)
        ));
        debug_tx
            .send(EmbeddedEventDebugState {
                debug: EditorGraphDebug {
                    revision: 0,
                    graph: EditorEventGraph::default(),
                    pending: 0,
                    pending_nodes: vec![],
                    breakpoint_node: None,
                    trace: vec![],
                    paused: false,
                },
                ack: Some(EmbeddedEventAck {
                    id,
                    result: Err("Pause before stepping".into()),
                }),
                room_ack: None,
                scene_ack: None,
                exterior_ack: None,
            })
            .unwrap();
        client.poll();
        assert!(client
            .graph_debug()
            .unwrap()
            .ack
            .as_ref()
            .unwrap()
            .result
            .is_err());
        assert_eq!(client.status(), &EmbeddedStatus::Running);
        assert!(client.graph_clear().is_ok());
        // A queued clock command fills the bounded mailbox; graph commands fail explicitly.
        client.graph_pending = None;
        assert!(client.graph_trigger(1).is_err());
        client.state = EmbeddedStatus::Stopped;
        assert!(client.graph_clear().is_err());
    }
    #[test]
    fn debug_view_changes_only_camera_and_logs_bounded_ordered_transitions() {
        let (mut client, requests) = debug_fixture();
        let authority = serde_json::to_value(client.snapshot()).unwrap();
        for i in 0..40 {
            client
                .set_debug_view(if i % 2 == 0 {
                    EmbeddedView::External
                } else {
                    EmbeddedView::Tactical3d
                })
                .unwrap();
        }
        assert_eq!(serde_json::to_value(client.snapshot()).unwrap(), authority);
        assert!(requests.try_recv().is_err());
        let state = client.debug_view_state();
        assert_eq!(state.current, Some(EmbeddedView::Tactical3d));
        assert_eq!(state.transitions.len(), 32);
        assert!(state
            .transitions
            .windows(2)
            .all(|w| w[0].sequence < w[1].sequence && w[0].observed_at_ms <= w[1].observed_at_ms));
        assert!(state
            .transitions
            .iter()
            .all(|e| e.simulation_time_ms == Some(1000)));
        assert_eq!(state.viewed_pilot, "pilot");
        client.cockpit.view_mode = ViewMode::Cockpit;
        client.observe_view("In-game view control changed");
        assert_eq!(
            client.debug_view_state().transitions.last().unwrap().to,
            Some(EmbeddedView::Cockpit)
        );
    }
    #[test]
    fn debug_view_rejects_embodiment_changes_and_disconnected_controls() {
        let (mut client, requests) = debug_fixture();
        client.cockpit.snapshot.as_mut().unwrap().pilots[0].location = Location::Docked;
        assert_eq!(client.scene_state(), Some(EditorSceneState::Cockpit));
        assert_eq!(
            client.debug_view_state().available,
            vec![
                EmbeddedView::Cockpit,
                EmbeddedView::External,
                EmbeddedView::Tactical,
                EmbeddedView::Tactical3d
            ]
        );
        client.set_debug_view(EmbeddedView::Cockpit).unwrap();
        assert!(client.set_debug_view(EmbeddedView::Hangar).is_err());
        assert!(client.set_debug_view(EmbeddedView::FirstPerson).is_err());
        assert!(client.snapshot().unwrap().pilots[0]
            .station_avatar
            .is_none());
        // The standalone game keeps its docked hangar presentation; the editor
        // camera exception grants no avatar or gameplay mutation.
        client.cockpit.embedded = false;
        assert_eq!(
            client.debug_view_state().available,
            vec![EmbeddedView::Hangar]
        );
        assert!(client.set_debug_view(EmbeddedView::Cockpit).is_err());
        client.cockpit.embedded = true;
        client.cockpit.snapshot.as_mut().unwrap().pilots[0].station_avatar = Some(StationAvatar {
            position: arkline_protocol::PIONEER_LIFT_POINT,
            heading: 0.0,
            moving: false,
            updated_at_ms: 1000,
            onboard_interior: true,
            interior_deck: 2,
            seated: false,
            jump_started_ms: None,
        });
        assert_eq!(client.scene_state(), Some(EditorSceneState::InteriorDeck2));
        assert_eq!(
            client.debug_view_state().available,
            vec![EmbeddedView::FirstPerson, EmbeddedView::ThirdPerson]
        );
        client.set_debug_view(EmbeddedView::FirstPerson).unwrap();
        assert_eq!(
            client.cockpit.hangar_view.on_foot_view,
            flight3d::hangar::OnFootView::FirstPerson
        );
        assert!(client.debug_view_state().scene.contains("deck 2"));
        assert!(client.set_debug_view(EmbeddedView::External).is_err());
        client.set_debug_view(EmbeddedView::ThirdPerson).unwrap();
        client.cockpit.connected = false;
        assert!(client.debug_view_state().available.is_empty());
        assert!(client.set_debug_view(EmbeddedView::FirstPerson).is_err());
        assert!(requests.try_recv().is_err());
    }
    #[test]
    fn hidden_poll_drains_state_without_paint_or_input() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        let snapshot = cockpit.snapshot.take().unwrap();
        let expected_time = snapshot.server_time_ms;
        cockpit.embedded = true;
        let (updates, receiver) = mpsc::channel();
        cockpit.updates = Some(receiver);
        let mut response = Response::error("");
        response.ok = true;
        response.snapshot = Some(snapshot);
        updates
            .send(Update::Response(Box::new(response), false))
            .unwrap();
        let (clock, _commands) = mpsc::sync_channel(16);
        let (states, receiver) = mpsc::channel();
        states.send(EmbeddedStatus::Paused).unwrap();
        let mut client = EmbeddedClient {
            project_data_dir: PathBuf::new(),
            data_dir: PathBuf::new(),
            realm: EmbeddedRealm::Project,
            mode: EmbeddedSessionMode::Local,
            session_info: None,
            session_updates: mpsc::channel().1,
            worker: None,
            cockpit,
            clock,
            graph_states: mpsc::sync_channel(1).1,
            graph_state: None,
            graph_pending: None,
            graph_sequence: 0,
            room_pending: None,
            room_ack: None,
            room_sequence: 0,
            scene_pending: None,
            scene_ack: None,
            scene_sequence: 0,
            exterior_pending: None,
            exterior_ack: None,
            exterior_sequence: 0,
            session_generation: 0,
            states: receiver,
            state: EmbeddedStatus::Loading,
            destroyed: false,
            views: ViewDiagnostics {
                started: Some(Instant::now()),
                ..Default::default()
            },
            free_camera: None,
            free_camera_enabled: false,
            free_camera_scene_hangar: None,
            free_camera_saved_view: None,
            free_camera_input_time: None,
        };
        client.cockpit.frame_intervals.push_back(1.245);
        client.cockpit.frame_observed = Instant::now() - Duration::from_secs(30);
        let output = ctx.run(Default::default(), |_| client.poll_hidden());
        assert!(client.cockpit.frame_intervals.is_empty());
        assert!(client.cockpit.frame_observed.elapsed() < Duration::from_secs(1));
        assert_eq!(client.status(), &EmbeddedStatus::Paused);
        assert_eq!(client.snapshot().unwrap().server_time_ms, expected_time);
        assert_eq!(client.cockpit.frozen_time, Some(expected_time));
        assert!(!client.cockpit.embedded_input);
        assert!(output.shapes.is_empty());
        assert!(output
            .viewport_output
            .values()
            .all(|v| v.commands.is_empty()));
    }
    #[test]
    fn embedded_frame_leaves_editor_panels_and_window_ownership_intact() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.embedded = true;
        cockpit.embedded_input = false;
        cockpit.focus_on_first_frame = false;
        cockpit.view_mode = ViewMode::Tactical;
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1400., 900.),
                )),
                events: vec![egui::Event::Key {
                    key: egui::Key::F11,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            },
            |ctx| {
                egui::TopBottomPanel::top("editor_toolbar")
                    .exact_height(60.)
                    .show(ctx, |ui| {
                        ui.label("Editor");
                    });
                egui::SidePanel::left("editor_inspector")
                    .exact_width(260.)
                    .show(ctx, |ui| {
                        ui.label("Inspector");
                    });
                let available = ctx.available_rect();
                assert!(available.left() >= 260. && available.top() >= 60.);
                cockpit.update(ctx, &mut eframe::Frame::_new_kittest());
            },
        );
        assert!(output
            .viewport_output
            .values()
            .all(|v| v.commands.is_empty()));
        assert!(rx.try_recv().is_err());
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(t) if t.galley.text().contains("Fullscreen") || t.galley.text() == "Open second cockpit")));
    }
    #[test]
    fn hangar_access_anchor_and_paint_stay_above_editor_console() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.embedded = true;
        let pilot = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        pilot.location = Location::Docked;
        pilot.station_avatar = None;
        for frame in 0..3 {
            let mut viewport = egui::Rect::NOTHING;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(1224., 768.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::TopBottomPanel::top("toolbar")
                        .exact_height(54.)
                        .show(ctx, |_| {});
                    egui::TopBottomPanel::bottom("console")
                        .exact_height(67.)
                        .show(ctx, |_| {});
                    egui::SidePanel::left("outliner")
                        .exact_width(205.)
                        .show(ctx, |_| {});
                    egui::SidePanel::right("inspector")
                        .exact_width(290.)
                        .show(ctx, |_| {});
                    viewport = ctx.available_rect();
                    cockpit.station_overlay(ctx);
                    let game = ctx.available_rect();
                    assert!(viewport.bottom() - game.bottom() <= 148.);
                    assert!(game.height() >= viewport.height() - 148.);
                },
            );
            let mut found = false;
            for clipped in output.shapes {
                if let egui::Shape::Text(t) = clipped.shape {
                    if t.galley.text() == "Leave ship" {
                        found = true;
                        assert!(
                            viewport.contains_rect(clipped.clip_rect),
                            "{:?} outside {:?}",
                            clipped.clip_rect,
                            viewport
                        );
                        assert!(
                            viewport.contains(t.pos),
                            "button outside viewport: {:?}",
                            t.pos
                        );
                    }
                }
            }
            if frame > 0 {
                assert!(found);
            }
        }
    }
    #[test]
    fn worker_pauses_steps_restarts_and_preserves_authenticated_pilot() {
        let dir = std::env::temp_dir().join(format!(
            "arkline-embedded-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (clock, rx) = mpsc::channel();
        let (requests, req_rx) = mpsc::channel();
        let (updates, out) = mpsc::sync_channel(64);
        let (states, status) = mpsc::channel();
        let (graph_tx, graph_rx) = mpsc::sync_channel(1);
        let worker_dir = dir.clone();
        let worker = std::thread::spawn(move || {
            run(
                worker_dir,
                "EditorPilot".into(),
                rx,
                req_rx,
                updates,
                Default::default(),
                (states, graph_tx),
            )
        });
        assert_eq!(
            status.recv_timeout(Duration::from_secs(5)).unwrap(),
            EmbeddedStatus::Paused
        );
        let snapshot = |out: &Receiver<Update>| loop {
            if let Update::Response(response, _) = out.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                if let Some(snapshot) = response.snapshot {
                    break snapshot;
                }
            }
        };
        let first = snapshot(&out);
        assert!(
            matches!(
                out.recv_timeout(Duration::from_millis(180)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "paused idle world must not rebuild snapshots at 20Hz"
        );
        let graph_ack = |id| loop {
            let state = graph_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            if state.ack.as_ref().is_some_and(|a| a.id == id) {
                break state;
            }
        };
        let graph = EditorEventGraph {
            nodes: vec![arkline_protocol::EditorGraphNode {
                id: 1,
                label: "Manual test".into(),
                position: [0., 0.],
                operation: arkline_protocol::EditorNodeOp::Manual,
                breakpoint: false,
            }],
            ..Default::default()
        };
        clock
            .send(ClockCommand::Graph(
                1,
                GraphCommand::Replace {
                    revision: 0,
                    graph,
                    clear_pending: false,
                },
            ))
            .unwrap();
        assert!(graph_ack(1).ack.unwrap().result.is_ok());
        clock
            .send(ClockCommand::Graph(2, GraphCommand::Trigger(1)))
            .unwrap();
        let queued = graph_ack(2);
        assert_eq!(queued.debug.pending, 1);
        assert!(queued.debug.paused);
        clock
            .send(ClockCommand::Graph(3, GraphCommand::Step))
            .unwrap();
        let stepped = graph_ack(3);
        assert!(stepped.ack.unwrap().result.is_ok());
        assert_eq!(stepped.debug.pending, 0);
        assert!(stepped
            .debug
            .trace
            .iter()
            .all(|e| e.simulation_time_ms == first.server_time_ms));
        clock
            .send(ClockCommand::Graph(
                4,
                GraphCommand::Replace {
                    revision: 0,
                    graph: Default::default(),
                    clear_pending: false,
                },
            ))
            .unwrap();
        assert!(graph_ack(4).ack.unwrap().result.is_err());
        let graph = EditorEventGraph {
            nodes: vec![arkline_protocol::EditorGraphNode {
                id: 1,
                label: "Breakpoint test".into(),
                position: [0., 0.],
                operation: arkline_protocol::EditorNodeOp::Manual,
                breakpoint: true,
            }],
            ..Default::default()
        };
        clock
            .send(ClockCommand::Graph(
                5,
                GraphCommand::Replace {
                    revision: 1,
                    graph,
                    clear_pending: false,
                },
            ))
            .unwrap();
        assert!(graph_ack(5).ack.unwrap().result.is_ok());
        clock
            .send(ClockCommand::Graph(6, GraphCommand::Trigger(1)))
            .unwrap();
        assert!(graph_ack(6).ack.unwrap().result.is_ok());
        clock.send(ClockCommand::Play).unwrap();
        loop {
            let debug = graph_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            if debug.debug.breakpoint_node == Some(1) {
                assert!(debug.debug.paused);
                break;
            }
        }
        clock
            .send(ClockCommand::Graph(7, GraphCommand::Clear))
            .unwrap();
        assert!(graph_ack(7).ack.unwrap().result.is_ok());
        // Consume graph-related snapshots before the existing ship/clock journey.
        out.try_iter().for_each(drop);
        requests.send(Request::Undock).unwrap();
        loop {
            if let Update::Response(response, true) =
                out.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                assert!(!response.ok);
                assert!(response.message.contains("paused"));
                break;
            }
        }
        assert_eq!(snapshot(&out).server_time_ms, first.server_time_ms);
        clock.send(ClockCommand::Step).unwrap();
        loop {
            let current = snapshot(&out);
            if current.server_time_ms > first.server_time_ms {
                assert_eq!(current.server_time_ms, first.server_time_ms + 50);
                break;
            }
        }
        clock.send(ClockCommand::Play).unwrap();
        // Queue before polling any new state: invalid Step must not kill authority.
        clock.send(ClockCommand::Step).unwrap();
        clock.send(ClockCommand::Pause).unwrap();
        assert_eq!(
            status.recv_timeout(Duration::from_secs(5)).unwrap(),
            EmbeddedStatus::Running
        );
        assert_eq!(
            status.recv_timeout(Duration::from_secs(5)).unwrap(),
            EmbeddedStatus::Paused
        );
        loop {
            if let Update::Response(response, false) =
                out.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                if !response.ok {
                    assert!(response.message.contains("pause"));
                    break;
                }
            }
        }
        clock.send(ClockCommand::Play).unwrap();
        assert_eq!(
            status.recv_timeout(Duration::from_secs(5)).unwrap(),
            EmbeddedStatus::Running
        );
        requests.send(Request::Undock).unwrap();
        loop {
            if let Update::Response(response, true) =
                out.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                assert!(response.ok, "{}", response.message);
                break;
            }
        }
        clock.send(ClockCommand::Stop).unwrap();
        assert_eq!(
            status.recv_timeout(Duration::from_secs(5)).unwrap(),
            EmbeddedStatus::Stopped
        );
        clock.send(ClockCommand::Play).unwrap();
        assert_eq!(
            status.recv_timeout(Duration::from_secs(5)).unwrap(),
            EmbeddedStatus::Running
        );
        assert_eq!(snapshot(&out).pilots.len(), 1);
        clock.send(ClockCommand::Shutdown).unwrap();
        worker.join().unwrap().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
