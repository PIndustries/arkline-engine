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
mod effects;
mod flight3d;
mod layout;
pub use layout::panel_geometry as cockpit_panel_geometry;
mod control_bindings;
mod controls_reference;
mod exchange_readiness;
mod flight_workspace;
mod first_person_hud;
mod lighting_settings;
mod motion;
mod nearby_action_model;
mod nearby_interaction_input;
mod operations_workspace;
mod overview_contacts;
mod pilot_input;
mod performance_overlay;
#[cfg(test)]
mod presentation_clock;
mod remote_control_ui;
mod resource_jobs_ui;
mod room_controls;
mod sandbox;
mod services;
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
mod camera_sources;
mod theme;
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

/// Old snapshots retain their exact layout. A declared descriptor must validate;
/// unsupported revisions are never replaced with guessed client coordinates.
fn snapshot_world_layout(snapshot: &Snapshot) -> Result<&WorldLayoutDefinition, String> {
    static LEGACY: std::sync::OnceLock<WorldLayoutDefinition> = std::sync::OnceLock::new();
    let definition = snapshot
        .world_layout
        .as_ref()
        .unwrap_or_else(|| LEGACY.get_or_init(legacy_world_layout));
    definition.validate().map_err(|error| {
        format!("Unsupported world layout: {error}. Reconnect with a compatible client/server.")
    })?;
    Ok(definition)
}

fn validate_snapshot_scene(snapshot: &Snapshot, assets: Option<&AssetsView>) -> Result<(), String> {
    let layout = snapshot_world_layout(snapshot)?;
    if assets
        .and_then(|assets| assets.hangar.as_ref())
        .is_some_and(|hangar| hangar.layout_revision != layout.fleet_layout)
    {
        return Err("Unsupported world layout: station hangar revision contradicts the snapshot descriptor. Reconnect with a compatible client/server.".into());
    }
    Ok(())
}

/// Admission and polling must reject unsupported geometry before credentials,
/// Ready events or a response can escape the transport worker.
fn validate_world_response(response: &Response) -> Result<(), String> {
    if let Some(snapshot) = response.snapshot.as_ref() {
        validate_snapshot_scene(snapshot, response.assets.as_ref())?;
    }
    if let Some(ledger) = response.ledger.as_ref() {
        validate_snapshot_scene(&ledger.snapshot, response.assets.as_ref())?;
        if let Some(snapshot) = response.snapshot.as_ref() {
            if snapshot_world_layout(snapshot)? != snapshot_world_layout(&ledger.snapshot)? {
                return Err("Unsupported world layout: response and ledger describe different realms. Reconnect with a compatible client/server.".into());
            }
        }
    }
    Ok(())
}

fn validate_world_admission_response(response: &Response) -> Result<(), String> {
    if response.ok && response.snapshot.is_none() {
        return Err("Unsupported world layout: admission response omitted its authoritative world snapshot. Reconnect with a compatible client/server.".into());
    }
    validate_world_response(response)
}

#[cfg(not(target_arch = "wasm32"))]
fn terminal_transport_protocol_error(error: &str) -> bool {
    // These prefixes are emitted by our decoders. A new connection cannot make
    // an unsupported reply safe; timeouts and interrupted connections can recover.
    error.starts_with("Invalid server response:")
        || error.starts_with("Invalid server reply:")
        || error.starts_with("Server sent unsupported binary data")
        || error.starts_with("Server response exceeded the size limit")
}

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
    Failure(String),
}

fn resume_guidance(message: String) -> String {
    if message.contains("resume token required for existing pilot") {
        format!("{message}. This pilot needs its saved credential from the original browser or device. Browser and native credentials are stored separately. Retrying alone cannot recover it; use the original client or an authorized recovery process.")
    } else {
        message
    }
}

fn credentials_path(dir: &std::path::Path, server: &str, pilot: &str) -> PathBuf {
    let identity = serde_json::to_vec(&(server, pilot)).unwrap();
    dir.join(format!(
        "pilot-{}.token",
        hex::encode(Sha256::digest(identity))
    ))
}
#[cfg(not(target_arch = "wasm32"))]
fn exchange(reader: &mut BufReader<TcpStream>, request: &Request) -> Result<Response, String> {
    exchange_with_timeout(reader, request, Duration::from_secs(5))
}
#[cfg(not(target_arch = "wasm32"))]
fn exchange_with_timeout(
    reader: &mut BufReader<TcpStream>,
    request: &Request,
    timeout: Duration,
) -> Result<Response, String> {
    let mut bytes = serde_json::to_vec(request).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    reader
        .get_mut()
        .write_all(&bytes)
        .map_err(|e| format!("Connection interrupted while sending request: {e}"))?;
    let deadline = Instant::now() + timeout;
    let mut line = Vec::new();
    const LIMIT: usize = 16 * 1024 * 1024;
    loop {
        if Instant::now() >= deadline {
            return Err("Server response timed out; the connection may have paused during sleep or a network interruption".into());
        }
        // Keep fragments across transient read timeouts, but bound the whole response.
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(250))
            .max(Duration::from_millis(1));
        reader
            .get_mut()
            .set_read_timeout(Some(remaining))
            .map_err(|e| e.to_string())?;
        let consumed = match reader.fill_buf() {
            Ok([]) => {
                return Err("Server closed the connection before completing its response".into())
            }
            Ok(buffer) => {
                let count = buffer
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(buffer.len(), |index| index + 1)
                    .min(LIMIT - line.len());
                line.extend_from_slice(&buffer[..count]);
                count
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue
            }
            Err(e) => {
                return Err(format!(
                    "Connection interrupted while receiving response: {e}"
                ))
            }
        };
        reader.consume(consumed);
        if line.last() == Some(&b'\n') {
            return serde_json::from_slice(&line)
                .map_err(|e| format!("Invalid server response: {e}"));
        }
        if line.len() >= LIMIT {
            return Err("Server response exceeded the size limit".into());
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn connect_transport(server: &str) -> Result<BufReader<TcpStream>, String> {
    let addresses: Vec<_> = server
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .collect();
    let stream = addresses
        .iter()
        .find_map(|address| TcpStream::connect_timeout(address, Duration::from_secs(3)).ok())
        .ok_or_else(|| {
            format!("Cannot connect to {server}. Start arkline-server, then reconnect.")
        })?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream.set_nodelay(true).map_err(|e| e.to_string())?;
    Ok(BufReader::new(stream))
}
/// Drop all requests accumulated before recovery, never replay actions or controls.
#[cfg(not(target_arch = "wasm32"))]
fn discard_pending(commands: &Receiver<Request>, controls: &pilot_input::Mailbox) -> bool {
    controls.clear();
    loop {
        match commands.try_recv() {
            Ok(_) => {}
            Err(mpsc::TryRecvError::Empty) => return true,
            Err(mpsc::TryRecvError::Disconnected) => return false,
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn save_token(path: &std::path::Path, token: &str) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid credential directory")?;
    fs::create_dir_all(parent).map_err(|e| format!("Cannot create credential directory: {e}"))?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        if path.exists() {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("Cannot save resume credential: {e}"))?;
    file.write_all(token.as_bytes())
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())
}
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct WorkerIdentity {
    scope: Option<String>,
    account: Option<account_login::Credential>,
    link_existing: bool,
}
#[cfg(not(target_arch = "wasm32"))]
impl WorkerIdentity {
    fn request(&self, server: &str, pilot: &str, token: Option<String>) -> Result<Request, String> {
        if let Some(account) = &self.account {
            if !account.matches(server, pilot) {
                return Err("Account session expired or changed. Sign in again.".into());
            }
            Ok(Request::AccountConnect {
                access_token: account.token.clone(),
                pilot: pilot.into(),
                legacy_resume_token: if self.link_existing { token } else { None },
                link_existing: self.link_existing,
                protocol_version: PROTOCOL_VERSION,
                supports_pioneer_interior: true,
                supports_fleet_hangar: true,
                supports_room_controls: true,
                supports_docked_traversal_v2: true,
                world_layout_version: WORLD_LAYOUT_SCHEMA_VERSION,
            })
        } else {
            Ok(Request::Connect {
                pilot: pilot.into(),
                resume_token: token,
                protocol_version: PROTOCOL_VERSION,
                supports_pioneer_interior: true,
                supports_fleet_hangar: true,
                supports_room_controls: true,
                supports_docked_traversal_v2: true,
                world_layout_version: WORLD_LAYOUT_SCHEMA_VERSION,
            })
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn worker(
    server: String,
    pilot: String,
    dir: PathBuf,
    identity: WorkerIdentity,
    commands: Receiver<Request>,
    updates: Sender<Update>,
    controls: pilot_input::Mailbox,
) {
    let run = || -> Result<(), String> {
        let path = credentials_path(&dir, identity.scope.as_deref().unwrap_or(&server), &pilot);
        let mut token = if identity.account.is_some() && !identity.link_existing {
            None
        } else {
            match fs::read_to_string(&path) {
                Ok(v) => Some(v),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(format!("Cannot read saved credential: {e}")),
            }
        };
        if identity.account.is_some() && identity.link_existing && token.is_none() {
            return Err("Cannot link: this device has no saved credential for that pilot. Use the original device or operator recovery.".into());
        }
        // Check storage before creating a server-side identity.
        if identity.account.is_none() {
            fs::create_dir_all(&dir)
                .map_err(|e| format!("Credential directory unavailable: {e}"))?;
        }
        let mut reader = transport::Connection::connect(&server)?;
        let mut response = reader.exchange(&identity.request(&server, &pilot, token.clone())?)?;
        if !response.ok {
            return Err(response.message);
        }
        validate_world_admission_response(&response)?;
        if identity.account.is_some() && response.resume_token.is_some() {
            return Err("Account admission returned an unexpected legacy credential".into());
        }
        if let Some(issued) = response.resume_token.take() {
            save_token(&path, &issued)?;
            token = Some(issued);
        }
        updates.send(Update::Ready).map_err(|e| e.to_string())?;
        updates
            .send(Update::Response(Box::new(response), true))
            .map_err(|e| e.to_string())?;
        let mut last_snapshot = Instant::now();
        let mut retries_left = 3u32;
        loop {
            if identity
                .account
                .as_ref()
                .is_some_and(|a| !a.matches(&server, &pilot))
            {
                reader.close();
                return Err("Account session expired. Sign in again.".into());
            }
            let (request, visible) = match commands.recv_timeout(Duration::from_millis(50)) {
                Ok(request) => (request, true),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(request) = controls.take() {
                        (request, false)
                    } else if last_snapshot.elapsed() >= Duration::from_millis(350) {
                        (Request::Snapshot, false)
                    } else {
                        continue;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            };
            // Lease transitions invalidate controls sampled under the old view,
            // including unwrapped own-ship samples queued before remote takeover.
            if matches!(
                request,
                Request::BeginRemoteControl { .. } | Request::EndRemoteControl
            ) {
                controls.clear();
            }
            let response = match reader.exchange(&request) {
                Ok(response) => response,
                Err(error) => {
                    if terminal_transport_protocol_error(&error) {
                        controls.clear();
                        reader.close();
                        return Err(error);
                    }
                    let uncertainty = if visible {
                        " Action outcome unknown; inspect refreshed state before retrying. The action will not be replayed."
                    } else {
                        ""
                    };
                    let explanation = format!("{error}.{uncertainty} Reconnecting; queued actions and flight input are discarded.");
                    updates
                        .send(Update::Reconnecting(explanation.clone()))
                        .map_err(|e| e.to_string())?;
                    reader.close();
                    if token.is_none() && identity.account.is_none() {
                        return Err(format!("{explanation} No saved identity is available for safe automatic recovery."));
                    }
                    let mut recovered = None;
                    let mut last_error = error;
                    while retries_left > 0 {
                        retries_left -= 1;
                        if !discard_pending(&commands, &controls) {
                            return Ok(());
                        }
                        // A dropped sender cancels recovery promptly; received requests are discarded.
                        let wait_until =
                            Instant::now() + Duration::from_millis(250 * (3 - retries_left) as u64);
                        while Instant::now() < wait_until {
                            match commands
                                .recv_timeout(wait_until.saturating_duration_since(Instant::now()))
                            {
                                Ok(_) => {}
                                Err(mpsc::RecvTimeoutError::Timeout) => break,
                                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                            }
                        }
                        let attempt = (|| {
                            let mut next = transport::Connection::connect(&server)?;
                            let mut resumed = next
                                .exchange(&identity.request(&server, &pilot, token.clone())?)
                                .map_err(|error| {
                                    if terminal_transport_protocol_error(&error) {
                                        format!("Resume rejected: {error}")
                                    } else {
                                        error
                                    }
                                })?;
                            if !resumed.ok {
                                return Err(format!("Resume rejected: {}", resumed.message));
                            }
                            validate_world_admission_response(&resumed)
                                .map_err(|error| format!("Resume rejected: {error}"))?;
                            if identity.account.is_some() && resumed.resume_token.is_some() {
                                return Err(
                                    "Account admission returned an unexpected legacy credential"
                                        .into(),
                                );
                            }
                            if let Some(issued) = resumed.resume_token.take() {
                                save_token(&path, &issued)?;
                                token = Some(issued);
                            }
                            Ok::<_, String>((next, resumed))
                        })();
                        match attempt {
                            Ok(value) => {
                                recovered = Some(value);
                                break;
                            }
                            Err(error) => {
                                let rejected = error.starts_with("Resume rejected:");
                                last_error = error;
                                if rejected {
                                    break;
                                }
                            }
                        }
                    }
                    let Some((next, mut resumed)) = recovered else {
                        return Err(format!("Automatic reconnect stopped: {last_error}.{uncertainty} Use Reconnect when the server is available."));
                    };
                    if !discard_pending(&commands, &controls) {
                        return Ok(());
                    }
                    reader = next;
                    resumed.message =
                        format!("Connection restored; authoritative state refreshed.{uncertainty}");
                    updates.send(Update::Ready).map_err(|e| e.to_string())?;
                    updates
                        .send(Update::Response(Box::new(resumed), true))
                        .map_err(|e| e.to_string())?;
                    last_snapshot = Instant::now();
                    continue;
                }
            };
            validate_world_response(&response)?;
            last_snapshot = Instant::now();
            updates
                .send(Update::Response(Box::new(response), visible))
                .map_err(|e| e.to_string())?;
        }
    };
    if let Err(error) = run() {
        let _ = updates.send(Update::Failure(error));
    }
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
    performance_open: bool,
    performance_snapshot_received: Option<Instant>,
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
    ship_layout: ship_layouts::LayoutViewer,
    selected_object: SelectedObject,
    mining_target: Option<u64>,
    camera_source: camera_sources::CameraSource,
    drone_camera: bool,
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
            performance_open: false,
            performance_snapshot_received: None,
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
            ship_layout: ship_layouts::LayoutViewer::default(),
            selected_object: SelectedObject::Station,
            mining_target: None,
            camera_source: Default::default(),
            drone_camera: true,
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
    fn settings_panel(&mut self, ui: &mut egui::Ui) {
        if ui.button("Controls & ownership").clicked() {
            self.open_controls_reference();
        }
        let context = sound_settings::SoundContext {
            paused: self.embedded && self.embedded_paused,
            live_world: self.operations_fresh() && self.own().is_some(),
            listening: self.voice.is_listening(),
            speaking: self.voice.is_speaking(),
        };
        let action = self.sound_settings.ui(
            ui,
            &self.sound.output_status(),
            context,
            &self.sound_test_notice,
        );
        if action.changed {
            if let Err(error) = self.sound_settings.save(&self.data_dir) {
                self.settings_notice = error;
            }
        }
        if action.retry {
            self.sound.retry();
            self.sound_test_notice =
                "Audio retry requested · output status below reports the result".into();
        }
        if action.test {
            self.sound
                .set_mix(self.preferred_sound_mix(self.sound.current_mix()));
            self.sound_test_notice = match self.sound.test_output() {
                Ok(()) => "Two-tone test requested · listen to your output device".into(),
                Err(error) => error,
            };
        }
        ui.separator();
        ui.label("LIGHTING QUALITY");
        let previous = self.flight_view.lighting_quality;
        ui.horizontal_wrapped(|ui| {
            for quality in lighting_settings::LightingQuality::ALL {
                ui.selectable_value(
                    &mut self.flight_view.lighting_quality,
                    quality,
                    quality.label(),
                );
            }
        });
        ui.small("Low disables cast shadows. Balanced and High improve nearby shadows; drone screens use a smaller budget.");
        if previous != self.flight_view.lighting_quality {
            if let Err(error) = self.flight_view.lighting_quality.save(&self.data_dir) {
                self.settings_notice = error;
            }
        }
        ui.separator();
        ui.small("VIEW MODE");
        ui.horizontal_wrapped(|ui| self.view_selector(ui));
        ui.separator();
        ui.label(
            egui::RichText::new("COCKPIT APPEARANCE")
                .color(self.accent())
                .strong(),
        );
        ui.label("Choose a local interface accent. Your universe and pilot state are unchanged.");
        ui.separator();
        ui.label("Panel layout");
        if ui
            .checkbox(
                &mut self.layout.pinned,
                "Pin panels (disable moving and resizing)",
            )
            .changed()
        {
            self.layout.settings_changed();
        }
        if ui
            .checkbox(
                &mut self.layout.snap,
                "Snap panels to viewport edges and center",
            )
            .changed()
        {
            self.layout.settings_changed();
        }
        if ui.button("Reset panel layout").clicked() {
            self.layout.reset();
        }
        ui.small("Drag panel titles; release near an edge to snap. Layout saves locally and follows viewport resizing.");
        let previous = self.theme;
        ui.horizontal_wrapped(|ui| {
            for theme in theme::Theme::ALL {
                ui.selectable_value(
                    &mut self.theme,
                    theme,
                    egui::RichText::new(theme.name()).color(theme.accent()),
                );
            }
        });
        if self.theme != previous {
            self.theme.apply(ui.ctx());
            self.settings_notice = match self.theme.save(&self.data_dir) {
                Ok(()) => format!("{} appearance saved", self.theme.name()),
                Err(error) => error,
            };
        }
        ui.small("Near-black panels · High-contrast interface accents · Saved on this computer");
        ui.small(
            "World materials retain their physical colors; this setting changes the interface.",
        );
        if !self.settings_notice.is_empty() {
            ui.colored_label(MUTED, &self.settings_notice);
        }
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
        let (mut mix, cues) =
            self.sound_scene
                .update(self.snapshot.as_ref(), &name, self.connected, now);
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
        self.can_operate()
            && self.own().is_some_and(|p| {
                p.station_avatar.is_none()
                    && p.flight.is_some()
                    && !matches!(
                        p.location,
                        Location::Docked | Location::InTransit { .. } | Location::Jumping { .. }
                    )
            })
    }
    fn warp_power_ready(&self) -> bool {
        self.own().is_none_or(|p| {
            p.power
                .as_ref()
                .is_none_or(|power| power.charge_j >= power_spec(p.ship_class).warp_cost_j)
        })
    }
    fn can_warp(&self) -> bool {
        self.can_fly() && self.warp_power_ready()
    }
    fn can_operate(&self) -> bool {
        self.connected && !self.request_pending && !self.embedded_paused
    }
    fn send(&mut self, request: Request) {
        if !self.can_operate() {
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
        if !matches!(request, Request::Snapshot | Request::ExportLedger) {
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
            match update {
                Update::Ready => {
                    self.invalidate_ai_session();
                    self.frozen_time = None;
                    self.timeline_origin = None;
                    self.motion.clear();
                    self.effects.clear();
                    self.snapshot = None;
                    self.connected = true;
                    self.connecting = false;
                    self.notice.clear();
                }
                Update::Reconnecting(message) => {
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
                }
                Update::Response(response, visible) => {
                    if let Some(assets) = response.assets {
                        self.assets = Some(assets);
                    }
                    if visible {
                        self.request_pending = false;
                    }
                    if let Some(snapshot) = response.snapshot {
                        if let Err(error) = validate_snapshot_scene(&snapshot, self.assets.as_ref())
                        {
                            self.reject_world_layout(error);
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
                        self.motion.observe(&snapshot);
                        self.performance_snapshot_received = Some(Instant::now());
                        self.snapshot = Some(snapshot);
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
    fn reject_world_layout(&mut self, error: String) {
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
    fn operation_status(&self) -> (String, String, Option<f32>, Color32) {
        if !self.connected {
            return (
                if self.connecting {
                    "CONNECTING"
                } else {
                    "OFFLINE"
                }
                .into(),
                if self.notice.is_empty() {
                    "Waiting for a live server connection".into()
                } else {
                    self.notice.clone()
                },
                None,
                AMBER,
            );
        }
        if self.request_pending {
            return (
                "COMMAND PENDING".into(),
                "Awaiting server confirmation".into(),
                None,
                AMBER,
            );
        }
        let Some(pilot) = self.own() else {
            return (
                "SYNCHRONIZING".into(),
                "Waiting for vessel snapshot".into(),
                None,
                MUTED,
            );
        };
        if let Some(state) = pilot
            .station_avatar
            .as_ref()
            .and_then(|avatar| avatar.docked_traversal.as_ref())
        {
            return (
                format!("ON FOOT / {}", station_ui::docked_state_label(state)),
                if pilot.location == Location::Docked {
                    station_ui::docked_exit_guidance(state).into()
                } else {
                    "Autopilot continues · return to the cockpit chair to pilot · exterior access locked".into()
                },
                None,
                self.accent(),
            );
        }
        if let Some(avatar) = &pilot.station_avatar {
            let (title, guidance) = if avatar.onboard_interior {
                (
                    format!("ON FOOT / PIONEER DECK {}", avatar.interior_deck),
                    if avatar.interior_deck == 1 {
                        "Return to the airlock to leave; access-core lift leads to deck 2".into()
                    } else {
                        "Use the access-core lift to deck 1 for the airlock exit".into()
                    },
                )
            } else if self.in_captains_quarters() {
                (
                    "ON FOOT / CAPTAIN'S QUARTERS".into(),
                    "Return through the doorway to the hangar boarding marker".into(),
                )
            } else {
                (
                    "ON FOOT / STATION HANGAR".into(),
                    "Walk to the boarding marker to enter your ship".into(),
                )
            };
            return (title, guidance, None, self.accent());
        }
        let now = self.presentation_time();
        if let Location::InTransit {
            destination,
            arrives_at_ms,
        } = pilot.location
        {
            let destination = match destination {
                Destination::Belt => "asteroid belt".to_string(),
                Destination::Station => "station".to_string(),
                Destination::Gate { id } => gates()
                    .into_iter()
                    .find(|g| g.id == id)
                    .map(|g| g.name)
                    .unwrap_or_else(|| "stargate".into()),
            };
            if let Some((phase, progress)) = motion::warp_phase(&pilot, now) {
                return (
                    phase.into(),
                    format!(
                        "To {destination} · {} remaining · {:.1}s",
                        pilot
                            .flight
                            .as_ref()
                            .and_then(|f| f.warp.as_ref())
                            .map_or_else(
                                || "—".into(),
                                |path| format_distance(distance_m(
                                    motion::position(&pilot, now),
                                    path.target
                                ))
                            ),
                        arrives_at_ms.saturating_sub(now) as f32 / 1000.0
                    ),
                    Some(progress),
                    self.accent(),
                );
            }
            return (
                "ARRIVAL CHECK".into(),
                "Awaiting server-confirmed arrival".into(),
                Some(1.0),
                self.accent(),
            );
        }
        if let Location::Jumping {
            gate_id,
            arrives_at_ms,
        } = pilot.location
        {
            let remaining = arrives_at_ms.saturating_sub(now);
            let route = gates()
                .into_iter()
                .find(|g| g.id == gate_id)
                .map(|g| system_name(g.destination_system_id))
                .unwrap_or_else(|| "destination area".into());
            return (
                "STARGATE TRANSIT".into(),
                format!("To {route} · {:.1}s", remaining as f32 / 1000.0),
                Some(1.0 - remaining.min(JUMP_DURATION_MS) as f32 / JUMP_DURATION_MS as f32),
                self.accent(),
            );
        }
        if let Some(module) = pilot
            .mining_modules
            .iter()
            .filter(|m| m.target_id.is_some())
            .min_by_key(|m| m.cycle_completes_at_ms)
        {
            let count = pilot
                .mining_modules
                .iter()
                .filter(|m| m.target_id.is_some())
                .count();
            let remaining = module
                .cycle_completes_at_ms
                .unwrap_or(now)
                .saturating_sub(now);
            return (
                "MINING MODULES ACTIVE".into(),
                format!(
                    "{count} active turret(s) · next cycle {:.1}s · ore credited on completion",
                    remaining as f64 / 1000.0
                ),
                None,
                self.accent(),
            );
        }
        if self.mining_effect_target().is_some() {
            if let Some((progress, units)) = self.effects.mining(now) {
                return (
                    "MINING LASER ACTIVE".into(),
                    format!("+{units} ore confirmed; cargo already credited"),
                    Some(progress),
                    self.accent(),
                );
            }
        }
        if let Some(event) = self.effects.events.iter().rev().find(|event| {
            matches!(
                (event.kind, &pilot.location),
                (effects::EffectKind::Docking, Location::Docked)
                    | (effects::EffectKind::Undocking, Location::Station)
            )
        }) {
            return match event.kind {
                effects::EffectKind::Docking => (
                    "DOCKING CONFIRMED".into(),
                    "Station services available".into(),
                    Some(1.0),
                    self.accent(),
                ),
                _ => (
                    "UNDOCKED".into(),
                    "Navigation available".into(),
                    Some(1.0),
                    self.accent(),
                ),
            };
        }
        if pilot.location != Location::Docked {
            if let Some(flight) = &pilot.flight {
                let label = match flight.mode {
                    NavigationMode::Manual => "DIRECT FLIGHT / ASSISTED BRAKING",
                    NavigationMode::Stop => "STOP / BRAKING",
                    NavigationMode::Approach { .. } => "APPROACHING TARGET",
                    NavigationMode::Orbit { .. } => "ORBITING TARGET",
                    NavigationMode::KeepRange { .. } => "KEEPING RANGE",
                    NavigationMode::FlyTo { .. } => "FLYING TO POINT",
                };
                return (
                    label.into(),
                    format!(
                        "{:.0} m/s · server-authoritative flight",
                        distance_m(flight.velocity, [0.0; 3])
                    ),
                    Some(
                        (distance_m(flight.velocity, [0.0; 3]) / MAX_SHIP_SPEED).clamp(0.0, 1.0)
                            as f32,
                    ),
                    self.accent(),
                );
            }
        }
        match pilot.location {
            Location::Docked => (
                "DOCKED / SECURE".into(),
                "Station exchange ready".into(),
                None,
                self.accent(),
            ),
            Location::Station => (
                "FLIGHT READY".into(),
                "Select the asteroid belt to begin a route".into(),
                None,
                self.accent(),
            ),
            Location::Belt => (
                "EXTRACTION READY".into(),
                "Lock an asteroid, then activate a mining turret".into(),
                None,
                self.accent(),
            ),
            Location::InTransit { .. } | Location::Jumping { .. } => unreachable!(),
        }
    }
    fn view_selector(&mut self, ui: &mut egui::Ui) {
        self.flight_workspace_view_selector(ui);
    }
    fn mining_status(&self) -> (String, Option<f32>) {
        let Some(pilot) = self.own() else {
            return ("Select an asteroid and lock".into(), None);
        };
        if !self.connected {
            return ("Offline · last known state".into(), None);
        }
        if let Some(id) = self.module_target(&pilot) {
            let name = self.asteroid_name(id);
            if let Some(asteroid) = self
                .snapshot
                .as_ref()
                .and_then(|s| s.asteroids.iter().find(|a| a.id == id))
            {
                if asteroid.ore == 0 {
                    return (format!("Depleted · {name}"), None);
                }
                if !effects::can_extract_target(&pilot, asteroid) {
                    return (format!("Out of extraction range · {name}"), None);
                }
            }
            return (format!("Locked · {name}"), None);
        }
        if let Some(lock) = pilot.target_locks.first().or(pilot.target_lock.as_ref()) {
            let now = self.presentation_time();
            let remaining = lock.completes_at_ms.saturating_sub(now);
            return (
                format!(
                    "Acquiring {} · {}",
                    self.asteroid_name(lock.asteroid_id),
                    if remaining == 0 {
                        "confirming…".into()
                    } else {
                        format!("{:.1}s", remaining as f64 / 1000.0)
                    }
                ),
                Some(
                    (now.saturating_sub(lock.started_at_ms) as f32
                        / lock
                            .completes_at_ms
                            .saturating_sub(lock.started_at_ms)
                            .max(1) as f32)
                        .clamp(0.0, 1.0),
                ),
            );
        }
        ("Select an asteroid and lock".into(), None)
    }
    fn mining_module(&mut self, ui: &mut egui::Ui) {
        ui.small(
            egui::RichText::new("MINING MODULE")
                .strong()
                .color(self.accent()),
        );
        self.manual_operations_summary(ui);
        let fresh = self.operations_fresh();
        let seated = self
            .own()
            .is_some_and(|pilot| pilot.station_avatar.is_none());
        ui.add_enabled_ui(seated, |ui| self.target_chips(ui));
        let (status, progress) = self.mining_status();
        ui.small(status);
        if let Some(progress) = progress {
            ui.add(
                egui::ProgressBar::new(progress)
                    .desired_height(4.0)
                    .fill(self.accent()),
            );
        }
        if let Some(pilot) = self.own() {
            let spec = mining_spec(pilot.ship_class);
            let cost = power_spec(pilot.ship_class).mining_cycle_cost_j;
            ui.colored_label(
                if pilot.power.as_ref().is_some_and(|p| p.charge_j < cost) {
                    AMBER
                } else {
                    MUTED
                },
                format!(
                    "Capacitor {cost} J / turret cycle{}",
                    if pilot.power.as_ref().is_some_and(|p| p.charge_j < cost) {
                        " · low charge"
                    } else {
                        ""
                    }
                ),
            );
            ui.small(format!(
                "Up to {} ore / {:.1}s per turret · repeats until stopped",
                spec.yield_units,
                spec.cycle_ms as f64 / 1000.0
            ));
            if pilot.mining_modules.is_empty() {
                ui.small("Waiting for mining module state…");
            }
            for module in &pilot.mining_modules {
                ui.push_id(module.module_id, |ui| {
                    let active = module.target_id.is_some();
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(
                            if active { self.accent() } else { MUTED },
                            format!(
                                "{} TURRET {} · {}",
                                if active { "●" } else { "○" },
                                module.module_id + 1,
                                if module.module_id == 0 {
                                    "LEFT"
                                } else {
                                    "RIGHT"
                                }
                            ),
                        );
                        if active {
                            ui.small(self.asteroid_name(module.target_id.unwrap()));
                            let remaining = module
                                .cycle_completes_at_ms
                                .unwrap_or(0)
                                .saturating_sub(self.presentation_time());
                            ui.small(if remaining == 0 {
                                "Confirming cycle…".into()
                            } else {
                                format!("{:.1}s", remaining as f64 / 1000.0)
                            });
                            if ui
                                .add_enabled(
                                    self.can_operate() && seated && fresh,
                                    egui::Button::new("Stop turret").small(),
                                )
                                .clicked()
                            {
                                self.send(Request::StopMining {
                                    module_id: module.module_id,
                                });
                            }
                        } else if ui
                            .add_enabled(
                                self.can_mine(&pilot),
                                egui::Button::new("Activate turret").small(),
                            )
                            .clicked()
                        {
                            self.start_module(module.module_id);
                        }
                    });
                    if let (Some(start), Some(end)) =
                        (module.cycle_started_at_ms, module.cycle_completes_at_ms)
                    {
                        let progress = (self.presentation_time().saturating_sub(start) as f32
                            / end.saturating_sub(start).max(1) as f32)
                            .clamp(0.0, 1.0);
                        ui.add(
                            egui::ProgressBar::new(progress)
                                .desired_height(4.0)
                                .fill(self.accent()),
                        );
                    } else if let Some(reason) = &module.stop_reason {
                        ui.small(reason);
                    }
                });
            }
            if self.selected_asteroid().is_some()
                && ui
                    .add_enabled(self.can_lock_selected(), egui::Button::new("Lock").small())
                    .clicked()
            {
                if let Some(a) = self.selected_asteroid() {
                    self.send(Request::LockAsteroid { id: a.id });
                }
            }
        }
        if let Some(pilot) = self.own() {
            if (!Self::pending_locks(&pilot).is_empty()
                || !Self::confirmed_locks(&pilot).is_empty())
                && ui
                    .add_enabled(
                        self.can_operate() && seated && fresh,
                        egui::Button::new(
                            if Self::pending_locks(&pilot).len()
                                + Self::confirmed_locks(&pilot).len()
                                > 1
                            {
                                "Unlock all targets"
                            } else if pilot.target_lock.is_some() {
                                "Cancel lock"
                            } else {
                                "Unlock target"
                            },
                        )
                        .small(),
                    )
                    .clicked()
            {
                self.send(Request::UnlockTarget);
            }
        }
    }

    fn cockpit_actions(&mut self, ui: &mut egui::Ui) {
        let Some(pilot) = self.own() else { return };
        let readiness = self.navigation_order_reason();
        let ready = readiness.is_none() && self.live_command_snapshot_fresh();
        let docked = pilot.location == Location::Docked;
        let (status, help) = if !self.connected {
            (
                "OFFLINE",
                "Reconnect to the authority before issuing ship orders",
            )
        } else if self.embedded_paused {
            ("PAUSED", "Resume simulation before issuing ship orders")
        } else if self.request_pending {
            ("PENDING", "Command queued; awaiting the authority response")
        } else if !self.live_command_snapshot_fresh() {
            (
                "STALE",
                "Last received readings; waiting for a fresh authority snapshot",
            )
        } else if pilot.station_avatar.is_some() {
            (
                "ON FOOT",
                "Physically return to the pilot seat for ship orders",
            )
        } else if docked {
            ("DOCKED", "Authority reports the ship docked")
        } else if matches!(pilot.location, Location::InTransit { .. }) {
            (
                "WARP",
                "Transit in progress; berth and stop controls unavailable",
            )
        } else if matches!(pilot.location, Location::Jumping { .. }) {
            (
                "JUMP",
                "Gate transit in progress; berth and stop controls unavailable",
            )
        } else if self.can_dock(&pilot) {
            (
                "PORT IN RANGE",
                "Authority reports the docking port within range",
            )
        } else {
            ("IN FLIGHT", "Approach the station docking port to dock")
        };
        let state_color = if ready { self.accent() } else { AMBER };
        let text_size = if self.cockpit_console_detail {
            14.0
        } else {
            22.0
        };
        let caption_size = if self.cockpit_console_detail {
            12.0
        } else {
            16.0
        };
        // Absolute canvas rectangles are shared with the physical cartridges.
        // Help/status length and compact/detail typography cannot move the hit faces.
        ui.allocate_space(Vec2::new(560.0, 82.0));
        for (label, x) in [("BERTH", 12.0), ("FLIGHT", 164.0), ("AUTHORITY", 280.0)] {
            ui.painter().text(Pos2::new(x, 12.0), egui::Align2::LEFT_TOP,
                label, egui::FontId::proportional(caption_size), MUTED);
        }
        let (label, enabled, command) = if docked {
            ("UNDOCK", ready && self.can_operate() && pilot.station_avatar.is_none(), Request::Undock)
        } else { ("DOCK", ready && self.can_dock(&pilot), Request::Dock) };
        let berth_rect = egui::Rect::from_min_max(Pos2::new(12.0, 32.0), Pos2::new(144.0, 70.0));
        let mut berth_ui = ui.new_child(egui::UiBuilder::new().id_salt("physical_berth").max_rect(berth_rect));
        berth_ui.set_enabled(enabled);
        let berth = berth_ui.put(berth_rect,
            egui::Button::new(egui::RichText::new(label).strong().size(text_size)).frame(false));
        if berth.contains_pointer() {
            berth.show_tooltip_ui(|ui| { ui.set_max_width(400.0); ui.label(help); });
        }
        if berth.clicked() { self.send(command); }
        let braking = matches!(pilot.location, Location::Station | Location::Belt)
            && pilot.flight.as_ref().is_some_and(|flight| matches!(flight.mode, NavigationMode::Stop));
        let stop_rect = egui::Rect::from_min_max(Pos2::new(164.0, 32.0), Pos2::new(260.0, 70.0));
        let mut stop_ui = ui.new_child(egui::UiBuilder::new().id_salt("physical_stop").max_rect(stop_rect));
        stop_ui.set_enabled(ready && self.can_fly());
        let stop = stop_ui.put(stop_rect,
            egui::Button::new(egui::RichText::new("STOP").size(text_size)).frame(false));
        if stop.contains_pointer() {
            stop.show_tooltip_ui(|ui| { ui.set_max_width(400.0); ui.label(help); });
        }
        if stop.clicked() { self.send(Request::Navigate { mode: NavigationMode::Stop }); }
        // Real hover/pressed and confirmed braking feedback belongs to the
        // unchanged input face; no opaque UI rectangle conceals modeled relief.
        for (response, confirmed) in [(&berth, false), (&stop, braking)] {
            if response.hovered() || response.is_pointer_button_down_on() || confirmed {
                ui.painter().rect_stroke(response.rect.shrink(1.0), 2.0,
                    egui::Stroke::new(if response.is_pointer_button_down_on() { 2.0 } else { 1.0 }, self.accent()),
                    egui::StrokeKind::Inside);
            }
        }
        ui.painter().text(Pos2::new(280.0, 33.0), egui::Align2::LEFT_TOP,
            status, egui::FontId::proportional(caption_size), state_color);
        if ui.put(egui::Rect::from_min_max(Pos2::new(280.0, 54.0), Pos2::new(420.0, 78.0)),
            egui::Button::new(egui::RichText::new("Expand panel").size(18.0)).small()).clicked() { self.services_open = true; }
        // Compact status already names the authority state. Keep detailed
        // guidance on both control faces, reserving rows for actual readings.
        if self.cockpit_console_detail {
            ui.label(egui::RichText::new(help).size(14.0).color(MUTED));
        } else {
            ui.skip_ahead_auto_ids(1); // Preserve the removed label's ID slot.
        }
        if self.snapshot.as_ref().is_some_and(|snapshot| snapshot.resource_jobs.iter()
            .any(|job| job.pilot == pilot.name && !job.phase.is_terminal())) {
            ui.label(egui::RichText::new("Manual ship orders can cancel the active mining routine")
                .size(14.0).color(AMBER));
        }

    }
    /// Workspace selectors share their fixed canvas faces with the modeled
    /// cartridges. These change local UI context, never ship authority.
    fn cockpit_workspace_keys(&mut self, ui: &mut egui::Ui) {
        for (label, id, tab, left, right) in [
            ("FLIGHT", "cockpit.workspace.flight", ServiceTab::Navigation, 12.0, 180.0),
            ("DRONES", "cockpit.workspace.drones", ServiceTab::Drones, 196.0, 364.0),
            ("MINING", "cockpit.workspace.mining", ServiceTab::ShipComputer, 380.0, 548.0),
        ] {
            let rect = egui::Rect::from_min_max(Pos2::new(left, 330.0), Pos2::new(right, 374.0));
            let mut key_ui = ui.new_child(egui::UiBuilder::new().id_salt(id).max_rect(rect));
            let response = key_ui.put(rect, egui::Button::new(
                egui::RichText::new(label).strong().size(24.0)).frame(false));
            let selected = self.services_open && self.service_tab == tab;
            if selected || response.hovered() || response.is_pointer_button_down_on() {
                ui.painter().rect_stroke(rect.shrink(2.0), 2.0,
                    egui::Stroke::new(if selected || response.is_pointer_button_down_on() { 2.0 } else { 1.0 }, self.accent()),
                    egui::StrokeKind::Inside);
            }
            response.clone().on_hover_text("Open workspace · view selection only; ship control remains unchanged");
            if response.clicked() {
                self.service_tab = tab;
                self.services_open = true;
            }
        }
    }
    fn power_gauge(&self, ui: &mut egui::Ui) {
        let Some(pilot) = self.own() else { return };
        if let Some(power) = &pilot.power {
            let fraction = if power.capacity_j == 0 {
                0.0
            } else {
                (power.charge_j as f64 / power.capacity_j as f64).clamp(0.0, 1.0) as f32
            };
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_height(18.0)
                    .fill(self.accent())
                    .text(format!(
                        "Capacitor {:.1}/{:.1} kJ · regen rating {} W{}",
                        power.charge_j as f64 / 1000.0,
                        power.capacity_j as f64 / 1000.0,
                        power.regen_w,
                        if !self.connected { " · last known" } else if self.embedded_paused { " · paused" } else if !self.live_command_snapshot_fresh() { " · stale" } else { "" }
                    )),
            )
            .on_hover_text({let spec=power_spec(pilot.ship_class);format!("Server-confirmed energy. Warp {} J; turret cycle {} J; mining drone cycle {} J; drone weapon shot {} J.",spec.warp_cost_j,spec.mining_cycle_cost_j,spec.drone_mining_cycle_cost_j,spec.drone_attack_cost_j)});
        } else {
            ui.small("Capacitor data unavailable");
        }
    }
    /// Read-only content; physical placement and input authority remain external.
    fn compact_flight_summary(&self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("FLIGHT CONSOLE").size(20.0).color(MUTED));
        let Some(pilot) = self.own() else {
            ui.label("Waiting for vessel telemetry");
            return;
        };
        let mode = match pilot.location {
            Location::Docked => "Docked",
            Location::InTransit { .. } => "Warp transit",
            Location::Jumping { .. } => "Gate transit",
            _ => pilot
                .flight
                .as_ref()
                .map_or("Unavailable", |flight| match flight.mode {
                    NavigationMode::Stop => "Braking / holding",
                    NavigationMode::Manual => "Manual flight",
                    NavigationMode::Approach { .. } => "Approaching",
                    NavigationMode::Orbit { .. } => "Orbiting",
                    NavigationMode::KeepRange { .. } => "Keeping range",
                    NavigationMode::FlyTo { .. } => "Flying to waypoint",
                }),
        };
        let speed: String = pilot.flight.as_ref().map_or_else(
            || "Unavailable".into(),
            |flight| {
                let velocity = flight.warp.as_ref().map_or(flight.velocity, |warp| {
                    sample_warp(warp, self.presentation_time()).1
                });
                let speed = distance_m(velocity, [0.0; 3]);
                if speed.is_finite() {
                    format_speed(speed)
                } else {
                    "Unavailable".into()
                }
            },
        );
        let job = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .resource_jobs
                .iter()
                .filter(|job| job.pilot == pilot.name)
                .max_by_key(|job| (!job.phase.is_terminal(), job.updated_at_ms, job.id))
        });
        let mining = job.map_or_else(
            || "No routine".into(),
            |job| resource_jobs_ui::resource_phase_label(job.phase).to_owned(),
        );
        let delivery = job.map_or_else(
            || "No target".into(),
            |job| format!("{} / {} ore", job.stored_units, job.target_units),
        );
        // Two compact readout groups leave the live control bank and view
        // unobstructed. The expanded pages retain detailed management.
        ui.columns(2, |columns| {
            egui::Frame::new().fill(Color32::from_rgb(7, 17, 23))
                .inner_margin(egui::Margin::same(6)).show(&mut columns[0], |ui| {
                    ui.label(egui::RichText::new("SPEED").size(22.0).color(MUTED));
                    let width = ui.painter().layout_no_wrap(speed.clone(), egui::FontId::monospace(44.0), self.accent()).size().x;
                    let font = 44.0 * ((ui.available_width()-2.0) / width.max(1.0)).min(1.0);
                    ui.add(egui::Label::new(egui::RichText::new(speed).monospace().size(font).color(self.accent())).wrap_mode(egui::TextWrapMode::Extend));
                    ui.label(egui::RichText::new(mode).size(20.0));
                });
            egui::Frame::new().fill(Color32::from_rgb(7, 17, 23))
                .inner_margin(egui::Margin::same(6)).show(&mut columns[1], |ui| {
                    ui.label(egui::RichText::new("Routine").size(22.0).color(MUTED));
                    let width = ui.painter().layout_no_wrap(mining.clone(), egui::FontId::proportional(28.0), self.accent()).size().x;
                    let font = 28.0 * ((ui.available_width()-2.0) / width.max(1.0)).min(1.0);
                    ui.add(egui::Label::new(egui::RichText::new(mining).size(font).color(self.accent())).wrap_mode(egui::TextWrapMode::Extend));
                    ui.label(egui::RichText::new("Delivered").size(22.0).color(MUTED));
                    let width = ui.painter().layout_no_wrap(delivery.clone(), egui::FontId::monospace(28.0), self.accent()).size().x;
                    let font = 28.0 * ((ui.available_width()-2.0) / width.max(1.0)).min(1.0);
                    ui.add(egui::Label::new(egui::RichText::new(delivery).monospace().size(font)).wrap_mode(egui::TextWrapMode::Extend));
                });
        });
        ui.label(egui::RichText::new(format!(
            "Hold {} / {} ore · {} drones",
            pilot.ore,
            cargo_capacity(pilot.ship_class),
            Self::fleet(&pilot).len()
        )).size(22.0))
        .on_hover_text("Current ship ore and reported hold capacity; drones counts deployed craft, not craft in the bay. Power and regeneration ratings are available in the expanded console.");
    }
    /// Build the physical computer in one fixed logical canvas. Perspective
    /// changes only its 3D vertices and ray map, never egui's allocation sizes.
    fn render_cockpit_computer(&mut self, ctx: &egui::Context, in_hangar: bool) {
        use flight3d::cockpit_computer::ComputerPermission;
        let docked = in_hangar && self.verified_docked_binding().is_some();
        let surface = if docked {
            self.hangar_view.computer_surface()
        } else {
            self.flight_view.computer_surface()
        };
        let hit: Box<dyn Fn(egui::Pos2) -> Option<egui::Pos2>> = if docked {
            Box::new(self.hangar_view.computer_hit_tester(ctx))
        } else {
            Box::new(self.flight_view.computer_hit_tester(ctx))
        };
        let active = self.connected
            && (docked
                || !in_hangar && self.flight_view.cockpit_presentation_active(self.view_mode));
        let input_allowed = !self.ship_observation
            && !self.controls_reference_open
            && !self.performance_open
            && !self.services_open
            && self.service_tab != ServiceTab::Layouts
            && !self.workspace_input_waiting_release
            && self.embedded_input
            && !self.embedded_paused
            && ctx.input(|input| input.focused && !input.viewport().minimized.unwrap_or(false));
        // A real glass hit may transfer focus from an editor/overlay field.
        // Layer/depth rejection happens for each coordinate in the event batch.
        // Context::input holds egui's exclusive context lock. The hit tester
        // queries that same context's layers, so call it only after the input
        // transaction has ended; otherwise the first glass press deadlocks.
        let pressed_positions = if active && input_allowed {
            ctx.input(|input| {
                input
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::PointerButton {
                            pos, pressed: true, ..
                        }
                        | egui::Event::Touch {
                            pos,
                            phase: egui::TouchPhase::Start,
                            ..
                        } => Some(*pos),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
        } else {
            Vec::new()
        };
        let glass_pressed = pressed_positions
            .into_iter()
            .any(|point| hit(point).is_some());
        if glass_pressed {
            ctx.memory_mut(|memory| {
                if let Some(focused) = memory.focused() {
                    memory.surrender_focus(focused);
                }
            });
        }
        let permission = ComputerPermission {
            active,
            pointer: input_allowed,
            keyboard: input_allowed && !ctx.wants_keyboard_input(),
        };
        let mut computer = std::mem::take(&mut self.cockpit_computer);
        #[cfg(test)]
        let mut labels = None;
        let output = computer.run(ctx, surface.as_ref(), permission, hit, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 3.0);
            if !self.cockpit_console_detail {
                for font in ui.style_mut().text_styles.values_mut() {
                    font.size = font.size.max(22.0);
                }
            }
            self.cockpit_actions(ui);
            let display_state = if !self.connected {
                "Disconnected · last received state · controls locked"
            } else if self.snapshot.is_none() {
                "Synchronizing · waiting for world data"
            } else if self.embedded_paused {
                "Paused · display held · controls locked"
            } else if !self.live_command_snapshot_fresh() {
                "Stale world data · values may be out of date"
            } else if self.ship_observation {
                "Observation only · pilot remains on foot · controls locked"
            } else if !input_allowed {
                "Live state · console input inactive"
            } else {
                "Live state"
            };
            ui.label(display_state);
            if self.cockpit_console_detail {
                if ui.small_button("Flight view").clicked() {
                    self.cockpit_console_detail = false;
                }
                self.power_gauge(ui);
                self.cockpit_detail_tabs(ui);
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .min_scrolled_height(0.0)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .id_salt("cockpit_console_content")
                    .max_height((322.0 - ui.cursor().top()).max(0.0))
                    .show(ui, |ui| self.service_content(ui));
            } else {
                self.compact_flight_summary(ui);
                if ui.put(egui::Rect::from_min_max(Pos2::new(380.0, 300.0), Pos2::new(548.0, 324.0)),
                    egui::Button::new(egui::RichText::new("DETAILS").size(16.0)).small())
                    .on_hover_text("Expand console pages").clicked() {
                    self.cockpit_console_detail = true;
                }
            }
            self.cockpit_workspace_keys(ui);
            #[cfg(test)]
            {
                // Observe the actual secondary-context widgets for CPU input
                // journeys. This never adds text or geometry to the parent UI.
                labels = Some(ui.ctx().graphics_mut(|graphics| {
                    graphics
                        .entry(ui.layer_id())
                        .all_entries()
                        .filter_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) => {
                                let center = text.pos + text.galley.rect.center().to_vec2();
                                shape
                                    .clip_rect
                                    .contains(center)
                                    .then(|| (text.galley.text().to_owned(), center))
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                }));
            }
        });
        self.cockpit_computer = computer;
        #[cfg(test)]
        if let Some(labels) = labels {
            self.computer_ui_labels = if active && surface.is_some() {
                labels
            } else {
                Vec::new()
            };
        }
        self.flight_view
            .set_computer_keyboard_owned(!docked && output.wants_keyboard_input);
        self.hangar_view
            .set_computer_keyboard_owned(docked && output.wants_keyboard_input);
        if output.wants_keyboard_input {
            let ime = output.platform_output.ime.and_then(|ime| {
                surface
                    .as_ref()
                    .and_then(|surface| project_computer_ime(surface, ime))
            });
            ctx.output_mut(|parent| parent.ime = ime);
        }
        if !output.diagnostics.is_empty() {
            self.settings_notice = format!("Cockpit display: {}", output.diagnostics.join("; "));
        }
        if active {
            if docked {
                self.hangar_view.submit_computer(ctx, output.primitives);
            } else {
                self.flight_view.submit_computer(ctx, output.primitives);
            }
        }
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
        if let Some(reason) = readiness {
            ui.colored_label(AMBER, reason);
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
                    fresh
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
    fn navigation_controls(&mut self, ui: &mut egui::Ui) {
        let readiness = self.navigation_order_reason();
        let fresh = readiness.is_none();
        if let Some(reason) = readiness {
            ui.colored_label(AMBER, reason);
        }
        ui.add_enabled_ui(fresh, |ui| {
        if matches!(
            self.selected_object,
            SelectedObject::Planet(_) | SelectedObject::Prop(_)
        ) {
            if let Some(mut position) = self.selected_position() {
                let planet = matches!(self.selected_object, SelectedObject::Planet(_));
                if !planet {
                    position[1] += 100.0;
                }
                if ui
                    .add_enabled(
                        self.can_fly(),
                        egui::Button::new(if planet {
                            "Approach planet"
                        } else {
                            "Fly above prop"
                        }),
                    )
                    .clicked()
                {
                    self.send(Request::Navigate {
                        mode: NavigationMode::FlyTo { position },
                    });
                }
            }
        }
        let target = self.selected_target();
        let enabled = self.can_fly() && target.is_some();
        if let Some(own) = self.own() {
            if let Some(position) = self.selected_position() {
                let own_position = motion::position(&own, self.presentation_time());
                ui.colored_label(
                    self.accent(),
                    if let Some(asteroid) = self.selected_asteroid() {
                        format!(
                            "Surface {}",
                            format_distance(
                                (distance_m(own_position, position) - asteroid.radius).max(0.0)
                            )
                        )
                    } else {
                        format!(
                            "Range {}",
                            format_distance(distance_m(own_position, position))
                        )
                    },
                );
            }
        }
        ui.horizontal(|ui| {
            ui.label("Range");
            egui::ComboBox::from_id_salt("nav-range")
                .selected_text(format_distance(self.navigation_range))
                .show_ui(ui, |ui| {
                    for range in [100.0, 500.0, 1000.0] {
                        ui.selectable_value(
                            &mut self.navigation_range,
                            range,
                            format_distance(range),
                        );
                    }
                });
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(enabled, egui::Button::new("Approach"))
                .clicked()
            {
                self.send(Request::Navigate {
                    mode: NavigationMode::Approach {
                        target: target.clone().unwrap(),
                    },
                });
            }
            if ui
                .add_enabled(enabled, egui::Button::new("Orbit"))
                .clicked()
            {
                self.send(Request::Navigate {
                    mode: NavigationMode::Orbit {
                        target: target.clone().unwrap(),
                        radius: self.navigation_range,
                    },
                });
            }
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(enabled, egui::Button::new("Keep range"))
                .clicked()
            {
                self.send(Request::Navigate {
                    mode: NavigationMode::KeepRange {
                        target: target.clone().unwrap(),
                        distance: self.navigation_range,
                    },
                });
            }
            if ui
                .add_enabled(self.can_fly(), egui::Button::new("Stop"))
                .clicked()
            {
                self.send(Request::Navigate {
                    mode: NavigationMode::Stop,
                });
            }
        });
        let destination = match self.selected_object {
            SelectedObject::Station => Some(Destination::Station),
            SelectedObject::Belt | SelectedObject::Asteroid(_) => Some(Destination::Belt),
            SelectedObject::Gate(id) => Some(Destination::Gate { id }),
            _ => None,
        };
        if let Some(destination) = destination {
            if let Some(pilot) = self.own() {
                let cost = power_spec(pilot.ship_class).warp_cost_j;
                ui.colored_label(
                    if self.warp_power_ready() {
                        MUTED
                    } else {
                        AMBER
                    },
                    format!(
                        "Warp {:.1} kJ{}",
                        cost as f64 / 1000.0,
                        if self.warp_power_ready() {
                            ""
                        } else {
                            " · insufficient charge; recharging"
                        }
                    ),
                );
            }
            if ui
                .add_enabled(
                    self.can_warp(),
                    egui::Button::new(
                        if matches!(self.selected_object, SelectedObject::Asteroid(_)) {
                            "Warp to belt"
                        } else {
                            "Warp to selected"
                        },
                    ),
                )
                .clicked()
            {
                self.send(Request::Warp { destination });
            }
        }
        if self.selected_object == SelectedObject::Station {
            let can_dock = self.own().is_some_and(|p| self.can_dock(&p));
            if ui
                .add_enabled(can_dock, egui::Button::new("Dock"))
                .clicked()
            {
                self.send(Request::Dock);
            }
            ui.small(format!(
                "Docking port access: {} from hull envelope",
                format_distance(DOCK_RANGE)
            ));
        }
        if let Some(asteroid) = self.selected_asteroid().cloned() {
            if ui.small_button("Mining modules").clicked() {
                self.service_tab = ServiceTab::Modules;
                self.services_open = true;
            }
            let locked = self
                .own()
                .is_some_and(|p| Self::confirmed_locks(&p).contains(&asteroid.id));
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        self.can_lock_selected(),
                        egui::Button::new(if locked { "Locked" } else { "Lock target" }),
                    )
                    .clicked()
                {
                    self.send(Request::LockAsteroid { id: asteroid.id });
                }
            });
            ui.small(format!(
                "Surface range ≤ {} to extract",
                format_distance(MINING_RANGE)
            ));
        }
        if let Some(gate) = self.selected_gate() {
            if ui
                .add_enabled(
                    self.can_jump_selected(),
                    egui::Button::new(format!(
                        "Jump to {}",
                        system_name(gate.destination_system_id)
                    )),
                )
                .clicked()
            {
                self.send(Request::Jump { gate_id: gate.id });
            }
            ui.small(format!(
                "Gate activation range {}",
                format_distance(GATE_RANGE)
            ));
        }
        if matches!(&self.selected_object, SelectedObject::Pilot(name) if name == self.controlled_name())
        {
            ui.small("Select another vessel or a location to navigate.");
        }
        ui.small(
            if self
                .own()
                .is_some_and(|pilot| pilot.station_avatar.is_some())
            {
                "Read-only navigation · selecting a contact does not move your body or ship"
            } else {
                "Ctrl+Space: stop · RMB: aim · Release controls: assisted braking"
            },
        );
        });
    }
    fn overview(&mut self, ui: &mut egui::Ui) {
        let system = self.current_system();
        ui.label(
            egui::RichText::new(system_name(system))
                .size(19.0)
                .color(self.accent())
                .strong(),
        );
        ui.small(if !self.connected {
            "DISCONNECTED · last reported contacts"
        } else if self.embedded_paused {
            "PAUSED · last reported local contacts"
        } else if !self.live_command_snapshot_fresh() {
            "STALE · waiting for current contact telemetry"
        } else {
            "LOCAL CONTACTS · authoritative snapshot"
        });
        ui.separator();
        ui.label(
            egui::RichText::new("SELECTED OBJECT")
                .size(11.0)
                .color(MUTED),
        );
        ui.small(format!(
            "Selection · {}",
            overview_contacts::identity(&self.selected_object)
        ));
        match self.selected_object.clone() {
            SelectedObject::Station => {
                ui.heading("Arkline Station");
                ui.small(format!(
                    "{} span · {} high",
                    format_distance(STATION_DIAMETER_M),
                    format_distance(STATION_HEIGHT_M)
                ));
                ui.small("Range is measured to the docking port");
            }
            SelectedObject::Planet(id) => {
                if let Some(planet) = self.scene_planets().iter().find(|p| p.id == id) {
                    ui.heading(&planet.name);
                    ui.small("Approach in normal flight, then descend continuously through the atmosphere.");
                }
                self.surface_status(ui);
            }
            SelectedObject::Prop(id) => {
                if let Some(prop) = self
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.surface_props.iter().find(|p| p.id == id))
                {
                    ui.heading(format!("{:?} #{id}", prop.kind));
                    ui.small(format!("Owned by {}", prop.owner));
                }
                if ui.button("Surface sandbox").clicked() {
                    self.service_tab = ServiceTab::Sandbox;
                    self.services_open = true;
                }
            }
            SelectedObject::Belt => {
                ui.heading("Asteroid field");
                ui.small("Select an individual asteroid, lock it, then extract.");
            }
            SelectedObject::Asteroid(id) => {
                if let Some(a) = self.selected_asteroid() {
                    ui.heading(&a.name);
                    ui.colored_label(
                        if a.ore > 0 { self.accent() } else { AMBER },
                        format!("{} ore remaining · #{}", a.ore, id),
                    );
                    if self
                        .own()
                        .is_some_and(|p| Self::confirmed_locks(&p).contains(&id))
                    {
                        ui.colored_label(self.accent(), "TARGET LOCK CONFIRMED");
                    }
                }
            }
            SelectedObject::Gate(_) => {
                if let Some(g) = self.selected_gate() {
                    ui.heading(&g.name);
                    ui.small(format!("Route to {}", system_name(g.destination_system_id)));
                }
            }
            SelectedObject::Pilot(name) => {
                ui.heading(&name);
                if let Some(p) = self
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.pilots.iter().find(|p| p.name == name))
                {
                    ui.small(location_label(&p.location));
                    ui.small(format!(
                        "{} · {} hull",
                        p.ship_class.spec().name,
                        format_distance(p.ship_class.spec().length_m)
                    ));
                    let online = self
                        .snapshot
                        .as_ref()
                        .is_some_and(|s| s.online_pilots.contains(&name));
                    ui.small(if online {
                        "Connected"
                    } else {
                        "Disconnected; vessel persists"
                    });
                }
            }
        }
        self.navigation_controls(ui);
        ui.separator();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.overview_filter, OverviewFilter::All, "All");
            ui.selectable_value(&mut self.overview_filter, OverviewFilter::Mining, "Mining");
            ui.selectable_value(&mut self.overview_filter, OverviewFilter::Travel, "Travel");
            ui.selectable_value(&mut self.overview_filter, OverviewFilter::Pilots, "Pilots");
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.overview_query)
                    .hint_text("Search contacts · name, type or ID")
                    .char_limit(128)
                    .desired_width(240.0),
            );
            if ui
                .add_enabled(!self.overview_query.is_empty(), egui::Button::new("Clear"))
                .clicked()
            {
                self.overview_query.clear();
            }
        });
        let mut rows: Vec<(SelectedObject, String, &str, Option<[f64; 3]>)> = vec![];
        if matches!(
            self.overview_filter,
            OverviewFilter::All | OverviewFilter::Travel
        ) {
            rows.push((
                SelectedObject::Station,
                "Arkline Station".into(),
                "Station",
                Some(STATION_POSITION),
            ));
            rows.push((
                SelectedObject::Belt,
                "Asteroid field".into(),
                "Field",
                Some(BELT_POSITION),
            ));
            for planet in self
                .scene_planets()
                .iter()
                .filter(|p| p.system_id == system)
            {
                rows.push((
                    SelectedObject::Planet(planet.id),
                    planet.name.clone(),
                    "Planet",
                    Some(planet.approach_position),
                ));
            }
            for gate in gates().into_iter().filter(|g| g.system_id == system) {
                rows.push((
                    SelectedObject::Gate(gate.id),
                    gate.name,
                    "Gate",
                    Some(gate.position),
                ));
            }
        }
        if self.overview_filter == OverviewFilter::All {
            if let Some(snapshot) = &self.snapshot {
                for prop in snapshot
                    .surface_props
                    .iter()
                    .filter(|p| p.system_id == system)
                {
                    rows.push((
                        SelectedObject::Prop(prop.id),
                        format!("{:?} #{}", prop.kind, prop.id),
                        "Prop",
                        Some(prop.position),
                    ));
                }
            }
        }
        if matches!(
            self.overview_filter,
            OverviewFilter::All | OverviewFilter::Mining
        ) {
            if let Some(s) = &self.snapshot {
                for a in s.asteroids.iter().filter(|a| a.system_id == system) {
                    rows.push((
                        SelectedObject::Asteroid(a.id),
                        a.name.clone(),
                        if a.ore > 0 { "Ore" } else { "Empty" },
                        Some(a.position),
                    ));
                }
            }
        }
        if matches!(
            self.overview_filter,
            OverviewFilter::All | OverviewFilter::Pilots
        ) {
            if let Some(s) = &self.snapshot {
                for p in s.pilots.iter().filter(|p| p.system_id == system) {
                    rows.push((
                        SelectedObject::Pilot(p.name.clone()),
                        p.name.clone(),
                        if p.location == Location::Docked {
                            "Docked"
                        } else {
                            "Ship"
                        },
                        p.flight
                            .as_ref()
                            .map(|_| motion::position(p, self.presentation_time())),
                    ));
                }
            }
        }
        let total = rows.len();
        let selected_in_category = rows.iter().any(|row| row.0 == self.selected_object);
        rows.retain(|(object, name, kind, _)| {
            overview_contacts::matches(&self.overview_query, object, name, kind)
        });
        ui.horizontal_wrapped(|ui| {
            ui.small(format!("{} / {} contacts", rows.len(), total));
            if !rows.iter().any(|row| row.0 == self.selected_object) {
                ui.small(if selected_in_category {
                    "Selection hidden by search · unchanged"
                } else {
                    "Selection outside this category or current local snapshot · unchanged"
                });
            }
        });
        if rows.is_empty() {
            ui.colored_label(
                MUTED,
                if self.overview_query.trim().is_empty() {
                    "No contacts reported in this category."
                } else {
                    "No matching contacts · clear search or choose another category."
                },
            );
        }
        let own_position = self
            .own()
            .map(|p| motion::position(&p, self.presentation_time()));
        rows.sort_by(|a, b| {
            let da = own_position
                .zip(a.3)
                .map_or(f64::INFINITY, |(own, p)| distance_m(own, p));
            let db = own_position
                .zip(b.3)
                .map_or(f64::INFINITY, |(own, p)| distance_m(own, p));
            let ordering = da.total_cmp(&db).then_with(|| a.1.cmp(&b.1));
            if self.overview_nearest_first {
                ordering
            } else {
                ordering.reverse()
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("overview-table")
            .show(ui, |ui| {
                egui::Grid::new("overview-grid")
                    .striped(true)
                    .spacing([8.0, 3.0])
                    .show(ui, |ui| {
                        ui.small("NAME");
                        ui.small("TYPE");
                        if ui
                            .small_button(if self.overview_nearest_first {
                                "RANGE ↑"
                            } else {
                                "RANGE ↓"
                            })
                            .on_hover_text("Sort by distance")
                            .clicked()
                        {
                            self.overview_nearest_first = !self.overview_nearest_first;
                        }
                        ui.end_row();
                        for (object, name, kind, position) in rows {
                            if overview_contacts::button(
                                ui,
                                &object,
                                self.selected_object == object,
                                &name,
                            )
                            .clicked()
                            {
                                self.selected_object = object;
                            }
                            ui.small(kind);
                            ui.small(
                                own_position
                                    .zip(position)
                                    .map(|(a, b)| format_distance(distance_m(a, b)))
                                    .unwrap_or_else(|| "—".into()),
                            );
                            ui.end_row();
                        }
                    });
            });
    }
    fn ledger_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new("ECONOMIC LEDGER")
                    .color(self.accent())
                    .strong(),
            );
            if ui
                .add_enabled(
                    self.connected && self.audit_updates.is_none(),
                    egui::Button::new("Audit current ledger"),
                )
                .clicked()
            {
                let server = self.server.clone();
                let scope = self.credential_scope.as_deref().unwrap_or(&server);
                let path = self.data_dir.join(format!(
                    "audit-{}.json",
                    hex::encode(Sha256::digest(scope.as_bytes()))
                ));
                let (tx, rx) = mpsc::channel();
                self.audit_updates = Some(rx);
                #[cfg(not(target_arch = "wasm32"))]
                std::thread::spawn(move || {
                    let _ = tx.send(audit::audit(&server, &path));
                });
                #[cfg(target_arch = "wasm32")]
                { let _ = (server, path); let _ = tx.send(Err("Independent ledger export auditing currently requires the native client or arkline-verifier CLI.".into())); }
            }
            if self.audit_updates.is_some() {
                ui.spinner();
                ui.small("Checking independent export…");
            }
        });
        if let Some(snapshot) = &self.snapshot {
            ui.small(format!(
                "Latest server checkpoint: #{}",
                snapshot.ledger_sequence
            ));
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&snapshot.ledger_hash)
                        .monospace()
                        .color(MUTED),
                )
                .wrap(),
            );
        }
        ui.separator();
        ui.label(
            egui::RichText::new("LAST COMPLETED AUDIT")
                .size(11.0)
                .color(MUTED),
        );
        match &self.audit_result {
            Some(Ok(result)) => {
                ui.colored_label(self.accent(), result);
            }
            Some(Err(error)) => {
                ui.colored_label(AMBER, format!("Audit failed: {error}"));
            }
            None => {
                ui.colored_label(MUTED, "No independent audit in this cockpit session. The first successful audit pins a local checkpoint; later audits must preserve its history.");
            }
        }
        ui.small("Server history may advance after this audit. A successful result applies to its reported head, not automatically to the latest checkpoint.");
        ui.small("An audit checks the exported economic rules and history. It does not prove every gameplay event occurred or certify server honesty.");
    }
    fn market(&mut self, ui: &mut egui::Ui) {
        use exchange_readiness::ExchangeAction;
        ui.label(
            egui::RichText::new("STATION EXCHANGE")
                .color(self.accent())
                .strong(),
        );
        let cargo_limit = self.own_cargo_capacity();
        let readiness = self.exchange_readiness_reason();
        ui.small("Player sell orders · ore · credits per unit");
        if let Some(reason) = readiness {
            ui.colored_label(AMBER, reason);
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Sell");
            ui.add(egui::DragValue::new(&mut self.sell_amount).range(1..=cargo_limit));
            ui.label("at");
            ui.add(egui::DragValue::new(&mut self.price).range(1..=1_000_000));
            let reason = self.exchange_action_reason(ExchangeAction::List {
                units: self.sell_amount,
                unit_price: self.price,
            });
            if ui
                .add_enabled(reason.is_none(), egui::Button::new("List ore"))
                .on_disabled_hover_text(reason.unwrap_or(""))
                .clicked()
            {
                self.send(Request::PlaceSell {
                    units: self.sell_amount,
                    unit_price: self.price,
                });
            }
            if readiness.is_none() {
                if let Some(reason) = reason {
                    ui.small(reason);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Buy quantity");
            ui.add(egui::DragValue::new(&mut self.buy_amount).range(1..=cargo_limit));
        });
        let orders = self
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.orders.clone())
            .unwrap_or_default();
        egui::ScrollArea::vertical()
            .max_height(190.0)
            .show(ui, |ui| {
                if orders.is_empty() {
                    ui.colored_label(
                        MUTED,
                        if self.snapshot.is_none() {
                            "Waiting for exchange listings."
                        } else if readiness.is_some() {
                            "No listings in the last received snapshot."
                        } else {
                            "No listings yet. Mine ore and create the first order."
                        },
                    );
                }
                for order in orders {
                    ui.push_id(order.id, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!(
                                "#{}   {}   {} ore × {} CR",
                                order.id, order.seller, order.units, order.unit_price
                            ));
                            let (label, action) = if order.seller == self.pilot {
                                ("Cancel", ExchangeAction::Cancel { order_id: order.id })
                            } else {
                                (
                                    "Buy",
                                    ExchangeAction::Buy {
                                        order_id: order.id,
                                        units: self.buy_amount,
                                    },
                                )
                            };
                            let reason = self.exchange_action_reason(action);
                            if ui
                                .add_enabled(reason.is_none(), egui::Button::new(label))
                                .on_disabled_hover_text(reason.unwrap_or(""))
                                .clicked()
                            {
                                if order.seller == self.pilot {
                                    self.send(Request::CancelSell { order_id: order.id });
                                } else {
                                    self.send(Request::Buy {
                                        order_id: order.id,
                                        units: self.buy_amount,
                                    });
                                }
                            }
                            if readiness.is_none() {
                                if let Some(reason) = reason {
                                    ui.small(reason);
                                }
                            }
                        });
                    });
                    ui.separator();
                }
            });
    }
    fn overlay_frame(&self, ctx: &egui::Context) -> egui::Frame {
        egui::Frame::window(&ctx.style())
            .fill(Color32::from_rgba_unmultiplied(3, 9, 6, 212))
            .stroke(Stroke::new(1.0, self.accent().gamma_multiply(0.32)))
            .inner_margin(8.0)
    }
    fn session_menu(&mut self, ui: &mut egui::Ui) {
        if self.embedded {
            ui.small("Embedded local world");
            return;
        }
        ui.menu_button("Session", |ui| {
            ui.small(&self.server);
            if self.connected {
                if ui.button("Open second cockpit").clicked() {
                    self.second_cockpit();
                    ui.close_menu();
                }
                if ui
                    .add_enabled(!self.request_pending, egui::Button::new("Disconnect"))
                    .clicked()
                {
                    self.commands = None;
                    self.updates = None;
                    self.connected = false;
                    self.snapshot = None;
                    self.effects.clear();
                    ui.close_menu();
                }
            } else {
                if ui
                    .add_enabled(!self.connecting, egui::Button::new("Reconnect"))
                    .clicked()
                {
                    self.connect();
                    ui.close_menu();
                }
                if ui.button("Change pilot / server").clicked() {
                    self.snapshot = None;
                    self.commands = None;
                    self.updates = None;
                    self.effects.clear();
                    ui.close_menu();
                }
            }
        });
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
            && self.can_fly()
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
                        .add_enabled(self.can_fly(), egui::Button::new("Fly here"))
                        .clicked()
                    {
                        self.send(Request::Navigate {
                            mode: NavigationMode::FlyTo { position },
                        });
                        ui.close_menu();
                    }
                } else if let Some(target) = self.selected_target() {
                    if ui
                        .add_enabled(self.can_fly(), egui::Button::new("Approach selected"))
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
        if !self.performance_open { self.operations_keyboard_navigation(ctx); }
        if self.performance_open || self.controls_reference_open
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
                    self.commands = None;
                    self.updates = None;
                    self.connected = false;
                    self.connecting = false;
                    self.snapshot = None;
                    return;
                }
                boot::Action::Wait => return,
            }
        }
        self.effects.retain(self.presentation_time());
        if !self.performance_open && !self.controls_reference_open
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
        let in_hangar = self.in_station_hangar();
        // Reserve persistent bottom chrome before placing any task/identity window.
        if in_hangar
            || self.space_walk_available()
            || self
                .own()
                .is_some_and(|pilot| pilot.station_avatar.is_some())
        {
            self.station_overlay(ctx);
        }
        egui::TopBottomPanel::bottom("flight_authority_status")
            .frame(
                egui::Frame::new()
                    .fill(self.theme.panel())
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| self.flight_workspace_status(ui));
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
        let panel_response =
            egui::Area::new(egui::Id::new("cockpit_identity_overlay").with(self.layout.generation))
                .order(egui::Order::Foreground)
                .current_pos(panel_pos)
                .movable(!self.layout.pinned)
                .constrain_to(bounds)
                .show(ctx, |ui| {
                    self.overlay_frame(ctx).show(ui, |ui| {
                        ui.set_max_width((bounds.width() - 24.0).clamp(1.0, 490.0));
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
                        ui.horizontal(|ui| {
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
                        });
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
            ctx.input(|i| i.pointer.any_down()),
        );
        self.controls_reference(ctx);
        // Service destinations and detailed flight controls now share the
        // deliberate task workspace, instead of competing launcher/overview panels.
        if self.services_open && self.service_tab == ServiceTab::Layouts {
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
        if self.services_open && self.service_tab != ServiceTab::Layouts {
            let mut open = true;
            let (panel_pos, panel_size) = self.layout.position(
                "task_workspace",
                bounds,
                [1.0, 0.5],
                Vec2::new(-16.0, 0.0),
                Vec2::new(
                    (bounds.width() * 0.52).clamp(520.0, 820.0),
                    (bounds.height() - 72.0).clamp(280.0, 680.0),
                ),
            );
            let mut content_size = None;
            let panel_response = egui::Window::new(self.operations_title())
                .id(egui::Id::new("cockpit_services_overlay").with(self.layout.generation))
                .open(&mut open)
                .current_pos(panel_pos)
                .default_size(panel_size)
                .max_width(bounds.width() - 20.0)
                .resize(|resize| {
                    if self.layout.reflow {
                        resize.min_size(panel_size).max_size(panel_size)
                    } else {
                        resize
                    }
                })
                .min_width((bounds.width() - 32.0).clamp(240.0, 600.0))
                .max_height(bounds.height() - 30.0)
                .resizable(!self.layout.pinned)
                .movable(!self.layout.pinned)
                .collapsible(false)
                .constrain_to(bounds)
                .frame(self.overlay_frame(ctx).fill(self.theme.panel()))
                .vscroll(false)
                .show(ctx, |ui| {
                    content_size = Some(ui.max_rect().size());
                    self.service_tabs(ui);
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("focused_task_content")
                        .auto_shrink([false, false])
                        .min_scrolled_height(0.0)
                        .max_height(ui.available_height().max(0.0))
                        .show(ui, |ui| self.service_content(ui));
                });
            if let Some(response) = panel_response {
                self.layout.record(
                    "task_workspace",
                    bounds,
                    response.response.rect,
                    content_size,
                    ctx.input(|i| i.pointer.any_down()),
                );
            }
            self.services_open &= open;
        }
        self.hangar_view.set_space_snapshot(
            if self
                .own()
                .is_some_and(|pilot| pilot.location != Location::Docked)
            {
                self.snapshot.clone().map(std::sync::Arc::new)
            } else {
                None
            },
        );
        self.hangar_view
            .set_docked_traversal_enabled(self.docked_navigation_present());
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
                            target_controls::feed_ids(
                                &Self::fleet(&p).iter().map(|d| d.id).collect::<Vec<_>>(),
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
                                    if self.can_fly() {
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
        performance_overlay::show(ctx, &mut self.performance_open, performance_overlay::PerformanceSamples {
            frame_intervals_s: &self.frame_intervals,
            snapshot_age_ms: self.snapshot.as_ref().and(self.performance_snapshot_received).map(|received| received.elapsed().as_millis().min(u64::MAX as u128) as u64),
            connected: self.connected,
            paused: self.embedded && self.embedded_paused,
            session: if self.embedded { "Embedded local session" } else { "Remote server session" },
            server_tick: self.performance_server_tick,
        });
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
        if self.performance_open || self.controls_reference_open
            || self.services_open
            || self.service_tab == ServiceTab::Layouts
        {
            self.release_workspace_input();
        }
        let workspace_allows_input = !self.performance_open && !self.controls_reference_open
            && !self.services_open
            && self.service_tab != ServiceTab::Layouts
            && !self.workspace_input_waiting_release;
        let flight_allowed = workspace_allows_input
            && !ctx.input(|i| i.pointer.primary_down())
            && self.view_mode != ViewMode::Tactical
            && self.can_operate()
            && (self.manual_remote.is_none() || self.controlled_remote().is_some())
            && self.own().is_some_and(|p| {
                p.station_avatar.is_none()
                    && p.flight.is_some()
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

/// Only the process that spawned a local host owns its lifetime.
#[cfg(not(target_arch = "wasm32"))]
struct LocalServer(Child);
#[cfg(target_arch = "wasm32")]
struct LocalServer;
#[cfg(not(target_arch = "wasm32"))]
impl Drop for LocalServer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn start_local_server(dir: &std::path::Path) -> Result<(LocalServer, String), String> {
    fs::create_dir_all(dir).map_err(|e| format!("Cannot create preview storage: {e}"))?;
    let executable = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("Cannot locate client directory")?
        .join(if cfg!(windows) {
            "arkline-server.exe"
        } else {
            "arkline-server"
        });
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("server.log"))
        .map_err(|e| e.to_string())?;
    let child = Command::new(executable)
        .arg("--bind")
        .arg("127.0.0.1:0")
        .arg("--db")
        .arg(dir.join("world.sqlite"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|e| format!("Cannot start bundled universe server: {e}"))?;
    let mut guard = LocalServer(child);
    let stdout = guard
        .0
        .stdout
        .take()
        .ok_or("Cannot read local server startup")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(address) = line.strip_prefix("Arkline server listening on ") {
                let _ = tx.send(address.to_string());
            }
        }
    });
    let address = rx.recv_timeout(Duration::from_secs(10)).map_err(|_| {
        format!(
            "Local universe could not start. See {}",
            dir.join("server.log").display()
        )
    })?;
    let socket: std::net::SocketAddr = address
        .parse()
        .map_err(|_| "Invalid local server address")?;
    if !socket.ip().is_loopback() || socket.port() == 0 {
        return Err("Local server did not bind safely".into());
    }
    Ok((guard, address))
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
        assert!(cockpit.resource_start_reason().unwrap().contains("Enter"));
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
        assert!(labels.iter().any(|text| text == "Mining routine"));
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
        let ctx=egui::Context::default();
        let mut app=view_test_cockpit(&ctx);
        let mut ids=Vec::new();
        for detail in [false,true] {
            app.cockpit_console_detail=detail;
            let _=ctx.run(egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(Pos2::ZERO,Vec2::new(560.,380.))),..Default::default()}, |ctx| {
                egui::CentralPanel::default().show(ctx,|ui| {
                    app.cockpit_actions(ui);
                    ids.push((ui.next_auto_id(),ui.button("Following control sentinel").id));
                });
            });
        }
        assert_eq!(ids[0],ids[1],"Omitting compact help must preserve subsequent control IDs");
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
            ctx.run(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(560.0, 380.0))),
                events, ..Default::default()
            }, |ctx| { egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
                if !enabled { ui.disable(); }
                c.cockpit_workspace_keys(ui);
            }); })
        };
        for (label, tab, x) in [("FLIGHT", ServiceTab::Navigation, 96.0),
            ("DRONES", ServiceTab::Drones, 280.0), ("MINING", ServiceTab::ShipComputer, 464.0)] {
            cockpit.services_open = false;
            let out = render(&mut cockpit, vec![], true);
            let center = out.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label =>
                    Some(text.pos + text.galley.rect.center().to_vec2()), _ => None,
            }).unwrap();
            assert!((center.x - x).abs() < 1.0 && (center.y - 352.0).abs() < 1.0);
            for enabled in [false, true] {
                cockpit.services_open = false;
                render(&mut cockpit, vec![], enabled);
                for pressed in [true, false] {
                    render(&mut cockpit, vec![egui::Event::PointerMoved(center),
                        egui::Event::PointerButton { pos: center, button: egui::PointerButton::Primary,
                            pressed, modifiers: egui::Modifiers::NONE }], enabled);
                }
                assert_eq!(cockpit.services_open, enabled, "{label} eligibility");
                if enabled { assert!(cockpit.service_tab == tab, "{label} opened a different workspace"); }
                assert!(rx.try_recv().is_err(), "Workspace key issued a ship command");
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
                assert!(text.iter().any(|(_, t)| t.galley.job.text == "Manual ship orders can cancel the active mining routine"));
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
                        assert!(bounds.bottom() <= 324.0, "Details label overlaps physical key row");
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

    pub(crate) fn view_test_cockpit(ctx: &egui::Context) -> Cockpit {
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut cockpit = Cockpit::new(
            &cc,
            "fixture".into(),
            std::env::temp_dir().join("arkline-headless-view-unused"),
            None,
        );
        cockpit.boot.visible = false;
        cockpit.connected = true;
        cockpit.pilot = "pilot".into();
        cockpit.snapshot = Some(Snapshot {
            walk_sprint_v1: false,
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
        cockpit
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
                        cockpit.mining_module(ui);
                        cockpit.drone_controls(ui);
                    }
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
                .any(|(text, _)| text.contains("Capacitor 2.5/10.0 kJ · reactor 300 W")));
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
    fn overview_pointer_drag_persists_and_reflows_in_real_app_update() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.view_mode = ViewMode::Tactical;
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
                m.area_rect(egui::Id::new("cockpit_overview_overlay").with(c.layout.generation))
                    .unwrap()
            })
        }
        let size = Vec2::new(1200.0, 850.0);
        for _ in 0..4 {
            frame(&ctx, &mut cockpit, size, vec![]);
        }
        let before = frame(&ctx, &mut cockpit, size, vec![]);
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
        let end = start + Vec2::new(-180.0, 55.0);
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
            (moved.min - before.min - Vec2::new(-180.0, 55.0)).length() < 3.0,
            "before={before:?}, moved={moved:?}"
        );
        cockpit.layout.wait_for_save();
        assert!(dir.join("layout.json").exists());
        cockpit.layout = layout::Layout::load(&dir);
        for _ in 0..3 {
            frame(&ctx, &mut cockpit, size, vec![]);
        }
        let loaded = frame(&ctx, &mut cockpit, size, vec![]);
        assert!(loaded.min.distance(moved.min) < 3.0);
        let larger = Vec2::new(1400.0, 950.0);
        for _ in 0..3 {
            frame(&ctx, &mut cockpit, larger, vec![]);
        }
        let resized = frame(&ctx, &mut cockpit, larger, vec![]);
        assert!((resized.left() - loaded.left() - 200.0).abs() < 3.0);
        assert!((resized.top() - loaded.top()).abs() < 3.0);
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
        let end = start + Vec2::new(larger.x - 20.0 - resized.right(), 0.0);
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
        assert!((larger.x - snapped.right() - 16.0).abs() < 3.0);
        cockpit.layout.reset();
        for _ in 0..3 {
            frame(&ctx, &mut cockpit, larger, vec![]);
        }
        let reset = frame(&ctx, &mut cockpit, larger, vec![]);
        assert!((larger.x - reset.right() - 16.0).abs() < 3.0);
        assert!((reset.top() - 16.0).abs() < 3.0);
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
                    Vec2::new(950.0, 1100.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| match section {
                    0 => cockpit.drone_controls(ui),
                    1 => cockpit.mining_module(ui),
                    _ => cockpit.resource_job_panel(ui),
                });
            },
        )
        .shapes
        .into_iter()
        .filter_map(|shape| match shape.shape {
            egui::Shape::Text(text) => Some((
                text.galley.text().to_owned(),
                text.pos + text.galley.rect.center().to_vec2(),
            )),
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
        let labels = operations_safety_frame(&ctx, &mut cockpit, 0, vec![]);
        operations_safety_click(
            &ctx,
            &mut cockpit,
            0,
            labels
                .iter()
                .find(|(text, _)| text == "Recall drone")
                .unwrap()
                .1,
        );
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
        assert!(labels
            .iter()
            .any(|(text, _)| text.contains("Ship hold full")));
        operations_safety_click(
            &ctx,
            &mut cockpit,
            0,
            labels
                .iter()
                .find(|(text, _)| text == "Mine locked target")
                .unwrap()
                .1,
        );
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
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        ui_test_click(
            &ctx,
            &mut cockpit,
            labels
                .iter()
                .find(|(t, _)| t == "Launch Recon 0/5")
                .unwrap()
                .1,
            false,
        );
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::LaunchDroneClass {
                class: DroneClass::Recon
            }
        ));
        cockpit.request_pending = false;
        cockpit.snapshot.as_mut().unwrap().pilots[0].drone = Some(DroneView {
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
            let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
            ui_test_click(
                &ctx,
                &mut cockpit,
                labels.iter().find(|(t, _)| t == label).unwrap().1,
                false,
            );
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
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        ui_test_click(
            &ctx,
            &mut cockpit,
            labels
                .iter()
                .find(|(t, _)| t == "Drone to selected")
                .unwrap()
                .1,
            false,
        );
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
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        let second = labels
            .iter()
            .find(|(t, _)| t.starts_with("#02 Recon"))
            .unwrap()
            .1;
        ui_test_click(&ctx, &mut cockpit, second, false);
        assert_eq!(cockpit.selected_drone, Some(2));
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        ui_test_click(
            &ctx,
            &mut cockpit,
            labels.iter().find(|(t, _)| t == "Hold drone").unwrap().1,
            false,
        );
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::DroneHoldById { id: 2 }
        ));
        cockpit.request_pending = false;
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        ui_test_click(
            &ctx,
            &mut cockpit,
            labels
                .iter()
                .find(|(t, _)| t == "Launch Recon 5/5")
                .unwrap()
                .1,
            false,
        );
        assert!(rx.try_recv().is_err());
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        ui_test_click(
            &ctx,
            &mut cockpit,
            labels
                .iter()
                .find(|(t, _)| t == "Launch Mining 0/5")
                .unwrap()
                .1,
            false,
        );
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::LaunchDroneClass {
                class: DroneClass::Mining
            }
        ));
        cockpit.request_pending = false;
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        ui_test_click(
            &ctx,
            &mut cockpit,
            labels.iter().find(|(t, _)| t == "Orbit camera").unwrap().1,
            false,
        );
        assert_eq!(cockpit.drone_camera_mode, flight3d::DroneCameraMode::Orbit);
        assert!(rx.try_recv().is_err());
        for label in ["Split feeds", "Next feeds", "Raised"] {
            let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
            ui_test_click(
                &ctx,
                &mut cockpit,
                labels.iter().find(|(text, _)| text == label).unwrap().1,
                false,
            );
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
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        ui_test_click(
            &ctx,
            &mut cockpit,
            labels
                .iter()
                .find(|(t, _)| t == "Mine locked target")
                .unwrap()
                .1,
            false,
        );
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
        let labels = ui_test_frame(&ctx, &mut cockpit, vec![], false);
        ui_test_click(
            &ctx,
            &mut cockpit,
            labels
                .iter()
                .find(|(t, _)| t == "Attack selected drone")
                .unwrap()
                .1,
            false,
        );
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
            pioneer_room: None,
            pioneer_room_layout: None,
            hangar: None,
            pioneer_interior_available: true,
            pioneer_interior_decks: 2,
            ships: vec![
                HangarShip {
                    power: None,
                    ship_class: ShipClass::Hauler,
                    system_id: 1,
                    ore: 99,
                },
                HangarShip {
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
            walk_sprint_v1: false,
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
            pioneer_room: None,
            pioneer_room_layout: None,
            hangar: None,
            pioneer_interior_available: true,
            pioneer_interior_decks: 2,
            ships: ALL_SHIP_CLASSES
                .into_iter()
                .filter(|c| *c != ShipClass::Pioneer)
                .map(|ship_class| HangarShip {
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
