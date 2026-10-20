use super::*;
use std::time::{Duration, Instant};
fn imports() -> Imports {
    let mut imports = Imports::with_worker(|work| match work {
        Work::Load => Ok(Answer::Loaded(Loaded {
            index: AssetIndex::default(),
            verified: None,
            unavailable: None,
        })),
        _ => Err("Unexpected bridge test operation".into()),
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while imports.pending.is_some() {
        imports.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    imports
}
fn wedge() -> Verified {
    Verified::decode(include_bytes!("../tests/fixtures/import/wedge.glb").to_vec()).unwrap()
}
fn spire() -> Verified {
    Verified::decode(include_bytes!("../tests/fixtures/import/spire.glb").to_vec()).unwrap()
}

#[test]
fn browser_snapshots_borrow_real_accepted_and_staged_data_without_authoring_changes() {
    let mut imports = imports();
    let accepted = wedge();
    let staged = spire();
    imports.index = imports
        .index
        .accept(&accepted, "Original source".into(), "License".into())
        .unwrap();
    imports.accepted = Some(accepted.clone());
    imports.staged = Some(staged.clone());
    imports.source = "Unsaved candidate source".into();
    imports.license = "Unsaved candidate license".into();
    let before = (
        imports.index.clone(),
        imports.saved.clone(),
        imports.edit,
        imports.candidate,
        imports.source.clone(),
        imports.license.clone(),
    );
    let snapshots = imports.browser_snapshots();
    assert_eq!(snapshots.len(), 2);
    assert_eq!(
        snapshots[0].verified.as_ref().unwrap().sha256,
        accepted.sha256
    );
    assert_eq!(
        snapshots[1].verified.as_ref().unwrap().sha256,
        staged.sha256
    );
    assert!(Arc::ptr_eq(
        &snapshots[0].verified.as_ref().unwrap().geometry,
        &accepted.geometry
    ));
    assert_eq!(
        (
            imports.index.clone(),
            imports.saved.clone(),
            imports.edit,
            imports.candidate,
            imports.source.clone(),
            imports.license.clone()
        ),
        before
    );
}

#[test]
fn explicit_stage_bridge_preserves_different_staged_work_and_never_accepts_or_saves() {
    let mut imports = imports();
    let existing = wedge();
    let incoming = spire();
    imports.staged = Some(existing.clone());
    imports.candidate_label = "Existing candidate".into();
    let before = (
        imports.index.clone(),
        imports.saved.clone(),
        imports.edit,
        imports.candidate,
        imports.candidate_label.clone(),
    );
    assert!(imports
        .stage_browser_preview("Incoming".into(), incoming)
        .unwrap_err()
        .contains("different staged import"));
    assert_eq!(imports.staged.as_ref().unwrap().sha256, existing.sha256);
    assert_eq!(
        (
            imports.index.clone(),
            imports.saved.clone(),
            imports.edit,
            imports.candidate,
            imports.candidate_label.clone()
        ),
        before
    );
    imports
        .stage_browser_preview("Same verified asset".into(), existing.clone())
        .unwrap();
    assert_eq!(imports.staged.as_ref().unwrap().sha256, existing.sha256);
    assert!(imports.accepted.is_none());
    assert_eq!(imports.index, before.0);
    assert_eq!(imports.saved, before.1);
}
