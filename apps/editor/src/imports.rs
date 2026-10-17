//! One bounded background worker per project; staged preview is separate from accepted/saved.
use crate::import_document::{self as doc, AssetIndex, Catalog, Loaded, Verified};
use arkline_client::ImportedAssetPreview;
use eframe::{egui, glow};
use std::{
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, Receiver, SyncSender, TryRecvError},
        Arc,
    },
};
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Import,
    Pick,
    Browse,
    Load,
    Save,
}
enum Work {
    Import(String),
    Selected(PathBuf),
    SelectedBytes(Vec<u8>),
    Pick,
    Browse,
    Load,
    Save(Box<AssetIndex>, Option<Verified>),
}
enum Answer {
    Imported(Verified),
    Picked(Option<(String, Verified)>),
    Browsed(Catalog),
    Loaded(Loaded),
    Saved(AssetIndex),
}
struct Job {
    id: u64,
    work: Work,
}
struct Reply {
    id: u64,
    result: Result<Answer, String>,
}
struct Pending {
    id: u64,
    kind: Kind,
    edit: u64,
    candidate: u64,
}
pub struct Imports {
    sender: Option<SyncSender<Job>>,
    receiver: Receiver<Reply>,
    pending: Option<Pending>,
    serial: u64,
    edit: u64,
    candidate: u64,
    pub(super) index: AssetIndex,
    pub(super) saved: Option<AssetIndex>,
    pub(super) accepted: Option<Verified>,
    pub(super) staged: Option<Verified>,
    pub(super) blocked: bool,
    pub(super) message: String,
    pub(super) path: String,
    catalog: Catalog,
    search: String,
    browser_message: String,
    browse_after_load: bool,
    candidate_label: String,
    source: String,
    license: String,
    preview: Option<ImportedAssetPreview>,
    shown: Option<String>,
    show_staged: bool,
}
impl Imports {
    pub fn open(root: &Path) -> Self {
        let root = root.to_path_buf();
        let mut imports = Self::with_worker(move |work| run(&root, work));
        imports.browse_after_load = true;
        imports
    }
    fn with_worker(worker: impl Fn(Work) -> Result<Answer, String> + Send + 'static) -> Self {
        let (sender, jobs) = mpsc::sync_channel::<Job>(1);
        let (results, receiver) = mpsc::sync_channel(1);
        let started = std::thread::Builder::new()
            .name("project-asset-files".into())
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
        let mut s = Self {
            sender: started.ok().map(|_| sender),
            receiver,
            pending: None,
            serial: 0,
            edit: 0,
            candidate: 0,
            index: AssetIndex::default(),
            saved: None,
            accepted: None,
            staged: None,
            blocked: true,
            message: String::new(),
            path: String::new(),
            catalog: Catalog::default(),
            search: String::new(),
            browser_message: String::new(),
            browse_after_load: false,
            candidate_label: String::new(),
            source: String::new(),
            license: String::new(),
            preview: None,
            shown: None,
            show_staged: true,
        };
        s.queue(Work::Load, Kind::Load);
        s
    }
    fn queue(&mut self, work: Work, kind: Kind) -> bool {
        if self.pending.is_some() {
            self.message = "An asset operation is already pending.".into();
            return false;
        }
        self.serial += 1;
        let id = self.serial;
        if self
            .sender
            .as_ref()
            .is_some_and(|s| s.try_send(Job { id, work }).is_ok())
        {
            self.pending = Some(Pending {
                id,
                kind,
                edit: self.edit,
                candidate: self.candidate,
            });
            if kind == Kind::Browse {
                self.browser_message = "Finding project GLBs…".into();
            } else if kind == Kind::Pick {
                self.message =
                    "Choose a GLB in the native file dialog. Cancel leaves the preview unchanged."
                        .into();
            } else {
                self.message = "Checking project asset…".into();
            }
            true
        } else {
            self.message="Asset worker is unavailable or still finishing a cancelled operation. Nothing was retried.".into();
            false
        }
    }
    fn candidate_changed(&mut self, path_changed: bool) {
        self.candidate += 1;
        if path_changed {
            self.staged = None;
            self.show_staged = false;
            if self
                .pending
                .as_ref()
                .is_some_and(|p| matches!(p.kind, Kind::Import | Kind::Pick))
            {
                self.cancel();
            }
            self.candidate_label = self.path.clone();
            self.message =
                "Selection changed. Inspect the selected GLB to review its geometry.".into();
        }
    }
    pub(super) fn stage(&mut self) {
        if self.path.is_empty() {
            self.message = "Select a project GLB or choose Import GLB…".into();
            return;
        }
        self.queue(Work::Import(self.path.clone()), Kind::Import);
    }
    fn select_project(&mut self, relative: &str) {
        if self.path != relative {
            self.path = relative.into();
            self.candidate_changed(true);
        }
    }
    fn selected_file(&mut self, path: PathBuf) {
        if self.pending.is_some() {
            self.message =
                "Finish or cancel the pending asset operation, then import the file again.".into();
            return;
        }
        self.path.clear();
        self.candidate_changed(true);
        self.candidate_label = file_label(&path);
        self.queue(Work::Selected(path), Kind::Import);
    }
    pub(super) fn selected_bytes(&mut self, label: String, bytes: Vec<u8>) -> bool {
        self.poll();
        if self.pending.is_some() {
            self.message =
                "Finish the pending asset operation, then preview the file again.".into();
            return false;
        }
        if !self.queue(Work::SelectedBytes(bytes), Kind::Import) {
            return false;
        }
        self.path.clear();
        self.candidate += 1;
        self.staged = None;
        self.show_staged = false;
        self.candidate_label = label;
        if let Some(pending) = &mut self.pending {
            pending.candidate = self.candidate;
        }
        true
    }
    fn picker_result(&mut self, picked: Option<(String, Verified)>) {
        if let Some((label, verified)) = picked {
            self.path.clear();
            self.candidate_changed(true);
            self.candidate_label = label;
            self.staged = Some(verified);
            self.show_staged = true;
            self.message =
                "Verified staged geometry. Review its report, then Accept preview.".into();
        } else {
            self.message = "No file selected. Previous preview and saved revision retained.".into();
        }
    }
    pub(super) fn accept(&mut self) {
        let Some(v) = &self.staged else {
            return;
        };
        match self
            .index
            .accept(v, self.source.clone(), self.license.clone())
        {
            Ok(index) => {
                self.index = index;
                self.accepted = Some(v.clone());
                self.staged = None;
                self.show_staged = false;
                self.edit += 1;
                self.message = "Accepted local preview. Save to keep this asset revision.".into();
            }
            Err(e) => self.message = e,
        }
    }
    pub(super) fn save(&mut self) {
        if self.blocked {
            self.message =
                "Save locked: repair and Reload or explicitly allow index replacement.".into();
            return;
        }
        self.queue(
            Work::Save(Box::new(self.index.clone()), self.accepted.clone()),
            Kind::Save,
        );
    }
    pub(super) fn cancel(&mut self) {
        if self.pending.as_ref().is_some_and(|p| p.kind == Kind::Save) {
            return;
        }
        let picker = self.pending.as_ref().is_some_and(|p| p.kind == Kind::Pick);
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.kind == Kind::Browse)
        {
            self.browser_message = "Project scan cancelled. Previous list retained.".into();
        }
        self.pending = None;
        self.serial += 1;
        self.staged = None;
        self.show_staged = false;
        self.edit += 1;
        self.message = if picker {
            "Import cancelled. Close the native file dialog if still open. Accepted and saved content retained."
        } else {
            "Staged import cancelled. Accepted and saved content retained."
        }.into();
    }
    pub(super) fn poll(&mut self) {
        loop {
            match self.receiver.try_recv() {
                Ok(reply) => {
                    if self.pending.as_ref().is_none_or(|p| p.id != reply.id) {
                        continue;
                    }
                    let pending = self.pending.take().unwrap();
                    match reply.result {
                        Ok(Answer::Imported(v)) if pending.kind == Kind::Import => {
                            self.staged = Some(v);
                            self.show_staged = true;
                            self.message =
                                "Verified staged geometry. Review its report, then Accept preview."
                                    .into();
                        }
                        Ok(Answer::Picked(picked)) if pending.kind == Kind::Pick => {
                            self.picker_result(picked);
                        }
                        Ok(Answer::Browsed(catalog)) if pending.kind == Kind::Browse => {
                            self.browser_message = format!(
                                "{} project GLBs{}{}",
                                catalog.files.len(),
                                if catalog.truncated {
                                    " · scan limit reached; list is partial"
                                } else {
                                    ""
                                },
                                if catalog.skipped > 0 {
                                    format!(
                                        " · {} unreadable/symlink entries skipped",
                                        catalog.skipped
                                    )
                                } else {
                                    String::new()
                                },
                            );
                            self.catalog = catalog;
                        }
                        Ok(Answer::Saved(index)) if pending.kind == Kind::Save => {
                            self.saved = Some(index);
                            self.message = if self.saved.as_ref() == Some(&self.index) {
                                "Saved accepted asset revision."
                            } else {
                                "Submitted revision saved; newer accepted content is still unsaved."
                            }
                            .into();
                        }
                        Ok(Answer::Loaded(loaded)) if pending.kind == Kind::Load => {
                            self.saved = Some(loaded.index.clone());
                            self.blocked = false;
                            if pending.edit == self.edit {
                                self.index = loaded.index;
                                self.accepted = loaded.verified;
                                self.staged = None;
                                self.show_staged = false;
                                self.shown = None;
                                if pending.candidate == self.candidate {
                                    if let Some(a) = &self.index.asset {
                                        self.source = a.source_reference.clone();
                                        self.license = a.license_reference.clone();
                                    }
                                }
                                self.message=loaded.unavailable.map(|e|format!("Saved asset unavailable: {e}. Import a verified replacement; no fallback was substituted.")).unwrap_or_else(||"Loaded project asset references; geometry rechecked from saved bytes.".into());
                            } else {
                                self.message =
                                    "Saved baseline loaded; newer authoring changes retained."
                                        .into();
                            }
                        }
                        Ok(_) => {
                            self.sender = None;
                            self.blocked = true;
                            self.message =
                                "Mismatched worker result. Save disabled; nothing was replayed."
                                    .into();
                        }
                        Err(e) => {
                            if pending.kind == Kind::Load {
                                self.blocked = true;
                            }
                            if pending.kind == Kind::Browse {
                                self.browser_message = format!("Project scan failed: {e}. Previous list retained; inspection rechecks every file.");
                            } else {
                                self.message = format!("{e}. Previous verified preview retained.");
                            }
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.sender = None;
                    if self.pending.take().is_some() {
                        self.blocked = true;
                        self.message =
                            "Worker result lost. Save disabled; no automatic retry.".into();
                    }
                    break;
                }
            }
        }
        if self.browse_after_load && self.pending.is_none() && self.sender.is_some() {
            self.browse_after_load = false;
            self.queue(Work::Browse, Kind::Browse);
        }
    }
    fn visible(&self) -> Option<&Verified> {
        if self.show_staged {
            self.staged.as_ref().or(self.accepted.as_ref())
        } else {
            self.accepted.as_ref()
        }
    }
    pub fn show(&mut self, ctx: &egui::Context, gl: Option<&Arc<glow::Context>>) {
        self.poll();
        let dropped = ctx.input_mut(|input| std::mem::take(&mut input.raw.dropped_files));
        if !dropped.is_empty() {
            if dropped.len() != 1 {
                self.message = "Drop one GLB at a time; the existing preview was retained.".into();
            } else if let Some(path) = dropped[0].path.clone() {
                self.selected_file(path);
            } else {
                self.message =
                    "This native importer needs a local GLB file. No file was imported.".into();
            }
        }
        egui::SidePanel::left("project_assets").default_width(350.).width_range(300. ..=480.).show(ctx,|ui|{egui::ScrollArea::vertical().show(ui,|ui|{
            ui.heading("Project assets");ui.label("Static GLB preview · project content only");
            self.browser(ui);
            ui.horizontal(|ui|{if ui.add_enabled(self.pending.is_none()&&!self.path.is_empty(),egui::Button::new("Inspect selected")).clicked(){self.stage();}
                if ui.add_enabled(self.staged.is_some()||self.pending.as_ref().is_some_and(|p|p.kind!=Kind::Save),egui::Button::new("Cancel staged")).clicked(){self.cancel();}});
            egui::CollapsingHeader::new("Advanced: relative path").show(ui,|ui|{
                ui.label("Project-relative .glb path");if ui.text_edit_singleline(&mut self.path).changed(){self.candidate_changed(true);}
                ui.small("Manual fallback inside this project; traversal and symlinks are rejected.");
            });
            if !self.candidate_label.is_empty(){ui.label(format!("Candidate: {}",self.candidate_label));}
            ui.small("One accepted preview slot. Selection does not assign geometry to the running world.");
            ui.label("Candidate source reference (blank = unknown)");if ui.text_edit_singleline(&mut self.source).changed(){self.candidate_changed(false);}
            ui.label("Candidate license reference (blank = unknown)");if ui.text_edit_singleline(&mut self.license).changed(){self.candidate_changed(false);}
            ui.horizontal(|ui|{if ui.add_enabled(self.staged.is_some(),egui::Button::new("Accept preview")).clicked(){self.accept();}
                if ui.add_enabled(!self.blocked&&self.pending.is_none()&&self.accepted.is_some(),egui::Button::new("Save assets")).clicked(){self.save();}});
            if ui.add_enabled(self.pending.is_none(),egui::Button::new("Reload assets")).clicked(){self.queue(Work::Load,Kind::Load);}
            if self.blocked&&self.pending.is_none(){ui.colored_label(egui::Color32::YELLOW,"Save locked: asset index could not be verified.");if ui.button("Allow replacing unreadable asset index").clicked(){self.blocked=false;self.saved=None;self.edit+=1;self.message="Replacement allowed. Save will publish the accepted preview; nothing written yet.".into();}}
            if self.pending.is_some(){ui.spinner();}
            ui.label(&self.message);ui.separator();
            if let Some(a)=&self.index.asset {ui.label(format!("Asset #{} · revision {}",a.id,a.revision));ui.monospace(&a.sha256);}
            ui.label(if self.saved.as_ref()==Some(&self.index){"Accepted content saved"}else{"Accepted content unsaved"});
            if self.staged.is_some(){ui.checkbox(&mut self.show_staged,"Show staged geometry");}
            ui.strong(if self.show_staged&&self.staged.is_some(){"Staged geometry report"}else{"Accepted geometry report"});
            if let Some(v)=self.visible(){let r=&v.geometry.report;let d=std::array::from_fn::<_,3,_>(|i|r.max_m[i]-r.min_m[i]);
                ui.label(format!("Measured XYZ: {:.3} × {:.3} × {:.3} m",d[0],d[1],d[2]));ui.label(format!("Origin-relative min: {:?}\nmax: {:?}",r.min_m,r.max_m));ui.label(format!("{} triangles · {} material batches",r.triangles,r.primitives));
                for note in &r.notes {ui.small(note);}ui.collapsing("Material bindings",|ui|{for (i,m) in r.materials.iter().enumerate(){ui.label(format!("Batch {i}: {}",if m.is_empty(){"unnamed"}else{m}));}});
            }else{ui.label("No verified geometry available. Import a GLB to preview; no embedded hull fallback.");}
            ui.separator();ui.small("No source-part hierarchy, collision, physical ratings or live-world replacement. Imported material batches are not construction parts.");
        });});
        let geometry = self.visible().cloned();
        if self.preview.is_none() && gl.is_some() {
            self.preview = Some(ImportedAssetPreview::new(gl));
        }
        if let Some(preview) = &mut self.preview {
            match &geometry {
                Some(v) if self.shown.as_ref() != Some(&v.sha256) => match preview.set(&v.geometry)
                {
                    Ok(()) => self.shown = Some(v.sha256.clone()),
                    Err(e) => self.message = format!("GPU preview unavailable: {e}"),
                },
                None if self.shown.is_some() => {
                    preview.clear();
                    self.shown = None;
                }
                _ => {}
            }
        }
        egui::CentralPanel::default().show(ctx,|ui|{
            ui.heading("Imported geometry");
            if geometry.as_ref().is_some_and(|v| self.shown.as_ref()!=Some(&v.sha256)) && self.preview.is_some() { ui.label("Requested geometry is not displayed: GPU upload failed. The previous GPU allocation is retained for recovery."); } else if let Some(preview)=&mut self.preview {preview.show(ui,geometry.as_ref().map(|v|&v.geometry.report));}else{ui.label("A GPU context is required for actual geometry preview. Validation report remains available.");}
        });
    }
    fn browser(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.pending.is_none(), egui::Button::new("Import GLB…"))
                .clicked()
            {
                self.queue(Work::Pick, Kind::Pick);
            }
            if ui
                .add_enabled(self.pending.is_none(), egui::Button::new("Refresh files"))
                .clicked()
            {
                self.queue(Work::Browse, Kind::Browse);
            }
        });
        ui.small("Choose a project file below, import a file, or drop one GLB here. Save copies verified bytes into this project.");
        ui.small("Type: GLB models (.glb)");
        if let Some(asset) = &self.index.asset {
            if ui
                .selectable_label(
                    !self.show_staged && self.accepted.is_some(),
                    format!("Accepted asset #{} · revision {}", asset.id, asset.revision),
                )
                .clicked()
            {
                self.show_staged = false;
            }
        }
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search project GLBs"));
        let files = self
            .catalog
            .matching(&self.search)
            .cloned()
            .collect::<Vec<_>>();
        egui::ScrollArea::vertical()
            .id_salt("project_glb_choices")
            .max_height(160.)
            .show(ui, |ui| {
                if files.is_empty() {
                    ui.label(if self.catalog.files.is_empty() {
                        "No project GLBs found. Import GLB… adds a candidate."
                    } else {
                        "No GLBs match this search."
                    });
                }
                for file in files {
                    let label =
                        format!("{}  ·  {:.1} KiB", file.relative, file.bytes as f64 / 1024.);
                    let selected = ui.selectable_label(self.path == file.relative, label);
                    if selected.clicked() {
                        self.select_project(&file.relative);
                    }
                    if selected.double_clicked() && self.pending.is_none() {
                        self.select_project(&file.relative);
                        self.stage();
                    }
                }
            });
        if !self.browser_message.is_empty() {
            ui.small(&self.browser_message);
        }
    }
    pub fn destroy(&mut self) {
        if let Some(mut p) = self.preview.take() {
            p.destroy();
        }
    }
}
fn run(root: &Path, work: Work) -> Result<Answer, String> {
    match work {
        Work::Import(path) => doc::import(root, &path).map(Answer::Imported),
        Work::Selected(path) => doc::import_selected(&path).map(Answer::Imported),
        Work::SelectedBytes(bytes) => Verified::decode(bytes).map(Answer::Imported),
        Work::Browse => doc::catalog(root).map(Answer::Browsed),
        Work::Pick => {
            // RFD supports worker-thread dialogs in a windowed macOS app. Its
            // native backend dispatches to AppKit; no file I/O blocks egui.
            // https://docs.rs/rfd/0.17.2/rfd/#macos-non-windowed-applications-async-and-threading
            let picked = rfd::FileDialog::new()
                .set_title("Import a GLB into this project")
                .add_filter("GLB model", &["glb"])
                .set_directory(root)
                .pick_file();
            picked
                .map(|path| doc::import_selected(&path).map(|v| (file_label(&path), v)))
                .transpose()
                .map(Answer::Picked)
        }
        Work::Load => doc::load(root).map(Answer::Loaded),
        Work::Save(index, v) => {
            doc::save(root, &index, v.as_ref())?;
            Ok(Answer::Saved(*index))
        }
    }
}
fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Selected GLB".into())
}
#[cfg(test)]
mod byte_handoff_tests {
    use super::*;
    use std::time::{Duration, Instant};
    const MODEL: &[u8] = include_bytes!("../tests/fixtures/import/wedge.glb");

    fn empty_loaded() -> Answer {
        Answer::Loaded(Loaded {
            index: AssetIndex::default(),
            verified: None,
            unavailable: None,
        })
    }

    fn settle(panel: &mut Imports) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while panel.pending.is_some() {
            panel.poll();
            assert!(Instant::now() < deadline, "asset worker did not settle");
            std::thread::yield_now();
        }
    }

    #[test]
    fn files_handoff_drains_initial_load_and_only_stages_verified_bytes() {
        let mut panel = Imports::with_worker(|work| match work {
            Work::Load => Ok(empty_loaded()),
            Work::SelectedBytes(bytes) => Verified::decode(bytes).map(Answer::Imported),
            _ => panic!("handoff must not accept or save an asset"),
        });
        assert!(panel.pending.as_ref().is_some_and(|p| p.kind == Kind::Load));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !panel.selected_bytes("fixture.glb".into(), MODEL.to_vec()) {
            assert!(Instant::now() < deadline, "initial load was never drained");
            std::thread::yield_now();
        }
        settle(&mut panel);
        assert_eq!(panel.candidate_label, "fixture.glb");
        assert!(panel.staged.is_some(), "{}", panel.message);
        assert!(panel.accepted.is_none());
        assert!(panel.index.asset.is_none());
        assert!(panel.saved.as_ref().unwrap().asset.is_none());
    }

    #[test]
    fn files_handoff_preserves_candidate_while_an_asset_save_is_pending() {
        let (started, saving) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let mut panel = Imports::with_worker(move |work| match work {
            Work::Load => Ok(empty_loaded()),
            Work::Save(index, _) => {
                started.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(3)).unwrap();
                Ok(Answer::Saved(*index))
            }
            _ => panic!("busy save must not be replaced by an import"),
        });
        settle(&mut panel);
        panel.staged = Some(Verified::decode(MODEL.to_vec()).unwrap());
        panel.candidate_label = "retained.glb".into();
        panel.queue(Work::Save(Box::new(panel.index.clone()), None), Kind::Save);
        saving.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(!panel.selected_bytes("replacement.glb".into(), MODEL.to_vec()));
        assert_eq!(panel.candidate_label, "retained.glb");
        assert!(panel.staged.is_some());
        assert!(panel.pending.as_ref().is_some_and(|p| p.kind == Kind::Save));
        release.send(()).unwrap();
        settle(&mut panel);
    }

    #[test]
    fn unavailable_handoff_does_not_clear_the_previous_candidate() {
        let mut panel = Imports::with_worker(|_| Ok(empty_loaded()));
        settle(&mut panel);
        panel.staged = Some(Verified::decode(MODEL.to_vec()).unwrap());
        panel.candidate_label = "retained.glb".into();
        panel.sender = None;
        assert!(!panel.selected_bytes("replacement.glb".into(), MODEL.to_vec()));
        assert_eq!(panel.candidate_label, "retained.glb");
        assert!(panel.staged.is_some());
        assert!(panel.pending.is_none());
    }
}
#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
