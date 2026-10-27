//! Explicit acceptance of the real drone gallery callbacks in a private CGL context.
//! No desktop window is created, captured or driven by this fixture.
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

const WIDTH: usize = 960;
const HEIGHT: usize = 640;

struct Shot {
    output: egui::FullOutput,
    canvas: egui::Rect,
    rgba: Vec<u8>,
    mask: Vec<bool>,
}
impl Shot {
    fn occupied(&self) -> usize {
        self.mask.iter().filter(|&&value| value).count()
    }
    fn label(&self, label: &str) -> egui::Pos2 {
        self.output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    let rect = text.galley.rect.translate(text.pos.to_vec2());
                    shape.clip_rect.contains_rect(rect).then_some(rect.center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Visible gallery control {label:?} missing"))
    }
}

struct Harness {
    gl: Arc<glow::Context>,
    gallery: EmbeddedDroneGallery,
    ctx: egui::Context,
    painter: egui_glow::Painter,
    target: Option<CachedFeed>,
    time: f64,
}
impl Harness {
    unsafe fn new(gl: Arc<glow::Context>) -> Self {
        let gallery = EmbeddedDroneGallery::new(Some(&gl));
        assert!(gallery.renderer_error.is_none() && gallery.model_error.is_none());
        let painter = egui_glow::Painter::new(gl.clone(), "", None, false).unwrap();
        let target = CachedFeed::create(&gl, 8020, WIDTH as i32, HEIGHT as i32).unwrap();
        Self {
            gl,
            gallery,
            ctx: egui::Context::default(),
            painter,
            target: Some(target),
            time: 0.0,
        }
    }
    unsafe fn frame(&mut self, events: Vec<egui::Event>) -> Shot {
        self.time += 0.02;
        let mut canvas = egui::Rect::NOTHING;
        let output = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(WIDTH as f32, HEIGHT as f32),
                )),
                time: Some(self.time),
                predicted_dt: 0.02,
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| canvas = self.gallery.show(ui).rect);
            },
        );
        let callbacks: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Callback(callback) = &shape.shape {
                    Some((shape.clip_rect, callback.rect))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            callbacks.len(),
            1,
            "The actual drone GPU callback must be scheduled"
        );
        let (clip, rect) = callbacks[0];
        assert_eq!(rect, canvas);
        assert!(
            canvas.top() > 60.0,
            "Selection and Reset must stay above the canvas"
        );
        assert!(canvas.contains_rect(clip) && !clip.is_negative());
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
        self.gl.clear_color(0.0, 0.0, 0.0, 1.0);
        self.gl.clear_depth_f32(1.0);
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
            "Real gallery callback and egui controls"
        );
        self.gl
            .bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target.framebuffer));
        let mut rgba = vec![0u8; WIDTH * HEIGHT * 4];
        self.gl.read_pixels(
            0,
            0,
            WIDTH as i32,
            HEIGHT as i32,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut rgba)),
        );
        let mut depths = vec![0u8; WIDTH * HEIGHT * 4];
        self.gl.read_pixels(
            0,
            0,
            WIDTH as i32,
            HEIGHT as i32,
            glow::DEPTH_COMPONENT,
            glow::FLOAT,
            glow::PixelPackData::Slice(Some(&mut depths)),
        );
        assert_eq!(
            self.gl.get_error(),
            glow::NO_ERROR,
            "Actual gallery colour/depth readback"
        );
        let mask = depths
            .chunks_exact(4)
            .enumerate()
            .map(|(index, bytes)| {
                let depth = f32::from_ne_bytes(bytes.try_into().unwrap());
                assert!(depth.is_finite() && (0.0..=1.0).contains(&depth));
                let point = egui::pos2(
                    (index % WIDTH) as f32 + 0.5,
                    (HEIGHT - 1 - index / WIDTH) as f32 + 0.5,
                );
                let occupied = depth < 1.0 - 1e-7;
                assert!(
                    !occupied || canvas.contains(point),
                    "3D depth escaped the clipped gallery canvas"
                );
                occupied
            })
            .collect();
        Shot {
            output,
            canvas,
            rgba,
            mask,
        }
    }
    unsafe fn click(&mut self, point: egui::Pos2) -> Shot {
        self.frame(pointer(point, true));
        self.frame(pointer(point, false))
    }
    unsafe fn reset(&mut self, previous: &Shot) -> Shot {
        let shot = self.click(previous.label("Reset framing"));
        self.frame(vec![egui::Event::PointerMoved(shot.canvas.center())]);
        self.frame(vec![])
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.gallery.destroy();
        self.painter.destroy();
        if let Some(target) = self.target.take() {
            unsafe {
                target.destroy(&self.gl);
            }
        }
    }
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
fn changed_pixels(first: &Shot, second: &Shot) -> usize {
    first
        .rgba
        .chunks_exact(4)
        .zip(second.rgba.chunks_exact(4))
        .enumerate()
        .filter(|(index, (a, b))| (first.mask[*index] || second.mask[*index]) && a != b)
        .count()
}
fn changed_silhouette(first: &Shot, second: &Shot) -> usize {
    first
        .mask
        .iter()
        .zip(&second.mask)
        .filter(|(a, b)| a != b)
        .count()
}
fn source_hash(class: DroneClass) -> String {
    let mut hash = Sha256::new();
    let geometry = flight_geometry().unwrap();
    for &(mesh, matrix, color, emission) in cached_drone_parts(class).unwrap().iter() {
        hash.update((mesh as u64).to_le_bytes());
        for number in matrix
            .to_cols_array()
            .into_iter()
            .chain(color.to_array())
            .chain([emission])
        {
            hash.update(number.to_le_bytes());
        }
        for vertex in geometry.catalog.uploads()[mesh].iter() {
            for number in vertex.p.to_array().into_iter().chain(vertex.n.to_array()) {
                hash.update(number.to_le_bytes());
            }
        }
    }
    hex::encode(hash.finalize())
}
fn capture(directory: Option<&Path>, name: &str, shot: &Shot) -> serde_json::Value {
    let top_down: Vec<u8> = shot
        .rgba
        .chunks_exact(WIDTH * 4)
        .rev()
        .flatten()
        .copied()
        .collect();
    let filename = format!("{name}.png");
    if let Some(directory) = directory {
        image::save_buffer_with_format(
            directory.join(&filename),
            &top_down,
            WIDTH as u32,
            HEIGHT as u32,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .unwrap();
    }
    serde_json::json!({"png":directory.map(|_| filename),"rgba_sha256":hex::encode(Sha256::digest(&top_down)),"occupied_depth_pixels":shot.occupied(),"width":WIDTH,"height":HEIGHT,"orientation":"top-left RGBA8"})
}

#[test]
#[ignore = "Explicit actual drone gallery callbacks and pointer journey in private macOS CGL; no desktop window"]
fn native_drone_gallery_all_classes_orbit_zoom_reset_share_runtime_geometry() {
    unsafe {
        let directory =
            std::env::var_os("ARKLINE_DRONE_MODEL_EVIDENCE").map(std::path::PathBuf::from);
        if let Some(directory) = &directory {
            std::fs::create_dir(directory).unwrap();
        }
        let (_context, gl) = PrivateContext::create();
        let mut harness = Harness::new(gl.clone());
        harness.frame(vec![]);
        let mut shot = harness.frame(vec![]);
        let renderer = harness.gallery.renderer.as_ref().unwrap().clone();
        let initial_buffers = renderer
            .lock()
            .unwrap()
            .meshes
            .iter()
            .map(|mesh| (mesh.vao, mesh.buffer, mesh.count))
            .collect::<Vec<_>>();
        let initial_catalog = flight_geometry().unwrap();
        let mut baselines = Vec::new();
        let mut receipts = Vec::new();
        for class in drone_models::CLASSES {
            shot = harness.click(shot.label(class_label(class)));
            assert_eq!(harness.gallery.selected_class(), class);
            let canvas_center = shot.canvas.center();
            harness.frame(vec![egui::Event::PointerMoved(canvas_center)]);
            let baseline = harness.frame(vec![]);
            assert!(
                baseline.occupied() > 3000,
                "{class:?} must produce a substantial actual model"
            );
            assert!(
                baseline.occupied() < WIDTH * HEIGHT / 2,
                "Default framing must fit the complete model"
            );
            let source = cached_drone_parts(class).unwrap();
            let source_before = source.as_ref().clone();
            let before_hash = source_hash(class);
            let (root, _) = display_transform(class).unwrap();
            assert_eq!(harness.gallery.parts.len(), source.len());
            for (preview, runtime) in harness.gallery.parts.iter().zip(source.iter()) {
                assert_eq!(
                    (preview.0, preview.2, preview.3),
                    (runtime.0, runtime.2, runtime.3)
                );
                assert!(preview.1.abs_diff_eq(root * runtime.1, 1e-6));
            }
            assert!(
                Arc::ptr_eq(&renderer, harness.gallery.renderer.as_ref().unwrap()),
                "Selection must retain the viewer renderer"
            );
            let metrics = renderer.lock().unwrap().debug_metrics.get();
            assert_eq!(metrics.draw_calls, source.len() as u64);
            assert!(metrics.draw_calls <= 5 && metrics.triangles <= 1800);
            let start = baseline.canvas.center();
            let end = start + egui::vec2(90.0, 35.0);
            harness.frame(pointer(start, true));
            harness.frame(vec![egui::Event::PointerMoved(end)]);
            let orbit = harness.frame(pointer(end, false));
            let orbit_pixels = changed_pixels(&baseline, &orbit);
            let orbit_silhouette = changed_silhouette(&baseline, &orbit);
            assert!(
                orbit_pixels > 300 && orbit_silhouette > 100,
                "{class:?}: real orbit must change the geometry view"
            );
            let reset = harness.reset(&orbit);
            assert_eq!(
                changed_pixels(&baseline, &reset),
                0,
                "{class:?}: Reset must restore the actual model pixels"
            );
            assert_eq!(baseline.mask, reset.mask);
            harness.frame(vec![
                egui::Event::PointerMoved(start),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -50.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            for _ in 0..20 {
                harness.frame(vec![]);
            }
            let zoomed = harness.frame(vec![]);
            assert!(
                zoomed.occupied() * 100 < baseline.occupied() * 95,
                "{class:?}: actual wheel input must visibly zoom out"
            );
            assert!(changed_pixels(&baseline, &zoomed) > 300);
            shot = harness.reset(&zoomed);
            assert_eq!(
                changed_pixels(&baseline, &shot),
                0,
                "{class:?}: Reset after zoom must restore model pixels"
            );
            assert_eq!(baseline.mask, shot.mask);
            assert_eq!(source.as_ref(), &source_before);
            assert!(Arc::ptr_eq(&source, &cached_drone_parts(class).unwrap()));
            assert_eq!(before_hash, source_hash(class));
            assert!(Arc::ptr_eq(&initial_catalog, &flight_geometry().unwrap()));
            assert_eq!(
                initial_buffers,
                renderer
                    .lock()
                    .unwrap()
                    .meshes
                    .iter()
                    .map(|mesh| (mesh.vao, mesh.buffer, mesh.count))
                    .collect::<Vec<_>>(),
                "Selection/orbit/zoom must retain actual GPU buffers"
            );
            let label = class_label(class).to_lowercase();
            receipts.push(serde_json::json!({"class":class_label(class),"runtime_parts":source.len(),"triangles":metrics.triangles,"source_sha256":before_hash,"geometry_material_parity":true,"gpu_buffers_retained":true,"orbit_changed_pixels":orbit_pixels,"orbit_changed_silhouette_pixels":orbit_silhouette,"reset_exact_model_pixels":true,"captures":[capture(directory.as_deref(), &format!("{label}-default"), &baseline),capture(directory.as_deref(), &format!("{label}-orbit"), &orbit),capture(directory.as_deref(), &format!("{label}-zoom-out"), &zoomed),capture(directory.as_deref(), &format!("{label}-reset"), &shot)]}));
            baselines.push(baseline);
        }
        for first in 0..baselines.len() {
            for second in first + 1..baselines.len() {
                assert!(
                    changed_silhouette(&baselines[first], &baselines[second]) > 100,
                    "Drone classes must have visibly different geometry, independent of paint"
                );
            }
        }
        if let Some(directory) = &directory {
            std::fs::write(directory.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({"schema_version":1,"scope":"Actual EmbeddedDroneGallery::show, real egui pointer frames and egui_glow GPU callbacks in a private macOS CGL context","vendor":gl.get_parameter_string(glow::VENDOR),"renderer":gl.get_parameter_string(glow::RENDERER),"gl_version":gl.get_parameter_string(glow::VERSION),"selection_orbit_zoom_reset":"All three classes through actual gallery controls","runtime_geometry_and_materials":"Exact shared catalog mesh slots, color/emission tuples and immutable source vertices; display normalization only","cases":receipts,"limitations":["Offscreen renderer acceptance does not establish desktop OS input, native app focus, or running simulation authority","No existing project, gameplay specifications, drone kinematics, canonical ship asset or desktop window is changed"]})).unwrap()).unwrap();
        }
        drop(harness);
        assert_eq!(gl.get_error(), glow::NO_ERROR);
    }
}
