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
        match draw.range {
            Some((first, count)) => {
                hash.update([1]);
                hash.update((first as u64).to_le_bytes());
                hash.update((count as u64).to_le_bytes());
            }
            None => hash.update([0]),
        }
        for number in matrix
            .to_cols_array()
            .into_iter()
            .chain(color.to_array())
            .chain([emission])
            .chain(draw.material.parameters())
        {
            hash.update(number.to_le_bytes());
        }
        let vertices = draw
            .selected_vertices(&geometry.catalog.uploads()[mesh_index(mesh)])
            .expect("Runtime hash uses only the selected source vertex range");
        for vertex in vertices {
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
        let catalog = model_catalog::runtime_model_catalog();
        assert_eq!(catalog.len(), 111);
        let unavailable = catalog
            .iter()
            .filter(|entry| entry.unavailable.is_some())
            .collect::<Vec<_>>();
        assert_eq!(
            unavailable.len(),
            3,
            "Only logical equipment remains without geometry"
        );
        let added_ids = catalog
            .iter()
            .filter(|entry| {
                entry.id.starts_with("room/")
                    || entry.id.starts_with("exterior/pioneer.exterior.window.")
            })
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(added_ids.len(), 16);
        let mut rows = Vec::new();
        let mut categories = std::collections::BTreeSet::new();
        let mut window_ranges = std::collections::BTreeSet::new();
        let mut window_bounds = std::collections::BTreeSet::new();
        let mut room_count = 0;
        for descriptor in catalog.iter().filter(|entry| entry.unavailable.is_none()) {
            let model = model_catalog::load_runtime_model(&descriptor.id).unwrap();
            let before = runtime_hash(&model);
            if descriptor
                .id
                .starts_with("exterior/pioneer.exterior.window.")
            {
                assert_eq!((model.info.triangles, model.info.batches), (12, 1));
                assert_eq!(model.parts.len(), 1);
                let draw = &model.parts[0];
                let range = draw
                    .range
                    .expect("Each authored pane selects a source vertex range");
                assert_eq!(range.1, 36);
                assert!(
                    window_ranges.insert(range),
                    "Window pane source ranges must differ"
                );
                assert_eq!(
                    mesh_index(draw.part.0),
                    10 + 6,
                    "All panes retain the original Navigation glass upload"
                );
                let source = flight_geometry().unwrap();
                let full = &source.catalog.uploads()[mesh_index(draw.part.0)];
                let selected = draw.selected_vertices(full).unwrap();
                assert_eq!(selected.len(), 36);
                assert!(full.len() > selected.len());
                let (min, max) = selected.iter().fold(
                    (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
                    |(min, max), vertex| {
                        let point = draw.part.1.transform_point3(vertex.p) * 100.0;
                        (min.min(point), max.max(point))
                    },
                );
                assert_eq!(model.info.min_m, min.to_array());
                assert_eq!(model.info.max_m, max.to_array());
                let bounds = model
                    .info
                    .min_m
                    .into_iter()
                    .chain(model.info.max_m)
                    .map(f32::to_bits)
                    .collect::<Vec<_>>();
                assert!(
                    window_bounds.insert(bounds),
                    "The four panes retain distinct original body-space bounds"
                );
            }
            if descriptor.id.starts_with("room/") {
                room_count += 1;
                assert!(model.info.triangles > 12 && model.parts.len() > 1,
                    "Room preview must retain the actual multi-part wall, doorway and obstacle recipe");
                assert!(model.parts.iter().all(|draw| draw.range.is_none()));
            }
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
            rows.push(serde_json::json!({"id":descriptor.id,"category":descriptor.category,"source":descriptor.source,"source_notes":descriptor.notes,"triangles":model.info.triangles,"batches":model.info.batches,"min_m":model.info.min_m,"max_m":model.info.max_m,"vertex_ranges":model.parts.iter().map(|draw|draw.range).collect::<Vec<_>>(),"occupied_depth_pixels":shot.occupied(),"runtime_material_uniforms_match":true,"selected_source_sha256":before,"gpu_buffer_identities_retained":true,"representative_capture":representative.then(||capture(directory.as_deref(),&format!("catalog-{}",rows.len()),&shot))}));
        }
        assert_eq!(rows.len(), 108);
        assert_eq!(room_count, 12);
        assert_eq!(
            window_ranges,
            [(72, 36), (108, 36), (216, 36), (252, 36)]
                .into_iter()
                .collect()
        );
        assert_eq!(window_bounds.len(), 4);
        harness
            .preview
            .set_runtime(
                model_catalog::load_runtime_model("exterior/pioneer.exterior.window.port.forward")
                    .unwrap(),
            )
            .unwrap();
        renderer.lock().unwrap().debug_wireframe = true;
        let wireframe = harness.frame(vec![]);
        let metrics = renderer.lock().unwrap().debug_metrics.get();
        assert_eq!(
            (
                metrics.draw_calls,
                metrics.triangles,
                metrics.wireframe_draw_calls
            ),
            (1, 12, 1),
            "Wireframe must use the same authored pane range as the fill pass"
        );
        assert!(wireframe.occupied() > 5);
        renderer.lock().unwrap().debug_wireframe = false;
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
        let wireframe_capture = capture(
            directory.as_deref(),
            "authored-window-range-wireframe",
            &wireframe,
        );
        let mut journeys = Vec::new();
        let mut journey_ids = [
            "ship/pioneer",
            "drone/recon",
            "drone/mining",
            "drone/attack",
            "station/exterior",
            "world/asteroid_ore",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        journey_ids.extend(added_ids);
        for id in journey_ids {
            harness
                .preview
                .set_runtime(model_catalog::load_runtime_model(&id).unwrap())
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
            std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"schema_version":2,"scope":"Actual ModelPreview::show runtime/imported callbacks, real egui orbit/pan/wheel/Fit/Reset controls, private macOS CGL; no desktop window or uploads","vendor":gl.get_parameter_string(glow::VENDOR),"renderer":gl.get_parameter_string(glow::RENDERER),"gl_version":gl.get_parameter_string(glow::VERSION),"runtime_catalog_count":catalog.len(),"runtime_rendered_count":rows.len(),"unavailable_logical_entries":unavailable.iter().map(|entry|serde_json::json!({"id":entry.id,"reason":entry.unavailable})).collect::<Vec<_>>(),"room_previews":room_count,"authored_window_previews":window_bounds.len(),"runtime_models":rows,"window_range_wireframe_capture":wireframe_capture,"control_journeys":journeys,"clear_and_error_remove_old_geometry":true,"captures_after_clear":[capture(Some(directory),"cleared",&cleared),capture(Some(directory),"invalid-selection",&failed)],"limitations":["GPU buffer identity retention is verified; GPU payload byte-readback invariance is not claimed","Source geometry/material invariance uses CPU hashes and exact uploaded scalar material tuples","Room cutaways use actual per-room wall, storage, doorway and equipment geometry; shared deck floors, ceilings, corridor ribs, lift/access core and deck perimeter context are omitted, with no proxy floor","Window previews select authored Navigation glass source ranges from the unchanged shared GPU buffer; mappings apply to this asset snapshot","This offscreen acceptance does not establish OS input, app focus or simulation authority"]})).unwrap()).unwrap();
        }
        drop(harness);
        assert_eq!(gl.get_error(), glow::NO_ERROR);
    }
}

/// Read the actual preview upload for the source cube, rather than assuming
/// that a preview-only normal flag produced finite, correctly oriented data.
unsafe fn inspect_generated_normal_upload(
    gl: &glow::Context,
    bindings: &[(glow::VertexArray, glow::Buffer, i32)],
    report: &imported_asset::ImportReport,
) -> serde_json::Value {
    let previous = gl.get_parameter_buffer(glow::ARRAY_BUFFER_BINDING);
    let mut corners = 0usize;
    let mut normal_hash = Sha256::new();
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for &(_, buffer, count) in bindings {
        assert!(count > 0 && count % 3 == 0);
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
        let mut bytes = vec![0u8; count as usize * 24];
        gl.get_buffer_sub_data(glow::ARRAY_BUFFER, 0, &mut bytes);
        for face in bytes.chunks_exact(3 * 24) {
            let vertices = face
                .chunks_exact(24)
                .map(|vertex| {
                    let values = vertex
                        .chunks_exact(4)
                        .map(|number| f32::from_ne_bytes(number.try_into().unwrap()))
                        .collect::<Vec<_>>();
                    let position = Vec3::new(values[0], values[1], values[2]);
                    let normal = Vec3::new(values[3], values[4], values[5]);
                    assert!(position.is_finite() && normal.is_finite());
                    assert!((normal.length() - 1.0).abs() < 1e-5);
                    min = min.min(position);
                    max = max.max(position);
                    normal_hash.update(&vertex[12..]);
                    (position, normal)
                })
                .collect::<Vec<_>>();
            let face_normal = (vertices[1].0 - vertices[0].0)
                .cross(vertices[2].0 - vertices[0].0)
                .normalize();
            assert!(face_normal.is_finite());
            assert!(
                vertices
                    .iter()
                    .all(|(_, normal)| normal.dot(face_normal) > 0.9999),
                "Generated preview normals must follow each actual transformed triangle"
            );
            corners += vertices.len();
        }
    }
    gl.bind_buffer(glow::ARRAY_BUFFER, previous);
    assert_eq!(gl.get_error(), glow::NO_ERROR);
    assert_eq!(corners, report.triangles * 3);
    assert_eq!(min.to_array(), report.min_m);
    assert_eq!(max.to_array(), report.max_m);
    serde_json::json!({"readback_corners":corners,"finite_unit_face_normals":true,
        "normals_follow_triangle_winding":true,"meter_bounds_match_report":true,
        "normal_bytes_sha256":hex::encode(normal_hash.finalize())})
}

#[test]
#[ignore = "Explicit actual repository GLB files through shared imported GPU preview; no desktop window"]
fn native_shared_model_preview_actual_repository_glbs_and_preview_normals() {
    unsafe {
        let directory =
            std::env::var_os("ARKLINE_REPOSITORY_GLB_EVIDENCE").map(std::path::PathBuf::from);
        if let Some(directory) = &directory {
            std::fs::create_dir(directory).unwrap();
        }
        let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let (_context, gl) = PrivateContext::create();
        let mut harness = Harness::new(gl.clone());
        harness.frame(vec![]);
        let mut rows = Vec::new();
        let mut sources = std::collections::BTreeSet::new();
        for (relative, triangles, batches) in [
            ("assets/ships/pioneer/pioneer.glb", 103_466, 9),
            ("assets/ships/pioneer/studies/pioneer-study.glb", 78_246, 9),
            (
                "assets/ships/pioneer/studies/study05/pioneer-study05.glb",
                103_466,
                9,
            ),
        ] {
            // Match browser loading: remove the previous model before validating
            // the selected file, then use the same verified imported backend.
            harness.preview.clear();
            let path = repository.join(relative);
            let bytes = std::fs::read(&path).unwrap();
            let sha256 = hex::encode(Sha256::digest(&bytes));
            sources.insert(sha256.clone());
            let geometry = Arc::new(ImportedGeometry::decode_preview(&bytes).unwrap());
            assert!(!geometry.uses_generated_normals());
            let report = geometry.report.clone();
            assert_eq!(
                ImportedGeometry::decode(&bytes).unwrap().report,
                report,
                "Sources with normals keep the strict report unchanged"
            );
            assert_eq!(report.triangles, triangles, "{relative}");
            assert_eq!(report.primitives, batches, "{relative}");
            let (min, max, corners) = geometry.triangle_positions_m().fold(
                (
                    Vec3::splat(f32::INFINITY),
                    Vec3::splat(f32::NEG_INFINITY),
                    0usize,
                ),
                |(min, max, count), position| {
                    let point = Vec3::from_array(position);
                    assert!(point.is_finite());
                    (min.min(point), max.max(point), count + 1)
                },
            );
            assert_eq!(report.min_m, min.to_array());
            assert_eq!(report.max_m, max.to_array());
            assert_eq!(corners, triangles * 3);
            let source = ship_asset::decode(&bytes).unwrap();
            assert_eq!(
                report.materials,
                source
                    .iter()
                    .map(|part| part.material_name.clone())
                    .collect::<Vec<_>>()
            );
            let expected_materials = source
                .iter()
                .map(|part| {
                    (
                        part.color,
                        part.emissive_color * part.emission,
                        part.roughness,
                        part.metallic,
                    )
                })
                .collect::<Vec<_>>();
            harness.preview.set_imported(geometry.clone()).unwrap();
            let bindings = harness.preview.imported.debug_mesh_bindings();
            assert_eq!(bindings.len(), batches);
            assert_eq!(
                bindings
                    .iter()
                    .map(|binding| binding.2 as usize)
                    .collect::<Vec<_>>(),
                source
                    .iter()
                    .map(|part| part.vertices.len())
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                harness.preview.imported.debug_materials(),
                expected_materials,
                "{relative}: every uploaded batch must retain its source GLB scalar material"
            );
            harness.frame(vec![]);
            let shot = harness.frame(vec![]);
            assert!(
                shot.occupied() > 500,
                "{relative}: source geometry must be visible"
            );
            assert!(
                matches!(&harness.preview.source, PreviewSource::Imported(current) if Arc::ptr_eq(current, &geometry))
            );
            harness.preview.set_imported(geometry.clone()).unwrap();
            assert_eq!(bindings, harness.preview.imported.debug_mesh_bindings());
            assert_eq!(geometry.report, report);
            assert_eq!(
                sha256,
                hex::encode(Sha256::digest(std::fs::read(&path).unwrap()))
            );
            rows.push(serde_json::json!({
                "source_path":relative,
                "source_sha256":sha256,
                "source_bytes":bytes.len(),
                "report":report,
                "preview_only_generated_normals":false,
                "report_matches_decoded_meter_vertices":true,
                "uploaded_materials_match_all_source_batches":true,
                "repeated_selection_retains_gpu_bindings":true,
                "source_file_unchanged":true,
                "capture":capture(directory.as_deref(), &format!("repository-glb-{}", rows.len()), &shot),
            }));
        }
        // Only explicit preview decoding generates normals for this source.
        // Strict authoring validation still rejects the unchanged original bytes.
        let relative = "assets/examples/calibration-cube.glb";
        let path = repository.join(relative);
        let bytes = std::fs::read(&path).unwrap();
        let sha256 = hex::encode(Sha256::digest(&bytes));
        let strict_error = match ImportedGeometry::decode(&bytes) {
            Ok(_) => panic!("Strict calibration import without NORMAL unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(strict_error, "Missing ship normals");
        harness.preview.clear();
        let geometry = Arc::new(ImportedGeometry::decode_preview(&bytes).unwrap());
        assert!(geometry.uses_generated_normals());
        let report = geometry.report.clone();
        assert_eq!((report.triangles, report.primitives), (12, 1));
        assert!(report.notes.iter().any(|note| note == "Preview only: missing NORMAL attributes were replaced with generated flat face normals. Original asset bytes are unchanged; this geometry cannot be staged, accepted or saved."));
        harness.preview.set_imported(geometry.clone()).unwrap();
        let bindings = harness.preview.imported.debug_mesh_bindings();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].2, 36);
        let normal_readback = inspect_generated_normal_upload(&gl, &bindings, &report);
        harness.frame(vec![]);
        let calibration = harness.frame(vec![]);
        assert!(calibration.occupied() > 500);
        assert_eq!(geometry.report, report);
        assert_eq!(
            sha256,
            hex::encode(Sha256::digest(std::fs::read(&path).unwrap()))
        );
        sources.insert(sha256.clone());
        rows.push(serde_json::json!({"source_path":relative,"source_sha256":sha256,
            "source_bytes":bytes.len(),"report":report,"preview_only_generated_normals":true,
            "strict_authoring_error":strict_error,"normal_upload_readback":normal_readback,
            "source_file_unchanged":true,"capture":capture(directory.as_deref(),"calibration-generated-face-normals",&calibration)}));
        assert_eq!(rows.len(), 4);

        // Reject malformed source geometry even in preview mode. This modifies
        // only an in-memory copy of the cube's actual POSITION buffer.
        let document = gltf::Gltf::from_slice(&bytes).unwrap();
        let primitive = document
            .meshes()
            .next()
            .unwrap()
            .primitives()
            .next()
            .unwrap();
        let positions = primitive.get(&gltf::Semantic::Positions).unwrap();
        let view = positions.view().unwrap();
        let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let binary_start = 20 + json_length + 8;
        let stride = view.stride().unwrap_or(positions.size());
        let mut invalid = bytes.clone();
        for index in 0..positions.count() {
            let start = binary_start + view.offset() + positions.offset() + index * stride;
            invalid[start..start + 12].fill(0);
        }
        harness.preview.clear();
        let invalid_error = match ImportedGeometry::decode_preview(&invalid) {
            Ok(_) => panic!("Degenerate source geometry unexpectedly received a preview fallback"),
            Err(error) => error,
        };
        assert_eq!(invalid_error, "Degenerate ship triangle");
        let rejected = harness.frame(vec![]);
        assert!(matches!(harness.preview.source, PreviewSource::Empty));
        assert_eq!(rejected.occupied(), 0);
        assert!(harness.preview.imported.debug_mesh_bindings().is_empty());
        assert_eq!(
            sha256,
            hex::encode(Sha256::digest(std::fs::read(&path).unwrap()))
        );
        if let Some(directory) = &directory {
            std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version":2,
                "scope":"Four actual repository source paths through explicit preview decoding, shared ModelPreview callbacks and private macOS CGL color/depth readback",
                "vendor":gl.get_parameter_string(glow::VENDOR),"renderer":gl.get_parameter_string(glow::RENDERER),"gl_version":gl.get_parameter_string(glow::VERSION),
                "rendered_source_paths":rows.len(),"distinct_rendered_content_hashes":sources.len(),"sources":rows,
                "strict_calibration_import_rejection":strict_error,
                "rejected_invalid_source":{"source":"In-memory degenerate POSITION copy of calibration-cube.glb","error":invalid_error,
                    "old_model_callback_removed":true,"no_geometry_fallback":true,"original_source_file_unchanged":true,
                    "capture":capture(Some(directory),"invalid-source-geometry-cleared",&rejected)},
                "limitations":["Canonical Pioneer and study05 are currently identical content at distinct source paths; no distinct shape is claimed",
                    "Calibration generated flat normals are explicit preview presentation only; strict import still rejects it and no source bytes are changed",
                    "Actual calibration GPU vertices/normals were read once to verify finite unit face normals and measured bounds; GPU payload invariance is not claimed",
                    "Offscreen source-file acceptance does not establish desktop OS input or a running game session"]
            })).unwrap()).unwrap();
        }
        drop(harness);
        assert_eq!(gl.get_error(), glow::NO_ERROR);
    }
}
