use super::*;
fn connect(server: &mut Server, session: &mut Option<String>, name: &str, now: u64) -> Response {
    let response = server.handle_at(
        session,
        Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            world_layout_version: 1,
            supports_room_controls: true,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            supports_docked_traversal_v2: false,
            pilot: name.into(),
            resume_token: None,
        },
        now,
    );
    assert!(response.ok, "{}", response.message);
    response
}
fn request(
    server: &mut Server,
    session: &mut Option<String>,
    request: Request,
    now: u64,
) -> Response {
    let response = server.handle_at(session, request, now);
    assert!(response.ok, "{}", response.message);
    response
}
fn raw(server: &Server) -> String {
    server
        .db
        .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
        .unwrap()
}
fn world(server: &Server) -> World {
    serde_json::from_str(&raw(server)).unwrap()
}
fn seed_power(server: &Server, name: &str, charge: u64, now: u64) {
    let mut w = world(server);
    let p = w.pilots.get_mut(name).unwrap().view.power.as_mut().unwrap();
    p.charge_j = charge;
    p.updated_at_ms = now;
    p.regen_remainder_mj = 137;
    server
        .db
        .execute(
            "UPDATE world SET json=?1 WHERE id=1",
            [serde_json::to_string(&w).unwrap()],
        )
        .unwrap();
}
fn view(response: &Response, name: &str) -> PowerAccountingView {
    let p = response
        .snapshot
        .as_ref()
        .unwrap()
        .pilots
        .iter()
        .find(|p| p.name == name)
        .unwrap();
    let v = p
        .power_accounting
        .clone()
        .expect("authoritative accounting");
    v.validate_for(name, p.ship_class, p.power.as_ref().unwrap())
        .unwrap();
    v
}
#[test]
fn response_only_budget_tracks_real_room_commands_and_never_persists_telemetry() {
    let mut server = Server::open(":memory:").unwrap();
    let mut session = None;
    let r = connect(&mut server, &mut session, "Budget", 1000);
    let initial = view(&r, "Budget");
    assert_eq!(initial.stored_charge_rate_w, 0);
    assert!(initial.net_available_charge_w > 0);
    assert!(!raw(&server).contains("power_accounting"));
    let ext = initial.accounted_reserved_w;
    for (action, room_w) in [
        (PioneerRoomCommand::Breaker { closed: true }, 28),
        (PioneerRoomCommand::Light { on: false }, 4),
        (
            PioneerRoomCommand::Door {
                open: false,
                manual: false,
            },
            4,
        ),
        (
            PioneerRoomCommand::Vent {
                mode: VentMode::Exhaust,
            },
            100,
        ),
        (PioneerRoomCommand::Breaker { closed: false }, 0),
    ] {
        let r = request(
            &mut server,
            &mut session,
            Request::SetPioneerRoom { action },
            1000,
        );
        let v = view(&r, "Budget");
        assert_eq!(v.accounted_reserved_w, ext + room_w);
        assert_eq!(
            v.loads.iter().map(|r| r.reserved_w).sum::<u64>(),
            v.accounted_reserved_w
        );
        assert!(!raw(&server).contains("power_accounting"));
    }
    let r = request(&mut server, &mut session, Request::Snapshot, 2000);
    let revision = r
        .snapshot
        .as_ref()
        .unwrap()
        .exterior
        .as_ref()
        .unwrap()
        .definition
        .revision;
    let r = request(
        &mut server,
        &mut session,
        Request::SetPioneerExterior {
            enabled: false,
            expected_revision: revision,
        },
        2000,
    );
    assert_eq!(view(&r, "Budget").accounted_reserved_w, 0);
    assert_eq!(r.snapshot.unwrap().pilots[0].ore, 0);
}
#[test]
fn partial_and_zero_charge_reconcile_with_existing_reserved_rate_without_double_debits() {
    let mut server = Server::open(":memory:").unwrap();
    let mut session = None;
    connect(&mut server, &mut session, "Budget", 1000);
    request(
        &mut server,
        &mut session,
        Request::SetPioneerRoom {
            action: PioneerRoomCommand::Breaker { closed: true },
        },
        1000,
    );
    seed_power(&server, "Budget", 100000, 1000);
    let before = request(&mut server, &mut session, Request::Snapshot, 1000);
    let b = view(&before, "Budget");
    let after = request(&mut server, &mut session, Request::Snapshot, 1234);
    let a = view(&after, "Budget");
    assert_eq!(
        a.charge_j - b.charge_j,
        (b.net_available_charge_w * 234 + 137) / 1000
    );
    assert_eq!(a.instantaneous_debits, b.instantaneous_debits);
    seed_power(&server, "Budget", 0, 1234);
    let zero = request(&mut server, &mut session, Request::Snapshot, 1234);
    let z = view(&zero, "Budget");
    let exterior = zero.snapshot.as_ref().unwrap().exterior.as_ref().unwrap();
    assert!(
        !exterior
            .vessels
            .iter()
            .find(|v| v.pilot == "Budget")
            .unwrap()
            .powered
    );
    assert!(
        z.accounted_reserved_w > 0,
        "enabled exterior remains reserved even with lamps off"
    );
    assert!(z
        .loads
        .iter()
        .filter(|r| r.component_id.starts_with("pioneer.deck1"))
        .all(|r| r.reserved_w == 0));
    assert_eq!(z.stored_charge_rate_w, z.net_available_charge_w);
    assert_eq!(z.charge_j, 0);
}
#[test]
fn parked_hull_and_other_pilot_projection_keep_actual_vessel_identity() {
    let mut server = Server::open(":memory:").unwrap();
    let (mut a, mut b) = (None, None);
    let first = connect(&mut server, &mut a, "A", 1000);
    let original = view(&first, "A");
    connect(&mut server, &mut b, "B", 1000);
    let switched = request(
        &mut server,
        &mut a,
        Request::SelectShip {
            ship_class: ShipClass::Prospector,
        },
        1000,
    );
    let active = view(&switched, "A");
    assert_ne!(active.vessel_id, original.vessel_id);
    assert!(active
        .unmetered
        .iter()
        .all(|row| !row.component_id.starts_with("pioneer.")));
    let parked = switched
        .assets
        .as_ref()
        .unwrap()
        .ships
        .iter()
        .find(|s| s.ship_class == ShipClass::Pioneer)
        .unwrap();
    let parked_view = parked.power_accounting.as_ref().unwrap();
    assert_eq!(parked_view.vessel_id, original.vessel_id);
    assert_eq!(parked_view.owner_pilot, "A");
    parked_view
        .validate_for("A", parked.ship_class, parked.power.as_ref().unwrap())
        .unwrap();
    assert_ne!(view(&switched, "B").vessel_id, parked_view.vessel_id);
    let w = world(&server);
    assert_eq!(
        parked_view.vessel_id,
        w.accounts["A"].vessel_id(ShipClass::Pioneer).unwrap()
    );
    assert!(!raw(&server).contains("power_accounting"));
}
#[test]
fn failed_room_transaction_does_not_commit_budget_or_energy_changes() {
    let mut server = Server::open(":memory:").unwrap();
    let mut session = None;
    connect(&mut server, &mut session, "Budget", 1000);
    let before = raw(&server);
    server.db.execute_batch("CREATE TRIGGER reject_budget_write BEFORE UPDATE ON world BEGIN SELECT RAISE(ABORT,'test rollback'); END;").unwrap();
    let r = server.handle_at(
        &mut session,
        Request::SetPioneerRoom {
            action: PioneerRoomCommand::Breaker { closed: true },
        },
        1000,
    );
    assert!(!r.ok);
    assert_eq!(raw(&server), before);
    server
        .db
        .execute_batch("DROP TRIGGER reject_budget_write;")
        .unwrap();
    let r = request(&mut server, &mut session, Request::Snapshot, 1000);
    assert_eq!(
        view(&r, "Budget")
            .loads
            .iter()
            .filter(|r| r.component_id.starts_with("pioneer.deck1"))
            .map(|r| r.reserved_w)
            .sum::<u64>(),
        0
    );
}

#[test]
#[ignore = "writes actual command-driven electrical accounting Responses"]
fn export_electrical_accounting_responses() {
    use sha2::{Digest, Sha256};
    let path = std::path::PathBuf::from(
        std::env::var_os("ARKLINE_POWER_ACCOUNTING_PROOF").expect("explicit output"),
    );
    std::fs::create_dir(&path).expect("new proof directory");
    let mut rows = Vec::new();
    let mut save = |label: &str, mut response: Response, paused: bool| {
        assert!(response.ok, "{}", response.message);
        response.resume_token = None;
        let file = format!("{label}.json");
        let bytes = serde_json::to_vec_pretty(&response).unwrap();
        let state = view(&response, "Budget");
        rows.push(serde_json::json!({"file":file,"sha256":hex::encode(Sha256::digest(&bytes)),"paused":paused,"sampled_at_ms":state.sampled_at_ms,"charge_j":state.charge_j,"accounted_reserved_w":state.accounted_reserved_w,"stored_charge_rate_w":state.stored_charge_rate_w}));
        std::fs::write(path.join(file), bytes).unwrap();
    };
    let mut server = Server::open(":memory:").unwrap();
    let mut session = None;
    save(
        "00-full-uncommissioned",
        connect(&mut server, &mut session, "Budget", 1000),
        false,
    );
    for (label, action) in [
        (
            "01-room-commissioned",
            PioneerRoomCommand::Breaker { closed: true },
        ),
        ("02-main-light-off", PioneerRoomCommand::Light { on: false }),
        (
            "03-door-closed",
            PioneerRoomCommand::Door {
                open: false,
                manual: false,
            },
        ),
        (
            "04-vent-exhaust",
            PioneerRoomCommand::Vent {
                mode: VentMode::Exhaust,
            },
        ),
    ] {
        save(
            label,
            request(
                &mut server,
                &mut session,
                Request::SetPioneerRoom { action },
                1000,
            ),
            false,
        );
    }
    let r = request(&mut server, &mut session, Request::Snapshot, 1000);
    let revision = r.snapshot.unwrap().exterior.unwrap().definition.revision;
    save(
        "05-exterior-disabled",
        request(
            &mut server,
            &mut session,
            Request::SetPioneerExterior {
                enabled: false,
                expected_revision: revision,
            },
            1000,
        ),
        false,
    );
    save(
        "06-parked-pioneer",
        request(
            &mut server,
            &mut session,
            Request::SelectShip {
                ship_class: ShipClass::Prospector,
            },
            1000,
        ),
        false,
    );
    request(
        &mut server,
        &mut session,
        Request::SelectShip {
            ship_class: ShipClass::Pioneer,
        },
        1000,
    );
    request(&mut server, &mut session, Request::Undock, 1000);
    save(
        "07-partial-after-real-warp-debit",
        request(
            &mut server,
            &mut session,
            Request::Warp {
                destination: Destination::Belt,
            },
            1000,
        ),
        false,
    );
    save(
        "08-partial-charging",
        request(&mut server, &mut session, Request::Snapshot, 1600),
        false,
    );
    save(
        "09-emergency-light",
        request(
            &mut server,
            &mut session,
            Request::SetPioneerRoom {
                action: PioneerRoomCommand::Breaker { closed: false },
            },
            1600,
        ),
        false,
    );
    save(
        "10-reserve-used",
        request(&mut server, &mut session, Request::Snapshot, 2600),
        false,
    );
    save(
        "11-reserve-recharging",
        request(
            &mut server,
            &mut session,
            Request::SetPioneerRoom {
                action: PioneerRoomCommand::Breaker { closed: true },
            },
            2600,
        ),
        false,
    );
    let directory = std::env::temp_dir().join(format!(
        "arkline-electrical-paused-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let mut editor = editor::EditorWorld::open(&directory).unwrap();
    let mut session = None;
    let r = editor.handle(
        &mut session,
        Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            world_layout_version: WORLD_LAYOUT_SCHEMA_VERSION,
            supports_room_controls: true,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            supports_docked_traversal_v2: false,
            pilot: "Budget".into(),
            resume_token: None,
        },
    );
    assert!(r.ok, "{}", r.message);
    editor.play();
    let r = editor.handle(
        &mut session,
        Request::SetPioneerRoom {
            action: PioneerRoomCommand::Breaker { closed: true },
        },
    );
    assert!(r.ok, "{}", r.message);
    editor.pause();
    let before = editor.handle(&mut session, Request::Snapshot);
    let state = view(&before, "Budget");
    save("12-paused-before", before, true);
    editor.advance(60_000).unwrap();
    let after = editor.handle(&mut session, Request::Snapshot);
    assert_eq!(view(&after, "Budget"), state);
    save("13-paused-after", after, true);
    std::fs::write(path.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"pilot":"Budget","actual_command_responses":true,"response_values_unmodified":true,"resume_credentials_removed":true,"zero_charge_native_fixture_omitted":true,"frames":rows})).unwrap()).unwrap();
}

#[test]
#[ignore = "writes actual current-flagship electrical accounting Responses"]
fn export_current_electrical_accounting_responses() {
    use sha2::{Digest, Sha256};
    fn call(w: &mut editor::EditorWorld, s: &mut Option<String>, q: Request) -> Response {
        let r = w.handle(s, q);
        assert!(r.ok, "{}", r.message);
        r
    }
    let path = std::path::PathBuf::from(
        std::env::var_os("ARKLINE_POWER_ACCOUNTING_PROOF").expect("explicit output"),
    );
    std::fs::create_dir(&path).expect("new proof directory");
    let mut rows = Vec::new();
    let mut save = |label: &str, mut response: Response, paused: bool| {
        response.resume_token = None;
        let file = format!("{label}.json");
        let bytes = serde_json::to_vec_pretty(&response).unwrap();
        let state = view(&response, "Budget");
        rows.push(serde_json::json!({"file":file,"sha256":hex::encode(Sha256::digest(&bytes)),"paused":paused,"sampled_at_ms":state.sampled_at_ms,"charge_j":state.charge_j,"accounted_reserved_w":state.accounted_reserved_w,"stored_charge_rate_w":state.stored_charge_rate_w}));
        std::fs::write(path.join(file), bytes).unwrap();
    };
    let dir = std::env::temp_dir().join(format!(
        "arkline-power-current-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let mut w = editor::EditorWorld::open_with_creation_scene(&dir, SceneLayoutRevision::DockingV1)
        .unwrap();
    w.play();
    let mut session = None;
    let first = call(
        &mut w,
        &mut session,
        Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            world_layout_version: WORLD_LAYOUT_SCHEMA_VERSION,
            supports_room_controls: true,
            supports_fleet_hangar: true,
            supports_pioneer_interior: true,
            supports_docked_traversal_v2: true,
            pilot: "Budget".into(),
            resume_token: None,
        },
    );
    assert_eq!(
        first
            .assets
            .as_ref()
            .unwrap()
            .drone_bay
            .as_ref()
            .unwrap()
            .stock
            .len(),
        15
    );
    save("00-full-uncommissioned", first, false);
    for (label, action) in [
        (
            "01-room-commissioned",
            PioneerRoomCommand::Breaker { closed: true },
        ),
        ("02-main-light-off", PioneerRoomCommand::Light { on: false }),
        (
            "03-door-closed",
            PioneerRoomCommand::Door {
                open: false,
                manual: false,
            },
        ),
        (
            "04-vent-exhaust",
            PioneerRoomCommand::Vent {
                mode: VentMode::Exhaust,
            },
        ),
    ] {
        save(
            label,
            call(&mut w, &mut session, Request::SetPioneerRoom { action }),
            false,
        );
    }
    let r = call(&mut w, &mut session, Request::Snapshot);
    let revision = r.snapshot.unwrap().exterior.unwrap().definition.revision;
    save(
        "05-exterior-disabled",
        call(
            &mut w,
            &mut session,
            Request::SetPioneerExterior {
                enabled: false,
                expected_revision: revision,
            },
        ),
        false,
    );
    // DockingV1 intentionally forbids hull switching; parked-hull accounting
    // is covered by the separate legacy command fixture, not policy bypass.
    save(
        "06-before-departure",
        call(&mut w, &mut session, Request::Snapshot),
        false,
    );
    call(&mut w, &mut session, Request::Undock);
    for _ in 0..180 {
        w.advance(100).unwrap();
    }
    let before = call(&mut w, &mut session, Request::Snapshot);
    assert_eq!(
        before.snapshot.as_ref().unwrap().pilots[0]
            .flight
            .as_ref()
            .unwrap()
            .docking
            .as_ref()
            .unwrap()
            .phase,
        DockingPhase::Free
    );
    let warp = call(
        &mut w,
        &mut session,
        Request::Warp {
            destination: Destination::Belt,
        },
    );
    assert_eq!(
        view(&before, "Budget").charge_j - view(&warp, "Budget").charge_j,
        power_spec(ShipClass::Pioneer).warp_cost_j
    );
    save("07-partial-after-real-warp-debit", warp, false);
    for _ in 0..6 {
        w.advance(100).unwrap();
    }
    save(
        "08-partial-charging",
        call(&mut w, &mut session, Request::Snapshot),
        false,
    );
    save(
        "09-emergency-light",
        call(
            &mut w,
            &mut session,
            Request::SetPioneerRoom {
                action: PioneerRoomCommand::Breaker { closed: false },
            },
        ),
        false,
    );
    for _ in 0..10 {
        w.advance(100).unwrap();
    }
    save(
        "10-reserve-used",
        call(&mut w, &mut session, Request::Snapshot),
        false,
    );
    save(
        "11-reserve-recharging",
        call(
            &mut w,
            &mut session,
            Request::SetPioneerRoom {
                action: PioneerRoomCommand::Breaker { closed: true },
            },
        ),
        false,
    );
    w.pause();
    let before = call(&mut w, &mut session, Request::Snapshot);
    let state = view(&before, "Budget");
    save("12-paused-before", before, true);
    w.advance(60000).unwrap();
    let after = call(&mut w, &mut session, Request::Snapshot);
    assert_eq!(view(&after, "Budget"), state);
    save("13-paused-after", after, true);
    std::fs::write(path.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"pilot":"Budget","realm":"fresh DockingV1 current flagship preview","capability":WORLD_LAYOUT_SCHEMA_VERSION,"registration":PIONEER_DOCKED_REGISTRATION_REVISION,"actual_command_responses":true,"response_values_unmodified":true,"resume_credentials_removed":true,"zero_charge_native_fixture_omitted":true,"frames":rows})).unwrap()).unwrap();
}
