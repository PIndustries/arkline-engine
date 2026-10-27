//! Replays unchanged real Request responses through actual App/GL callbacks.
use super::*;
use arkline_protocol::{CockpitScreenMode, CockpitScreenTarget, Response};
use std::path::PathBuf;

fn neutral_pointer() -> Vec<egui::Event> {
    vec![egui::Event::PointerMoved(egui::pos2(1100., 210.))]
}

fn install(app: &mut crate::Cockpit, source: &Path, name: &str) -> serde_json::Value {
    let bytes = std::fs::read(source.join(name)).unwrap();
    let response: Response = serde_json::from_slice(&bytes).unwrap();
    assert!(response.ok);
    let snapshot = response.snapshot.unwrap();
    if app
        .snapshot
        .as_ref()
        .is_some_and(|old| snapshot.server_time_ms < old.server_time_ms)
    {
        app.flight_view.reset_activity_session();
        app.hangar_view.reset_mechanisms();
        app.motion = Default::default();
    }
    app.assets = response.assets;
    app.pilot = snapshot.pilots[0].name.clone();
    app.frozen_time = Some(snapshot.server_time_ms);
    app.timeline_origin = Some(snapshot.server_time_ms);
    app.observed = std::time::Instant::now();
    app.motion.observe(&snapshot);
    app.snapshot = Some(snapshot);
    app.request_pending = false;
    serde_json::json!({"file":name,"sha256":hex::encode(Sha256::digest(bytes))})
}
fn projected_button(app: &crate::Cockpit, ctx: &egui::Context, label: &str) -> egui::Pos2 {
    let point = app
        .computer_ui_labels
        .iter()
        .find(|(text, _)| text == label)
        .unwrap_or_else(|| panic!("Missing {label}: {:?}", app.computer_ui_labels))
        .1;
    let onfoot = app.snapshot.as_ref().unwrap().pilots[0]
        .station_avatar
        .is_some();
    let surface = if onfoot {
        app.hangar_view.computer_surface()
    } else {
        app.flight_view.computer_surface()
    }
    .unwrap();
    let point = surface.project(point).unwrap();
    let hit = if onfoot {
        app.hangar_view.computer_hit_tester(ctx)(point)
    } else {
        app.flight_view.computer_hit(ctx, point)
    };
    assert!(hit.is_some());
    point
}

pub(super) unsafe fn toggle_interaction(
    h: &mut Harness,
    app: &mut crate::Cockpit,
    clock: &std::time::Instant,
    expected_open: bool,
) {
    let rect = h
        .ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("ship_interaction_window_rect")))
        .unwrap();
    let bounds = h
        .ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("ship_interaction_bounds")))
        .unwrap();
    assert!(
        rect.bottom() <= bounds.bottom() + 0.1,
        "Interaction access must stay above the footer"
    );
    let toggle = h
        .ctx
        .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("ship_interaction_toggle_rect")));
    let point = if let Some(toggle) = toggle {
        assert!(
            rect.contains_rect(toggle) && bounds.contains_rect(toggle),
            "The actual reserved-strip header must be fully accessible"
        );
        toggle.center()
    } else {
        // Retained floating-window titlebar path (e.g. third-person on foot).
        rect.min + egui::vec2(20., 20.)
    };
    for pressed in [true, false] {
        physical_cockpit_tests::physical_frame(
            h,
            app,
            clock.elapsed().as_millis() as u64,
            0.,
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
    let state = egui::collapsing_header::CollapsingState::load(
        &h.ctx,
        egui::Id::new("docked_traversal_actions").with("collapsing"),
    )
    .unwrap();
    assert_eq!(
        state.is_open(),
        expected_open,
        "Actual interaction header pointer must toggle the same saved access state"
    );
    for _ in 0..2 {
        physical_cockpit_tests::physical_frame(
            h,
            app,
            clock.elapsed().as_millis() as u64,
            0.,
            neutral_pointer(),
        );
    }
}

#[test]
#[ignore = "Explicit real captain Request replay, actual physical computer pointer and GL proof"]
fn native_captain_display_controls_and_authority_replay() {
    unsafe {
        let directory = PathBuf::from(
            std::env::var_os("ARKLINE_CAPTAIN_EVIDENCE").expect("Fresh output directory"),
        );
        let source = PathBuf::from(
            std::env::var_os("ARKLINE_CAPTAIN_RESPONSES").expect("Real Response export"),
        );
        let onfoot = PathBuf::from(
            std::env::var_os("ARKLINE_CAPTAIN_ONFOOT_RESPONSES")
                .expect("Real owned on-foot Response export"),
        );
        std::fs::create_dir(&directory).unwrap();
        let (_context, gl) = PrivateContext::create();
        let mut h = Harness::new(gl.clone());
        let profile = directory.join("profile-captain-registration-9");
        std::fs::create_dir(&profile).unwrap();
        let mut app = crate::tests::view_test_cockpit_with_profile(&h.ctx, profile);
        app.flight_view = FlightView::new(Some(&gl));
        app.hangar_view = HangarView::new(Some(&gl));
        app.hangar_view.on_foot_view = crate::flight3d::hangar::OnFootView::FirstPerson;
        app.drone_camera = false;
        app.service_tab = crate::ServiceTab::Displays;
        app.cockpit_console_detail = true;
        let (tx, rx) = std::sync::mpsc::channel();
        app.commands = Some(tx);
        let clock = std::time::Instant::now();
        let mut captures = Vec::new();
        let mut samples = Vec::new();
        let mut inputs = Vec::new();
        let first = install(&mut app, &source, "01-auto-mining-response.json");
        // Supported user keyboard toggle; footer keeps authority/freshness visible.
        for pressed in [true, false] {
            physical_cockpit_tests::physical_frame(
                &mut h,
                &mut app,
                clock.elapsed().as_millis() as u64,
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
        for _ in 0..9 {
            let (proof, _) = physical_cockpit_tests::physical_frame(
                &mut h,
                &mut app,
                clock.elapsed().as_millis() as u64,
                0.,
                neutral_pointer(),
            );
            samples.push(proof);
        }
        let (proof, pixels) = physical_cockpit_tests::physical_frame(
            &mut h,
            &mut app,
            clock.elapsed().as_millis() as u64,
            0.,
            neutral_pointer(),
        );
        captures.push(physical_cockpit_tests::save(
            &directory,
            "default-open-interaction",
            proof,
            &pixels,
        ));
        assert!(egui::collapsing_header::CollapsingState::load(
            &h.ctx,
            egui::Id::new("docked_traversal_actions").with("collapsing")
        )
        .unwrap()
        .is_open());
        toggle_interaction(&mut h, &mut app, &clock, false);
        assert!(
            rx.try_recv().is_err(),
            "Interaction collapse must not leak a cockpit command"
        );
        let (mut proof, pixels) = physical_cockpit_tests::physical_frame(
            &mut h,
            &mut app,
            clock.elapsed().as_millis() as u64,
            0.,
            neutral_pointer(),
        );
        physical_cockpit_tests::save(&directory, "initial-diagnostic", proof.clone(), &pixels);
        assert_eq!(proof["feeds"].as_array().unwrap().len(), 7);
        for label in ["AUTO", "OPEN", "STOWED"] {
            assert_eq!(
                app.computer_ui_labels
                    .iter()
                    .filter(|(s, _)| s == label)
                    .count(),
                2,
                "Large ALL and selected-bay actions must remain visible in the canonical physical canvas: {:?}",
                app.computer_ui_labels
            );
        }
        proof["response"] = first;
        proof["computer_labels"] = serde_json::json!(app
            .computer_ui_labels
            .iter()
            .map(|(s, p)| (s, [p.x, p.y]))
            .collect::<Vec<_>>());
        captures.push(physical_cockpit_tests::save(
            &directory,
            "auto-mining-displays",
            proof,
            &pixels,
        ));
        for (label, mode, response) in [
            (
                "STOWED",
                CockpitScreenMode::Stowed,
                "04-all-stowed-response.json",
            ),
            ("OPEN", CockpitScreenMode::Open, "05-all-open-response.json"),
        ] {
            let point = projected_button(&app, &h.ctx, label);
            app.ai_pin = Some((app.pilot.clone(), app.connection_generation));
            let before = app.ai_pin.clone();
            for pressed in [true, false] {
                physical_cockpit_tests::physical_frame(
                    &mut h,
                    &mut app,
                    clock.elapsed().as_millis() as u64,
                    0.,
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
            let request = rx
                .try_recv()
                .expect("Actual physical glass must emit monitor-only request");
            let state = app.snapshot.as_ref().unwrap().pilots[0]
                .cockpit_mechanisms
                .as_ref()
                .unwrap();
            assert_eq!(
                serde_json::to_value(&request).unwrap(),
                serde_json::to_value(arkline_protocol::Request::SetCockpitScreenMode {
                    vessel_id: state.vessel_id.clone(),
                    target: CockpitScreenTarget::All {
                        expected_assignment_generations: state.assignment_generations
                    },
                    mode
                })
                .unwrap()
            );
            assert!(rx.try_recv().is_err());
            assert_eq!(app.ai_pin, before);
            inputs.push(serde_json::to_value(&request).unwrap());
            install(&mut app, &source, response);
            for _ in 0..2 {
                physical_cockpit_tests::physical_frame(
                    &mut h,
                    &mut app,
                    clock.elapsed().as_millis() as u64,
                    0.,
                    neutral_pointer(),
                );
            }
        }
        // Real responses follow authority time; UI/capture age uses elapsed wall time.
        for (phase, expected_end) in [("stow", 0.), ("open", 1.), ("warp", 0.)] {
            for i in 0..=12 {
                let receipt = install(&mut app, &source, &format!("{phase}-{i:02}-response.json"));
                let (mut proof, pixels) = physical_cockpit_tests::physical_frame(
                    &mut h,
                    &mut app,
                    clock.elapsed().as_millis() as u64,
                    0.,
                    neutral_pointer(),
                );
                proof["response"] = receipt;
                samples.push(proof.clone());
                if [0, 6, 12].contains(&i) {
                    captures.push(physical_cockpit_tests::save(
                        &directory,
                        &format!("{phase}-{i:02}-displays"),
                        proof.clone(),
                        &pixels,
                    ));
                }
                if i == 12 {
                    assert!(proof["fractions"].as_array().unwrap().iter().all(|f| (f
                        .as_f64()
                        .unwrap()
                        - expected_end)
                        .abs()
                        < 1e-6));
                    if phase != "open" {
                        assert!(proof["feeds"].as_array().unwrap().is_empty());
                    }
                }
            }
            if phase == "stow" {
                let (proof, pixels) = physical_cockpit_tests::physical_frame(
                    &mut h,
                    &mut app,
                    clock.elapsed().as_millis() as u64,
                    -0.4,
                    neutral_pointer(),
                );
                captures.push(physical_cockpit_tests::save(
                    &directory,
                    "stowed-wing-fixed-telemetry",
                    proof,
                    &pixels,
                ));
                let (mut proof, pixels) = physical_cockpit_tests::physical_frame(
                    &mut h,
                    &mut app,
                    clock.elapsed().as_millis() as u64,
                    -1.85,
                    neutral_pointer(),
                );
                let display = h
                    .ctx
                    .data(|d| {
                        d.get_temp::<serde_json::Value>(egui::Id::new("reactor_display_proof"))
                    })
                    .expect(
                        "Permanent reactor display stays available while camera hardware stows",
                    );
                proof["reactor_display"] = display;
                captures.push(physical_cockpit_tests::save(
                    &directory,
                    "stowed-permanent-reactor",
                    proof,
                    &pixels,
                ));
            }
            if phase == "open" {
                for _ in 0..10 {
                    let (proof, _) = physical_cockpit_tests::physical_frame(
                        &mut h,
                        &mut app,
                        clock.elapsed().as_millis() as u64,
                        0.,
                        neutral_pointer(),
                    );
                    samples.push(proof);
                }
                let (proof, pixels) = physical_cockpit_tests::physical_frame(
                    &mut h,
                    &mut app,
                    clock.elapsed().as_millis() as u64,
                    0.,
                    neutral_pointer(),
                );
                let now = app.snapshot.as_ref().unwrap().server_time_ms;
                for feed in proof["feeds"].as_array().unwrap() {
                    assert!(
                        proof["cache"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|c| c["key"] == feed["key"] && c["painted_sim_ms"] == now),
                        "Every reopened view must actually refresh"
                    );
                }
                assert_eq!(proof["feeds"].as_array().unwrap().len(), 7);
                captures.push(physical_cockpit_tests::save(
                    &directory,
                    "reopened-seven-refreshed",
                    proof,
                    &pixels,
                ));
            }
        }
        let receipt = install(&mut app, &onfoot, "routine-standing-response.json");
        assert!(app.verified_docked_binding().is_some());
        for _ in 0..9 {
            physical_cockpit_tests::physical_frame(
                &mut h,
                &mut app,
                clock.elapsed().as_millis() as u64,
                0.,
                neutral_pointer(),
            );
        }
        let (mut proof, pixels) = physical_cockpit_tests::physical_frame(
            &mut h,
            &mut app,
            clock.elapsed().as_millis() as u64,
            0.,
            neutral_pointer(),
        );
        proof["response"] = receipt;
        captures.push(physical_cockpit_tests::save(
            &directory,
            "owned-onfoot-displays",
            proof,
            &pixels,
        ));
        toggle_interaction(&mut h, &mut app, &clock, true);
        assert!(
            rx.try_recv().is_err(),
            "Interaction reopen must not leak a cockpit command"
        );
        let (proof, pixels) = physical_cockpit_tests::physical_frame(
            &mut h,
            &mut app,
            clock.elapsed().as_millis() as u64,
            0.,
            neutral_pointer(),
        );
        captures.push(physical_cockpit_tests::save(
            &directory,
            "owned-onfoot-interaction-reopened",
            proof,
            &pixels,
        ));
        // Registered first-person access deliberately cannot be collapsed:
        // the sole visible F/action surface must survive a titlebar click.
        assert!(app.first_person_hud_active());
        toggle_interaction(&mut h, &mut app, &clock, true);
        let (prompt, clip) = h
            .ctx
            .data(|data| {
                data.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new(
                    "ship_interaction_primary_prompt_bounds",
                ))
            })
            .unwrap();
        assert!(
            clip.contains_rect(prompt),
            "The primary on-foot prompt must remain fully visible"
        );
        assert!(
            app.nearby_ship_action().is_some(),
            "Real standing fixture retains its reachable F action"
        );
        assert!(
            rx.try_recv().is_err(),
            "First-person header click must not leak a cockpit command"
        );
        app.services_open = true;
        for _ in 0..3 {
            physical_cockpit_tests::physical_frame(
                &mut h,
                &mut app,
                clock.elapsed().as_millis() as u64,
                0.,
                neutral_pointer(),
            );
        }
        let buttons = h
            .ctx
            .data(|d| {
                d.get_temp::<Vec<(String, egui::Rect)>>(egui::Id::new("captain_display_buttons"))
            })
            .unwrap();
        assert_eq!(
            buttons.len(),
            (arkline_protocol::COCKPIT_TOTAL_MECHANISM_SLOTS + 1) * 3,
            "Expanded destination retains every individual mechanism row plus ALL, with three modes each"
        );
        let (proof, pixels) = physical_cockpit_tests::physical_frame(
            &mut h,
            &mut app,
            clock.elapsed().as_millis() as u64,
            0.,
            neutral_pointer(),
        );
        captures.push(physical_cockpit_tests::save(
            &directory,
            "owned-onfoot-expanded-displays",
            proof,
            &pixels,
        ));
        for _ in 0..3 {
            physical_cockpit_tests::physical_frame(
                &mut h,
                &mut app,
                clock.elapsed().as_millis() as u64,
                0.,
                vec![
                    egui::Event::PointerMoved(egui::pos2(1100., 580.)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0., -220.),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        let lower_buttons = h
            .ctx
            .data(|d| {
                d.get_temp::<Vec<(String, egui::Rect)>>(egui::Id::new("captain_display_buttons"))
            })
            .unwrap();
        let last_slot_label = format!(
            "{}/STOWED",
            arkline_protocol::COCKPIT_TOTAL_MECHANISM_SLOTS - 1
        );
        let last_mechanism = lower_buttons
            .iter()
            .find(|(label, _)| label == &last_slot_label)
            .unwrap()
            .1;
        assert!(
            last_mechanism.center().y > 250. && last_mechanism.bottom() < 650.,
            "The final mechanism row must remain reachable by ordinary scroll: {last_mechanism:?}"
        );
        let (proof, pixels) = physical_cockpit_tests::physical_frame(
            &mut h,
            &mut app,
            clock.elapsed().as_millis() as u64,
            0.,
            neutral_pointer(),
        );
        captures.push(physical_cockpit_tests::save(
            &directory,
            "owned-onfoot-expanded-lower",
            proof,
            &pixels,
        ));
        for _ in 0..3 {
            physical_cockpit_tests::physical_frame(
                &mut h,
                &mut app,
                clock.elapsed().as_millis() as u64,
                0.,
                vec![
                    egui::Event::PointerMoved(egui::pos2(1100., 580.)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0., 400.),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        let buttons = h
            .ctx
            .data(|d| {
                d.get_temp::<Vec<(String, egui::Rect)>>(egui::Id::new("captain_display_buttons"))
            })
            .unwrap();
        let point = buttons
            .iter()
            .find(|(label, _)| label == "all/STOWED")
            .unwrap()
            .1
            .center();
        let unchanged = serde_json::to_value(&app.snapshot).unwrap();
        for pressed in [true, false] {
            physical_cockpit_tests::physical_frame(
                &mut h,
                &mut app,
                clock.elapsed().as_millis() as u64,
                0.,
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
        let request = rx
            .try_recv()
            .expect("Confirmed owner on foot may change monitor policy without piloting");
        assert!(matches!(
            request,
            arkline_protocol::Request::SetCockpitScreenMode {
                mode: CockpitScreenMode::Stowed,
                ..
            }
        ));
        assert!(rx.try_recv().is_err());
        assert_eq!(serde_json::to_value(&app.snapshot).unwrap(), unchanged);
        inputs.push(serde_json::to_value(&request).unwrap());
        std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
            "scope":"Actual App/GL and physical glass input; replay of unchanged real server Request Responses, not live network or desktop FPS",
            "registration":arkline_protocol::PIONEER_DOCKED_REGISTRATION_REVISION,"source":source,"onfoot_source":onfoot,
            "inputs":inputs,"captures":captures,"frame_samples":samples,
            "limits":["Authority snapshots are held/delivered by controlled fixture; wall-clock UI time measures capture ages", "Mode pointer emits exact validated request; exported server response journey independently verifies equipment/job preservation", "Warp itself retains existing job cancellation and drone invalidation", "No camera cache, material or lighting changes"]
        })).unwrap()).unwrap();
    }
}
