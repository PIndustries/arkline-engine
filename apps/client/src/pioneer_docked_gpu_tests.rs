//! Actual registered docked scene in a private CGL context; no desktop window.
use super::*;
use sha2::{Digest, Sha256};
use std::ffi::{c_void, CString};
use std::path::Path;

#[link(name = "OpenGL", kind = "framework")]
unsafe extern "C" {
    fn CGLChoosePixelFormat(a: *const u32, f: *mut *mut c_void, n: *mut i32) -> i32;
    fn CGLCreateContext(f: *mut c_void, s: *mut c_void, c: *mut *mut c_void) -> i32;
    fn CGLGetCurrentContext() -> *mut c_void;
    fn CGLSetCurrentContext(c: *mut c_void) -> i32;
    fn CGLDestroyContext(c: *mut c_void) -> i32;
    fn CGLDestroyPixelFormat(f: *mut c_void) -> i32;
}
unsafe extern "C" {
    fn dlsym(handle: *mut c_void, name: *const std::ffi::c_char) -> *mut c_void;
}

struct PrivateContext {
    current: *mut c_void,
    previous: *mut c_void,
}
impl PrivateContext {
    unsafe fn create() -> (Self, Arc<glow::Context>) {
        let previous = CGLGetCurrentContext();
        let mut format = std::ptr::null_mut();
        let mut count = 0;
        assert_eq!(
            CGLChoosePixelFormat(
                [99, 0x3200, 73, 8, 24, 12, 24, 0].as_ptr(),
                &mut format,
                &mut count
            ),
            0
        );
        assert!(!format.is_null() && count > 0);
        let mut current = std::ptr::null_mut();
        let result = CGLCreateContext(format, std::ptr::null_mut(), &mut current);
        assert_eq!(CGLDestroyPixelFormat(format), 0);
        assert_eq!(result, 0);
        assert!(!current.is_null());
        let guard = Self { current, previous };
        assert_eq!(CGLSetCurrentContext(current), 0);
        let gl = Arc::new(glow::Context::from_loader_function(|name| {
            dlsym(
                (-2_isize) as *mut c_void,
                CString::new(name).unwrap().as_ptr(),
            )
            .cast()
        }));
        (guard, gl)
    }
}
impl Drop for PrivateContext {
    fn drop(&mut self) {
        unsafe {
            let _ = CGLSetCurrentContext(self.previous);
            let _ = CGLDestroyContext(self.current);
        }
    }
}

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;

struct Capture {
    rgba: Vec<u8>,
    occupied: usize,
    depth: Vec<f32>,
    viewport: egui::Rect,
    eye: Vec3,
    direction: (f32, f32),
    metrics: ViewRenderMetrics,
}
struct Harness {
    gl: Arc<glow::Context>,
    view: HangarView,
    ctx: egui::Context,
    painter: egui_glow::Painter,
    target: Option<CachedFeed>,
    time: f64,
    modifiers: egui::Modifiers,
    instruments: bool,
}
impl Harness {
    unsafe fn new(gl: Arc<glow::Context>) -> Self {
        let mut view = HangarView::new(Some(&gl));
        assert!(view.error.is_none());
        view.set_docked_traversal_enabled(true);
        Self {
            target: Some(CachedFeed::create(&gl, 9020, WIDTH as i32, HEIGHT as i32).unwrap()),
            painter: egui_glow::Painter::new(gl.clone(), "", None, false).unwrap(),
            gl,
            view,
            ctx: egui::Context::default(),
            time: 0.0,
            modifiers: egui::Modifiers::NONE,
            instruments: true,
        }
    }
    unsafe fn frame(&mut self, pilot: &PilotView, now: u64, events: Vec<egui::Event>) -> Capture {
        self.time += 0.1;
        if let Some(modifiers) = events.iter().rev().find_map(|event| match event {
            egui::Event::Key { modifiers, .. } => Some(*modifiers),
            _ => None,
        }) {
            self.modifiers = modifiers;
        }
        let snapshot:Snapshot=serde_json::from_value(serde_json::json!({"server_time_ms":now,"pilots":[pilot],"docked_traversal_capability":arkline_protocol::docked_traversal_capability(),"orders":[],"belt_ore":0,"ledger_sequence":0,"ledger_hash":"native-render-fixture"})).unwrap();
        let output = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(WIDTH as f32, HEIGHT as f32),
                )),
                time: Some(self.time),
                predicted_dt: 0.1,
                focused: true,
                modifiers: self.modifiers,
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.view.ui(ui, pilot, now, true, None, None, None, now);
                    if self.instruments {
                        self.view
                            .paint_docked_instruments(ui, pilot, &snapshot, now, true);
                    }
                });
            },
        );
        assert!(
            output
                .shapes
                .iter()
                .any(|shape| matches!(shape.shape, egui::Shape::Callback(_))),
            "Registered scene must schedule its actual GPU callback"
        );
        let primitives = self
            .ctx
            .tessellate(output.shapes.clone(), output.pixels_per_point);
        let target = self.target.as_ref().unwrap();
        self.gl
            .bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
        self.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
        self.gl.disable(glow::SCISSOR_TEST);
        self.gl.color_mask(true, true, true, true);
        self.gl.depth_mask(true);
        self.gl.clear_depth_f32(1.0);
        self.gl.clear_color(0.0, 0.0, 0.0, 1.0);
        self.gl
            .clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        self.painter.paint_and_update_textures(
            [WIDTH as u32, HEIGHT as u32],
            output.pixels_per_point,
            &primitives,
            &output.textures_delta,
        );
        assert_eq!(
            self.gl.get_error(),
            glow::NO_ERROR,
            "Scene, registered glass and physical displays"
        );
        self.gl
            .bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target.framebuffer));
        let mut rgba = vec![0; WIDTH * HEIGHT * 4];
        self.gl.read_pixels(
            0,
            0,
            WIDTH as i32,
            HEIGHT as i32,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut rgba)),
        );
        let mut depths = vec![0; WIDTH * HEIGHT * 4];
        self.gl.read_pixels(
            0,
            0,
            WIDTH as i32,
            HEIGHT as i32,
            glow::DEPTH_COMPONENT,
            glow::FLOAT,
            glow::PixelPackData::Slice(Some(&mut depths)),
        );
        assert_eq!(self.gl.get_error(), glow::NO_ERROR);
        let depth = depths
            .chunks_exact(4)
            .map(|bytes| {
                let value = f32::from_ne_bytes(bytes.try_into().unwrap());
                assert!(value.is_finite() && (0.0..=1.0).contains(&value));
                value
            })
            .collect::<Vec<_>>();
        let occupied = depth.iter().filter(|value| **value < 1.0 - 1e-7).count();
        let (eye, yaw, pitch) = self.view.editor_camera_pose().unwrap();
        let metrics = self
            .view
            .debug_view_render_metrics()
            .into_iter()
            .find(|sample| sample.key == "docked")
            .unwrap();
        Capture {
            rgba,
            occupied,
            depth,
            viewport: self.view.surface.expect("actual registered canvas").0,
            eye,
            direction: (yaw, pitch),
            metrics,
        }
    }
    unsafe fn settle(&mut self, pilot: &PilotView, now: u64) -> Capture {
        for _ in 0..8 {
            self.frame(pilot, now, vec![]);
        }
        self.frame(pilot, now, vec![])
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.view.destroy();
        self.painter.destroy();
        if let Some(target) = self.target.take() {
            unsafe {
                target.destroy(&self.gl);
            }
        }
    }
}
fn capture(directory: Option<&Path>, name: &str, shot: &Capture) -> serde_json::Value {
    let bytes = shot
        .rgba
        .chunks_exact(WIDTH * 4)
        .rev()
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    let filename = format!("{name}.png");
    if let Some(directory) = directory {
        image::save_buffer_with_format(
            directory.join(&filename),
            &bytes,
            WIDTH as u32,
            HEIGHT as u32,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .unwrap();
    }
    let v = &shot.metrics.visibility;
    serde_json::json!({"png":directory.map(|_|filename),"rgba_sha256":hex::encode(Sha256::digest(bytes)),"width":WIDTH,"height":HEIGHT,"orientation":"top-left RGBA8","occupied_depth_pixels":shot.occupied,"camera_eye_station_m":shot.eye.to_array(),"camera_yaw_pitch":shot.direction,"candidate_draws":v.candidate_draws,"submitted_draws":v.submitted_draws,"candidate_triangles":v.candidate_triangles,"submitted_triangles":v.submitted_triangles,"cpu_prepare_ns":v.cpu_prepare_ns,"cpu_submit_ns":shot.metrics.cpu_submit_ns,"gpu_elapsed_ns":shot.metrics.gpu_elapsed_ns,"gpu_frame":shot.metrics.gpu_frame})
}
fn state_at(
    body: [f64; 3],
    support: arkline_protocol::DockedSupport,
) -> arkline_protocol::DockedTraversalState {
    let pose = PioneerDockedPose::private();
    let document = arkline_protocol::default_pioneer();
    let binding = arkline_protocol::docked_traversal_binding(
        "native-pioneer-fixture".into(),
        &document,
        arkline_protocol::FleetLayoutRevision::LinearV1,
        false,
    );
    arkline_protocol::DockedTraversalState {
        sprinting: false,
        version: arkline_protocol::DOCKED_TRAVERSAL_VERSION,
        binding: binding.clone(),
        feet_m: body_to_station(pose, body),
        heading: pose.yaw,
        support,
        surface_id: "native-render-fixture".into(),
        moving: false,
        grounded: true,
        vertical_velocity_mps: 0.0,
        updated_at_ms: 1000,
        seat_transition: None,
        actuators: arkline_protocol::DockedVesselActuators {
            binding,
            inner_hatch_open: false,
            outer_hatch_open: false,
            lift: arkline_protocol::DockedLiftState {
                floor_station_m: arkline_protocol::lift_geometry(pose).ship_floor_m,
                target: arkline_protocol::DockedLiftStop::Ship,
                moving: false,
                updated_at_ms: 1000,
            },
            service_power_available: true,
        },
    }
}
/// These are real floor/tread positions, not markers added to the scene.
/// Match the depth readback against their world-space positions so a nearby
/// bulkhead cannot pass merely by filling the framebuffer with opaque pixels.
fn useful_registered_view(shot: &Capture, landmarks_body: &[[f64; 3]]) -> serde_json::Value {
    let (yaw, pitch) = shot.direction;
    let vp = view_projection(
        shot.eye,
        shot.eye + editor_camera_direction(yaw, pitch),
        shot.viewport.aspect_ratio(),
    );
    let inverse = vp.inverse();
    let mut visible = Vec::new();
    for body in landmarks_body {
        let point = vector(body_to_station(PioneerDockedPose::private(), *body));
        let clip = vp * point.extend(1.0);
        if clip.w <= 0.0 {
            continue;
        }
        let ndc = clip.truncate() / clip.w;
        if ndc.abs().max_element() >= 1.0 {
            continue;
        }
        let x = shot.viewport.min.x + (ndc.x + 1.0) * 0.5 * shot.viewport.width();
        let y = shot.viewport.min.y + (1.0 - ndc.y) * 0.5 * shot.viewport.height();
        let mut best = f32::INFINITY;
        // Permit pixel quantization at an actual surface, not an unrelated
        // depth at the same projected coordinate.
        for dy in -1..=1 {
            for dx in -1..=1 {
                let px = x.floor() as i32 + dx;
                let py = y.floor() as i32 + dy;
                if px < 0 || py < 0 || px >= WIDTH as i32 || py >= HEIGHT as i32 {
                    continue;
                }
                let depth = shot.depth[(HEIGHT - 1 - py as usize) * WIDTH + px as usize];
                if depth >= 1.0 - 1e-7 {
                    continue;
                }
                let pixel_ndc = Vec3::new(
                    ((px as f32 + 0.5 - shot.viewport.min.x) / shot.viewport.width()) * 2.0 - 1.0,
                    1.0 - ((py as f32 + 0.5 - shot.viewport.min.y) / shot.viewport.height()) * 2.0,
                    depth * 2.0 - 1.0,
                );
                let world = inverse * pixel_ndc.extend(1.0);
                best = best.min((world.truncate() / world.w).distance(point));
            }
        }
        if best < 0.15 {
            visible.push(serde_json::json!({"body_m":body,"gpu_depth_world_error_m":best}));
        }
    }
    assert!(
        visible.len() >= 2,
        "At least two actual floor/tread landmarks must be visible; got {visible:?}"
    );
    let mut distances = Vec::new();
    let mut colors = std::collections::BTreeSet::new();
    for py in (HEIGHT / 5..HEIGHT * 4 / 5).step_by(3) {
        for px in (WIDTH / 5..WIDTH * 4 / 5).step_by(3) {
            let index = py * WIDTH + px;
            let depth = shot.depth[index];
            if depth < 1.0 - 1e-7 {
                // The actual projection uses OpenGL depth, near .08 / far 30000.
                distances.push(
                    (2.0 * 0.08 * 30000.0)
                        / (30000.0 + 0.08 - (depth * 2.0 - 1.0) * (30000.0 - 0.08)),
                );
                let c = &shot.rgba[index * 4..index * 4 + 3];
                colors.insert([c[0] / 4, c[1] / 4, c[2] / 4]);
            }
        }
    }
    assert!(distances.len() > 100);
    distances.sort_by(f32::total_cmp);
    let near = distances[distances.len() / 10];
    let far = distances[distances.len() * 9 / 10];
    assert!(
        far - near > 0.5,
        "Actual view must contain spatial depth structure: p10={near}, p90={far}"
    );
    assert!(
        colors.len() >= 8,
        "Actual useful view needs spatial color detail beyond an opaque surface"
    );
    serde_json::json!({"visible_actual_landmarks":visible,"landmark_tolerance_m":0.15,"depth_p10_m":near,"depth_p90_m":far,"depth_spread_m":far-near,"quantized_color_bins":colors.len()})
}

fn supported_state_at(
    body: [f64; 3],
    heading: f64,
    support: arkline_protocol::DockedSupport,
) -> arkline_protocol::DockedTraversalState {
    let mut state = state_at(body, support);
    let pose = PioneerDockedPose::private();
    let registered = registered_geometry(&arkline_protocol::default_pioneer()).unwrap();
    let dynamic = dynamic_geometry(pose, actuator_pose(Some(&state), pose, None, 3300)).unwrap();
    let hit = arkline_protocol::support_at_station(
        &registered,
        &dynamic,
        pose,
        state.feet_m,
        Some(support),
        0.0,
    )
    .expect("Supplemental camera must be grounded on actual registered support");
    assert_eq!(hit.support, support);
    assert!((hit.height_m - state.feet_m[1]).abs() < 1e-8);
    assert!(
        arkline_protocol::capsule_clear_station(&registered, &dynamic, pose, state.feet_m),
        "Supplemental feet {body:?} must clear the actual collision geometry"
    );
    state.surface_id = hit.surface_id;
    state.heading = heading;
    state
}

fn pilot(state: Option<arkline_protocol::DockedTraversalState>) -> PilotView {
    let mut pilot:PilotView=serde_json::from_value(serde_json::json!({"name":"NativeDockedPioneer","credits":0,"ore":0,"location":Location::Docked})).unwrap();
    pilot.station_avatar = state.map(|state| StationAvatar {
        sprinting: false,
        docked_traversal: Some(state),
        onboard_interior: false,
        interior_deck: 1,
        jump_started_ms: None,
        seated: false,
        position: arkline_protocol::hangar_layout(ShipClass::Pioneer).boarding_point,
        heading: 0.0,
        moving: false,
        updated_at_ms: 1000,
    });
    pilot
}
unsafe fn source_gpu_hash(gl: &glow::Context, renderer: &Renderer) -> String {
    let previous = gl.get_parameter_buffer(glow::ARRAY_BUFFER_BINDING);
    let mut hash = Sha256::new();
    for mesh in &renderer.meshes[10..19] {
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(mesh.buffer));
        let mut bytes = vec![0; mesh.count as usize * 24];
        gl.get_buffer_sub_data(glow::ARRAY_BUFFER, 0, &mut bytes);
        hash.update(bytes);
    }
    gl.bind_buffer(glow::ARRAY_BUFFER, previous);
    assert_eq!(gl.get_error(), glow::NO_ERROR);
    hex::encode(hash.finalize())
}

#[test]
#[ignore = "Explicit registered Pioneer native scene, physical displays, transition and per-view visibility in private macOS CGL"]
fn native_pioneer_registered_docked_scene_displays_continuity_and_visibility() {
    unsafe {
        let directory =
            std::env::var_os("ARKLINE_PIONEER_DOCKED_EVIDENCE").map(std::path::PathBuf::from);
        if let Some(directory) = &directory {
            std::fs::create_dir(directory).unwrap();
        }
        let (_context, gl) = PrivateContext::create();
        let mut harness = Harness::new(gl.clone());
        let renderer = harness.view.renderer.as_ref().unwrap().clone();
        let buffers = renderer
            .lock()
            .unwrap()
            .meshes
            .iter()
            .map(|mesh| (mesh.vao, mesh.buffer, mesh.count))
            .collect::<Vec<_>>();
        let original = source_gpu_hash(&gl, &renderer.lock().unwrap());
        let seated = pilot(None);
        let seated_source = serde_json::to_vec(&seated).unwrap();
        let first = harness.settle(&seated, 1000);
        let mut captures = vec![capture(directory.as_deref(), "pilot-seat", &first)];
        assert!(first.occupied > 1000);
        let surface = harness
            .view
            .computer_surface()
            .expect("Actual fitted primary computer face must project in seated view");
        let canonical = surface.canvas_rect().center();
        let screen = surface.project(canonical).unwrap();
        let recovered = harness.view.computer_hit_tester(&harness.ctx)(screen)
            .expect("Visible physical computer must accept its actual face ray");
        assert!(recovered.distance(canonical) < 0.1);
        assert!(
            first.eye.distance(vector(
                arkline_protocol::seat_anchors(PioneerDockedPose::private()).eye_station_m
            )) < 1e-4
        );
        let live_surface = harness.view.docked_surfaces[1]
            .as_ref()
            .expect("Actual side telemetry backing must project")
            .bounds();
        harness.instruments = false;
        let unpainted = harness.frame(&seated, 1000, vec![]);
        harness.instruments = true;
        let painted = harness.frame(&seated, 1000, vec![]);
        let instrument_pixels = unpainted
            .rgba
            .chunks_exact(4)
            .zip(painted.rgba.chunks_exact(4))
            .enumerate()
            .filter(|(index, (a, b))| {
                let point = egui::pos2(
                    (index % WIDTH) as f32 + 0.5,
                    (HEIGHT - 1 - index / WIDTH) as f32 + 0.5,
                );
                live_surface.contains(point) && a != b
            })
            .count();
        assert!(instrument_pixels>20,"Actual live instrument triangles must render in front of their real opaque backing, not disappear behind it");

        let frame = egui::Event::PointerMoved(egui::pos2(WIDTH as f32 * 0.5, HEIGHT as f32 * 0.5));
        for _ in 0..3 {
            harness.frame(
                &seated,
                1000,
                vec![
                    frame.clone(),
                    egui::Event::Key {
                        key: egui::Key::ArrowLeft,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers {
                            shift: true,
                            ..Default::default()
                        },
                    },
                ],
            );
        }
        let looked = harness.frame(&seated, 1000, vec![]);
        assert!(harness.view.head_look_angles()[0].abs() > 0.01);
        assert!(looked.eye.distance(first.eye) < 1e-4);
        assert_ne!(looked.direction, first.direction);
        captures.push(capture(directory.as_deref(), "pilot-headlook", &looked));
        // Clear only the held test keys; actual geometry/state are untouched.
        harness.frame(
            &seated,
            1000,
            vec![egui::Event::Key {
                key: egui::Key::ArrowLeft,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        for _ in 0..16 {
            harness.frame(&seated, 1000, vec![frame.clone()]);
        }
        assert_eq!(serde_json::to_vec(&seated).unwrap(), seated_source);
        let mut checkpoints = Vec::new();
        for (name, elapsed) in [
            ("seat-folding", 225),
            ("seat-folded", 450),
            ("pilot-standing", 900),
        ] {
            let mut state = state_at(
                [34.54, -0.32, 3.0],
                arkline_protocol::DockedSupport::Cockpit,
            );
            state.seat_transition = Some(arkline_protocol::DockedSeatTransition {
                phase: arkline_protocol::DockedSeatPhase::Leaving,
                started_at_ms: 1000,
                ends_at_ms: 1900,
            });
            checkpoints.push((name, pilot(Some(state)), 1000 + elapsed));
        }
        checkpoints.push((
            "bridge-clear",
            pilot(Some(state_at(
                [34.36, -0.32, 0.0],
                arkline_protocol::DockedSupport::Cockpit,
            ))),
            2000,
        ));
        checkpoints.push((
            "registered-upper-deck",
            pilot(Some(state_at(
                [18.0, -0.32, 0.0],
                arkline_protocol::DockedSupport::Deck2,
            ))),
            2100,
        ));
        checkpoints.push((
            "registered-lower-deck",
            pilot(Some(state_at(
                [18.0, -4.12, 0.0],
                arkline_protocol::DockedSupport::Deck1,
            ))),
            2200,
        ));
        let pose = PioneerDockedPose::private();
        let anchors = arkline_protocol::hatch_anchors(pose);
        let mut inner = state_at(
            [34.1, -0.32, -7.8],
            arkline_protocol::DockedSupport::Connector,
        );
        inner.feet_m = anchors.inner_inside_station_m;
        let inner_center = body_to_station(pose, anchors.inner_bounds.center_m);
        inner.heading =
            (inner_center[2] - inner.feet_m[2]).atan2(inner_center[0] - inner.feet_m[0]);
        checkpoints.push(("inner-hatch-closed", pilot(Some(inner.clone())), 2300));
        inner.actuators.inner_hatch_open = true;
        checkpoints.push(("inner-hatch-open", pilot(Some(inner.clone())), 2300));
        inner.feet_m = anchors.inner_outside_station_m;
        checkpoints.push(("vestibule", pilot(Some(inner)), 2400));
        let mut outer = state_at(
            [34.1, -0.32, -7.8],
            arkline_protocol::DockedSupport::Connector,
        );
        outer.feet_m = anchors.outer_inside_station_m;
        let outer_center = body_to_station(pose, anchors.outer_bounds.center_m);
        outer.heading =
            (outer_center[2] - outer.feet_m[2]).atan2(outer_center[0] - outer.feet_m[0]);
        checkpoints.push(("outer-hatch-closed", pilot(Some(outer.clone())), 2500));
        outer.actuators.outer_hatch_open = true;
        checkpoints.push(("outer-hatch-open", pilot(Some(outer.clone())), 2500));
        let lift = arkline_protocol::lift_geometry(pose);
        outer.feet_m = lift.center_station_m;
        outer.feet_m[1] = lift.ship_floor_m;
        outer.support = arkline_protocol::DockedSupport::BoardingLift;
        checkpoints.push(("lift-ship", pilot(Some(outer.clone())), 2600));
        outer.actuators.outer_hatch_open = false;
        outer.actuators.lift.floor_station_m = lift.ship_floor_m * 0.5;
        outer.feet_m[1] = lift.ship_floor_m * 0.5;
        checkpoints.push(("lift-mid", pilot(Some(outer.clone())), 2700));
        outer.actuators.lift.floor_station_m = 0.0;
        outer.feet_m[1] = 0.0;
        checkpoints.push(("lift-station", pilot(Some(outer.clone())), 2800));
        // The legacy concourse joins the narrow proved approach at n16.
        // Look back from there, rather than taking a screenshot under the hull.
        outer.feet_m = body_to_station(
            pose,
            [
                32.985 + 16. * 0.61271035,
                -pose.origin_m[1],
                -9.815 - 16. * 0.79030755,
            ],
        );
        outer.heading =
            (outer_center[2] - outer.feet_m[2]).atan2(outer_center[0] - outer.feet_m[0]);
        outer.support = arkline_protocol::DockedSupport::Station;
        checkpoints.push(("station-lookback", pilot(Some(outer)), 2900));
        let mut closed_hatches = std::collections::BTreeMap::new();
        let mut hatch_changed_pixels = std::collections::BTreeMap::new();
        for (name, pilot, now) in checkpoints {
            let source = serde_json::to_vec(&pilot).unwrap();
            let shot = harness.settle(&pilot, now);
            if let Some(prefix) = name.strip_suffix("-hatch-closed") {
                closed_hatches.insert(prefix, shot.rgba.clone());
            }
            if let Some(prefix) = name.strip_suffix("-hatch-open") {
                let before = closed_hatches
                    .get(prefix)
                    .expect("closed hatch evidence precedes open");
                let changed = before
                    .chunks_exact(4)
                    .zip(shot.rgba.chunks_exact(4))
                    .filter(|(a, b)| a != b)
                    .count();
                assert!(changed>10000,"{prefix} hatch must be visibly closed/open in this camera; changed {changed} pixels");
                hatch_changed_pixels.insert(prefix, changed);
            }
            assert!(
                shot.occupied > 100,
                "{name}: actual scene must remain visible"
            );
            assert!(shot.eye.is_finite());
            assert_eq!(serde_json::to_vec(&pilot).unwrap(), source);
            captures.push(capture(directory.as_deref(), name, &shot));
        }
        // Preserve the original x=18 forward-bulkhead views above. These
        // supplemental poses are on the already proved authority stair route,
        // with actual grounded support/capsule checks rather than guessed eyes.
        let mut useful_views = Vec::new();
        let mut corridor = None;
        for (name, body, heading, pitch, support, landmarks) in [
            (
                "upper-deck-corridor",
                [-7.2, -0.32, 0.0],
                0.0,
                0.15,
                arkline_protocol::DockedSupport::Deck2,
                vec![[-3.0, -0.32, 0.0], [2.0, -0.32, 0.0], [7.0, -0.32, 0.0]],
            ),
            (
                "lower-deck-corridor",
                [-7.2, -4.12, 0.0],
                0.0,
                0.15,
                arkline_protocol::DockedSupport::Deck1,
                vec![[-3.0, -4.12, 0.0], [2.0, -4.12, 0.0], [7.0, -4.12, 0.0]],
            ),
            (
                "lower-core-stairs",
                [-11.1, -4.12, -1.2],
                -std::f64::consts::FRAC_PI_2,
                0.35,
                arkline_protocol::DockedSupport::Deck1,
                vec![
                    [-11.1, -3.55, -2.60],
                    [-11.1, -3.17, -3.24],
                    [-11.1, -2.79, -3.88],
                ],
            ),
            (
                "upper-core-stairs",
                [-8.9, -0.32, -2.6],
                -std::f64::consts::FRAC_PI_2,
                0.65,
                arkline_protocol::DockedSupport::Deck2,
                vec![
                    [-8.9, -0.89, -3.88],
                    [-8.9, -1.27, -4.52],
                    [-8.9, -1.65, -5.16],
                ],
            ),
        ] {
            let state = supported_state_at(body, heading, support);
            let surface_id = state.surface_id.clone();
            let pilot = pilot(Some(state));
            let unchanged = serde_json::to_vec(&pilot).unwrap();
            harness.view.pitch = pitch;
            let shot = harness.settle(&pilot, 3300);
            let details = useful_registered_view(&shot, &landmarks);
            assert_eq!(serde_json::to_vec(&pilot).unwrap(), unchanged);
            useful_views.push(serde_json::json!({"view":name,"feet_body_m":body,"surface_id":surface_id,"grounded_and_capsule_clear":true,"visibility":details}));
            captures.push(capture(directory.as_deref(), name, &shot));
            if name == "upper-deck-corridor" {
                corridor = Some(pilot);
            }
        }
        harness.view.pitch = 0.0;
        let standing = pilot(Some(state_at(
            [18.0, -0.32, 0.0],
            arkline_protocol::DockedSupport::Deck2,
        )));
        harness.settle(&standing, 3000);
        let mut comparisons = Vec::new();
        for (name, pilot) in [
            ("seated", seated.clone()),
            ("upper-deck", standing),
            ("supplemental-upper-corridor", corridor.unwrap()),
        ] {
            harness.settle(&pilot, 3100);
            let follow = harness.view.follow;
            let heading = harness.view.follow_heading;
            harness.view.docked_disable_culling = true;
            let full = harness.frame(&pilot, 3100, vec![]);
            harness.view.follow = follow;
            harness.view.follow_heading = heading;
            harness.view.docked_disable_culling = false;
            let culled = harness.frame(&pilot, 3100, vec![]);
            assert_eq!(
                full.rgba, culled.rgba,
                "{name}: conservative frustum rejection must preserve actual pixels"
            );
            assert!(
                culled.metrics.visibility.submitted_draws
                    < culled.metrics.visibility.candidate_draws,
                "{name}: offscreen work must actually be omitted"
            );
            comparisons.push(serde_json::json!({"view":name,"exact_pixels_match":true,"full":capture(directory.as_deref(),&format!("{name}-unculled"),&full),"culled":capture(directory.as_deref(),&format!("{name}-culled"),&culled)}));
        }
        let returned = harness.settle(&seated, 3200);
        captures.push(capture(
            directory.as_deref(),
            "return-pilot-seat",
            &returned,
        ));
        assert!(returned.eye.distance(first.eye) < 1e-4);
        assert_eq!(
            buffers,
            renderer
                .lock()
                .unwrap()
                .meshes
                .iter()
                .map(|mesh| (mesh.vao, mesh.buffer, mesh.count))
                .collect::<Vec<_>>()
        );
        assert_eq!(original, source_gpu_hash(&gl, &renderer.lock().unwrap()));
        if let Some(directory) = &directory {
            std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"schema_version":2,"scope":"Actual HangarView::ui registered docked callback and physical instrument surfaces in private macOS CGL; no desktop window","source_glb_sha256":arkline_protocol::PIONEER_DOCKED_SOURCE_SHA256,"registration_revision":arkline_protocol::PIONEER_DOCKED_REGISTRATION_REVISION,"captures":captures,"supplemental_grounded_useful_views":useful_views,"retained_original_forward_bulkhead_views":["registered-upper-deck","registered-lower-deck"],"visibility_comparisons":comparisons,"source_gpu_payload_sha256":original,"canonical_gpu_payload_unchanged":true,"all_buffer_identities_retained":true,"physical_computer_face_roundtrip":true,"actual_instrument_pixels_on_backing":instrument_pixels,"visible_hatch_changed_pixels":hatch_changed_pixels,"headlook_preserves_eye_and_authority":true,"limitations":["Checkpoint snapshots exercise rendering; authority collision, hatch interlocks and continuous walking are verified by separate server tests","Original x=18 deck checkpoints honestly face the forward bulkhead; supplemental grounded corridor/stair views separately verify visible actual floor/tread landmarks", "Both deck checkpoints and supplemental views use one registered scene but do not alone prove the complete stair walk","Each view retains an independent conservative frustum decision; this does not establish portal occlusion or LOD","CPU submit and optional delayed GPU timings are samples, not a performance guarantee or a vsync FPS measurement","No OS focus, desktop input, packaged app launch or engineering approval is established"]})).unwrap()).unwrap();
        }
        drop(harness);
        assert_eq!(gl.get_error(), glow::NO_ERROR);
    }
}
