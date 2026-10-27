//! Permanent cargo glass in the real App, using unchanged actual Request exports.
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
unsafe fn frame(
    h: &mut Harness,
    app: &mut crate::Cockpit,
    clock: &std::time::Instant,
    events: Vec<egui::Event>,
) -> (serde_json::Value, Vec<u8>) {
    app.flight_view.head_look = [-0.55, -0.08];
    app.hangar_view.head_look = [-0.55, -0.08];
    app.observed = std::time::Instant::now();
    let before = serde_json::to_vec(app.snapshot.as_ref().unwrap()).unwrap();
    let began = std::time::Instant::now();
    h.ctx.data_mut(|data| {
        data.remove::<serde_json::Value>(egui::Id::new("cargo_instrument_proof"));
    });
    let output = h.ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(WIDTH as f32, HEIGHT as f32),
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
    assert_eq!(
        serde_json::to_vec(app.snapshot.as_ref().unwrap()).unwrap(),
        before,
        "Cargo presentation cannot mutate authority"
    );
    let mut proof = h
        .ctx
        .data(|d| d.get_temp::<serde_json::Value>(egui::Id::new("cargo_instrument_proof")))
        .expect("Permanent real cargo surface must paint");
    proof["wall_ms"] = began.elapsed().as_secs_f64().mul_add(1000., 0.).into();
    proof["snapshot_sha256"] = hex::encode(Sha256::digest(&before)).into();
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
#[test]
#[ignore = "Explicit real mined/atomic-deposit cargo Response replay and actual fixed glass pixels"]
fn native_cargo_overview_real_empty_partial_full_and_stowed() {
    unsafe {
        let output = PathBuf::from(
            std::env::var_os("ARKLINE_CARGO_EVIDENCE").expect("Fresh output directory"),
        );
        let source = PathBuf::from(
            std::env::var_os("ARKLINE_CARGO_RESPONSES").expect("Real request export directory"),
        );
        std::fs::create_dir(&output).unwrap();
        let (_context, gl) = PrivateContext::create();
        let mut h = Harness::new(gl.clone());
        let profile = output.join("fresh-cargo-profile-registration-10");
        std::fs::create_dir(&profile).unwrap();
        let mut app = crate::tests::view_test_cockpit_with_profile(&h.ctx, profile);
        app.flight_view = FlightView::new(Some(&gl));
        app.hangar_view = HangarView::new(Some(&gl));
        app.hangar_view.on_foot_view = crate::flight3d::hangar::OnFootView::FirstPerson;
        app.drone_camera = false;
        let (tx, rx) = mpsc::channel();
        app.updates = Some(rx);
        let (commands, _requests) = mpsc::channel();
        app.commands = Some(commands);
        tx.send(crate::Update::Ready).unwrap();
        app.poll();
        let clock = std::time::Instant::now();
        let mut captures = Vec::new();
        let mut inputs = Vec::new();
        for name in [
            "01-empty",
            "02-partial-mining",
            "03-partial-all-stowed",
            "04-full",
            "05-deposited-empty",
        ] {
            let file = format!("{name}-response.json");
            let bytes = std::fs::read(source.join(&file)).unwrap();
            inputs.push(
                serde_json::json!({"file":file,"sha256":hex::encode(Sha256::digest(&bytes))}),
            );
            for i in 0..8 {
                deliver(&mut app, &tx, &bytes);
                let events = if name == "05-deposited-empty" && i < 2 {
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: i == 0,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }]
                } else if name == "01-empty" && i < 2 {
                    vec![egui::Event::Key {
                        key: egui::Key::H,
                        physical_key: None,
                        pressed: i == 0,
                        repeat: false,
                        modifiers: egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
                    }]
                } else {
                    vec![egui::Event::PointerMoved(egui::pos2(1100., 210.))]
                };
                frame(&mut h, &mut app, &clock, events);
            }
            assert!(app.identity_collapsed);
            deliver(&mut app, &tx, &bytes);
            let (proof, pixels) = frame(&mut h, &mut app, &clock, vec![]);
            let own = app.own().unwrap();
            let used = proof["used"].as_u64().unwrap();
            let capacity = arkline_protocol::cargo_capacity(own.ship_class);
            assert_eq!(used, own.ore);
            assert_eq!(proof["capacity"], capacity);
            assert_eq!(proof["free"], capacity.saturating_sub(used));
            assert_eq!(proof["availability"], "LIVE");
            assert_eq!(proof["surface"], 7);
            match name {
                "01-empty" | "05-deposited-empty" => assert_eq!(used, 0),
                "04-full" => assert_eq!(used, capacity),
                _ => assert!(used > 0 && used < capacity),
            }
            if name == "03-partial-all-stowed" {
                assert!(own
                    .cockpit_mechanisms
                    .as_ref()
                    .unwrap()
                    .all_fractions_at(app.snapshot.as_ref().unwrap().server_time_ms)
                    .iter()
                    .all(|f| *f == 0.));
            }
            if name == "05-deposited-empty" {
                assert!(
                    app.snapshot.as_ref().unwrap().stored_ore > 0,
                    "Personal storage remains distinct from empty onboard hold"
                );
            }
            captures.push(physical_cockpit_tests::save(&output, name, proof, &pixels));
        }
        std::fs::write(output.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"scope":"Actual App/EGL fixed surface7 cargo; unchanged real Request exports through Update::Response/poll. Fresh registration10 fixture only.","inputs":inputs,"captures":captures,"limitations":["Controlled replay: duplicate accepted Responses warm presentation, source state remains unchanged","Generic ore units, no cargo mass/volume/varieties","Job cumulative mined/stored totals are separate from current ship hold and personal station storage","Transfers atomic; no unloading timer, rate or progress inferred","No desktop FPS claim; nine moving screens integrated; this cargo view does not claim all nine inside one forward FOV"]})).unwrap()).unwrap();
    }
}
