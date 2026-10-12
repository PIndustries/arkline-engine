//! One-window local development workspace using the real game client and authority.
mod asset_document;
mod inspector;
mod materials;
use arkline_client::{EmbeddedClient, EmbeddedStatus};
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
}
impl Editor {
    fn new(cc: &eframe::CreationContext<'_>, project: PathBuf, pilot: String) -> Self {
        let client = EmbeddedClient::new(cc, project.join(".arkline-studio"), pilot.clone());
        Self {
            materials: materials::Materials::open(&project),
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
                            ready && !active,
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
                            paused,
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
                    ui.colored_label(MUTED,"This build embeds gameplay and live inspection. Ship, room and material authoring are not available yet.");
                }
            });
        });
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
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.inspector_tab, 0, "Inspector");
                    ui.selectable_value(&mut self.inspector_tab, 1, "Materials");
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
        self.client.set_material_preview(self.materials.preview());
        self.client.update(ctx, frame);
        ctx.request_repaint_after(Duration::from_millis(if active { 16 } else { 50 }));
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        self.client.on_exit();
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
    let mut pilot = "EditorPilot".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("Arkline Editor {}\nUsage: arkline-editor [--project PATH] [--pilot NAME]\nEmbedded game viewport with local Play, Pause, Step and Stop. No separate client windows or public connections.",BUILD.trim());
                return Ok(());
            }
            "--project" => {
                project = args.next().map(PathBuf::from).unwrap_or_else(|| {
                    eprintln!("--project requires PATH");
                    std::process::exit(2)
                })
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
