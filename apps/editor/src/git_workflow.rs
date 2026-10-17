//! Explicit native Git operations. This panel never replaces the authoring project or buffers.
mod ops;
#[cfg(test)]
mod tests;

use eframe::egui::{self, RichText};
use ops::{Outcome, Request, Snapshot};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
        Arc,
    },
};

pub struct GitWorkspace {
    repository: String,
    clone_url: String,
    clone_destination: String,
    transport_trusted: bool,
    trusted: bool,
    snapshot: Option<Snapshot>,
    selected: BTreeSet<String>,
    branch_name: String,
    switch_branch: String,
    remote: String,
    message: String,
    commit_message: String,
    diff: String,
    staged_diff: bool,
    handoff: Option<String>,
    worker: Option<Receiver<Result<Outcome, String>>>,
    cancellation: Option<Arc<AtomicBool>>,
    job: String,
    needs_refresh: bool,
    push_review: bool,
}

impl GitWorkspace {
    pub fn open(project: &Path) -> Self {
        Self {
            repository: project.display().to_string(),
            clone_url: String::new(),
            clone_destination: String::new(),
            transport_trusted: false,
            trusted: false,
            snapshot: None,
            selected: BTreeSet::new(),
            branch_name: String::new(),
            switch_branch: String::new(),
            remote: String::new(),
            message: "Open a checkout to inspect its saved files.".into(),
            commit_message: String::new(),
            diff: String::new(),
            staged_diff: false,
            handoff: None,
            worker: None,
            cancellation: None,
            job: String::new(),
            needs_refresh: false,
            push_review: false,
        }
    }

    fn busy(&self) -> bool {
        self.worker.is_some()
    }

    fn start(&mut self, request: Request, label: &str) {
        if self.busy() {
            return;
        }
        self.handoff = None;
        self.push_review = false;
        let (tx, rx) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        match std::thread::Builder::new()
            .name("arkline-git-workflow".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(|| ops::execute(request, &worker_cancel))
                    .unwrap_or_else(|_| {
                        Err("Git worker stopped unexpectedly. Refresh status.".into())
                    });
                let _ = tx.send(result);
            }) {
            Ok(_) => {
                self.worker = Some(rx);
                self.cancellation = Some(cancel);
                self.job = label.into();
                self.message = format!("{label}…");
            }
            Err(_) => {
                self.message = "Could not start the Git worker. No operation was started.".into()
            }
        }
    }

    pub fn poll(&mut self) {
        let result = match self.worker.as_ref().map(Receiver::try_recv) {
            Some(Ok(result)) => result,
            Some(Err(TryRecvError::Disconnected)) => {
                Err("Git worker disconnected. Refresh status before continuing.".into())
            }
            _ => return,
        };
        self.worker = None;
        self.cancellation = None;
        self.job.clear();
        match result {
            Ok(outcome) => {
                self.message = outcome.message;
                if let Some(snapshot) = outcome.snapshot {
                    self.diff.clear();
                    let changed_repo =
                        self.snapshot.as_ref().map(|s| &s.repo) != Some(&snapshot.repo);
                    if changed_repo {
                        self.trusted = false;
                        self.selected.clear();
                        self.diff.clear();
                    }
                    self.repository = snapshot.repo.display().to_string();
                    self.selected
                        .retain(|path| snapshot.entries.iter().any(|e| &e.path == path));
                    self.switch_branch.clone_from(&snapshot.branch);
                    if !snapshot.remotes.iter().any(|r| r.name == self.remote) {
                        self.remote = snapshot
                            .remotes
                            .first()
                            .map(|r| r.name.clone())
                            .unwrap_or_default();
                    }
                    self.snapshot = Some(snapshot);
                    self.needs_refresh = false;
                }
                if let Some(diff) = outcome.diff {
                    self.diff = diff;
                    const PREVIEW_LIMIT: usize = 256 * 1024;
                    if self.diff.len() > PREVIEW_LIMIT {
                        let mut end = PREVIEW_LIMIT;
                        while !self.diff.is_char_boundary(end) {
                            end -= 1;
                        }
                        self.diff.truncate(end);
                        self.diff.push_str("\n… Preview limited to 256 KiB. Inspect the full diff in your Git client.\n");
                    }
                }
                self.handoff = outcome.handoff_url;
            }
            Err(error) => {
                self.message = error;
                self.handoff = None;
                self.needs_refresh = true;
            }
        }
    }

    fn paths(&self) -> Vec<String> {
        self.selected.iter().cloned().collect()
    }

    fn ready(&self) -> bool {
        !self.busy()
            && !self.needs_refresh
            && self.trusted
            && self
                .snapshot
                .as_ref()
                .is_some_and(|s| self.repository == s.repo.display().to_string())
    }

    pub fn show(&mut self, ctx: &egui::Context, project: &Path) {
        egui::TopBottomPanel::top("git_repository_controls").show(ctx, |ui| {
            ui.add_space(6.);
            ui.horizontal_wrapped(|ui| {
                ui.heading("Git workspace");
                ui.small("Saved files → Stage → Commit → Push → Pull request");
            });
            ui.small(format!("Authoring project stays open: {}", project.display()));
            ui.small("Git does not save, apply, reload or discard your authoring buffers.");
            ui.add_enabled_ui(!self.busy(), |ui| {
                ui.horizontal(|ui| {
                    ui.label("Checkout");
                    if ui.add(egui::TextEdit::singleline(&mut self.repository).desired_width(570.)).changed() {
                        self.trusted = false;
                        self.handoff = None;
                    }
                    if ui.button("Browse checkout…").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.repository = path.display().to_string();
                            self.trusted = false;
                            self.handoff = None;
                        }
                    }
                    if ui.button("Open / Refresh").clicked() {
                        self.start(Request::Inspect { path: PathBuf::from(&self.repository) }, "Inspecting checkout");
                    }
                });
                egui::CollapsingHeader::new("Clone a repository").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Source");
                        if ui.add(egui::TextEdit::singleline(&mut self.clone_url).desired_width(570.)
                            .hint_text("HTTPS, SSH or a local repository path")).changed() {
                            self.transport_trusted = false;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("New folder");
                        ui.add(egui::TextEdit::singleline(&mut self.clone_destination).desired_width(570.));
                        if ui.button("Choose parent…").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                self.clone_destination = path.join("contribution-checkout").display().to_string();
                            }
                        }
                    });
                    ui.checkbox(&mut self.transport_trusted, "Use my configured Git transport helpers for this source");
                    ui.small("Destination must not exist. Cancelled or failed clones may leave a partial folder for you to inspect.");
                    if ui.add_enabled(self.transport_trusted && !self.clone_url.trim().is_empty()
                        && !self.clone_destination.trim().is_empty(), egui::Button::new("Clone into new folder")).clicked() {
                        self.start(Request::Clone { url: self.clone_url.clone(), destination: PathBuf::from(&self.clone_destination) }, "Cloning repository");
                    }
                });
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if self.busy() {
                    ui.spinner();
                    ui.label(&self.job);
                    let cancelling = self.cancellation.as_ref().is_some_and(|c| c.load(Ordering::Relaxed));
                    if ui.add_enabled(!cancelling, egui::Button::new("Cancel Git job")).clicked() {
                        if let Some(cancel) = &self.cancellation {
                            cancel.store(true, Ordering::Relaxed);
                        }
                    }
                    if cancelling { ui.small("Cancelling; wait for the current job to finish before starting another."); }
                }
                ui.label(&self.message);
            });
        });

        let Some(snapshot) = self.snapshot.clone() else {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_space(25.);
                ui.heading("Open a local checkout or clone a repository");
                ui.label("Nothing is staged, committed or pushed automatically.");
            });
            return;
        };
        egui::SidePanel::left("git_changes_list")
            .default_width(365.)
            .width_range(290. ..=520.)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Changes");
                ui.monospace(snapshot.repo.display().to_string());
                ui.label(format!("Branch: {}", if snapshot.branch.is_empty() { "detached HEAD" } else { &snapshot.branch }));
                ui.small(format!("HEAD: {}", if snapshot.head.is_empty() { "unborn branch" } else { &snapshot.head }));
                ui.small("I = index · W = working file · ? = untracked");
                if snapshot.entries.is_empty() { ui.label("Working tree clean"); }
                egui::ScrollArea::vertical().id_salt("git_change_entries")
                    .max_height((ui.available_height() - 185.).max(100.)).show(ui, |ui| {
                    for entry in &snapshot.entries {
                        ui.push_id(&entry.path, |ui| {
                            let mut selected = self.selected.contains(&entry.path);
                            let name = display_path(&entry.path);
                            let rename = entry.original_path.as_ref().map(|p| format!("{} → ", display_path(p))).unwrap_or_default();
                            let label = format!("{}{}  {rename}{name}", entry.index, entry.worktree);
                            if ui.add_enabled(!self.busy(), egui::Checkbox::new(&mut selected, label)).changed() {
                                self.diff.clear();
                                if selected { self.selected.insert(entry.path.clone()); }
                                else { self.selected.remove(&entry.path); }
                            }
                            if entry.conflicted() { ui.colored_label(egui::Color32::YELLOW, "Resolve conflicts outside the editor, then Refresh."); }
                        });
                    }
                });
                ui.separator();
                ui.label(format!("{} selected", self.selected.len()));
                ui.add_enabled_ui(!self.busy(), |ui| {
                    ui.horizontal(|ui| {
                        let working_changed = ui.selectable_value(&mut self.staged_diff, false, "Working diff").changed();
                        let staged_changed = ui.selectable_value(&mut self.staged_diff, true, "Staged diff").changed();
                        if working_changed || staged_changed {
                            self.diff.clear();
                        }
                    });
                    if ui.add_enabled(!self.selected.is_empty(), egui::Button::new("Inspect selected diff")).clicked() {
                        self.start(Request::Diff { snapshot: snapshot.clone(), paths: self.paths(), staged: self.staged_diff }, "Reading selected diff");
                    }
                });
                ui.small("Inspection disables external diff, text conversion and filesystem-monitor hooks.");
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().id_salt("git_actions_and_diff").show(ui, |ui| {
                ui.add_enabled_ui(!self.busy(), |ui| {
                    ui.checkbox(&mut self.trusted, "Trust this checkout and its configured credential / signing helpers");
                });
                ui.small("Git hooks are disabled for these operations. Filtered/LFS files and conflict resolution require an external Git client.");
                if self.needs_refresh { ui.colored_label(egui::Color32::YELLOW, "Refresh status before another mutation."); }
                if self.repository != snapshot.repo.display().to_string() {
                    ui.colored_label(egui::Color32::YELLOW, "Open the edited path before operating on it. The inspected checkout is shown on the left.");
                }
                let ready = self.ready();
                ui.add_enabled_ui(ready, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.branch_name).hint_text("contribution/my-change").desired_width(260.));
                        if ui.add_enabled(!self.branch_name.trim().is_empty(), egui::Button::new("Create branch")).clicked() {
                            self.start(Request::CreateBranch { snapshot: snapshot.clone(), name: self.branch_name.clone() }, "Creating contribution branch");
                        }
                        egui::ComboBox::from_id_salt("git_branch_choice").selected_text(&self.switch_branch).show_ui(ui, |ui| {
                            for branch in &snapshot.branches { ui.selectable_value(&mut self.switch_branch, branch.clone(), branch); }
                        });
                        if ui.add_enabled(!self.switch_branch.is_empty() && self.switch_branch != snapshot.branch,
                            egui::Button::new("Switch branch")).clicked() {
                            self.start(Request::SwitchBranch { snapshot: snapshot.clone(), name: self.switch_branch.clone() }, "Switching branch");
                        }
                    });
                    ui.small("Branch operations require a clean checkout. Authoring buffers remain in memory and are never reloaded.");
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.selected.is_empty(), egui::Button::new("Stage selected files")).clicked() {
                            self.start(Request::Stage { snapshot: snapshot.clone(), paths: self.paths() }, "Staging selected files");
                        }
                        if ui.add_enabled(!self.selected.is_empty(), egui::Button::new("Unstage selected files")).clicked() {
                            self.start(Request::Unstage { snapshot: snapshot.clone(), paths: self.paths() }, "Unstaging selected files");
                        }
                    });
                    ui.add(egui::TextEdit::multiline(&mut self.commit_message).desired_rows(2)
                        .desired_width(f32::INFINITY).hint_text("Describe this contribution"));
                    ui.small("Commit includes only the reviewed selection. Other staged files or partial staging block the commit safely.");
                    if ui.add_enabled(!self.selected.is_empty() && !self.commit_message.trim().is_empty(),
                        egui::Button::new("Commit selected staged files")).clicked() {
                        self.start(Request::Commit { snapshot: snapshot.clone(), paths: self.paths(), message: self.commit_message.clone() }, "Committing selected staged files");
                    }
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Push remote");
                        egui::ComboBox::from_id_salt("git_push_remote").selected_text(&self.remote).show_ui(ui, |ui| {
                            for remote in &snapshot.remotes {
                                if ui.selectable_value(&mut self.remote, remote.name.clone(), &remote.name).changed() {
                                    self.handoff = None;
                                }
                            }
                        });
                        if ui.add_enabled(!self.remote.is_empty() && !snapshot.branch.is_empty() && !snapshot.head.is_empty(), egui::Button::new("Review push…")).clicked() {
                            self.push_review = true;
                        }
                        if ui.add_enabled(!self.remote.is_empty() && !snapshot.branch.is_empty() && !snapshot.head.is_empty(), egui::Button::new("Verify PR handoff")).clicked() {
                            self.start(Request::PrHandoff { snapshot: snapshot.clone(), remote: self.remote.clone() }, "Verifying GitHub branch");
                        }
                    });
                });
                if let Some(remote) = snapshot.remotes.iter().find(|r| r.name == self.remote) {
                    ui.monospace(format!("{} → {}", remote.name, remote.push_url));
                    ui.small(format!("Destination branch: {} · normal push only", snapshot.branch));
                } else { ui.small("No push remote configured. Configure it in your Git client, then Refresh."); }
                if let Some(url) = &self.handoff {
                    ui.separator();
                    ui.label(RichText::new("Remote commit matched HEAD when verified").color(crate::GREEN));
                    ui.hyperlink_to("Open GitHub PR form", url);
                    ui.small("Choose Create draft pull request on GitHub. Opening this link does not submit a PR.");
                }
                ui.separator();
                ui.heading(if self.staged_diff { "Selected staged diff" } else { "Selected working diff" });
                ui.small("Binary files have no text preview. LFS content is not downloaded here.");
                ui.add(egui::TextEdit::multiline(&mut self.diff).code_editor().interactive(false)
                    .desired_width(f32::INFINITY).desired_rows(20));
            });
        });

        if self.push_review {
            egui::Window::new("Push this branch?").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label("This sends committed source to the displayed destination.");
                ui.monospace(snapshot.repo.display().to_string());
                ui.monospace(format!("{} · {}", snapshot.branch, snapshot.head));
                if let Some(remote) = snapshot.remotes.iter().find(|r| r.name == self.remote) {
                    ui.monospace(format!("{} → {}", remote.name, remote.push_url));
                }
                ui.small("No force push. Working files and unsaved authoring buffers are not included.");
                ui.horizontal(|ui| {
                    if ui.button("Back").clicked() { self.push_review = false; }
                    if ui.add_enabled(self.ready(), egui::Button::new("Push branch")).clicked() {
                        self.start(Request::Push { snapshot: snapshot.clone(), remote: self.remote.clone() }, "Pushing and verifying branch");
                    }
                });
            });
        }
    }
}

impl Drop for GitWorkspace {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancellation {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

fn display_path(path: &str) -> String {
    path.replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod worker_tests {
    use super::*;

    #[test]
    fn repeat_click_and_cancel_cannot_start_an_overlapping_git_job() {
        let mut workspace = GitWorkspace::open(Path::new("authoring-project"));
        let (tx, rx) = mpsc::sync_channel(1);
        let cancellation = Arc::new(AtomicBool::new(false));
        workspace.worker = Some(rx);
        workspace.cancellation = Some(cancellation.clone());
        workspace.job = "First job".into();
        workspace.start(
            Request::Inspect {
                path: PathBuf::from("never-opened"),
            },
            "Second job",
        );
        assert_eq!(workspace.job, "First job");
        cancellation.store(true, Ordering::Relaxed);
        workspace.poll();
        assert!(
            workspace.busy(),
            "cancellation must wait for the worker result"
        );
        workspace.start(
            Request::Inspect {
                path: PathBuf::from("never-opened"),
            },
            "Third job",
        );
        assert_eq!(workspace.job, "First job");
        tx.send(Err("Cancelled; refresh status".into())).unwrap();
        workspace.poll();
        assert!(!workspace.busy());
        assert!(workspace.needs_refresh);
        assert!(workspace.handoff.is_none());
        assert_eq!(workspace.repository, "authoring-project");
    }
}
