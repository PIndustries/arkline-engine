//! Whole-client native route acceptance against a disposable editor authority.
//! Included under `flight3d::hangar`; no desktop window or OS input is used.
use super::*;
use arkline_protocol::{
    DockedSupport, DockedTraversalRequest, EditorSceneState, Request, Response,
    SceneLayoutRevision, DOCKED_TRAVERSAL_VERSION, PROTOCOL_VERSION,
};
use arkline_server::editor::EditorWorld;
use sha2::{Digest, Sha256};
use std::{
    ffi::{c_void, CString},
    path::{Path, PathBuf},
    sync::mpsc,
};

#[link(name = "OpenGL", kind = "framework")]
unsafe extern "C" {
    fn CGLChoosePixelFormat(a: *const u32, f: *mut *mut c_void, n: *mut i32) -> i32;
    fn CGLCreateContext(f: *mut c_void, s: *mut c_void, c: *mut *mut c_void) -> i32;
    fn CGLGetCurrentContext() -> *mut c_void;
    fn CGLSetCurrentContext(c: *mut c_void) -> i32;
    fn CGLDestroyContext(c: *mut c_void) -> i32;
    fn CGLDestroyPixelFormat(f: *mut c_void) -> i32;
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

const WIDTH: usize = 1500;
const HEIGHT: usize = 960;
const PILOT: &str = "NativePioneerRoutes";

struct Shot {
    frame_wall_ms: f64,
    rgba: Vec<u8>,
    depth: Vec<f32>,
    viewport: Option<egui::Rect>,
    camera: Option<(Vec3, f32, f32)>,
    body_eye: Option<Vec3>,
    surface: Option<CockpitSurface>,
    computer_labels: Vec<(String, egui::Pos2)>,
    registered_route: bool,
    frame: u64,
}
impl Shot {
    fn physical_label(&self, label: &str) -> egui::Pos2 {
        let canonical = self
            .computer_labels
            .iter()
            .find(|(text, _)| text == label)
            .unwrap_or_else(|| panic!("Physical label {label:?} missing"))
            .1;
        self.surface
            .as_ref()
            .expect("Actual physical computer surface")
            .project(canonical)
            .expect("Physical control must project onto visible glass")
    }

    fn occupied(&self) -> usize {
        self.depth
            .iter()
            .filter(|&&value| value < 1.0 - 1e-7)
            .count()
    }
}

struct Harness {
    gl: Arc<glow::Context>,
    ctx: egui::Context,
    cockpit: crate::Cockpit,
    painter: egui_glow::Painter,
    target: Option<CachedFeed>,
    world: Option<EditorWorld>,
    directory: PathBuf,
    evidence_directory: Option<PathBuf>,
    session: Option<String>,
    updates: mpsc::Sender<crate::Update>,
    requests: mpsc::Receiver<Request>,
    input_time: f64,
    modifiers: egui::Modifiers,
    ui_undocks: usize,
    authority_requests: Vec<serde_json::Value>,
}
impl Harness {
    unsafe fn new(gl: Arc<glow::Context>, evidence_directory: Option<PathBuf>) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "arkline-native-pioneer-routes-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut world =
            EditorWorld::open_with_creation_scene(&directory, SceneLayoutRevision::OrbitalV1)
                .unwrap();
        if let Some(evidence) = &evidence_directory {
            std::fs::write(
                evidence.join("fixture.json"),
                serde_json::to_vec_pretty(&authority_fixture(&directory)).unwrap(),
            )
            .unwrap();
        }
        let mut session = None;
        let response = world.handle(
            &mut session,
            Request::Connect {
                supports_pioneer_interior: true,
                supports_docked_traversal_v2: true,
                world_layout_version: 1,
                supports_fleet_hangar: true,
                supports_room_controls: true,
                protocol_version: PROTOCOL_VERSION,
                pilot: PILOT.into(),
                resume_token: None,
            },
        );
        assert!(
            response.ok,
            "Disposable authority Connect: {}",
            response.message
        );
        let ctx = egui::Context::default();
        ctx.style_mut(|style| style.animation_time = 0.0);
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.flight_view = FlightView::new(Some(&gl));
        cockpit.hangar_view = HangarView::new(Some(&gl));
        assert!(cockpit.flight_view.renderer.is_ok() && cockpit.hangar_view.error.is_none());
        cockpit.pilot = PILOT.into();
        cockpit.snapshot = None;
        cockpit.assets = None;
        cockpit.embedded = true;
        cockpit.embedded_input = true;
        cockpit.embedded_paused = false;
        cockpit.focus_on_first_frame = false;
        cockpit.view_mode = crate::ViewMode::Cockpit;
        cockpit.services_open = false;
        cockpit.overview_open = false;
        cockpit.drone_camera = false;
        cockpit.selected_object = crate::SelectedObject::Station;
        cockpit.sound_settings.muted = true;
        let (tx, requests) = mpsc::channel();
        cockpit.commands = Some(tx);
        let (updates, receiver) = mpsc::channel();
        cockpit.updates = Some(receiver);
        updates
            .send(crate::Update::Response(Box::new(response), true))
            .unwrap();
        cockpit.poll();
        assert!(cockpit.verified_docked_binding().is_some());
        let painter = egui_glow::Painter::new(gl.clone(), "", None, false).unwrap();
        let target = CachedFeed::create(&gl, 9040, WIDTH as i32, HEIGHT as i32).unwrap();
        world.play();
        Self {
            gl,
            ctx,
            cockpit,
            painter,
            target: Some(target),
            world: Some(world),
            directory,
            evidence_directory,
            session,
            updates,
            requests,
            input_time: 0.0,
            modifiers: egui::Modifiers::NONE,
            ui_undocks: 0,
            authority_requests: Vec::new(),
        }
    }

    fn accept(&mut self, response: Response, visible: bool) {
        assert!(
            response.ok,
            "Actual authority rejected fixture request: {}",
            response.message
        );
        self.updates
            .send(crate::Update::Response(Box::new(response), visible))
            .unwrap();
        self.cockpit.poll();
    }

    fn refresh(&mut self) {
        let response = self
            .world
            .as_mut()
            .unwrap()
            .handle(&mut self.session, Request::Snapshot);
        self.accept(response, false);
    }

    fn command(&mut self, request: Request) {
        self.authority_requests
            .push(serde_json::to_value(&request).unwrap());
        let response = self
            .world
            .as_mut()
            .unwrap()
            .handle(&mut self.session, request);
        self.accept(response, true);
    }

    fn placement(&mut self, scene: EditorSceneState) {
        let world = self.world.as_mut().unwrap();
        let time = world.time();
        world.set_scene_state(&self.session, scene).unwrap();
        assert_eq!(
            world.time(),
            time,
            "Editor placement itself must preserve simulation time"
        );
        self.refresh();
        self.cockpit.view_mode = crate::ViewMode::Cockpit;
        self.cockpit.hangar_view.on_foot_view = OnFootView::FirstPerson;
        self.cockpit.hangar_view.pitch = 0.0;
    }

    fn traversal(&self) -> arkline_protocol::DockedTraversalState {
        self.cockpit
            .own()
            .unwrap()
            .station_avatar
            .as_ref()
            .unwrap()
            .docked_traversal
            .clone()
            .expect("Editor and gameplay routes must retain canonical v2 embodiment")
    }

    fn advance_time(&mut self, milliseconds: u64) {
        assert_eq!(milliseconds % 50, 0);
        let before = self.world.as_ref().unwrap().time();
        for _ in 0..milliseconds / 50 {
            self.world.as_mut().unwrap().advance(50).unwrap();
        }
        assert_eq!(self.world.as_ref().unwrap().time() - before, milliseconds);
    }

    fn walk_to_body(&mut self, target: [f64; 3]) {
        for _ in 0..240 {
            let state = self.traversal();
            let pose = arkline_protocol::docked_binding_pose(&state.binding).unwrap();
            let station = arkline_protocol::body_to_station(pose, target);
            let dx = station[0] - state.feet_m[0];
            let dz = station[2] - state.feet_m[2];
            let distance = dx.hypot(dz);
            if distance < 0.015 {
                self.command(Request::DockedTraversal {
                    request: DockedTraversalRequest::Walk {
                        sprint: false,
                        binding: state.binding,
                        forward: 0.0,
                        strafe: 0.0,
                        turn: 0.0,
                    },
                });
                return;
            }
            let amount = (distance / (arkline_protocol::WALK_SPEED_MPS * 0.05)).min(1.0);
            let (sin, cos) = state.heading.sin_cos();
            self.command(Request::DockedTraversal {
                request: DockedTraversalRequest::Walk {
                    sprint: false,
                    binding: state.binding,
                    forward: ((dx * cos + dz * sin) / distance * amount).clamp(-1.0, 1.0),
                    strafe: ((-dx * sin + dz * cos) / distance * amount).clamp(-1.0, 1.0),
                    turn: 0.0,
                },
            });
            self.world.as_mut().unwrap().advance(50).unwrap();
            self.refresh();
        }
        panic!(
            "Bounded authority walk stalled toward {target:?}: {:?}",
            self.traversal().feet_m
        );
    }

    fn heading(&mut self, target: f64) {
        for _ in 0..80 {
            let state = self.traversal();
            let delta = (target - state.heading + std::f64::consts::PI)
                .rem_euclid(std::f64::consts::TAU)
                - std::f64::consts::PI;
            let turn = if delta.abs() < 1e-5 {
                0.0
            } else {
                (delta / 0.1).clamp(-1.0, 1.0)
            };
            self.command(Request::DockedTraversal {
                request: DockedTraversalRequest::Walk {
                    sprint: false,
                    binding: state.binding,
                    forward: 0.0,
                    strafe: 0.0,
                    turn,
                },
            });
            if turn == 0.0 {
                return;
            }
            self.world.as_mut().unwrap().advance(50).unwrap();
            self.refresh();
        }
        panic!("Authority heading did not converge");
    }

    unsafe fn frame(&mut self, events: Vec<egui::Event>) -> Shot {
        let frame_start = std::time::Instant::now();
        for event in &events {
            if let egui::Event::Key { modifiers, .. } = event {
                self.modifiers = *modifiers;
            }
        }
        self.world.as_mut().unwrap().advance(50).unwrap();
        self.refresh();
        self.input_time += 0.05;
        let now = self.world.as_ref().unwrap().time();
        self.cockpit.frozen_time = Some(now);
        self.cockpit.timeline_origin = Some(now);
        self.cockpit.observed = crate::Instant::now();
        let authority = serde_json::to_value(&self.cockpit.snapshot).unwrap();
        let output = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(WIDTH as f32, HEIGHT as f32),
                )),
                time: Some(self.input_time),
                predicted_dt: 0.05,
                modifiers: self.modifiers,
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut self.cockpit, ctx, &mut eframe::Frame::_new_kittest()),
        );
        assert_eq!(
            serde_json::to_value(&self.cockpit.snapshot).unwrap(),
            authority,
            "Painting and input must not edit received authority snapshots"
        );
        let registered_route = self.cockpit.in_station_hangar();
        let is_tactical = self.cockpit.view_mode == crate::ViewMode::Tactical;
        let viewport = if registered_route {
            self.cockpit
                .hangar_view
                .input_surface()
                .map(|value| value.0)
        } else {
            self.cockpit
                .flight_view
                .input_surface()
                .map(|value| value.0)
        };
        let camera = if registered_route {
            self.cockpit.hangar_view.editor_camera_pose()
        } else {
            self.cockpit.flight_view.editor_camera_pose()
        };
        let pilot = self.cockpit.own().unwrap();
        let body_eye = camera.map(|(eye, _, _)| {
            if registered_route {
                let binding = self.cockpit.verified_docked_binding().unwrap();
                let pose = arkline_protocol::docked_binding_pose(&binding).unwrap();
                Vec3::from_array(
                    arkline_protocol::station_to_body(pose, eye.to_array().map(f64::from))
                        .map(|v| v as f32),
                )
            } else {
                ship_orientation_at(&pilot, now).inverse()
                    * (eye - self.cockpit.flight_view.space.position(&pilot, now))
                    * 100.0
            }
        });
        let surface = if registered_route {
            self.cockpit.hangar_view.computer_surface()
        } else {
            self.cockpit.flight_view.computer_surface()
        };
        let computer_labels = self.cockpit.computer_ui_labels.clone();
        let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        if !is_tactical {
            assert!(
                primitives
                    .iter()
                    .any(|p| matches!(p.primitive, egui::epaint::Primitive::Callback(_))),
                "Whole-client route must schedule an actual GPU callback"
            );
        }
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
            "Whole-client route composition"
        );
        if !is_tactical {
            let metrics = if registered_route {
                self.cockpit.hangar_view.debug_view_render_metrics()
            } else {
                self.cockpit.flight_view.debug_view_render_metrics()
            };
            let key = if registered_route { "docked" } else { "main" };
            assert!(metrics.iter().any(|sample| sample.key == key && sample.frame == now),
                "This frame must execute {key:?}; old metrics cannot disguise a legacy renderer fallback");
        }
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
        let mut bytes = vec![0; WIDTH * HEIGHT * 4];
        self.gl.read_pixels(
            0,
            0,
            WIDTH as i32,
            HEIGHT as i32,
            glow::DEPTH_COMPONENT,
            glow::FLOAT,
            glow::PixelPackData::Slice(Some(&mut bytes)),
        );
        assert_eq!(
            self.gl.get_error(),
            glow::NO_ERROR,
            "Route colour/depth readback"
        );
        let depth = bytes
            .chunks_exact(4)
            .map(|bytes| {
                let value = f32::from_ne_bytes(bytes.try_into().unwrap());
                assert!(value.is_finite() && (0.0..=1.0).contains(&value));
                value
            })
            .collect();
        let shot = Shot {
            rgba,
            depth,
            viewport,
            camera,
            body_eye,
            surface,
            computer_labels,
            registered_route,
            frame: now,
            frame_wall_ms: frame_start.elapsed().as_secs_f64() * 1000.0,
        };
        let mut requests = self.requests.try_iter().collect::<Vec<_>>();
        if let Some(control) = self.cockpit.controls.take() {
            requests.push(control);
        }
        for request in requests {
            if matches!(request, Request::Undock) {
                self.ui_undocks += 1;
            }
            self.command(request);
        }
        shot
    }

    unsafe fn settle(&mut self) -> Shot {
        for _ in 0..8 {
            self.frame(vec![]);
        }
        self.frame(vec![])
    }

    unsafe fn click(&mut self, point: egui::Pos2) -> Shot {
        self.frame(vec![egui::Event::PointerMoved(point)]);
        self.frame(pointer(point, true));
        self.frame(pointer(point, false))
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.cockpit.updates = None;
        self.cockpit.hangar_view.destroy();
        self.cockpit.flight_view.destroy();
        self.painter.destroy();
        if let Some(target) = self.target.take() {
            unsafe {
                target.destroy(&self.gl);
            }
        }
        drop(self.world.take());
        if self.evidence_directory.is_none() {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
}

fn authority_fixture(directory: &Path) -> serde_json::Value {
    serde_json::json!({"schema_version":1,"pilot":PILOT,
        "directory":directory,"database":directory.join("world.sqlite"),
        "retained_for_evidence":true})
}

fn pointer(point: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(point),
        egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

unsafe fn source_snapshot(
    gl: &glow::Context,
    renderer: &Renderer,
) -> (Vec<(glow::VertexArray, glow::Buffer, i32)>, String) {
    let identities = renderer
        .meshes
        .iter()
        .map(|mesh| (mesh.vao, mesh.buffer, mesh.count))
        .collect();
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
    (identities, hex::encode(hash.finalize()))
}

fn computer_evidence(shot: &Shot) -> serde_json::Value {
    let surface = shot
        .surface
        .as_ref()
        .expect("Seated canonical cockpit must expose its physical computer");
    let canvas = surface.canvas_rect().shrink(8.0);
    let mut lit = 0;
    let mut visible = 0;
    for y in 0..64 {
        for x in 0..96 {
            let canonical = canvas.min
                + canvas.size() * egui::vec2((x as f32 + 0.5) / 96.0, (y as f32 + 0.5) / 64.0);
            let Some(screen) = surface.project(canonical) else {
                continue;
            };
            let Some(recovered) = surface.unproject(screen) else {
                continue;
            };
            assert!(recovered.distance(canonical) < 0.2);
            let (px, py) = (screen.x.floor() as i32, screen.y.floor() as i32);
            if px < 0 || py < 0 || px >= WIDTH as i32 || py >= HEIGHT as i32 {
                continue;
            }
            let index = (HEIGHT - 1 - py as usize) * WIDTH + px as usize;
            if shot.depth[index] < 1.0 - 1e-7 {
                visible += 1;
                lit += usize::from(
                    shot.rgba[index * 4..index * 4 + 3]
                        .iter()
                        .any(|value| *value > 70),
                );
            }
        }
    }
    assert!(
        visible > 100 && lit > 40,
        "Physical screen needs actual depth and lit UI pixels: visible={visible}, lit={lit}"
    );
    assert!(!shot.computer_labels.is_empty());
    let surface = shot.surface.as_ref().unwrap();
    let inset = surface.canvas_rect().shrink(8.0);
    assert!(
        [
            inset.left_top(),
            inset.right_top(),
            inset.left_bottom(),
            inset.right_bottom()
        ]
        .iter()
        .all(|point| surface.project(*point).is_some()),
        "All four interactive screen corners must be visible in the neutral seated view"
    );
    assert!(
        screen_corners(shot)
            .iter()
            .all(|point| point.x >= 0.02 && point.x <= 0.98 && point.y >= 0.02 && point.y <= 0.98),
        "Physical screen needs a visible viewport margin: {:?}",
        screen_corners(shot)
    );

    serde_json::json!({"depth_samples":visible,"lit_samples":lit,"physical_roundtrip":true,
        "projection":screen_corner_evidence(shot)})
}

fn screen_corners(shot: &Shot) -> [egui::Vec2; 4] {
    let surface = shot.surface.as_ref().unwrap();
    let canvas = surface.canvas_rect().shrink(8.0);
    let (local_to_clip, left_top, right_bottom) = surface
        .feed_geometry(canvas)
        .expect("Actual inset physical screen geometry");
    // Compare the same physical corners even when homogeneous clipping removes
    // the screen's bottom edge. Visible glass is tested separately with depth.
    [
        left_top,
        Vec3::new(left_top.x, left_top.y, right_bottom.z),
        Vec3::new(left_top.x, right_bottom.y, left_top.z),
        right_bottom,
    ]
    .map(|point| {
        let clip = local_to_clip * point.extend(1.0);
        assert!(
            clip.is_finite() && clip.w > 1e-6,
            "Physical screen corner must remain finite and in front of the camera: {clip:?}"
        );
        let projected = egui::vec2((clip.x / clip.w + 1.0) * 0.5, (1.0 - clip.y / clip.w) * 0.5);
        assert!(projected.is_finite());
        projected
    })
}

fn screen_corner_evidence(shot: &Shot) -> serde_json::Value {
    let surface = shot.surface.as_ref().unwrap();
    let canvas = surface.canvas_rect().shrink(8.0);
    let clipped = [
        canvas.left_top(),
        canvas.right_top(),
        canvas.left_bottom(),
        canvas.right_bottom(),
    ]
    .map(|point| surface.project(point).is_none());
    serde_json::json!({"canonical_inset_px":8,
        "corner_order":["left_top","right_top","left_bottom","right_bottom"],
        "raw_viewport_normalized_corners":screen_corners(shot).map(|point| [point.x,point.y]),
        "frustum_clipped_corners":clipped})
}

fn same_seated_view(reference: &Shot, actual: &Shot) {
    assert!(reference.body_eye.unwrap().distance(actual.body_eye.unwrap()) < 0.03,
        "Docking and route changes must preserve the canonical seated body-space eye: {:?} versus {:?}", reference.body_eye, actual.body_eye);
    for (a, b) in screen_corners(reference).iter().zip(screen_corners(actual)) {
        assert!(
            (*a - b).length() < 0.015,
            "Shared cockpit screen/FOV changed across routes: {a:?} vs {b:?}"
        );
    }
}

fn floor_evidence(
    shot: &Shot,
    binding: &arkline_protocol::DockedTraversalBinding,
    body_points: &[[f64; 3]],
) -> serde_json::Value {
    assert!(shot.registered_route && shot.occupied() > 1000);
    let viewport = shot.viewport.unwrap();
    let (eye, yaw, pitch) = shot.camera.unwrap();
    let vp = view_projection(
        eye,
        eye + editor_camera_direction(yaw, pitch),
        viewport.aspect_ratio(),
    );
    let inverse = glam::DMat4::from_cols_array(&vp.to_cols_array().map(f64::from)).inverse();
    let pose = arkline_protocol::docked_binding_pose(binding).unwrap();
    let mut matches = Vec::new();
    for body in body_points {
        let point =
            Vec3::from_array(arkline_protocol::body_to_station(pose, *body).map(|v| v as f32));
        let Some(screen) = project_point(vp, viewport, point) else {
            continue;
        };
        let mut best = f64::INFINITY;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (px, py) = (screen.x.floor() as i32 + dx, screen.y.floor() as i32 + dy);
                if px < 0 || py < 0 || px >= WIDTH as i32 || py >= HEIGHT as i32 {
                    continue;
                }
                let depth = shot.depth[(HEIGHT - 1 - py as usize) * WIDTH + px as usize];
                if depth >= 1.0 - 1e-7 {
                    continue;
                }
                let ndc = glam::DVec4::new(
                    f64::from((px as f32 + 0.5 - viewport.min.x) * 2.0 / viewport.width() - 1.0),
                    f64::from(1.0 - (py as f32 + 0.5 - viewport.min.y) * 2.0 / viewport.height()),
                    f64::from(depth) * 2.0 - 1.0,
                    1.0,
                );
                let world = inverse * ndc;
                let actual = world.truncate() / world.w;
                let near = inverse * glam::DVec4::new(ndc.x, ndc.y, -1.0, 1.0);
                let far = inverse * glam::DVec4::new(ndc.x, ndc.y, 1.0, 1.0);
                let origin = near.truncate() / near.w;
                let ray = far.truncate() / far.w - origin;
                if ray.y.abs() < 1e-9 {
                    continue;
                }
                let t = (f64::from(point.y) - origin.y) / ray.y;
                if t <= 0.0 {
                    continue;
                }
                let expected = origin + ray * t;
                // The pixel ray, rather than the unsnapped projected centre,
                // resolves quantization at grazing floor angles. The bounded
                // patch keeps another floor or a distant wall from qualifying.
                if expected.distance(point.as_dvec3()) <= 0.35 {
                    best = best.min(actual.distance(expected));
                }
            }
        }
        if best < 0.03 {
            matches.push(serde_json::json!({"body_m":body,"gpu_plane_ray_error_m":best}));
        }
    }
    assert!(
        matches.len() >= 2,
        "Useful route must expose actual floor landmarks, not a wall-only view: {matches:?}"
    );
    serde_json::json!({"visible_floor_landmarks":matches,"tolerance_m":0.03,"pixel_quantization_patch_radius_m":0.35})
}

fn capture(directory: Option<&Path>, name: &str, shot: &Shot) -> serde_json::Value {
    let bytes = shot
        .rgba
        .chunks_exact(WIDTH * 4)
        .rev()
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    let filename = format!("{name}.png");
    let evidence = serde_json::json!({"name":name,"png":directory.map(|_|filename.clone()),"rgba_sha256":hex::encode(Sha256::digest(&bytes)),
        "frame_wall_ms_including_readback":shot.frame_wall_ms,"width":WIDTH,"height":HEIGHT,"orientation":"top-left RGBA8","authority_frame":shot.frame,
        "registered_route":shot.registered_route,"occupied_depth_pixels":shot.occupied(),
        "body_eye_m":shot.body_eye.map(|eye| eye.to_array()),"computer_surface":shot.surface.is_some(),
        "physical_screen_projection":shot.surface.as_ref().map(|_|screen_corner_evidence(shot))});
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
        std::fs::write(
            directory.join(format!("{name}.json")),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
    }
    evidence
}

fn remote_payload_isolation(snapshot: &arkline_protocol::Snapshot) {
    let own = snapshot
        .pilots
        .iter()
        .find(|pilot| pilot.name == PILOT)
        .unwrap();
    let sentinel = Arc::new(vec![(0, Mat4::IDENTITY, Vec3::ONE, 0.123)]);
    let main = Frame {
        pioneer_parts: Some(sentinel.clone()),
        exterior: snapshot.exterior.clone().map(Arc::new),
        lighting_quality: Default::default(),
        cockpit_vp: Mat4::IDENTITY,
        cockpit_occlusion: None,
        snapshot_time: snapshot.server_time_ms,
        planet_vp: Mat4::IDENTITY,
        system_id: own.system_id,
        ship_class: own.ship_class,
        drone_camera: false,
        camera_drone: None,
        scene: Arc::new(FrameScene {
            world_layout: snapshot.world_layout.clone().unwrap(),
            traffic: snapshot.traffic.clone(),
            surface_props: snapshot.surface_props.clone(),
            asteroids: snapshot.asteroids.clone(),
            gates: arkline_protocol::gates(),
            pilots: snapshot.pilots.clone(),
        }),
        space: Space { origin: [0.0; 3] },
        cockpit: true,
        rotation: Quat::IDENTITY,
        connected: true,
        vp: Mat4::IDENTITY,
        eye: Vec3::ZERO,
        now: snapshot.server_time_ms,
        own: own.name.clone(),
        docking: None,
    };
    let same = remote_ship_frame(&main, snapshot, own, 1.5);
    assert!(Arc::ptr_eq(same.pioneer_parts.as_ref().unwrap(), &sentinel));
    // A different Pioneer owner must resolve its own/default recipe, even
    // though both cameras refer to the same ship class and source catalog.
    let mut other = own.clone();
    other.name = "DifferentPioneerOwner".into();
    let remote = remote_ship_frame(&main, snapshot, &other, 1.5);
    assert!(remote.pioneer_parts.is_none());
    assert!(Arc::ptr_eq(main.pioneer_parts.as_ref().unwrap(), &sentinel));
}

#[test]
#[ignore = "Explicit actual Cockpit GPU route matrix + disposable EditorWorld; private macOS CGL, no OS window"]
fn native_pioneer_editor_gameplay_decks_and_space_share_canonical_views() {
    unsafe {
        let directory = std::env::var_os("ARKLINE_PIONEER_ROUTE_EVIDENCE").map(PathBuf::from);
        if let Some(directory) = &directory {
            std::fs::create_dir(directory).unwrap();
        }
        let (_context, gl) = PrivateContext::create();
        let mut harness = Harness::new(gl.clone(), directory.clone());
        let flight_renderer = harness
            .cockpit
            .flight_view
            .renderer
            .as_ref()
            .unwrap()
            .clone();
        let hangar_renderer = harness
            .cockpit
            .hangar_view
            .renderer
            .as_ref()
            .unwrap()
            .clone();
        let flight_before = source_snapshot(&gl, &flight_renderer.lock().unwrap());
        let hangar_before = source_snapshot(&gl, &hangar_renderer.lock().unwrap());
        let catalog = flight_geometry().unwrap();
        let seated = harness.settle();
        let mut captures = vec![capture(
            directory.as_deref(),
            "docked-cockpit-baseline",
            &seated,
        )];
        let mut cases =
            vec![serde_json::json!({"route":"docked-cockpit","screen":computer_evidence(&seated)})];
        assert!(seated.registered_route);

        // Carry a real physical press across a confirmed editor placement.
        // Releasing the vanished seated surface must not undock the ship.
        let stale = seated.physical_label("UNDOCK");
        harness.frame(vec![egui::Event::PointerMoved(stale)]);
        harness.frame(pointer(stale, true));
        let undocks = harness.ui_undocks;
        harness.placement(EditorSceneState::InteriorDeck2);
        let released = harness.frame(pointer(stale, false));
        captures.push(capture(
            directory.as_deref(),
            "stale-seated-release-after-deck2-placement",
            &released,
        ));
        assert_eq!(harness.ui_undocks, undocks);
        assert_eq!(harness.cockpit.own().unwrap().location, Location::Docked);
        assert!(!harness.cockpit.cockpit_computer.wants_keyboard_input());
        let state = harness.traversal();
        assert_eq!(state.version, DOCKED_TRAVERSAL_VERSION);
        assert_eq!(state.support, DockedSupport::Deck2);
        assert!(state.grounded);
        let deck2_pose = arkline_protocol::docked_binding_pose(&state.binding).unwrap();
        assert!(
            (arkline_protocol::station_to_body(deck2_pose, state.feet_m)[1] + 0.32).abs() < 1e-6
        );
        harness.heading(0.0);
        let deck2 = harness.settle();
        captures.push(capture(
            directory.as_deref(),
            "editor-deck2-registered-corridor",
            &deck2,
        ));
        cases.push(serde_json::json!({"route":"editor-deck2","geometry":floor_evidence(&deck2, &state.binding,
            &[[5.0,-0.32,0.0],[10.0,-0.32,0.0],[15.0,-0.32,0.0]])}));

        // Real held Shift+W feeds authoritative sprint and visible third-person gait.
        harness.cockpit.hangar_view.on_foot_view = OnFootView::ThirdPerson;
        harness.cockpit.hangar_view.pitch = 0.15;
        let before_sprint = harness.traversal().feet_m;
        let pointer = deck2.viewport.unwrap().center();
        harness.frame(vec![
            egui::Event::PointerMoved(pointer),
            egui::Event::Key {
                key: egui::Key::W,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::SHIFT,
            },
        ]);
        for _ in 0..5 {
            harness.frame(vec![]);
        }
        let stride_a = harness.frame(vec![]);
        captures.push(capture(
            directory.as_deref(),
            "third-person-sprint-stride-a",
            &stride_a,
        ));
        for _ in 0..3 {
            harness.frame(vec![]);
        }
        let stride_b = harness.frame(vec![]);
        captures.push(capture(
            directory.as_deref(),
            "third-person-sprint-stride-b",
            &stride_b,
        ));
        let sprint_state = harness.traversal();
        assert!(sprint_state.sprinting && sprint_state.moving);
        assert!(
            Vec3::from_array(sprint_state.feet_m.map(|v| v as f32))
                .distance(Vec3::from_array(before_sprint.map(|v| v as f32)))
                > 2.0
        );
        harness.frame(vec![egui::Event::Key {
            key: egui::Key::W,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        harness.settle();
        assert!(!harness.traversal().sprinting);
        harness.placement(EditorSceneState::InteriorDeck2);
        let document = harness
            .cockpit
            .assets
            .as_ref()
            .unwrap()
            .pioneer_room_layout
            .as_ref()
            .unwrap()
            .document
            .clone();
        let crew = document
            .rooms
            .iter()
            .find(|room| room.deck == 2 && room.source_role.as_deref() == Some("CREW / GALLEY"))
            .expect("Actual crew/galley source room");
        assert!(
            crew.runtime_room_id.is_none(),
            "This walk uses the actual ungated galley doorway, not the connected closed airlock"
        );
        let door_x = crew.door_center_xz_m[0] - 12.0;
        let door_z = crew.door_wall_z() - 7.0;
        let inward =
            (crew.origin_xz_m[1] + crew.clear_lwh_m[1] * 0.5 - crew.door_wall_z()).signum();
        assert!(inward.abs() == 1.0);
        harness.walk_to_body([door_x, -0.32, 0.0]);
        harness.walk_to_body([door_x, -0.32, door_z + inward * 0.45]);
        harness.heading(inward * std::f64::consts::FRAC_PI_2);
        harness.cockpit.hangar_view.pitch = 0.65;
        let galley = harness.settle();
        captures.push(capture(
            directory.as_deref(),
            "gameplay-walk-crew-galley-door",
            &galley,
        ));
        let crew_state = harness.traversal();
        assert_eq!(crew_state.binding, state.binding);
        assert_eq!(crew_state.support, DockedSupport::Deck2);
        cases.push(serde_json::json!({"route":"gameplay-crew-galley-door","room_source_id":crew.source_room_id,
            "geometry":floor_evidence(&galley, &crew_state.binding,
                &[[door_x,-0.32,door_z+inward*1.35],[door_x,-0.32,door_z+inward*1.6],[door_x,-0.32,door_z+inward*2.0]])}));

        harness.walk_to_body([door_x, -0.32, door_z + inward * 2.1]);
        harness.heading(0.0);
        harness.cockpit.hangar_view.pitch = 0.12;
        let galley_inside = harness.settle();
        captures.push(capture(
            directory.as_deref(),
            "gameplay-galley-fitted-furniture",
            &galley_inside,
        ));
        harness.placement(EditorSceneState::InteriorDeck1);
        harness.heading(0.0);
        let deck1_state = harness.traversal();
        assert_eq!(deck1_state.support, DockedSupport::Deck1);
        assert!(deck1_state.grounded);
        let deck1_pose = arkline_protocol::docked_binding_pose(&deck1_state.binding).unwrap();
        assert!(
            (arkline_protocol::station_to_body(deck1_pose, deck1_state.feet_m)[1] + 4.12).abs()
                < 1e-6
        );
        let deck1 = harness.settle();
        captures.push(capture(
            directory.as_deref(),
            "editor-deck1-registered-corridor",
            &deck1,
        ));
        cases.push(serde_json::json!({"route":"editor-deck1","geometry":floor_evidence(&deck1, &deck1_state.binding,
            &[[5.0,-4.12,0.0],[10.0,-4.12,0.0],[15.0,-4.12,0.0]])}));
        harness.placement(EditorSceneState::Cockpit);
        assert!(harness.cockpit.own().unwrap().station_avatar.is_none());
        let returned = harness.settle();
        captures.push(capture(
            directory.as_deref(),
            "editor-return-docked-cockpit",
            &returned,
        ));
        same_seated_view(&seated, &returned);
        cases.push(serde_json::json!({"route":"editor-return-cockpit","screen":computer_evidence(&returned)}));

        let selected = harness.cockpit.selected_object.clone();
        for (name, mode) in [
            ("external", crate::ViewMode::External),
            ("tactical", crate::ViewMode::Tactical),
            ("tactical3d", crate::ViewMode::Tactical3d),
            ("cockpit", crate::ViewMode::Cockpit),
        ] {
            let own = harness.cockpit.own().unwrap();
            let authority = (own.location, own.station_avatar.is_some(), own.ship_class);
            let count = harness.ui_undocks;
            harness.cockpit.view_mode = mode;
            let shot = harness.settle();
            captures.push(capture(
                directory.as_deref(),
                &format!("docked-presentation-{name}"),
                &shot,
            ));
            let own = harness.cockpit.own().unwrap();
            assert_eq!(
                (own.location, own.station_avatar.is_some(), own.ship_class),
                authority
            );
            assert!(harness.cockpit.selected_object == selected);
            assert_eq!(harness.ui_undocks, count);
            if mode == crate::ViewMode::Cockpit {
                same_seated_view(&seated, &shot);
            } else {
                assert!(shot.surface.is_none() && shot.computer_labels.is_empty());
                assert!(!harness.cockpit.cockpit_computer.wants_keyboard_input());
            }
        }

        let ready = harness.settle();
        harness.click(ready.physical_label("UNDOCK"));
        assert_eq!(
            harness.ui_undocks,
            undocks + 1,
            "Actual physical release must execute exactly one authority Undock"
        );
        assert_eq!(harness.cockpit.own().unwrap().location, Location::Station);
        let space = harness.settle();
        captures.push(capture(
            directory.as_deref(),
            "authority-undock-space-cockpit",
            &space,
        ));
        assert!(!space.registered_route);
        same_seated_view(&seated, &space);
        remote_payload_isolation(harness.cockpit.snapshot.as_ref().unwrap());
        cases.push(serde_json::json!({"route":"authority-undock-space-cockpit","screen":computer_evidence(&space)}));
        harness.command(Request::Dock);
        assert_eq!(harness.cockpit.own().unwrap().location, Location::Docked);
        let docked_again = harness.settle();
        captures.push(capture(
            directory.as_deref(),
            "authority-redock-cockpit",
            &docked_again,
        ));
        same_seated_view(&seated, &docked_again);
        cases.push(serde_json::json!({"route":"authority-redock-cockpit","screen":computer_evidence(&docked_again)}));

        assert_eq!(
            flight_before,
            source_snapshot(&gl, &flight_renderer.lock().unwrap()),
            "Flight source buffers changed across routes"
        );
        assert_eq!(
            hangar_before,
            source_snapshot(&gl, &hangar_renderer.lock().unwrap()),
            "Hangar source buffers changed across routes"
        );
        assert!(Arc::ptr_eq(&catalog, &flight_geometry().unwrap()));
        if let Some(directory) = &directory {
            let receipt = serde_json::json!({"schema_version":1,
                "scope":"Actual Cockpit::update, both native GL renderers and actual disposable EditorWorld responses/placements/walk/undock/dock",
                "source_glb_sha256":arkline_protocol::PIONEER_DOCKED_SOURCE_SHA256,
                "canonical_flight_payload_sha256":flight_before.1,"canonical_hangar_payload_sha256":hangar_before.1,
                "both_gpu_payloads_and_buffer_identities_unchanged":true,"shared_catalog_retained":true,
                "remote_worker_does_not_inherit_other_owners_canonical_parts":true,
                "stale_seated_press_after_deck_placement_sends_no_undock":true,"actual_ui_undocks":harness.ui_undocks,
                "seated_body_eye_tolerance_m":0.03,"physical_screen_normalized_corner_tolerance":0.015,
                "authority_fixture":authority_fixture(&harness.directory),
                "cases":cases,"captures":captures,"authority_requests":harness.authority_requests,
                "limitations":["Private CGL acceptance does not prove desktop OS focus/input or packaged app behavior",
                    "Editor placements use the real authority API; this does not click the editor toolbar",
                    "Crew movement uses bounded real authority Walk requests; OS keyboard delivery is separate",
                    "Space traversal uses the supported seated cockpit only; no unsupported space avatar is injected",
                    "Canonical screen corners may be clipped by the viewport; raw projections and clipping masks are recorded",
                    "The disposable world and SQLite are retained at authority_fixture.directory when evidence capture is requested"]});
            std::fs::write(
                directory.join("receipt.json"),
                serde_json::to_vec_pretty(&receipt).unwrap(),
            )
            .unwrap();
        }
        drop(harness);
        assert_eq!(gl.get_error(), glow::NO_ERROR);
    }
}

#[test]
#[ignore = "Explicit native macOS animated hatch and physical boarding journey; disposable EditorWorld"]
fn native_pioneer_animated_hatches_boarding_and_return() {
    unsafe {
        let directory = PathBuf::from(
            std::env::var_os("ARKLINE_PIONEER_HATCH_EVIDENCE").expect("Retained evidence path"),
        );
        std::fs::create_dir(&directory).unwrap();
        let (_context, gl) = PrivateContext::create();
        let mut h = Harness::new(gl, Some(directory.clone()));
        h.settle();
        let binding = h.cockpit.verified_docked_binding().unwrap();
        let pose = arkline_protocol::docked_binding_pose(&binding).unwrap();
        h.command(Request::DockedTraversal {
            request: DockedTraversalRequest::LeaveSeat {
                binding: binding.clone(),
            },
        });
        h.advance_time(900);
        h.refresh();
        for target in [
            [33.36, -0.32, 3.0],
            [33.36, -0.32, 0.0],
            [34.1, -0.32, -0.5],
            [34.1, -0.32, -7.8],
        ] {
            h.walk_to_body(target);
        }
        let a = arkline_protocol::hatch_anchors(pose);
        let lift = arkline_protocol::lift_geometry(pose);
        let body = |point| arkline_protocol::station_to_body(pose, point);
        let facing = (a.inner_bounds.axes[2][2]).atan2(a.inner_bounds.axes[2][0]);
        h.heading(facing);
        let mut captures = vec![capture(Some(&directory), "inner-closed", &h.settle())];
        let mut motion = Vec::new();
        let set = |h: &mut Harness, hatch, open| {
            h.command(Request::DockedTraversal {
                request: DockedTraversalRequest::SetHatch {
                    binding: binding.clone(),
                    hatch,
                    open,
                },
            });
        };
        set(&mut h, arkline_protocol::DockedHatch::Inner, true);
        for step in 1..=5 {
            h.advance_time(250);
            let shot = h.frame(vec![]);
            let fraction = h.traversal().actuators.hatch_fractions()[0];
            assert!((fraction - step as f64 / 5.0).abs() < 1e-8);
            motion.push(serde_json::json!({"step":step,"fraction":fraction,"frame":shot.frame}));
            captures.push(capture(
                Some(&directory),
                &format!("inner-opening-{step}"),
                &shot,
            ));
        }
        assert!(h.traversal().actuators.inner_hatch_open);
        h.walk_to_body(body(a.inner_outside_station_m));
        set(&mut h, arkline_protocol::DockedHatch::Inner, false);
        h.advance_time(1500);
        h.refresh();
        h.walk_to_body(body(a.outer_inside_station_m));
        set(&mut h, arkline_protocol::DockedHatch::Outer, true);
        h.advance_time(1500);
        h.refresh();
        let outer = h.settle();
        captures.push(capture(
            Some(&directory),
            "outer-open-retained-leaves",
            &outer,
        ));
        h.walk_to_body(body(a.outer_outside_station_m));
        set(&mut h, arkline_protocol::DockedHatch::Outer, false);
        h.advance_time(1500);
        h.refresh();
        h.walk_to_body(body([
            lift.center_station_m[0],
            lift.ship_floor_m,
            lift.center_station_m[2],
        ]));
        h.command(Request::DockedTraversal {
            request: DockedTraversalRequest::CallLift {
                binding: binding.clone(),
                stop: arkline_protocol::DockedLiftStop::Station,
            },
        });
        h.advance_time(12000);
        h.refresh();
        assert_eq!(
            h.traversal().actuators.lift.floor_station_m,
            lift.station_floor_m
        );
        let call = arkline_protocol::lift_call_anchors(pose);
        h.walk_to_body(body(call.station_station_m));
        let station = h.settle();
        captures.push(capture(Some(&directory), "station-exit", &station));
        assert_eq!(h.traversal().support, DockedSupport::Station);
        h.walk_to_body(body([
            lift.center_station_m[0],
            lift.station_floor_m,
            lift.center_station_m[2],
        ]));
        h.command(Request::DockedTraversal {
            request: DockedTraversalRequest::CallLift {
                binding: binding.clone(),
                stop: arkline_protocol::DockedLiftStop::Ship,
            },
        });
        h.advance_time(12000);
        h.refresh();
        h.walk_to_body(body(a.outer_outside_station_m));
        set(&mut h, arkline_protocol::DockedHatch::Outer, true);
        h.advance_time(1500);
        h.refresh();
        h.walk_to_body(body(a.outer_inside_station_m));
        set(&mut h, arkline_protocol::DockedHatch::Outer, false);
        h.advance_time(1500);
        h.refresh();
        set(&mut h, arkline_protocol::DockedHatch::Inner, true);
        h.advance_time(1500);
        h.refresh();
        h.walk_to_body(body(a.inner_inside_station_m));
        set(&mut h, arkline_protocol::DockedHatch::Inner, false);
        h.advance_time(1500);
        h.refresh();
        for target in [
            [34.1, -0.32, -0.5],
            [33.36, -0.32, 0.0],
            [33.36, -0.32, 3.0],
            [33.54, -0.32, 3.0],
        ] {
            h.walk_to_body(target);
        }
        h.command(Request::DockedTraversal {
            request: DockedTraversalRequest::TakeSeat {
                binding: binding.clone(),
            },
        });
        h.advance_time(900);
        h.refresh();
        h.cockpit.hangar_view.pitch = 0.0;
        h.cockpit.hangar_view.yaw = pose.yaw as f32;
        h.cockpit.hangar_view.head_look = [0.0; 2];
        let returned = h.settle();
        captures.push(capture(Some(&directory), "returned-reseated", &returned));
        assert!(h.cockpit.own().unwrap().station_avatar.is_none());
        let screen = computer_evidence(&returned);
        let cache =
            pioneer_docked_scene::SceneCache::new(arkline_protocol::default_pioneer()).unwrap();
        let parts = pioneer_docked_scene::seated_body_parts(&cache, None, 0).unwrap();
        std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
            "native_macOS_offscreen":true,"desktop_acceptance":false,"captures":captures,"motion":motion,
            "registered_panels":arkline_protocol::registered_geometry(&arkline_protocol::default_pioneer()).unwrap().panels.len(),"body_draw_parts":parts.len(),
            "source_sha256":arkline_protocol::PIONEER_DOCKED_SOURCE_SHA256,"registration_revision":arkline_protocol::PIONEER_DOCKED_REGISTRATION_REVISION,
            "returned_screen":screen,"authority_requests":h.authority_requests,"fixture":authority_fixture(&h.directory)})).unwrap()).unwrap();
    }
}
