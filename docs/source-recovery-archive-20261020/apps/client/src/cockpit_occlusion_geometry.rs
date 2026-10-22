//! Exact triangle instances for the cockpit depth pass and pointer rejection.
//!
//! These recipes mirror the depth-writing draws in `Renderer::paint`,
//! `paint_living_world`, `paint_environment` and `paint_cockpit`. Keep each
//! transform and visibility condition aligned with the corresponding color draw.
//! The caller freezes the same turret snapshot for input, depth and color.
//!
//! The distant planet sphere is a background: `paint_environment` deliberately
//! clears its depth. Its nearby terrain patch is included here. Additive effects
//! and the warp tunnel disable depth writes and are intentionally excluded.
use super::cockpit_occlusion::{DepthDraw, DepthMeshCatalog};
use super::*;
use std::collections::HashMap;

/// Return (external depth-writing geometry, own fitted cockpit geometry).
/// The retained catalog contains the actual vertices uploaded for every mesh,
/// including rocks, terrain and station batches; no collision proxy is used.
pub(super) fn build(
    frame: &Frame,
    catalog: &DepthMeshCatalog,
    station_batches: &[(usize, Vec3, f32)],
    turrets: &HashMap<(String, u8), TurretState>,
) -> Option<(Vec<DepthDraw>, Vec<DepthDraw>)> {
    Some((
        external_draws(frame, catalog, station_batches, turrets)?,
        own_cockpit_draws(frame, catalog)?,
    ))
}

struct Collector<'a> {
    catalog: &'a DepthMeshCatalog,
    draws: Vec<DepthDraw>,
}

impl<'a> Collector<'a> {
    fn new(catalog: &'a DepthMeshCatalog) -> Self {
        Self {
            catalog,
            draws: Vec::new(),
        }
    }

    fn push(&mut self, mesh: usize, model: Mat4) -> Option<()> {
        self.draws
            .push(DepthDraw::whole(self.catalog, mesh, model)?);
        Some(())
    }

    fn parts(&mut self, parts: impl IntoIterator<Item = ModelPart>) -> Option<()> {
        for (mesh, model, _, _) in parts {
            self.push(mesh, model)?;
        }
        Some(())
    }

    /// All three navigation fixtures always write depth; only their emission
    /// changes with clock/seed. This reproduces world_visuals::navigation_lights.
    fn navigation_lights(&mut self, ship: Mat4, class: ShipClass) -> Option<()> {
        let spec = class.spec();
        let length = spec.length_m as f32 / 100.0;
        let width = spec.width_m as f32 / 100.0;
        let height = spec.height_m as f32 / 100.0;
        let size = (width * 0.012).clamp(0.016, 0.16);
        for side in [-1.0, 1.0] {
            self.push(
                0,
                ship * transform(
                    Vec3::new(length * 0.03, height * 0.24, side * width * 0.505),
                    Vec3::splat(size),
                    Quat::IDENTITY,
                ),
            )?;
        }
        self.push(
            0,
            ship * transform(
                Vec3::new(-length * 0.27, height * 0.505, 0.0),
                Vec3::splat(size * 1.2),
                Quat::IDENTITY,
            ),
        )
    }
}

fn external_draws(
    frame: &Frame,
    catalog: &DepthMeshCatalog,
    station_batches: &[(usize, Vec3, f32)],
    turrets: &HashMap<(String, u8), TurretState>,
) -> Option<Vec<DepthDraw>> {
    let mut out = Collector::new(catalog);
    let space = frame.space;

    // Exact nearby patch from paint_environment, using the same f64 anchor and
    // the uploaded surface_patch mesh (slot 7), never an analytic sphere proxy.
    const PATCH_REFERENCE_RADIUS_M: f64 = 6_000_000.0;
    for planet in frame
        .scene
        .world_layout
        .planets
        .iter()
        .filter(|planet| planet.system_id == frame.system_id)
    {
        let camera = space.world(frame.eye);
        let radial = glam::DVec3::from_array(std::array::from_fn(|i| camera[i] - planet.center[i]));
        if radial.length() - planet.radius_m < 100_000.0 {
            let Some(normal) = radial.try_normalize() else {
                continue;
            };
            let anchor = std::array::from_fn(|i| planet.center[i] + normal[i] * planet.radius_m);
            out.push(
                7,
                transform(
                    space.scene(anchor),
                    Vec3::splat((planet.radius_m / PATCH_REFERENCE_RADIUS_M) as f32),
                    Quat::from_rotation_arc(Vec3::Y, normal.as_vec3()),
                ),
            )?;
        }
    }

    if space.scene(STATION_CENTER_POSITION).distance(frame.eye) < 1100.0 {
        let model = Mat4::from_translation(space.scene(STATION_CENTER_POSITION));
        for &(mesh, _, _) in station_batches {
            out.push(mesh, model)?;
        }
    }
    for asteroid in frame
        .scene
        .asteroids
        .iter()
        .filter(|asteroid| space.scene(asteroid.position).distance(frame.eye) < 1100.0)
    {
        out.push(3, asteroid_model(asteroid, space, frame.now))?;
    }
    for gate in frame
        .scene
        .gates
        .iter()
        .filter(|gate| space.scene(gate.position).distance(frame.eye) < 1100.0)
    {
        for model in gate_models(gate, space) {
            out.push(1, model)?;
        }
    }

    for pilot in &frame.scene.pilots {
        if space.position(pilot, frame.now).distance(frame.eye) > 1100.0 {
            continue;
        }
        let ship = transform(
            space.position(pilot, frame.now),
            Vec3::ONE,
            ship_orientation_at(pilot, frame.now),
        );
        if !frame.cockpit || pilot.name != frame.own {
            for &(mesh, model, _, _) in cached_ship_parts(pilot.ship_class) {
                out.push(mesh, ship * model)?;
            }
        }
        // Own turrets are also drawn before paint_cockpit clears world depth.
        // Include their real instances rather than treating them as hull proxies.
        for module_id in 0..2 {
            if let Some(state) = turrets.get(&(pilot.name.clone(), module_id)) {
                for (mesh, model, _, _) in turret_parts(pilot.ship_class, state.rotation, module_id)
                {
                    out.push(mesh, ship * model)?;
                }
            }
        }
    }

    // The source drone loop has no distance cull. Preserve that exact selection.
    for pilot in &frame.scene.pilots {
        for drone in fleet(pilot) {
            if frame.camera_drone == Some(drone.id) && pilot.name == frame.own {
                continue;
            }
            let base = transform(
                space.scene(drone_present_position(drone, frame.now)),
                Vec3::ONE,
                orientation(drone.heading).unwrap_or(Quat::IDENTITY),
            );
            out.push(
                0,
                base * transform(Vec3::ZERO, Vec3::new(0.025, 0.009, 0.017), Quat::IDENTITY),
            )?;
            for side in [-1.0, 1.0] {
                out.push(
                    0,
                    base * transform(
                        Vec3::new(0.0, 0.0, side * 0.023),
                        Vec3::new(0.012, 0.004, 0.007),
                        Quat::IDENTITY,
                    ),
                )?;
            }
        }
    }

    // These particular beams are drawn with depth writes still enabled in
    // Renderer::paint. They differ from the later additive paint_effects pass.
    if frame.connected {
        for pilot in &frame.scene.pilots {
            for drone in fleet(pilot) {
                if drone.signal_strength <= 0.0
                    || (frame.camera_drone == Some(drone.id) && pilot.name == frame.own)
                {
                    continue;
                }
                let start = space.scene(drone_present_position(drone, frame.now));
                let end = match &drone.mode {
                    arkline_protocol::DroneMode::Mining { target_id }
                        if drone.mining_cycle.is_some() =>
                    {
                        frame
                            .scene
                            .asteroids
                            .iter()
                            .find(|asteroid| asteroid.id == *target_id && asteroid.ore > 0)
                            .map(|asteroid| {
                                let center = space.scene(asteroid.position);
                                let direction = (start - center).normalize_or_zero();
                                center + direction * asteroid.radius as f32 / 100.0
                            })
                    }
                    arkline_protocol::DroneMode::Attacking {
                        target_pilot,
                        target_drone_id,
                    } if frame.now % 1000 < 250 => frame
                        .scene
                        .pilots
                        .iter()
                        .find(|pilot| pilot.name == *target_pilot)
                        .and_then(|pilot| {
                            fleet(pilot)
                                .into_iter()
                                .find(|drone| drone.id == *target_drone_id)
                        })
                        .filter(|drone| {
                            space.scene(drone.position).distance(start)
                                <= arkline_protocol::DRONE_ATTACK_RANGE as f32 / 100.0
                        })
                        .map(|drone| space.scene(drone.position)),
                    _ => None,
                };
                if let Some(model) = end.and_then(|end| segment_model(start, end, 0.009)) {
                    out.push(0, model)?;
                }
            }
        }
    }

    // Exact paint_living_world opaque detail, including its short extrapolation.
    for actor in &frame.scene.traffic {
        let dt = if frame.connected {
            frame.now.saturating_sub(frame.snapshot_time).min(350) as f64 / 1000.0
        } else {
            0.0
        };
        let position = space.scene(std::array::from_fn(|i| {
            actor.position[i] + actor.velocity[i] * dt
        }));
        if position.distance(frame.eye) > 1100.0 {
            continue;
        }
        let ship = transform(
            position,
            Vec3::ONE,
            orientation(actor.heading).unwrap_or(Quat::IDENTITY),
        );
        for &(mesh, model, _, _) in cached_ship_parts(actor.ship_class) {
            out.push(mesh, ship * model)?;
        }
        out.navigation_lights(ship, actor.ship_class)?;
        if actor
            .velocity
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            > 1.0
        {
            let spec = actor.ship_class.spec();
            out.push(
                2,
                ship * transform(
                    Vec3::new(-spec.length_m as f32 * 0.0051, 0.0, 0.0),
                    Vec3::new(0.15, 0.04, 0.04) * (spec.length_m as f32 / 72.0).sqrt(),
                    Quat::IDENTITY,
                ),
            )?;
        }
    }
    for pilot in &frame.scene.pilots {
        let position = space.position(pilot, frame.now);
        if position.distance(frame.eye) > 1100.0 || (frame.cockpit && pilot.name == frame.own) {
            continue;
        }
        let pose = transform(position, Vec3::ONE, ship_orientation_at(pilot, frame.now));
        if let Some(binding) = exterior_visuals::for_pilot(frame, pilot) {
            out.parts(binding.parts(pose))?;
        } else {
            out.navigation_lights(pose, pilot.ship_class)?;
        }
    }
    for prop in &frame.scene.surface_props {
        if space.scene(prop.position).distance(frame.eye) > 1100.0 {
            continue;
        }
        let Some(model) = surface_prop_model(prop, space, &frame.scene.world_layout) else {
            continue;
        };
        out.push(0, model)?;
        out.push(
            0,
            model
                * Mat4::from_translation(Vec3::Y * 1.01)
                * Mat4::from_scale(Vec3::new(0.75, 0.025, 0.75)),
        )?;
    }
    if space.scene(STATION_CENTER_POSITION).distance(frame.eye) < 1100.0 {
        let port = space.scene(STATION_POSITION);
        for side in [-1.0, 1.0] {
            for i in 0..12 {
                out.push(
                    0,
                    transform(
                        port + Vec3::new(1.0 + i as f32 * 1.4, -5.8, side * 6.5),
                        Vec3::new(0.08, 0.045, 0.35),
                        Quat::IDENTITY,
                    ),
                )?;
            }
        }
        let station = space.scene(STATION_CENTER_POSITION);
        for i in 0..8 {
            let angle = i as f32 * std::f32::consts::TAU / 8.0;
            out.push(
                0,
                transform(
                    station + Vec3::new(angle.cos() * 91.0, 16.0, angle.sin() * 91.0),
                    Vec3::splat(0.12),
                    Quat::IDENTITY,
                ),
            )?;
        }
    }
    Some(out.draws)
}

fn own_cockpit_draws(frame: &Frame, catalog: &DepthMeshCatalog) -> Option<Vec<DepthDraw>> {
    if !frame.cockpit {
        return Some(Vec::new());
    }
    let origin = frame
        .scene
        .pilots
        .iter()
        .find(|pilot| pilot.name == frame.own)
        .map_or_else(
            || {
                frame.eye
                    - frame.rotation * (cockpit_fit::eye_body_meters(frame.ship_class) / 100.0)
            },
            |pilot| frame.space.position(pilot, frame.now),
        );
    let base = cockpit_fit::mount(origin, frame.rotation, frame.ship_class);
    let mut out = Collector::new(catalog);
    if frame.ship_class == ShipClass::Pioneer {
        let body = Mat4::from_rotation_translation(frame.rotation, origin);
        for &(mesh, model, _, _) in cached_ship_parts(frame.ship_class) {
            // This is exactly the current paint_cockpit aperture condition.
            if cockpit_fit::canopy_ranges(mesh).is_some_and(|ranges| ranges.is_empty()) {
                continue;
            }
            out.push(mesh, body * model)?;
        }
    }
    for side in [-1.0, 1.0] {
        if frame.ship_class != ShipClass::Pioneer {
            out.push(
                0,
                base * transform(
                    Vec3::new(0.88, 0.03, side * cockpit_span(frame.ship_class)),
                    Vec3::new(0.025, 0.62, 0.018),
                    Quat::from_rotation_x(side * 0.22),
                ),
            )?;
        }
        out.push(
            0,
            base * transform(
                Vec3::new(0.63, -0.38, side * 0.42),
                Vec3::new(0.35, 0.15, 0.13),
                Quat::IDENTITY,
            ),
        )?;
        for i in 0..6 {
            out.push(
                0,
                base * transform(
                    Vec3::new(0.77, -0.34, side * (0.22 + i as f32 * 0.04)),
                    Vec3::splat(0.013),
                    Quat::IDENTITY,
                ),
            )?;
        }
    }
    if frame.ship_class != ShipClass::Pioneer {
        out.push(
            0,
            base * transform(
                Vec3::new(0.75, 0.57, 0.0),
                Vec3::new(0.065, 0.035, 0.57),
                Quat::IDENTITY,
            ),
        )?;
        out.push(
            0,
            base * transform(
                Vec3::new(0.82, -0.50, 0.0),
                Vec3::new(0.22, 0.13, 0.58),
                Quat::IDENTITY,
            ),
        )?;
    }
    for (index, (center, size)) in cockpit_screens(frame.ship_class).into_iter().enumerate() {
        if index == 0 {
            continue;
        }
        out.push(0, base * transform(center, size, Quat::IDENTITY))?;
        out.push(
            0,
            base * transform(
                center - Vec3::X * 0.025,
                Vec3::new(0.006, size.y - 0.008, size.z - 0.008),
                Quat::IDENTITY,
            ),
        )?;
    }
    Some(out.draws)
}
