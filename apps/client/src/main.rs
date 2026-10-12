//! Native Arkline cockpit. Live world data is server-authoritative; ship layouts are labeled design references.
#[cfg(not(target_arch = "wasm32"))]
mod audit;
mod boot;
#[cfg(target_arch = "wasm32")]
mod browser;
mod effects;
mod flight3d;
mod layout;
mod lighting_settings;
mod motion;
mod pilot_input;
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
fn worker(
    server: String,
    pilot: String,
    dir: PathBuf,
    credential_scope: Option<String>,
    commands: Receiver<Request>,
    updates: Sender<Update>,
    controls: pilot_input::Mailbox,
) {
    let run = || -> Result<(), String> {
        let path = credentials_path(&dir, credential_scope.as_deref().unwrap_or(&server), &pilot);
        let mut token = match fs::read_to_string(&path) {
            Ok(v) => Some(v),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("Cannot read saved credential: {e}")),
        };
        // Check storage before creating a server-side identity.
        fs::create_dir_all(&dir).map_err(|e| format!("Credential directory unavailable: {e}"))?;
        let mut reader = transport::Connection::connect(&server)?;
        let mut response = reader.exchange(&Request::Connect {
            supports_pioneer_interior: true,
            supports_fleet_hangar: true,
            supports_room_controls: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: pilot.clone(),
            resume_token: token.clone(),
        })?;
        if !response.ok {
            return Err(response.message);
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
                    if token.is_none() {
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
                            let mut resumed = next.exchange(&Request::Connect {
                                supports_pioneer_interior: true,
                                supports_fleet_hangar: true,
                                supports_room_controls: true,
                                protocol_version: PROTOCOL_VERSION,
                                pilot: pilot.clone(),
                                resume_token: token.clone(),
                            })?;
                            if !resumed.ok {
                                return Err(format!("Resume rejected: {}", resumed.message));
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
    logo: egui::TextureHandle,
    boot: boot::Boot,
    server: String,
    pilot: String,
    data_dir: PathBuf,
    credential_scope: Option<String>,
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
    hangar_view: flight3d::hangar::HangarView,
    station_walk_active: bool,
    frozen_time: Option<u64>,
    focus_on_first_frame: bool,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    local_server: Option<LocalServer>,
    service_tab: ServiceTab,
    ship_layout: ship_layouts::LayoutViewer,
    selected_object: SelectedObject,
    mining_target: Option<u64>,
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
    theme: theme::Theme,
    settings_notice: String,
    navigation_range: f64,
    context_fly_to: Option<[f64; 3]>,
    overview_filter: OverviewFilter,
    services_open: bool,
    overview_nearest_first: bool,
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
            logo,
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
            hangar_view: flight3d::hangar::HangarView::new(cc.gl.as_ref()),
            station_walk_active: false,
            frozen_time: None,
            focus_on_first_frame: true,
            local_server: None,
            service_tab: ServiceTab::Modules,
            ship_layout: ship_layouts::LayoutViewer::default(),
            selected_object: SelectedObject::Station,
            mining_target: None,
            drone_camera: true,
            selected_drone: None,
            drone_camera_mode: flight3d::DroneCameraMode::Raised,
            drone_split: false,
            drone_attack_target: None,
            prop_kind: SurfacePropKind::Crate,
            prop_offset: [20.0, 0.0, 0.0],
            resource_goal: 100,
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
            theme,
            settings_notice: String::new(),
            navigation_range: 500.0,
            context_fly_to: None,
            overview_filter: OverviewFilter::All,
            services_open: false,
            overview_nearest_first: true,
            overview_open: true,
        }
    }
    fn accent(&self) -> Color32 {
        self.theme.accent()
    }
    fn settings_panel(&mut self, ui: &mut egui::Ui) {
        let action = self.sound_settings.ui(ui, &self.sound.status());
        if action.changed {
            if let Err(error) = self.sound_settings.save(&self.data_dir) {
                self.settings_notice = error;
            }
        }
        if action.retry {
            self.sound.retry();
        }
        if action.test {
            self.sound.cue(sound_engine::Cue::Ui);
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
    fn update_sound(&mut self) {
        let now = self.timeline_origin.map_or(0, |t| {
            t.saturating_add(self.observed.elapsed().as_millis() as u64)
        });
        let name = self.controlled_name().to_owned();
        let (mut mix, cues) =
            self.sound_scene
                .update(self.snapshot.as_ref(), &name, self.connected, now);
        mix.master = self.sound_settings.master;
        mix.ambience = self.sound_settings.ambience;
        mix.effects = self.sound_settings.effects;
        mix.muted = self.sound_settings.muted || self.voice.is_listening();
        mix.ducked = self.voice.is_speaking();
        self.sound.set_mix(mix);
        for cue in cues {
            self.sound.cue(cue);
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
        #[cfg(target_arch = "wasm32")]
        self.sound.start();
        self.sound_scene.reset();
        self.invalidate_ai_session();
        self.resource_pilot = None;
        self.remote_ship = None;
        self.manual_remote = None;
        self.boot.connect();
        self.connected = false;
        self.controls = pilot_input::Mailbox::default();
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
        std::thread::spawn(move || worker(server, pilot, dir, scope, rx, out_tx, controls));
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
        self.connected && !self.request_pending
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
                | Request::EnterInterior
                | Request::ExitInterior
                | Request::InteriorLift { .. }
                | Request::StationSit { .. }
                | Request::BoardShip { .. }
                | Request::EnterShip
                | Request::Jump { .. }
                | Request::StartResourceJob { .. }
        ) {
            self.pilot_input.relinquish(&self.controls);
            self.station_walk_active = false;
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
                        if !previously_docked
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
                        if self.timeline_origin.is_none() {
                            self.timeline_origin = Some(snapshot.server_time_ms);
                            self.observed = Instant::now();
                        }
                        self.motion.observe(&snapshot);
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
                    "UNDOCKING CONFIRMED".into(),
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
        if self.own().is_some_and(|p| p.station_avatar.is_some()) {
            self.on_foot_view_selector(ui);
            return;
        }
        ui.selectable_value(&mut self.view_mode, ViewMode::Tactical, "2D Tactical");
        ui.selectable_value(&mut self.view_mode, ViewMode::Tactical3d, "3D Tactical");
        ui.selectable_value(&mut self.view_mode, ViewMode::External, "3D External");
        ui.selectable_value(&mut self.view_mode, ViewMode::Cockpit, "Cockpit");
        if self.view_mode == ViewMode::Tactical {
            let previous = self.tactical_system;
            ui.selectable_value(&mut self.tactical_system, false, "Local");
            ui.selectable_value(&mut self.tactical_system, true, "System");
            if previous != self.tactical_system || ui.button("Reset map").clicked() {
                self.tactical_pan = [0.0; 2];
                self.tactical_zoom = 1.0;
            }
            if ui.button("Focus my ship").clicked() {
                self.tactical_system = false;
                self.tactical_pan = [0.0; 2];
                self.tactical_zoom = 1.0;
            }
            ui.small("Wheel: zoom at cursor · drag empty space: pan");
        }
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
        self.target_chips(ui);
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
                                    self.can_operate(),
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
                        self.can_operate(),
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
        ui.horizontal_wrapped(|ui| {
            let docked = pilot.location == Location::Docked;
            let (label, enabled, command) = if docked {
                (
                    "UNDOCK",
                    self.can_operate() && pilot.station_avatar.is_none(),
                    Request::Undock,
                )
            } else {
                ("DOCK", self.can_dock(&pilot), Request::Dock)
            };
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new(egui::RichText::new(label).strong().size(16.0))
                        .min_size(Vec2::new(108.0, 32.0))
                        .fill(self.accent().gamma_multiply(0.26)),
                )
                .clicked()
            {
                self.send(command);
            }
            if ui
                .add_enabled(
                    self.can_fly(),
                    egui::Button::new("STOP").min_size(Vec2::new(76.0, 32.0)),
                )
                .clicked()
            {
                self.send(Request::Navigate {
                    mode: NavigationMode::Stop,
                });
            }
            let (status, help) = if !self.connected {
                ("OFFLINE", "Reconnect to operate")
            } else if self.request_pending {
                ("PENDING", "Awaiting server confirmation")
            } else if docked {
                ("DOCKED", "Undock to begin flight")
            } else if !self.can_dock(&pilot) {
                ("IN FLIGHT", "Approach the station docking port to dock")
            } else {
                ("IN RANGE", "Station is within docking range")
            };
            ui.small(status).on_hover_text(help);
        });
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
                        "Capacitor {:.1}/{:.1} kJ · +{} W{}",
                        power.charge_j as f64 / 1000.0,
                        power.capacity_j as f64 / 1000.0,
                        power.regen_w,
                        if self.connected { "" } else { " · last known" }
                    )),
            )
            .on_hover_text({let spec=power_spec(pilot.ship_class);format!("Server-confirmed energy. Warp {} J; turret cycle {} J; mining drone cycle {} J; drone weapon shot {} J.",spec.warp_cost_j,spec.mining_cycle_cost_j,spec.drone_mining_cycle_cost_j,spec.drone_attack_cost_j)});
        } else {
            ui.small("Capacitor data unavailable");
        }
    }
    fn service_visible(&self, tab: ServiceTab) -> bool {
        self.own().is_none_or(|p| p.station_avatar.is_none())
            || !matches!(
                tab,
                ServiceTab::Navigation
                    | ServiceTab::Hangar
                    | ServiceTab::Modules
                    | ServiceTab::Drones
                    | ServiceTab::ShipComputer
                    | ServiceTab::Sandbox
            )
    }
    fn service_tabs(&mut self, ui: &mut egui::Ui) {
        // Every destination stays visible; narrow physical screens wrap the grid.
        let columns = ((ui.available_width() / 72.0).floor() as usize).clamp(3, 6);
        let width = (ui.available_width() - 4.0 * (columns - 1) as f32) / columns as f32;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 3.0);
            ui.horizontal_wrapped(|ui| {
                for (tab, label, description) in [
                    (ServiceTab::Navigation, "Flight", "Targets and navigation"),
                    (ServiceTab::Hangar, "Ship", "Owned ship hangar"),
                    (
                        ServiceTab::Layouts,
                        "Layouts",
                        "Decks, rooms and proposed fittings",
                    ),
                    (
                        ServiceTab::Modules,
                        "Modules",
                        "Mining turrets and target locks",
                    ),
                    (ServiceTab::Drones, "Drones", "Drone fleet and cameras"),
                    (
                        ServiceTab::ShipComputer,
                        "AI",
                        "Ship computer and resource jobs",
                    ),
                    (ServiceTab::Cargo, "Inventory", "Cargo and station storage"),
                    (ServiceTab::Station, "Station", "Station services"),
                    (ServiceTab::Exchange, "Exchange", "Ore market"),
                    (
                        ServiceTab::Corporation,
                        "Corp",
                        "Corporation and shared storage",
                    ),
                    (ServiceTab::Activity, "Activity", "Server activity log"),
                    (ServiceTab::Ledger, "Audit", "Ledger verification"),
                    (
                        ServiceTab::Settings,
                        "Settings",
                        "View modes, theme and layout",
                    ),
                    (ServiceTab::Sandbox, "Surface", "Surface construction tools"),
                ] {
                    if !self.service_visible(tab) {
                        continue;
                    }
                    if ui
                        .add_sized(
                            Vec2::new(width, 24.0),
                            egui::Button::new(label).selected(self.service_tab == tab),
                        )
                        .on_hover_text(description)
                        .clicked()
                    {
                        self.service_tab = tab;
                        if tab == ServiceTab::Layouts {
                            self.services_open = true;
                        }
                        self.sound.cue(sound_engine::Cue::Ui);
                    }
                }
            });
        });
    }
    fn service_content(&mut self, ui: &mut egui::Ui) {
        if self.own().is_some_and(|p| p.station_avatar.is_some())
            && matches!(
                self.service_tab,
                ServiceTab::Navigation
                    | ServiceTab::Modules
                    | ServiceTab::Drones
                    | ServiceTab::ShipComputer
                    | ServiceTab::Sandbox
                    | ServiceTab::Hangar
                    | ServiceTab::Station
            )
        {
            self.station_actions(ui);
            ui.separator();
            ui.label("Ship controls are unavailable on foot. Inventory, room plans and account services remain accessible.");
            return;
        }
        match self.service_tab {
            ServiceTab::Layouts => {
                ui.label("Ship layout viewer is open in its own window.");
            }
            ServiceTab::Navigation => {
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
                    self.can_operate() && pilot.location == Location::Docked,
                    egui::Button::new("Undock").small(),
                )
                .clicked()
            {
                self.send(Request::Undock);
            }
            if ui
                .add_enabled(self.can_dock(&pilot), egui::Button::new("Dock").small())
                .clicked()
            {
                self.send(Request::Dock);
            }
            if ui
                .add_enabled(self.can_fly(), egui::Button::new("Stop").small())
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
            SelectedObject::Planet(id) => planets()
                .iter()
                .find(|p| p.id == *id && p.system_id == self.current_system())
                .map(|_| PLANET_APPROACH_POSITION),
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
        ui.small("Ctrl+Space: stop · RMB: aim · Release controls: assisted braking");
    }
    fn overview(&mut self, ui: &mut egui::Ui) {
        let system = self.current_system();
        ui.label(
            egui::RichText::new(system_name(system))
                .size(19.0)
                .color(self.accent())
                .strong(),
        );
        ui.small("Connected space / local objects");
        ui.separator();
        ui.label(
            egui::RichText::new("SELECTED OBJECT")
                .size(11.0)
                .color(MUTED),
        );
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
                if let Some(planet) = planets().iter().find(|p| p.id == id) {
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
            for planet in planets().into_iter().filter(|p| p.system_id == system) {
                rows.push((
                    SelectedObject::Planet(planet.id),
                    planet.name,
                    "Planet",
                    Some(PLANET_APPROACH_POSITION),
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
                            if ui
                                .selectable_label(
                                    self.selected_object == object,
                                    egui::RichText::new(name).size(12.0),
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
        ui.label(
            egui::RichText::new("STATION EXCHANGE")
                .color(self.accent())
                .strong(),
        );
        let own = self.own();
        let cargo_limit = self.own_cargo_capacity();
        let docked =
            self.can_operate() && own.as_ref().is_some_and(|p| p.location == Location::Docked);
        ui.small("Player sell orders · ore · credits per unit");
        ui.horizontal(|ui| {
            ui.label("Sell");
            ui.add(egui::DragValue::new(&mut self.sell_amount).range(1..=cargo_limit));
            ui.label("at");
            ui.add(egui::DragValue::new(&mut self.price).range(1..=1_000_000));
            let can_sell = docked && own.as_ref().is_some_and(|p| p.ore >= self.sell_amount);
            if ui
                .add_enabled(can_sell, egui::Button::new("List ore"))
                .clicked()
            {
                self.send(Request::PlaceSell {
                    units: self.sell_amount,
                    unit_price: self.price,
                });
            }
        });
        ui.horizontal(|ui| {
            ui.label("Buy quantity");
            ui.add(egui::DragValue::new(&mut self.buy_amount).range(1..=cargo_limit));
        });
        if !docked {
            ui.colored_label(AMBER, "Dock at the station to place or fill orders.");
        }
        let orders = self
            .snapshot
            .as_ref()
            .map(|s| s.orders.clone())
            .unwrap_or_default();
        egui::ScrollArea::vertical()
            .max_height(190.0)
            .show(ui, |ui| {
                if orders.is_empty() {
                    ui.colored_label(
                        MUTED,
                        "No listings yet. Mine ore and create the first order.",
                    );
                }
                for order in orders {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!(
                            "#{}   {}   {} ore × {} CR",
                            order.id, order.seller, order.units, order.unit_price
                        ));
                        if order.seller == self.pilot {
                            if ui
                                .add_enabled(
                                    docked
                                        && own.as_ref().is_some_and(|p| {
                                            p.ore.saturating_add(order.units)
                                                <= cargo_capacity(p.ship_class)
                                        }),
                                    egui::Button::new("Cancel"),
                                )
                                .clicked()
                            {
                                self.send(Request::CancelSell { order_id: order.id });
                            }
                        } else {
                            let affordable = own.as_ref().is_some_and(|p| {
                                order
                                    .unit_price
                                    .checked_mul(self.buy_amount)
                                    .is_some_and(|cost| cost <= p.credits)
                                    && p.ore.saturating_add(self.buy_amount)
                                        <= cargo_capacity(p.ship_class)
                            });
                            if ui
                                .add_enabled(
                                    docked && affordable && self.buy_amount <= order.units,
                                    egui::Button::new("Buy"),
                                )
                                .clicked()
                            {
                                self.send(Request::Buy {
                                    order_id: order.id,
                                    units: self.buy_amount,
                                });
                            }
                        }
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
        for planet in planets()
            .into_iter()
            .filter(|p| p.system_id == self.current_system())
        {
            let point = tactical_project(rect, PLANET_APPROACH_POSITION);
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
        self.poll();
        self.sync_remote_control();
        self.poll_voice();
        self.poll_ship_ai();
        self.update_sound();
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
        if !ctx.wants_keyboard_input()
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
                        ui.add_enabled_ui(!self.connecting, |ui| {
                            ui.horizontal(|ui| {
                                for (label, endpoint) in [("Production", transport::OFFICIAL_SERVER), ("Test", transport::TEST_SERVER)] {
                                    if ui.add_enabled(self.credential_scope.is_none(), egui::Button::new(label).selected(self.server == endpoint)).clicked() { self.server = endpoint.into(); }
                                }
                                ui.label("or custom:");
                            });
                            ui.label("Server address"); ui.add_enabled(self.credential_scope.is_none(), egui::TextEdit::singleline(&mut self.server).desired_width(380.0));
                            ui.label("Pilot name"); ui.add(egui::TextEdit::singleline(&mut self.pilot).hint_text("Choose a pilot name").desired_width(380.0));
                            ui.small("Creates a pilot on first connection. Saved credentials resume it automatically on this device.");
                            ui.add_space(10.0);
                            if ui.add_enabled(!self.pilot.trim().is_empty() && !self.server.trim().is_empty(), egui::Button::new("Enter the universe").min_size(Vec2::new(380.0, 44.0))).clicked() { self.connect(); }
                        });
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
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F11)) {
            toggle_fullscreen(ctx);
        }
        if !self.service_visible(self.service_tab) {
            self.service_tab = ServiceTab::Station;
        }
        let in_hangar = self.in_station_hangar();
        let bounds = ctx.screen_rect().shrink(8.0);
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
                        ui.set_max_width(490.0);
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
                                let interior_deck = self
                                    .own()
                                    .and_then(|p| p.station_avatar)
                                    .filter(|a| a.onboard_interior)
                                    .map(|a| a.interior_deck);
                                ui.strong(if let Some(deck) = interior_deck {
                                    format!("PIONEER INTERIOR · DECK {deck}")
                                } else if self.in_captains_quarters() {
                                    "CAPTAIN'S QUARTERS".into()
                                } else {
                                    "STATION HANGAR".into()
                                });
                            } else {
                                self.view_selector(ui);
                            }
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
        let (panel_pos, _) = self.layout.position(
            "service_launcher_overlay",
            bounds,
            [0.0, 0.0],
            Vec2::new(8.0, 156.0),
            Vec2::new(38.0, 330.0),
        );
        let panel_response =
            egui::Area::new(egui::Id::new("service_launcher_overlay").with(self.layout.generation))
                .current_pos(panel_pos)
                .movable(!self.layout.pinned)
                .constrain_to(bounds)
                .show(ctx, |ui| {
                    self.overlay_frame(ctx).show(ui, |ui| {
                        ui.set_width(28.0);
                        if self.service_visible(ServiceTab::Navigation)
                            && ui
                                .add_sized([28.0, 26.0], egui::Button::new("◎"))
                                .on_hover_text("Overview")
                                .clicked()
                        {
                            if self.view_mode == ViewMode::Cockpit {
                                self.service_tab = ServiceTab::Navigation;
                            } else {
                                self.overview_open = true;
                            }
                        }
                        for (tab, icon, label) in [
                            (ServiceTab::Station, "⌂", "Station"),
                            (ServiceTab::Hangar, "H", "Hangar"),
                            (ServiceTab::Layouts, "L", "Ship layouts"),
                            (ServiceTab::Cargo, "▣", "Inventory"),
                            (ServiceTab::Modules, "M", "Mining modules"),
                            (ServiceTab::Drones, "D", "Drone fleet"),
                            (ServiceTab::ShipComputer, "AI", "Ship computer"),
                            (ServiceTab::Corporation, "C", "Corporation"),
                            (ServiceTab::Exchange, "⇄", "Exchange"),
                            (ServiceTab::Activity, "≡", "Activity"),
                            (ServiceTab::Ledger, "✓", "Ledger audit"),
                            (ServiceTab::Settings, "⚙", "Settings"),
                        ] {
                            if !self.service_visible(tab) {
                                continue;
                            }
                            if ui
                                .add_sized(
                                    [28.0, 26.0],
                                    egui::Button::new(icon)
                                        .selected(self.services_open && self.service_tab == tab),
                                )
                                .on_hover_text(label)
                                .clicked()
                            {
                                self.service_tab = tab;
                                self.sound.cue(sound_engine::Cue::Ui);
                                self.services_open = true;
                            }
                        }
                    });
                });
        self.layout.record(
            "service_launcher_overlay",
            bounds,
            panel_response.response.rect,
            None,
            ctx.input(|i| i.pointer.any_down()),
        );
        if self.view_mode != ViewMode::Cockpit && !in_hangar {
            let mut overview_open = self.overview_open;
            let (panel_pos, panel_size) = self.layout.position(
                "overview",
                bounds,
                [1.0, 0.0],
                Vec2::new(-8.0, 8.0),
                Vec2::new(310.0, (bounds.height() - 260.0).max(280.0)),
            );
            let mut content_size = None;
            let panel_response = egui::Window::new("Overview")
                .id(egui::Id::new("cockpit_overview_overlay").with(self.layout.generation))
                .open(&mut overview_open)
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
                .min_width(275.0)
                .max_height(bounds.height() - 24.0)
                .resizable(!self.layout.pinned)
                .movable(!self.layout.pinned)
                .collapsible(true)
                .constrain_to(bounds)
                .frame(self.overlay_frame(ctx))
                .vscroll(true)
                .show(ctx, |ui| {
                    content_size = Some(ui.max_rect().size());
                    self.overview(ui);
                });
            if let Some(response) = panel_response {
                self.layout.record(
                    "overview",
                    bounds,
                    response.response.rect,
                    content_size,
                    ctx.input(|i| i.pointer.any_down()),
                );
            }
            self.overview_open = overview_open;
        }
        if self.view_mode != ViewMode::Cockpit && !in_hangar {
            let ship_width = (bounds.width() - 70.0).min(760.0);
            let (panel_pos, panel_size) = self.layout.position(
                "ship",
                bounds,
                [0.5, 1.0],
                Vec2::new(0.0, -8.0),
                Vec2::new(ship_width, 52.0),
            );
            let mut content_size = None;
            let panel_response = egui::Window::new("Ship controls")
                .id(egui::Id::new("ship_controls_overlay").with(self.layout.generation))
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
                .min_width(570.0)
                .resizable(!self.layout.pinned)
                .movable(!self.layout.pinned)
                .collapsible(true)
                .constrain_to(bounds)
                .frame(self.overlay_frame(ctx))
                .show(ctx, |ui| {
                    content_size = Some(ui.max_rect().size());
                    self.flight_deck(ui);
                });
            if let Some(response) = panel_response {
                self.layout.record(
                    "ship",
                    bounds,
                    response.response.rect,
                    content_size,
                    ctx.input(|i| i.pointer.any_down()),
                );
            }
        }
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
                .show(ctx, |ui| self.ship_layout_panel(ui));
            if !open {
                self.service_tab = ServiceTab::Modules;
            }
        }
        if self.services_open
            && self.service_tab != ServiceTab::Layouts
            && (self.view_mode != ViewMode::Cockpit || in_hangar)
        {
            let mut open = true;
            let (panel_pos, panel_size) = self.layout.position(
                "services",
                bounds,
                if in_hangar { [1.0, 0.0] } else { [0.0, 0.0] },
                if in_hangar {
                    Vec2::new(-8.0, 164.0)
                } else {
                    Vec2::new(64.0, 164.0)
                },
                Vec2::new(if in_hangar { 350.0 } else { 620.0 }, 310.0),
            );
            let mut content_size = None;
            let panel_response = egui::Window::new(
                if self.own().is_some_and(|p| p.location == Location::Docked) {
                    "Station services"
                } else {
                    "Personal services"
                },
            )
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
            .min_width(if in_hangar { 280.0 } else { 400.0 })
            .max_height(bounds.height() - 30.0)
            .resizable(!self.layout.pinned)
            .movable(!self.layout.pinned)
            .collapsible(true)
            .constrain_to(bounds)
            .frame(self.overlay_frame(ctx))
            .vscroll(true)
            .show(ctx, |ui| {
                content_size = Some(ui.max_rect().size());
                self.service_tabs(ui);
                ui.separator();
                self.service_content(ui);
            });
            if let Some(response) = panel_response {
                self.layout.record(
                    "services",
                    bounds,
                    response.response.rect,
                    content_size,
                    ctx.input(|i| i.pointer.any_down()),
                );
            }
            self.services_open = open;
        }
        if in_hangar {
            self.station_overlay(ctx);
        }
        if !in_hangar
            && self.view_mode == ViewMode::Cockpit
            && !self.flight_view.head_looking()
            && !ctx.input(|i| i.modifiers.shift)
        {
            {
                let rect = flight3d::cockpit_console_bounds(
                    ctx.available_rect(),
                    self.own().map_or(ShipClass::Pioneer, |p| p.ship_class),
                )
                .shrink(1.0);
                egui::Area::new(egui::Id::new("physical_cockpit_console"))
                    .fixed_pos(rect.min)
                    .constrain(false)
                    .movable(false)
                    .show(ctx, |ui| {
                        ui.set_min_size(rect.size());
                        ui.set_max_size(rect.size());
                        ui.set_clip_rect(rect);
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 3.0);
                        self.cockpit_actions(ui);
                        self.power_gauge(ui);
                        self.service_tabs(ui);
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .min_scrolled_height(0.0)
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
                            )
                            .id_salt("cockpit_console_content")
                            .max_height((rect.bottom() - ui.cursor().top()).max(20.0))
                            .max_width(rect.width())
                            .show(ui, |ui| {
                                ui.set_max_width(rect.width() - 18.0);
                                self.service_content(ui);
                            });
                    });
            }
        }
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
                        if let Some(action) = self.hangar_view.ui(
                            ui,
                            &pilot,
                            now,
                            live,
                            self.assets.as_ref().and_then(|a| a.hangar.as_ref()),
                            self.assets.as_ref().and_then(|a| a.pioneer_room.as_ref()),
                        ) {
                            self.activate_quarters_terminal(action);
                        }
                    }
                } else if self.view_mode != ViewMode::Tactical {
                    let now = self.presentation_time();
                    let display = self.snapshot.as_ref().map(|s| self.motion.sample(s, now));
                    if let Some(snapshot) = &display {
                        self.flight_view.set_drone_camera(self.drone_camera);
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
        if let Err(error) = self
            .layout
            .finish_frame(ctx.input(|i| i.pointer.any_down()))
        {
            self.settings_notice = error;
        }
        if self.view_mode != ViewMode::Cockpit {
            self.pilot_input.release_mouse(ctx);
        }
        let flight_allowed = !ctx.input(|i| i.pointer.primary_down())
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
            flight_allowed,
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
        self.sample_station_walk(ctx, in_hangar);
        self.boot.paint_handoff(ctx);
    }
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
fn main() -> eframe::Result<()> {
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
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("Arkline {}", BUILD_VERSION.trim());
                return Ok(());
            }
            "--help" | "-h" => {
                println!("Arkline native client\n\nUsage: arkline-client [--local | --remote-only] [--server WSS_URL_OR_HOST:PORT] [--data-dir PATH] [--pilot NAME]\n\n--pilot creates or resumes the named pilot immediately.\n--local starts a sibling arkline-server on a private loopback port.\nThe macOS bundle saves in ~/Library/Application Support/Arkline/Preview.\n--server selects an existing server; --remote-only disables an explicit --local option.\nDefault server: wss://relay.arkline.network/ws (Production). CLI storage: data/client.\n--credential-scope NAME selects a stable credential namespace (used by local preview cockpits).\nResume credentials are private local files. WSS verifies TLS certificates. Plain TCP is retained for local development.");
                return Ok(());
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
            cockpit.sound.start();
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
                    if text.galley.text() == "Start gathering" {
                        target = Some(text.pos + text.galley.size() * 0.5);
                    }
                }
            }
        }
        assert!(
            matches!(rx.try_recv().unwrap(),Request::StartResourceJob {target_units:100,pilot:Some(name)} if name=="worker")
        );
        cockpit.ai_pin = Some(("worker".into(), cockpit.connection_generation));
        cockpit.resource_pilot = None;
        cockpit.poll_ship_ai();
        assert!(cockpit.ai_pin.is_none());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn physical_ai_console_scrolls_prompt_into_clipped_interactive_screen() {
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.view_mode = ViewMode::Cockpit;
        cockpit.service_tab = ServiceTab::ShipComputer;
        cockpit.snapshot.as_mut().unwrap().pilots[0].location = Location::Belt;
        let viewport = egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(1106., 768.));
        let console = flight3d::cockpit_console_bounds(viewport, ShipClass::Pioneer).shrink(1.);
        let mut prompt = None;
        let mut saw_submit = false;
        for step in 0..35 {
            let events = if step > 3 {
                vec![
                    egui::Event::PointerMoved(console.center()),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: Vec2::new(0., -20.),
                        modifiers: Default::default(),
                    },
                ]
            } else {
                vec![]
            };
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(viewport),
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(&mut cockpit, ctx, &mut eframe::Frame::_new_kittest()),
            );
            for shape in output.shapes {
                if let egui::Shape::Text(t) = shape.shape {
                    let center = t.pos + t.galley.rect.center().to_vec2();
                    if t.galley.text() == "Submit instruction"
                        && console.contains(center)
                        && shape.clip_rect.contains(center)
                    {
                        saw_submit = true;
                    }
                    if t.galley.text() == "Gather 100 ore and bring it back"
                        && console.contains(center)
                        && shape.clip_rect.contains(center)
                    {
                        prompt = Some(center);
                    }
                }
            }
            if saw_submit && prompt.is_some() {
                break;
            }
        }
        assert!(saw_submit, "Submit must scroll into the physical screen");
        let point = prompt.expect("Prompt must become visible");
        for pressed in [true, false] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(viewport),
                    events: vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                    ..Default::default()
                },
                |ctx| eframe::App::update(&mut cockpit, ctx, &mut eframe::Frame::_new_kittest()),
            );
        }
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(viewport),
                events: vec![egui::Event::Text("Gather 20 ore".into())],
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut cockpit, ctx, &mut eframe::Frame::_new_kittest()),
        );
        assert_eq!(cockpit.ai_prompt, "Gather 20 ore");
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
            ("2D Tactical", ViewMode::Tactical),
            ("3D Tactical", ViewMode::Tactical3d),
            ("3D External", ViewMode::External),
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
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.selected_object = SelectedObject::Planet(1);
        assert_eq!(cockpit.selected_position(), Some(PLANET_APPROACH_POSITION));
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
            matches!(rx.try_recv().unwrap(),Request::Navigate{mode:NavigationMode::FlyTo{position}} if position==PLANET_APPROACH_POSITION)
        );
        for (index, class) in MINING_SHIP_CLASSES.into_iter().enumerate() {
            cockpit.snapshot.as_mut().unwrap().pilots[0].ship_class = class;
            assert_eq!(class.spec().mining_tier, Some(index as u8 + 1));
            assert_eq!(cockpit.own_cargo_capacity(), class.spec().cargo_capacity);
        }
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
                .any(|(text, _)| text.contains("Capacitor 2.5/10.0 kJ · +300 W")));
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
        let ctx = egui::Context::default();
        let mut cockpit = view_test_cockpit(&ctx);
        cockpit.view_mode = ViewMode::Cockpit;
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
                        Vec2::new(1500.0, 1000.0),
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
        let rect = cockpit.flight_view.cockpit_console_rect().unwrap();
        let labels = frame(&ctx, &mut cockpit, vec![]);
        assert!(!labels.iter().any(|(s, _)| s == "Overview"));
        let fleet = labels
            .iter()
            .find(|(s, p)| s == "Drones" && rect.contains(*p))
            .unwrap()
            .1;
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut cockpit,
                vec![
                    egui::Event::PointerMoved(fleet),
                    egui::Event::PointerButton {
                        pos: fleet,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(cockpit.service_tab == ServiceTab::Drones);
        let labels = frame(&ctx, &mut cockpit, vec![]);
        let launch = labels
            .iter()
            .find(|(s, p)| s == "Launch Recon 0/5" && rect.contains(*p))
            .unwrap()
            .1;
        frame(
            &ctx,
            &mut cockpit,
            vec![
                egui::Event::PointerMoved(launch),
                egui::Event::Key {
                    key: egui::Key::W,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: launch,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(
            cockpit.controls.is_empty(),
            "clicking console with W held must not emit thrust"
        );
        frame(
            &ctx,
            &mut cockpit,
            vec![
                egui::Event::Key {
                    key: egui::Key::W,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: launch,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::LaunchDroneClass {
                class: DroneClass::Recon
            }
        ));
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
                        protocol_version,
                        pilot,
                        resume_token,
                    } => {
                        assert_eq!(protocol_version, PROTOCOL_VERSION);
                        assert_eq!(pilot, "test-pilot");
                        assert_eq!(
                            resume_token.as_deref(),
                            reconnect.then_some("fixture-resume-token")
                        );
                    }
                    _ => panic!("first request must authenticate"),
                }
                let response = Response {
                    assets: None,
                    ok: true,
                    message: "Connected".into(),
                    resume_token: Some("fixture-resume-token".into()),
                    snapshot: None,
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
                    None,
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
fn main() {
    browser::start();
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod interior_acceptance;
