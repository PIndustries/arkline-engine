//! One inspection camera and canvas for exact runtime and verified imported models.
//! This widget owns no simulation state and issues no gameplay or save commands.
use super::*;
use imported_asset::{ImportedAssetPreview, ImportedGeometry};
use model_catalog::RuntimeInspectionModel;

#[cfg(all(test, target_os = "macos"))]
#[path = "model_preview_gpu_tests.rs"]
mod native_tests;

#[cfg(test)]
#[path = "model_components_tests.rs"]
mod component_tests;

const DEFAULT_YAW: f32 = 1.15;
const DEFAULT_PITCH: f32 = 0.4;
const FIELD_OF_VIEW: f32 = 45.0_f32.to_radians();

#[derive(Clone, Copy, Debug)]
struct Framing {
    display: Mat4,
    radius: f32,
}
impl Framing {
    fn bounds(min: Vec3, max: Vec3) -> Result<Self, String> {
        if !min.is_finite() || !max.is_finite() || (max - min).min_element() < 0.0 {
            return Err("Model bounds are non-finite or inverted".into());
        }
        // Do arithmetic in f64 so tiny/large source coordinates do not overflow
        // while centering. Rendering changes only uniform scale and translation.
        let min = min.as_dvec3();
        let max = max.as_dvec3();
        let span = (max - min).max_element();
        if !span.is_finite() || span <= 0.0 {
            return Err("Model has no measurable geometry span".into());
        }
        let scale = span.recip() as f32;
        let center = (min + (max - min) * 0.5) / span;
        let display = Mat4::from_cols(
            glam::Vec4::new(scale, 0.0, 0.0, 0.0),
            glam::Vec4::new(0.0, scale, 0.0, 0.0),
            glam::Vec4::new(0.0, 0.0, scale, 0.0),
            (-center).as_vec3().extend(1.0),
        );
        let radius = ((max - min) / span * 0.5).length() as f32;
        if !display.is_finite()
            || !scale.is_finite()
            || scale <= 0.0
            || !radius.is_finite()
            || radius <= 0.0
        {
            return Err("Model cannot be framed with finite display coordinates".into());
        }
        Ok(Self { display, radius })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PreviewCamera {
    yaw: f32,
    pitch: f32,
    zoom: f32,
    pan: egui::Vec2,
}
impl Default for PreviewCamera {
    fn default() -> Self {
        Self {
            yaw: DEFAULT_YAW,
            pitch: DEFAULT_PITCH,
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
        }
    }
}
impl PreviewCamera {
    fn sanitize(&mut self) {
        if !self.yaw.is_finite()
            || !self.pitch.is_finite()
            || !self.zoom.is_finite()
            || !self.pan.is_finite()
        {
            *self = Self::default();
        }
        self.pitch = self.pitch.clamp(-1.45, 1.45);
        self.zoom = self.zoom.clamp(0.1, 100.0);
        self.pan = self
            .pan
            .clamp(egui::vec2(-1000.0, -1000.0), egui::vec2(1000.0, 1000.0));
    }
    fn fit(&mut self) {
        self.zoom = 1.0;
        self.pan = egui::Vec2::ZERO;
    }
    fn pose(self, size: egui::Vec2, radius: f32) -> (Vec3, Mat4) {
        let aspect = (size.x.max(1.0) / size.y.max(1.0)).clamp(0.001, 1000.0);
        let half_vertical = FIELD_OF_VIEW * 0.5;
        let half_horizontal = (half_vertical.tan() * aspect).atan();
        let distance = radius / half_vertical.min(half_horizontal).sin() * 1.12 * self.zoom;
        let direction = Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        );
        let right = (-direction).cross(Vec3::Y).normalize();
        let up = right.cross(-direction).normalize();
        let target = right * self.pan.x + up * self.pan.y;
        let eye = target + direction * distance;
        let near = (radius * 0.0001).max(0.00001);
        let far = (distance + radius * 4.0 + self.pan.length() * 2.0).max(10.0);
        let vp = Mat4::perspective_rh_gl(FIELD_OF_VIEW, aspect, near, far)
            * Mat4::look_at_rh(eye, target, up);
        (eye, vp)
    }
    fn pan_delta(&mut self, delta: egui::Vec2, size: egui::Vec2, radius: f32) {
        let aspect = (size.x.max(1.0) / size.y.max(1.0)).clamp(0.001, 1000.0);
        let half_vertical = FIELD_OF_VIEW * 0.5;
        let half_horizontal = (half_vertical.tan() * aspect).atan();
        // Compute distance directly; subtracting a large pan from eye length
        // loses the small view distance through floating-point cancellation.
        let distance = radius / half_vertical.min(half_horizontal).sin() * 1.12 * self.zoom;
        let per_pixel = 2.0 * distance * (FIELD_OF_VIEW * 0.5).tan() / size.y.max(1.0);
        self.pan += egui::vec2(-delta.x, delta.y) * per_pixel;
        self.sanitize();
    }
}

enum PreviewSource {
    Empty,
    Runtime(Arc<RuntimeInspectionModel>),
    Imported(Arc<ImportedGeometry>),
}

pub struct ModelPreview {
    renderer: Option<Arc<Mutex<Renderer>>>,
    renderer_error: Option<String>,
    imported: ImportedAssetPreview,
    has_gpu: bool,
    source: PreviewSource,
    context: Option<Arc<RuntimeInspectionModel>>,
    show_context: bool,
    framing: Option<Framing>,
    camera: PreviewCamera,
    error: Option<String>,
}
impl ModelPreview {
    pub fn new(gl: Option<&Arc<glow::Context>>) -> Self {
        let (renderer, renderer_error) = match gl.map(|gl| unsafe { Renderer::new(gl.clone()) }) {
            Some(Ok(renderer)) => (Some(Arc::new(Mutex::new(renderer))), None),
            Some(Err(error)) => (None, Some(error)),
            None => (None, Some("Model preview requires a GPU context".into())),
        };
        Self {
            renderer,
            renderer_error,
            imported: ImportedAssetPreview::new(gl),
            has_gpu: gl.is_some(),
            source: PreviewSource::Empty,
            context: None,
            show_context: false,
            framing: None,
            camera: PreviewCamera::default(),
            error: None,
        }
    }

    pub fn set_runtime(&mut self, model: Arc<RuntimeInspectionModel>) -> Result<(), String> {
        self.set_runtime_context(model, None)
    }

    /// Inspect a source-backed piece with optional geometry in the same source frame.
    /// Context is never inferred from a display name or an unregistered room plan.
    pub fn set_runtime_context(
        &mut self,
        model: Arc<RuntimeInspectionModel>,
        context: Option<Arc<RuntimeInspectionModel>>,
    ) -> Result<(), String> {
        let same_context = match (&self.context, &context) {
            (Some(current), Some(next)) => Arc::ptr_eq(current, next),
            (None, None) => true,
            _ => false,
        };
        if same_context
            && matches!(&self.source, PreviewSource::Runtime(current) if Arc::ptr_eq(current, &model))
        {
            return Ok(());
        }
        self.clear();
        let framing = self.accept_framing(Self::runtime_framing(&model))?;
        if let Some(context) = &context {
            self.accept_framing(Self::runtime_framing(context))?;
        }
        self.framing = Some(framing);
        self.source = PreviewSource::Runtime(model);
        self.context = context;
        Ok(())
    }

    fn runtime_framing(model: &RuntimeInspectionModel) -> Result<Framing, String> {
        let framing = Framing::bounds(model.min, model.max)?;
        if model.parts.is_empty()
            || model.parts.iter().any(|draw| {
                let (_, matrix, color, emission) = draw.part;
                !matrix.is_finite() || !color.is_finite() || !emission.is_finite()
            })
        {
            return Err("Runtime model has no finite renderable draws".into());
        }
        Ok(framing)
    }

    pub fn has_context(&self) -> bool {
        self.context.is_some()
    }

    pub fn context_visible(&self) -> bool {
        self.show_context
    }

    /// Switch inspection presentation only. No source or runtime material is edited.
    pub fn set_context_visible(&mut self, visible: bool) -> Result<(), String> {
        if self.show_context == visible {
            return Ok(());
        }
        let model = if visible {
            self.context
                .as_ref()
                .ok_or("This selection has no verified geometry in the ship frame")?
        } else if let PreviewSource::Runtime(model) = &self.source {
            model
        } else {
            return Err("Select a runtime piece before changing its context".into());
        };
        self.framing = Some(Self::runtime_framing(model)?);
        self.show_context = visible;
        self.camera.fit();
        Ok(())
    }

    /// Enlarge the selected source geometry without changing its location in context.
    pub fn focus_selected(&mut self) -> Result<(), String> {
        let PreviewSource::Runtime(model) = &self.source else {
            return Err("Select a runtime piece before focusing it".into());
        };
        self.framing = Some(Self::runtime_framing(model)?);
        self.camera.fit();
        Ok(())
    }

    /// Return to the complete verified ship frame, retaining the viewing direction.
    pub fn fit_context(&mut self) -> Result<(), String> {
        self.set_context_visible(true)?;
        let context = self.context.as_ref().ok_or("No verified ship context")?;
        self.framing = Some(Self::runtime_framing(context)?);
        self.camera.fit();
        Ok(())
    }

    pub fn set_imported(&mut self, geometry: Arc<ImportedGeometry>) -> Result<(), String> {
        if matches!(&self.source, PreviewSource::Imported(current) if Arc::ptr_eq(current, &geometry))
        {
            return Ok(());
        }
        self.clear();
        let framing = Framing::bounds(
            Vec3::from_array(geometry.report.min_m),
            Vec3::from_array(geometry.report.max_m),
        );
        let framing = self.accept_framing(framing)?;
        if self.has_gpu {
            if let Err(error) = self.imported.set(&geometry) {
                self.error = Some(error.clone());
                return Err(error);
            }
        }
        self.framing = Some(framing);
        self.source = PreviewSource::Imported(geometry);
        Ok(())
    }

    fn accept_framing(&mut self, framing: Result<Framing, String>) -> Result<Framing, String> {
        framing.inspect_err(|error| self.error = Some(error.clone()))
    }

    pub fn clear(&mut self) {
        self.source = PreviewSource::Empty;
        self.context = None;
        self.show_context = false;
        self.framing = None;
        self.error = None;
        self.camera = PreviewCamera::default();
        self.imported.clear();
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> egui::Response {
        self.camera.sanitize();
        if self.has_context() {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .selectable_label(!self.show_context, "Isolate selected")
                    .clicked()
                {
                    let _ = self.set_context_visible(false);
                }
                if ui
                    .selectable_label(self.show_context, "Highlight in ship")
                    .clicked()
                {
                    let _ = self.set_context_visible(true);
                }
                if self.show_context {
                    ui.small("Ship wireframe · selected piece outlined in green");
                } else {
                    ui.small("Selected piece only · original source materials");
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            let has_model = self.framing.is_some();
            if self.show_context {
                if ui.button("Focus piece").clicked() {
                    let _ = self.focus_selected();
                }
                if ui.button("Fit ship").clicked() {
                    let _ = self.fit_context();
                }
            }
            if ui
                .add_enabled(has_model, egui::Button::new("Fit model"))
                .clicked()
            {
                self.camera.fit();
            }
            if ui
                .add_enabled(has_model, egui::Button::new("Reset view"))
                .clicked()
            {
                self.camera = PreviewCamera::default();
                if self.show_context {
                    let _ = self.fit_context();
                } else if matches!(self.source, PreviewSource::Runtime(_)) {
                    let _ = self.focus_selected();
                }
            }
            ui.label("Left drag: orbit · right/middle drag: pan · wheel: zoom");
        });
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(egui::vec2(1.0, 1.0)),
            egui::Sense::drag(),
        );
        let clip = rect.intersect(ui.clip_rect());
        let focused = ui.input(|input| input.focused);
        let interactive = focused
            && ui.ctx().pointer_hover_pos().is_some_and(|point| {
                clip.contains(point) && ui.ctx().layer_id_at(point) == Some(ui.layer_id())
            });
        if interactive && self.framing.is_some() {
            // Sense::drag owns the press frame immediately. Its pointer delta
            // may include movement before that press; applying it makes a new
            // drag jump (or cancel the first actual motion on the next frame).
            let delta = ui.input(|input| {
                if input.pointer.any_pressed() {
                    egui::Vec2::ZERO
                } else {
                    input.pointer.delta()
                }
            });
            if delta.is_finite() {
                if response.dragged_by(egui::PointerButton::Secondary)
                    || response.dragged_by(egui::PointerButton::Middle)
                {
                    self.camera
                        .pan_delta(delta, rect.size(), self.framing.unwrap().radius);
                } else if response.dragged_by(egui::PointerButton::Primary) {
                    self.camera.yaw =
                        (self.camera.yaw - delta.x * 0.008).rem_euclid(std::f32::consts::TAU);
                    self.camera.pitch += delta.y * 0.006;
                }
            }
            if response.hovered() {
                let scroll = ui.input(|input| input.smooth_scroll_delta.y);
                if scroll.is_finite() {
                    self.camera.zoom *= (-scroll * 0.002).clamp(-10.0, 10.0).exp();
                }
            }
            self.camera.sanitize();
        }
        let painter = ui.painter().with_clip_rect(clip);
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(2, 4, 5));
        let message = if let Some(error) = &self.error {
            Some(error.as_str())
        } else if self.framing.is_none() {
            Some("Select a model to preview.")
        } else if !self.has_gpu {
            Some("Model preview requires a GPU context")
        } else {
            None
        };
        if let Some(message) = message {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                message,
                egui::FontId::proportional(14.0),
                egui::Color32::LIGHT_GRAY,
            );
            return response;
        }
        let framing = self.framing.unwrap();
        let (eye, vp) = self.camera.pose(rect.size(), framing.radius);
        match &self.source {
            PreviewSource::Runtime(model) => {
                if let Some(renderer) = &self.renderer {
                    let renderer = renderer.clone();
                    let model = model.clone();
                    let context = self.context.clone().filter(|_| self.show_context);
                    painter.add(egui::PaintCallback {
                        rect,
                        callback: Arc::new(egui_glow::CallbackFn::new(move |_, _| {
                            if let Ok(mut renderer) = renderer.lock() {
                                unsafe {
                                    if let Some(context) = &context {
                                        hangar::paint_inspection_context(
                                            &mut renderer,
                                            vp,
                                            eye,
                                            &model,
                                            context,
                                            framing.display,
                                        );
                                    } else {
                                        hangar::paint_inspection(
                                            &mut renderer,
                                            vp,
                                            eye,
                                            &model,
                                            framing.display,
                                        );
                                    }
                                    #[cfg(test)]
                                    assert_eq!(
                                        renderer.gl.get_error(),
                                        glow::NO_ERROR,
                                        "Runtime model callback"
                                    );
                                }
                            }
                        })),
                    });
                } else {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        self.renderer_error
                            .as_deref()
                            .unwrap_or("Runtime model renderer unavailable"),
                        egui::FontId::proportional(14.0),
                        egui::Color32::LIGHT_RED,
                    );
                }
            }
            PreviewSource::Imported(_) => {
                match self
                    .imported
                    .shared_callback(rect, vp, eye, framing.display)
                {
                    Ok(callback) => {
                        painter.add(callback);
                    }
                    Err(error) => {
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            error,
                            egui::FontId::proportional(14.0),
                            egui::Color32::LIGHT_RED,
                        );
                    }
                }
            }
            PreviewSource::Empty => {}
        }
        response
    }

    pub fn destroy(&mut self) {
        self.clear();
        self.imported.destroy();
        if let Some(renderer) = self.renderer.take() {
            if let Ok(mut renderer) = renderer.lock() {
                unsafe {
                    renderer.destroy();
                }
            }
        }
        self.has_gpu = false;
    }
}

#[cfg(test)]
#[path = "model_preview_tests.rs"]
mod tests;
