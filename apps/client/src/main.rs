//! Native Arkline cockpit. All displayed world data originates from the server.
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
        }
    }
    fn log(&mut self, text: String, error: bool) {
        self.feed.push_front((text, error));
        self.feed.truncate(60);
    }
    fn connect(&mut self) {
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
    fn send(&mut self, request: Request) {
        if let Some(tx) = &self.commands {
            let _ = tx.send(request);
        }
    }
    fn poll(&mut self) {
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
                    self.connected = false;
                    self.connecting = false;
                    self.commands = None;
                    self.notice = error.clone();
                    self.log(error, true);
                }
                Update::Response(response, visible) => {
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
        ui.label(egui::RichText::new("FLIGHT CONTROL").color(TEAL).strong());
        if let Some(pilot) = self.own() {
            ui.heading(&pilot.name);
            ui.colored_label(MUTED, location_label(&pilot.location));
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(format!("{}  CR", pilot.credits))
                    .size(25.0)
                    .color(AMBER),
            );
            ui.label(format!("Cargo   {} / {} ore", pilot.ore, CARGO_CAPACITY));
            ui.add(egui::ProgressBar::new(pilot.ore as f32 / CARGO_CAPACITY as f32).fill(TEAL));
            ui.separator();
            let active = self.connected;
            if ui
                .add_enabled(
                    active && pilot.location == Location::Docked,
                    egui::Button::new("Undock").min_size(Vec2::new(210.0, 34.0)),
                )
                .clicked()
            {
                self.send(Request::Undock);
            }
            if ui
                .add_enabled(
                    active && pilot.location == Location::Station,
                    egui::Button::new("Warp to asteroid belt").min_size(Vec2::new(210.0, 34.0)),
                )
                .clicked()
            {
                self.send(Request::Warp {
                    destination: Destination::Belt,
                });
            }
            if ui
                .add_enabled(
                    active && pilot.location == Location::Belt,
                    egui::Button::new("Warp to station").min_size(Vec2::new(210.0, 34.0)),
                )
                .clicked()
            {
                self.send(Request::Warp {
                    destination: Destination::Station,
                });
            }
            if ui
                .add_enabled(
                    active && pilot.location == Location::Station,
                    egui::Button::new("Dock at station").min_size(Vec2::new(210.0, 34.0)),
                )
                .clicked()
            {
                self.send(Request::Dock);
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Ore units");
                ui.add(egui::DragValue::new(&mut self.amount).range(1..=MAX_MINE_UNITS));
            });
            let can_mine = self.amount <= MAX_MINE_UNITS
                && active
                && pilot.location == Location::Belt
                && pilot.ore.saturating_add(self.amount) <= CARGO_CAPACITY
                && self
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| s.belt_ore >= self.amount);
            if ui
                .add_enabled(
                    can_mine,
                    egui::Button::new("Mine ore").min_size(Vec2::new(210.0, 34.0)),
                )
                .clicked()
            {
                self.send(Request::Mine { units: self.amount });
            }
            ui.colored_label(MUTED, "Mine at the belt. Dock to trade.");
        }
        ui.add_space(18.0);
        ui.separator();
        ui.label(egui::RichText::new("LOCAL PILOTS").color(TEAL));
        if let Some(snapshot) = &self.snapshot {
            egui::ScrollArea::vertical()
                .max_height(190.0)
                .show(ui, |ui| {
                    for pilot in &snapshot.pilots {
                        ui.label(format!(
                            "{} {}",
                            if pilot.name == self.pilot {
                                "◆"
                            } else {
                                "◇"
                            },
                            pilot.name
                        ));
                        ui.small(location_label(&pilot.location));
                    }
                });
        }
    }
    fn market(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("STATION EXCHANGE").color(TEAL).strong());
        let own = self.own();
        let docked = self.connected && own.as_ref().is_some_and(|p| p.location == Location::Docked);
        ui.small("Player sell orders · ore · credits per unit");
        ui.horizontal(|ui| {
            ui.label("Sell");
            ui.add(egui::DragValue::new(&mut self.amount).range(1..=CARGO_CAPACITY));
            ui.label("at");
            ui.add(egui::DragValue::new(&mut self.price).range(1..=1_000_000));
            let can_sell = docked && own.as_ref().is_some_and(|p| p.ore >= self.amount);
            if ui
                .add_enabled(can_sell, egui::Button::new("List ore"))
                .clicked()
            {
                self.send(Request::PlaceSell {
                    units: self.amount,
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
            let server_now = snapshot
                .server_time_ms
                .saturating_add(self.observed.elapsed().as_millis() as u64);
            for (i, pilot) in snapshot.pilots.iter().enumerate() {
                let t = match pilot.location {
                    Location::Docked | Location::Station => 0.0,
                    Location::Belt => 1.0,
                    Location::InTransit {
                        destination,
                        arrives_at_ms,
                    } => {
                        let progress = 1.0
                            - arrives_at_ms
                                .saturating_sub(server_now)
                                .min(WARP_DURATION_MS) as f32
                                / WARP_DURATION_MS as f32;
                        if destination == Destination::Belt {
                            progress
                        } else {
                            1.0 - progress
                        }
                    }
                };
                let position = station.lerp(belt, t)
                    + Vec2::new((i % 3) as f32 * 18.0 - 18.0, -56.0 - (i / 3) as f32 * 29.0);
                let color = if pilot.name == self.pilot {
                    TEAL
                } else {
                    AMBER
                };
                if matches!(pilot.location, Location::InTransit { .. }) {
                    let dir = if matches!(
                        pilot.location,
                        Location::InTransit {
                            destination: Destination::Belt,
                            ..
                        }
                    ) {
                        -1.0
                    } else {
                        1.0
                    };
                    paint.line_segment(
                        [position, position + Vec2::new(dir * 28.0, 0.0)],
                        Stroke::new(2.0, color.gamma_multiply(0.4)),
                    );
                }
                paint.add(egui::Shape::convex_polygon(
                    vec![
                        position + Vec2::new(0.0, -8.0),
                        position + Vec2::new(-5.0, 6.0),
                        position + Vec2::new(5.0, 6.0),
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
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
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
                ui.colored_label(MUTED, "COMMUNITY UNIVERSE  /  PRIVATE PROTOTYPE");
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
                    ui.colored_label(
                        MUTED,
                        "Server record · independent verification via arkline-verifier",
                    );
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
            .exact_width(250.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(12.0);
                self.flight(ui);
                ui.separator();
                if self.connected {
                    if ui.button("Open second cockpit").clicked() {
                        self.second_cockpit();
                    }
                    if ui.button("Disconnect").clicked() {
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
        egui::TopBottomPanel::bottom("economy")
            .min_height(230.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.columns(2, |columns| {
                    self.market(&mut columns[0]);
                    columns[1].label(egui::RichText::new("ACTIVITY").color(TEAL).strong());
                    egui::ScrollArea::vertical()
                        .max_height(205.0)
                        .show(&mut columns[1], |ui| {
                            for (entry, error) in &self.feed {
                                ui.colored_label(if *error { AMBER } else { MUTED }, entry);
                                ui.separator();
                            }
                        });
                });
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.scene(ui);
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
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("Arkline native client\n\nUsage: arkline-client [--local | --remote-only] [--server HOST:PORT] [--data-dir PATH]\n\n--local starts a sibling arkline-server on a private loopback port.\nA macOS app bundle starts locally by default and saves in ~/Library/Application Support/Arkline/Preview.\n--server selects an existing server; --remote-only prevents bundle auto-hosting.\nCLI defaults: 127.0.0.1:7777, data/client.\n--credential-scope NAME selects a stable credential namespace (used by local preview cockpits).\nResume credentials are private local files. Development TCP requires a secure tunnel for remote use.");
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
    let _local_server = if local {
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
            if let Some(error) = startup_error {
                cockpit.notice = error;
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
