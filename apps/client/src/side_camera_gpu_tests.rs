//! Actual revision-10 authority Responses replayed through App and EGL, not a live session.
use super::*;
use arkline_protocol::Response;
use std::sync::mpsc;
#[test]
#[ignore = "Explicit actual nine-screen authority response replay and EGL captures"]
fn native_nine_screens_and_exterior_hull_views() {
    unsafe {
        let directory =
            std::path::PathBuf::from(std::env::var_os("ARKLINE_SIDE_CAMERA_EVIDENCE").unwrap());
        let source =
            std::path::PathBuf::from(std::env::var_os("ARKLINE_SIDE_CAMERA_RESPONSES").unwrap());
        std::fs::create_dir(&directory).unwrap();
        let (_context, gl) = PrivateContext::create();
        let mut h = Harness::new(gl.clone());
        let profile = directory.join("fresh-registration-10-profile");
        std::fs::create_dir(&profile).unwrap();
        let mut app = crate::tests::view_test_cockpit_with_profile(&h.ctx, profile);
        app.flight_view = FlightView::new(Some(&gl));
        app.hangar_view = HangarView::new(Some(&gl));
        app.drone_camera = false;
        let (tx, rx) = mpsc::channel();
        app.updates = Some(rx);
        tx.send(crate::Update::Ready).unwrap();
        app.poll();
        let (commands, _requests) = mpsc::channel();
        app.commands = Some(commands);
        let clock = std::time::Instant::now();
        let mut receipts = Vec::new();
        let mut inputs = Vec::new();
        let mut deployed_bays = std::collections::BTreeMap::new();
        for (name, file) in [
            ("deployed", "01-auto-mining-response.json"),
            ("manual-four-plus-nine", "01-auto-mining-response.json"),
            ("manual-stowed", "stow-12-response.json"),
            ("manual-open", "open-12-response.json"),
            ("captain-port", "open-12-response.json"),
            ("captain-starboard", "open-12-response.json"),
            ("warp", "warp-12-response.json"),
            ("arrival", "08-normal-reopen-response.json"),
            (
                "arrival-open",
                "09-normal-external-auto-deployed-response.json",
            ),
            ("onfoot", "routine-standing-response.json"),
        ] {
            app.cockpit_console_detail = name.starts_with("captain-");
            app.service_tab = if name.starts_with("captain-") {
                crate::ServiceTab::Displays
            } else {
                crate::ServiceTab::Navigation
            };
            app.captain_screen_slot = if name == "captain-starboard" { 8 } else { 7 };
            app.drone_camera = name == "manual-four-plus-nine";
            app.drone_split = app.drone_camera;
            app.drone_camera_mode = DroneCameraMode::Raised;
            let input_path = if name == "onfoot" {
                tx.send(crate::Update::Ready).unwrap();
                app.poll();
                app.hangar_view.on_foot_view = crate::flight3d::hangar::OnFootView::FirstPerson;
                std::path::PathBuf::from(
                    std::env::var_os("ARKLINE_SIDE_CAMERA_ONFOOT_RESPONSES").unwrap(),
                )
                .join(file)
            } else if name == "arrival-open" {
                std::path::PathBuf::from(
                    std::env::var_os("ARKLINE_SIDE_CAMERA_ARRIVAL_RESPONSES").unwrap(),
                )
                .join(file)
            } else {
                source.join(file)
            };
            let bytes = std::fs::read(&input_path).unwrap();
            inputs.push(
                serde_json::json!({"file":input_path,"sha256":hex::encode(Sha256::digest(&bytes))}),
            );
            for (label, yaw, pitch) in [
                ("front", 0., 0.),
                ("port", 0.65, 0.13),
                ("starboard", -0.65, 0.13),
                ("metrics-port", 0.65, 0.30),
                ("metrics-starboard", -0.65, 0.30),
                ("cargo", -0.70, -0.13),
                ("reactor", -1.85, 0.),
                ("side-window-port", std::f32::consts::FRAC_PI_2, 0.),
                ("side-window-starboard", -std::f32::consts::FRAC_PI_2, 0.),
            ] {
                if (label.starts_with("metrics-")
                    || label.starts_with("side-window-")
                    || label == "cargo"
                    || label == "reactor")
                    && name != "deployed"
                    && name != "manual-stowed"
                {
                    continue;
                }
                if name.starts_with("captain-") && label != "front" {
                    continue;
                }
                let mut last = None;
                for i in 0..18 {
                    let response: Response = serde_json::from_slice(&bytes).unwrap();
                    assert!(response.ok);
                    let snapshot = response.snapshot.as_ref().unwrap();
                    app.pilot = snapshot.pilots[0].name.clone();
                    let ms = snapshot.server_time_ms;
                    tx.send(crate::Update::Response(Box::new(response), false))
                        .unwrap();
                    app.poll();
                    app.frozen_time = Some(ms);
                    let events = if name == "deployed" && label == "front" && i < 2 {
                        vec![egui::Event::Key {
                            key: egui::Key::H,
                            physical_key: None,
                            pressed: i == 0,
                            repeat: false,
                            modifiers: egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                        }]
                    } else {
                        vec![]
                    };
                    last = Some(physical_cockpit_tests::physical_frame_at_pitch(
                        &mut h,
                        &mut app,
                        clock.elapsed().as_millis() as u64,
                        yaw,
                        pitch,
                        events,
                    ));
                    if name == "deployed" && label == "front" && i == 2 {
                        captain_displays_tests::toggle_interaction(&mut h, &mut app, &clock, false);
                    }
                }
                let (proof, pixels) = last.unwrap();
                eprintln!(
                    "{name}-{label} feeds={} cache={}",
                    proof["feeds"].as_array().unwrap().len(),
                    proof["cache"].as_array().unwrap().len()
                );
                if name == "manual-stowed" || name == "warp" {
                    assert!(proof["feeds"].as_array().unwrap().is_empty());
                }
                if name == "manual-four-plus-nine" && label == "starboard" {
                    assert_eq!(proof["cache"].as_array().unwrap().len(), 13);
                }
                for feed in proof["feeds"].as_array().unwrap() {
                    let key = feed["key"].as_u64().unwrap();
                    if name == "deployed" {
                        let bay = feed["bay"].as_u64().unwrap();
                        let identity = (
                            feed["source_key"].clone(),
                            feed["drone"].clone(),
                            feed["turret"].clone(),
                            feed["exterior"].clone(),
                        );
                        if let Some(old) = deployed_bays.insert(bay, identity.clone()) {
                            assert_eq!(old, identity, "Head turn cannot change hardware source");
                        }
                    }
                    assert!(
                        proof["cache"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|c| c["key"].as_u64() == Some(key)
                                && c["captured_ui_ms"].is_number()),
                        "Visible feed {key} must have its own actual capture"
                    );
                }
                receipts.push(physical_cockpit_tests::save(
                    &directory,
                    &format!("{name}-{label}"),
                    proof,
                    &pixels,
                ));
                std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"inputs":inputs,"captures":receipts,"note":"Actual App/EGL rendering from exported real authority Responses. Controlled replay clock, not live session or real-time performance proof."})).unwrap()).unwrap();
            }
        }
        assert_eq!(
            deployed_bays.keys().copied().collect::<Vec<_>>(),
            vec![8, 9, 10, 11, 12, 13, 14, 21, 22]
        );
    }
}
