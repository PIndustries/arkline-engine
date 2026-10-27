//! Test-only matched software-render measurement. No production render/cache changes.
use super::*;

unsafe fn profile_frame(
    h: &mut Harness,
    app: &mut crate::Cockpit,
    clock: &std::time::Instant,
    size: [usize; 2],
    headlook: [f32; 2],
    events: Vec<egui::Event>,
    capture_pixels: bool,
) -> (serde_json::Value, Vec<u8>) {
    let [yaw, pitch] = headlook;
    app.flight_view.head_look = headlook;
    app.hangar_view.head_look = [yaw, pitch];
    let before = serde_json::to_vec(app.snapshot.as_ref().unwrap()).unwrap();
    h.ctx
        .data_mut(|d| d.remove::<serde_json::Value>(egui::Id::new("drone_cassette_readout_proof")));
    let ui_ms = clock.elapsed().as_millis() as u64;
    let frame_started = std::time::Instant::now();
    let phase = std::time::Instant::now();
    let output = h.ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(size[0] as f32, size[1] as f32),
            )),
            time: Some(ui_ms as f64 / 1000.),
            focused: true,
            modifiers: egui::Modifiers {
                shift: true,
                ..Default::default()
            },
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut eframe::Frame::_new_kittest()),
    );
    let update_ms = phase.elapsed().as_secs_f64() * 1000.;
    let phase = std::time::Instant::now();
    let primitives = h.ctx.tessellate(output.shapes, output.pixels_per_point);
    let tessellate_ms = phase.elapsed().as_secs_f64() * 1000.;
    let phase = std::time::Instant::now();
    h.gl.bind_framebuffer(
        glow::FRAMEBUFFER,
        Some(h.target.as_ref().unwrap().framebuffer),
    );
    h.gl.viewport(0, 0, size[0] as i32, size[1] as i32);
    h.gl.disable(glow::SCISSOR_TEST);
    h.gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
    h.painter.paint_and_update_textures(
        [size[0] as u32, size[1] as u32],
        output.pixels_per_point,
        &primitives,
        &output.textures_delta,
    );
    let submit_ms = phase.elapsed().as_secs_f64() * 1000.;
    let phase = std::time::Instant::now();
    h.gl.finish();
    let finish_wait_ms = phase.elapsed().as_secs_f64() * 1000.;
    let through_finish_ms = frame_started.elapsed().as_secs_f64() * 1000.;
    assert_eq!(h.gl.get_error(), glow::NO_ERROR);
    assert_eq!(
        before,
        serde_json::to_vec(app.snapshot.as_ref().unwrap()).unwrap()
    );
    let rows = h
        .ctx
        .data(|d| d.get_temp::<serde_json::Value>(egui::Id::new("drone_cassette_readout_proof")))
        .unwrap_or_default();
    let mut proof = serde_json::json!({"readouts":rows,"size":size,"headlook":[yaw,pitch],
        "availability":app.drone_telemetry_time.availability.label(),
        "snapshot_sha256":hex::encode(Sha256::digest(&before)),
        "confirmed_ms":app.drone_telemetry_time.server_ms,
        "receipt_age_ms":app.performance_snapshot_received.map(|t|t.elapsed().as_millis() as u64),
        "world_observation_age_ms":app.observed.elapsed().as_millis() as u64});
    proof["registered_camera_views"] = h
        .ctx
        .data(|d| d.get_temp::<serde_json::Value>(egui::Id::new("physical_cockpit_proof")))
        .map(|v| v["feeds"].clone())
        .unwrap_or_default();
    let renderer = app.flight_view.renderer.as_ref().unwrap().lock().unwrap();
    proof["cache"] = serde_json::json!(renderer.feed_cache.iter().map(|c|serde_json::json!({
        "key":c.key,"size":[c.width,c.height],"captured_ui_ms":c.captured_ui_ms,"painted_sim_ms":c.painted_ms,
        "image_age_ms":c.captured_ui_ms.map(|t|ui_ms.saturating_sub(t)),
        "represented_sim_age_ms":c.painted_ms.map(|t|app.snapshot.as_ref().unwrap().server_time_ms.saturating_sub(t))
    })).collect::<Vec<_>>());
    proof["last_retained_view_profiles"] = serde_json::json!(renderer.view_profiler.borrow().samples().iter().map(|m|serde_json::json!({
        "key":m.key,"sim_ms":m.frame,"cpu_submit_ns":m.cpu_submit_ns,"gpu_ns":m.gpu_elapsed_ns,
        "gpu_sim_ms":m.gpu_frame,"draws":m.visibility.submitted_draws,"triangles":m.visibility.submitted_triangles
    })).collect::<Vec<_>>());
    drop(renderer);
    proof["ui_ms"] = ui_ms.into();
    proof["phases_ms"] = serde_json::json!({"app_update":update_ms,"tessellate":tessellate_ms,
        "cpu_submit_including_driver_stalls":submit_ms,"gpu_finish_wait":finish_wait_ms,"through_finish":through_finish_ms});
    if !capture_pixels {
        return (proof, Vec::new());
    }
    let phase = std::time::Instant::now();
    let mut pixels = vec![0; size[0] * size[1] * 4];
    h.gl.read_pixels(
        0,
        0,
        size[0] as i32,
        size[1] as i32,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelPackData::Slice(Some(&mut pixels)),
    );
    proof["readback_ms"] = (phase.elapsed().as_secs_f64() * 1000.).into();
    (
        proof,
        pixels
            .chunks_exact(size[0] * 4)
            .rev()
            .flatten()
            .copied()
            .collect(),
    )
}

fn distribution(values: impl Iterator<Item = f64>) -> serde_json::Value {
    let mut values: Vec<_> = values.collect();
    values.sort_by(f64::total_cmp);
    assert!(!values.is_empty());
    let percentile = |p: f64| values[((values.len() as f64 * p).ceil() as usize).saturating_sub(1)];
    serde_json::json!({"samples":values.len(),"median":percentile(0.5),"p95":percentile(0.95),
        "min":values[0],"max":values[values.len()-1],"mean":values.iter().sum::<f64>()/values.len() as f64})
}

#[test]
#[ignore = "Explicit matched Linux native measurements; actual exported authority; no runtime FPS claim"]
fn native_matched_seated_frame_and_camera_age_audit() {
    unsafe {
        const WARMUP: usize = 16;
        const SAMPLES: usize = 48;
        const DRAIN: usize = 16;
        let directory = PathBuf::from(std::env::var_os("ARKLINE_MATCHED_FRAME_EVIDENCE").unwrap());
        let source =
            PathBuf::from(std::env::var_os("ARKLINE_NINE_FEED_PROFILE_RESPONSES").unwrap());
        std::fs::create_dir(&directory).unwrap();
        let initial = std::fs::read(source.join("initial-response.json")).unwrap();
        let inputs: Vec<_> = (1..=WARMUP + SAMPLES + DRAIN)
            .map(|n| {
                let file = format!("steady-{n:02}-response.json");
                let bytes = std::fs::read(source.join(&file)).unwrap();
                let response: Response = serde_json::from_slice(&bytes).unwrap();
                assert!(response.ok);
                let ms = response.snapshot.as_ref().unwrap().server_time_ms;
                (file, bytes, ms)
            })
            .collect();
        assert!(inputs.windows(2).all(|pair| pair[1].2 == pair[0].2 + 40));
        let (_context, gl) = PrivateContext::create();
        let backend = serde_json::json!({"renderer":gl.get_parameter_string(glow::RENDERER),
            "vendor":gl.get_parameter_string(glow::VENDOR),"version":gl.get_parameter_string(glow::VERSION)});
        let mut cases = Vec::new();
        for size in [[900, 650], [1280, 720]] {
            for advancing in [false, true] {
                let name = format!(
                    "{}x{}-{}",
                    size[0],
                    size[1],
                    if advancing {
                        "advancing-authority"
                    } else {
                        "frozen-no-updates"
                    }
                );
                let profile = directory.join(&name);
                std::fs::create_dir(&profile).unwrap();
                let mut h = Harness::new(gl.clone());
                let mut app = crate::tests::view_test_cockpit_with_profile(&h.ctx, profile);
                app.flight_view = FlightView::new(Some(&gl));
                app.hangar_view = HangarView::new(Some(&gl));
                app.drone_camera = false;
                let (tx, rx) = mpsc::channel();
                app.updates = Some(rx);
                let (commands, requests) = mpsc::channel();
                app.commands = Some(commands);
                tx.send(crate::Update::Ready).unwrap();
                app.poll();
                deliver(&mut app, &tx, &initial);
                let clock = std::time::Instant::now();
                let mut previous = std::collections::BTreeMap::new();
                let mut samples = Vec::new();
                let mut warmup = Vec::new();
                let mut drain = Vec::new();
                for (index, (file, bytes, _)) in inputs.iter().enumerate() {
                    let delivery_started = std::time::Instant::now();
                    if index < WARMUP || advancing {
                        deliver(&mut app, &tx, bytes);
                    }
                    let delivery_ms = delivery_started.elapsed().as_secs_f64() * 1000.;
                    let (mut proof, pixels) =
                        profile_frame(&mut h, &mut app, &clock, size, [0., 0.], neutral(), false);
                    assert!(
                        pixels.is_empty(),
                        "Measured windows cannot read screenshot pixels"
                    );
                    let cache = proof["cache"].as_array().unwrap();
                    assert!(cache.len() <= 9);
                    assert!(!cache.is_empty(), "Actual visible sources must render");
                    if index >= WARMUP {
                        assert!(
                            cache.iter().all(|row| row["captured_ui_ms"].is_number()),
                            "Every allocated visible source must have real pixels after warmup"
                        );
                    }
                    let admitted = cache
                        .iter()
                        .filter(|row| {
                            row["captured_ui_ms"].is_number()
                                && previous.get(&row["key"].as_u64().unwrap())
                                    != Some(&row["captured_ui_ms"])
                        })
                        .count();
                    assert!(
                        admitted <= 1,
                        "Production global admission policy remains intact"
                    );
                    previous = cache
                        .iter()
                        .map(|row| (row["key"].as_u64().unwrap(), row["captured_ui_ms"].clone()))
                        .collect();
                    proof["new_captures"] = admitted.into();
                    proof["response_delivery_ms"] = delivery_ms.into();
                    proof["response_file"] = if index < WARMUP || advancing {
                        file.clone()
                    } else {
                        inputs[WARMUP - 1].0.clone()
                    }
                    .into();
                    proof["response_delivered_this_frame"] = (index < WARMUP || advancing).into();
                    if index < WARMUP {
                        warmup.push(proof);
                    } else if index < WARMUP + SAMPLES {
                        samples.push(proof);
                    } else {
                        drain.push(proof);
                    }
                }
                let distributions = serde_json::json!([
                    "app_update",
                    "tessellate",
                    "cpu_submit_including_driver_stalls",
                    "gpu_finish_wait",
                    "through_finish"
                ]
                .into_iter()
                .map(|key| (
                    key,
                    distribution(
                        samples
                            .iter()
                            .map(|s| s["phases_ms"][key].as_f64().unwrap())
                    )
                ))
                .collect::<std::collections::BTreeMap<_, _>>());
                let (proof, pixels) =
                    profile_frame(&mut h, &mut app, &clock, size, [0., 0.], neutral(), true);
                let image = save(&directory, &name, size, proof, &pixels);
                assert!(
                    requests.try_recv().is_err(),
                    "Profiling must not dispatch gameplay"
                );
                cases.push(serde_json::json!({"name":name,"size":size,"advancing":advancing,"warmup":warmup,
                    "samples":samples,"query_drain":drain,"phase_distributions_ms":distributions,"capture_outside_measured_window":image}));
            }
        }
        let receipt = serde_json::json!({"source_commit":std::env::var("ARKLINE_FRAME_SOURCE_COMMIT").unwrap_or_default(),
            "backend":backend,"warmup_frames":WARMUP,"measured_frames":SAMPLES,"drain_frames":DRAIN,
            "initial_sha256":hex::encode(Sha256::digest(&initial)),
            "inputs":inputs.iter().map(|(file,bytes,ms)|serde_json::json!({"file":file,"sha256":hex::encode(Sha256::digest(bytes)),"server_ms":ms})).collect::<Vec<_>>(),
            "cases":cases,"limits":["Actual authoritative40ms sequence replayed at host speed, not real-time server/network pacing",
                "Frozen case delivers no new Responses after16warmup frames; honest authority and image ages continue growing",
                "Same camera pose and default UI, canonical eye/FOV/assets, same16/48/16frame counts at each resolution",
                "CPU submission includes driver stalls; glFinish wait is not an isolated GPU execution timer; no desktop/vsync FPS measured",
                "No pixel readback in measured windows; final screenshot readback separately timed outside samples",
                "Last-retained asynchronous view profiles may be old and drone:unbound aliases types; never sum as current total",
                "One global capture per presented frame retained; image wall age and represented simulation age are separate",
                "All nine equipment mechanisms deployed; canonical neutral frustum naturally culls off-screen camera callbacks. Actual rendered source keys/counts recorded; resolutions may not have equal visible-feed workload"]});
        std::fs::write(
            directory.join("receipt.json"),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
    }
}
