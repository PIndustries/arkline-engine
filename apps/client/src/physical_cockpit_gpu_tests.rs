//! Actual App callbacks, new in-memory confirmation fixture, never live telemetry.
use super::*;

unsafe fn physical_frame(
    h: &mut Harness,
    app: &mut crate::Cockpit,
    ui_ms: u64,
    angle: f32,
    events: Vec<egui::Event>,
) -> (serde_json::Value, Vec<u8>) {
    let onfoot = app
        .snapshot
        .as_ref()
        .unwrap()
        .pilots
        .iter()
        .find(|p| p.name == app.pilot)
        .unwrap()
        .station_avatar
        .is_some();
    app.flight_view.head_look = [angle, 0.];
    app.hangar_view.head_look = [angle, 0.];
    let renderer = if onfoot {
        app.hangar_view.renderer.as_ref().unwrap().clone()
    } else {
        app.flight_view.renderer.as_ref().unwrap().clone()
    };
    let began = std::time::Instant::now();
    app.observed = std::time::Instant::now();
    let before = serde_json::to_vec(app.snapshot.as_ref().unwrap()).unwrap();
    let old = renderer
        .lock()
        .unwrap()
        .feed_cache
        .iter()
        .map(|c| (c.key, c.captured_ui_ms))
        .collect::<std::collections::BTreeMap<_, _>>();
    let output = h.ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(WIDTH as f32, HEIGHT as f32),
            )),
            time: Some(ui_ms as f64 / 1000.),
            focused: true,
            events,
            modifiers: egui::Modifiers {
                shift: true,
                ..Default::default()
            },
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut eframe::Frame::_new_kittest()),
    );
    let primitives = h.ctx.tessellate(output.shapes, output.pixels_per_point);
    h.gl.bind_framebuffer(
        glow::FRAMEBUFFER,
        Some(h.target.as_ref().unwrap().framebuffer),
    );
    h.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
    h.gl.disable(glow::SCISSOR_TEST);
    h.gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
    h.painter.paint_and_update_textures(
        [WIDTH as u32, HEIGHT as u32],
        output.pixels_per_point,
        &primitives,
        &output.textures_delta,
    );
    h.gl.finish();
    let elapsed_ms = began.elapsed().as_secs_f64() * 1000.;
    assert_eq!(h.gl.get_error(), glow::NO_ERROR);
    assert_eq!(
        serde_json::to_vec(app.snapshot.as_ref().unwrap()).unwrap(),
        before,
        "Presentation cannot mutate authority"
    );
    let mut proof = h
        .ctx
        .data(|d| {
            d.get_temp::<serde_json::Value>(egui::Id::new(if onfoot {
                "physical_cockpit_onfoot_proof"
            } else {
                "physical_cockpit_proof"
            }))
        })
        .unwrap();
    let renderer = renderer.lock().unwrap();
    assert!(renderer.feed_cache.len() <= 11);
    let captures = renderer
        .feed_cache
        .iter()
        .filter(|c| c.captured_ui_ms.is_some() && old.get(&c.key) != Some(&c.captured_ui_ms))
        .count();
    assert!(
        captures <= 1,
        "One actual camera capture per presented frame, got {captures}"
    );
    proof["actual_new_captures"] = captures.into();
    proof["cache"]=serde_json::json!(renderer.feed_cache.iter().map(|c|serde_json::json!({"key":c.key,"size":[c.width,c.height],"captured_ui_ms":c.captured_ui_ms,"painted_sim_ms":c.painted_ms})).collect::<Vec<_>>());
    proof["view_profiles"] = serde_json::json!(renderer.view_profiler.borrow().samples().iter().map(|m|serde_json::json!({
        "key":m.key,"sim_ms":m.frame,"cpu_submit_ns":m.cpu_submit_ns,"gpu_ns":m.gpu_elapsed_ns,
        "gpu_sim_ms":m.gpu_frame,"draws":m.visibility.submitted_draws,"triangles":m.visibility.submitted_triangles})).collect::<Vec<_>>());
    proof["ui_ms"] = ui_ms.into();
    proof["frame_wall_ms"] = elapsed_ms.into();
    assert!(proof["feeds"].as_array().unwrap().len() <= 11);
    assert!(proof["feeds"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["nonrecursive"] == true));
    let mut rgba = vec![0; WIDTH * HEIGHT * 4];
    h.gl.read_pixels(
        0,
        0,
        WIDTH as i32,
        HEIGHT as i32,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelPackData::Slice(Some(&mut rgba)),
    );
    (
        proof,
        rgba.chunks_exact(WIDTH * 4)
            .rev()
            .flatten()
            .copied()
            .collect(),
    )
}
fn save(
    directory: &Path,
    name: &str,
    mut proof: serde_json::Value,
    pixels: &[u8],
) -> serde_json::Value {
    image::save_buffer_with_format(
        directory.join(format!("{name}.png")),
        pixels,
        WIDTH as u32,
        HEIGHT as u32,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .unwrap();
    proof["png"] = format!("{name}.png").into();
    proof["rgba_sha256"] = hex::encode(Sha256::digest(pixels)).into();
    proof
}
fn install_authority(app: &mut crate::Cockpit, path: &Path) {
    let snapshot: Snapshot = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    if app
        .snapshot
        .as_ref()
        .is_some_and(|s| snapshot.server_time_ms < s.server_time_ms)
    {
        app.flight_view.reset_activity_session();
        app.hangar_view.reset_mechanisms();
        app.motion = Default::default();
    }
    app.pilot = snapshot.pilots[0].name.clone();
    app.frozen_time = Some(snapshot.server_time_ms);
    app.timeline_origin = Some(snapshot.server_time_ms);
    app.observed = std::time::Instant::now();
    app.motion.observe(&snapshot);
    app.snapshot = Some(snapshot);
}
#[test]
#[ignore = "Explicit physical seven-screen actual App and capture-budget proof in private EGL/CGL"]
fn native_physical_seven_screen_deployment_and_capture_budget() {
    unsafe {
        let directory = std::path::PathBuf::from(
            std::env::var_os("ARKLINE_PHYSICAL_COCKPIT_EVIDENCE").expect("New evidence directory"),
        );
        let source = std::path::PathBuf::from(
            std::env::var_os("ARKLINE_MECHANISM_AUTHORITY_SNAPSHOTS")
                .expect("Fresh authority journey export"),
        );
        std::fs::create_dir(&directory).unwrap();
        let (_context, gl) = PrivateContext::create();
        let mut h = Harness::new(gl.clone());
        let profile = directory.join("profile-physical-registration-6");
        std::fs::create_dir(&profile).unwrap();
        let mut app = crate::tests::view_test_cockpit_with_profile(&h.ctx, profile);
        app.flight_view = FlightView::new(Some(&gl));
        app.drone_camera = false;
        let mut receipts = Vec::new();
        let mut samples = Vec::new();
        let clock = std::time::Instant::now();
        let ui_time = || clock.elapsed().as_millis() as u64;
        install_authority(&mut app, &source.join("01-prelaunch-stowed.json"));
        let (proof, pixels) = physical_frame(&mut h, &mut app, ui_time(), 0., vec![]);
        receipts.push(save(&directory, "stowed-normal-status", proof, &pixels));
        // Exercise the supported keyboard toggle, not screenshot-only suppression.
        let modifiers = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;
        physical_frame(
            &mut h,
            &mut app,
            ui_time(),
            0.,
            vec![egui::Event::Key {
                key: egui::Key::H,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
        );
        assert!(app.identity_collapsed);
        physical_frame(
            &mut h,
            &mut app,
            ui_time(),
            0.,
            vec![egui::Event::Key {
                key: egui::Key::H,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers,
            }],
        );
        // Matched held-authority 0/1-source baselines; explicitly PAUSED, not live.
        app.embedded_paused = true;
        for count in [0, 1] {
            app.drone_camera = count == 1;
            app.camera_source = crate::camera_sources::CameraSource::Turret(0);
            for _ in 0..8 {
                let (mut proof, _) = physical_frame(&mut h, &mut app, ui_time(), 0., vec![]);
                proof["case"] = format!("paused-{count}-source").into();
                samples.push(proof);
            }
        }
        app.embedded_paused = false;
        app.drone_camera = false;
        app.camera_source = crate::camera_sources::CameraSource::Drones;
        for (name, file) in [
            ("launch-start", "02-confirmed-launch-start.json"),
            ("deploy-half-front", "03-deploy-half.json"),
            ("deploy-full-cold-front", "04-full-mining-deployed.json"),
        ] {
            install_authority(&mut app, &source.join(file));
            let (proof, pixels) = physical_frame(&mut h, &mut app, ui_time(), 0., vec![]);
            receipts.push(save(&directory, name, proof, &pixels));
        }
        let mut last = None;
        for i in 1..=12 {
            install_authority(&mut app, &source.join(format!("steady-{i:02}.json")));
            let (mut proof, pixels) = physical_frame(&mut h, &mut app, ui_time(), 0., vec![]);
            proof["case"] = "seven-source".into();
            samples.push(proof.clone());
            last = Some((proof, pixels));
        }
        let (proof, pixels) = last.unwrap();
        assert_eq!(proof["feeds"].as_array().unwrap().len(), 7);
        let keys: std::collections::BTreeSet<_> = proof["feeds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["key"].as_u64().unwrap())
            .collect();
        assert_eq!(keys.len(), 7);
        assert!(keys.iter().all(|key| proof["cache"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["key"].as_u64() == Some(*key) && c["captured_ui_ms"].is_number())));
        receipts.push(save(&directory, "seven-warm-front", proof, &pixels));
        for (name, angle) in [("seven-warm-port", 0.45), ("seven-warm-starboard", -0.45)] {
            let (proof, pixels) = physical_frame(&mut h, &mut app, ui_time(), angle, vec![]);
            receipts.push(save(&directory, name, proof, &pixels));
        }
        app.drone_camera = true;
        app.drone_split = true;
        app.selected_drone = app.snapshot.as_ref().unwrap().pilots[0]
            .drones
            .first()
            .map(|d| d.id);
        for i in 13..=24 {
            install_authority(&mut app, &source.join(format!("steady-{i:02}.json")));
            let (mut proof, pixels) = physical_frame(&mut h, &mut app, ui_time(), 0., vec![]);
            proof["case"] = "eleven-source".into();
            samples.push(proof.clone());
            if i == 24 {
                assert_eq!(proof["feeds"].as_array().unwrap().len(), 11);
                receipts.push(save(&directory, "eleven-warm-front", proof, &pixels));
            }
        }
        for (name, file) in [
            ("warp-accepted-start", "05-accepted-warp-start.json"),
            ("warp-stowing-half", "06-warp-retract-half.json"),
            ("warp-stowed", "07-warp-stowed.json"),
            ("arrival-stowed", "08-normal-arrival-no-drones.json"),
        ] {
            install_authority(&mut app, &source.join(file));
            let (proof, pixels) = physical_frame(&mut h, &mut app, ui_time(), 0., vec![]);
            receipts.push(save(&directory, name, proof, &pixels));
        }
        app.drone_camera = false;
        let mut motion = Vec::new();
        for phase in ["deploy", "warp"] {
            for i in 0..=12 {
                install_authority(
                    &mut app,
                    &source.join(format!("motion-{phase}-{i:02}.json")),
                );
                let (proof, pixels) = physical_frame(&mut h, &mut app, ui_time(), 0., vec![]);
                motion.push(save(
                    &directory,
                    &format!("motion-{phase}-{i:02}"),
                    proof,
                    &pixels,
                ));
            }
        }
        if let Some(routine) = std::env::var_os("ARKLINE_MECHANISM_ROUTINE_RESPONSES") {
            let routine = std::path::PathBuf::from(routine);
            app.hangar_view = HangarView::new(Some(&gl));
            app.hangar_view.on_foot_view = crate::flight3d::hangar::OnFootView::FirstPerson;
            for (name, file) in [
                ("routine-seated", "routine-seated-response.json"),
                ("routine-standing", "routine-standing-response.json"),
            ] {
                let response: arkline_protocol::Response =
                    serde_json::from_slice(&std::fs::read(routine.join(file)).unwrap()).unwrap();
                assert!(response.ok);
                let snapshot = response.snapshot.unwrap();
                app.assets = response.assets;
                app.pilot = snapshot.pilots[0].name.clone();
                app.frozen_time = Some(snapshot.server_time_ms);
                app.timeline_origin = Some(snapshot.server_time_ms);
                app.observed = std::time::Instant::now();
                app.flight_view.reset_activity_session();
                app.hangar_view.reset_mechanisms();
                app.motion = Default::default();
                app.motion.observe(&snapshot);
                app.snapshot = Some(snapshot);
                app.embedded_paused = true;
                // The standing eye is closer to the wraparound arc. The outer
                // two physical screens leave the forward frustum; prove their
                // real images with ordinary head turns, not a wider fake camera.
                let views: &[(&str, f32)] = if name == "routine-standing" {
                    &[("front", 0.), ("port", 0.55), ("starboard", -0.55)]
                } else {
                    &[("front", 0.)]
                };
                let mut seen_bays = std::collections::BTreeSet::new();
                for (view, angle) in views {
                    for _ in 0..12 {
                        physical_frame(&mut h, &mut app, ui_time(), *angle, vec![]);
                    }
                    let (proof, pixels) =
                        physical_frame(&mut h, &mut app, ui_time(), *angle, vec![]);
                    let saved = save(
                        &directory,
                        &format!("{name}-{view}"),
                        proof.clone(),
                        &pixels,
                    );
                    std::fs::write(
                        directory.join(format!("{name}-{view}-debug.json")),
                        serde_json::to_vec_pretty(&proof).unwrap(),
                    )
                    .unwrap();
                    let feeds = proof["feeds"].as_array().unwrap();
                    if name == "routine-seated" {
                        assert_eq!(feeds.len(), 7, "{proof}");
                    }
                    for feed in feeds {
                        seen_bays.insert(feed["bay"].as_u64().unwrap());
                        assert!(
                            proof["cache"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .any(|cache| cache["key"] == feed["key"]
                                    && cache["captured_ui_ms"].is_number()),
                            "Each visible physical screen must have its own actual captured image"
                        );
                    }
                    receipts.push(saved);
                }
                assert_eq!(seen_bays, (8..15).collect(), "All seven own images must be visible and captured across the real head-turn views");
            }
            app.embedded_paused = false;
        }
        std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
            "scope":"Actual Cockpit::update using unmodified snapshots exported by fresh real-Request authority journey; exact authority timestamps and observed receipt freshness; actual GL mesh/depth/camera callbacks",
            "authority_source":source,"authority_receipt":serde_json::from_slice::<serde_json::Value>(&std::fs::read(source.join("receipt.json")).unwrap()).unwrap(),
            "registration_revision":arkline_protocol::PIONEER_DOCKED_REGISTRATION_REVISION,"capture_source_cap":11,"maximum_new_captures_per_ui_frame":1,"max_capture_edge_px":384,
            "renderer":gl.get_parameter_string(glow::RENDERER),"captures":receipts,"frame_samples":samples,"motion_frames":motion,
            "limits":["Private EGL headless aggregate frame wall time is not desktop FPS","0/1 baseline deliberately pauses an accepted snapshot; 7/11 sample real advancing 40ms authority snapshots","No hydraulic/mass/power subsystem simulation"]
        })).unwrap()).unwrap();
    }
}
