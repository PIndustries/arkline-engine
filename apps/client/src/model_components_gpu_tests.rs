//! Representative actual-source selection and context acceptance using the shared UI.
//! Exhaustive per-object receipt/range coverage lives in model_components_tests.rs.
use super::*;

unsafe fn source_gpu_hash(gl: &glow::Context, renderer: &Renderer) -> String {
    let previous = gl.get_parameter_buffer(glow::ARRAY_BUFFER_BINDING);
    let mut hash = Sha256::new();
    for mesh in &renderer.meshes[10..19] {
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(mesh.buffer));
        let mut bytes = vec![0; mesh.count as usize * 24];
        gl.get_buffer_sub_data(glow::ARRAY_BUFFER, 0, &mut bytes);
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    gl.bind_buffer(glow::ARRAY_BUFFER, previous);
    assert_eq!(
        gl.get_error(),
        glow::NO_ERROR,
        "Read actual shared source GPU buffers"
    );
    hex::encode(hash.finalize())
}

fn canvas_hash(shot: &Shot) -> String {
    let mut hash = Sha256::new();
    for (index, pixel) in shot.rgba.chunks_exact(4).enumerate() {
        let point = egui::pos2(
            (index % WIDTH) as f32 + 0.5,
            (HEIGHT - 1 - index / WIDTH) as f32 + 0.5,
        );
        if shot.canvas.contains(point) {
            hash.update(pixel);
        }
    }
    hex::encode(hash.finalize())
}

fn canvas_changes(first: &Shot, second: &Shot) -> usize {
    first
        .rgba
        .chunks_exact(4)
        .zip(second.rgba.chunks_exact(4))
        .enumerate()
        .filter(|(index, (a, b))| {
            let point = egui::pos2(
                (index % WIDTH) as f32 + 0.5,
                (HEIGHT - 1 - index / WIDTH) as f32 + 0.5,
            );
            first.canvas.contains(point) && second.canvas.contains(point) && a != b
        })
        .count()
}

/// The context does not write depth; these colour pixels independently show its
/// lines outside the selected solid's depth coverage, inside the clipped canvas.
fn context_line_pixels(shot: &Shot) -> usize {
    shot.rgba
        .chunks_exact(4)
        .enumerate()
        .filter(|(index, pixel)| {
            let point = egui::pos2(
                (index % WIDTH) as f32 + 0.5,
                (HEIGHT - 1 - index / WIDTH) as f32 + 0.5,
            );
            shot.canvas.contains(point)
                && !shot.mask[*index]
                && pixel[0] as u16 + pixel[1] as u16 + pixel[2] as u16 > 45
        })
        .count()
}

fn selected_green_pixels(
    shot: &Shot,
    model: &RuntimeInspectionModel,
    preview: &ModelPreview,
) -> usize {
    let framing = preview.framing.unwrap();
    let (_, vp) = preview.camera.pose(shot.canvas.size(), framing.radius);
    let mut rect = egui::Rect::NOTHING;
    for x in [model.min.x, model.max.x] {
        for y in [model.min.y, model.max.y] {
            for z in [model.min.z, model.max.z] {
                let clip = vp * framing.display * Vec3::new(x, y, z).extend(1.0);
                assert!(clip.is_finite() && clip.w > 0.0);
                let ndc = clip.truncate() / clip.w;
                rect.extend_with(egui::pos2(
                    shot.canvas.left() + (ndc.x + 1.0) * 0.5 * shot.canvas.width(),
                    shot.canvas.top() + (1.0 - ndc.y) * 0.5 * shot.canvas.height(),
                ));
            }
        }
    }
    let rect = rect.expand(3.0).intersect(shot.canvas);
    shot.rgba
        .chunks_exact(4)
        .enumerate()
        .filter(|(index, pixel)| {
            let point = egui::pos2(
                (index % WIDTH) as f32 + 0.5,
                (HEIGHT - 1 - index / WIDTH) as f32 + 0.5,
            );
            rect.contains(point)
                && pixel[1] > 70
                && pixel[1] as u16 > pixel[0] as u16 + 35
                && pixel[1] as u16 > pixel[2] as u16 + 20
        })
        .count()
}

unsafe fn settle(harness: &mut Harness) -> Shot {
    let first = harness.frame(vec![]);
    harness.frame(vec![egui::Event::PointerMoved(first.canvas.center())]);
    harness.frame(vec![])
}

#[test]
#[ignore = "Explicit native macOS source-piece isolation/highlight GPU acceptance; no desktop window"]
fn native_source_components_exact_ranges_context_controls_and_buffer_invariance() {
    unsafe {
        let directory =
            std::env::var_os("ARKLINE_MODEL_COMPONENT_EVIDENCE").map(std::path::PathBuf::from);
        if let Some(directory) = &directory {
            std::fs::create_dir(directory).unwrap();
        }
        let receipt_bytes = include_bytes!("../../../assets/ships/pioneer/pioneer-export.json");
        let receipt: serde_json::Value = serde_json::from_slice(receipt_bytes).unwrap();
        let objects = receipt["source_parts"].as_array().unwrap();
        assert_eq!(objects.len(), 686);
        // One exact source object per material, then a tiny repeated fastener
        // and the actual proposed port hatch pane. No proxy meshes are added.
        let mut representatives = (0..9)
            .map(|material| {
                objects
                    .iter()
                    .filter(|row| row["render_material_index"].as_u64() == Some(material))
                    .max_by_key(|row| row["retained_triangles"].as_u64().unwrap())
                    .unwrap()
            })
            .collect::<Vec<_>>();
        for name in ["Panel fastener", "Bridge wrap glazing port"] {
            let row = objects.iter().find(|row| row["name"] == name).unwrap();
            if !representatives
                .iter()
                .any(|other| other["source_id"] == row["source_id"])
            {
                representatives.push(row);
            }
        }
        assert_eq!(representatives.len(), 11);
        let catalog = model_catalog::runtime_model_catalog();
        assert_eq!(
            catalog
                .iter()
                .filter(|entry| entry.id.starts_with("pioneer/part/"))
                .count(),
            686
        );
        let assembly = model_catalog::load_runtime_model("ship/pioneer").unwrap();
        assert_eq!(
            (assembly.parts.len(), assembly.info.triangles),
            (9, 103_466)
        );
        let assembly_before = runtime_hash(&assembly);
        let (_context, gl) = PrivateContext::create();
        let mut harness = Harness::new(gl.clone());
        let renderer = harness.preview.renderer.as_ref().unwrap().clone();
        let buffers = renderer
            .lock()
            .unwrap()
            .meshes
            .iter()
            .map(|mesh| (mesh.vao, mesh.buffer, mesh.count))
            .collect::<Vec<_>>();
        let gpu_before = source_gpu_hash(&gl, &renderer.lock().unwrap());
        let mut rows = Vec::new();
        let mut isolation_hashes = std::collections::BTreeSet::new();
        let mut previous_model: Option<Arc<RuntimeInspectionModel>> = None;
        for row in representatives {
            let name = row["name"].as_str().unwrap();
            let suffix = row["source_id"]
                .as_str()
                .unwrap()
                .strip_prefix("part:")
                .unwrap();
            let id = format!("pioneer/part/{suffix}");
            let model = model_catalog::load_runtime_model(&id).unwrap();
            let before = runtime_hash(&model);
            let first = row["first_index"].as_u64().unwrap() as usize;
            let count = row["index_count"].as_u64().unwrap() as usize;
            let primitive = row["render_primitive"].as_u64().unwrap() as usize;
            assert_eq!(model.parts[0].range, Some((first, count)));
            assert_eq!(model.parts[0].part.0, 10 + primitive);
            harness
                .preview
                .set_runtime_context(model.clone(), Some(assembly.clone()))
                .unwrap();
            assert!(harness.preview.has_context() && !harness.preview.context_visible());
            assert_eq!(
                harness.preview.camera,
                PreviewCamera::default(),
                "New selection must reset camera and isolate"
            );
            if let PreviewSource::Runtime(current) = &harness.preview.source {
                assert!(Arc::ptr_eq(current, &model));
                if let Some(previous) = &previous_model {
                    assert!(!Arc::ptr_eq(current, previous));
                }
            } else {
                panic!("Source piece must replace the previous selection");
            }
            let isolated = settle(&mut harness);
            assert!(
                isolated.occupied() > 5,
                "{name}: actual isolated source geometry must be visible"
            );
            assert!(
                isolation_hashes.insert(canvas_hash(&isolated)),
                "Different source selections must change visible geometry/materials"
            );
            let metrics = renderer.lock().unwrap().debug_metrics.get();
            assert_eq!(
                (
                    metrics.draw_calls,
                    metrics.triangles,
                    metrics.wireframe_draw_calls
                ),
                (1, count as u64 / 3, 0),
                "{name}: draw only the exact saved range"
            );
            {
                let locked = renderer.lock().unwrap();
                let source = embedded_primitive(10 + primitive).unwrap();
                let mut actual = [0.0; 4];
                gl.get_uniform_f32(
                    locked.program,
                    locked.uniforms.material_params.as_ref().unwrap(),
                    &mut actual,
                );
                assert_eq!(
                    actual,
                    [source.roughness, source.metallic, 0.0, 0.0],
                    "{name}: retain actual scalar material"
                );
            }
            harness.click(isolated.label("Highlight in ship"));
            assert!(harness.preview.context_visible());
            let highlighted = settle(&mut harness);
            assert!(
                highlighted.occupied() > 0,
                "{name}: selected piece must remain visible within ship framing"
            );
            let context_pixels = context_line_pixels(&highlighted);
            let green_pixels = selected_green_pixels(&highlighted, &model, &harness.preview);
            assert!(
                context_pixels > 100,
                "{name}: actual assembled ship wireframe must be visible"
            );
            assert!(
                green_pixels > 0,
                "{name}: selected piece must be visibly outlined at its projected source position"
            );
            let metrics = renderer.lock().unwrap().debug_metrics.get();
            assert_eq!(
                (
                    metrics.draw_calls,
                    metrics.triangles,
                    metrics.wireframe_draw_calls
                ),
                (10, 103_466 + count as u64 / 3, 1),
                "{name}: complete source context plus one exact selected range and outline"
            );
            let focus_piece_journey = if name == "Panel fastener" {
                // The recorded whole-ship view leaves this real 18 cm part at
                // roughly three pixels. Focus changes framing of the same
                // source coordinates; it never substitutes marker geometry.
                let direction = (harness.preview.camera.yaw, harness.preview.camera.pitch);
                harness.click(highlighted.label("Focus piece"));
                let focused = settle(&mut harness);
                assert!(harness.preview.has_context() && harness.preview.context_visible());
                let expected = Framing::bounds(model.min, model.max).unwrap();
                let actual = harness.preview.framing.unwrap();
                assert_eq!(actual.display, expected.display);
                assert_eq!(actual.radius, expected.radius);
                assert_eq!(
                    (harness.preview.camera.yaw, harness.preview.camera.pitch),
                    direction
                );
                let focused_green = selected_green_pixels(&focused, &model, &harness.preview);
                assert!(focused_green > 32 && focused_green > green_pixels * 8,
                    "Focus piece must make the actual tiny fastener clearly larger than its whole-ship highlight");
                assert!(focused.occupied() > highlighted.occupied() * 16);
                let metrics = renderer.lock().unwrap().debug_metrics.get();
                assert_eq!(
                    (
                        metrics.draw_calls,
                        metrics.triangles,
                        metrics.wireframe_draw_calls
                    ),
                    (10, 103_466 + count as u64 / 3, 1),
                    "Focus must retain full source context and the exact selected range/outline"
                );
                harness.click(focused.label("Fit ship"));
                let ship_fit = settle(&mut harness);
                assert!(harness.preview.context_visible());
                assert_eq!(
                    (harness.preview.camera.yaw, harness.preview.camera.pitch),
                    direction
                );
                assert_eq!(
                    canvas_hash(&highlighted),
                    canvas_hash(&ship_fit),
                    "Fit ship must reproduce the original whole-context framing"
                );
                harness.click(ship_fit.label("Focus piece"));
                let refocused = settle(&mut harness);
                let reset_context = harness.reset(&refocused);
                assert!(harness.preview.context_visible());
                assert_eq!(
                    canvas_hash(&highlighted),
                    canvas_hash(&reset_context),
                    "Reset from Focus must restore the current context mode's default ship framing"
                );
                assert_eq!(before, runtime_hash(&model));
                assert_eq!(assembly_before, runtime_hash(&assembly));
                Some(
                    serde_json::json!({"actual_focus_and_fit_ship_pointer_controls":true,
                    "whole_ship_green_pixels":green_pixels,"focused_green_pixels":focused_green,
                    "focused_selected_depth_pixels":focused.occupied(),
                    "same_source_context_and_exact_range":true,"direction_retained":true,
                    "fit_ship_restores_exact_pixels":true,"reset_restores_context_default":true,
                    "captures":[capture(directory.as_deref(),"fastener-focus-piece",&focused),
                        capture(directory.as_deref(),"fastener-fit-ship",&ship_fit),
                        capture(directory.as_deref(),"fastener-focus-reset",&reset_context)]}),
                )
            } else {
                None
            };
            let center = highlighted.canvas.center();
            let end = center + egui::vec2(60.0, 24.0);
            harness.frame(pointer(center, true));
            harness.frame(vec![egui::Event::PointerMoved(end)]);
            let orbit = harness.frame(pointer(end, false));
            assert!(
                canvas_changes(&highlighted, &orbit) > 100,
                "{name}: real orbit must move source/context rendering"
            );
            harness.frame(pointer_button(center, egui::PointerButton::Middle, true));
            let pan_end = center + egui::vec2(30.0, -18.0);
            harness.frame(vec![egui::Event::PointerMoved(pan_end)]);
            let panned = harness.frame(pointer_button(pan_end, egui::PointerButton::Middle, false));
            assert!(harness.preview.camera.pan.length() > 0.01);
            assert!(
                canvas_changes(&orbit, &panned) > 100,
                "{name}: real middle-drag must pan actual geometry"
            );
            harness.frame(vec![
                egui::Event::PointerMoved(panned.canvas.center()),
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
                harness.preview.camera.zoom > 1.02,
                "{name}: actual wheel input must zoom the source/context camera"
            );
            assert!(
                canvas_changes(&panned, &zoomed) > 100,
                "{name}: wheel must visibly change rendering"
            );
            let direction = (harness.preview.camera.yaw, harness.preview.camera.pitch);
            let fit = harness.click(zoomed.label("Fit model"));
            let fitted = settle(&mut harness);
            assert_eq!(
                (harness.preview.camera.yaw, harness.preview.camera.pitch),
                direction
            );
            assert_eq!(
                (harness.preview.camera.zoom, harness.preview.camera.pan),
                (1.0, egui::Vec2::ZERO)
            );
            assert_eq!(
                canvas_hash(&orbit),
                canvas_hash(&fitted),
                "{name}: Fit must restore framing while retaining direction"
            );
            let reset = harness.reset(&fit);
            assert_eq!(
                canvas_hash(&highlighted),
                canvas_hash(&reset),
                "{name}: Reset reproduces exact selected/context pixels"
            );
            harness.click(reset.label("Isolate selected"));
            assert!(!harness.preview.context_visible());
            let isolated_again = settle(&mut harness);
            assert_eq!(
                canvas_hash(&isolated),
                canvas_hash(&isolated_again),
                "{name}: Isolate restores original source materials and framing"
            );
            for size in [
                egui::vec2(320.0, HEIGHT as f32),
                egui::vec2(WIDTH as f32, 320.0),
            ] {
                harness.size = size;
                let resized = settle(&mut harness);
                assert!(
                    resized.occupied() > 3,
                    "{name}: aspect fit must keep exact source geometry visible at {size:?}"
                );
            }
            harness.size = egui::vec2(WIDTH as f32, HEIGHT as f32);
            settle(&mut harness);
            // Repeated selection with the same context retains camera and buffers.
            harness.preview.camera.pan = egui::vec2(0.1, 0.2);
            let moved = harness.preview.camera;
            harness
                .preview
                .set_runtime_context(model.clone(), Some(assembly.clone()))
                .unwrap();
            assert_eq!(harness.preview.camera, moved);
            assert_eq!(before, runtime_hash(&model));
            assert_eq!(assembly_before, runtime_hash(&assembly));
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
            let prefix = format!("part-{}", rows.len());
            rows.push(serde_json::json!({"id":id,"name":name,"material":row["material"],"source_primitive":primitive,"first_corner":first,"corner_count":count,"triangles":model.info.triangles,"min_m":model.info.min_m,"max_m":model.info.max_m,"selected_source_sha256":before,"scalar_material_exact":true,"context_line_pixels_outside_selected_depth":context_pixels,"green_outline_pixels_at_projected_source_position":green_pixels,"actual_mode_controls":true,"focus_piece_journey":focus_piece_journey,"orbit_pan_wheel_fit_reset":true,"resize_sizes":[[320,HEIGHT],[WIDTH,320]],"captures":[capture(directory.as_deref(),&format!("{prefix}-isolate"),&isolated),capture(directory.as_deref(),&format!("{prefix}-highlight"),&highlighted),capture(directory.as_deref(),&format!("{prefix}-orbit"),&orbit),capture(directory.as_deref(),&format!("{prefix}-pan"),&panned),capture(directory.as_deref(),&format!("{prefix}-zoom"),&zoomed)]}));
            previous_model = Some(model);
        }
        harness.preview.clear();
        assert!(!harness.preview.has_context() && !harness.preview.context_visible());
        let cleared = settle(&mut harness);
        assert_eq!(cleared.occupied(), 0);
        let last = previous_model.unwrap();
        harness
            .preview
            .set_runtime_context(last.clone(), Some(assembly.clone()))
            .unwrap();
        harness.preview.set_context_visible(true).unwrap();
        let invalid = Arc::new(RuntimeInspectionModel {
            parts: assembly.parts.clone(),
            min: Vec3::splat(f32::NAN),
            max: assembly.max,
            info: assembly.info.clone(),
        });
        assert!(harness
            .preview
            .set_runtime_context(last, Some(invalid))
            .is_err());
        let failed = settle(&mut harness);
        assert_eq!(
            failed.occupied(),
            0,
            "Invalid context must clear all old source and context drawings"
        );
        assert!(!harness.preview.has_context() && !harness.preview.context_visible());
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
        let gpu_after = source_gpu_hash(&gl, &renderer.lock().unwrap());
        assert_eq!(gpu_before, gpu_after, "Actual source GPU payloads must remain byte-identical after every selection/mode/control change");
        assert_eq!(assembly_before, runtime_hash(&assembly));
        if let Some(directory) = &directory {
            std::fs::write(directory.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({"schema_version":2,"scope":"Actual shared ModelPreview callbacks and egui mode/Focus piece/Fit ship/orbit/pan/Fit/Reset controls, private macOS CGL; no desktop window or uploads","saved_export_receipt":"assets/ships/pioneer/pioneer-export.json","saved_export_receipt_sha256":hex::encode(Sha256::digest(receipt_bytes)),"source_glb_sha256":receipt["output"]["sha256"],"source_blend_sha256":receipt["source"]["sha256"],"source_identity_scope":receipt["source"]["identity_scope"],"source_part_count":objects.len(),"cpu_oracle":"all_686_source_pieces_independently_match_saved_receipt_and_partition_source","representative_gpu_count":rows.len(),"material_classes_rendered":9,"context":"ship/pioneer","context_primitive_batches":assembly.parts.len(),"context_triangles":assembly.info.triangles,"representatives":rows,"gpu_buffer_identities_retained":true,"nine_source_gpu_payloads_byte_identical":true,"source_gpu_payload_sha256":gpu_after,"assembly_cpu_source_sha256":assembly_before,"clear_and_invalid_context_remove_stale_drawings":true,"cleared":capture(Some(directory),"cleared",&cleared),"invalid_context":capture(Some(directory),"invalid-context",&failed),"limitations":["Every exported source object has one primitive/material; the nine-primitive assembly is context, not a fabricated multi-primitive object","Representative GPU journeys cover 11 exact objects; independent CPU tests cover all 686 receipt rows and the full 103466-triangle partition","Wireframe context and green selection outline are inspection overlays; original source geometry, scalar materials and bytes are unchanged","Tiny objects may occupy only a few pixels in whole-ship context; Focus piece is required to inspect their actual geometry at useful size, and Fit ship restores the full context","Context mode retains all nine ship source batches; this acceptance does not establish lower hidden-geometry cost or frame-time improvements","This offscreen acceptance does not establish OS input, app focus, gameplay or engineering approval"]})).unwrap()).unwrap();
        }
        drop(harness);
        assert_eq!(gl.get_error(), glow::NO_ERROR);
    }
}
