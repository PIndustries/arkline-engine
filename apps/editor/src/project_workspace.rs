//! Project-scoped source browsing and editing. Saving a file never stages or applies it.
mod files;

use eframe::egui::{self, Color32, FontId, RichText};
use files::{Document, DocumentKind, Entry, Outcome, Request, Stamp};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
        Arc,
    },
};

/// A request to use an existing editor asset viewer, never an invented preview.
pub enum PreviewRequest {
    Model { label: String, bytes: Vec<u8> },
}

struct Directory {
    entries: Vec<Entry>,
    truncated: bool,
}

struct Tab {
    relative: String,
    kind: DocumentKind,
    text: String,
    saved_text: String,
    stamp: Stamp,
    bytes: u64,
    external: Option<Document>,
    needs_check: bool,
    find: String,
    show_find: bool,
    find_focus: bool,
    layout: Option<LayoutCache>,
}

struct LayoutCache {
    source: String,
    query: String,
    dark: bool,
    pixels_per_point: f32,
    galley: Arc<egui::Galley>,
}

impl Tab {
    fn open(document: Document) -> Self {
        let text = document.text.unwrap_or_default();
        Self {
            relative: document.relative,
            kind: document.kind,
            saved_text: text.clone(),
            text,
            stamp: document.stamp,
            bytes: document.bytes,
            external: None,
            needs_check: false,
            find: String::new(),
            show_find: false,
            find_focus: false,
            layout: None,
        }
    }

    fn dirty(&self) -> bool {
        self.text != self.saved_text
    }

    fn editable(&self) -> bool {
        matches!(self.kind, DocumentKind::Text)
    }

    // The user can keep typing while Save runs. Only the submitted revision becomes saved.
    fn saved(&mut self, stamp: Stamp, submitted_text: String) {
        self.stamp = stamp;
        self.bytes = submitted_text.len() as u64;
        self.saved_text = submitted_text;
        self.external = None;
        self.needs_check = false;
    }
}

enum JobKind {
    List,
    Search,
    Open,
    Check,
    Reload,
    Preview,
    Save { relative: String, text: String },
    Folder,
}

enum JobResult {
    Files(Outcome),
    Folder(Option<PathBuf>),
}

struct Job {
    receiver: Receiver<Result<JobResult, String>>,
    cancellation: Arc<AtomicBool>,
    kind: JobKind,
    label: String,
    disk_generation: u64,
}

pub struct ProjectWorkspace {
    root: PathBuf,
    root_input: String,
    directories: BTreeMap<String, Directory>,
    expanded: BTreeSet<String>,
    show_hidden: bool,
    show_ignored: bool,
    search: String,
    search_results: Option<(Vec<String>, bool)>,
    tabs: Vec<Tab>,
    active: Option<usize>,
    job: Option<Job>,
    message: String,
    branch_change_pending: bool,
    local_git_mutation_pending: bool,
    close_warning: Option<String>,
    disk_compare: Option<(String, String)>,
    disk_generation: u64,
    preview_ready: Option<PreviewRequest>,
}

impl ProjectWorkspace {
    pub fn open(project: &Path) -> Self {
        let mut workspace = Self {
            root: project.to_owned(),
            root_input: project.display().to_string(),
            directories: BTreeMap::new(),
            expanded: BTreeSet::from([String::new()]),
            show_hidden: false,
            show_ignored: false,
            search: String::new(),
            search_results: None,
            tabs: Vec::new(),
            active: None,
            job: None,
            message: "Loading the project folder…".into(),
            branch_change_pending: false,
            local_git_mutation_pending: false,
            close_warning: None,
            disk_compare: None,
            disk_generation: 0,
            preview_ready: None,
        };
        workspace.list(String::new());
        workspace
    }

    pub fn has_dirty_buffers(&self) -> bool {
        self.tabs.iter().any(Tab::dirty)
    }

    pub fn has_pending_save(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|job| matches!(job.kind, JobKind::Save { .. }))
    }

    pub fn has_pending_file_job(&self) -> bool {
        self.job.is_some()
    }

    pub fn preview_handoff_result(&mut self, queued: bool) {
        self.message = if queued {
            "GLB snapshot queued in the model tools. Review the report and choose Accept preview there; no asset was applied or saved."
        } else {
            "The model tools are busy. Open Assets and finish its current operation, then return here and choose Preview in model tools again."
        }.into();
    }

    /// The root integrator supplies the actual Git worker state every frame.
    pub fn set_git_activity(&mut self, branch_change_pending: bool, local_mutation_pending: bool) {
        self.branch_change_pending = branch_change_pending;
        self.local_git_mutation_pending = local_mutation_pending;
    }

    /// Branch operations invalidate disk observations, never the source buffers themselves.
    pub fn notify_external_change(&mut self) {
        self.disk_generation = self.disk_generation.wrapping_add(1);
        for tab in &mut self.tabs {
            tab.needs_check = true;
        }
        self.message =
            "Repository files may have changed. Check disk before editing or saving an open tab."
                .into();
    }

    fn start(&mut self, request: Request, kind: JobKind, label: &str) {
        if self.job.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancellation = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancellation.clone();
        match std::thread::Builder::new()
            .name("arkline-project-files".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(|| files::execute(request, &worker_cancel))
                    .map_err(|_| {
                        "The file worker stopped unexpectedly. Your editor buffers are preserved."
                            .to_owned()
                    })
                    .and_then(|result| result)
                    .map(JobResult::Files);
                let _ = sender.send(result);
            }) {
            Ok(_) => {
                self.job = Some(Job {
                    receiver,
                    cancellation,
                    kind,
                    label: label.into(),
                    disk_generation: self.disk_generation,
                });
                self.message = format!("{label}…");
            }
            Err(error) => self.message = format!("Could not start the file worker: {error}"),
        }
    }

    fn choose_folder(&mut self) {
        if self.job.is_some() || self.branch_change_pending {
            return;
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancellation = Arc::new(AtomicBool::new(false));
        match std::thread::Builder::new()
            .name("arkline-project-folder".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(|| rfd::FileDialog::new().pick_folder())
                    .map(JobResult::Folder)
                    .map_err(|_| "The folder chooser could not open.".to_owned());
                let _ = sender.send(result);
            }) {
            Ok(_) => {
                self.job = Some(Job {
                    receiver,
                    cancellation,
                    kind: JobKind::Folder,
                    label: "Choose a project folder; opening it is a separate action".into(),
                    disk_generation: self.disk_generation,
                });
            }
            Err(error) => self.message = format!("Could not open the folder chooser: {error}"),
        }
    }

    fn list(&mut self, relative: String) {
        self.start(
            Request::List {
                root: self.root.clone(),
                relative,
                show_hidden: self.show_hidden,
                show_ignored: self.show_ignored,
            },
            JobKind::List,
            "Reading folder",
        );
    }

    fn read(&mut self, relative: String, kind: JobKind) {
        self.start(
            Request::Read {
                root: self.root.clone(),
                relative,
            },
            kind,
            "Reading file",
        );
    }

    pub fn poll(&mut self) {
        let result = match self.job.as_ref().map(|job| job.receiver.try_recv()) {
            Some(Ok(result)) => result,
            Some(Err(TryRecvError::Disconnected)) => Err(
                "File worker disconnected. Your buffers are preserved; retry the operation.".into(),
            ),
            _ => return,
        };
        let Some(job) = self.job.take() else {
            return;
        };
        if job.disk_generation != self.disk_generation
            && matches!(&result, Ok(JobResult::Files(_)))
            && !matches!(&result, Ok(JobResult::Files(Outcome::Saved { .. })))
        {
            self.refresh_tree();
            self.message = "Repository changed during the file read. The old result was ignored; check or reopen the file.".into();
            return;
        }
        match result {
            Err(error) => self.message = error,
            Ok(JobResult::Folder(Some(path))) => {
                self.root_input = path.display().to_string();
                self.message = "Folder selected. Open folder changes only this workspace, after all edits are saved.".into();
            }
            Ok(JobResult::Folder(None)) => self.message = "Folder selection cancelled.".into(),
            Ok(JobResult::Files(outcome)) => self.accept(outcome, job.kind),
        }
    }

    fn accept(&mut self, outcome: Outcome, kind: JobKind) {
        match outcome {
            Outcome::Listed {
                relative,
                entries,
                truncated,
            } => {
                const MAX_CACHED_DIRECTORIES: usize = 24;
                if !self.directories.contains_key(&relative)
                    && self.directories.len() >= MAX_CACHED_DIRECTORIES
                {
                    self.directories.retain(|path, _| path.is_empty());
                    self.expanded.retain(|path| path.is_empty());
                    self.expanded.insert(relative.clone());
                }
                self.message = if truncated {
                    "Folder listing reached its safety limit. Use filename search or open a smaller project folder.".into()
                } else {
                    "Folder loaded. Files open only when selected.".into()
                };
                self.directories
                    .insert(relative, Directory { entries, truncated });
            }
            Outcome::Found { paths, truncated } => {
                self.message = if truncated {
                    "Search reached its safety limit; narrow the filename query.".into()
                } else {
                    format!(
                        "{} matching filenames. Search does not read file contents.",
                        paths.len()
                    )
                };
                self.search_results = Some((paths, truncated));
            }
            Outcome::Opened(document) => {
                let existing = self
                    .tabs
                    .iter()
                    .position(|tab| tab.relative == document.relative);
                if let Some(index) = existing {
                    let tab = &mut self.tabs[index];
                    if matches!(kind, JobKind::Reload) && !tab.dirty() {
                        *tab = Tab::open(document);
                        self.message = "Reloaded the saved disk version.".into();
                    } else if tab.stamp != document.stamp {
                        tab.external = Some(document);
                        tab.needs_check = false;
                        self.message = "File changed on disk. Editor text is preserved; compare the disk version before resolving it.".into();
                    } else {
                        tab.external = None;
                        tab.needs_check = false;
                        self.message =
                            "Disk version still matches this tab's saved baseline.".into();
                    }
                    self.active = Some(index);
                } else {
                    self.tabs.push(Tab::open(document));
                    self.active = Some(self.tabs.len() - 1);
                    self.message = "File opened. Save writes this file only; Git staging and runtime Apply are separate.".into();
                }
            }
            Outcome::Saved { relative, stamp } => {
                if let JobKind::Save {
                    relative: pending,
                    text,
                } = kind
                {
                    if pending == relative {
                        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.relative == relative)
                        {
                            tab.saved(stamp, text);
                            self.message = if tab.dirty() {
                                "Submitted revision saved. Newer editor changes remain unsaved."
                                    .into()
                            } else {
                                "File saved to disk. Nothing was staged, committed, applied or reloaded.".into()
                            };
                        }
                    }
                }
            }
            Outcome::PreviewGlb { relative, bytes } => {
                self.preview_ready = Some(PreviewRequest::Model {
                    label: relative,
                    bytes,
                });
                self.message = "Verified GLB snapshot is ready for the model tools.".into();
            }
        }
    }

    fn open_root(&mut self) {
        if self.has_dirty_buffers() || self.job.is_some() || self.branch_change_pending {
            self.message = "Save every dirty source tab and wait for current jobs before switching the project folder. Your buffers stay open.".into();
            return;
        }
        if self.root_input.trim().is_empty() {
            self.message = "Choose a project folder first.".into();
            return;
        }
        self.root = PathBuf::from(&self.root_input);
        self.disk_generation = self.disk_generation.wrapping_add(1);
        self.directories.clear();
        self.expanded = BTreeSet::from([String::new()]);
        self.search_results = None;
        self.tabs.clear();
        self.active = None;
        self.close_warning = None;
        self.disk_compare = None;
        self.preview_ready = None;
        self.list(String::new());
    }

    fn open_file(&mut self, relative: String) {
        if let Some(index) = self.tabs.iter().position(|tab| tab.relative == relative) {
            self.active = Some(index);
        } else if self.tabs.len() >= 24 {
            self.message = "24 tabs are open. Save and close a tab before opening another.".into();
        } else {
            self.read(relative, JobKind::Open);
        }
    }

    fn save(&mut self, index: usize) {
        if self.job.is_some() || self.branch_change_pending || self.local_git_mutation_pending {
            self.message = "Wait for the current file or Git mutation before saving. Editor text is preserved.".into();
            return;
        }
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        if !tab.editable() || !tab.dirty() {
            return;
        }
        if tab.external.is_some() || tab.needs_check {
            self.message = "Check and reconcile the external file change before saving; the disk version will not be overwritten.".into();
            return;
        }
        self.start(
            Request::Save {
                root: self.root.clone(),
                relative: tab.relative.clone(),
                expected: tab.stamp.clone(),
                text: tab.text.clone(),
            },
            JobKind::Save {
                relative: tab.relative.clone(),
                text: tab.text.clone(),
            },
            "Saving selected file",
        );
    }

    fn close_tab(&mut self, index: usize) {
        if self.branch_change_pending || self.has_pending_save() {
            self.message = "Wait for the current mutation before closing source tabs.".into();
            return;
        }
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        if tab.dirty() {
            self.close_warning = Some(tab.relative.clone());
            self.active = Some(index);
            return;
        }
        self.tabs.remove(index);
        self.active = match (self.active, self.tabs.is_empty()) {
            (_, true) => None,
            (Some(active), false) if active > index => Some(active - 1),
            (Some(active), false) if active == index => Some(index.min(self.tabs.len() - 1)),
            (active, false) => active,
        };
    }

    fn refresh_tree(&mut self) {
        if self.job.is_none() {
            self.directories.clear();
            self.expanded = BTreeSet::from([String::new()]);
            self.search_results = None;
            self.list(String::new());
        }
    }

    fn tree_rows(&self, relative: &str, depth: usize, rows: &mut Vec<(Entry, usize)>) {
        if depth > 32 {
            return;
        }
        if let Some(directory) = self.directories.get(relative) {
            for entry in &directory.entries {
                rows.push((entry.clone(), depth));
                if entry.is_dir && self.expanded.contains(&entry.relative) {
                    self.tree_rows(&entry.relative, depth + 1, rows);
                }
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<PreviewRequest> {
        let preview = self.preview_ready.take();
        egui::TopBottomPanel::top("project_workspace_toolbar").show(ctx, |ui| {
            ui.add_space(5.);
            ui.horizontal_wrapped(|ui| {
                ui.heading("Project files");
                let count = self.tabs.iter().filter(|tab| tab.dirty()).count();
                ui.label(format!("{count} unsaved tab{}", if count == 1 { "" } else { "s" }));
                ui.small("Save → Git stage → Commit → Push · Runtime Apply is separate");
            });
            ui.add_enabled_ui(self.job.is_none() && !self.branch_change_pending, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Folder");
                    ui.add(egui::TextEdit::singleline(&mut self.root_input).desired_width(540.));
                    if ui.button("Browse…").clicked() { self.choose_folder(); }
                    if ui.button("Open folder").clicked() { self.open_root(); }
                });
            });
            ui.small(format!("Current root: {}", self.root.display()));
            ui.horizontal_wrapped(|ui| {
                if let Some(job) = &self.job {
                    ui.spinner();
                    ui.label(&job.label);
                    if !matches!(job.kind, JobKind::Folder) {
                        let cancelling = job.cancellation.load(Ordering::Relaxed);
                        if ui.add_enabled(!cancelling, egui::Button::new("Cancel file job")).clicked() {
                            job.cancellation.store(true, Ordering::Relaxed);
                        }
                        if cancelling { ui.small("Cancelling; wait for the result before another operation."); }
                    }
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                }
                ui.label(&self.message);
            });
            if self.branch_change_pending {
                ui.colored_label(Color32::YELLOW, "Git is changing branches. Source editing and file mutations are paused; open buffers remain intact.");
            } else if self.local_git_mutation_pending {
                ui.small("Git mutation in progress. You can edit, but Save waits until it finishes.");
            }
        });

        egui::SidePanel::left("project_file_tree")
            .default_width(300.)
            .width_range(220. ..=500.)
            .resizable(true)
            .show(ctx, |ui| self.show_tree(ui));

        egui::CentralPanel::default().show(ctx, |ui| {
            let mut close = None;
            egui::ScrollArea::horizontal().id_salt("source_tabs").show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (index, tab) in self.tabs.iter().enumerate() {
                        ui.group(|ui| {
                            let name = tab.relative.rsplit('/').next().unwrap_or(&tab.relative);
                            let marker = if tab.dirty() { " •" } else { "" };
                            if ui.selectable_label(self.active == Some(index), format!("{name}{marker}"))
                                .on_hover_text(&tab.relative).clicked() {
                                self.active = Some(index);
                            }
                            if ui.small_button("×").on_hover_text("Close tab; unsaved changes block closing").clicked() {
                                close = Some(index);
                            }
                        });
                    }
                });
            });
            if let Some(index) = close { self.close_tab(index); }
            ui.separator();
            if let Some(index) = self.active.filter(|index| *index < self.tabs.len()) {
                self.show_document(ui, index);
            } else {
                ui.add_space(25.);
                ui.heading("Browse your project");
                ui.label("Expand a folder or search by filename, then select a file to open it.");
                ui.label("UTF-8 text is editable. Binary files show metadata and available asset viewers.");
                ui.small("Folder access, filename search and file reads are bounded. Symlinks and credential files are not followed automatically.");
                ui.small("This workspace does not compile or hot reload arbitrary source files. Use the existing engine controls for supported runtime changes.");
            }
        });
        self.show_close_warning(ctx);
        self.show_disk_compare(ctx);
        preview
    }

    fn show_tree(&mut self, ui: &mut egui::Ui) {
        ui.heading("Explorer");
        ui.add_enabled_ui(self.job.is_none() && !self.branch_change_pending, |ui| {
            let hidden = ui.checkbox(&mut self.show_hidden, "Hidden files").changed();
            let ignored = ui
                .checkbox(&mut self.show_ignored, "Git ignored files")
                .changed();
            ui.small("Sensitive names stay restricted. The .git internals are not browsed.");
            if hidden || ignored {
                self.refresh_tree();
            }
            ui.horizontal(|ui| {
                if ui.button("Refresh folders").clicked() {
                    self.refresh_tree();
                }
                if ui.button("Tree").clicked() {
                    self.search_results = None;
                }
            });
            let search = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Find filenames / paths…")
                    .char_limit(256)
                    .desired_width(f32::INFINITY),
            );
            let enter =
                search.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if (ui.button("Search filenames").clicked() || enter) && !self.search.trim().is_empty()
            {
                self.start(
                    Request::Search {
                        root: self.root.clone(),
                        query: self.search.clone(),
                        show_hidden: self.show_hidden,
                        show_ignored: self.show_ignored,
                    },
                    JobKind::Search,
                    "Searching project filenames",
                );
            }
        });
        ui.separator();
        if let Some((paths, truncated)) = self.search_results.clone() {
            ui.small(format!(
                "{} matches{}",
                paths.len(),
                if truncated { " · limit reached" } else { "" }
            ));
            egui::ScrollArea::vertical()
                .id_salt("project_search_results")
                .show_rows(ui, 23., paths.len(), |ui, range| {
                    for index in range {
                        let relative = &paths[index];
                        if ui
                            .add_enabled(
                                self.job.is_none() && !self.branch_change_pending,
                                egui::SelectableLabel::new(
                                    self.active
                                        .is_some_and(|i| self.tabs[i].relative == *relative),
                                    relative,
                                ),
                            )
                            .clicked()
                        {
                            self.open_file(relative.clone());
                        }
                    }
                });
            return;
        }
        if self
            .directories
            .values()
            .any(|directory| directory.truncated)
        {
            ui.colored_label(Color32::YELLOW, "Some folders reached the listing limit.");
        }
        let mut rows = Vec::new();
        self.tree_rows("", 0, &mut rows);
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .id_salt("project_folder_tree")
            .show_rows(ui, 23., rows.len(), |ui, range| {
                for index in range {
                    let (entry, depth) = &rows[index];
                    ui.horizontal(|ui| {
                        ui.add_space((*depth as f32 * 14.).min(160.));
                        let prefix = if entry.is_symlink {
                            "↗"
                        } else if entry.restricted {
                            "⊘"
                        } else if entry.is_dir && self.expanded.contains(&entry.relative) {
                            "▾"
                        } else if entry.is_dir {
                            "▸"
                        } else {
                            "·"
                        };
                        let selected = self
                            .active
                            .is_some_and(|i| self.tabs[i].relative == entry.relative);
                        let mut title = RichText::new(format!("{prefix} {}", entry.name));
                        if entry.ignored {
                            title = title.weak();
                        }
                        if ui
                            .add_enabled(
                                self.job.is_none() && !self.branch_change_pending,
                                egui::SelectableLabel::new(selected, title),
                            )
                            .on_hover_text(&entry.relative)
                            .clicked()
                        {
                            clicked = Some(entry.clone());
                        }
                    });
                }
            });
        if let Some(entry) = clicked {
            if entry.is_dir && !entry.is_symlink && !entry.restricted {
                if !self.expanded.remove(&entry.relative) {
                    self.expanded.insert(entry.relative.clone());
                    if !self.directories.contains_key(&entry.relative) {
                        self.list(entry.relative);
                    }
                }
            } else {
                self.open_file(entry.relative);
            }
        }
    }

    fn show_document(&mut self, ui: &mut egui::Ui, index: usize) {
        let mut save = false;
        let mut check = false;
        let mut reload = false;
        let mut preview = false;
        let job_pending = self.job.is_some();
        let may_save =
            !job_pending && !self.branch_change_pending && !self.local_git_mutation_pending;
        let tab = &mut self.tabs[index];
        ui.monospace(&tab.relative);
        ui.horizontal_wrapped(|ui| {
            if tab.editable() {
                save = ui
                    .add_enabled(
                        may_save && tab.dirty() && tab.external.is_none() && !tab.needs_check,
                        egui::Button::new("Save file"),
                    )
                    .on_hover_text(
                        "Writes only this tab to disk. Ctrl/Cmd+S while the editor has focus.",
                    )
                    .clicked();
                if ui.button("Find in file").clicked() {
                    tab.show_find = true;
                    tab.find_focus = true;
                }
            }
            check = ui
                .add_enabled(
                    !job_pending && !self.branch_change_pending,
                    egui::Button::new("Check disk"),
                )
                .clicked();
            reload = ui
                .add_enabled(
                    !job_pending && !self.branch_change_pending && !tab.dirty(),
                    egui::Button::new("Reload disk version"),
                )
                .on_hover_text("Explicitly reload a clean tab. Dirty buffers are never replaced.")
                .clicked();
            ui.small(format!(
                "{} bytes on disk · {}",
                tab.bytes,
                if tab.dirty() {
                    "UNSAVED"
                } else {
                    "Saved baseline"
                }
            ));
        });
        if tab.needs_check {
            ui.colored_label(
                Color32::YELLOW,
                "Repository files changed. Check disk before editing or saving this tab.",
            );
        }
        if let Some(external) = &tab.external {
            ui.colored_label(
                Color32::YELLOW,
                "External change detected. Your editor text is retained; Save is blocked.",
            );
            ui.horizontal(|ui| {
                if ui.button("Copy editor text").clicked() { ui.ctx().copy_text(tab.text.clone()); }
                if ui.button("Compare disk version").clicked() {
                    self.disk_compare = Some((tab.relative.clone(), external.text.clone().unwrap_or_else(||
                        "The disk version is no longer editable UTF-8 text. Metadata only; no binary preview.".into())));
                }
            });
            ui.small("Reconcile the change externally, or return the buffer to its saved baseline before explicitly reloading. No file is overwritten automatically.");
        }

        if tab.editable() {
            let editor_id = ui.make_persistent_id(("project_source", &tab.relative));
            if tab.show_find {
                ui.horizontal(|ui| {
                    ui.label("Find");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut tab.find)
                            .id_salt(("file_find", &tab.relative))
                            .char_limit(256)
                            .desired_width(270.),
                    );
                    if tab.find_focus {
                        response.request_focus();
                        tab.find_focus = false;
                    }
                    let count = if tab.find.is_empty() {
                        0
                    } else {
                        tab.text.match_indices(&tab.find).take(501).count()
                    };
                    ui.small(format!(
                        "{}{} exact matches",
                        count.min(500),
                        if count > 500 { "+" } else { "" }
                    ));
                    if ui.small_button("Close find").clicked() {
                        tab.show_find = false;
                    }
                });
            }
            let extension = Path::new(&tab.relative)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_owned();
            let query = if tab.show_find {
                tab.find.clone()
            } else {
                String::new()
            };
            let dark = ui.visuals().dark_mode;
            let cache = &mut tab.layout;
            let mut layouter = |ui: &egui::Ui, text: &str, _width: f32| {
                let pixels_per_point = ui.ctx().pixels_per_point();
                if let Some(cached) = cache.as_ref() {
                    if cached.source == text
                        && cached.query == query
                        && cached.dark == dark
                        && cached.pixels_per_point == pixels_per_point
                    {
                        return cached.galley.clone();
                    }
                }
                let galley =
                    ui.fonts(|fonts| fonts.layout_job(highlight(text, &extension, &query, dark)));
                *cache = Some(LayoutCache {
                    source: text.into(),
                    query: query.clone(),
                    dark,
                    pixels_per_point,
                    galley: galley.clone(),
                });
                galley
            };
            let mut focused = false;
            egui::ScrollArea::both()
                .id_salt(("source_scroll", &tab.relative))
                .max_height((ui.available_height() - 42.).max(100.))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let response = ui.add_enabled(
                        !self.branch_change_pending && !tab.needs_check,
                        egui::TextEdit::multiline(&mut tab.text)
                            .id(editor_id)
                            .code_editor()
                            .font(FontId::monospace(13.))
                            .desired_rows(30)
                            .desired_width(f32::INFINITY)
                            .char_limit(1024 * 1024)
                            .layouter(&mut layouter),
                    );
                    focused = response.has_focus();
                });
            if focused {
                let find_key =
                    ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::F));
                if find_key {
                    tab.show_find = true;
                    tab.find_focus = true;
                }
                let save_key = ui.input_mut(|input| {
                    input.consume_key(egui::Modifiers::COMMAND, egui::Key::S)
                        || input.consume_key(egui::Modifiers::CTRL, egui::Key::S)
                });
                save |= save_key;
            }
        } else {
            ui.add_space(15.);
            ui.heading(match tab.kind {
                DocumentKind::Binary => "Binary file",
                DocumentKind::Oversized => "File exceeds the 1 MiB editor limit",
                DocumentKind::Restricted => "Sensitive file is restricted",
                DocumentKind::Symlink => "Symbolic link",
                DocumentKind::Text => "Text file",
            });
            ui.label("No file contents are displayed or editable here.");
            if matches!(tab.kind, DocumentKind::Binary) {
                ui.small("Binary and Git LFS pointer handling belongs to the asset pipeline. A metadata view is not a successful import.");
            }
        }

        // An asset event is explicit. Root routes it only to an existing supported viewer.
        if matches!(tab.kind, DocumentKind::Binary | DocumentKind::Oversized) {
            let lower = tab.relative.to_ascii_lowercase();
            if lower.ends_with(".glb") {
                ui.add_space(5.);
                preview = ui.add_enabled(!tab.dirty() && !tab.needs_check && tab.external.is_none()
                    && !self.branch_change_pending && !job_pending, egui::Button::new("Open model viewer"))
                    .on_hover_text("Validates and snapshots this GLB (up to 16 MiB) for the existing asset tools. A changed file is rejected; nothing is applied automatically.").clicked();
            }
        }
        let relative = tab.relative.clone();
        let expected = tab.stamp.clone();
        if save {
            self.save(index);
        } else if check {
            self.read(relative, JobKind::Check);
        } else if reload {
            self.read(relative, JobKind::Reload);
        } else if preview {
            self.start(
                Request::PreviewGlb {
                    root: self.root.clone(),
                    relative,
                    expected,
                },
                JobKind::Preview,
                "Verifying GLB snapshot",
            );
        }
    }

    fn show_close_warning(&mut self, ctx: &egui::Context) {
        let Some(relative) = self.close_warning.clone() else {
            return;
        };
        let mut save = false;
        let mut keep = false;
        egui::Window::new("Unsaved source buffer")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.monospace(&relative);
                ui.label("This tab has unsaved changes. It will stay open until you save them.");
                ui.horizontal(|ui| {
                    save = ui
                        .add_enabled(
                            self.job.is_none()
                                && !self.branch_change_pending
                                && !self.local_git_mutation_pending,
                            egui::Button::new("Save file, keep tab open"),
                        )
                        .clicked();
                    keep = ui.button("Keep editing").clicked();
                });
            });
        if save {
            if let Some(index) = self.tabs.iter().position(|tab| tab.relative == relative) {
                self.save(index);
            }
            self.close_warning = None;
        } else if keep {
            self.close_warning = None;
        }
    }

    fn show_disk_compare(&mut self, ctx: &egui::Context) {
        let Some((relative, disk)) = self.disk_compare.as_ref() else {
            return;
        };
        let mut open = true;
        egui::Window::new("Disk version · read only").open(&mut open)
            .default_size([760., 500.]).show(ctx, |ui| {
                ui.monospace(relative);
                ui.small("This is the most recent explicit disk check. Your editable tab remains unchanged.");
                egui::ScrollArea::both().show(ui, |ui| {
                    let mut text = disk.as_str();
                    ui.add(egui::TextEdit::multiline(&mut text).code_editor().desired_width(f32::INFINITY).desired_rows(25));
                });
            });
        if !open {
            self.disk_compare = None;
        }
    }
}

impl Drop for ProjectWorkspace {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.cancellation.store(true, Ordering::Relaxed);
        }
    }
}

// A small deterministic lexer: useful Rust/JSON colors, not compiler diagnostics.
fn highlight(text: &str, extension: &str, query: &str, dark: bool) -> egui::text::LayoutJob {
    let normal = if dark {
        Color32::from_rgb(218, 225, 236)
    } else {
        Color32::from_rgb(35, 42, 52)
    };
    let string = if dark {
        Color32::from_rgb(163, 211, 150)
    } else {
        Color32::from_rgb(35, 111, 50)
    };
    let comment = if dark {
        Color32::from_rgb(135, 151, 167)
    } else {
        Color32::from_rgb(106, 117, 123)
    };
    let keyword = if dark {
        Color32::from_rgb(157, 184, 255)
    } else {
        Color32::from_rgb(65, 65, 178)
    };
    let number = if dark {
        Color32::from_rgb(234, 188, 132)
    } else {
        Color32::from_rgb(148, 78, 33)
    };
    let colored = matches!(extension, "rs" | "json" | "jsonc" | "toml") && text.len() <= 256 * 1024;
    let matches: Vec<_> = if query.is_empty() {
        Vec::new()
    } else {
        text.match_indices(query)
            .take(500)
            .map(|(start, found)| start..start + found.len())
            .collect()
    };
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let bytes = text.as_bytes();
    let mut position = 0;
    let mut match_index = 0;
    while position < text.len() {
        let start = position;
        let color = if !colored {
            position = text.len();
            normal
        } else if bytes[position] == b'"' {
            position += 1;
            while position < bytes.len() {
                if bytes[position] == b'\\' {
                    position = (position + 2).min(bytes.len());
                } else if bytes[position] == b'"' {
                    position += 1;
                    break;
                } else {
                    position += 1;
                }
            }
            // ASCII quotes are UTF-8 boundaries, including strings containing Unicode.
            string
        } else if bytes[position..].starts_with(b"//")
            || (extension == "toml" && bytes[position] == b'#')
        {
            while position < bytes.len() && bytes[position] != b'\n' {
                position += 1;
            }
            comment
        } else if bytes[position..].starts_with(b"/*") {
            position += 2;
            while position < bytes.len() && !bytes[position..].starts_with(b"*/") {
                position += 1;
            }
            position = (position + 2).min(bytes.len());
            comment
        } else if bytes[position].is_ascii_digit() {
            position += 1;
            while position < bytes.len()
                && (bytes[position].is_ascii_alphanumeric()
                    || matches!(bytes[position], b'_' | b'.'))
            {
                position += 1;
            }
            number
        } else if bytes[position].is_ascii_alphabetic() || bytes[position] == b'_' {
            position += 1;
            while position < bytes.len()
                && (bytes[position].is_ascii_alphanumeric() || bytes[position] == b'_')
            {
                position += 1;
            }
            if matches!(
                &text[start..position],
                "fn" | "pub"
                    | "struct"
                    | "enum"
                    | "impl"
                    | "trait"
                    | "use"
                    | "mod"
                    | "let"
                    | "mut"
                    | "const"
                    | "static"
                    | "if"
                    | "else"
                    | "match"
                    | "for"
                    | "while"
                    | "loop"
                    | "return"
                    | "break"
                    | "continue"
                    | "async"
                    | "await"
                    | "move"
                    | "self"
                    | "Self"
                    | "true"
                    | "false"
                    | "null"
                    | "where"
                    | "type"
                    | "in"
                    | "as"
                    | "ref"
                    | "dyn"
                    | "unsafe"
                    | "extern"
            ) {
                keyword
            } else {
                normal
            }
        } else {
            position += text[position..].chars().next().map_or(1, char::len_utf8);
            normal
        };
        // An incomplete escape may leave a byte offset inside a multi-byte scalar.
        while position < text.len() && !text.is_char_boundary(position) {
            position += 1;
        }
        let mut cursor = start;
        while match_index < matches.len() && matches[match_index].end <= start {
            match_index += 1;
        }
        let mut found_index = match_index;
        while found_index < matches.len() && matches[found_index].start < position {
            let found = &matches[found_index];
            let begin = found.start.max(start);
            let end = found.end.min(position);
            if cursor < begin {
                append_token(&mut job, &text[cursor..begin], color, false);
            }
            append_token(&mut job, &text[begin..end], color, true);
            cursor = end;
            found_index += 1;
        }
        if cursor < position {
            append_token(&mut job, &text[cursor..position], color, false);
        }
    }
    job
}

fn append_token(job: &mut egui::text::LayoutJob, text: &str, color: Color32, found: bool) {
    let format = egui::text::TextFormat {
        font_id: FontId::monospace(13.),
        color,
        background: if found {
            Color32::from_rgb(111, 86, 25)
        } else {
            Color32::TRANSPARENT
        },
        ..Default::default()
    };
    if let Some(section) = job.sections.last_mut() {
        if section.format == format && section.byte_range.end == job.text.len() {
            job.text.push_str(text);
            section.byte_range.end = job.text.len();
            return;
        }
    }
    job.append(text, 0., format);
}

#[cfg(test)]
mod ui_tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            use std::sync::atomic::AtomicU64;
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "arkline-project-ui-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join("source.rs"), "original\n").unwrap();
            Self(path)
        }

        fn read(&self) -> Document {
            match files::execute(
                Request::Read {
                    root: self.0.clone(),
                    relative: "source.rs".into(),
                },
                &AtomicBool::new(false),
            )
            .unwrap()
            {
                Outcome::Opened(document) => document,
                _ => panic!("expected a text document"),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn completed_save_keeps_newer_edits_dirty_and_updates_the_baseline() {
        let fixture = Fixture::new();
        let mut tab = Tab::open(fixture.read());
        let submitted = "submitted revision\n".to_owned();
        tab.text.clone_from(&submitted);
        let result = files::execute(
            Request::Save {
                root: fixture.0.clone(),
                relative: tab.relative.clone(),
                expected: tab.stamp.clone(),
                text: submitted.clone(),
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        tab.text = "newer typing while Save completes\n".into();
        let Outcome::Saved { stamp, .. } = result else {
            panic!("expected saved receipt")
        };
        tab.saved(stamp, submitted.clone());
        assert!(tab.dirty());
        assert_eq!(tab.saved_text, submitted);
        assert_eq!(tab.text, "newer typing while Save completes\n");
        // A second explicit save uses the successful first save's stamp.
        let second = files::execute(
            Request::Save {
                root: fixture.0.clone(),
                relative: tab.relative.clone(),
                expected: tab.stamp.clone(),
                text: tab.text.clone(),
            },
            &AtomicBool::new(false),
        );
        assert!(matches!(second, Ok(Outcome::Saved { .. })));
    }

    #[test]
    fn stale_check_and_reload_preserve_dirty_text() {
        let fixture = Fixture::new();
        let mut workspace = ProjectWorkspace::open(&fixture.0);
        let mut tab = Tab::open(fixture.read());
        tab.text = "unsaved editor text\n".into();
        workspace.tabs.push(tab);
        workspace.active = Some(0);
        std::fs::write(fixture.0.join("source.rs"), "external replacement\n").unwrap();
        workspace.accept(Outcome::Opened(fixture.read()), JobKind::Reload);
        assert_eq!(workspace.tabs[0].text, "unsaved editor text\n");
        assert!(workspace.tabs[0].dirty());
        assert!(workspace.tabs[0].external.is_some());
        workspace.close_tab(0);
        assert_eq!(workspace.tabs.len(), 1);
        assert!(workspace.close_warning.is_some());
        workspace.root_input = fixture.0.join("another-root").display().to_string();
        workspace.open_root();
        assert_eq!(workspace.root, fixture.0);
        assert_eq!(workspace.tabs[0].text, "unsaved editor text\n");
    }

    #[test]
    fn highlighting_preserves_unicode_and_incomplete_strings() {
        for source in [
            "let café = \"世界\"; // 🚀",
            "\"\\é",
            "/* aé",
            "fn main() { true }",
            "",
        ] {
            let result = highlight(source, "rs", "é", true);
            assert_eq!(result.text, source);
            for section in result.sections {
                assert!(source.is_char_boundary(section.byte_range.start));
                assert!(source.is_char_boundary(section.byte_range.end));
            }
        }
    }

    #[test]
    fn plain_text_find_and_large_source_keep_original_text() {
        let source = "key 世界 key";
        let result = highlight(source, "txt", "key", true);
        assert_eq!(result.text, source);
        assert_eq!(
            result
                .sections
                .iter()
                .filter(|section| section.format.background != Color32::TRANSPARENT)
                .count(),
            2
        );
        let large = "fn x() {}\n".repeat(30_000);
        assert_eq!(highlight(&large, "rs", "", true).text, large);
    }
}
