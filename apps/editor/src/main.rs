//! Native development launcher. This is not a ship/scene authoring tool.
mod runtime;
use eframe::egui;
use runtime::{Action, Configuration, Event, Runtime};
use std::{collections::VecDeque, path::PathBuf, time::Duration};
const BUILD: &str = include_str!("../../../BUILD_VERSION");
struct Editor {
    runtime: Runtime,
    project: String,
    binaries: String,
    pilot: String,
    status: String,
    address: Option<std::net::SocketAddr>,
    busy: bool,
    clients: usize,
    error: bool,
    logs: VecDeque<String>,
}
impl Editor {
    fn new(project: PathBuf, binaries: PathBuf) -> Self {
        Self {
            runtime: Runtime::default(),
            project: project.display().to_string(),
            binaries: binaries.display().to_string(),
            pilot: "EditorPilot".into(),
            status: "Stopped".into(),
            address: None,
            busy: false,
            clients: 0,
            error: false,
            logs: VecDeque::new(),
        }
    }
    fn poll(&mut self) {
        while let Ok(event) = self.runtime.events.try_recv() {
            match event {
                Event::Starting => {
                    self.error = false;
                    self.busy = true;
                    self.status = "Starting isolated local server…".into();
                }
                Event::Ready(addr) => {
                    self.error = false;
                    self.address = Some(addr);
                    self.status = format!("LOCAL ONLY · {addr}");
                }
                Event::ClientCount(n) => self.clients = n,
                Event::Stopped => {
                    self.busy = false;
                    self.address = None;
                    self.status = "Stopped".into();
                }
                Event::Error(error) => {
                    self.error = true;
                    self.status = error.clone();
                    runtime::append_log(&mut self.logs, error);
                }
            }
        }
        for _ in 0..256 {
            let Ok(line) = self.runtime.logs.try_recv() else {
                break;
            };
            runtime::append_log(&mut self.logs, line);
        }
    }
}
impl eframe::App for Editor {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.poll();
        ctx.request_repaint_after(Duration::from_millis(100));
        egui::CentralPanel::default().show(ctx,|ui|{
            ui.heading(format!("ARKLINE EDITOR · {}",BUILD.trim()));
            ui.label("Local development launcher · real server and client · no production connection controls");
            ui.separator();
            ui.add_enabled_ui(!self.busy,|ui|{
                ui.label("Project folder (world and pilot credentials stay in .arkline-editor)");ui.text_edit_singleline(&mut self.project);
                ui.label("Directory containing arkline-server and arkline-client");ui.text_edit_singleline(&mut self.binaries);
            });
            ui.horizontal(|ui|{
                if ui.add_enabled(!self.busy,egui::Button::new("Start local world")).clicked(){self.busy=true;self.status="Starting…".into();self.runtime.send(Action::Start(Configuration{project:self.project.clone().into(),binaries:self.binaries.clone().into()}));}
                if ui.add_enabled(self.busy,egui::Button::new("Stop world and test clients")).clicked(){self.runtime.send(Action::Stop);self.status="Stopping owned processes…".into();}
            });
            ui.colored_label(if self.error { egui::Color32::from_rgb(255,190,90) } else { egui::Color32::from_rgb(100,225,165) },&self.status);
            ui.separator();ui.horizontal(|ui|{ui.label("Test pilot");ui.text_edit_singleline(&mut self.pilot);});
            ui.horizontal(|ui|{if ui.add_enabled(self.address.is_some()&&self.clients<8,egui::Button::new("Launch real client")).clicked(){self.runtime.send(Action::Launch(self.pilot.trim().into()));}ui.label(format!("{} owned test clients",self.clients));});
            ui.small("Existing pilots require their saved resume credentials. New names register through normal server authentication. Restarting this project preserves its world and credential scope.");
            ui.small("Closing the editor stops only its own server and clients. Save work in a client before stopping. No production worlds, accounts or external game processes are managed here.");
            ui.separator();ui.strong("Process logs (last 200 lines)");
            egui::ScrollArea::vertical().stick_to_bottom(true).show(ui,|ui|{for line in &self.logs{ui.monospace(line);}});
        });
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        self.runtime.shutdown();
    }
}
fn main() -> eframe::Result<()> {
    let mut project = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("data/editor-project");
    let mut binaries = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("target/debug"));
    #[cfg(target_os = "macos")]
    if binaries.ends_with("Contents/MacOS") {
        if let Some(home) = std::env::var_os("HOME") {
            project =
                PathBuf::from(home).join("Library/Application Support/Arkline/Editor/FirstProject");
        }
    }
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("Arkline Editor {}\nUsage: arkline-editor [--project PATH] [--bin-dir PATH]\nStarts only explicit loopback local-development servers. Server/client binaries must be colocated or selected using --bin-dir. No production access, scene authoring, or credential bypass.",BUILD.trim());
                return Ok(());
            }
            "--project" | "--bin-dir" => {
                let Some(value) = args.next() else {
                    eprintln!("{arg} requires a path");
                    std::process::exit(2);
                };
                if arg == "--project" {
                    project = value.into();
                } else {
                    binaries = value.into();
                }
            }
            _ => {
                eprintln!("Unknown argument: {arg}");
                std::process::exit(2);
            }
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("Arkline Editor — {}", BUILD.trim()))
            .with_inner_size([850., 620.])
            .with_min_inner_size([680., 480.]),
        ..Default::default()
    };
    eframe::run_native(
        "Arkline Editor",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(Editor::new(project, binaries)))
        }),
    )
}
