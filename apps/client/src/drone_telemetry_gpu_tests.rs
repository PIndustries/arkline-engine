//! Actual App/GL proof from unchanged exported real Request Responses.
use super::*;
use crate::drone_telemetry::Availability;
use arkline_protocol::Response;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};

fn deliver(app: &mut crate::Cockpit, tx: &Sender<crate::Update>, bytes: &[u8]) {
    let response: Response = serde_json::from_slice(bytes).unwrap();
    assert!(response.ok);
    let snapshot = response.snapshot.as_ref().unwrap();
    app.pilot = snapshot.pilots[0].name.clone();
    let server_ms = snapshot.server_time_ms;
    tx.send(crate::Update::Response(Box::new(response), false))
        .unwrap();
    app.poll();
    // Controlled replay clock. Source payload, cycle and drone pose are unchanged.
    app.frozen_time = Some(server_ms);
    assert!(app.performance_snapshot_received.is_some());
}
unsafe fn frame(
    h: &mut Harness,
    app: &mut crate::Cockpit,
    clock: &std::time::Instant,
    pitch: f32,
    events: Vec<egui::Event>,
) -> (serde_json::Value, Vec<u8>) {
    frame_at_yaw(h, app, clock, 0., pitch, events)
}
unsafe fn frame_at_yaw(
    h: &mut Harness,
    app: &mut crate::Cockpit,
    clock: &std::time::Instant,
    yaw: f32,
    pitch: f32,
    events: Vec<egui::Event>,
) -> (serde_json::Value, Vec<u8>) {
    let (mut proof, pixels) = physical_cockpit_tests::physical_frame_at_pitch(
        h,
        app,
        clock.elapsed().as_millis() as u64,
        yaw,
        pitch,
        events,
    );
    proof["cassette_readouts"] = h
        .ctx
        .data(|d| d.get_temp::<serde_json::Value>(egui::Id::new("drone_cassette_readout_proof")))
        .unwrap_or_default();
    proof["availability"] = app.drone_telemetry_time.availability.label().into();
    proof["confirmed_ms"] = app.drone_telemetry_time.server_ms.into();
    proof["accepted_receipt_age_ms"] = (app
        .performance_snapshot_received
        .unwrap()
        .elapsed()
        .as_millis()
        .min(u64::MAX as u128) as u64)
        .into();
    (proof, pixels)
}
fn neutral() -> Vec<egui::Event> {
    vec![egui::Event::PointerMoved(egui::pos2(1100., 210.))]
}

#[test]
#[ignore = "Explicit unchanged real mining/Hold/Recall Response replay and actual GL pixels"]
fn native_drone_telemetry_real_responses_and_freshness() {
    unsafe {
        let directory = PathBuf::from(
            std::env::var_os("ARKLINE_DRONE_TELEMETRY_EVIDENCE").expect("Fresh proof output"),
        );
        let source = PathBuf::from(
            std::env::var_os("ARKLINE_DRONE_TELEMETRY_RESPONSES").expect("Real Request exports"),
        );
        std::fs::create_dir(&directory).unwrap();
        let (_context, gl) = PrivateContext::create();
        let mut h = Harness::new(gl.clone());
        let profile = directory.join("fresh-profile-registration-9");
        std::fs::create_dir(&profile).unwrap();
        let mut app = crate::tests::view_test_cockpit_with_profile(&h.ctx, profile);
        app.flight_view = FlightView::new(Some(&gl));
        app.hangar_view = HangarView::new(Some(&gl));
        app.drone_camera = false;
        app.selected_drone = Some(1);
        let (tx, rx) = mpsc::channel();
        app.updates = Some(rx);
        tx.send(crate::Update::Ready).unwrap();
        app.poll();
        let (commands, _requests) = mpsc::channel();
        app.commands = Some(commands);
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
            for _ in 0..8 {
                deliver(&mut app, &tx, &bytes);
                frame(&mut h, &mut app, &clock, 0., neutral());
            }
            deliver(&mut app, &tx, &bytes);
            let (proof, pixels) = frame(&mut h, &mut app, &clock, 0., neutral());
            assert_eq!(app.drone_telemetry_time.availability, Availability::Live);
            captures.push(physical_cockpit_tests::save(
                &directory,
                &format!("{name}-forward"),
                proof,
                &pixels,
            ));
            if name == "mining" {
                // Real supported header shortcut, not test-only overlay hiding.
                for pressed in [true, false] {
                    deliver(&mut app, &tx, &bytes);
                    frame(
                        &mut h,
                        &mut app,
                        &clock,
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
                assert!(app.identity_collapsed);
            }
            deliver(&mut app, &tx, &bytes);
            let yaw = if name == "mining" { 0. } else { 0.55 };
            let (proof, pixels) = frame_at_yaw(&mut h, &mut app, &clock, yaw, 0.20, neutral());
            let rows = proof["cassette_readouts"].as_array().unwrap();
            assert!(!rows.is_empty());
            let selected = rows.iter().find(|r| r["id"] == 1).unwrap();
            assert_eq!(selected["status"], "LIVE");
            if name == "mining" {
                assert!(selected["cycle_fraction"]
                    .as_f64()
                    .is_some_and(|p| p > 0. && p < 1.));
            } else {
                assert!(selected["cycle_fraction"].is_null());
            }
            if name == "hold" {
                assert!(selected["task"].as_str().unwrap().starts_with("HOLD"));
                assert!(
                    selected["server_ms"].as_u64().unwrap()
                        - selected["pose_updated_ms"].as_u64().unwrap()
                        >= 3000
                );
            }
            if name == "returning" {
                assert!(selected["task"].as_str().unwrap().starts_with("RETURNING"));
            }
            captures.push(physical_cockpit_tests::save(
                &directory,
                &format!("{name}-cassette-headturn"),
                proof,
                &pixels,
            ));
            if name == "mining" {
                for (direction, yaw) in [("port", 0.55), ("starboard", -0.55)] {
                    for _ in 0..7 {
                        deliver(&mut app, &tx, &bytes);
                        frame_at_yaw(&mut h, &mut app, &clock, yaw, 0.20, neutral());
                    }
                    deliver(&mut app, &tx, &bytes);
                    let (proof, pixels) =
                        frame_at_yaw(&mut h, &mut app, &clock, yaw, 0.20, neutral());
                    captures.push(physical_cockpit_tests::save(
                        &directory,
                        &format!("mining-cassettes-{direction}"),
                        proof,
                        &pixels,
                    ));
                }
            }
        }
        // Replay mining anew as a separate observation session, then deliberately
        // stop delivering. Actual elapsed receipt age—not render time—goes stale.
        let mining = std::fs::read(source.join("01-mining-response.json")).unwrap();
        tx.send(crate::Update::Ready).unwrap();
        app.poll();
        deliver(&mut app, &tx, &mining);
        let expected = crate::drone_telemetry::DroneReadout::new(
            &app.snapshot.as_ref().unwrap().pilots[0].drones[0],
            crate::drone_telemetry::TelemetryTime::from_observation(
                Some(app.snapshot.as_ref().unwrap().server_time_ms),
                Some(0),
                app.snapshot.as_ref().unwrap().server_time_ms,
                true,
                false,
            ),
        )
        .cycle
        .unwrap()
        .fraction;
        let receipt = app.performance_snapshot_received.unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1600));
        app.frozen_time = Some(
            app.snapshot.as_ref().unwrap().server_time_ms + receipt.elapsed().as_millis() as u64,
        );
        let (proof, pixels) = frame(&mut h, &mut app, &clock, 0.20, neutral());
        assert_eq!(app.performance_snapshot_received, Some(receipt));
        assert_eq!(app.drone_telemetry_time.availability, Availability::Stale);
        assert_eq!(
            crate::drone_telemetry::DroneReadout::new(
                &app.snapshot.as_ref().unwrap().pilots[0].drones[0],
                app.drone_telemetry_time
            )
            .cycle
            .unwrap()
            .fraction,
            expected
        );
        captures.push(physical_cockpit_tests::save(
            &directory,
            "mining-no-response-stale",
            proof,
            &pixels,
        ));
        // Intentional pause is a supported App state, not fabricated drone state.
        app.embedded = true;
        app.embedded_paused = true;
        app.frozen_time = Some(app.snapshot.as_ref().unwrap().server_time_ms);
        let (proof, pixels) = frame(&mut h, &mut app, &clock, 0.20, neutral());
        assert_eq!(app.drone_telemetry_time.availability, Availability::Paused);
        captures.push(physical_cockpit_tests::save(
            &directory,
            "mining-paused",
            proof,
            &pixels,
        ));
        tx.send(crate::Update::Reconnecting(
            "Controlled fixture disconnect".into(),
        ))
        .unwrap();
        app.poll();
        // Reconnect intentionally replaces the world with connection UI. Use the
        // App renderer that does not require a cockpit callback for this state;
        // never inherit a prior frame's physical surface evidence.
        let disconnected_receipt = app.performance_snapshot_received;
        let (proof, pixels) =
            drone_fullface_tests::frame(&mut h, &mut app, &clock, [WIDTH, HEIGHT], 0., neutral());
        assert!(proof["readouts"].is_null());
        assert_eq!(app.performance_snapshot_received, disconnected_receipt);
        assert_eq!(
            app.drone_telemetry_time.availability,
            Availability::Disconnected
        );
        captures.push(physical_cockpit_tests::save(
            &directory,
            "mining-disconnected",
            proof,
            &pixels,
        ));
        app.embedded = false;
        app.embedded_paused = false;
        tx.send(crate::Update::Ready).unwrap();
        app.poll();
        deliver(&mut app, &tx, &mining);
        app.service_tab = crate::ServiceTab::Drones;
        app.services_open = true;
        for _ in 0..3 {
            deliver(&mut app, &tx, &mining);
            frame(&mut h, &mut app, &clock, 0., neutral());
        }
        for step in 0..4 {
            deliver(&mut app, &tx, &mining);
            let events = if step == 0 {
                vec![
                    egui::Event::PointerMoved(egui::pos2(1180., 500.)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0., -430.),
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            } else {
                vec![]
            };
            let (proof, pixels) = frame(&mut h, &mut app, &clock, 0., events);
            if step == 3 {
                captures.push(physical_cockpit_tests::save(
                    &directory,
                    "mining-expanded-drone-details",
                    proof,
                    &pixels,
                ));
            }
        }
        for step in 0..4 {
            deliver(&mut app, &tx, &mining);
            let events = if step == 0 {
                vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0., -280.),
                    modifiers: egui::Modifiers::NONE,
                }]
            } else {
                vec![]
            };
            let (proof, pixels) = frame(&mut h, &mut app, &clock, 0., events);
            if step == 3 {
                captures.push(physical_cockpit_tests::save(
                    &directory,
                    "mining-expanded-heading-and-power",
                    proof,
                    &pixels,
                ));
            }
        }
        std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
            "scope":"Actual App/GL controlled replay; unchanged real Request exports, accepted through normal Update::Response/poll. Fresh profile; no live user world.",
            "inputs":inputs,"captures":captures,
            "freshness":"Only deliver() enqueues accepted Responses and refreshes receipt. Repeated source payloads warm renderer deterministically; no-response test waits real wall time. Controlled presentation clock remains at exported server time except stale case advances by actual receipt age.",
            "cycles":"Single authoritative interval, sampled at confirmed server time. No interpolation, next-cycle creation, completion or yield award.",
            "limitations":["EGL llvmpipe proof, not desktop FPS","Source Hold command cancels resource job under existing server policy; display does not change it","Yaw/pitch is heading only, no roll; power/thrust unmodeled"]
        })).unwrap()).unwrap();
    }
}
