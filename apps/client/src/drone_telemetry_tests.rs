//! App receipt/session regression. Synthetic inputs are deliberately not flight proof.
use super::*;
use crate::drone_telemetry::Availability;

fn frame(app: &mut Cockpit, ctx: &egui::Context) {
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(1280., 720.),
            )),
            time: Some(1.),
            focused: true,
            ..Default::default()
        },
        |ctx| eframe::App::update(app, ctx, &mut eframe::Frame::_new_kittest()),
    );
}
fn response(snapshot: Snapshot) -> Update {
    Update::Response(
        Box::new(Response {
            physical_cargo: None,
            auth: None,
            assets: None,
            ok: true,
            message: "Synthetic receipt test".into(),
            resume_token: None,
            snapshot: Some(snapshot),
            ledger: None,
        }),
        false,
    )
}
#[test]
fn only_accepted_responses_refresh_drone_receipt_and_ready_clears_context() {
    let ctx = egui::Context::default();
    let mut app = tests::view_test_cockpit(&ctx);
    let snapshot = app.snapshot.clone().unwrap();
    let (tx, rx) = mpsc::channel();
    app.updates = Some(rx);
    app.frozen_time = Some(snapshot.server_time_ms);
    frame(&mut app, &ctx);
    assert_eq!(
        app.drone_telemetry_time.availability,
        Availability::Unavailable
    );
    tx.send(response(snapshot.clone())).unwrap();
    frame(&mut app, &ctx);
    assert_eq!(app.drone_telemetry_time.availability, Availability::Live);
    let receipt = Instant::now() - std::time::Duration::from_secs(3);
    app.performance_snapshot_received = Some(receipt);
    for _ in 0..3 {
        frame(&mut app, &ctx);
    }
    assert_eq!(
        app.performance_snapshot_received,
        Some(receipt),
        "Render cannot refresh accepted receipt"
    );
    assert_eq!(app.drone_telemetry_time.availability, Availability::Stale);
    assert_eq!(app.drone_telemetry_time.server_ms, snapshot.server_time_ms);
    // An equal-time genuine receipt is still a receipt; no global fingerprint cache.
    tx.send(response(snapshot)).unwrap();
    frame(&mut app, &ctx);
    assert_eq!(app.drone_telemetry_time.availability, Availability::Live);
    tx.send(Update::Ready).unwrap();
    frame(&mut app, &ctx);
    assert_eq!(
        app.drone_telemetry_time.availability,
        Availability::Unavailable
    );
}
#[test]
fn intentional_pause_keeps_confirmed_time_but_disconnect_wins() {
    let ctx = egui::Context::default();
    let mut app = tests::view_test_cockpit(&ctx);
    let (tx, rx) = mpsc::channel();
    app.updates = Some(rx);
    tx.send(response(app.snapshot.clone().unwrap())).unwrap();
    app.poll();
    app.embedded = true;
    app.embedded_paused = true;
    app.frozen_time = Some(1000);
    app.performance_snapshot_received = Some(Instant::now() - std::time::Duration::from_secs(10));
    frame(&mut app, &ctx);
    assert_eq!(app.drone_telemetry_time.availability, Availability::Paused);
    assert_eq!(app.drone_telemetry_time.server_ms, 1000);
    tx.send(Update::Reconnecting("Test disconnect".into()))
        .unwrap();
    frame(&mut app, &ctx);
    assert_eq!(
        app.drone_telemetry_time.availability,
        Availability::Disconnected
    );
    assert_eq!(app.drone_telemetry_time.server_ms, 1000);
}
