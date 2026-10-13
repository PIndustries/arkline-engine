//! One-window local development workspace using the real game client and authority.
mod asset_document;
mod community;
mod component_view;
mod engineering;
mod engineering_document;
mod event_graph;
mod github;
mod github_auth;
mod inspector;
mod materials;
mod physics_lab;
mod provenance;
mod render_debug;
use arkline_client::{EmbeddedAssetGallery, EmbeddedClient, EmbeddedStatus};
use eframe::egui::{self, Color32, RichText};
use std::{collections::VecDeque, path::PathBuf, time::Duration};
const BUILD: &str = include_str!("../../../BUILD_VERSION");
const GREEN: Color32 = Color32::from_rgb(62, 233, 145);
const MUTED: Color32 = Color32::from_rgb(140, 165, 158);
struct Editor {
    client: EmbeddedClient,
    project: PathBuf,
    selected_pilot: String,
    console: VecDeque<String>,
    last_status: String,
    bottom_tab: usize,
    inspector_tab: usize,
    materials: materials::Materials,
    engineering: engineering::Engineering,
    community: community::Community,
    event_graph: event_graph::EventGraphPanel,
    events_open: bool,
    workspace: usize,
    wireframe: bool,
    asset_gallery: Option<EmbeddedAssetGallery>,
    graphics: Option<std::sync::Arc<eframe::glow::Context>>,
    render_debug: render_debug::RenderDebug,
    physics_lab: physics_lab::PhysicsLab,
    provenance: provenance::Provenance,
}
impl Editor {
    fn new(cc: &eframe::CreationContext<'_>, project: PathBuf, pilot: String) -> Self {
        let client = EmbeddedClient::new(cc, project.join(".arkline-studio"), pilot.clone());
        Self {
            materials: materials::Materials::open(&project),
            engineering: engineering::Engineering::open(&project),
            community: community::Community::open(&project),
            event_graph: event_graph::EventGraphPanel::open(&project),
            events_open: false,
            workspace: 0,
            wireframe: false,
            asset_gallery: None,
            graphics: cc.gl.clone(),
            render_debug: render_debug::RenderDebug::default(),
            physics_lab: physics_lab::PhysicsLab::default(),
            provenance: provenance::Provenance::default(),
            client,
            project,
            selected_pilot: pilot,
            console: VecDeque::from([
                "Opening isolated editor world. No public relay connection.".into(),
                "Play runs the simulation. Pause freezes it. Step advances 50 ms.".into(),
            ]),
            last_status: String::new(),
            bottom_tab: 0,
            inspector_tab: 0,
        }
    }
}
impl eframe::App for Editor {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.client.poll();
        self.community.poll();
        let status = match self.client.status() {
            EmbeddedStatus::Loading => "LOADING".into(),
            EmbeddedStatus::Paused => "PAUSED".into(),
            EmbeddedStatus::Running => "PLAYING".into(),
            EmbeddedStatus::Stopped => "STOPPED".into(),
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
        egui::TopBottomPanel::top("editor_toolbar")
            .exact_height(58.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("A R K L I N E")
                            .color(GREEN)
                            .strong()
                            .size(19.0),
                    );
                    ui.label(RichText::new("EDITOR").color(MUTED).size(11.0));
                    ui.separator();
                    if ui
                        .add_enabled(
                            ready && !active && self.workspace == 0,
                            egui::Button::new("▶  Play").min_size(egui::vec2(82., 32.)),
                        )
                        .clicked()
                    {
                        self.client.play();
                    }
                    if ui
                        .add_enabled(
                            active,
                            egui::Button::new("Ⅱ  Pause").min_size(egui::vec2(82., 32.)),
                        )
                        .clicked()
                    {
                        self.client.pause();
                    }
                    if ui
                        .add_enabled(
                            paused && self.workspace == 0,
                            egui::Button::new("Step 50 ms").min_size(egui::vec2(95., 32.)),
                        )
                        .clicked()
                    {
                        self.client.step();
                    }
                    if ui
                        .add_enabled(
                            active || paused,
                            egui::Button::new("■  Stop").min_size(egui::vec2(82., 32.)),
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
        egui::TopBottomPanel::top("workspace_tabs")
            .exact_height(30.)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.workspace, 0, "Ship workspace");
                    if ui
                        .selectable_value(&mut self.workspace, 1, "Community · GitHub")
                        .clicked()
                    {
                        self.client.pause();
                        self.community.ensure_loaded();
                    }
                    if ui
                        .selectable_value(&mut self.workspace, 2, "Physics lab")
                        .clicked()
                    {
                        self.client.pause();
                    }
                    if ui
                        .selectable_value(&mut self.workspace, 3, "Fleet materials")
                        .clicked()
                    {
                        self.client.pause();
                    }
                    if ui
                        .toggle_value(&mut self.events_open, "Event flow")
                        .clicked()
                        && self.events_open
                    {
                        self.workspace = 0;
                    }
                    ui.label(RichText::new("Local sandbox").color(MUTED));
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
        } else {
            egui::TopBottomPanel::bottom("editor_console").resizable(true).default_height(120.).height_range(65. ..=280.).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.bottom_tab,0,"Console");
                ui.selectable_value(&mut self.bottom_tab,1,"Project");
                ui.separator();
                if let Some(world)=&snapshot { ui.label(RichText::new(format!("Simulation time: {} ms",world.server_time_ms)).color(MUTED)); }
            });
            ui.separator();
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
        if self.workspace == 3 {
            self.client.poll_hidden();
            egui::SidePanel::right("fleet_materials_inspector")
                .resizable(true)
                .default_width(310.)
                .width_range(270. ..=430.)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("fleet_materials_scroll")
                        .show(ui, |ui| {
                            self.materials.show(ui, &self.project);
                            self.provenance.show(
                                ui,
                                &self.project,
                                provenance::AssetFile::Material,
                                None,
                            );
                        });
                });
            egui::CentralPanel::default().show(ctx, |ui| {
                self.asset_gallery
                    .get_or_insert_with(|| EmbeddedAssetGallery::new(self.graphics.as_ref()))
                    .show(ui, self.materials.preview());
            });
            self.client.set_material_preview(self.materials.preview());
            ctx.request_repaint_after(Duration::from_millis(33));
            return;
        }
        if self.workspace == 2 {
            self.client.poll_hidden();
            self.physics_lab.show(ctx);
            ctx.request_repaint_after(Duration::from_millis(16));
            return;
        }
        if self.workspace == 1 {
            self.client.poll_hidden();
            self.community.show(ctx, &self.project);
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        egui::SidePanel::left("editor_outliner").resizable(true).default_width(205.).width_range(165. ..=330.).show(ctx, |ui| {
            ui.add_space(10.); ui.heading("World outliner");
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
                if self.engineering.outliner(ui) { self.inspector_tab = 2; }
                ui.add_space(16.); ui.separator(); ui.label(RichText::new("WORKSPACE").color(GREEN).small());
                ui.label("Game viewport"); ui.label("Live systems inspector");
                ui.add_space(8.); ui.small("The game runs inside this window. Editor controls remain outside the viewport.");
            });
        });
        egui::SidePanel::right("editor_inspector")
            .resizable(true)
            .default_width(290.)
            .width_range(245. ..=430.)
            .show(ctx, |ui| {
                ui.add_space(10.);
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.inspector_tab, 0, "Inspector");
                    ui.selectable_value(&mut self.inspector_tab, 1, "Materials");
                    ui.selectable_value(&mut self.inspector_tab, 2, "Engineering");
                    ui.selectable_value(&mut self.inspector_tab, 3, "Render debug");
                });
                if !active {
                    ui.colored_label(MUTED, "Captured state · simulation is not running");
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
        // The real client owns the remaining central viewport and its normal controls.
        self.client.set_debug_wireframe(self.wireframe);
        self.client.set_material_preview(self.materials.preview());
        if self.inspector_tab == 2 {
            self.client.poll_hidden();
            egui::CentralPanel::default().show(ctx, |ui| {
                self.engineering.viewport(ui);
            });
        } else {
            self.client.update(ctx, frame);
        }
        ctx.request_repaint_after(Duration::from_millis(if active { 16 } else { 50 }));
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        self.client.on_exit();
        if let Some(gallery) = &mut self.asset_gallery {
            gallery.destroy();
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
                    "Arkline Editor {}\nUsage: arkline-editor [--project PATH] [--pilot NAME]\nARKLINE_EDITOR_PROJECT sets the default project; --project overrides it. Blank environment values are ignored.\nEmbedded game viewport with local Play, Pause, Step and Stop. No separate client windows or public connections.",
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
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        multisampling: 4,
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
            Ok(Box::new(Editor::new(cc, project, pilot)))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::App;
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
            "isolated test authority did not become ready"
        );
        for workspace in [0, 2, 0] {
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
    fn toolbar_controls_are_visible_above_the_game_viewport() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let project =
            std::env::temp_dir().join(format!("arkline-editor-toolbar-{}", std::process::id()));
        let mut editor = Editor::new(&cc, project, "ToolbarPilot".into());
        for _ in 0..3 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1500., 960.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    editor.update(ctx, &mut eframe::Frame::_new_kittest());
                },
            );
            let mut found = 0;
            for clipped in output.shapes {
                if let egui::Shape::Text(t) = clipped.shape {
                    if ["▶  Play", "Ⅱ  Pause", "Step 50 ms", "■  Stop"].contains(&t.galley.text())
                    {
                        assert!(clipped.clip_rect.contains(t.pos));
                        assert!(t.pos.y < 58.0);
                        found += 1;
                    }
                }
            }
            assert_eq!(found, 4);
        }
        editor.client.on_exit();
    }
}
