#[cfg(not(target_arch = "wasm32"))]
pub use flight3d::asset_gallery::EmbeddedAssetGallery;
#[cfg(not(target_arch = "wasm32"))]
pub use flight3d::drone_gallery::EmbeddedDroneGallery;
#[cfg(not(target_arch = "wasm32"))]
pub use flight3d::imported_asset::ImportedAssetPreview;
pub use flight3d::imported_asset::{ImportReport, ImportedGeometry};
#[cfg(not(target_arch = "wasm32"))]
pub use flight3d::model_catalog::{
    load_runtime_model, runtime_model_catalog, runtime_model_context_id, ModelGeometryInfo,
    RuntimeInspectionModel, RuntimeModelDescriptor,
};
#[cfg(not(target_arch = "wasm32"))]
pub use flight3d::model_preview::ModelPreview;
#[cfg(not(target_arch = "wasm32"))]
pub use flight3d::{DebugRenderMetrics, ViewRenderMetrics, ViewVisibility};
mod material_preview;
pub use material_preview::EmbeddedMaterialPreview;
mod material_program;
mod spatial_material;
pub use spatial_material::{
    MaterialProgramDiagnostic, MaterialProgramErrorPhase, MaterialProgramPhase,
    MaterialProgramStatus, MaterialRevision, MaterialStamp,
};
#[cfg(not(target_arch = "wasm32"))]
mod embedded;
#[cfg(not(target_arch = "wasm32"))]
pub use embedded::{
    EmbeddedClient, EmbeddedEventAck, EmbeddedEventDebugState, EmbeddedRealm, EmbeddedSessionInfo,
    EmbeddedSessionMode, EmbeddedStatus, EmbeddedView, EmbeddedViewState, ViewTransition,
};
// Native Arkline cockpit. Live world data is server-authoritative; ship layouts are labeled design references.
#[cfg(not(target_arch = "wasm32"))]
mod account_login;
#[cfg(not(target_arch = "wasm32"))]
mod audit;
mod boot;
#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(test)]
mod cockpit_computer_integration_tests;
mod cockpit_panels;
mod cockpit_shell;
mod drone_telemetry;
#[cfg(test)]
mod drone_telemetry_tests;
mod effects;
mod flight3d;
mod layout;
pub use layout::panel_geometry as cockpit_panel_geometry;
mod camera_sources;
mod captain_displays;
mod carrier_camera_geometry;
// Retain reviewed source helper APIs, including navigation-position compatibility;
// navigation itself consumes the authoritative protocol definition, not relocation.
mod carrier_inventory_ui;
mod carrier_mounted_feed;
#[allow(dead_code)]
mod carrier_thruster_geometry;
mod carrier_warning_adapter;
mod carrier_warning_geometry;
mod control_bindings;
mod controls_reference;
mod exchange_readiness;
mod first_person_hud;
mod flight_workspace;
mod lighting_settings;
mod motion;
mod nearby_action_model;
mod nearby_interaction_input;
mod operations_workspace;
mod overview_contacts;
#[allow(dead_code)]
mod parallel_thruster_geometry;
mod parallel_turret_controls;
mod performance_overlay;
#[cfg(not(target_arch = "wasm32"))]
mod physical_cargo_session;
#[cfg(not(target_arch = "wasm32"))]
pub use physical_cargo_session::{
    run_physical_bay_preview, PhysicalBayClient, PhysicalBayClientOptions,
};
mod physical_cargo_ui;
mod pilot_input;
mod power_accounting;
#[cfg(test)]
mod presentation_clock;
mod remote_control_ui;
mod resource_jobs_ui;
mod room_controls;
mod sandbox;
mod services;
mod session;
#[cfg(not(target_arch = "wasm32"))]
mod ship_ai;
#[cfg(target_arch = "wasm32")]
#[path = "browser_ship_ai.rs"]
mod ship_ai;
mod ship_layouts;
mod sound_engine;
mod sound_scene;
mod sound_settings;
mod station_ui;
mod target_controls;
mod theme;
use session::*;
mod transport;
#[cfg(test)]
mod transport_tests;
#[cfg(not(target_arch = "wasm32"))]
mod voice;
#[cfg(target_arch = "wasm32")]
#[path = "browser_voice.rs"]
mod voice;
use arkline_protocol::*;
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

#[cfg(not(target_arch = "wasm32"))]
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::{TcpStream, ToSocketAddrs},
    process::{Child, Command, Stdio},
};
use web_time::Instant;

fn is_fullscreen(ctx: &egui::Context) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        web_sys::window()
            .and_then(|w| w.document())
            .is_some_and(|d| d.fullscreen_element().is_some())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        ctx.input(|i| i.viewport().fullscreen.unwrap_or(false))
    }
}
fn toggle_fullscreen(ctx: &egui::Context) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(document) = web_sys::window().and_then(|w| w.document()) {
            if is_fullscreen(ctx) {
                document.exit_fullscreen();
            } else if let Some(canvas) = document.get_element_by_id("arkline_canvas") {
                let _ = canvas.request_fullscreen();
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!is_fullscreen(ctx)));
    }
}

const TEAL: Color32 = Color32::from_rgb(70, 235, 133);
const AMBER: Color32 = Color32::from_rgb(255, 192, 104);
const MUTED: Color32 = Color32::from_rgb(138, 157, 180);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewMode {
    Tactical,
    Tactical3d,
    External,
    Cockpit,
}

enum Update {
    Ready,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Reconnecting(String),
    Response(Box<Response>, bool),
    #[cfg(not(target_arch = "wasm32"))]
    PhysicalResponse(
        Box<Response>,
        bool,
        Instant,
        Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ),
    Failure(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ServiceTab {
    Layouts,
    Navigation,
    Station,
    Corporation,
    Hangar,
    Cargo,
    Exchange,
    Activity,
    Ledger,
    Settings,
    Modules,
    Drones,
    Sandbox,
    ShipComputer,
    Displays,
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum SelectedObject {
    Planet(u64),
    Prop(u64),
    Asteroid(u64),
    Gate(u64),
    Station,
    Belt,
    Pilot(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OverviewFilter {
    All,
    Mining,
    Travel,
    Pilots,
}

struct Cockpit {
    embedded: bool,
    embedded_paused: bool,
    embedded_input: bool,
    workspace_input_waiting_release: bool,
    workspace_keyboard_owned_last_frame: bool,
    operations_search: String,
    controls_reference_open: bool,
    identity_collapsed: bool,
    workspace_identity_override: Option<bool>,
    performance_open: bool,
    performance_snapshot_received: Option<Instant>,
    drone_telemetry_time: drone_telemetry::TelemetryTime,
    performance_server_tick: Option<(u64, u64)>,
    nearby_geometry: nearby_action_model::NearbyGeometryCache,
    nearby_shortcut: nearby_interaction_input::NearbyGesture,
    ship_observation: bool,
    logo: egui::TextureHandle,
    boot: boot::Boot,
    server: String,
    pilot: String,
    data_dir: PathBuf,
    credential_scope: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    account_login: account_login::Login,
    #[cfg(not(target_arch = "wasm32"))]
    link_existing_account: bool,
    #[cfg(not(target_arch = "wasm32"))]
    oauth_config_path: Option<PathBuf>,
    commands: Option<Sender<Request>>,
    controls: pilot_input::Mailbox,
    pilot_input: pilot_input::PilotInput,
    updates: Option<Receiver<Update>>,
    connecting: bool,
    connected: bool,
    snapshot: Option<Snapshot>,
    physical_cargo: Option<arkline_protocol::physical_cargo::control::PhysicalControlPublicView>,
    physical_panel_open: bool,
    physical_session: bool,
    #[cfg(not(target_arch = "wasm32"))]
    physical_stop: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    assets: Option<AssetsView>,
    layout: layout::Layout,
    frame_observed: Instant,
    frame_intervals: VecDeque<f64>,
    transfer_amount: u64,
    corporation_name: String,
    invite_pilot: String,
    observed: Instant,
    feed: VecDeque<(String, bool)>,
    price: u64,
    buy_amount: u64,
    notice: String,
    view_mode: ViewMode,
    tactical_system: bool,
    tactical_pan: [f64; 2],
    tactical_zoom: f64,
    tactical_drag: bool,
    motion: motion::MotionHistory,
    timeline_origin: Option<u64>,
    flight_view: flight3d::FlightView,
    cockpit_computer: flight3d::cockpit_computer::CockpitComputer,
    cockpit_console_detail: bool,
    #[cfg(test)]
    computer_ui_labels: Vec<(String, Pos2)>,
    hangar_view: flight3d::hangar::HangarView,
    station_walk_active: bool,
    docked_walk_binding: Option<DockedTraversalBinding>,
    frozen_time: Option<u64>,
    focus_on_first_frame: bool,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    local_server: Option<LocalServer>,
    service_tab: ServiceTab,
    captain_screen_slot: usize,
    ship_layout: ship_layouts::LayoutViewer,
    selected_object: SelectedObject,
    mining_target: Option<u64>,
    camera_source: camera_sources::CameraSource,
    drone_camera: bool,
    bay_monitor_doorway: bool,
    selected_drone: Option<u64>,
    drone_camera_mode: flight3d::DroneCameraMode,
    drone_split: bool,
    drone_attack_target: Option<(String, u64)>,
    prop_kind: SurfacePropKind,
    prop_offset: [f64; 3],
    resource_goal: u64,
    resource_pilot: Option<String>,
    fleet_controller: String,
    remote_ship: Option<String>,
    manual_remote: Option<String>,
    ship_ai: ship_ai::ShipAi,
    voice: voice::Voice,
    voice_pin: Option<(String, u64)>,
    voice_transcript: String,
    spoken_replies: bool,
    spoken_job: Option<(u64, arkline_protocol::ResourceJobPhase)>,
    ai_prompt: String,
    ai_message: String,
    ai_pin: Option<(String, u64)>,
    connection_generation: u64,
    sell_amount: u64,
    audit_updates: Option<Receiver<Result<String, String>>>,
    audit_result: Option<Result<String, String>>,
    request_pending: bool,
    effects: effects::Effects,
    sound: sound_engine::SoundEngine,
    sound_scene: sound_scene::Scene,
    sound_settings: sound_settings::SoundSettings,
    sound_test_notice: String,
    theme: theme::Theme,
    settings_notice: String,
    navigation_range: f64,
    context_fly_to: Option<[f64; 3]>,
    overview_filter: OverviewFilter,
    services_open: bool,
    engineering_panel: Option<String>,
    overview_nearest_first: bool,
    overview_query: String,
    overview_open: bool,
}
impl Cockpit {
    fn new(
        cc: &eframe::CreationContext<'_>,
        server: String,
        data_dir: PathBuf,
        credential_scope: Option<String>,
    ) -> Self {
        let sound_settings = sound_settings::SoundSettings::load(&data_dir);
        let theme = theme::Theme::load(&data_dir);
        let layout = layout::Layout::load(&data_dir);
        let lighting_quality = lighting_settings::LightingQuality::load(&data_dir);
        theme.apply(&cc.egui_ctx);
        let logo_image = image::load_from_memory_with_format(
            include_bytes!("../../../assets/Arkline-256.png"),
            image::ImageFormat::Png,
        )
        .expect("bundled official Arkline logo")
        .into_rgba8();
        let logo = cc.egui_ctx.load_texture(
            "official-arkline-logo",
            egui::ColorImage::from_rgba_unmultiplied(
                [logo_image.width() as usize, logo_image.height() as usize],
                logo_image.as_raw(),
            ),
            egui::TextureOptions::LINEAR,
        );

        Self {
            embedded: false,
            embedded_paused: false,
            embedded_input: true,
            workspace_input_waiting_release: false,
            workspace_keyboard_owned_last_frame: false,
            operations_search: String::new(),
            controls_reference_open: false,
            identity_collapsed: false,
            workspace_identity_override: None,
            performance_open: false,
            performance_snapshot_received: None,
            drone_telemetry_time: Default::default(),
            performance_server_tick: None,
            nearby_geometry: nearby_action_model::NearbyGeometryCache::default(),
            nearby_shortcut: nearby_interaction_input::NearbyGesture::default(),
            ship_observation: false,
            logo,
            #[cfg(not(target_arch = "wasm32"))]
            account_login: account_login::Login::default(),
            #[cfg(not(target_arch = "wasm32"))]
            link_existing_account: false,
            #[cfg(not(target_arch = "wasm32"))]
            oauth_config_path: None,
            boot: boot::Boot::default(),
            server,
            pilot: String::new(),
            data_dir,
            credential_scope,
            commands: None,
            controls: pilot_input::Mailbox::default(),
            pilot_input: pilot_input::PilotInput::default(),
            updates: None,
            connecting: false,
            connected: false,
            snapshot: None,
            physical_cargo: None,
            physical_panel_open: true,
            physical_session: false,
            #[cfg(not(target_arch = "wasm32"))]
            physical_stop: None,
            assets: None,
            layout,
            frame_observed: Instant::now(),
            frame_intervals: VecDeque::new(),
            transfer_amount: 25,
            corporation_name: String::new(),
            invite_pilot: String::new(),
            observed: Instant::now(),
            feed: VecDeque::new(),
            price: 10,
            buy_amount: 1,
            notice: String::new(),
            view_mode: ViewMode::Cockpit,
            tactical_system: false,
            tactical_pan: [0.0; 2],
            tactical_zoom: 1.0,
            tactical_drag: false,
            motion: motion::MotionHistory::default(),
            timeline_origin: None,
            flight_view: {
                let mut view = flight3d::FlightView::new(cc.gl.as_ref());
                view.lighting_quality = lighting_quality;
                view
            },
            cockpit_computer: Default::default(),
            cockpit_console_detail: false,
            #[cfg(test)]
            computer_ui_labels: Vec::new(),
            hangar_view: flight3d::hangar::HangarView::new(cc.gl.as_ref()),
            station_walk_active: false,
            docked_walk_binding: None,
            frozen_time: None,
            focus_on_first_frame: true,
            local_server: None,
            service_tab: ServiceTab::Modules,
            captain_screen_slot: 0,
            ship_layout: ship_layouts::LayoutViewer::default(),
            selected_object: SelectedObject::Station,
            mining_target: None,
            camera_source: Default::default(),
            drone_camera: true,
            bay_monitor_doorway: false,
            selected_drone: None,
            drone_camera_mode: flight3d::DroneCameraMode::Raised,
            drone_split: false,
            drone_attack_target: None,
            prop_kind: SurfacePropKind::Crate,
            prop_offset: [20.0, 0.0, 0.0],
            resource_goal: cargo_capacity(ShipClass::Pioneer),
            resource_pilot: None,
            fleet_controller: String::new(),
            remote_ship: None,
            manual_remote: None,
            ship_ai: ship_ai::ShipAi::default(),
            voice: voice::Voice::default(),
            voice_pin: None,
            voice_transcript: String::new(),
            spoken_replies: false,
            spoken_job: None,
            ai_prompt: String::new(),
            ai_message: String::new(),
            ai_pin: None,
            connection_generation: 0,
            sell_amount: 25,
            audit_updates: None,
            audit_result: None,
            request_pending: false,
            effects: effects::Effects::default(),
            sound: sound_engine::SoundEngine::default(),
            sound_scene: sound_scene::Scene::default(),
            sound_settings,
            sound_test_notice: String::new(),
            theme,
            settings_notice: String::new(),
            navigation_range: 500.0,
            context_fly_to: None,
            overview_filter: OverviewFilter::All,
            services_open: false,
            engineering_panel: None,
            overview_nearest_first: true,
            overview_query: String::new(),
            overview_open: true,
        }
    }
    fn accent(&self) -> Color32 {
        self.theme.accent()
    }
    fn log(&mut self, text: String, error: bool) {
        self.feed.push_front((text, error));
        self.feed.truncate(60);
    }
    fn preferred_sound_mix(&self, mut mix: sound_engine::Mix) -> sound_engine::Mix {
        mix.master = self.sound_settings.master;
        mix.ambience = self.sound_settings.ambience;
        mix.effects = self.sound_settings.effects;
        mix.muted = self.sound_settings.muted || self.voice.is_listening();
        mix.ducked = self.voice.is_speaking();
        mix
    }
    fn start_audio_output(&mut self) {
        self.sound
            .set_mix(self.preferred_sound_mix(sound_engine::Mix::default()));
        self.sound.start();
    }
    fn update_sound(&mut self) {
        let now = self.timeline_origin.map_or(0, |t| {
            t.saturating_add(self.observed.elapsed().as_millis() as u64)
        });
        let name = self.controlled_name().to_owned();
        let listener = if self.own().is_some_and(|p| p.station_avatar.is_some()) {
            self.hangar_view.audio_listener(&name, now)
        } else {
            self.flight_view.audio_listener(&name, now)
        };
        let (mut mix, cues) = self.sound_scene.update_spatial(
            self.snapshot.as_ref(),
            &name,
            self.connected,
            now,
            listener,
            self.embedded && self.embedded_paused,
        );
        let audible = !self.embedded || !self.embedded_paused;
        if !audible {
            mix.active = false;
            mix.engine = 0.;
            mix.mining = 0.;
            mix.warp = 0.;
        }
        self.sound.set_mix(self.preferred_sound_mix(mix));
        if audible {
            for cue in cues {
                self.sound.cue(cue);
            }
        }
    }
    fn presentation_time(&self) -> u64 {
        self.frozen_time.unwrap_or_else(|| {
            self.timeline_origin.map_or(0, |origin| {
                origin.saturating_add(self.observed.elapsed().as_millis() as u64)
            })
        })
    }
    fn connect(&mut self) {
        if self.physical_session {
            self.notice = "Physical preview uses its explicit local owner session; reopen that preview to reconnect.".into();
            return;
        }
        if self.embedded {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.account_login.configured(&self.server)
            && !self
                .account_login
                .credential
                .as_ref()
                .is_some_and(|a| a.matches(&self.server, self.pilot.trim()))
        {
            self.notice = match self.account_login.begin(&self.server, self.pilot.trim()) {
                Ok(()) => "Waiting for browser sign-in...".into(),
                Err(error) => error,
            };
            self.boot.visible = false;
            return;
        }
        #[cfg(target_arch = "wasm32")]
        self.start_audio_output();
        self.sound_scene.reset();
        self.cockpit_computer.cancel_input();
        self.flight_view.invalidate_bay_monitor_session();
        self.flight_view.clear_cockpit_presentation();
        self.invalidate_ai_session();
        self.resource_pilot = None;
        self.remote_ship = None;
        self.manual_remote = None;
        self.boot.connect();
        self.connected = false;
        self.controls = pilot_input::Mailbox::default();
        self.station_walk_active = false;
        self.docked_walk_binding = None;
        self.pilot_input = pilot_input::PilotInput::default();
        self.motion.clear();
        self.timeline_origin = None;
        self.effects.clear();
        self.feed.clear();
        self.selected_object = SelectedObject::Station;
        self.mining_target = None;
        self.selected_drone = None;
        self.drone_attack_target = None;
        self.audit_result = None;
        self.audit_updates = None;
        self.request_pending = false;
        self.frozen_time = None;
        self.pilot = self.pilot.trim().to_string();
        self.snapshot = None;
        self.assets = None;
        self.ship_observation = false;
        self.corporation_name.clear();
        self.invite_pilot.clear();
        let (tx, rx) = mpsc::channel();
        let (out_tx, out_rx) = mpsc::channel();
        let server = self.server.clone();
        let pilot = self.pilot.clone();
        let dir = self.data_dir.clone();
        let scope = self.credential_scope.clone();
        let controls = self.controls.clone();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let identity = WorkerIdentity {
                scope,
                // Preserve account mode across expiry races; the worker rejects expired
                // credentials rather than silently issuing a legacy Connect.
                account: if self.account_login.configured(&server) {
                    self.account_login.credential.clone()
                } else {
                    None
                },
                link_existing: self.link_existing_account,
            };
            std::thread::spawn(move || worker(server, pilot, dir, identity, rx, out_tx, controls));
        }
        #[cfg(target_arch = "wasm32")]
        browser::worker(server, pilot, dir, scope, rx, out_tx, controls);
        self.commands = Some(tx);
        self.updates = Some(out_rx);
        self.connecting = true;
        self.notice = "Connecting…".into();
    }
    #[cfg(target_arch = "wasm32")]
    fn second_cockpit(&mut self) {
        if let Some(window) = web_sys::window() {
            let _ = window.open_with_url("?network=production");
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn second_cockpit(&mut self) {
        if self.physical_session {
            self.notice =
                "The selected physical preview has one local owner; no network cockpit is opened."
                    .into();
            return;
        }
        let result = std::env::current_exe().and_then(|exe| {
            let mut command = Command::new(exe);
            command
                .args(["--remote-only", "--server", &self.server, "--data-dir"])
                .arg(&self.data_dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if let Some(path) = &self.oauth_config_path {
                command.arg("--oauth-config").arg(path);
            }
            if let Some(scope) = &self.credential_scope {
                command.args(["--credential-scope", scope]);
            }
            command.spawn()
        });
        match result {
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(error) => self.log(format!("Could not open second cockpit: {error}"), true),
        }
    }
    fn can_fly(&self) -> bool {
        !self.physical_session
            && self.physical_cargo.is_none()
            && self.can_operate()
            && self.own().is_some_and(|p| {
                p.station_avatar.is_none()
                    && p.flight.is_some()
                    && p.flight
                        .as_ref()
                        .and_then(|f| f.docking.as_ref())
                        .is_none_or(|d| !d.phase.busy())
                    && !matches!(
                        p.location,
                        Location::Docked | Location::InTransit { .. } | Location::Jumping { .. }
                    )
            })
    }
    /// Bay authority locks carrier motion, not the seated pilot's ability to
    /// STOP or control the identified handling operation.
    fn can_maneuver(&self) -> bool {
        self.can_fly() && self.bay_maneuver_reason().is_none()
    }
    fn warp_power_ready(&self) -> bool {
        self.own().is_none_or(|p| {
            p.power
                .as_ref()
                .is_none_or(|power| power.charge_j >= power_spec(p.ship_class).warp_cost_j)
        })
    }
    fn can_warp(&self) -> bool {
        self.can_fly() && self.warp_power_ready() && self.bay_departure_reason().is_none()
    }
    fn can_operate(&self) -> bool {
        self.connected
            && !self.request_pending
            && !self.embedded_paused
            && (!self.physical_session || self.physical_cargo.is_some())
    }
    fn send(&mut self, request: Request) {
        if !self.can_operate() {
            return;
        }
        if self.physical_cargo.is_some() && !self.physical_request_allowed(&request) {
            self.notice = "Selected physical bay: this control is not admitted".into();
            return;
        }
        if let Some(reason) = self.bay_command_reason(&request) {
            self.notice = reason.into();
            return;
        }
        // A known reservation must not relinquish input, cancel AI or mark a
        // pending command before the server can reject it. This is UX only;
        // unknown/foreign receipts and all races remain server-authoritative.
        if matches!(request, Request::Undock) && self.cargo_departure_reason().is_some() {
            self.notice = self.departure_order_reason().unwrap().into();
            return;
        }
        if self.own().is_some_and(|p| p.station_avatar.is_some())
            && (request.is_remote_gameplay()
                || matches!(
                    request,
                    Request::SelectShip { .. }
                        | Request::Mine { .. }
                        | Request::MineAsteroid { .. }
                        | Request::StartResourceJob { .. }
                        | Request::BeginRemoteControl { .. }
                        | Request::RemoteControl { .. }
                        | Request::SpawnSurfaceProp { .. }
                        | Request::MoveSurfaceProp { .. }
                        | Request::RemoveSurfaceProp { .. }
                ))
        {
            self.notice =
                "On foot: return through the airlock and board your ship to operate it.".into();
            return;
        }
        if matches!(request, Request::Warp { .. }) && !self.warp_power_ready() {
            self.notice = "Insufficient capacitor for warp. Wait for reactor recharge.".into();
            return;
        }
        if matches!(
            request,
            Request::Navigate { .. }
                | Request::SelectShip { .. }
                | Request::Warp { .. }
                | Request::Dock
                | Request::Undock
                | Request::DisembarkShip
                | Request::DockedTraversal { .. }
                | Request::EnterInterior
                | Request::ExitInterior
                | Request::InteriorLift { .. }
                | Request::StationSit { .. }
                | Request::BoardShip { .. }
                | Request::EnterShip
                | Request::Jump { .. }
                | Request::StartResourceJob { .. }
        ) {
            if matches!(request, Request::DockedTraversal { .. }) {
                self.stop_station_walk();
                let _ = self.pilot_input.update(false, Default::default());
                self.controls.clear_flight();
            } else {
                self.pilot_input.relinquish(&self.controls);
            }
            self.station_walk_active = false;
            self.docked_walk_binding = None;
        }
        if !matches!(
            request,
            Request::Snapshot | Request::ExportLedger | Request::SetCockpitScreenMode { .. }
        ) {
            self.cancel_ai();
        }
        let request = if let Some(pilot) = self.controlled_remote() {
            if request.is_remote_gameplay() {
                Request::RemoteControl {
                    pilot: pilot.to_owned(),
                    request: Box::new(request),
                }
            } else if matches!(
                request,
                Request::Snapshot
                    | Request::ExportLedger
                    | Request::BeginRemoteControl { .. }
                    | Request::EndRemoteControl
            ) {
                request
            } else {
                self.notice =
                    "Return to your own ship to use account, inventory or AI job actions.".into();
                return;
            }
        } else {
            request
        };
        if let Some(tx) = &self.commands {
            if tx.send(request).is_ok() {
                self.request_pending = true;
            } else {
                self.log(
                    "Command was not sent: connection worker unavailable.".into(),
                    true,
                );
            }
        }
    }
    fn compact_control_label(&self, fresh: bool) -> &'static str {
        if !fresh || !self.connected || self.own().is_none() {
            "NO CONFIRMED CONTROL"
        } else if self.ship_observation {
            "OBSERVATION ONLY"
        } else if self.own().is_some_and(|p| p.station_avatar.is_some()) {
            "ON FOOT / FLIGHT LOCKED"
        } else if self.manual_remote.is_some() && self.controlled_remote().is_none() {
            "REMOTE REQUEST / NO CONTROL"
        } else if self.bay_maneuver_reason().is_some() {
            "BAY INTERLOCK / MOTION LOCKED"
        } else if self.controlled_remote().is_some() {
            "CONFIRMED REMOTE CONTROL"
        } else if self.can_fly() {
            "LOCAL PILOT CONTROL"
        } else {
            "MANUAL FLIGHT LOCKED"
        }
    }
    fn poll(&mut self) {
        if let Some(result) = self
            .audit_updates
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.audit_result = Some(result);
            self.audit_updates = None;
        }
        let updates: Vec<_> = self
            .updates
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for update in updates {
            let produced_at: Option<Instant>;
            #[cfg(not(target_arch = "wasm32"))]
            let update = match update {
                Update::PhysicalResponse(response, visible, emitted, pending) => {
                    // Release only the presentation observation slot. Authority commands,
                    // transactions and their acknowledgements retain FIFO delivery.
                    if let Some(pending) = pending {
                        pending.store(false, std::sync::atomic::Ordering::Release);
                    }
                    produced_at = Some(emitted);
                    Update::Response(response, visible)
                }
                other => {
                    produced_at = None;
                    other
                }
            };
            #[cfg(target_arch = "wasm32")]
            {
                produced_at = None;
            }
            match update {
                #[cfg(not(target_arch = "wasm32"))]
                Update::PhysicalResponse(..) => unreachable!("normalized above"),
                Update::Ready => {
                    // A complete reconnect can be drained in one poll. Revoke
                    // old glass gestures at the boundary, not after the batch.
                    self.cockpit_computer.cancel_input();
                    self.flight_view.reset_activity_session();
                    self.hangar_view.reset_mechanisms();
                    self.invalidate_ai_session();
                    self.frozen_time = None;
                    self.timeline_origin = None;
                    self.motion.clear();
                    self.effects.clear();
                    self.snapshot = None;
                    self.physical_cargo = None;
                    self.flight_view.clear_physical();
                    self.connected = true;
                    self.connecting = false;
                    self.notice.clear();
                }
                Update::Reconnecting(message) => {
                    if self.physical_session {
                        self.snapshot = None;
                        self.motion.clear();
                    }
                    self.physical_cargo = None;
                    self.flight_view.clear_physical();
                    self.cockpit_computer.cancel_input();
                    self.flight_view.disconnect_activity();
                    self.hangar_view.disconnect_mechanisms();
                    self.invalidate_ai_session();
                    if self.frozen_time.is_none() {
                        self.frozen_time = Some(self.presentation_time());
                    }
                    self.effects.clear();
                    self.request_pending = false;
                    self.pilot_input.relinquish(&self.controls);
                    self.connected = false;
                    self.connecting = true;
                    self.notice = message.clone();
                    self.log(message, true);
                }
                Update::Failure(error) => {
                    if self.physical_session {
                        self.disconnect_session();
                    }
                    self.physical_cargo = None;
                    self.flight_view.clear_physical();
                    self.cockpit_computer.cancel_input();
                    self.flight_view.disconnect_activity();
                    self.hangar_view.disconnect_mechanisms();
                    let error = resume_guidance(error);
                    self.invalidate_ai_session();
                    if self.boot.visible {
                        self.boot.fail();
                    }

                    self.effects.clear();
                    self.request_pending = false;
                    self.frozen_time = Some(self.presentation_time());
                    self.connected = false;
                    self.connecting = false;
                    self.commands = None;
                    self.notice = error.clone();
                    self.log(error, true);
                    if self.physical_session {
                        #[cfg(not(target_arch = "wasm32"))]
                        if let Some(stop) = &self.physical_stop {
                            stop.store(true, std::sync::atomic::Ordering::Release);
                        }
                        self.updates = None;
                        break;
                    }
                }
                Update::Response(response, visible) => {
                    if let Some(assets) = response.assets {
                        self.assets = Some(assets);
                    }
                    if visible {
                        self.request_pending = false;
                    }
                    if let Some(snapshot) = response.snapshot {
                        if let Err(error) =
                            self.accept_physical_response(response.physical_cargo, &snapshot)
                        {
                            self.reject_world_layout(error);
                            if self.physical_session {
                                break;
                            }
                            continue;
                        }
                        if let Err(error) = validate_snapshot_scene(&snapshot, self.assets.as_ref())
                        {
                            self.reject_world_layout(error);
                            if self.physical_session {
                                break;
                            }
                            continue;
                        }
                        if self.connected && response.ok {
                            if let Some(previous) = &self.snapshot {
                                self.effects.observe(
                                    previous,
                                    &snapshot,
                                    &self.pilot,
                                    snapshot.server_time_ms,
                                );
                            }
                        }
                        let previously_docked =
                            self.own().is_some_and(|p| p.location == Location::Docked);
                        if !self.embedded
                            && !previously_docked
                            && snapshot
                                .world_layout
                                .as_ref()
                                .is_none_or(|l| l.revision != SceneLayoutRevision::DockingV1)
                            && snapshot
                                .pilots
                                .iter()
                                .any(|p| p.name == self.pilot && p.location == Location::Docked)
                        {
                            self.service_tab = ServiceTab::Station;
                            self.services_open = true;
                        }
                        let previous_system = self.own().map(|p| p.system_id);
                        if snapshot
                            .pilots
                            .iter()
                            .find(|p| p.name == self.pilot)
                            .is_some_and(|p| previous_system.is_some_and(|old| old != p.system_id))
                        {
                            self.selected_object = SelectedObject::Station;
                        }
                        if self.embedded {
                            self.timeline_origin = Some(snapshot.server_time_ms);
                            self.observed = Instant::now();
                            self.frozen_time =
                                self.embedded_paused.then_some(snapshot.server_time_ms);
                        }
                        if self.timeline_origin.is_none() {
                            self.timeline_origin = Some(snapshot.server_time_ms);
                            self.observed = Instant::now();
                        }
                        let activity_owner = self.controlled_name().to_owned();
                        self.flight_view.observe_activity(
                            &snapshot,
                            &activity_owner,
                            self.connected,
                            snapshot.server_time_ms,
                        );
                        self.hangar_view.observe_mechanisms(
                            &snapshot,
                            &activity_owner,
                            self.connected,
                            snapshot.server_time_ms,
                        );
                        self.motion.observe(&snapshot);
                        self.performance_snapshot_received =
                            Some(produced_at.unwrap_or_else(Instant::now));
                        self.snapshot = Some(snapshot);
                        if response.ok {
                            self.reconcile_empty_legacy_drone_selection();
                        }
                    }
                    if visible || !response.ok {
                        self.log(response.message, !response.ok);
                    }
                }
            }
        }
    }
    fn own(&self) -> Option<PilotView> {
        self.snapshot
            .as_ref()?
            .pilots
            .iter()
            .find(|p| p.name == self.controlled_name())
            .cloned()
    }
    fn scene_planets(&self) -> &[PlanetDefinition] {
        self.snapshot
            .as_ref()
            .and_then(|snapshot| snapshot_world_layout(snapshot).ok())
            .map_or(&[], |layout| layout.planets.as_slice())
    }
    /// Leave a presentation session without joining its authority worker on the UI thread.
    /// An in-flight transaction finishes atomically; no queued request starts after stop.
    fn disconnect_session(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(stop) = &self.physical_stop {
            stop.store(true, std::sync::atomic::Ordering::Release);
        }
        self.commands = None;
        self.updates = None;
        self.connected = false;
        self.connecting = false;
        self.snapshot = None;
        self.effects.clear();
        if self.physical_session {
            self.physical_cargo = None;
            self.flight_view.clear_physical();
            self.flight_view.invalidate_bay_monitor_session();
            self.flight_view.clear_cockpit_presentation();
            self.hangar_view.clear_cockpit_presentation();
            self.cockpit_computer.cancel_input();
            self.pilot_input.relinquish(&self.controls);
            self.request_pending = false;
            self.performance_snapshot_received = None;
            self.motion.clear();
            self.invalidate_ai_session();
            self.notice =
                "Physical preview closed. Reopen the explicit local preview to reconnect.".into();
        }
    }
    fn reject_world_layout(&mut self, error: String) {
        if self.physical_session {
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(stop) = &self.physical_stop {
                stop.store(true, std::sync::atomic::Ordering::Release);
            }
            self.updates = None;
            self.performance_snapshot_received = None;
        }
        self.physical_cargo = None;
        self.flight_view.clear_physical();
        self.flight_view.invalidate_bay_monitor_session();
        self.cockpit_computer.cancel_input();
        self.flight_view.clear_cockpit_presentation();
        self.invalidate_ai_session();
        self.effects.clear();
        self.motion.clear();
        self.snapshot = None;
        self.pilot_input.relinquish(&self.controls);
        self.connected = false;
        self.connecting = false;
        self.request_pending = false;
        self.commands = None;
        if self.boot.visible {
            self.boot.fail();
        }
        self.notice = error.clone();
        self.log(error, true);
    }
    fn view_selector(&mut self, ui: &mut egui::Ui) {
        self.flight_workspace_view_selector(ui);
    }

    fn service_visible(&self, _tab: ServiceTab) -> bool {
        // Destinations remain reachable on foot; each panel gates its commands.
        true
    }
    fn release_workspace_input(&mut self) {
        self.nearby_shortcut.cancel();
        self.workspace_input_waiting_release = true;
        self.cockpit_computer.cancel_input();
        let _ = self.pilot_input.update(false, Default::default());
        self.controls.clear_flight();
        self.stop_station_walk();
    }

    fn service_tabs(&mut self, ui: &mut egui::Ui) {
        self.operations_navigation(ui);
    }
    fn service_content(&mut self, ui: &mut egui::Ui) {
        let on_foot = self
            .own()
            .is_some_and(|pilot| pilot.station_avatar.is_some());
        if on_foot
            && matches!(
                self.service_tab,
                ServiceTab::Navigation | ServiceTab::Modules
            )
        {
            ui.colored_label(
                AMBER,
                "Inspection only · return to the physical pilot seat to use ship controls.",
            );
            ui.separator();
        }
        if on_foot && self.service_tab == ServiceTab::Sandbox {
            ui.heading("Surface tools");
            self.surface_status(ui);
            ui.colored_label(AMBER, "Surface editing requires the physical pilot seat. Your current body and camera remain unchanged.");
            return;
        }
        match self.service_tab {
            ServiceTab::Layouts => {
                ui.label("Ship layout viewer is open in its own window.");
            }
            ServiceTab::Navigation => {
                ui.heading("Navigation");
                self.flight_deck(ui);
                self.surface_status(ui);
                self.overview(ui);
            }
            ServiceTab::ShipComputer => self.resource_job_panel(ui),
            ServiceTab::Displays => self.captain_displays_panel(ui),
            ServiceTab::Sandbox => self.sandbox_panel(ui),
            ServiceTab::Modules => self.mining_module(ui),
            ServiceTab::Drones => self.drone_controls(ui),
            ServiceTab::Station => self.station_panel(ui),
            ServiceTab::Corporation => self.corporation_panel(ui),
            ServiceTab::Hangar => self.hangar_panel(ui),
            ServiceTab::Cargo => self.cargo_panel(ui),
            ServiceTab::Exchange => self.market(ui),
            ServiceTab::Ledger => self.ledger_panel(ui),
            ServiceTab::Settings => self.settings_panel(ui),
            ServiceTab::Activity => {
                for (entry, error) in &self.feed {
                    ui.colored_label(if *error { AMBER } else { MUTED }, entry);
                    ui.separator();
                }
            }
        }
    }
    fn flight_deck(&mut self, ui: &mut egui::Ui) {
        let Some(pilot) = self.own() else { return };
        let readiness = self.navigation_order_reason();
        let fresh = readiness.is_none();
        let departure_reason = self.departure_order_reason();
        if let Some(reason) = departure_reason {
            ui.colored_label(AMBER, reason);
        }
        if departure_reason.is_none() {
            if let Some(reason) = self.bay_departure_reason() {
                ui.colored_label(AMBER, reason);
            }
        }
        let speed = pilot.flight.as_ref().map_or(0.0, |f| {
            distance_m(
                f.warp
                    .as_ref()
                    .filter(|_| matches!(pilot.location, Location::InTransit { .. }))
                    .map_or(f.velocity, |w| sample_warp(w, self.presentation_time()).1),
                [0.0; 3],
            )
        });
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(
                self.accent(),
                format!("{} / {}", pilot.name, pilot.ship_class.spec().name),
            );
            ui.small(format!(
                "{} · {} CR · {}/{} ore",
                format_speed(speed),
                pilot.credits,
                pilot.ore,
                cargo_capacity(pilot.ship_class)
            ));
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    departure_reason.is_none()
                        && self.can_operate()
                        && pilot.station_avatar.is_none()
                        && pilot.location == Location::Docked,
                    egui::Button::new("Undock").small(),
                )
                .clicked()
            {
                self.send(Request::Undock);
            }
            if ui
                .add_enabled(
                    fresh && self.can_dock(&pilot),
                    egui::Button::new("Dock").small(),
                )
                .clicked()
            {
                self.send(Request::Dock);
            }
            if ui
                .add_enabled(fresh && self.can_fly(), egui::Button::new("Stop").small())
                .clicked()
            {
                self.send(Request::Navigate {
                    mode: NavigationMode::Stop,
                });
            }
            if ui.small_button("Modules").clicked() {
                self.service_tab = ServiceTab::Modules;
                self.services_open = true;
            }
            if ui.small_button("Drone fleet").clicked() {
                self.service_tab = ServiceTab::Drones;
                self.services_open = true;
            }
            let active = pilot
                .mining_modules
                .iter()
                .filter(|m| m.target_id.is_some())
                .count();
            ui.colored_label(
                if active > 0 { self.accent() } else { MUTED },
                format!(
                    "{active}/2 turrets · {} locks",
                    Self::confirmed_locks(&pilot).len()
                ),
            );
        });
    }
    fn current_system(&self) -> u32 {
        self.own().map_or(HOME_SYSTEM_ID, |p| p.system_id)
    }
    fn selected_asteroid(&self) -> Option<&Asteroid> {
        let SelectedObject::Asteroid(id) = self.selected_object else {
            return None;
        };
        self.snapshot
            .as_ref()?
            .asteroids
            .iter()
            .find(|a| a.id == id && a.system_id == self.current_system())
    }
    fn selected_gate(&self) -> Option<Stargate> {
        let SelectedObject::Gate(id) = self.selected_object else {
            return None;
        };
        gates()
            .into_iter()
            .find(|g| g.id == id && g.system_id == self.current_system())
    }
    fn can_lock_selected(&self) -> bool {
        self.can_fly()
            && self.own().is_some_and(|p| {
                self.selected_asteroid().is_some_and(|a| {
                    a.ore > 0
                        && Self::confirmed_locks(&p).len() + Self::pending_locks(&p).len()
                            < MAX_TARGET_LOCKS
                        && !Self::confirmed_locks(&p).contains(&a.id)
                        && !Self::pending_locks(&p)
                            .iter()
                            .any(|lock| lock.asteroid_id == a.id)
                        && p.flight.as_ref().is_some_and(|f| {
                            (distance_m(f.position, a.position) - a.radius).max(0.0) <= LOCK_RANGE
                        })
                })
            })
    }
    fn can_jump_selected(&self) -> bool {
        self.can_fly()
            && self.bay_departure_reason().is_none()
            && self.own().is_some_and(|p| {
                self.selected_gate().is_some_and(|g| {
                    p.flight
                        .as_ref()
                        .is_some_and(|f| distance_m(f.position, g.position) <= GATE_RANGE)
                })
            })
    }
    fn start_module(&mut self, module_id: u8) {
        if let Some(pilot) = self.own().filter(|p| self.can_mine(p)) {
            if let Some(target_id) = self.module_target(&pilot) {
                self.mining_target = Some(target_id);
                self.send(Request::StartMining {
                    module_id,
                    target_id,
                });
            }
        }
    }
    fn mining_effect_target(&self) -> Option<&Asteroid> {
        let id = self.effects.mining_target(self.presentation_time())?;
        let asteroid = self
            .snapshot
            .as_ref()?
            .asteroids
            .iter()
            .find(|a| a.id == id)?;
        self.own()
            .filter(|p| effects::can_extract_target(p, asteroid))
            .map(|_| asteroid)
    }
    fn can_dock(&self, pilot: &PilotView) -> bool {
        if self.bay_departure_reason().is_some() {
            return false;
        }
        if let Some(f) = pilot.flight.as_ref().filter(|f| f.docking.is_some()) {
            let d = f.docking.as_ref().unwrap();
            return self.can_operate()
                && pilot.station_avatar.is_none()
                && d.phase == DockingPhase::Free
                && pilot.drones.is_empty()
                && matches!(pilot.location, Location::Station | Location::Belt)
                && distance_m(f.velocity, [0.; 3]) <= 10.
                && distance_m(f.position, docking_gate_position(d.berth_id)) <= 6000.
                && f.position[0] >= docking_berth_position(d.berth_id)[0] + 100.;
        }
        self.can_operate()
            && pilot.station_avatar.is_none()
            && in_navigation_range(
                pilot,
                STATION_POSITION,
                DOCK_RANGE + hull_radius_m(pilot.ship_class),
            )
    }
    fn own_cargo_capacity(&self) -> u64 {
        self.own()
            .map_or(CARGO_CAPACITY, |p| cargo_capacity(p.ship_class))
    }
    fn can_mine(&self, pilot: &PilotView) -> bool {
        self.can_operate()
            && self.operations_fresh()
            && pilot.station_avatar.is_none()
            && pilot.ore < cargo_capacity(pilot.ship_class)
            && self.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.asteroids.iter().any(|a| {
                    Some(a.id) == self.module_target(pilot)
                        && a.ore > 0
                        && effects::can_extract_target(pilot, a)
                })
            })
    }
    fn selected_target(&self) -> Option<NavigationTarget> {
        match &self.selected_object {
            SelectedObject::Planet(_) | SelectedObject::Prop(_) => None,
            SelectedObject::Asteroid(id) => self
                .selected_asteroid()
                .map(|_| NavigationTarget::Asteroid { id: *id }),
            SelectedObject::Gate(id) => self
                .selected_gate()
                .map(|_| NavigationTarget::Gate { id: *id }),
            SelectedObject::Station => Some(NavigationTarget::Station),
            SelectedObject::Belt => Some(NavigationTarget::Belt),
            SelectedObject::Pilot(name) => self
                .snapshot
                .as_ref()
                .and_then(|s| {
                    s.pilots.iter().find(|p| {
                        p.name == *name
                            && p.name != self.controlled_name()
                            && p.system_id == self.current_system()
                            && p.location != Location::Docked
                            && p.flight.is_some()
                    })
                })
                .map(|p| NavigationTarget::Pilot {
                    pilot: p.name.clone(),
                }),
        }
    }
    fn selected_position(&self) -> Option<[f64; 3]> {
        match &self.selected_object {
            SelectedObject::Planet(id) => self
                .scene_planets()
                .iter()
                .find(|p| p.id == *id && p.system_id == self.current_system())
                .map(|planet| planet.approach_position),
            SelectedObject::Prop(id) => self
                .snapshot
                .as_ref()?
                .surface_props
                .iter()
                .find(|p| p.id == *id && p.system_id == self.current_system())
                .map(|p| p.position),
            SelectedObject::Asteroid(_) => self.selected_asteroid().map(|a| a.position),
            SelectedObject::Gate(_) => self.selected_gate().map(|g| g.position),
            SelectedObject::Station => Some(STATION_POSITION),
            SelectedObject::Belt => Some(BELT_POSITION),
            SelectedObject::Pilot(name) => self
                .snapshot
                .as_ref()?
                .pilots
                .iter()
                .find(|p| p.name == *name && p.system_id == self.current_system())
                .map(|p| motion::position(p, self.presentation_time())),
        }
    }
    fn scene(&mut self, ui: &mut egui::Ui) {
        let now = self.presentation_time();
        let display = self.snapshot.as_ref().map(|s| self.motion.sample(s, now));
        let own_position = display
            .as_ref()
            .and_then(|s| s.pilots.iter().find(|p| p.name == self.controlled_name()))
            .map_or(STATION_POSITION, |p| motion::position(p, now));
        let base = if self.tactical_system {
            [0.0; 3]
        } else {
            own_position
        };
        let base_span = if self.tactical_system {
            AU_METERS * 20.0
        } else {
            32000.0
        };
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), ui.available_height().max(220.0)),
            egui::Sense::click_and_drag(),
        );
        let world_input = scene_pointer_allowed(ui.ctx(), ui.layer_id(), rect);
        if world_input {
            if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    tactical_zoom_at(
                        rect,
                        pointer,
                        base,
                        base_span,
                        &mut self.tactical_pan,
                        &mut self.tactical_zoom,
                        scroll,
                    );
                }
            }
            if response.dragged() && self.tactical_drag {
                let delta = ui.input(|i| i.pointer.delta());
                let meters =
                    base_span * self.tactical_zoom / rect.width().min(rect.height()) as f64;
                self.tactical_pan[0] -= delta.x as f64 * meters;
                self.tactical_pan[1] -= delta.y as f64 * meters;
            }
        }
        if !response.dragged() {
            self.tactical_drag = false;
        }
        let center = [
            base[0] + self.tactical_pan[0],
            base[1],
            base[2] + self.tactical_pan[1],
        ];
        let span = base_span * self.tactical_zoom;
        let tactical_scale = |rect: egui::Rect| rect.width().min(rect.height()) as f64 / span;
        let tactical_project =
            |rect: egui::Rect, point: [f64; 3]| map_project(rect, point, center, span);
        let tactical_unproject = |rect: egui::Rect, point: Pos2, altitude: f64| {
            map_unproject(rect, point, center, span, altitude)
        };
        let clicked = if world_input && response.drag_started() {
            ui.input(|i| i.pointer.press_origin())
        } else if world_input && (response.clicked() || response.secondary_clicked()) {
            response.interact_pointer_pos()
        } else {
            None
        };
        let mut selected = None;
        let mut hit_score = f32::INFINITY;
        let mut pick = |center: Pos2, radius: f32, object: SelectedObject| {
            if let Some(point) = clicked {
                let score = point.distance(center) / radius;
                if score <= 1.0 && score < hit_score {
                    hit_score = score;
                    selected = Some(object);
                }
            }
        };
        let paint = ui.painter_at(rect);
        paint.rect_filled(rect, 0.0, Color32::from_rgb(2, 6, 4));
        paint.text(
            rect.center_top() + Vec2::new(0.0, 18.0),
            egui::Align2::CENTER_TOP,
            if self.tactical_system {
                "SYSTEM MAP · 20 AU span · select a destination to warp"
            } else {
                "LOCAL NAVIGATION · 32 km span"
            },
            egui::FontId::proportional(11.0),
            MUTED,
        );
        for i in 0..150 {
            let x = ((i * 7919 + 67) % 1000) as f32 / 1000.0;
            let y = ((i * 3571 + 239) % 1000) as f32 / 1000.0;
            paint.circle_filled(
                Pos2::new(
                    rect.left() + x * rect.width(),
                    rect.top() + y * rect.height(),
                ),
                if i % 9 == 0 { 1.3 } else { 0.65 },
                Color32::from_gray(50 + (i % 75) as u8),
            );
        }
        for planet in self
            .scene_planets()
            .iter()
            .filter(|p| p.system_id == self.current_system())
        {
            let point = tactical_project(rect, planet.approach_position);
            paint.circle_stroke(point, 18.0, Stroke::new(2.0, self.accent()));
            paint.text(
                point + Vec2::new(0.0, 22.0),
                egui::Align2::CENTER_TOP,
                &planet.name,
                egui::FontId::proportional(12.0),
                self.accent(),
            );
            pick(point, 22.0, SelectedObject::Planet(planet.id));
        }
        if let Some(snapshot) = &self.snapshot {
            for prop in snapshot
                .surface_props
                .iter()
                .filter(|p| p.system_id == self.current_system())
            {
                let point = tactical_project(rect, prop.position);
                paint.rect_filled(
                    egui::Rect::from_center_size(point, Vec2::splat(7.0)),
                    1.0,
                    AMBER,
                );
                pick(point, 12.0, SelectedObject::Prop(prop.id));
            }
        }
        let station = tactical_project(rect, STATION_POSITION);
        let belt = tactical_project(rect, BELT_POSITION);
        pick(station, 43.0, SelectedObject::Station);
        pick(belt, 50.0, SelectedObject::Belt);
        if self.selected_object == SelectedObject::Station {
            selection_brackets(&paint, station, 44.0, self.accent());
        }
        if self.selected_object == SelectedObject::Belt {
            selection_brackets(&paint, belt, 53.0, self.accent());
        }
        paint.line_segment(
            [station, belt],
            Stroke::new(1.0, Color32::from_rgb(35, 57, 76)),
        );
        paint.circle_stroke(station, 36.0, Stroke::new(1.5, self.accent()));
        paint.circle_stroke(
            station,
            24.0,
            Stroke::new(1.0, self.accent().gamma_multiply(0.4)),
        );
        paint.rect_filled(
            egui::Rect::from_center_size(station, Vec2::new(15.0, 45.0)),
            3.0,
            self.accent().gamma_multiply(0.6),
        );
        paint.rect_filled(
            egui::Rect::from_center_size(station, Vec2::new(45.0, 15.0)),
            3.0,
            self.accent().gamma_multiply(0.6),
        );
        paint.text(
            station + Vec2::new(0.0, 60.0),
            egui::Align2::CENTER_TOP,
            "ARKLINE STATION",
            egui::FontId::proportional(13.0),
            self.accent(),
        );
        paint.circle_stroke(
            belt,
            (2100.0 * tactical_scale(rect)) as f32,
            Stroke::new(1.0, MUTED.gamma_multiply(0.2)),
        );
        if let Some(snapshot) = &display {
            for asteroid in snapshot
                .asteroids
                .iter()
                .filter(|a| a.system_id == self.current_system())
            {
                let center = tactical_project(rect, asteroid.position);
                let radius = ((asteroid.radius * tactical_scale(rect)) as f32).max(3.0);
                let tint = if asteroid.ore > 0 {
                    Color32::from_rgb(135, 116, 88)
                } else {
                    Color32::from_gray(50)
                };
                paint.add(egui::Shape::convex_polygon(
                    vec![
                        center + Vec2::new(-radius, -radius * 0.45),
                        center + Vec2::new(radius * 0.3, -radius),
                        center + Vec2::new(radius, radius * 0.5),
                        center + Vec2::new(-radius * 0.4, radius),
                    ],
                    tint,
                    Stroke::new(0.7, MUTED),
                ));
                pick(center, radius + 5.0, SelectedObject::Asteroid(asteroid.id));
                if self.selected_object == SelectedObject::Asteroid(asteroid.id) {
                    selection_brackets(&paint, center, radius + 6.0, self.accent());
                    paint.text(
                        center + Vec2::new(0.0, -radius - 10.0),
                        egui::Align2::CENTER_BOTTOM,
                        &asteroid.name,
                        egui::FontId::proportional(11.0),
                        self.accent(),
                    );
                }
                if self
                    .own()
                    .is_some_and(|p| Self::confirmed_locks(&p).contains(&asteroid.id))
                {
                    paint.circle_stroke(center, radius + 4.0, Stroke::new(1.5, self.accent()));
                }
            }
        }
        for gate in gates()
            .into_iter()
            .filter(|g| g.system_id == self.current_system())
        {
            let center = tactical_project(rect, gate.position);
            let radius = ((gate.radius * tactical_scale(rect)) as f32).max(8.0);
            paint.circle_stroke(center, radius, Stroke::new(2.5, self.accent()));
            paint.circle_stroke(
                center,
                radius * 0.55,
                Stroke::new(1.0, self.accent().gamma_multiply(0.4)),
            );
            paint.text(
                center + Vec2::new(0.0, radius + 8.0),
                egui::Align2::CENTER_TOP,
                format!(
                    "{} → {}",
                    gate.name,
                    system_name(gate.destination_system_id)
                ),
                egui::FontId::proportional(11.0),
                self.accent(),
            );
            pick(center, radius + 7.0, SelectedObject::Gate(gate.id));
            if self.selected_object == SelectedObject::Gate(gate.id) {
                selection_brackets(&paint, center, radius + 8.0, self.accent());
            }
        }
        if let Some(snapshot) = &display {
            let server_now = self.presentation_time();
            for pilot in &snapshot.pilots {
                if pilot.system_id != self.current_system() || pilot.location == Location::Docked {
                    continue;
                }
                let position =
                    tactical_project(rect, flight3d::display_position(pilot, server_now));
                pick(position, 20.0, SelectedObject::Pilot(pilot.name.clone()));
                if self.selected_object == SelectedObject::Pilot(pilot.name.clone()) {
                    selection_brackets(&paint, position, 15.0, self.accent());
                }
                let heading = flight3d::heading(&pilot.location);
                let direction = pilot
                    .flight
                    .as_ref()
                    .map(|f| {
                        let heading = f
                            .warp
                            .as_ref()
                            .map_or(f.heading, |path| sample_warp_heading(path, server_now));
                        Vec2::new(heading[0] as f32, heading[2] as f32).normalized()
                    })
                    .filter(|v| v.length_sq() > 0.1)
                    .unwrap_or(Vec2::new(heading, 0.0));
                let perpendicular = Vec2::new(-direction.y, direction.x);
                let color = if pilot.name == self.controlled_name() {
                    self.accent()
                } else {
                    AMBER
                };
                if pilot.name == self.controlled_name() {
                    for module in pilot.mining_modules.iter().filter(|_| self.connected) {
                        if let Some(cycle) =
                            effects::module_cycle(pilot, module.module_id, server_now)
                        {
                            let Some(asteroid) =
                                snapshot.asteroids.iter().find(|a| a.id == cycle.target_id)
                            else {
                                continue;
                            };
                            let progress = cycle.progress;
                            let muzzle = position
                                + perpendicular * if module.module_id == 0 { -5.0 } else { 5.0 };
                            let target = tactical_project(rect, asteroid.position);
                            let intensity = 0.85 + 0.15 * (server_now as f32 * 0.014).sin();
                            paint.line_segment(
                                [muzzle, target],
                                Stroke::new(9.0, self.accent().gamma_multiply(0.10 * intensity)),
                            );
                            paint.line_segment(
                                [muzzle, target],
                                Stroke::new(3.0, self.accent().gamma_multiply(0.65 * intensity)),
                            );
                            paint.line_segment(
                                [muzzle, target],
                                Stroke::new(
                                    1.0,
                                    Color32::from_rgb(218, 255, 244).gamma_multiply(intensity),
                                ),
                            );
                            for i in 0..12 {
                                let travel = (progress * 2.8 + i as f32 / 12.0).fract();
                                let drift =
                                    Vec2::new((i as f32 * 2.4).sin(), (i as f32 * 3.7).cos())
                                        * (1.0 - travel)
                                        * 9.0;
                                let particle = target.lerp(muzzle, travel) + drift;
                                paint.circle_filled(
                                    particle,
                                    1.2 + (i % 3) as f32 * 0.5,
                                    AMBER.gamma_multiply(intensity * (1.0 - travel * 0.5)),
                                );
                            }
                            paint.circle_stroke(
                                target,
                                7.0 + (progress * 13.0).fract() * 6.0,
                                Stroke::new(1.0, self.accent().gamma_multiply(intensity)),
                            );
                            paint.text(
                                position + Vec2::new(0.0, 18.0 + module.module_id as f32 * 15.0),
                                egui::Align2::CENTER_TOP,
                                format!("Turret {} · cycling", module.module_id + 1),
                                egui::FontId::proportional(12.0),
                                self.accent(),
                            );
                        }
                    }
                    for event in &self.effects.events {
                        let ring_color = match event.kind {
                            effects::EffectKind::Docking if pilot.location == Location::Docked => {
                                self.accent()
                            }
                            effects::EffectKind::Undocking
                                if pilot.location == Location::Station =>
                            {
                                AMBER
                            }
                            _ => continue,
                        };
                        let phase = server_now.saturating_sub(event.started_ms) as f32 / 1800.0;
                        if phase < 1.0 {
                            paint.circle_stroke(
                                position,
                                10.0 + phase * 34.0,
                                Stroke::new(2.0, ring_color.gamma_multiply(1.0 - phase)),
                            );
                        }
                    }
                }
                if let Some((phase, progress)) = motion::warp_phase(pilot, server_now) {
                    if phase == "WARP WARMUP" {
                        let radius = 12.0 + progress * 18.0;
                        paint.circle_stroke(
                            position,
                            radius,
                            Stroke::new(2.0, color.gamma_multiply(0.75)),
                        );
                        paint.circle_stroke(
                            position,
                            radius + 6.0,
                            Stroke::new(5.0, color.gamma_multiply(0.12)),
                        );
                        for i in 0..10 {
                            let angle =
                                server_now as f32 * 0.005 + i as f32 * std::f32::consts::TAU / 10.0;
                            paint.circle_filled(
                                position + Vec2::angled(angle) * radius,
                                1.8,
                                color,
                            );
                        }
                    } else {
                        let dir = -direction;
                        let trail = if matches!(phase, "WARP DECELERATING" | "WARP ARRIVAL") {
                            70.0 * (1.0 - progress) + 12.0
                        } else {
                            40.0 + progress * 32.0
                        };
                        paint.line_segment(
                            [position, position + dir * trail],
                            Stroke::new(7.0, color.gamma_multiply(0.12)),
                        );
                        paint.line_segment(
                            [position, position + dir * trail],
                            Stroke::new(2.5, color.gamma_multiply(0.8)),
                        );
                        for i in 0..7 {
                            let offset = (server_now as f32 * 0.06 + i as f32 * 13.0) % 72.0;
                            let y = ((i % 2) as f32 * 2.0 - 1.0) * (11.0 + (i / 2) as f32 * 7.0);
                            let start = position + dir * offset + perpendicular * y;
                            paint.line_segment(
                                [start, start + dir * 16.0],
                                Stroke::new(1.0, color.gamma_multiply(0.35)),
                            );
                        }
                    }
                    if pilot.name == self.controlled_name() {
                        paint.text(
                            position + Vec2::new(0.0, 34.0),
                            egui::Align2::CENTER_TOP,
                            phase,
                            egui::FontId::monospace(12.0),
                            color,
                        );
                    }
                }
                paint.add(egui::Shape::convex_polygon(
                    vec![
                        position + direction * 8.0,
                        position - direction * 6.0 - perpendicular * 5.0,
                        position - direction * 6.0 + perpendicular * 5.0,
                    ],
                    color,
                    Stroke::NONE,
                ));
                paint.text(
                    position + Vec2::new(0.0, -13.0),
                    egui::Align2::CENTER_BOTTOM,
                    &pilot.name,
                    egui::FontId::proportional(11.0),
                    color,
                );
            }
            paint.text(
                rect.left_top() + Vec2::new(18.0, 18.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "{} / {} local asteroids",
                    system_name(self.current_system()),
                    snapshot
                        .asteroids
                        .iter()
                        .filter(|a| a.system_id == self.current_system())
                        .count()
                ),
                egui::FontId::monospace(12.0),
                MUTED,
            );
        }
        let hit_object = selected.is_some();
        if world_input && response.drag_started() {
            self.tactical_drag = !hit_object;
        }
        if response.clicked() || response.secondary_clicked() {
            if let Some(selected) = selected {
                self.selected_object = selected;
            }
        }
        if world_input
            && !self.tactical_system
            && response.double_clicked()
            && !hit_object
            && self.can_maneuver()
        {
            if let Some(point) = response.interact_pointer_pos() {
                let altitude = self
                    .own()
                    .and_then(|p| p.flight.map(|f| f.position[1]))
                    .unwrap_or(0.0);
                self.send(Request::Navigate {
                    mode: NavigationMode::FlyTo {
                        position: tactical_unproject(rect, point, altitude),
                    },
                });
            }
        }
        if world_input && response.secondary_clicked() {
            self.context_fly_to = response
                .interact_pointer_pos()
                .filter(|_| !hit_object && !self.tactical_system)
                .map(|point| {
                    tactical_unproject(
                        rect,
                        point,
                        self.own()
                            .and_then(|p| p.flight.map(|f| f.position[1]))
                            .unwrap_or(0.0),
                    )
                });
        }
        if world_input || response.context_menu_opened() {
            response.context_menu(|ui| {
                if let Some(position) = self.context_fly_to {
                    if ui
                        .add_enabled(self.can_maneuver(), egui::Button::new("Fly here"))
                        .clicked()
                    {
                        self.send(Request::Navigate {
                            mode: NavigationMode::FlyTo { position },
                        });
                        ui.close_menu();
                    }
                } else if let Some(target) = self.selected_target() {
                    if ui
                        .add_enabled(self.can_maneuver(), egui::Button::new("Approach selected"))
                        .clicked()
                    {
                        self.send(Request::Navigate {
                            mode: NavigationMode::Approach { target },
                        });
                        ui.close_menu();
                    }
                }
                if let Some(asteroid) = self.selected_asteroid().cloned() {
                    if ui
                        .add_enabled(self.can_lock_selected(), egui::Button::new("Lock asteroid"))
                        .clicked()
                    {
                        self.send(Request::LockAsteroid { id: asteroid.id });
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.own().is_some_and(|p| self.can_mine(&p)),
                            egui::Button::new("Activate turret 1"),
                        )
                        .clicked()
                    {
                        self.start_module(0);
                        ui.close_menu();
                    }
                }
                if let Some(gate) = self.selected_gate() {
                    if ui
                        .add_enabled(self.can_warp(), egui::Button::new("Warp to gate"))
                        .clicked()
                    {
                        self.send(Request::Warp {
                            destination: Destination::Gate { id: gate.id },
                        });
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            self.can_jump_selected(),
                            egui::Button::new("Jump through gate"),
                        )
                        .clicked()
                    {
                        self.send(Request::Jump { gate_id: gate.id });
                        ui.close_menu();
                    }
                }
                if ui
                    .add_enabled(self.can_fly(), egui::Button::new("Stop ship"))
                    .clicked()
                {
                    self.send(Request::Navigate {
                        mode: NavigationMode::Stop,
                    });
                    ui.close_menu();
                }
            });
        }
        paint.text(
            rect.right_bottom() + Vec2::new(-12.0, -12.0),
            egui::Align2::RIGHT_BOTTOM,
            "CLICK TO SELECT · USE FLIGHT CONTROLS TO NAVIGATE",
            egui::FontId::monospace(10.0),
            MUTED,
        );
        if !self.connected {
            paint.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "CONNECTION LOST · LAST KNOWN STATE",
                egui::FontId::proportional(16.0),
                AMBER,
            );
        }
    }
}
fn tactical_zoom_at(
    rect: egui::Rect,
    cursor: Pos2,
    base: [f64; 3],
    base_span: f64,
    pan: &mut [f64; 2],
    zoom: &mut f64,
    scroll: f32,
) {
    let center = [base[0] + pan[0], base[1], base[2] + pan[1]];
    let before = map_unproject(rect, cursor, center, base_span * *zoom, base[1]);
    *zoom = (*zoom * (-scroll as f64 * 0.004).exp())
        .clamp(100.0 / base_span, AU_METERS * 100.0 / base_span);
    let after = map_unproject(rect, cursor, center, base_span * *zoom, base[1]);
    pan[0] += before[0] - after[0];
    pan[1] += before[2] - after[2];
}
fn scene_pointer_allowed(ctx: &egui::Context, layer: egui::LayerId, rect: egui::Rect) -> bool {
    ctx.input(|i| i.pointer.hover_pos())
        .is_some_and(|point| rect.contains(point) && ctx.layer_id_at(point) == Some(layer))
}
fn in_navigation_range(pilot: &PilotView, target: [f64; 3], range: f64) -> bool {
    !matches!(
        pilot.location,
        Location::Docked | Location::InTransit { .. } | Location::Jumping { .. }
    ) && pilot
        .flight
        .as_ref()
        .is_some_and(|f| distance_m(f.position, target) <= range)
}
fn system_name(id: u32) -> String {
    systems()
        .into_iter()
        .find(|s| s.id == id)
        .map(|s| s.name)
        .unwrap_or_else(|| format!("Area {id}"))
}
fn distance_m(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter()
        .zip(b)
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f64>()
        .sqrt()
}
pub(crate) fn format_speed(value: f64) -> String {
    if value >= AU_METERS * 0.001 {
        format!("{:.3} AU/s", value / AU_METERS)
    } else if value >= 1000.0 {
        format!("{:.1} km/s", value / 1000.0)
    } else {
        format!("{value:.0} m/s")
    }
}
pub(crate) fn format_distance(value: f64) -> String {
    if value >= AU_METERS * 0.01 {
        format!("{:.3} AU", value / AU_METERS)
    } else if value >= 1_000_000.0 {
        format!("{:.0} km", value / 1000.0)
    } else if value >= 1000.0 {
        format!("{:.1} km", value / 1000.0)
    } else {
        format!("{value:.0} m")
    }
}
fn map_project(rect: egui::Rect, point: [f64; 3], center: [f64; 3], span: f64) -> Pos2 {
    let scale = rect.width().min(rect.height()) as f64 / span;
    Pos2::new(
        rect.center().x + ((point[0] - center[0]) * scale) as f32,
        rect.center().y + ((point[2] - center[2]) * scale) as f32,
    )
}
fn map_unproject(
    rect: egui::Rect,
    point: Pos2,
    center: [f64; 3],
    span: f64,
    altitude: f64,
) -> [f64; 3] {
    let scale = rect.width().min(rect.height()) as f64 / span;
    [
        center[0] + (point.x - rect.center().x) as f64 / scale,
        altitude,
        center[2] + (point.y - rect.center().y) as f64 / scale,
    ]
    .map(|v| v.clamp(-SPACE_BOUND, SPACE_BOUND))
}
fn selection_brackets(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            let corner = center + Vec2::new(x * radius, y * radius);
            painter.line_segment(
                [corner, corner - Vec2::new(x * 9.0, 0.0)],
                Stroke::new(1.5, color),
            );
            painter.line_segment(
                [corner, corner - Vec2::new(0.0, y * 9.0)],
                Stroke::new(1.5, color),
            );
        }
    }
}
fn location_label(location: &Location) -> &'static str {
    match location {
        Location::Docked => "Docked · Arkline Station",
        Location::Jumping { .. } => "Stargate transfer in progress",
        Location::Station => "In space",
        Location::Belt => "In space",
        Location::InTransit { .. } => "Warp in progress",
    }
}
impl eframe::App for Cockpit {
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(error) = self.layout.flush_on_exit(Duration::from_secs(1)) {
            eprintln!("{error}");
        }
        // macOS application termination may bypass unwinding main's stack.
        // Shut down only this cockpit's owned host inside the lifecycle callback.
        #[cfg(not(target_arch = "wasm32"))]
        drop(self.local_server.take());
        self.cockpit_computer.clear();
        self.flight_view.destroy();
        self.hangar_view.destroy();
    }
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if self.focus_on_first_frame {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.focus_on_first_frame = false;
        }
        let frame_now = Instant::now();
        self.frame_intervals
            .push_back(frame_now.duration_since(self.frame_observed).as_secs_f64());
        self.frame_observed = frame_now;
        while self.frame_intervals.len() > 120 {
            self.frame_intervals.pop_front();
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(result) = self.account_login.poll(ctx) {
            match result {
                Ok(())
                    if self
                        .account_login
                        .credential
                        .as_ref()
                        .is_some_and(|a| a.matches(&self.server, self.pilot.trim())) =>
                {
                    self.connect()
                }
                Ok(()) => {
                    self.account_login.credential = None;
                    self.notice = "Sign-in target changed. Sign in again.".into();
                }
                Err(error) => self.notice = error,
            }
        }
        self.poll();
        if !self.first_person_capture_allowed(ctx) {
            self.hangar_view.release_first_person_mouse(ctx);
        }
        if !self.docked_navigation_present() {
            self.nearby_shortcut.cancel();
        }
        if !self.performance_open {
            self.operations_keyboard_navigation(ctx);
        }
        if self.performance_open
            || self.controls_reference_open
            || self.services_open
            || self.service_tab == ServiceTab::Layouts
        {
            self.release_workspace_input();
        } else if self.workspace_input_waiting_release
            && ctx.input(|i| i.keys_down.is_empty() && !i.pointer.any_down())
        {
            self.workspace_input_waiting_release = false;
        }
        if let Some(error) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| validate_snapshot_scene(snapshot, self.assets.as_ref()).err())
        {
            self.reject_world_layout(error);
        }
        if !self.connected {
            self.engineering_panel = None;
        }
        self.sync_remote_control();
        // Receipt freshness is updated only by accepted Response snapshots in
        // poll(), never by a rendered frame or DroneView's motion timestamp.
        // All drone displays share this one confirmed-time sample for the frame.
        self.drone_telemetry_time = drone_telemetry::TelemetryTime::from_observation(
            self.snapshot
                .as_ref()
                .map(|snapshot| snapshot.server_time_ms),
            self.performance_snapshot_received
                .map(|received| received.elapsed().as_millis().min(u64::MAX as u128) as u64),
            self.presentation_time(),
            self.connected,
            self.embedded_paused,
        );
        let unload = self
            .assets
            .as_ref()
            .and_then(|a| a.cargo_unloading.as_ref())
            .filter(|s| s.schema_version == arkline_protocol::CARGO_UNLOADING_SCHEMA_VERSION)
            .and_then(|s| s.latest())
            .cloned();
        self.flight_view.set_cargo_unload(unload.clone());
        self.hangar_view.set_cargo_unload(unload);
        self.flight_view
            .set_drone_telemetry_time(self.drone_telemetry_time);
        self.hangar_view
            .set_drone_telemetry_time(self.drone_telemetry_time);
        if let Some(snapshot) = self.snapshot.as_ref() {
            let owner = self.controlled_name().to_owned();
            self.flight_view.observe_activity(
                snapshot,
                &owner,
                self.connected,
                self.presentation_time(),
            );
            self.hangar_view.observe_mechanisms(
                snapshot,
                &owner,
                self.connected,
                self.presentation_time(),
            );
        } else if !self.connected {
            self.flight_view.disconnect_activity();
            self.hangar_view.disconnect_mechanisms();
        }

        // A previous frame's mounted surface must never survive an early return,
        // a world switch or a hidden/free-camera presentation.
        self.flight_view.clear_cockpit_presentation();
        self.hangar_view.clear_cockpit_presentation();
        let computer_active = !self.ship_observation
            && self.connected
            && !self.controls_reference_open
            && !self.performance_open
            && !self.services_open
            && self.service_tab != ServiceTab::Layouts
            && !self.boot.visible
            && (self.in_station_hangar() && self.verified_docked_binding().is_some()
                || !self.in_station_hangar()
                    && self.flight_view.cockpit_presentation_active(self.view_mode));
        if !computer_active
            || !self.embedded_input
            || self.embedded_paused
            || !ctx.input(|input| input.focused && !input.viewport().minimized.unwrap_or(false))
            || ctx.wants_keyboard_input()
        {
            self.cockpit_computer.cancel_input();
        }
        self.flight_view
            .set_computer_keyboard_owned(self.cockpit_computer.wants_keyboard_input());
        self.hangar_view
            .set_computer_keyboard_owned(self.cockpit_computer.wants_keyboard_input());
        self.poll_voice();
        self.poll_ship_ai();
        self.update_sound();
        if self.embedded && !self.connected {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label(&self.notice);
            });
            ctx.request_repaint_after(Duration::from_millis(50));
            return;
        }
        if self.boot.visible {
            let ready = self.connected && self.own().is_some();
            match self
                .boot
                .show(ctx, ready, self.connected, &self.notice, self.logo.id())
            {
                boot::Action::Enter => {}
                boot::Action::Retry => {
                    self.connect();
                    return;
                }
                boot::Action::Back => {
                    self.disconnect_session();
                    return;
                }
                boot::Action::Wait => return,
            }
        }
        self.effects.retain(self.presentation_time());
        if !self.performance_open
            && !self.controls_reference_open
            && !self.services_open
            && !self.workspace_input_waiting_release
            && self.embedded_input
            && !ctx.wants_keyboard_input()
            && !self.cockpit_computer.wants_keyboard_input()
            && !ctx.input(|input| input.pointer.any_pressed())
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Space))
            && self.can_fly()
        {
            self.send(Request::Navigate {
                mode: NavigationMode::Stop,
            });
        }
        if ctx.input(|i| i.focused && !i.viewport().minimized.unwrap_or(false)) {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if !self.connected && self.snapshot.is_none() {
            egui::TopBottomPanel::top("login-brand").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.image((self.logo.id(), egui::vec2(36.0, 36.0)));
                    ui.heading(egui::RichText::new("ARKLINE").color(self.accent()));
                    ui.colored_label(AMBER, BUILD_VERSION.trim());
                    ui.small("EARLY ACCESS");
                });
            });
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_space((ui.available_height() * 0.13).max(20.0));
                ui.vertical_centered(|ui| {
                    ui.heading(egui::RichText::new("Your universe starts here.").size(34.0));
                    ui.add_space(8.0); ui.colored_label(MUTED, "Connect to a real Arkline server. Fly, mine, trade, and verify.");
                    ui.add_space(28.0);
                    ui.scope(|ui| {
                        ui.set_max_width(400.0);
                        let login_pending = self.connecting;
                        #[cfg(not(target_arch = "wasm32"))]
                        let login_pending = login_pending || self.account_login.pending();
                        ui.add_enabled_ui(!login_pending, |ui| {
                            ui.horizontal(|ui| {
                                for (label, endpoint) in [("Production", transport::OFFICIAL_SERVER), ("Test", transport::TEST_SERVER)] {
                                    if ui.add_enabled(self.credential_scope.is_none(), egui::Button::new(label).selected(self.server == endpoint)).clicked() { self.server = endpoint.into(); }
                                }
                                ui.label("or custom:");
                            });
                            ui.label("Server address"); ui.add_enabled(self.credential_scope.is_none(), egui::TextEdit::singleline(&mut self.server).desired_width(380.0));
                            ui.label("Pilot name"); ui.add(egui::TextEdit::singleline(&mut self.pilot).hint_text("Choose a pilot name").desired_width(380.0));
                            #[cfg(not(target_arch = "wasm32"))]
                            if self.account_login.configured(&self.server) {
                                ui.label("Account sign-in opens your browser. Your password stays with the identity provider.");
                                ui.checkbox(&mut self.link_existing_account, "Link this pilot using its saved legacy credential");
                                ui.small("Linking requires both account sign-in and this pilot's original credential. Missing credentials require operator recovery.");
                            } else {
                                ui.small("Legacy pilot credentials: creates a pilot on first connection, then resumes it on this device. Account sign-in is not configured here.");
                            }
                            #[cfg(target_arch = "wasm32")]
                            ui.small("Creates a pilot on first connection. Saved credentials resume it automatically on this device.");
                            ui.add_space(10.0);
                            let enter_label = "Enter the universe";
                            #[cfg(not(target_arch = "wasm32"))]
                            let enter_label = if self.account_login.configured(&self.server) { "Sign in and enter the universe" } else { enter_label };
                            if ui.add_enabled(!self.pilot.trim().is_empty() && !self.server.trim().is_empty(), egui::Button::new(enter_label).min_size(Vec2::new(380.0, 44.0))).clicked() { self.connect(); }
                        });
                        #[cfg(not(target_arch = "wasm32"))]
                        if self.account_login.pending() && ui.button("Cancel browser sign-in").clicked() {
                            self.account_login.cancel(); self.notice = "Sign-in cancelled".into();
                        }
                        if !self.notice.is_empty() { ui.add_space(12.0); ui.colored_label(AMBER, &self.notice); }
                        ui.add_space(22.0); ui.separator();
                        if self.credential_scope.is_some() {
                            ui.colored_label(self.accent(), "Local universe is running. Your world saves automatically.");
                            ui.small("Keep the hosting cockpit open while other pilots play.");
                            if ui.button("Open second cockpit").clicked() { self.second_cockpit(); }
                        } else {
                            ui.label("Start a local universe from the repository:");
                            ui.code("cargo run -p arkline-server");
                        }
                        ui.small("Official servers use encrypted WSS. Production and Test have separate pilots and worlds.");
                        let version = BUILD_VERSION.trim();
                        ui.horizontal_wrapped(|ui| {
                            ui.hyperlink_to("Source (AGPL-3.0)", format!("https://downloads.arkline.network/{version}/Arkline-{version}-aarch64-apple-darwin-source.tar.gz"));
                            ui.hyperlink_to("License", "https://play.arkline.network/LICENSE");
                        });
                        #[cfg(target_arch = "wasm32")]
                        ui.small("Browser credentials are saved in this browser. Clearing site data loses your resume key. Native speech, SSH model orders, and independent ledger auditing require the native client.");
                    });
                });
            });
            return;
        }
        if !self.embedded && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F11))
        {
            toggle_fullscreen(ctx);
        }
        if !self.service_visible(self.service_tab) {
            self.service_tab = ServiceTab::Station;
        }
        self.sync_workspace_identity();
        if !ctx.wants_keyboard_input()
            && ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::H)
            })
        {
            self.toggle_identity_status();
        }
        let in_hangar = self.in_station_hangar();
        // Reserve persistent bottom chrome before placing any task/identity window.
        let show_station_overlay = in_hangar
            || self.space_walk_available()
            || self
                .own()
                .is_some_and(|pilot| pilot.station_avatar.is_some());
        if show_station_overlay && self.embedded {
            self.station_overlay(ctx);
        }
        egui::TopBottomPanel::bottom("flight_authority_status")
            .frame(
                egui::Frame::new()
                    .fill(self.theme.panel())
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| self.flight_workspace_status(ui));
        if show_station_overlay && !self.embedded {
            self.station_overlay(ctx);
        }
        let bounds = ctx.available_rect().shrink(8.0);
        self.layout.begin_frame(bounds);
        // Floating layers are registered before the background consumes pointer input.
        let (panel_pos, _) = self.layout.position(
            "cockpit_identity_overlay",
            bounds,
            [0.0, 0.0],
            Vec2::new(8.0, 6.0),
            Vec2::new(490.0, 120.0),
        );
        let identity_was_collapsed = self.identity_status_collapsed();
        let panel_response =
            egui::Area::new(egui::Id::new("cockpit_identity_overlay").with(self.layout.generation))
                .order(egui::Order::Foreground)
                .current_pos(panel_pos)
                .movable(!self.layout.pinned)
                .constrain_to(bounds)
                .show(ctx, |ui| {
                    self.overlay_frame(ctx).show(ui, |ui| {
                        // Keep a readable task column beside explicit expanded
                        // status on small viewports. This is temporary geometry,
                        // not a change to the saved authority-panel preference.
                        let authority_width = if self.services_open {
                            bounds.width() - 360.0 - 32.0
                        } else {
                            bounds.width() - 24.0
                        };
                        ui.set_max_width(authority_width.clamp(1.0, 490.0));
                        self.identity_header(ui);
                        if self.identity_status_collapsed() {
                            return;
                        }
                        if self.embedded {
                            ui.horizontal_wrapped(|ui| {
                                ui.strong(system_name(self.current_system()));
                                ui.colored_label(
                                    if self.connected { self.accent() } else { AMBER },
                                    if self.connected {
                                        "Connected"
                                    } else {
                                        "Offline"
                                    },
                                );
                                if in_hangar {
                                    ui.label(
                                        if self
                                            .own()
                                            .is_some_and(|pilot| pilot.location != Location::Docked)
                                        {
                                            "Ship interior"
                                        } else {
                                            "Station / interior"
                                        },
                                    );
                                }
                            });
                            self.flight_workspace_navigation(ui);
                            if !self.notice.is_empty() {
                                ui.small(&self.notice);
                            }
                            return;
                        }
                        let wrap = self.services_open;
                        let banner = |ui: &mut egui::Ui| {
                            ui.label(
                                egui::RichText::new("A R K L I N E")
                                    .size(21.0)
                                    .color(self.accent())
                                    .strong(),
                            );
                            ui.colored_label(AMBER, BUILD_VERSION.trim());
                            ui.small("EARLY ACCESS");
                            let frame_ms = self.frame_intervals.iter().sum::<f64>()
                                / self.frame_intervals.len().max(1) as f64
                                * 1000.0;
                            if frame_ms > 0.0 {
                                ui.small(format!(
                                    "{:.0} FPS · {:.1} ms",
                                    1000.0 / frame_ms,
                                    frame_ms
                                ));
                            }
                            self.session_menu(ui);
                        };
                        if wrap {
                            ui.horizontal_wrapped(banner);
                        } else {
                            ui.horizontal(banner);
                        }
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                egui::RichText::new(system_name(self.current_system())).strong(),
                            );
                            ui.colored_label(
                                if self.connected { self.accent() } else { AMBER },
                                if self.connected {
                                    "● Connected"
                                } else {
                                    "○ Offline"
                                },
                            );
                            if in_hangar {
                                let registered = self.own().and_then(|pilot| {
                                    pilot
                                        .station_avatar
                                        .and_then(|avatar| avatar.docked_traversal)
                                });
                                let interior_deck = self
                                    .own()
                                    .and_then(|p| p.station_avatar)
                                    .filter(|a| a.onboard_interior)
                                    .map(|a| a.interior_deck);
                                ui.strong(if let Some(state) = registered {
                                    station_ui::docked_state_label(&state)
                                } else if let Some(deck) = interior_deck {
                                    format!("PIONEER INTERIOR · DECK {deck}")
                                } else if self.in_captains_quarters() {
                                    "CAPTAIN'S QUARTERS".into()
                                } else {
                                    "STATION HANGAR".into()
                                });
                            }
                            self.flight_workspace_navigation(ui);
                            if !self.embedded {
                                let fullscreen = is_fullscreen(ctx);
                                if ui
                                    .small_button(if fullscreen {
                                        "Windowed (F11)"
                                    } else {
                                        "Fullscreen (F11)"
                                    })
                                    .clicked()
                                {
                                    toggle_fullscreen(ctx);
                                }
                            }
                        });
                        self.ship_connection_panel(ui);
                        let (status, detail, _, color) = self.operation_status();
                        ui.colored_label(color, egui::RichText::new(status).size(12.0).strong());
                        ui.small(detail);
                    });
                });
        self.layout.record(
            "cockpit_identity_overlay",
            bounds,
            panel_response.response.rect,
            None,
            panel_response.response.dragged() && ctx.input(|i| i.pointer.is_decidedly_dragging()),
        );
        let identity_rect = panel_response.response.rect;
        // The standalone Operations button can open the workspace while this
        // expanded identity Area is being painted. Do not create a Window with
        // a transient, tiny constraint that egui would retain as its resize size.
        let defer_workspace =
            self.services_open && !identity_was_collapsed && self.identity_status_collapsed();
        if defer_workspace {
            ctx.request_repaint();
        }
        self.controls_reference(ctx);
        // Service destinations and detailed flight controls now share the
        // deliberate task workspace, instead of competing launcher/overview panels.
        if self.services_open && !defer_workspace && self.service_tab == ServiceTab::Layouts {
            let mut open = true;
            egui::Window::new("Ship layout viewer")
                .id(egui::Id::new("ship_layout_viewer"))
                .open(&mut open)
                .default_pos(Pos2::new(
                    (bounds.right() - 675.0).max(bounds.left() + 64.0),
                    bounds.top() + 32.0,
                ))
                .default_size(Vec2::new(650.0, (bounds.height() - 80.0).max(300.0)))
                .max_width(bounds.width() - 24.0)
                .max_height(bounds.height() - 24.0)
                .constrain_to(bounds)
                .vscroll(true)
                .show(ctx, |ui| {
                    self.service_tabs(ui);
                    self.ship_layout_panel(ui);
                });
            if !open {
                self.close_operations_workspace();
            }
        }
        if self.services_open && !defer_workspace && self.service_tab != ServiceTab::Layouts {
            let mut open = true;
            self.layout.position(
                "task_workspace",
                bounds,
                [1.0, 0.5],
                Vec2::new(-16.0, 0.0),
                Vec2::new(
                    (bounds.width() * 0.52).clamp(520.0, 820.0),
                    (bounds.height() - 72.0).clamp(280.0, 680.0),
                ),
            );
            let carrier_workspace = self.carrier_workspace();
            let (panel_pos, panel_size, task_bounds) = self.layout.avoid_foreground(
                "task_workspace",
                bounds,
                identity_rect,
                (bounds.width() - 32.0).clamp(240.0, if carrier_workspace { 640.0 } else { 600.0 }),
            );
            // Physical carrier telemetry must not resize its workspace as ready
            // labels become shorter busy labels. Preserve user resizing above
            // this viewport-bounded floor; programmatic clamps are not saved drags.
            let carrier_width = 640.0_f32.min((task_bounds.width() - 20.0).max(1.0));
            let mut content_size = None;
            let panel_response = egui::Window::new(self.operations_title())
                .id(egui::Id::new("cockpit_services_overlay").with(self.layout.generation))
                .open(&mut open)
                .current_pos(panel_pos)
                .default_size(panel_size)
                .max_width((task_bounds.width() - 20.0).max(1.0))
                .resize(|resize| {
                    if self.layout.reflow {
                        resize.min_size(panel_size).max_size(panel_size)
                    } else {
                        resize
                    }
                })
                .min_width(if carrier_workspace {
                    carrier_width
                } else {
                    (task_bounds.width() - 32.0).clamp(1.0, 600.0)
                })
                .max_height((task_bounds.height() - 36.0).max(1.0))
                .resizable(!self.layout.pinned)
                .movable(!self.layout.pinned)
                .collapsible(false)
                .constrain_to(task_bounds)
                .frame(self.overlay_frame(ctx).fill(self.theme.panel()))
                .vscroll(false)
                .show(ctx, |ui| {
                    content_size = Some(ui.max_rect().size());
                    self.service_tabs(ui);
                    egui::ScrollArea::vertical()
                        .id_salt("focused_task_content")
                        .auto_shrink([false, false])
                        .min_scrolled_height(0.0)
                        .max_height(ui.available_height().max(0.0))
                        .show(ui, |ui| self.service_content(ui));
                });
            if let Some(response) = panel_response {
                if let Some(content) = content_size {
                    self.layout.observe_window_chrome(
                        "task_workspace",
                        response.response.rect,
                        content,
                    );
                }
                let resizing = layout::window_resize_dragged(ctx, response.response.layer_id);
                let moving =
                    response.response.dragged() && ctx.input(|i| i.pointer.is_decidedly_dragging());
                self.layout.record(
                    "task_workspace",
                    bounds,
                    response.response.rect,
                    content_size.filter(|_| resizing),
                    moving || resizing,
                );
            }
            self.services_open &= open;
        }
        self.hangar_view
            .set_presentation_paused(self.embedded && self.embedded_paused);
        self.hangar_view.set_space_snapshot(
            if self.own().is_some_and(|pilot| {
                pilot.location != Location::Docked
                    || pilot.flight.as_ref().is_some_and(|f| f.docking.is_some())
            }) {
                self.snapshot.clone().map(std::sync::Arc::new)
            } else {
                None
            },
        );
        self.hangar_view
            .set_docked_traversal_enabled(self.docked_navigation_present());
        self.hangar_view
            .set_parent_managed_body_hud(self.docked_navigation_present());
        self.hangar_view.set_head_look_input_allowed(
            self.embedded_input
                && !self.workspace_input_waiting_release
                && !self.controls_reference_open
                && !self.performance_open
                && !self.services_open
                && !self.cockpit_computer.wants_keyboard_input(),
        );
        self.hangar_view.set_room_layout(
            self.assets
                .as_ref()
                .and_then(|a| a.pioneer_room_layout.clone()),
        );
        self.flight_view.set_radar_paused(self.embedded_paused);
        self.flight_view.set_pioneer_layout(
            self.assets
                .as_ref()
                .and_then(|assets| assets.pioneer_room_layout.clone()),
        );
        self.flight_view.set_pioneer_room(
            self.assets
                .as_ref()
                .and_then(|assets| assets.pioneer_room.clone()),
        );
        self.physical_engineering_window(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                if in_hangar {
                    if let Some(pilot) = self.own() {
                        let now = self.presentation_time();
                        let live = self.connected
                            && self
                                .snapshot
                                .as_ref()
                                .is_some_and(|s| now.saturating_sub(s.server_time_ms) <= 1500);
                        if pilot.station_avatar.is_none() && self.docked_navigation_present() {
                            // Aboard Cockpit is the actual pilot-eye camera;
                            // exterior views are selected through ship camera modes.
                            self.hangar_view.on_foot_view =
                                flight3d::hangar::OnFootView::FirstPerson;
                        }
                        if let Some(action) = self.hangar_view.ui(
                            ui,
                            &pilot,
                            now,
                            live,
                            self.assets.as_ref().and_then(|a| a.hangar.as_ref()),
                            self.assets.as_ref().and_then(|a| a.pioneer_room.as_ref()),
                            self.snapshot.as_ref().and_then(|s| s.exterior.as_ref()),
                            self.snapshot.as_ref().map_or(now, |s| s.server_time_ms),
                            self.snapshot
                                .as_ref()
                                .and_then(|s| s.carrier_inventory_registration.as_ref()),
                        ) {
                            self.activate_quarters_terminal(action);
                        }
                        if self.verified_docked_binding().is_some() {
                            let selected_center_distance_m = self
                                .selected_position()
                                .map(|target| distance_m(motion::position(&pilot, now), target));
                            if let Some(snapshot) = self.snapshot.as_ref() {
                                self.hangar_view.paint_docked_instruments(
                                    ui,
                                    &pilot,
                                    snapshot,
                                    selected_center_distance_m,
                                    now,
                                    self.connected,
                                    self.embedded_paused,
                                );
                            }
                        }
                    }
                } else if self.view_mode != ViewMode::Tactical {
                    let now = self.presentation_time();
                    let display = self.snapshot.as_ref().map(|s| self.motion.sample(s, now));
                    if let Some(snapshot) = &display {
                        self.flight_view.set_drone_camera(self.drone_camera);
                        self.flight_view.set_camera_source(self.camera_source);
                        self.flight_view.set_drone_view(self.selected_drone, false);
                        self.flight_view
                            .set_drone_camera_mode(self.drone_camera_mode);
                        let feeds = self.own().map_or_else(Vec::new, |p| {
                            flight3d::selected_drone_feed_ids(
                                &p,
                                self.selected_drone,
                                self.drone_split,
                            )
                        });
                        self.flight_view.set_drone_feeds(&feeds);
                        self.flight_view.set_remote_ship(self.remote_ship.clone());
                        let controlled_name = self.controlled_name().to_owned();
                        if let Some(action) = self.flight_view.ui(
                            ui,
                            snapshot,
                            &controlled_name,
                            now,
                            self.connected,
                            &self.effects,
                            &self.selected_object,
                            self.view_mode,
                            &mut self.layout,
                        ) {
                            match action {
                                flight3d::ViewAction::Select(object) => {
                                    self.selected_object = object
                                }
                                flight3d::ViewAction::Command(command) => self.send(command),
                                flight3d::ViewAction::FlyTo(position) => {
                                    if self.can_maneuver() {
                                        self.send(Request::Navigate {
                                            mode: NavigationMode::FlyTo { position },
                                        });
                                    }
                                }
                            }
                        }
                    }
                } else {
                    self.scene(ui);
                }
            });
        self.first_person_hud(ctx);
        if !self.first_person_capture_allowed(ctx) {
            self.hangar_view.release_first_person_mouse(ctx);
        }
        performance_overlay::show(
            ctx,
            &mut self.performance_open,
            performance_overlay::PerformanceSamples {
                frame_intervals_s: &self.frame_intervals,
                snapshot_age_ms: self
                    .snapshot
                    .as_ref()
                    .and(self.performance_snapshot_received)
                    .map(|received| received.elapsed().as_millis().min(u64::MAX as u128) as u64),
                connected: self.connected,
                paused: self.embedded && self.embedded_paused,
                session: if self.embedded {
                    "Embedded local session"
                } else {
                    "Remote server session"
                },
                server_tick: self.performance_server_tick,
            },
        );
        self.render_cockpit_computer(ctx, in_hangar);
        if let Err(error) = self
            .layout
            .finish_frame(ctx.input(|i| i.pointer.any_down()))
        {
            self.settings_notice = error;
        }
        if self.view_mode != ViewMode::Cockpit {
            self.pilot_input.release_mouse(ctx);
        }
        if self.performance_open
            || self.controls_reference_open
            || self.services_open
            || self.service_tab == ServiceTab::Layouts
        {
            self.release_workspace_input();
        }
        let workspace_allows_input = !self.performance_open
            && !self.controls_reference_open
            && !self.services_open
            && self.service_tab != ServiceTab::Layouts
            && !self.workspace_input_waiting_release;
        let flight_allowed = workspace_allows_input
            && !ctx.input(|i| i.pointer.primary_down())
            && self.view_mode != ViewMode::Tactical
            && self.can_maneuver()
            && (self.manual_remote.is_none() || self.controlled_remote().is_some())
            && self.own().is_some_and(|p| {
                p.station_avatar.is_none()
                    && p.flight.is_some()
                    && p.flight
                        .as_ref()
                        .and_then(|f| f.docking.as_ref())
                        .is_none_or(|d| !d.phase.busy())
                    && matches!(p.location, Location::Station | Location::Belt)
            });
        if self.own().is_some_and(|p| p.station_avatar.is_some()) {
            // Discard flight release state without erasing a queued StationWalk stop.
            let _ = self.pilot_input.update(false, Default::default());
        }
        if let Some(control) = self.pilot_input.sample(
            ctx,
            flight_allowed
                && !self.embedded_paused
                && self.embedded_input
                && !self.cockpit_computer.wants_keyboard_input(),
            if self.view_mode == ViewMode::Cockpit {
                self.flight_view.input_surface()
            } else {
                None
            },
        ) {
            self.cancel_ai();
            self.controls
                .publish_for(control, self.controlled_remote().map(str::to_owned));
        }
        self.sample_station_walk(
            ctx,
            in_hangar && workspace_allows_input && !self.embedded_paused && self.embedded_input,
        );
        self.record_operations_keyboard_owner(ctx);
        self.boot.paint_handoff(ctx);
    }
}
/// IME placement is the only window-space projection of the computer UI.
/// All content rendering stays on the physical 3D display plane.
fn project_computer_ime(
    surface: &flight3d::CockpitSurface,
    ime: egui::output::IMEOutput,
) -> Option<egui::output::IMEOutput> {
    let project = |rect: egui::Rect| {
        if !rect.is_finite() || rect.width() < 0.0 || rect.height() < 0.0 {
            return None;
        }
        let corners = [
            surface.project(rect.left_top())?,
            surface.project(rect.right_top())?,
            surface.project(rect.right_bottom())?,
            surface.project(rect.left_bottom())?,
        ];
        let projected = egui::Rect::from_points(&corners);
        projected.is_finite().then_some(projected)
    };
    Some(egui::output::IMEOutput {
        rect: project(ime.rect)?,
        cursor_rect: project(ime.cursor_rect)?,
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_native() -> eframe::Result<()> {
    let bundled = std::env::current_exe()
        .ok()
        .is_some_and(|path| path.parent().is_some_and(|p| p.ends_with("Contents/MacOS")));
    let mut local = false;
    let mut remote_only = false;
    let mut explicit_server = false;
    let mut server = transport::OFFICIAL_SERVER.to_string();
    let mut data_dir = if bundled {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Library/Application Support/Arkline/Preview")
    } else {
        PathBuf::from("data/client")
    };
    let mut credential_scope = None;
    let mut startup_pilot = None;
    let mut oauth_config = None;
    let mut oauth_config_path = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("Arkline {}", BUILD_VERSION.trim());
                return Ok(());
            }
            "--help" | "-h" => {
                println!("Arkline native client\n\nUsage: arkline-client [--local | --remote-only] [--server WSS_URL_OR_HOST:PORT] [--data-dir PATH] [--pilot NAME]\n\n--pilot creates or resumes the named pilot immediately.\n--local starts a sibling arkline-server on a private loopback port.\nThe macOS bundle saves in ~/Library/Application Support/Arkline/Preview.\n--server selects an existing server; --remote-only disables an explicit --local option.\nDefault server: wss://relay.arkline.network/ws (Production). CLI storage: data/client.\n--credential-scope NAME selects a stable credential namespace (used by local preview cockpits).\n--oauth-config PATH enables operator-configured browser sign-in for one exact WSS endpoint.\nResume credentials are private local files. WSS verifies TLS certificates. Plain TCP is retained for local development.");
                return Ok(());
            }
            "--oauth-config" => {
                let path = args.next().unwrap_or_else(|| {
                    eprintln!("--oauth-config requires PATH");
                    std::process::exit(2)
                });
                oauth_config_path = Some(std::fs::canonicalize(&path).unwrap_or_else(|_| {
                    eprintln!("Cannot resolve OAuth configuration path");
                    std::process::exit(2)
                }));
                oauth_config = Some(
                    account_login::Config::load(std::path::Path::new(&path)).unwrap_or_else(|e| {
                        eprintln!("{e}");
                        std::process::exit(2)
                    }),
                );
            }
            "--local" => local = true,
            "--remote-only" => remote_only = true,
            "--server" => {
                explicit_server = true;
                server = args.next().unwrap_or_else(|| {
                    eprintln!("--server requires HOST:PORT");
                    std::process::exit(2)
                });
            }
            "--data-dir" => {
                data_dir = PathBuf::from(args.next().unwrap_or_else(|| {
                    eprintln!("--data-dir requires PATH");
                    std::process::exit(2)
                }))
            }
            "--pilot" => {
                startup_pilot = Some(args.next().unwrap_or_else(|| {
                    eprintln!("--pilot requires NAME");
                    std::process::exit(2)
                }));
            }
            "--credential-scope" => {
                credential_scope = Some(args.next().unwrap_or_else(|| {
                    eprintln!("--credential-scope requires NAME");
                    std::process::exit(2)
                }))
            }
            _ => {
                eprintln!("Unknown argument: {arg}. Use --help.");
                std::process::exit(2);
            }
        }
    }
    if oauth_config.is_some() && (local || credential_scope.is_some()) {
        eprintln!("Account sign-in cannot be combined with a local credential scope");
        std::process::exit(2);
    }
    if let Some(config) = &oauth_config {
        if explicit_server && server != config.server {
            eprintln!("Server must match OAuth configuration");
            std::process::exit(2);
        }
        server = config.server.clone();
    }
    local = local && !remote_only && !explicit_server;
    let mut startup_error = None;
    let local_server = if local {
        match start_local_server(&data_dir) {
            Ok((guard, address)) => {
                server = address;
                credential_scope = Some("local-preview".into());
                Some(guard)
            }
            Err(error) => {
                server.clear();
                startup_error = Some(error);
                None
            }
        }
    } else {
        None
    };
    let options = eframe::NativeOptions {
        multisampling: 4,
        depth_buffer: 24,
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("Arkline — {}", BUILD_VERSION.trim()))
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!(
                    "../../../assets/Arkline-256.png"
                ))
                .expect("embedded Arkline icon must be valid PNG"),
            )
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([900.0, 650.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Arkline",
        options,
        Box::new(move |cc| {
            let mut cockpit = Cockpit::new(cc, server, data_dir, credential_scope);
            cockpit.account_login.config = oauth_config;
            cockpit.oauth_config_path = oauth_config_path;
            cockpit.start_audio_output();
            cockpit.local_server = local_server;
            if let Some(error) = startup_error {
                cockpit.notice = error;
            } else if let Some(pilot) = startup_pilot {
                if !pilot.trim().is_empty() {
                    cockpit.pilot = pilot;
                    cockpit.connect();
                }
            }
            Ok(Box::new(cockpit))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn remote_view_takeover_and_return_never_change_player_identity() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let mut worker = cockpit.snapshot.as_ref().unwrap().pilots[0].clone();
        worker.name = "worker".into();
        cockpit.snapshot.as_mut().unwrap().pilots.push(worker);
        cockpit
            .snapshot
            .as_mut()
            .unwrap()
            .fleet_links
            .push(FleetLink {
                pilot: "worker".into(),
                controller: "pilot".into(),
            });
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        cockpit.remote_ship = Some("worker".into());
        assert_eq!(
            cockpit.own().unwrap().name,
            "pilot",
            "watching never changes controls"
        );
        cockpit.take_remote_control("worker".into());
        assert!(
            matches!(rx.try_recv().unwrap(), Request::BeginRemoteControl { pilot } if pilot == "worker")
        );
        assert_eq!(
            cockpit.own().unwrap().name,
            "pilot",
            "unconfirmed takeover cannot route controls"
        );
        cockpit
            .snapshot
            .as_mut()
            .unwrap()
            .remote_controls
            .push(RemoteControlLease {
                pilot: "worker".into(),
                controller: "pilot".into(),
            });
        cockpit.request_pending = false;
        assert_eq!(cockpit.own().unwrap().name, "worker");
        cockpit.send(Request::Undock);
        assert!(
            matches!(rx.try_recv().unwrap(), Request::RemoteControl { pilot, request } if pilot == "worker" && matches!(*request,Request::Undock))
        );
        cockpit.return_to_my_ship();
        assert!(matches!(rx.try_recv().unwrap(), Request::EndRemoteControl));
        assert_eq!(cockpit.pilot, "pilot");
        assert_eq!(cockpit.own().unwrap().name, "pilot");
        assert!(cockpit.remote_ship.is_none() && cockpit.manual_remote.is_none());
        assert!(cockpit.controls.is_empty());
    }

    #[test]
    fn remote_control_blocks_account_actions_and_revocation_restores_player() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.manual_remote = Some("worker".into());
        cockpit
            .snapshot
            .as_mut()
            .unwrap()
            .remote_controls
            .push(RemoteControlLease {
                pilot: "worker".into(),
                controller: "pilot".into(),
            });
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        cockpit.send(Request::DepositOre { units: 10 });
        assert!(rx.try_recv().is_err());
        assert!(cockpit.notice.contains("Return to your own ship"));
        cockpit.snapshot.as_mut().unwrap().remote_controls.clear();
        cockpit.sync_remote_control();
        assert!(cockpit.manual_remote.is_none());
        assert_eq!(cockpit.own().unwrap().name, "pilot");
    }

    #[test]
    fn voice_order_is_discarded_when_ship_session_or_freshness_changes() {
        let ctx = egui::Context::default();
        for changed in 0..3 {
            let mut cockpit = view_test_cockpit(&ctx);
            cockpit.voice_pin = Some((cockpit.pilot.clone(), cockpit.connection_generation));
            match changed {
                0 => cockpit.connection_generation += 1,
                1 => cockpit.resource_pilot = Some("different-worker".into()),
                _ => {
                    cockpit.timeline_origin = Some(1000);
                    cockpit.observed = Instant::now() - Duration::from_secs(3);
                }
            }
            cockpit.poll_voice();
            assert!(cockpit.voice_pin.is_none());
            assert!(cockpit.ai_pin.is_none());
            assert!(cockpit.voice_transcript.contains("cancelled"));
        }
    }

    #[test]
    fn manual_takeover_cancels_pending_voice_order() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.voice_pin = Some((cockpit.pilot.clone(), cockpit.connection_generation));
        cockpit.cancel_ai();
        assert!(cockpit.voice_pin.is_none());
        assert!(!cockpit.voice.is_listening());
        assert!(!cockpit.spoken_replies);
    }

    #[test]
    fn mining_panel_is_readable_on_foot_and_start_remains_disabled() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.service_tab = ServiceTab::ShipComputer;
        let pilot = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        pilot.location = Location::Docked;
        pilot.station_avatar = Some(StationAvatar {
            sprinting: false,
            docked_traversal: None,
            onboard_interior: false,
            interior_deck: 1,
            jump_started_ms: None,
            seated: false,
            position: hangar_layout(pilot.ship_class).boarding_point,
            heading: 0.,
            moving: false,
            updated_at_ms: 0,
        });
        assert!(cockpit.service_visible(ServiceTab::ShipComputer));
        assert!(cockpit
            .resource_start_reason()
            .unwrap()
            .contains("Return to the pilot seat"));
        let before = serde_json::to_value(&cockpit.snapshot).unwrap();
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let mut target = None;
        let mut labels = Vec::new();
        for pass in 0..4 {
            let events = match (pass, target) {
                (2, Some(pos)) => vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ],
                (3, Some(pos)) => vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
                _ => vec![],
            };
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(900., 1200.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| cockpit.service_content(ui));
                },
            );
            labels.clear();
            for shape in output.shapes {
                if let egui::Shape::Text(text) = shape.shape {
                    labels.push(text.galley.text().to_owned());
                    if text.galley.text() == "Start mining routine" {
                        target = Some(text.pos + text.galley.size() * 0.5);
                    }
                }
            }
        }
        assert!(target.is_some());
        assert!(labels.iter().any(|text| text == "Mining operations"));
        assert!(labels
            .iter()
            .any(|text| text.contains("Status remains visible on foot")));
        assert!(
            rx.try_recv().is_err(),
            "Disabled Start must not send a request"
        );
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
    }

    #[test]
    fn resource_job_requires_explicit_worker_permission_and_ready_ship() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let snapshot = cockpit.snapshot.as_mut().unwrap();
        snapshot.pilots[0].location = Location::Docked;
        snapshot.pilots[0].ore = 0;
        let mut worker = snapshot.pilots[0].clone();
        worker.name = "worker".into();
        snapshot.pilots.push(worker);
        assert!(cockpit.resource_start_reason().is_none());
        cockpit.resource_pilot = Some("worker".into());
        assert!(cockpit.resource_start_reason().is_some());
        cockpit
            .snapshot
            .as_mut()
            .unwrap()
            .fleet_links
            .push(FleetLink {
                pilot: "worker".into(),
                controller: "pilot".into(),
            });
        assert!(cockpit.resource_start_reason().is_none());
        cockpit.snapshot.as_mut().unwrap().pilots[1].ore = 1;
        assert!(cockpit.resource_start_reason().is_none());
        cockpit.snapshot.as_mut().unwrap().pilots[1].location = Location::Belt;
        assert!(cockpit.resource_start_reason().is_none());
        cockpit.snapshot.as_mut().unwrap().pilots[1].location = Location::InTransit {
            destination: Destination::Belt,
            arrives_at_ms: 5000,
        };
        assert!(cockpit.resource_start_reason().is_none());
        cockpit.snapshot.as_mut().unwrap().pilots[1].location = Location::Jumping {
            gate_id: 1,
            arrives_at_ms: 5000,
        };
        assert!(cockpit
            .resource_start_reason()
            .unwrap()
            .contains("gate transit"));
        cockpit.snapshot.as_mut().unwrap().pilots[1].ore = 0;
        cockpit.timeline_origin = Some(1000);
        cockpit.observed = Instant::now() - Duration::from_secs(3);
        assert!(cockpit.resource_start_reason().unwrap().contains("stale"));
        cockpit.ai_pin = Some(("worker".into(), cockpit.connection_generation));
        cockpit.poll_ship_ai();
        assert!(cockpit.ai_pin.is_none());
        cockpit.connected = false;
        assert!(cockpit.resource_start_reason().is_some());
    }

    #[test]
    fn resource_job_pointer_start_routes_selected_worker_and_stale_ai_is_cancelled() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let snapshot = cockpit.snapshot.as_mut().unwrap();
        snapshot.pilots[0].location = Location::Docked;
        snapshot.pilots[0].ore = 0;
        let mut worker = snapshot.pilots[0].clone();
        worker.name = "worker".into();
        snapshot.pilots.push(worker);
        snapshot.fleet_links.push(FleetLink {
            pilot: "worker".into(),
            controller: "pilot".into(),
        });
        cockpit.resource_pilot = Some("worker".into());
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let mut target = None;
        assert!(rx.try_recv().is_err());
        for pass in 0..4 {
            let events = match (pass, target) {
                (2, Some(pos)) => vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ],
                (3, Some(pos)) => vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
                _ => vec![],
            };
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(900., 1200.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| cockpit.resource_job_panel(ui));
                },
            );
            for shape in output.shapes {
                if let egui::Shape::Text(text) = shape.shape {
                    if text.galley.text() == "Start mining routine" {
                        target = Some(text.pos + text.galley.size() * 0.5);
                    }
                }
            }
        }
        assert!(
            matches!(rx.try_recv().unwrap(),Request::StartResourceJob {target_units,pilot:Some(name)} if name=="worker" && target_units == cargo_capacity(ShipClass::Pioneer))
        );
        cockpit.ai_pin = Some(("worker".into(), cockpit.connection_generation));
        cockpit.resource_pilot = None;
        cockpit.poll_ship_ai();
        assert!(cockpit.ai_pin.is_none());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn physical_ai_console_scrolls_prompt_into_clipped_interactive_screen() {
        crate::cockpit_computer_integration_tests::ai_console_input_journey();
    }

    #[test]
    fn tactical_cursor_zoom_preserves_anchor_and_bounds_span() {
        let rect = egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 700.0));
        let point = Pos2::new(700.0, 400.0);
        let base = [AU_METERS, 0.0, -AU_METERS];
        let mut pan = [0.0; 2];
        let mut zoom = 1.0;
        let before = map_unproject(rect, point, base, 32000.0, 0.0);
        tactical_zoom_at(rect, point, base, 32000.0, &mut pan, &mut zoom, 120.0);
        let after = map_unproject(
            rect,
            point,
            [base[0] + pan[0], 0.0, base[2] + pan[1]],
            32000.0 * zoom,
            0.0,
        );
        assert!((before[0] - after[0]).abs() < 0.001 && (before[2] - after[2]).abs() < 0.001);
        tactical_zoom_at(rect, point, base, 32000.0, &mut pan, &mut zoom, 1e6);
        assert_eq!(32000.0 * zoom, 100.0);
        tactical_zoom_at(rect, point, base, 32000.0, &mut pan, &mut zoom, -1e6);
        assert_eq!(32000.0 * zoom, AU_METERS * 100.0);
    }

    #[test]
    fn tactical_empty_drag_pans_without_selecting_or_sending_navigation() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.view_mode = ViewMode::Tactical;
        let original = cockpit.selected_object.clone();
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let start = Pos2::new(250.0, 150.0);
        let end = start + Vec2::new(70.0, 40.0);
        for events in [
            vec![],
            vec![],
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            vec![egui::Event::PointerMoved(start + Vec2::new(20.0, 10.0))],
            vec![egui::Event::PointerMoved(end)],
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(900.0, 650.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| cockpit.scene(ui));
                },
            );
        }
        assert!(cockpit.tactical_pan[0] < 0.0 && cockpit.tactical_pan[1] < 0.0);
        assert!(cockpit.selected_object == original);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn cockpit_control_bank_keeps_following_widget_ids_between_compact_and_detail() {
        let ctx = egui::Context::default();
        let mut app = view_test_cockpit(&ctx);
        let mut ids = Vec::new();
        for detail in [false, true] {
            app.cockpit_console_detail = detail;
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(560., 380.))),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.cockpit_actions(ui);
                        ids.push((
                            ui.next_auto_id(),
                            ui.button("Following control sentinel").id,
                        ));
                    });
                },
            );
        }
        assert_eq!(
            ids[0], ids[1],
            "Omitting compact help must preserve subsequent control IDs"
        );
    }

    #[test]
    fn cockpit_control_bank_preserves_commands_and_blocks_unready_orders() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.snapshot.as_mut().unwrap().pilots[0].location = Location::Docked;
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let render = |c: &mut Cockpit, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(560.0, 380.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| c.cockpit_actions(ui));
                },
            )
        };
        let output = render(&mut cockpit, vec![]);
        let point = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "UNDOCK" => {
                    Some(text.pos + text.galley.rect.center().to_vec2())
                }
                _ => None,
            })
            .unwrap();
        let click = |c: &mut Cockpit| {
            for pressed in [true, false] {
                render(
                    c,
                    vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        };
        let before = serde_json::to_value(&cockpit.snapshot).unwrap();
        click(&mut cockpit);
        assert!(matches!(rx.try_recv(), Ok(Request::Undock)));
        assert!(cockpit.request_pending);
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
        for condition in [0, 1, 2, 3, 4] {
            cockpit.request_pending = false;
            cockpit.connected = true;
            cockpit.embedded_paused = false;
            cockpit.observed = Instant::now();
            cockpit.snapshot.as_mut().unwrap().pilots[0].station_avatar = None;
            match condition {
                0 => cockpit.request_pending = true,
                1 => cockpit.connected = false,
                2 => cockpit.embedded_paused = true,
                3 => cockpit.observed = Instant::now() - std::time::Duration::from_secs(2),
                _ => cockpit.snapshot.as_mut().unwrap().pilots[0].station_avatar = Some(serde_json::from_value(serde_json::json!({"position":[0,0,0],"heading":0,"moving":false,"updated_at_ms":1000})).unwrap()),
            }
            click(&mut cockpit);
            assert!(
                rx.try_recv().is_err(),
                "unready condition {condition} dispatched"
            );
        }
    }
    #[test]
    fn physical_workspace_keys_select_only_the_displayed_local_workspace() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let authority = serde_json::to_value(&cockpit.snapshot).unwrap();
        let render = |c: &mut Cockpit, events: Vec<egui::Event>, enabled: bool| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(560.0, 380.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            if !enabled {
                                ui.disable();
                            }
                            c.cockpit_workspace_keys(ui);
                        });
                },
            )
        };
        for (label, tab, x) in [
            ("FLIGHT", ServiceTab::Navigation, 96.0),
            ("DRONES", ServiceTab::Drones, 280.0),
            ("MINING", ServiceTab::ShipComputer, 464.0),
        ] {
            cockpit.services_open = false;
            let out = render(&mut cockpit, vec![], true);
            let center = out
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == label => {
                        Some(text.pos + text.galley.rect.center().to_vec2())
                    }
                    _ => None,
                })
                .unwrap();
            assert!((center.x - x).abs() < 1.0 && (center.y - 352.0).abs() < 1.0);
            for enabled in [false, true] {
                cockpit.services_open = false;
                render(&mut cockpit, vec![], enabled);
                for pressed in [true, false] {
                    render(
                        &mut cockpit,
                        vec![
                            egui::Event::PointerMoved(center),
                            egui::Event::PointerButton {
                                pos: center,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                        enabled,
                    );
                }
                assert_eq!(cockpit.services_open, enabled, "{label} eligibility");
                if enabled {
                    assert!(
                        cockpit.service_tab == tab,
                        "{label} opened a different workspace"
                    );
                }
                assert!(
                    rx.try_recv().is_err(),
                    "Workspace key issued a ship command"
                );
                assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), authority);
            }
        }
    }
    #[test]
    fn compact_console_long_phase_missing_speed_and_viewport_stress() {
        for size in [Vec2::new(800., 650.), Vec2::new(1500., 638.)] {
            for missing_speed in [false, true] {
                let ctx = egui::Context::default();
                let mut app = view_test_cockpit(&ctx);
                if missing_speed {
                    app.snapshot.as_mut().unwrap().pilots[0].flight = None;
                }
                app.snapshot.as_mut().unwrap().resource_jobs = vec![ResourceJob {
                    drone_cargo: None,
                    controller: "pilot".into(),
                    pilot: "pilot".into(),
                    id: 1,
                    target_units: 5000,
                    mined_units: 5000,
                    stored_units: 1200,
                    target_id: None,
                    phase: ResourceJobPhase::ReturningToStation,
                    message: String::new(),
                    started_at_ms: 1000,
                    updated_at_ms: 1000,
                }];
                let before = serde_json::to_value(&app.snapshot).unwrap();
                for frame in 0..4 {
                    app.observed = Instant::now();
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, size)),
                            time: Some(frame as f64 / 60.),
                            ..Default::default()
                        },
                        |ctx| {
                            eframe::App::update(&mut app, ctx, &mut eframe::Frame::_new_kittest())
                        },
                    );
                }
                let canvas = egui::Rect::from_min_size(
                    Pos2::ZERO,
                    flight3d::cockpit_computer::COMPUTER_CANVAS_SIZE,
                );
                let text: Vec<_> = app
                    .cockpit_computer
                    .last_shapes
                    .iter()
                    .filter_map(|shape| {
                        if let egui::Shape::Text(t) = &shape.shape {
                            Some((shape, t))
                        } else {
                            None
                        }
                    })
                    .collect();
                assert!(text
                    .iter()
                    .any(|(_, t)| t.galley.job.text == "Returning to station"));
                assert!(text
                    .iter()
                    .any(|(_, t)| t.galley.job.text == "1200 / 5000 ore"));
                assert!(text.iter().any(|(_, t)| t.galley.job.text
                    == "Manual ship orders can cancel the active mining routine"));
                assert!(text.iter().any(|(_, t)| t.galley.job.text
                    == if missing_speed {
                        "Unavailable"
                    } else {
                        "0 m/s"
                    }));
                for (index, (shape, t)) in text.iter().enumerate() {
                    let ink = t.visual_bounding_rect();
                    assert!(
                        canvas.contains_rect(ink) && shape.clip_rect.contains_rect(ink),
                        "Clipped {} at {size:?}",
                        t.galley.job.text
                    );
                    if t.galley.job.text == "Expand panel" {
                        assert!(
                            ink.height() <= 24.,
                            "Expand panel must stay in its fixed one-line control face"
                        );
                    }
                    for (_, other) in &text[index + 1..] {
                        assert!(
                            !ink.shrink(0.5)
                                .intersects(other.visual_bounding_rect().shrink(0.5)),
                            "Text overlap: {} / {} at {size:?}",
                            t.galley.job.text,
                            other.galley.job.text
                        );
                    }
                }
                assert_eq!(serde_json::to_value(&app.snapshot).unwrap(), before);
            }
        }
    }

    #[test]
    fn compact_flight_summary_actual_canvas_fits_and_selects_current_owned_routine() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let job = |id, phase, updated_at_ms, stored_units| ResourceJob {
            drone_cargo: None,
            controller: "pilot".into(),
            pilot: "pilot".into(),
            id,
            target_units: 5000,
            mined_units: stored_units,
            stored_units,
            target_id: None,
            phase,
            message: String::new(),
            started_at_ms: 1000,
            updated_at_ms,
        };
        cockpit.snapshot.as_mut().unwrap().resource_jobs = vec![
            job(1, ResourceJobPhase::Completed, 3000, 5000),
            job(2, ResourceJobPhase::Mining, 2000, 1200),
        ];
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let before = serde_json::to_value(&cockpit.snapshot).unwrap();
        for frame in 0..4 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(1280.0, 800.0),
                    )),
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |ctx| eframe::App::update(&mut cockpit, ctx, &mut eframe::Frame::_new_kittest()),
            );
        }
        let canvas =
            egui::Rect::from_min_size(Pos2::ZERO, flight3d::cockpit_computer::COMPUTER_CANVAS_SIZE);
        let labels: Vec<_> = cockpit
            .cockpit_computer
            .last_shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    let bounds = text.visual_bounding_rect();
                    assert!(
                        canvas.contains_rect(bounds),
                        "canvas clips {:?}: {:?}",
                        text.galley.job.text,
                        bounds
                    );
                    assert!(
                        shape.clip_rect.contains_rect(bounds),
                        "widget clips {:?}",
                        text.galley.job.text
                    );
                    if text.galley.job.text == "DETAILS" {
                        assert!(
                            bounds.bottom() <= 324.0,
                            "Details label overlaps physical key row"
                        );
                    }
                    Some(text.galley.job.text.as_str())
                } else {
                    None
                }
            })
            .collect();
        for label in [
            "FLIGHT CONSOLE",
            "SPEED",
            "Braking / holding",
            "Routine",
            "Mining",
            "Delivered",
            "1200 / 5000 ore",
            "Hold 25 / 1000 ore · 0 drones",
            "FLIGHT",
            "DRONES",
            "MINING",
            "DETAILS",
        ] {
            assert!(labels.contains(&label), "missing {label:?}: {labels:?}");
        }
        assert!(!labels.contains(&"5000 / 5000 ore"));
        assert!(rx.try_recv().is_err());
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
        cockpit.snapshot.as_mut().unwrap().resource_jobs[1].phase = ResourceJobPhase::Completed;
        // A newer terminal result wins when no active routine remains.
        let authority = serde_json::to_value(&cockpit.snapshot).unwrap();
        let summary_ctx = egui::Context::default();
        let output = summary_ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| cockpit.compact_flight_summary(ui));
        });
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "5000 / 5000 ore")));
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), authority);
    }

    #[test]
    fn task_workspace_clears_measured_authority_and_keeps_real_controls_reachable() {
        fn frame(
            ctx: &egui::Context,
            app: &mut Cockpit,
            size: Vec2,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            app.observed = Instant::now();
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, size)),
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ctx| eframe::App::update(app, ctx, &mut eframe::Frame::_new_kittest()),
            )
        }
        fn label(output: &egui::FullOutput, scope: egui::Rect, wanted: &str) -> Pos2 {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == wanted => {
                        let rect = text.galley.rect.translate(text.pos.to_vec2());
                        (scope.contains_rect(rect) && shape.clip_rect.contains_rect(rect))
                            .then_some(rect.center())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("fully visible {wanted:?} missing in {scope:?}"))
        }
        fn click(ctx: &egui::Context, app: &mut Cockpit, size: Vec2, point: Pos2) {
            frame(ctx, app, size, vec![egui::Event::PointerMoved(point)]);
            for pressed in [true, false] {
                frame(
                    ctx,
                    app,
                    size,
                    vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        }
        for size in [Vec2::new(900., 650.), Vec2::new(800., 600.)] {
            for lower in [false, true] {
                let ctx = egui::Context::default();
                ctx.options_mut(|options| options.max_passes = std::num::NonZeroUsize::MIN);
                let profile = std::env::temp_dir().join(format!(
                    "arkline-authority-clearance-{}-{}-{}",
                    std::process::id(),
                    size.x,
                    lower
                ));
                let _ = std::fs::remove_dir_all(&profile);
                std::fs::create_dir_all(&profile).unwrap();
                if lower {
                    std::fs::write(profile.join("layout.json"), serde_json::to_vec(&serde_json::json!({
                        "panels":{"cockpit_identity_overlay":{"anchor":[0.,0.],"offset":[8.,220.],"size":[490.,120.]}}
                    })).unwrap()).unwrap();
                }
                let saved_layout = std::fs::read(profile.join("layout.json")).ok();
                let mut app = view_test_cockpit_with_profile(&ctx, profile.clone());
                app.identity_collapsed = true;
                app.services_open = true;
                app.service_tab = ServiceTab::Navigation;
                let before = serde_json::to_value(&app.snapshot).unwrap();
                let (tx, rx) = mpsc::channel();
                app.commands = Some(tx);
                let identity_id =
                    egui::Id::new("cockpit_identity_overlay").with(app.layout.generation);
                let task_id = egui::Id::new("cockpit_services_overlay").with(app.layout.generation);
                for expanded in [false, true, false] {
                    let mut output = frame(&ctx, &mut app, size, vec![]);
                    for _ in 0..5 {
                        output = frame(&ctx, &mut app, size, vec![]);
                    }
                    let mut settled = None;
                    for _ in 0..5 {
                        output = frame(&ctx, &mut app, size, vec![]);
                        let identity = ctx.memory(|m| m.area_rect(identity_id)).unwrap();
                        let task = ctx.memory(|m| m.area_rect(task_id)).unwrap();
                        assert!(
                            egui::Rect::from_min_size(Pos2::ZERO, size).contains_rect(task),
                            "task chrome outside viewport: size={size:?}, lower={lower}, expanded={expanded}, identity={identity:?}, task={task:?}"
                        );
                        assert!(!task.intersects(identity), "authority overlaps task: size={size:?}, lower={lower}, expanded={expanded}, identity={identity:?}, task={task:?}");
                        if let Some(previous) = settled {
                            assert_eq!(task, previous, "workspace oscillated");
                        }
                        settled = Some(task);
                        label(&output, task, "Flight · Navigation");
                        label(&output, task, "Back to Flight");
                        label(&output, task, "Search panels");
                    }
                    let identity = ctx.memory(|m| m.area_rect(identity_id)).unwrap();
                    let point = label(
                        &output,
                        identity,
                        if expanded {
                            "Hide status"
                        } else {
                            "Show status"
                        },
                    );
                    click(&ctx, &mut app, size, point);
                    assert_eq!(app.identity_collapsed, expanded);
                }
                // End collapsed, then type through the actual visible search field.
                app.identity_collapsed = true;
                let mut output = frame(&ctx, &mut app, size, vec![]);
                for _ in 0..5 {
                    output = frame(&ctx, &mut app, size, vec![]);
                }
                let task = ctx.memory(|m| m.area_rect(task_id)).unwrap();
                click(
                    &ctx,
                    &mut app,
                    size,
                    label(&output, task, "Mining, cargo, settings…"),
                );
                assert!(ctx.wants_keyboard_input());
                output = frame(
                    &ctx,
                    &mut app,
                    size,
                    vec![egui::Event::Text("cargo".into())],
                );
                assert_eq!(app.operations_search, "cargo");
                click(&ctx, &mut app, size, label(&output, task, "Back to Flight"));
                assert!(!app.services_open);
                app.services_open = true;
                app.operations_search.clear();
                for _ in 0..5 {
                    frame(&ctx, &mut app, size, vec![]);
                }
                let task = ctx.memory(|m| m.area_rect(task_id)).unwrap();
                click(
                    &ctx,
                    &mut app,
                    size,
                    Pos2::new(task.right() - 14., task.top() + 18.),
                );
                assert!(
                    !app.services_open,
                    "real title Close must dismiss workspace: size={size:?}, lower={lower}, task={task:?}"
                );
                assert_eq!(serde_json::to_value(&app.snapshot).unwrap(), before);
                assert!(
                    rx.try_recv().is_err(),
                    "workspace inspection emitted gameplay"
                );
                eframe::App::on_exit(&mut app, None);
                assert_eq!(
                    std::fs::read(profile.join("layout.json")).ok(),
                    saved_layout,
                    "automatic clearance must not overwrite saved positions"
                );
                let _ = std::fs::remove_dir_all(profile);
            }
        }
    }

    #[test]
    fn task_and_authority_deliberate_move_resize_still_save() {
        fn frame(ctx: &egui::Context, app: &mut Cockpit, events: Vec<egui::Event>) {
            app.observed = Instant::now();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 650.))),
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ctx| eframe::App::update(app, ctx, &mut eframe::Frame::_new_kittest()),
            );
        }
        fn drag(
            ctx: &egui::Context,
            app: &mut Cockpit,
            from: Pos2,
            to: Pos2,
            resize: Option<egui::LayerId>,
        ) {
            frame(ctx, app, vec![egui::Event::PointerMoved(from)]);
            frame(
                ctx,
                app,
                vec![egui::Event::PointerButton {
                    pos: from,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            frame(
                ctx,
                app,
                vec![egui::Event::PointerMoved(from.lerp(to, 0.5))],
            );
            frame(ctx, app, vec![egui::Event::PointerMoved(to)]);
            if let Some(layer) = resize {
                assert!(
                    layout::window_resize_dragged(ctx, layer),
                    "actual resize handle must be recognized"
                );
            }
            frame(
                ctx,
                app,
                vec![egui::Event::PointerButton {
                    pos: to,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            for _ in 0..6 {
                frame(ctx, app, vec![]);
            }
        }
        let profile = std::env::temp_dir().join(format!(
            "arkline-clearance-user-drags-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&profile);
        let ctx = egui::Context::default();
        ctx.options_mut(|options| options.max_passes = std::num::NonZeroUsize::MIN);
        let mut app = view_test_cockpit_with_profile(&ctx, profile.clone());
        app.identity_collapsed = true;
        app.services_open = true;
        app.service_tab = ServiceTab::Navigation;
        let task_id = egui::Id::new("cockpit_services_overlay").with(app.layout.generation);
        let identity_id = egui::Id::new("cockpit_identity_overlay").with(app.layout.generation);
        for _ in 0..8 {
            frame(&ctx, &mut app, vec![]);
        }
        let before = ctx.memory(|m| m.area_rect(task_id)).unwrap();
        let corner = before.right_bottom() - Vec2::splat(2.);
        drag(
            &ctx,
            &mut app,
            corner,
            corner - Vec2::new(0., 100.),
            Some(egui::LayerId::new(egui::Order::Middle, task_id)),
        );
        let resized = ctx.memory(|m| m.area_rect(task_id)).unwrap();
        assert!(
            resized.height() < before.height() - 40.,
            "deliberate resize must change height"
        );
        let title = Pos2::new(resized.left() + 180., resized.top() + 18.);
        drag(&ctx, &mut app, title, title + Vec2::new(-60., 12.), None);
        let moved = ctx.memory(|m| m.area_rect(task_id)).unwrap();
        assert!(
            moved.min.distance(resized.min) > 20.,
            "deliberate title move must remain available"
        );
        let identity = ctx.memory(|m| m.area_rect(identity_id)).unwrap();
        let handle = identity.left_top() + Vec2::splat(4.);
        drag(&ctx, &mut app, handle, handle + Vec2::new(20., 150.), None);
        let moved_identity = ctx.memory(|m| m.area_rect(identity_id)).unwrap();
        assert!(
            moved_identity.top() > identity.top() + 80.,
            "deliberate authority move must remain available"
        );
        let final_task = ctx.memory(|m| m.area_rect(task_id)).unwrap();
        assert!(!final_task.intersects(moved_identity));
        eframe::App::on_exit(&mut app, None);
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(profile.join("layout.json")).unwrap()).unwrap();
        assert!(
            saved["panels"]["task_workspace"]["size"][1]
                .as_f64()
                .unwrap()
                < before.height() as f64 - 40.
        );
        assert_ne!(
            saved["panels"]["task_workspace"]["offset"],
            serde_json::json!([-16., 0.])
        );
        assert!(
            saved["panels"]["cockpit_identity_overlay"]["offset"][1]
                .as_f64()
                .unwrap()
                > 80.
        );
        let _ = std::fs::remove_dir_all(profile);
    }

    #[test]
    fn expanded_status_operations_click_defers_transient_window_constraint() {
        fn frame(
            ctx: &egui::Context,
            app: &mut Cockpit,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            app.observed = Instant::now();
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 650.))),
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ctx| eframe::App::update(app, ctx, &mut eframe::Frame::_new_kittest()),
            )
        }
        for on_foot in [false, true] {
            let ctx = egui::Context::default();
            ctx.options_mut(|o| o.max_passes = std::num::NonZeroUsize::MIN);
            let mut app = view_test_cockpit(&ctx);
            app.embedded = false;
            app.services_open = false;
            app.identity_collapsed = false;
            app.snapshot.as_mut().unwrap().pilots[0].station_avatar = on_foot.then(|| serde_json::from_value(serde_json::json!({"position":[0,0,0],"heading":0,"moving":false,"updated_at_ms":1000})).unwrap());
            let before = serde_json::to_value(&app.snapshot).unwrap();
            let (tx, rx) = mpsc::channel();
            app.commands = Some(tx);
            let mut out = frame(&ctx, &mut app, vec![]);
            for _ in 0..6 {
                out = frame(&ctx, &mut app, vec![]);
            }
            let point = out
                .shapes
                .iter()
                .find_map(|s| {
                    if let egui::epaint::Shape::Text(t) = &s.shape {
                        (t.galley.text() == "Operations"
                            && s.clip_rect
                                .contains_rect(egui::Rect::from_min_size(t.pos, t.galley.size())))
                        .then(|| t.pos + t.galley.size() * 0.5)
                    } else {
                        None
                    }
                })
                .expect("actual visible standalone Operations");
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert!(app.services_open);
            assert!(!app.identity_collapsed);
            let task_id = egui::Id::new("cockpit_services_overlay").with(app.layout.generation);
            assert!(
                ctx.memory(|m| m.area_rect(task_id)).is_none(),
                "do not construct against the old expanded authority bounds"
            );
            for _ in 0..8 {
                frame(&ctx, &mut app, vec![]);
            }
            let task = ctx.memory(|m| m.area_rect(task_id)).unwrap();
            let identity = ctx
                .memory(|m| {
                    m.area_rect(
                        egui::Id::new("cockpit_identity_overlay").with(app.layout.generation),
                    )
                })
                .unwrap();
            assert!(!task.intersects(identity));
            assert!(
                task.height() > 400.,
                "workspace must retain normal usable height after opening: {task:?}"
            );
            assert_eq!(serde_json::to_value(&app.snapshot).unwrap(), before);
            assert!(rx.try_iter().all(|r| matches!(r, Request::Snapshot)));
        }
    }

    #[test]
    fn workspace_back_and_title_close_restore_status_preference() {
        fn frame(
            ctx: &egui::Context,
            app: &mut Cockpit,
            size: Vec2,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            app.observed = Instant::now();
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, size)),
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ctx| eframe::App::update(app, ctx, &mut eframe::Frame::_new_kittest()),
            )
        }
        for on_foot in [false, true] {
            let ctx = egui::Context::default();
            let mut app = view_test_cockpit(&ctx);
            app.embedded = true;
            app.snapshot.as_mut().unwrap().pilots[0].station_avatar = on_foot.then(|| serde_json::from_value(serde_json::json!({"position":[0,0,0],"heading":0,"moving":false,"updated_at_ms":1000})).unwrap());
            let before = serde_json::to_value(&app.snapshot).unwrap();
            let (tx, rx) = mpsc::channel();
            app.commands = Some(tx);
            for preferred in [false, true] {
                app.identity_collapsed = preferred;
                for (size, title_close) in [
                    (Vec2::new(900., 650.), false),
                    (Vec2::new(1280., 800.), true),
                    (Vec2::new(900., 650.), false),
                ] {
                    app.services_open = true;
                    app.service_tab = ServiceTab::Drones;
                    let mut output = frame(&ctx, &mut app, size, vec![]);
                    for _ in 0..8 {
                        output = frame(&ctx, &mut app, size, vec![]);
                    }
                    assert!(app.identity_status_collapsed());
                    assert_eq!(app.identity_collapsed, preferred);
                    let area = ctx
                        .memory(|m| {
                            m.area_rect(
                                egui::Id::new("cockpit_services_overlay")
                                    .with(app.layout.generation),
                            )
                        })
                        .unwrap();
                    let point = if title_close {
                        Pos2::new(area.right() - 14., area.top() + 18.)
                    } else {
                        output
                            .shapes
                            .iter()
                            .find_map(|s| {
                                if let egui::epaint::Shape::Text(t) = &s.shape {
                                    (t.galley.text() == "Back to Flight")
                                        .then(|| t.pos + t.galley.size() * 0.5)
                                } else {
                                    None
                                }
                            })
                            .expect("visible real Back button")
                    };
                    for pressed in [true, false] {
                        frame(
                            &ctx,
                            &mut app,
                            size,
                            vec![
                                egui::Event::PointerMoved(point),
                                egui::Event::PointerButton {
                                    pos: point,
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                        );
                    }
                    assert!(!app.services_open, "real Back/Close must dismiss");
                    frame(&ctx, &mut app, size, vec![]);
                    assert_eq!(app.identity_status_collapsed(), preferred);
                    assert!(app.workspace_identity_override.is_none());
                }
            }
            assert_eq!(serde_json::to_value(&app.snapshot).unwrap(), before);
            assert!(rx.try_iter().all(|r| matches!(r, Request::Snapshot)));
        }
    }

    #[test]
    fn workspace_status_is_temporary_and_explicit_toggles_preserve_preference() {
        let ctx = egui::Context::default();
        let mut app = view_test_cockpit(&ctx);
        let (tx, rx) = mpsc::channel();
        app.commands = Some(tx);
        for on_foot in [false, true] {
            app.snapshot.as_mut().unwrap().pilots[0].station_avatar = on_foot.then(|| {
                serde_json::from_value(serde_json::json!({"position":[0,0,0],"heading":0,"moving":false,"updated_at_ms":1000})).unwrap()
            });
            let before = serde_json::to_value(&app.snapshot).unwrap();
            for preferred in [false, true] {
                app.services_open = false;
                app.sync_workspace_identity();
                app.identity_collapsed = preferred;
                for size in [
                    Vec2::new(900., 650.),
                    Vec2::new(1280., 800.),
                    Vec2::new(900., 650.),
                ] {
                    app.services_open = true;
                    assert!(app.identity_status_collapsed());
                    assert_eq!(app.identity_collapsed, preferred);
                    let mut button = egui::Rect::NOTHING;
                    let run =
                        |app: &mut Cockpit, events: Vec<egui::Event>, button: &mut egui::Rect| {
                            let _ = ctx.run(
                                egui::RawInput {
                                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, size)),
                                    events,
                                    focused: true,
                                    ..Default::default()
                                },
                                |ctx| {
                                    egui::CentralPanel::default()
                                        .show(ctx, |ui| *button = app.identity_header(ui).rect);
                                },
                            );
                        };
                    run(&mut app, vec![], &mut button);
                    // Actual Show then Hide clicks override the temporary state and
                    // deliberately update the preference; no automatic frame resets it.
                    for collapsed in [false, true] {
                        let point = button.center();
                        for pressed in [true, false] {
                            run(
                                &mut app,
                                vec![
                                    egui::Event::PointerMoved(point),
                                    egui::Event::PointerButton {
                                        pos: point,
                                        button: egui::PointerButton::Primary,
                                        pressed,
                                        modifiers: egui::Modifiers::NONE,
                                    },
                                ],
                                &mut button,
                            );
                        }
                        app.sync_workspace_identity();
                        assert_eq!(app.identity_status_collapsed(), collapsed);
                        assert_eq!(app.identity_collapsed, collapsed);
                    }
                    app.services_open = false;
                    app.sync_workspace_identity();
                    assert!(app.identity_status_collapsed());
                    assert!(app.workspace_identity_override.is_none());
                    // Automatic compact-only opening restores either prior preference.
                    app.identity_collapsed = preferred;
                    app.services_open = true;
                    assert!(app.identity_status_collapsed());
                    app.services_open = false;
                    app.sync_workspace_identity();
                    assert_eq!(app.identity_status_collapsed(), preferred);
                }
            }
            assert_eq!(serde_json::to_value(&app.snapshot).unwrap(), before);
        }
        assert!(rx.try_recv().is_err(), "presentation emitted gameplay");
    }

    #[test]
    fn compact_status_collapse_reopen_preserves_authority_and_does_not_take_focus() {
        let ctx = egui::Context::default();
        let profile =
            std::env::temp_dir().join(format!("arkline-status-r6-{}", std::process::id()));
        let mut app = view_test_cockpit_with_profile(&ctx, profile);
        let mut button = egui::Rect::NOTHING;
        let run = |app: &mut Cockpit, events: Vec<egui::Event>, button: &mut egui::Rect| {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| *button = app.identity_header(ui).rect);
                },
            );
        };
        run(&mut app, vec![], &mut button);
        for expected in [true, false] {
            let pos = button.center();
            for pressed in [true, false] {
                run(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    &mut button,
                );
            }
            assert_eq!(app.identity_collapsed, expected);
            assert!(!ctx.wants_keyboard_input());
        }
        assert_eq!(app.compact_control_label(true), "LOCAL PILOT CONTROL");
        app.manual_remote = Some("unconfirmed".into());
        assert_eq!(
            app.compact_control_label(true),
            "REMOTE REQUEST / NO CONTROL"
        );
        app.connected = false;
        assert_eq!(app.compact_control_label(true), "NO CONFIRMED CONTROL");
        app.connected = true;
        app.snapshot = None;
        assert_eq!(app.compact_control_label(true), "NO CONFIRMED CONTROL");
    }

    /// A confirmed reservation fixture; never derives vessel identity from a name.
    pub(crate) fn cargo_departure_fixture(
        c: &mut Cockpit,
        phase: arkline_protocol::CargoUnloadPhase,
    ) {
        use arkline_protocol::{
            CargoUnloadDestination, CargoUnloadOperation, CargoUnloadingState, DockingState,
            DOCKING_VERSION,
        };
        let pilot = &mut c.snapshot.as_mut().unwrap().pilots[0];
        pilot.location = Location::Docked;
        pilot.flight.as_mut().unwrap().docking = Some(DockingState {
            version: DOCKING_VERSION,
            host_vessel_id: "owned-pioneer".into(),
            berth_id: 2,
            operation_id: 7,
            phase: DockingPhase::Secured,
            segment: None,
            clamps: 1.,
            updated_at_ms: 1000,
            hold_reason: None,
            departing: false,
        });
        let operation = CargoUnloadOperation {
            operation_id: 1,
            pilot: pilot.name.clone(),
            vessel_id: "owned-pioneer".into(),
            ship_class: pilot.ship_class,
            system_id: pilot.system_id,
            berth_id: 2,
            destination: CargoUnloadDestination::Personal,
            destination_owner: pilot.name.clone(),
            requested_units: 100,
            committed_units: if phase == arkline_protocol::CargoUnloadPhase::Completed {
                100
            } else {
                25
            },
            reserved_units: if phase.active() { 75 } else { 0 },
            phase,
            next_batch_at_ms: 1500,
            job_id: None,
            message: "Confirmed fixture".into(),
            receipts: vec![],
        };
        let assets = c.assets.get_or_insert_with(|| {
            serde_json::from_value(serde_json::json!({
                "ships": [], "storage": [], "corporation": null, "invitations": []
            }))
            .unwrap()
        });
        assets.cargo_unloading = Some(CargoUnloadingState {
            high_watermark: 1,
            operations: vec![operation],
            ..Default::default()
        });
    }

    pub(crate) fn view_test_cockpit(ctx: &egui::Context) -> Cockpit {
        view_test_cockpit_with_profile(
            ctx,
            std::env::temp_dir().join("arkline-headless-view-unused"),
        )
    }

    pub(crate) fn view_test_cockpit_with_profile(ctx: &egui::Context, profile: PathBuf) -> Cockpit {
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut cockpit = Cockpit::new(&cc, "fixture".into(), profile, None);
        cockpit.boot.visible = false;
        cockpit.connected = true;
        cockpit.pilot = "pilot".into();
        cockpit.snapshot = Some(Snapshot {
            carrier_inventory_registration: None,
            walk_sprint_v1: false,
            drone_cargo_registration: None,
            drone_cargo_totals: None,
            docked_traversal_capability: None,
            world_layout: None,
            exterior: None,
            resource_jobs: vec![],
            fleet_links: vec![],
            remote_controls: vec![],
            surface_props: vec![],
            traffic: vec![],
            stored_ore: 0,
            server_time_ms: 1000,
            asteroids: vec![Asteroid {
                id: 7,
                system_id: HOME_SYSTEM_ID,
                name: "Test rock".into(),
                position: BELT_POSITION,
                radius: 100.0,
                ore: 1000,
            }],
            online_pilots: vec![],
            orders: vec![],
            belt_ore: 1000,
            ledger_sequence: 9,
            ledger_hash: "fixture".into(),
            pilots: vec![PilotView {
                carrier_inventory: None,
                power_accounting: None,
                drone_bay: None,
                drone_cargo: None,
                cockpit_mechanisms: None,
                reactor_configuration: None,
                name: "pilot".into(),
                system_id: HOME_SYSTEM_ID,
                ship_class: ShipClass::Pioneer,
                target_locks: vec![],
                locked_asteroids: vec![],
                surface: None,
                station_avatar: None,
                power: None,
                drones: vec![],
                drone: None,
                target_lock: None,
                mining_modules: (0..2)
                    .map(|module_id| MiningModule {
                        module_id,
                        target_id: None,
                        cycle_started_at_ms: None,
                        cycle_completes_at_ms: None,
                        stop_reason: None,
                    })
                    .collect(),
                locked_asteroid: None,
                credits: 100,
                ore: 25,
                location: Location::Belt,
                flight: Some(FlightState {
                    docking: None,
                    acceleration: [0.; 3],
                    position: BELT_POSITION,
                    velocity: [0.0; 3],
                    heading: [1.0, 0.0, 0.0],
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    angular_velocity: [0.0; 3],
                    mode: NavigationMode::Stop,
                    updated_at_ms: 1000,
                    warp: None,
                }),
            }],
        });
        cockpit.selected_object = SelectedObject::Asteroid(7);
        cockpit.timeline_origin = Some(1000);
        // This synthetic snapshot is received after construction, just as the
        // production listener stamps observed when accepting a new response.
        // Cold source/catalog initialization must not age unreceived telemetry.
        cockpit.observed = Instant::now();
        cockpit
    }
    #[test]
    fn physical_navigation_buttons_preserve_parent_disable_and_authority_gates() {
        for berth in [false, true] {
            for gate in [
                "ready",
                "parent-disabled",
                "paused",
                "offline",
                "pending",
                "stale",
            ] {
                let ctx = egui::Context::default();
                let mut cockpit = view_test_cockpit(&ctx);
                if berth {
                    cockpit.snapshot.as_mut().unwrap().pilots[0].location = Location::Docked;
                }
                cockpit.embedded_paused = gate == "paused";
                cockpit.connected = gate != "offline";
                cockpit.request_pending = gate == "pending";
                cockpit.observed = if gate == "stale" {
                    Instant::now() - Duration::from_secs(10)
                } else {
                    Instant::now()
                };
                let before = serde_json::to_value(&cockpit.snapshot).unwrap();
                let (commands, requests) = mpsc::channel();
                cockpit.commands = Some(commands);
                let mut frame = |events| {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                Pos2::ZERO,
                                Vec2::new(600., 220.),
                            )),
                            events,
                            focused: true,
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                if gate == "parent-disabled" {
                                    ui.disable();
                                }
                                cockpit.cockpit_actions(ui);
                            });
                        },
                    );
                };
                frame(vec![]);
                frame(vec![]);
                let point = Pos2::new(if berth { 78. } else { 212. }, 51.);
                for pressed in [true, false] {
                    frame(vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]);
                }
                if gate == "ready" {
                    let request = requests
                        .try_recv()
                        .expect("ready control must emit an actual request");
                    assert!(if berth {
                        matches!(request, Request::Undock)
                    } else {
                        matches!(
                            request,
                            Request::Navigate {
                                mode: NavigationMode::Stop
                            }
                        )
                    });
                }
                assert!(
                    requests.try_recv().is_err(),
                    "unexpected command: berth={berth}, gate={gate}"
                );
                assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
            }
        }
    }

    #[test]
    fn physical_computer_help_close_cancels_old_press_and_restores_deliberate_click() {
        fn frame(
            ctx: &egui::Context,
            cockpit: &mut Cockpit,
            tick: &mut u64,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            *tick += 1;
            cockpit.observed = Instant::now();
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(960., 640.))),
                    time: Some(*tick as f64 / 60.),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(cockpit, ctx, &mut eframe::Frame::_new_kittest()),
            )
        }
        fn stop_point(ctx: &egui::Context, cockpit: &Cockpit) -> Pos2 {
            let logical = cockpit
                .computer_ui_labels
                .iter()
                .find(|(text, _)| text == "STOP")
                .unwrap()
                .1;
            let point = cockpit
                .flight_view
                .computer_surface()
                .unwrap()
                .project(logical)
                .unwrap();
            assert!(cockpit.flight_view.computer_hit(ctx, point).is_some());
            point
        }
        fn visible_label(output: &egui::FullOutput, wanted: &str) -> Option<Pos2> {
            output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == wanted => {
                        let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                        shape.clip_rect.contains_rect(rect).then_some(rect.center())
                    }
                    _ => None,
                })
                .min_by(|a, b| a.y.total_cmp(&b.y))
        }
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let ctx = egui::Context::default();
        ctx.options_mut(|options| options.max_passes = std::num::NonZeroUsize::MIN);
        ctx.style_mut(|style| style.animation_time = 0.);
        let mut cockpit = view_test_cockpit(&ctx);
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let before = serde_json::to_value(&cockpit.snapshot).unwrap();
        let mut tick = 0;
        for _ in 0..4 {
            frame(&ctx, &mut cockpit, &mut tick, vec![]);
        }
        let old_point = stop_point(&ctx, &cockpit);
        frame(
            &ctx,
            &mut cockpit,
            &mut tick,
            vec![
                egui::Event::PointerMoved(old_point),
                button(old_point, true),
            ],
        );
        assert!(rx.try_recv().is_err());
        // Opening the real reference while a glass press is pending must revoke
        // it immediately, even before the next parent frame/permission pass.
        cockpit.open_controls_reference();
        assert!(cockpit.workspace_input_waiting_release);
        frame(&ctx, &mut cockpit, &mut tick, vec![]);
        let mut output = frame(
            &ctx,
            &mut cockpit,
            &mut tick,
            vec![button(old_point, false)],
        );
        assert!(cockpit.controls_reference_open && rx.try_recv().is_err());
        let mut close = None;
        for _ in 0..30 {
            if let Some(point) = visible_label(&output, "Close controls reference") {
                close = Some(point);
                break;
            }
            let title =
                visible_label(&output, "Controls & ownership").expect("reachable reference title");
            output = frame(
                &ctx,
                &mut cockpit,
                &mut tick,
                vec![
                    egui::Event::PointerMoved(title + Vec2::new(0., 100.)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: Vec2::new(0., -100.),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        let close = close.expect("actual Close control must be reachable by scrolling at 960x640");
        frame(
            &ctx,
            &mut cockpit,
            &mut tick,
            vec![egui::Event::PointerMoved(close), button(close, true)],
        );
        frame(&ctx, &mut cockpit, &mut tick, vec![button(close, false)]);
        assert!(
            !cockpit.controls_reference_open,
            "actual Close pointer must dismiss the reference"
        );
        // A delayed old release after dismissal still cannot finish a glass click.
        frame(
            &ctx,
            &mut cockpit,
            &mut tick,
            vec![
                egui::Event::PointerMoved(old_point),
                button(old_point, false),
            ],
        );
        assert!(
            rx.try_recv().is_err(),
            "modal open/close or delayed release leaked gameplay"
        );
        for _ in 0..2 {
            frame(&ctx, &mut cockpit, &mut tick, vec![]);
        }
        let new_point = stop_point(&ctx, &cockpit);
        frame(
            &ctx,
            &mut cockpit,
            &mut tick,
            vec![
                egui::Event::PointerMoved(new_point),
                button(new_point, true),
            ],
        );
        frame(
            &ctx,
            &mut cockpit,
            &mut tick,
            vec![button(new_point, false)],
        );
        assert!(matches!(
            rx.try_recv(),
            Ok(Request::Navigate {
                mode: NavigationMode::Stop
            })
        ));
        assert!(rx.try_recv().is_err());
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
    }

    #[test]
    fn physical_computer_old_press_cannot_cross_batched_reconnect_into_fresh_session() {
        fn step(
            ctx: &egui::Context,
            cockpit: &mut Cockpit,
            index: &mut u64,
            events: Vec<egui::Event>,
        ) {
            *index += 1;
            cockpit.observed = Instant::now();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(1280., 900.),
                    )),
                    time: Some(*index as f64 / 60.),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(cockpit, ctx, &mut eframe::Frame::_new_kittest()),
            );
        }
        fn stop_point(ctx: &egui::Context, cockpit: &Cockpit) -> Pos2 {
            let logical = cockpit
                .computer_ui_labels
                .iter()
                .find(|(label, _)| label == "STOP")
                .expect("actual compact STOP widget")
                .1;
            let projected = cockpit
                .flight_view
                .computer_surface()
                .unwrap()
                .project(logical)
                .unwrap();
            assert!(
                cockpit.flight_view.computer_hit(ctx, projected).is_some(),
                "current glass must accept the button ray"
            );
            projected
        }
        fn label(ctx: &egui::Context, cockpit: &Cockpit, wanted: &str) -> Option<Pos2> {
            let logical = cockpit
                .computer_ui_labels
                .iter()
                .find(|(text, _)| text == wanted)?
                .1;
            let projected = cockpit.flight_view.computer_surface()?.project(logical)?;
            cockpit.flight_view.computer_hit(ctx, projected)?;
            Some(projected)
        }
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let ctx = egui::Context::default();
        ctx.options_mut(|options| options.max_passes = std::num::NonZeroUsize::MIN);
        let mut cockpit = view_test_cockpit(&ctx);
        let (commands, requests) = mpsc::channel();
        cockpit.commands = Some(commands);
        let (updates, receiver) = mpsc::channel();
        cockpit.updates = Some(receiver);
        let mut index = 0;
        for _ in 0..4 {
            step(&ctx, &mut cockpit, &mut index, vec![]);
        }
        let point = stop_point(&ctx, &cockpit);
        step(
            &ctx,
            &mut cockpit,
            &mut index,
            vec![egui::Event::PointerMoved(point), button(point, true)],
        );
        assert!(
            requests.try_recv().is_err(),
            "press alone cannot issue a ship order"
        );
        let resumed_snapshot = cockpit.snapshot.clone().unwrap();
        let generation = cockpit.connection_generation;
        // Exactly the order emitted by transport; all updates may be queued
        // while the UI is suspended and consumed by one resumed frame.
        updates
            .send(Update::Reconnecting("fixture reconnect".into()))
            .unwrap();
        updates.send(Update::Ready).unwrap();
        updates
            .send(Update::Response(
                Box::new(Response {
                    physical_cargo: None,
                    auth: None,
                    ok: true,
                    message: "resumed".into(),
                    snapshot: Some(resumed_snapshot.clone()),
                    assets: None,
                    resume_token: None,
                    ledger: None,
                }),
                true,
            ))
            .unwrap();
        step(&ctx, &mut cockpit, &mut index, vec![button(point, false)]);
        assert!(cockpit.connected && cockpit.connection_generation > generation);
        let old_requests: Vec<_> = requests.try_iter().collect();
        assert!(
            old_requests.is_empty(),
            "old-session physical press leaked after reconnect: {old_requests:?}"
        );
        assert_eq!(
            serde_json::to_value(&cockpit.snapshot).unwrap(),
            serde_json::to_value(Some(resumed_snapshot)).unwrap()
        );
        step(&ctx, &mut cockpit, &mut index, vec![]);
        let point = stop_point(&ctx, &cockpit);
        step(
            &ctx,
            &mut cockpit,
            &mut index,
            vec![egui::Event::PointerMoved(point), button(point, true)],
        );
        step(&ctx, &mut cockpit, &mut index, vec![button(point, false)]);
        assert!(matches!(
            requests.try_recv(),
            Ok(Request::Navigate {
                mode: NavigationMode::Stop
            })
        ));
        assert!(
            requests.try_recv().is_err(),
            "new deliberate click issues exactly one order"
        );
        // Focus a real physical instruction field, not a synthetic ownership bit.
        updates
            .send(Update::Response(
                Box::new(Response {
                    physical_cargo: None,
                    auth: None,
                    ok: true,
                    message: String::new(),
                    snapshot: cockpit.snapshot.clone(),
                    assets: None,
                    resume_token: None,
                    ledger: None,
                }),
                true,
            ))
            .unwrap();
        cockpit.cockpit_console_detail = true;
        cockpit.service_tab = ServiceTab::ShipComputer;
        let mut orders_opened = false;
        let mut focused = false;
        for _ in 0..80 {
            step(&ctx, &mut cockpit, &mut index, vec![]);
            if !orders_opened {
                if let Some(point) = label(&ctx, &cockpit, "Ship computer · text & voice orders") {
                    step(
                        &ctx,
                        &mut cockpit,
                        &mut index,
                        vec![egui::Event::PointerMoved(point), button(point, true)],
                    );
                    step(&ctx, &mut cockpit, &mut index, vec![button(point, false)]);
                    orders_opened = true;
                    continue;
                }
            }
            if label(&ctx, &cockpit, "Submit instruction").is_some() {
                if let Some(point) = label(&ctx, &cockpit, "Gather 100 ore and bring it back") {
                    step(
                        &ctx,
                        &mut cockpit,
                        &mut index,
                        vec![egui::Event::PointerMoved(point), button(point, true)],
                    );
                    step(&ctx, &mut cockpit, &mut index, vec![button(point, false)]);
                    focused = cockpit.cockpit_computer.wants_keyboard_input();
                    break;
                }
            }
            let surface = cockpit.flight_view.computer_surface().unwrap();
            let point = [
                Pos2::new(420., 300.),
                Pos2::new(300., 280.),
                Pos2::new(420., 240.),
            ]
            .into_iter()
            .filter_map(|p| surface.project(p))
            .find(|p| cockpit.flight_view.computer_hit(&ctx, *p).is_some())
            .expect("visible scroll area");
            step(
                &ctx,
                &mut cockpit,
                &mut index,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: Vec2::new(0., -24.),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(
            focused,
            "real physical text field must own keyboard before reconnect"
        );
        step(
            &ctx,
            &mut cockpit,
            &mut index,
            vec![egui::Event::Text("Keep".into())],
        );
        assert_eq!(cockpit.ai_prompt, "Keep");
        updates
            .send(Update::Reconnecting("text reconnect".into()))
            .unwrap();
        updates.send(Update::Ready).unwrap();
        updates
            .send(Update::Response(
                Box::new(Response {
                    physical_cargo: None,
                    auth: None,
                    ok: true,
                    message: String::new(),
                    snapshot: cockpit.snapshot.clone(),
                    assets: None,
                    resume_token: None,
                    ledger: None,
                }),
                true,
            ))
            .unwrap();
        step(
            &ctx,
            &mut cockpit,
            &mut index,
            vec![egui::Event::Text(" stale input".into())],
        );
        assert_eq!(
            cockpit.ai_prompt, "Keep",
            "old text ownership cannot cross a connection boundary"
        );
        assert!(!cockpit.cockpit_computer.wants_keyboard_input());
        assert!(
            requests.try_recv().is_err(),
            "viewing/editing a field cannot submit a ship instruction"
        );
    }

    #[test]
    fn pioneer_completion_onfoot_destinations_keep_readouts_and_disable_ship_commands() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let pilot = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        pilot.station_avatar = Some(
            serde_json::from_value(serde_json::json!({
                "position":[0.0,0.0,0.0], "heading":0.0, "moving":false,
                "updated_at_ms":1000, "onboard_interior":true, "interior_deck":2
            }))
            .unwrap(),
        );
        pilot.flight.as_mut().unwrap().position = STATION_POSITION;
        pilot.locked_asteroid = Some(7);
        pilot.locked_asteroids = vec![7];
        pilot.mining_modules[0].target_id = Some(7);
        pilot.mining_modules[0].cycle_started_at_ms = Some(1000);
        pilot.mining_modules[0].cycle_completes_at_ms = Some(2000);
        let before = serde_json::to_value(&cockpit.snapshot).unwrap();
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        fn content_frame(
            ctx: &egui::Context,
            cockpit: &mut Cockpit,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(1000.0, 850.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| cockpit.service_content(ui));
                },
            )
        }
        for (tab, expected) in [
            (ServiceTab::Navigation, "SELECTED OBJECT"),
            (ServiceTab::Modules, "MINING MODULE"),
        ] {
            cockpit.service_tab = tab;
            let output = content_frame(&ctx, &mut cockpit, vec![]);
            let labels: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.text()),
                    _ => None,
                })
                .collect();
            assert!(
                labels.contains(&expected),
                "Selected destination must keep its actual content"
            );
            assert!(labels
                .iter()
                .any(|label| label.contains("physical pilot seat")));
            assert!(!labels.contains(&"Pioneer traversal unavailable"));
            assert!(cockpit.service_tab == tab);
            if tab == ServiceTab::Modules {
                let point = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text() == "Stop turret" => {
                            Some(text.pos + text.galley.rect.center().to_vec2())
                        }
                        _ => None,
                    })
                    .expect("Actual turret command remains visibly present");
                for pressed in [true, false] {
                    content_frame(
                        &ctx,
                        &mut cockpit,
                        vec![
                            egui::Event::PointerMoved(point),
                            egui::Event::PointerButton {
                                pos: point,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    );
                }
                assert!(
                    cockpit.notice.is_empty(),
                    "Disabled command must not reach send rejection"
                );
            }
        }
        assert!(!cockpit.can_fly());
        assert!(!cockpit.can_dock(&cockpit.own().unwrap()));
        cockpit.service_tab = ServiceTab::Sandbox;
        let output = content_frame(&ctx, &mut cockpit, vec![]);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text().contains("Surface editing requires the physical pilot seat"))));
        assert!(
            rx.try_recv().is_err(),
            "Inspection must issue no gameplay commands"
        );
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
    }

    fn ui_test_frame(
        ctx: &egui::Context,
        cockpit: &mut Cockpit,
        events: Vec<egui::Event>,
        selector: bool,
    ) -> Vec<(String, Pos2)> {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(1000.0, 650.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if selector {
                        ui.horizontal(|ui| cockpit.view_selector(ui));
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("command_test_scroll")
                            .animated(false)
                            .show(ui, |ui| {
                                cockpit.mining_module(ui);
                                cockpit.drone_controls(ui);
                            });
                    }
                });
            },
        );
        output
            .shapes
            .into_iter()
            .filter_map(|s| {
                if let egui::Shape::Text(t) = s.shape {
                    let point = t.pos + t.galley.rect.center().to_vec2();
                    s.clip_rect
                        .contains(point)
                        .then_some((t.galley.job.text.clone(), point))
                } else {
                    None
                }
            })
            .collect()
    }
    // Exercise real wheel input at the delivery-sized viewport; never click
    // offscreen text or inflate the screen until controls happen to fit.
    fn scroll_test_find(
        mut frame: impl FnMut(Vec<egui::Event>) -> Vec<(String, Pos2)>,
        wanted: &str,
    ) -> Pos2 {
        let pointer = Pos2::new(900., 600.);
        let wheel = |delta| {
            vec![
                egui::Event::PointerMoved(pointer),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0., delta),
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        frame(vec![]);
        frame(wheel(10000.));
        for _ in 0..24 {
            frame(vec![]);
            let labels = frame(vec![]);
            if let Some((_, point)) = labels.iter().find(|(text, _)| text.contains(wanted)) {
                return *point;
            }
            frame(wheel(-180.));
        }
        panic!("Scrollable control not visible: {wanted}");
    }
    fn ui_test_find(ctx: &egui::Context, cockpit: &mut Cockpit, wanted: &str) -> Pos2 {
        scroll_test_find(|events| ui_test_frame(ctx, cockpit, events, false), wanted)
    }
    fn ui_test_click(ctx: &egui::Context, cockpit: &mut Cockpit, point: Pos2, selector: bool) {
        ui_test_frame(
            ctx,
            cockpit,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            selector,
        );
        ui_test_frame(
            ctx,
            cockpit,
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            selector,
        );
    }
    #[test]
    fn view_buttons_switch_without_mutating_navigation_or_selection() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let before = serde_json::to_string(cockpit.snapshot.as_ref().unwrap()).unwrap();
        for (label, mode) in [
            ("2D", ViewMode::Tactical),
            ("3D", ViewMode::Tactical3d),
            ("External", ViewMode::External),
            ("Cockpit", ViewMode::Cockpit),
        ] {
            let labels = ui_test_frame(&ctx, &mut cockpit, vec![], true);
            let point = labels.iter().find(|(t, _)| t == label).unwrap().1;
            ui_test_click(&ctx, &mut cockpit, point, true);
            assert_eq!(cockpit.view_mode, mode);
            assert!(cockpit.selected_object == SelectedObject::Asteroid(7));
            assert_eq!(
                serde_json::to_string(cockpit.snapshot.as_ref().unwrap()).unwrap(),
                before
            );
            assert!(rx.try_recv().is_err());
        }
    }
    #[test]
    fn mining_turret_activation_and_stop_use_slot_identity_without_quantity() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.snapshot.as_mut().unwrap().pilots[0].locked_asteroid = Some(7);
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        let buttons: Vec<_> = labels
            .iter()
            .filter(|(text, _)| text == "Activate turret")
            .map(|(_, p)| *p)
            .collect();
        assert_eq!(buttons.len(), 2);
        ui_test_click(&ctx, &mut cockpit, buttons[1], false);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::StartMining {
                module_id: 1,
                target_id: 7
            }
        ));
        assert_eq!(
            cockpit.own().unwrap().ore,
            25,
            "activation must not grant speculative ore"
        );
        cockpit.request_pending = false;
        let module = &mut cockpit.snapshot.as_mut().unwrap().pilots[0].mining_modules[1];
        module.target_id = Some(7);
        module.cycle_started_at_ms = Some(1000);
        module.cycle_completes_at_ms = Some(6000);
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        assert!(labels.iter().any(|(t, _)| t.contains("TURRET 2")));
        let stop = labels.iter().find(|(t, _)| t == "Stop turret").unwrap().1;
        ui_test_click(&ctx, &mut cockpit, stop, false);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::StopMining { module_id: 1 }
        ));
    }
    #[test]
    fn boot_blocks_flight_commands_until_cockpit_entry() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.boot.connect();
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(1200.0, 800.0),
                )),
                events: vec![egui::Event::Key {
                    key: egui::Key::W,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut cockpit, ctx, &mut eframe::Frame::_new_kittest()),
        );
        assert!(cockpit.boot.visible);
        assert!(rx.try_recv().is_err());
        assert!(cockpit.controls.is_empty());
    }
    #[test]
    fn surface_sandbox_commands_require_confirmed_near_ground_and_owned_props() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let p = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        p.flight.as_mut().unwrap().position = [
            0.0,
            arkline_protocol::PLANET_CENTER_POSITION[1] + arkline_protocol::PLANET_RADIUS_M + 20.0,
            0.0,
        ];
        p.surface = Some(SurfaceState {
            planet_id: 1,
            altitude_m: 20.0,
            ground_clearance_m: 11.0,
            grounded: false,
        });
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        fn frame(
            ctx: &egui::Context,
            c: &mut Cockpit,
            events: Vec<egui::Event>,
        ) -> Vec<(String, Pos2)> {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(900.0, 650.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| c.sandbox_panel(ui));
                },
            )
            .shapes
            .into_iter()
            .filter_map(|s| {
                if let egui::Shape::Text(t) = s.shape {
                    Some((
                        t.galley.job.text.clone(),
                        t.pos + t.galley.rect.center().to_vec2(),
                    ))
                } else {
                    None
                }
            })
            .collect()
        }
        let click = |c: &mut Cockpit, label: &str| {
            let labels = frame(&ctx, c, vec![]);
            let point = labels.iter().find(|(s, _)| s == label).unwrap().1;
            for pressed in [true, false] {
                frame(
                    &ctx,
                    c,
                    vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        };
        click(&mut cockpit, "Spawn selected prop");
        let Request::SpawnSurfaceProp { kind, position } = rx.try_recv().unwrap() else {
            panic!("expected prop spawn")
        };
        assert!(kind == SurfacePropKind::Crate);
        assert!((planet_altitude(position) - 4.0).abs() < 0.001);
        assert!(cockpit.snapshot.as_ref().unwrap().surface_props.is_empty());
        cockpit.request_pending = false;
        cockpit.snapshot.as_mut().unwrap().surface_props = vec![
            SurfaceProp {
                id: 8,
                owner: "pilot".into(),
                system_id: HOME_SYSTEM_ID,
                kind,
                position,
            },
            SurfaceProp {
                id: 9,
                owner: "other".into(),
                system_id: HOME_SYSTEM_ID,
                kind,
                position,
            },
        ];
        let labels = frame(&ctx, &mut cockpit, vec![]);
        assert_eq!(labels.iter().filter(|(s, _)| s == "Delete prop").count(), 1);
        click(&mut cockpit, "Move here");
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::MoveSurfaceProp { id: 8, .. }
        ));
        cockpit.request_pending = false;
        click(&mut cockpit, "Delete prop");
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::RemoveSurfaceProp { id: 8 }
        ));
        cockpit.request_pending = false;
        cockpit.snapshot.as_mut().unwrap().pilots[0]
            .surface
            .as_mut()
            .unwrap()
            .ground_clearance_m = 200.0;
        click(&mut cockpit, "Spawn selected prop");
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn selected_planet_uses_continuous_flight_entry_and_hulls_use_actual_cargo() {
        for definition in [None, Some(SceneLayoutRevision::OrbitalV1.definition())] {
            let ctx = egui::Context::default();
            let mut cockpit = view_test_cockpit(&ctx);
            cockpit.snapshot.as_mut().unwrap().world_layout = definition;
            let approach = snapshot_world_layout(cockpit.snapshot.as_ref().unwrap())
                .unwrap()
                .planets[0]
                .approach_position;
            cockpit.selected_object = SelectedObject::Planet(1);
            assert_eq!(cockpit.selected_position(), Some(approach));
            assert!(cockpit.selected_target().is_none());
            let (tx, rx) = mpsc::channel();
            cockpit.commands = Some(tx);
            let render = |c: &mut Cockpit, events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            Pos2::ZERO,
                            Vec2::new(900.0, 650.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| c.navigation_controls(ui));
                    },
                )
            };
            let output = render(&mut cockpit, vec![]);
            let point = output
                .shapes
                .iter()
                .find_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape {
                        (t.galley.job.text == "Approach planet")
                            .then_some(t.pos + t.galley.rect.center().to_vec2())
                    } else {
                        None
                    }
                })
                .unwrap();
            for pressed in [true, false] {
                render(
                    &mut cockpit,
                    vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert!(
                matches!(rx.try_recv().unwrap(),Request::Navigate{mode:NavigationMode::FlyTo{position}} if position==approach)
            );
            for (index, class) in MINING_SHIP_CLASSES.into_iter().enumerate() {
                cockpit.snapshot.as_mut().unwrap().pilots[0].ship_class = class;
                assert_eq!(class.spec().mining_tier, Some(index as u8 + 1));
                assert_eq!(cockpit.own_cargo_capacity(), class.spec().cargo_capacity);
            }
        }
    }
    #[test]
    fn world_layout_compatibility_is_exact_and_unsupported_snapshots_are_rejected() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let snapshot = cockpit.snapshot.as_ref().unwrap();
        assert_eq!(
            snapshot_world_layout(snapshot).unwrap(),
            &legacy_world_layout()
        );
        assert_eq!(
            snapshot_world_layout(snapshot).unwrap().planets[0].approach_position,
            PLANET_APPROACH_POSITION
        );
        let mut unsupported = snapshot.clone();
        let mut layout = SceneLayoutRevision::OrbitalV1.definition();
        layout.schema_version = WORLD_LAYOUT_SCHEMA_VERSION + 1;
        unsupported.world_layout = Some(layout);
        assert!(snapshot_world_layout(&unsupported)
            .unwrap_err()
            .contains("Unsupported world layout"));
        let (commands, _requests) = mpsc::channel();
        cockpit.commands = Some(commands);
        let (updates, receiver) = mpsc::channel();
        cockpit.updates = Some(receiver);
        updates
            .send(Update::Response(
                Box::new(Response {
                    physical_cargo: None,
                    auth: None,
                    assets: None,
                    ok: true,
                    message: "fixture snapshot".into(),
                    resume_token: None,
                    snapshot: Some(unsupported),
                    ledger: None,
                }),
                false,
            ))
            .unwrap();
        cockpit.poll();
        assert!(!cockpit.connected);
        assert!(
            cockpit.snapshot.is_none(),
            "unsupported geometry must not be rendered over legacy data"
        );
        assert!(cockpit.commands.is_none());
        assert!(cockpit.notice.contains("Unsupported world layout"));
        assert!(!cockpit.can_operate());
    }
    #[test]
    fn malformed_known_layout_is_not_silently_replaced_by_legacy_coordinates() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let mut layout = SceneLayoutRevision::OrbitalV1.definition();
        layout.planets[0].center = PLANET_CENTER_POSITION;
        cockpit.snapshot.as_mut().unwrap().world_layout = Some(layout);
        cockpit.selected_object = SelectedObject::Planet(1);
        assert!(snapshot_world_layout(cockpit.snapshot.as_ref().unwrap()).is_err());
        assert!(cockpit.selected_position().is_none());
        assert!(cockpit.scene_planets().is_empty());
    }
    #[test]
    fn station_hangar_and_snapshot_must_agree_on_layout_revision() {
        let ctx = egui::Context::default();
        let cockpit = view_test_cockpit(&ctx);
        let mut snapshot = cockpit.snapshot.as_ref().unwrap().clone();
        let mut assets = asset_test_fixture();
        assets.hangar = Some(HangarView {
            layout_revision: FleetLayoutRevision::LinearV1,
            layout: hangar_layout(ShipClass::Pioneer),
            berths: vec![],
        });
        assert!(validate_snapshot_scene(&snapshot, Some(&assets)).is_ok());
        snapshot.world_layout = Some(SceneLayoutRevision::OrbitalV1.definition());
        assert!(validate_snapshot_scene(&snapshot, Some(&assets))
            .unwrap_err()
            .contains("contradicts"));
        assets.hangar.as_mut().unwrap().layout_revision = FleetLayoutRevision::CircularV1;
        assert!(validate_snapshot_scene(&snapshot, Some(&assets)).is_ok());
        snapshot.world_layout = None;
        assert!(validate_snapshot_scene(&snapshot, Some(&assets)).is_err());
    }
    #[test]
    fn warp_waits_for_confirmed_capacitor_without_speculative_recharge() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let cost = power_spec(cockpit.own().unwrap().ship_class).warp_cost_j;
        cockpit.snapshot.as_mut().unwrap().pilots[0].power = Some(PowerState {
            charge_j: cost - 1,
            capacity_j: cost * 2,
            regen_w: 1000,
            updated_at_ms: 1,
            regen_remainder_mj: 0,
        });
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        assert!(!cockpit.can_warp());
        cockpit.send(Request::Warp {
            destination: Destination::Station,
        });
        assert!(rx.try_recv().is_err());
        assert!(cockpit.notice.contains("Insufficient capacitor"));
        cockpit.snapshot.as_mut().unwrap().pilots[0]
            .power
            .as_mut()
            .unwrap()
            .charge_j = cost;
        assert!(cockpit.can_warp());
        cockpit.send(Request::Warp {
            destination: Destination::Station,
        });
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::Warp {
                destination: Destination::Station
            }
        ));
    }
    #[test]
    fn hangar_undock_is_visible_on_every_tab_and_disabled_offline() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.view_mode = ViewMode::Cockpit;
        cockpit.snapshot.as_mut().unwrap().pilots[0].location = Location::Docked;
        cockpit.snapshot.as_mut().unwrap().pilots[0].power = Some(PowerState {
            charge_j: 2500,
            capacity_j: 10000,
            regen_w: 300,
            updated_at_ms: 1000,
            regen_remainder_mj: 0,
        });
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        fn frame(
            ctx: &egui::Context,
            c: &mut Cockpit,
            events: Vec<egui::Event>,
        ) -> Vec<(String, Pos2)> {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(900.0, 650.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(c, ctx, &mut eframe::Frame::_new_kittest()),
            )
            .shapes
            .into_iter()
            .filter_map(|s| {
                if let egui::Shape::Text(t) = s.shape {
                    Some((
                        t.galley.job.text.clone(),
                        t.pos + t.galley.rect.center().to_vec2(),
                    ))
                } else {
                    None
                }
            })
            .collect()
        }
        for _ in 0..4 {
            frame(&ctx, &mut cockpit, vec![]);
        }
        for tab in [
            ServiceTab::Layouts,
            ServiceTab::Modules,
            ServiceTab::Navigation,
            ServiceTab::Hangar,
            ServiceTab::Drones,
            ServiceTab::Cargo,
            ServiceTab::Station,
            ServiceTab::Exchange,
            ServiceTab::Corporation,
            ServiceTab::Activity,
            ServiceTab::Ledger,
            ServiceTab::Settings,
        ] {
            cockpit.service_tab = tab;
            cockpit.request_pending = false;
            let labels = frame(&ctx, &mut cockpit, vec![]);
            assert!(labels
                .iter()
                .any(|(text, _)| text.contains("Capacitor 2.5/10.0 kJ · regen rating 300 W")));
            let rect = ctx
                .memory(|m| m.area_rect(egui::Id::new("station_embodiment_controls")))
                .unwrap();
            let point = labels
                .iter()
                .find(|(s, p)| s == "Undock active vessel" && rect.contains(*p))
                .unwrap()
                .1;
            assert!(
                rect.contains_rect(egui::Rect::from_center_size(point, Vec2::new(100.0, 16.0))),
                "Undock button must fit hangar access panel at minimum viewport: rect={rect:?}, center={point:?}"
            );
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut cockpit,
                    vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert!(matches!(rx.try_recv().unwrap(), Request::Undock));
            assert!(
                cockpit.own().unwrap().location == Location::Docked,
                "only server confirmation changes location"
            );
        }
        cockpit.connected = false;
        cockpit.request_pending = false;
        let labels = frame(&ctx, &mut cockpit, vec![]);
        let rect = ctx
            .memory(|m| m.area_rect(egui::Id::new("station_embodiment_controls")))
            .unwrap();
        let point = labels
            .iter()
            .find(|(s, p)| s == "Undock active vessel" && rect.contains(*p))
            .unwrap()
            .1;
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut cockpit,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn physical_console_fleet_click_sends_real_command_without_flight_input() {
        crate::cockpit_computer_integration_tests::fleet_command_input_journey();
    }
    #[test]
    fn navigation_workspace_pointer_drag_persists_and_reflows_in_real_app_update() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.view_mode = ViewMode::Tactical;
        // This test measures unobstructed anchor reflow; expanded-authority
        // exclusion is covered separately with real status-toggle clicks.
        cockpit.identity_collapsed = true;
        cockpit.services_open = true;
        cockpit.service_tab = ServiceTab::Navigation;
        let dir = std::env::temp_dir().join(format!("arkline-window-drag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        cockpit.layout = layout::Layout::load(&dir);
        fn frame(
            ctx: &egui::Context,
            c: &mut Cockpit,
            size: Vec2,
            events: Vec<egui::Event>,
        ) -> egui::Rect {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, size)),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    eframe::App::update(c, ctx, &mut eframe::Frame::_new_kittest());
                },
            );
            ctx.memory(|m| {
                m.area_rect(egui::Id::new("cockpit_services_overlay").with(c.layout.generation))
                    .unwrap()
            })
        }
        let size = Vec2::new(1200.0, 1000.0);
        for _ in 0..4 {
            frame(&ctx, &mut cockpit, size, vec![]);
        }
        let before = frame(&ctx, &mut cockpit, size, vec![]);
        assert!(
            !before.intersects(
                ctx.memory(|m| m.area_rect(
                    egui::Id::new("cockpit_identity_overlay").with(cockpit.layout.generation)
                ))
                .expect("authority banner remains visible")
            ),
            "anchor premise must remain unobstructed"
        );
        let start = before.left_top() + Vec2::new(100.0, 12.0);
        frame(
            &ctx,
            &mut cockpit,
            size,
            vec![egui::Event::PointerMoved(start)],
        );
        frame(
            &ctx,
            &mut cockpit,
            size,
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let end = start + Vec2::new(-180.0, 0.0);
        frame(
            &ctx,
            &mut cockpit,
            size,
            vec![egui::Event::PointerMoved(end)],
        );
        frame(&ctx, &mut cockpit, size, vec![]);
        frame(
            &ctx,
            &mut cockpit,
            size,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let moved = frame(&ctx, &mut cockpit, size, vec![]);
        assert!(
            (moved.min - before.min - Vec2::new(-180.0, 0.0)).length() < 3.0,
            "before={before:?}, moved={moved:?}"
        );
        cockpit.layout.wait_for_save();
        assert!(dir.join("layout.json").exists());
        cockpit.layout = layout::Layout::load(&dir);
        for _ in 0..3 {
            frame(&ctx, &mut cockpit, size, vec![]);
        }
        let loaded = frame(&ctx, &mut cockpit, size, vec![]);
        assert!(
            !loaded.intersects(
                ctx.memory(|m| m.area_rect(
                    egui::Id::new("cockpit_identity_overlay").with(cockpit.layout.generation)
                ))
                .expect("authority banner remains visible")
            ),
            "anchor premise must remain unobstructed"
        );
        assert!(loaded.min.distance(moved.min) < 3.0);
        let larger = Vec2::new(1400.0, 1100.0);
        for _ in 0..3 {
            frame(&ctx, &mut cockpit, larger, vec![]);
        }
        let resized = frame(&ctx, &mut cockpit, larger, vec![]);
        assert!(
            !resized.intersects(
                ctx.memory(|m| m.area_rect(
                    egui::Id::new("cockpit_identity_overlay").with(cockpit.layout.generation)
                ))
                .expect("authority banner remains visible")
            ),
            "anchor premise must remain unobstructed"
        );
        assert!((resized.left() - loaded.left() - 200.0).abs() < 3.0);
        assert!(
            (resized.top() - loaded.top() - 50.0).abs() < 3.0,
            "Workspace retains its vertical center anchor: loaded={loaded:?}, resized={resized:?}"
        );
        let start = resized.left_top() + Vec2::new(100.0, 12.0);
        frame(
            &ctx,
            &mut cockpit,
            larger,
            vec![egui::Event::PointerMoved(start)],
        );
        frame(
            &ctx,
            &mut cockpit,
            larger,
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let end = start
            + Vec2::new(
                ctx.available_rect().shrink(8.0).right() - 20.0 - resized.right(),
                0.0,
            );
        frame(
            &ctx,
            &mut cockpit,
            larger,
            vec![egui::Event::PointerMoved(end)],
        );
        frame(&ctx, &mut cockpit, larger, vec![]);
        frame(
            &ctx,
            &mut cockpit,
            larger,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let snapped = frame(&ctx, &mut cockpit, larger, vec![]);
        assert!((ctx.available_rect().shrink(8.0).right() - snapped.right() - 8.0).abs() < 3.0);
        cockpit.layout.reset();
        for _ in 0..3 {
            frame(&ctx, &mut cockpit, larger, vec![]);
        }
        let reset = frame(&ctx, &mut cockpit, larger, vec![]);
        assert!((ctx.available_rect().shrink(8.0).right() - reset.right() - 16.0).abs() < 3.0);
        assert!(
            (reset.center().y - ctx.available_rect().shrink(8.0).center().y).abs() < 3.0,
            "Reset restores the current centered workspace default"
        );
        cockpit.layout.wait_for_save();
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn five_lock_rack_selects_independent_turret_targets_and_unlocks_one() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let snapshot = cockpit.snapshot.as_mut().unwrap();
        let rock = snapshot.asteroids[0].clone();
        snapshot.asteroids = (7..12)
            .map(|id| {
                let mut a = rock.clone();
                a.id = id;
                a.name = format!("Rock {id}");
                a
            })
            .collect();
        snapshot.pilots[0].locked_asteroids = vec![7, 8, 9, 10];
        snapshot.pilots[0].target_locks = vec![TargetLock {
            asteroid_id: 11,
            started_at_ms: 1000,
            completes_at_ms: 9000,
        }];
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        assert!(labels.iter().any(|(t, _)| t.contains("TARGET RACK · 5/5")));
        let point = labels
            .iter()
            .find(|(t, _)| t == "Locked · Rock 8")
            .unwrap()
            .1;
        ui_test_click(&ctx, &mut cockpit, point, false);
        assert_eq!(cockpit.module_target(&cockpit.own().unwrap()), Some(8));
        cockpit.start_module(0);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::StartMining {
                module_id: 0,
                target_id: 8
            }
        ));
        cockpit.request_pending = false;
        cockpit.selected_object = SelectedObject::Asteroid(9);
        cockpit.start_module(1);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::StartMining {
                module_id: 1,
                target_id: 9
            }
        ));
        cockpit.request_pending = false;
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        let point = labels.iter().find(|(t, _)| t == "Unlock").unwrap().1;
        ui_test_click(&ctx, &mut cockpit, point, false);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::UnlockAsteroid { id: 7 }
        ));
    }
    fn operations_safety_frame(
        ctx: &egui::Context,
        cockpit: &mut Cockpit,
        section: usize,
        events: Vec<egui::Event>,
    ) -> Vec<(String, Pos2)> {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(1000.0, 650.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(("safety_test_scroll", section))
                        .animated(false)
                        .show(ui, |ui| match section {
                            0 => cockpit.drone_controls(ui),
                            1 => cockpit.mining_module(ui),
                            _ => cockpit.resource_job_panel(ui),
                        });
                });
            },
        )
        .shapes
        .into_iter()
        .filter_map(|shape| match shape.shape {
            egui::Shape::Text(text) => {
                let point = text.pos + text.galley.rect.center().to_vec2();
                shape
                    .clip_rect
                    .contains(point)
                    .then_some((text.galley.text().to_owned(), point))
            }
            _ => None,
        })
        .collect()
    }
    fn operations_safety_click(
        ctx: &egui::Context,
        cockpit: &mut Cockpit,
        section: usize,
        point: Pos2,
    ) {
        for pressed in [true, false] {
            operations_safety_frame(
                ctx,
                cockpit,
                section,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    fn operations_safety_drone(id: u64, class: DroneClass) -> DroneView {
        DroneView {
            orientation_xyzw: None,
            id,
            class,
            hit_points: 100,
            signal_strength: 1.0,
            mining_cycle: None,
            attack_ready_at_ms: None,
            position: BELT_POSITION,
            velocity: [0.0; 3],
            heading: [1.0, 0.0, 0.0],
            mode: DroneMode::Hold,
            updated_at_ms: 1000,
        }
    }
    #[test]
    fn operations_safety_disappearing_drone_requires_explicit_reselection() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        cockpit.snapshot.as_mut().unwrap().pilots[0].drones = vec![
            operations_safety_drone(2, DroneClass::Recon),
            operations_safety_drone(7, DroneClass::Recon),
        ];
        cockpit.selected_drone = Some(7);
        assert_eq!(
            cockpit
                .selected_drone_view(&cockpit.own().unwrap())
                .unwrap()
                .id,
            7
        );
        cockpit.snapshot.as_mut().unwrap().pilots[0]
            .drones
            .retain(|d| d.id != 7);
        let before = serde_json::to_value(&cockpit.snapshot).unwrap();
        assert!(cockpit
            .selected_drone_view(&cockpit.own().unwrap())
            .is_none());
        let labels = operations_safety_frame(&ctx, &mut cockpit, 0, vec![]);
        assert!(labels
            .iter()
            .any(|(text, _)| text.contains("Selected drone #7 is no longer reported")));
        assert!(!labels
            .iter()
            .any(|(text, _)| text == "Recall drone" || text == "Hold drone"));
        assert_eq!(cockpit.selected_drone, Some(7));
        assert!(rx.try_recv().is_err());
        let point = labels
            .iter()
            .find(|(text, _)| text.starts_with("#02 "))
            .unwrap()
            .1;
        operations_safety_click(&ctx, &mut cockpit, 0, point);
        assert_eq!(cockpit.selected_drone, Some(2));
        assert!(rx.try_recv().is_err());
        let point = scroll_test_find(
            |events| operations_safety_frame(&ctx, &mut cockpit, 0, events),
            "Recall drone",
        );
        operations_safety_click(&ctx, &mut cockpit, 0, point);
        assert!(matches!(
            rx.try_recv(),
            Ok(Request::RecallDroneById { id: 2 })
        ));
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
    }
    #[test]
    fn operations_safety_stale_turret_orders_wait_for_fresh_authority() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let pilot = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        pilot.locked_asteroids = vec![7];
        pilot.mining_modules[0].target_id = Some(7);
        cockpit.timeline_origin = Some(3000);
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let before = serde_json::to_value(&cockpit.snapshot).unwrap();
        assert!(!cockpit.can_mine(&cockpit.own().unwrap()));
        let labels = operations_safety_frame(&ctx, &mut cockpit, 1, vec![]);
        assert!(labels
            .iter()
            .any(|(text, _)| text.contains("Stale telemetry")));
        for name in ["Stop turret", "Unlock target"] {
            let point = labels.iter().find(|(text, _)| text == name).unwrap().1;
            operations_safety_click(&ctx, &mut cockpit, 1, point);
        }
        assert!(rx.try_recv().is_err());
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
        cockpit.timeline_origin = Some(1000);
        let labels = operations_safety_frame(&ctx, &mut cockpit, 1, vec![]);
        operations_safety_click(
            &ctx,
            &mut cockpit,
            1,
            labels
                .iter()
                .find(|(text, _)| text == "Stop turret")
                .unwrap()
                .1,
        );
        assert!(matches!(
            rx.try_recv(),
            Ok(Request::StopMining { module_id: 0 })
        ));
    }
    #[test]
    fn operations_safety_reports_real_stage_and_full_hold_without_manual_orders() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let pilot = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        pilot.locked_asteroids = vec![7];
        pilot.ore = cargo_capacity(pilot.ship_class);
        pilot.drones = vec![operations_safety_drone(7, DroneClass::Mining)];
        let job:ResourceJob=serde_json::from_value(serde_json::json!({"controller":"pilot","pilot":"pilot","id":8,"target_units":10000,"mined_units":200,"stored_units":1000,"target_id":7,"phase":"mining","message":"Waiting for all mining drones to return","started_at_ms":1000,"updated_at_ms":1000})).unwrap();
        cockpit.snapshot.as_mut().unwrap().resource_jobs.push(job);
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let before = serde_json::to_value(&cockpit.snapshot).unwrap();
        let labels = operations_safety_frame(&ctx, &mut cockpit, 0, vec![]);
        assert!(labels
            .iter()
            .any(|(text, _)| text.contains("manual orders cancel this mining routine")));
        scroll_test_find(
            |events| operations_safety_frame(&ctx, &mut cockpit, 0, events),
            "Ship hold full",
        );
        let point = scroll_test_find(
            |events| operations_safety_frame(&ctx, &mut cockpit, 0, events),
            "Mine locked target",
        );
        operations_safety_click(&ctx, &mut cockpit, 0, point);
        assert!(rx.try_recv().is_err());
        let labels = operations_safety_frame(&ctx, &mut cockpit, 2, vec![]);
        assert!(labels
            .iter()
            .any(|(text, _)| text
                .contains("Authority stage: Waiting for all mining drones to return")));
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
    }

    #[test]
    fn drone_controls_emit_real_launch_hold_recall_and_selected_position() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let point = ui_test_find(&ctx, &mut cockpit, "Launch Recon 0/5");
        ui_test_click(&ctx, &mut cockpit, point, false);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::LaunchDroneClass {
                class: DroneClass::Recon
            }
        ));
        cockpit.request_pending = false;
        cockpit.snapshot.as_mut().unwrap().pilots[0].drone = Some(DroneView {
            orientation_xyzw: None,
            id: 1,
            class: DroneClass::Recon,
            hit_points: 100,
            signal_strength: 1.0,
            mining_cycle: None,
            attack_ready_at_ms: None,
            position: BELT_POSITION,
            velocity: [0.0; 3],
            heading: [1.0, 0.0, 0.0],
            mode: DroneMode::Hold,
            updated_at_ms: 1000,
        });
        let expected = cockpit.drone_destination(&cockpit.own().unwrap()).unwrap();
        assert!(
            (distance_m(expected, BELT_POSITION) - 200.0).abs() < 0.001,
            "100m radius plus 100m standoff"
        );
        for label in ["Hold drone", "Recall drone", "Drone to selected"] {
            let point = ui_test_find(&ctx, &mut cockpit, label);
            ui_test_click(&ctx, &mut cockpit, point, false);
            let request = rx.try_recv().unwrap();
            assert!(match label {
                "Hold drone" => matches!(request, Request::DroneHoldById { id: 1 }),
                "Recall drone" => matches!(request, Request::RecallDroneById { id: 1 }),
                _ =>
                    matches!(request,Request::DroneNavigateById{id:1,position} if position==expected),
            });
            cockpit.request_pending = false;
        }
        cockpit.selected_object = SelectedObject::Station;
        let point = ui_test_find(&ctx, &mut cockpit, "Drone to selected");
        ui_test_click(&ctx, &mut cockpit, point, false);
        assert!(
            rx.try_recv().is_err(),
            "out-of-range drone navigation stays disabled"
        );
    }
    #[test]
    fn fleet_selection_routes_commands_by_id_and_limits_each_class() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.snapshot.as_mut().unwrap().pilots[0].drones = (1..=5)
            .map(|id| DroneView {
                orientation_xyzw: None,
                id,
                class: DroneClass::Recon,
                hit_points: 100,
                signal_strength: 1.0,
                mining_cycle: None,
                attack_ready_at_ms: None,
                position: BELT_POSITION,
                velocity: [0.0; 3],
                heading: [1.0, 0.0, 0.0],
                mode: DroneMode::Hold,
                updated_at_ms: 1000,
            })
            .collect();
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let second = ui_test_find(&ctx, &mut cockpit, "#02 Recon");
        ui_test_click(&ctx, &mut cockpit, second, false);
        assert_eq!(cockpit.selected_drone, Some(2));
        let point = ui_test_find(&ctx, &mut cockpit, "Hold drone");
        ui_test_click(&ctx, &mut cockpit, point, false);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::DroneHoldById { id: 2 }
        ));
        cockpit.request_pending = false;
        let point = ui_test_find(&ctx, &mut cockpit, "Launch Recon 5/5");
        ui_test_click(&ctx, &mut cockpit, point, false);
        assert!(rx.try_recv().is_err());
        let point = ui_test_find(&ctx, &mut cockpit, "Launch Mining 0/5");
        ui_test_click(&ctx, &mut cockpit, point, false);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::LaunchDroneClass {
                class: DroneClass::Mining
            }
        ));
        cockpit.request_pending = false;
        let point = ui_test_find(&ctx, &mut cockpit, "Orbit camera");
        ui_test_click(&ctx, &mut cockpit, point, false);
        assert_eq!(cockpit.drone_camera_mode, flight3d::DroneCameraMode::Orbit);
        assert!(rx.try_recv().is_err());
        for label in ["Split feeds", "Next feeds", "Raised"] {
            let point = ui_test_find(&ctx, &mut cockpit, label);
            ui_test_click(&ctx, &mut cockpit, point, false);
        }
        assert!(cockpit.drone_split);
        assert_eq!(cockpit.selected_drone, Some(5));
        assert_eq!(cockpit.drone_camera_mode, flight3d::DroneCameraMode::Raised);
        assert!(
            rx.try_recv().is_err(),
            "Camera choices must not steer drones"
        );
        cockpit.selected_drone = Some(2);
        cockpit.snapshot.as_mut().unwrap().pilots[0].drones[1].class = DroneClass::Mining;
        cockpit.snapshot.as_mut().unwrap().pilots[0].locked_asteroids = vec![7];
        let point = ui_test_find(&ctx, &mut cockpit, "Mine locked target");
        ui_test_click(&ctx, &mut cockpit, point, false);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::AssignMiningDrone {
                id: 2,
                target_id: 7
            }
        ));
        cockpit.request_pending = false;
        cockpit.snapshot.as_mut().unwrap().pilots[0].drones[1].class = DroneClass::Attack;
        let mut enemy = cockpit.snapshot.as_ref().unwrap().pilots[0].clone();
        enemy.name = "Other pilot".into();
        enemy.drones.truncate(1);
        enemy.drones[0].id = 99;
        cockpit.snapshot.as_mut().unwrap().pilots.push(enemy);
        cockpit.drone_attack_target = Some(("Other pilot".into(), 99));
        let point = ui_test_find(&ctx, &mut cockpit, "Attack selected drone");
        ui_test_click(&ctx, &mut cockpit, point, false);
        assert!(
            matches!(rx.try_recv().unwrap(),Request::AssignAttackDrone{id:2,target_pilot,target_drone_id:99} if target_pilot=="Other pilot")
        );
    }
    #[test]
    fn drone_asteroid_waypoint_faces_drone_and_stays_outside_surface() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let pilot = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        pilot.drone = Some(DroneView {
            orientation_xyzw: None,
            id: 1,
            class: DroneClass::Recon,
            hit_points: 100,
            signal_strength: 1.0,
            mining_cycle: None,
            attack_ready_at_ms: None,
            position: [BELT_POSITION[0] - 3000.0, 0.0, 0.0],
            velocity: [0.0; 3],
            heading: [1.0, 0.0, 0.0],
            mode: DroneMode::Hold,
            updated_at_ms: 1000,
        });
        let target = cockpit.drone_destination(&cockpit.own().unwrap()).unwrap();
        assert_eq!(target, [BELT_POSITION[0] - 200.0, 0.0, 0.0]);
        cockpit.snapshot.as_mut().unwrap().asteroids[0].position[0] += DRONE_MAX_RANGE * 2.0;
        assert!(cockpit.drone_destination(&cockpit.own().unwrap()).is_none());
    }
    #[test]
    fn mining_activation_uses_confirmed_lock_while_browsing_station() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.snapshot.as_mut().unwrap().pilots[0].locked_asteroid = Some(7);
        cockpit.selected_object = SelectedObject::Station;
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        assert!(cockpit.can_mine(&cockpit.own().unwrap()));
        assert!(!cockpit.can_lock_selected());
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        assert!(labels
            .iter()
            .any(|(text, _)| text.contains("Locked · Test rock")));
        let activate = labels
            .iter()
            .find(|(text, _)| text == "Activate turret")
            .unwrap()
            .1;
        ui_test_click(&ctx, &mut cockpit, activate, false);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::StartMining {
                module_id: 0,
                target_id: 7
            }
        ));
        assert!(cockpit.selected_object == SelectedObject::Station);
    }
    #[test]
    fn acquiring_lock_blocks_duplicate_and_extract_until_server_confirmation() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        cockpit.snapshot.as_mut().unwrap().pilots[0].target_lock = Some(TargetLock {
            asteroid_id: 7,
            started_at_ms: 1000,
            completes_at_ms: 5000,
        });
        assert!(!cockpit.can_lock_selected());
        assert!(!cockpit.can_mine(&cockpit.own().unwrap()));
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        assert!(labels
            .iter()
            .any(|(text, _)| text.contains("Acquiring Test rock")));
        for label in ["Lock", "Activate turret"] {
            let point = labels.iter().find(|(t, _)| t == label).unwrap().1;
            ui_test_click(&ctx, &mut cockpit, point, false);
            assert!(rx.try_recv().is_err());
        }
        cockpit.frozen_time = Some(6000);
        assert!(cockpit.mining_status().0.contains("confirming"));
        assert!(!cockpit.can_mine(&cockpit.own().unwrap()));
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        let cancel = labels.iter().find(|(t, _)| t == "Cancel lock").unwrap().1;
        ui_test_click(&ctx, &mut cockpit, cancel, false);
        assert!(
            rx.try_recv().is_err(),
            "A stale snapshot must not authorize lock cancellation"
        );
        // A fresh authority snapshot still reports the pending lock: elapsed local
        // time does not confirm acquisition, but cancellation is now safe.
        cockpit.snapshot.as_mut().unwrap().server_time_ms = 6000;
        assert!(!cockpit.can_mine(&cockpit.own().unwrap()));
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        let cancel = labels.iter().find(|(t, _)| t == "Cancel lock").unwrap().1;
        ui_test_click(&ctx, &mut cockpit, cancel, false);
        assert!(matches!(rx.try_recv().unwrap(), Request::UnlockTarget));
        cockpit.request_pending = false;
        let pilot = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        pilot.target_lock = None;
        pilot.locked_asteroid = Some(7);
        assert!(cockpit.can_mine(&cockpit.own().unwrap()));
    }

    fn service_test_frame(
        ctx: &egui::Context,
        cockpit: &mut Cockpit,
        events: Vec<egui::Event>,
        tab: ServiceTab,
    ) -> Vec<(String, Pos2)> {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(1100.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| match tab {
                    ServiceTab::Cargo => cockpit.cargo_panel(ui),
                    ServiceTab::Corporation => cockpit.corporation_panel(ui),
                    ServiceTab::Hangar => cockpit.hangar_panel(ui),
                    _ => cockpit.station_panel(ui),
                });
            },
        );
        output
            .shapes
            .into_iter()
            .filter_map(|s| {
                if let egui::Shape::Text(t) = s.shape {
                    Some((
                        t.galley.job.text.clone(),
                        t.pos + t.galley.rect.center().to_vec2(),
                    ))
                } else {
                    None
                }
            })
            .collect()
    }
    fn service_test_click(
        ctx: &egui::Context,
        cockpit: &mut Cockpit,
        point: Pos2,
        tab: ServiceTab,
    ) {
        service_test_frame(
            ctx,
            cockpit,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            tab,
        );
        service_test_frame(
            ctx,
            cockpit,
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            tab,
        );
    }
    fn asset_test_fixture() -> AssetsView {
        AssetsView {
            carrier_inventory: None,
            drone_bay: None,
            drone_cargo: None,
            cargo_unloading: None,
            pioneer_room: None,
            pioneer_room_layout: None,
            hangar: None,
            pioneer_interior_available: true,
            pioneer_interior_decks: 2,
            ships: vec![
                HangarShip {
                    power_accounting: None,
                    power: None,
                    ship_class: ShipClass::Hauler,
                    system_id: 1,
                    ore: 99,
                },
                HangarShip {
                    power_accounting: None,
                    power: None,
                    ship_class: ShipClass::Vanguard,
                    system_id: 2,
                    ore: 88,
                },
            ],
            storage: vec![
                StationStorage {
                    system_id: 1,
                    ore: 50,
                },
                StationStorage {
                    system_id: 2,
                    ore: 777,
                },
            ],
            corporation: None,
            invitations: vec![],
        }
    }
    #[test]
    fn inventory_is_remote_visible_but_transfers_and_boarding_require_local_dock() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.assets = Some(asset_test_fixture());
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let labels = service_test_frame(&ctx, &mut cockpit, vec![], ServiceTab::Cargo);
        assert!(labels.iter().any(|(t, _)| t == "777"));
        assert!(labels.iter().any(|(t, _)| t.contains("Stored")));
        let store = labels.iter().find(|(t, _)| t == "Store ore").unwrap().1;
        service_test_click(&ctx, &mut cockpit, store, ServiceTab::Cargo);
        assert!(rx.try_recv().is_err());
        cockpit.snapshot.as_mut().unwrap().pilots[0].location = Location::Docked;
        let labels = service_test_frame(&ctx, &mut cockpit, vec![], ServiceTab::Cargo);
        let store = labels.iter().find(|(t, _)| t == "Store ore").unwrap().1;
        service_test_click(&ctx, &mut cockpit, store, ServiceTab::Cargo);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::DepositOre { units: 25 }
        ));
        cockpit.request_pending = false;
        let labels = service_test_frame(&ctx, &mut cockpit, vec![], ServiceTab::Hangar);
        let boards: Vec<_> = labels
            .iter()
            .filter(|(t, _)| t == "Board ship")
            .map(|(_, p)| *p)
            .collect();
        assert_eq!(boards.len(), 2);
        service_test_click(&ctx, &mut cockpit, boards[1], ServiceTab::Hangar);
        assert!(rx.try_recv().is_err());
        service_test_click(&ctx, &mut cockpit, boards[0], ServiceTab::Hangar);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::SelectShip {
                ship_class: ShipClass::Hauler
            }
        ));
    }
    #[test]
    fn corporation_actions_use_real_requests_and_owner_storage_permissions() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        cockpit.assets = Some(asset_test_fixture());
        cockpit.corporation_name = "Test corporation".into();
        let labels = service_test_frame(&ctx, &mut cockpit, vec![], ServiceTab::Corporation);
        let button = labels
            .iter()
            .find(|(t, _)| t == "Create corporation")
            .unwrap()
            .1;
        service_test_click(&ctx, &mut cockpit, button, ServiceTab::Corporation);
        assert!(
            matches!(rx.try_recv().unwrap(),Request::CreateCorporation {name} if name=="Test corporation")
        );
        cockpit.request_pending = false;
        cockpit.assets.as_mut().unwrap().invitations = vec!["Inviting corporation".into()];
        let labels = service_test_frame(&ctx, &mut cockpit, vec![], ServiceTab::Corporation);
        let button = labels
            .iter()
            .find(|(t, _)| t == "Accept invitation")
            .unwrap()
            .1;
        service_test_click(&ctx, &mut cockpit, button, ServiceTab::Corporation);
        assert!(
            matches!(rx.try_recv().unwrap(),Request::AcceptCorporation {corporation} if corporation=="Inviting corporation")
        );
        cockpit.request_pending = false;
        cockpit.snapshot.as_mut().unwrap().pilots[0].location = Location::Docked;
        cockpit.assets.as_mut().unwrap().corporation = Some(CorporationView {
            name: "Inviting corporation".into(),
            owner: "another pilot".into(),
            members: vec!["pilot".into()],
            storage: vec![StationStorage {
                system_id: 1,
                ore: 100,
            }],
        });
        let labels = service_test_frame(&ctx, &mut cockpit, vec![], ServiceTab::Corporation);
        let button = labels.iter().find(|(t, _)| t == "Withdraw ore").unwrap().1;
        service_test_click(&ctx, &mut cockpit, button, ServiceTab::Corporation);
        assert!(rx.try_recv().is_err());
        cockpit
            .assets
            .as_mut()
            .unwrap()
            .corporation
            .as_mut()
            .unwrap()
            .owner = "pilot".into();
        let labels = service_test_frame(&ctx, &mut cockpit, vec![], ServiceTab::Corporation);
        let button = labels.iter().find(|(t, _)| t == "Withdraw ore").unwrap().1;
        service_test_click(&ctx, &mut cockpit, button, ServiceTab::Corporation);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::WithdrawCorporationOre { units: 25 }
        ));
    }
    #[test]
    fn contextual_interactions_escape_returns_without_commands_or_state_changes() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.embedded_input = true;
        let before = serde_json::to_value(&cockpit.snapshot).unwrap();
        let camera = cockpit.view_mode;
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        for tab in [
            ServiceTab::Cargo,
            ServiceTab::ShipComputer,
            ServiceTab::Layouts,
        ] {
            cockpit.services_open = true;
            cockpit.service_tab = tab;
            let _ = ctx.run(
                egui::RawInput {
                    focused: true,
                    events: vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                },
                |ctx| cockpit.operations_keyboard_navigation(ctx),
            );
            assert!(!cockpit.services_open);
            assert!(cockpit.service_tab == ServiceTab::Navigation);
            assert!(cockpit.workspace_input_waiting_release);
            assert_eq!(cockpit.view_mode, camera);
            assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
            assert!(rx.try_recv().is_err());
            let _ = ctx.run(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: false,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                },
                |_| {},
            );
        }
    }

    #[test]
    fn contextual_interactions_escape_respects_parent_and_text_focus() {
        for text_focus in [false, true] {
            let ctx = egui::Context::default();
            let mut cockpit = view_test_cockpit(&ctx);
            cockpit.services_open = true;
            cockpit.service_tab = ServiceTab::Cargo;
            cockpit.embedded_input = text_focus;
            let field = egui::Id::new("contextual_focus_fixture");
            let mut value = String::from("do not close my editor");
            if text_focus {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.add(egui::TextEdit::singleline(&mut value).id(field))
                            .request_focus();
                    });
                    cockpit.record_operations_keyboard_owner(ctx);
                });
            }
            let _ = ctx.run(
                egui::RawInput {
                    focused: true,
                    events: vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                },
                |ctx| {
                    cockpit.operations_keyboard_navigation(ctx);
                    if text_focus {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.add(egui::TextEdit::singleline(&mut value).id(field));
                        });
                    }
                },
            );
            assert!(cockpit.services_open);
            assert!(cockpit.service_tab == ServiceTab::Cargo);
        }
    }

    #[test]
    fn docking_opens_station_once_and_missing_assets_never_claim_empty_inventory() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let labels = service_test_frame(&ctx, &mut cockpit, vec![], ServiceTab::Hangar);
        assert!(labels
            .iter()
            .any(|(t, _)| t.contains("Waiting for authenticated")));
        assert!(!labels.iter().any(|(t, _)| t == "Board ship"));
        let (tx, rx) = mpsc::channel();
        cockpit.updates = Some(rx);
        let mut docked = cockpit.snapshot.clone().unwrap();
        docked.pilots[0].location = Location::Docked;
        tx.send(Update::Response(
            Box::new(Response {
                physical_cargo: None,
                auth: None,
                ok: true,
                message: "Docked".into(),
                snapshot: Some(docked.clone()),
                assets: Some(asset_test_fixture()),
                resume_token: None,
                ledger: None,
            }),
            true,
        ))
        .unwrap();
        cockpit.poll();
        assert!(cockpit.services_open);
        assert!(cockpit.service_tab == ServiceTab::Station);
        cockpit.services_open = false;
        tx.send(Update::Response(
            Box::new(Response {
                physical_cargo: None,
                auth: None,
                ok: true,
                message: String::new(),
                snapshot: Some(docked),
                assets: Some(asset_test_fixture()),
                resume_token: None,
                ledger: None,
            }),
            false,
        ))
        .unwrap();
        cockpit.poll();
        assert!(!cockpit.services_open);
    }

    #[test]
    fn embedded_initial_docking_does_not_open_services_or_close_explicit_services() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.embedded = true;
        cockpit.services_open = false;
        let mut docked = cockpit.snapshot.take().unwrap();
        docked.pilots[0].location = Location::Docked;
        let (updates, receiver) = mpsc::channel();
        cockpit.updates = Some(receiver);
        let response = || {
            Update::Response(
                Box::new(Response {
                    physical_cargo: None,
                    auth: None,
                    ok: true,
                    message: String::new(),
                    snapshot: Some(docked.clone()),
                    assets: Some(asset_test_fixture()),
                    resume_token: None,
                    ledger: None,
                }),
                false,
            )
        };
        updates.send(Update::Ready).unwrap();
        updates.send(response()).unwrap();
        cockpit.poll();
        assert!(cockpit.connected);
        assert!(
            !cockpit.services_open,
            "initial editor state must not open a floating Services window"
        );
        cockpit.services_open = true;
        cockpit.service_tab = ServiceTab::Cargo;
        updates.send(response()).unwrap();
        cockpit.poll();
        assert!(
            cockpit.services_open,
            "explicitly opened services remain available"
        );
        assert!(cockpit.service_tab == ServiceTab::Cargo);
    }

    #[test]
    fn hangar_boards_docked_hull_and_waits_for_confirmation_but_disables_in_space() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut cockpit = Cockpit::new(
            &cc,
            "fixture".into(),
            std::env::temp_dir().join("arkline-headless-hangar-unused"),
            None,
        );
        cockpit.connected = true;
        cockpit.pilot = "pilot".into();
        cockpit.snapshot = Some(Snapshot {
            carrier_inventory_registration: None,
            walk_sprint_v1: false,
            drone_cargo_registration: None,
            drone_cargo_totals: None,
            docked_traversal_capability: None,
            world_layout: None,
            exterior: None,
            resource_jobs: vec![],
            fleet_links: vec![],
            remote_controls: vec![],
            surface_props: vec![],
            traffic: vec![],
            stored_ore: 0,
            server_time_ms: 0,
            asteroids: vec![],
            online_pilots: vec![],
            orders: vec![],
            belt_ore: 0,
            ledger_sequence: 0,
            ledger_hash: String::new(),
            pilots: vec![PilotView {
                carrier_inventory: None,
                power_accounting: None,
                drone_bay: None,
                drone_cargo: None,
                cockpit_mechanisms: None,
                reactor_configuration: None,
                name: "pilot".into(),
                system_id: HOME_SYSTEM_ID,
                ship_class: ShipClass::Pioneer,
                locked_asteroid: None,
                target_locks: vec![],
                locked_asteroids: vec![],
                surface: None,
                station_avatar: None,
                power: None,
                drones: vec![],
                drone: None,
                target_lock: None,
                mining_modules: vec![],
                credits: 100,
                ore: 25,
                location: Location::Docked,
                flight: None,
            }],
        });
        cockpit.assets = Some(AssetsView {
            carrier_inventory: None,
            drone_bay: None,
            drone_cargo: None,
            cargo_unloading: None,
            pioneer_room: None,
            pioneer_room_layout: None,
            hangar: None,
            pioneer_interior_available: true,
            pioneer_interior_decks: 2,
            ships: ALL_SHIP_CLASSES
                .into_iter()
                .filter(|c| *c != ShipClass::Pioneer)
                .map(|ship_class| HangarShip {
                    power_accounting: None,
                    power: None,
                    ship_class,
                    system_id: HOME_SYSTEM_ID,
                    ore: 0,
                })
                .collect(),
            storage: vec![],
            corporation: None,
            invitations: vec![],
        });
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let render = |cockpit: &mut Cockpit, events: Vec<egui::Event>| {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(900.0, 650.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| cockpit.hangar_panel(ui));
                },
            );
            output
                .shapes
                .into_iter()
                .filter_map(|shape| {
                    if let egui::Shape::Text(text) = shape.shape {
                        Some((
                            text.galley.job.text.clone(),
                            text.pos + text.galley.rect.center().to_vec2(),
                        ))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };
        let click = |cockpit: &mut Cockpit, point: Pos2| {
            render(
                cockpit,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            render(
                cockpit,
                vec![egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        };
        let labels = render(&mut cockpit, vec![]);
        let active = labels.iter().find(|(text, _)| text == "ACTIVE").unwrap().1;
        let pioneer = labels
            .iter()
            .find(|(text, _)| text == ShipClass::Pioneer.spec().name)
            .unwrap()
            .1;
        assert!((active.y - pioneer.y).abs() < 8.0);
        let board = labels
            .iter()
            .find(|(text, _)| text == "Board ship")
            .unwrap()
            .1;
        click(&mut cockpit, board);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::SelectShip {
                ship_class: ShipClass::Prospector
            }
        ));
        assert!(cockpit.request_pending);
        assert_eq!(
            cockpit.own().unwrap().ship_class,
            ShipClass::Pioneer,
            "selection must wait for server confirmation"
        );
        cockpit.snapshot.as_mut().unwrap().pilots[0].ship_class = ShipClass::Prospector;
        cockpit
            .assets
            .as_mut()
            .unwrap()
            .ships
            .retain(|s| s.ship_class != ShipClass::Prospector);
        cockpit.assets.as_mut().unwrap().ships.push(HangarShip {
            power_accounting: None,
            power: None,
            ship_class: ShipClass::Pioneer,
            system_id: HOME_SYSTEM_ID,
            ore: 25,
        });
        cockpit.request_pending = false;
        let labels = render(&mut cockpit, vec![]);
        let active = labels.iter().find(|(text, _)| text == "ACTIVE").unwrap().1;
        let selected = labels
            .iter()
            .find(|(text, _)| text == ShipClass::Prospector.spec().name)
            .unwrap()
            .1;
        assert!((active.y - selected.y).abs() < 8.0);
        cockpit.snapshot.as_mut().unwrap().pilots[0].location = Location::Station;
        let labels = render(&mut cockpit, vec![]);
        let board = labels
            .iter()
            .find(|(text, _)| text == "Board ship")
            .unwrap()
            .1;
        click(&mut cockpit, board);
        assert!(
            rx.try_recv().is_err(),
            "in-space button must not issue SelectShip"
        );
    }

    #[test]
    fn floating_overlay_blocks_scene_pointer_but_empty_space_allows_it() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 650.0));
        for (pointer, expected) in [
            (Pos2::new(60.0, 60.0), false),
            (Pos2::new(600.0, 400.0), true),
        ] {
            // Two frames establish and then test the same interactive layout.
            for _ in 0..2 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events: vec![egui::Event::PointerMoved(pointer)],
                        ..Default::default()
                    },
                    |ctx| {
                        egui::Area::new(egui::Id::new("test_overlay"))
                            .order(egui::Order::Foreground)
                            .fixed_pos(Pos2::new(20.0, 20.0))
                            .show(ctx, |ui| {
                                ui.allocate_exact_size(
                                    Vec2::new(180.0, 100.0),
                                    egui::Sense::click_and_drag(),
                                );
                            });
                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE)
                            .show(ctx, |ui| {
                                assert_eq!(
                                    scene_pointer_allowed(ctx, ui.layer_id(), screen),
                                    expected
                                );
                                assert_eq!(ui.max_rect(), screen);
                            });
                    },
                );
            }
        }
    }

    #[test]
    fn local_map_retains_meter_precision_at_astronomical_origin() {
        let rect = egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 700.0));
        let center = [AU_METERS * 8.0, 0.0, -AU_METERS * 4.0];
        let point = [center[0] + 125.0, 0.0, center[2] - 300.0];
        let pixel = map_project(rect, point, center, 32000.0);
        let roundtrip = map_unproject(rect, pixel, center, 32000.0, 0.0);
        assert!(distance_m(point, roundtrip) < 0.01);
        assert_eq!(format_distance(AU_METERS * 2.0), "2.000 AU");
    }
    #[test]
    fn tactical_pointer_projection_preserves_meter_coordinates() {
        let rect = egui::Rect::from_min_size(egui::pos2(200.0, 100.0), egui::vec2(740.0, 400.0));
        for position in [
            STATION_POSITION,
            [6200.0, 0.0, 0.0],
            [1234.0, 77.0, -2345.0],
        ] {
            let result = map_unproject(
                rect,
                map_project(rect, position, [0.0; 3], 32000.0),
                [0.0; 3],
                32000.0,
                position[1],
            );
            assert!(distance_m(result, position) < 0.01);
        }
        let bounded = map_unproject(rect, egui::pos2(1e18, -1e18), [0.0; 3], 32000.0, 1e18);
        assert!(bounded.iter().all(|v| v.abs() <= SPACE_BOUND));
    }
    #[test]
    fn capital_hull_can_dock_at_server_surface_envelope_boundary() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        let pilot = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        pilot.ship_class = ShipClass::Monolith;
        pilot.location = Location::Station;
        pilot.flight.as_mut().unwrap().position = [
            STATION_POSITION[0] + DOCK_RANGE + hull_radius_m(pilot.ship_class),
            STATION_POSITION[1],
            STATION_POSITION[2],
        ];
        assert!(cockpit.can_dock(&cockpit.own().unwrap()));
        cockpit.snapshot.as_mut().unwrap().pilots[0]
            .flight
            .as_mut()
            .unwrap()
            .position[0] += 0.1;
        assert!(!cockpit.can_dock(&cockpit.own().unwrap()));
    }
    #[test]
    fn action_range_uses_physical_position_and_rejects_docked_or_transit() {
        let mut pilot = PilotView {
            carrier_inventory: None,
            power_accounting: None,
            drone_bay: None,
            drone_cargo: None,
            cockpit_mechanisms: None,
            reactor_configuration: None,
            name: "Range test".into(),
            ship_class: ShipClass::default(),
            system_id: HOME_SYSTEM_ID,
            locked_asteroid: None,
            target_locks: vec![],
            locked_asteroids: vec![],
            surface: None,
            station_avatar: None,
            power: None,
            drones: vec![],
            drone: None,
            target_lock: None,
            mining_modules: vec![],
            credits: 0,
            ore: 0,
            location: Location::Station,
            flight: Some(FlightState {
                docking: None,
                acceleration: [0.; 3],
                position: [BELT_POSITION[0] + MINING_RANGE, 0.0, 0.0],
                velocity: [0.0; 3],
                heading: [1.0, 0.0, 0.0],
                mode: NavigationMode::Stop,
                updated_at_ms: 0,
                warp: None,
                orientation: [0.0, 0.0, 0.0, 1.0],
                angular_velocity: [0.0; 3],
            }),
        };
        assert!(in_navigation_range(&pilot, BELT_POSITION, MINING_RANGE));
        pilot.flight.as_mut().unwrap().position[0] += 0.1;
        assert!(!in_navigation_range(&pilot, BELT_POSITION, MINING_RANGE));
        pilot.flight.as_mut().unwrap().position = STATION_POSITION;
        assert!(in_navigation_range(&pilot, STATION_POSITION, DOCK_RANGE));
        pilot.location = Location::Docked;
        assert!(!in_navigation_range(&pilot, STATION_POSITION, DOCK_RANGE));
        pilot.location = Location::InTransit {
            destination: Destination::Station,
            arrives_at_ms: 100,
        };
        assert!(!in_navigation_range(&pilot, STATION_POSITION, DOCK_RANGE));
        pilot.location = Location::Station;
        pilot.flight = None;
        assert!(!in_navigation_range(&pilot, STATION_POSITION, DOCK_RANGE));
    }
    #[test]
    fn connection_persists_private_credential_and_resumes_identity() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server = listener.local_addr().unwrap().to_string();
        let dir = std::env::temp_dir().join(format!(
            "arkline-client-test-{}-{}",
            std::process::id(),
            listener.local_addr().unwrap().port()
        ));
        let _ = fs::remove_dir_all(&dir);
        let endpoint = std::thread::spawn(move || {
            for reconnect in [false, true] {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                match serde_json::from_str::<Request>(&line).unwrap() {
                    Request::Connect {
                        supports_pioneer_interior: _,
                        supports_fleet_hangar: _,
                        supports_room_controls: _,
                        supports_docked_traversal_v2: _,
                        world_layout_version,
                        protocol_version,
                        pilot,
                        resume_token,
                    } => {
                        assert_eq!(protocol_version, PROTOCOL_VERSION);
                        assert_eq!(world_layout_version, WORLD_LAYOUT_SCHEMA_VERSION);
                        assert_eq!(pilot, "test-pilot");
                        assert_eq!(
                            resume_token.as_deref(),
                            reconnect.then_some("fixture-resume-token")
                        );
                    }
                    _ => panic!("first request must authenticate"),
                }
                let response = Response {
                    physical_cargo: None,
                    auth: None,
                    assets: None,
                    ok: true,
                    message: "Connected".into(),
                    resume_token: Some("fixture-resume-token".into()),
                    snapshot: Some(
                        serde_json::from_value(serde_json::json!({
                            "server_time_ms": 0, "pilots": [], "orders": [],
                            "belt_ore": INITIAL_BELT_ORE, "ledger_sequence": 0,
                            "ledger_hash": GENESIS_HASH
                        }))
                        .unwrap(),
                    ),
                    ledger: None,
                };
                let mut bytes = serde_json::to_vec(&response).unwrap();
                bytes.push(b'\n');
                reader.get_mut().write_all(&bytes).unwrap();
            }
        });
        for _ in 0..2 {
            let (commands_tx, commands_rx) = mpsc::channel();
            let (updates_tx, updates_rx) = mpsc::channel();
            let (address, directory) = (server.clone(), dir.clone());
            let client = std::thread::spawn(move || {
                worker(
                    address,
                    "test-pilot".into(),
                    directory,
                    WorkerIdentity::default(),
                    commands_rx,
                    updates_tx,
                    pilot_input::Mailbox::default(),
                )
            });
            assert!(matches!(
                updates_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
                Update::Ready
            ));
            // Resume credentials never flow into the UI or activity feed.
            match updates_rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                Update::Response(response, _) => assert!(response.resume_token.is_none()),
                _ => panic!("expected successful response"),
            }
            drop(commands_tx);
            client.join().unwrap();
        }
        endpoint.join().unwrap();
        let path = credentials_path(&dir, &server, "test-pilot");
        assert_eq!(fs::read_to_string(&path).unwrap(), "fixture-resume-token");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(target_arch = "wasm32")]
pub fn run_browser() {
    browser::start();
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod interior_acceptance;
