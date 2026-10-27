//! Switch the editor between its local world and a loopback session.
use arkline_client::{EmbeddedClient, EmbeddedRealm, EmbeddedSessionMode, EmbeddedStatus};
use arkline_protocol::SceneLayoutRevision;
use eframe::egui::{self, Color32};
use std::net::SocketAddr;

const LOOPBACK_DEFAULT: &str = "127.0.0.1:7779";
const WORLD_OPEN_RECOVERY: &str = "The selected world could not open. Your project world is preserved; choose Local, Project world, then Use local world to return.";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Choice {
    #[default]
    Local,
    Host,
    Join,
}

#[derive(Default)]
pub struct SessionPanel {
    pub open: bool,
    initialized: bool,
    choice: Choice,
    realm: EmbeddedRealm,
    pilot: String,
    bind_address: String,
    join_address: String,
    join_secret: String,
    reveal_join_secret: bool,
    reveal_editor_secret: bool,
    error: Option<String>,
    pending_physical: Option<EmbeddedSessionMode>,
    pending_ore_receiver: bool,
}

impl SessionPanel {
    pub fn show(&mut self, ctx: &egui::Context, client: &mut EmbeddedClient) {
        if !self.initialized {
            self.pilot = client.pilot().to_owned();
            self.realm = client.realm();
            self.choice = match client.session_mode() {
                EmbeddedSessionMode::Local => Choice::Local,
                EmbeddedSessionMode::Host { .. } => Choice::Host,
                EmbeddedSessionMode::Join { .. } => Choice::Join,
            };
            self.initialized = true;
        }
        if self.bind_address.is_empty() {
            self.bind_address = LOOPBACK_DEFAULT.to_owned();
        }
        if self.join_address.is_empty() {
            self.join_address = LOOPBACK_DEFAULT.to_owned();
        }

        // Use a local open flag so the window's mutable borrow does not
        // overlap edits to the rest of this panel state in the closure.
        let mut open = self.open;
        egui::Window::new("Multiplayer session")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(410.0)
            .max_height((ctx.available_rect().height() - 40.0).max(100.0))
            .vscroll(true)
            .show(ctx, |ui| self.contents(ui, client));
        self.open = open;
        if !open {
            self.pending_physical = None;
            self.pending_ore_receiver = false;
        }
    }

    fn contents(&mut self, ui: &mut egui::Ui, client: &mut EmbeddedClient) {
        if let Some(mode) = self.pending_physical.clone() {
            ui.separator();
            let ore_receiver = self.pending_ore_receiver;
            ui.heading(if ore_receiver {
                "Open ore receiver preview?"
            } else {
                "Open physical flagship preview?"
            });
            ui.strong(if matches!(mode, EmbeddedSessionMode::Host { .. }) {
                "Target: editor-hosted physical preview"
            } else {
                "Target: local editor physical preview"
            });
            ui.small("Only this editor changes sessions. A joined host and any standalone client keep their existing worlds.");
            ui.label(if ore_receiver { "Create or reopen a separate ore-receiver world: real mining-drone payloads, a physical quick drop-off, and full recall/stow as a distinct operation. First use creates it; later visits preserve progress." } else { "Create or reopen a separate persistent DockingV1 world with the physical Pioneer drone bay. First use creates it; later visits keep its drones, damage and progress." });
            if ore_receiver {
                ui.small("Storage: realms/ore-receiver-v1. Existing Physical flagship, Project, Orbital and Continuous docking worlds are not converted, reset or replaced. Cargo capability must be confirmed by authority.");
            } else {
                ui.small("Storage: realms/physical-flagship-v1. Project world, Orbital preview and Continuous docking preview are not converted, reset or replaced. This does not change a standalone client's world.");
            }
            ui.small("Opening the preview starts paused. Use Resume simulation deliberately to run it. Its actual capability is confirmed after the authority connects.");
            ui.horizontal_wrapped(|ui| {
                if ui.button("Back · keep current world").clicked() {
                    self.pending_physical = None;
                    self.pending_ore_receiver = false;
                }
                if ui
                    .button(if ore_receiver {
                        "Open separate ore receiver preview"
                    } else {
                        "Open separate physical preview"
                    })
                    .clicked()
                {
                    self.pending_physical = None;
                    self.pending_ore_receiver = false;
                    self.realm = if ore_receiver {
                        EmbeddedRealm::OreReceiverPreview
                    } else {
                        EmbeddedRealm::PhysicalFlagshipPreview
                    };
                    self.choice = if matches!(mode, EmbeddedSessionMode::Host { .. }) {
                        Choice::Host
                    } else {
                        Choice::Local
                    };
                    self.perform_switch(client, mode);
                }
            });
            if let Some(error) = &self.error {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
            return;
        }
        ui.heading("Editor embedded session");
        ui.small("This controls the game inside Arkline Editor, not a separately running standalone Arkline client.");
        ui.label(
            "The game world is shared authoritatively. Project and authoring files stay local.",
        );
        ui.small("Network access is loopback-only. Teammates can connect through an SSH tunnel.");
        ui.separator();

        let active = match client.session_mode() {
            EmbeddedSessionMode::Local => "Local world".to_owned(),
            EmbeddedSessionMode::Host { bind_addr } => format!("Host selected · {bind_addr}"),
            EmbeddedSessionMode::Join { address, .. } => format!("Join selected · {address}"),
        };
        ui.horizontal_wrapped(|ui| {
            ui.strong("Session mode");
            ui.label(active);
            ui.separator();
            ui.label(if client.can_control_clock() {
                "Clock control available"
            } else {
                "Host controls the clock"
            });
        });
        ui.label(format!(
            "Selected world: {}",
            if matches!(client.session_mode(), EmbeddedSessionMode::Join { .. }) {
                "Joined host world"
            } else {
                client.realm().label()
            }
        ));
        ui.label(format!(
            "Current layout: {}",
            match client.scene_layout_revision() {
                Some(SceneLayoutRevision::LegacyV1) =>
                    "LegacyV1 · original planet and linear berths",
                Some(SceneLayoutRevision::DockingV1) =>
                    "DockingV1 · continuous external Pioneer berths",
                Some(SceneLayoutRevision::OrbitalV1) => {
                    "OrbitalV1 · measured planet and circular berths"
                }
                None if matches!(client.status(), EmbeddedStatus::Error(_)) => {
                    "Unavailable · selected world is not connected"
                }
                None => "Awaiting authority snapshot",
            }
        ));
        show_session_error(ui, client.status());
        ui.strong(client.drone_behavior_label());
        if ui
            .add_enabled(
                !matches!(client.status(), EmbeddedStatus::Loading),
                egui::Button::new("Open physical flagship preview…"),
            )
            .clicked()
        {
            self.pending_ore_receiver = false;
            self.pending_physical = Some(EmbeddedSessionMode::Local);
        }

        if ui
            .add_enabled(
                !matches!(client.status(), EmbeddedStatus::Loading),
                egui::Button::new("Open ore receiver preview…"),
            )
            .clicked()
        {
            self.pending_ore_receiver = true;
            self.pending_physical = Some(EmbeddedSessionMode::Local);
        }

        if let Some(info) = client.session_info() {
            ui.label(&info.label);
            if let Some(address) = info.address {
                ui.monospace(address.to_string());
            }
            if info.is_host {
                if let Some(secret) = info.editor_secret.as_deref() {
                    ui.horizontal(|ui| {
                        ui.label("Editor secret");
                        if self.reveal_editor_secret {
                            ui.monospace(secret);
                        } else {
                            ui.label("••••••••••••");
                        }
                        if ui
                            .small_button(if self.reveal_editor_secret {
                                "Hide"
                            } else {
                                "Reveal"
                            })
                            .clicked()
                        {
                            self.reveal_editor_secret = !self.reveal_editor_secret;
                        }
                        if ui.small_button("Copy").clicked() {
                            ui.ctx().copy_text(secret.to_owned());
                        }
                    });
                    ui.small("The editor secret is shown only when you reveal it or copy it.");
                }
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.choice, Choice::Local, "Local");
            ui.selectable_value(&mut self.choice, Choice::Host, "Host");
            ui.selectable_value(&mut self.choice, Choice::Join, "Join");
        });
        self.realm_controls(ui);

        ui.horizontal(|ui| {
            let pilot_label = ui.label("Pilot");
            ui.add(
                egui::TextEdit::singleline(&mut self.pilot)
                    .desired_width(220.0)
                    .char_limit(24),
            )
            .labelled_by(pilot_label.id);
        });

        match self.choice {
            Choice::Local => {
                ui.label("Opens the selected preserved local world.");
                if ui.button("Use local world").clicked() {
                    self.switch(client, EmbeddedSessionMode::Local);
                }
            }
            Choice::Host => {
                ui.horizontal(|ui| {
                    ui.label("Loopback bind address");
                    ui.add(egui::TextEdit::singleline(&mut self.bind_address).desired_width(180.0));
                });
                ui.small("Hosting keeps this editor open and shares the selected preserved world.");
                if ui.button("Host shared world").clicked() {
                    match parse_loopback(&self.bind_address) {
                        Ok(bind_addr) => {
                            self.switch(client, EmbeddedSessionMode::Host { bind_addr });
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            }
            Choice::Join => {
                ui.horizontal(|ui| {
                    ui.label("Loopback address");
                    ui.add(egui::TextEdit::singleline(&mut self.join_address).desired_width(180.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Editor secret");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.join_secret)
                            .password(!self.reveal_join_secret)
                            .desired_width(180.0),
                    );
                    if ui
                        .small_button(if self.reveal_join_secret {
                            "Hide"
                        } else {
                            "Show"
                        })
                        .clicked()
                    {
                        self.reveal_join_secret = !self.reveal_join_secret;
                    }
                });
                ui.small("Joining changes only this editor's session; it never replaces your local world.");
                if ui.button("Join shared world").clicked() {
                    match parse_loopback(&self.join_address) {
                        Ok(address) if !self.join_secret.is_empty() => {
                            let secret = std::mem::take(&mut self.join_secret);
                            self.reveal_join_secret = false;
                            self.switch(client, EmbeddedSessionMode::Join { address, secret });
                        }
                        Ok(_) => {
                            self.error =
                                Some("Enter the editor secret supplied by the host.".into())
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            }
        }

        if let Some(error) = &self.error {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
    }

    fn realm_controls(&mut self, ui: &mut egui::Ui) {
        match self.choice {
            Choice::Local | Choice::Host => {
                ui.horizontal_wrapped(|ui| {
                    ui.strong("World");
                    for realm in [
                        EmbeddedRealm::Project,
                        EmbeddedRealm::OrbitalPreview,
                        EmbeddedRealm::DockingPreview,
                        EmbeddedRealm::PhysicalFlagshipPreview,
                        EmbeddedRealm::OreReceiverPreview,
                    ] {
                        ui.selectable_value(&mut self.realm, realm, realm.label());
                    }
                });
                ui.small("Existing worlds keep their original planet and berths. Previews open separate persistent worlds; none converts the project world. Continuous docking currently supports Pioneer exterior berths only.");
            }
            Choice::Join => {
                ui.small("The remote host chooses the world layout. Your preserved local worlds stay on this computer.");
            }
        }
    }

    fn switch(&mut self, client: &mut EmbeddedClient, mode: EmbeddedSessionMode) {
        if matches!(
            self.realm,
            EmbeddedRealm::PhysicalFlagshipPreview | EmbeddedRealm::OreReceiverPreview
        ) && !matches!(mode, EmbeddedSessionMode::Join { .. })
        {
            self.pending_ore_receiver = self.realm == EmbeddedRealm::OreReceiverPreview;
            self.pending_physical = Some(mode);
        } else {
            self.perform_switch(client, mode);
        }
    }
    fn perform_switch(&mut self, client: &mut EmbeddedClient, mode: EmbeddedSessionMode) {
        let pilot = self.pilot.trim().to_owned();
        let result = if matches!(mode, EmbeddedSessionMode::Join { .. }) {
            client.switch_session(mode, pilot)
        } else {
            client.switch_session_in_realm(mode, pilot, self.realm)
        };
        self.error = match result {
            Ok(()) => {
                self.reveal_editor_secret = false;
                None
            }
            Err(error) => Some(error),
        };
    }
}

fn show_session_error(ui: &mut egui::Ui, status: &EmbeddedStatus) {
    if let EmbeddedStatus::Error(error) = status {
        ui.colored_label(Color32::LIGHT_RED, error);
        ui.colored_label(Color32::LIGHT_RED, WORLD_OPEN_RECOVERY);
    }
}

fn parse_loopback(text: &str) -> Result<SocketAddr, String> {
    let address: SocketAddr = text
        .trim()
        .parse()
        .map_err(|_| "Enter a valid IP address and port, such as 127.0.0.1:7779.".to_owned())?;
    if !address.ip().is_loopback() {
        return Err(
            "Only loopback addresses are supported. Use an SSH tunnel for teammates.".into(),
        );
    }
    Ok(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_and_host_offer_realms_but_join_uses_the_remote_layout() {
        for choice in [Choice::Local, Choice::Host, Choice::Join] {
            let ctx = egui::Context::default();
            let mut panel = SessionPanel {
                choice,
                ..Default::default()
            };
            let output = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| panel.realm_controls(ui));
            });
            let labels: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                    _ => None,
                })
                .collect();
            for label in [
                "Project world",
                "Orbital station preview",
                "Continuous docking preview",
                "Physical flagship preview",
            ] {
                assert_eq!(labels.contains(&label), choice != Choice::Join);
            }
            if choice == Choice::Join {
                assert!(labels
                    .iter()
                    .any(|label| label.contains("remote host chooses")));
            }
        }
    }

    #[test]
    fn asynchronous_open_error_shows_worker_reason_and_project_return_path() {
        let ctx = egui::Context::default();
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                show_session_error(
                    ui,
                    &EmbeddedStatus::Error("Cannot open world database".into()),
                );
            });
        });
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(labels.contains(&"Cannot open world database"));
        assert!(labels.contains(&WORLD_OPEN_RECOVERY));
    }
    #[test]
    fn physical_preview_real_clicks_preserve_old_worlds_and_require_confirmation() {
        preview_real_clicks_preserve_old_worlds(false);
    }
    #[test]
    fn ore_receiver_preview_real_clicks_preserve_old_worlds_and_require_confirmation() {
        preview_real_clicks_preserve_old_worlds(true);
    }
    fn preview_real_clicks_preserve_old_worlds(ore_receiver: bool) {
        let open_label = if ore_receiver {
            "Open ore receiver preview…"
        } else {
            "Open physical flagship preview…"
        };
        let confirm_label = if ore_receiver {
            "Open separate ore receiver preview"
        } else {
            "Open separate physical preview"
        };
        let realm = if ore_receiver {
            EmbeddedRealm::OreReceiverPreview
        } else {
            EmbeddedRealm::PhysicalFlagshipPreview
        };
        use sha2::{Digest, Sha256};
        use std::{
            fs,
            time::{Duration, Instant, SystemTime, UNIX_EPOCH},
        };
        fn wait(client: &mut EmbeddedClient) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                client.poll();
                if client.ready() {
                    return;
                }
                if let EmbeddedStatus::Error(error) = client.status() {
                    panic!("{error}");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("authority did not become ready: {:?}", client.status());
        }
        fn frame(
            ctx: &egui::Context,
            panel: &mut SessionPanel,
            client: &mut EmbeddedClient,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900., 650.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| panel.show(ctx, client),
            )
        }
        fn click(
            ctx: &egui::Context,
            panel: &mut SessionPanel,
            client: &mut EmbeddedClient,
            label: &str,
        ) {
            frame(ctx, panel, client, vec![]);
            let rect = ctx
                .memory(|m| m.area_rect(egui::Id::new("Multiplayer session")))
                .unwrap();
            let scroll_at = rect.center();
            // Use the real scrollable dialog. Begin at its top, then scroll
            // down only until the whole target label is actually visible.
            frame(
                ctx,
                panel,
                client,
                vec![
                    egui::Event::PointerMoved(scroll_at),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0., 2000.),
                        modifiers: Default::default(),
                    },
                ],
            );
            for _ in 0..24 {
                let mut output = frame(ctx, panel, client, vec![]);
                for _ in 0..24 {
                    output = frame(ctx, panel, client, vec![]);
                }
                if let Some(pos) = output.shapes.iter().find_map(|s| match &s.shape {
                    egui::Shape::Text(t)
                        if t.galley.job.text == label
                            && s.clip_rect.contains_rect(t.visual_bounding_rect()) =>
                    {
                        Some(t.visual_bounding_rect().center())
                    }
                    _ => None,
                }) {
                    pointer(ctx, panel, client, pos);
                    return;
                }
                frame(
                    ctx,
                    panel,
                    client,
                    vec![
                        egui::Event::PointerMoved(scroll_at),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0., -80.),
                            modifiers: Default::default(),
                        },
                    ],
                );
            }
            panic!("missing fully visible button {label} in real scrollable dialog");
        }

        fn pointer(
            ctx: &egui::Context,
            panel: &mut SessionPanel,
            client: &mut EmbeddedClient,
            pos: egui::Pos2,
        ) {
            frame(ctx, panel, client, vec![egui::Event::PointerMoved(pos)]);
            for pressed in [true, false] {
                frame(
                    ctx,
                    panel,
                    client,
                    vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    }],
                );
            }
        }
        let dir = std::env::temp_dir().join(format!(
            "arkline-physical-profile-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut client = EmbeddedClient::new(&cc, dir.clone(), "PreviewPilot".into());
        wait(&mut client);
        let existing_physical_hash = if ore_receiver {
            client
                .switch_session_in_realm(
                    EmbeddedSessionMode::Local,
                    "PreviewPilot".into(),
                    EmbeddedRealm::PhysicalFlagshipPreview,
                )
                .unwrap();
            wait(&mut client);
            assert!(client
                .snapshot()
                .unwrap()
                .pilots
                .iter()
                .all(|pilot| pilot.drone_cargo.is_none()));
            assert_eq!(client.assets().unwrap().drone_bay.unwrap().stock.len(), 15);
            client
                .switch_session_in_realm(
                    EmbeddedSessionMode::Local,
                    "PreviewPilot".into(),
                    EmbeddedRealm::Project,
                )
                .unwrap();
            wait(&mut client);
            Some(
                Sha256::digest(
                    fs::read(dir.join("realms/physical-flagship-v1/world.sqlite")).unwrap(),
                )
                .to_vec(),
            )
        } else {
            None
        };
        let original = client.snapshot().unwrap();
        let original_assets = serde_json::to_value(client.assets()).unwrap();
        assert_eq!(
            client.scene_layout_revision(),
            Some(SceneLayoutRevision::OrbitalV1)
        );
        assert!(client
            .drone_behavior_label()
            .starts_with("Legacy simplified"));
        // Existing preview files are sentinels, not candidates for conversion.
        for path in ["realms/orbital-preview-v1", "realms/docking-preview-v1"] {
            fs::create_dir_all(dir.join(path)).unwrap();
            fs::write(
                dir.join(path).join("world.sqlite"),
                b"preserved older preview",
            )
            .unwrap();
        }
        let digest = || Sha256::digest(fs::read(dir.join("world.sqlite")).unwrap()).to_vec();
        let before = digest();
        let preview = dir.join(if ore_receiver {
            "realms/ore-receiver-v1"
        } else {
            "realms/physical-flagship-v1"
        });
        let mut panel = SessionPanel {
            open: true,
            ..Default::default()
        };
        click(&ctx, &mut panel, &mut client, open_label);
        assert!(panel.pending_physical.is_some());
        assert!(!preview.exists());
        click(&ctx, &mut panel, &mut client, "Back · keep current world");
        assert!(panel.pending_physical.is_none());
        assert_eq!(client.realm(), EmbeddedRealm::Project);
        assert_eq!(
            serde_json::to_value(client.snapshot()).unwrap(),
            serde_json::to_value(Some(&original)).unwrap()
        );
        assert_eq!(digest(), before);
        click(&ctx, &mut panel, &mut client, open_label);
        for _ in 0..8 {
            frame(&ctx, &mut panel, &mut client, vec![]);
        }
        let rect = ctx
            .memory(|m| m.area_rect(egui::Id::new("Multiplayer session")))
            .unwrap();
        let output = frame(&ctx, &mut panel, &mut client, vec![]);
        let close = output
            .shapes
            .iter()
            .find_map(|s| {
                if let egui::Shape::LineSegment { points, .. } = &s.shape {
                    let r = egui::Rect::from_two_pos(points[0], points[1]);
                    if r.width() > 4.
                        && r.height() > 4.
                        && r.left() > rect.right() - 55.
                        && r.bottom() < rect.top() + 80.
                        && s.clip_rect.contains_rect(r)
                    {
                        return Some(r.center());
                    }
                }
                None
            })
            .expect("actual visible title Close X");
        pointer(&ctx, &mut panel, &mut client, close);
        assert!(!panel.open && panel.pending_physical.is_none());
        assert!(!preview.exists());
        assert_eq!(digest(), before);
        panel.open = true;
        click(&ctx, &mut panel, &mut client, open_label);
        click(&ctx, &mut panel, &mut client, confirm_label);
        wait(&mut client);
        assert_eq!(client.realm(), realm);
        assert_eq!(
            client.scene_layout_revision(),
            Some(SceneLayoutRevision::DockingV1)
        );
        assert!(client.paused());
        assert!(client
            .drone_behavior_label()
            .starts_with("Physical drone bay ·"));
        let private_bay = client.assets().unwrap().drone_bay.unwrap();
        assert_eq!(
            private_bay.schema_version,
            arkline_protocol::DRONE_BAY_SCHEMA_VERSION
        );
        assert!(
            private_bay.batches.is_some(),
            "fresh profile must advertise durable batch commands"
        );
        let snapshot = client.snapshot().unwrap();
        let public_bay = snapshot
            .pilots
            .iter()
            .find_map(|p| p.drone_bay.as_ref())
            .unwrap();
        assert_eq!(
            public_bay.schema_version,
            arkline_protocol::DRONE_BAY_SCHEMA_VERSION
        );
        assert_eq!(
            public_bay.batch_capability,
            Some(arkline_protocol::DRONE_BAY_BATCH_VERSION)
        );
        if ore_receiver {
            let pilot = snapshot
                .pilots
                .iter()
                .find(|pilot| pilot.name == client.pilot())
                .unwrap();
            let cargo = pilot
                .drone_cargo
                .as_ref()
                .expect("explicit ore profile advertises actual cargo capability");
            cargo.validate_projection(pilot).unwrap();
            assert_eq!(cargo.holds.len(), 5);
            assert!(cargo
                .holds
                .iter()
                .all(|hold| hold.ore_units == 0 && hold.capacity_units == 100));
            assert!(client.assets().unwrap().drone_cargo.is_some());
            assert!(client.drone_behavior_label().contains("real ore payloads"));
        } else {
            assert!(
                snapshot
                    .pilots
                    .iter()
                    .all(|pilot| pilot.drone_cargo.is_none()),
                "ordinary physical profile must remain cargo-free"
            );
        }
        let stock = private_bay.stock;
        assert_eq!(stock.len(), 15);
        // Closing the original worker checkpoints SQLite's WAL into the main
        // file. Hash only this quiescent saved database, not its pre-checkpoint
        // pages; original world contents are compared exactly on return below.
        let checkpointed_project = digest();
        for path in ["realms/orbital-preview-v1", "realms/docking-preview-v1"] {
            assert_eq!(
                fs::read(dir.join(path).join("world.sqlite")).unwrap(),
                b"preserved older preview"
            );
        }
        click(&ctx, &mut panel, &mut client, "Project world");
        click(&ctx, &mut panel, &mut client, "Use local world");
        wait(&mut client);
        assert_eq!(client.realm(), EmbeddedRealm::Project);
        assert_eq!(
            serde_json::to_value(client.snapshot()).unwrap(),
            serde_json::to_value(Some(&original)).unwrap()
        );
        assert_eq!(digest(), checkpointed_project);
        assert_eq!(
            serde_json::to_value(client.assets()).unwrap(),
            original_assets
        );
        click(&ctx, &mut panel, &mut client, open_label);
        click(&ctx, &mut panel, &mut client, confirm_label);
        wait(&mut client);
        assert_eq!(client.assets().unwrap().drone_bay.unwrap().stock, stock);
        assert!(client.paused());
        assert_eq!(digest(), checkpointed_project);
        if let Some(before) = existing_physical_hash {
            assert_eq!(Sha256::digest(fs::read(dir.join("realms/physical-flagship-v1/world.sqlite")).unwrap()).to_vec(), before, "existing real batch-capable physical world remains byte-identical after ore preview visits");
        }
        client.on_exit();
        drop(client);
        fs::remove_dir_all(dir).unwrap();
    }
}
