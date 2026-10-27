//! Opt-in actual cockpit renderer evidence in a private macOS CGL context.
//! This creates no window and does not capture or drive the desktop UI.
use super::*;
use sha2::{Digest, Sha256};
use std::ffi::{CString, c_void};
use std::path::{Path, PathBuf};

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

fn cases() -> [ViewCase; 11] {
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
            name: "port",
            yaw: FRAC_PI_2,
            pitch: 0.,
            roll: 0.,
            open_window: true,
        },
        ViewCase {
            name: "starboard",
            yaw: -FRAC_PI_2,
            pitch: 0.,
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
    let camera = rotation * Quat::from_rotation_y(case.yaw) * Quat::from_rotation_z(case.pitch);
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
        exterior: None,
        lighting_quality: super::super::lighting_settings::LightingQuality::Balanced,
        cockpit_vp,
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

/// Raycast the actual retained source triangles, not a surrogate enclosure box.
fn canonical_hit(frame: &Frame, pointer: egui::Pos2, viewport: egui::Rect) -> Option<f32> {
    let (origin, direction) = screen_ray(frame.cockpit_vp, viewport, pointer).unwrap();
    let ship = Mat4::from_quat(frame.rotation);
    pioneer_asset()
        .unwrap()
        .iter()
        .enumerate()
        .filter_map(|(index, primitive)| {
            if let Some(ranges) = cockpit_fit::canopy_ranges(10 + index) {
                ranges
                    .iter()
                    .filter_map(|range| {
                        ray_mesh_hit(
                            origin,
                            direction,
                            ship,
                            &primitive.vertices[range.start as usize..range.end as usize],
                        )
                    })
                    .min_by(f32::total_cmp)
            } else {
                ray_mesh_hit(origin, direction, ship, &primitive.vertices)
            }
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
        let (source, geometry) = pioneer_parts::PioneerSource::load_embedded().unwrap();
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
                assert!(
                    depth
                        .iter()
                        .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
                );
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
                assert_eq!(
                    expected.is_none(),
                    case.open_window,
                    "{}: source triangle ray must agree with the aperture expectation",
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
                        "{}: GPU surface must match actual source triangles within 3cm; actual {}m, source {}m",
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
                    "view_projection":frame.cockpit_vp.to_cols_array(),
                    "central_surface_fraction":fraction,"source_ray_distance_m":expected.map(|value|value*100.),
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
                "source_sha256":source.source_sha256,"glb_sha256":source.glb_sha256,"receipt_sha256":source.receipt_sha256,
                "source_part_count":source.parts.len(),"source_triangles":geometry.report.triangles,
                "vendor":gl.get_parameter_string(glow::VENDOR),"renderer":gl.get_parameter_string(glow::RENDERER),"gl_version":gl.get_parameter_string(glow::VERSION),
                "fitted_eye_body_m":cockpit_fit::eye_body_meters(ShipClass::Pioneer).to_array(),
                "eye_provenance":"Presentation camera fitted to measured canonical bridge cavity; not an authored seat or registered walkable interior",
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
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width as f32, height as f32));
        let canvas_size = egui::vec2(2048., 1024.);
        let background = [17u8, 23, 31, 255];
        // Rows follow OpenGL texture coordinates: bottom-left, bottom-right,
        // top-left, top-right. Four distinct colours detect either-axis flips.
        let quadrants = [[230u8, 31, 47, 255], [29, 211, 83, 255], [53, 73, 233, 255], [237, 199, 41, 255]];
        let mut evidence = Vec::new();
        for (index, case) in [
            ViewCase { name: "head-look", yaw: 0.50, pitch: -0.22, roll: 0., open_window: true },
            ViewCase { name: "head-look-roll-inset-tile", yaw: 0.85, pitch: -0.10, roll: 0.60, open_window: true },
        ].into_iter().enumerate() {
            let frame = frame(case, width, height);
            let mount = cockpit_fit::mount(Vec3::ZERO, frame.rotation, ShipClass::Pioneer);
            let surface = CockpitSurface::new(viewport, frame.cockpit_vp * mount,
                ShipClass::Pioneer, 1, canvas_size).expect("real drone display must be visible");
            let tile = if index == 0 { surface.canvas_rect() }
                else { surface.canvas_rect().shrink2(egui::vec2(256., 128.)) };
            let corners = [tile.left_top(), tile.right_top(), tile.right_bottom(), tile.left_bottom()]
                .map(|point| surface.project(point).expect("test quad corners must remain in the actual view"));
            let (cache_width, cache_height) = feed_resolution(tile.width().round() as i32, tile.height().round() as i32);
            assert_eq!((cache_width, cache_height), (384, 192), "large logical displays retain the real bounded feed resolution");
            renderer.prepare_exterior(&frame);
            let key = 8200 + index as u64;
            let mut cache = CachedFeed::create(&gl, key, cache_width, cache_height).unwrap();
            let mut texture = Vec::with_capacity(cache_width as usize * cache_height as usize * 4);
            for y in 0..cache_height {
                for x in 0..cache_width {
                    texture.extend_from_slice(&quadrants[usize::from(x >= cache_width / 2) + 2 * usize::from(y >= cache_height / 2)]);
                }
            }
            gl.bind_texture(glow::TEXTURE_2D, Some(cache.texture));
            gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, cache_width, cache_height, 0,
                glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(Some(&texture)));
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
            gl.clear_color(f32::from(background[0]) / 255., f32::from(background[1]) / 255., f32::from(background[2]) / 255., 1.);
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
            gl.blend_func_separate(glow::ONE_MINUS_SRC_ALPHA, glow::SRC_ALPHA, glow::DST_ALPHA, glow::ONE_MINUS_DST_ALPHA);
            gl.blend_equation_separate(glow::FUNC_SUBTRACT, glow::FUNC_REVERSE_SUBTRACT);
            let before = FeedGlState::read(&gl);
            renderer.paint_cached_feed(&frame, key, egui::PaintCallbackInfo {
                viewport, clip_rect: viewport, pixels_per_point: 1., screen_size_px: [width as u32, height as u32],
            }, 1, None, Some((&surface, tile)));
            assert_eq!(gl.get_error(), glow::NO_ERROR, "actual physical feed composition");
            assert_eq!(FeedGlState::read(&gl), before, "feed composition restores the caller's framebuffer, viewport, scissor, depth/blend/mask and sRGB state");
            let cached = renderer.feed_cache.iter().find(|cache|cache.key == key).unwrap();
            assert_eq!((cached.width, cached.height, cached.texture, cached.painted_ms),
                (384, 192, texture_id, Some(frame.now)), "fresh bounded cached pixels must be reused");
            assert!(renderer.feed_cache.len() <= 4);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target.framebuffer));
            let mut pixels = vec![0u8; width * height * 4];
            gl.read_pixels(0, 0, width as i32, height as i32, glow::RGBA, glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)));
            assert_eq!(gl.get_error(), glow::NO_ERROR);
            let name = format!("physical-feed-{}", case.name);
            let png = directory.as_deref().map(|directory|capture(directory, &name, width, height, &pixels));
            let mut samples = Vec::new();
            for (u, v, expected) in [(0.25, 0.25, quadrants[2]), (0.75, 0.25, quadrants[3]),
                (0.25, 0.75, quadrants[0]), (0.75, 0.75, quadrants[1])] {
                let local = tile.min + tile.size() * egui::vec2(u, v);
                let point = surface.project(local).unwrap();
                let actual = rgba_at(&pixels, width, height, point);
                assert!(actual.iter().zip(expected).all(|(actual, expected)|actual.abs_diff(expected) <= 2),
                    "{} local UV({u},{v}) must preserve cached texture orientation and encoding: {actual:?} vs {expected:?}", case.name);
                samples.push(serde_json::json!({"tile_uv":[u,v],"screen_px":[point.x,point.y],"expected_rgba":expected,"actual_rgba":actual}));
            }
            let mut outside_pixels = 0usize;
            let mut changed_pixels = 0usize;
            for y in 0..height {
                for x in 0..width {
                    let pixel = &pixels[(y * width + x) * 4..(y * width + x + 1) * 4];
                    let changed = pixel.iter().zip(background).any(|(actual, expected)|actual.abs_diff(expected) > 1);
                    changed_pixels += usize::from(changed);
                    let point = egui::pos2(x as f32 + 0.5, height as f32 - y as f32 - 0.5);
                    if outside_quad(point, &corners, 2.) {
                        outside_pixels += 1;
                        assert!(!changed, "{} camera pixels escaped the actual physical quad at {point:?}", case.name);
                    }
                }
            }
            assert!(outside_pixels > width * height * 3 / 4);
            assert!(changed_pixels > 500, "physical quad must contain a visible image");
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
            std::fs::write(directory.join("receipt.json"), serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
        }
        renderer.destroy();
        target.destroy(&gl);
        read_target.destroy(&gl);
        assert_eq!(gl.get_error(), glow::NO_ERROR);
    }
}
