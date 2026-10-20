//! Unified inspection catalog. Browsing never accepts, saves or applies an asset.
use crate::import_document::Verified;
use crate::imports::{BrowserImportSnapshot, Imports};
use arkline_client::{
    load_runtime_model, runtime_model_catalog, runtime_model_context_id, ModelGeometryInfo,
    ModelPreview, RuntimeInspectionModel, RuntimeModelDescriptor,
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
#[path = "model_browser_tree.rs"]
mod tree;

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
    hierarchy: Vec<String>,
    source: String,
    identity: String,
    notes: Vec<String>,
    unavailable: Option<String>,
    spec: Option<[f64; 3]>,
    model: Source,
}
impl Entry {
    fn verified_hash(&self) -> Option<&str> {
        match &self.model {
            Source::Verified(verified) => Some(&verified.sha256),
            _ => None,
        }
    }
    fn same_import_snapshot(&self, other: &Self) -> bool {
        self.id == other.id
            && self.name == other.name
            && self.category == other.category
            && self.hierarchy == other.hierarchy
            && self.source == other.source
            && self.identity == other.identity
            && self.notes == other.notes
            && self.unavailable == other.unavailable
            && self.spec == other.spec
            && self.verified_hash() == other.verified_hash()
    }
    fn matches(&self, search: &str, category: Option<&str>) -> bool {
        category.is_none_or(|category| category == self.category)
            && (search.is_empty()
                || [&self.name, &self.category, &self.source, &self.identity]
                    .into_iter()
                    .chain(self.hierarchy.iter())
                    .any(|text| text.to_lowercase().contains(search)))
    }
}
#[derive(Clone)]
enum LoadedModel {
    Runtime {
        model: Arc<RuntimeInspectionModel>,
        context: Option<Arc<RuntimeInspectionModel>>,
        context_error: Option<String>,
    },
    Imported(Verified),
}
impl LoadedModel {
    fn info(&self) -> ModelGeometryInfo {
        match self {
            Self::Runtime { model, .. } => model.info.clone(),
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
    tree_open: BTreeSet<Vec<String>>,
    tree: Option<tree::Tree>,
    tree_filter: Option<(String, Option<String>)>,
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
                Source::Runtime(id) => load_runtime_model(&id).map(|model| {
                    let context = runtime_model_context_id(&id).map(|id| load_runtime_model(&id));
                    let (context, context_error) = match context {
                        Some(Ok(context)) => (Some(context), None),
                        Some(Err(error)) => (None, Some(error)),
                        None => (None, None),
                    };
                    LoadedModel::Runtime {
                        model,
                        context,
                        context_error,
                    }
                }),
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
                category: model.category.clone(),
                hierarchy: if model.hierarchy.is_empty() {
                    vec![model.category]
                } else {
                    model.hierarchy
                },
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
            tree_open: BTreeSet::from([
                vec!["Ships".into()],
                vec!["Ships".into(), "Pioneer".into()],
                vec!["Ships".into(), "Pioneer".into(), "Source parts".into()],
            ]),
            tree: None,
            tree_filter: None,
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
            .find(|entry| entry.id == "ship/pioneer" && entry.unavailable.is_none())
            .or_else(|| {
                browser
                    .runtime
                    .iter()
                    .find(|entry| entry.unavailable.is_none())
            })
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
            hierarchy: vec!["Files snapshots".into()],
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
        self.tree = None;
        self.clear_filters();
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
        self.reveal_entry(&entry);
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
        let imports = snapshots
            .into_iter()
            .map(|snapshot| Entry {
                id: snapshot.id,
                name: snapshot.name,
                category: "Imported snapshots".into(),
                hierarchy: vec!["Imported snapshots".into()],
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
            .collect::<Vec<_>>();
        let previous = std::mem::replace(&mut self.imports, imports);
        if previous.len() != self.imports.len()
            || previous
                .iter()
                .zip(&self.imports)
                .any(|(before, after)| !before.same_import_snapshot(after))
        {
            self.tree = None;
        }
        let Some(selected) = self.selected.clone() else {
            return;
        };
        if let Some(current) = self
            .imports
            .iter()
            .find(|entry| entry.id == selected.id)
            .cloned()
        {
            let restored = matches!(self.state, LoadState::Unavailable(_))
                && current.unavailable.is_none()
                && current.verified_hash().is_some();
            if !selected.same_import_snapshot(&current) || restored {
                // A Save or provenance/revision update does not change verified
                // content. Keep its camera, GPU upload, cache and pending work.
                let unchanged_geometry = current.unavailable.is_none()
                    && self.state == LoadState::Ready
                    && matches!(&self.loaded, Some(LoadedModel::Imported(verified)) if current.verified_hash() == Some(verified.sha256.as_str()));
                if unchanged_geometry {
                    self.selected = Some(current);
                } else if let (LoadState::Unavailable(_), Some(reason)) =
                    (&self.state, &current.unavailable)
                {
                    // Refresh a changed failure message without retrying or
                    // rebuilding the same unavailable preview every frame.
                    self.state = LoadState::Unavailable(reason.clone());
                    self.selected = Some(current);
                } else {
                    if selected.verified_hash() != current.verified_hash() {
                        self.cache.retain(|(id, _)| id != &current.id);
                    }
                    // Restored bytes are already verified by import management;
                    // select consumes that snapshot without another decode.
                    self.select(current, true);
                }
            }
        } else if previous.iter().any(|entry| entry.id == selected.id) {
            self.epoch += 1;
            self.loaded = None;
            self.waiting = None;
            self.uploaded_epoch = None;
            self.state = LoadState::Unavailable(
                "This import snapshot changed. Select the current accepted or staged entry.".into(),
            );
            if let Some(preview) = &mut self.preview {
                preview.clear();
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
                                    hierarchy: vec![file.category.clone()],
                                    source: file.source.clone(),
                                    identity: file.relative.clone(),
                                    notes: vec![format!("{} bytes", file.bytes)],
                                    unavailable: file.unavailable.clone(),
                                    spec: None,
                                    model: Source::File(file),
                                })
                                .collect();
                            self.tree = None;
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
    fn reveal_entry(&mut self, entry: &Entry) {
        for end in 1..=entry.hierarchy.len() {
            self.tree_open.insert(entry.hierarchy[..end].to_vec());
        }
    }
    fn clear_filters(&mut self) {
        self.search.clear();
        self.category = None;
        if let Some(entry) = self.selected.clone() {
            self.reveal_entry(&entry);
        }
    }
    fn rebuild_tree(&mut self) {
        let filter = (self.search.trim().to_lowercase(), self.category.clone());
        if self.tree.is_none() || self.tree_filter.as_ref() != Some(&filter) {
            self.tree = Some(tree::Tree::build(
                &self.entries().cloned().collect::<Vec<_>>(),
                &filter.0,
                filter.1.as_deref(),
            ));
            self.tree_filter = Some(filter);
        }
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
                Some(LoadedModel::Runtime { model, context, .. }) => {
                    preview.set_runtime_context(model.clone(), context.clone())
                }
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
                self.rebuild_tree();
                let matching = self.tree.as_ref().unwrap().matches;
                ui.small(format!("{} of {} models", matching, self.entries().count()));
                if (!self.search.is_empty() || self.category.is_some())
                    && ui.small_button("Clear filters").clicked()
                {
                    self.clear_filters();
                }
                ui.separator();
                let auto_expand = !self.search.trim().is_empty() || self.category.is_some();
                let selected_id = self.selected_id().map(str::to_owned);
                let mut selection = None;
                egui::ScrollArea::vertical()
                    .id_salt("model_browser_entries")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let tree = self.tree.as_ref().unwrap();
                        if tree.matches == 0 {
                            ui.label("No models match this search.");
                        }
                        for node in &tree.roots {
                            show_tree_node(
                                ui,
                                node,
                                &mut self.tree_open,
                                auto_expand,
                                selected_id.as_deref(),
                                &mut selection,
                            );
                        }
                    });
                if let Some(entry) = selection {
                    self.select(entry, false);
                }
                ui.small(&self.scan_message);
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(entry) = self.selected.clone() {
                ui.small(entry.hierarchy.join(" › "));
                ui.heading(&entry.name);
                if matches!(&entry.model, Source::Runtime(id) if id.starts_with("pioneer/part/")) {
                    ui.small("Source object from this export; folders group names, not authored assemblies.");
                } else if matches!(&entry.model, Source::Runtime(id) if id.starts_with("pioneer/material/")) {
                    ui.small("Material batch: can include many disconnected source objects.");
                }
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
                if let Some(LoadedModel::Runtime { context_error: Some(error), .. }) = &self.loaded {
                    ui.colored_label(egui::Color32::YELLOW, format!("Ship context unavailable: {error}"));
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
            self.update_preview(gl);
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

fn show_tree_node(
    ui: &mut egui::Ui,
    node: &tree::Node,
    open: &mut BTreeSet<Vec<String>>,
    auto_expand: bool,
    selected: Option<&str>,
    selection: &mut Option<Entry>,
) {
    let id = node
        .entry
        .as_ref()
        .map(|entry| entry.id.as_str())
        .unwrap_or("");
    ui.push_id((&node.path, id), |ui| {
        let expanded = auto_expand || open.contains(&node.path);
        ui.horizontal(|ui| {
            if node.children.is_empty() {
                ui.add_space(18.0);
            } else if ui
                .small_button(if expanded { "▾" } else { "▸" })
                .on_hover_text(if expanded {
                    "Collapse group"
                } else {
                    "Expand group"
                })
                .clicked()
                && !auto_expand
            {
                if expanded {
                    open.remove(&node.path);
                } else {
                    open.insert(node.path.clone());
                }
            }
            let is_selected = node
                .entry
                .as_ref()
                .is_some_and(|entry| selected == Some(entry.id.as_str()));
            let label = if is_selected || node.entry.is_none() {
                egui::RichText::new(&node.label).strong()
            } else {
                egui::RichText::new(&node.label)
            };
            let unavailable = node
                .entry
                .as_ref()
                .is_some_and(|entry| entry.unavailable.is_some());
            let badge_width = if unavailable {
                78.0
            } else if !node.children.is_empty() {
                36.0
            } else {
                0.0
            };
            let response = ui.add_sized(
                [(ui.available_width() - badge_width).max(40.0), 18.0],
                egui::Button::new(label)
                    .selected(is_selected)
                    .frame(false)
                    .wrap(),
            );
            if let Some(entry) = &node.entry {
                let response = response.on_hover_text(format!(
                    "{}\n{}{}",
                    entry.source,
                    entry.identity,
                    entry
                        .unavailable
                        .as_ref()
                        .map(|reason| format!("\nUnavailable: {reason}"))
                        .unwrap_or_default()
                ));
                if response.clicked() {
                    *selection = Some(entry.clone());
                }
            } else if response.clicked() && !auto_expand {
                if expanded {
                    open.remove(&node.path);
                } else {
                    open.insert(node.path.clone());
                }
            }
            if !node.children.is_empty() {
                ui.weak(node.matches.to_string());
            }
            if node
                .entry
                .as_ref()
                .is_some_and(|entry| entry.unavailable.is_some())
            {
                ui.weak("unavailable");
            }
        });
        if expanded && !node.children.is_empty() {
            ui.indent("children", |ui| {
                for child in &node.children {
                    show_tree_node(ui, child, open, auto_expand, selected, selection);
                }
            });
        }
    });
}

#[cfg(test)]
#[path = "model_browser_tests.rs"]
mod tests;
