//! One bounded background worker per project; staged preview is separate from accepted/saved.
use crate::import_document::{self as doc, AssetIndex, Loaded, Verified};
use arkline_client::ImportedAssetPreview;
use eframe::{egui, glow};
use std::{
    path::Path,
    sync::{
        mpsc::{self, Receiver, SyncSender, TryRecvError},
        Arc,
    },
};
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Import,
    Load,
    Save,
}
enum Work {
    Import(String),
    Load,
    Save(Box<AssetIndex>, Option<Verified>),
}
enum Answer {
    Imported(Verified),
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
    source: String,
    license: String,
    preview: Option<ImportedAssetPreview>,
    shown: Option<String>,
    show_staged: bool,
}
impl Imports {
    pub fn open(root: &Path) -> Self {
        let root = root.to_path_buf();
        Self::with_worker(move |work| run(&root, work))
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
            source: String::new(),
            license: String::new(),
            preview: None,
            shown: None,
            show_staged: true,
        };
        s.queue(Work::Load, Kind::Load);
        s
    }
    fn queue(&mut self, work: Work, kind: Kind) {
        if self.pending.is_some() {
            self.message = "An asset operation is already pending.".into();
            return;
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
            self.message = "Checking project asset…".into();
        } else {
            self.message="Asset worker is unavailable or still finishing a cancelled operation. Nothing was retried.".into();
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
                .is_some_and(|p| p.kind == Kind::Import)
            {
                self.cancel();
            }
        }
    }
    pub(super) fn stage(&mut self) {
        self.queue(Work::Import(self.path.clone()), Kind::Import);
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
        self.pending = None;
        self.serial += 1;
        self.staged = None;
        self.show_staged = false;
        self.edit += 1;
        self.message = "Staged import cancelled. Accepted and saved content retained.".into();
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
                            self.message = format!("{e}. Previous verified preview retained.");
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
        egui::SidePanel::left("project_assets").default_width(350.).width_range(300. ..=480.).show(ctx,|ui|{egui::ScrollArea::vertical().show(ui,|ui|{
            ui.heading("Project assets");ui.label("Static GLB preview · project content only");
            ui.label("Project-relative .glb path");if ui.text_edit_singleline(&mut self.path).changed(){self.candidate_changed(true);}
            ui.small("Place the file inside this project. One asset slot; Import replaces its preview candidate.");
            ui.horizontal(|ui|{if ui.add_enabled(self.pending.is_none(),egui::Button::new("Inspect GLB")).clicked(){self.stage();}
                if ui.add_enabled(self.staged.is_some()||self.pending.as_ref().is_some_and(|p|p.kind!=Kind::Save),egui::Button::new("Cancel staged")).clicked(){self.cancel();}});
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
    pub fn destroy(&mut self) {
        if let Some(mut p) = self.preview.take() {
            p.destroy();
        }
    }
}
fn run(root: &Path, work: Work) -> Result<Answer, String> {
    match work {
        Work::Import(path) => doc::import(root, &path).map(Answer::Imported),
        Work::Load => doc::load(root).map(Answer::Loaded),
        Work::Save(index, v) => {
            doc::save(root, &index, v.as_ref())?;
            Ok(Answer::Saved(*index))
        }
    }
}
#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
