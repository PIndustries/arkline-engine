//! Authority acceptance for the negotiated docked Pioneer traversal path.
use super::*;
use arkline_protocol::pioneer_docked as geometry;

const CONNECTED_AT: u64 = 1_000;
const LEFT_AT: u64 = CONNECTED_AT + 1;
const STANDING_AT: u64 = LEFT_AT + DOCKED_SEAT_TRANSITION_MS;

fn connect_request(capable: bool, resume_token: Option<String>) -> Request {
    Request::Connect {
        protocol_version: PROTOCOL_VERSION,
        world_layout_version: WORLD_LAYOUT_SCHEMA_VERSION,
        pilot: "Walker".into(),
        resume_token,
        supports_room_controls: true,
        supports_fleet_hangar: false,
        supports_pioneer_interior: true,
        supports_docked_traversal_v2: capable,
    }
}
fn binding() -> DockedTraversalBinding {
    docked_traversal_binding(
        "v1:Walker:Pioneer".into(),
        &default_pioneer_room_layout(),
        FleetLayoutRevision::LinearV1,
        false,
    )
}
fn ok(response: Response) -> Response {
    assert!(response.ok, "{}", response.message);
    response
}
fn command(
    server: &mut Server,
    session: &mut Option<String>,
    request: DockedTraversalRequest,
    now: u64,
) -> Response {
    server.handle_at(session, Request::DockedTraversal { request }, now)
}
fn snapshot(server: &mut Server, session: &mut Option<String>, now: u64) -> Response {
    ok(server.handle_at(session, Request::Snapshot, now))
}
fn pilot(response: &Response) -> &PilotView {
    response
        .snapshot
        .as_ref()
        .unwrap()
        .pilots
        .iter()
        .find(|pilot| pilot.name == "Walker")
        .unwrap()
}
fn traversal(response: &Response) -> &DockedTraversalState {
    pilot(response)
        .station_avatar
        .as_ref()
        .unwrap()
        .docked_traversal
        .as_ref()
        .unwrap()
}
fn durable(server: &Server) -> String {
    server
        .db
        .query_row("SELECT json FROM world WHERE id=1", [], |row| row.get(0))
        .unwrap()
}
fn connected(server: &mut Server, session: &mut Option<String>) -> String {
    let response = ok(server.handle_at(session, connect_request(true, None), CONNECTED_AT));
    assert!(pilot(&response).station_avatar.is_none());
    assert_eq!(
        response
            .snapshot
            .as_ref()
            .unwrap()
            .docked_traversal_capability,
        Some(docked_traversal_capability())
    );
    response.resume_token.unwrap()
}
fn leave_and_stand(server: &mut Server, session: &mut Option<String>, at: u64) -> Response {
    ok(command(
        server,
        session,
        DockedTraversalRequest::LeaveSeat { binding: binding() },
        at,
    ));
    snapshot(server, session, at + DOCKED_SEAT_TRANSITION_MS)
}
fn assert_stopped_legacy_marker(response: &Response) {
    let avatar = pilot(response).station_avatar.as_ref().unwrap();
    assert!(!avatar.onboard_interior && !avatar.moving && !avatar.seated);
    assert_eq!(avatar.interior_deck, 1);
    assert!(avatar.jump_started_ms.is_none());
    assert_eq!(
        avatar.position,
        hangar_layout(ShipClass::Pioneer).boarding_point
    );
    assert!(avatar.heading.is_finite());
}

#[test]
fn authority_seat_transitions_use_registered_station_feet_and_preserve_legacy_projection() {
    let mut server = Server::open(":memory:").unwrap();
    let mut session = None;
    connected(&mut server, &mut session);
    let leaving = ok(command(
        &mut server,
        &mut session,
        DockedTraversalRequest::LeaveSeat { binding: binding() },
        LEFT_AT,
    ));
    let phase = traversal(&leaving).seat_transition.as_ref().unwrap();
    assert_eq!(phase.phase, DockedSeatPhase::Leaving);
    assert_eq!(
        phase.ends_at_ms - phase.started_at_ms,
        DOCKED_SEAT_TRANSITION_MS
    );
    assert_stopped_legacy_marker(&leaving);
    assert!(
        !command(
            &mut server,
            &mut session,
            DockedTraversalRequest::TakeSeat { binding: binding() },
            LEFT_AT + 1
        )
        .ok,
        "cannot reverse an in-flight seat transition"
    );
    let before_finish = snapshot(&mut server, &mut session, STANDING_AT - 1);
    assert!(traversal(&before_finish).seat_transition.is_some());
    let standing = snapshot(&mut server, &mut session, STANDING_AT);
    let state = traversal(&standing);
    assert!(state.seat_transition.is_none());
    assert_eq!(state.binding, binding());
    assert_eq!(
        state.feet_m,
        geometry::seat_anchors(PioneerDockedPose::private()).root_station_m
    );
    assert!(state.grounded && !state.moving);
    assert_eq!(state.vertical_velocity_mps, 0.0);
    assert!(!state.actuators.inner_hatch_open && !state.actuators.outer_hatch_open);
    assert!(!state.actuators.lift.moving);
    assert_eq!(state.actuators.lift.target, DockedLiftStop::Ship);
    assert_stopped_legacy_marker(&standing);
    let entering_at = STANDING_AT + 1;
    let entering = ok(command(
        &mut server,
        &mut session,
        DockedTraversalRequest::TakeSeat { binding: binding() },
        entering_at,
    ));
    assert_eq!(
        traversal(&entering).seat_transition.as_ref().unwrap().phase,
        DockedSeatPhase::Entering
    );
    assert!(pilot(&snapshot(
        &mut server,
        &mut session,
        entering_at + DOCKED_SEAT_TRANSITION_MS - 1
    ))
    .station_avatar
    .is_some());
    let seated = snapshot(
        &mut server,
        &mut session,
        entering_at + DOCKED_SEAT_TRANSITION_MS,
    );
    assert!(pilot(&seated).station_avatar.is_none());
    assert_eq!(pilot(&seated).location, Location::Docked);
}

#[test]
fn authority_take_seat_rejects_open_hatches_and_unstowed_lift() {
    let mut server = Server::open(":memory:").unwrap();
    let mut session = None;
    connected(&mut server, &mut session);
    leave_and_stand(&mut server, &mut session, LEFT_AT);
    let saved = durable(&server);
    // Clone a valid registered authority fixture to exercise each unsafe
    // equipment condition without teleporting a live pilot or altering its DB.
    for condition in 0..4 {
        let mut world: World = serde_json::from_str(&saved).unwrap();
        let actuator = world
            .accounts
            .get_mut("Walker")
            .unwrap()
            .docked_actuators
            .get_mut(&binding().host_vessel_id)
            .unwrap();
        match condition {
            0 => actuator.inner_hatch_open = true,
            1 => actuator.outer_hatch_open = true,
            2 => actuator.lift.moving = true,
            3 => {
                actuator.lift.floor_station_m =
                    geometry::lift_geometry(PioneerDockedPose::private()).station_floor_m;
                actuator.lift.target = DockedLiftStop::Station;
            }
            _ => unreachable!(),
        }
        assert!(
            docked_traversal::execute(
                &mut world,
                "Walker",
                DockedTraversalRequest::TakeSeat { binding: binding() },
                STANDING_AT,
                None,
            )
            .is_err(),
            "unsafe equipment condition {condition} allowed reseating"
        );
    }
    assert_eq!(durable(&server), saved);
}

#[test]
fn authority_legacy_bypasses_and_invalid_bound_commands_roll_back_exactly() {
    let mut server = Server::open(":memory:").unwrap();
    let mut session = None;
    connected(&mut server, &mut session);
    leave_and_stand(&mut server, &mut session, LEFT_AT);
    let before = durable(&server);
    for request in [
        Request::EnterShip,
        Request::EnterInterior,
        Request::InteriorLift { deck: 2 },
        Request::Undock,
    ] {
        let response = server.handle_at(&mut session, request, STANDING_AT);
        assert!(
            !response.ok,
            "legacy command bypassed canonical traversal: {}",
            response.message
        );
        assert_eq!(durable(&server), before);
    }
    let mut other_vessel = binding();
    other_vessel.host_vessel_id = "v1:SomeoneElse:Pioneer".into();
    let mut stale_layout = binding();
    stale_layout.layout_sha256 = "f".repeat(64);
    let mut stale_registration = binding();
    stale_registration.registration_revision += 1;
    for bound in [other_vessel, stale_layout, stale_registration] {
        let response = command(
            &mut server,
            &mut session,
            DockedTraversalRequest::Walk {
                binding: bound,
                forward: 1.0,
                strafe: 0.0,
                turn: 0.0,
            },
            STANDING_AT,
        );
        assert!(!response.ok, "mismatched authority binding was accepted");
        assert_eq!(durable(&server), before);
    }
    for (forward, strafe, turn) in [
        (f64::NAN, 0.0, 0.0),
        (0.0, f64::NAN, 0.0),
        (0.0, 0.0, f64::NAN),
    ] {
        let response = command(
            &mut server,
            &mut session,
            DockedTraversalRequest::Walk {
                binding: binding(),
                forward,
                strafe,
                turn,
            },
            STANDING_AT,
        );
        assert!(!response.ok, "nonfinite movement was accepted");
        assert_eq!(durable(&server), before);
    }
    assert_eq!(session.as_deref(), Some("Walker"));
}

#[test]
fn authority_walk_lease_expires_after_five_hundred_ms_without_hidden_drift() {
    let mut server = Server::open(":memory:").unwrap();
    let mut session = None;
    connected(&mut server, &mut session);
    leave_and_stand(&mut server, &mut session, LEFT_AT);
    let walking_at = STANDING_AT + 1;
    let walking = ok(command(
        &mut server,
        &mut session,
        DockedTraversalRequest::Walk {
            binding: binding(),
            forward: 0.0,
            strafe: 1.0,
            turn: 0.0,
        },
        walking_at,
    ));
    assert!(traversal(&walking).moving);
    assert_stopped_legacy_marker(&walking);
    let expired = snapshot(
        &mut server,
        &mut session,
        walking_at + STATION_WALK_LEASE_MS,
    );
    let stopped = traversal(&expired).clone();
    assert!(!stopped.moving && stopped.grounded);
    let later = snapshot(
        &mut server,
        &mut session,
        walking_at + STATION_WALK_LEASE_MS + 2_000,
    );
    assert_eq!(traversal(&later).feet_m, stopped.feet_m);
    assert!(!traversal(&later).moving);
    assert_stopped_legacy_marker(&later);
}

#[test]
fn authority_restart_retains_canonical_feet_but_clears_active_movement() {
    // Retained disposable files provide a real SQLite close/reopen boundary.
    let directory = std::env::temp_dir().join(format!(
        "arkline-docked-restart-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("world.sqlite");
    let mut server = Server::open(&database).unwrap();
    let mut session = None;
    let token = connected(&mut server, &mut session);
    leave_and_stand(&mut server, &mut session, LEFT_AT);
    let walking_at = STANDING_AT + 1;
    let walking = ok(command(
        &mut server,
        &mut session,
        DockedTraversalRequest::Walk {
            binding: binding(),
            forward: 0.0,
            strafe: 1.0,
            turn: 0.0,
        },
        walking_at,
    ));
    assert!(traversal(&walking).moving);
    let feet = traversal(&walking).feet_m;
    drop(server);
    let mut reopened = Server::open(&database).unwrap();
    let stopped = snapshot(&mut reopened, &mut None, walking_at + 1);
    assert_eq!(traversal(&stopped).feet_m, feet);
    assert!(!traversal(&stopped).moving);
    assert!(stopped.snapshot.as_ref().unwrap().online_pilots.is_empty());
    let mut resumed = None;
    let resumed_response = ok(reopened.handle_at(
        &mut resumed,
        connect_request(true, Some(token)),
        walking_at + 2,
    ));
    assert_eq!(traversal(&resumed_response).feet_m, feet);
    let later = snapshot(&mut reopened, &mut resumed, walking_at + 2_000);
    assert_eq!(traversal(&later).feet_m, feet);
    assert!(!traversal(&later).moving);
    assert_stopped_legacy_marker(&later);
}

#[test]
fn authority_legacy_capability_latches_only_after_authentication_until_last_disconnect() {
    let mut server = Server::open(":memory:").unwrap();
    let mut capable = None;
    let token = connected(&mut server, &mut capable);
    leave_and_stand(&mut server, &mut capable, LEFT_AT);
    let before_rejected_connect = durable(&server);
    let mut rejected = None;
    let rejected_response = server.handle_at(
        &mut rejected,
        connect_request(false, Some("not-the-pilot-credential".into())),
        STANDING_AT,
    );
    assert!(!rejected_response.ok && rejected.is_none());
    assert_eq!(durable(&server), before_rejected_connect);
    assert_eq!(server.online_sessions.get("Walker"), Some(&1));
    ok(command(
        &mut server,
        &mut capable,
        DockedTraversalRequest::TakeSeat { binding: binding() },
        STANDING_AT + 1,
    ));
    let seated_at = STANDING_AT + 1 + DOCKED_SEAT_TRANSITION_MS;
    assert!(pilot(&snapshot(&mut server, &mut capable, seated_at))
        .station_avatar
        .is_none());
    // A rejected old client must not poison the next valid capable activation.
    let active_at = seated_at + 1;
    leave_and_stand(&mut server, &mut capable, active_at);
    let recovered_at = active_at + DOCKED_SEAT_TRANSITION_MS;
    let mut legacy = None;
    let recovered = ok(server.handle_at(
        &mut legacy,
        connect_request(false, Some(token.clone())),
        recovered_at,
    ));
    assert!(pilot(&recovered)
        .station_avatar
        .as_ref()
        .is_none_or(|avatar| avatar.docked_traversal.is_none()));
    assert_eq!(server.online_sessions.get("Walker"), Some(&2));
    if pilot(&recovered).station_avatar.is_some() {
        ok(server.handle_at(&mut capable, Request::EnterShip, recovered_at + 1));
    }
    assert!(
        !command(
            &mut server,
            &mut capable,
            DockedTraversalRequest::LeaveSeat { binding: binding() },
            recovered_at + 2
        )
        .ok
    );
    server.disconnect(&mut legacy);
    assert_eq!(server.online_sessions.get("Walker"), Some(&1));
    assert!(
        !command(
            &mut server,
            &mut capable,
            DockedTraversalRequest::LeaveSeat { binding: binding() },
            recovered_at + 3
        )
        .ok,
        "the latch must survive while another session for the pilot remains"
    );
    server.disconnect(&mut capable);
    assert!(!server.online_sessions.contains_key("Walker"));
    let mut reconnected = None;
    let seated = ok(server.handle_at(
        &mut reconnected,
        connect_request(true, Some(token)),
        recovered_at + 4,
    ));
    if pilot(&seated).station_avatar.is_some() {
        ok(server.handle_at(&mut reconnected, Request::EnterShip, recovered_at + 5));
    }
    assert!(
        command(
            &mut server,
            &mut reconnected,
            DockedTraversalRequest::LeaveSeat { binding: binding() },
            recovered_at + 6
        )
        .ok,
        "final disconnect must release the incompatible-client latch"
    );
}

const WALK_TICK_MS: u64 = 25;
const WAYPOINT_TOLERANCE_M: f64 = 0.025;

fn xz_distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}
fn walk_tick_toward(
    server: &mut Server,
    session: &mut Option<String>,
    now: &mut u64,
    state: &DockedTraversalState,
    target: [f64; 3],
) -> Response {
    let dx = target[0] - state.feet_m[0];
    let dz = target[2] - state.feet_m[2];
    let distance = dx.hypot(dz);
    let stride = WALK_SPEED_MPS * WALK_TICK_MS as f64 / 1_000.0;
    let scale = distance.max(stride);
    let (sin, cos) = state.heading.sin_cos();
    ok(command(
        server,
        session,
        DockedTraversalRequest::Walk {
            binding: binding(),
            forward: (cos * dx + sin * dz) / scale,
            strafe: (-sin * dx + cos * dz) / scale,
            turn: 0.0,
        },
        *now,
    ));
    *now += WALK_TICK_MS;
    snapshot(server, session, *now)
}
fn stop_walking(server: &mut Server, session: &mut Option<String>, now: u64) -> Response {
    ok(command(
        server,
        session,
        DockedTraversalRequest::Walk {
            binding: binding(),
            forward: 0.0,
            strafe: 0.0,
            turn: 0.0,
        },
        now,
    ))
}
fn walk_to(
    server: &mut Server,
    session: &mut Option<String>,
    now: &mut u64,
    target: [f64; 3],
) -> Response {
    let mut response = snapshot(server, session, *now);
    let deadline = *now + 10_000;
    let wall_deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
    let mut stalled = 0;
    for _ in 0..400 {
        let state = traversal(&response).clone();
        let before = xz_distance(state.feet_m, target);
        if before <= WAYPOINT_TOLERANCE_M {
            assert!(
                (state.feet_m[1] - target[1]).abs() <= WAYPOINT_TOLERANCE_M,
                "waypoint height differs: {:?} -> {target:?}",
                state.feet_m
            );
            return stop_walking(server, session, *now);
        }
        assert!(
            *now < deadline && std::time::Instant::now() < wall_deadline,
            "waypoint exceeded bounded progress deadline: {:?} -> {target:?}",
            state.feet_m
        );
        response = walk_tick_toward(server, session, now, &state, target);
        let next = traversal(&response);
        assert!(next.grounded && next.binding == binding());
        assert!(
            xz_distance(next.feet_m, state.feet_m)
                <= WALK_SPEED_MPS * WALK_TICK_MS as f64 / 1_000.0 + 1.0e-7,
            "walking moved farther than one accepted tick"
        );
        if xz_distance(next.feet_m, target) >= before - 1.0e-7 {
            stalled += 1;
        } else {
            stalled = 0;
        }
        assert!(
            stalled < 12,
            "route stalled at {:?} ({:?}, {}) toward {target:?}",
            next.feet_m,
            next.support,
            next.surface_id
        );
    }
    panic!("waypoint exceeded 400 bounded ticks toward {target:?}");
}
fn probe_closed_hatch(
    server: &mut Server,
    session: &mut Option<String>,
    now: &mut u64,
    target: [f64; 3],
) -> Response {
    let mut response = snapshot(server, session, *now);
    for _ in 0..20 {
        let state = traversal(&response).clone();
        response = walk_tick_toward(server, session, now, &state, target);
    }
    let response = stop_walking(server, session, *now);
    assert!(
        xz_distance(traversal(&response).feet_m, target) > 0.8,
        "a closed hatch let the capsule cross to its far-side anchor"
    );
    response
}
fn set_hatch(
    server: &mut Server,
    session: &mut Option<String>,
    now: u64,
    hatch: DockedHatch,
    open: bool,
) -> Response {
    command(
        server,
        session,
        DockedTraversalRequest::SetHatch {
            binding: binding(),
            hatch,
            open,
        },
        now,
    )
}
fn midpoint(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|axis| (a[axis] + b[axis]) * 0.5)
}
fn ship_route(pose: PioneerDockedPose) -> [[f64; 3]; 4] {
    // Geometry-owner-proved route around the folded chair, then down the port aisle.
    [
        [34.36, -0.32, 3.0],
        [34.36, -0.32, 0.0],
        [34.1, -0.32, -0.5],
        [34.1, -0.32, -7.8],
    ]
    .map(|point| geometry::body_to_station(pose, point))
}
fn assert_power_failure_holds_lift_on_cloned_world(server: &Server, now: u64) {
    let original = durable(server);
    let mut world: World = serde_json::from_str(&original).unwrap();
    let state = world.pilots["Walker"]
        .view
        .station_avatar
        .as_ref()
        .unwrap()
        .docked_traversal
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!(state.support, DockedSupport::BoardingLift);
    assert!(state.actuators.lift.moving);
    let actuator = world
        .accounts
        .get_mut("Walker")
        .unwrap()
        .docked_actuators
        .get_mut(&binding().host_vessel_id)
        .unwrap();
    let floor = actuator.lift.floor_station_m;
    actuator.service_power_available = false;
    crate::docked_traversal::advance(&mut world, now + 1_000, None).unwrap();
    let held = world.pilots["Walker"]
        .view
        .station_avatar
        .as_ref()
        .unwrap()
        .docked_traversal
        .as_ref()
        .unwrap();
    assert_eq!(held.feet_m, state.feet_m);
    assert_eq!(held.actuators.lift.floor_station_m, floor);
    assert!(!held.actuators.lift.moving && !held.actuators.service_power_available);
    assert!(!held.moving);
    assert!(crate::docked_traversal::execute(
        &mut world,
        "Walker",
        DockedTraversalRequest::CallLift {
            binding: binding(),
            stop: DockedLiftStop::Station
        },
        now + 1_000,
        None
    )
    .is_err());
    assert_eq!(
        durable(server),
        original,
        "power fixture must not write the live DB"
    );
}
fn ride_lift_to(
    server: &mut Server,
    session: &mut Option<String>,
    now: &mut u64,
    target: f64,
) -> Response {
    let mut response = snapshot(server, session, *now);
    let initial = traversal(&response).feet_m;
    let deadline = *now + 12_000;
    for _ in 0..480 {
        let state = traversal(&response).clone();
        if !state.actuators.lift.moving {
            assert!((state.actuators.lift.floor_station_m - target).abs() < 1.0e-7);
            assert!((state.feet_m[1] - target).abs() < 1.0e-7);
            return response;
        }
        assert!(
            *now < deadline,
            "boarding lift exceeded its bounded stroke time"
        );
        *now += WALK_TICK_MS;
        response = snapshot(server, session, *now);
        let next = traversal(&response);
        let floor_delta =
            next.actuators.lift.floor_station_m - state.actuators.lift.floor_station_m;
        assert!(
            floor_delta.abs() <= WALK_TICK_MS as f64 / 1_000.0 + 1.0e-8,
            "powered lift exceeded 1 m/s"
        );
        assert!(
            (target - next.actuators.lift.floor_station_m).abs()
                <= (target - state.actuators.lift.floor_station_m).abs() + 1.0e-8
        );
        assert_eq!(next.support, DockedSupport::BoardingLift);
        assert!(next.grounded && !next.moving);
        assert!((next.feet_m[1] - next.actuators.lift.floor_station_m).abs() < 1.0e-7);
        assert_eq!([next.feet_m[0], next.feet_m[2]], [initial[0], initial[2]]);
        assert_stopped_legacy_marker(&response);
    }
    panic!("boarding lift exceeded 480 bounded ticks");
}

#[test]
fn authority_physical_hatches_lift_and_station_roundtrip_preserve_canonical_feet() {
    let mut server = Server::open(":memory:").unwrap();
    let mut session = None;
    connected(&mut server, &mut session);
    leave_and_stand(&mut server, &mut session, LEFT_AT);
    let mut now = STANDING_AT;
    let pose = PioneerDockedPose::private();
    let hatches = geometry::hatch_anchors(pose);
    let lift = geometry::lift_geometry(pose);
    for point in ship_route(pose) {
        walk_to(&mut server, &mut session, &mut now, point);
    }
    walk_to(
        &mut server,
        &mut session,
        &mut now,
        hatches.inner_inside_station_m,
    );
    probe_closed_hatch(
        &mut server,
        &mut session,
        &mut now,
        hatches.inner_outside_station_m,
    );
    ok(set_hatch(
        &mut server,
        &mut session,
        now,
        DockedHatch::Inner,
        true,
    ));
    assert!(
        !set_hatch(&mut server, &mut session, now, DockedHatch::Outer, true).ok,
        "both boarding hatches must not open together"
    );
    let inner_midpoint = midpoint(
        hatches.inner_inside_station_m,
        hatches.inner_outside_station_m,
    );
    let occupied = walk_to(&mut server, &mut session, &mut now, inner_midpoint);
    assert!(geometry::hatch_occupied(
        pose,
        geometry::PioneerDockedHatch::Inner,
        traversal(&occupied).feet_m
    ));
    assert!(
        !set_hatch(&mut server, &mut session, now, DockedHatch::Inner, false).ok,
        "a hatch must not close through an occupied capsule"
    );
    walk_to(
        &mut server,
        &mut session,
        &mut now,
        hatches.inner_outside_station_m,
    );
    walk_to(
        &mut server,
        &mut session,
        &mut now,
        hatches.outer_inside_station_m,
    );
    ok(set_hatch(
        &mut server,
        &mut session,
        now,
        DockedHatch::Inner,
        false,
    ));
    probe_closed_hatch(
        &mut server,
        &mut session,
        &mut now,
        hatches.outer_outside_station_m,
    );
    ok(set_hatch(
        &mut server,
        &mut session,
        now,
        DockedHatch::Outer,
        true,
    ));
    let occupied = walk_to(
        &mut server,
        &mut session,
        &mut now,
        midpoint(
            hatches.outer_inside_station_m,
            hatches.outer_outside_station_m,
        ),
    );
    assert!(geometry::hatch_occupied(
        pose,
        geometry::PioneerDockedHatch::Outer,
        traversal(&occupied).feet_m
    ));
    assert!(!set_hatch(&mut server, &mut session, now, DockedHatch::Outer, false).ok);
    walk_to(
        &mut server,
        &mut session,
        &mut now,
        hatches.outer_outside_station_m,
    );
    ok(set_hatch(
        &mut server,
        &mut session,
        now,
        DockedHatch::Outer,
        false,
    ));
    let ship_platform = [
        lift.center_station_m[0],
        lift.ship_floor_m,
        lift.center_station_m[2],
    ];
    let on_lift = walk_to(&mut server, &mut session, &mut now, ship_platform);
    assert_eq!(traversal(&on_lift).support, DockedSupport::BoardingLift);
    ok(command(
        &mut server,
        &mut session,
        DockedTraversalRequest::CallLift {
            binding: binding(),
            stop: DockedLiftStop::Station,
        },
        now,
    ));
    assert!(
        !command(
            &mut server,
            &mut session,
            DockedTraversalRequest::Jump { binding: binding() },
            now
        )
        .ok
    );
    assert_power_failure_holds_lift_on_cloned_world(&server, now);
    ride_lift_to(&mut server, &mut session, &mut now, lift.station_floor_m);
    let station = geometry::lift_call_anchors(pose).station_station_m;
    let outside = walk_to(&mut server, &mut session, &mut now, station);
    assert_eq!(traversal(&outside).support, DockedSupport::Station);
    assert_eq!(traversal(&outside).feet_m[1], 0.0);
    assert!(!server.handle_at(&mut session, Request::Undock, now).ok);
    walk_to(
        &mut server,
        &mut session,
        &mut now,
        [
            lift.center_station_m[0],
            lift.station_floor_m,
            lift.center_station_m[2],
        ],
    );
    ok(command(
        &mut server,
        &mut session,
        DockedTraversalRequest::CallLift {
            binding: binding(),
            stop: DockedLiftStop::Ship,
        },
        now,
    ));
    ride_lift_to(&mut server, &mut session, &mut now, lift.ship_floor_m);
    walk_to(
        &mut server,
        &mut session,
        &mut now,
        hatches.outer_outside_station_m,
    );
    ok(set_hatch(
        &mut server,
        &mut session,
        now,
        DockedHatch::Outer,
        true,
    ));
    walk_to(
        &mut server,
        &mut session,
        &mut now,
        hatches.outer_inside_station_m,
    );
    ok(set_hatch(
        &mut server,
        &mut session,
        now,
        DockedHatch::Outer,
        false,
    ));
    ok(set_hatch(
        &mut server,
        &mut session,
        now,
        DockedHatch::Inner,
        true,
    ));
    walk_to(
        &mut server,
        &mut session,
        &mut now,
        hatches.inner_outside_station_m,
    );
    walk_to(
        &mut server,
        &mut session,
        &mut now,
        hatches.inner_inside_station_m,
    );
    ok(set_hatch(
        &mut server,
        &mut session,
        now,
        DockedHatch::Inner,
        false,
    ));
    for point in ship_route(pose).into_iter().rev() {
        walk_to(&mut server, &mut session, &mut now, point);
    }
    let root = geometry::seat_anchors(pose).root_station_m;
    walk_to(&mut server, &mut session, &mut now, root);
    ok(command(
        &mut server,
        &mut session,
        DockedTraversalRequest::TakeSeat { binding: binding() },
        now,
    ));
    let seated = snapshot(&mut server, &mut session, now + DOCKED_SEAT_TRANSITION_MS);
    assert!(pilot(&seated).station_avatar.is_none());
}

#[test]
fn authority_reseat_approaches_continuously_before_removing_avatar() {
    let mut server = Server::open(":memory:").unwrap();
    let mut session = None;
    connected(&mut server, &mut session);
    leave_and_stand(&mut server, &mut session, LEFT_AT);
    let mut now = STANDING_AT;
    let pose = PioneerDockedPose::private();
    let root = geometry::seat_anchors(pose).root_station_m;
    let egress = geometry::seat_anchors(pose).egress_station_m;
    let standing = walk_to(&mut server, &mut session, &mut now, egress);
    let mut previous = traversal(&standing).feet_m;
    assert!((0.1..=1.0).contains(&flight::distance(previous, root)));
    ok(command(
        &mut server,
        &mut session,
        DockedTraversalRequest::TakeSeat { binding: binding() },
        now,
    ));
    let ends_at = now + DOCKED_SEAT_TRANSITION_MS;
    for tick in 1..=DOCKED_SEAT_TRANSITION_MS / WALK_TICK_MS {
        now += WALK_TICK_MS;
        let response = snapshot(&mut server, &mut session, now);
        if let Some(state) = pilot(&response)
            .station_avatar
            .as_ref()
            .and_then(|avatar| avatar.docked_traversal.as_ref())
        {
            assert!(
                flight::distance(previous, state.feet_m)
                    <= 2.5 * WALK_TICK_MS as f64 / 1_000.0 + 1.0e-8,
                "reseating snapped farther than the capped approach speed"
            );
            assert!(state
                .seat_transition
                .as_ref()
                .is_some_and(|phase| phase.phase == DockedSeatPhase::Entering));
            assert!(
                flight::distance(state.feet_m, root) <= flight::distance(previous, root) + 1.0e-8
            );
            previous = state.feet_m;
            assert!(
                now < ends_at,
                "seat transition did not complete after 900 ms"
            );
        } else {
            assert_eq!(tick, DOCKED_SEAT_TRANSITION_MS / WALK_TICK_MS);
            assert!(
                flight::distance(previous, root) <= 0.015,
                "avatar was removed before physically reaching the seat root"
            );
        }
    }
    assert!(pilot(&snapshot(&mut server, &mut session, now))
        .station_avatar
        .is_none());
}

#[test]
fn authority_editor_layout_install_blocks_active_traversal_then_rebinds_after_reseating() {
    use crate::editor::EditorWorld;
    let directory = std::env::temp_dir().join(format!(
        "arkline-docked-layout-install-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let mut world =
        EditorWorld::open_with_creation_scene(&directory, SceneLayoutRevision::LegacyV1).unwrap();
    let mut session = None;
    ok(world.handle(&mut session, connect_request(true, None)));
    let original = world.room_layout_state();
    assert_eq!(original.revision, 0);
    assert_eq!(original.document, default_pioneer_room_layout());
    ok(world.handle(
        &mut session,
        Request::DockedTraversal {
            request: DockedTraversalRequest::LeaveSeat { binding: binding() },
        },
    ));
    for _ in 0..DOCKED_SEAT_TRANSITION_MS / 100 {
        world.step(100).unwrap();
    }
    let before = ok(world.handle(&mut session, Request::Snapshot));
    let before_state = traversal(&before).clone();
    assert!(before_state.seat_transition.is_none());
    let fixed_clock = world.time();
    let before_public = serde_json::to_value((&before.snapshot, &before.assets)).unwrap();
    let layout_path = directory.join("room_layout.json");
    let before_saved = std::fs::read(&layout_path).ok();
    let original_hash = docked_layout_sha256(&original.document);
    assert_eq!(before_state.binding.layout_sha256, original_hash);
    let mut metadata_candidate = original.document.clone();
    metadata_candidate
        .rooms
        .iter_mut()
        .find(|room| room.source_room_id.as_deref() == Some("D1-P2"))
        .unwrap()
        .name
        .push_str(" layout install candidate");
    metadata_candidate.validate_live().unwrap();
    let mut geometry_candidate = metadata_candidate.clone();
    geometry_candidate
        .rooms
        .iter_mut()
        .find(|room| room.source_room_id.as_deref() == Some("D1-P2"))
        .unwrap()
        .clear_lwh_m[0] -= 0.2;
    geometry_candidate.validate_live().unwrap();
    assert_ne!(docked_layout_sha256(&metadata_candidate), original_hash);
    assert_ne!(docked_layout_sha256(&geometry_candidate), original_hash);
    for candidate in [&metadata_candidate, &geometry_candidate] {
        assert!(
            world
                .room_layout_replace(original.revision, candidate.clone())
                .is_err(),
            "active canonical traversal must block metadata and geometry installation"
        );
        assert_eq!(world.room_layout_state(), original);
        assert_eq!(std::fs::read(&layout_path).ok(), before_saved);
        assert_eq!(world.time(), fixed_clock);
        let unchanged = ok(world.handle(&mut session, Request::Snapshot));
        assert_eq!(
            serde_json::to_value((&unchanged.snapshot, &unchanged.assets)).unwrap(),
            before_public
        );
        assert_eq!(traversal(&unchanged).feet_m, before_state.feet_m);
        assert_eq!(traversal(&unchanged).binding.layout_sha256, original_hash);
    }
    ok(world.handle(
        &mut session,
        Request::DockedTraversal {
            request: DockedTraversalRequest::TakeSeat { binding: binding() },
        },
    ));
    for _ in 0..DOCKED_SEAT_TRANSITION_MS / 100 {
        world.step(100).unwrap();
    }
    assert!(pilot(&ok(world.handle(&mut session, Request::Snapshot)))
        .station_avatar
        .is_none());
    let installed = world
        .room_layout_replace(original.revision, geometry_candidate.clone())
        .unwrap();
    assert_eq!(installed.revision, original.revision + 1);
    assert_eq!(installed.document, geometry_candidate);
    assert_eq!(world.room_layout_state(), installed);
    let persisted: RoomLayoutState =
        serde_json::from_slice(&std::fs::read(&layout_path).unwrap()).unwrap();
    assert_eq!(persisted, installed);
    let next_binding = docked_traversal_binding(
        "v1:Walker:Pioneer".into(),
        &installed.document,
        FleetLayoutRevision::LinearV1,
        false,
    );
    assert_ne!(next_binding.layout_sha256, original_hash);
    assert!(
        !world
            .handle(
                &mut session,
                Request::DockedTraversal {
                    request: DockedTraversalRequest::LeaveSeat { binding: binding() },
                }
            )
            .ok,
        "the previous document binding must not activate the installed layout"
    );
    let next = ok(world.handle(
        &mut session,
        Request::DockedTraversal {
            request: DockedTraversalRequest::LeaveSeat {
                binding: next_binding.clone(),
            },
        },
    ));
    assert_eq!(traversal(&next).binding, next_binding);
    assert_eq!(
        traversal(&next).feet_m,
        geometry::seat_anchors(PioneerDockedPose::private()).root_station_m
    );
    assert_eq!(
        next.assets.as_ref().unwrap().pioneer_room_layout.as_ref(),
        Some(&installed)
    );
}

#[test]
fn authority_walks_from_pilot_seat_down_both_core_stairs_and_back_without_teleports() {
    fn walk_xz(
        server: &mut Server,
        session: &mut Option<String>,
        now: &mut u64,
        path: &[[f64; 2]],
        trace: &mut Vec<DockedTraversalState>,
    ) -> Response {
        let pose = docked_binding_pose(&binding()).unwrap();
        let document = server.editor_room_layout.as_ref().unwrap();
        let registered = geometry::registered_geometry(document).unwrap();
        let mut response = snapshot(server, session, *now);
        let actuator = &traversal(&response).actuators;
        let dynamic = geometry::dynamic_geometry(
            pose,
            PioneerDockedActuators {
                room_door_open: false,
                inner_hatch_open: actuator.inner_hatch_open,
                outer_hatch_open: actuator.outer_hatch_open,
                lift_floor_station_m: actuator.lift.floor_station_m,
                chair_fold: 1.0,
            },
        )
        .unwrap();
        let joints = geometry::pilot_joints(1.0);
        let eye = |state: &DockedTraversalState| -> [f64; 3] {
            std::array::from_fn(|axis| {
                state.feet_m[axis] + joints.eye_m[axis] - joints.root_m[axis]
            })
        };
        let stride = WALK_SPEED_MPS * WALK_TICK_MS as f64 / 1_000.0;
        for &[x, z] in path {
            let target = geometry::body_to_station(pose, [x, -0.32, z]);
            let wall_deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
            let mut stalled = 0;
            let mut reached = false;
            for _ in 0..400 {
                let before = traversal(&response).clone();
                let remaining = xz_distance(before.feet_m, target);
                if remaining <= WAYPOINT_TOLERANCE_M {
                    response = stop_walking(server, session, *now);
                    reached = true;
                    break;
                }
                assert!(std::time::Instant::now() < wall_deadline);
                response = walk_tick_toward(server, session, now, &before, target);
                let after = traversal(&response);
                assert!(after.grounded && after.seat_transition.is_none());
                assert_eq!(after.binding, binding());
                assert_eq!(after.heading, before.heading);
                assert_eq!(after.vertical_velocity_mps, 0.0);
                assert_eq!(after.updated_at_ms, *now);
                assert!(xz_distance(before.feet_m, after.feet_m) <= stride + 1.0e-7);
                assert!(
                    (after.feet_m[1] - before.feet_m[1]).abs() <= 0.24 + 1.0e-7,
                    "stair step remapped canonical height: {:?} -> {:?}",
                    before.feet_m,
                    after.feet_m
                );
                assert!(
                    flight::distance(eye(&before), eye(after)) <= stride.hypot(0.24) + 1.0e-7,
                    "standing head root jumped between registered supports"
                );
                let hit = geometry::support_at_station(
                    &registered,
                    &dynamic,
                    pose,
                    after.feet_m,
                    Some(after.support),
                    0.0,
                )
                .unwrap();
                assert_eq!(hit.support, after.support);
                assert_eq!(hit.surface_id, after.surface_id);
                assert!((hit.height_m - after.feet_m[1]).abs() < 1.0e-8);
                assert!(
                    geometry::capsule_clear_station(&registered, &dynamic, pose, after.feet_m),
                    "authority crossed a cabinet, stair rail, ceiling, or hull-side solid"
                );
                assert_stopped_legacy_marker(&response);
                if xz_distance(after.feet_m, target) >= remaining - 1.0e-7 {
                    stalled += 1;
                } else {
                    stalled = 0;
                }
                assert!(
                    stalled < 12,
                    "authority stair route stalled at {:?} ({:?}, {}) toward {target:?}",
                    after.feet_m,
                    after.support,
                    after.surface_id
                );
                trace.push(after.clone());
            }
            assert!(
                reached,
                "authority route exceeded 400 ticks toward {target:?}"
            );
        }
        response
    }

    let mut server = Server::open(":memory:").unwrap();
    let source = default_pioneer_room_layout();
    let source_bytes = serde_json::to_vec(&source).unwrap();
    let source_geometry = geometry::registered_geometry(&source).unwrap();
    let cabinet = source_geometry
        .panels
        .iter()
        .find(|panel| panel.id == "deck1/access-cabinet")
        .unwrap()
        .clone();
    assert!((0..3).all(|axis| {
        (cabinet.bounds.center_m[axis] - [-11.15, -3.47, 0.0][axis]).abs() < 1.0e-8
    }));
    assert!((cabinet.bounds.half_m[0] - 0.15).abs() < 1.0e-8);
    assert!((cabinet.bounds.half_m[1] - 0.65).abs() < 1.0e-8);
    assert!((cabinet.bounds.half_m[2] - 0.55).abs() < 1.0e-8);
    server.set_editor_room_layout(source);
    let mut session = None;
    connected(&mut server, &mut session);
    let standing = leave_and_stand(&mut server, &mut session, LEFT_AT);
    let mut trace = vec![traversal(&standing).clone()];
    let mut now = STANDING_AT;
    let pose = PioneerDockedPose::private();
    let deck2_y = pose.origin_m[1] + geometry::deck_floor_y(2).unwrap();
    let deck1_y = pose.origin_m[1] + geometry::deck_floor_y(1).unwrap();
    // Seat egress, the actual bridge opening, connector, and deck-two corridor.
    let approach = [
        [34.36, 3.0],
        [34.36, 0.0],
        [33.4, 0.0],
        [20.0, 0.0],
        [10.0, 0.0],
        [0.0, 0.0],
        [-7.2, 0.0],
    ];
    let upper = walk_xz(&mut server, &mut session, &mut now, &approach, &mut trace);
    assert_eq!(traversal(&upper).support, DockedSupport::Deck2);
    assert!((traversal(&upper).feet_m[1] - deck2_y).abs() < 1.0e-8);
    // Exact proven lower-to-upper geometry route, in deck-plan coordinates.
    // Reverse it for descent; the x=2 detour preserves the access cabinet.
    let ascent = [
        [2.0, 7.0],
        [2.0, 5.8],
        [0.9, 5.8],
        [0.9, 1.0],
        [3.1, 1.0],
        [3.1, 5.8],
        [2.0, 5.8],
        [2.0, 7.0],
        [4.8, 7.0],
    ];
    let descent: Vec<_> = ascent
        .into_iter()
        .rev()
        .skip(1)
        .chain(std::iter::once([4.8, 7.0]))
        .map(|[x, z]| [x - 12.0, z - 7.0])
        .collect();
    let descent_start = trace.len();
    let lower = walk_xz(&mut server, &mut session, &mut now, &descent, &mut trace);
    assert_eq!(traversal(&lower).support, DockedSupport::Deck1);
    assert!((traversal(&lower).feet_m[1] - deck1_y).abs() < 1.0e-8);
    assert!(trace[descent_start..]
        .windows(2)
        .all(|pair| pair[1].feet_m[1] <= pair[0].feet_m[1] + 1.0e-8));
    // A lost release on the real lower deck still expires at the 500ms lease.
    let lease_start = traversal(&lower).feet_m;
    ok(command(
        &mut server,
        &mut session,
        DockedTraversalRequest::Walk {
            binding: binding(),
            forward: 0.25,
            strafe: 0.0,
            turn: 0.0,
        },
        now,
    ));
    now += STATION_WALK_LEASE_MS;
    let expired = snapshot(&mut server, &mut session, now);
    let lease_end = traversal(&expired).feet_m;
    assert!(!traversal(&expired).moving);
    assert!((xz_distance(lease_start, lease_end) - 0.5).abs() < 1.0e-7);
    assert!((lease_end[1] - deck1_y).abs() < 1.0e-8);
    now += STATION_WALK_LEASE_MS + WALK_TICK_MS;
    let held = snapshot(&mut server, &mut session, now);
    assert_eq!(traversal(&held).feet_m, lease_end);
    assert!(!traversal(&held).moving);
    walk_xz(
        &mut server,
        &mut session,
        &mut now,
        &[[-7.2, 0.0]],
        &mut trace,
    );
    let ascent_start = trace.len();
    let ascent_body = ascent.map(|[x, z]| [x - 12.0, z - 7.0]);
    let upper = walk_xz(
        &mut server,
        &mut session,
        &mut now,
        &ascent_body,
        &mut trace,
    );
    assert_eq!(traversal(&upper).support, DockedSupport::Deck2);
    assert!((traversal(&upper).feet_m[1] - deck2_y).abs() < 1.0e-8);
    assert!(trace[ascent_start..]
        .windows(2)
        .all(|pair| pair[1].feet_m[1] >= pair[0].feet_m[1] - 1.0e-8));
    for section in [&trace[descent_start..ascent_start], &trace[ascent_start..]] {
        let surfaces: std::collections::BTreeSet<_> = section
            .iter()
            .map(|state| state.surface_id.as_str())
            .collect();
        assert!((1..=11).all(|step| surfaces.contains(format!("stairs/a/{step}").as_str())));
        assert!((1..=7).all(|step| surfaces.contains(format!("stairs/b/{step}").as_str())));
        assert!(surfaces.contains("stairs/midlanding"));
    }
    let return_path: Vec<_> = approach.into_iter().rev().skip(1).collect();
    walk_xz(
        &mut server,
        &mut session,
        &mut now,
        &return_path,
        &mut trace,
    );
    let returned = walk_xz(
        &mut server,
        &mut session,
        &mut now,
        &[[34.54, 3.0]],
        &mut trace,
    );
    assert!(
        flight::distance(
            traversal(&returned).feet_m,
            geometry::seat_anchors(pose).root_station_m
        ) <= WAYPOINT_TOLERANCE_M
    );
    ok(command(
        &mut server,
        &mut session,
        DockedTraversalRequest::TakeSeat { binding: binding() },
        now,
    ));
    let seated = snapshot(&mut server, &mut session, now + DOCKED_SEAT_TRANSITION_MS);
    assert!(pilot(&seated).station_avatar.is_none());
    assert_eq!(pilot(&seated).location, Location::Docked);
    assert_eq!(
        serde_json::to_vec(server.editor_room_layout.as_ref().unwrap()).unwrap(),
        source_bytes
    );
    assert_eq!(
        geometry::registered_geometry(server.editor_room_layout.as_ref().unwrap()).unwrap(),
        source_geometry
    );
}

#[test]
fn authority_reverses_lift_within_hatch_tolerance_without_stopping_short() {
    let mut server = Server::open(":memory:").unwrap();
    let mut session = None;
    connected(&mut server, &mut session);
    leave_and_stand(&mut server, &mut session, LEFT_AT);
    let mut now = STANDING_AT;
    let pose = PioneerDockedPose::private();
    let hatches = geometry::hatch_anchors(pose);
    let lift = geometry::lift_geometry(pose);
    for point in ship_route(pose) {
        walk_to(&mut server, &mut session, &mut now, point);
    }
    ok(set_hatch(
        &mut server,
        &mut session,
        now,
        DockedHatch::Inner,
        true,
    ));
    walk_to(
        &mut server,
        &mut session,
        &mut now,
        hatches.inner_outside_station_m,
    );
    walk_to(
        &mut server,
        &mut session,
        &mut now,
        hatches.outer_inside_station_m,
    );
    ok(set_hatch(
        &mut server,
        &mut session,
        now,
        DockedHatch::Inner,
        false,
    ));
    ok(set_hatch(
        &mut server,
        &mut session,
        now,
        DockedHatch::Outer,
        true,
    ));
    walk_to(
        &mut server,
        &mut session,
        &mut now,
        hatches.outer_outside_station_m,
    );
    ok(set_hatch(
        &mut server,
        &mut session,
        now,
        DockedHatch::Outer,
        false,
    ));
    let platform = [
        lift.center_station_m[0],
        lift.ship_floor_m,
        lift.center_station_m[2],
    ];
    let aboard_lift = walk_to(&mut server, &mut session, &mut now, platform);
    assert_eq!(traversal(&aboard_lift).support, DockedSupport::BoardingLift);
    assert_eq!(
        traversal(&aboard_lift).actuators.lift.floor_station_m,
        lift.ship_floor_m
    );
    ok(command(
        &mut server,
        &mut session,
        DockedTraversalRequest::CallLift {
            binding: binding(),
            stop: DockedLiftStop::Station,
        },
        now,
    ));
    now += 13;
    let lowered = snapshot(&mut server, &mut session, now);
    let lowered_state = traversal(&lowered);
    let lowered_floor = lowered_state.actuators.lift.floor_station_m;
    assert!((lift.ship_floor_m - lowered_floor - 0.013).abs() < 1.0e-8);
    assert_eq!(lowered_state.support, DockedSupport::BoardingLift);
    assert!(lowered_state.grounded && lowered_state.actuators.lift.moving);
    assert!((lowered_state.feet_m[1] - lowered_floor).abs() < 1.0e-8);
    let dynamic = |state: &DockedTraversalState| {
        geometry::dynamic_geometry(
            pose,
            PioneerDockedActuators {
                room_door_open: true,
                inner_hatch_open: state.actuators.inner_hatch_open,
                outer_hatch_open: state.actuators.outer_hatch_open,
                lift_floor_station_m: state.actuators.lift.floor_station_m,
                chair_fold: 1.0,
            },
        )
        .unwrap()
    };
    assert!(dynamic(lowered_state)
        .panels
        .iter()
        .any(|panel| panel.id == "boarding/lift-gate/ship"));
    let reversed = ok(command(
        &mut server,
        &mut session,
        DockedTraversalRequest::CallLift {
            binding: binding(),
            stop: DockedLiftStop::Ship,
        },
        now,
    ));
    let reversed_state = traversal(&reversed);
    assert_eq!(reversed_state.actuators.lift.target, DockedLiftStop::Ship);
    assert!(
        reversed_state.actuators.lift.moving,
        "13mm remaining must finish under powered integration"
    );
    assert_eq!(reversed_state.actuators.lift.floor_station_m, lowered_floor);
    assert_eq!(
        reversed_state.feet_m, lowered_state.feet_m,
        "a call must not snap the platform or rider to its target"
    );
    now += 5;
    let approaching = snapshot(&mut server, &mut session, now);
    let approaching_state = traversal(&approaching);
    assert!(approaching_state.actuators.lift.moving);
    assert!(
        (approaching_state.actuators.lift.floor_station_m - lowered_floor - 0.005).abs() < 1.0e-8
    );
    assert!((approaching_state.feet_m[1] - lowered_state.feet_m[1] - 0.005).abs() < 1.0e-8);
    assert_eq!(
        [approaching_state.feet_m[0], approaching_state.feet_m[2]],
        [lowered_state.feet_m[0], lowered_state.feet_m[2]]
    );
    now += 8;
    let docked = snapshot(&mut server, &mut session, now);
    let docked_state = traversal(&docked);
    assert!(!docked_state.actuators.lift.moving);
    assert_eq!(
        docked_state.actuators.lift.floor_station_m,
        lift.ship_floor_m
    );
    assert!((docked_state.feet_m[1] - lift.ship_floor_m).abs() < 1.0e-8);
    assert_eq!(docked_state.support, DockedSupport::BoardingLift);
    assert!(!dynamic(docked_state)
        .panels
        .iter()
        .any(|panel| panel.id == "boarding/lift-gate/ship"));
    // Cross the actual lowered gate and outer hatch into the ship, using only
    // accepted walking commands after the platform has physically docked.
    walk_to(
        &mut server,
        &mut session,
        &mut now,
        hatches.outer_outside_station_m,
    );
    ok(set_hatch(
        &mut server,
        &mut session,
        now,
        DockedHatch::Outer,
        true,
    ));
    let inside = walk_to(
        &mut server,
        &mut session,
        &mut now,
        hatches.outer_inside_station_m,
    );
    let inside_state = traversal(&inside);
    assert!(inside_state.grounded && !inside_state.moving);
    assert_ne!(inside_state.support, DockedSupport::BoardingLift);
    assert!((inside_state.feet_m[1] - lift.ship_floor_m).abs() < 1.0e-8);
    assert!(geometry::capsule_clear_station(
        &geometry::registered_geometry(&default_pioneer_room_layout()).unwrap(),
        &dynamic(inside_state),
        pose,
        inside_state.feet_m
    ));
    assert_stopped_legacy_marker(&inside);
    ok(set_hatch(
        &mut server,
        &mut session,
        now,
        DockedHatch::Outer,
        false,
    ));
}
