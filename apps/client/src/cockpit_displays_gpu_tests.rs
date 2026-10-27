//! Opt-in display-pipeline evidence in a private CGL context; no app window.
use super::*;
use std::ffi::{c_void, CString};

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
        let attributes = [99u32, 0x3200, 73, 8, 24, 12, 24, 0];
        assert_eq!(
            CGLChoosePixelFormat(attributes.as_ptr(), &mut format, &mut count),
            0
        );
        assert!(!format.is_null() && count > 0);
        let mut context = std::ptr::null_mut();
        let result = CGLCreateContext(format, std::ptr::null_mut(), &mut context);
        assert_eq!(CGLDestroyPixelFormat(format), 0);
        assert_eq!(result, 0);
        assert_eq!(CGLSetCurrentContext(context), 0);
        let gl = Arc::new(glow::Context::from_loader_function(|name| {
            dlsym(
                (-2_isize) as *mut c_void,
                CString::new(name).unwrap().as_ptr(),
            )
            .cast()
        }));
        (Self { context, previous }, gl)
    }
}
impl Drop for PrivateContext {
    fn drop(&mut self) {
        unsafe {
            let _ = CGLSetCurrentContext(self.previous);
            let _ = CGLDestroyContext(self.context);
        }
    }
}

fn perspective_u(matrix: Mat4, triangle: &[DisplayVertex; 3], pixel: [f32; 2]) -> (f32, f32) {
    let clip = triangle.map(|vertex| matrix * vertex.position.extend(1.0));
    let p = clip.map(|point| egui::vec2(point.x / point.w, point.y / point.w));
    let q = egui::vec2(pixel[0] / 64.0 - 1.0, pixel[1] / 64.0 - 1.0);
    let cross = |a: egui::Vec2, b: egui::Vec2| a.x * b.y - a.y * b.x;
    let area = cross(p[1] - p[0], p[2] - p[0]);
    let b1 = cross(q - p[0], p[2] - p[0]) / area;
    let b2 = cross(p[1] - p[0], q - p[0]) / area;
    let barycentric = [1.0 - b1 - b2, b1, b2];
    assert!(barycentric.iter().all(|weight| *weight > 0.0));
    let affine: f32 = (0..3).map(|i| barycentric[i] * triangle[i].uv.x).sum();
    let numerator: f32 = (0..3)
        .map(|i| barycentric[i] * triangle[i].uv.x / clip[i].w)
        .sum();
    let denominator: f32 = (0..3).map(|i| barycentric[i] / clip[i].w).sum();
    (numerator / denominator, affine)
}

#[test]
#[ignore = "Explicit private macOS CGL display depth/perspective/state test; no app window"]
fn native_cockpit_display_preserves_depth_perspective_and_state() {
    unsafe {
        let (_context, gl) = PrivateContext::create();
        let target = CachedFeed::create(&gl, 991, 128, 128).unwrap();
        let mut painter = egui_glow::Painter::new(gl.clone(), "", None, false).unwrap();
        let mut renderer = CockpitDisplayRenderer::new(gl.clone()).unwrap();
        let texture = egui::TextureId::Managed(911);
        let image = egui::ColorImage {
            size: [256, 1],
            pixels: (0..256)
                .map(|value| egui::Color32::from_gray(value as u8))
                .collect(),
        };
        painter.set_texture(
            texture,
            &egui::epaint::ImageDelta::full(image, egui::TextureOptions::LINEAR),
        );
        let triangle = [
            DisplayVertex {
                position: Vec3::new(-0.8, -0.8, -2.0),
                uv: egui::pos2(0.0, 0.0),
                color: egui::Color32::WHITE,
            },
            DisplayVertex {
                position: Vec3::new(0.8, -0.8, -4.0),
                uv: egui::pos2(1.0, 0.0),
                color: egui::Color32::WHITE,
            },
            DisplayVertex {
                position: Vec3::new(0.8, 0.8, -4.0),
                uv: egui::pos2(1.0, 1.0),
                color: egui::Color32::WHITE,
            },
        ];
        let matrix = Mat4::perspective_rh_gl(std::f32::consts::FRAC_PI_2, 1.0, 1.0, 10.0);
        let (expected_u, affine_u) = perspective_u(matrix, &triangle, [64.5, 55.5]);
        assert!(
            (expected_u - affine_u).abs() > 0.12,
            "sample distinguishes perspective from flattened interpolation"
        );
        let batches = [DisplayBatch {
            texture,
            vertices: triangle.to_vec(),
        }];
        let seed = compile(&gl, SEED_VERTEX, SEED_FRAGMENT).unwrap();
        let vao = gl.create_vertex_array().unwrap();
        let buffer = gl.create_buffer().unwrap();
        let sampler = gl.create_sampler().unwrap();
        let info = || egui::PaintCallbackInfo {
            viewport: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(128.0, 128.0)),
            clip_rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(128.0, 128.0)),
            pixels_per_point: 1.0,
            screen_size_px: [128, 128],
        };
        for (name, seed_z, occludes) in [
            ("baseline", None, false),
            ("foreground", Some(-0.8), true),
            ("background", Some(0.99), false),
            ("near-clipped", Some(-2.0), false),
            ("far-clipped", Some(2.0), false),
        ] {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            gl.viewport(0, 0, 128, 128);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::FRAMEBUFFER_SRGB);
            gl.disable(glow::BLEND);
            gl.disable(glow::CULL_FACE);
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LESS);
            gl.depth_mask(true);
            gl.color_mask(true, true, true, true);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear_depth_f32(1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            gl.use_program(Some(seed));
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
            if let Some(z) = seed_z {
                let points = [
                    [-1.0, -1.0, z],
                    [1.0, -1.0, z],
                    [1.0, 1.0, z],
                    [-1.0, -1.0, z],
                    [1.0, 1.0, z],
                    [-1.0, 1.0, z],
                ];
                let bytes: Vec<u8> = points
                    .into_iter()
                    .flatten()
                    .flat_map(f32::to_ne_bytes)
                    .collect();
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &bytes, glow::STREAM_DRAW);
                gl.enable_vertex_attrib_array(0);
                gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 12, 0);
                gl.draw_arrays(glow::TRIANGLES, 0, 6);
            }
            // Deliberately non-default callback state; the display pass must
            // restore it while using its own known state for actual drawing.
            gl.viewport(3, 7, 105, 110);
            gl.scissor(0, 0, 128, 128);
            gl.enable(glow::SCISSOR_TEST);
            gl.disable(glow::DEPTH_TEST);
            gl.depth_func(glow::GREATER);
            gl.enable(glow::CULL_FACE);
            gl.blend_equation_separate(glow::FUNC_SUBTRACT, glow::FUNC_REVERSE_SUBTRACT);
            gl.blend_func_separate(
                glow::SRC_ALPHA,
                glow::ONE,
                glow::ZERO,
                glow::ONE_MINUS_DST_ALPHA,
            );
            gl.color_mask(false, true, false, true);
            gl.active_texture(glow::TEXTURE0);
            let old_texture = painter.texture(texture);
            gl.bind_texture(glow::TEXTURE_2D, old_texture);
            gl.bind_sampler(0, Some(sampler));
            gl.active_texture(glow::TEXTURE3);
            assert_eq!(gl.get_error(), glow::NO_ERROR, "before display depth query");
            assert!(draw_framebuffer_depth_bits(&gl) > 0);
            assert_eq!(gl.get_error(), glow::NO_ERROR, "draw target depth query");
            renderer.paint(info(), &painter, matrix, &batches);
            assert_eq!(gl.get_error(), glow::NO_ERROR, "{name} display pass");
            let mut viewport = [0; 4];
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
            assert_eq!(viewport, [3, 7, 105, 110]);
            assert!(!gl.is_enabled(glow::DEPTH_TEST));
            assert!(gl.get_parameter_bool(glow::DEPTH_WRITEMASK));
            assert_eq!(gl.get_parameter_i32(glow::DEPTH_FUNC) as u32, glow::GREATER);
            assert!(gl.is_enabled(glow::CULL_FACE));
            assert!(!gl.is_enabled(glow::BLEND));
            assert!(gl.is_enabled(glow::SCISSOR_TEST));
            assert_eq!(
                gl.get_parameter_bool_array::<4>(glow::COLOR_WRITEMASK),
                [false, true, false, true]
            );
            assert_eq!(gl.get_parameter_program(glow::CURRENT_PROGRAM), Some(seed));
            assert_eq!(
                gl.get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING),
                Some(vao)
            );
            assert_eq!(
                gl.get_parameter_buffer(glow::ARRAY_BUFFER_BINDING),
                Some(buffer)
            );
            assert_eq!(
                gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING),
                Some(target.framebuffer)
            );
            assert_eq!(
                gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32,
                glow::TEXTURE3
            );
            gl.active_texture(glow::TEXTURE0);
            assert_eq!(
                gl.get_parameter_texture(glow::TEXTURE_BINDING_2D),
                old_texture
            );
            assert_eq!(
                gl.get_parameter_sampler(glow::SAMPLER_BINDING),
                Some(sampler)
            );
            let mut pixel = [0u8; 4];
            gl.read_pixels(
                64,
                55,
                1,
                1,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixel)),
            );
            if occludes {
                assert!(
                    pixel[0] > 245 && pixel[1] < 5 && pixel[2] < 5,
                    "front triangle hides display: {pixel:?}"
                );
            } else {
                let expected = expected_u * 255.0;
                assert!(
                    (f32::from(pixel[0]) - expected).abs() < 5.0,
                    "{name} perspective sample: {pixel:?}, expected {expected}"
                );
                assert_eq!(pixel[0], pixel[1]);
                assert_eq!(pixel[1], pixel[2]);
            }
            if let Some(directory) = std::env::var_os("ARKLINE_COCKPIT_QA_DIR") {
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory).unwrap();
                let mut rgba = vec![0u8; 128 * 128 * 4];
                gl.read_pixels(
                    0,
                    0,
                    128,
                    128,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut rgba)),
                );
                let top_down: Vec<u8> = rgba
                    .chunks_exact(128 * 4)
                    .rev()
                    .flatten()
                    .copied()
                    .collect();
                image::save_buffer_with_format(
                    directory.join(format!("display-{name}.png")),
                    &top_down,
                    128,
                    128,
                    image::ColorType::Rgba8,
                    image::ImageFormat::Png,
                )
                .unwrap();
            }
            assert_eq!(
                gl.get_error(),
                glow::NO_ERROR,
                "{name} readback and restoration"
            );
        }
        gl.bind_sampler(0, None);
        gl.bind_vertex_array(None);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        gl.use_program(None);
        gl.delete_sampler(sampler);
        gl.delete_buffer(buffer);
        gl.delete_vertex_array(vao);
        gl.delete_program(seed);
        renderer.destroy();
        painter.destroy();
        target.destroy(&gl);
    }
}

#[test]
#[ignore = "Explicit private macOS CGL draw-target depth query regression; no app window"]
fn native_cockpit_depth_query_uses_draw_framebuffer_without_rebinding() {
    unsafe {
        let (_context, gl) = PrivateContext::create();
        let depth = CachedFeed::create(&gl, 992, 32, 32).unwrap();
        let color_only = CachedFeed::create(&gl, 993, 32, 32).unwrap();
        gl.framebuffer_renderbuffer(
            glow::FRAMEBUFFER,
            glow::DEPTH_ATTACHMENT,
            glow::RENDERBUFFER,
            None,
        );
        assert_eq!(
            gl.check_framebuffer_status(glow::FRAMEBUFFER),
            glow::FRAMEBUFFER_COMPLETE
        );
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(depth.framebuffer));
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(color_only.framebuffer));
        assert_eq!(gl.get_error(), glow::NO_ERROR, "depth query fixture");
        assert_eq!(draw_framebuffer_depth_bits(&gl), 0);
        assert_eq!(gl.get_error(), glow::NO_ERROR, "color-only draw target");
        assert_eq!(
            gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING),
            Some(color_only.framebuffer)
        );
        assert_eq!(
            gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING),
            Some(depth.framebuffer)
        );
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(depth.framebuffer));
        let bits = draw_framebuffer_depth_bits(&gl);
        assert!(bits >= 24, "actual depth attachment: {bits}");
        assert_eq!(gl.get_error(), glow::NO_ERROR, "depth draw target");
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, None);
        let default_bits = draw_framebuffer_depth_bits(&gl);
        assert!(default_bits >= 0);
        assert_eq!(gl.get_error(), glow::NO_ERROR, "default draw target");
        assert_eq!(
            gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING),
            None
        );
        assert_eq!(
            gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING),
            Some(depth.framebuffer)
        );
        println!(
            "GL {} / {}: color-only depth0, FBO depth{bits}, default depth{default_bits}",
            gl.get_parameter_string(glow::VERSION),
            gl.get_parameter_string(glow::RENDERER)
        );
        color_only.destroy(&gl);
        depth.destroy(&gl);
    }
}

const SEED_VERTEX: &str =
    "#version 150\nin vec3 position;void main(){gl_Position=vec4(position,1.0);}";
const SEED_FRAGMENT: &str =
    "#version 150\nout vec4 fragment_color;void main(){fragment_color=vec4(1.0,0.0,0.0,1.0);}";
