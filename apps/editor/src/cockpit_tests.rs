use super::*;
use document::{Binding, Control};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static ID: AtomicU64 = AtomicU64::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "arkline-cockpit-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn settle(panel: &mut CockpitPanel) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while panel.pending_file.is_some() {
        panel.poll_files();
        assert!(Instant::now() < deadline, "cockpit file worker stalled");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn assigned() -> Document {
    let mut doc = Document::default();
    doc.ships.insert(
        "ship-A".into(),
        Assignment {
            template: "airlock".into(),
            override_panel: None,
        },
    );
    doc
}
fn panel() -> CockpitPanel {
    let mut p = CockpitPanel::with_worker(|_, _| Ok(None));
    settle(&mut p);
    p.history = History::new(assigned());
    p.selected_vessel = "ship-A".into();
    p
}

#[test]
fn templates_are_inherited_until_explicit_full_ship_override() {
    let mut d = assigned();
    d.ships.insert("ship-B".into(), d.ships["ship-A"].clone());
    d.ships.get_mut("ship-B").unwrap().override_panel = Some(d.panel("ship-B").unwrap().clone());
    d.templates.get_mut("airlock").unwrap().title = "Shared edit".into();
    assert_eq!(d.panel("ship-A").unwrap().title, "Shared edit");
    assert_eq!(d.panel("ship-B").unwrap().title, "Pioneer airlock");
    d.editable_panel("ship-B", false).unwrap().title = "Only B".into();
    assert_eq!(d.panel("ship-A").unwrap().title, "Shared edit");
    d.validate().unwrap();
}
#[test]
fn invalid_documents_bindings_and_geometry_are_rejected() {
    let good = assigned();
    good.validate().unwrap();
    let mut bad = good.clone();
    bad.schema_version = 2;
    assert!(bad.validate().is_err());
    let mut bad = good.clone();
    bad.ships.get_mut("ship-A").unwrap().template = "missing".into();
    assert!(bad.validate().is_err());
    let mut bad = good.clone();
    bad.templates.get_mut("airlock").unwrap().controls[0].binding = Binding::Pressure {
        equipment: "pioneer.deck1.airlock.light".into(),
        max_kpa: 120.,
    };
    assert!(bad.validate().is_err());
    let mut bad = good.clone();
    bad.templates.get_mut("airlock").unwrap().controls[0].position[0] = f32::NAN;
    assert!(bad.validate().is_err());
    let mut bad = good.clone();
    bad.templates.get_mut("airlock").unwrap().controls[1].id = 1;
    assert!(bad.validate().is_err());
    let mut bad = good;
    bad.templates.get_mut("airlock").unwrap().controls[0].size[0] = 1.;
    assert!(bad.validate().is_err());
}
#[test]
fn save_reopen_is_local_content_and_does_not_install_runtime() {
    let project = Project::new();
    document::save(&project.0, &assigned()).unwrap();
    let mut p = CockpitPanel::open(&project.0);
    settle(&mut p);
    assert_eq!(p.history.current, assigned());
    assert!(p.applied.is_none());
    assert!(!p.history.can_undo());
    p.apply();
    assert_eq!(p.applied.as_ref().unwrap().document, assigned());
    let mut next = p.history.current.clone();
    next.templates.get_mut("airlock").unwrap().title = "Draft only".into();
    p.edit(next, false);
    assert_eq!(p.applied.as_ref().unwrap().document, assigned());
    p.queue(FileKind::Reload);
    settle(&mut p);
    p.move_history(false);
    assert_eq!(p.history.current.templates["airlock"].title, "Draft only");
}
#[test]
fn corrupt_file_is_not_overwritten_until_recovery_is_explicit() {
    let project = Project::new();
    fs::create_dir(project.0.join("Assets")).unwrap();
    let path = project.0.join(document::FILE);
    fs::write(&path, b"{broken").unwrap();
    let mut p = CockpitPanel::open(&project.0);
    settle(&mut p);
    assert!(p.blocked);
    p.queue(FileKind::Save);
    assert!(p.pending_file.is_none());
    assert_eq!(fs::read(&path).unwrap(), b"{broken");
    p.blocked = false;
    p.saved = None;
    assert_eq!(fs::read(&path).unwrap(), b"{broken");
    p.queue(FileKind::Save);
    settle(&mut p);
    assert_eq!(
        document::load(&project.0).unwrap(),
        Some(p.history.current.clone())
    );
}
#[test]
fn delayed_save_keeps_newer_draft_and_tracks_submitted_snapshot() {
    let (release, gate) = mpsc::channel();
    let mut p = CockpitPanel::with_worker(move |kind, doc| {
        if kind == FileKind::Save {
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        Ok(doc)
    });
    settle(&mut p);
    p.history = History::new(assigned());
    p.queue(FileKind::Save);
    let mut next = p.history.current.clone();
    next.templates.get_mut("airlock").unwrap().title = "New draft".into();
    p.edit(next.clone(), false);
    p.move_history(false);
    p.move_history(true);
    release.send(()).unwrap();
    settle(&mut p);
    assert_eq!(p.saved, Some(assigned()));
    assert_eq!(p.history.current, next);
    assert!(p.message.contains("newer edits"));
}
#[test]
fn delayed_reload_keeps_redo_even_if_value_returns_to_its_original() {
    let (release, gate) = mpsc::channel();
    let mut p = CockpitPanel::with_worker(move |kind, _| {
        if kind == FileKind::Reload {
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        Ok(Some(assigned()))
    });
    settle(&mut p);
    p.queue(FileKind::Reload);
    let mut next = p.history.current.clone();
    next.templates.get_mut("airlock").unwrap().title = "New draft".into();
    p.edit(next.clone(), false);
    p.move_history(false);
    release.send(()).unwrap();
    settle(&mut p);
    assert!(p.history.can_redo());
    p.move_history(true);
    assert_eq!(p.history.current, next);
}
#[test]
fn stale_startup_result_cannot_replace_a_control_drag() {
    let (release, gate) = mpsc::channel();
    let mut p = CockpitPanel::with_worker(move |_, _| {
        gate.recv_timeout(Duration::from_secs(5)).unwrap();
        Ok(Some(assigned()))
    });
    let mut next = p.history.current.clone();
    next.templates.get_mut("airlock").unwrap().controls[0].position[0] = 0.08;
    p.edit(next.clone(), true);
    release.send(()).unwrap();
    settle(&mut p);
    assert_eq!(p.draft(), &next);
    p.finish_gesture();
    assert_eq!(p.history.current, next);
}
#[test]
fn missing_file_starts_with_template_but_not_saved_or_applied() {
    let project = Project::new();
    let mut p = CockpitPanel::open(&project.0);
    settle(&mut p);
    assert!(!p.blocked);
    assert!(p.saved.is_none());
    assert!(p.applied.is_none());
    assert!(p.history.current.ships.is_empty());
    assert!(!project.0.join(document::FILE).exists());
}
#[test]
fn readings_keep_pressure_units_and_command_output_distinct() {
    let mut room = PioneerRoomState::uncommissioned("ship-A".into());
    room.pressure_pa = 101_325;
    room.light_commanded = true;
    room.light_on = false;
    let doc = assigned();
    let c = &doc.templates["airlock"].controls;
    assert_eq!(reading(&c[0].binding, Some(&room)), "101.33 kPa");
    assert_eq!(
        reading(&c[1].binding, Some(&room)),
        "command On · output Off"
    );
    assert_eq!(reading(&c[0].binding, None), "Unavailable");
}
#[test]
fn geometry_uses_the_same_edge_relative_clamping_as_existing_hud() {
    let b = egui::Rect::from_min_size(egui::pos2(100., 50.), egui::vec2(800., 600.));
    let (p, s) = arkline_client::cockpit_panel_geometry(
        b,
        [1., 1.],
        egui::vec2(999., 999.),
        egui::vec2(360., 200.),
        egui::vec2(376., 236.),
    );
    assert_eq!(p, egui::pos2(524., 414.));
    assert_eq!(s, egui::vec2(360., 200.));
    let c = Control {
        id: 1,
        label: "Gauge".into(),
        position: [0.1, 0.2],
        size: [0.5, 0.3],
        binding: Binding::Pressure {
            equipment: "pioneer.deck1.airlock.pressure".into(),
            max_kpa: 120.,
        },
    };
    let r = control_rect(b, &c);
    assert_eq!(r.min, egui::pos2(180., 170.));
}

fn live() -> Live {
    let snapshot:Snapshot=serde_json::from_value(serde_json::json!({
        "server_time_ms":1000,"pilots":[{"name":"Builder","credits":0,"ore":0,"location":{"kind":"docked"}}],"orders":[],"belt_ore":0,"ledger_sequence":0,"ledger_hash":"test"
    })).unwrap();
    let mut room = PioneerRoomState::uncommissioned("ship-A".into());
    room.commissioned = true;
    room.updated_at_ms = 1000;
    let assets = AssetsView {
        pioneer_room: Some(room),
        pioneer_room_layout: None,
        hangar: None,
        pioneer_interior_available: true,
        pioneer_interior_decks: 2,
        ships: vec![],
        storage: vec![],
        corporation: None,
        invitations: vec![],
    };
    Live {
        generation: 1,
        pilot: "Builder".into(),
        running: true,
        ready: true,
        controls_ready: true,
        notice: String::new(),
        snapshot: Some(snapshot),
        assets: Some(assets),
    }
}
#[test]
fn switch_requires_applied_content_matching_vessel_and_current_session_gates() {
    let base = live();
    let mut p = panel();
    p.poll(&base);
    assert!(p.light_request(&base, "ship-A", 2).is_none());
    p.apply();
    for case in 0..8 {
        let mut context = base.clone();
        match case {
            0 => context.running = false,
            1 => context.ready = false,
            2 => context.assets = None,
            3 => {
                context
                    .assets
                    .as_mut()
                    .unwrap()
                    .pioneer_room
                    .as_mut()
                    .unwrap()
                    .vessel_id = "other".into()
            }
            4 => {
                context
                    .assets
                    .as_mut()
                    .unwrap()
                    .pioneer_room
                    .as_mut()
                    .unwrap()
                    .commissioned = false
            }
            5 => context.snapshot.as_mut().unwrap().pilots[0].ship_class = ShipClass::Prospector,
            6 => context.controls_ready = false,
            _ => p.fresh_at = Instant::now() - Duration::from_secs(4),
        }
        assert!(
            p.light_request(&context, "ship-A", 2).is_none(),
            "gate {case}"
        );
        assert!(p.pending_command.is_none());
    }
    p.fresh_at = Instant::now();
    let before = base.assets.as_ref().unwrap().pioneer_room.clone();
    assert!(matches!(
        p.light_request(&base, "ship-A", 2),
        Some(Request::SetPioneerRoom {
            action: PioneerRoomCommand::Light { on: false }
        })
    ));
    assert_eq!(base.assets.as_ref().unwrap().pioneer_room, before);
    assert!(p.light_request(&base, "ship-A", 2).is_none());
    let mut next = base.clone();
    next.generation += 1;
    p.poll(&next);
    assert!(p.applied.is_none());
    assert!(p.pending_command.is_none());
    assert!(p.light_request(&next, "ship-A", 2).is_none());
}
#[test]
fn only_new_reported_state_can_clear_pending_command_and_timeout_never_replays() {
    let context = live();
    let mut p = panel();
    p.poll(&context);
    p.apply();
    p.light_request(&context, "ship-A", 2).unwrap();
    p.poll(&context);
    assert!(p.pending_command.is_some());
    p.pending_command.as_mut().unwrap().started = Instant::now() - Duration::from_secs(6);
    p.poll(&context);
    assert!(p.pending_command.as_ref().unwrap().timed_out);
    assert!(p.light_request(&context, "ship-A", 2).is_none());
    let mut next = context.clone();
    let r = next.assets.as_mut().unwrap().pioneer_room.as_mut().unwrap();
    r.light_commanded = false;
    r.light_on = true;
    r.updated_at_ms += 1;
    p.poll(&next);
    assert!(p.pending_command.is_none());
    assert!(p.runtime_message.contains("newer report"));
    assert!(
        next.assets
            .as_ref()
            .unwrap()
            .pioneer_room
            .as_ref()
            .unwrap()
            .light_on,
        "command report is not invented output"
    );
}
#[test]
fn lost_worker_locks_saving_and_does_not_replay() {
    let mut p = panel();
    let (tx, rx) = mpsc::sync_channel(1);
    let (reply, results) = mpsc::sync_channel(1);
    p.requests = Some(tx);
    p.results = results;
    p.queue(FileKind::Save);
    let _ = rx.recv().unwrap();
    drop(reply);
    p.poll_files();
    assert!(p.blocked);
    assert!(p.requests.is_none());
    assert!(p.message.contains("may have completed"));
    p.queue(FileKind::Save);
    assert!(rx.try_recv().is_err());
}
#[cfg(unix)]
#[test]
fn file_recovery_cannot_follow_symbolic_links() {
    use std::os::unix::fs::symlink;
    let project = Project::new();
    fs::create_dir(project.0.join("Assets")).unwrap();
    let target = project.0.join("missing");
    let file = project.0.join(document::FILE);
    symlink(&target, &file).unwrap();
    assert!(document::load(&project.0).is_err());
    assert!(document::save(&project.0, &assigned()).is_err());
    assert!(!target.exists());
    assert!(fs::symlink_metadata(&file)
        .unwrap()
        .file_type()
        .is_symlink());
}

fn frame(
    p: &mut CockpitPanel,
    ctx: &egui::Context,
    context: &Live,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Option<Request>) {
    let mut request = None;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1500., 960.),
            )),
            events,
            ..Default::default()
        },
        |ctx| request = p.show(ctx, context),
    );
    (output, request)
}
fn text_point(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == text => {
                Some(t.pos + t.galley.rect.center().to_vec2())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing visible label {text}"))
}
fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        },
    ]
}
#[test]
fn visible_workspace_selects_drags_and_applies_without_operating_draft() {
    let context = live();
    let mut p = panel();
    let ctx = egui::Context::default();
    frame(&mut p, &ctx, &context, vec![]);
    let (output, _) = frame(&mut p, &ctx, &context, vec![]);
    let point = text_point(&output, "Main light\ncommand On · output Off");
    assert!(frame(&mut p, &ctx, &context, pointer(point, true))
        .1
        .is_none());
    assert!(frame(&mut p, &ctx, &context, pointer(point, false))
        .1
        .is_none());
    assert_eq!(p.selected_control, 2);
    let original = p.history.current.clone();
    frame(&mut p, &ctx, &context, pointer(point, true));
    let end = point + egui::vec2(16., 10.);
    frame(&mut p, &ctx, &context, vec![egui::Event::PointerMoved(end)]);
    frame(&mut p, &ctx, &context, pointer(end, false));
    let moved = p.history.current.clone();
    assert_ne!(moved, original);
    p.move_history(false);
    assert_eq!(p.history.current, original);
    p.move_history(true);
    assert_eq!(p.history.current, moved);
    let (output, _) = frame(&mut p, &ctx, &context, vec![]);
    let apply = text_point(&output, "Apply panel");
    frame(&mut p, &ctx, &context, pointer(apply, true));
    frame(&mut p, &ctx, &context, pointer(apply, false));
    assert_eq!(p.applied.as_ref().unwrap().document, moved);
    assert!(p.pending_command.is_none());
}

fn wait_client(client: &mut EmbeddedClient, predicate: impl Fn(&EmbeddedClient) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        client.poll();
        if predicate(client) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "authority did not reach expected state: {:?}",
            client.status()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn full_editor_applies_content_while_paused_then_light_round_trip_uses_real_authority() {
    use eframe::App;
    let project = Project::new();
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut editor = crate::Editor::new(&cc, project.0.clone(), "CockpitRoundTrip".into());
    editor.workspace = 4;
    wait_client(&mut editor.client, |c| {
        c.ready() && c.assets().and_then(|a| a.pioneer_room).is_some()
    });
    settle(&mut editor.cockpit);
    let initial = editor.client.snapshot().unwrap().server_time_ms;
    let context = Live::from_client(&editor.client);
    editor.cockpit.poll(&context);
    let vessel = context.current_vessel().unwrap().to_owned();
    let mut doc = editor.cockpit.history.current.clone();
    doc.ships.insert(
        vessel.clone(),
        Assignment {
            template: "airlock".into(),
            override_panel: None,
        },
    );
    editor.cockpit.edit(doc, false);
    editor.cockpit.apply();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1500., 960.),
            )),
            ..Default::default()
        },
        |ctx| editor.update(ctx, &mut eframe::Frame::_new_kittest()),
    );
    assert_eq!(editor.client.snapshot().unwrap().server_time_ms, initial);
    assert!(
        !editor
            .client
            .assets()
            .unwrap()
            .pioneer_room
            .unwrap()
            .commissioned
    );
    editor.client.play();
    wait_client(&mut editor.client, |c| {
        c.status() == &EmbeddedStatus::Running
            && c.snapshot().is_some_and(|s| s.server_time_ms > initial)
    });
    editor.client.send_command(Request::SetPioneerRoom {
        action: PioneerRoomCommand::Breaker { closed: true },
    });
    wait_client(&mut editor.client, |c| {
        c.assets()
            .and_then(|a| a.pioneer_room)
            .is_some_and(|r| r.commissioned && r.breaker_closed)
    });
    let context = Live::from_client(&editor.client);
    editor.cockpit.poll(&context);
    let before = context
        .assets
        .as_ref()
        .unwrap()
        .pioneer_room
        .as_ref()
        .unwrap()
        .clone();
    let request = editor.cockpit.light_request(&context, &vessel, 2).unwrap();
    assert_eq!(
        editor.client.assets().unwrap().pioneer_room.unwrap(),
        before
    );
    editor.client.send_command(request);
    wait_client(&mut editor.client, |c| {
        c.assets().and_then(|a| a.pioneer_room).is_some_and(|r| {
            !r.light_commanded && !r.light_on && r.updated_at_ms > before.updated_at_ms
        })
    });
    editor.cockpit.poll(&Live::from_client(&editor.client));
    assert!(editor.cockpit.pending_command.is_none());
    assert!(editor.cockpit.runtime_message.contains("newer report"));
    editor.client.pause();
    wait_client(&mut editor.client, |c| {
        c.status() == &EmbeddedStatus::Paused
    });
    editor.client.on_exit();
}

#[test]
fn applied_switch_pointer_click_emits_typed_request_without_predicting_output() {
    let context = live();
    let mut p = panel();
    p.poll(&context);
    p.apply();
    p.operate = true;
    let ctx = egui::Context::default();
    frame(&mut p, &ctx, &context, vec![]);
    let (output, _) = frame(&mut p, &ctx, &context, vec![]);
    let point = text_point(&output, "Main light · command On · output Off");
    frame(&mut p, &ctx, &context, pointer(point, true));
    let (_, request) = frame(&mut p, &ctx, &context, pointer(point, false));
    assert!(matches!(
        request,
        Some(Request::SetPioneerRoom {
            action: PioneerRoomCommand::Light { on: false }
        })
    ));
    assert!(p.pending_command.is_some());
    assert!(
        context
            .assets
            .unwrap()
            .pioneer_room
            .unwrap()
            .light_commanded
    );
}
#[test]
fn late_startup_selects_the_saved_ship_override_without_installing_runtime() {
    let mut doc = assigned();
    let mut custom = doc.panel("ship-A").unwrap().clone();
    custom.title = "Saved ship override".into();
    doc.ships.get_mut("ship-A").unwrap().override_panel = Some(custom);
    let (release, gate) = mpsc::channel();
    let mut p = CockpitPanel::with_worker(move |_, _| {
        gate.recv_timeout(Duration::from_secs(5)).unwrap();
        Ok(Some(doc.clone()))
    });
    p.select_ship("ship-A");
    release.send(()).unwrap();
    settle(&mut p);
    assert!(!p.edit_shared);
    assert_eq!(p.panel(p.draft()).unwrap().title, "Saved ship override");
    assert!(p.applied.is_none());
}
#[test]
fn failed_reads_preserve_saved_and_applied_content_until_explicit_recovery() {
    let mut p = CockpitPanel::with_worker(|kind, _| {
        if kind == FileKind::Reload {
            Err("permission denied".into())
        } else {
            Ok(Some(assigned()))
        }
    });
    settle(&mut p);
    p.apply();
    let mut next = p.history.current.clone();
    next.templates.get_mut("airlock").unwrap().title = "Unsaved".into();
    p.edit(next.clone(), false);
    p.queue(FileKind::Reload);
    settle(&mut p);
    assert!(p.blocked);
    assert_eq!(p.saved, Some(assigned()));
    assert_eq!(p.history.current, next);
    assert_eq!(p.applied.as_ref().unwrap().document, assigned());
    assert!(p.message.contains("permission denied"));
}
#[test]
fn failed_save_keeps_previous_saved_baseline_and_draft() {
    let mut p = CockpitPanel::with_worker(|kind, _| {
        if kind == FileKind::Save {
            Err("disk full".into())
        } else {
            Ok(Some(assigned()))
        }
    });
    settle(&mut p);
    let mut next = p.history.current.clone();
    next.templates.get_mut("airlock").unwrap().title = "Unsaved".into();
    p.edit(next.clone(), false);
    p.queue(FileKind::Save);
    settle(&mut p);
    assert_eq!(p.saved, Some(assigned()));
    assert_eq!(p.history.current, next);
    assert!(p.message.contains("disk full"));
}
#[test]
fn closing_an_old_project_cannot_deliver_its_save_into_a_new_project() {
    let old = Project::new();
    let new = Project::new();
    let root = old.0.clone();
    let (started_tx, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let (finished_tx, finished) = mpsc::channel();
    let mut a = CockpitPanel::with_worker(move |kind, doc| {
        if kind == FileKind::Save {
            started_tx.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            let doc = doc.unwrap();
            document::save(&root, &doc)?;
            finished_tx.send(()).unwrap();
            Ok(Some(doc))
        } else {
            Ok(None)
        }
    });
    settle(&mut a);
    a.history = History::new(assigned());
    a.queue(FileKind::Save);
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(a);
    let mut different = assigned();
    different.templates.get_mut("airlock").unwrap().title = "New project".into();
    document::save(&new.0, &different).unwrap();
    let mut b = CockpitPanel::open(&new.0);
    settle(&mut b);
    release.send(()).unwrap();
    finished.recv_timeout(Duration::from_secs(5)).unwrap();
    b.poll_files();
    assert_eq!(b.history.current, different);
    assert_eq!(document::load(&new.0).unwrap(), Some(different));
    assert_eq!(document::load(&old.0).unwrap(), Some(assigned()));
}
