//! Presentation lights from fresh confirmed activity and authored emitting fixtures.
use super::*;

pub(super) const MAX_LIGHTS: usize = 8;
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Light {
    pub(super) position: Vec3,
    pub(super) range: f32,
    pub(super) color: Vec3,
    pub(super) power: f32,
}
#[derive(Default)]
struct Lights {
    entries: [Light; MAX_LIGHTS],
    len: usize,
}
impl Lights {
    fn offer(&mut self, light: Light, eye: Vec3, budget: usize) {
        let budget = budget.min(MAX_LIGHTS);
        if budget == 0
            || !light.position.is_finite()
            || !light.color.is_finite()
            || !light.range.is_finite()
            || !light.power.is_finite()
            || light.range <= 0.0
            || light.power <= 0.0
        {
            return;
        }
        let score = |l: Light| (l.position.distance(eye) - l.range).max(0.0);
        let distance = score(light);
        if distance > 150.0 {
            return;
        }
        let insert = self.entries[..self.len]
            .iter()
            .position(|other| score(*other) > distance)
            .unwrap_or(self.len);
        if insert >= budget {
            return;
        }
        let end = self.len.min(budget - 1);
        for i in (insert..end).rev() {
            self.entries[i + 1] = self.entries[i];
        }
        self.entries[insert] = light;
        self.len = (self.len + 1).min(budget);
    }
}

fn fresh(frame: &Frame) -> bool {
    frame.connected && frame.now.saturating_sub(frame.snapshot_time) <= 1500
}
fn eligible_mining(pilot: &PilotView, asteroid: &Asteroid) -> bool {
    super::super::effects::can_extract_target(pilot, asteroid)
}
fn impact_light(
    source: Vec3,
    asteroid: &Asteroid,
    space: Space,
    now: u64,
    color: Vec3,
) -> Option<Light> {
    let impact = mining_impact(source, asteroid, space, now)?;
    // Lift off the actual triangle along the incoming beam so the surface sees it.
    let outward = (source - impact).normalize_or_zero();
    Some(Light {
        position: impact + outward * 0.08,
        range: 0.9,
        color,
        power: 5.0,
    })
}
/// The canonical body-metre geometry is shared with the docked renderer. A
/// fixture tag alone never creates light: it must also carry emitted output.
/// Keep its existing docked 30-metre range and power after the flight unit change.
fn offer_pioneer_fixtures(frame: &Frame, lights: &mut Lights, budget: usize) {
    if frame.ship_class != ShipClass::Pioneer {
        return;
    }
    let Some(parts) = frame.pioneer_parts.as_ref() else {
        return;
    };
    let mount = frame.pioneer_body_mount();
    for &(mesh, model, color, emission) in parts.iter() {
        if surface_roles::material(mesh) != Some(Material::FixtureDiffuser)
            || !emission.is_finite()
            || emission < 0.7
            || color.min_element() <= 0.25
        {
            continue;
        }
        lights.offer(
            Light {
                position: mount.transform_point3(model.w_axis.truncate()),
                range: 30.0 / 100.0,
                color,
                power: 2.5,
            },
            frame.eye,
            budget,
        );
    }
}

fn collect(
    frame: &Frame,
    turrets: &std::collections::HashMap<(String, u8), TurretState>,
    budget: usize,
) -> Lights {
    let mut lights = Lights::default();
    if !fresh(frame) {
        return lights;
    }
    let budget = budget.min(if frame.drone_camera { 2 } else { MAX_LIGHTS });
    offer_pioneer_fixtures(frame, &mut lights, budget);
    for pilot in &frame.scene.pilots {
        if pilot.system_id != frame.system_id
            || !matches!(pilot.location, Location::Station | Location::Belt)
        {
            continue;
        }
        let position = frame.space.position(pilot, frame.now);
        if position.distance(frame.eye) > 170.0 {
            continue;
        }
        let rotation = ship_orientation_at(pilot, frame.now);
        let pose = Mat4::from_rotation_translation(rotation, position);
        if let Some(binding) = exterior_visuals::for_pilot(frame, pilot) {
            for light in binding.lights(pose) {
                lights.offer(light, frame.eye, budget);
            }
        }
        let acceleration = confirmed_body_acceleration(pilot, frame.now);
        for axis in 0..3 {
            let component = acceleration[axis];
            if component.abs() < 0.01 {
                continue;
            }
            let exhaust = -component.signum() * [Vec3::X, Vec3::Y, Vec3::Z][axis];
            let index = axis * 2 + usize::from(exhaust[axis] < 0.0);
            let Some(mount) = reaction_mounts(pilot.ship_class)[index] else {
                continue;
            };
            let strength =
                (component.abs() / arkline_protocol::SHIP_ACCELERATION as f32).clamp(0.0, 1.0);
            lights.offer(
                Light {
                    position: pose.transform_point3(mount + exhaust * 0.06),
                    range: (pilot.ship_class.spec().width_m as f32 / 200.0).clamp(0.35, 2.0),
                    color: Vec3::new(0.18, 0.48, 1.0),
                    power: 2.5 * strength,
                },
                frame.eye,
                budget,
            );
        }
        for slot in 0..2 {
            let Some(cycle) = super::super::effects::module_cycle(pilot, slot, frame.now) else {
                continue;
            };
            let Some(asteroid) = frame
                .scene
                .asteroids
                .iter()
                .find(|a| a.id == cycle.target_id && eligible_mining(pilot, a))
            else {
                continue;
            };
            let Some(turret) = turrets.get(&(pilot.name.clone(), slot)) else {
                continue;
            };
            let source =
                pose.transform_point3(turret_muzzle(pilot.ship_class, turret.rotation, slot));
            let forward = pose.transform_vector3(turret.rotation * Vec3::X);
            if (frame.space.scene(asteroid.position) - source)
                .normalize_or_zero()
                .dot(forward)
                < 0.995
            {
                continue;
            }
            if let Some(light) = impact_light(
                source,
                asteroid,
                frame.space,
                frame.now,
                if slot == 0 {
                    Vec3::new(0.12, 0.9, 0.48)
                } else {
                    Vec3::new(0.12, 0.55, 1.0)
                },
            ) {
                lights.offer(light, frame.eye, budget);
            }
        }
        for drone in fleet(pilot) {
            let arkline_protocol::DroneMode::Mining { target_id } = drone.mode else {
                continue;
            };
            let Some(cycle) = &drone.mining_cycle else {
                continue;
            };
            if drone.signal_strength <= 0.0
                || frame.now.saturating_sub(drone.updated_at_ms) > 1500
                || (!pilot.locked_asteroids.contains(&target_id)
                    && pilot.locked_asteroid != Some(target_id))
                || frame.now < cycle.started_at_ms
                || cycle.completes_at_ms <= cycle.started_at_ms
            {
                continue;
            }
            let Some(asteroid) = frame
                .scene
                .asteroids
                .iter()
                .find(|a| a.id == target_id && a.system_id == pilot.system_id && a.ore > 0)
            else {
                continue;
            };
            let source = frame.space.scene(drone_present_position(drone, frame.now));
            if (source.distance(frame.space.scene(asteroid.position))
                - asteroid.radius as f32 / 100.0)
                .max(0.0)
                > arkline_protocol::DRONE_MINING_RANGE as f32 / 100.0
            {
                continue;
            }
            if let Some(light) = impact_light(
                source,
                asteroid,
                frame.space,
                frame.now,
                Vec3::new(0.25, 0.85, 0.4),
            ) {
                lights.offer(light, frame.eye, budget);
            }
        }
    }
    lights
}

pub(super) struct Uniforms {
    count: Option<glow::UniformLocation>,
    positions: Option<glow::UniformLocation>,
    colors: Option<glow::UniformLocation>,
}
impl Uniforms {
    pub(super) unsafe fn new(gl: &glow::Context, program: glow::Program) -> Self {
        Self {
            count: gl.get_uniform_location(program, "ark_local_count"),
            positions: gl.get_uniform_location(program, "ark_local_position_range[0]"),
            colors: gl.get_uniform_location(program, "ark_local_color_power[0]"),
        }
    }
    pub(super) unsafe fn upload_station(&self, gl: &glow::Context, lights: &[(Vec3, Vec3)]) {
        self.upload_lights(
            gl,
            lights.iter().map(|(position, color)| Light {
                position: *position,
                range: 30.,
                color: *color,
                power: 2.5,
            }),
        );
    }
    pub(super) unsafe fn upload_lights(
        &self,
        gl: &glow::Context,
        lights: impl IntoIterator<Item = Light>,
    ) {
        let mut positions = [0.; MAX_LIGHTS * 4];
        let mut colors = [0.; MAX_LIGHTS * 4];
        let mut count = 0;
        for (i, light) in lights.into_iter().take(MAX_LIGHTS).enumerate() {
            positions[i * 4..i * 4 + 4].copy_from_slice(&[
                light.position.x,
                light.position.y,
                light.position.z,
                light.range,
            ]);
            colors[i * 4..i * 4 + 4].copy_from_slice(&[
                light.color.x,
                light.color.y,
                light.color.z,
                light.power,
            ]);
            count = i as i32 + 1;
        }
        gl.uniform_1_i32(self.count.as_ref(), count);
        gl.uniform_4_f32_slice(self.positions.as_ref(), &positions);
        gl.uniform_4_f32_slice(self.colors.as_ref(), &colors);
    }
    #[cfg(all(test, target_os = "macos"))]
    pub(super) unsafe fn clear(&self, gl: &glow::Context) {
        gl.uniform_1_i32(self.count.as_ref(), 0);
    }
    pub(super) unsafe fn upload(
        &self,
        gl: &glow::Context,
        frame: &Frame,
        turrets: &std::collections::HashMap<(String, u8), TurretState>,
        budget: usize,
    ) {
        let lights = collect(frame, turrets, budget);
        let mut positions = [0.0; MAX_LIGHTS * 4];
        let mut colors = [0.0; MAX_LIGHTS * 4];
        for (i, light) in lights.entries[..lights.len].iter().enumerate() {
            positions[i * 4..i * 4 + 4].copy_from_slice(&[
                light.position.x,
                light.position.y,
                light.position.z,
                light.range,
            ]);
            colors[i * 4..i * 4 + 4].copy_from_slice(&[
                light.color.x,
                light.color.y,
                light.color.z,
                light.power,
            ]);
        }
        gl.uniform_1_i32(self.count.as_ref(), lights.len as i32);
        gl.uniform_4_f32_slice(self.positions.as_ref(), &positions);
        gl.uniform_4_f32_slice(self.colors.as_ref(), &colors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame() -> Frame {
        Frame {
            pioneer_parts: None,
            exterior: None,
            lighting_quality: Default::default(),
            cockpit_vp: Mat4::IDENTITY,
            cockpit_occlusion: None,
            snapshot_time: 1000,
            planet_vp: Mat4::IDENTITY,
            system_id: 1,
            ship_class: ShipClass::Pioneer,
            drone_camera: false,
            camera_drone: None,
            scene: std::sync::Arc::new(FrameScene {
                world_layout: arkline_protocol::legacy_world_layout(),
                traffic: vec![],
                surface_props: vec![],
                asteroids: vec![],
                gates: vec![],
                pilots: vec![],
            }),
            space: Space::default(),
            cockpit: false,
            rotation: Quat::IDENTITY,
            connected: true,
            vp: Mat4::IDENTITY,
            eye: Vec3::ZERO,
            now: 1000,
            own: "fixture".into(),
            docking: None,
        }
    }
    fn scene_mut(frame: &mut Frame) -> &mut FrameScene {
        std::sync::Arc::make_mut(&mut frame.scene)
    }
    fn pilot() -> PilotView {
        serde_json::from_value(serde_json::json!({"name":"fixture","credits":100,"ore":0,
            "location":Location::Belt,"system_id":1,"locked_asteroids":[1],
            "flight":{"position":[0.0,0.0,0.0],"velocity":[100.0,0.0,0.0],"heading":[1.0,0.0,0.0],
            "mode":NavigationMode::Stop,"updated_at_ms":1000,"acceleration":[0.0,0.0,0.0]}}))
        .unwrap()
    }
    #[test]
    fn nearest_selection_is_bounded_finite_and_range_limited() {
        let mut selected = Lights::default();
        for i in (0..40).rev() {
            selected.offer(
                Light {
                    position: Vec3::X * i as f32,
                    range: 1.0,
                    color: Vec3::ONE,
                    power: 1.0,
                },
                Vec3::ZERO,
                4,
            );
        }
        assert_eq!(selected.len, 4);
        assert!(selected.entries[..4].iter().all(|l| l.position.x <= 3.0));
        let mut invalid = Lights::default();
        for p in [Vec3::splat(f32::NAN), Vec3::X * 1000.0] {
            invalid.offer(
                Light {
                    position: p,
                    range: 1.0,
                    color: Vec3::ONE,
                    power: 1.0,
                },
                Vec3::ZERO,
                8,
            );
        }
        assert_eq!(invalid.len, 0);
    }
    #[test]
    fn canonical_fixture_output_preserves_body_mount_metres_and_freshness() {
        let mut f = frame();
        scene_mut(&mut f).pilots.push(pilot());
        f.rotation = Quat::from_rotation_y(0.5);
        let body_position = Vec3::new(4.0, 2.8, 1.0);
        let fixture = (
            surface_roles::SurfaceRole::FixtureDiffuser.cube(),
            Mat4::from_translation(body_position),
            Vec3::new(0.5, 0.65, 0.55),
            0.7,
        );
        let mut off = fixture;
        off.3 = 0.0;
        let mut screen = fixture;
        screen.0 = surface_roles::SurfaceRole::InstrumentGlass.cube();
        f.pioneer_parts = Some(Arc::new(vec![fixture, off, screen]));
        let turrets = std::collections::HashMap::new();
        let lights = collect(&f, &turrets, 4);
        assert_eq!(lights.len, 1);
        let expected = f.pioneer_body_mount().transform_point3(body_position);
        assert!(lights.entries[0].position.distance(expected) < 1e-6);
        assert!((lights.entries[0].range * 100.0 - 30.0).abs() < 1e-5);
        f.now = 2600;
        assert_eq!(collect(&f, &turrets, 4).len, 0);
        f.now = 1000;
        f.connected = false;
        assert_eq!(collect(&f, &turrets, 4).len, 0);

        // Opt-in diagnostic only: collector CPU time, not GPU cost or frame rate.
        if std::env::var_os("ARKLINE_MEASURE_FIXTURE_LIGHTS").is_some() {
            let parts = hangar::pioneer_docked_scene::default_body_parts()
                .expect("canonical Pioneer geometry");
            f.connected = true;
            let mut measure = |parts: Option<Arc<Vec<ModelPart>>>| {
                f.pioneer_parts = parts;
                let count = collect(&f, &turrets, 8).len;
                let mut samples = [0.0_f64; 7];
                for sample in &mut samples {
                    let start = std::time::Instant::now();
                    for _ in 0..256 {
                        std::hint::black_box(collect(
                            std::hint::black_box(&f),
                            std::hint::black_box(&turrets),
                            8,
                        ));
                    }
                    *sample = start.elapsed().as_secs_f64() * 1_000_000.0 / 256.0;
                }
                samples.sort_by(f64::total_cmp);
                (samples[3], count)
            };
            let (baseline_us, baseline_count) = measure(None);
            let (fixture_us, fixture_count) = measure(Some(parts.clone()));
            eprintln!(
                "fixture collector CPU only: median of 7 batches x 256 calls; \
                 baseline={baseline_us:.3} us/call ({baseline_count} lights), \
                 canonical={fixture_us:.3} us/call ({fixture_count} lights, {} parts); \
                 GPU/FPS cost unmeasured",
                parts.len()
            );
        }
    }

    #[test]
    fn coasting_stale_disconnected_and_docked_cannot_emit_thruster_light() {
        let mut f = frame();
        scene_mut(&mut f).pilots.push(pilot());
        let turrets = std::collections::HashMap::new();
        assert_eq!(collect(&f, &turrets, 8).len, 0);
        scene_mut(&mut f).pilots[0]
            .flight
            .as_mut()
            .unwrap()
            .acceleration = [150.0, 0.0, 0.0];
        assert!(collect(&f, &turrets, 8).len > 0);
        f.connected = false;
        assert_eq!(collect(&f, &turrets, 8).len, 0);
        f.connected = true;
        f.now = 2600;
        assert_eq!(collect(&f, &turrets, 8).len, 0);
        f.now = 1000;
        scene_mut(&mut f).pilots[0].location = Location::Docked;
        assert_eq!(collect(&f, &turrets, 8).len, 0);
    }
    #[test]
    fn mining_needs_confirmed_assignment_lock_range_and_aligned_barrel() {
        let mut f = frame();
        scene_mut(&mut f).pilots.push(pilot());
        scene_mut(&mut f).asteroids.push(Asteroid {
            id: 1,
            system_id: 1,
            name: "fixture rock".into(),
            position: [300.0, 0.0, 0.0],
            radius: 50.0,
            ore: 100,
        });
        let mut turrets = std::collections::HashMap::new();
        let target = (Vec3::X * 3.0 - turret_mount(ShipClass::Pioneer, 0).0).normalize();
        turrets.insert(
            ("fixture".into(), 0),
            TurretState {
                rotation: Quat::from_rotation_arc(Vec3::X, target),
                last_ms: 1000,
            },
        );
        assert_eq!(collect(&f, &turrets, 8).len, 0);
        scene_mut(&mut f).pilots[0].mining_modules=serde_json::from_value(serde_json::json!([{
            "module_id":0,"target_id":1,"cycle_started_at_ms":900,"cycle_completes_at_ms":3000,"stop_reason":null}])).unwrap();
        let lights = collect(&f, &turrets, 8);
        assert_eq!(lights.len, 1);
        assert!(lights.entries[0].position.x > 1.0);
        scene_mut(&mut f).pilots[0].locked_asteroids.clear();
        assert_eq!(collect(&f, &turrets, 8).len, 0);
        scene_mut(&mut f).pilots[0].locked_asteroids.push(1);
        turrets.get_mut(&("fixture".into(), 0)).unwrap().rotation = Quat::from_rotation_y(2.0);
        assert_eq!(collect(&f, &turrets, 8).len, 0);
        scene_mut(&mut f).asteroids[0].ore = 0;
        assert_eq!(collect(&f, &turrets, 8).len, 0);
    }
    #[test]
    fn drone_impact_requires_live_cycle_signal_lock_and_nearby_ore() {
        let mut f = frame();
        let mut p = pilot();
        p.drones=serde_json::from_value(serde_json::json!([{
            "id":9,"class":arkline_protocol::DroneClass::Mining,"hit_points":100,"signal_strength":1.0,
            "position":[200.0,0.0,0.0],"velocity":[0.0,0.0,0.0],"heading":[1.0,0.0,0.0],
            "mode":arkline_protocol::DroneMode::Mining{target_id:1},"updated_at_ms":1000,
            "mining_cycle":{"started_at_ms":900,"completes_at_ms":5000}}])).unwrap();
        scene_mut(&mut f).pilots.push(p);
        scene_mut(&mut f).asteroids.push(Asteroid {
            id: 1,
            system_id: 1,
            name: "rock".into(),
            position: [300.0, 0.0, 0.0],
            radius: 50.0,
            ore: 100,
        });
        let turrets = std::collections::HashMap::new();
        assert_eq!(collect(&f, &turrets, 8).len, 1);
        scene_mut(&mut f).pilots[0].drones[0].signal_strength = 0.0;
        assert_eq!(collect(&f, &turrets, 8).len, 0);
        scene_mut(&mut f).pilots[0].drones[0].signal_strength = 1.0;
        scene_mut(&mut f).pilots[0].drones[0].position = [-5000.0, 0.0, 0.0];
        assert_eq!(collect(&f, &turrets, 8).len, 0);
        scene_mut(&mut f).pilots[0].drones[0].position = [200.0, 0.0, 0.0];
        scene_mut(&mut f).pilots[0].drones[0].mining_cycle = None;
        assert_eq!(collect(&f, &turrets, 8).len, 0);
    }
    #[test]
    fn feeds_limit_uniform_light_budget_to_two() {
        let mut f = frame();
        let mut p = pilot();
        p.flight.as_mut().unwrap().acceleration = [150.0, 150.0, 150.0];
        scene_mut(&mut f).pilots.push(p);
        let turrets = std::collections::HashMap::new();
        assert!(collect(&f, &turrets, 8).len >= 3);
        f.drone_camera = true;
        assert_eq!(collect(&f, &turrets, 8).len, 2);
        assert_eq!(collect(&f, &turrets, 1).len, 1);
        assert_eq!(collect(&f, &turrets, 0).len, 0);
    }
}

#[cfg(all(test, target_os = "macos"))]
#[test]
#[ignore = "Explicit offscreen native local-light shader/readback, no window"]
fn native_local_light_has_bounded_illumination() {
    use std::ffi::{c_void, CString};
    #[link(name = "OpenGL", kind = "framework")]
    unsafe extern "C" {
        fn CGLChoosePixelFormat(a: *const u32, p: *mut *mut c_void, n: *mut i32) -> i32;
        fn CGLCreateContext(p: *mut c_void, s: *mut c_void, c: *mut *mut c_void) -> i32;
        fn CGLSetCurrentContext(c: *mut c_void) -> i32;
        fn CGLDestroyContext(c: *mut c_void) -> i32;
        fn CGLDestroyPixelFormat(p: *mut c_void) -> i32;
    }
    unsafe extern "C" {
        fn dlsym(h: *mut c_void, s: *const std::ffi::c_char) -> *mut c_void;
    }
    unsafe {
        let mut format = std::ptr::null_mut();
        let mut context = std::ptr::null_mut();
        let mut count = 0;
        assert_eq!(
            CGLChoosePixelFormat(
                [99, 0x3200, 73, 8, 24, 12, 24, 0].as_ptr(),
                &mut format,
                &mut count
            ),
            0
        );
        assert_eq!(
            CGLCreateContext(format, std::ptr::null_mut(), &mut context),
            0
        );
        CGLDestroyPixelFormat(format);
        assert_eq!(CGLSetCurrentContext(context), 0);
        let gl = glow::Context::from_loader_function(|name| {
            dlsym(
                (-2_isize) as *mut c_void,
                CString::new(name).unwrap().as_ptr(),
            )
            .cast()
        });
        // Substitute unit BRDF to measure only this module's radiance/window.
        let fragment = concat!(
            "#version 150\nvec3 ark_brdf(vec3 n,vec3 v,vec3 l,vec3 a,float r,float m){return vec3(1.0);}\n",
            include_str!("shaders/local_lighting.glsl"),
            "\nuniform vec3 sample_world;out vec4 color;void main(){color=vec4(ark_local_lighting(sample_world,vec3(0,0,1),vec3(0,0,1),vec3(1),0.5,0.0),1);}"
        );
        let program = compile(&gl, BACK_VERTEX, fragment).unwrap();
        let target = CachedFeed::create(&gl, 123, 8, 8).unwrap();
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
        gl.viewport(0, 0, 8, 8);
        gl.disable(glow::DEPTH_TEST);
        gl.disable(glow::BLEND);
        gl.disable(glow::FRAMEBUFFER_SRGB);
        let vao = gl.create_vertex_array().unwrap();
        gl.bind_vertex_array(Some(vao));
        gl.use_program(Some(program));
        let uniforms = Uniforms::new(&gl, program);
        let mut p = [0.0; MAX_LIGHTS * 4];
        p[3] = 1.0;
        let mut c = [0.0; MAX_LIGHTS * 4];
        c[..4].copy_from_slice(&[0.1, 0.8, 0.3, 1.0]);
        gl.uniform_4_f32_slice(uniforms.positions.as_ref(), &p);
        gl.uniform_4_f32_slice(uniforms.colors.as_ref(), &c);
        let location = gl.get_uniform_location(program, "sample_world");
        let sample = |distance: f32, enabled: i32| {
            gl.uniform_1_i32(uniforms.count.as_ref(), enabled);
            gl.uniform_3_f32(location.as_ref(), distance, 0.0, 0.0);
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            let mut pixel = [0u8; 4];
            gl.read_pixels(
                4,
                4,
                1,
                1,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixel)),
            );
            pixel
        };
        let near = sample(0.1, 1);
        let middle = sample(0.5, 1);
        let outside = sample(1.01, 1);
        let off = sample(0.1, 0);
        assert!(near[1] > middle[1] && middle[1] > 0, "{near:?} {middle:?}");
        assert_eq!(&outside[..3], &[0, 0, 0]);
        assert_eq!(&off[..3], &[0, 0, 0]);
        assert_eq!(gl.get_error(), glow::NO_ERROR);
        target.destroy(&gl);
        gl.delete_vertex_array(vao);
        gl.delete_program(program);
        CGLSetCurrentContext(std::ptr::null_mut());
        CGLDestroyContext(context);
    }
}
