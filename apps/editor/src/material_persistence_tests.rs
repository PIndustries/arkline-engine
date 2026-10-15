use super::*;
use std::{
    fs,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    sync::Arc,
    time::{Duration, Instant},
};

static PROJECT_ID: AtomicU64 = AtomicU64::new(0);
pub(super) struct Project(pub(super) PathBuf);
impl Project {
    pub(super) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "arkline-material-io-{}-{}",
            std::process::id(),
            PROJECT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self) -> PathBuf {
        self.0.join("Assets/Pioneer.material.json")
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub(super) fn settle(panel: &mut Materials) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while panel.pending.is_some() && Instant::now() < deadline {
        panel.poll_io();
        thread::sleep(Duration::from_millis(1));
    }
    assert!(panel.pending.is_none(), "material I/O did not finish");
}
fn edited(value: f32) -> MaterialDocument {
    MaterialDocument {
        emissive_strength: value,
        ..Default::default()
    }
}

#[test]
fn save_keeps_submitted_snapshot_and_ui_runs_while_storage_is_blocked() {
    let project = Project::new();
    let root = project.0.clone();
    let (started_tx, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let finished = Arc::new(AtomicBool::new(false));
    let worker_finished = finished.clone();
    let mut panel = Materials::with_worker(&project.0, move |action| match action {
        IoAction::Open => Ok(None),
        IoAction::Save(document) => {
            started_tx.send(document.clone()).unwrap();
            let unblocked = gate.recv_timeout(Duration::from_secs(5));
            worker_finished.store(true, Ordering::SeqCst);
            unblocked.map_err(|_| "test storage deadline".to_string())?;
            asset_document::save(&root, &document).map(|()| Some(document))
        }
        IoAction::Reload => unreachable!(),
    });
    settle(&mut panel);
    panel.apply(edited(2.));
    panel.save(&project.0);
    let submitted = started.recv_timeout(Duration::from_secs(5)).unwrap();
    panel.apply(edited(3.));
    panel.move_history(false);
    panel.move_history(true);
    let newer = panel.history.current().clone();
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| panel.show_graph(ui, &project.0));
    });
    assert!(
        !finished.load(Ordering::SeqCst),
        "the UI waited for storage"
    );
    assert!(panel.pending.is_some());
    panel.save(&project.0); // A second Save cannot queue or change the submitted graph.
    release.send(()).unwrap();
    settle(&mut panel);
    assert_eq!(panel.saved.as_ref(), Some(&submitted));
    assert_eq!(asset_document::load(&project.0).unwrap(), submitted);
    assert_eq!(panel.history.current(), &newer);
    assert!(panel.is_dirty());
    assert!(panel.message.contains("newer edits"));
}

#[test]
fn startup_is_async_and_preserves_early_edits_and_disabled_preview() {
    let project = Project::new();
    let (started_tx, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut panel = Materials::with_worker(&project.0, move |_| {
        started_tx.send(()).unwrap();
        gate.recv_timeout(Duration::from_secs(5)).unwrap();
        Ok(Some(edited(2.)))
    });
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(panel.preview().is_none());
    panel.apply(edited(3.));
    panel.enabled = false;
    panel.preview_revision += 1;
    release.send(()).unwrap();
    settle(&mut panel);
    assert_eq!(panel.history.current(), &edited(3.));
    assert_eq!(panel.saved.as_ref(), Some(&edited(2.)));
    assert!(panel.preview().is_none());
    assert!(panel.is_dirty());
}

#[test]
fn stale_reload_keeps_undo_redo_even_when_document_returns_to_starting_value() {
    let project = Project::new();
    let (started_tx, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut panel = Materials::with_worker(&project.0, move |action| match action {
        IoAction::Open => Ok(Some(edited(1.))),
        IoAction::Reload => {
            started_tx.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(Some(edited(4.)))
        }
        _ => unreachable!(),
    });
    settle(&mut panel);
    panel.reload(&project.0);
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    panel.apply(edited(2.));
    panel.move_history(false);
    assert_eq!(panel.history.current(), &edited(1.));
    release.send(()).unwrap();
    settle(&mut panel);
    assert_eq!(panel.history.current(), &edited(1.));
    assert_eq!(panel.saved.as_ref(), Some(&edited(4.)));
    assert!(panel.history.can_redo());
    panel.move_history(true);
    assert_eq!(panel.history.current(), &edited(2.));
}

#[test]
fn late_reload_preserves_an_uncommitted_node_drag() {
    let project = Project::new();
    let (release, gate) = mpsc::channel();
    let mut panel = Materials::with_worker(&project.0, move |action| match action {
        IoAction::Open => Ok(Some(edited(1.))),
        IoAction::Reload => {
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(Some(edited(4.)))
        }
        _ => unreachable!(),
    });
    settle(&mut panel);
    panel.reload(&project.0);
    panel.drag_document = Some(edited(3.));
    release.send(()).unwrap();
    settle(&mut panel);
    assert_eq!(panel.drag_document.as_ref(), Some(&edited(3.)));
    assert_eq!(panel.history.current(), &edited(1.));
    assert_eq!(panel.saved.as_ref(), Some(&edited(4.)));
    assert!(panel.is_dirty());
}

#[test]
fn reload_is_undoable_and_restores_legacy_material_without_an_edit_in_flight() {
    let project = Project::new();
    asset_document::save(&project.0, &edited(1.)).unwrap();
    let mut panel = Materials::open(&project.0);
    settle(&mut panel);
    assert!(!panel.history.can_undo());
    panel.apply(edited(2.));
    panel.reload(&project.0);
    settle(&mut panel);
    assert!(!panel.is_dirty());
    panel.move_history(false);
    assert_eq!(panel.history.current(), &edited(2.));
    panel.move_history(true);
    assert_eq!(panel.history.current(), &edited(1.));
}

#[test]
fn corrupt_file_stays_untouched_until_explicit_replacement() {
    let project = Project::new();
    fs::create_dir(project.0.join("Assets")).unwrap();
    let broken = b"{broken material";
    fs::write(project.file(), broken).unwrap();
    let mut panel = Materials::open(&project.0);
    settle(&mut panel);
    assert!(panel.load_blocked);
    panel.apply(edited(3.));
    panel.save(&project.0);
    assert!(panel.pending.is_none());
    assert_eq!(fs::read(project.file()).unwrap(), broken);
    let draft = panel.history.current().clone();
    panel.allow_replacement();
    assert_eq!(panel.history.current(), &draft);
    assert_eq!(fs::read(project.file()).unwrap(), broken);
    panel.save(&project.0);
    settle(&mut panel);
    assert_eq!(
        asset_document::load(&project.0).unwrap(),
        *panel.history.current()
    );
    assert!(!panel.is_dirty());
}

#[test]
fn reload_failure_protects_last_good_draft_and_successful_reload_unlocks() {
    let project = Project::new();
    asset_document::save(&project.0, &edited(1.)).unwrap();
    let mut panel = Materials::open(&project.0);
    settle(&mut panel);
    panel.apply(edited(2.));
    fs::write(project.file(), b"broken").unwrap();
    panel.reload(&project.0);
    settle(&mut panel);
    assert_eq!(panel.history.current(), &edited(2.));
    assert_eq!(panel.saved.as_ref(), Some(&edited(1.)));
    assert!(!panel.can_save());
    asset_document::save(&project.0, &edited(4.)).unwrap();
    panel.reload(&project.0);
    settle(&mut panel);
    assert_eq!(panel.history.current(), &edited(4.));
    assert!(panel.can_save());
}

#[test]
fn failed_save_keeps_saved_snapshot_and_newer_draft() {
    let project = Project::new();
    let mut panel = Materials::with_worker(&project.0, |action| match action {
        IoAction::Open => Ok(Some(edited(1.))),
        IoAction::Save(_) => Err("storage unavailable".into()),
        _ => unreachable!(),
    });
    settle(&mut panel);
    panel.apply(edited(2.));
    panel.save(&project.0);
    let submitted = panel.history.current().clone();
    settle(&mut panel);
    assert_eq!(panel.history.current(), &submitted);
    assert_eq!(panel.saved.as_ref(), Some(&edited(1.)));
    assert!(panel.message.contains("storage unavailable"));
    assert!(panel.is_dirty());
    assert!(!project.file().exists());
}

#[test]
fn different_project_cannot_receive_this_panels_operations() {
    let original = Project::new();
    let other = Project::new();
    let mut panel = Materials::open(&original.0);
    settle(&mut panel);
    panel.apply(edited(2.));
    let before = panel.history.current().clone();
    panel.save(&other.0);
    panel.reload(&other.0);
    assert!(panel.pending.is_none());
    assert_eq!(panel.history.current(), &before);
    assert!(!original.file().exists());
    assert!(!other.file().exists());
    assert!(panel.message.contains("different project"));
}

#[test]
fn mismatched_receipt_locks_saving_without_accepting_other_document() {
    let project = Project::new();
    let mut panel = Materials::open(&project.0);
    settle(&mut panel);
    let (requests, _held_requests) = mpsc::sync_channel(1);
    let (results, receiver) = mpsc::sync_channel(1);
    panel.io_requests = Some(requests);
    panel.io_results = receiver;
    panel.save(&project.0);
    results
        .send(IoResult {
            id: panel.pending.as_ref().unwrap().id + 1,
            kind: IoKind::Save,
            result: Ok(Some(edited(4.))),
        })
        .unwrap();
    panel.poll_io();
    assert!(panel.load_blocked);
    assert!(panel.saved.is_none());
    assert!(panel.io_requests.is_none());
    assert!(!project.file().exists());
}

#[test]
fn lost_save_worker_reports_unknown_outcome_without_retry() {
    let project = Project::new();
    let mut panel = Materials::open(&project.0);
    settle(&mut panel);
    let (requests, held_requests) = mpsc::sync_channel(1);
    let (results, receiver) = mpsc::sync_channel(1);
    panel.io_requests = Some(requests);
    panel.io_results = receiver;
    panel.save(&project.0);
    assert!(held_requests.try_recv().is_ok());
    drop(results);
    panel.poll_io();
    assert!(panel.load_blocked);
    assert!(panel.message.contains("may have completed"));
    panel.save(&project.0);
    assert!(held_requests.try_recv().is_err());
}

#[cfg(unix)]
#[test]
fn broken_symlinks_are_errors_and_recovery_cannot_follow_them() {
    use std::os::unix::fs::symlink;
    let project = Project::new();
    fs::create_dir(project.0.join("Assets")).unwrap();
    let target = project.0.join("missing-target");
    symlink(&target, project.file()).unwrap();
    assert!(asset_document::load_optional(&project.0).is_err());
    let mut panel = Materials::open(&project.0);
    settle(&mut panel);
    assert!(panel.load_blocked);
    panel.allow_replacement();
    panel.save(&project.0);
    settle(&mut panel);
    assert!(fs::symlink_metadata(project.file())
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!target.exists());
    assert!(panel.saved.is_none());
}

#[test]
fn missing_material_is_not_a_load_error_or_a_claim_that_a_file_was_saved() {
    let project = Project::new();
    assert_eq!(asset_document::load_optional(&project.0).unwrap(), None);
    let mut panel = Materials::open(&project.0);
    settle(&mut panel);
    assert!(panel.can_save());
    assert!(panel.saved.is_none());
    assert!(panel.preview().is_none());
    assert!(!project.file().exists());
}
