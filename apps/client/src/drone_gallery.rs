//! Editor inspection of the same cached geometry and materials drawn in flight.
use super::*;
use arkline_protocol::DroneClass;

const DEFAULT_YAW: f32 = 1.15;
const DEFAULT_PITCH: f32 = 0.4;
const VERTICAL_FOV: f32 = std::f32::consts::FRAC_PI_4;

pub struct EmbeddedDroneGallery {
    renderer: Option<Arc<Mutex<Renderer>>>,
    renderer_error: Option<String>,
    model_error: Option<String>,
    selected: DroneClass,
    parts: Arc<Vec<ModelPart>>,
    radius: f32,
    yaw: f32,
    pitch: f32,
    zoom: f32,
}

impl EmbeddedDroneGallery {
    pub fn new(gl: Option<&Arc<glow::Context>>) -> Self {
        let (renderer, renderer_error) = match gl.map(|gl| unsafe { Renderer::new(gl.clone()) }) {
            Some(Ok(renderer)) => (Some(Arc::new(Mutex::new(renderer))), None),
            Some(Err(error)) => (None, Some(error)),
            None => (None, Some("Drone preview requires a GPU context".into())),
        };
        let mut gallery = Self {
            renderer,
            renderer_error,
            model_error: None,
            selected: DroneClass::Recon,
            parts: Arc::new(Vec::new()),
            radius: 1.0,
            yaw: DEFAULT_YAW,
            pitch: DEFAULT_PITCH,
            zoom: 1.0,
        };
        gallery.load_selected();
        gallery
    }

    pub fn selected_class(&self) -> DroneClass {
        self.selected
    }

    pub fn select_class(&mut self, class: DroneClass) {
        if self.selected != class {
            self.selected = class;
            self.load_selected();
            self.reset_framing();
        }
    }

    fn load_selected(&mut self) {
        match preview_parts(self.selected) {
            Ok((parts, radius)) => {
                self.parts = Arc::new(parts);
                self.radius = radius;
                self.model_error = None;
            }
            Err(error) => {
                self.parts = Arc::new(Vec::new());
                self.model_error = Some(error);
            }
        }
    }

    fn reset_framing(&mut self) {
        self.yaw = DEFAULT_YAW;
        self.pitch = DEFAULT_PITCH;
        self.zoom = 1.0;
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> egui::Response {
        ui.heading("Drone models");
        ui.label("Inspect the in-game drone models and materials.");
        ui.horizontal_wrapped(|ui| {
            for class in drone_models::CLASSES {
                if ui
                    .selectable_label(self.selected == class, class_label(class))
                    .clicked()
                {
                    self.select_class(class);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Drag to orbit · scroll to zoom");
            if ui.button("Reset framing").clicked() {
                self.reset_framing();
            }
        });
        self.paint_canvas(ui)
    }

    fn paint_canvas(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(egui::vec2(1.0, 1.0)),
            egui::Sense::drag(),
        );
        let interactive = ui.ctx().pointer_hover_pos().is_some_and(|point| {
            rect.contains(point) && ui.ctx().layer_id_at(point) == Some(ui.layer_id())
        });
        if interactive && response.dragged_by(egui::PointerButton::Primary) {
            let delta = ui.input(|input| input.pointer.delta());
            self.yaw -= delta.x * 0.008;
            self.pitch = (self.pitch + delta.y * 0.006).clamp(-1.3, 1.3);
        }
        if interactive && response.hovered() {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            self.zoom = (self.zoom * (-scroll * 0.002).exp()).clamp(0.4, 4.0);
        }
        // The renderer clears its target. Restrict that callback to the canvas
        // so class selection and Reset remain visible above it.
        let canvas = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        canvas.rect_filled(rect, 0.0, egui::Color32::from_rgb(2, 4, 5));
        if let Some(error) = self.model_error.as_ref().or(self.renderer_error.as_ref()) {
            canvas.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                error,
                egui::FontId::proportional(14.0),
                egui::Color32::LIGHT_RED,
            );
        } else if let Some(renderer) = &self.renderer {
            let (eye, vp) = self.camera(rect.size());
            let renderer = renderer.clone();
            let parts = self.parts.clone();
            canvas.add(egui::PaintCallback {
                rect,
                callback: Arc::new(egui_glow::CallbackFn::new(move |_, _| {
                    if let Ok(mut renderer) = renderer.lock() {
                        unsafe {
                            hangar::paint_preview(&mut renderer, vp, eye, &parts, true);
                            #[cfg(test)]
                            assert_eq!(
                                renderer.gl.get_error(),
                                glow::NO_ERROR,
                                "Drone callback before egui GL error handling"
                            );
                        }
                    }
                })),
            });
        }
        response
    }

    fn camera(&self, size: egui::Vec2) -> (Vec3, Mat4) {
        let aspect = (size.x / size.y.max(1.0)).max(0.01);
        let vertical_half = VERTICAL_FOV * 0.5;
        let horizontal_half = (vertical_half.tan() * aspect).atan();
        let distance = self.radius / vertical_half.min(horizontal_half).sin() * 1.12 * self.zoom;
        let eye = Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        ) * distance;
        let vp = Mat4::perspective_rh_gl(
            VERTICAL_FOV,
            aspect,
            0.001,
            (distance + self.radius * 4.0).max(10.0),
        ) * Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
        (eye, vp)
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

fn class_label(class: DroneClass) -> &'static str {
    match class {
        DroneClass::Recon => "Recon",
        DroneClass::Mining => "Mining",
        DroneClass::Attack => "Attack",
    }
}

fn display_transform(class: DroneClass) -> Result<(Mat4, f32), String> {
    let (min, max) = drone_model_bounds(class);
    let span = (max - min).max_element();
    if !min.is_finite() || !max.is_finite() || span <= 0.0 || (max - min).min_element() < 0.0 {
        return Err("Drone model has invalid bounds".into());
    }
    let center = (min + max) * 0.5;
    let root = Mat4::from_scale(Vec3::splat(span.recip())) * Mat4::from_translation(-center);
    let radius = ((max - min) * (0.5 / span)).length();
    Ok((root, radius))
}

fn preview_parts(class: DroneClass) -> Result<(Vec<ModelPart>, f32), String> {
    let source = cached_drone_parts(class)?;
    let (root, radius) = display_transform(class)?;
    if source.is_empty() {
        return Err("Drone model has no renderable parts".into());
    }
    Ok((
        source
            .iter()
            .map(|&(mesh, model, color, emission)| (mesh, root * model, color, emission))
            .collect(),
        radius,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounded(test: impl FnOnce() + Send + 'static) {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(test));
            let _ = sender.send(result);
        });
        if let Err(panic) = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("Drone gallery interaction frame did not finish within five seconds")
        {
            std::panic::resume_unwind(panic);
        }
    }

    fn frame(
        ctx: &egui::Context,
        gallery: &mut EmbeddedDroneGallery,
        size: egui::Vec2,
        time: &mut f64,
        events: Vec<egui::Event>,
        overlay: Option<egui::Rect>,
    ) -> (egui::FullOutput, egui::Rect) {
        *time += 0.02;
        let mut canvas = egui::Rect::NOTHING;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(*time),
                predicted_dt: 0.02,
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| {
                if let Some(rect) = overlay {
                    egui::Area::new(egui::Id::new("drone_gallery_test_overlay"))
                        .order(egui::Order::Foreground)
                        .fixed_pos(rect.min)
                        .show(ctx, |ui| {
                            ui.allocate_exact_size(rect.size(), egui::Sense::click_and_drag());
                        });
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    canvas = gallery.show(ui).rect;
                });
            },
        );
        (output, canvas)
    }

    fn visible_label(output: &egui::FullOutput, label: &str) -> egui::Rect {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    let rect = text.galley.rect.translate(text.pos.to_vec2());
                    shape.clip_rect.contains_rect(rect).then_some(rect)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Fully visible {label:?} not found"))
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

    fn wheel(point: egui::Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(point),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -6.0),
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    #[test]
    fn all_drone_previews_preserve_exact_runtime_parts_and_materials() {
        assert_eq!(drone_models::CLASSES.len(), 3);
        for class in drone_models::CLASSES {
            let source = cached_drone_parts(class).expect("Runtime drone parts");
            let before = source.as_ref().clone();
            let (preview, _) = preview_parts(class).expect("Drone preview");
            let (root, _) = display_transform(class).unwrap();
            assert_eq!(source.len(), preview.len());
            for (actual, original) in preview.iter().zip(source.iter()) {
                assert_eq!(
                    (actual.0, actual.2, actual.3),
                    (original.0, original.2, original.3)
                );
                assert!(actual.1.abs_diff_eq(root * original.1, 1e-6));
                assert!(actual.1.is_finite());
            }
            assert_eq!(
                *source, before,
                "Viewer must preserve cached runtime models"
            );
        }
    }

    #[test]
    fn selection_orbit_zoom_and_reset_work_through_real_pointer_frames() {
        bounded(|| {
            let ctx = egui::Context::default();
            let mut gallery = EmbeddedDroneGallery::new(None);
            let size = egui::vec2(900.0, 600.0);
            let mut time = 0.0;
            for _ in 0..2 {
                frame(&ctx, &mut gallery, size, &mut time, vec![], None);
            }
            for class in drone_models::CLASSES {
                let (output, _) = frame(&ctx, &mut gallery, size, &mut time, vec![], None);
                let point = visible_label(&output, class_label(class)).center();
                for pressed in [true, false] {
                    frame(
                        &ctx,
                        &mut gallery,
                        size,
                        &mut time,
                        pointer(point, pressed),
                        None,
                    );
                }
                assert_eq!(gallery.selected_class(), class);
                assert_eq!(
                    gallery.parts.len(),
                    cached_drone_parts(class).unwrap().len()
                );
            }
            let (_, canvas) = frame(&ctx, &mut gallery, size, &mut time, vec![], None);
            let start = canvas.center();
            let end = start + egui::vec2(45.0, 25.0);
            frame(
                &ctx,
                &mut gallery,
                size,
                &mut time,
                pointer(start, true),
                None,
            );
            frame(
                &ctx,
                &mut gallery,
                size,
                &mut time,
                vec![egui::Event::PointerMoved(end)],
                None,
            );
            frame(
                &ctx,
                &mut gallery,
                size,
                &mut time,
                pointer(end, false),
                None,
            );
            assert!(gallery.yaw < DEFAULT_YAW);
            assert!(gallery.pitch > DEFAULT_PITCH);
            frame(&ctx, &mut gallery, size, &mut time, wheel(end), None);
            assert!(gallery.zoom > 1.0);
            let (output, _) = frame(&ctx, &mut gallery, size, &mut time, vec![], None);
            let reset = visible_label(&output, "Reset framing").center();
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut gallery,
                    size,
                    &mut time,
                    pointer(reset, pressed),
                    None,
                );
            }
            assert_eq!(
                (gallery.yaw, gallery.pitch, gallery.zoom),
                (DEFAULT_YAW, DEFAULT_PITCH, 1.0)
            );
        });
    }

    #[test]
    fn covered_canvas_and_toolbar_do_not_take_camera_input() {
        bounded(|| {
            let ctx = egui::Context::default();
            let mut gallery = EmbeddedDroneGallery::new(None);
            let size = egui::vec2(900.0, 600.0);
            let mut time = 0.0;
            let mut canvas = egui::Rect::NOTHING;
            for _ in 0..2 {
                canvas = frame(&ctx, &mut gallery, size, &mut time, vec![], None).1;
            }
            for _ in 0..2 {
                frame(&ctx, &mut gallery, size, &mut time, vec![], Some(canvas));
            }
            let original = (gallery.yaw, gallery.pitch, gallery.zoom);
            let start = canvas.center();
            let end = start + egui::vec2(50.0, 20.0);
            frame(
                &ctx,
                &mut gallery,
                size,
                &mut time,
                pointer(start, true),
                Some(canvas),
            );
            frame(
                &ctx,
                &mut gallery,
                size,
                &mut time,
                vec![egui::Event::PointerMoved(end)],
                Some(canvas),
            );
            frame(
                &ctx,
                &mut gallery,
                size,
                &mut time,
                pointer(end, false),
                Some(canvas),
            );
            frame(
                &ctx,
                &mut gallery,
                size,
                &mut time,
                wheel(end),
                Some(canvas),
            );
            assert_eq!((gallery.yaw, gallery.pitch, gallery.zoom), original);
            for _ in 0..2 {
                frame(&ctx, &mut gallery, size, &mut time, vec![], None);
            }
            let (output, _) = frame(&ctx, &mut gallery, size, &mut time, vec![], None);
            let toolbar = visible_label(&output, "Drag to orbit · scroll to zoom").center();
            frame(&ctx, &mut gallery, size, &mut time, wheel(toolbar), None);
            assert_eq!((gallery.yaw, gallery.pitch, gallery.zoom), original);
        });
    }

    #[test]
    fn compact_controls_and_reset_framing_fit_every_runtime_vertex() {
        bounded(|| {
            for size in [
                egui::vec2(280.0, 240.0),
                egui::vec2(360.0, 270.0),
                egui::vec2(900.0, 600.0),
            ] {
                let ctx = egui::Context::default();
                let mut gallery = EmbeddedDroneGallery::new(None);
                let mut time = 0.0;
                for class in drone_models::CLASSES {
                    gallery.select_class(class);
                    frame(&ctx, &mut gallery, size, &mut time, vec![], None);
                    let (output, canvas) = frame(&ctx, &mut gallery, size, &mut time, vec![], None);
                    for label in ["Recon", "Mining", "Attack", "Reset framing"] {
                        let control = visible_label(&output, label);
                        assert!(
                            control.bottom() < canvas.top(),
                            "{label} overlaps canvas at {size:?}"
                        );
                    }
                    assert!(canvas.width() > 0.0 && canvas.height() > 0.0);
                    let (_, vp) = gallery.camera(canvas.size());
                    let geometry = flight_geometry().unwrap();
                    for (mesh, model, _, _) in gallery.parts.iter() {
                        for vertex in geometry.catalog.uploads()[mesh_index(*mesh)].iter() {
                            let point = model.transform_point3(vertex.p);
                            let clip = vp * point.extend(1.0);
                            assert!(clip.w > 0.0);
                            let ndc = clip.truncate() / clip.w;
                            assert!(
                                ndc.is_finite() && ndc.abs().max_element() <= 1.0,
                                "{class:?} vertex outside reset framing at {size:?}: {ndc:?}"
                            );
                        }
                    }
                }
            }
        });
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "drone_model_gpu_tests.rs"]
mod drone_model_gpu_tests;
