//! Actual app state while repeatedly switching the existing render modes.
use super::*;

#[test]
fn cockpit_tactical_exterior_switches_preserve_authority_selection_and_commands() {
    let ctx = egui::Context::default();
    let mut cockpit = tests::view_test_cockpit(&ctx);
    cockpit.embedded = true;
    cockpit.embedded_input = false;
    cockpit.embedded_paused = true;
    cockpit.frozen_time = Some(1000);
    cockpit.snapshot.as_mut().unwrap().pilots[0].location = Location::Belt;
    cockpit.selected_object = SelectedObject::Belt;
    let authority = serde_json::to_value(&cockpit.snapshot).unwrap();
    let (tx, rx) = mpsc::channel();
    cockpit.commands = Some(tx);
    for step in 0..36 {
        let mode = [ViewMode::Cockpit, ViewMode::Tactical, ViewMode::External][step % 3];
        cockpit.view_mode = mode;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(1106.0, 768.0),
                )),
                time: Some(1.0 + step as f64 / 60.0),
                focused: true,
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut cockpit, ctx, &mut eframe::Frame::_new_kittest()),
        );
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), authority);
        assert!(cockpit.selected_object == SelectedObject::Belt);
        assert_eq!(
            cockpit.flight_view.computer_surface().is_some(),
            mode == ViewMode::Cockpit
        );
        assert!(!cockpit.cockpit_computer.wants_keyboard_input());
        assert!(cockpit.controls.is_empty());
        assert!(
            rx.try_recv().is_err(),
            "Changing render modes must not send a flight command"
        );
    }
}
