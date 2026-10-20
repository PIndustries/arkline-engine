//! Negotiated, owned-vessel docked traversal in one canonical station-meter frame.
use super::*;
use arkline_protocol::pioneer_docked as geometry;
use std::sync::{Arc, Mutex, OnceLock};

const CONTROL_RANGE_M: f64 = 3.0;
const SEAT_RANGE_M: f64 = 1.0;
const STEP_M: f64 = 0.24;
const LIFT_SPEED_MPS: f64 = 1.0;
const EPSILON_M: f64 = 0.015;

type CachedGeometry = Option<(String, Arc<PioneerDockedGeometry>)>;
fn registered(document: Option<&RoomLayoutDocument>) -> Result<Arc<PioneerDockedGeometry>> {
    let fallback = default_pioneer_room_layout();
    let document = document.unwrap_or(&fallback);
    let hash = docked_layout_sha256(document);
    static CACHE: OnceLock<Mutex<CachedGeometry>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| anyhow::anyhow!("docked geometry cache unavailable"))?;
    if let Some((current, geometry)) = &*cache {
        if current == &hash {
            return Ok(geometry.clone());
        }
    }
    let geometry = Arc::new(geometry::registered_geometry(document).map_err(anyhow::Error::msg)?);
    *cache = Some((hash, geometry.clone()));
    Ok(geometry)
}
fn binding(
    w: &World,
    name: &str,
    document: Option<&RoomLayoutDocument>,
) -> Result<DockedTraversalBinding> {
    let account = w.accounts.get(name).context("owned account missing")?;
    let host = account
        .vessel_id(ShipClass::Pioneer)
        .context("owned Pioneer missing")?;
    let fallback = default_pioneer_room_layout();
    Ok(docked_traversal_binding(
        host.into(),
        document.unwrap_or(&fallback),
        w.scene_layout.definition().fleet_layout,
        account.fleet_hangar,
    ))
}
fn initial_actuators(binding: DockedTraversalBinding, now: u64) -> Result<DockedVesselActuators> {
    let pose = docked_binding_pose(&binding).map_err(anyhow::Error::msg)?;
    let lift = geometry::lift_geometry(pose);
    Ok(DockedVesselActuators {
        binding,
        inner_hatch_open: false,
        outer_hatch_open: false,
        lift: DockedLiftState {
            floor_station_m: lift.ship_floor_m,
            target: DockedLiftStop::Ship,
            moving: false,
            updated_at_ms: now,
        },
        service_power_available: true,
    })
}
fn projection(state: DockedTraversalState, now: u64) -> StationAvatar {
    let point = if state.binding.fleet_hangar {
        fleet_berth_pose(state.binding.fleet_layout, ShipClass::Pioneer).boarding_point
    } else {
        hangar_layout(ShipClass::Pioneer).boarding_point
    };
    StationAvatar {
        docked_traversal: Some(state),
        onboard_interior: false,
        interior_deck: 1,
        jump_started_ms: None,
        seated: false,
        position: point,
        heading: 0.0,
        moving: false,
        updated_at_ms: now,
    }
}
pub(super) fn stop(pilot: &mut Pilot) {
    pilot.walk_input = None;
    if let Some(state) = pilot
        .view
        .station_avatar
        .as_mut()
        .and_then(|avatar| avatar.docked_traversal.as_mut())
    {
        state.moving = false;
        state.vertical_velocity_mps = 0.0;
        // A disconnected pilot remains at exactly the same feet position. Gravity
        // can settle an airborne capsule on a later authoritative tick.
        state.seat_transition = None;
    }
}
pub(super) fn stop_lifts(w: &mut World, now: u64) {
    for account in w.accounts.values_mut() {
        for actuator in account.docked_actuators.values_mut() {
            actuator.lift.moving = false;
            actuator.lift.updated_at_ms = now;
        }
    }
    refresh_actuator_views(w);
}
fn refresh_actuator_views(w: &mut World) {
    for (name, pilot) in &mut w.pilots {
        if let Some(state) = pilot
            .view
            .station_avatar
            .as_mut()
            .and_then(|avatar| avatar.docked_traversal.as_mut())
        {
            if let Some(actuator) = w
                .accounts
                .get(name)
                .and_then(|account| account.docked_actuators.get(&state.binding.host_vessel_id))
            {
                state.actuators = actuator.clone();
            }
        }
    }
}
pub(super) fn legacy_recovery(w: &mut World, name: &str, now: u64) {
    let Some(pilot) = w.pilots.get_mut(name) else {
        return;
    };
    let Some(mut state) = pilot
        .view
        .station_avatar
        .as_ref()
        .and_then(|avatar| avatar.docked_traversal.clone())
    else {
        return;
    };
    state.moving = false;
    pilot.view.station_avatar = Some(projection(state, now));
    pilot.view.station_avatar.as_mut().unwrap().docked_traversal = None;
    pilot.walk_input = None;
    if let Some(account) = w.accounts.get_mut(name) {
        for actuator in account.docked_actuators.values_mut() {
            // Explicit authenticated compatibility recovery neutralizes boarding
            // equipment for a client that cannot represent its physical path.
            if let Ok(pose) = docked_binding_pose(&actuator.binding) {
                actuator.inner_hatch_open = false;
                actuator.outer_hatch_open = false;
                actuator.lift.floor_station_m = geometry::lift_geometry(pose).ship_floor_m;
                actuator.lift.target = DockedLiftStop::Ship;
                actuator.lift.moving = false;
                actuator.lift.updated_at_ms = now;
            }
        }
    }
}
fn dynamic(
    actuator: &DockedVesselActuators,
    room_open: bool,
    state: &DockedTraversalState,
    now: u64,
) -> geometry::PioneerDockedActuators {
    geometry::PioneerDockedActuators {
        room_door_open: room_open,
        inner_hatch_open: actuator.inner_hatch_open,
        outer_hatch_open: actuator.outer_hatch_open,
        lift_floor_station_m: actuator.lift.floor_station_m,
        chair_fold: state
            .seat_transition
            .as_ref()
            .map_or(1.0, |phase| phase.chair_fold(now)),
    }
}
fn near(a: [f64; 3], b: [f64; 3], range: f64) -> bool {
    flight::distance(a, b) <= range
}
fn room_open(w: &World, name: &str) -> bool {
    w.accounts[name]
        .pioneer_room
        .as_ref()
        .is_none_or(|room| room.door_open)
}
fn station_clear(binding: &DockedTraversalBinding, feet: [f64; 3]) -> bool {
    if !feet.iter().all(|v| v.is_finite()) || feet[1] < -EPSILON_M {
        return false;
    }
    let layout = if binding.fleet_hangar {
        fleet_hangar_layout_for(binding.fleet_layout)
    } else {
        hangar_layout(ShipClass::Pioneer)
    };
    if feet[0].abs() > layout.half_length - STATION_AVATAR_RADIUS_M
        || feet[2].abs() > layout.half_width - STATION_AVATAR_RADIUS_M
    {
        return false;
    }
    if binding.fleet_hangar {
        if binding.fleet_layout == FleetLayoutRevision::LinearV1 {
            if feet[2] > 3.0 || !fleet_hull_clear(binding.fleet_layout, feet) {
                return false;
            }
        } else if !fleet_walkable(binding.fleet_layout, feet) {
            return false;
        }
    } else {
        let spec = ShipClass::Pioneer.spec();
        if feet[0].abs() < spec.length_m / 2.0 + STATION_AVATAR_RADIUS_M
            && feet[2].abs() < spec.width_m / 2.0 + STATION_AVATAR_RADIUS_M
        {
            return false;
        }
    }
    let obstacles = if binding.fleet_hangar {
        fleet_station_obstacles()
    } else {
        station_obstacles(ShipClass::Pioneer)
    };
    !obstacles.iter().any(|obstacle| {
        let r = STATION_AVATAR_RADIUS_M;
        feet[0] > obstacle.min[0] - r
            && feet[0] < obstacle.max[0] + r
            && feet[2] > obstacle.min[2] - r
            && feet[2] < obstacle.max[2] + r
            && feet[1] + AVATAR_HEIGHT_M > obstacle.min[1]
            && feet[1] < obstacle.max[1]
    })
}
fn support(
    static_geometry: &PioneerDockedGeometry,
    dynamic_geometry: &PioneerDockedGeometry,
    pose: PioneerDockedPose,
    state: &DockedTraversalState,
    feet: [f64; 3],
    max_step: f64,
) -> Option<geometry::PioneerDockedSupportHit> {
    if let Some(hit) = geometry::support_at_station(
        static_geometry,
        dynamic_geometry,
        pose,
        feet,
        Some(state.support),
        max_step,
    ) {
        return Some(hit);
    }
    if feet[1] >= -max_step && feet[1] <= max_step && station_clear(&state.binding, feet) {
        return Some(geometry::PioneerDockedSupportHit {
            support: DockedSupport::Station,
            surface_id: "station.floor".into(),
            height_m: 0.0,
        });
    }
    None
}
fn clear(
    static_geometry: &PioneerDockedGeometry,
    dynamic_geometry: &PioneerDockedGeometry,
    pose: PioneerDockedPose,
    state: &DockedTraversalState,
    feet: [f64; 3],
) -> bool {
    // Only explicit registered supports/approaches override the legacy broad
    // hull exclusion. Other ships and the rest of station walking retain it.
    let registered = geometry::support_at_station(
        static_geometry,
        dynamic_geometry,
        pose,
        feet,
        Some(state.support),
        1.5,
    )
    .is_some_and(|hit| {
        hit.support != DockedSupport::Station
            || geometry::is_registered_station_approach(pose, feet)
    });
    if registered {
        geometry::capsule_clear_station(static_geometry, dynamic_geometry, pose, feet)
    } else {
        station_clear(&state.binding, feet)
    }
}
pub(super) fn can_undock(w: &World, name: &str) -> Result<()> {
    if let Some(account) = w.accounts.get(name) {
        if let Some(host) = account.vessel_id(ShipClass::Pioneer) {
            if let Some(actuator) = account.docked_actuators.get(host) {
                let pose = docked_binding_pose(&actuator.binding).map_err(anyhow::Error::msg)?;
                ensure!(
                    !actuator.inner_hatch_open
                        && !actuator.outer_hatch_open
                        && !actuator.lift.moving
                        && (actuator.lift.floor_station_m
                            - geometry::lift_geometry(pose).ship_floor_m)
                            .abs()
                            <= EPSILON_M,
                    "close both boarding hatches and stow the boarding lift before piloting"
                );
            }
        }
    }
    Ok(())
}
pub(super) fn execute(
    w: &mut World,
    name: &str,
    request: DockedTraversalRequest,
    now: u64,
    document: Option<&RoomLayoutDocument>,
) -> Result<()> {
    let pilot = w.pilots.get(name).context("pilot missing")?;
    ensure!(
        pilot.view.location == Location::Docked && pilot.view.ship_class == ShipClass::Pioneer,
        "pilot-seat exit requires your docked Pioneer"
    );
    ensure!(
        w.accounts[name].docked_traversal_compatible,
        "all sessions must support docked traversal v2"
    );
    ensure!(
        !w.remote_controls.contains_key(name)
            && !w.remote_controls.values().any(|worker| worker == name),
        "end remote control before leaving the pilot seat"
    );
    ensure!(
        !w.resource_jobs
            .get(name)
            .is_some_and(|job| !job.view.phase.is_terminal()),
        "stop automated vessel work before docked traversal"
    );
    let expected = binding(w, name, document)?;
    ensure!(
        request.binding() == &expected,
        "docked traversal vessel, registration, layout or berth binding changed"
    );
    let pose = docked_binding_pose(&expected).map_err(anyhow::Error::msg)?;
    let static_geometry = registered(document)?;
    let existing = pilot
        .view
        .station_avatar
        .as_ref()
        .and_then(|avatar| avatar.docked_traversal.clone());
    let mut actuator = w.accounts[name]
        .docked_actuators
        .get(&expected.host_vessel_id)
        .cloned()
        .unwrap_or(initial_actuators(expected.clone(), now)?);
    if actuator.binding != expected && existing.is_none() {
        let old_pose = docked_binding_pose(&actuator.binding).map_err(anyhow::Error::msg)?;
        ensure!(
            !actuator.inner_hatch_open
                && !actuator.outer_hatch_open
                && !actuator.lift.moving
                && (actuator.lift.floor_station_m - geometry::lift_geometry(old_pose).ship_floor_m)
                    .abs()
                    <= EPSILON_M,
            "close and stow boarding equipment before changing its registration"
        );
        let service_power_available = actuator.service_power_available;
        actuator = initial_actuators(expected.clone(), now)?;
        actuator.service_power_available = service_power_available;
    }
    ensure!(
        actuator.binding == expected,
        "boarding equipment belongs to an older registration; reconnect safely"
    );
    if matches!(request, DockedTraversalRequest::LeaveSeat { .. }) {
        ensure!(
            pilot.view.station_avatar.is_none(),
            "return aboard before leaving the pilot seat"
        );
        ensure!(
            pilot.view.flight.as_ref().is_none_or(|flight| matches!(
                flight.mode,
                NavigationMode::Stop | NavigationMode::Manual
            ) && flight
                .velocity
                .iter()
                .all(|v| v.abs() < 0.001)),
            "stop vessel automation before leaving the pilot seat"
        );
        let anchor = geometry::seat_anchors(pose);
        let state = DockedTraversalState {
            version: DOCKED_TRAVERSAL_VERSION,
            binding: expected.clone(),
            feet_m: anchor.root_station_m,
            heading: anchor.heading,
            support: DockedSupport::Cockpit,
            surface_id: "cockpit.floor".into(),
            moving: false,
            grounded: true,
            vertical_velocity_mps: 0.0,
            updated_at_ms: now,
            seat_transition: Some(DockedSeatTransition {
                phase: DockedSeatPhase::Leaving,
                started_at_ms: now,
                ends_at_ms: now.saturating_add(DOCKED_SEAT_TRANSITION_MS),
            }),
            actuators: actuator.clone(),
        };
        let pilot = w.pilots.get_mut(name).unwrap();
        pilot.controls = None;
        pilot.walk_input = None;
        pilot.view.station_avatar = Some(projection(state, now));
    } else {
        let mut state =
            existing.context("leave the pilot seat before docked traversal commands")?;
        ensure!(
            state.binding == expected,
            "active traversal binding changed"
        );
        let in_transition = state.seat_transition.is_some();
        if let DockedTraversalRequest::Walk {
            forward,
            strafe,
            turn,
            ..
        } = &request
        {
            ensure!(
                [*forward, *strafe, *turn]
                    .iter()
                    .all(|value| value.is_finite() && value.abs() <= 1.0),
                "walk inputs must be finite within -1 through 1"
            );
            let moving = *forward != 0.0 || *strafe != 0.0 || *turn != 0.0;
            ensure!(
                !in_transition || !moving,
                "finish the pilot-seat transition before walking"
            );
            state.moving = moving;
            let pilot = w.pilots.get_mut(name).unwrap();
            pilot.walk_input = moving.then_some(station::WalkInput {
                forward: *forward,
                strafe: *strafe,
                turn: *turn,
                accepted_ms: now,
            });
        } else {
            ensure!(!in_transition, "finish the pilot-seat transition first");
            ensure!(state.grounded, "land before operating docked equipment");
            w.pilots.get_mut(name).unwrap().walk_input = None;
            state.moving = false;
            match request {
                DockedTraversalRequest::TakeSeat { .. } => {
                    ensure!(
                        near(
                            state.feet_m,
                            geometry::seat_anchors(pose).root_station_m,
                            SEAT_RANGE_M
                        ),
                        "stand at your pilot seat before reseating"
                    );
                    can_undock(w, name)?;
                    state.seat_transition = Some(DockedSeatTransition {
                        phase: DockedSeatPhase::Entering,
                        started_at_ms: now,
                        ends_at_ms: now.saturating_add(DOCKED_SEAT_TRANSITION_MS),
                    });
                }
                DockedTraversalRequest::Jump { .. } => {
                    ensure!(
                        state.support != DockedSupport::BoardingLift || !actuator.lift.moving,
                        "do not jump from a moving boarding lift"
                    );
                    state.grounded = false;
                    state.vertical_velocity_mps = STATION_JUMP_SPEED_MPS;
                }
                DockedTraversalRequest::SetHatch { hatch, open, .. } => {
                    let anchors = geometry::hatch_anchors(pose);
                    let (inside, outside, other_open, geometry_hatch) = match hatch {
                        DockedHatch::Inner => (
                            anchors.inner_inside_station_m,
                            anchors.inner_outside_station_m,
                            actuator.outer_hatch_open,
                            geometry::PioneerDockedHatch::Inner,
                        ),
                        DockedHatch::Outer => (
                            anchors.outer_inside_station_m,
                            anchors.outer_outside_station_m,
                            actuator.inner_hatch_open,
                            geometry::PioneerDockedHatch::Outer,
                        ),
                    };
                    ensure!(
                        near(state.feet_m, inside, CONTROL_RANGE_M)
                            || near(state.feet_m, outside, CONTROL_RANGE_M),
                        "walk to this hatch control first"
                    );
                    if open {
                        ensure!(!other_open, "close the other boarding hatch first");
                        if hatch == DockedHatch::Outer {
                            ensure!(!actuator.lift.moving && (actuator.lift.floor_station_m - geometry::lift_geometry(pose).ship_floor_m).abs() <= EPSILON_M, "call and stop the boarding lift at ship level before opening the outer hatch");
                        }
                    } else {
                        let occupied = w
                            .pilots
                            .values()
                            .filter_map(|pilot| {
                                pilot
                                    .view
                                    .station_avatar
                                    .as_ref()?
                                    .docked_traversal
                                    .as_ref()
                            })
                            .filter(|occupant| {
                                occupant.binding.host_vessel_id == expected.host_vessel_id
                            })
                            .any(|occupant| {
                                geometry::hatch_occupied(pose, geometry_hatch, occupant.feet_m)
                            });
                        ensure!(!occupied, "boarding hatch doorway is occupied");
                    }
                    match hatch {
                        DockedHatch::Inner => actuator.inner_hatch_open = open,
                        DockedHatch::Outer => actuator.outer_hatch_open = open,
                    }
                }
                DockedTraversalRequest::CallLift { stop, .. } => {
                    let anchors = geometry::lift_call_anchors(pose);
                    ensure!(
                        state.support == DockedSupport::BoardingLift
                            || near(state.feet_m, anchors.station_station_m, CONTROL_RANGE_M)
                            || near(state.feet_m, anchors.ship_station_m, CONTROL_RANGE_M),
                        "walk to a boarding-lift control first"
                    );
                    ensure!(
                        !actuator.inner_hatch_open && !actuator.outer_hatch_open,
                        "close both boarding hatches before moving the lift"
                    );
                    ensure!(
                        actuator.service_power_available,
                        "station docking-service power unavailable; lift held safely"
                    );
                    actuator.lift.target = stop;
                    actuator.lift.updated_at_ms = now;
                    let lift = geometry::lift_geometry(pose);
                    let target = match stop {
                        DockedLiftStop::Ship => lift.ship_floor_m,
                        DockedLiftStop::Station => lift.station_floor_m,
                    };
                    actuator.lift.moving =
                        (actuator.lift.floor_station_m - target).abs() > EPSILON_M;
                }
                DockedTraversalRequest::LeaveSeat { .. } | DockedTraversalRequest::Walk { .. } => {
                    unreachable!()
                }
            }
        }
        state.updated_at_ms = now;
        state.actuators = actuator.clone();
        w.pilots.get_mut(name).unwrap().view.station_avatar = Some(projection(state, now));
    }
    w.accounts
        .get_mut(name)
        .unwrap()
        .docked_actuators
        .insert(expected.host_vessel_id.clone(), actuator);
    let _ = static_geometry;
    Ok(())
}

fn advance_actor(
    state: &mut DockedTraversalState,
    actuator: &mut DockedVesselActuators,
    input: Option<&station::WalkInput>,
    static_geometry: &PioneerDockedGeometry,
    room_open: bool,
    now: u64,
) -> Result<bool> {
    let pose = docked_binding_pose(&state.binding).map_err(anyhow::Error::msg)?;
    let lift_geometry = geometry::lift_geometry(pose);
    let mut cursor = state.updated_at_ms.min(now);
    // A complete current lift stroke and jump fit within this bound. Long
    // offline intervals never create unbounded integration work or extra input.
    let until = now.min(cursor.saturating_add(20_000));
    while cursor < until {
        let input_boundary = input
            .map(|input| input.accepted_ms.saturating_add(STATION_WALK_LEASE_MS))
            .filter(|expires| *expires > cursor)
            .unwrap_or(until);
        let next = cursor.saturating_add(25).min(until).min(input_boundary);
        let dt = (next - cursor) as f64 / 1000.0;
        let old_floor = actuator.lift.floor_station_m;
        if actuator.lift.moving {
            if !actuator.service_power_available
                || actuator.inner_hatch_open
                || actuator.outer_hatch_open
            {
                actuator.lift.moving = false;
            } else {
                let target = match actuator.lift.target {
                    DockedLiftStop::Ship => lift_geometry.ship_floor_m,
                    DockedLiftStop::Station => lift_geometry.station_floor_m,
                };
                let delta = (target - old_floor).clamp(-LIFT_SPEED_MPS * dt, LIFT_SPEED_MPS * dt);
                actuator.lift.floor_station_m += delta;
                if (target - actuator.lift.floor_station_m).abs() <= 1.0e-8 {
                    actuator.lift.floor_station_m = target;
                    actuator.lift.moving = false;
                }
            }
        }
        actuator.lift.updated_at_ms = next;
        if state.grounded && state.support == DockedSupport::BoardingLift {
            state.feet_m[1] += actuator.lift.floor_station_m - old_floor;
        }
        let dynamic_geometry =
            geometry::dynamic_geometry(pose, dynamic(actuator, room_open, state, next))
                .map_err(anyhow::Error::msg)?;
        if let Some(transition) = state.seat_transition.clone() {
            if transition.phase == DockedSeatPhase::Entering {
                let target = geometry::seat_anchors(pose).root_station_m;
                let arrive = transition
                    .started_at_ms
                    .saturating_add(DOCKED_SEAT_TRANSITION_MS / 2);
                let fraction = (next.saturating_sub(cursor) as f64
                    / arrive.saturating_sub(cursor).max(1) as f64)
                    .clamp(0.0, 1.0);
                let candidate = std::array::from_fn(|index| {
                    state.feet_m[index] + (target[index] - state.feet_m[index]) * fraction
                });
                if near(candidate, state.feet_m, 2.5 * dt + 1.0e-8)
                    && (cursor >= arrive
                        || geometry::capsule_clear_station(
                            static_geometry,
                            &dynamic_geometry,
                            pose,
                            candidate,
                        ))
                {
                    state.feet_m = candidate;
                } else {
                    // A blocked approach cancels reseating in place. It never
                    // completes by snapping the avatar to a distant chair.
                    state.seat_transition = None;
                    state.updated_at_ms = now;
                    state.actuators = actuator.clone();
                    return Ok(false);
                }
            }
            state.moving = false;
            if next >= transition.ends_at_ms {
                if transition.phase == DockedSeatPhase::Entering {
                    if !near(
                        state.feet_m,
                        geometry::seat_anchors(pose).root_station_m,
                        EPSILON_M,
                    ) {
                        state.seat_transition = None;
                        state.updated_at_ms = now;
                        state.actuators = actuator.clone();
                        return Ok(false);
                    }
                    state.updated_at_ms = now;
                    return Ok(true);
                }
                state.seat_transition = None;
            }
        } else {
            let active = input
                .filter(|input| cursor < input.accepted_ms.saturating_add(STATION_WALK_LEASE_MS));
            let (forward, strafe, turn) = active.map_or((0.0, 0.0, 0.0), |input| {
                (input.forward, input.strafe, input.turn)
            });
            state.heading = (state.heading + turn * 2.0 * dt).rem_euclid(std::f64::consts::TAU);
            let (sin, cos) = state.heading.sin_cos();
            let norm = forward.hypot(strafe).max(1.0);
            let delta = [
                (cos * forward - sin * strafe) / norm * WALK_SPEED_MPS * dt,
                (sin * forward + cos * strafe) / norm * WALK_SPEED_MPS * dt,
            ];
            for (axis, distance) in [(0, delta[0]), (2, delta[1])] {
                if distance == 0.0 {
                    continue;
                }
                let mut candidate = state.feet_m;
                candidate[axis] += distance;
                if state.grounded {
                    if let Some(hit) = support(
                        static_geometry,
                        &dynamic_geometry,
                        pose,
                        state,
                        candidate,
                        STEP_M,
                    ) {
                        candidate[1] = hit.height_m;
                        if clear(static_geometry, &dynamic_geometry, pose, state, candidate) {
                            state.feet_m = candidate;
                            state.support = hit.support;
                            state.surface_id = hit.surface_id;
                        }
                    }
                } else if clear(static_geometry, &dynamic_geometry, pose, state, candidate) {
                    state.feet_m = candidate;
                }
            }
            if state.grounded {
                if let Some(hit) = support(
                    static_geometry,
                    &dynamic_geometry,
                    pose,
                    state,
                    state.feet_m,
                    STEP_M,
                ) {
                    state.feet_m[1] = hit.height_m;
                    state.support = hit.support;
                    state.surface_id = hit.surface_id;
                } else {
                    state.grounded = false;
                    state.vertical_velocity_mps = 0.0;
                }
            }
            if !state.grounded {
                let previous_y = state.feet_m[1];
                state.vertical_velocity_mps -= STATION_GRAVITY_MPS2 * dt;
                let mut candidate = state.feet_m;
                candidate[1] += state.vertical_velocity_mps * dt;
                let mut probe = candidate;
                probe[1] = previous_y;
                let landing = support(
                    static_geometry,
                    &dynamic_geometry,
                    pose,
                    state,
                    probe,
                    STEP_M + 0.15,
                );
                if let Some(hit) = landing.filter(|hit| {
                    state.vertical_velocity_mps <= 0.0
                        && candidate[1] <= hit.height_m + 1.0e-8
                        && previous_y >= hit.height_m - EPSILON_M
                }) {
                    candidate[1] = hit.height_m;
                    state.grounded = true;
                    state.vertical_velocity_mps = 0.0;
                    state.support = hit.support;
                    state.surface_id = hit.surface_id;
                    state.feet_m = candidate;
                } else if clear(static_geometry, &dynamic_geometry, pose, state, candidate) {
                    state.feet_m = candidate;
                } else {
                    // A ceiling or solid stops vertical velocity without moving
                    // the capsule through it. Subsequent gravity settles it.
                    state.vertical_velocity_mps = state.vertical_velocity_mps.min(0.0);
                }
            }
            state.moving = active.is_some() && (forward != 0.0 || strafe != 0.0 || turn != 0.0);
        }
        cursor = next;
    }
    state.updated_at_ms = now;
    if input.is_none_or(|input| now >= input.accepted_ms.saturating_add(STATION_WALK_LEASE_MS)) {
        state.moving = false;
    }
    actuator.lift.updated_at_ms = now;
    state.actuators = actuator.clone();
    Ok(false)
}
pub(super) fn advance(
    w: &mut World,
    now: u64,
    document: Option<&RoomLayoutDocument>,
) -> Result<()> {
    let names = w
        .pilots
        .iter()
        .filter(|(_, pilot)| {
            pilot
                .view
                .station_avatar
                .as_ref()
                .is_some_and(|avatar| avatar.docked_traversal.is_some())
        })
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    if names.is_empty() {
        return Ok(());
    }
    let static_geometry = registered(document)?;
    for name in names {
        let expected = binding(w, &name, document)?;
        let pilot = &w.pilots[&name];
        let mut state = pilot
            .view
            .station_avatar
            .as_ref()
            .unwrap()
            .docked_traversal
            .clone()
            .unwrap();
        ensure!(
            pilot.view.location == Location::Docked && pilot.view.ship_class == ShipClass::Pioneer,
            "docked traversal cannot follow a flying or swapped vessel"
        );
        ensure!(
            state.binding.host_vessel_id == expected.host_vessel_id
                && state.binding.registration_revision == expected.registration_revision
                && state.binding.fleet_layout == expected.fleet_layout
                && state.binding.fleet_hangar == expected.fleet_hangar,
            "active docked traversal host or registration changed"
        );
        let mut actuator = w.accounts[&name]
            .docked_actuators
            .get(&expected.host_vessel_id)
            .cloned()
            .context("active boarding equipment missing")?;
        if state.binding.layout_sha256 != expected.layout_sha256 {
            // Live layout replacement is validated before publication. Rebind
            // the same feet, stop input, and never relocate them into new rooms.
            state.binding = expected.clone();
            actuator.binding = expected.clone();
            w.pilots.get_mut(&name).unwrap().walk_input = None;
            state.moving = false;
        }
        let input = w.pilots[&name].walk_input.clone();
        let completed_seat = advance_actor(
            &mut state,
            &mut actuator,
            input.as_ref(),
            &static_geometry,
            room_open(w, &name),
            now,
        )?;
        let pilot = w.pilots.get_mut(&name).unwrap();
        if completed_seat {
            pilot.view.station_avatar = None;
            pilot.walk_input = None;
        } else {
            if input
                .as_ref()
                .is_some_and(|input| now >= input.accepted_ms.saturating_add(STATION_WALK_LEASE_MS))
            {
                pilot.walk_input = None;
            }
            pilot.view.station_avatar = Some(projection(state, now));
        }
        w.accounts
            .get_mut(&name)
            .unwrap()
            .docked_actuators
            .insert(expected.host_vessel_id, actuator);
    }
    Ok(())
}
pub(super) fn validate_room_layout(w: &World, document: &RoomLayoutDocument) -> Result<()> {
    let static_geometry = registered(Some(document))?;
    for (name, pilot) in &w.pilots {
        if let Some(state) = pilot
            .view
            .station_avatar
            .as_ref()
            .and_then(|avatar| avatar.docked_traversal.as_ref())
        {
            ensure!(state.binding.layout_sha256 == docked_layout_sha256(document),
                "return all affected pilots to their pilot seats before replacing the bound docked layout");
            let pose = docked_binding_pose(&state.binding).map_err(anyhow::Error::msg)?;
            let dynamic_geometry = geometry::dynamic_geometry(
                pose,
                dynamic(
                    &state.actuators,
                    room_open(w, name),
                    state,
                    state.updated_at_ms,
                ),
            )
            .map_err(anyhow::Error::msg)?;
            ensure!(
                state.seat_transition.is_some()
                    || clear(
                        &static_geometry,
                        &dynamic_geometry,
                        pose,
                        state,
                        state.feet_m
                    ),
                "new layout overlaps a docked traversal capsule"
            );
            ensure!(
                !state.grounded
                    || support(
                        &static_geometry,
                        &dynamic_geometry,
                        pose,
                        state,
                        state.feet_m,
                        EPSILON_M
                    )
                    .is_some(),
                "new layout removes occupied docked support"
            );
        }
    }
    Ok(())
}
pub(super) fn validate(w: &World, document: Option<&RoomLayoutDocument>) -> Result<()> {
    for (name, account) in &w.accounts {
        ensure!(
            account.docked_actuators.len() <= 1,
            "unexpected per-vessel boarding equipment count"
        );
        for (host, actuator) in &account.docked_actuators {
            ensure!(
                account.vessel_id(ShipClass::Pioneer) == Some(host.as_str())
                    && actuator.binding.host_vessel_id == *host,
                "boarding equipment is not bound to its owned vessel"
            );
            let pose = docked_binding_pose(&actuator.binding).map_err(anyhow::Error::msg)?;
            let lift = geometry::lift_geometry(pose);
            ensure!(
                actuator.lift.floor_station_m.is_finite()
                    && actuator.lift.floor_station_m >= lift.station_floor_m - EPSILON_M
                    && actuator.lift.floor_station_m <= lift.ship_floor_m + EPSILON_M
                    && !(actuator.inner_hatch_open && actuator.outer_hatch_open)
                    && (!actuator.lift.moving
                        || (actuator.service_power_available
                            && !actuator.inner_hatch_open
                            && !actuator.outer_hatch_open)),
                "invalid docked boarding actuator state"
            );
        }
        let pilot = &w.pilots[name];
        if let Some(state) = pilot
            .view
            .station_avatar
            .as_ref()
            .and_then(|avatar| avatar.docked_traversal.as_ref())
        {
            ensure!(
                state.version == DOCKED_TRAVERSAL_VERSION
                    && pilot.view.location == Location::Docked
                    && pilot.view.ship_class == ShipClass::Pioneer
                    && state.feet_m.iter().all(|v| v.is_finite())
                    && state.heading.is_finite()
                    && state.vertical_velocity_mps.is_finite()
                    && state.vertical_velocity_mps.abs() <= 100.0
                    && state.feet_m[1] >= -EPSILON_M
                    && state.feet_m[1] <= 40.0
                    && state.surface_id.len() <= 128
                    && state.binding.host_vessel_id
                        == account.vessel_id(ShipClass::Pioneer).unwrap()
                    && account.docked_actuators.get(&state.binding.host_vessel_id)
                        == Some(&state.actuators),
                "invalid canonical docked traversal avatar"
            );
            if let Some(phase) = &state.seat_transition {
                ensure!(
                    phase.ends_at_ms.saturating_sub(phase.started_at_ms)
                        == DOCKED_SEAT_TRANSITION_MS
                        && state.grounded
                        && !state.moving,
                    "invalid pilot-seat transition"
                );
            }
        }
    }
    if w.pilots.values().any(|pilot| {
        pilot
            .view
            .station_avatar
            .as_ref()
            .is_some_and(|avatar| avatar.docked_traversal.is_some())
    }) {
        let fallback = default_pioneer_room_layout();
        validate_room_layout(w, document.unwrap_or(&fallback))?;
    }
    Ok(())
}

pub(super) fn project_legacy_response(response: &mut Response) {
    fn snapshot(snapshot: &mut Snapshot) {
        snapshot.docked_traversal_capability = None;
        for pilot in &mut snapshot.pilots {
            if let Some(avatar) = &mut pilot.station_avatar {
                avatar.docked_traversal = None;
            }
        }
    }
    if let Some(state) = &mut response.snapshot {
        snapshot(state);
    }
    if let Some(ledger) = &mut response.ledger {
        snapshot(&mut ledger.snapshot);
    }
}
