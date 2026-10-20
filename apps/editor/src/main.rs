//! One-window local development workspace using the real game client and authority.
mod asset_document;
mod cockpit;
mod cockpit_document;
mod community;
mod component_view;
#[cfg(test)]
mod drone_gallery_tests;
mod engineering;
mod engineering_document;
mod equipment;
mod event_graph;
mod exterior;
mod git_workflow;
mod github;
mod github_auth;
mod import_document;
mod imports;
mod inspector;
mod material_graph;
mod materials;
mod model_browser;
mod physics_lab;
mod project_workspace;
mod provenance;
mod render_debug;
mod rooms;
mod service_paths;
mod session_panel;
#[cfg(test)]
mod shift_embedded_tests;
mod surface_materials;
use arkline_client::{EmbeddedAssetGallery, EmbeddedClient, EmbeddedStatus};
use eframe::egui::{self, Color32, RichText};
use std::{collections::VecDeque, path::PathBuf, time::Duration};
const BUILD: &str = include_str!("../../../BUILD_VERSION");
const GREEN: Color32 = Color32::from_rgb(62, 233, 145);
const MUTED: Color32 = Color32::from_rgb(140, 165, 158);
struct Editor {
    cockpit: cockpit::CockpitPanel,
    imports: imports::Imports,
    client: EmbeddedClient,
    project: PathBuf,
    selected_pilot: String,
    selected_equipment: String,
    console: VecDeque<String>,
    last_status: String,
    bottom_tab: usize,
    drawer_open: bool,
    show_world: bool,
    flight_focus: bool,
    room_plan: bool,
    show_inspector: bool,
    inspector_tab: usize,
    materials: materials::Materials,
    surface_materials: surface_materials::SurfaceMaterials,
    surface_mode: bool,
    surface_hangar_light: bool,
    engineering: engineering::Engineering,
    service_paths: service_paths::ServicePaths,
    rooms: rooms::Rooms,
    exterior: exterior::Exterior,
    session_panel: session_panel::SessionPanel,
    community: community::Community,
    git_workspace: git_workflow::GitWorkspace,
    project_workspace: project_workspace::ProjectWorkspace,
    files_close_blocked: bool,
    event_graph: event_graph::EventGraphPanel,
    events_open: bool,
    workspace: usize,
    wireframe: bool,
    asset_gallery: Option<EmbeddedAssetGallery>,
    model_browser: Option<model_browser::ModelBrowser>,
    graphics: Option<std::sync::Arc<eframe::glow::Context>>,
    render_debug: render_debug::RenderDebug,
    physics_lab: physics_lab::PhysicsLab,
    provenance: provenance::Provenance,
    scene_message: String,
    scene_generation: u64,
}
impl Editor {
    fn console_visible(&self) -> bool {
        self.drawer_open && !(self.events_open && self.workspace == 0)
    }

    fn open_inspector(&mut self, tab: usize) {
        self.flight_focus = false;
        self.workspace = 0;
        self.inspector_tab = tab;
        self.show_inspector = true;
    }

    fn play_game(&mut self) {
        self.workspace = 0;
        self.inspector_tab = 0;
        self.client.set_free_camera(false);
        if self.client.can_control_clock() || self.client.status() == &EmbeddedStatus::Stopped {
            self.client.play();
        }
    }
    fn new(cc: &eframe::CreationContext<'_>, project: PathBuf, pilot: String) -> Self {
        static MATERIAL_GENERATION: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(1);
        let material_generation =
            MATERIAL_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let client = EmbeddedClient::new(cc, project.join(".arkline-studio"), pilot.clone());
        Self {
            cockpit: cockpit::CockpitPanel::open(&project),
            imports: imports::Imports::open(&project),
            materials: materials::Materials::open(&project),
            surface_materials: surface_materials::SurfaceMaterials::open(
                &project,
                material_generation,
            ),
            surface_mode: false,
            surface_hangar_light: true,
            engineering: engineering::Engineering::open(&project),
            service_paths: service_paths::ServicePaths::open(&project),
            rooms: rooms::Rooms::open(&project),
            exterior: exterior::Exterior::open(&project),
            session_panel: session_panel::SessionPanel::default(),
            community: community::Community::open(&project),
            git_workspace: git_workflow::GitWorkspace::open(&project),
            project_workspace: project_workspace::ProjectWorkspace::open(&project),
            files_close_blocked: false,
            event_graph: event_graph::EventGraphPanel::open(&project),
            events_open: false,
            workspace: 0,
            wireframe: false,
            asset_gallery: None,
            model_browser: None,
            graphics: cc.gl.clone(),
            render_debug: render_debug::RenderDebug::default(),
            physics_lab: physics_lab::PhysicsLab::default(),
            provenance: provenance::Provenance::default(),
            client,
            project,
            selected_pilot: pilot,
            selected_equipment: String::new(),
            console: VecDeque::from([
                "Opening isolated editor world. No public relay connection.".into(),
                "Play runs the simulation. Pause freezes it. Step advances 50 ms.".into(),
            ]),
            last_status: String::new(),
            bottom_tab: 0,
            drawer_open: false,
            show_world: true,
            flight_focus: true,
            room_plan: true,
            show_inspector: true,
            inspector_tab: 0,
            scene_message: String::new(),
            scene_generation: 0,
        }
    }

    fn scene_controls(&mut self, ctx: &egui::Context) {
        use arkline_protocol::EditorSceneState;
        let confirmed = self.client.scene_state();
        let ready = self.client.ready() && !self.client.scene_state_pending();
        let pioneer = self.client.snapshot().is_some_and(|world| {
            world.pilots.iter().any(|pilot| {
                pilot.name == self.client.pilot()
                    && pilot.ship_class == arkline_protocol::ShipClass::Pioneer
            })
        });
        egui::TopBottomPanel::top("editor_scene_controls").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong(format!("SCENE · {}", self.client.pilot()));
                for state in [EditorSceneState::Cockpit, EditorSceneState::StationHangar,
                    EditorSceneState::InteriorDeck1, EditorSceneState::InteriorDeck2] {
                    let supported = !matches!(state, EditorSceneState::InteriorDeck1 | EditorSceneState::InteriorDeck2) || pioneer;
                    if ui.add_enabled(ready && confirmed.is_some() && supported,
                        egui::Button::new(state.label()).selected(confirmed == Some(state)))
                        .on_hover_text("Editor sandbox placement for your pilot. Works while paused; preserves simulation time and ship systems.")
                        .clicked() {
                        self.inspector_tab = 0;
                        self.client.set_free_camera(false);
                        self.scene_message = match self.client.set_scene_state(state) {
                            Ok(_) => "Applying scene…".into(),
                            Err(error) => error,
                        };
                    }
                }
                ui.separator();
                let free = self.client.free_camera_active();
                if ui.selectable_label(free, "Free camera").clicked() {
                    self.inspector_tab = 0;
                    self.client.set_free_camera(!free);
                }
                if ui.selectable_label(!free, "Player camera").clicked() {
                    self.inspector_tab = 0;
                    self.client.set_free_camera(false);
                }
            });
            if let Some(ack) = self.client.scene_state_ack() {
                self.scene_message = match &ack.result {
                    Ok(()) => "Scene confirmed · simulation time and systems preserved".into(),
                    Err(error) => format!("Scene change was not confirmed: {error}"),
                };
            }
            let text = if confirmed.is_none() {
                "Scene placement requires a docked ship. Dock through the game controls first."
            } else if self.scene_message.is_empty() {
                "Editor sandbox · choose where to inspect your docked ship. Gameplay uses doors, walking and boarding."
            } else { &self.scene_message };
            ui.small(text);
        });
    }

    fn session_controls(&mut self, ctx: &egui::Context) {
        let prior_pilot = self.client.pilot().to_owned();
        self.session_panel.show(ctx, &mut self.client);
        if self.client.pilot() != prior_pilot {
            self.selected_pilot = self.client.pilot().to_owned();
        }
        if self.scene_generation != self.client.session_generation() {
            self.scene_generation = self.client.session_generation();
            self.scene_message.clear();
        }
    }
}
impl eframe::App for Editor {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.client.poll();
        // Parent editor focus and document ownership release flight input without
        // removing the cockpit's physical computer or its own text focus.
        self.client.set_editor_input_blocked(
            !ctx.input(|input| input.focused)
                || ctx.wants_keyboard_input()
                || self.workspace != 0
                || self.inspector_tab == 2
                || (self.inspector_tab == 6 && self.room_plan),
        );
        self.cockpit.poll(&cockpit::Live::from_client(&self.client));
        self.rooms.poll_live(&mut self.client);
        self.exterior.poll_live(&mut self.client);
        self.community.poll();
        self.imports.poll();
        self.project_workspace.poll();
        let branch_was_pending = self.git_workspace.branch_change_pending();
        self.git_workspace.poll();
        if branch_was_pending && !self.git_workspace.branch_change_pending() {
            self.project_workspace.notify_external_change();
        }
        self.git_workspace.set_source_guard(
            self.project_workspace.has_dirty_buffers()
                || self.project_workspace.has_pending_file_job(),
            self.project_workspace.has_pending_save(),
        );
        self.project_workspace.set_git_activity(
            self.git_workspace.branch_change_pending(),
            self.git_workspace.local_mutation_pending(),
        );
        let source_close_guard =
            self.project_workspace.has_dirty_buffers() || self.project_workspace.has_pending_save();
        if !source_close_guard {
            self.files_close_blocked = false;
        }
        if ctx.input(|input| input.viewport().close_requested()) {
            if source_close_guard {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.workspace = 7;
                self.files_close_blocked = true;
            } else if self.git_workspace.has_pending_job() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.workspace = 6;
                self.git_workspace.keep_open_for_job();
            }
        }
        let surface_revision = self.surface_materials.revision();
        self.client.set_spatial_material(surface_revision.clone());
        if let Some(gallery) = &mut self.asset_gallery {
            gallery.set_spatial_material(surface_revision.clone());
            self.surface_materials
                .set_program_status(gallery.material_program_status());
        }
        let clock_control = self.client.can_control_clock();
        let status = match self.client.status() {
            EmbeddedStatus::Loading => "LOADING".into(),
            EmbeddedStatus::Paused => if clock_control {
                "PAUSED"
            } else {
                "HOST PAUSED"
            }
            .into(),
            EmbeddedStatus::Running => "PLAYING".into(),
            EmbeddedStatus::Stopped => if clock_control {
                "STOPPED"
            } else {
                "LOCAL CONTROLS SUSPENDED"
            }
            .into(),
            EmbeddedStatus::Error(error) => format!("ERROR · {error}"),
        };
        if status != self.last_status {
            self.console.push_back(status.clone());
            while self.console.len() > 200 {
                self.console.pop_front();
            }
            self.last_status = status.clone();
        }
        let snapshot = self.client.snapshot();
        let assets = self.client.assets();
        let active = matches!(self.client.status(), EmbeddedStatus::Running);
        let paused = matches!(self.client.status(), EmbeddedStatus::Paused);
        let ready = matches!(
            self.client.status(),
            EmbeddedStatus::Paused | EmbeddedStatus::Running | EmbeddedStatus::Stopped
        );
        if !ctx.wants_keyboard_input()
            && !self.client.computer_wants_keyboard_input()
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F5))
            && ready
        {
            if active
                && self.workspace == 0
                && self.inspector_tab != 2
                && !(self.inspector_tab == 6 && self.room_plan)
                && clock_control
            {
                self.client.pause();
            } else {
                self.play_game();
            }
        }
        egui::TopBottomPanel::top("editor_toolbar")
            .exact_height(if self.flight_focus { 44.0 } else { 58.0 })
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("A R K L I N E")
                            .color(GREEN)
                            .strong()
                            .size(19.0),
                    );
                    ui.label(
                        RichText::new(if self.flight_focus {
                            "FLIGHT"
                        } else {
                            "EDITOR"
                        })
                        .color(MUTED)
                        .size(11.0),
                    );
                    ui.separator();
                    if ui
                        .add_enabled(
                            ready
                                && (if clock_control {
                                    !active
                                } else {
                                    self.client.status() == &EmbeddedStatus::Stopped
                                } || self.workspace != 0
                                    || self.inspector_tab == 2
                                    || (self.inspector_tab == 6 && self.room_plan)),
                            egui::Button::new(if clock_control {
                                "▶  Play"
                            } else {
                                "Game / resume"
                            })
                            .min_size(egui::vec2(82., 32.)),
                        )
                        .clicked()
                    {
                        self.play_game();
                    }
                    if ui
                        .add_enabled(
                            active && clock_control,
                            egui::Button::new("Ⅱ  Pause").min_size(egui::vec2(82., 32.)),
                        )
                        .clicked()
                        && clock_control
                    {
                        self.client.pause();
                    }
                    if ui
                        .add_enabled(
                            paused && clock_control,
                            egui::Button::new("Step 50 ms").min_size(egui::vec2(95., 32.)),
                        )
                        .clicked()
                    {
                        self.client.step();
                    }
                    if ui
                        .add_enabled(
                            active || paused,
                            egui::Button::new(if clock_control {
                                "■  Stop"
                            } else {
                                "Suspend controls"
                            })
                            .min_size(egui::vec2(82., 32.)),
                        )
                        .clicked()
                    {
                        self.client.stop();
                    }
                    ui.separator();
                    ui.label(RichText::new(&status).color(if active {
                        GREEN
                    } else {
                        Color32::from_rgb(239, 191, 102)
                    }));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(BUILD.trim()).color(MUTED));
                        ui.label(RichText::new("LOCAL DEVELOPMENT").color(GREEN).small());
                    });
                });
            });
        egui::TopBottomPanel::top("workspace_tabs").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .selectable_label(self.flight_focus && self.workspace == 0, "Flight")
                    .clicked()
                {
                    self.workspace = 0;
                    self.inspector_tab = 0;
                    self.flight_focus = true;
                    self.client.set_free_camera(false);
                }
                if ui.selectable_label(!self.flight_focus, "Editor").clicked() {
                    self.workspace = 0;
                    self.flight_focus = false;
                }
                ui.separator();
                ui.selectable_value(&mut self.workspace, 8, "Models");
                if !self.flight_focus {
                    if ui
                        .selectable_label(self.workspace == 0 && self.inspector_tab == 6, "Rooms")
                        .clicked()
                    {
                        self.open_inspector(6);
                    }
                    ui.selectable_value(&mut self.workspace, 3, "Materials");
                    ui.selectable_value(&mut self.workspace, 6, "Git");
                    ui.selectable_value(&mut self.workspace, 7, "Files");
                    ui.menu_button("Systems", |ui| {
                        for (tab, label) in [
                            (2, "Engineering candidate"),
                            (4, "Equipment"),
                            (7, "Exterior"),
                            (5, "Service paths"),
                        ] {
                            if ui.button(label).clicked() {
                                self.open_inspector(tab);
                                ui.close_menu();
                            }
                        }
                        ui.selectable_value(&mut self.workspace, 4, "Cockpit panels");
                        if ui
                            .toggle_value(&mut self.events_open, "Event flow")
                            .clicked()
                            && self.events_open
                        {
                            self.workspace = 0;
                        }
                    });
                    if ui
                        .selectable_label(
                            self.workspace == 0 && self.inspector_tab == 0,
                            "Play view",
                        )
                        .clicked()
                    {
                        self.open_inspector(0);
                    }
                }
                if ui
                    .button("Mining")
                    .on_hover_text("Open the server-controlled repeat mining routine")
                    .clicked()
                {
                    self.workspace = 0;
                    self.inspector_tab = 0;
                    self.flight_focus = true;
                    self.client.open_mining_panel();
                }
                if ui.button("Sound").clicked() {
                    self.workspace = 0;
                    self.flight_focus = true;
                    self.client.open_sound_panel();
                }
                if !self.flight_focus {
                    ui.menu_button("Tools", |ui| {
                        ui.selectable_value(&mut self.workspace, 2, "Physics lab");
                        if ui.button("Render debug").clicked() {
                            self.open_inspector(3);
                            ui.close_menu();
                        }
                        if ui
                            .selectable_value(&mut self.workspace, 1, "Community")
                            .clicked()
                        {
                            self.community.ensure_loaded();
                        }
                    });
                }
                if ui.button("Session").clicked() {
                    self.session_panel.open = true;
                }
                ui.label(
                    RichText::new(match self.client.session_mode() {
                        arkline_client::EmbeddedSessionMode::Local => "Local authority",
                        arkline_client::EmbeddedSessionMode::Host { .. } => "Shared host",
                        arkline_client::EmbeddedSessionMode::Join { .. } => "Shared universe",
                    })
                    .color(MUTED),
                );
            });
        });
        if self.workspace != 0 || self.inspector_tab != 0 {
            self.flight_focus = false;
        }
        if self.workspace == 0 && !self.flight_focus {
            self.scene_controls(ctx);
        }
        if self.workspace == 3 {
            egui::TopBottomPanel::top("editor_material_mode").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.surface_mode, false, "Fleet finish");
                    ui.selectable_value(&mut self.surface_mode, true, "Hull surface");
                });
                if self.surface_mode {
                    self.surface_materials.toolbar(ui);
                }
            });
        }
        egui::TopBottomPanel::bottom("editor_drawer_bar")
            .exact_height(28.)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in [(0, "Console"), (1, "Project")] {
                        let open = self.console_visible() && self.bottom_tab == tab;
                        if ui.selectable_label(open, label).clicked() {
                            self.drawer_open = !open;
                            self.bottom_tab = tab;
                            self.events_open = false;
                        }
                    }
                    if let Some(world) = &snapshot {
                        ui.small(format!("Simulation: {} ms", world.server_time_ms));
                    }
                    ui.separator();
                    ui.add_enabled_ui(self.workspace == 0, |ui| {
                        if self.flight_focus {
                            if ui.button("World / inspector").clicked() {
                                self.flight_focus = false;
                            }
                            return;
                        }
                        ui.toggle_value(
                            &mut self.show_world,
                            if self.workspace == 0 && self.inspector_tab == 6 {
                                "Rooms list"
                            } else {
                                "World"
                            },
                        );
                        ui.toggle_value(&mut self.show_inspector, "Inspector");
                    });
                });
            });
        if self.events_open && self.workspace == 0 {
            egui::TopBottomPanel::bottom("editor_event_flow")
                .resizable(true)
                .default_height(350.)
                .height_range(250. ..=600.)
                .show(ctx, |ui| {
                    self.event_graph.show(ui, &self.project, &mut self.client);
                });
        } else if self.console_visible() {
            egui::TopBottomPanel::bottom("editor_console").resizable(true).default_height(120.).height_range(65. ..=280.).show(ctx, |ui| {
            egui::ScrollArea::vertical().id_salt("editor_console_scroll").show(ui, |ui| {
                if self.bottom_tab==0 {
                    for line in &self.console { ui.monospace(line); }
                } else {
                    ui.label("Project folder"); ui.monospace(self.project.display().to_string());
                    ui.label("World and pilot credentials live in .arkline-studio. Stop preserves the world; it does not reset it.");
                    ui.colored_label(MUTED,"Pioneer materials and candidate engineering data are saved under Assets. Community connects to GitHub; simulation remains local.");
                }
            });
        });
        }
        if self.workspace == 6 {
            self.client.poll_hidden();
            self.git_workspace.show(ctx, &self.project);
            self.session_controls(ctx);
            ctx.request_repaint_after(Duration::from_millis(50));
            return;
        }
        if self.workspace == 7 {
            self.client.poll_hidden();
            if self.files_close_blocked {
                egui::TopBottomPanel::top("source_close_guard").show(ctx, |ui| {
                    ui.colored_label(egui::Color32::YELLOW, "Source tabs have unsaved changes or a Save is pending. Save them before closing the editor.");
                });
            }
            if let Some(project_workspace::PreviewRequest::Model { label, bytes }) =
                self.project_workspace.show(ctx)
            {
                let queued = self
                    .model_browser
                    .get_or_insert_with(|| model_browser::ModelBrowser::open(&self.project))
                    .open_snapshot(label, bytes);
                self.project_workspace.preview_handoff_result(queued);
                if queued {
                    self.workspace = 8;
                }
            }
            self.session_controls(ctx);
            ctx.request_repaint_after(Duration::from_millis(50));
            return;
        }
        if self.workspace == 4 {
            self.client.poll_hidden();
            if let Some(request) = self
                .cockpit
                .show(ctx, &cockpit::Live::from_client(&self.client))
            {
                self.client.send_command(request);
            }
            self.session_controls(ctx);
            ctx.request_repaint_after(Duration::from_millis(33));
            return;
        }
        if self.workspace == 8 {
            self.client.poll_hidden();
            let action = self
                .model_browser
                .get_or_insert_with(|| model_browser::ModelBrowser::open(&self.project))
                .show(ctx, self.graphics.as_ref(), &self.imports);
            match action {
                Some(model_browser::Action::ManageImports) => self.workspace = 5,
                Some(model_browser::Action::Stage { label, verified }) => {
                    match self.imports.stage_browser_preview(label, verified) {
                        Ok(()) => self.workspace = 5,
                        Err(error) => self.model_browser.as_mut().unwrap().set_notice(error),
                    }
                }
                None => {}
            }
            self.session_controls(ctx);
            ctx.request_repaint_after(Duration::from_millis(33));
            return;
        }
        if self.workspace == 5 {
            self.client.poll_hidden();
            if self.imports.show(ctx, self.graphics.as_ref()) {
                self.workspace = 8;
            }
            self.session_controls(ctx);
            ctx.request_repaint_after(Duration::from_millis(33));
            return;
        }
        if self.workspace == 3 && self.surface_mode {
            self.client.poll_hidden();
            egui::SidePanel::right("surface_material_inspector")
                .resizable(true)
                .default_width(310.)
                .width_range(255. ..=500.)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("surface_inspector_scroll")
                        .show(ui, |ui| self.surface_materials.show_node_inspector(ui));
                });
            egui::SidePanel::left("surface_material_library")
                .resizable(true)
                .default_width(360.)
                .width_range(280. ..=560.)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("surface_library_scroll")
                        .max_height((ui.available_height() * 0.4).clamp(120., 280.))
                        .show(ui, |ui| self.surface_materials.show_library(ui));
                    ui.separator();
                    ui.strong("PIONEER HULL SECTION");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.surface_hangar_light, true, "Hangar light");
                        ui.selectable_value(&mut self.surface_hangar_light, false, "Space light");
                    });
                    let gallery = self
                        .asset_gallery
                        .get_or_insert_with(|| EmbeddedAssetGallery::new(self.graphics.as_ref()));
                    gallery.select_ship(arkline_protocol::ShipClass::Pioneer);
                    gallery.set_preview_station_lighting(self.surface_hangar_light);
                    gallery.set_spatial_material(self.surface_materials.revision());
                    let height = ui.available_height().clamp(160., 440.);
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| gallery.show_hull_surface(ui, self.materials.preview()),
                    );
                });
            egui::CentralPanel::default().show(ctx, |ui| {
                self.surface_materials.show_graph(ui);
            });
            self.session_controls(ctx);
            ctx.request_repaint_after(Duration::from_millis(33));
            return;
        }
        if self.workspace == 3 {
            self.client.poll_hidden();
            egui::SidePanel::right("fleet_materials_inspector")
                .resizable(true)
                .default_width(280.)
                .width_range(245. ..=430.)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("fleet_materials_scroll")
                        .show(ui, |ui| {
                            self.materials.show_node_inspector(ui, &self.project);
                            self.provenance.show(
                                ui,
                                &self.project,
                                provenance::AssetFile::Material,
                                None,
                            );
                        });
                });
            egui::SidePanel::left("material_node_library")
                .resizable(true)
                .default_width(265.)
                .width_range(220. ..=380.)
                .show(ctx, |ui| {
                    self.materials.show_library(ui);
                    ui.separator();
                    ui.strong("SHIP PREVIEW");
                    let height = ui.available_height().clamp(120., 390.);
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            let gallery = self.asset_gallery.get_or_insert_with(|| {
                                EmbeddedAssetGallery::new(self.graphics.as_ref())
                            });
                            gallery.set_preview_station_lighting(true);
                            gallery.show_compact(ui, self.materials.preview());
                        },
                    );
                });
            egui::CentralPanel::default().show(ctx, |ui| {
                self.materials.show_graph(ui, &self.project);
            });
            self.client.set_material_preview(self.materials.preview());
            self.session_controls(ctx);
            ctx.request_repaint_after(Duration::from_millis(33));
            return;
        }
        if self.workspace == 2 {
            self.client.poll_hidden();
            self.physics_lab.show(ctx);
            self.session_controls(ctx);
            ctx.request_repaint_after(Duration::from_millis(16));
            return;
        }
        if self.workspace == 1 {
            self.client.poll_hidden();
            self.community.show(ctx, &self.project);
            self.session_controls(ctx);
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        if self.inspector_tab == 6 {
            egui::TopBottomPanel::top("room_view_bar").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.room_plan, true, "Draft plan");
                    ui.selectable_value(&mut self.room_plan, false, "Live 3D");
                    ui.small(if self.room_plan {
                        "Editing draft geometry"
                    } else {
                        "Confirmed live geometry"
                    });
                });
            });
        }
        if self.show_world && !self.flight_focus {
            egui::SidePanel::left("editor_outliner").resizable(true).default_width(200.).width_range(180. ..=260.).show(ctx, |ui| {
            if self.inspector_tab == 6 { self.rooms.navigation(ui); return; }
            ui.add_space(10.); ui.heading("World outliner");
            if self.client.free_camera_active() {
                ui.colored_label(GREEN, "FREE CAMERA · local view only");
                ui.small("WASD move · E/Q or Space/C vertical · Shift faster · RMB look");
                ui.small("Ship/avatar input is suspended. Player camera returns to your controls.");
            }
            ui.colored_label(MUTED,"Live simulation objects"); ui.separator();
            egui::ScrollArea::vertical().id_salt("editor_world_scroll").show(ui, |ui| {
                if let Some(world)=&snapshot {
                    ui.collapsing(format!("Pilots ({})",world.pilots.len()),|ui| {
                        for pilot in &world.pilots {
                            ui.selectable_value(&mut self.selected_pilot,pilot.name.clone(),&pilot.name);
                        }
                    });
                    ui.collapsing("Stations and gates", |ui| {
                        for system in arkline_protocol::systems() {
                            ui.label(&system.name);
                        }
                        ui.small("Inspect ships through the pilot list. Select navigation objects inside the game viewport.");
                    });
                    ui.collapsing(format!("Asteroids ({})",world.asteroids.len()), |ui| {
                        for asteroid in world.asteroids.iter().take(24) { ui.label(format!("Ore asteroid {}",asteroid.id)); }
                        if world.asteroids.len()>24 { ui.small("First 24 shown. The in-game overview lists local targets."); }
                    });
                } else { ui.label("Waiting for the local authority…"); }
                if self.selected_pilot == self.client.pilot() && assets.as_ref().and_then(|a| a.pioneer_room.as_ref()).is_some() {
                    ui.collapsing("Live Pioneer room equipment", |ui| {
                        for device in arkline_protocol::pioneer_room_catalog().equipment {
                            if ui.selectable_value(&mut self.selected_equipment, device.id.to_owned(), device.label).clicked() {
                                self.inspector_tab = 4; self.show_inspector = true;
                            }
                        }
                    });
                }
                if self.engineering.outliner(ui) { self.inspector_tab = 2; self.show_inspector = true; }
            });
        });
        }
        if self.show_inspector && !self.flight_focus {
            egui::SidePanel::right("editor_inspector")
            .resizable(true)
            .default_width(280.)
            .width_range(260. ..=(ctx.available_rect().width() - 560.).clamp(260., 360.))
            .show(ctx, |ui| {
                ui.add_space(10.);
                ui.heading(match self.inspector_tab {
                    2 => "Engineering candidate", 3 => "Render debug",
                    4 => "Room equipment", 5 => "Service paths",
                    6 => "Room properties", 7 => "Exterior systems", _ => "Live state",
                });
                if self.inspector_tab != 6 {
                ui.label(RichText::new(&self.selected_pilot).color(MUTED));
                if !active {
                    ui.colored_label(MUTED, "Captured state · simulation is not running");
                }
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("editor_inspector_scroll")
                    .show(ui, |ui| {
                        if self.inspector_tab == 1 {
                            self.materials.show(ui, &self.project);
                            self.provenance.show(
                                ui,
                                &self.project,
                                provenance::AssetFile::Material,
                                None,
                            );
                        } else if self.inspector_tab == 2 {
                            self.engineering.show(ui, &self.project);
                            self.provenance.show(
                                ui,
                                &self.project,
                                provenance::AssetFile::Engineering,
                                self.engineering.selected_component(),
                            );
                        } else if self.inspector_tab == 3 {
                            self.render_debug
                                .show(ui, &mut self.client, &mut self.wireframe);
                        } else if self.inspector_tab == 7 {
                            self.exterior.show(ui, &self.project, &mut self.client);
                        } else if self.inspector_tab == 6 {
                            self.rooms.show(ui, &self.project, &mut self.client);
                        } else if self.inspector_tab == 5 {
                            self.service_paths.show(ui, &self.project, self.engineering.components());
                        } else if self.inspector_tab == 4 {
                            let own_pilot = self.selected_pilot == self.client.pilot();
                            if let Some(action) = equipment::show(
                                ui, snapshot.as_ref(), if own_pilot { assets.as_ref() } else { None },
                                &self.selected_equipment, active && own_pilot,
                            ) {
                                match action {
                                    equipment::Action::Send(request) => self.client.send_command(request),
                                    equipment::Action::AddToGraph(action) => {
                                        match self.event_graph.add_room_action(action) {
                                            Ok(()) => self.events_open = true,
                                            Err(error) => self.console.push_back(format!("Event draft: {error}")),
                                        }
                                    }
                                }
                            }
                            if !active { ui.small("Play to operate equipment. Use event graph stepping for paused experiments."); }
                        } else {
                            inspector::show(
                                ui,
                                snapshot.as_ref(),
                                if self.selected_pilot == self.client.pilot() {
                                    assets.as_ref()
                                } else {
                                    None
                                },
                                &self.selected_pilot,
                            );
                        }
                    });
            });
        }
        // The real client owns the remaining central viewport and its normal controls.
        self.client.set_debug_wireframe(self.wireframe);
        self.client.set_material_preview(self.materials.preview());
        if self.inspector_tab == 2 {
            self.client.poll_hidden();
            egui::CentralPanel::default().show(ctx, |ui| {
                self.engineering.viewport(ui);
            });
        } else if self.inspector_tab == 6 && self.room_plan {
            self.client.poll_hidden();
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("room_plan_scroll")
                    .show(ui, |ui| self.rooms.preview(ui));
            });
        } else {
            if let Some(request) = self
                .cockpit
                .show_applied(ctx, &cockpit::Live::from_client(&self.client))
            {
                self.client.send_command(request);
            }
            self.client.set_editor_input_blocked(
                !ctx.input(|input| input.focused) || ctx.wants_keyboard_input(),
            );
            self.client.update(ctx, frame);
        }
        // Editor dialogs are drawn after the embedded client's UI pass, which
        // finalizes its accessibility tree and viewport layers.
        self.session_controls(ctx);
        ctx.request_repaint_after(Duration::from_millis(if active { 16 } else { 50 }));
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        self.client.on_exit();
        self.imports.destroy();
        if let Some(gallery) = &mut self.asset_gallery {
            gallery.destroy();
        }
        if let Some(browser) = &mut self.model_browser {
            browser.destroy();
        }
    }
}
fn project_path(
    default: PathBuf,
    environment: Option<std::ffi::OsString>,
    explicit: Option<PathBuf>,
) -> PathBuf {
    explicit
        .or_else(|| {
            environment
                .filter(|value| !value.to_string_lossy().trim().is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or(default)
}
fn editor_msaa(value: Option<&std::ffi::OsStr>) -> Result<u16, String> {
    match value {
        None => Ok(4),
        Some(value) => match value.to_str() {
            Some("0") => Ok(0),
            Some("2") => Ok(2),
            Some("4") => Ok(4),
            Some("8") => Ok(8),
            _ => Err("ARKLINE_EDITOR_MSAA must be 0, 2, 4 or 8".into()),
        },
    }
}
fn main() -> eframe::Result<()> {
    let mut project = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("data/editor-project");
    #[cfg(target_os = "macos")]
    if std::env::current_exe()
        .ok()
        .is_some_and(|p| p.parent().is_some_and(|p| p.ends_with("Contents/MacOS")))
    {
        if let Some(home) = std::env::var_os("HOME") {
            project =
                PathBuf::from(home).join("Library/Application Support/Arkline/Editor/FirstProject");
        }
    }
    project = project_path(project, std::env::var_os("ARKLINE_EDITOR_PROJECT"), None);
    let mut pilot = "EditorPilot".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "Arkline Editor {}\nUsage: arkline-editor [--project PATH] [--pilot NAME]\nARKLINE_EDITOR_PROJECT sets the default project; --project overrides it. Blank environment values are ignored.\nARKLINE_EDITOR_MSAA sets graphics multisampling to 0, 2, 4 or 8 (default 4).\nEmbedded game viewport with local Play, Pause, Step and Stop. No separate client windows or public connections.",
                    BUILD.trim()
                );
                return Ok(());
            }
            "--project" => {
                let explicit = args.next().map(PathBuf::from).unwrap_or_else(|| {
                    eprintln!("--project requires PATH");
                    std::process::exit(2)
                });
                project = project_path(project, None, Some(explicit));
            }
            "--pilot" => {
                pilot = args.next().unwrap_or_else(|| {
                    eprintln!("--pilot requires NAME");
                    std::process::exit(2)
                })
            }
            _ => {
                eprintln!("Unknown argument: {arg}");
                std::process::exit(2);
            }
        }
    }
    let multisampling = editor_msaa(std::env::var_os("ARKLINE_EDITOR_MSAA").as_deref())
        .unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(2)
        });
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        multisampling,
        depth_buffer: 24,
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("Arkline Editor — {}", BUILD.trim()))
            .with_inner_size([1500., 960.])
            .with_min_inner_size([1080., 720.]),
        ..Default::default()
    };
    eframe::run_native(
        "Arkline Editor",
        options,
        Box::new(move |cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = Color32::from_rgb(10, 17, 15);
            visuals.window_fill = Color32::from_rgb(13, 23, 19);
            visuals.selection.bg_fill = Color32::from_rgb(15, 80, 47);
            visuals.selection.stroke.color = GREEN;
            cc.egui_ctx.set_visuals(visuals);
            let mut editor = Editor::new(cc, project, pilot);
            editor.client.start_audio_output();
            Ok(Box::new(editor))
        }),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn multisampling_override_accepts_supported_counts_and_rejects_invalid_values() {
        use std::ffi::OsStr;
        assert_eq!(super::editor_msaa(None).unwrap(), 4);
        for samples in ["0", "2", "4", "8"] {
            assert_eq!(
                super::editor_msaa(Some(OsStr::new(samples))).unwrap(),
                samples.parse::<u16>().unwrap()
            );
        }
        for invalid in ["", "1", "3", "16", " 0", "NaN"] {
            assert!(super::editor_msaa(Some(OsStr::new(invalid)))
                .unwrap_err()
                .contains("ARKLINE_EDITOR_MSAA"));
        }
    }
    use super::*;
    use eframe::App;

    pub(super) fn visible_point(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    let rect = text.galley.rect.translate(text.pos.to_vec2());
                    shape.clip_rect.contains_rect(rect).then_some(rect.center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("fully visible {label:?} not found"))
    }
    #[test]
    fn project_environment_is_optional_and_explicit_path_wins() {
        let default = PathBuf::from("default-project");
        for env in [None, Some("".into()), Some("  ".into())] {
            assert_eq!(project_path(default.clone(), env, None), default);
        }
        assert_eq!(
            project_path(default.clone(), Some("QA project".into()), None),
            PathBuf::from("QA project")
        );
        assert_eq!(
            project_path(
                default,
                Some("QA project".into()),
                Some(PathBuf::from("explicit-project"))
            ),
            PathBuf::from("explicit-project")
        );
    }
    #[test]
    fn editor_controls_remain_reachable_in_final_accessibility_tree() {
        use std::collections::{HashMap, HashSet};
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let project = std::env::temp_dir().join(format!(
            "arkline-editor-accessibility-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&project).unwrap();
        let mut editor = Editor::new(&cc, project.clone(), "AccessibilityPilot".into());
        editor.events_open = true;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !editor.client.ready() && std::time::Instant::now() < deadline {
            editor.client.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            editor.client.ready(),
            "isolated test authority did not become ready; status={:?}",
            editor.client.status()
        );
        for workspace in [0, 2, 6, 7, 8, 0] {
            editor.workspace = workspace;
            for _ in 0..3 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1500., 960.),
                        )),
                        ..Default::default()
                    },
                    |ctx| editor.update(ctx, &mut eframe::Frame::_new_kittest()),
                );
                let update = output
                    .platform_output
                    .accesskit_update
                    .expect("AccessKit output enabled");
                let root = update.tree.as_ref().unwrap().root;
                let nodes: HashMap<_, _> =
                    update.nodes.iter().map(|(id, node)| (*id, node)).collect();
                let mut reachable = HashSet::new();
                let mut stack = vec![root];
                while let Some(id) = stack.pop() {
                    if reachable.insert(id) {
                        if let Some(node) = nodes.get(&id) {
                            stack.extend(node.children().iter().copied());
                        }
                    }
                }
                let labels: Vec<_> = update
                    .nodes
                    .iter()
                    .filter_map(|(id, node)| node.label().map(|label| (*id, label.to_owned())))
                    .collect();
                let expected: &[&str] = if workspace == 0 {
                    &[
                        "▶  Play",
                        "Ⅱ  Pause",
                        "Step 50 ms",
                        "■  Stop",
                        "Validate draft",
                        "Save draft",
                        "Apply to runtime",
                        "Step one node",
                    ]
                } else {
                    &["▶  Play", "Ⅱ  Pause", "Step 50 ms", "■  Stop"]
                };
                for label in expected {
                    assert!(
                        labels
                            .iter()
                            .any(|(id, text)| text == label && reachable.contains(id)),
                        "workspace {workspace}: missing reachable {label:?}; labels={labels:?}"
                    );
                }
            }
        }
        editor.client.on_exit();
        drop(editor);
        let _ = std::fs::remove_dir_all(project);
    }
    #[test]
    fn session_dialog_controls_remain_reachable_after_game_and_material_updates() {
        use std::collections::{HashMap, HashSet};
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let project = std::env::temp_dir().join(format!(
            "arkline-editor-session-accessibility-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&project).unwrap();
        let mut editor = Editor::new(&cc, project.clone(), "SessionAccessPilot".into());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !editor.client.ready() && std::time::Instant::now() < deadline {
            editor.client.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            editor.client.ready(),
            "isolated test authority did not become ready; status={:?}",
            editor.client.status()
        );

        for workspace in [0, 3, 8] {
            editor.workspace = workspace;
            editor.session_panel.open = true;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1500., 960.),
                    )),
                    ..Default::default()
                },
                |ctx| editor.update(ctx, &mut eframe::Frame::_new_kittest()),
            );
            let update = output
                .platform_output
                .accesskit_update
                .expect("AccessKit output enabled");
            let root = update.tree.as_ref().unwrap().root;
            let nodes: HashMap<_, _> = update.nodes.iter().map(|(id, node)| (*id, node)).collect();
            let mut reachable = HashSet::new();
            let mut stack = vec![root];
            while let Some(id) = stack.pop() {
                if reachable.insert(id) {
                    if let Some(node) = nodes.get(&id) {
                        stack.extend(node.children().iter().copied());
                    }
                }
            }
            let labels: Vec<_> = update
                .nodes
                .iter()
                .filter_map(|(id, node)| node.label().map(|label| (*id, label.to_owned())))
                .collect();
            for expected in ["Multiplayer session", "Local", "Host", "Join"] {
                assert!(
                    labels
                        .iter()
                        .any(|(id, text)| text == expected && reachable.contains(id)),
                    "workspace {workspace}: missing reachable session control {expected:?}; labels={labels:?}"
                );
            }
            let pilot_field = update.nodes.iter().find(|(id, node)| {
                reachable.contains(id)
                    && node.value() == Some("SessionAccessPilot")
                    && format!("{:?}", node.role()) == "TextInput"
            });
            assert!(
                pilot_field.is_some(),
                "workspace {workspace}: session pilot text field is not reachable; labels={labels:?}"
            );
            let pilot_field = pilot_field.unwrap();
            assert!(
                pilot_field.1.labelled_by().iter().any(|label_id| nodes
                    .get(label_id)
                    .is_some_and(|label| { label.value() == Some("Pilot") })),
                "workspace {workspace}: session pilot text field is not labelled by the Pilot control"
            );
        }

        editor.client.on_exit();
        drop(editor);
        let _ = std::fs::remove_dir_all(project);
    }
    #[test]
    fn room_name_accepts_pointer_and_text_events_in_paused_full_editor() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let project = std::env::temp_dir().join(format!(
            "arkline-editor-room-text-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&project).unwrap();
        let mut editor = Editor::new(&cc, project.clone(), "RoomTextPilot".into());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !editor.client.ready() && std::time::Instant::now() < deadline {
            editor.client.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            editor.client.ready(),
            "isolated test authority did not become ready"
        );
        assert!(
            editor.client.paused(),
            "room text test requires a paused world"
        );
        let starting_time = editor.client.snapshot().unwrap().server_time_ms;
        editor.inspector_tab = 6;

        let frame = |editor: &mut Editor, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1500., 960.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| editor.update(ctx, &mut eframe::Frame::_new_kittest()),
            )
        };
        let row = "D1 · D1-P1 · AIRLOCK / EVA";
        let output = frame(&mut editor, vec![]);
        let row_point = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == row => {
                    let bounds = text.galley.rect.translate(text.pos.to_vec2());
                    shape
                        .clip_rect
                        .contains_rect(bounds)
                        .then_some(bounds.center())
                }
                _ => None,
            })
            .expect("room selector is visible in the Game workspace inspector");
        for pressed in [true, false] {
            frame(
                &mut editor,
                vec![
                    egui::Event::PointerMoved(row_point),
                    egui::Event::PointerButton {
                        pos: row_point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }

        let output = frame(&mut editor, vec![]);
        let name_point = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "AIRLOCK / EVA" => {
                    let bounds = text.galley.rect.translate(text.pos.to_vec2());
                    shape
                        .clip_rect
                        .contains_rect(bounds)
                        .then_some(egui::pos2(bounds.right() - 1., bounds.center().y))
                }
                _ => None,
            })
            .expect("selected room name field is visible");
        for pressed in [true, false] {
            frame(
                &mut editor,
                vec![
                    egui::Event::PointerMoved(name_point),
                    egui::Event::PointerButton {
                        pos: name_point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        frame(&mut editor, vec![egui::Event::Text(" QA".into())]);

        let room_name = editor
            .rooms
            .draft_room_name_by_source_id("D1-P1")
            .expect("the selected source room remains in the draft");
        assert!(
            room_name.contains("QA"),
            "room name did not receive the Text event: {room_name:?}"
        );
        assert!(
            editor.client.paused(),
            "editing a room name changed pause state"
        );
        assert_eq!(
            editor.client.snapshot().unwrap().server_time_ms,
            starting_time
        );

        editor.client.on_exit();
        drop(editor);
        let _ = std::fs::remove_dir_all(project);
    }
    #[test]
    fn scene_toolbar_places_pilot_while_paused_and_material_workspace_has_graph_panels() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let project = std::env::temp_dir().join(format!(
            "arkline-editor-scene-ui-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut editor = Editor::new(&cc, project.clone(), "SceneToolbarPilot".into());
        editor.flight_focus = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !editor.client.ready() && std::time::Instant::now() < deadline {
            editor.client.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(editor.client.paused());
        let time = editor.client.snapshot().unwrap().server_time_ms;
        let frame = |editor: &mut Editor, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1500., 960.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| editor.update(ctx, &mut eframe::Frame::_new_kittest()),
            )
        };
        frame(&mut editor, vec![]);
        let output = frame(&mut editor, vec![]);
        let destination = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "Station Hangar" => {
                    let bounds = text.galley.rect.translate(text.pos.to_vec2());
                    assert!(shape.clip_rect.contains_rect(bounds));
                    Some(bounds.center())
                }
                _ => None,
            })
            .expect("scene destination is visible above viewport");
        for pressed in [true, false] {
            frame(
                &mut editor,
                vec![
                    egui::Event::PointerMoved(destination),
                    egui::Event::PointerButton {
                        pos: destination,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        // Snapshot and acknowledgement arrive on separate channels. Observing
        // the new scene does not mean poll has drained its acknowledgement yet.
        while (editor.client.scene_state()
            != Some(arkline_protocol::EditorSceneState::StationHangar)
            || editor.client.scene_state_ack().is_none())
            && std::time::Instant::now() < deadline
        {
            editor.client.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            editor.client.scene_state(),
            Some(arkline_protocol::EditorSceneState::StationHangar)
        );
        assert!(editor
            .client
            .scene_state_ack()
            .expect("scene acknowledgement should arrive before the deadline")
            .result
            .is_ok());
        assert!(editor.client.paused());
        assert_eq!(editor.client.snapshot().unwrap().server_time_ms, time);
        editor.workspace = 3;
        frame(&mut editor, vec![]);
        let output = frame(&mut editor, vec![]);
        for label in [
            "Node library",
            "+ Color",
            "SHIP PREVIEW",
            "Save",
            "Fleet Material",
        ] {
            assert!(
                output.shapes.iter().any(|shape| match &shape.shape {
                    egui::Shape::Text(text) =>
                        text.galley.text() == label
                            && shape
                                .clip_rect
                                .contains_rect(text.galley.rect.translate(text.pos.to_vec2())),
                    _ => false,
                }),
                "material workspace is missing visible {label}"
            );
        }
        editor.client.on_exit();
        drop(editor);
        let _ = std::fs::remove_dir_all(project);
    }
    #[test]
    fn mining_header_opens_existing_routine_without_starting_the_world() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let project = std::env::temp_dir().join(format!(
            "arkline-mining-header-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut editor = Editor::new(&cc, project, "MiningHeaderPilot".into());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !editor.client.ready() && std::time::Instant::now() < deadline {
            editor.client.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(editor.client.ready());
        let before = serde_json::to_value(editor.client.snapshot().unwrap()).unwrap();
        for size in [egui::vec2(1080., 720.), egui::vec2(1500., 960.)] {
            editor.workspace = 3;
            let frame = |editor: &mut Editor, events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        events,
                        ..Default::default()
                    },
                    |ctx| editor.update(ctx, &mut eframe::Frame::_new_kittest()),
                )
            };
            frame(&mut editor, vec![]);
            let output = frame(&mut editor, vec![]);
            let point = visible_point(&output, "Mining");
            for pressed in [true, false] {
                frame(
                    &mut editor,
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
            frame(&mut editor, vec![]);
            let output = frame(&mut editor, vec![]);
            visible_point(&output, "Mining routine");
            assert_eq!(editor.workspace, 0);
            assert_eq!(editor.inspector_tab, 0);
            assert_eq!(editor.client.status(), &EmbeddedStatus::Paused);
            assert_eq!(
                serde_json::to_value(editor.client.snapshot().unwrap()).unwrap(),
                before
            );
        }
        editor.client.on_exit();
    }

    #[test]
    fn toolbar_controls_are_visible_above_the_game_viewport() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let project =
            std::env::temp_dir().join(format!("arkline-editor-toolbar-{}", std::process::id()));
        let mut editor = Editor::new(&cc, project, "ToolbarPilot".into());
        for size in [egui::vec2(1180., 812.), egui::vec2(1080., 720.)] {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                },
                |ctx| {
                    editor.update(ctx, &mut eframe::Frame::_new_kittest());
                },
            );
            let mut found = 0;
            let mut console_y = 0.;
            for clipped in output.shapes {
                if let egui::Shape::Text(t) = clipped.shape {
                    if t.galley.text() == "Console" {
                        console_y = t.pos.y;
                    }
                    if ["▶  Play", "Ⅱ  Pause", "Step 50 ms", "■  Stop"].contains(&t.galley.text())
                    {
                        assert!(clipped.clip_rect.contains(t.pos));
                        assert!(t.pos.y < 58.0);
                        found += 1;
                    }
                }
            }
            assert_eq!(found, 4);
            assert!(
                console_y > size.y - 40.,
                "default drawer consumes the working view: {console_y}"
            );
        }
        editor.client.on_exit();
    }
    #[test]
    fn toolbar_play_from_engineering_workspace_returns_to_game_and_runs() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let project = std::env::temp_dir().join(format!(
            "arkline-editor-play-from-engineering-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut editor = Editor::new(&cc, project.clone(), "EngineeringPlayPilot".into());
        let ready_deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !editor.client.ready() && std::time::Instant::now() < ready_deadline {
            editor.client.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            editor.client.paused(),
            "test requires a ready, paused local world; status={:?}",
            editor.client.status()
        );
        editor.workspace = 3;
        editor.inspector_tab = 2;

        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1500., 960.),
                )),
                ..Default::default()
            },
            |ctx| editor.update(ctx, &mut eframe::Frame::_new_kittest()),
        );
        let play_position = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == "▶  Play" => {
                    Some(text.pos + text.galley.rect.center().to_vec2())
                }
                _ => None,
            })
            .expect("toolbar Play button is visible from the Engineering workspace");

        let frame = |editor: &mut Editor, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1500., 960.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| editor.update(ctx, &mut eframe::Frame::_new_kittest()),
            )
        };
        frame(
            &mut editor,
            vec![
                egui::Event::PointerMoved(play_position),
                egui::Event::PointerButton {
                    pos: play_position,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        frame(
            &mut editor,
            vec![egui::Event::PointerButton {
                pos: play_position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        assert_eq!(editor.workspace, 0);
        assert_eq!(editor.inspector_tab, 0);

        let running_deadline = std::time::Instant::now() + Duration::from_secs(5);
        while editor.client.status() != &EmbeddedStatus::Running
            && std::time::Instant::now() < running_deadline
        {
            editor.client.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            editor.client.status(),
            &EmbeddedStatus::Running,
            "Play click did not start the authoritative simulation"
        );
        editor.client.on_exit();
        drop(editor);
        let _ = std::fs::remove_dir_all(project);
    }
    #[test]
    fn drawer_closes_after_event_workspace_switch() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let project = std::env::temp_dir().join(format!(
            "arkline-editor-drawer-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut editor = Editor::new(&cc, project.clone(), "EngineeringPlayPilot".into());
        let ready_deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !editor.client.ready() && std::time::Instant::now() < ready_deadline {
            editor.client.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            editor.client.paused(),
            "test requires a ready, paused local world; status={:?}",
            editor.client.status()
        );
        let frame = |editor: &mut Editor, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1080., 720.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| editor.update(ctx, &mut eframe::Frame::_new_kittest()),
            )
        };
        let clock = format!(
            "Simulation: {} ms",
            editor.client.snapshot().unwrap().server_time_ms
        );
        for (tab, label) in [(0, "Console"), (1, "Project")] {
            editor.drawer_open = true;
            editor.bottom_tab = tab;
            editor.workspace = 0;
            editor.events_open = true;
            frame(&mut editor, vec![]);
            assert!(!editor.console_visible());
            editor.workspace = 3;
            let output = frame(&mut editor, vec![]);
            assert!(editor.console_visible());
            let play_position = output
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text() == label => {
                        Some(t.pos + t.galley.rect.center().to_vec2())
                    }
                    _ => None,
                })
                .unwrap();
            frame(
                &mut editor,
                vec![
                    egui::Event::PointerMoved(play_position),
                    egui::Event::PointerButton {
                        pos: play_position,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ],
            );
            frame(
                &mut editor,
                vec![egui::Event::PointerButton {
                    pos: play_position,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
            );
            assert!(
                !editor.drawer_open && !editor.events_open && !editor.console_visible(),
                "{label} must close on first click"
            );
            let output = frame(&mut editor, vec![]);
            assert!(output
                .shapes
                .iter()
                .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == clock)));
        }
        editor.client.on_exit();
        drop(editor);
        let _ = std::fs::remove_dir_all(project);
    }
    #[test]
    fn room_workspace_layout_at_normal_and_compact_sizes() {
        for size in [egui::vec2(1180., 812.), egui::vec2(1080., 720.)] {
            let ctx = egui::Context::default();
            let cc = eframe::CreationContext::_new_kittest(ctx.clone());
            let project = std::env::temp_dir().join(format!(
                "arkline-editor-room-text-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&project).unwrap();
            let mut editor = Editor::new(&cc, project.clone(), "RoomTextPilot".into());
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !editor.client.ready() && std::time::Instant::now() < deadline {
                editor.client.poll();
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                editor.client.ready(),
                "isolated test authority did not become ready"
            );
            assert!(
                editor.client.paused(),
                "room text test requires a paused world"
            );
            let starting_time = editor.client.snapshot().unwrap().server_time_ms;
            editor.inspector_tab = 6;

            let frame = |editor: &mut Editor, events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        events,
                        ..Default::default()
                    },
                    |ctx| editor.update(ctx, &mut eframe::Frame::_new_kittest()),
                )
            };
            let row = "D1 · D1-P1 · AIRLOCK / EVA";
            let output = frame(&mut editor, vec![]);

            for label in [
                "Pioneer rooms",
                "Room properties",
                "Draft plan",
                "Live 3D",
                "Save draft",
                "Save + apply live",
            ] {
                assert!(
                    output.shapes.iter().any(|s| match &s.shape {
                        egui::Shape::Text(t) =>
                            t.galley.text() == label
                                && s.clip_rect
                                    .contains_rect(t.galley.rect.translate(t.pos.to_vec2())),
                        _ => false,
                    }),
                    "missing visible {label} at {size:?}"
                );
            }
            let row_point = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == row => {
                        let bounds = text.galley.rect.translate(text.pos.to_vec2());
                        shape
                            .clip_rect
                            .contains_rect(bounds)
                            .then_some(bounds.center())
                    }
                    _ => None,
                })
                .expect("room selector is visible in the Game workspace inspector");
            for pressed in [true, false] {
                frame(
                    &mut editor,
                    vec![
                        egui::Event::PointerMoved(row_point),
                        egui::Event::PointerButton {
                            pos: row_point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }

            let output = frame(&mut editor, vec![]);
            {
                let label = "Deck 1";
                assert!(
                    output.shapes.iter().any(|s| match &s.shape {
                        egui::Shape::Text(t) =>
                            t.galley.text() == label
                                && s.clip_rect
                                    .contains_rect(t.galley.rect.translate(t.pos.to_vec2())),
                        _ => false,
                    }),
                    "missing visible {label} at {size:?}"
                );
            }
            let name_point = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == "AIRLOCK / EVA" => {
                        let bounds = text.galley.rect.translate(text.pos.to_vec2());
                        shape
                            .clip_rect
                            .contains_rect(bounds)
                            .then_some(egui::pos2(bounds.right() - 1., bounds.center().y))
                    }
                    _ => None,
                })
                .expect("selected room name field is visible");
            for pressed in [true, false] {
                frame(
                    &mut editor,
                    vec![
                        egui::Event::PointerMoved(name_point),
                        egui::Event::PointerButton {
                            pos: name_point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            frame(&mut editor, vec![egui::Event::Text(" QA".into())]);

            let room_name = editor
                .rooms
                .draft_room_name_by_source_id("D1-P1")
                .expect("the selected source room remains in the draft");
            assert!(
                room_name.contains("QA"),
                "room name did not receive the Text event: {room_name:?}"
            );
            let edited = editor
                .rooms
                .draft_room_name_by_source_id("D1-P1")
                .unwrap()
                .to_owned();
            let scene = editor.client.scene_state();
            let camera = editor.client.free_camera_active();
            for label in ["Live 3D", "Draft plan"] {
                let output = frame(&mut editor, vec![]);
                let row_point = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text() == label => {
                            let bounds = text.galley.rect.translate(text.pos.to_vec2());
                            shape
                                .clip_rect
                                .contains_rect(bounds)
                                .then_some(bounds.center())
                        }
                        _ => None,
                    })
                    .expect("room selector is visible in the Game workspace inspector");
                for pressed in [true, false] {
                    frame(
                        &mut editor,
                        vec![
                            egui::Event::PointerMoved(row_point),
                            egui::Event::PointerButton {
                                pos: row_point,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    );
                }

                assert_eq!(editor.room_plan, label == "Draft plan");
                assert_eq!(editor.client.scene_state(), scene);
                assert_eq!(editor.client.free_camera_active(), camera);
            }
            assert_eq!(
                editor.rooms.draft_room_name_by_source_id("D1-P1"),
                Some(edited.as_str())
            );
            assert!(
                editor.client.paused(),
                "editing a room name changed pause state"
            );
            assert_eq!(
                editor.client.snapshot().unwrap().server_time_ms,
                starting_time
            );

            editor.client.on_exit();
            drop(editor);
            let _ = std::fs::remove_dir_all(project);
        }
    }
    #[test]
    fn flight_editor_focus_keeps_tools_accessible_and_preserves_paused_world() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let project = std::env::temp_dir().join(format!(
            "arkline-flight-focus-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut editor = Editor::new(&cc, project, "FlightFocusQA".into());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !editor.client.ready() && std::time::Instant::now() < deadline {
            editor.client.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(editor.client.paused());
        let before = serde_json::to_value(editor.client.snapshot().unwrap()).unwrap();
        let frame = |editor: &mut Editor, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1500., 960.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| editor.update(ctx, &mut eframe::Frame::_new_kittest()),
            )
        };
        let has = |output: &egui::FullOutput, label: &str| {
            output.shapes.iter().any(
                |shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.text()==label),
            )
        };
        frame(&mut editor, vec![]);
        let output = frame(&mut editor, vec![]);
        assert!(editor.flight_focus);
        assert!(!has(&output, "World outliner") && !has(&output, "Inspector"));
        for label in ["Editor", "Models", "Mining", "Sound"] {
            visible_point(&output, label);
        }
        let point = visible_point(&output, "Editor");
        for pressed in [true, false] {
            frame(
                &mut editor,
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
        frame(&mut editor, vec![]);
        let output = frame(&mut editor, vec![]);
        assert!(!editor.flight_focus);
        assert!(has(&output, "World outliner") && has(&output, "Inspector"));
        let point = visible_point(&output, "Flight");
        for pressed in [true, false] {
            frame(
                &mut editor,
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
        frame(&mut editor, vec![]);
        let output = frame(&mut editor, vec![]);
        assert!(editor.flight_focus);
        assert!(!has(&output, "World outliner") && !has(&output, "Inspector"));
        assert_eq!(
            serde_json::to_value(editor.client.snapshot().unwrap()).unwrap(),
            before
        );
        editor.client.on_exit();
    }
}
