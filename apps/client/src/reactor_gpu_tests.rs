//! Real registered room and actual App overview from unmodified authority Responses.
use super::*;

fn response(path: &Path) -> arkline_protocol::Response {
    let response: arkline_protocol::Response =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(response.ok);
    assert!(response.snapshot.as_ref().unwrap().pilots[0]
        .reactor_configuration
        .is_some());
    response
}

unsafe fn room_frame(
    h: &mut Harness,
    response: &arkline_protocol::Response,
    ui_ms: u64,
    oblique: bool,
) -> (serde_json::Value, Vec<u8>) {
    let snapshot = response.snapshot.as_ref().unwrap();
    let pilot = &snapshot.pilots[0];
    let now = snapshot.server_time_ms;
    let before = serde_json::to_vec(response).unwrap();
    let expected = pilot
        .reactor_configuration
        .as_ref()
        .unwrap()
        .fraction_at(now);
    h.view.set_room_layout(
        response
            .assets
            .as_ref()
            .and_then(|a| a.pioneer_room_layout.clone()),
    );
    h.view.set_space_snapshot(Some(Arc::new(snapshot.clone())));
    h.view.observe_mechanisms(snapshot, &pilot.name, true, now);
    let pose = PioneerDockedPose::private();
    let body_eye = if oblique {
        [-3.0, -2.0, 3.6]
    } else {
        [-5.55, -2.82, 3.4]
    };
    let eye = vector(body_to_station(pose, body_eye));
    let direction = (vector(body_to_station(
        pose,
        arkline_protocol::REACTOR_CONFIGURATION_CENTER,
    )) - eye)
        .normalize();
    h.view.set_editor_free_camera(
        true,
        Some((eye, direction.z.atan2(direction.x), direction.y.asin())),
    );
    let output = h.ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(WIDTH as f32, HEIGHT as f32),
            )),
            time: Some(ui_ms as f64 / 1000.),
            focused: true,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                h.view.ui(
                    ui,
                    pilot,
                    now,
                    true,
                    None,
                    response
                        .assets
                        .as_ref()
                        .and_then(|a| a.pioneer_room.as_ref()),
                    snapshot.exterior.as_ref(),
                    now,
                    None,
                );
                ui.painter().text(
                    egui::pos2(20., 20.),
                    egui::Align2::LEFT_TOP,
                    format!(
                        "D1-S1 / DIAGNOSTIC INSPECTION CAMERA / {} / {:.0}% COMPACT",
                        h.view.reactor_pose.status(),
                        expected * 100.
                    ),
                    egui::FontId::monospace(16.),
                    egui::Color32::WHITE,
                );
                ui.painter().text(
                    egui::pos2(20., HEIGHT as f32 - 22.),
                    egui::Align2::LEFT_BOTTOM,
                    "ACTUAL REGISTERED GEOMETRY · REAL REQUEST SNAPSHOT · NOT A PLAYER WALK ROUTE",
                    egui::FontId::monospace(13.),
                    egui::Color32::LIGHT_GRAY,
                );
            });
        },
    );
    assert_eq!(h.view.reactor_pose.fraction(), Some(expected));
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
    assert_eq!(h.gl.get_error(), glow::NO_ERROR);
    let mut pixels = vec![0; WIDTH * HEIGHT * 4];
    h.gl.read_pixels(
        0,
        0,
        WIDTH as i32,
        HEIGHT as i32,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelPackData::Slice(Some(&mut pixels)),
    );
    assert_eq!(
        serde_json::to_vec(response).unwrap(),
        before,
        "Inspection cannot edit the authority response"
    );
    (
        serde_json::json!({"snapshot_ms":now,"fraction":expected,"state":pilot.reactor_configuration,
        "eye_body_m":body_eye,"camera":"diagnostic registered interior, not player traversal"}),
        pixels
            .chunks_exact(WIDTH * 4)
            .rev()
            .flatten()
            .copied()
            .collect(),
    )
}

#[test]
#[ignore = "Explicit reactor real-Request room and actual App overview proof in private EGL/CGL"]
fn native_reactor_configuration_room_and_cockpit() {
    unsafe {
        let directory = std::path::PathBuf::from(
            std::env::var_os("ARKLINE_REACTOR_EVIDENCE").expect("Fresh proof directory"),
        );
        let source = std::path::PathBuf::from(
            std::env::var_os("ARKLINE_REACTOR_RESPONSES").expect("Real authority Response exports"),
        );
        std::fs::create_dir(&directory).unwrap();
        let (_context, gl) = PrivateContext::create();
        let mut h = Harness::new(gl.clone());
        let profile = directory.join("profile-reactor-registration-7");
        std::fs::create_dir(&profile).unwrap();
        let mut app = crate::tests::view_test_cockpit_with_profile(&h.ctx, profile);
        app.flight_view = FlightView::new(Some(&gl));
        app.drone_camera = false;
        let clock = std::time::Instant::now();
        let mut captures = Vec::new();
        for phase in ["compress", "open"] {
            for i in 0..=12 {
                let file = format!("{phase}-{i:02}-response.json");
                let response = response(&source.join(&file));
                let (mut proof, pixels) =
                    room_frame(&mut h, &response, clock.elapsed().as_millis() as u64, false);
                proof["source"] = file.into();
                captures.push(physical_cockpit_tests::save(
                    &directory,
                    &format!("room-{phase}-{i:02}"),
                    proof,
                    &pixels,
                ));
                if [0, 6, 12].contains(&i) {
                    let (proof, pixels) =
                        room_frame(&mut h, &response, clock.elapsed().as_millis() as u64, true);
                    captures.push(physical_cockpit_tests::save(
                        &directory,
                        &format!("room-oblique-{phase}-{i:02}"),
                        proof,
                        &pixels,
                    ));
                    let snapshot = response.snapshot.clone().unwrap();
                    let expected = snapshot.pilots[0]
                        .reactor_configuration
                        .as_ref()
                        .unwrap()
                        .fraction_at(snapshot.server_time_ms);
                    app.assets = response.assets.clone();
                    app.pilot = snapshot.pilots[0].name.clone();
                    app.frozen_time = Some(snapshot.server_time_ms);
                    app.timeline_origin = Some(snapshot.server_time_ms);
                    app.observed = std::time::Instant::now();
                    app.motion.observe(&snapshot);
                    app.snapshot = Some(snapshot);
                    if !app.identity_collapsed {
                        let modifiers = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;
                        for pressed in [true, false] {
                            physical_cockpit_tests::physical_frame(
                                &mut h,
                                &mut app,
                                clock.elapsed().as_millis() as u64,
                                -1.85,
                                vec![egui::Event::Key {
                                    key: egui::Key::H,
                                    physical_key: None,
                                    pressed,
                                    repeat: false,
                                    modifiers,
                                }],
                            );
                        }
                        assert!(app.identity_collapsed);
                    }
                    h.ctx.data_mut(|d| {
                        d.remove::<serde_json::Value>(egui::Id::new("reactor_display_proof"))
                    });
                    let (mut proof, pixels) = physical_cockpit_tests::physical_frame(
                        &mut h,
                        &mut app,
                        clock.elapsed().as_millis() as u64,
                        -1.85,
                        vec![],
                    );
                    assert_eq!(app.flight_view.reactor_pose.fraction(), Some(expected));
                    let display = h
                        .ctx
                        .data(|d| {
                            d.get_temp::<serde_json::Value>(egui::Id::new("reactor_display_proof"))
                        })
                        .expect("Actual permanent reactor display must paint");
                    assert_eq!(
                        display["fraction"].as_f64(),
                        Some(expected),
                        "Schematic and world must share the accepted pose"
                    );
                    proof["reactor_display"] = display;
                    proof["reactor_fraction"] = expected.into();
                    captures.push(physical_cockpit_tests::save(
                        &directory,
                        &format!("cockpit-{phase}-{i:02}"),
                        proof,
                        &pixels,
                    ));
                }
            }
        }
        std::fs::write(directory.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "scope":"Real accepted server Responses; unchanged authority; registered interior diagnostic camera plus actual Cockpit::update permanent reactor schematic",
            "source":source,"registration":arkline_protocol::PIONEER_DOCKED_REGISTRATION_REVISION,"captures":captures,
            "limits":["Rigid four-collar mechanism only","Heat, fuel, motor power and mass are unmodeled","No new camera feed or capture budget change","Room inspection is not a player traversal journey"]
        })).unwrap()).unwrap();
    }
}
