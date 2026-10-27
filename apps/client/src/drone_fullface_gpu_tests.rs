//! Full fixed data faces in the actual App; no camera/geometry or source changes.
use super::*;
use arkline_protocol::Response;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};

fn deliver(app: &mut crate::Cockpit, tx: &Sender<crate::Update>, bytes: &[u8]) {
    let response: Response = serde_json::from_slice(bytes).unwrap();
    assert!(response.ok);
    let s = response.snapshot.as_ref().unwrap();
    app.pilot = s.pilots[0].name.clone();
    let ms = s.server_time_ms;
    tx.send(crate::Update::Response(Box::new(response), false))
        .unwrap();
    app.poll();
    app.frozen_time = Some(ms);
}
pub(super) unsafe fn frame(
    h: &mut Harness,
    app: &mut crate::Cockpit,
    clock: &std::time::Instant,
    size: [usize; 2],
    yaw: f32,
    events: Vec<egui::Event>,
) -> (serde_json::Value, Vec<u8>) {
    frame_at_pitch(h, app, clock, size, yaw, 0.30, events)
}
unsafe fn frame_at_pitch(
    h: &mut Harness,
    app: &mut crate::Cockpit,
    clock: &std::time::Instant,
    size: [usize; 2],
    yaw: f32,
    pitch: f32,
    events: Vec<egui::Event>,
) -> (serde_json::Value, Vec<u8>) {
    app.flight_view.head_look = [yaw, pitch];
    app.hangar_view.head_look = [yaw, pitch];
    let before = serde_json::to_vec(app.snapshot.as_ref().unwrap()).unwrap();
    h.ctx
        .data_mut(|d| d.remove::<serde_json::Value>(egui::Id::new("drone_cassette_readout_proof")));
    let output = h.ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(size[0] as f32, size[1] as f32),
            )),
            time: Some(clock.elapsed().as_secs_f64()),
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
    let primitives = h.ctx.tessellate(output.shapes, output.pixels_per_point);
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
    h.gl.finish();
    assert_eq!(h.gl.get_error(), glow::NO_ERROR);
    assert_eq!(
        before,
        serde_json::to_vec(app.snapshot.as_ref().unwrap()).unwrap()
    );
    let rows = h
        .ctx
        .data(|d| d.get_temp::<serde_json::Value>(egui::Id::new("drone_cassette_readout_proof")))
        .unwrap_or_default();
    let proof = serde_json::json!({"readouts":rows,"size":size,"headlook":[yaw,pitch],
        "availability":app.drone_telemetry_time.availability.label(),
        "snapshot_sha256":hex::encode(Sha256::digest(&before)),
        "confirmed_ms":app.drone_telemetry_time.server_ms,
        "receipt_age_ms":app.performance_snapshot_received.map(|t|t.elapsed().as_millis() as u64),
        "world_observation_age_ms":app.observed.elapsed().as_millis() as u64});
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
fn save(
    directory: &std::path::Path,
    name: &str,
    size: [usize; 2],
    mut proof: serde_json::Value,
    pixels: &[u8],
) -> serde_json::Value {
    image::save_buffer_with_format(
        directory.join(format!("{name}.png")),
        pixels,
        size[0] as u32,
        size[1] as u32,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .unwrap();
    proof["png"] = format!("{name}.png").into();
    proof["rgba_sha256"] = hex::encode(Sha256::digest(pixels)).into();
    proof
}
fn neutral() -> Vec<egui::Event> {
    vec![egui::Event::PointerMoved(egui::pos2(700., 600.))]
}

#[test]
#[ignore = "Explicit actual App full-face data-panel real Response replay and narrow-aspect proof"]
fn native_fullface_drone_data_real_responses_and_narrow_view() {
    unsafe {
        let directory = PathBuf::from(
            std::env::var_os("ARKLINE_FULLFACE_EVIDENCE").expect("Fresh evidence directory"),
        );
        let source = PathBuf::from(
            std::env::var_os("ARKLINE_FULLFACE_RESPONSES").expect("Real response directory"),
        );
        std::fs::create_dir(&directory).unwrap();
        let (_context, gl) = PrivateContext::create();
        let mut h = Harness::new(gl.clone());
        let profile = directory.join("fresh-fullface-registration-9");
        std::fs::create_dir(&profile).unwrap();
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
        let clock = std::time::Instant::now();
        let mut captures = Vec::new();
        let mut inputs = Vec::new();
        for (name, file) in [
            ("mining", "01-mining-response.json"),
            ("hold", "03-hold-old-pose-fresh-snapshot-response.json"),
            ("returning", "04-returning-response.json"),
        ] {
            let bytes = std::fs::read(source.join(file)).unwrap();
            inputs.push(
                serde_json::json!({"file":file,"sha256":hex::encode(Sha256::digest(&bytes))}),
            );
            for i in 0..9 {
                deliver(&mut app, &tx, &bytes);
                let events = if name == "mining" && i < 2 {
                    vec![egui::Event::Key {
                        key: egui::Key::H,
                        physical_key: None,
                        pressed: i == 0,
                        repeat: false,
                        modifiers: egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                    }]
                } else {
                    neutral()
                };
                frame(&mut h, &mut app, &clock, [1280, 720], 0.55, events);
            }
            for (view, size, yaw) in [("port", [1280, 720], 0.55), ("narrow", [800, 650], 0.85)] {
                deliver(&mut app, &tx, &bytes);
                let (proof, pixels) = frame(&mut h, &mut app, &clock, size, yaw, neutral());
                assert_eq!(proof["availability"], "LIVE");
                let rows = proof["readouts"].as_array().unwrap();
                let chosen = rows
                    .iter()
                    .find(|r| r["id"] == 1)
                    .expect("Actual selected fixed data face");
                assert_eq!(chosen["full_face"], true);
                assert_eq!(
                    chosen["health"]["maximum"].as_f64(),
                    Some(f64::from(arkline_protocol::DRONE_HIT_POINTS))
                );
                assert_eq!(chosen["health"]["fraction"], 1.);
                assert!(chosen["heading_yaw_pitch"].is_array());
                if name == "mining" {
                    assert!(chosen["cycle_fraction"].as_f64().unwrap() > 0.);
                } else {
                    assert!(chosen["cycle_fraction"].is_null());
                }
                captures.push(save(
                    &directory,
                    &format!("{name}-{view}"),
                    size,
                    proof,
                    &pixels,
                ));
            }
            if name == "mining" {
                for _ in 0..7 {
                    deliver(&mut app, &tx, &bytes);
                    frame(&mut h, &mut app, &clock, [1280, 720], -0.55, neutral());
                }
                deliver(&mut app, &tx, &bytes);
                let (proof, pixels) =
                    frame(&mut h, &mut app, &clock, [1280, 720], -0.55, neutral());
                captures.push(save(
                    &directory,
                    "mining-starboard",
                    [1280, 720],
                    proof,
                    &pixels,
                ));
                // Supported title-bar collapse, not a test-only overlay filter.
                deliver(&mut app, &tx, &bytes);
                frame(&mut h, &mut app, &clock, [1280, 720], 0.55, neutral());
                let rect = h
                    .ctx
                    .data(|d| {
                        d.get_temp::<egui::Rect>(egui::Id::new("ship_interaction_window_rect"))
                    })
                    .unwrap();
                let point = rect.min + egui::vec2(20., 20.);
                for pressed in [true, false] {
                    deliver(&mut app, &tx, &bytes);
                    frame(
                        &mut h,
                        &mut app,
                        &clock,
                        [1280, 720],
                        0.55,
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
                assert!(!egui::collapsing_header::CollapsingState::load(
                    &h.ctx,
                    egui::Id::new("docked_traversal_actions").with("collapsing")
                )
                .unwrap()
                .is_open());
                assert!(
                    requests.try_recv().is_err(),
                    "Overlay collapse must not emit a cockpit command"
                );
                for _ in 0..3 {
                    deliver(&mut app, &tx, &bytes);
                    frame(&mut h, &mut app, &clock, [1280, 720], 0.55, neutral());
                }
                deliver(&mut app, &tx, &bytes);
                let (proof, pixels) = frame(&mut h, &mut app, &clock, [1280, 720], 0.55, neutral());
                captures.push(save(
                    &directory,
                    "mining-port-clean",
                    [1280, 720],
                    proof,
                    &pixels,
                ));
            }
        }
        // Reconnect establishes a new replay session; never rewrite source times.
        tx.send(crate::Update::Ready).unwrap();
        app.poll();
        let bytes = std::fs::read(source.join("01-mining-response.json")).unwrap();
        deliver(&mut app, &tx, &bytes);
        let (expected, _) = frame(&mut h, &mut app, &clock, [1280, 720], 0.55, neutral());
        let receipt = app.performance_snapshot_received;
        std::thread::sleep(std::time::Duration::from_millis(1600));
        for state in ["stale", "paused", "disconnected"] {
            if state == "paused" {
                app.embedded = true;
                app.embedded_paused = true;
            }
            if state == "disconnected" {
                tx.send(crate::Update::Reconnecting(
                    "Controlled full-face proof".into(),
                ))
                .unwrap();
                app.poll();
            }
            let (proof, pixels) = frame(&mut h, &mut app, &clock, [1280, 720], 0.55, neutral());
            assert_eq!(receipt, app.performance_snapshot_received);
            assert!(
                app.observed.elapsed().as_millis() > 1500,
                "Neither world nor telemetry receipt age may refresh from rendering"
            );
            if state == "disconnected" {
                // Actual App replaces the world with reconnect UI; never reuse
                // an earlier frame's surface proof as visible disconnected glass.
                assert!(proof["readouts"].is_null());
                let drone = &app.snapshot.as_ref().unwrap().pilots[0].drones[0];
                let data =
                    crate::drone_telemetry::DroneReadout::new(drone, app.drone_telemetry_time);
                let previous = &expected["readouts"][0];
                assert_eq!(
                    serde_json::json!(data.heading_yaw_pitch_deg),
                    previous["heading_yaw_pitch"]
                );
                assert_eq!(
                    serde_json::json!(data.cycle.map(|c| c.fraction)),
                    previous["cycle_fraction"]
                );
            } else {
                let previous = &expected["readouts"][0];
                let current = &proof["readouts"][0];
                for key in ["heading_yaw_pitch", "health", "signal", "cycle_fraction"] {
                    assert_eq!(previous[key], current[key]);
                }
            }
            assert!(proof["availability"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .starts_with(state));
            captures.push(save(
                &directory,
                &format!("mining-{state}"),
                [1280, 720],
                proof,
                &pixels,
            ));
        }
        std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
            "scope":"Actual App/EGL controlled replay of unchanged real Request Responses, accepted via normal poll; fresh profile; actual narrow 800x650 viewport, no screenshot cropping.",
            "inputs":inputs,"captures":captures,"source_mutation":false,
            "meaning":"Heading schematic shows +X/+Z yaw and independent pitch, no roll. HP max is protocol DRONE_HIT_POINTS; signal is reported 0..1. One confirmed mining cycle; no invented next cycle, battery, thrust or video.",
            "clock":"Confirmed authority time only. Repeated identical actual response payloads warm renderer; elapsed UI time does not animate telemetry. Stale/paused cases stop delivery and preserve visible gauge/heading/cycle values. Disconnect replaces the cockpit with reconnect UI; no visible gauges claimed, underlying confirmed model remains frozen.",
            "limitations":["Headless llvmpipe, not desktop FPS","Narrow head-look proves individual faces, not all five simultaneously readable","No diagnostic HP or signal values substituted into real response captures"]
        })).unwrap()).unwrap();
    }
}

#[test]
#[ignore = "Explicit neutral-glance cabin polish proof using unchanged authority"]
fn native_cabin_glance_health_and_panel_finish() {
    unsafe {
        let directory = PathBuf::from(
            std::env::var_os("ARKLINE_CABIN_POLISH_EVIDENCE").expect("Fresh evidence directory"),
        );
        let source = PathBuf::from(
            std::env::var_os("ARKLINE_FULLFACE_RESPONSES").expect("Actual response directory"),
        );
        std::fs::create_dir(&directory).unwrap();
        let bytes = std::fs::read(source.join("01-mining-response.json")).unwrap();
        let (_context, gl) = PrivateContext::create();
        let mut h = Harness::new(gl.clone());
        let profile = directory.join("fresh-registration-11");
        std::fs::create_dir(&profile).unwrap();
        let mut app = crate::tests::view_test_cockpit_with_profile(&h.ctx, profile);
        app.flight_view = FlightView::new(Some(&gl));
        app.hangar_view = HangarView::new(Some(&gl));
        app.drone_camera = false;
        let (tx, rx) = mpsc::channel();
        app.updates = Some(rx);
        let (commands, _requests) = mpsc::channel();
        app.commands = Some(commands);
        tx.send(crate::Update::Ready).unwrap();
        app.poll();
        let clock = std::time::Instant::now();
        for index in 0..9 {
            deliver(&mut app, &tx, &bytes);
            let events = if index < 2 {
                vec![egui::Event::Key {
                    key: egui::Key::H,
                    physical_key: None,
                    pressed: index == 0,
                    repeat: false,
                    modifiers: egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                }]
            } else {
                neutral()
            };
            frame_at_pitch(&mut h, &mut app, &clock, [1280, 720], 0., 0., events);
            if index == 2 {
                captain_displays_tests::toggle_interaction(&mut h, &mut app, &clock, false);
            }
        }
        assert!(
            app.identity_collapsed,
            "Use the supported status collapse control"
        );
        assert!(!egui::collapsing_header::CollapsingState::load(
            &h.ctx,
            egui::Id::new("docked_traversal_actions").with("collapsing")
        )
        .unwrap()
        .is_open());
        let mut captures = Vec::new();
        let mut inspected_bays = std::collections::BTreeSet::new();
        for (name, size, yaw, pitch) in [
            ("neutral-front", [1280, 720], 0., 0.),
            ("neutral-port", [1280, 720], 0.55, 0.),
            ("neutral-starboard", [1280, 720], -0.55, 0.),
            ("detail-up", [1280, 720], 0., 0.30),
            ("neutral-narrow", [800, 650], 0., 0.),
        ] {
            deliver(&mut app, &tx, &bytes);
            let (proof, pixels) =
                frame_at_pitch(&mut h, &mut app, &clock, size, yaw, pitch, neutral());
            assert_eq!(proof["availability"], "LIVE");
            let readouts = proof["readouts"].as_array().unwrap();
            assert!(!readouts.is_empty() && readouts.len() <= 5);
            if name == "neutral-front" {
                assert!(
                    readouts.iter().any(|row| row["bay"] == 2),
                    "Center drone face must be visible at neutral glance"
                );
            }
            for row in readouts {
                inspected_bays.insert(row["bay"].as_u64().unwrap());
                assert_eq!(row["health"]["maximum"], 100.);
                assert_eq!(row["glance_health"], "HP 100/100");
            }
            captures.push(save(&directory, name, size, proof, &pixels));
        }
        std::fs::write(directory.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "source":"unchanged actual server mining Response; canonical FOV/eye; no camera geometry changes",
            "input":source.join("01-mining-response.json"), "input_sha256":hex::encode(Sha256::digest(&bytes)),
            "captures":captures
        })).unwrap()).unwrap();
    }
}

#[test]
#[ignore = "Read-only actual seated narrow/headturn and projected-control audit"]
fn native_seated_narrow_drone_and_control_audit() {
    unsafe {
        let directory = PathBuf::from(std::env::var_os("ARKLINE_SEATED_AUDIT_EVIDENCE").unwrap());
        let source = PathBuf::from(std::env::var_os("ARKLINE_FULLFACE_RESPONSES").unwrap());
        std::fs::create_dir(&directory).unwrap();
        let bytes = std::fs::read(source.join("01-mining-response.json")).unwrap();
        let (_context, gl) = PrivateContext::create();
        let mut h = Harness::new(gl.clone());
        let clock = std::time::Instant::now();
        let mut captures = Vec::new();
        for (size_label, size) in [
            ("960x640", [960, 640]),
            ("desktop-min", [900, 650]),
            ("below-min-stress", [800, 650]),
        ] {
            let profile = directory.join(size_label);
            std::fs::create_dir(&profile).unwrap();
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
            for _ in 0..5 {
                deliver(&mut app, &tx, &bytes);
                frame_at_pitch(&mut h, &mut app, &clock, size, 0., 0., neutral());
            }
            deliver(&mut app, &tx, &bytes);
            let (proof, pixels) = frame_at_pitch(&mut h, &mut app, &clock, size, 0., 0., neutral());
            captures.push(save(
                &directory,
                &format!("{size_label}-default-front"),
                size,
                proof,
                &pixels,
            ));
            // Exercise the supported status toggle; retain a default-open capture above.
            for pressed in [true, false] {
                deliver(&mut app, &tx, &bytes);
                frame_at_pitch(
                    &mut h,
                    &mut app,
                    &clock,
                    size,
                    0.,
                    0.,
                    vec![egui::Event::Key {
                        key: egui::Key::H,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                    }],
                );
            }
            for (bay, yaw) in [(0, 0.85), (1, 0.4), (2, 0.), (3, -0.4), (4, -0.85)] {
                for _ in 0..3 {
                    deliver(&mut app, &tx, &bytes);
                    frame_at_pitch(&mut h, &mut app, &clock, size, yaw, 0.30, neutral());
                }
                deliver(&mut app, &tx, &bytes);
                let started = std::time::Instant::now();
                let (mut proof, pixels) =
                    frame_at_pitch(&mut h, &mut app, &clock, size, yaw, 0.30, neutral());
                proof["actual_frame_wall_ms"] =
                    serde_json::json!(started.elapsed().as_secs_f64() * 1000.);
                assert!(
                    proof["readouts"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|row| row["bay"] == bay),
                    "The intended drone face must be rendered at its ordinary headturn"
                );
                proof["intended_bay"] = bay.into();
                proof["identity_collapsed"] = app.identity_collapsed.into();
                captures.push(save(
                    &directory,
                    &format!("{size_label}-drone-{}", bay + 1),
                    size,
                    proof,
                    &pixels,
                ));
            }
            app.service_tab = crate::ServiceTab::Displays;
            app.cockpit_console_detail = true;
            for (view, pitch) in [("neutral", 0.), ("look-down", -0.2)] {
                for _ in 0..3 {
                    deliver(&mut app, &tx, &bytes);
                    frame_at_pitch(&mut h, &mut app, &clock, size, 0., pitch, neutral());
                }
                deliver(&mut app, &tx, &bytes);
                let (mut proof, pixels) =
                    frame_at_pitch(&mut h, &mut app, &clock, size, 0., pitch, neutral());
                let surface = app.flight_view.computer_surface();
                let buttons=app.computer_ui_labels.iter().filter(|(label,_)|matches!(label.as_str(),"AUTO"|"OPEN"|"STOWED")).map(|(label,point)|{
                    let projected=surface.as_ref().and_then(|surface|surface.project(*point));
                    serde_json::json!({"label":label,"canvas_point":[point.x,point.y],"projected":projected.map(|p|[p.x,p.y]),"actual_hit":projected.is_some_and(|p|app.flight_view.computer_hit(&h.ctx,p).is_some())})
                }).collect::<Vec<_>>();
                assert_eq!(
                    buttons.len(),
                    6,
                    "Both captain control groups remain visible"
                );
                assert!(
                    buttons.iter().all(|button| button["actual_hit"] == true),
                    "Every actual captain button centre must pass scene depth picking"
                );
                proof["captain_button_centres"] = serde_json::json!(buttons);
                captures.push(save(
                    &directory,
                    &format!("{size_label}-captain-{view}"),
                    size,
                    proof,
                    &pixels,
                ));
            }
            // Open the existing readable desktop workspace via its actual glass
            // button, then return through the real Escape route without orders.
            let point = app
                .computer_ui_labels
                .iter()
                .find(|(label, _)| label == "Expand panel")
                .unwrap()
                .1;
            let point = app
                .flight_view
                .computer_surface()
                .unwrap()
                .project(point)
                .unwrap();
            assert!(app.flight_view.computer_hit(&h.ctx, point).is_some());
            for pressed in [true, false] {
                frame_at_pitch(
                    &mut h,
                    &mut app,
                    &clock,
                    size,
                    0.,
                    -0.2,
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
            assert!(
                app.services_open,
                "Actual projected Expand panel click must open readable workspace"
            );
            for _ in 0..3 {
                frame_at_pitch(&mut h, &mut app, &clock, size, 0., -0.2, neutral());
            }
            let (proof, pixels) =
                frame_at_pitch(&mut h, &mut app, &clock, size, 0., -0.2, neutral());
            captures.push(save(
                &directory,
                &format!("{size_label}-expanded-controls"),
                size,
                proof,
                &pixels,
            ));
            let scroll_point = egui::pos2(size[0] as f32 - 180., size[1] as f32 - 270.);
            for _ in 0..4 {
                frame_at_pitch(
                    &mut h,
                    &mut app,
                    &clock,
                    size,
                    0.,
                    -0.2,
                    vec![
                        egui::Event::PointerMoved(scroll_point),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0., -120.),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            for _ in 0..3 {
                frame_at_pitch(&mut h, &mut app, &clock, size, 0., -0.2, neutral());
            }
            let (proof, pixels) =
                frame_at_pitch(&mut h, &mut app, &clock, size, 0., -0.2, neutral());
            captures.push(save(
                &directory,
                &format!("{size_label}-expanded-controls-scrolled"),
                size,
                proof,
                &pixels,
            ));
            for pressed in [true, false] {
                frame_at_pitch(
                    &mut h,
                    &mut app,
                    &clock,
                    size,
                    0.,
                    -0.2,
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
            assert!(
                !app.services_open,
                "Escape must restore cockpit without issuing orders"
            );
            assert!(
                requests.try_recv().is_err(),
                "Read-only headturn/status audit cannot issue gameplay"
            );
        }
        std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
            "source":std::env::var("ARKLINE_SEATED_AUDIT_SOURCE").unwrap_or_default(),
            "input_sha256":hex::encode(Sha256::digest(&bytes)),"captures":captures,
            "scope":"Read-only controlled replay;960x640 embedded,900x650 desktop minimum,800x650 below-min stress. Canonical eye/FOV unchanged. Headturn cases target individual bays; default status capture retained before supported collapse.",
            "timing_limit":"Measured llvmpipe frame wall time includes App/egui/GPU finish/readback. Not desktop FPS or network freshness benchmark."
        })).unwrap()).unwrap();
    }
}

#[path = "seated_frame_profile_tests.rs"]
mod seated_frame_profile_tests;
