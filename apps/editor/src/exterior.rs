//! Project exterior authoring. Disk snapshots and live authority edits are separate actions.
use arkline_client::EmbeddedClient;
use arkline_protocol::{
    default_pioneer_exterior, ExteriorDefinition, ExteriorFixtureKind, ExteriorWorldState,
};
use eframe::egui;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread,
};

const MAX_BYTES: u64 = 64 * 1024;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Save,
    Reload,
}
enum IoRequest {
    Save(PathBuf, ExteriorDefinition),
    Reload(PathBuf),
}
struct IoResult {
    operation: Operation,
    result: Result<ExteriorDefinition, String>,
}
#[derive(Clone, Copy)]
struct PendingIo {
    operation: Operation,
    draft_revision: u64,
}
enum LiveEdit {
    Definition {
        revision: u64,
        document: ExteriorDefinition,
    },
    Enabled(bool),
}
struct PendingLive {
    id: u64,
    edit: LiveEdit,
}

pub struct Exterior {
    draft: ExteriorDefinition,
    saved: ExteriorDefinition,
    draft_revision: u64,
    selected: Option<String>,
    message: String,
    io_requests: Option<SyncSender<IoRequest>>,
    io_results: Receiver<IoResult>,
    pending_io: Option<PendingIo>,
    load_blocked: bool,
    explicit_reset: bool,
    live: Option<ExteriorWorldState>,
    based_on_revision: Option<u64>,
    session_generation: u64,
    pending_live: Option<PendingLive>,
    // An acknowledgement can precede its snapshot. Match the accepted document
    // before advancing the draft base; never render an unconfirmed definition.
    awaiting_definition: Option<(u64, ExteriorDefinition)>,
}

impl Exterior {
    pub fn open(project: &Path) -> Self {
        let (request_tx, request_rx) = mpsc::sync_channel::<IoRequest>(1);
        let (result_tx, result_rx) = mpsc::sync_channel::<IoResult>(1);
        let worker = thread::Builder::new()
            .name("arkline-exterior-io".into())
            .spawn(move || {
                while let Ok(request) = request_rx.recv() {
                    let (operation, result) = match request {
                        IoRequest::Save(project, document) => (
                            Operation::Save,
                            save(&project, &document).map(|()| document),
                        ),
                        IoRequest::Reload(project) => (Operation::Reload, load(&project)),
                    };
                    if result_tx.send(IoResult { operation, result }).is_err() {
                        break;
                    }
                }
            });
        let mut panel = Self {
            draft: default_pioneer_exterior(),
            saved: default_pioneer_exterior(),
            draft_revision: 0,
            selected: None,
            message: "Loading saved exterior definition…".into(),
            io_requests: worker.ok().map(|_| request_tx),
            io_results: result_rx,
            pending_io: None,
            load_blocked: true,
            explicit_reset: false,
            live: None,
            based_on_revision: None,
            session_generation: 0,
            pending_live: None,
            awaiting_definition: None,
        };
        panel.queue(Operation::Reload, IoRequest::Reload(project.to_path_buf()));
        panel
    }

    fn queue(&mut self, operation: Operation, request: IoRequest) {
        if self.pending_io.is_some() {
            self.message = "An exterior file operation is already pending.".into();
            return;
        }
        let Some(sender) = &self.io_requests else {
            self.load_blocked = true;
            self.message = "Exterior file worker unavailable; saving is locked.".into();
            return;
        };
        match sender.try_send(request) {
            Ok(()) => {
                self.pending_io = Some(PendingIo {
                    operation,
                    draft_revision: self.draft_revision,
                });
                self.message = match operation {
                    Operation::Save => "Saving exterior draft…",
                    Operation::Reload => "Loading exterior draft…",
                }
                .into();
            }
            Err(TrySendError::Full(_)) => {
                self.message = "Exterior file queue is full; wait and retry.".into();
            }
            Err(TrySendError::Disconnected(_)) => {
                self.load_blocked = true;
                self.message = "Exterior file worker stopped; saving is locked.".into();
            }
        }
    }

    fn poll_io(&mut self) {
        let Some(pending) = self.pending_io else {
            return;
        };
        match self.io_results.try_recv() {
            Ok(IoResult { operation, result }) => {
                self.pending_io = None;
                if operation != pending.operation {
                    self.load_blocked = true;
                    self.message = "Exterior file result mismatch; saving is locked.".into();
                    return;
                }
                match (operation, result) {
                    (Operation::Save, Ok(document)) => {
                        self.saved = document;
                        self.explicit_reset = false;
                        self.load_blocked = false;
                        self.message = if self.draft_revision == pending.draft_revision {
                            "Saved Assets/Pioneer.exterior.json. Apply is a separate live action."
                        } else {
                            "Saved the submitted snapshot; newer draft edits remain unsaved."
                        }
                        .into();
                    }
                    (Operation::Reload, Ok(document)) => {
                        if self.draft_revision == pending.draft_revision {
                            self.draft = document.clone();
                            self.selected = None;
                            self.based_on_revision = self.live.as_ref().and_then(|live| {
                                (live.definition.document == document)
                                    .then_some(live.definition.revision)
                            });
                            self.message =
                                "Reloaded project exterior draft. Live state is unchanged.".into();
                        } else {
                            self.message = "Reload completed; newer local edits were kept.".into();
                        }
                        self.saved = document;
                        self.explicit_reset = false;
                        self.load_blocked = false;
                    }
                    (Operation::Save, Err(error)) => {
                        self.message =
                            format!("Save failed: {error}. Draft and live state were kept.");
                    }
                    (Operation::Reload, Err(error)) => {
                        self.load_blocked = true;
                        self.explicit_reset = false;
                        self.message = format!("Reload failed: {error}. Saving is locked until a successful reload or explicit reset.");
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.pending_io = None;
                self.load_blocked = true;
                self.message = "Exterior file worker stopped; saving is locked.".into();
            }
        }
    }

    fn reset_after_load_failure(&mut self) {
        self.draft = default_pioneer_exterior();
        self.draft_revision = self.draft_revision.saturating_add(1);
        self.selected = None;
        self.based_on_revision = self.live.as_ref().and_then(|live| {
            (live.definition.document == self.draft).then_some(live.definition.revision)
        });
        self.load_blocked = false;
        self.explicit_reset = true;
        self.message = "Reset explicitly confirmed. Save may replace the unreadable project file; live state is unchanged.".into();
    }

    fn can_save(&self) -> bool {
        self.pending_io.is_none() && !self.load_blocked && self.draft.validate().is_ok()
    }

    fn observe_live(&mut self, state: ExteriorWorldState) {
        let follows_previous = self
            .live
            .as_ref()
            .is_some_and(|previous| self.draft == previous.definition.document);
        let accepted_submission =
            self.awaiting_definition
                .as_ref()
                .is_some_and(|(revision, document)| {
                    state.definition.revision >= *revision && document == &state.definition.document
                });
        let superseded_submission = self
            .awaiting_definition
            .as_ref()
            .and_then(|(revision, _)| (state.definition.revision > *revision).then_some(*revision));
        if accepted_submission {
            self.based_on_revision = Some(state.definition.revision);
            self.awaiting_definition = None;
        } else if let Some(revision) = superseded_submission {
            self.based_on_revision = Some(revision);
            self.awaiting_definition = None;
            self.message = "Applied definition was superseded by a newer authority revision. Local edits were kept; reconcile the current live definition before applying again.".into();
        } else if self.draft == state.definition.document {
            self.based_on_revision = Some(state.definition.revision);
        } else if follows_previous && self.pending_io.is_none() && !self.load_blocked {
            self.draft = state.definition.document.clone();
            self.draft_revision = self.draft_revision.saturating_add(1);
            self.based_on_revision = Some(state.definition.revision);
        }
        self.live = Some(state);
    }

    fn load_live_draft(&mut self) {
        if let Some(live) = &self.live {
            self.draft = live.definition.document.clone();
            self.draft_revision = self.draft_revision.saturating_add(1);
            self.based_on_revision = Some(live.definition.revision);
            self.selected = None;
            self.message =
                "Loaded confirmed live definition into the draft. Save changes the project file."
                    .into();
        }
    }

    fn rebase_draft(&mut self) {
        if let Some(live) = &self.live {
            self.based_on_revision = Some(live.definition.revision);
            self.message = "Draft kept and based on the current confirmed revision. Apply will explicitly replace that live definition.".into();
        }
    }

    fn reset_session(&mut self, generation: u64) {
        if self.session_generation != generation {
            if self.pending_live.is_some() || self.awaiting_definition.is_some() {
                self.message = "Session ended before live confirmation. Its outcome is unknown; inspect confirmed state after reconnecting. No exterior edit will replay.".into();
            }
            self.session_generation = generation;
            self.live = None;
            self.based_on_revision = None;
            self.pending_live = None;
            self.awaiting_definition = None;
        }
    }

    fn finish_live(&mut self, id: u64, result: &Result<(), String>) {
        if self
            .pending_live
            .as_ref()
            .is_none_or(|pending| pending.id != id)
        {
            return;
        }
        let pending = self.pending_live.take().unwrap();
        match result {
            Ok(()) => match pending.edit {
                LiveEdit::Definition { revision, document } => {
                    self.awaiting_definition =
                        revision.checked_add(1).map(|revision| (revision, document));
                    self.message = "Authority accepted the exterior definition; awaiting confirmed snapshot. Project save remains separate.".into();
                }
                LiveEdit::Enabled(enabled) => {
                    self.message = format!("Authority accepted exterior {}. Power and drive state follow the confirmed snapshot.", if enabled { "ON" } else { "OFF" });
                }
            },
            Err(error) => {
                self.message = format!("Live apply was not confirmed: {error}. Local edits and the accepted live definition were kept.");
            }
        }
    }

    /// Poll even while hidden. Neither file completion nor session changes issue commands.
    pub fn poll_live(&mut self, client: &mut EmbeddedClient) {
        self.reset_session(client.session_generation());
        self.poll_io();
        if let Some(ack) = client.exterior_ack() {
            self.finish_live(ack.id, &ack.result);
        }
        if self.pending_live.is_some() && !client.ready() {
            self.pending_live = None;
            self.message = "Disconnected before live confirmation; outcome unknown. Inspect the authority after reconnecting before retrying.".into();
        }
        if let Some(state) = client.snapshot().and_then(|world| world.exterior.clone()) {
            self.observe_live(state);
        } else {
            self.live = None;
            self.based_on_revision = None;
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, project: &Path, client: &mut EmbeddedClient) {
        self.poll_live(client);
        let before = self.draft.clone();
        ui.push_id("pioneer_exterior", |ui| {
            ui.heading("Pioneer exterior");
            ui.small("Editable fixture mounts · ship-local meters · +X forward · canonical hull 72 × 18 × 38 m (X/Y/Z)");
            ui.small("Save / Reload edit this project's file. Apply changes the shared Pioneer definition after authority approval.");
            self.show_power(ui, client);
            if let Some(live) = &self.live {
                ui.label(format!("Confirmed definition revision {} · draft base {}", live.definition.revision,
                    self.based_on_revision.map_or_else(|| "unlinked".into(), |revision| revision.to_string())));
                if self.based_on_revision != Some(live.definition.revision) {
                    ui.colored_label(egui::Color32::YELLOW, "This draft is unlinked or stale. Load live, or explicitly rebase your retained edits, before applying.");
                }
            } else {
                ui.colored_label(egui::Color32::YELLOW, "Exterior authority support is unavailable. Project authoring remains available.");
            }
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(self.can_save(), egui::Button::new("Save draft")).clicked() {
                    self.queue(Operation::Save, IoRequest::Save(project.to_path_buf(), self.draft.clone()));
                }
                if ui.add_enabled(self.pending_io.is_none(), egui::Button::new("Reload saved")).clicked() {
                    self.queue(Operation::Reload, IoRequest::Reload(project.to_path_buf()));
                }
                if ui.add_enabled(self.live.is_some() && self.pending_live.is_none() && self.awaiting_definition.is_none(), egui::Button::new("Load live (replace draft)")).clicked() {
                    self.load_live_draft();
                }
                let stale = self.live.as_ref().is_some_and(|live| self.based_on_revision != Some(live.definition.revision));
                if ui.add_enabled(stale && self.pending_live.is_none() && self.awaiting_definition.is_none(), egui::Button::new("Rebase draft (keep edits)"))
                    .on_hover_text("Keep this draft and accept the current confirmed revision as its base. Apply will replace that live definition.")
                    .clicked() {
                    self.rebase_draft();
                }
                // Only the confirmed snapshot advertises protocol support.
                if let Some(live) = &self.live {
                    let revision = live.definition.revision;
                    let can_apply = client.ready() && self.pending_live.is_none()
                        && self.awaiting_definition.is_none() && self.draft.validate().is_ok()
                        && self.based_on_revision == Some(revision);
                    if ui.add_enabled(can_apply, egui::Button::new("Apply to authority")).clicked() {
                        let document = self.draft.clone();
                        match client.exterior_replace(revision, document.clone()) {
                            Ok(id) => {
                                self.pending_live = Some(PendingLive { id, edit: LiveEdit::Definition { revision, document } });
                                self.message = "Waiting for authority acknowledgement…".into();
                            }
                            Err(error) => self.message = format!("Apply could not be queued: {error}. Draft kept."),
                        }
                    }
                }
            });
            if self.load_blocked {
                ui.colored_label(egui::Color32::LIGHT_RED, "Project file not loaded. Save is locked to protect unreadable content.");
                if ui.add_enabled(self.pending_io.is_none(), egui::Button::new("Reset to runtime defaults; allow file replacement")).clicked() {
                    self.reset_after_load_failure();
                }
            }
            ui.small(if self.draft == self.saved && !self.explicit_reset { "Project draft saved" } else { "Unsaved project changes" });
            ui.small(format!("Candidate exterior load: {} W · combined room/exterior gameplay budget: 1200 W", self.draft.load_w()));
            if let Err(error) = self.draft.validate() {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            ui.small(format!("Hull binding: {}", self.draft.hull_asset_sha256));
            ui.small("A different hull hash requires a reviewed definition; this panel preserves fixture IDs and the existing hull binding.");
            ui.separator();
            egui::ScrollArea::vertical().id_salt("fixtures").max_height(140.0).show(ui, |ui| {
                for fixture in &self.draft.fixtures {
                    ui.push_id(&fixture.id, |ui| {
                        if ui.selectable_label(self.selected.as_deref() == Some(fixture.id.as_str()), format!("{} · {:?}", fixture.id, fixture.kind)).clicked() {
                            self.selected = Some(fixture.id.clone());
                        }
                    });
                }
            });
            if let Some(fixture) = self.selected.as_ref().and_then(|id| self.draft.fixtures.iter_mut().find(|fixture| &fixture.id == id)) {
                let fixture_id = fixture.id.clone();
                ui.push_id((&fixture_id, "fields"), |ui| {
                    ui.strong(format!("{} · {:?}", fixture.id, fixture.kind));
                    ui.label("Mount position (m)");
                    ui.horizontal_wrapped(|ui| {
                        for (axis, value) in fixture.position_m.iter_mut().enumerate() {
                            ui.push_id(("mount", axis), |ui| {
                                ui.label(["X", "Y", "Z"][axis]);
                                ui.add(egui::DragValue::new(value).speed(0.1).range(-[38.0, 11.0, 21.0][axis]..=[38.0, 11.0, 21.0][axis]));
                            });
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("RGB color");
                        ui.push_id("color", |ui| { ui.color_edit_button_rgb(&mut fixture.color); });
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Emission");
                        ui.push_id("emission", |ui| { ui.add(egui::DragValue::new(&mut fixture.emission).speed(0.1).range(0.0..=16.0)); });
                        ui.label("Load (W)");
                        ui.push_id("load", |ui| { ui.add(egui::DragValue::new(&mut fixture.load_w).speed(1.0).range(0..=if fixture.kind == ExteriorFixtureKind::EngineGlow { 0 } else { 1066 })); });
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Local light range (m)");
                        ui.push_id("range", |ui| { ui.add(egui::DragValue::new(&mut fixture.light_range_m).speed(0.1).range(0.0..=30.0)); });
                        ui.label("Intensity");
                        ui.push_id("intensity", |ui| { ui.add(egui::DragValue::new(&mut fixture.light_intensity).speed(0.1).range(0.0..=8.0)); });
                    });
                    ui.small("Emission makes the fixture visible. Local light range/intensity illuminate nearby hull pixels.");
                    // Fixture kind and identity remain stable; timing is editable only for existing strobes.
                    if fixture.kind == ExteriorFixtureKind::Strobe {
                        ui.strong("Timed strobe · simulation clock");
                    }
                    if let Some(pulse) = &mut fixture.pulse {
                        ui.horizontal_wrapped(|ui| {
                            for (name, value) in [("Period (ms)", &mut pulse.period_ms), ("On (ms)", &mut pulse.on_ms), ("Phase (ms)", &mut pulse.phase_ms)] {
                                ui.push_id(name, |ui| { ui.label(name); ui.add(egui::DragValue::new(value).speed(10.0)); });
                            }
                        });
                        ui.small("Pause freezes pulse time. Step advances the authority simulation by 50 ms.");
                    }
                });
            } else {
                ui.label("Select a fixture to edit its mount, color, emission, local light, load and timing.");
            }
            ui.separator();
            ui.label(&self.message);
        });
        if before != self.draft {
            self.draft_revision = self.draft_revision.saturating_add(1);
        }
    }

    fn show_power(&mut self, ui: &mut egui::Ui, client: &mut EmbeddedClient) {
        let Some(live) = &self.live else {
            return;
        };
        let Some(vessel) = live
            .vessels
            .iter()
            .find(|vessel| vessel.pilot == client.pilot())
        else {
            ui.small("No supported exterior vessel for this pilot.");
            return;
        };
        ui.label(format!(
            "Exterior {} · supply {} · {} W · Drive {}",
            if vessel.enabled { "ON" } else { "OFF" },
            if vessel.powered {
                "POWERED"
            } else {
                "UNPOWERED"
            },
            vessel.load_w,
            format!("{:?}", vessel.drive).to_uppercase()
        ));
        ui.small("Exterior electrical power and drive OFF / IDLE / ACTIVE are independent confirmed states.");
        let enabled = !vessel.enabled;
        let revision = live.definition.revision;
        if ui
            .add_enabled(
                client.ready() && self.pending_live.is_none() && self.awaiting_definition.is_none(),
                egui::Button::new(if enabled {
                    "Turn exterior ON"
                } else {
                    "Turn exterior OFF"
                }),
            )
            .clicked()
        {
            match client.exterior_set_enabled(enabled, revision) {
                Ok(id) => {
                    self.pending_live = Some(PendingLive {
                        id,
                        edit: LiveEdit::Enabled(enabled),
                    });
                    self.message = "Waiting for exterior power acknowledgement…".into();
                }
                Err(error) => {
                    self.message = format!("Exterior power command could not be queued: {error}")
                }
            }
        }
    }
}

fn ordinary_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Cannot inspect exterior directory: {error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Exterior directories must be ordinary directories, not symbolic links".into());
    }
    Ok(())
}
fn document_path(project: &Path, create: bool) -> Result<PathBuf, String> {
    ordinary_directory(project)?;
    let assets = project.join("Assets");
    if create {
        match fs::create_dir(&assets) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("Cannot create Assets directory: {error}")),
        }
    } else if fs::symlink_metadata(&assets)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(assets.join("Pioneer.exterior.json"));
    }
    ordinary_directory(&assets)?;
    let path = assets.join("Pioneer.exterior.json");
    match fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err("Exterior destination must be an ordinary file, not a symbolic link".into())
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Cannot inspect exterior file: {error}")),
    }
    Ok(path)
}
fn load(project: &Path) -> Result<ExteriorDefinition, String> {
    let file = match fs::File::open(document_path(project, false)?) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(default_pioneer_exterior())
        }
        Err(error) => return Err(format!("Cannot open exterior definition: {error}")),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read exterior definition: {error}"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Exterior definition exceeds 64 KiB".into());
    }
    let document: ExteriorDefinition = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid exterior JSON: {error}"))?;
    document.validate()?;
    Ok(document)
}
fn save(project: &Path, document: &ExteriorDefinition) -> Result<(), String> {
    document.validate()?;
    let bytes = serde_json::to_vec_pretty(document)
        .map_err(|error| format!("Cannot encode exterior definition: {error}"))?;
    if bytes.len() as u64 + 1 > MAX_BYTES {
        return Err("Exterior definition exceeds 64 KiB".into());
    }
    let path = document_path(project, true)?;
    let parent = path.parent().ok_or("Exterior parent unavailable")?;
    for _ in 0..32 {
        let temp = parent.join(format!(
            ".Pioneer.exterior.{}.{}.tmp",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = match options.open(&temp) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Cannot create exterior temporary file: {error}")),
        };
        let written = file
            .write_all(&bytes)
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.sync_all());
        drop(file);
        if let Err(error) = written.and_then(|_| fs::rename(&temp, &path)) {
            let _ = fs::remove_file(&temp);
            return Err(format!("Cannot save exterior definition: {error}"));
        }
        #[cfg(unix)]
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        return Ok(());
    }
    Err("Cannot allocate an exterior temporary file".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arkline_protocol::ExteriorDefinitionState;

    struct Project(PathBuf);
    impl Project {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "arkline-exterior-editor-{}-{}",
                std::process::id(),
                TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Project {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn panel() -> (Exterior, SyncSender<IoResult>) {
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        (
            Exterior {
                draft: default_pioneer_exterior(),
                saved: default_pioneer_exterior(),
                draft_revision: 0,
                selected: None,
                message: String::new(),
                io_requests: None,
                io_results: result_rx,
                pending_io: None,
                load_blocked: false,
                explicit_reset: false,
                live: None,
                based_on_revision: None,
                session_generation: 0,
                pending_live: None,
                awaiting_definition: None,
            },
            result_tx,
        )
    }
    fn state(revision: u64, document: ExteriorDefinition) -> ExteriorWorldState {
        ExteriorWorldState {
            definition: ExteriorDefinitionState { revision, document },
            vessels: vec![],
        }
    }

    #[test]
    fn selected_fixture_color_widget_preserves_draft_without_input() {
        let project = Project::new();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut client = EmbeddedClient::new(
            &cc,
            project.0.join(".arkline-studio"),
            "ExteriorIdleColorPilot".into(),
        );
        let (mut panel, _results) = panel();
        panel.selected = Some(panel.draft.fixtures[0].id.clone());
        let original = panel.draft.clone();
        for _ in 0..2 {
            let _output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1500.0, 960.0),
                    )),
                    events: vec![],
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| panel.show(ui, &project.0, &mut client));
                },
            );
        }
        client.on_exit();
        assert_eq!(
            panel.draft.fixtures[0].color.map(f32::to_bits),
            original.fixtures[0].color.map(f32::to_bits),
            "merely displaying the selected fixture changed its authored RGB color: {:?} -> {:?}",
            original.fixtures[0].color,
            panel.draft.fixtures[0].color,
        );
        assert_eq!(panel.draft, original);
        assert_eq!(panel.draft_revision, 0);
        assert_eq!(panel.draft, panel.saved);
    }

    #[test]
    fn published_fixture_document_matches_runtime_default() {
        let document: ExteriorDefinition = serde_json::from_str(include_str!(
            "../../../assets/ships/pioneer/exterior-v1.json"
        ))
        .unwrap();
        document.validate().unwrap();
        assert_eq!(document, default_pioneer_exterior());
        assert_eq!(document.load_w(), 88);
    }

    #[test]
    fn project_roundtrip_preserves_fixture_identity_and_hull_binding() {
        let project = Project::new();
        let mut document = load(&project.0).unwrap();
        let original = document.clone();
        document.fixtures[0].position_m[1] += 0.25;
        document.fixtures[0].color = [0.8, 0.2, 0.1];
        save(&project.0, &document).unwrap();
        assert_eq!(load(&project.0).unwrap(), document);
        assert_eq!(document.hull_asset_sha256, original.hull_asset_sha256);
        assert_eq!(document.fixtures[0].id, original.fixtures[0].id);
        let mut entries = fs::read_dir(project.0.join("Assets")).unwrap();
        assert_eq!(
            entries.next().unwrap().unwrap().file_name(),
            "Pioneer.exterior.json"
        );
        assert!(
            entries.next().is_none(),
            "atomic save must remove temporary files"
        );
    }

    #[test]
    fn invalid_and_oversized_files_are_not_silently_replaced() {
        let project = Project::new();
        save(&project.0, &default_pioneer_exterior()).unwrap();
        let path = document_path(&project.0, false).unwrap();
        fs::write(&path, b"{unreadable").unwrap();
        assert!(load(&project.0).is_err());
        let (mut panel, result) = panel();
        panel.pending_io = Some(PendingIo {
            operation: Operation::Reload,
            draft_revision: 0,
        });
        result
            .send(IoResult {
                operation: Operation::Reload,
                result: load(&project.0),
            })
            .unwrap();
        panel.poll_io();
        assert!(panel.load_blocked);
        assert!(!panel.can_save());
        assert_eq!(fs::read(&path).unwrap(), b"{unreadable");
        panel.reset_after_load_failure();
        assert!(panel.can_save());
        assert!(panel.explicit_reset);
        // Reset unlocks the action but does not write without an explicit Save.
        assert_eq!(fs::read(&path).unwrap(), b"{unreadable");
        fs::write(&path, vec![b' '; MAX_BYTES as usize + 1]).unwrap();
        assert!(load(&project.0).unwrap_err().contains("64 KiB"));
    }

    #[test]
    fn failed_save_and_apply_preserve_newer_draft_and_confirmed_definition() {
        let (mut panel, result) = panel();
        panel.observe_live(state(4, default_pioneer_exterior()));
        let accepted = panel.live.clone();
        let saved = panel.saved.clone();
        panel.draft.fixtures[0].emission = 4.0;
        panel.draft_revision = 1;
        panel.pending_io = Some(PendingIo {
            operation: Operation::Save,
            draft_revision: 1,
        });
        let submitted = panel.draft.clone();
        panel.pending_live = Some(PendingLive {
            id: 10,
            edit: LiveEdit::Definition {
                revision: 4,
                document: submitted,
            },
        });
        panel.draft.fixtures[1].emission = 5.0;
        panel.draft_revision = 2;
        let newer = panel.draft.clone();
        result
            .send(IoResult {
                operation: Operation::Save,
                result: Err("disk full".into()),
            })
            .unwrap();
        panel.poll_io();
        panel.finish_live(10, &Err("stale revision".into()));
        assert_eq!(panel.draft, newer);
        assert_eq!(panel.saved, saved);
        assert_eq!(panel.live, accepted);
        assert_eq!(panel.based_on_revision, Some(4));
        assert!(panel.pending_live.is_none());
        assert!(panel.awaiting_definition.is_none());
    }

    #[test]
    fn successful_save_keeps_edits_made_during_disk_write_and_never_queues_apply() {
        let (mut panel, result) = panel();
        let submitted = panel.draft.clone();
        panel.pending_io = Some(PendingIo {
            operation: Operation::Save,
            draft_revision: 0,
        });
        panel.draft.fixtures[0].emission = 7.0;
        panel.draft_revision = 1;
        result
            .send(IoResult {
                operation: Operation::Save,
                result: Ok(submitted.clone()),
            })
            .unwrap();
        panel.poll_io();
        assert_eq!(panel.saved, submitted);
        assert_eq!(panel.draft.fixtures[0].emission, 7.0);
        assert!(panel.pending_live.is_none());
        assert!(panel.awaiting_definition.is_none());
    }

    #[test]
    fn accepted_apply_preserves_newer_edits_and_waits_for_confirmed_snapshot() {
        let (mut panel, _) = panel();
        panel.observe_live(state(0, default_pioneer_exterior()));
        panel.draft.fixtures[0].emission = 3.0;
        let submitted = panel.draft.clone();
        panel.pending_live = Some(PendingLive {
            id: 1,
            edit: LiveEdit::Definition {
                revision: 0,
                document: submitted.clone(),
            },
        });
        panel.draft.fixtures[1].emission = 5.0;
        let newer = panel.draft.clone();
        panel.finish_live(1, &Ok(()));
        assert_eq!(panel.live.as_ref().unwrap().definition.revision, 0);
        assert_eq!(panel.based_on_revision, Some(0));
        panel.observe_live(state(1, submitted));
        assert_eq!(panel.draft, newer);
        assert_eq!(panel.based_on_revision, Some(1));
        assert!(panel.awaiting_definition.is_none());
    }

    #[test]
    fn peer_changes_refresh_clean_drafts_and_require_reconciliation_for_local_edits() {
        let (mut panel, _) = panel();
        panel.observe_live(state(0, default_pioneer_exterior()));
        let mut peer = default_pioneer_exterior();
        peer.fixtures[0].emission = 4.0;
        panel.observe_live(state(1, peer.clone()));
        assert_eq!(panel.draft, peer);
        panel.draft.fixtures[1].emission = 6.0;
        let local = panel.draft.clone();
        peer.fixtures[2].emission = 2.5;
        panel.observe_live(state(2, peer.clone()));
        assert_eq!(panel.draft, local);
        assert_eq!(panel.based_on_revision, Some(1));
        panel.load_live_draft();
        assert_eq!(panel.draft, peer);
        assert_eq!(panel.based_on_revision, Some(2));
    }

    #[test]
    fn unchanged_apply_does_not_confirm_against_the_prior_snapshot() {
        let (mut panel, _) = panel();
        let document = default_pioneer_exterior();
        panel.observe_live(state(4, document.clone()));
        panel.pending_live = Some(PendingLive {
            id: 1,
            edit: LiveEdit::Definition {
                revision: 4,
                document: document.clone(),
            },
        });
        panel.finish_live(1, &Ok(()));
        panel.observe_live(state(4, document.clone()));
        assert!(panel.awaiting_definition.is_some());
        panel.observe_live(state(5, document));
        assert!(panel.awaiting_definition.is_none());
        assert_eq!(panel.based_on_revision, Some(5));
    }

    #[test]
    fn superseded_apply_unlocks_reconciliation_without_adopting_peer_edits() {
        let (mut panel, _) = panel();
        panel.observe_live(state(4, default_pioneer_exterior()));
        panel.draft.fixtures[0].emission = 7.0;
        panel.pending_live = Some(PendingLive {
            id: 1,
            edit: LiveEdit::Definition {
                revision: 4,
                document: panel.draft.clone(),
            },
        });
        panel.finish_live(1, &Ok(()));
        let local = panel.draft.clone();
        let mut peer = default_pioneer_exterior();
        peer.fixtures[1].emission = 6.0;
        panel.observe_live(state(6, peer));
        assert!(panel.awaiting_definition.is_none());
        assert_eq!(panel.based_on_revision, Some(5));
        assert_eq!(panel.draft, local);
        assert!(panel.message.contains("superseded"));
    }

    #[test]
    fn explicit_rebase_keeps_saved_candidate_without_installing_it_live() {
        let (mut panel, _) = panel();
        panel.observe_live(state(5, default_pioneer_exterior()));
        panel.draft.fixtures[0].emission = 8.0;
        let candidate = panel.draft.clone();
        panel.based_on_revision = None;
        panel.rebase_draft();
        assert_eq!(panel.draft, candidate);
        assert_eq!(panel.based_on_revision, Some(5));
        assert_ne!(panel.live.as_ref().unwrap().definition.document, candidate);
        assert!(panel.pending_live.is_none());
    }

    #[test]
    fn reconnect_cancels_pending_mutation_and_save_completion_cannot_replay_it() {
        let (mut panel, result) = panel();
        panel.observe_live(state(8, default_pioneer_exterior()));
        panel.pending_live = Some(PendingLive {
            id: 3,
            edit: LiveEdit::Definition {
                revision: 8,
                document: panel.draft.clone(),
            },
        });
        panel.pending_io = Some(PendingIo {
            operation: Operation::Save,
            draft_revision: 0,
        });
        panel.reset_session(1);
        assert!(panel.message.contains("No exterior edit will replay"));
        result
            .send(IoResult {
                operation: Operation::Save,
                result: Ok(panel.draft.clone()),
            })
            .unwrap();
        panel.poll_io();
        panel.finish_live(3, &Ok(()));
        assert!(panel.pending_live.is_none());
        assert!(panel.awaiting_definition.is_none());
        assert!(panel.live.is_none());
        assert!(panel.based_on_revision.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_destinations_are_rejected_without_changing_the_target() {
        use std::os::unix::fs::symlink;
        let project = Project::new();
        let target = project.0.join("unrelated.json");
        fs::write(&target, b"keep me").unwrap();
        fs::create_dir(project.0.join("Assets")).unwrap();
        symlink(&target, project.0.join("Assets/Pioneer.exterior.json")).unwrap();
        assert!(load(&project.0).is_err());
        assert!(save(&project.0, &default_pioneer_exterior()).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"keep me");
    }
}
