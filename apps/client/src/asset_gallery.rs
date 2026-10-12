//! Editor-only asset comparison. No world entities, ownership or gameplay requests.
use super::*;

pub struct EmbeddedAssetGallery {
    renderer: Option<Arc<Mutex<Renderer>>>,
    error: Option<String>,
    selected: ShipClass,
    parts: Arc<Vec<ModelPart>>,
    yaw: f32,
    pitch: f32,
    zoom: f32,
}
impl EmbeddedAssetGallery {
    pub fn new(gl: Option<&Arc<glow::Context>>) -> Self {
        let result = gl.map(|gl| unsafe { Renderer::new(gl.clone()) });
        let (renderer, error) = match result {
            Some(Ok(r)) => (Some(Arc::new(Mutex::new(r))), None),
            Some(Err(e)) => (None, Some(e)),
            None => (None, Some("Asset comparison requires a GPU context".into())),
        };
        Self {
            renderer,
            error,
            selected: ShipClass::Pioneer,
            parts: Arc::new(comparison_parts(ShipClass::Pioneer)),
            yaw: 1.15,
            pitch: 0.4,
            zoom: 1.0,
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, preview: Option<crate::EmbeddedMaterialPreview>) {
        ui.heading("Ship asset comparison");
        ui.label("Asset comparison — not world scale. Two instances of the selected asset; no ships are spawned.");
        ui.horizontal_wrapped(|ui| {
            for class in arkline_protocol::ALL_SHIP_CLASSES {
                if ui
                    .selectable_label(self.selected == class, class.spec().name)
                    .clicked()
                {
                    self.selected = class;
                    self.parts = Arc::new(comparison_parts(class));
                }
            }
        });
        let spec = self.selected.spec();
        ui.label(format!(
            "Source dimensions: {:.0} m long × {:.0} m wide × {:.0} m high. {}",
            spec.length_m,
            spec.width_m,
            spec.height_m,
            if self.selected == ShipClass::Pioneer {
                "Embedded GLB asset"
            } else {
                "Procedural class model"
            }
        ));
        ui.horizontal_wrapped(|ui| {
            ui.label("Drag to orbit · scroll to zoom");
            if ui.button("Reset camera").clicked() {
                self.yaw = 1.15;
                self.pitch = 0.4;
                self.zoom = 1.;
            }
            ui.label(if preview.is_some() {
                "Shared live material override on both instances"
            } else {
                "Authored materials on both instances"
            });
        });
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(egui::vec2(1., 1.)),
            egui::Sense::drag(),
        );
        // The callback clears color/depth: clip it to the canvas, never the
        // parent panel containing class buttons and source dimensions.
        let canvas = ui.painter().with_clip_rect(rect);
        let interactive = ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|p| rect.contains(p) && ui.ctx().layer_id_at(p) == Some(ui.layer_id()));
        if interactive && response.dragged() {
            let delta = ui.input(|i| i.pointer.delta());
            self.yaw -= delta.x * 0.008;
            self.pitch = (self.pitch + delta.y * 0.006).clamp(-0.8, 1.2);
        }
        if interactive && response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            self.zoom = (self.zoom * (-scroll * 0.002).exp()).clamp(0.55, 2.5);
        }
        if let Some(renderer) = &self.renderer {
            if let Ok(mut r) = renderer.lock() {
                r.set_material_preview(preview);
            }
            let eye = Vec3::new(
                self.yaw.cos() * self.pitch.cos(),
                self.pitch.sin(),
                self.yaw.sin() * self.pitch.cos(),
            ) * (3.2 * self.zoom);
            let vp = Mat4::perspective_rh_gl(
                45f32.to_radians(),
                (rect.width() / rect.height()).max(0.01),
                0.01,
                100.,
            ) * Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
            let renderer = renderer.clone();
            let parts = self.parts.clone();
            canvas.add(egui::PaintCallback {
                rect,
                callback: Arc::new(egui_glow::CallbackFn::new(move |_, _| {
                    if let Ok(renderer) = renderer.lock() {
                        unsafe {
                            hangar::paint(&renderer, vp, eye, &parts);
                        }
                    }
                })),
            });
            canvas.text(
                rect.left_bottom() + egui::vec2(12., -12.),
                egui::Align2::LEFT_BOTTOM,
                "Same asset and material policy · display-normalized geometry · no authority state",
                egui::FontId::proportional(12.),
                egui::Color32::LIGHT_GRAY,
            );
        } else {
            canvas.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                self.error
                    .as_deref()
                    .unwrap_or("Asset renderer unavailable"),
                egui::FontId::proportional(16.),
                egui::Color32::LIGHT_RED,
            );
        }
    }
    pub fn destroy(&mut self) {
        if let Some(renderer) = self.renderer.take() {
            if let Ok(mut renderer) = renderer.lock() {
                unsafe {
                    renderer.destroy();
                }
            }
        }
    }
}

fn comparison_parts(class: ShipClass) -> Vec<ModelPart> {
    // Runtime models use 100 m units. Normalize body length to one display unit,
    // preserving aspect ratio, class geometry, material role and instance parity.
    let scale = (100. / class.spec().length_m) as f32;
    [-0.68, 0.68]
        .into_iter()
        .flat_map(|x| {
            let root =
                Mat4::from_translation(Vec3::new(x, 0., 0.)) * Mat4::from_scale(Vec3::splat(scale));
            cached_ship_parts(class)
                .iter()
                .map(move |&(mesh, model, color, emission)| (mesh, root * model, color, emission))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_classes_use_two_identical_cached_assets_with_only_display_transform_changed() {
        for class in arkline_protocol::ALL_SHIP_CLASSES {
            let source = cached_ship_parts(class);
            let parts = comparison_parts(class);
            assert_eq!(parts.len(), source.len() * 2);
            for (instance, x) in [-0.68, 0.68].into_iter().enumerate() {
                let root = Mat4::from_translation(Vec3::new(x, 0., 0.))
                    * Mat4::from_scale(Vec3::splat((100. / class.spec().length_m) as f32));
                for (actual, original) in parts
                    [instance * source.len()..(instance + 1) * source.len()]
                    .iter()
                    .zip(source)
                {
                    assert_eq!(actual.0, original.0);
                    assert_eq!(actual.2, original.2);
                    assert_eq!(actual.3, original.3);
                    assert!(actual.1.abs_diff_eq(root * original.1, 1e-5));
                    assert!(actual.1.is_finite());
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "Explicit offscreen actual asset gallery render"]
    fn native_gallery_two_instances_render_and_preview_without_mutating_source() {
        use std::ffi::{c_void, CString};
        #[link(name = "OpenGL", kind = "framework")]
        unsafe extern "C" {
            fn CGLChoosePixelFormat(a: *const u32, f: *mut *mut c_void, n: *mut i32) -> i32;
            fn CGLCreateContext(f: *mut c_void, s: *mut c_void, c: *mut *mut c_void) -> i32;
            fn CGLSetCurrentContext(c: *mut c_void) -> i32;
            fn CGLDestroyContext(c: *mut c_void) -> i32;
            fn CGLDestroyPixelFormat(f: *mut c_void) -> i32;
        }
        unsafe extern "C" {
            fn dlsym(h: *mut c_void, n: *const std::ffi::c_char) -> *mut c_void;
        }
        unsafe {
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
            let mut context = std::ptr::null_mut();
            assert_eq!(
                CGLCreateContext(format, std::ptr::null_mut(), &mut context),
                0
            );
            CGLDestroyPixelFormat(format);
            assert_eq!(CGLSetCurrentContext(context), 0);
            let gl = Arc::new(glow::Context::from_loader_function(|name| {
                dlsym(
                    (-2_isize) as *mut c_void,
                    CString::new(name).unwrap().as_ptr(),
                )
                .cast()
            }));
            let mut renderer = Renderer::new(gl.clone()).unwrap();
            let target = CachedFeed::create(&gl, 7001, 256, 160).unwrap();
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            gl.viewport(0, 0, 256, 160);
            gl.disable(glow::SCISSOR_TEST);
            gl.color_mask(true, true, true, true);
            let eye = Vec3::new(0., 1.4, 3.);
            let vp = Mat4::perspective_rh_gl(45f32.to_radians(), 256. / 160., 0.01, 100.)
                * Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
            let read = || {
                let mut pixels = vec![0u8; 256 * 160 * 4];
                gl.read_pixels(
                    0,
                    0,
                    256,
                    160,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut pixels)),
                );
                assert_eq!(gl.get_error(), glow::NO_ERROR);
                pixels
            };
            let parts = comparison_parts(ShipClass::Pioneer);
            hangar::paint(&renderer, vp, eye, &parts);
            let original = read();
            renderer.set_material_preview(Some(crate::EmbeddedMaterialPreview {
                hull_rgb: [0.8, 0.04, 0.02],
                trim_rgb: [0.8, 0.2, 0.02],
                roughness: Some(0.5),
                metallic: Some(0.),
                ..Default::default()
            }));
            hangar::paint(&renderer, vp, eye, &parts);
            let edited = read();
            for half in 0..2 {
                let changed = original
                    .chunks_exact(4)
                    .zip(edited.chunks_exact(4))
                    .enumerate()
                    .filter(|(i, (a, b))| i % 256 / 128 == half && a != b)
                    .count();
                assert!(
                    changed > 100,
                    "Both actual asset instances must visibly change: {half}: {changed}"
                );
            }
            renderer.set_material_preview(None);
            hangar::paint(&renderer, vp, eye, &parts);
            assert_eq!(
                original,
                read(),
                "Reset must reproduce original cached asset pixels"
            );
            // Verify the actual UI callback clip excludes the controls above it.
            let ctx = egui::Context::default();
            let mut gallery = EmbeddedAssetGallery::new(Some(&gl));
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800., 600.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| gallery.show(ui, None));
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
            assert_eq!(callbacks.len(), 1);
            let (clip, canvas) = callbacks[0];
            assert!(canvas.top() > 60., "Controls must be outside the canvas");
            assert!(clip.top() >= canvas.top() && clip.bottom() <= canvas.bottom());
            assert!(clip.left() >= canvas.left() && clip.right() <= canvas.right());
            gallery.destroy();
            target.destroy(&gl);
            renderer.destroy();
            CGLSetCurrentContext(std::ptr::null_mut());
            CGLDestroyContext(context);
        }
    }
}
