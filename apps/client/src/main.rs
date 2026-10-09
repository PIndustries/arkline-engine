//! Native Arkline cockpit. All displayed world data originates from the server.
mod audit;
mod flight3d;
use arkline_protocol::*;
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader, Write},
    net::{TcpStream, ToSocketAddrs},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

const TEAL: Color32 = Color32::from_rgb(89, 224, 198);
const AMBER: Color32 = Color32::from_rgb(255, 192, 104);
const MUTED: Color32 = Color32::from_rgb(138, 157, 180);

enum Update {
    Ready,
    Response(Box<Response>, bool),
    Failure(String),
}

fn credentials_path(dir: &std::path::Path, server: &str, pilot: &str) -> PathBuf {
    let identity = serde_json::to_vec(&(server, pilot)).unwrap();
    dir.join(format!(
        "pilot-{}.token",
        hex::encode(Sha256::digest(identity))
    ))
}
fn exchange(reader: &mut BufReader<TcpStream>, request: &Request) -> Result<Response, String> {
    let mut bytes = serde_json::to_vec(request).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    reader
        .get_mut()
        .write_all(&bytes)
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    // The development server is trusted; cap each response to protect the UI process.
    let n = std::io::Read::take(reader, 16 * 1024 * 1024)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("Server closed the connection".into());
    }
    if !line.ends_with('\n') {
        return Err("Server response exceeded the size limit".into());
    }
    serde_json::from_str(&line).map_err(|e| format!("Invalid server response: {e}"))
}
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
fn worker(
    server: String,
    pilot: String,
    dir: PathBuf,
    credential_scope: Option<String>,
    commands: Receiver<Request>,
    updates: Sender<Update>,
) {
    let run = || -> Result<(), String> {
        let path = credentials_path(&dir, credential_scope.as_deref().unwrap_or(&server), &pilot);
        let token = match fs::read_to_string(&path) {
            Ok(v) => Some(v),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("Cannot read saved credential: {e}")),
        };
        // Check storage before creating a server-side identity.
        fs::create_dir_all(&dir).map_err(|e| format!("Credential directory unavailable: {e}"))?;
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
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        stream.set_nodelay(true).map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream);
        let mut response = exchange(
            &mut reader,
            &Request::Connect {
                protocol_version: PROTOCOL_VERSION,
                pilot,
                resume_token: token,
            },
        )?;
        if !response.ok {
            return Err(response.message);
        }
        if let Some(token) = response.resume_token.take() {
            save_token(&path, &token)?;
        }
        updates.send(Update::Ready).map_err(|e| e.to_string())?;
        updates
            .send(Update::Response(Box::new(response), true))
            .map_err(|e| e.to_string())?;
        loop {
            let (request, visible) = match commands.recv_timeout(Duration::from_millis(350)) {
                Ok(request) => (request, true),
                Err(mpsc::RecvTimeoutError::Timeout) => (Request::Snapshot, false),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            };
            let response = exchange(&mut reader, &request).map_err(|error| {
                if visible { format!("{error}. Action outcome unknown; reconnect and inspect state before retrying.") }
                else { error }
            })?;
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
    Cargo,
    Exchange,
    Activity,
    Ledger,
}
#[derive(Clone, PartialEq, Eq)]
enum SelectedObject {
    Station,
    Belt,
    Pilot(String),
}

struct Cockpit {
    server: String,
    pilot: String,
    data_dir: PathBuf,
    credential_scope: Option<String>,
    commands: Option<Sender<Request>>,
    updates: Option<Receiver<Update>>,
    connecting: bool,
    connected: bool,
    snapshot: Option<Snapshot>,
    observed: Instant,
    feed: VecDeque<(String, bool)>,
    amount: u64,
    price: u64,
    buy_amount: u64,
    notice: String,
    three_dimensional: bool,
    flight_view: flight3d::FlightView,
    frozen_time: Option<u64>,
    focus_on_first_frame: bool,
    local_server: Option<LocalServer>,
    service_tab: ServiceTab,
    selected_object: SelectedObject,
    sell_amount: u64,
    audit_updates: Option<Receiver<Result<String, String>>>,
    audit_result: Option<Result<String, String>>,
    request_pending: bool,
}
impl Cockpit {
    fn new(
        cc: &eframe::CreationContext<'_>,
        server: String,
        data_dir: PathBuf,
        credential_scope: Option<String>,
    ) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::from_rgb(12, 19, 30);
        style.visuals.window_fill = Color32::from_rgb(17, 27, 41);
        style.visuals.selection.bg_fill = Color32::from_rgb(29, 102, 97);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(29, 43, 60);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(39, 70, 83);
        style.spacing.item_spacing = Vec2::new(10.0, 10.0);
        cc.egui_ctx.set_style(style);
        Self {
            server,
            pilot: String::new(),
            data_dir,
            credential_scope,
            commands: None,
            updates: None,
            connecting: false,
            connected: false,
            snapshot: None,
            observed: Instant::now(),
            feed: VecDeque::new(),
            amount: 25,
            price: 10,
            buy_amount: 1,
            notice: String::new(),
            three_dimensional: true,
            flight_view: flight3d::FlightView::new(cc.gl.as_ref()),
            frozen_time: None,
            focus_on_first_frame: true,
            local_server: None,
            service_tab: ServiceTab::Cargo,
            selected_object: SelectedObject::Station,
            sell_amount: 25,
            audit_updates: None,
            audit_result: None,
            request_pending: false,
        }
    }
    fn log(&mut self, text: String, error: bool) {
        self.feed.push_front((text, error));
        self.feed.truncate(60);
    }
    fn presentation_time(&self) -> u64 {
        self.frozen_time.unwrap_or_else(|| {
            self.snapshot.as_ref().map_or(0, |s| {
                s.server_time_ms
                    .saturating_add(self.observed.elapsed().as_millis() as u64)
            })
        })
    }
    fn connect(&mut self) {
        self.feed.clear();
        self.selected_object = SelectedObject::Station;
        self.audit_result = None;
        self.audit_updates = None;
        self.request_pending = false;
        self.frozen_time = None;
        self.pilot = self.pilot.trim().to_string();
        self.snapshot = None;
        let (tx, rx) = mpsc::channel();
        let (out_tx, out_rx) = mpsc::channel();
        let server = self.server.clone();
        let pilot = self.pilot.clone();
        let dir = self.data_dir.clone();
        let scope = self.credential_scope.clone();
        std::thread::spawn(move || worker(server, pilot, dir, scope, rx, out_tx));
        self.commands = Some(tx);
        self.updates = Some(out_rx);
        self.connecting = true;
        self.notice = "Connecting…".into();
    }
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
    fn can_operate(&self) -> bool {
        self.connected && !self.request_pending
    }
    fn send(&mut self, request: Request) {
        if !self.can_operate() {
            return;
        }
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
                    self.connected = true;
                    self.connecting = false;
                    self.notice.clear();
                }
                Update::Failure(error) => {
                    self.request_pending = false;
                    self.frozen_time = Some(self.presentation_time());
                    self.connected = false;
                    self.connecting = false;
                    self.commands = None;
                    self.notice = error.clone();
                    self.log(error, true);
                }
                Update::Response(response, visible) => {
                    if visible {
                        self.request_pending = false;
                    }
                    if let Some(snapshot) = response.snapshot {
                        self.snapshot = Some(snapshot);
                        self.observed = Instant::now();
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
            .find(|p| p.name == self.pilot)
            .cloned()
    }
    fn flight(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("PILOT / VESSEL").color(TEAL).strong());
        if let Some(pilot) = self.own() {
            ui.heading(&pilot.name);
            ui.colored_label(MUTED, location_label(&pilot.location));
            ui.add_space(8.0);
            egui::Frame::group(ui.style())
                .fill(Color32::from_rgb(18, 33, 47))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.small("AVAILABLE BALANCE");
                    ui.label(
                        egui::RichText::new(format!("{} CR", pilot.credits))
                            .size(25.0)
                            .color(AMBER),
                    );
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("ORE HOLD");
                        ui.colored_label(TEAL, format!("{} / {}", pilot.ore, CARGO_CAPACITY));
                    });
                    ui.add(
                        egui::ProgressBar::new(pilot.ore as f32 / CARGO_CAPACITY as f32)
                            .fill(TEAL)
                            .desired_height(6.0),
                    );
                    let escrow: u64 = self
                        .snapshot
                        .as_ref()
                        .map(|s| {
                            s.orders
                                .iter()
                                .filter(|o| o.seller == self.pilot)
                                .map(|o| o.units)
                                .sum()
                        })
                        .unwrap_or(0);
                    ui.small(format!("{} ore listed on exchange", escrow));
                });
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("FLIGHT OPERATIONS")
                    .color(TEAL)
                    .strong(),
            );
            if self.request_pending {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.small("Awaiting server confirmation");
                });
            }
            let active = self.can_operate();
            let (guidance, primary) = match pilot.location {
                Location::Docked if pilot.ore > 0 => (
                    "Ore secured. Open the exchange to list cargo for another pilot.",
                    "Undock",
                ),
                Location::Docked => (
                    "Undock, travel to the belt, then mine your first cargo.",
                    "Undock",
                ),
                Location::Station => (
                    "Station perimeter. Dock for market access or travel to the belt.",
                    "Dock at station",
                ),
                Location::Belt => (
                    "Extract ore here, then return to the station to trade.",
                    "Return to station",
                ),
                Location::InTransit { .. } => (
                    "Transit is in progress. Navigation resumes after server-confirmed arrival.",
                    "In transit",
                ),
            };
            ui.colored_label(MUTED, guidance);
            let can_navigate = active && !matches!(pilot.location, Location::InTransit { .. });
            if ui
                .add_enabled(
                    can_navigate,
                    egui::Button::new(primary).min_size(Vec2::new(ui.available_width(), 34.0)),
                )
                .clicked()
            {
                self.send(match pilot.location {
                    Location::Docked => Request::Undock,
                    Location::Station => Request::Dock,
                    _ => Request::Warp {
                        destination: Destination::Station,
                    },
                });
            }
            if matches!(pilot.location, Location::Station)
                && ui
                    .add_enabled(
                        active,
                        egui::Button::new("Warp to asteroid belt")
                            .min_size(Vec2::new(ui.available_width(), 30.0)),
                    )
                    .clicked()
            {
                self.send(Request::Warp {
                    destination: Destination::Belt,
                });
            }
            if let Location::InTransit { arrives_at_ms, .. } = pilot.location {
                let remaining = arrives_at_ms.saturating_sub(self.presentation_time());
                ui.add(
                    egui::ProgressBar::new(
                        1.0 - remaining.min(WARP_DURATION_MS) as f32 / WARP_DURATION_MS as f32,
                    )
                    .fill(TEAL)
                    .text(format!("{:.1}s estimated", remaining as f32 / 1000.0)),
                );
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Mine units");
                ui.add(egui::DragValue::new(&mut self.amount).range(1..=MAX_MINE_UNITS));
            });
            if ui
                .add_enabled(
                    self.can_mine(&pilot),
                    egui::Button::new("Extract ore")
                        .min_size(Vec2::new(ui.available_width(), 32.0)),
                )
                .clicked()
            {
                self.send(Request::Mine { units: self.amount });
            }
            if pilot.location != Location::Belt {
                ui.small("Extraction requires arrival at the belt.");
            }
            if ui.button("Inspect cargo").clicked() {
                self.service_tab = ServiceTab::Cargo;
            }
            if ui.button("Open exchange").clicked() {
                self.service_tab = ServiceTab::Exchange;
            }
        }
    }
    fn can_mine(&self, pilot: &PilotView) -> bool {
        self.can_operate()
            && self.amount > 0
            && self.amount <= MAX_MINE_UNITS
            && pilot.location == Location::Belt
            && pilot.ore.saturating_add(self.amount) <= CARGO_CAPACITY
            && self
                .snapshot
                .as_ref()
                .is_some_and(|s| s.belt_ore >= self.amount)
    }
    fn overview(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("SECTOR OVERVIEW").color(TEAL).strong());
        ui.small("01 / Arkline orbital region");
        ui.separator();
        ui.selectable_value(
            &mut self.selected_object,
            SelectedObject::Station,
            "◇  Arkline Station",
        );
        ui.selectable_value(
            &mut self.selected_object,
            SelectedObject::Belt,
            "◆  Asteroid Belt",
        );
        ui.add_space(8.0);
        let pilots = self
            .snapshot
            .as_ref()
            .map(|s| s.pilots.clone())
            .unwrap_or_default();
        ui.label(
            egui::RichText::new(format!("KNOWN PILOTS  /  {}", pilots.len()))
                .size(11.0)
                .color(MUTED),
        );
        egui::ScrollArea::vertical()
            .id_salt("overview-roster")
            .max_height(150.0)
            .show(ui, |ui| {
                for pilot in &pilots {
                    let label = format!(
                        "{} {}",
                        if pilot.name == self.pilot {
                            "●"
                        } else {
                            "○"
                        },
                        pilot.name
                    );
                    ui.selectable_value(
                        &mut self.selected_object,
                        SelectedObject::Pilot(pilot.name.clone()),
                        label,
                    );
                }
            });
        ui.small("Registered pilots; presence is not tracked.");
        ui.add_space(10.0);
        ui.separator();
        ui.label(egui::RichText::new("SELECTED OBJECT").color(TEAL).strong());
        let own = self.own();
        match self.selected_object.clone() {
            SelectedObject::Station => {
                ui.heading("Arkline Station");
                ui.colored_label(MUTED, "Orbital trade facility");
                ui.label("Dock to exchange ore, settle trades, or manage sell orders.");
                if let Some(pilot) = &own {
                    match pilot.location {
                        Location::Docked => {
                            ui.colored_label(TEAL, "You are docked here");
                            if ui.button("Station exchange").clicked() {
                                self.service_tab = ServiceTab::Exchange;
                            }
                        }
                        Location::Station => {
                            if ui
                                .add_enabled(self.can_operate(), egui::Button::new("Dock"))
                                .clicked()
                            {
                                self.send(Request::Dock);
                            }
                        }
                        Location::Belt => {
                            if ui
                                .add_enabled(
                                    self.can_operate(),
                                    egui::Button::new("Warp to station"),
                                )
                                .clicked()
                            {
                                self.send(Request::Warp {
                                    destination: Destination::Station,
                                });
                            }
                        }
                        Location::InTransit { .. } => {
                            ui.small("Navigation locked during transit");
                        }
                    }
                }
            }
            SelectedObject::Belt => {
                ui.heading("Asteroid Belt");
                ui.colored_label(MUTED, "Ore extraction site");
                if let Some(s) = &self.snapshot {
                    ui.label(
                        egui::RichText::new(format!("{} ore remaining", s.belt_ore)).color(AMBER),
                    );
                }
                ui.label(format!("Extraction batches: up to {} units. Cargo is credited after server confirmation.", MAX_MINE_UNITS));
                if let Some(pilot) = &own {
                    match pilot.location {
                        Location::Docked => {
                            ui.small("Undock before entering the route.");
                            if ui
                                .add_enabled(self.can_operate(), egui::Button::new("Undock"))
                                .clicked()
                            {
                                self.send(Request::Undock);
                            }
                        }
                        Location::Station => {
                            if ui
                                .add_enabled(self.can_operate(), egui::Button::new("Warp to belt"))
                                .clicked()
                            {
                                self.send(Request::Warp {
                                    destination: Destination::Belt,
                                });
                            }
                        }
                        Location::Belt => {
                            if ui
                                .add_enabled(
                                    self.can_mine(pilot),
                                    egui::Button::new(format!("Extract {} ore", self.amount)),
                                )
                                .clicked()
                            {
                                self.send(Request::Mine { units: self.amount });
                            }
                        }
                        Location::InTransit { .. } => {
                            ui.small("Navigation locked during transit");
                        }
                    }
                }
            }
            SelectedObject::Pilot(name) => {
                if let Some(pilot) = pilots.iter().find(|p| p.name == name) {
                    ui.heading(&pilot.name);
                    ui.colored_label(MUTED, location_label(&pilot.location));
                    ui.separator();
                    ui.label(format!("{} CR", pilot.credits));
                    ui.label(format!("{} ore in cargo", pilot.ore));
                    let order_count = self
                        .snapshot
                        .as_ref()
                        .map_or(0, |s| s.orders.iter().filter(|o| o.seller == name).count());
                    ui.label(format!("{} active sell orders", order_count));
                    if ui.button("View exchange").clicked() {
                        self.service_tab = ServiceTab::Exchange;
                    }
                } else {
                    ui.small("Pilot unavailable in current snapshot.");
                }
            }
        }
        ui.add_space(12.0);
        ui.separator();
        if let Some((message, error)) = self.feed.front() {
            ui.label(egui::RichText::new("LATEST UPDATE").size(11.0).color(MUTED));
            ui.colored_label(if *error { AMBER } else { TEAL }, message);
        }
    }
    fn cargo_panel(&mut self, ui: &mut egui::Ui) {
        if let Some(pilot) = self.own() {
            let escrow: u64 = self
                .snapshot
                .as_ref()
                .map(|s| {
                    s.orders
                        .iter()
                        .filter(|o| o.seller == self.pilot)
                        .map(|o| o.units)
                        .sum()
                })
                .unwrap_or(0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("ORE INVENTORY").color(TEAL).strong());
                ui.colored_label(MUTED, "Server-confirmed holdings");
            });
            egui::Grid::new("cargo-grid")
                .num_columns(3)
                .spacing([45.0, 10.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Location");
                    ui.strong("Units");
                    ui.strong("Availability");
                    ui.end_row();
                    ui.label("Vessel cargo");
                    ui.colored_label(TEAL, pilot.ore.to_string());
                    ui.label("Available to list while docked");
                    ui.end_row();
                    ui.label("Exchange escrow");
                    ui.colored_label(AMBER, escrow.to_string());
                    ui.label("Reserved by your active sell orders");
                    ui.end_row();
                    ui.label("Free hold capacity");
                    ui.label(CARGO_CAPACITY.saturating_sub(pilot.ore).to_string());
                    ui.label("Mining, purchases, and cancellations need room");
                    ui.end_row();
                });
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Manage sell orders").clicked() {
                    self.service_tab = ServiceTab::Exchange;
                }
                ui.colored_label(
                    MUTED,
                    if pilot.ore == 0 {
                        "Select the belt in Overview to begin gathering cargo."
                    } else {
                        "Dock at the station to offer this cargo to other pilots."
                    },
                );
            });
        }
    }
    fn ledger_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("ECONOMIC LEDGER").color(TEAL).strong());
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
                std::thread::spawn(move || {
                    let _ = tx.send(audit::audit(&server, &path));
                });
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
                ui.colored_label(TEAL, result);
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
        ui.label(egui::RichText::new("STATION EXCHANGE").color(TEAL).strong());
        let own = self.own();
        let docked =
            self.can_operate() && own.as_ref().is_some_and(|p| p.location == Location::Docked);
        ui.small("Player sell orders · ore · credits per unit");
        ui.horizontal(|ui| {
            ui.label("Sell");
            ui.add(egui::DragValue::new(&mut self.sell_amount).range(1..=CARGO_CAPACITY));
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
            ui.add(egui::DragValue::new(&mut self.buy_amount).range(1..=CARGO_CAPACITY));
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
                                            p.ore.saturating_add(order.units) <= CARGO_CAPACITY
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
                                    && p.ore.saturating_add(self.buy_amount) <= CARGO_CAPACITY
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
    fn scene(&self, ui: &mut egui::Ui) {
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), ui.available_height().max(220.0)),
            egui::Sense::hover(),
        );
        let paint = ui.painter_at(rect);
        paint.rect_filled(rect, 9.0, Color32::from_rgb(6, 13, 24));
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
        let station = Pos2::new(rect.left() + rect.width() * 0.23, rect.center().y);
        let belt = Pos2::new(rect.left() + rect.width() * 0.78, rect.center().y);
        paint.line_segment(
            [station, belt],
            Stroke::new(1.0, Color32::from_rgb(35, 57, 76)),
        );
        paint.circle_stroke(station, 36.0, Stroke::new(1.5, TEAL));
        paint.circle_stroke(station, 24.0, Stroke::new(1.0, TEAL.gamma_multiply(0.4)));
        paint.rect_filled(
            egui::Rect::from_center_size(station, Vec2::new(15.0, 45.0)),
            3.0,
            TEAL.gamma_multiply(0.6),
        );
        paint.rect_filled(
            egui::Rect::from_center_size(station, Vec2::new(45.0, 15.0)),
            3.0,
            TEAL.gamma_multiply(0.6),
        );
        paint.text(
            station + Vec2::new(0.0, 60.0),
            egui::Align2::CENTER_TOP,
            "ARKLINE STATION",
            egui::FontId::proportional(13.0),
            TEAL,
        );
        for i in 0..15 {
            let angle = i as f32 * 2.399;
            let distance = 15.0 + (i % 5) as f32 * 8.0;
            paint.circle_filled(
                belt + Vec2::angled(angle) * distance,
                3.0 + (i % 4) as f32,
                Color32::from_rgb(121, 105, 91),
            );
        }
        paint.text(
            belt + Vec2::new(0.0, 60.0),
            egui::Align2::CENTER_TOP,
            "ASTEROID BELT",
            egui::FontId::proportional(13.0),
            AMBER,
        );
        if let Some(snapshot) = &self.snapshot {
            let server_now = self.presentation_time();
            for pilot in &snapshot.pilots {
                let t = flight3d::route_progress(&pilot.location, server_now);
                let position = station.lerp(belt, t)
                    + Vec2::new(0.0, -56.0 - flight3d::lane(&pilot.name) * 14.0);
                let heading = flight3d::heading(&pilot.location);
                let color = if pilot.name == self.pilot {
                    TEAL
                } else {
                    AMBER
                };
                if matches!(pilot.location, Location::InTransit { .. }) {
                    let dir = -heading;
                    paint.line_segment(
                        [position, position + Vec2::new(dir * 28.0, 0.0)],
                        Stroke::new(2.0, color.gamma_multiply(0.4)),
                    );
                }
                paint.add(egui::Shape::convex_polygon(
                    vec![
                        position + Vec2::new(heading * 8.0, 0.0),
                        position + Vec2::new(-heading * 6.0, -5.0),
                        position + Vec2::new(-heading * 6.0, 5.0),
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
                format!("SECTOR 01   /   {} ore remaining", snapshot.belt_ore),
                egui::FontId::monospace(12.0),
                MUTED,
            );
        }
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
fn location_label(location: &Location) -> &'static str {
    match location {
        Location::Docked => "Docked · Arkline Station",
        Location::Station => "Space · Station perimeter",
        Location::Belt => "Space · Asteroid belt",
        Location::InTransit { .. } => "Warp in progress",
    }
}
impl eframe::App for Cockpit {
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        // macOS application termination may bypass unwinding main's stack.
        // Shut down only this cockpit's owned host inside the lifecycle callback.
        drop(self.local_server.take());
        self.flight_view.destroy();
    }
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if self.focus_on_first_frame {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.focus_on_first_frame = false;
        }
        self.poll();
        ctx.request_repaint_after(Duration::from_millis(33));
        egui::TopBottomPanel::top("brand").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("A R K L I N E")
                        .size(25.0)
                        .color(TEAL)
                        .strong(),
                );
                ui.colored_label(MUTED, "PRIVATE PREVIEW");
                ui.separator();
                ui.selectable_value(&mut self.three_dimensional, false, "2D Tactical");
                ui.selectable_value(&mut self.three_dimensional, true, "3D Flight");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.colored_label(
                        if self.connected { TEAL } else { AMBER },
                        if self.connected {
                            "● ONLINE"
                        } else if self.connecting {
                            "○ CONNECTING"
                        } else {
                            "○ OFFLINE"
                        },
                    );
                });
            });
            ui.add_space(8.0);
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.small(&self.server);
                if let Some(s) = &self.snapshot {
                    ui.separator();
                    ui.small(format!(
                        "Ledger #{} · {}",
                        s.ledger_sequence,
                        s.ledger_hash.chars().take(16).collect::<String>()
                    ));
                    ui.colored_label(MUTED, "Server checkpoint · Audit in Ledger");
                }
            });
        });
        if !self.connected && self.snapshot.is_none() {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_space((ui.available_height() * 0.13).max(20.0));
                ui.vertical_centered(|ui| {
                    ui.heading(egui::RichText::new("Your universe starts here.").size(34.0));
                    ui.add_space(8.0); ui.colored_label(MUTED, "Connect to a real Arkline server. Fly, mine, trade, and verify.");
                    ui.add_space(28.0);
                    ui.scope(|ui| {
                        ui.set_max_width(400.0);
                        ui.add_enabled_ui(!self.connecting, |ui| {
                            ui.label("Server address"); ui.add_enabled(self.credential_scope.is_none(), egui::TextEdit::singleline(&mut self.server).desired_width(380.0));
                            ui.label("Pilot name"); ui.add(egui::TextEdit::singleline(&mut self.pilot).hint_text("Choose a pilot name").desired_width(380.0));
                            ui.small("Creates a pilot on first connection. Saved credentials resume it automatically on this computer.");
                            ui.add_space(10.0);
                            if ui.add_enabled(!self.pilot.trim().is_empty() && !self.server.trim().is_empty(), egui::Button::new("Enter the universe").min_size(Vec2::new(380.0, 44.0))).clicked() { self.connect(); }
                        });
                        if !self.notice.is_empty() { ui.add_space(12.0); ui.colored_label(AMBER, &self.notice); }
                        ui.add_space(22.0); ui.separator();
                        if self.credential_scope.is_some() {
                            ui.colored_label(TEAL, "Local universe is running. Your world saves automatically.");
                            ui.small("Keep the hosting cockpit open while other pilots play.");
                            if ui.button("Open second cockpit").clicked() { self.second_cockpit(); }
                        } else {
                            ui.label("Start a local universe from the repository:");
                            ui.code("cargo run -p arkline-server");
                        }
                        ui.small("This prototype uses local TCP. Use a secure tunnel for remote play.");
                    });
                });
            });
            return;
        }
        egui::SidePanel::left("flight")
            .exact_width(220.0)
            .resizable(false)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("flight-scroll")
                    .show(ui, |ui| {
                        ui.add_space(10.0);
                        self.flight(ui);
                        ui.add_space(12.0);
                        ui.separator();
                        ui.label(egui::RichText::new("CONNECTION").size(11.0).color(MUTED));
                        if self.connected {
                            if ui.button("Open second cockpit").clicked() {
                                self.second_cockpit();
                            }
                            if ui
                                .add_enabled(!self.request_pending, egui::Button::new("Disconnect"))
                                .clicked()
                            {
                                self.commands = None;
                                self.updates = None;
                                self.connected = false;
                                self.snapshot = None;
                            }
                        } else {
                            ui.colored_label(AMBER, &self.notice);
                            if ui
                                .add_enabled(!self.connecting, egui::Button::new("Reconnect"))
                                .clicked()
                            {
                                self.connect();
                            }
                            if ui.button("Change pilot / server").clicked() {
                                self.snapshot = None;
                                self.commands = None;
                                self.updates = None;
                            }
                        }
                    });
            });
        egui::SidePanel::right("overview")
            .exact_width(230.0)
            .resizable(false)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("overview-scroll")
                    .show(ui, |ui| {
                        ui.add_space(10.0);
                        self.overview(ui);
                    });
            });
        egui::TopBottomPanel::bottom("services")
            .default_height(235.0)
            .min_height(185.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("SERVICES").color(MUTED).size(11.0));
                    ui.selectable_value(&mut self.service_tab, ServiceTab::Cargo, "Cargo");
                    ui.selectable_value(&mut self.service_tab, ServiceTab::Exchange, "Exchange");
                    ui.selectable_value(&mut self.service_tab, ServiceTab::Activity, "Activity");
                    ui.selectable_value(&mut self.service_tab, ServiceTab::Ledger, "Ledger");
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("service-content")
                    .show(ui, |ui| match self.service_tab {
                        ServiceTab::Cargo => self.cargo_panel(ui),
                        ServiceTab::Exchange => self.market(ui),
                        ServiceTab::Ledger => self.ledger_panel(ui),
                        ServiceTab::Activity => {
                            ui.label(egui::RichText::new("SERVER ACTIVITY").color(TEAL).strong());
                            for (entry, error) in &self.feed {
                                ui.colored_label(if *error { AMBER } else { MUTED }, entry);
                                ui.separator();
                            }
                        }
                    });
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.three_dimensional {
                let now = self.presentation_time();
                if let Some(snapshot) = &self.snapshot {
                    self.flight_view
                        .ui(ui, snapshot, &self.pilot, now, self.connected);
                }
            } else {
                self.scene(ui);
            }
        });
    }
}
/// Only the process that spawned a local host owns its lifetime.
struct LocalServer(Child);
impl Drop for LocalServer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
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
fn main() -> eframe::Result<()> {
    let bundled = std::env::current_exe()
        .ok()
        .is_some_and(|path| path.parent().is_some_and(|p| p.ends_with("Contents/MacOS")));
    let mut local = bundled;
    let mut remote_only = false;
    let mut explicit_server = false;
    let mut server = "127.0.0.1:7777".to_string();
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
            "--help" | "-h" => {
                println!("Arkline native client\n\nUsage: arkline-client [--local | --remote-only] [--server HOST:PORT] [--data-dir PATH] [--pilot NAME]\n\n--pilot creates or resumes the named pilot immediately.\n--local starts a sibling arkline-server on a private loopback port.\nA macOS app bundle starts locally by default and saves in ~/Library/Application Support/Arkline/Preview.\n--server selects an existing server; --remote-only prevents bundle auto-hosting.\nCLI defaults: 127.0.0.1:7777, data/client.\n--credential-scope NAME selects a stable credential namespace (used by local preview cockpits).\nResume credentials are private local files. Development TCP requires a secure tunnel for remote use.");
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
        depth_buffer: 24,
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([960.0, 680.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Arkline",
        options,
        Box::new(move |cc| {
            let mut cockpit = Cockpit::new(cc, server, data_dir, credential_scope);
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
