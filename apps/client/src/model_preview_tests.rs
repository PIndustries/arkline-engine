use super::*;

const WEDGE: &[u8] = include_bytes!("../../editor/tests/fixtures/import/wedge.glb");
const SPIRE: &[u8] = include_bytes!("../../editor/tests/fixtures/import/spire.glb");

fn bounded(test: impl FnOnce() + Send + 'static) {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(test));
        let _ = sender.send(result);
    });
    if let Err(panic) = receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("Model-preview interaction must finish within five seconds")
    {
        std::panic::resume_unwind(panic);
    }
}

#[allow(clippy::too_many_arguments)]
fn frame(
    ctx: &egui::Context,
    preview: &mut ModelPreview,
    size: egui::Vec2,
    time: &mut f64,
    events: Vec<egui::Event>,
    focused: bool,
    overlay: bool,
) -> (egui::FullOutput, egui::Rect) {
    *time += 0.02;
    let mut canvas = egui::Rect::NOTHING;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(*time),
            predicted_dt: 0.02,
            focused,
            events,
            ..Default::default()
        },
        |ctx| {
            if overlay {
                egui::Area::new(egui::Id::new("model_preview_overlay"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(egui::pos2(150.0, 150.0))
                    .show(ctx, |ui| {
                        ui.allocate_exact_size(
                            egui::vec2(600.0, 380.0),
                            egui::Sense::click_and_drag(),
                        );
                    });
            }
            egui::CentralPanel::default().show(ctx, |ui| canvas = preview.show(ui).rect);
        },
    );
    (output, canvas)
}
fn pointer(point: egui::Pos2, button: egui::PointerButton, pressed: bool) -> Vec<egui::Event> {
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
fn label(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(rendered) if rendered.galley.text() == text => {
                let rect = rendered.galley.rect.translate(rendered.pos.to_vec2());
                shape.clip_rect.contains_rect(rect).then_some(rect.center())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Visible model-preview control {text:?} missing"))
}

#[test]
fn fitted_camera_contains_actual_bounds_at_wide_and_narrow_aspects() {
    for (min, max) in [
        (Vec3::new(-2.0, -1.0, -3.0), Vec3::new(4.0, 2.0, 1.0)),
        (Vec3::splat(-1e-12), Vec3::splat(1e-12)),
        (Vec3::splat(-1e12), Vec3::splat(1e12)),
        (
            Vec3::new(1000.0, 20.0, -0.00001),
            Vec3::new(1001.0, 20.0, 0.00001),
        ),
    ] {
        let framing = Framing::bounds(min, max).unwrap();
        for size in [
            egui::vec2(240.0, 700.0),
            egui::vec2(1600.0, 300.0),
            egui::vec2(600.0, 600.0),
        ] {
            let (eye, vp) = PreviewCamera::default().pose(size, framing.radius);
            assert!(eye.is_finite() && vp.is_finite());
            for x in [min.x, max.x] {
                for y in [min.y, max.y] {
                    for z in [min.z, max.z] {
                        let point =
                            vp.project_point3(framing.display.transform_point3(Vec3::new(x, y, z)));
                        assert!(
                            point.is_finite()
                                && point.x.abs() < 0.95
                                && point.y.abs() < 0.95
                                && (-1.0..=1.0).contains(&point.z),
                            "Bounds clipped at {size:?}: {point:?}"
                        );
                    }
                }
            }
        }
    }
    assert!(Framing::bounds(Vec3::ZERO, Vec3::ZERO).is_err());
    assert!(Framing::bounds(Vec3::ONE, Vec3::ZERO).is_err());
    assert!(Framing::bounds(Vec3::splat(f32::NAN), Vec3::ONE).is_err());
    assert!(Framing::bounds(Vec3::ZERO, Vec3::splat(f32::INFINITY)).is_err());
}

#[test]
fn shared_controls_orbit_pan_zoom_fit_and_reset_through_real_pointer_frames() {
    bounded(|| {
        let ctx = egui::Context::default();
        let mut preview = ModelPreview::new(None);
        let geometry = Arc::new(ImportedGeometry::decode(WEDGE).unwrap());
        preview.set_imported(geometry.clone()).unwrap();
        let size = egui::vec2(900.0, 600.0);
        let mut time = 0.0;
        frame(&ctx, &mut preview, size, &mut time, vec![], true, false);
        let (_, canvas) = frame(&ctx, &mut preview, size, &mut time, vec![], true, false);
        let initial = preview.camera;
        let start = canvas.center();
        let end = start + egui::vec2(45.0, 25.0);
        for button in [
            egui::PointerButton::Primary,
            egui::PointerButton::Secondary,
            egui::PointerButton::Middle,
        ] {
            let before = preview.camera;
            frame(
                &ctx,
                &mut preview,
                size,
                &mut time,
                pointer(start, button, true),
                true,
                false,
            );
            assert_eq!(
                preview.camera, before,
                "A new drag must ignore movement before the button press"
            );
            frame(
                &ctx,
                &mut preview,
                size,
                &mut time,
                vec![egui::Event::PointerMoved(end)],
                true,
                false,
            );
            frame(
                &ctx,
                &mut preview,
                size,
                &mut time,
                pointer(end, button, false),
                true,
                false,
            );
            if button == egui::PointerButton::Primary {
                assert_ne!(before.yaw, preview.camera.yaw);
                assert_ne!(before.pitch, preview.camera.pitch);
                assert_eq!(before.pan, preview.camera.pan);
            } else {
                assert_ne!(before.pan, preview.camera.pan);
                assert_eq!(
                    (before.yaw, before.pitch),
                    (preview.camera.yaw, preview.camera.pitch)
                );
            }
        }
        let before_zoom = preview.camera.zoom;
        frame(
            &ctx,
            &mut preview,
            size,
            &mut time,
            wheel(start),
            true,
            false,
        );
        for _ in 0..20 {
            frame(&ctx, &mut preview, size, &mut time, vec![], true, false);
        }
        assert!(preview.camera.zoom > before_zoom);
        let moved = preview.camera;
        preview.set_imported(geometry.clone()).unwrap();
        assert_eq!(
            preview.camera, moved,
            "Repeated selection must not reset the camera or re-upload the model"
        );
        let (output, _) = frame(&ctx, &mut preview, size, &mut time, vec![], true, false);
        let point = label(&output, "Fit model");
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut preview,
                size,
                &mut time,
                pointer(point, egui::PointerButton::Primary, pressed),
                true,
                false,
            );
        }
        assert_eq!(
            (preview.camera.yaw, preview.camera.pitch),
            (moved.yaw, moved.pitch)
        );
        assert_eq!(
            (preview.camera.zoom, preview.camera.pan),
            (1.0, egui::Vec2::ZERO)
        );
        let (output, _) = frame(&ctx, &mut preview, size, &mut time, vec![], true, false);
        let point = label(&output, "Reset view");
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut preview,
                size,
                &mut time,
                pointer(point, egui::PointerButton::Primary, pressed),
                true,
                false,
            );
        }
        assert_eq!(preview.camera, initial);
        assert!(
            matches!(&preview.source,PreviewSource::Imported(current) if Arc::ptr_eq(current,&geometry))
        );
    });
}

#[test]
fn overlays_unfocused_windows_and_controls_do_not_move_the_preview_camera() {
    bounded(|| {
        let ctx = egui::Context::default();
        let mut preview = ModelPreview::new(None);
        preview
            .set_imported(Arc::new(ImportedGeometry::decode(WEDGE).unwrap()))
            .unwrap();
        let size = egui::vec2(900.0, 600.0);
        let mut time = 0.0;
        frame(&ctx, &mut preview, size, &mut time, vec![], true, true);
        let (_, canvas) = frame(&ctx, &mut preview, size, &mut time, vec![], true, true);
        let start = canvas.center();
        let end = start + egui::vec2(30.0, 15.0);
        let original = preview.camera;
        for button in [
            egui::PointerButton::Primary,
            egui::PointerButton::Secondary,
            egui::PointerButton::Middle,
        ] {
            frame(
                &ctx,
                &mut preview,
                size,
                &mut time,
                pointer(start, button, true),
                true,
                true,
            );
            frame(
                &ctx,
                &mut preview,
                size,
                &mut time,
                vec![egui::Event::PointerMoved(end)],
                true,
                true,
            );
            frame(
                &ctx,
                &mut preview,
                size,
                &mut time,
                pointer(end, button, false),
                true,
                true,
            );
        }
        frame(
            &ctx,
            &mut preview,
            size,
            &mut time,
            wheel(start),
            true,
            true,
        );
        assert_eq!(preview.camera, original);
        // Removing the foreground layer and returning focus also settle egui's hit map.
        for _ in 0..20 {
            frame(&ctx, &mut preview, size, &mut time, vec![], true, false);
        }
        for button in [
            egui::PointerButton::Primary,
            egui::PointerButton::Secondary,
            egui::PointerButton::Middle,
        ] {
            frame(
                &ctx,
                &mut preview,
                size,
                &mut time,
                pointer(start, button, true),
                false,
                false,
            );
            frame(
                &ctx,
                &mut preview,
                size,
                &mut time,
                vec![egui::Event::PointerMoved(end)],
                false,
                false,
            );
            frame(
                &ctx,
                &mut preview,
                size,
                &mut time,
                pointer(end, button, false),
                false,
                false,
            );
        }
        frame(
            &ctx,
            &mut preview,
            size,
            &mut time,
            wheel(start),
            false,
            false,
        );
        assert_eq!(preview.camera, original);
    });
}

#[test]
fn selection_switch_clear_error_and_resize_preserve_source_and_camera_invariants() {
    bounded(|| {
        let ctx = egui::Context::default();
        let mut preview = ModelPreview::new(None);
        let wedge = Arc::new(ImportedGeometry::decode(WEDGE).unwrap());
        let spire = Arc::new(ImportedGeometry::decode(SPIRE).unwrap());
        let before = wedge.report.clone();
        preview.set_imported(wedge.clone()).unwrap();
        preview.camera.pan = egui::vec2(1.0, 2.0);
        preview.set_imported(spire.clone()).unwrap();
        assert_eq!(preview.camera, PreviewCamera::default());
        assert_eq!(wedge.report, before);
        let mut time = 0.0;
        for size in [
            egui::vec2(320.0, 500.0),
            egui::vec2(1200.0, 300.0),
            egui::vec2(300.0, 200.0),
        ] {
            let (_, canvas) = frame(&ctx, &mut preview, size, &mut time, vec![], true, false);
            let (eye, vp) = preview
                .camera
                .pose(canvas.size(), preview.framing.unwrap().radius);
            assert!(
                canvas.is_finite() && canvas.is_positive() && eye.is_finite() && vp.is_finite()
            );
        }
        preview.clear();
        assert!(matches!(preview.source, PreviewSource::Empty) && preview.framing.is_none());
        let mut invalid = ImportedGeometry::decode(WEDGE).unwrap();
        invalid.report.min_m[0] = f32::NAN;
        assert!(preview.set_imported(Arc::new(invalid)).is_err());
        assert!(matches!(preview.source, PreviewSource::Empty) && preview.framing.is_none());
        assert!(preview.error.as_deref().unwrap().contains("bounds"));
        preview.set_imported(wedge).unwrap();
        preview.camera.yaw = f32::NAN;
        frame(
            &ctx,
            &mut preview,
            egui::vec2(600.0, 400.0),
            &mut time,
            vec![],
            true,
            false,
        );
        assert_eq!(preview.camera, PreviewCamera::default());
    });
}
