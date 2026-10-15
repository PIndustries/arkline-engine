use super::*;
use std::time::{Duration, Instant};

static PROJECT_ID: AtomicU64 = AtomicU64::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "arkline-surface-test-{}-{}",
            std::process::id(),
            PROJECT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self) -> PathBuf {
        self.0.join("Assets").join(FILE_NAME)
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn settle(panel: &mut SurfaceMaterials) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while panel.pending.is_some() && Instant::now() < deadline {
        panel.poll_io();
        thread::sleep(Duration::from_millis(1));
    }
    assert!(panel.pending.is_none(), "surface I/O did not settle");
}
fn edited(value: f32) -> MaterialGraphDocument {
    let mut document = MaterialGraphDocument::pioneer_default();
    document
        .parameters
        .iter_mut()
        .find(|p| p.id == 7)
        .unwrap()
        .value = MaterialParameterValue::Scalar(value);
    document
}
#[test]
fn persisted_graph_roundtrips_exact_binding_parameters_seed_and_layout() {
    let project = Project::new();
    let mut document = edited(0.67);
    document.layout.insert(document.nodes[0].id, [90., -20.]);
    save(&project.0, &document).unwrap();
    assert_eq!(load(&project.0).unwrap(), document);
    let mut panel = SurfaceMaterials::open(&project.0, 41);
    settle(&mut panel);
    assert_eq!(panel.document(), &document);
    assert!(
        panel.revision().is_none(),
        "reopen requires an explicit Apply"
    );
    panel.enabled = true;
    assert_eq!(panel.revision().unwrap().generation(), 41);
    assert!(!panel.is_dirty());
    let temp_files = fs::read_dir(project.0.join("Assets")).unwrap().count();
    assert_eq!(temp_files, 1, "atomic writer left a temporary file");
}
#[test]
fn invalid_draft_keeps_last_good_revision_and_remains_undoable() {
    let project = Project::new();
    let mut panel = SurfaceMaterials::open(&project.0, 5);
    settle(&mut panel);
    panel.enabled = true;
    let previous = panel.revision().unwrap();
    let original = panel.document().clone();
    panel.add_node(MaterialNodeOp::Noise3D {
        position: 0,
        seed: 0,
    });
    assert!(!panel.diagnostics().is_empty());
    assert!(Arc::ptr_eq(&previous, &panel.revision().unwrap()));
    assert!(!panel.can_save());
    panel.move_history(false);
    assert_eq!(panel.document(), &original);
    assert!(panel.diagnostics().is_empty());
    assert!(panel.can_save());
    panel.move_history(true);
    assert!(!panel.diagnostics().is_empty());
}
#[test]
fn canvas_edits_are_saved_and_undoable_without_gpu_revisions() {
    let project = Project::new();
    let mut panel = SurfaceMaterials::open(&project.0, 8);
    settle(&mut panel);
    panel.enabled = true;
    let previous = panel.revision().unwrap();
    let mut draft = panel.document().clone();
    draft.layout.insert(draft.nodes[0].id, [400., 550.]);
    panel.apply(draft.clone());
    assert!(Arc::ptr_eq(&previous, &panel.revision().unwrap()));
    panel.save();
    settle(&mut panel);
    assert_eq!(load(&project.0).unwrap(), draft);
    panel.move_history(false);
    assert!(Arc::ptr_eq(&previous, &panel.revision().unwrap()));
    assert!(panel.is_dirty());
}
#[test]
fn idle_inspector_preserves_valid_large_values_and_hdr_emission() {
    let project = Project::new();
    let mut document = MaterialGraphDocument::pioneer_default();
    for parameter in &mut document.parameters {
        match parameter.id {
            1 => parameter.value = MaterialParameterValue::Vector3([50_000.; 3]),
            7 => parameter.value = MaterialParameterValue::Scalar(50_000.),
            9 => parameter.value = MaterialParameterValue::LinearColor([1.5, 2., 3.]),
            _ => {}
        }
    }
    let saved = document.clone();
    let mut panel = SurfaceMaterials::with_worker(&project.0, 7, move |_| Ok(Some(saved.clone())));
    settle(&mut panel);
    panel.enabled = true;
    let original_revision = panel.revision().unwrap();
    let context = egui::Context::default();
    for parameter in [1, 7, 9] {
        panel.selected = document.nodes.iter().find_map(|node| match node.op {
            MaterialNodeOp::Parameter { parameter: id } if id == parameter => Some(node.id),
            _ => None,
        });
        let _ = context.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| panel.show_node_inspector(ui));
        });
        assert_eq!(panel.document(), &document);
        assert!(!panel.is_dirty());
        assert!(Arc::ptr_eq(&original_revision, &panel.revision().unwrap()));
    }
}
#[test]
fn typed_connections_reject_mismatch_and_cycles_without_mutation() {
    let mut document = MaterialGraphDocument::pioneer_default();
    let position = document
        .nodes
        .iter()
        .find(|n| matches!(n.op, MaterialNodeOp::BodyPosition))
        .unwrap()
        .id;
    let transform = document
        .nodes
        .iter()
        .find(|n| matches!(n.op, MaterialNodeOp::TransformPosition { .. }))
        .unwrap()
        .id;
    let noise = document
        .nodes
        .iter()
        .find(|n| matches!(n.op, MaterialNodeOp::Noise3D { .. }))
        .unwrap()
        .id;
    let original = document.clone();
    assert!(connect_nodes(&mut document, position, noise, 1)
        .unwrap_err()
        .contains("Port types"));
    assert_eq!(document, original);
    assert!(connect_nodes(&mut document, transform, transform, 0)
        .unwrap_err()
        .contains("cycle"));
    assert_eq!(document, original);
    connect_nodes(&mut document, position, noise, 0).unwrap();
    document.validate().unwrap();
}
#[test]
fn canvas_port_gestures_connect_typed_nodes_and_publish_valid_revision() {
    let project = Project::new();
    let mut panel = SurfaceMaterials::open(&project.0, 3);
    settle(&mut panel);
    panel.enabled = true;
    let mut draft = panel.document().clone();
    let source = draft
        .nodes
        .iter()
        .find(|n| matches!(n.op, MaterialNodeOp::BodyPosition))
        .unwrap()
        .id;
    let target = draft
        .nodes
        .iter()
        .find(|n| matches!(n.op, MaterialNodeOp::Noise3D { .. }))
        .unwrap()
        .id;
    for node in &draft.nodes {
        draft.layout.insert(node.id, [2000., 2000.]);
    }
    draft.layout.insert(source, [30., 60.]);
    draft.layout.insert(target, [300., 60.]);
    draft
        .nodes
        .iter_mut()
        .find(|n| n.id == target)
        .unwrap()
        .op
        .set_input(0, 0)
        .unwrap();
    panel.apply(draft);
    assert!(!panel.diagnostics.is_empty());
    panel.pan = egui::Vec2::ZERO;
    panel.zoom = 1.;
    panel.frame_requested = false;
    let context = egui::Context::default();
    let render = |panel: &mut SurfaceMaterials, events| {
        context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900., 700.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| panel.show_graph(ui));
            },
        )
    };
    let output = render(&mut panel, Vec::new());
    let origin = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) if shape.fill == egui::Color32::from_rgb(19, 23, 30) => {
                Some(shape.rect.min)
            }
            _ => None,
        })
        .expect("actual canvas background");
    for position in [
        origin + egui::vec2(224., 110.),
        origin + egui::vec2(300., 110.),
    ] {
        for pressed in [true, false] {
            let _ = render(
                &mut panel,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    let node = panel
        .document()
        .nodes
        .iter()
        .find(|n| n.id == target)
        .unwrap();
    assert_eq!(node.op.inputs()[0].2, source);
    assert!(panel.diagnostics.is_empty());
    assert!(panel.revision().is_some());
}
#[test]
fn background_pan_keeps_drag_ownership_when_pointer_crosses_a_node() {
    let project = Project::new();
    let mut panel = SurfaceMaterials::open(&project.0, 3);
    settle(&mut panel);
    let mut draft = panel.document().clone();
    for node in &draft.nodes {
        draft.layout.insert(node.id, [2000., 2000.]);
    }
    draft.layout.insert(draft.nodes[0].id, [260., 260.]);
    panel.apply(draft);
    panel.pan = egui::Vec2::ZERO;
    panel.zoom = 1.;
    panel.frame_requested = false;
    let context = egui::Context::default();
    let render = |panel: &mut SurfaceMaterials, events| {
        context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900., 700.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| panel.show_graph(ui));
            },
        )
    };
    let output = render(&mut panel, Vec::new());
    let origin = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) if shape.fill == egui::Color32::from_rgb(19, 23, 30) => {
                Some(shape.rect.min)
            }
            _ => None,
        })
        .unwrap();
    let start = origin + egui::vec2(200., 300.);
    let _ = render(
        &mut panel,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    for x in [220., 300.] {
        let _ = render(
            &mut panel,
            vec![egui::Event::PointerMoved(origin + egui::vec2(x, 300.))],
        );
    }
    assert!(
        (panel.pan.x - 100.).abs() < 0.01,
        "pan stopped at a node: {:?}",
        panel.pan
    );
}
#[test]
fn late_save_keeps_newer_edits_and_ui_does_not_wait_for_storage() {
    let project = Project::new();
    let root = project.0.clone();
    let (started_tx, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut panel = SurfaceMaterials::with_worker(&project.0, 1, move |action| match action {
        IoAction::Open => Ok(None),
        IoAction::Save(document) => {
            started_tx.send(document.clone()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            save(&root, &document).map(|()| Some(document))
        }
        IoAction::Reload => unreachable!(),
    });
    settle(&mut panel);
    panel.apply(edited(0.2));
    panel.save();
    let submitted = started.recv_timeout(Duration::from_secs(5)).unwrap();
    panel.apply(edited(0.9));
    let context = egui::Context::default();
    let _ = context.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| panel.toolbar(ui));
    });
    assert!(panel.pending.is_some());
    release.send(()).unwrap();
    settle(&mut panel);
    assert_eq!(panel.saved.as_ref(), Some(&submitted));
    assert_eq!(load(&project.0).unwrap(), submitted);
    assert_eq!(panel.document(), &edited(0.9));
    assert!(panel.is_dirty());
}
#[test]
fn reload_keeps_early_edits_history_and_disabled_preview() {
    let project = Project::new();
    let (started_tx, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut panel = SurfaceMaterials::with_worker(&project.0, 11, move |action| match action {
        IoAction::Open => Ok(Some(edited(0.3))),
        IoAction::Reload => {
            started_tx.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(Some(edited(0.8)))
        }
        IoAction::Save(_) => unreachable!(),
    });
    settle(&mut panel);
    panel.reload();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    panel.apply(edited(0.6));
    panel.move_history(false);
    panel.enabled = false;
    release.send(()).unwrap();
    settle(&mut panel);
    assert_eq!(panel.document(), &edited(0.3));
    assert_eq!(panel.saved.as_ref(), Some(&edited(0.8)));
    assert!(panel.revision().is_none());
    panel.move_history(true);
    assert_eq!(panel.document(), &edited(0.6));
}
#[test]
fn late_reload_preserves_an_uncommitted_node_drag() {
    let project = Project::new();
    let (release, gate) = mpsc::channel();
    let mut panel = SurfaceMaterials::with_worker(&project.0, 11, move |action| match action {
        IoAction::Open => Ok(Some(edited(0.3))),
        IoAction::Reload => {
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(Some(edited(0.8)))
        }
        IoAction::Save(_) => unreachable!(),
    });
    settle(&mut panel);
    panel.reload();
    let mut drag = panel.document().clone();
    drag.layout.insert(drag.nodes[0].id, [500., 700.]);
    panel.drag_document = Some(drag.clone());
    release.send(()).unwrap();
    settle(&mut panel);
    assert_eq!(panel.drag_document.as_ref(), Some(&drag));
    assert_eq!(panel.document(), &edited(0.3));
    assert_eq!(panel.saved.as_ref(), Some(&edited(0.8)));
}
#[test]
fn mismatched_save_receipt_locks_writes_without_replacing_saved_baseline() {
    let project = Project::new();
    let mut panel = SurfaceMaterials::with_worker(&project.0, 1, |action| match action {
        IoAction::Open => Ok(Some(edited(0.2))),
        IoAction::Save(_) => Ok(Some(edited(0.5))),
        IoAction::Reload => unreachable!(),
    });
    settle(&mut panel);
    panel.apply(edited(0.8));
    panel.save();
    settle(&mut panel);
    assert_eq!(panel.document(), &edited(0.8));
    assert_eq!(panel.saved.as_ref(), Some(&edited(0.2)));
    assert!(panel.io_requests.is_none());
    assert!(!panel.can_save());
    assert!(panel.message.contains("receipt differed"));
}
#[test]
fn reopening_waits_for_an_already_submitted_save_from_the_closed_panel() {
    let project = Project::new();
    save(&project.0, &edited(0.1)).unwrap();
    let root = project.0.clone();
    let (started_tx, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut old = SurfaceMaterials::with_worker(&project.0, 1, move |action| match action {
        IoAction::Open => load_optional(&root),
        IoAction::Save(document) => {
            started_tx.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            save(&root, &document).map(|()| Some(document))
        }
        IoAction::Reload => unreachable!(),
    });
    settle(&mut old);
    old.apply(edited(0.2));
    old.save();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(old);
    let mut reopened = SurfaceMaterials::open(&project.0, 2);
    reopened.poll_io();
    assert!(
        reopened.pending.is_some(),
        "reopen bypassed the outstanding save"
    );
    assert_eq!(load(&project.0).unwrap(), edited(0.1));
    release.send(()).unwrap();
    settle(&mut reopened);
    assert_eq!(reopened.document(), &edited(0.2));
    assert_eq!(reopened.saved.as_ref(), Some(&edited(0.2)));
    assert!(!reopened.is_dirty());
    reopened.save();
    settle(&mut reopened);
    assert_eq!(load(&project.0).unwrap(), edited(0.2));
}
#[test]
fn accepted_save_precedes_reopen_even_if_the_old_worker_has_not_started() {
    let project = Project::new();
    save(&project.0, &edited(0.1)).unwrap();
    let order = ProjectOrder::shared(&project.0);
    let (sender, requests) = mpsc::sync_channel(1);
    order
        .submit(
            &sender,
            IoRequest {
                id: 0,
                project_ticket: 0,
                action: IoAction::Save(edited(0.2)),
            },
        )
        .unwrap();
    let mut reopened = SurfaceMaterials::open(&project.0, 2);
    reopened.poll_io();
    assert!(reopened.pending.is_some());
    let root = project.0.clone();
    let worker = thread::spawn(move || {
        let request = requests.recv().unwrap();
        let _permit = order.enter(request.project_ticket);
        if let IoAction::Save(document) = request.action {
            save(&root, &document).unwrap();
        }
    });
    settle(&mut reopened);
    worker.join().unwrap();
    assert_eq!(reopened.document(), &edited(0.2));
    assert!(!reopened.is_dirty());
}
#[test]
fn failed_old_worker_releases_project_order_for_reopen() {
    let project = Project::new();
    save(&project.0, &edited(0.1)).unwrap();
    let root = project.0.clone();
    let (started_tx, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut old = SurfaceMaterials::with_worker(&project.0, 1, move |action| match action {
        IoAction::Open => load_optional(&root),
        IoAction::Save(_) => {
            started_tx.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            panic!("injected worker failure before save receipt");
        }
        IoAction::Reload => unreachable!(),
    });
    settle(&mut old);
    old.apply(edited(0.2));
    old.save();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut reopened = SurfaceMaterials::open(&project.0, 2);
    release.send(()).unwrap();
    settle(&mut old);
    assert!(old.io_requests.is_none());
    assert!(!old.can_save());
    assert!(old.message.contains("may have completed"));
    drop(old);
    settle(&mut reopened);
    assert_eq!(reopened.document(), &edited(0.1));
}
#[test]
fn corrupt_file_and_failed_reload_preserve_draft_until_explicit_replacement() {
    let project = Project::new();
    fs::create_dir(project.0.join("Assets")).unwrap();
    let broken = b"{not a surface graph";
    fs::write(project.file(), broken).unwrap();
    let mut panel = SurfaceMaterials::open(&project.0, 1);
    settle(&mut panel);
    assert!(panel.load_blocked);
    panel.enabled = true;
    panel.apply(edited(0.9));
    let valid = panel.revision().unwrap();
    panel.save();
    assert_eq!(fs::read(project.file()).unwrap(), broken);
    panel.reload();
    settle(&mut panel);
    assert!(Arc::ptr_eq(&valid, &panel.revision().unwrap()));
    assert!(!panel.can_save());
    panel.allow_replacement();
    assert_eq!(fs::read(project.file()).unwrap(), broken);
    panel.save();
    settle(&mut panel);
    assert_eq!(load(&project.0).unwrap(), edited(0.9));
}
#[test]
fn node_id_exhaustion_does_not_discard_redo_or_modify_the_draft() {
    let project = Project::new();
    let mut panel = SurfaceMaterials::open(&project.0, 1);
    settle(&mut panel);
    let mut draft = panel.document().clone();
    draft.nodes.push(MaterialNode {
        id: u32::MAX,
        op: MaterialNodeOp::BodyPosition,
    });
    panel.apply(draft.clone());
    panel.apply(edited(0.8));
    panel.move_history(false);
    panel.add_node(MaterialNodeOp::BodyPosition);
    assert_eq!(panel.document(), &draft);
    assert!(!panel.history.redo.is_empty());
}
#[test]
fn oversized_or_invalid_graph_never_replaces_a_valid_file() {
    let project = Project::new();
    let valid = edited(0.4);
    save(&project.0, &valid).unwrap();
    let bytes = fs::read(project.file()).unwrap();
    let mut invalid = valid.clone();
    invalid.schema_version = u32::MAX;
    assert!(save(&project.0, &invalid).is_err());
    assert_eq!(fs::read(project.file()).unwrap(), bytes);
    fs::write(project.file(), vec![b' '; MAX_BYTES as usize + 1]).unwrap();
    assert!(load(&project.0).unwrap_err().contains("128 KiB"));
}
#[cfg(unix)]
#[test]
fn symbolic_link_files_and_directories_are_not_followed_even_when_missing() {
    use std::os::unix::fs::symlink;
    let project = Project::new();
    let outside = Project::new();
    symlink(&outside.0, project.0.join("Assets")).unwrap();
    assert!(load_optional(&project.0).unwrap_err().contains("symbolic"));
    assert!(save(&project.0, &edited(0.2)).is_err());
    fs::remove_file(project.0.join("Assets")).unwrap();
    fs::create_dir(project.0.join("Assets")).unwrap();
    let target = outside.0.join("missing-surface");
    symlink(&target, project.file()).unwrap();
    assert!(load_optional(&project.0).is_err());
    assert!(save(&project.0, &edited(0.2)).is_err());
    assert!(!target.exists());
}
