//! Conservative current-source beam envelope. A clear centerline alone does
//! not authorize a wide effect; client admitted beam radius must match 0.10m.
use super::*;
use arkline_protocol::drone_bay_geometry::BayPose;
use arkline_protocol::parallel_turret_aim::{ray_box, ray_sphere, TurretAim};
const BEAM_RADIUS_M: f64 = 0.10;
const TOLERANCE_M: f64 = 0.001;
pub(super) const EMITTER_OFFSET_M: f64 = 5.285;
fn inverse(pose: BayPose) -> Option<BayPose> {
    pose.valid().then_some(BayPose {
        position_m: [0.; 3],
        orientation_xyzw: [
            -pose.orientation_xyzw[0],
            -pose.orientation_xyzw[1],
            -pose.orientation_xyzw[2],
            pose.orientation_xyzw[3],
        ],
    })
}
fn ray_local(origin: [f64; 3], axis: [f64; 3], pose: BayPose) -> Option<([f64; 3], [f64; 3])> {
    let inv = inverse(pose)?;
    Some((
        inv.transform_vector(std::array::from_fn(|i| origin[i] - pose.position_m[i]))?,
        inv.transform_vector(axis)?,
    ))
}
fn expanded(bounds: [[f64; 3]; 2]) -> [[f64; 3]; 2] {
    [
        bounds[0].map(|v| v - BEAM_RADIUS_M - TOLERANCE_M),
        bounds[1].map(|v| v + BEAM_RADIUS_M + TOLERANCE_M),
    ]
}
fn hits_box(origin: [f64; 3], axis: [f64; 3], bounds: [[f64; 3]; 2], limit: f64) -> bool {
    ray_box(origin, axis, expanded(bounds)).is_some_and(|t| t <= limit + TOLERANCE_M)
}
pub(super) fn target_delta(w: &World, name: &str, slot: usize, target: u64) -> Option<[f64; 3]> {
    let p = &w.pilots.get(name)?.view;
    let f = p.flight.as_ref()?;
    let a = w
        .asteroids
        .iter()
        .find(|a| a.id == target && a.system_id == p.system_id)?;
    let parent = BayPose {
        position_m: f.position,
        orientation_xyzw: f.orientation,
    };
    let local = ray_local(a.position, [1., 0., 0.], parent)?.0;
    let mount = PIONEER_MINING_TURRET_MOUNTS_M.get(slot)?;
    Some(std::array::from_fn(|i| local[i] - mount[i]))
}
/// Returns physical target entry distance, only if every nearer solid is clear.
/// None is obstruction/unavailable, never permission to ignore missing geometry.
pub(super) fn clear_target(
    w: &World,
    name: &str,
    slot: usize,
    aim: &TurretAim,
    now: u64,
) -> Option<f64> {
    if !turret_stow::enabled(w, name) || slot >= 2 || aim.sample_ms != now {
        return None;
    }
    carrier_inventory::world::validate(w).ok()?;
    let s = carrier_inventory::world::state(w, name)?;
    let def = carrier_inventory::world::definition(w).ok()??;
    if !def.has_complete_compound_collision() || s.machinery.len() != def.mechanisms.len() {
        return None;
    }
    let p = &w.pilots.get(name)?.view;
    let f = p.flight.as_ref()?;
    let parent = BayPose {
        position_m: f.position,
        orientation_xyzw: f.orientation,
    };
    let axis = aim.forward();
    let mount = *PIONEER_MINING_TURRET_MOUNTS_M.get(slot)?;
    let origin = std::array::from_fn(|i| mount[i] + EMITTER_OFFSET_M * axis[i]);
    let world_axis = parent.transform_vector(axis)?;
    let offset = parent.transform_vector(origin)?;
    let world_origin = std::array::from_fn(|i| parent.position_m[i] + offset[i]);
    let target = w
        .asteroids
        .iter()
        .find(|a| Some(a.id) == aim.target_id && a.system_id == p.system_id && a.ore > 0)?;
    let entry = ray_sphere(world_origin, world_axis, target.position, target.radius)?;
    // Check conservatively through the target center plane as the displayed
    // closed rock mesh may lie inside its gameplay sphere near face.
    let limit = (0..3)
        .map(|i| (target.position[i] - world_origin[i]) * world_axis[i])
        .sum::<f64>();
    if !entry.is_finite() || entry <= 0. || !limit.is_finite() || limit < entry {
        return None;
    }
    for solid in arkline_protocol::parallel_turret_ray_geometry::fixed_solids(
        &s.inventory.binding.registration,
    )? {
        if hits_box(origin, axis, solid.bounds_m, limit) {
            return None;
        }
    }
    for mechanism in &def.mechanisms {
        let pose = *s.machinery.get(&mechanism.semantic_id)?;
        let (o, d) = ray_local(origin, axis, pose)?;
        for solid in def.mechanism_collision_parts(&mechanism.semantic_id)? {
            if hits_box(o, d, solid.local_bounds_m, limit) {
                return None;
            }
        }
    }
    // Other gun: full proved union conservatively includes every current pose.
    let bounds = arkline_protocol::parallel_turret_binding::sweep(
        &s.inventory.binding.registration,
        1 - slot,
        0.,
        1.,
    )?;
    let other = [
        std::array::from_fn(|i| bounds.center_m[i] - bounds.half_m[i]),
        std::array::from_fn(|i| bounds.center_m[i] + bounds.half_m[i]),
    ];
    if hits_box(origin, axis, other, limit) {
        return None;
    }
    // Active gun's barrel/body/cap all end at local+X5.284m; emitted finite
    // cylinder begins at5.285m, certified beyond that exit plane. Only that
    // behind-exit rigid gun segment is excluded. Pedestal/shutters remain solids.
    let state = w.accounts.get(name)?.cockpit_mechanisms.as_ref()?;
    let fraction = state.turrets.actuators[slot].fraction_at(now);
    if !state.turrets.operational_at(now) {
        return None;
    }
    for side in [-1., 1.] {
        let h = arkline_protocol::turret_stow_geometry::hatch_box(slot, side, fraction)?;
        let b = [
            std::array::from_fn(|i| h.center_m[i] - h.half_m[i]),
            std::array::from_fn(|i| h.center_m[i] + h.half_m[i]),
        ];
        if hits_box(origin, axis, b, limit) {
            return None;
        }
    }
    let pedestal = [
        [mount[0] - 0.6048, 7.08, mount[2] - 0.6048],
        [mount[0] + 0.6048, mount[1], mount[2] + 0.6048],
    ];
    if hits_box(origin, axis, pedestal, limit) {
        return None;
    }
    // Same exact versioned source provider as the exterior consumer; fixtures
    // remain ray solids when dark. Window panes are already in fixed source.
    let exterior = arkline_protocol::parallel_exterior::for_exact_source(
        s.inventory.binding.registration.assembly_revision,
        &s.inventory.binding.registration.assembly_source_sha256,
        &w.exterior_definition,
    )
    .ok()?;
    for fixture in &exterior.document.fixtures {
        let half = match fixture.kind {
            ExteriorFixtureKind::Navigation => [0.18; 3],
            ExteriorFixtureKind::Strobe => [0.24; 3],
            ExteriorFixtureKind::Panel => [0.08, 0.15, 0.25],
            ExteriorFixtureKind::EngineGlow => [0.18, 1.1, 1.1],
            ExteriorFixtureKind::Window => continue,
        };
        let bounds = [
            std::array::from_fn(|i| fixture.position_m[i] - half[i]),
            std::array::from_fn(|i| fixture.position_m[i] + half[i]),
        ];
        if hits_box(origin, axis, bounds, limit) {
            return None;
        }
    }
    for a in &w.asteroids {
        if a.id != target.id
            && a.system_id == p.system_id
            && ray_sphere(
                world_origin,
                world_axis,
                a.position,
                a.radius + BEAM_RADIUS_M + TOLERANCE_M,
            )
            .is_some_and(|t| t <= limit)
        {
            return None;
        }
    }
    for (other_name, other) in &w.pilots {
        if other.view.system_id != p.system_id {
            continue;
        }
        if other_name != name {
            let peer = other.view.flight.as_ref()?;
            let radius = match docking::owner_envelope(w, other_name).ok()? {
                Some(e) => e.origin_radius_m()?,
                None => hull_radius_m(other.view.ship_class),
            };
            if !radius.is_finite()
                || radius <= 0.
                || ray_sphere(
                    world_origin,
                    world_axis,
                    peer.position,
                    radius + BEAM_RADIUS_M + TOLERANCE_M,
                )
                .is_some_and(|t| t <= limit)
            {
                return None;
            }
        }
        for d in &other.view.drones {
            if ray_sphere(
                world_origin,
                world_axis,
                d.position,
                4.008 + BEAM_RADIUS_M + TOLERANCE_M,
            )
            .is_some_and(|t| t <= limit)
            {
                return None;
            }
        }
        if let Some(t) = other
            .view
            .station_avatar
            .as_ref()
            .and_then(|a| a.docked_traversal.as_ref())
        {
            if t.binding.host_vessel_id == s.inventory.binding.vessel_id {
                let pose = docked_binding_pose(&t.binding).ok()?;
                let feet = pose.station_to_body(t.feet_m);
                let center = [feet[0], feet[1] + 0.9, feet[2]];
                if ray_sphere(origin, axis, center, 1.2 + BEAM_RADIUS_M + TOLERANCE_M)
                    .is_some_and(|d| d <= limit)
                {
                    return None;
                }
            }
        }
    }
    Some(entry)
}
