//! Unified inspection catalog. Browsing never accepts, saves or applies an asset.
use crate::import_document::Verified;
use crate::imports::{BrowserImportSnapshot, Imports};
use arkline_client::{
    load_runtime_model, runtime_model_catalog, ModelGeometryInfo, ModelPreview,
    RuntimeInspectionModel, RuntimeModelDescriptor,
};
use eframe::{egui, glow};
use std::{
    collections::{BTreeSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, Receiver, SyncSender, TryRecvError},
        Arc,
    },
};
#[path = "model_browser_sources.rs"]
mod sources;

#[derive(Clone)]
enum Source {
    Runtime(String),
    File(sources::SourceFile),
    Verified(Verified),
    Snapshot(Arc<Vec<u8>>),
}
#[derive(Clone)]
struct Entry {
    id: String,
    name: String,
    category: String,
    source: String,
    identity: String,
    notes: Vec<String>,
    unavailable: Option<String>,
    spec: Option<[f64; 3]>,
    model: Source,
}
impl Entry {
    fn matches(&self, search: &str, category: Option<&str>) -> bool {
        category.is_none_or(|category| category == self.category)
            && (search.is_empty()
                || [&self.name, &self.category, &self.source, &self.identity]
                    .iter()
                    .any(|text| text.to_lowercase().contains(search)))
    }
}
#[derive(Clone)]
enum LoadedModel {
    Runtime(Arc<RuntimeInspectionModel>),
    Imported(Verified),
}
impl LoadedModel {
    fn info(&self) -> ModelGeometryInfo {
        match self {
            Self::Runtime(model) => model.info.clone(),
            Self::Imported(verified) => {
                let report = &verified.geometry.report;
                ModelGeometryInfo {
                    min_m: report.min_m,
                    max_m: report.max_m,
                    triangles: report.triangles,
                    batches: report.primitives,
                }
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
enum LoadState {
    Empty,
    Loading,
    Ready,
    Unavailable(String),
    Error(String),
}
enum Work {
    Scan,
    Load { entry: Box<Entry> },
}
enum Answer {
    Scanned(sources::SourceCatalog),
    Loaded(LoadedModel),
}
struct Job {
    id: u64,
    work: Work,
}
struct Reply {
    id: u64,
    result: Result<Answer, String>,
}
enum ActiveKind {
    Scan,
    Load { epoch: u64, id: String },
}
struct Active {
    id: u64,
    kind: ActiveKind,
}

pub(super) enum Action {
    ManageImports,
    Stage { label: String, verified: Verified },
}

pub(super) struct ModelBrowser {
    runtime: Vec<Entry>,
    files: Vec<Entry>,
    imports: Vec<Entry>,
    snapshot: Option<Entry>,
    search: String,
    category: Option<String>,
    selected: Option<Entry>,
    state: LoadState,
    loaded: Option<LoadedModel>,
    cache: VecDeque<(String, LoadedModel)>,
    preview: Option<ModelPreview>,
    uploaded_epoch: Option<u64>,
    gpu_error: Option<String>,
    sender: Option<SyncSender<Job>>,
    receiver: Receiver<Reply>,
    active: Option<Active>,
    waiting: Option<(u64, Entry)>,
    refresh_requested: bool,
    serial: u64,
    epoch: u64,
    snapshot_serial: u64,
    scan_message: String,
    notice: String,
}
impl ModelBrowser {
    pub(super) fn open(project: &Path) -> Self {
        let project = project.to_path_buf();
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        Self::with_worker(runtime_model_catalog(), move |work| match work {
            Work::Scan => sources::scan(&project, &repository).map(Answer::Scanned),
            Work::Load { entry } => match entry.model {
                Source::Runtime(id) => load_runtime_model(&id).map(LoadedModel::Runtime),
                Source::File(file) => file.load().map(LoadedModel::Imported),
                Source::Verified(verified) => Ok(LoadedModel::Imported(verified)),
                Source::Snapshot(bytes) => {
                    Verified::decode_preview(bytes.as_ref().clone()).map(LoadedModel::Imported)
                }
            }
            .map(Answer::Loaded),
        })
    }
    fn with_worker(
        runtime: Vec<RuntimeModelDescriptor>,
        worker: impl Fn(Work) -> Result<Answer, String> + Send + 'static,
    ) -> Self {
        let (sender, jobs) = mpsc::sync_channel::<Job>(1);
        let (results, receiver) = mpsc::sync_channel(1);
        let started = std::thread::Builder::new()
            .name("model-browser".into())
            .spawn(move || {
                while let Ok(job) = jobs.recv() {
                    if results
                        .send(Reply {
                            id: job.id,
                            result: worker(job.work),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
        let runtime = runtime
            .into_iter()
            .map(|model| Entry {
                id: model.id.clone(),
                name: model.name,
                category: model.category,
                source: model.source,
                identity: model.id.clone(),
                notes: model.notes,
                unavailable: model.unavailable,
                spec: model.spec_dimensions_m,
                model: Source::Runtime(model.id),
            })
            .collect();
        let mut browser = Self {
            runtime,
            files: Vec::new(),
            imports: Vec::new(),
            snapshot: None,
            search: String::new(),
            category: None,
            selected: None,
            state: LoadState::Empty,
            loaded: None,
            cache: VecDeque::new(),
            preview: None,
            uploaded_epoch: None,
            gpu_error: None,
            sender: started.ok().map(|_| sender),
            receiver,
            active: None,
            waiting: None,
            refresh_requested: true,
            serial: 0,
            epoch: 0,
            snapshot_serial: 0,
            scan_message: "Finding model files…".into(),
            notice: String::new(),
        };
        if let Some(entry) = browser
            .runtime
            .iter()
            .find(|entry| entry.unavailable.is_none())
            .cloned()
        {
            browser.select(entry, false);
        }
        browser.dispatch();
        browser
    }
    pub(super) fn selected_id(&self) -> Option<&str> {
        self.selected.as_ref().map(|entry| entry.id.as_str())
    }
    pub(super) fn set_notice(&mut self, notice: String) {
        self.notice = notice;
    }
    pub(super) fn open_snapshot(&mut self, label: String, bytes: Vec<u8>) -> bool {
        if bytes.len() > 16 * 1024 * 1024 || self.sender.is_none() {
            return false;
        }
        self.snapshot_serial += 1;
        let entry = Entry {
            id: format!("files-snapshot:{}", self.snapshot_serial),
            name: label.clone(),
            category: "Files snapshots".into(),
            source: "Files · guarded snapshot".into(),
            identity: label,
            notes: vec![
                "Verified file snapshot; browsing does not change accepted or staged imports."
                    .into(),
            ],
            unavailable: None,
            spec: None,
            model: Source::Snapshot(Arc::new(bytes)),
        };
        self.snapshot = Some(entry.clone());
        self.search.clear();
        self.category = None;
        self.select(entry, true);
        true
    }
    fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.runtime
            .iter()
            .chain(&self.files)
            .chain(&self.imports)
            .chain(self.snapshot.iter())
    }
    fn select(&mut self, entry: Entry, force: bool) {
        if !force && self.selected_id() == Some(entry.id.as_str()) {
            return;
        }
        self.epoch += 1;
        self.loaded = None;
        self.waiting = None;
        self.uploaded_epoch = None;
        self.gpu_error = None;
        if let Some(preview) = &mut self.preview {
            preview.clear();
        }
        self.selected = Some(entry.clone());
        if let Some(reason) = &entry.unavailable {
            self.state = LoadState::Unavailable(reason.clone());
        } else if !force && self.cache.iter().any(|(id, _)| id == &entry.id) {
            self.loaded = self
                .cache
                .iter()
                .find(|(id, _)| id == &entry.id)
                .map(|(_, model)| model.clone());
            self.state = LoadState::Ready;
        } else if let Source::Verified(verified) = &entry.model {
            self.loaded = Some(LoadedModel::Imported(verified.clone()));
            self.state = LoadState::Ready;
        } else if self.sender.is_none() {
            self.state =
                LoadState::Error("Model worker unavailable. Reopen Models to retry.".into());
        } else {
            self.state = LoadState::Loading;
            self.waiting = Some((self.epoch, entry));
        }
        self.dispatch();
    }
    fn dispatch(&mut self) {
        if self.active.is_some() || self.sender.is_none() {
            return;
        }
        let (work, kind) = if let Some((epoch, entry)) = self.waiting.take() {
            let kind = ActiveKind::Load {
                epoch,
                id: entry.id.clone(),
            };
            (
                Work::Load {
                    entry: Box::new(entry),
                },
                kind,
            )
        } else if self.refresh_requested {
            self.refresh_requested = false;
            (Work::Scan, ActiveKind::Scan)
        } else {
            return;
        };
        self.serial += 1;
        let id = self.serial;
        if self
            .sender
            .as_ref()
            .unwrap()
            .try_send(Job { id, work })
            .is_ok()
        {
            self.active = Some(Active { id, kind });
        } else {
            self.sender = None;
            self.state = LoadState::Error("Model worker stopped; no work was replayed.".into());
        }
    }
    fn sync_imports(&mut self, snapshots: Vec<BrowserImportSnapshot>) {
        let previous = self
            .imports
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        self.imports = snapshots
            .into_iter()
            .map(|snapshot| Entry {
                id: snapshot.id,
                name: snapshot.name,
                category: "Imported snapshots".into(),
                source: snapshot.source,
                identity: snapshot.identity,
                notes: snapshot.notes,
                unavailable: snapshot.unavailable,
                spec: None,
                model: snapshot
                    .verified
                    .map(Source::Verified)
                    .unwrap_or_else(|| Source::Snapshot(Arc::new(Vec::new()))),
            })
            .collect();
        if let Some(selected) = &self.selected {
            if previous.contains(&selected.id)
                && !self.imports.iter().any(|entry| entry.id == selected.id)
            {
                self.epoch += 1;
                self.loaded = None;
                self.waiting = None;
                self.uploaded_epoch = None;
                self.state = LoadState::Unavailable(
                    "This import snapshot changed. Select the current accepted or staged entry."
                        .into(),
                );
                if let Some(preview) = &mut self.preview {
                    preview.clear();
                }
            }
        }
    }
    pub(super) fn poll(&mut self) {
        loop {
            match self.receiver.try_recv() {
                Ok(reply) => {
                    if self
                        .active
                        .as_ref()
                        .is_none_or(|active| active.id != reply.id)
                    {
                        continue;
                    }
                    let active = self.active.take().unwrap();
                    match (active.kind, reply.result) {
                        (ActiveKind::Scan, Ok(Answer::Scanned(catalog))) => {
                            self.scan_message = format!(
                                "{} source files{}{}",
                                catalog.matching("").count(),
                                if catalog.truncated {
                                    " · scan limit reached"
                                } else {
                                    ""
                                },
                                if catalog.skipped > 0 {
                                    format!(" · {} skipped", catalog.skipped)
                                } else {
                                    String::new()
                                }
                            );
                            self.files = catalog
                                .files
                                .into_iter()
                                .map(|file| Entry {
                                    id: file.id.clone(),
                                    name: file.name.clone(),
                                    category: file.category.clone(),
                                    source: file.source.clone(),
                                    identity: file.relative.clone(),
                                    notes: vec![format!("{} bytes", file.bytes)],
                                    unavailable: file.unavailable.clone(),
                                    spec: None,
                                    model: Source::File(file),
                                })
                                .collect();
                            self.cache.retain(|(id, _)| !id.starts_with("file:"));
                            if let Some(selected) = self
                                .selected
                                .clone()
                                .filter(|entry| matches!(entry.model, Source::File(_)))
                            {
                                if let Some(entry) = self
                                    .files
                                    .iter()
                                    .find(|entry| entry.id == selected.id)
                                    .cloned()
                                {
                                    self.select(entry, true);
                                } else {
                                    self.epoch += 1;
                                    self.loaded = None;
                                    self.waiting = None;
                                    self.uploaded_epoch = None;
                                    self.state = LoadState::Unavailable(
                                        "Selected source is no longer in the refreshed catalog."
                                            .into(),
                                    );
                                    if let Some(preview) = &mut self.preview {
                                        preview.clear();
                                    }
                                }
                            }
                        }
                        (ActiveKind::Scan, Err(error)) => {
                            self.scan_message =
                                format!("Model file scan failed: {error}. Previous list retained.")
                        }
                        (ActiveKind::Load { epoch, id }, result)
                            if epoch == self.epoch && self.selected_id() == Some(id.as_str()) =>
                        {
                            match result {
                                Ok(Answer::Loaded(model)) => {
                                    self.cache.retain(|(cached, _)| cached != &id);
                                    self.cache.push_front((id, model.clone()));
                                    while self.cache.len() > 3 {
                                        self.cache.pop_back();
                                    }
                                    self.loaded = Some(model);
                                    self.state = LoadState::Ready;
                                }
                                Err(error) => {
                                    self.loaded = None;
                                    self.state = LoadState::Error(error);
                                }
                                _ => {
                                    self.state =
                                        LoadState::Error("Mismatched model worker result.".into())
                                }
                            }
                        }
                        (ActiveKind::Load { .. }, _) => {}
                        _ => self.scan_message = "Mismatched model scan result.".into(),
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.sender = None;
                    self.active = None;
                    self.waiting = None;
                    if self.state == LoadState::Loading {
                        self.state = LoadState::Error("Model worker disconnected.".into());
                    }
                    break;
                }
            }
        }
        self.dispatch();
    }
    fn update_preview(&mut self, gl: Option<&Arc<glow::Context>>) {
        if self.preview.is_none() {
            self.preview = Some(ModelPreview::new(gl));
        }
        if self.uploaded_epoch == Some(self.epoch) {
            return;
        }
        let preview = self.preview.as_mut().unwrap();
        preview.clear();
        self.gpu_error = None;
        if self.state == LoadState::Ready {
            let result = match self.loaded.as_ref() {
                Some(LoadedModel::Runtime(model)) => preview.set_runtime(model.clone()),
                Some(LoadedModel::Imported(verified)) => {
                    preview.set_imported(verified.geometry.clone())
                }
                None => Err("Selected model has no verified geometry".into()),
            };
            if let Err(error) = result {
                preview.clear();
                self.gpu_error = Some(error);
            }
        }
        // A load completion must trigger its one upload even if the empty/loading
        // state for this selection was already painted.
        self.uploaded_epoch = (self.state != LoadState::Loading).then_some(self.epoch);
    }
    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        gl: Option<&Arc<glow::Context>>,
        imports: &Imports,
    ) -> Option<Action> {
        self.sync_imports(imports.browser_snapshots());
        self.poll();
        self.update_preview(gl);
        let mut action = None;
        egui::SidePanel::left("unified_model_catalog")
            .resizable(true)
            .default_width(285.0)
            .width_range(220.0..=440.0)
            .show(ctx, |ui| {
                ui.heading("Models");
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Refresh").clicked() {
                        self.refresh_requested = true;
                        self.scan_message = "Finding model files…".into();
                        self.dispatch();
                    }
                    if ui.button("Import / manage…").clicked() {
                        action = Some(Action::ManageImports);
                    }
                });
                ui.label("Search models");
                ui.add(
                    egui::TextEdit::singleline(&mut self.search).hint_text("Name or source path"),
                );
                let categories = self
                    .entries()
                    .map(|entry| entry.category.clone())
                    .collect::<BTreeSet<_>>();
                egui::ComboBox::from_id_salt("model_browser_category")
                    .selected_text(self.category.as_deref().unwrap_or("All models"))
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.category, None, "All models");
                        for category in categories {
                            ui.selectable_value(
                                &mut self.category,
                                Some(category.clone()),
                                category,
                            );
                        }
                    });
                let search = self.search.trim().to_lowercase();
                let entries = self
                    .entries()
                    .filter(|entry| entry.matches(&search, self.category.as_deref()))
                    .cloned()
                    .collect::<Vec<_>>();
                ui.small(format!(
                    "{} of {} entries",
                    entries.len(),
                    self.entries().count()
                ));
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("model_browser_entries")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if entries.is_empty() {
                            ui.label("No models match this search.");
                            if ui.button("Clear filters").clicked() {
                                self.search.clear();
                                self.category = None;
                            }
                        }
                        let mut group = String::new();
                        for entry in entries {
                            if group != entry.category {
                                group = entry.category.clone();
                                ui.strong(&group);
                            }
                            ui.push_id(&entry.id, |ui| {
                                let selected = ui.selectable_label(
                                    self.selected_id() == Some(entry.id.as_str()),
                                    &entry.name,
                                );
                                if selected.clicked() {
                                    self.select(entry.clone(), false);
                                }
                                ui.small(format!(
                                    "{}{}",
                                    entry.source,
                                    if entry.unavailable.is_some() {
                                        " · unavailable"
                                    } else {
                                        ""
                                    }
                                ));
                            });
                        }
                    });
                ui.small(&self.scan_message);
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(entry) = self.selected.clone() {
                ui.heading(&entry.name);
                ui.horizontal_wrapped(|ui| { ui.label(&entry.category); ui.label(format!("Source: {}", entry.source)); });
                ui.add(egui::Label::new(&entry.identity).truncate()).on_hover_text(&entry.identity);
                if let Some(model) = &self.loaded {
                    let info = model.info();
                    ui.label(format!("Measured XYZ: {:.3} × {:.3} × {:.3} m · {} triangles · {} batches",
                        info.max_m[0]-info.min_m[0], info.max_m[1]-info.min_m[1], info.max_m[2]-info.min_m[2], info.triangles, info.batches));
                }
                let imported_notes = match &self.loaded { Some(LoadedModel::Imported(verified)) => verified.geometry.report.notes.clone(), _ => Vec::new() };
                if !entry.notes.is_empty() || !imported_notes.is_empty() || entry.spec.is_some() {
                    ui.collapsing("Model details", |ui| {
                        egui::ScrollArea::vertical().id_salt("model_browser_details").max_height(120.0).show(ui, |ui| {
                            if let Some(spec) = entry.spec { ui.label(format!("Reference dimensions: {:.3} × {:.3} × {:.3} m", spec[0],spec[1],spec[2])); }
                            for note in entry.notes.iter().chain(imported_notes.iter()) { ui.small(note); }
                            if let Some(LoadedModel::Imported(verified)) = &self.loaded {
                                ui.monospace(format!("SHA-256 {}", verified.sha256));
                                for material in &verified.geometry.report.materials { ui.small(format!("Material: {}", if material.is_empty() { "unnamed" } else { material })); }
                            }
                        });
                    });
                }
                match self.state.clone() {
                    LoadState::Loading => { ui.horizontal(|ui| { ui.spinner(); ui.label("Loading selected model…"); }); },
                    LoadState::Unavailable(reason) => { ui.colored_label(egui::Color32::YELLOW, reason); },
                    LoadState::Error(error) => { ui.colored_label(egui::Color32::LIGHT_RED, error); if ui.button("Retry selected model").clicked() { self.select(entry.clone(), true); } },
                    _ => {},
                }
                if let Some(error) = &self.gpu_error { ui.colored_label(egui::Color32::LIGHT_RED, format!("Preview unavailable: {error}")); }
                if matches!(entry.model, Source::File(_) | Source::Snapshot(_)) {
                    if let Some(LoadedModel::Imported(verified)) = &self.loaded {
                        let generated_normals = verified.geometry.uses_generated_normals();
                        if ui.add_enabled(!generated_normals, egui::Button::new("Stage selected…")).on_hover_text("Open import management with this verified geometry. Existing staged work is preserved unless it is the same asset.").clicked() {
                            action = Some(Action::Stage { label: entry.name.clone(), verified: verified.clone() });
                        }
                        if generated_normals {
                            ui.small(crate::import_document::PREVIEW_ONLY_REASON);
                        }
                    }
                }
            } else { ui.heading("Models"); ui.label("Select a model from the catalog."); }
            if !self.notice.is_empty() { ui.colored_label(egui::Color32::YELLOW, &self.notice); }
            ui.separator();
            if let Some(preview) = &mut self.preview { preview.show(ui); }
        });
        action
    }
    pub(super) fn destroy(&mut self) {
        if let Some(mut preview) = self.preview.take() {
            preview.destroy();
        }
        self.sender = None;
    }
}

#[cfg(test)]
#[path = "model_browser_tests.rs"]
mod tests;
