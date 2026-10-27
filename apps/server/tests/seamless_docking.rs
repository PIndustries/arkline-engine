//! Real command/clock/reopen acceptance on disposable fresh realms.
use arkline_protocol::*;
use arkline_server::editor::EditorWorld;
fn directory(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "arkline-docking-{label}-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ))
}
fn connect(
    w: &mut EditorWorld,
    s: &mut Option<String>,
    name: &str,
    token: Option<String>,
    version: u32,
) -> Response {
    w.handle(
        s,
        Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            world_layout_version: version,
            supports_room_controls: true,
            supports_fleet_hangar: true,
            supports_pioneer_interior: true,
            supports_docked_traversal_v2: true,
            pilot: name.into(),
            resume_token: token,
        },
    )
}
fn ok(w: &mut EditorWorld, s: &mut Option<String>, r: Request) -> Response {
    let r = w.handle(s, r);
    assert!(r.ok, "{}", r.message);
    r
}
fn view(w: &mut EditorWorld, s: &mut Option<String>) -> PilotView {
    let name = s.as_ref().unwrap().clone();
    ok(w, s, Request::Snapshot)
        .snapshot
        .unwrap()
        .pilots
        .into_iter()
        .find(|p| p.name == name)
        .unwrap()
}
fn flight(w: &mut EditorWorld, s: &mut Option<String>) -> FlightState {
    view(w, s).flight.unwrap()
}
fn advance(w: &mut EditorWorld, ms: u64) {
    for _ in 0..ms / 50 {
        w.advance(50).unwrap();
    }
}
fn setup(label: &str) -> (EditorWorld, Option<String>, std::path::PathBuf, String) {
    let dir = directory(label);
    let mut w =
        EditorWorld::open_with_creation_scene(&dir, SceneLayoutRevision::DockingV1).unwrap();
    let mut s = None;
    let r = connect(&mut w, &mut s, "Pilot", None, WORLD_LAYOUT_SCHEMA_VERSION);
    assert!(r.ok, "{}", r.message);
    w.play();
    (w, s, dir, r.resume_token.unwrap())
}
#[test]
fn departure_and_arrival_are_continuous_and_access_waits_for_capture() {
    let (mut w, mut s, _dir, _token) = setup("roundtrip");
    let initial = flight(&mut w, &mut s);
    let slot = initial.docking.as_ref().unwrap().berth_id;
    let ledger = ok(&mut w, &mut s, Request::ExportLedger).ledger.unwrap();
    ok(&mut w, &mut s, Request::Undock);
    assert_eq!(flight(&mut w, &mut s).position, initial.position);
    assert_eq!(view(&mut w, &mut s).location, Location::Station);
    assert!(!w.handle(&mut s, Request::WithdrawOre { units: 1 }).ok);
    let mut previous = initial.position;
    for _ in 0..340 {
        advance(&mut w, 50);
        let f = flight(&mut w, &mut s);
        let distance = f
            .position
            .iter()
            .zip(previous)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt();
        assert!(distance <= 2., "pose jumped {distance}");
        previous = f.position;
    }
    let free = flight(&mut w, &mut s);
    assert_eq!(free.docking.as_ref().unwrap().phase, DockingPhase::Free);
    assert_eq!(free.position, docking_gate_position(slot));
    ok(&mut w, &mut s, Request::Dock);
    assert_eq!(flight(&mut w, &mut s).position, free.position);
    for _ in 0..519 {
        advance(&mut w, 50);
        let p = view(&mut w, &mut s);
        assert_ne!(p.location, Location::Docked);
        let f = p.flight.unwrap();
        assert!(
            f.position
                .iter()
                .zip(previous)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                .sqrt()
                <= 2.
        );
        previous = f.position;
    }
    advance(&mut w, 50);
    let p = view(&mut w, &mut s);
    assert_eq!(p.location, Location::Docked);
    let f = p.flight.unwrap();
    assert_eq!(f.position, initial.position);
    assert_eq!(f.orientation, docking_berth_orientation());
    assert!(f.docking.unwrap().secured());
    let after = ok(&mut w, &mut s, Request::ExportLedger).ledger.unwrap();
    assert_eq!(ledger.snapshot.ledger_hash, after.snapshot.ledger_hash);
    arkline_verifier::verify(&after, None).unwrap();
}
#[test]
fn cancellation_brakes_continuously_and_restart_requires_exact_resume() {
    let (mut w, mut s, dir, token) = setup("recovery");
    ok(&mut w, &mut s, Request::Undock);
    advance(&mut w, 7000);
    let before = flight(&mut w, &mut s);
    let id = before.docking.as_ref().unwrap().operation_id;
    assert!(
        !w.handle(
            &mut s,
            Request::CancelDocking {
                operation_id: id + 1
            }
        )
        .ok
    );
    ok(&mut w, &mut s, Request::CancelDocking { operation_id: id });
    let accepted = flight(&mut w, &mut s);
    assert_eq!(accepted.position, before.position);
    assert_eq!(accepted.velocity, before.velocity);
    ok(&mut w, &mut s, Request::CancelDocking { operation_id: id });
    advance(&mut w, 2000);
    let held = flight(&mut w, &mut s);
    assert_eq!(held.docking.as_ref().unwrap().phase, DockingPhase::Held);
    assert_eq!(held.velocity, [0.; 3]);
    w.disconnect(&mut s);
    drop(w);
    let mut w = EditorWorld::open(&dir).unwrap();
    assert!(w.is_paused());
    let r = connect(
        &mut w,
        &mut s,
        "Pilot",
        Some(token),
        WORLD_LAYOUT_SCHEMA_VERSION,
    );
    assert!(r.ok, "{}", r.message);
    assert_eq!(flight(&mut w, &mut s).position, held.position);
    w.play();
    advance(&mut w, 5000);
    assert_eq!(flight(&mut w, &mut s).position, held.position);
    ok(&mut w, &mut s, Request::ResumeDocking { operation_id: id });
    advance(&mut w, 15000);
    assert_eq!(
        flight(&mut w, &mut s).docking.unwrap().phase,
        DockingPhase::Free
    );
}
#[test]
fn unique_owned_berths_old_capability_rejected_and_late_join_sees_current_phase() {
    let (mut w, mut alice, _dir, _token) = setup("two-pilot");
    let a = flight(&mut w, &mut alice);
    let mut bob = None;
    assert!(!connect(&mut w, &mut bob, "Bob", None, 1).ok);
    assert!(bob.is_none());
    assert!(connect(&mut w, &mut bob, "Bob", None, WORLD_LAYOUT_SCHEMA_VERSION).ok);
    let b = flight(&mut w, &mut bob);
    assert_ne!(a.docking.unwrap().berth_id, b.docking.unwrap().berth_id);
    ok(&mut w, &mut alice, Request::Undock);
    advance(&mut w, 4000);
    let mut late = None;
    assert!(
        connect(
            &mut w,
            &mut late,
            "LateJoin",
            None,
            WORLD_LAYOUT_SCHEMA_VERSION
        )
        .ok
    );
    let own = flight(&mut w, &mut alice);
    let peer = ok(&mut w, &mut late, Request::Snapshot)
        .snapshot
        .unwrap()
        .pilots
        .into_iter()
        .find(|p| p.name == "Pilot")
        .unwrap()
        .flight
        .unwrap();
    assert_eq!(own, peer);
    assert!(
        !w.handle(
            &mut alice,
            Request::PilotInput {
                translation: [1., 0., 0.],
                rotation: [0.; 3]
            }
        )
        .ok
    );
    assert!(
        !w.handle(
            &mut alice,
            Request::Warp {
                destination: Destination::Belt
            }
        )
        .ok
    );
    assert!(
        !w.handle(
            &mut bob,
            Request::ResumeDocking {
                operation_id: own.docking.unwrap().operation_id
            }
        )
        .ok
    );
}
#[test]
fn opening_existing_orbital_world_never_enables_or_relocates_it() {
    let dir = directory("legacy");
    let mut w =
        EditorWorld::open_with_creation_scene(&dir, SceneLayoutRevision::OrbitalV1).unwrap();
    let mut s = None;
    assert!(connect(&mut w, &mut s, "Pilot", None, 1).ok);
    let before = flight(&mut w, &mut s);
    assert!(before.docking.is_none());
    drop(w);
    let mut w =
        EditorWorld::open_with_creation_scene(&dir, SceneLayoutRevision::DockingV1).unwrap();
    let snapshot = w.handle(&mut None, Request::Snapshot).snapshot.unwrap();
    assert_eq!(
        snapshot.world_layout.unwrap().revision,
        SceneLayoutRevision::OrbitalV1
    );
    assert_eq!(
        snapshot.pilots[0].flight.as_ref().unwrap().position,
        before.position
    );
    assert!(snapshot.pilots[0]
        .flight
        .as_ref()
        .unwrap()
        .docking
        .is_none());
}
#[test]
fn unclean_restart_preserves_midflight_pose_and_operation_without_offline_catchup() {
    let (mut w, mut s, dir, token) = setup("unclean");
    assert!(!w.handle(&mut s, Request::DisembarkShip).ok);
    assert!(!w.handle(&mut s, Request::EnterInterior).ok);
    ok(&mut w, &mut s, Request::Undock);
    advance(&mut w, 6300);
    let before = flight(&mut w, &mut s);
    let id = before.docking.as_ref().unwrap().operation_id;
    assert!(!w.handle(&mut s, Request::Undock).ok);
    assert_eq!(flight(&mut w, &mut s), before);
    drop(w);
    s = None;
    let mut w = EditorWorld::open(&dir).unwrap();
    assert!(
        connect(
            &mut w,
            &mut s,
            "Pilot",
            Some(token),
            WORLD_LAYOUT_SCHEMA_VERSION
        )
        .ok
    );
    let held = flight(&mut w, &mut s);
    assert_eq!(held.position, before.position);
    assert_eq!(held.docking.as_ref().unwrap().operation_id, id);
    assert_eq!(held.docking.as_ref().unwrap().phase, DockingPhase::Held);
    w.play();
    advance(&mut w, 3000);
    assert_eq!(flight(&mut w, &mut s).position, before.position);
    ok(&mut w, &mut s, Request::ResumeDocking { operation_id: id });
    advance(&mut w, 15000);
    assert_eq!(
        flight(&mut w, &mut s).docking.unwrap().phase,
        DockingPhase::Free
    );
}
#[test]
fn free_flight_cannot_tunnel_into_an_occupied_berth_or_platform() {
    let (mut w, mut s, _dir, _token) = setup("collision");
    ok(&mut w, &mut s, Request::Undock);
    advance(&mut w, 17000);
    let initial = flight(&mut w, &mut s);
    let slot = initial.docking.as_ref().unwrap().berth_id;
    ok(
        &mut w,
        &mut s,
        Request::Navigate {
            mode: NavigationMode::FlyTo {
                position: docking_berth_position(slot),
            },
        },
    );
    for _ in 0..400 {
        advance(&mut w, 50);
        let f = flight(&mut w, &mut s);
        assert!(f.position[0] > docking_berth_position(slot)[0] + 100.);
    }
    let stopped = flight(&mut w, &mut s);
    assert_eq!(stopped.velocity, [0.; 3]);
    assert_eq!(stopped.mode, NavigationMode::Stop);
    ok(&mut w, &mut s, Request::Dock);
    advance(&mut w, 26000);
    assert_eq!(view(&mut w, &mut s).location, Location::Docked);
}

#[test]
fn another_ship_blocks_capture_without_overlap_and_resume_uses_the_same_operation() {
    let (mut w, mut alice, _dir, _token) = setup("occupied-route");
    let mut bob = None;
    assert!(connect(&mut w, &mut bob, "Bob", None, WORLD_LAYOUT_SCHEMA_VERSION).ok);
    ok(&mut w, &mut alice, Request::Undock);
    ok(&mut w, &mut bob, Request::Undock);
    advance(&mut w, 17000);
    let a = flight(&mut w, &mut alice);
    let b = flight(&mut w, &mut bob);
    let slot = a.docking.as_ref().unwrap().berth_id;
    let mut obstruction = docking_berth_position(slot);
    obstruction[0] += 150.;
    ok(
        &mut w,
        &mut bob,
        Request::Navigate {
            mode: NavigationMode::FlyTo {
                position: obstruction,
            },
        },
    );
    advance(&mut w, 20000);
    assert!(flight(&mut w, &mut bob)
        .position
        .iter()
        .zip(obstruction)
        .all(|(a, b)| (a - b).abs() < 1.));
    ok(&mut w, &mut alice, Request::Dock);
    advance(&mut w, 26000);
    let held = flight(&mut w, &mut alice);
    let operation = held.docking.as_ref().unwrap();
    assert_eq!(operation.phase, DockingPhase::Held);
    assert_eq!(
        operation.hold_reason.as_deref(),
        Some("Docking path occupied")
    );
    assert_ne!(view(&mut w, &mut alice).location, Location::Docked);
    let other = flight(&mut w, &mut bob);
    assert!(
        held.position
            .iter()
            .zip(other.position)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt()
            >= 2. * DOCKING_HULL_RADIUS_M + 4.
    );
    assert!(
        !w.handle(
            &mut alice,
            Request::ResumeDocking {
                operation_id: operation.operation_id
            }
        )
        .ok
    );
    assert_eq!(flight(&mut w, &mut alice).position, held.position);
    // A direct diagonal toward Bob's gate initially moves toward Alice's hull.
    // Clear sideways first; FlyTo is not an obstacle-avoiding route planner.
    let mut clearance = other.position;
    clearance[2] = b.position[2];
    ok(
        &mut w,
        &mut bob,
        Request::Navigate {
            mode: NavigationMode::FlyTo {
                position: clearance,
            },
        },
    );
    advance(&mut w, 20000);
    assert!((flight(&mut w, &mut bob).position[2] - clearance[2]).abs() < 1.);
    ok(
        &mut w,
        &mut alice,
        Request::ResumeDocking {
            operation_id: operation.operation_id,
        },
    );
    advance(&mut w, 17000);
    let secured = flight(&mut w, &mut alice).docking.unwrap();
    assert!(secured.secured());
    assert_eq!(secured.operation_id, operation.operation_id);
}

#[test]
fn berth_capacity_rejection_does_not_create_a_ninth_owner() {
    let (mut w, mut first, _dir, _token) = setup("capacity");
    for i in 1..DOCKING_BERTH_COUNT {
        assert!(
            connect(
                &mut w,
                &mut None,
                &format!("Pilot{i}"),
                None,
                WORLD_LAYOUT_SCHEMA_VERSION
            )
            .ok
        );
    }
    let before = ok(&mut w, &mut first, Request::Snapshot).snapshot.unwrap();
    let mut ninth = None;
    assert!(
        !connect(
            &mut w,
            &mut ninth,
            "Overflow",
            None,
            WORLD_LAYOUT_SCHEMA_VERSION
        )
        .ok
    );
    assert!(ninth.is_none());
    let after = ok(&mut w, &mut first, Request::Snapshot).snapshot.unwrap();
    assert_eq!(after.pilots.len(), DOCKING_BERTH_COUNT as usize);
    assert_eq!(before.ledger_hash, after.ledger_hash);
    assert!(!after.pilots.iter().any(|p| p.name == "Overflow"));
}

#[test]
fn owned_resource_routine_waits_for_real_departure_and_capture_before_deposit() {
    let (mut w, mut session, _dir, _token) = setup("routine");
    ok(
        &mut w,
        &mut session,
        Request::StartResourceJob {
            target_units: 100,
            pilot: None,
        },
    );
    let mut saw_departure = false;
    let mut saw_arrival = false;
    // Five persistent miners now traverse the shared physical bay in both
    // directions. Observe the genuine routine without changing production
    // rates/timeouts or weakening departure, capture and deposit assertions.
    for _ in 0..24000 {
        advance(&mut w, 50);
        let response = ok(&mut w, &mut session, Request::Snapshot);
        let snapshot = response.snapshot.unwrap();
        let job = snapshot
            .resource_jobs
            .iter()
            .find(|j| j.pilot == "Pilot")
            .unwrap();
        assert_ne!(job.phase, ResourceJobPhase::Blocked, "{}", job.message);
        let pilot = snapshot.pilots.iter().find(|p| p.name == "Pilot").unwrap();
        let docking = pilot.flight.as_ref().unwrap().docking.as_ref().unwrap();
        if docking.phase == DockingPhase::Departing {
            saw_departure = true;
            assert_eq!(job.stored_units, 0);
        }
        if docking.phase == DockingPhase::Berthing {
            saw_arrival = true;
            assert_eq!(job.stored_units, 0);
        }
        if job.phase == ResourceJobPhase::Completed {
            assert!(saw_departure && saw_arrival);
            assert!(docking.secured());
            assert!(job.stored_units >= 100);
            let export = ok(&mut w, &mut session, Request::ExportLedger)
                .ledger
                .unwrap();
            arkline_verifier::verify(&export, None).unwrap();
            return;
        }
    }
    panic!("continuous docking routine did not finish within 1200 simulation seconds");
}

#[test]
fn captured_berth_keeps_real_seat_and_walk_authority_without_mid_approach_exit() {
    let (mut w, mut s, _dir, _token) = setup("walk-seat");
    let initial = view(&mut w, &mut s);
    let host = initial
        .flight
        .as_ref()
        .unwrap()
        .docking
        .as_ref()
        .unwrap()
        .host_vessel_id
        .clone();
    let binding = docked_traversal_binding(
        host,
        &default_pioneer_room_layout(),
        SceneLayoutRevision::DockingV1.definition().fleet_layout,
        false,
    );
    let leave = |binding: DockedTraversalBinding| Request::DockedTraversal {
        request: DockedTraversalRequest::LeaveSeat { binding },
    };
    let entered = ok(&mut w, &mut s, leave(binding.clone()));
    let actor = entered
        .snapshot
        .unwrap()
        .pilots
        .into_iter()
        .find(|p| p.name == "Pilot")
        .unwrap();
    assert_eq!(
        actor
            .station_avatar
            .unwrap()
            .docked_traversal
            .unwrap()
            .seat_transition
            .unwrap()
            .phase,
        DockedSeatPhase::Leaving
    );
    // Reversing an unfinished transition must not teleport back into the chair.
    assert!(
        !w.handle(
            &mut s,
            Request::DockedTraversal {
                request: DockedTraversalRequest::TakeSeat {
                    binding: binding.clone()
                }
            }
        )
        .ok
    );
    advance(&mut w, DOCKED_SEAT_TRANSITION_MS);
    let standing = view(&mut w, &mut s);
    let start = standing
        .station_avatar
        .as_ref()
        .unwrap()
        .docked_traversal
        .as_ref()
        .unwrap()
        .feet_m;
    assert_eq!(
        standing.flight.as_ref().unwrap().position,
        initial.flight.as_ref().unwrap().position
    );
    ok(
        &mut w,
        &mut s,
        Request::DockedTraversal {
            request: DockedTraversalRequest::Walk {
                binding: binding.clone(),
                forward: -1.,
                strafe: 0.,
                turn: 0.,
                sprint: false,
            },
        },
    );
    advance(&mut w, 100);
    let walked = view(&mut w, &mut s);
    let feet = walked
        .station_avatar
        .as_ref()
        .unwrap()
        .docked_traversal
        .as_ref()
        .unwrap()
        .feet_m;
    let distance = feet
        .iter()
        .zip(start)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt();
    assert!(
        distance > 0.01 && distance < 0.5,
        "real accepted walk distance {distance}"
    );
    ok(
        &mut w,
        &mut s,
        Request::DockedTraversal {
            request: DockedTraversalRequest::Walk {
                binding: binding.clone(),
                forward: 0.,
                strafe: 0.,
                turn: 0.,
                sprint: false,
            },
        },
    );
    ok(
        &mut w,
        &mut s,
        Request::DockedTraversal {
            request: DockedTraversalRequest::TakeSeat {
                binding: binding.clone(),
            },
        },
    );
    advance(&mut w, DOCKED_SEAT_TRANSITION_MS);
    assert!(view(&mut w, &mut s).station_avatar.is_none());
    ok(&mut w, &mut s, Request::Undock);
    advance(&mut w, 4000);
    assert!(
        !w.handle(&mut s, leave(binding.clone())).ok,
        "manual docking motion cannot grant routine-only walking authority"
    );
    assert!(view(&mut w, &mut s).station_avatar.is_none());
    advance(&mut w, 13000);
    ok(&mut w, &mut s, Request::Dock);
    advance(&mut w, 26000);
    assert!(flight(&mut w, &mut s).docking.unwrap().secured());
    ok(&mut w, &mut s, leave(binding));
    advance(&mut w, DOCKED_SEAT_TRANSITION_MS);
    let returned = view(&mut w, &mut s);
    assert_eq!(
        returned
            .station_avatar
            .unwrap()
            .docked_traversal
            .unwrap()
            .feet_m,
        start
    );
    assert_eq!(
        returned.flight.unwrap().position,
        initial.flight.unwrap().position
    );
}

#[test]
fn old_preview_capsule_inside_new_rail_is_rejected_without_relocating_saved_world() {
    let (mut w, mut s, dir, _token) = setup("old-rail-save");
    let host = flight(&mut w, &mut s).docking.unwrap().host_vessel_id;
    let binding = docked_traversal_binding(
        host,
        &default_pioneer_room_layout(),
        SceneLayoutRevision::DockingV1.definition().fleet_layout,
        false,
    );
    ok(
        &mut w,
        &mut s,
        Request::DockedTraversal {
            request: DockedTraversalRequest::LeaveSeat { binding },
        },
    );
    advance(&mut w, DOCKED_SEAT_TRANSITION_MS);
    drop(w);
    // Deliberately model the former preview's permitted edge in a disposable save.
    // This is compatibility input, not a claim that gameplay teleported there.
    let db = rusqlite::Connection::open(dir.join("world.sqlite")).unwrap();
    let json: String = db
        .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
        .unwrap();
    let mut world: serde_json::Value = serde_json::from_str(&json).unwrap();
    let state = &mut world["pilots"]["Pilot"]["view"]["station_avatar"]["docked_traversal"];
    assert!(state.is_object());
    state["feet_m"] = serde_json::json!([
        hangar_layout(ShipClass::Pioneer).half_length - STATION_AVATAR_RADIUS_M,
        0.,
        0.
    ]);
    state["support"] = serde_json::to_value(DockedSupport::Station).unwrap();
    state["surface_id"] = "station.floor".into();
    let fixture = serde_json::to_string(&world).unwrap();
    db.execute("UPDATE world SET json=?1 WHERE id=1", [&fixture])
        .unwrap();
    drop(db);
    let reopened = EditorWorld::open_with_creation_scene(&dir, SceneLayoutRevision::DockingV1);
    assert!(
        reopened.is_err(),
        "incompatible preview edge cannot silently migrate or trap a walking pilot"
    );
    let db = rusqlite::Connection::open(dir.join("world.sqlite")).unwrap();
    let after: String = db
        .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        after, fixture,
        "failed open must preserve source world bytes"
    );
}
