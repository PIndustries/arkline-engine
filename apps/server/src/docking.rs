//! Bounded, kinematic docking authority for the explicit DockingV1 realm.
//! Berths are assigned durably to owned Pioneer IDs; no hash collisions or
//! client animation callbacks can capture a berth or complete docking.
use super::*;

/// Exact source selection is independent of production admission. Historical
/// registered fixtures retain their original behavior; unknown sources fail shut.
pub(super) fn source_envelope(
    class: ShipClass,
    registration: Option<&CarrierInventoryRegistration>,
) -> Result<Option<arkline_protocol::carrier_inventory_geometry::CarrierEnvelope>> {
    use arkline_protocol::carrier_inventory_geometry as geometry;
    if class != ShipClass::Pioneer {
        return Ok(None);
    }
    let Some(registration) = registration else {
        return Ok(None);
    };
    if matches!(registration.assembly_revision, 1701 | 1702)
        && geometry::stationary_fixture_for_registration(registration).is_some()
    {
        return Ok(None);
    }
    geometry::source_envelope(registration, geometry::CarrierEnvelopeState::Operational)
        .map(Some)
        .context("Unsupported carrier motion envelope source")
}
pub(super) type Envelope = arkline_protocol::carrier_inventory_geometry::CarrierEnvelope;
pub(super) type EnvelopeMap = BTreeMap<String, Option<Envelope>>;
/// Resolve authoritative account binding, never an optional presentation snapshot.
pub(super) fn owner_envelope(w: &World, name: &str) -> Result<Option<Envelope>> {
    let p = w.pilots.get(name).context("Envelope pilot missing")?;
    if p.view.ship_class == ShipClass::Pioneer {
        if let Some(registration) = w.carrier_inventory_registration.as_ref() {
            let account = w.accounts.get(name).context("Carrier owner missing")?;
            let owned = account
                .carrier_inventory
                .as_ref()
                .context("Carrier owned state missing")?;
            ensure!(
                owned.inventory.binding.registration == *registration
                    && account.vessel_id(ShipClass::Pioneer)
                        == Some(owned.inventory.binding.vessel_id.as_str()),
                "Carrier motion envelope owner/source mismatch"
            );
        } else {
            ensure!(
                w.accounts
                    .get(name)
                    .and_then(|a| a.carrier_inventory.as_ref())
                    .is_none(),
                "Owned carrier has no world source registration"
            );
        }
    }
    source_envelope(p.view.ship_class, w.carrier_inventory_registration.as_ref())
}
pub(super) fn envelopes(w: &World) -> EnvelopeMap {
    w.pilots
        .keys()
        .filter_map(|name| owner_envelope(w, name).ok().map(|e| (name.clone(), e)))
        .collect()
}
fn radius(envelopes: &EnvelopeMap, name: &str) -> Option<f64> {
    envelopes
        .get(name)?
        .map_or(Some(DOCKING_HULL_RADIUS_M), |e| e.origin_radius_m())
}

pub(super) fn enabled(w: &World) -> bool {
    w.scene_layout == SceneLayoutRevision::DockingV1
}
pub(super) fn state(p: &Pilot) -> Option<&DockingState> {
    p.view.flight.as_ref()?.docking.as_ref()
}
pub(super) fn busy(p: &Pilot) -> bool {
    state(p).is_some_and(|d| d.phase.busy())
}

pub(super) fn assign(w: &mut World, name: &str, now: u64) -> Result<()> {
    if !enabled(w) || state(&w.pilots[name]).is_some() {
        return Ok(());
    }
    ensure!(
        w.pilots[name].view.ship_class == ShipClass::Pioneer,
        "docking preview supports Pioneer only"
    );
    let slot = (0..DOCKING_BERTH_COUNT)
        .find(|slot| {
            !w.pilots.values().any(|p| {
                p.view.system_id == w.pilots[name].view.system_id
                    && state(p).is_some_and(|d| d.berth_id == *slot)
            })
        })
        .context("all external Pioneer berths are assigned in this preview world")?;
    let host = w.accounts[name]
        .vessel_id(ShipClass::Pioneer)
        .context("owned Pioneer missing")?
        .to_owned();
    let p = w.pilots.get_mut(name).unwrap();
    flight::ensure_flight(p, now);
    let f = p.view.flight.as_mut().unwrap();
    // Only new pilots are assigned. Existing worlds never enter this branch.
    f.position = docking_berth_position(slot);
    f.orientation = docking_berth_orientation();
    f.heading = [-1., 0., 0.];
    f.docking = Some(DockingState {
        version: DOCKING_VERSION,
        host_vessel_id: host,
        berth_id: slot,
        operation_id: 0,
        phase: DockingPhase::Secured,
        segment: None,
        clamps: 1.,
        updated_at_ms: now,
        hold_reason: None,
        departing: false,
    });
    Ok(())
}

fn segment(
    f: &FlightState,
    target: [f64; 3],
    orientation: [f64; 4],
    now: u64,
    duration: u64,
) -> DockingSegment {
    DockingSegment {
        source_m: f.position,
        target_m: target,
        initial_velocity_mps: f.velocity,
        source_orientation: f.orientation,
        target_orientation: orientation,
        started_at_ms: now,
        duration_ms: duration,
    }
}
fn set_phase(
    p: &mut Pilot,
    phase: DockingPhase,
    target: [f64; 3],
    orientation: [f64; 4],
    now: u64,
    duration: u64,
) {
    let f = p.view.flight.as_mut().unwrap();
    let next = segment(f, target, orientation, now, duration);
    let d = f.docking.as_mut().unwrap();
    d.phase = phase;
    d.segment = Some(next);
    d.updated_at_ms = now;
    d.hold_reason = None;
    f.mode = NavigationMode::Stop;
    f.angular_velocity = [0.; 3];
    p.controls = None;
}
fn distance_segment(point: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let ab: [f64; 3] = std::array::from_fn(|i| b[i] - a[i]);
    let length2: f64 = ab.iter().map(|x| x * x).sum();
    let t = if length2 > 1e-12 {
        (point
            .iter()
            .zip(a)
            .zip(ab)
            .map(|((p, a), b)| (p - a) * b)
            .sum::<f64>()
            / length2)
            .clamp(0., 1.)
    } else {
        0.
    };
    flight::distance(point, std::array::from_fn(|i| a[i] + t * ab[i]))
}
/// Conservative sphere sweep, including other ships at their confirmed pose.
/// All new berths and their corridors stay outside the station's +X surface.
pub(super) fn clear(w: &World, name: &str, a: [f64; 3], b: [f64; 3]) -> bool {
    let envelopes = envelopes(w);
    let Some(radius) = radius(&envelopes, name) else {
        return false;
    };
    if !a
        .iter()
        .chain(&b)
        .all(|v| v.is_finite() && v.abs() <= SPACE_BOUND)
        || a[0].min(b[0]) < STATION_POSITION[0] + radius + 30.
    {
        return false;
    }
    let p = &w.pilots[name];
    let berth = state(p)
        .map(|d| docking_berth_position(d.berth_id))
        .unwrap();
    // Only the assigned, aligned capture corridor enters the platform envelope.
    if a[0].min(b[0]) < STATION_POSITION[0] + 300.
        && !(a[0].min(b[0]) >= berth[0] - 1e-6
            && [a, b]
                .iter()
                .all(|v| (v[1] - berth[1]).abs() < 1e-6 && (v[2] - berth[2]).abs() < 1e-6))
    {
        return false;
    }
    ships_clear(&w.pilots, name, p.view.system_id, a, b, &envelopes)
}

fn ships_clear(
    views: &BTreeMap<String, Pilot>,
    name: &str,
    system: u32,
    a: [f64; 3],
    b: [f64; 3],
    envelopes: &EnvelopeMap,
) -> bool {
    let Some(own_radius) = radius(envelopes, name) else {
        return false;
    };
    !views.iter().any(|(other, p)| {
        other != name
            && p.view.system_id == system
            && !matches!(
                p.view.location,
                Location::InTransit { .. } | Location::Jumping { .. }
            )
            && p.view.flight.as_ref().is_some_and(|f| {
                let Some(peer_radius) = radius(envelopes, other) else {
                    return true;
                };
                let separation = own_radius + peer_radius + 4.;
                if distance_segment(f.position, a, b) < separation {
                    return true;
                }
                // Reserve the remaining kinematic route as well as the current hull.
                // Conservative swept AABBs may hold crossing traffic, never let it overlap.
                f.docking
                    .as_ref()
                    .and_then(|d| d.segment.as_ref())
                    .is_some_and(|s| {
                        (0..3).all(|i| {
                            a[i].min(b[i]) <= f.position[i].max(s.target_m[i]) + separation
                                && a[i].max(b[i]) >= f.position[i].min(s.target_m[i]) - separation
                        })
                    })
            })
    })
}
pub(super) fn free_step_clear(
    views: &BTreeMap<String, Pilot>,
    name: &str,
    system: u32,
    a: [f64; 3],
    b: [f64; 3],
    envelopes: &EnvelopeMap,
) -> bool {
    let Some(radius) = radius(envelopes, name) else {
        return false;
    };
    if a[0].min(b[0]) < STATION_POSITION[0] + radius + 30. {
        return false;
    }
    for id in 0..DOCKING_BERTH_COUNT {
        let port = docking_berth_position(id);
        // Free flight cannot penetrate the platform/arm envelope. The docking
        // controller alone has the proved aligned corridor through this envelope.
        if (0..3).all(|i| {
            let half = [100., 55., 85.][i] + (radius - DOCKING_HULL_RADIUS_M).max(0.);
            a[i].min(b[i]) <= port[i] + half && a[i].max(b[i]) >= port[i] - half
        }) {
            return false;
        }
    }
    ships_clear(views, name, system, a, b, envelopes)
}

fn static_pose_clear(
    position: [f64; 3],
    orientation: [f64; 4],
    envelope: Option<Envelope>,
) -> bool {
    let world_bounds = if let Some(envelope) = envelope {
        let Some(bounds) =
            envelope.transformed_bounds(arkline_protocol::drone_bay_geometry::BayPose {
                position_m: position,
                orientation_xyzw: orientation,
            })
        else {
            return false;
        };
        bounds
    } else {
        let min = PIONEER_BODY_BOUNDS_M[0];
        let max = PIONEER_BODY_BOUNDS_M[1];
        let center_local = std::array::from_fn(|i| (min[i] + max[i]) * 0.5);
        let center_offset = flight::rotate(orientation, center_local);
        let center: [f64; 3] = std::array::from_fn(|i| position[i] + center_offset[i]);
        let half: [f64; 3] = std::array::from_fn(|i| (max[i] - min[i]) * 0.5);
        let mut extent = [0.; 3];
        for axis in 0..3 {
            let mut basis = [0.; 3];
            basis[axis] = half[axis];
            let rotated = flight::rotate(orientation, basis);
            for i in 0..3 {
                extent[i] += rotated[i].abs();
            }
        }
        [
            std::array::from_fn(|i| center[i] - extent[i]),
            std::array::from_fn(|i| center[i] + extent[i]),
        ]
    };
    static BOXES: std::sync::OnceLock<Vec<([f64; 3], [f64; 3])>> = std::sync::OnceLock::new();
    BOXES
        .get_or_init(|| {
            (0..DOCKING_BERTH_COUNT)
                .flat_map(docking_platform_boxes)
                .collect()
        })
        .iter()
        .all(|(lo, hi)| (0..3).any(|i| world_bounds[1][i] <= lo[i] || world_bounds[0][i] >= hi[i]))
}
// Exact-source corridor hazards; permission remains with the shared authority getter.
fn curve_bounds(s: &DockingSegment) -> Option<([f64; 3], [f64; 3])> {
    if s.duration_ms == 0 || s.started_at_ms.checked_add(s.duration_ms).is_none() {
        return None;
    }
    let control: [f64; 3] = std::array::from_fn(|i| {
        s.source_m[i] + s.initial_velocity_mps[i] * (s.duration_ms as f64 / 3000.)
    });
    let points = [s.source_m, control, s.target_m];
    if !points
        .iter()
        .flatten()
        .all(|v| v.is_finite() && v.abs() <= SPACE_BOUND)
    {
        return None;
    }
    Some((
        std::array::from_fn(|i| points.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min)),
        std::array::from_fn(|i| {
            points
                .iter()
                .map(|p| p[i])
                .fold(f64::NEG_INFINITY, f64::max)
        }),
    ))
}
fn sphere_route_clear(s: &DockingSegment, own_radius: f64, center: [f64; 3], radius: f64) -> bool {
    if !s
        .source_m
        .iter()
        .chain(&s.target_m)
        .chain(&center)
        .all(|v| v.is_finite() && v.abs() <= SPACE_BOUND)
        || !own_radius.is_finite()
        || own_radius <= 0.
        || !radius.is_finite()
        || radius < 0.
    {
        return false;
    }
    let separation = own_radius + radius + 4.;
    let Some((lo, hi)) = curve_bounds(s) else {
        return false;
    };
    let distance = flight::length(std::array::from_fn(|i| {
        (lo[i] - center[i]).max(center[i] - hi[i]).max(0.)
    }));
    separation.is_finite() && distance.is_finite() && distance > separation
}
fn drone_corridor_radius(d: &DroneView, end: u64, now: u64) -> Option<f64> {
    if d.updated_at_ms > now || !d.position.iter().chain(&d.velocity).all(|v| v.is_finite()) {
        return None;
    }
    let body =
        crate::independent_bays::independent_drone_radius([[-2.5, -0.9, -3.], [2.5, 0.9, 3.]])?;
    let speed = flight::length(d.velocity).max(drone_speed(d.class));
    // Complete stale-observation plus future segment horizon; NEVER truncate to
    // one tick or only test a target endpoint. A Hold command can change later.
    let radius = body + speed * (end.max(now).saturating_sub(d.updated_at_ms) as f64 / 1000.);
    radius.is_finite().then_some(radius)
}
fn retained_corridor_route_clear(
    s: &DockingSegment,
    own_radius: f64,
    parent: arkline_protocol::drone_bay_geometry::BayPose,
    poses: impl Iterator<Item = arkline_protocol::drone_bay_geometry::BayPose>,
) -> bool {
    let Some(body) =
        crate::independent_bays::independent_drone_radius([[-2.5, -0.9, -3.], [2.5, 0.9, 3.]])
    else {
        return false;
    };
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut count = 0;
    for pose in poses {
        let Some(world) = parent.compose(pose) else {
            return false;
        };
        if world
            .position_m
            .iter()
            .any(|v| !v.is_finite() || v.abs() > SPACE_BOUND)
        {
            return false;
        }
        for i in 0..3 {
            lo[i] = lo[i].min(world.position_m[i]);
            hi[i] = hi[i].max(world.position_m[i]);
        }
        count += 1;
    }
    if count == 0 {
        return false;
    }
    let Some((own_lo, own_hi)) = curve_bounds(s) else {
        return false;
    };
    let margin = own_radius + body + 4.;
    margin.is_finite() && (0..3).any(|i| own_hi[i] < lo[i] - margin || own_lo[i] > hi[i] + margin)
}
// Hermite with zero final tangent is a cubic Bezier with these four
// control points. Its entire curve lies in their convex hull, even when the
// accepted initial velocity overshoots the source/target endpoint box.
fn peer_curve_clear(
    s: &DockingSegment,
    own_radius: f64,
    peer: &DockingSegment,
    peer_radius: f64,
) -> bool {
    if peer.duration_ms == 0
        || peer.started_at_ms.checked_add(peer.duration_ms).is_none()
        || !peer.initial_velocity_mps.iter().all(|v| v.is_finite())
    {
        return false;
    }
    let Some((own_lo, own_hi)) = curve_bounds(s) else {
        return false;
    };
    let seconds = peer.duration_ms as f64 / 1000.;
    let control: [f64; 3] =
        std::array::from_fn(|i| peer.source_m[i] + peer.initial_velocity_mps[i] * seconds / 3.);
    let points = [peer.source_m, control, peer.target_m];
    let separation = own_radius + peer_radius + 4.;
    if !separation.is_finite()
        || own_radius <= 0.
        || peer_radius <= 0.
        || !points
            .iter()
            .flatten()
            .chain(s.source_m.iter())
            .chain(s.target_m.iter())
            .all(|v| v.is_finite() && v.abs() <= SPACE_BOUND)
    {
        return false;
    }
    // Four metres existing safety margin also dominates finite f64 arithmetic
    // error throughout SPACE_BOUND; do not remove it from this swept bound.
    (0..3).any(|i| {
        let lo = points.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min);
        let hi = points
            .iter()
            .map(|p| p[i])
            .fold(f64::NEG_INFINITY, f64::max);
        own_hi[i] < lo - separation || own_lo[i] > hi + separation
    })
}
fn free_peer_radius(
    base_radius: f64,
    velocity: [f64; 3],
    updated_at_ms: u64,
    end: u64,
    now: u64,
) -> Option<f64> {
    if updated_at_ms > now
        || !base_radius.is_finite()
        || base_radius <= 0.
        || !velocity.iter().all(|v| v.is_finite())
    {
        return None;
    }
    // Future controls are not locked: cover any permitted direction/speed over
    // the entire stale plus remaining horizon, not one tick or current heading.
    let speed = flight::length(velocity).max(MAX_SHIP_SPEED);
    let radius = base_radius + speed * (end.max(now).saturating_sub(updated_at_ms) as f64 / 1000.);
    radius.is_finite().then_some(radius)
}
fn peer_current_valid(f: &FlightState, now: u64) -> bool {
    f.updated_at_ms <= now
        && f.position
            .iter()
            .all(|v| v.is_finite() && v.abs() <= SPACE_BOUND)
        && f.velocity
            .iter()
            .chain(&f.angular_velocity)
            .chain(&f.acceleration)
            .chain(&f.heading)
            .all(|v| v.is_finite())
        && arkline_protocol::drone_bay_geometry::BayPose {
            position_m: f.position,
            orientation_xyzw: f.orientation,
        }
        .transform_vector([0.; 3])
        .is_some()
}
fn corridor_external_clear(w: &World, name: &str, s: &DockingSegment, now: u64) -> bool {
    let Some(p) = w.pilots.get(name) else {
        return false;
    };
    let Ok(Some(envelope)) = owner_envelope(w, name) else {
        return false;
    };
    let Some(own_radius) = envelope.origin_radius_m() else {
        return false;
    };
    let Some(end) = s.started_at_ms.checked_add(s.duration_ms) else {
        return false;
    };
    if now < s.started_at_ms {
        return false;
    }
    if w.asteroids
        .iter()
        .filter(|a| a.system_id == p.view.system_id)
        .any(|a| !sphere_route_clear(s, own_radius, a.position, a.radius))
    {
        return false;
    }
    for (peer_name, peer) in &w.pilots {
        if peer_name == name || peer.view.system_id != p.view.system_id {
            continue;
        }
        if let Some(f) = peer.view.flight.as_ref() {
            if !peer_current_valid(f, now) {
                return false;
            }
            let peer_radius = match owner_envelope(w, peer_name) {
                Ok(Some(e)) => e.origin_radius_m(),
                Ok(None) => Some(hull_radius_m(peer.view.ship_class).max(DOCKING_HULL_RADIUS_M)),
                Err(_) => None,
            };
            let Some(peer_radius) = peer_radius else {
                return false;
            };
            if let Some(route) = f.docking.as_ref().and_then(|d| d.segment.as_ref()) {
                if !peer_curve_clear(s, own_radius, route, peer_radius) {
                    return false;
                }
            } else if !matches!(
                peer.view.location,
                Location::Docked | Location::InTransit { .. } | Location::Jumping { .. }
            ) {
                let Some(reachable) =
                    free_peer_radius(peer_radius, f.velocity, f.updated_at_ms, end, now)
                else {
                    return false;
                };
                if !sphere_route_clear(s, own_radius, f.position, reachable) {
                    return false;
                }
            }
        } else if !matches!(
            peer.view.location,
            Location::Docked | Location::InTransit { .. } | Location::Jumping { .. }
        ) {
            // No current pose means no bounded free-flight reservation.
            return false;
        }
        for drone in &peer.view.drones {
            let Some(radius) = drone_corridor_radius(drone, end, now) else {
                return false;
            };
            if !sphere_route_clear(s, own_radius, drone.position, radius) {
                return false;
            }
        }
        if let Some(owned) = w
            .accounts
            .get(peer_name)
            .and_then(|a| a.carrier_inventory.as_ref())
        {
            // Retain complete exterior reservations even if a presentation actor
            // is not available yet. Rotations are covered by the full body radius.
            for arrival in owned.arrivals.values() {
                if arrival.system_id != peer.view.system_id
                    || !retained_corridor_route_clear(
                        s,
                        own_radius,
                        arrival.parent,
                        arrival
                            .path
                            .iter()
                            .copied()
                            .chain([arrival.original_body_pose, arrival.confirmed]),
                    )
                {
                    return false;
                }
            }
            for departure in owned.departures.values() {
                if !retained_corridor_route_clear(
                    s,
                    own_radius,
                    departure.parent,
                    [
                        departure.initial,
                        departure.source,
                        departure.target,
                        departure.confirmed,
                    ]
                    .into_iter(),
                ) {
                    return false;
                }
            }
        }
    }
    true
}
fn assigned_carrier_berth(w: &World, name: &str) -> Option<u16> {
    let p = w.pilots.get(name)?;
    let d = state(p)?;
    (enabled(w)
        && d.version == DOCKING_VERSION
        && d.berth_id < DOCKING_BERTH_COUNT
        && w.accounts.get(name)?.vessel_id(ShipClass::Pioneer) == Some(d.host_vessel_id.as_str()))
    .then_some(d.berth_id)
}
fn certified_corridor(w: &World, name: &str, s: &DockingSegment) -> bool {
    let Some(berth_id) = assigned_carrier_berth(w, name) else {
        return false;
    };
    let Some(reg) = w.carrier_inventory_registration.as_ref() else {
        return false;
    };
    let boxes: Vec<_> = (0..DOCKING_BERTH_COUNT)
        .flat_map(docking_platform_boxes)
        .collect();
    arkline_protocol::carrier_inventory_geometry::source_corridor_geometry_clear(
        reg,
        w.scene_layout,
        berth_id,
        s,
        &boxes,
    )
}

// Mandatory for source carriers even where the global station AABB is clear.
// Geometry refinement must never swallow an unsafe inventory or exterior hazard.
fn carrier_route_safe(w: &World, name: &str, s: &DockingSegment, now: u64) -> bool {
    match owner_envelope(w, name) {
        Ok(None) => true, // unchanged legacy behavior
        Ok(Some(_)) => {
            assigned_carrier_berth(w, name).is_some()
                && carrier_inventory::world::secured_for_departure(w, name, now).is_ok()
                && corridor_external_clear(w, name, s, now)
        }
        Err(_) => false,
    }
}

fn fraction_time(start: u64, duration: u64, index: u64) -> Option<u64> {
    if index > 100 {
        return None;
    }
    let offset = (u128::from(duration) * u128::from(index)) / 100;
    start.checked_add(u64::try_from(offset).ok()?)
}
fn route_clear(w: &World, name: &str, s: &DockingSegment) -> bool {
    let Ok(envelope) = owner_envelope(w, name) else {
        return false;
    };
    if !carrier_route_safe(w, name, s, s.started_at_ms) {
        return false;
    }
    let certified = certified_corridor(w, name, s);
    let mut previous = s.source_m;
    for i in 1..=100 {
        let Some(at) = fraction_time(s.started_at_ms, s.duration_ms, i) else {
            return false;
        };
        let (next, _, orientation) = s.sample(at);
        if (!static_pose_clear(next, orientation, envelope) && !certified)
            || !clear(w, name, previous, next)
        {
            return false;
        }
        previous = next;
    }
    true
}

#[cfg(test)]
pub(crate) fn source_route_checks_for_test(
    w: &World,
    name: &str,
    s: &DockingSegment,
) -> (bool, bool, bool) {
    (
        carrier_route_safe(w, name, s, s.started_at_ms),
        certified_corridor(w, name, s),
        route_clear(w, name, s),
    )
}

pub(super) fn begin(w: &mut World, name: &str, departing: bool, now: u64) -> Result<()> {
    ensure!(
        enabled(w),
        "continuous docking requires its fresh-world preview realm"
    );
    let p = &w.pilots[name];
    let d = state(p).context("assigned docking berth missing")?;
    ensure!(
        !d.phase.busy(),
        "docking operation already active; cancel or resume that operation"
    );
    ensure!(
        p.view.drones.is_empty(),
        "recover all drones before docking or departure"
    );
    ensure!(
        p.view.system_id == HOME_SYSTEM_ID,
        "external docking preview is in the home system"
    );
    docked_traversal::can_undock(w, name)?;
    let f = p.view.flight.as_ref().unwrap();
    let (phase, target, q, duration) = if departing {
        ensure!(
            d.secured() && p.view.location == Location::Docked,
            "departure requires secured docking"
        );
        (DockingPhase::Releasing, f.position, f.orientation, 2000)
    } else {
        ensure!(
            matches!(p.view.location, Location::Station | Location::Belt),
            "docking requires normal space"
        );
        ensure!(
            flight::length(f.velocity) <= 10.,
            "brake below 10 m/s before requesting berth approach"
        );
        let gate = docking_gate_position(d.berth_id);
        ensure!(
            flight::distance(f.position, gate) <= 6000.
                && f.position[0] >= docking_berth_position(d.berth_id)[0] + 100.,
            "approach the exterior berth from its space-facing side"
        );
        let duration =
            ((flight::distance(f.position, gate) * 1.5 / 40. * 1000.).ceil() as u64).max(6000);
        (DockingPhase::Approaching, gate, f.orientation, duration)
    };
    let s = segment(f, target, q, now, duration);
    ensure!(
        route_clear(w, name, &s),
        "docking approach is obstructed; hold clear of station and other ships"
    );
    let p = w.pilots.get_mut(name).unwrap();
    universe::clear_locks(p);
    mining::stop_all(p, "Docking authority took control");
    let d = p.view.flight.as_mut().unwrap().docking.as_mut().unwrap();
    d.operation_id = d
        .operation_id
        .checked_add(1)
        .context("docking operation ID exhausted")?;
    d.departing = departing;
    p.view.location = Location::Station;
    set_phase(p, phase, target, q, now, duration);
    Ok(())
}

/// Recovery never catches up an offline journey or changes the last confirmed pose.
pub(super) fn hold(p: &mut Pilot, now: u64, reason: &str) {
    if !busy(p) {
        return;
    }
    let f = p.view.flight.as_mut().unwrap();
    let d = f.docking.as_mut().unwrap();
    d.phase = DockingPhase::Held;
    d.segment = None;
    d.hold_reason = Some(reason.into());
    d.updated_at_ms = now;
    f.velocity = [0.; 3];
    f.acceleration = [0.; 3];
    f.angular_velocity = [0.; 3];
    f.mode = NavigationMode::Stop;
    f.updated_at_ms = now;
    p.controls = None;
}
pub(super) fn cancel(w: &mut World, name: &str, id: u64, now: u64) -> Result<()> {
    let p = &w.pilots[name];
    let d = state(p).context("docking operation missing")?;
    ensure!(d.operation_id == id, "docking operation changed");
    ensure!(d.phase.busy(), "no active docking operation");
    if matches!(d.phase, DockingPhase::Held | DockingPhase::Braking) {
        return Ok(());
    }
    let f = p.view.flight.as_ref().unwrap();
    let seconds = if f.velocity[0] < -1e-8 {
        (2. * (f.position[0] - docking_berth_position(d.berth_id)[0]).max(0.) / -f.velocity[0])
            .clamp(0.05, 2.)
    } else {
        2.
    };
    let duration = (seconds * 1000.).floor() as u64;
    let target = std::array::from_fn(|i| f.position[i] + f.velocity[i] * (duration as f64 / 2000.));
    let q = f.orientation;
    let s = segment(f, target, q, now, duration);
    ensure!(
        route_clear(w, name, &s),
        "safe braking path obstructed; docking safety hold required"
    );
    set_phase(
        w.pilots.get_mut(name).unwrap(),
        DockingPhase::Braking,
        target,
        q,
        now,
        duration,
    );
    Ok(())
}
pub(super) fn resume(w: &mut World, name: &str, id: u64, now: u64) -> Result<()> {
    let p = &w.pilots[name];
    let d = state(p).context("docking operation missing")?;
    ensure!(
        d.operation_id == id && d.phase == DockingPhase::Held,
        "held docking operation changed"
    );
    docked_traversal::can_undock(w, name)?;
    let f = p.view.flight.as_ref().unwrap();
    let berth = docking_berth_position(d.berth_id);
    let gate = docking_gate_position(d.berth_id);
    let (phase, target, q, duration) = if d.departing {
        if d.clamps > 0. {
            (DockingPhase::Releasing, f.position, f.orientation, 2000)
        } else {
            (DockingPhase::Departing, gate, f.orientation, 15000)
        }
    } else if flight::distance(f.position, berth) < 0.001 {
        (
            DockingPhase::Clamping,
            berth,
            docking_berth_orientation(),
            2000,
        )
    } else if f.position[0] < gate[0]
        && flight::distance([f.position[0], berth[1], berth[2]], f.position) < 0.01
    {
        (
            DockingPhase::Berthing,
            berth,
            docking_berth_orientation(),
            15000,
        )
    } else {
        (
            DockingPhase::Approaching,
            gate,
            f.orientation,
            ((flight::distance(f.position, gate) * 1.5 / 40. * 1000.).ceil() as u64).max(6000),
        )
    };
    ensure!(
        route_clear(w, name, &segment(f, target, q, now, duration)),
        "docking resume path obstructed"
    );
    set_phase(
        w.pilots.get_mut(name).unwrap(),
        phase,
        target,
        q,
        now,
        duration,
    );
    Ok(())
}

pub(super) fn advance(w: &mut World, now: u64) {
    if !enabled(w) {
        return;
    }
    let names = w.pilots.keys().cloned().collect::<Vec<_>>();
    for name in names {
        // Phase count is bounded independently of elapsed wall time.
        for _ in 0..6 {
            let p = &w.pilots[&name];
            let Some(d) = state(p).cloned() else { break };
            let Some(s) = d.segment.clone() else { break };
            if now < d.updated_at_ms {
                break;
            }
            let until = now.min(s.ends_at_ms());
            let (position, velocity, q) = s.sample(until);
            let old = p.view.flight.as_ref().unwrap().position;
            // Evaluate the original curve in bounded samples, not a straight chord
            // across a long/coarse tick with a nonzero initial tangent.
            let safe = carrier_route_safe(w, &name, &s, now);
            let certified = certified_corridor(w, &name, &s);
            let sweep_clear = safe
                && (0..=100).all(|i| {
                    let Some(interval) = until.checked_sub(d.updated_at_ms) else {
                        return false;
                    };
                    let Some(at) = fraction_time(d.updated_at_ms, interval, i) else {
                        return false;
                    };
                    let (point, _, orientation) = s.sample(at);
                    (owner_envelope(w, &name)
                        .is_ok_and(|envelope| static_pose_clear(point, orientation, envelope))
                        || certified)
                        && clear(w, &name, old, point)
                });
            if !sweep_clear || !clear(w, &name, old, position) {
                hold(
                    w.pilots.get_mut(&name).unwrap(),
                    now,
                    "Docking path occupied",
                );
                break;
            }
            let p = w.pilots.get_mut(&name).unwrap();
            let f = p.view.flight.as_mut().unwrap();
            let elapsed = until.saturating_sub(d.updated_at_ms) as f64 / 1000.;
            f.acceleration = if elapsed > 0. {
                std::array::from_fn(|i| (velocity[i] - f.velocity[i]) / elapsed)
            } else {
                [0.; 3]
            };
            f.position = position;
            f.velocity = velocity;
            f.orientation = q;
            f.heading = flight::rotate(q, [1., 0., 0.]);
            f.updated_at_ms = until;
            let state = f.docking.as_mut().unwrap();
            state.updated_at_ms = until;
            if d.phase == DockingPhase::Clamping {
                state.clamps = d.clamps.max(s.progress(until));
            }
            if d.phase == DockingPhase::Releasing {
                state.clamps = d.clamps.min(1. - s.progress(until));
            }
            if until < s.ends_at_ms() {
                break;
            }
            let berth = docking_berth_position(d.berth_id);
            let gate = docking_gate_position(d.berth_id);
            match d.phase {
                DockingPhase::Approaching => set_phase(
                    p,
                    DockingPhase::Aligning,
                    gate,
                    docking_berth_orientation(),
                    until,
                    3000,
                ),
                DockingPhase::Aligning => set_phase(
                    p,
                    DockingPhase::Berthing,
                    berth,
                    docking_berth_orientation(),
                    until,
                    15000,
                ),
                DockingPhase::Berthing => set_phase(
                    p,
                    DockingPhase::Clamping,
                    berth,
                    docking_berth_orientation(),
                    until,
                    2000,
                ),
                DockingPhase::Releasing => set_phase(
                    p,
                    DockingPhase::Departing,
                    gate,
                    docking_berth_orientation(),
                    until,
                    15000,
                ),
                DockingPhase::Clamping => {
                    state.phase = DockingPhase::Secured;
                    state.segment = None;
                    state.clamps = 1.;
                    f.acceleration = [0.; 3];
                    p.view.location = Location::Docked;
                }
                DockingPhase::Departing => {
                    state.phase = DockingPhase::Free;
                    state.segment = None;
                    state.clamps = 0.;
                    f.acceleration = [0.; 3];
                    p.view.location = Location::Station;
                }
                DockingPhase::Braking => {
                    hold(p, until, "Cancelled; resume when safe");
                }
                _ => break,
            }
        }
    }
}
pub(super) fn validate(w: &World) -> Result<()> {
    if !enabled(w) {
        ensure!(
            w.pilots.values().all(|p| state(p).is_none()),
            "docking state in legacy realm"
        );
        return Ok(());
    }
    let mut slots = std::collections::BTreeSet::new();
    for (name, p) in &w.pilots {
        let d = state(p).context("docking preview pilot has no assigned berth")?;
        ensure!(
            d.version == DOCKING_VERSION
                && d.berth_id < DOCKING_BERTH_COUNT
                && slots.insert((p.view.system_id, d.berth_id))
                && w.accounts[name].vessel_id(ShipClass::Pioneer)
                    == Some(d.host_vessel_id.as_str())
                && p.view.ship_class == ShipClass::Pioneer,
            "invalid docking vessel or berth reservation"
        );
        ensure!(
            d.clamps.is_finite() && (0. ..=1.).contains(&d.clamps),
            "invalid docking clamp position"
        );
        ensure!(
            (p.view.location == Location::Docked) == d.secured(),
            "docking access requires secured clamps"
        );
        ensure!(
            d.segment.is_some()
                != matches!(
                    d.phase,
                    DockingPhase::Free | DockingPhase::Secured | DockingPhase::Held
                ),
            "docking phase and trajectory disagree"
        );
        if d.secured() {
            ensure!(
                d.clamps == 1.
                    && flight::distance(
                        p.view.flight.as_ref().unwrap().position,
                        docking_berth_position(d.berth_id)
                    ) < 1e-6,
                "secured vessel is not captured at its assigned berth"
            );
        }
        if let Some(s) = &d.segment {
            ensure!(
                s.duration_ms > 0
                    && s.duration_ms <= 300_000
                    && s.ends_at_ms() >= s.started_at_ms
                    && s.source_m
                        .iter()
                        .chain(&s.target_m)
                        .chain(&s.initial_velocity_mps)
                        .chain(&s.source_orientation)
                        .chain(&s.target_orientation)
                        .all(|v| v.is_finite()),
                "invalid docking trajectory"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod envelope_tests {
    use super::*;
    pub(super) fn registration() -> CarrierInventoryRegistration {
        let mut reg = arkline_protocol::carrier_inventory_geometry::stationary_fixture_definition()
            .unwrap()
            .topology
            .registration
            .clone();
        reg.assembly_revision = 1703;
        reg.assembly_source_sha256 =
            "79ab43221861f74383ca36ca6a742ff5588e08003eb7b10f9d8b6f0d5469664a".into();
        reg
    }
    fn pilot(name: &str, position: [f64; 3]) -> Pilot {
        let mut p: Pilot = serde_json::from_value(serde_json::json!({
            "view": {"name":name,"ship_class":"pioneer","credits":0,"ore":0,"location":{"kind":"station"},"system_id":1,
                "drones":[],"target_locks":[],"locked_asteroids":[],"mining_modules":[]},
            "token_hash":"test","last_mine_ms":null
        })).unwrap();
        flight::ensure_flight(&mut p, 0);
        p.view.flight.as_mut().unwrap().position = position;
        p
    }
    #[test]
    fn source_envelope_berth_clearance_and_unsupported_source() {
        let reg = registration();
        let port = docking_berth_position(0);
        let operational = source_envelope(ShipClass::Pioneer, Some(&reg)).unwrap();
        let closed = arkline_protocol::carrier_inventory_geometry::source_envelope(
            &reg,
            arkline_protocol::carrier_inventory_geometry::CarrierEnvelopeState::Closed,
        );
        assert!(static_pose_clear(port, docking_berth_orientation(), None));
        assert!(!static_pose_clear(
            port,
            docking_berth_orientation(),
            operational
        ));
        assert!(!static_pose_clear(
            port,
            docking_berth_orientation(),
            closed
        ));
        assert!(static_pose_clear(
            [port[0], port[1] + 2., port[2]],
            docking_berth_orientation(),
            operational
        ));
        let bounds = operational
            .unwrap()
            .transformed_bounds(arkline_protocol::drone_bay_geometry::BayPose {
                position_m: port,
                orientation_xyzw: docking_berth_orientation(),
            })
            .unwrap();
        let overlaps: Vec<_> = docking_platform_boxes(0)
            .into_iter()
            .filter(|(lo, hi)| (0..3).all(|i| bounds[1][i] > lo[i] && bounds[0][i] < hi[i]))
            .collect();
        eprintln!("Exact1703 legacy berth overlap: hull={bounds:?}, station={overlaps:?}");
        assert!(!overlaps.is_empty());
        let mut bad = reg.clone();
        bad.assembly_source_sha256.replace_range(..1, "0");

        assert!(source_envelope(ShipClass::Pioneer, None).unwrap().is_none());
        assert!(source_envelope(ShipClass::Pioneer, Some(&bad)).is_err());
    }
    #[test]
    fn source_envelope_near_peer_and_legacy_radius_regression() {
        let reg = registration();
        let a = [1000., 1000., 1000.];
        let peer = [1100., 1000., 1000.];
        let mut views =
            BTreeMap::from([("a".into(), pilot("a", a)), ("b".into(), pilot("b", peer))]);
        let legacy = BTreeMap::from([("a".into(), None), ("b".into(), None)]);
        let envelope = source_envelope(ShipClass::Pioneer, Some(&reg)).unwrap();
        let current = BTreeMap::from([("a".into(), envelope), ("b".into(), envelope)]);
        assert!(ships_clear(&views, "a", 1, a, a, &legacy));
        assert!(!ships_clear(&views, "a", 1, a, a, &current));
        views
            .get_mut("b")
            .unwrap()
            .view
            .flight
            .as_mut()
            .unwrap()
            .position[0] = 1200.;
        assert!(ships_clear(&views, "a", 1, a, a, &current));
        assert!(!ships_clear(&views, "a", 1, a, a, &BTreeMap::new()));
        assert_eq!(radius(&legacy, "a"), Some(44.));
        let mut world = World {
            carrier_inventory_registration: Some(reg),
            pilots: views,
            ..World::default()
        };
        assert!(owner_envelope(&world, "a").is_err());
        assert!(envelopes(&world).is_empty());
        assets::ensure_accounts(&mut world);
        assert!(owner_envelope(&world, "a").is_err());
        world.carrier_inventory_loadout = Some(vec![DroneClass::Mining; 20]);
        carrier_inventory::world::initialize(&mut world, "a", 0).unwrap();
        assert!(owner_envelope(&world, "a").unwrap().is_some());
        let owned = world
            .accounts
            .get_mut("a")
            .unwrap()
            .carrier_inventory
            .as_mut()
            .unwrap();
        owned.inventory.binding.vessel_id.push_str("-wrong");
        assert!(owner_envelope(&world, "a").is_err());
        world.carrier_inventory_registration = None;
        assert!(owner_envelope(&world, "a").is_err());
        world.accounts.get_mut("a").unwrap().carrier_inventory = None;
        assert_eq!(envelopes(&world), legacy);
    }
}

#[cfg(test)]
mod corridor_hazard_tests {
    use super::*;
    fn segment() -> DockingSegment {
        DockingSegment {
            source_m: [0.; 3],
            target_m: [250., 0., 0.],
            initial_velocity_mps: [0.; 3],
            source_orientation: [0., 1., 0., 0.],
            target_orientation: [0., 1., 0., 0.],
            started_at_ms: 1000,
            duration_ms: 10000,
        }
    }
    #[test]
    fn corridor_asteroid_and_full_drone_horizon_are_not_endpoint_only() {
        let s = segment();
        assert!(!sphere_route_clear(&s, 57., [125., 0., 0.], 5.));
        assert!(sphere_route_clear(&s, 57., [125., 500., 0.], 5.));
        let mut drone:DroneView=serde_json::from_value(serde_json::json!({"class":"mining","position":[125.,500.,0.],"velocity":[0.,0.,0.],"heading":[1.,0.,0.],"mode":{"kind":"hold"},"updated_at_ms":1000})).unwrap();
        let r = drone_corridor_radius(&drone, s.ends_at_ms(), 1000).unwrap();
        assert!(r > 1000.);
        assert!(!sphere_route_clear(&s, 57., drone.position, r));
        drone.velocity = [400., 0., 0.];
        assert!(drone_corridor_radius(&drone, s.ends_at_ms(), 1000).unwrap() > 4000.);
        drone.updated_at_ms = 1001;
        assert!(drone_corridor_radius(&drone, s.ends_at_ms(), 1000).is_none());
        drone.updated_at_ms = 1000;
        drone.velocity[0] = f64::NAN;
        assert!(drone_corridor_radius(&drone, s.ends_at_ms(), 1000).is_none());
        assert!(!sphere_route_clear(&s, 57., [f64::NAN, 0., 0.], 1.));
    }
    #[test]
    fn corridor_retained_path_interior_and_rotation_are_conservatively_retained() {
        use arkline_protocol::drone_bay_geometry::BayPose;
        let s = segment();
        let parent = BayPose {
            position_m: [0.; 3],
            orientation_xyzw: [0., 0., 0., 1.],
        };
        let pose = |position_m| BayPose {
            position_m,
            ..parent
        };
        assert!(!retained_corridor_route_clear(
            &s,
            57.,
            parent,
            [
                pose([125., 500., 0.]),
                pose([125., 0., 0.]),
                pose([125., 500., 0.])
            ]
            .into_iter()
        ));
        assert!(!retained_corridor_route_clear(
            &s,
            57.,
            parent,
            [pose([-200., 0., 0.]), pose([450., 0., 0.])].into_iter()
        ));
        assert!(retained_corridor_route_clear(
            &s,
            57.,
            parent,
            [pose([0., 500., 0.]), pose([250., 500., 0.])].into_iter()
        ));
        assert!(!retained_corridor_route_clear(
            &s,
            57.,
            BayPose {
                orientation_xyzw: [0.; 4],
                ..parent
            },
            [pose([0., 500., 0.])].into_iter()
        ));
        assert!(!retained_corridor_route_clear(
            &s,
            57.,
            parent,
            std::iter::empty()
        ));
    }
    #[test]
    fn corridor_peer_curve_covers_initial_tangent_overshoot() {
        let own = segment();
        let mut peer = segment();
        peer.source_m = [125., 500., 0.];
        peer.target_m = [225., 500., 0.];
        assert!(peer_curve_clear(&own, 57., &peer, 44.));
        peer.initial_velocity_mps = [0., -400., 0.];
        // Both endpoints remain500m away, but the accepted curve enters our corridor.
        assert!(peer.sample(peer.started_at_ms + 3333).0[1].abs() < 100.);
        assert!(!peer_curve_clear(&own, 57., &peer, 44.));
        peer.initial_velocity_mps[1] = f64::NAN;
        assert!(!peer_curve_clear(&own, 57., &peer, 44.));
    }
    #[test]
    fn corridor_free_peer_reachable_bound_covers_full_stale_and_future_horizon() {
        let radius = free_peer_radius(44., [0.; 3], 500, 11000, 1000).unwrap();
        assert_eq!(radius, 44. + MAX_SHIP_SPEED * 10.5);
        let overspeed = MAX_SHIP_SPEED * 2.;
        assert!(
            free_peer_radius(44., [overspeed, 0., 0.], 500, 11000, 1000).unwrap()
                >= 44. + overspeed * 10.5
        );
        assert!(free_peer_radius(44., [0.; 3], 1001, 11000, 1000).is_none());
        assert!(free_peer_radius(44., [f64::NAN, 0., 0.], 500, 11000, 1000).is_none());
        assert!(!sphere_route_clear(
            &segment(),
            57.,
            [125., MAX_SHIP_SPEED * 5., 0.],
            radius
        ));
    }
    #[test]
    fn corridor_valid_peer_route_does_not_authorize_invalid_current_pose_or_clock() {
        let own = segment();
        let mut peer = segment();
        peer.source_m = [125., 500., 0.];
        peer.target_m = [225., 500., 0.];
        assert!(peer_curve_clear(&own, 57., &peer, 44.));
        let mut f = FlightState {
            docking: None,
            acceleration: [0.; 3],
            position: peer.source_m,
            velocity: [0.; 3],
            heading: [1., 0., 0.],
            orientation: identity_orientation(),
            angular_velocity: [0.; 3],
            mode: NavigationMode::Stop,
            updated_at_ms: 1000,
            warp: None,
        };
        assert!(peer_current_valid(&f, 1000));
        f.position[0] = f64::NAN;
        assert!(!peer_current_valid(&f, 1000));
        f.position = peer.source_m;
        f.updated_at_ms = 1001;
        assert!(!peer_current_valid(&f, 1000));
        f.updated_at_ms = 1000;
        f.orientation = [0.; 4];
        assert!(!peer_current_valid(&f, 1000));
    }
    #[test]
    fn corridor_sample_fraction_is_safe_at_maximum_clock_and_rejects_invalid_range() {
        let mut previous = 0;
        for i in 0..=100 {
            let at = fraction_time(0, u64::MAX, i).unwrap();
            assert!(at >= previous);
            previous = at;
        }
        assert_eq!(previous, u64::MAX);
        assert_eq!(fraction_time(u64::MAX - 100, 100, 100), Some(u64::MAX));
        assert_eq!(fraction_time(u64::MAX, 1, 100), None);
        assert_eq!(fraction_time(0, u64::MAX, 101), None);
    }
}
