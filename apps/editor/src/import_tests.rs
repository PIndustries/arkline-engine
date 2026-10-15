use super::*;
use std::path::PathBuf;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
static ID: AtomicU64 = AtomicU64::new(0);
const WEDGE: &[u8] = include_bytes!("../tests/fixtures/import/wedge.glb");
const SPIRE: &[u8] = include_bytes!("../tests/fixtures/import/spire.glb");
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "arkline-import-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn put(&self, name: &str, bytes: &[u8]) {
        fs::write(self.0.join(name), bytes).unwrap();
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn settle(p: &mut Imports) {
    let end = Instant::now() + Duration::from_secs(5);
    while p.pending.is_some() {
        p.poll();
        assert!(Instant::now() < end, "file worker stalled");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn opened(p: &Project) -> Imports {
    let mut s = Imports::open(&p.0);
    settle(&mut s);
    s
}
fn stage(p: &mut Imports, name: &str) {
    p.path = name.into();
    p.stage();
    settle(p);
    assert!(p.staged.is_some(), "{}", p.message);
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.001, "{a} != {b}");
}
#[test]
fn transformed_asymmetric_geometry_is_measured_without_fitting() {
    for (bytes, min, max) in [
        (WEDGE, [10., 2., -3.], [14., 4., -2.]),
        (SPIRE, [-7., 1., 3.], [-4., 7., 4.]),
    ] {
        let v = Verified::decode(bytes.to_vec()).unwrap();
        for i in 0..3 {
            close(v.geometry.report.min_m[i], min[i]);
            close(v.geometry.report.max_m[i], max[i]);
        }
        assert_eq!(v.geometry.report.triangles, 4);
        assert_eq!(v.geometry.report.primitives, 1);
    }
    let v = Verified::decode(include_bytes!("../../../assets/ships/pioneer/pioneer.glb").to_vec())
        .unwrap();
    for (i, e) in [72., 18., 38.].iter().enumerate() {
        close(v.geometry.report.max_m[i] - v.geometry.report.min_m[i], *e);
    }
    assert!(v.geometry.report.triangles > 30_000);
}
#[test]
fn import_accept_save_reopen_and_same_id_replacement() {
    let project = Project::new();
    project.put("wedge.glb", WEDGE);
    project.put("spire.glb", SPIRE);
    let mut p = opened(&project);
    stage(&mut p, "wedge.glb");
    assert!(p.accepted.is_none());
    assert!(p.index.asset.is_none());
    p.source = "Original fixture".into();
    p.license = "Repository license".into();
    p.accept();
    let first = p.index.clone();
    assert_ne!(p.saved.as_ref(), Some(&p.index));
    p.save();
    settle(&mut p);
    assert_eq!(p.saved.as_ref(), Some(&first));
    let mut p = opened(&project);
    assert_eq!(p.index, first);
    assert!(p.accepted.is_some());
    stage(&mut p, "spire.glb");
    assert_eq!(
        p.accepted.as_ref().unwrap().sha256,
        first.asset.as_ref().unwrap().sha256
    );
    p.accept();
    assert_eq!(
        p.index.asset.as_ref().unwrap().id,
        first.asset.as_ref().unwrap().id
    );
    assert!(p.index.revision > first.revision);
    assert_ne!(
        p.index.asset.as_ref().unwrap().sha256,
        first.asset.as_ref().unwrap().sha256
    );
    p.save();
    settle(&mut p);
    let q = opened(&project);
    assert_eq!(q.index, p.index);
    assert_eq!(q.accepted.unwrap().sha256, p.accepted.unwrap().sha256);
}
#[test]
fn failed_replacement_and_cancel_preserve_accepted_saved_content() {
    let root = Project::new();
    root.put("wedge.glb", WEDGE);
    root.put("bad.glb", b"bad");
    let mut p = opened(&root);
    stage(&mut p, "wedge.glb");
    p.accept();
    p.save();
    settle(&mut p);
    let first = p.index.clone();
    p.path = "bad.glb".into();
    p.stage();
    settle(&mut p);
    assert!(p.staged.is_none());
    assert_eq!(p.index, first);
    assert_eq!(p.saved.as_ref(), Some(&first));
    assert!(p.accepted.is_some());
    p.path = "wedge.glb".into();
    p.stage();
    p.stage();
    assert!(p.pending.is_some());
    p.cancel();
    std::thread::sleep(Duration::from_millis(20));
    p.poll();
    assert!(p.staged.is_none());
    assert_eq!(p.index, first);
}
#[test]
fn missing_or_hash_mismatched_reopen_is_unavailable_without_fallback() {
    let root = Project::new();
    let v = Verified::decode(WEDGE.to_vec()).unwrap();
    let index = AssetIndex::default()
        .accept(&v, String::new(), String::new())
        .unwrap();
    doc::save(&root.0, &index, Some(&v)).unwrap();
    let path = root
        .0
        .join("Assets/Imported")
        .join(format!("{}.glb", v.sha256));
    fs::write(&path, SPIRE).unwrap();
    let p = opened(&root);
    assert!(p.accepted.is_none());
    assert_eq!(p.index, index);
    assert!(p.message.contains("unavailable"));
    fs::remove_file(path).unwrap();
    let p = opened(&root);
    assert!(p.accepted.is_none());
    assert_eq!(p.index, index);
}
#[test]
fn corrupt_index_locks_save_until_explicit_recovery() {
    let root = Project::new();
    fs::create_dir(root.0.join("Assets")).unwrap();
    fs::write(root.0.join("Assets/ImportedAssets.json"), b"corrupt").unwrap();
    root.put("wedge.glb", WEDGE);
    let mut p = opened(&root);
    assert!(p.blocked);
    stage(&mut p, "wedge.glb");
    p.accept();
    p.save();
    assert!(p.pending.is_none());
    assert_eq!(
        fs::read(root.0.join("Assets/ImportedAssets.json")).unwrap(),
        b"corrupt"
    );
    p.blocked = false;
    p.save();
    settle(&mut p);
    assert!(opened(&root).accepted.is_some());
}
#[test]
fn stale_cancelled_result_cannot_replace_newer_state() {
    let (release, gate) = mpsc::channel();
    let mut p = Imports::with_worker(move |w| match w {
        Work::Load => Ok(Answer::Loaded(Loaded {
            index: AssetIndex::default(),
            verified: None,
            unavailable: None,
        })),
        Work::Import(_) => {
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(Answer::Imported(Verified::decode(WEDGE.to_vec()).unwrap()))
        }
        _ => unreachable!(),
    });
    settle(&mut p);
    p.path = "held.glb".into();
    p.stage();
    p.cancel();
    p.staged = Some(Verified::decode(SPIRE.to_vec()).unwrap());
    p.accept();
    let newer = p.index.clone();
    release.send(()).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    p.poll();
    assert_eq!(p.index, newer);
    assert!(p.staged.is_none());
}
#[test]
fn save_completion_keeps_newer_accepted_revision_unsaved() {
    let root = Project::new();
    let v = Verified::decode(WEDGE.to_vec()).unwrap();
    let index = AssetIndex::default()
        .accept(&v, String::new(), String::new())
        .unwrap();
    let (release, gate) = mpsc::channel();
    let path = root.0.clone();
    let mut p = Imports::with_worker(move |w| {
        if matches!(w, Work::Save(..)) {
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        run(&path, w)
    });
    settle(&mut p);
    p.index = index.clone();
    p.accepted = Some(v);
    p.save();
    p.staged = Some(Verified::decode(SPIRE.to_vec()).unwrap());
    p.accept();
    let newer = p.index.clone();
    release.send(()).unwrap();
    settle(&mut p);
    assert_eq!(p.index, newer);
    assert_eq!(p.saved, Some(index.clone()));
    assert_eq!(doc::load(&root.0).unwrap().index, index);
}
#[test]
fn failed_content_write_and_interrupted_publication_preserve_previous_index() {
    let root = Project::new();
    let a = Verified::decode(WEDGE.to_vec()).unwrap();
    let first = AssetIndex::default()
        .accept(&a, String::new(), String::new())
        .unwrap();
    doc::save(&root.0, &first, Some(&a)).unwrap();
    let b = Verified::decode(SPIRE.to_vec()).unwrap();
    let second = first.accept(&b, String::new(), String::new()).unwrap();
    let path = root
        .0
        .join("Assets/Imported")
        .join(format!("{}.glb", b.sha256));
    fs::create_dir(&path).unwrap();
    assert!(doc::save(&root.0, &second, Some(&b)).is_err());
    assert_eq!(doc::load(&root.0).unwrap().index, first);
    // A crash after durable content but before index publication leaves an unused blob.
    fs::remove_dir(&path).unwrap();
    fs::write(&path, SPIRE).unwrap();
    fs::write(
        root.0.join("Assets/.import-interrupted.tmp"),
        b"partial index",
    )
    .unwrap();
    let loaded = doc::load(&root.0).unwrap();
    assert_eq!(loaded.index, first);
    assert_eq!(loaded.verified.unwrap().sha256, a.sha256);
}
#[test]
fn worker_and_project_switch_keep_storage_isolated() {
    let a = Project::new();
    let b = Project::new();
    a.put("one.glb", WEDGE);
    b.put("two.glb", SPIRE);
    let mut p = opened(&a);
    stage(&mut p, "one.glb");
    p.accept();
    p.save();
    drop(p);
    let mut q = opened(&b);
    stage(&mut q, "two.glb");
    q.accept();
    q.save();
    settle(&mut q);
    let end = Instant::now() + Duration::from_secs(3);
    while !a.0.join("Assets/ImportedAssets.json").exists() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_ne!(
        doc::load(&a.0).unwrap().index.asset.unwrap().sha256,
        q.index.asset.unwrap().sha256
    );
}
#[test]
fn malformed_geometry_and_index_budgets_fail_closed() {
    assert!(Verified::decode(vec![0; 16 * 1024 * 1024 + 1]).is_err());
    assert!(Verified::decode(b"not glb".to_vec()).is_err());
    let i = AssetIndex {
        schema_version: 2,
        ..Default::default()
    };
    assert!(i.validate().is_err());
    let v = Verified::decode(WEDGE.to_vec()).unwrap();
    let mut i = AssetIndex::default()
        .accept(&v, String::new(), String::new())
        .unwrap();
    i.asset.as_mut().unwrap().sha256 = "../../escape".into();
    assert!(i.validate().is_err());
}
#[cfg(unix)]
#[test]
fn traversal_symlink_and_nonordinary_content_paths_are_rejected() {
    use std::os::unix::fs::symlink;
    let root = Project::new();
    let other = Project::new();
    other.put("outside.glb", WEDGE);
    symlink(other.0.join("outside.glb"), root.0.join("link.glb")).unwrap();
    assert!(doc::import(&root.0, "../outside.glb").is_err());
    assert!(doc::import(&root.0, "link.glb").is_err());
    symlink(&other.0, root.0.join("Assets")).unwrap();
    let v = Verified::decode(WEDGE.to_vec()).unwrap();
    let i = AssetIndex::default()
        .accept(&v, String::new(), String::new())
        .unwrap();
    assert!(doc::save(&root.0, &i, Some(&v)).is_err());
    assert!(doc::load(&root.0).is_err());
}

fn frame(p: &mut Imports, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1500., 960.),
            )),
            events,
            ..Default::default()
        },
        |ctx| p.show(ctx, None),
    )
}
fn point(o: &egui::FullOutput, label: &str) -> egui::Pos2 {
    o.shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == label => {
                Some(t.pos + t.galley.rect.center().to_vec2())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing visible {label}"))
}
fn click(p: &mut Imports, ctx: &egui::Context, pos: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            p,
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
}
#[test]
fn visible_inspect_accept_save_reload_workflow() {
    let root = Project::new();
    root.put("wedge.glb", WEDGE);
    let mut p = opened(&root);
    p.path = "wedge.glb".into();
    let ctx = egui::Context::default();
    frame(&mut p, &ctx, vec![]);
    let o = frame(&mut p, &ctx, vec![]);
    click(&mut p, &ctx, point(&o, "Inspect GLB"));
    settle(&mut p);
    assert!(p.staged.is_some());
    let o = frame(&mut p, &ctx, vec![]);
    assert!(o.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.text().contains("4.000 × 2.000 × 1.000"))));
    click(&mut p, &ctx, point(&o, "Accept preview"));
    assert!(p.staged.is_none());
    assert!(p.index.asset.is_some());
    let o = frame(&mut p, &ctx, vec![]);
    click(&mut p, &ctx, point(&o, "Save assets"));
    settle(&mut p);
    assert_eq!(p.saved.as_ref(), Some(&p.index));
    let o = frame(&mut p, &ctx, vec![]);
    click(&mut p, &ctx, point(&o, "Reload assets"));
    settle(&mut p);
    assert!(p.accepted.is_some());
}
#[test]
fn assets_workspace_pointer_and_keys_leave_real_authority_unchanged() {
    use arkline_client::EmbeddedStatus;
    use eframe::App;
    let root = Project::new();
    root.put("wedge.glb", WEDGE);
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut editor = crate::Editor::new(&cc, root.0.clone(), "ImportIsolation".into());
    let end = Instant::now() + Duration::from_secs(5);
    while !editor.client.ready() {
        assert!(Instant::now() < end);
        editor.client.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    settle(&mut editor.imports);
    stage(&mut editor.imports, "wedge.glb");
    editor.imports.accept();
    editor.workspace = 5;
    let before = serde_json::to_value(editor.client.snapshot().unwrap()).unwrap();
    assert_eq!(editor.client.status(), &EmbeddedStatus::Paused);
    for step in 0..20 {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1500., 960.),
                )),
                events: vec![
                    egui::Event::PointerMoved(egui::pos2(700. + step as f32, 400.)),
                    egui::Event::PointerButton {
                        pos: egui::pos2(700., 400.),
                        button: egui::PointerButton::Secondary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                    egui::Event::Key {
                        key: egui::Key::W,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            },
            |ctx| editor.update(ctx, &mut eframe::Frame::_new_kittest()),
        );
    }
    assert_eq!(editor.workspace, 5);
    assert_eq!(editor.client.status(), &EmbeddedStatus::Paused);
    let after = serde_json::to_value(editor.client.snapshot().unwrap()).unwrap();
    assert_eq!(before["server_time_ms"], after["server_time_ms"]);
    assert_eq!(before["pilots"], after["pilots"]);
    editor.client.on_exit();
}

#[test]
fn delayed_reload_cannot_replace_newer_accepted_geometry() {
    let (release, gate) = mpsc::channel();
    let mut p = Imports::with_worker(move |w| match w {
        Work::Load => {
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(Answer::Loaded(Loaded {
                index: AssetIndex::default(),
                verified: None,
                unavailable: None,
            }))
        }
        _ => unreachable!(),
    });
    p.staged = Some(Verified::decode(WEDGE.to_vec()).unwrap());
    p.accept();
    let expected = p.index.clone();
    release.send(()).unwrap();
    settle(&mut p);
    assert_eq!(p.index, expected);
    assert!(p.accepted.is_some());
    assert_eq!(p.saved, Some(AssetIndex::default()));
}
#[test]
fn lost_worker_result_disables_save_without_replay() {
    let mut p = Imports::with_worker(|_| panic!("simulated worker termination before result"));
    settle(&mut p);
    assert!(p.blocked);
    assert!(p.sender.is_none());
    assert!(p.message.contains("lost"));
    p.poll();
    assert!(p.index.asset.is_none());
}
fn edited_glb(edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let len = u32::from_le_bytes(WEDGE[12..16].try_into().unwrap()) as usize;
    let mut json: serde_json::Value = serde_json::from_slice(&WEDGE[20..20 + len]).unwrap();
    edit(&mut json);
    let mut json = serde_json::to_vec(&json).unwrap();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let rest = &WEDGE[20 + len..];
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x46546c67u32.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&((20 + json.len() + rest.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&0x4e4f534au32.to_le_bytes());
    bytes.extend_from_slice(&json);
    bytes.extend_from_slice(rest);
    bytes
}
#[test]
fn unsupported_features_and_hierarchy_budgets_are_rejected() {
    let huge_name =
        edited_glb(|j| j["materials"][0]["name"] = serde_json::Value::String("x".repeat(1025)));
    assert!(Verified::decode(huge_name).is_err());
    let unsupported = edited_glb(|j| j["extensionsUsed"] = serde_json::json!(["EXT_unsupported"]));
    assert!(Verified::decode(unsupported).is_err());
    let singular = edited_glb(|j| j["nodes"][0]["scale"] = serde_json::json!([0, 1, 1]));
    assert!(Verified::decode(singular).is_err());
    let deep = edited_glb(|j| {
        let mut nodes = vec![];
        for i in 0..34 {
            nodes.push(if i == 33 {
                serde_json::json!({"mesh":0})
            } else {
                serde_json::json!({"children":[i+1]})
            });
        }
        j["nodes"] = serde_json::Value::Array(nodes);
    });
    assert!(Verified::decode(deep).is_err());
}

fn wait_client(
    client: &mut arkline_client::EmbeddedClient,
    ok: impl Fn(&arkline_client::EmbeddedClient) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(5);
    while !ok(client) {
        assert!(
            Instant::now() < end,
            "authority did not reach expected state: {:?}",
            client.status()
        );
        client.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn import_view_input_does_not_fly_an_undocked_running_ship() {
    use arkline_client::EmbeddedStatus;
    use arkline_protocol::{Location, Request};
    use eframe::App;
    let root = Project::new();
    root.put("wedge.glb", WEDGE);
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut editor = crate::Editor::new(&cc, root.0.clone(), "ImportFlightIsolation".into());
    wait_client(&mut editor.client, |c| c.ready());
    settle(&mut editor.imports);
    stage(&mut editor.imports, "wedge.glb");
    editor.imports.accept();
    editor.client.play();
    wait_client(&mut editor.client, |c| {
        c.status() == &EmbeddedStatus::Running
    });
    editor.client.send_command(Request::Undock);
    wait_client(&mut editor.client, |c| {
        c.snapshot().is_some_and(|s| {
            s.pilots
                .iter()
                .any(|p| p.name == c.pilot() && p.location == Location::Station)
        })
    });
    let before = editor.client.snapshot().unwrap();
    let flight = before
        .pilots
        .iter()
        .find(|p| p.name == editor.client.pilot())
        .unwrap()
        .flight
        .clone()
        .unwrap();
    assert_eq!(flight.velocity, [0.; 3]);
    editor.workspace = 5;
    for i in 0..20 {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1500., 960.),
                )),
                events: vec![
                    egui::Event::PointerMoved(egui::pos2(800. + i as f32 * 2., 450.)),
                    egui::Event::PointerButton {
                        pos: egui::pos2(800., 450.),
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                    egui::Event::PointerButton {
                        pos: egui::pos2(800., 450.),
                        button: egui::PointerButton::Secondary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                    egui::Event::Key {
                        key: egui::Key::W,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            },
            |ctx| editor.update(ctx, &mut eframe::Frame::_new_kittest()),
        );
    }
    wait_client(&mut editor.client, |c| {
        c.snapshot()
            .is_some_and(|s| s.server_time_ms > before.server_time_ms + 200)
    });
    let after = editor.client.snapshot().unwrap();
    let actual = after
        .pilots
        .iter()
        .find(|p| p.name == editor.client.pilot())
        .unwrap()
        .flight
        .as_ref()
        .unwrap();
    assert_eq!(actual.position, flight.position);
    assert_eq!(actual.velocity, flight.velocity);
    assert_eq!(actual.orientation, flight.orientation);
    assert_eq!(actual.angular_velocity, flight.angular_velocity);
    editor.client.pause();
    wait_client(&mut editor.client, |c| c.paused());
    editor.client.on_exit();
}

#[test]
fn explicit_verified_reimport_repairs_a_damaged_same_hash_blob() {
    let root = Project::new();
    root.put("wedge.glb", WEDGE);
    let mut p = opened(&root);
    stage(&mut p, "wedge.glb");
    p.accept();
    p.save();
    settle(&mut p);
    let record = p.index.asset.clone().unwrap();
    fs::write(
        root.0
            .join("Assets/Imported")
            .join(format!("{}.glb", record.sha256)),
        b"damaged",
    )
    .unwrap();
    let mut q = opened(&root);
    assert!(q.accepted.is_none());
    stage(&mut q, "wedge.glb");
    q.accept();
    q.save();
    settle(&mut q);
    let reopened = opened(&root);
    assert_eq!(reopened.index.asset.as_ref().unwrap().id, record.id);
    assert_eq!(reopened.accepted.unwrap().sha256, record.sha256);
}

fn replace_candidate_text(p: &mut Imports, ctx: &egui::Context, label: &str, value: &str) {
    frame(p, ctx, vec![]);
    let o = frame(p, ctx, vec![]);
    let pos = point(&o, label) + egui::vec2(0., 24.);
    click(p, ctx, pos);
    frame(
        p,
        ctx,
        vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                },
            },
            egui::Event::Text(value.into()),
        ],
    );
}
#[test]
fn candidate_text_during_reload_preserves_new_source_and_license() {
    let v = Verified::decode(WEDGE.to_vec()).unwrap();
    let index = AssetIndex::default()
        .accept(&v, "saved source".into(), "saved license".into())
        .unwrap();
    let (release, gate) = mpsc::channel();
    let expected = index.clone();
    let mut p = Imports::with_worker(move |work| {
        assert!(matches!(work, Work::Load));
        gate.recv_timeout(Duration::from_secs(5)).unwrap();
        Ok(Answer::Loaded(Loaded {
            index: index.clone(),
            verified: Some(v.clone()),
            unavailable: None,
        }))
    });
    let ctx = egui::Context::default();
    replace_candidate_text(
        &mut p,
        &ctx,
        "Candidate source reference (blank = unknown)",
        "new source",
    );
    replace_candidate_text(
        &mut p,
        &ctx,
        "Candidate license reference (blank = unknown)",
        "new license",
    );
    assert_eq!(p.source, "new source");
    assert_eq!(p.license, "new license");
    release.send(()).unwrap();
    settle(&mut p);
    assert_eq!(p.source, "new source");
    assert_eq!(p.license, "new license");
    assert_eq!(p.index, expected);
    assert!(p.accepted.is_some());
}
#[test]
fn candidate_text_path_change_discards_pending_import_result() {
    let (release, gate) = mpsc::channel();
    let mut p = Imports::with_worker(move |work| match work {
        Work::Load => Ok(Answer::Loaded(Loaded {
            index: AssetIndex::default(),
            verified: None,
            unavailable: None,
        })),
        Work::Import(_) => {
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(Answer::Imported(Verified::decode(WEDGE.to_vec()).unwrap()))
        }
        _ => unreachable!(),
    });
    settle(&mut p);
    p.path = "held.glb".into();
    p.stage();
    let ctx = egui::Context::default();
    replace_candidate_text(&mut p, &ctx, "Project-relative .glb path", "changed.glb");
    assert_eq!(p.path, "changed.glb");
    release.send(()).unwrap();
    let end = Instant::now() + Duration::from_secs(1);
    while Instant::now() < end {
        p.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        p.staged.is_none(),
        "Old geometry must not be staged underneath a newly typed path"
    );
    assert!(p.index.asset.is_none());
}
