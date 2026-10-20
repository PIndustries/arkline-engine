//! Opt-in actual cockpit renderer evidence in a private macOS CGL context.
//! This creates no window and does not capture or drive the desktop UI.
use super::*;
use sha2::{Digest, Sha256};
use std::ffi::{c_void, CString};
use std::path::{Path, PathBuf};

// Reconstructed checkout: unavailable older edge-suite module is not included.
// The retained native tests below exercise enclosure, physical feed and Shift.

#[link(name = "OpenGL", kind = "framework")]
unsafe extern "C" {
    fn CGLChoosePixelFormat(
        attributes: *const u32,
        format: *mut *mut c_void,
        count: *mut i32,
    ) -> i32;
    fn CGLCreateContext(format: *mut c_void, share: *mut c_void, context: *mut *mut c_void) -> i32;
    fn CGLGetCurrentContext() -> *mut c_void;
    fn CGLSetCurrentContext(context: *mut c_void) -> i32;
    fn CGLDestroyContext(context: *mut c_void) -> i32;
    fn CGLDestroyPixelFormat(format: *mut c_void) -> i32;
}
unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
}

struct PrivateContext {
    context: *mut c_void,
    previous: *mut c_void,
}
impl PrivateContext {
    unsafe fn create() -> (Self, Arc<glow::Context>) {
        let previous = CGLGetCurrentContext();
        let mut format = std::ptr::null_mut();
        let mut count = 0;
        // Existing native tests' core 3.2 / offline-renderer / colour+depth configuration.
        let attributes = [99u32, 0x3200, 73, 8, 24, 12, 24, 0];
        assert_eq!(
            CGLChoosePixelFormat(attributes.as_ptr(), &mut format, &mut count),
            0,
            "private CGL pixel format"
        );
        assert!(!format.is_null() && count > 0);
        let mut context = std::ptr::null_mut();
        let result = CGLCreateContext(format, std::ptr::null_mut(), &mut context);
        assert_eq!(CGLDestroyPixelFormat(format), 0);
        assert_eq!(result, 0, "private CGL context");
        assert!(!context.is_null());
        let guard = Self { context, previous };
        assert_eq!(CGLSetCurrentContext(context), 0);
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
            // Restore a pre-existing context even when an assertion unwinds.
            let _ = CGLSetCurrentContext(self.previous);
            let _ = CGLDestroyContext(self.context);
        }
    }
}

#[derive(Clone, Copy)]
struct ViewCase {
    name: &'static str,
    yaw: f32,
    pitch: f32,
    roll: f32,
    open_window: bool,
}

fn cases() -> [ViewCase; 13] {
    use std::f32::consts::{FRAC_PI_2, PI};
    [
        ViewCase {
            name: "forward",
            yaw: 0.,
            pitch: 0.,
            roll: 0.,
            open_window: true,
        },
        ViewCase {
            // The fitted seat is aft of the wrap panes. A cardinal side look
            // hits the new bay wall; retain this as a strict enclosure case.
            name: "port-bulkhead",
            yaw: FRAC_PI_2,
            pitch: 0.,
            roll: 0.,
            open_window: false,
        },
        ViewCase {
            name: "starboard-bulkhead",
            yaw: -FRAC_PI_2,
            pitch: 0.,
            roll: 0.,
            open_window: false,
        },
        // Aim through the actual source wrap panes, above the instruments,
        // from the unchanged registered seated eye. These are reachable head
        // look angles, not a relocated camera or synthetic opening.
        ViewCase {
            name: "port-window",
            yaw: 65.0_f32.to_radians(),
            pitch: 0.12,
            roll: 0.,
            open_window: true,
        },
        ViewCase {
            name: "starboard-window",
            yaw: -PI / 4.0,
            pitch: 0.25,
            roll: 0.,
            open_window: true,
        },
        ViewCase {
            name: "up",
            yaw: 0.,
            pitch: 1.45,
            roll: 0.,
            open_window: false,
        },
        ViewCase {
            name: "down",
            yaw: 0.,
            pitch: -1.45,
            roll: 0.,
            open_window: false,
        },
        ViewCase {
            name: "aft",
            yaw: PI,
            pitch: 0.,
            roll: 0.,
            open_window: false,
        },
        ViewCase {
            name: "aft-negative-yaw",
            yaw: -PI,
            pitch: 0.,
            roll: 0.,
            open_window: false,
        },
        ViewCase {
            name: "aft-port",
            yaw: PI * 0.75,
            pitch: 0.,
            roll: 0.,
            open_window: false,
        },
        ViewCase {
            name: "aft-starboard",
            yaw: -PI * 0.75,
            pitch: 0.,
            roll: 0.,
            open_window: false,
        },
        ViewCase {
            name: "forward-rolled",
            yaw: 0.,
            pitch: 0.,
            roll: 0.7,
            open_window: true,
        },
        ViewCase {
            name: "aft-rolled",
            yaw: PI,
            pitch: 0.,
            roll: 0.7,
            open_window: false,
        },
    ]
}

fn frame(case: ViewCase, width: usize, height: usize) -> Frame {
    // A real own-pilot presentation, far from unrelated station/planet geometry.
    // The frame's floating origin retains the same precise body-space camera.
    let world_origin = [90_000_000_000.; 3];
    let rotation = Quat::from_rotation_x(case.roll);
    let steps = (case.yaw.abs().max(case.pitch.abs()) / 0.14)
        .ceil()
        .max(1.0) as usize;
    let axes = [
        case.yaw / (steps as f32 * 0.14),
        case.pitch / (steps as f32 * 0.14),
    ];
    let mut head = [0.0; 2];
    for _ in 0..steps {
        update_head_look(&mut head, true, axes, 0.1);
    }
    assert!(
        (head[0] - case.yaw).abs() < 1e-5 && (head[1] - case.pitch).abs() < 1e-5,
        "Enclosure view must be reachable by ordinary seated head-look input"
    );
    let camera = rotation * Quat::from_rotation_y(head[0]) * Quat::from_rotation_z(head[1]);
    let eye = rotation * (cockpit_fit::eye_body_meters(ShipClass::Pioneer) / 100.);
    let view = Mat4::look_at_rh(eye, eye + camera * Vec3::X, camera * Vec3::Y);
    let aspect = width as f32 / height as f32;
    let vp = Mat4::perspective_rh_gl(COCKPIT_FOV, aspect, 0.03, 1200.) * view;
    let cockpit_vp = Mat4::perspective_rh_gl(COCKPIT_FOV, aspect, 0.0002, 10.) * view;
    let pilot: PilotView = serde_json::from_value(serde_json::json!({
        "name":"CockpitEnclosureQA", "credits":0, "ore":0,
        "ship_class":"pioneer", "location":Location::Belt, "system_id":3,
        "flight":{
            "position":world_origin, "velocity":[0.,0.,0.], "heading":[1.,0.,0.],
            "orientation":rotation.to_array(), "mode":NavigationMode::Stop,
            "updated_at_ms":1000, "acceleration":[0.,0.,0.]
        }
    }))
    .unwrap();
    assert!(ship_orientation_at(&pilot, 1000).dot(rotation).abs() > 1. - 1.0e-6);
    Frame {
        pioneer_parts: None,
        exterior: None,
        lighting_quality: super::super::lighting_settings::LightingQuality::Balanced,
        cockpit_vp,
        cockpit_occlusion: None,
        snapshot_time: 1000,
        planet_vp: Mat4::perspective_rh_gl(COCKPIT_FOV, aspect, 1., 1.0e9) * view,
        system_id: 3,
        ship_class: ShipClass::Pioneer,
        drone_camera: false,
        camera_drone: None,
        scene: Arc::new(FrameScene {
            world_layout: arkline_protocol::legacy_world_layout(),
            traffic: vec![],
            surface_props: vec![],
            asteroids: vec![],
            gates: vec![],
            pilots: vec![pilot],
        }),
        space: Space {
            origin: world_origin,
        },
        cockpit: true,
        rotation,
        connected: true,
        vp,
        eye,
        now: 1000,
        own: "CockpitEnclosureQA".into(),
        docking: None,
    }
}

/// Raycast the same retained source-derived and installed construction triangles
/// as the current runtime. The uncut historical rear wall is not a live surface.
fn canonical_hit(frame: &Frame, pointer: egui::Pos2, viewport: egui::Rect) -> Option<f32> {
    let (origin, direction) = screen_ray(frame.cockpit_vp, viewport, pointer).unwrap();
    let geometry = flight_geometry().unwrap();
    let mount = frame.pioneer_body_mount();
    let parts = frame
        .canonical_pioneer_parts()
        .expect("Current canonical Pioneer recipe");
    parts
        .iter()
        .filter(|part| docked_source_primitive(part.0) != Some(6))
        .filter_map(|part| {
            let vertices = &geometry.catalog.uploads()[mesh_index(part.0)];
            ray_mesh_hit(origin, direction, mount * part.1, vertices)
        })
        .min_by(f32::total_cmp)
}

unsafe fn render(
    renderer: &mut Renderer,
    target: &CachedFeed,
    frame: &Frame,
) -> (Vec<u8>, Vec<f32>) {
    let gl = renderer.gl.clone();
    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
    gl.viewport(0, 0, target.width, target.height);
    gl.disable(glow::SCISSOR_TEST);
    gl.disable(glow::BLEND);
    gl.disable(glow::FRAMEBUFFER_SRGB);
    gl.enable(glow::DEPTH_TEST);
    gl.depth_mask(true);
    gl.color_mask(true, true, true, true);
    gl.clear_color(0., 0., 0., 1.);
    gl.clear_depth_f32(1.);
    gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
    renderer.paint(frame, Some(target.framebuffer));
    assert_eq!(gl.get_error(), glow::NO_ERROR, "actual Renderer::paint");
    assert_eq!(
        gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING),
        Some(target.framebuffer)
    );
    let byte_count = target.width as usize * target.height as usize * 4;
    let mut rgba = vec![0u8; byte_count];
    gl.read_pixels(
        0,
        0,
        target.width,
        target.height,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelPackData::Slice(Some(&mut rgba)),
    );
    let mut depth_bytes = vec![0u8; byte_count];
    gl.read_pixels(
        0,
        0,
        target.width,
        target.height,
        glow::DEPTH_COMPONENT,
        glow::FLOAT,
        glow::PixelPackData::Slice(Some(&mut depth_bytes)),
    );
    assert_eq!(
        gl.get_error(),
        glow::NO_ERROR,
        "actual colour/depth readback"
    );
    let depths = depth_bytes
        .chunks_exact(4)
        .map(|bytes| f32::from_ne_bytes(bytes.try_into().unwrap()))
        .collect();
    (rgba, depths)
}

fn capture(
    directory: &Path,
    name: &str,
    width: usize,
    height: usize,
    pixels: &[u8],
) -> serde_json::Value {
    let top_down: Vec<u8> = pixels
        .chunks_exact(width * 4)
        .rev()
        .flatten()
        .copied()
        .collect();
    let filename = format!("{name}.png");
    image::save_buffer_with_format(
        directory.join(&filename),
        &top_down,
        width as u32,
        height as u32,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .unwrap();
    serde_json::json!({"png":filename,"rgba_sha256":hex::encode(Sha256::digest(&top_down)),
        "width":width,"height":height,"orientation":"top-left, RGBA8"})
}

#[test]
#[ignore = "Explicit private macOS CGL multi-angle actual cockpit enclosure; no app window or UI capture"]
fn native_pioneer_cockpit_enclosure_matches_source_and_keeps_window_apertures() {
    unsafe {
        let directory = std::env::var_os("ARKLINE_COCKPIT_QA_DIR").map(PathBuf::from);
        if let Some(directory) = &directory {
            std::fs::create_dir(directory).unwrap();
        }
        let (_context, gl) = PrivateContext::create();
        let mut renderer = Renderer::new(gl.clone()).unwrap();
        let source_parts = pioneer_asset().unwrap();
        let source_triangles: usize = source_parts
            .iter()
            .map(|part| part.vertices.len() / 3)
            .sum();
        let original_meshes: Vec<_> = renderer.meshes.iter().map(|mesh| mesh.buffer).collect();
        let mut evidence = Vec::new();
        for (width, height) in [(1080usize, 720usize), (1500, 960)] {
            let target = CachedFeed::create(&gl, 7000, width as i32, height as i32).unwrap();
            let viewport = egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width as f32, height as f32),
            );
            for case in cases() {
                let frame = frame(case, width, height);
                let (rgba, depth) = render(&mut renderer, &target, &frame);
                // Preserve the real image even if a following geometric assertion fails.
                let name = format!("pioneer-cockpit-{}-{width}x{height}", case.name);
                let png = directory
                    .as_deref()
                    .map(|directory| capture(directory, &name, width, height, &rgba));
                assert!(depth
                    .iter()
                    .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
                let near_surface = |value: f32| value < 1. - 1.0e-6;
                // A small central patch distinguishes a genuine open pane from a missing shell.
                // Side windows really are open; they must not be mistaken for enclosure defects.
                let radius = height / 80;
                let mut patch_surfaces = 0usize;
                let mut patch_pixels = 0usize;
                for y in height / 2 - radius..=height / 2 + radius {
                    for x in width / 2 - radius..=width / 2 + radius {
                        patch_pixels += 1;
                        patch_surfaces += usize::from(near_surface(depth[y * width + x]));
                    }
                }
                let fraction = patch_surfaces as f64 / patch_pixels as f64;
                if case.open_window {
                    assert!(
                        fraction < 0.1,
                        "{} {width}×{height}: authored window must expose space, central surface fraction {fraction}",
                        case.name
                    );
                } else {
                    assert!(
                        fraction > 0.9,
                        "{} {width}×{height}: actual shell must enclose this direction, central surface fraction {fraction}",
                        case.name
                    );
                }
                let x = width / 2;
                let y = height / 2;
                let pointer = egui::pos2(x as f32 + 0.5, height as f32 - y as f32 - 0.5);
                let expected = canonical_hit(&frame, pointer, viewport);
                let (ray_origin, ray_direction) =
                    screen_ray(frame.cockpit_vp, viewport, pointer).unwrap();
                let glass_distance = ray_mesh_hit(
                    ray_origin,
                    ray_direction,
                    frame.pioneer_body_mount() * Mat4::from_scale(Vec3::splat(100.0)),
                    &source_parts[6].vertices,
                );
                if case.open_window {
                    assert!(glass_distance.is_some(),
                        "{}: an open sightline must cross actual authored navigation glazing, not a hull gap", case.name);
                }
                assert_eq!(
                    expected.is_none(),
                    case.open_window,
                    "{}: canonical runtime triangle ray must agree with the aperture expectation",
                    case.name
                );
                let actual_depth = depth[y * width + x];
                let actual_distance = if near_surface(actual_depth) {
                    let clip = glam::Vec4::new(
                        (x as f32 + 0.5) * 2. / width as f32 - 1.,
                        (y as f32 + 0.5) * 2. / height as f32 - 1.,
                        actual_depth * 2. - 1.,
                        1.,
                    );
                    let point = frame.cockpit_vp.inverse() * clip;
                    let point = point.truncate() / point.w;
                    let (near, _) = screen_ray(frame.cockpit_vp, viewport, pointer).unwrap();
                    Some(point.distance(near))
                } else {
                    None
                };
                if let Some(expected) = expected {
                    let actual = actual_distance.expect("actual source enclosure must write depth");
                    assert!(
                        (actual - expected).abs() * 100. < 0.03,
                        "{}: GPU surface must match actual canonical triangles within 3cm; actual {}m, source {}m",
                        case.name,
                        actual * 100.,
                        expected * 100.
                    );
                } else {
                    assert!(
                        actual_distance.is_none(),
                        "{}: open source window must not be replaced with an opaque plate",
                        case.name
                    );
                }
                evidence.push(serde_json::json!({
                    "name":case.name,"width":width,"height":height,
                    "yaw_rad":case.yaw,"pitch_rad":case.pitch,"ship_roll_rad":case.roll,
                    "open_window":case.open_window,"eye_scene":frame.eye.to_array(),
                    "authored_glazing_ray_distance_m":glass_distance.map(|distance| distance*100.),
                    "view_projection":frame.cockpit_vp.to_cols_array(),
                    "central_surface_fraction":fraction,"canonical_runtime_ray_distance_m":expected.map(|value|value*100.),
                    "actual_depth_distance_m":actual_distance.map(|value|value*100.),
                    "whole_frame_surface_pixels":depth.iter().filter(|value|near_surface(**value)).count(),
                    "capture":png
                }));
            }
            target.destroy(&gl);
        }
        assert_eq!(
            original_meshes,
            renderer
                .meshes
                .iter()
                .map(|mesh| mesh.buffer)
                .collect::<Vec<_>>(),
            "Head look and resize must retain canonical GPU geometry"
        );
        if let Some(directory) = &directory {
            let receipt = serde_json::json!({
                "schema_version":1,"scope":"Actual flight Renderer::paint, private macOS CGL offscreen colour/depth; no app window or UI capture",
                "build":include_str!("../../../BUILD_VERSION").trim(),
                "glb_sha256":pioneer_asset_sha256(),
                "source_provenance":"Surviving baseline embedded GLB and decoded canonical triangles; unavailable authored-part receipt is not asserted",
                "source_part_count":source_parts.len(),"source_triangles":source_triangles,
                "vendor":gl.get_parameter_string(glow::VENDOR),"renderer":gl.get_parameter_string(glow::RENDERER),"gl_version":gl.get_parameter_string(glow::VERSION),
                "fitted_eye_body_m":cockpit_fit::eye_body_meters(ShipClass::Pioneer).to_array(),
                "eye_provenance":"Shared registration-3 pilot_joints(0) seated eye; no camera relocation",
                "registration_revision":arkline_protocol::PIONEER_DOCKED_REGISTRATION_REVISION,
                "fov_vertical_rad":COCKPIT_FOV,"cockpit_near_m":0.02,"cockpit_far_m":1000.,
                "yaw_tested_rad":[-std::f32::consts::PI,std::f32::consts::PI],"pitch_tested_rad":[-1.45,1.45],
                "lighting":"Existing Balanced world material/shadow path","gpu_source_buffers_retained":true,
                "captures":evidence,
                "limitations":["Offscreen engine geometry evidence does not establish operating-system input or egui screen text/click compositing", "Fitted cockpit presentation does not register the separate two-deck walkable scene, change collision, or imply authored seating", "Window checks use retained canonical triangles and depth, not invented enclosure boxes or hardcoded pixel colours", "No project, canonical source, physics or authority state was changed"]
            });
            std::fs::write(
                directory.join("receipt.json"),
                serde_json::to_vec_pretty(&receipt).unwrap(),
            )
            .unwrap();
        }
        renderer.destroy();
        assert_eq!(gl.get_error(), glow::NO_ERROR);
    }
}

#[derive(Debug, PartialEq)]
struct FeedGlState {
    draw: Option<glow::Framebuffer>,
    read: Option<glow::Framebuffer>,
    viewport: [i32; 4],
    scissor_box: [i32; 4],
    scissor: bool,
    depth: bool,
    blend: bool,
    srgb: bool,
    depth_mask: bool,
    color_mask: [bool; 4],
    blend_factors: [i32; 4],
    blend_equations: [i32; 2],
}
impl FeedGlState {
    unsafe fn read(gl: &glow::Context) -> Self {
        let mut viewport = [0; 4];
        let mut scissor_box = [0; 4];
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
        gl.get_parameter_i32_slice(glow::SCISSOR_BOX, &mut scissor_box);
        Self {
            draw: gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING),
            read: gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING),
            viewport,
            scissor_box,
            scissor: gl.is_enabled(glow::SCISSOR_TEST),
            depth: gl.is_enabled(glow::DEPTH_TEST),
            blend: gl.is_enabled(glow::BLEND),
            srgb: gl.is_enabled(glow::FRAMEBUFFER_SRGB),
            depth_mask: gl.get_parameter_bool(glow::DEPTH_WRITEMASK),
            color_mask: gl.get_parameter_bool_array::<4>(glow::COLOR_WRITEMASK),
            blend_factors: [
                gl.get_parameter_i32(glow::BLEND_SRC_RGB),
                gl.get_parameter_i32(glow::BLEND_DST_RGB),
                gl.get_parameter_i32(glow::BLEND_SRC_ALPHA),
                gl.get_parameter_i32(glow::BLEND_DST_ALPHA),
            ],
            blend_equations: [
                gl.get_parameter_i32(glow::BLEND_EQUATION_RGB),
                gl.get_parameter_i32(glow::BLEND_EQUATION_ALPHA),
            ],
        }
    }
}

fn rgba_at(pixels: &[u8], width: usize, height: usize, point: egui::Pos2) -> [u8; 4] {
    assert!(point.x >= 0. && point.y >= 0. && point.x < width as f32 && point.y < height as f32);
    let x = point.x.floor() as usize;
    let y = height - 1 - point.y.floor() as usize;
    pixels[(y * width + x) * 4..(y * width + x + 1) * 4]
        .try_into()
        .unwrap()
}

fn outside_quad(point: egui::Pos2, corners: &[egui::Pos2; 4], margin: f32) -> bool {
    let cross = |a: egui::Vec2, b: egui::Vec2| a.x * b.y - a.y * b.x;
    let orientation = cross(corners[1] - corners[0], corners[2] - corners[1]).signum();
    (0..4).any(|index| {
        let a = corners[index];
        let edge = corners[(index + 1) % 4] - a;
        cross(edge, point - a) * orientation < -margin * edge.length()
    })
}

#[test]
#[ignore = "Explicit private macOS CGL physical camera-feed quad orientation/state; no app window"]
fn native_cockpit_cached_feed_tracks_physical_quad_under_head_look_and_roll() {
    unsafe {
        let directory = std::env::var_os("ARKLINE_COCKPIT_FEED_QA_DIR").map(PathBuf::from);
        if let Some(directory) = &directory {
            std::fs::create_dir(directory).unwrap();
        }
        let (_context, gl) = PrivateContext::create();
        let mut renderer = Renderer::new(gl.clone()).unwrap();
        let (width, height) = (1080usize, 720usize);
        let target = CachedFeed::create(&gl, 8100, width as i32, height as i32).unwrap();
        let read_target = CachedFeed::create(&gl, 8101, 32, 32).unwrap();
        let viewport =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width as f32, height as f32));
        let canvas_size = egui::vec2(2048., 1024.);
        let background = [17u8, 23, 31, 255];
        // Rows follow OpenGL texture coordinates: bottom-left, bottom-right,
        // top-left, top-right. Four distinct colours detect either-axis flips.
        let quadrants = [
            [230u8, 31, 47, 255],
            [29, 211, 83, 255],
            [53, 73, 233, 255],
            [237, 199, 41, 255],
        ];
        let mut evidence = Vec::new();
        for (index, case) in [
            ViewCase {
                name: "head-look",
                yaw: 0.50,
                pitch: -0.22,
                roll: 0.,
                open_window: true,
            },
            ViewCase {
                name: "head-look-roll-inset-tile",
                yaw: 0.85,
                pitch: -0.10,
                roll: 0.60,
                open_window: true,
            },
        ]
        .into_iter()
        .enumerate()
        {
            let frame = frame(case, width, height);
            let mount = cockpit_fit::mount(Vec3::ZERO, frame.rotation, ShipClass::Pioneer);
            let surface = CockpitSurface::new(
                viewport,
                frame.cockpit_vp * mount,
                ShipClass::Pioneer,
                1,
                canvas_size,
            )
            .expect("real drone display must be visible");
            let tile = if index == 0 {
                surface.canvas_rect()
            } else {
                surface.canvas_rect().shrink2(egui::vec2(256., 128.))
            };
            let corners = [
                tile.left_top(),
                tile.right_top(),
                tile.right_bottom(),
                tile.left_bottom(),
            ]
            .map(|point| {
                surface
                    .project(point)
                    .expect("test quad corners must remain in the actual view")
            });
            let (cache_width, cache_height) =
                feed_resolution(tile.width().round() as i32, tile.height().round() as i32);
            assert_eq!(
                (cache_width, cache_height),
                (384, 192),
                "large logical displays retain the real bounded feed resolution"
            );
            renderer.prepare_exterior(&frame);
            let key = 8200 + index as u64;
            let mut cache = CachedFeed::create(&gl, key, cache_width, cache_height).unwrap();
            let mut texture = Vec::with_capacity(cache_width as usize * cache_height as usize * 4);
            for y in 0..cache_height {
                for x in 0..cache_width {
                    texture.extend_from_slice(
                        &quadrants[usize::from(x >= cache_width / 2)
                            + 2 * usize::from(y >= cache_height / 2)],
                    );
                }
            }
            gl.bind_texture(glow::TEXTURE_2D, Some(cache.texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                cache_width,
                cache_height,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&texture)),
            );
            cache.painted_ms = Some(frame.now);
            let texture_id = cache.texture;
            renderer.feed_cache.push(cache);
            // A fresh cached frame avoids rendering a synthetic camera scene:
            // this test specifically exercises the production composition path.
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            gl.viewport(0, 0, width as i32, height as i32);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::FRAMEBUFFER_SRGB);
            gl.color_mask(true, true, true, true);
            gl.depth_mask(true);
            gl.clear_color(
                f32::from(background[0]) / 255.,
                f32::from(background[1]) / 255.,
                f32::from(background[2]) / 255.,
                1.,
            );
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(read_target.framebuffer));
            gl.viewport(7, 9, width as i32 - 14, height as i32 - 18);
            gl.scissor(8, 11, width as i32 - 16, height as i32 - 22);
            gl.enable(glow::SCISSOR_TEST);
            gl.enable(glow::DEPTH_TEST);
            gl.enable(glow::BLEND);
            gl.enable(glow::FRAMEBUFFER_SRGB);
            gl.depth_mask(false);
            gl.color_mask(true, false, true, false);
            gl.blend_func_separate(
                glow::ONE_MINUS_SRC_ALPHA,
                glow::SRC_ALPHA,
                glow::DST_ALPHA,
                glow::ONE_MINUS_DST_ALPHA,
            );
            gl.blend_equation_separate(glow::FUNC_SUBTRACT, glow::FUNC_REVERSE_SUBTRACT);
            let before = FeedGlState::read(&gl);
            renderer.paint_cached_feed_on_surface(
                &frame,
                key,
                egui::PaintCallbackInfo {
                    viewport,
                    clip_rect: viewport,
                    pixels_per_point: 1.,
                    screen_size_px: [width as u32, height as u32],
                },
                1,
                None,
                Some((&surface, tile)),
            );
            assert_eq!(
                gl.get_error(),
                glow::NO_ERROR,
                "actual physical feed composition"
            );
            assert_eq!(
                FeedGlState::read(&gl),
                before,
                "feed composition restores the caller's framebuffer, viewport, scissor, depth/blend/mask and sRGB state"
            );
            let cached = renderer
                .feed_cache
                .iter()
                .find(|cache| cache.key == key)
                .unwrap();
            assert_eq!(
                (
                    cached.width,
                    cached.height,
                    cached.texture,
                    cached.painted_ms
                ),
                (384, 192, texture_id, Some(frame.now)),
                "fresh bounded cached pixels must be reused"
            );
            assert!(renderer.feed_cache.len() <= 4);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target.framebuffer));
            let mut pixels = vec![0u8; width * height * 4];
            gl.read_pixels(
                0,
                0,
                width as i32,
                height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            assert_eq!(gl.get_error(), glow::NO_ERROR);
            let name = format!("physical-feed-{}", case.name);
            let png = directory
                .as_deref()
                .map(|directory| capture(directory, &name, width, height, &pixels));
            let mut samples = Vec::new();
            for (u, v, expected) in [
                (0.25, 0.25, quadrants[2]),
                (0.75, 0.25, quadrants[3]),
                (0.25, 0.75, quadrants[0]),
                (0.75, 0.75, quadrants[1]),
            ] {
                let local = tile.min + tile.size() * egui::vec2(u, v);
                let point = surface.project(local).unwrap();
                let actual = rgba_at(&pixels, width, height, point);
                assert!(
                    actual
                        .iter()
                        .zip(expected)
                        .all(|(actual, expected)| actual.abs_diff(expected) <= 2),
                    "{} local UV({u},{v}) must preserve cached texture orientation and encoding: {actual:?} vs {expected:?}",
                    case.name
                );
                samples.push(serde_json::json!({"tile_uv":[u,v],"screen_px":[point.x,point.y],"expected_rgba":expected,"actual_rgba":actual}));
            }
            let mut outside_pixels = 0usize;
            let mut changed_pixels = 0usize;
            for y in 0..height {
                for x in 0..width {
                    let pixel = &pixels[(y * width + x) * 4..(y * width + x + 1) * 4];
                    let changed = pixel
                        .iter()
                        .zip(background)
                        .any(|(actual, expected)| actual.abs_diff(expected) > 1);
                    changed_pixels += usize::from(changed);
                    let point = egui::pos2(x as f32 + 0.5, height as f32 - y as f32 - 0.5);
                    if outside_quad(point, &corners, 2.) {
                        outside_pixels += 1;
                        assert!(
                            !changed,
                            "{} camera pixels escaped the actual physical quad at {point:?}",
                            case.name
                        );
                    }
                }
            }
            assert!(outside_pixels > width * height * 3 / 4);
            assert!(
                changed_pixels > 500,
                "physical quad must contain a visible image"
            );
            evidence.push(serde_json::json!({"name":case.name,"yaw_rad":case.yaw,"pitch_rad":case.pitch,"ship_roll_rad":case.roll,
                "canvas_px":[canvas_size.x,canvas_size.y],"tile_px":[tile.min.x,tile.min.y,tile.max.x,tile.max.y],
                "quad_corners_px":corners.map(|point|[point.x,point.y]),"cache_px":[cache_width,cache_height],
                "changed_pixels":changed_pixels,"outside_quad_background_pixels":outside_pixels,"samples":samples,
                "caller_gl_state_restored":true,"cache_reused_without_scene_rerender":true,"capture":png}));
        }
        if let Some(directory) = &directory {
            let receipt = serde_json::json!({"schema_version":1,
                "scope":"Actual Renderer::paint_cached_feed physical quad shader in a private macOS CGL FBO; known cached test pattern, no app window or UI capture",
                "build":include_str!("../../../BUILD_VERSION").trim(),"glb_sha256":pioneer_asset_sha256(),
                "vendor":gl.get_parameter_string(glow::VENDOR),"renderer":gl.get_parameter_string(glow::RENDERER),"gl_version":gl.get_parameter_string(glow::VERSION),
                "viewport_px":[width,height],"physical_surface_index":1,"captures":evidence,
                "limitations":["Known cached RGBA pattern checks composition geometry/orientation, not a live camera scene or network freshness", "Actual native CGL shader execution does not establish browser GPU or operating-system/UI input acceptance", "No project, canonical geometry or authoritative world state was changed"]});
            std::fs::write(
                directory.join("receipt.json"),
                serde_json::to_vec_pretty(&receipt).unwrap(),
            )
            .unwrap();
        }
        renderer.destroy();
        target.destroy(&gl);
        read_target.destroy(&gl);
        assert_eq!(gl.get_error(), glow::NO_ERROR);
    }
}

fn has_text(shape: &egui::Shape, text: &str) -> bool {
    match shape {
        egui::Shape::Text(value) => value.galley.text().contains(text),
        egui::Shape::Vec(shapes) => shapes.iter().any(|shape| has_text(shape, text)),
        _ => false,
    }
}

fn surface_pixels(surface: &CockpitSurface, pixels: &[u8], width: usize, height: usize) -> Vec<u8> {
    let canvas = surface.canvas_rect();
    let mut result = Vec::with_capacity(96 * 64 * 4);
    for y in 0..64 {
        for x in 0..96 {
            let point = canvas.min
                + canvas.size() * egui::vec2((x as f32 + 0.5) / 96., (y as f32 + 0.5) / 64.);
            result.extend_from_slice(
                &surface
                    .project(point)
                    .map_or([0; 4], |point| rgba_at(pixels, width, height, point)),
            );
        }
    }
    result
}

fn rect_pixels(rect: egui::Rect, pixels: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut result = Vec::with_capacity(96 * 64 * 4);
    for y in 0..64 {
        for x in 0..96 {
            let point =
                rect.min + rect.size() * egui::vec2((x as f32 + 0.5) / 96., (y as f32 + 0.5) / 64.);
            result.extend_from_slice(&rgba_at(pixels, width, height, point));
        }
    }
    result
}

fn changed_samples(before: &[u8], after: &[u8]) -> usize {
    assert_eq!(before.len(), after.len());
    before
        .chunks_exact(4)
        .zip(after.chunks_exact(4))
        .filter(|(a, b)| a[..3].iter().zip(&b[..3]).any(|(a, b)| a.abs_diff(*b) > 4))
        .count()
}

fn lit_samples(pixels: &[u8]) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|pixel| pixel[..3].iter().any(|value| *value > 70))
        .count()
}

#[derive(Clone, Copy)]
struct ShiftCase {
    name: &'static str,
    shift: bool,
    focused: bool,
    arrow: Option<egui::Key>,
    text_focus: bool,
}

#[test]
#[ignore = "Explicit private macOS CGL whole-app Shift/live cockpit glyphs, textures and materials; no window"]
fn native_cockpit_shift_keeps_both_instrument_layers_and_live_feed_updating() {
    unsafe {
        let directory = std::env::var_os("ARKLINE_COCKPIT_SHIFT_QA_DIR").map(PathBuf::from);
        if let Some(directory) = &directory {
            std::fs::create_dir(directory).unwrap();
        }
        let (_context, gl) = PrivateContext::create();
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.flight_view = FlightView::new(Some(&gl));
        cockpit.embedded = true;
        cockpit.embedded_paused = false;
        // This renderer fixture owns game input. Parent text focus, rather
        // than a globally denied input flag, must block the negative cases.
        cockpit.embedded_input = true;
        cockpit.view_mode = crate::ViewMode::Cockpit;
        cockpit.service_tab = crate::ServiceTab::Navigation;
        cockpit.cockpit_console_detail = true; // Explicit console interaction fixture.
        cockpit.services_open = false;
        cockpit.overview_open = false;
        cockpit.drone_camera = true;
        cockpit.selected_drone = Some(7);
        cockpit.drone_camera_mode = DroneCameraMode::Pov;
        assert!(
            cockpit.commands.is_none()
                && cockpit.updates.is_none()
                && cockpit.local_server.is_none()
        );
        let mut painter = egui_glow::Painter::new(gl.clone(), "", None, false).unwrap();
        let (width, height) = (1500usize, 960usize);
        let target = CachedFeed::create(&gl, 8300, width as i32, height as i32).unwrap();
        let screen =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width as f32, height as f32));
        let world_origin = [90_000_000_000.; 3];
        let cases = [
            ShiftCase {
                name: "baseline",
                shift: false,
                focused: true,
                arrow: None,
                text_focus: false,
            },
            ShiftCase {
                name: "shift-held",
                shift: true,
                focused: true,
                arrow: None,
                text_focus: false,
            },
            ShiftCase {
                name: "shift-arrow-headlook",
                shift: true,
                focused: true,
                arrow: Some(egui::Key::ArrowLeft),
                text_focus: false,
            },
            ShiftCase {
                name: "shift-release",
                shift: false,
                focused: true,
                arrow: None,
                text_focus: false,
            },
            ShiftCase {
                name: "window-focus-lost",
                shift: true,
                focused: false,
                arrow: None,
                text_focus: false,
            },
            ShiftCase {
                name: "window-focus-returned",
                shift: true,
                focused: true,
                arrow: None,
                text_focus: false,
            },
            ShiftCase {
                name: "parent-text-focus-established",
                shift: false,
                focused: true,
                arrow: None,
                text_focus: true,
            },
            ShiftCase {
                name: "parent-text-focus-shift-arrow",
                shift: true,
                focused: true,
                arrow: Some(egui::Key::ArrowRight),
                text_focus: true,
            },
            ShiftCase {
                name: "parent-text-focus-shift-release",
                shift: false,
                focused: true,
                arrow: None,
                text_focus: true,
            },
        ];
        let parent_text_id = egui::Id::new("cockpit_gpu_fixture_parent_text");
        let mut parent_text = String::from("Parent fixture text input");
        let mut step = 0u64;
        let mut previous_arrow = None;
        let mut observations = Vec::new();
        let mut previous_video: Option<String> = None;
        for case in cases {
            let frame_count = if case.name == "baseline" { 4 } else { 2 };
            let mut previous_power_pixels: Option<Vec<u8>> = None;
            let mut previous_console_pixels: Option<Vec<u8>> = None;
            let mut previous_power_glyphs: Option<String> = None;
            for pass in 0..frame_count {
                let now = 2000 + step * 50;
                let charge = if step % 2 == 0 { 40_000 } else { 200_000 };
                let ore = 100 + step * 11;
                let reactor = 1200 + step * 10;
                let snapshot = cockpit.snapshot.as_mut().unwrap();
                snapshot.server_time_ms = now;
                snapshot.asteroids.clear();
                snapshot.online_pilots = vec!["pilot".into()];
                let own = &mut snapshot.pilots[0];
                own.location = Location::Belt;
                own.system_id = 3;
                own.ore = ore;
                own.power = Some(arkline_protocol::PowerState {
                    charge_j: charge,
                    capacity_j: 240_000,
                    regen_w: reactor,
                    updated_at_ms: now,
                    regen_remainder_mj: 0,
                });
                let flight = own.flight.as_mut().unwrap();
                flight.position = world_origin;
                flight.updated_at_ms = now;
                own.drones = vec![serde_json::from_value(serde_json::json!({
                    "id":7,"class":"recon","signal_strength":1.0,
                    "position":[world_origin[0] + if step % 2 == 0 { 80. } else { 125. }, world_origin[1] + 5., world_origin[2]],
                    "velocity":[0.,0.,0.],"heading":[-1.,0.,0.],"mode":{"kind":"hold"},"updated_at_ms":now
                })).unwrap()];
                cockpit.frozen_time = Some(now);
                cockpit.timeline_origin = Some(now);
                cockpit.observed = crate::Instant::now();
                cockpit.selected_object = crate::SelectedObject::Belt;
                let snapshot_before = serde_json::to_value(&cockpit.snapshot).unwrap();
                let modifiers = egui::Modifiers {
                    shift: case.shift,
                    ..Default::default()
                };
                // Real viewport ownership is required for head-look; the
                // private renderer journey must supply a physical pointer too.
                let pointer = cockpit
                    .flight_view
                    .cockpit_display_frame
                    .as_ref()
                    .map_or(screen.center(), |frame| frame.viewport.center());
                let mut events = vec![egui::Event::PointerMoved(pointer)];
                if previous_arrow != case.arrow {
                    if let Some(key) = previous_arrow {
                        events.push(egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed: false,
                            repeat: false,
                            modifiers,
                        });
                    }
                    if let Some(key) = case.arrow {
                        events.push(egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers,
                        });
                    }
                    previous_arrow = case.arrow;
                }
                let parent_text_focus_retained =
                    ctx.memory(|memory| memory.has_focus(parent_text_id));
                let mut parent_text_focused = false;
                let mut parent_text_rect = None;
                let previous_look = cockpit.flight_view.head_look;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        time: Some(now as f64 / 1000.),
                        predicted_dt: 0.05,
                        focused: case.focused,
                        modifiers,
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        if case.text_focus {
                            // This is a real parent-context editor widget. The
                            // cockpit service UI lives in its secondary context
                            // and cannot supply parent keyboard focus here.
                            egui::Area::new(egui::Id::new("cockpit_gpu_parent_text_fixture"))
                                .order(egui::Order::Foreground)
                                .fixed_pos(egui::pos2(8.0, 8.0))
                                .show(ctx, |ui| {
                                    let response = ui.add(
                                        egui::TextEdit::singleline(&mut parent_text)
                                            .id(parent_text_id)
                                            .desired_width(240.0),
                                    );
                                    // Preserve the TextEdit's installed arrow
                                    // filter once focus has been established.
                                    if !response.has_focus() {
                                        response.request_focus();
                                    }
                                    parent_text_focused = response.has_focus();
                                    parent_text_rect = Some(response.rect);
                                });
                            assert!(parent_text_focused && ctx.wants_keyboard_input());
                        } else {
                            ctx.memory_mut(|memory| {
                                if let Some(id) = memory.focused() {
                                    memory.surrender_focus(id);
                                }
                            });
                        }
                        eframe::App::update(&mut cockpit, ctx, &mut eframe::Frame::_new_kittest());
                    },
                );
                assert_eq!(
                    serde_json::to_value(&cockpit.snapshot).unwrap(),
                    snapshot_before,
                    "Rendered input must never mutate the received fixture snapshot"
                );
                assert!(
                    cockpit.connected
                        && cockpit
                            .flight_view
                            .cockpit_presentation_active(crate::ViewMode::Cockpit)
                );
                let primitives = ctx.tessellate(output.shapes.clone(), output.pixels_per_point);
                let callback_rects: Vec<_> = primitives
                    .iter()
                    .filter_map(|primitive| {
                        if let egui::epaint::Primitive::Callback(callback) = &primitive.primitive {
                            Some(callback.rect)
                        } else {
                            None
                        }
                    })
                    .collect();
                assert!(
                    callback_rects.len() >= 2,
                    "{}: both world/material and live video GPU callbacks must remain scheduled",
                    case.name
                );
                let viewport = callback_rects[0];
                let eye = cockpit.flight_view.last_camera_pose.unwrap().0;
                let look = cockpit.flight_view.head_look;
                let rotation = Quat::from_rotation_y(look[0]) * Quat::from_rotation_z(look[1]);
                let view = Mat4::look_at_rh(eye, eye + rotation * Vec3::X, rotation * Vec3::Y);
                let vp = Mat4::perspective_rh_gl(COCKPIT_FOV, viewport.aspect_ratio(), 0.0002, 10.)
                    * view;
                let mount = cockpit_fit::mount(Vec3::ZERO, Quat::IDENTITY, ShipClass::Pioneer);
                let surfaces: Vec<_> = (0..6).map(|index| CockpitSurface::new(viewport, vp * mount,
                    ShipClass::Pioneer, index, egui::vec2(300., 180.)).expect("all six real screens remain at least partly visible in this bounded head-look journey")).collect();
                let mut glyph_hash = Sha256::new();
                let mut power_glyph_vertices = 0usize;
                // The computer's own atlas/triangles stay canonical until the
                // physical GL pass; the parent has no flattened display mesh.
                for primitive in cockpit.cockpit_computer.debug_primitives() {
                    if let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive {
                        power_glyph_vertices += mesh.vertices.len();
                        for vertex in &mesh.vertices {
                            glyph_hash.update(vertex.uv.x.to_bits().to_le_bytes());
                            glyph_hash.update(vertex.uv.y.to_bits().to_le_bytes());
                        }
                    }
                }
                let power_glyph_hash = hex::encode(glyph_hash.finalize());
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
                gl.viewport(0, 0, width as i32, height as i32);
                gl.disable(glow::SCISSOR_TEST);
                gl.color_mask(true, true, true, true);
                gl.depth_mask(true);
                gl.clear_color(0., 0., 0., 1.);
                gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
                painter.paint_and_update_textures(
                    [width as u32, height as u32],
                    output.pixels_per_point,
                    &primitives,
                    &output.textures_delta,
                );
                assert_eq!(
                    gl.get_error(),
                    glow::NO_ERROR,
                    "whole-app egui glyph and real material/feed callbacks"
                );
                gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target.framebuffer));
                let mut pixels = vec![0u8; width * height * 4];
                gl.read_pixels(
                    0,
                    0,
                    width as i32,
                    height as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut pixels)),
                );
                assert_eq!(gl.get_error(), glow::NO_ERROR);
                let png = if pass + 1 == frame_count {
                    directory.as_deref().map(|directory| {
                        capture(
                            directory,
                            &format!("whole-app-{}", case.name),
                            width,
                            height,
                            &pixels,
                        )
                    })
                } else {
                    None
                };
                // Let egui's initial Area sizing settle, while still executing and painting all warm-up frames.
                if case.name == "baseline" && pass < 2 {
                    step += 1;
                    continue;
                }
                assert!(
                    cockpit.flight_view.computer_surface().is_some(),
                    "{}: actual second instrument/service layer must stay present",
                    case.name
                );
                let label = format!(
                    "Capacitor {:.1}/240.0 kJ · reactor {reactor} W",
                    charge as f64 / 1000.
                );
                assert!(
                    cockpit.cockpit_computer.last_shapes.iter()
                        .any(|shape| has_text(&shape.shape, &label)),
                    "{}: actual cockpit computer must display this received frame's power, not frozen labels",
                    case.name
                );
                assert!(
                    power_glyph_vertices > 12,
                    "{}: canonical computer glyphs must stay populated",
                    case.name
                );
                let screen_samples: Vec<_> = surfaces
                    .iter()
                    .map(|surface| surface_pixels(surface, &pixels, width, height))
                    .collect();
                let screen_lit: Vec<_> = screen_samples
                    .iter()
                    .map(|pixels| lit_samples(pixels))
                    .collect();
                assert!(
                    screen_lit.iter().all(|count| *count > 8),
                    "{}: all real display faces must contain rendered light/content, counts {screen_lit:?}",
                    case.name
                );
                let power_pixels = screen_samples[2].clone();
                let console_rect = cockpit
                    .flight_view
                    .cockpit_console_rect()
                    .unwrap()
                    .intersect(screen)
                    .shrink(2.);
                let console_pixels = rect_pixels(console_rect, &pixels, width, height);
                assert!(
                    lit_samples(&console_pixels) > 20,
                    "{}: actual computer glyphs/gauge must reach the GPU image",
                    case.name
                );
                let power_changed = previous_power_pixels
                    .as_ref()
                    .map(|before| changed_samples(before, &power_pixels));
                let console_changed = previous_console_pixels
                    .as_ref()
                    .map(|before| changed_samples(before, &console_pixels));
                if let Some(changed) = power_changed {
                    assert!(
                        changed > 5,
                        "{}: projected first instrument layer must update from new power/ore, changed {changed}",
                        case.name
                    );
                    assert_ne!(
                        previous_power_glyphs.as_ref().unwrap(),
                        &power_glyph_hash,
                        "{}: actual projected glyph texture coordinates must change with the received numbers",
                        case.name
                    );
                }
                if let Some(changed) = console_changed {
                    assert!(
                        changed > 5,
                        "{}: actual second-layer power gauge/text must keep updating, changed {changed}",
                        case.name
                    );
                }
                let (video_hash, video_png, cache_time, material_triangles) = {
                    let renderer = cockpit
                        .flight_view
                        .renderer
                        .as_ref()
                        .unwrap()
                        .lock()
                        .unwrap();
                    assert_eq!(
                        renderer.feed_cache.len(),
                        1,
                        "the real connected drone supplies one received live feed"
                    );
                    let cache = &renderer.feed_cache[0];
                    assert_eq!(
                        cache.painted_ms,
                        Some(now),
                        "{}: feed texture must render at the new received snapshot time",
                        case.name
                    );
                    assert!(cache.width <= 384 && cache.height <= 384);
                    gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(cache.framebuffer));
                    let mut video = vec![0u8; cache.width as usize * cache.height as usize * 4];
                    gl.read_pixels(
                        0,
                        0,
                        cache.width,
                        cache.height,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelPackData::Slice(Some(&mut video)),
                    );
                    assert_eq!(gl.get_error(), glow::NO_ERROR);
                    assert!(
                        lit_samples(&video) > 20,
                        "{}: actual fresh camera scene/materials must not be black",
                        case.name
                    );
                    let png = if pass + 1 == frame_count {
                        directory.as_deref().map(|directory| {
                            capture(
                                directory,
                                &format!("live-drone-{}", case.name),
                                cache.width as usize,
                                cache.height as usize,
                                &video,
                            )
                        })
                    } else {
                        None
                    };
                    (
                        hex::encode(Sha256::digest(&video)),
                        png,
                        cache.painted_ms,
                        renderer.debug_metrics.get().triangles,
                    )
                };
                assert!(
                    material_triangles > 100_000,
                    "actual source hull/material GPU pass must remain in the camera image"
                );
                if let Some(before) = &previous_video {
                    assert_ne!(
                        before, &video_hash,
                        "{}: new received drone position must produce fresh camera pixels, not a retained static image",
                        case.name
                    );
                }
                if case.text_focus {
                    assert!(parent_text_focused);
                    assert!(ctx.memory(|memory| memory.has_focus(parent_text_id)));
                    assert!(ctx.wants_keyboard_input());
                    let rect = parent_text_rect.expect("a real parent TextEdit was rendered");
                    assert!(rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0);
                    if case.name != "parent-text-focus-established" || pass > 0 {
                        assert!(
                            parent_text_focus_retained,
                            "the real parent TextEdit must retain focus between input frames"
                        );
                    }
                    assert!(
                        look.iter()
                            .zip(previous_look)
                            .all(|(after, before)| (*after - before).abs() <= 1.0e-6),
                        "Parent TextEdit ownership must freeze head look, including Shift+arrows"
                    );
                } else if case.shift && case.focused && case.arrow.is_some() {
                    assert!(
                        look.iter()
                            .zip(previous_look)
                            .any(|(after, before)| (*after - before).abs() > 1.0e-5),
                        "Owned scene input must still positively exercise Shift+arrow head look"
                    );
                }
                observations.push(serde_json::json!({"state":case.name,"frame":step,"received_time_ms":now,
                    "shift":case.shift,"window_focused":case.focused,"game_input_allowed":cockpit.embedded_input,
                    "parent_text_fixture_requested":case.text_focus,"parent_text_focused":parent_text_focused,
                    "parent_text_focus_retained_before_frame":parent_text_focus_retained,
                    "head_look_rad":look,"received_power_j":charge,"received_ore":ore,"console_label":label,
                    "canonical_computer_vertices":power_glyph_vertices,"canonical_computer_uv_sha256":power_glyph_hash,
                    "display_lit_samples":screen_lit,"power_changed_samples":power_changed,"console_changed_samples":console_changed,
                    "video_rgba_sha256":video_hash,"video_render_time_ms":cache_time,"actual_material_triangles":material_triangles,
                    "world_and_feed_callbacks":callback_rects.len(),"capture":png,"video_capture":video_png,
                    "received_snapshot_unchanged_by_input":true}));
                previous_video = Some(video_hash);
                previous_power_pixels = Some(power_pixels);
                previous_console_pixels = Some(console_pixels);
                previous_power_glyphs = Some(power_glyph_hash);
                step += 1;
            }
        }
        if let Some(directory) = &directory {
            let receipt = serde_json::json!({"schema_version":1,
                "scope":"Actual Cockpit::update + FlightView::ui + egui_glow Painter glyph/projected-mesh/material/live-camera callbacks in private macOS CGL FBO; no app window or UI capture",
                "build":include_str!("../../../BUILD_VERSION").trim(),"glb_sha256":pioneer_asset_sha256(),
                "vendor":gl.get_parameter_string(glow::VENDOR),"renderer":gl.get_parameter_string(glow::RENDERER),"gl_version":gl.get_parameter_string(glow::VERSION),
                "viewport_px":[width,height],"frames_rendered":step,"observations":observations,
                "fixture":"Isolated headless Cockpit with game input enabled and a real stable-ID parent TextEdit rendered before each text-focus frame; no connector/mailbox receiver/local server; received snapshot values changed by the test only",
                "limitations":["Real native offscreen glyph/material/video composition does not establish OS keyboard/window focus delivery or browser GPU acceptance", "Telemetry and drone snapshots are explicit deterministic test fixtures, not measurements from a running authority", "No world, project, canonical geometry, network session or credentials were modified"]});
            std::fs::write(
                directory.join("receipt.json"),
                serde_json::to_vec_pretty(&receipt).unwrap(),
            )
            .unwrap();
        }
        cockpit.flight_view.destroy();
        painter.destroy();
        target.destroy(&gl);
        assert_eq!(gl.get_error(), glow::NO_ERROR);
    }
}
