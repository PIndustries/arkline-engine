//! Explicit acceptance of the shared model preview callbacks in a private CGL context.
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
            .unwrap_or_else(|| panic!("Visible preview control {label:?} missing"))
    }
}

struct Harness {
    gl: Arc<glow::Context>,
    preview: ModelPreview,
    ctx: egui::Context,
    painter: egui_glow::Painter,
    target: Option<CachedFeed>,
    time: f64,
    size: egui::Vec2,
}
impl Harness {
    unsafe fn new(gl: Arc<glow::Context>) -> Self {
        let preview = ModelPreview::new(Some(&gl));
        assert!(preview.renderer_error.is_none());
        let painter = egui_glow::Painter::new(gl.clone(), "", None, false).unwrap();
        let target = CachedFeed::create(&gl, 8020, WIDTH as i32, HEIGHT as i32).unwrap();
        Self {
            gl,
            preview,
            ctx: egui::Context::default(),
            painter,
            target: Some(target),
            time: 0.0,
            size: egui::vec2(WIDTH as f32, HEIGHT as f32),
        }
    }
    unsafe fn frame(&mut self, events: Vec<egui::Event>) -> Shot {
        self.time += 0.02;
        let mut canvas = egui::Rect::NOTHING;
        let output = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
                time: Some(self.time),
                predicted_dt: 0.02,
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| canvas = self.preview.show(ui).rect);
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
        let expected = usize::from(self.preview.framing.is_some() && self.preview.error.is_none());
        assert_eq!(
            callbacks.len(),
            expected,
            "The selected model alone schedules a GPU callback"
        );
        if let Some(&(clip, rect)) = callbacks.first() {
            assert_eq!(rect, canvas);
            assert!(canvas.top() >= 25.0, "Controls remain above the canvas");
            assert!(canvas.contains_rect(clip) && !clip.is_negative());
        }
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
            "Real preview callback and egui controls"
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
            "Actual preview colour/depth readback"
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
                    "3D depth escaped the clipped preview canvas"
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
        let shot = self.click(previous.label("Reset view"));
        self.frame(vec![egui::Event::PointerMoved(shot.canvas.center())]);
        self.frame(vec![])
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.preview.destroy();
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

fn runtime_hash(model: &RuntimeInspectionModel) -> String {
    let mut hash = Sha256::new();
    let geometry = flight_geometry().unwrap();
    for draw in &model.parts {
        let (mesh, matrix, color, emission) = draw.part;
        hash.update((mesh as u64).to_le_bytes());
        for number in matrix
            .to_cols_array()
            .into_iter()
            .chain(color.to_array())
            .chain([emission])
            .chain(draw.material.parameters())
        {
            hash.update(number.to_le_bytes());
        }
        for vertex in geometry.catalog.uploads()[mesh_index(mesh)].iter() {
            for number in vertex.p.to_array().into_iter().chain(vertex.n.to_array()) {
                hash.update(number.to_le_bytes());
            }
        }
    }
    hex::encode(hash.finalize())
}

fn pointer_button(
    point: egui::Pos2,
    button: egui::PointerButton,
    pressed: bool,
) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(point),
        egui::Event::PointerButton {
            pos: point,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

unsafe fn control_journey(
    harness: &mut Harness,
    name: &str,
    directory: Option<&Path>,
) -> serde_json::Value {
    let first = harness.frame(vec![]);
    harness.frame(vec![egui::Event::PointerMoved(first.canvas.center())]);
    let baseline = harness.frame(vec![]);
    assert!(
        baseline.occupied() > 500,
        "{name}: actual model must be visible"
    );
    let center = baseline.canvas.center();
    let end = center + egui::vec2(65.0, 25.0);
    harness.frame(pointer(center, true));
    harness.frame(vec![egui::Event::PointerMoved(end)]);
    let orbit = harness.frame(pointer(end, false));
    assert!(
        changed_pixels(&baseline, &orbit) > 100 && changed_silhouette(&baseline, &orbit) > 40,
        "{name}: orbit must change actual rendered geometry"
    );
    let reset = harness.reset(&orbit);
    assert_eq!(baseline.mask, reset.mask);
    assert_eq!(
        changed_pixels(&baseline, &reset),
        0,
        "{name}: orbit Reset must reproduce model pixels"
    );
    harness.frame(pointer_button(center, egui::PointerButton::Secondary, true));
    let pan_end = center + egui::vec2(35.0, -20.0);
    harness.frame(vec![egui::Event::PointerMoved(pan_end)]);
    let panned = harness.frame(pointer_button(
        pan_end,
        egui::PointerButton::Secondary,
        false,
    ));
    assert!(harness.preview.camera.pan.length() > 0.01);
    assert!(
        changed_silhouette(&baseline, &panned) > 100,
        "{name}: pan must translate actual model coverage"
    );
    let reset = harness.reset(&panned);
    assert_eq!(baseline.mask, reset.mask);
    assert_eq!(
        changed_pixels(&baseline, &reset),
        0,
        "{name}: pan Reset must reproduce model pixels"
    );
    harness.frame(vec![
        egui::Event::PointerMoved(center),
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
        "{name}: wheel must visibly zoom out"
    );
    // Fit removes zoom/pan while retaining the view direction. Build this
    // state through actual pointer events, then compare against the earlier
    // orbit at default framing rather than a second rendering adapter.
    harness.frame(pointer(center, true));
    harness.frame(vec![egui::Event::PointerMoved(end)]);
    harness.frame(pointer(end, false));
    harness.frame(pointer_button(center, egui::PointerButton::Secondary, true));
    harness.frame(vec![egui::Event::PointerMoved(pan_end)]);
    let before_fit = harness.frame(pointer_button(
        pan_end,
        egui::PointerButton::Secondary,
        false,
    ));
    let direction = (harness.preview.camera.yaw, harness.preview.camera.pitch);
    assert!(harness.preview.camera.pan.length() > 0.01 && harness.preview.camera.zoom > 1.0);
    let fit_clicked = harness.click(before_fit.label("Fit model"));
    harness.frame(vec![egui::Event::PointerMoved(fit_clicked.canvas.center())]);
    let fitted = harness.frame(vec![]);
    assert_eq!(
        (harness.preview.camera.yaw, harness.preview.camera.pitch),
        direction,
        "{name}: Fit must retain the user's orbit direction"
    );
    assert_eq!(
        (harness.preview.camera.zoom, harness.preview.camera.pan),
        (1.0, egui::Vec2::ZERO)
    );
    assert_eq!(
        orbit.mask, fitted.mask,
        "{name}: Fit restores full framing at the retained direction"
    );
    assert_eq!(changed_pixels(&orbit, &fitted), 0);
    let reset = harness.reset(&fitted);
    assert_eq!(baseline.mask, reset.mask);
    assert_eq!(
        changed_pixels(&baseline, &reset),
        0,
        "{name}: Reset after Fit must reproduce the original view"
    );
    harness.size = egui::vec2(320.0, HEIGHT as f32);
    harness.frame(vec![]);
    let narrow = harness.frame(vec![]);
    assert!(
        narrow.occupied() > 30 && narrow.canvas.width() < 320.0,
        "{name}: aspect-aware fit must remain visible in a narrow canvas"
    );
    harness.size = egui::vec2(WIDTH as f32, HEIGHT as f32);
    harness.frame(vec![]);
    serde_json::json!({"model":name,"orbit_changed_pixels":changed_pixels(&baseline,&orbit),"pan_changed_silhouette_pixels":changed_silhouette(&baseline,&panned),"exact_reset":true,"fit_retains_direction_and_restores_framing":true,"captures":[capture(directory,&format!("{name}-default"),&baseline),capture(directory,&format!("{name}-orbit"),&orbit),capture(directory,&format!("{name}-pan"),&panned),capture(directory,&format!("{name}-zoom-out"),&zoomed),capture(directory,&format!("{name}-fit"),&fitted),capture(directory,&format!("{name}-narrow"),&narrow)]})
}

#[test]
#[ignore = "Explicit actual shared runtime/imported preview GPU, switching and pointer acceptance in private macOS CGL"]
fn native_shared_model_preview_catalog_import_materials_controls_and_clear() {
    unsafe {
        let directory =
            std::env::var_os("ARKLINE_MODEL_PREVIEW_EVIDENCE").map(std::path::PathBuf::from);
        if let Some(directory) = &directory {
            std::fs::create_dir(directory).unwrap();
        }
        let (_context, gl) = PrivateContext::create();
        let mut harness = Harness::new(gl.clone());
        let blank = harness.frame(vec![]);
        assert_eq!(blank.occupied(), 0);
        let renderer = harness.preview.renderer.as_ref().unwrap().clone();
        let buffers = renderer
            .lock()
            .unwrap()
            .meshes
            .iter()
            .map(|mesh| (mesh.vao, mesh.buffer, mesh.count))
            .collect::<Vec<_>>();
        let mut rows = Vec::new();
        let mut categories = std::collections::BTreeSet::new();
        for descriptor in model_catalog::runtime_model_catalog()
            .into_iter()
            .filter(|entry| entry.unavailable.is_none())
        {
            let model = model_catalog::load_runtime_model(&descriptor.id).unwrap();
            let before = runtime_hash(&model);
            harness.preview.set_runtime(model.clone()).unwrap();
            harness.frame(vec![]);
            let shot = harness.frame(vec![]);
            assert!(
                shot.occupied() > 5,
                "{} has no actual visible geometry",
                descriptor.id
            );
            assert!(Arc::ptr_eq(
                &renderer,
                harness.preview.renderer.as_ref().unwrap()
            ));
            let locked = renderer.lock().unwrap();
            assert_eq!(
                locked.debug_metrics.get().draw_calls,
                model.parts.len() as u64
            );
            assert_eq!(
                locked.debug_metrics.get().triangles,
                model.info.triangles as u64
            );
            assert_eq!(
                buffers,
                locked
                    .meshes
                    .iter()
                    .map(|mesh| (mesh.vao, mesh.buffer, mesh.count))
                    .collect::<Vec<_>>()
            );
            let last = model.parts.last().unwrap();
            let expected = embedded_primitive(last.part.0).map_or_else(
                || last.material.parameters(),
                |primitive| [primitive.roughness, primitive.metallic, 0.0, 0.0],
            );
            let mut actual = [0.0; 4];
            gl.get_uniform_f32(
                locked.program,
                locked.uniforms.material_params.as_ref().unwrap(),
                &mut actual,
            );
            assert_eq!(
                actual, expected,
                "{} must retain its actual runtime material response",
                descriptor.id
            );
            drop(locked);
            assert_eq!(before, runtime_hash(&model));
            assert!(Arc::ptr_eq(
                &model,
                &model_catalog::load_runtime_model(&descriptor.id).unwrap()
            ));
            let representative = categories.insert(descriptor.category.clone());
            rows.push(serde_json::json!({"id":descriptor.id,"category":descriptor.category,"triangles":model.info.triangles,"batches":model.info.batches,"occupied_depth_pixels":shot.occupied(),"runtime_material_uniforms_match":true,"source_sha256":before,"gpu_buffer_identities_retained":true,"representative_capture":representative.then(||capture(directory.as_deref(),&format!("catalog-{}",rows.len()),&shot))}));
        }
        let mut journeys = Vec::new();
        for id in [
            "ship/pioneer",
            "drone/recon",
            "drone/mining",
            "drone/attack",
            "station/exterior",
            "world/asteroid_ore",
        ] {
            harness
                .preview
                .set_runtime(model_catalog::load_runtime_model(id).unwrap())
                .unwrap();
            journeys.push(control_journey(
                &mut harness,
                &id.replace('/', "-"),
                directory.as_deref(),
            ));
        }
        let mut imported_pixels = Vec::new();
        for (name, bytes) in [
            (
                "wedge",
                include_bytes!("../../editor/tests/fixtures/import/wedge.glb").as_slice(),
            ),
            (
                "spire",
                include_bytes!("../../editor/tests/fixtures/import/spire.glb").as_slice(),
            ),
        ] {
            let geometry = Arc::new(ImportedGeometry::decode(bytes).unwrap());
            let before = geometry.report.clone();
            harness.preview.set_imported(geometry.clone()).unwrap();
            let bindings = harness.preview.imported.debug_mesh_bindings();
            let materials = harness.preview.imported.debug_materials();
            assert!(!bindings.is_empty());
            let gltf = gltf::Gltf::from_slice(bytes).unwrap();
            let material = gltf.materials().next().unwrap();
            let pbr = material.pbr_metallic_roughness();
            let rgba = pbr.base_color_factor();
            let expected = (
                Vec3::new(rgba[0], rgba[1], rgba[2]),
                Vec3::from_array(material.emissive_factor())
                    * material.emissive_strength().unwrap_or(1.0),
                pbr.roughness_factor().max(0.08),
                pbr.metallic_factor(),
            );
            assert_eq!(
                materials[0], expected,
                "GLB scalar materials must survive shared-camera presentation"
            );
            let initial = harness.frame(vec![]);
            imported_pixels.push(initial);
            harness.preview.camera.pan = egui::vec2(0.2, 0.1);
            let moved = harness.preview.camera;
            harness.preview.set_imported(geometry.clone()).unwrap();
            assert_eq!(
                harness.preview.camera, moved,
                "Repeated selection must preserve camera and GPU upload"
            );
            assert_eq!(bindings, harness.preview.imported.debug_mesh_bindings());
            harness.preview.camera = PreviewCamera::default();
            journeys.push(control_journey(&mut harness, name, directory.as_deref()));
            assert_eq!(bindings, harness.preview.imported.debug_mesh_bindings());
            assert_eq!(materials, harness.preview.imported.debug_materials());
            assert_eq!(geometry.report, before);
        }
        assert!(
            changed_pixels(&imported_pixels[0], &imported_pixels[1]) > 100,
            "Imported selection must replace the visible geometry/materials"
        );
        harness.preview.clear();
        let cleared = harness.frame(vec![]);
        assert_eq!(cleared.occupied(), 0);
        assert!(harness.preview.imported.debug_mesh_bindings().is_empty());
        let mut invalid = ImportedGeometry::decode(include_bytes!(
            "../../editor/tests/fixtures/import/wedge.glb"
        ))
        .unwrap();
        invalid.report.min_m[0] = f32::NAN;
        assert!(harness.preview.set_imported(Arc::new(invalid)).is_err());
        let failed = harness.frame(vec![]);
        assert_eq!(
            failed.occupied(),
            0,
            "Loading/error selections must not retain an old model callback"
        );
        if let Some(directory) = &directory {
            std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"schema_version":1,"scope":"Actual ModelPreview::show runtime/imported callbacks, real egui orbit/pan/wheel/Fit/Reset controls, private macOS CGL; no desktop window or uploads","vendor":gl.get_parameter_string(glow::VENDOR),"renderer":gl.get_parameter_string(glow::RENDERER),"gl_version":gl.get_parameter_string(glow::VERSION),"runtime_rendered_count":rows.len(),"runtime_models":rows,"control_journeys":journeys,"clear_and_error_remove_old_geometry":true,"captures_after_clear":[capture(Some(directory),"cleared",&cleared),capture(Some(directory),"invalid-selection",&failed)],"limitations":["GPU buffer identity retention is verified; GPU payload byte-readback invariance is not claimed","Source geometry/material invariance uses CPU hashes and exact uploaded scalar material tuples","This offscreen acceptance does not establish OS input, app focus or simulation authority"]})).unwrap()).unwrap();
        }
        drop(harness);
        assert_eq!(gl.get_error(), glow::NO_ERROR);
    }
}
