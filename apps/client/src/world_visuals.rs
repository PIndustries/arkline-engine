//! Meter-scaled world detail. Traffic and construction objects come from snapshots.
use super::*;
use arkline_protocol::{planets, SurfaceProp, TrafficActor};

pub(super) fn cockpit_span(class: ShipClass) -> f32 {
    0.74 + f32::from(class.spec().mining_tier.unwrap_or(2)) * 0.025
}

// A spherical tangent patch shares the server's ground radius. It adds visual
// resolution close to the ground without inventing collision terrain.
pub(super) fn surface_patch() -> Vec<Vertex> {
    let radius = arkline_protocol::PLANET_RADIUS_M / 100.0;
    let point = |x: usize, z: usize| {
        let x = (x as f64 / 96.0 - 0.5) * 2400.0;
        let z = (z as f64 / 96.0 - 0.5) * 2400.0;
        let y = (radius * radius - x * x - z * z).sqrt();
        Vertex {
            p: Vec3::new(x as f32, (y - radius) as f32, z as f32),
            n: Vec3::new(
                (x / radius) as f32,
                (y / radius) as f32,
                (z / radius) as f32,
            ),
        }
    };
    let mut out = Vec::with_capacity(96 * 96 * 6);
    for x in 0..96 {
        for z in 0..96 {
            for (a, b) in [
                (x, z),
                (x, z + 1),
                (x + 1, z + 1),
                (x, z),
                (x + 1, z + 1),
                (x + 1, z),
            ] {
                out.push(point(a, b));
            }
        }
    }
    out
}

pub(super) fn surface_prop_model(prop: &SurfaceProp, space: Space) -> Mat4 {
    let normal = planets()
        .iter()
        .find(|p| p.system_id == prop.system_id)
        .map(|p| {
            Vec3::from_array(std::array::from_fn(|i| {
                (prop.position[i] - p.center[i]) as f32
            }))
            .normalize_or_zero()
        })
        .unwrap_or(Vec3::Y);
    transform(
        space.scene(prop.position),
        Vec3::from_array(prop.kind.size().map(|v| v as f32 / 200.0)),
        Quat::from_rotation_arc(Vec3::Y, normal),
    )
}

fn traffic_position(actor: &TrafficActor, frame: &Frame) -> Vec3 {
    let dt = if frame.connected {
        frame.now.saturating_sub(frame.snapshot_time).min(350) as f64 / 1000.0
    } else {
        0.0
    };
    frame.space.scene(std::array::from_fn(|i| {
        actor.position[i] + actor.velocity[i] * dt
    }))
}

impl Renderer {
    pub(super) unsafe fn paint_environment(&self, frame: &Frame) {
        let gl = &self.gl;
        for planet in planets()
            .into_iter()
            .filter(|p| p.system_id == frame.system_id)
        {
            gl.uniform_matrix_4_f32_slice(
                self.uniforms.vp.as_ref(),
                false,
                &frame.planet_vp.to_cols_array(),
            );
            gl.uniform_1_i32(self.uniforms.material.as_ref(), 1);
            self.draw(
                5,
                transform(
                    frame.space.scene(planet.center),
                    Vec3::splat(planet.radius_m as f32 / 100.0),
                    Quat::IDENTITY,
                ),
                Vec3::ONE,
                0.0,
            );
            // Planet is a distant background. Nearby geometry keeps its own
            // high precision depth range rather than using astronomical far-z.
            gl.clear(glow::DEPTH_BUFFER_BIT);
            gl.uniform_matrix_4_f32_slice(
                self.uniforms.vp.as_ref(),
                false,
                &frame.vp.to_cols_array(),
            );
            let camera = frame.space.world(frame.eye);
            let radial =
                glam::DVec3::from_array(std::array::from_fn(|i| camera[i] - planet.center[i]));
            let altitude = radial.length() - planet.radius_m;
            if altitude < 100_000.0 {
                let normal = radial.normalize();
                let anchor =
                    std::array::from_fn(|i| planet.center[i] + normal[i] * planet.radius_m);
                gl.uniform_1_i32(self.uniforms.material.as_ref(), 2);
                gl.uniform_3_f32(
                    self.uniforms.planet_center.as_ref(),
                    frame.space.scene(planet.center).x,
                    frame.space.scene(planet.center).y,
                    frame.space.scene(planet.center).z,
                );
                self.draw(
                    7,
                    transform(
                        frame.space.scene(anchor),
                        Vec3::ONE,
                        Quat::from_rotation_arc(Vec3::Y, normal.as_vec3()),
                    ),
                    Vec3::new(0.22, 0.26, 0.18),
                    0.0,
                );
            }
        }
        gl.uniform_1_i32(self.uniforms.material.as_ref(), 0);
    }

    unsafe fn navigation_lights(&self, ship: Mat4, class: ShipClass, clock: u64, seed: u64) {
        let spec = class.spec();
        let length = spec.length_m as f32 / 100.0;
        let width = spec.width_m as f32 / 100.0;
        let height = spec.height_m as f32 / 100.0;
        let size = (width * 0.012).clamp(0.016, 0.16);
        for (side, color) in [
            (-1.0, Vec3::new(1.0, 0.035, 0.02)),
            (1.0, Vec3::new(0.03, 0.95, 0.36)),
        ] {
            self.draw(
                0,
                ship * transform(
                    Vec3::new(length * 0.03, height * 0.24, side * width * 0.505),
                    Vec3::splat(size),
                    Quat::IDENTITY,
                ),
                color,
                2.2,
            );
        }
        let phase = (clock.wrapping_add(seed * 97) % 1600) as f32 / 1600.0;
        let flash = if phase < 0.08 || (0.15..0.22).contains(&phase) {
            3.2
        } else {
            0.15
        };
        self.draw(
            0,
            ship * transform(
                Vec3::new(-length * 0.27, height * 0.505, 0.0),
                Vec3::splat(size * 1.2),
                Quat::IDENTITY,
            ),
            Vec3::new(0.8, 0.88, 1.0),
            flash,
        );
    }

    pub(super) unsafe fn paint_living_world(&self, frame: &Frame) {
        for actor in &frame.scene.traffic {
            let pos = traffic_position(actor, frame);
            if pos.distance(frame.eye) > 1100.0 {
                continue;
            }
            let ship = transform(
                pos,
                Vec3::ONE,
                orientation(actor.heading).unwrap_or(Quat::IDENTITY),
            );
            for (mesh, part, color, emission) in ship_parts(actor.ship_class) {
                self.draw(mesh, ship * part, color, emission);
            }
            self.navigation_lights(ship, actor.ship_class, frame.now, actor.id);
            if actor.velocity.iter().map(|v| v * v).sum::<f64>() > 1.0 {
                let spec = actor.ship_class.spec();
                self.draw(
                    2,
                    ship * transform(
                        Vec3::new(-spec.length_m as f32 * 0.0051, 0.0, 0.0),
                        Vec3::new(0.15, 0.04, 0.04) * (spec.length_m as f32 / 72.0).sqrt(),
                        Quat::IDENTITY,
                    ),
                    Vec3::new(0.1, 0.46, 0.85),
                    2.0,
                );
            }
        }
        for pilot in &frame.scene.pilots {
            let pos = frame.space.position(pilot, frame.now);
            if pos.distance(frame.eye) > 1100.0 || (frame.cockpit && pilot.name == frame.own) {
                continue;
            }
            let pose = transform(pos, Vec3::ONE, ship_orientation_at(pilot, frame.now));
            if let Some(binding) = exterior_visuals::for_pilot(frame, pilot) {
                for (mesh, model, color, emission) in binding.parts(pose) {
                    self.draw(mesh, model, color, emission);
                }
            } else {
                self.navigation_lights(pose, pilot.ship_class, frame.now, 0);
            }
        }
        for prop in &frame.scene.surface_props {
            if frame.space.scene(prop.position).distance(frame.eye) > 1100.0 {
                continue;
            }
            self.draw(
                0,
                surface_prop_model(prop, frame.space),
                Vec3::new(0.17, 0.3, 0.27),
                0.1,
            );
            let mut marker = surface_prop_model(prop, frame.space);
            marker *= Mat4::from_translation(Vec3::Y * 1.01)
                * Mat4::from_scale(Vec3::new(0.75, 0.025, 0.75));
            self.draw(0, marker, Vec3::new(0.13, 0.7, 0.45), 0.7);
        }
        if frame
            .space
            .scene(STATION_CENTER_POSITION)
            .distance(frame.eye)
            < 1100.0
        {
            let port = frame.space.scene(STATION_POSITION);
            let beat = (frame.now % 2400) as f32 / 2400.0;
            for side in [-1.0, 1.0] {
                for i in 0..12 {
                    let p = port + Vec3::new(1.0 + i as f32 * 1.4, -5.8, side * 6.5);
                    let pulse = (beat * 12.0 - i as f32).rem_euclid(12.0) < 2.0;
                    self.draw(
                        0,
                        transform(p, Vec3::new(0.08, 0.045, 0.35), Quat::IDENTITY),
                        Vec3::new(0.15, 0.8, 0.52),
                        if pulse { 2.0 } else { 0.25 },
                    );
                }
            }
            let station = frame.space.scene(STATION_CENTER_POSITION);
            for i in 0..8 {
                let angle = i as f32 * std::f32::consts::TAU / 8.0;
                let p = station + Vec3::new(angle.cos() * 91.0, 16.0, angle.sin() * 91.0);
                let flash = ((frame.now + i * 130) % 2000) < 170;
                self.draw(
                    0,
                    transform(p, Vec3::splat(0.12), Quat::IDENTITY),
                    Vec3::new(1.0, 0.24, 0.045),
                    if flash { 2.5 } else { 0.05 },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn surface_visuals_share_authoritative_spherical_ground() {
        let r = arkline_protocol::PLANET_RADIUS_M as f32 / 100.0;
        for v in surface_patch() {
            assert!(((v.p + Vec3::Y * r).length() - r).abs() < 0.01);
            assert!((v.n.length() - 1.0).abs() < 0.00001);
        }
        let p = arkline_protocol::planets().remove(0);
        for kind in [
            arkline_protocol::SurfacePropKind::Crate,
            arkline_protocol::SurfacePropKind::Pylon,
            arkline_protocol::SurfacePropKind::Platform,
        ] {
            let size = kind.size();
            let prop = SurfaceProp {
                id: 1,
                owner: "test".into(),
                system_id: p.system_id,
                kind,
                position: [
                    p.center[0],
                    p.center[1] + p.radius_m + size[1] * 0.5,
                    p.center[2],
                ],
            };
            let space = Space {
                origin: prop.position,
            };
            let model = surface_prop_model(&prop, space);
            let bottom = space.world(model.transform_point3(-Vec3::Y));
            assert!((bottom[1] - (p.center[1] + p.radius_m)).abs() < 0.01);
        }
    }
}
