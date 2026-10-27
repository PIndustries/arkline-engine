//! Headless acceptance of source-piece context selection and its real egui controls.
//! Native pixels and GPU resource identity are checked by the separate CGL fixture.
use super::super::*;

// Registration12 receipt identities: the same port glazing, Pressure hull, and
// Panel fastener (Cube.005 at Blender [-25, 18.22, -1.6]), not arbitrary parts.
const GLASS: &str = "pioneer/part/d2788c0f4c6ab84c68e35ed2c30e970935cac88ecadeee3697fbacc62f58021e";
const HULL: &str = "pioneer/part/3f7df0ff130e6b9acd7701480afccfbcc4aa659d36a08601083c0567c1541e3e";
const FASTENER: &str =
    "pioneer/part/69ebe20883d3a5864ae44941bfa5df3e1e338cd5eb72a9d3e8c90e5e7686dc3a";
const WEDGE: &[u8] = include_bytes!("../../editor/tests/fixtures/import/wedge.glb");

fn bounded(test: impl FnOnce() + Send + 'static) {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(test));
        let _ = sender.send(result);
    });
    if let Err(panic) = receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("Source-piece context controls must finish within five seconds")
    {
        std::panic::resume_unwind(panic);
    }
}

struct Controls {
    ctx: egui::Context,
    time: f64,
    size: egui::Vec2,
}
impl Controls {
    fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
            time: 0.0,
            size: egui::vec2(960.0, 640.0),
        }
    }
    fn frame(
        &mut self,
        preview: &mut ModelPreview,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, egui::Rect) {
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
                egui::CentralPanel::default().show(ctx, |ui| canvas = preview.show(ui).rect);
            },
        );
        assert!(output
            .shapes
            .iter()
            .all(|shape| !matches!(shape.shape, egui::Shape::Callback(_))));
        (output, canvas)
    }
    fn click(&mut self, preview: &mut ModelPreview, output: &egui::FullOutput, text: &str) {
        let point =
            label(output, text).unwrap_or_else(|| panic!("Visible {text:?} button missing"));
        for pressed in [true, false] {
            self.frame(
                preview,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    fn drag(
        &mut self,
        preview: &mut ModelPreview,
        canvas: egui::Rect,
        button: egui::PointerButton,
        delta: egui::Vec2,
    ) {
        let start = canvas.center();
        let end = start + delta;
        self.frame(
            preview,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        self.frame(preview, vec![egui::Event::PointerMoved(end)]);
        self.frame(
            preview,
            vec![egui::Event::PointerButton {
                pos: end,
                button,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
}

fn label(output: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(rendered) if rendered.galley.text() == text => {
            let rect = rendered.galley.rect.translate(rendered.pos.to_vec2());
            shape.clip_rect.contains_rect(rect).then_some(rect.center())
        }
        _ => None,
    })
}

fn source_signature(model: &RuntimeInspectionModel) -> Vec<u32> {
    let mut signature = model
        .info
        .min_m
        .into_iter()
        .chain(model.info.max_m)
        .map(f32::to_bits)
        .collect::<Vec<_>>();
    signature.extend([model.info.triangles as u32, model.info.batches as u32]);
    for draw in &model.parts {
        signature.push(draw.part.0 as u32);
        match draw.range {
            Some((first, count)) => signature.extend([1, first as u32, count as u32]),
            None => signature.push(0),
        }
        signature.extend(
            draw.part
                .1
                .to_cols_array()
                .into_iter()
                .chain(draw.part.2.to_array())
                .chain([draw.part.3])
                .chain(draw.material.parameters())
                .map(f32::to_bits),
        );
    }
    signature
}

fn assert_framing(preview: &ModelPreview, model: &RuntimeInspectionModel) {
    let actual = preview.framing.unwrap();
    let expected = Framing::bounds(model.min, model.max).unwrap();
    assert_eq!(actual.display, expected.display);
    assert_eq!(actual.radius, expected.radius);
    for size in [egui::vec2(240.0, 700.0), egui::vec2(1600.0, 300.0)] {
        let (_, vp) = preview.camera.pose(size, actual.radius);
        for x in [model.min.x, model.max.x] {
            for y in [model.min.y, model.max.y] {
                for z in [model.min.z, model.max.z] {
                    let ndc =
                        vp.project_point3(actual.display.transform_point3(Vec3::new(x, y, z)));
                    assert!(ndc.is_finite());
                    assert!(ndc.x.abs() < 0.95 && ndc.y.abs() < 0.95);
                    assert!((-1.0..=1.0).contains(&ndc.z));
                }
            }
        }
    }
}

#[test]
fn source_piece_context_buttons_fit_registered_bounds_and_preserve_source() {
    bounded(|| {
        let selected = model_catalog::load_runtime_model(GLASS).unwrap();
        let context = model_catalog::load_runtime_model("ship/pioneer").unwrap();
        assert_eq!(selected.parts.len(), 1);
        assert_eq!(context.parts.len(), 9);
        let selected_before = source_signature(&selected);
        let context_before = source_signature(&context);
        let geometry = flight_geometry().unwrap();
        let uploads = geometry.catalog.uploads().to_vec();
        let mut preview = ModelPreview::new(None);
        preview
            .set_runtime_context(selected.clone(), Some(context.clone()))
            .unwrap();
        assert!(preview.has_context());
        assert!(
            !preview.context_visible(),
            "Each new selection starts isolated"
        );
        assert_framing(&preview, &selected);
        let mut controls = Controls::new();
        controls.frame(&mut preview, vec![]);
        let (_, canvas) = controls.frame(&mut preview, vec![]);
        controls.drag(
            &mut preview,
            canvas,
            egui::PointerButton::Primary,
            egui::vec2(45.0, 20.0),
        );
        controls.drag(
            &mut preview,
            canvas,
            egui::PointerButton::Secondary,
            egui::vec2(25.0, -15.0),
        );
        controls.frame(
            &mut preview,
            vec![
                egui::Event::PointerMoved(canvas.center()),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -20.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..20 {
            controls.frame(&mut preview, vec![]);
        }
        let direction = (preview.camera.yaw, preview.camera.pitch);
        assert_ne!(direction, (DEFAULT_YAW, DEFAULT_PITCH));
        assert!(preview.camera.pan.length() > 0.01 && preview.camera.zoom > 1.0);
        let (output, _) = controls.frame(&mut preview, vec![]);
        controls.click(&mut preview, &output, "Highlight in ship");
        assert!(preview.context_visible());
        assert_eq!((preview.camera.yaw, preview.camera.pitch), direction);
        assert_eq!(
            (preview.camera.zoom, preview.camera.pan),
            (1.0, egui::Vec2::ZERO)
        );
        assert_framing(&preview, &context);
        assert!(
            matches!(&preview.source, PreviewSource::Runtime(current) if Arc::ptr_eq(current, &selected))
        );
        assert!(Arc::ptr_eq(preview.context.as_ref().unwrap(), &context));
        let (_, canvas) = controls.frame(&mut preview, vec![]);
        controls.drag(
            &mut preview,
            canvas,
            egui::PointerButton::Middle,
            egui::vec2(-20.0, 15.0),
        );
        let (output, _) = controls.frame(&mut preview, vec![]);
        assert!(preview.camera.pan.length() > 0.01);
        controls.click(&mut preview, &output, "Isolate selected");
        assert!(!preview.context_visible());
        assert_eq!((preview.camera.yaw, preview.camera.pitch), direction);
        assert_eq!(
            (preview.camera.zoom, preview.camera.pan),
            (1.0, egui::Vec2::ZERO)
        );
        assert_framing(&preview, &selected);
        assert_eq!(source_signature(&selected), selected_before);
        assert_eq!(source_signature(&context), context_before);
        let after = flight_geometry().unwrap();
        assert!(Arc::ptr_eq(&geometry, &after));
        assert_eq!(after.catalog.uploads().len(), uploads.len());
        for (before, after) in uploads.iter().zip(after.catalog.uploads()) {
            assert!(Arc::ptr_eq(before, after));
            assert_eq!(before.len(), after.len());
        }
    });
}

#[test]
fn source_piece_new_selection_import_and_clear_remove_old_context() {
    let selected = model_catalog::load_runtime_model(GLASS).unwrap();
    let next = model_catalog::load_runtime_model(HULL).unwrap();
    let context = model_catalog::load_runtime_model("ship/pioneer").unwrap();
    let mut preview = ModelPreview::new(None);
    preview
        .set_runtime_context(selected.clone(), Some(context.clone()))
        .unwrap();
    preview.set_context_visible(true).unwrap();
    preview.camera.pan = egui::vec2(0.2, -0.1);
    preview.camera.zoom = 1.8;
    let previous = preview.camera;
    preview
        .set_runtime_context(selected, Some(context.clone()))
        .unwrap();
    assert_eq!(preview.camera, previous);
    assert!(
        preview.context_visible(),
        "Same cached selection retains its current presentation"
    );
    preview
        .set_runtime_context(next.clone(), Some(context.clone()))
        .unwrap();
    assert_eq!(preview.camera, PreviewCamera::default());
    assert!(preview.has_context() && !preview.context_visible());
    assert_framing(&preview, &next);
    preview.set_context_visible(true).unwrap();
    let imported = Arc::new(ImportedGeometry::decode(WEDGE).unwrap());
    preview.set_imported(imported.clone()).unwrap();
    assert!(!preview.has_context() && !preview.context_visible());
    assert!(
        matches!(&preview.source, PreviewSource::Imported(current) if Arc::ptr_eq(current, &imported))
    );
    preview
        .set_runtime_context(next.clone(), Some(context))
        .unwrap();
    preview.set_context_visible(true).unwrap();
    preview.set_runtime(next).unwrap();
    assert!(!preview.has_context() && !preview.context_visible());
    assert_eq!(preview.camera, PreviewCamera::default());
    preview.clear();
    assert!(matches!(preview.source, PreviewSource::Empty));
    assert!(!preview.has_context() && !preview.context_visible());
    assert!(preview.framing.is_none() && preview.error.is_none());
}

#[test]
fn source_piece_invalid_selection_or_context_removes_stale_framing() {
    let selected = model_catalog::load_runtime_model(GLASS).unwrap();
    let context = model_catalog::load_runtime_model("ship/pioneer").unwrap();
    for invalid_context in [true, false] {
        let mut preview = ModelPreview::new(None);
        preview
            .set_runtime_context(selected.clone(), Some(context.clone()))
            .unwrap();
        preview.set_context_visible(true).unwrap();
        let invalid = Arc::new(RuntimeInspectionModel {
            parts: selected.parts.clone(),
            min: Vec3::splat(f32::NAN),
            max: selected.max,
            info: selected.info.clone(),
        });
        let result = if invalid_context {
            preview.set_runtime_context(selected.clone(), Some(invalid))
        } else {
            preview.set_runtime_context(invalid, Some(context.clone()))
        };
        assert!(result.is_err());
        assert!(preview.error.is_some());
        assert!(preview.framing.is_none());
        assert!(matches!(preview.source, PreviewSource::Empty));
        assert!(!preview.has_context() && !preview.context_visible());
        assert_eq!(preview.camera, PreviewCamera::default());
        let mut controls = Controls::new();
        let (output, _) = controls.frame(&mut preview, vec![]);
        assert!(label(&output, "Highlight in ship").is_none());
    }
}

#[test]
fn source_piece_unsupported_context_toggle_preserves_current_selection() {
    let mut preview = ModelPreview::new(None);
    assert!(preview.set_context_visible(true).is_err());
    assert!(matches!(preview.source, PreviewSource::Empty));
    assert!(preview.framing.is_none() && !preview.has_context());
    let selected = model_catalog::load_runtime_model(GLASS).unwrap();
    preview.set_runtime(selected.clone()).unwrap();
    let framing = preview.framing.unwrap();
    assert!(preview.set_context_visible(true).is_err());
    assert!(!preview.context_visible() && !preview.has_context());
    assert_eq!(preview.framing.unwrap().display, framing.display);
    assert!(
        matches!(&preview.source, PreviewSource::Runtime(current) if Arc::ptr_eq(current, &selected))
    );
    let imported = Arc::new(ImportedGeometry::decode(WEDGE).unwrap());
    preview.set_imported(imported.clone()).unwrap();
    let framing = preview.framing.unwrap();
    assert!(preview.set_context_visible(true).is_err());
    assert_eq!(preview.framing.unwrap().display, framing.display);
    assert!(
        matches!(&preview.source, PreviewSource::Imported(current) if Arc::ptr_eq(current, &imported))
    );
    assert!(!preview.has_context() && !preview.context_visible());
}

#[test]
fn source_piece_focus_and_fit_ship_buttons_keep_context_and_restore_mode_framing() {
    bounded(|| {
        let selected = model_catalog::load_runtime_model(FASTENER).unwrap();
        let context = model_catalog::load_runtime_model("ship/pioneer").unwrap();
        assert_eq!((selected.parts.len(), selected.info.triangles), (1, 108));
        let selected_before = source_signature(&selected);
        let context_before = source_signature(&context);
        let geometry = flight_geometry().unwrap();
        let uploads = geometry.catalog.uploads().to_vec();
        let selected_framing = Framing::bounds(selected.min, selected.max).unwrap();
        let ship_framing = Framing::bounds(context.min, context.max).unwrap();
        assert_ne!(selected_framing.display, ship_framing.display);
        assert!(
            selected_framing.display.x_axis.x > ship_framing.display.x_axis.x * 100.0,
            "The tiny saved fastener must use its own measured source bounds"
        );
        let mut preview = ModelPreview::new(None);
        preview
            .set_runtime_context(selected.clone(), Some(context.clone()))
            .unwrap();
        let mut controls = Controls::new();
        controls.frame(&mut preview, vec![]);
        let (output, _) = controls.frame(&mut preview, vec![]);
        controls.click(&mut preview, &output, "Highlight in ship");
        assert!(preview.context_visible());
        let (_, canvas) = controls.frame(&mut preview, vec![]);
        controls.drag(
            &mut preview,
            canvas,
            egui::PointerButton::Primary,
            egui::vec2(40.0, 18.0),
        );
        controls.drag(
            &mut preview,
            canvas,
            egui::PointerButton::Secondary,
            egui::vec2(22.0, -12.0),
        );
        controls.frame(
            &mut preview,
            vec![
                egui::Event::PointerMoved(canvas.center()),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -20.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..20 {
            controls.frame(&mut preview, vec![]);
        }
        let direction = (preview.camera.yaw, preview.camera.pitch);
        assert_ne!(direction, (DEFAULT_YAW, DEFAULT_PITCH));
        assert!(preview.camera.pan.length() > 0.01 && preview.camera.zoom > 1.0);
        let (output, _) = controls.frame(&mut preview, vec![]);
        controls.click(&mut preview, &output, "Focus piece");
        assert!(preview.has_context() && preview.context_visible());
        assert_eq!((preview.camera.yaw, preview.camera.pitch), direction);
        assert_eq!(
            (preview.camera.zoom, preview.camera.pan),
            (1.0, egui::Vec2::ZERO)
        );
        assert_framing(&preview, &selected);
        assert!(
            matches!(&preview.source, PreviewSource::Runtime(current) if Arc::ptr_eq(current, &selected))
        );
        assert!(Arc::ptr_eq(preview.context.as_ref().unwrap(), &context));
        let (_, canvas) = controls.frame(&mut preview, vec![]);
        controls.drag(
            &mut preview,
            canvas,
            egui::PointerButton::Middle,
            egui::vec2(-20.0, 14.0),
        );
        assert!(preview.camera.pan.length() > 0.01);
        let (output, _) = controls.frame(&mut preview, vec![]);
        controls.click(&mut preview, &output, "Fit ship");
        assert!(preview.has_context() && preview.context_visible());
        assert_eq!((preview.camera.yaw, preview.camera.pitch), direction);
        assert_eq!(
            (preview.camera.zoom, preview.camera.pan),
            (1.0, egui::Vec2::ZERO)
        );
        assert_framing(&preview, &context);
        let (output, _) = controls.frame(&mut preview, vec![]);
        controls.click(&mut preview, &output, "Focus piece");
        assert_framing(&preview, &selected);
        let (_, canvas) = controls.frame(&mut preview, vec![]);
        controls.drag(
            &mut preview,
            canvas,
            egui::PointerButton::Primary,
            egui::vec2(-30.0, 12.0),
        );
        assert_ne!(preview.camera, PreviewCamera::default());
        let (output, _) = controls.frame(&mut preview, vec![]);
        controls.click(&mut preview, &output, "Reset view");
        assert!(preview.has_context() && preview.context_visible());
        assert_eq!(preview.camera, PreviewCamera::default());
        assert_framing(&preview, &context);
        assert!(
            matches!(&preview.source, PreviewSource::Runtime(current) if Arc::ptr_eq(current, &selected))
        );
        assert!(Arc::ptr_eq(preview.context.as_ref().unwrap(), &context));
        assert_eq!(source_signature(&selected), selected_before);
        assert_eq!(source_signature(&context), context_before);
        let after = flight_geometry().unwrap();
        assert!(Arc::ptr_eq(&geometry, &after));
        assert_eq!(uploads.len(), after.catalog.uploads().len());
        for (before, after) in uploads.iter().zip(after.catalog.uploads()) {
            assert!(Arc::ptr_eq(before, after));
            assert_eq!(before.len(), after.len());
        }
    });
}

#[test]
fn source_piece_focus_and_fit_errors_without_context_leave_selection_safe() {
    let mut preview = ModelPreview::new(None);
    assert!(preview.focus_selected().is_err());
    assert!(preview.fit_context().is_err());
    assert!(matches!(preview.source, PreviewSource::Empty));
    assert!(preview.framing.is_none() && !preview.has_context());
    let selected = model_catalog::load_runtime_model(FASTENER).unwrap();
    preview.set_runtime(selected.clone()).unwrap();
    let framing = preview.framing.unwrap();
    let camera = preview.camera;
    assert!(preview.fit_context().is_err());
    assert_eq!(preview.framing.unwrap().display, framing.display);
    assert_eq!(preview.camera, camera);
    assert!(!preview.has_context() && !preview.context_visible());
    assert!(
        matches!(&preview.source, PreviewSource::Runtime(current) if Arc::ptr_eq(current, &selected))
    );
    let imported = Arc::new(ImportedGeometry::decode(WEDGE).unwrap());
    preview.set_imported(imported.clone()).unwrap();
    let framing = preview.framing.unwrap();
    assert!(preview.fit_context().is_err());
    assert_eq!(preview.framing.unwrap().display, framing.display);
    assert!(
        matches!(&preview.source, PreviewSource::Imported(current) if Arc::ptr_eq(current, &imported))
    );
    assert!(!preview.has_context() && !preview.context_visible());
    preview.clear();
    assert!(preview.focus_selected().is_err());
    assert!(preview.fit_context().is_err());
    assert!(matches!(preview.source, PreviewSource::Empty));
    assert!(preview.framing.is_none() && !preview.has_context());
}
