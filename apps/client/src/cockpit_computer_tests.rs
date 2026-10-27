//! Secondary-egui input journeys. These use real egui widgets and raw events;
//! scene occlusion itself is tested by the flight renderer's exact-ray tests.
use super::*;

fn permission() -> ComputerPermission {
    ComputerPermission {
        active: true,
        pointer: true,
        keyboard: true,
    }
}

fn raw(time: f64, events: Vec<Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, COMPUTER_CANVAS_SIZE)),
        time: Some(time / 60.0),
        focused: true,
        events,
        ..Default::default()
    }
}

fn point() -> Pos2 {
    egui::pos2(40.0, 20.0)
}

fn hit(point: Pos2) -> Option<Pos2> {
    Rect::from_min_size(Pos2::ZERO, COMPUTER_CANVAS_SIZE)
        .contains(point)
        .then_some(point)
}

fn button(point: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: point,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn key(key: Key, pressed: bool) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

fn button_ui_frame(
    bridge: &mut InputBridge,
    context: &Context,
    time: f64,
    events: Vec<Event>,
    pointer: Option<Pos2>,
) -> (bool, Rect) {
    let (input, _) = bridge.map(&raw(time, events), permission(), hit, pointer);
    let mut clicked = false;
    let mut bounds = Rect::NOTHING;
    let _ = context.run(input, |context| {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(context, |ui| {
                let response = ui.add_sized([120.0, 40.0], egui::Button::new("Transfer"));
                bounds = response.rect;
                clicked |= response.clicked();
            });
    });
    (clicked, bounds)
}

fn context() -> Context {
    let context = Context::default();
    context.options_mut(|options| options.max_passes = std::num::NonZeroUsize::MIN);
    context
}

#[test]
fn computer_inside_press_outside_release_never_activates_real_button() {
    let mut bridge = InputBridge::default();
    let context = context();
    let (_, bounds) = button_ui_frame(&mut bridge, &context, 1.0, vec![], None);
    let inside = bounds.center();
    let outside = egui::pos2(1000.0, 900.0);
    assert!(
        !button_ui_frame(
            &mut bridge,
            &context,
            2.0,
            vec![Event::PointerMoved(inside), button(inside, true)],
            Some(inside),
        )
        .0
    );
    assert!(
        !button_ui_frame(
            &mut bridge,
            &context,
            3.0,
            vec![button(outside, false)],
            Some(outside),
        )
        .0
    );
    assert!(bridge.buttons.is_empty());
    assert!(!bridge.keyboard_owned);
}

#[test]
fn computer_inside_release_before_outside_move_activates_real_button_once() {
    let mut bridge = InputBridge::default();
    let context = context();
    let (_, bounds) = button_ui_frame(&mut bridge, &context, 1.0, vec![], None);
    let inside = bounds.center();
    let outside = egui::pos2(1000.0, 900.0);
    let _ = button_ui_frame(
        &mut bridge,
        &context,
        2.0,
        vec![Event::PointerMoved(inside), button(inside, true)],
        Some(inside),
    );
    assert!(
        button_ui_frame(
            &mut bridge,
            &context,
            3.0,
            vec![button(inside, false), Event::PointerMoved(outside)],
            Some(outside),
        )
        .0
    );
    assert!(!button_ui_frame(&mut bridge, &context, 4.0, vec![], Some(outside)).0);
    assert!(bridge.buttons.is_empty());
}

#[test]
fn computer_touch_end_then_pointer_up_and_gone_retains_valid_click() {
    let mut bridge = InputBridge::default();
    let context = context();
    let (_, bounds) = button_ui_frame(&mut bridge, &context, 1.0, vec![], None);
    let inside = bounds.center();
    let touch = |phase| Event::Touch {
        device_id: egui::TouchDeviceId(1),
        id: egui::TouchId(2),
        phase,
        pos: inside,
        force: None,
    };
    let _ = button_ui_frame(
        &mut bridge,
        &context,
        2.0,
        vec![
            Event::PointerMoved(inside),
            touch(egui::TouchPhase::Start),
            button(inside, true),
        ],
        Some(inside),
    );
    assert!(
        button_ui_frame(
            &mut bridge,
            &context,
            3.0,
            vec![
                touch(egui::TouchPhase::End),
                button(inside, false),
                Event::PointerGone,
            ],
            None,
        )
        .0
    );
    assert!(bridge.buttons.is_empty() && bridge.touches.is_empty());
}

#[test]
fn computer_lost_ray_cancels_capture_before_later_text_or_shortcuts() {
    for positional_loss in [false, true] {
        let mut bridge = InputBridge::default();
        let inside = point();
        let _ = bridge.map(
            &raw(
                1.0,
                vec![
                    Event::PointerMoved(inside),
                    button(inside, true),
                    key(Key::W, true),
                ],
            ),
            permission(),
            hit,
            Some(inside),
        );
        assert!(bridge.keyboard_owned && bridge.keys.contains(&Key::W));
        let mut events = Vec::new();
        if positional_loss {
            events.push(Event::PointerMoved(egui::pos2(2000.0, 2000.0)));
        }
        events.extend([
            Event::Text("must not reach focused TextEdit".into()),
            Event::Paste("must not paste".into()),
            key(Key::Space, true),
        ]);
        // The callback models geometry moving in front of the same pointer.
        let (mapped, hovered) = bridge.map(&raw(2.0, events), permission(), |_| None, Some(inside));
        assert!(!hovered && !mapped.focused && !bridge.keyboard_owned);
        assert!(bridge.buttons.is_empty() && bridge.keys.is_empty());
        assert!(!mapped.events.iter().any(|event| matches!(
            event,
            Event::Text(_) | Event::Paste(_) | Event::Key { pressed: true, .. }
        )));
        assert!(mapped.events.iter().any(|event| matches!(
            event,
            Event::Key {
                key: Key::W,
                pressed: false,
                ..
            }
        )));
        assert!(mapped.events.iter().any(|event| matches!(
            event,
            Event::PointerButton { pos, pressed: false, .. } if *pos == OUTSIDE
        )));
    }
}

#[test]
fn computer_inactive_unowned_key_release_is_not_forwarded_as_a_shortcut() {
    let mut bridge = InputBridge::default();
    let mut blocked = permission();
    blocked.active = false;
    let (mapped, hovered) = bridge.map(
        &raw(1.0, vec![key(Key::Space, false), key(Key::W, false)]),
        blocked,
        hit,
        Some(point()),
    );
    assert!(!hovered && !mapped.focused);
    assert!(!mapped
        .events
        .iter()
        .any(|event| matches!(event, Event::Key { .. })));
}

#[test]
fn computer_native_focus_loss_releases_only_owned_keys_and_captured_buttons() {
    let mut bridge = InputBridge::default();
    let inside = point();
    let _ = bridge.map(
        &raw(1.0, vec![button(inside, true), key(Key::W, true)]),
        permission(),
        hit,
        Some(inside),
    );
    let mut unfocused = raw(2.0, vec![Event::WindowFocused(false), key(Key::A, false)]);
    unfocused.focused = false;
    let (mapped, hovered) = bridge.map(&unfocused, permission(), hit, Some(inside));
    assert!(!hovered && !mapped.focused);
    assert!(!bridge.keyboard_owned && bridge.buttons.is_empty() && bridge.keys.is_empty());
    let keys: Vec<_> = mapped
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Key { key, pressed, .. } => Some((*key, *pressed)),
            _ => None,
        })
        .collect();
    assert_eq!(keys, vec![(Key::W, false)]);
    let (mapped, _) = bridge.map(
        &raw(3.0, vec![key(Key::W, false)]),
        permission(),
        hit,
        Some(inside),
    );
    assert!(!mapped
        .events
        .iter()
        .any(|event| matches!(event, Event::Key { .. })));
}

#[test]
fn computer_wheel_requires_a_ray_hit_and_foreign_keyboard_owner_blocks_typing() {
    let wheel = Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -32.0),
        modifiers: Modifiers::NONE,
    };
    for (pointer, expected) in [(point(), 1), (egui::pos2(2000.0, 2000.0), 0)] {
        let mut bridge = InputBridge::default();
        let (mapped, _) = bridge.map(
            &raw(1.0, vec![Event::PointerMoved(pointer), wheel.clone()]),
            permission(),
            hit,
            Some(pointer),
        );
        assert_eq!(
            mapped
                .events
                .iter()
                .filter(|event| matches!(event, Event::MouseWheel { .. }))
                .count(),
            expected,
        );
    }
    let mut bridge = InputBridge::default();
    let mut blocked = permission();
    blocked.keyboard = false;
    let (mapped, _) = bridge.map(
        &raw(
            2.0,
            vec![
                button(point(), true),
                Event::Text("foreign".into()),
                key(Key::W, true),
            ],
        ),
        blocked,
        hit,
        Some(point()),
    );
    assert!(!mapped.focused && !bridge.keyboard_owned && bridge.buttons.is_empty());
    assert!(!mapped.events.iter().any(|event| matches!(
        event,
        Event::Text(_) | Event::Key { .. } | Event::PointerButton { pressed: true, .. }
    )));
}

#[test]
fn computer_pointer_exit_does_not_reappear_on_the_next_frame() {
    let mut bridge = InputBridge::default();
    let (_, hovered) = bridge.map(
        &raw(1.0, vec![Event::PointerMoved(point())]),
        permission(),
        hit,
        Some(point()),
    );
    assert!(hovered);
    let (mapped, hovered) =
        bridge.map(&raw(2.0, vec![Event::PointerGone]), permission(), hit, None);
    assert!(!hovered && bridge.window_pointer.is_none());
    assert!(matches!(mapped.events.last(), Some(Event::PointerGone)));
    let (mapped, hovered) = bridge.map(&raw(3.0, vec![]), permission(), hit, None);
    assert!(!hovered && bridge.window_pointer.is_none());
    assert!(matches!(mapped.events.last(), Some(Event::PointerGone)));
}

fn surface(window: egui::Vec2) -> CockpitSurface {
    use super::super::{Mat4, ShipClass, Vec3, COCKPIT_FOV};
    let viewport = Rect::from_min_size(Pos2::ZERO, window);
    let vp = Mat4::perspective_rh_gl(COCKPIT_FOV, viewport.aspect_ratio(), 0.0002, 10.0)
        * Mat4::look_at_rh(Vec3::ZERO, Vec3::X, Vec3::Y);
    CockpitSurface::new(viewport, vp, ShipClass::Pioneer, 3, COMPUTER_CANVAS_SIZE).unwrap()
}

#[test]
fn computer_fixed_canvas_and_widget_bounds_survive_projected_surface_resize() {
    let parent = context();
    let mut computer = CockpitComputer::default();
    let mut canonical = Vec::new();
    let mut projected = Vec::new();
    for (frame, window) in [egui::vec2(1500.0, 960.0), egui::vec2(600.0, 700.0)]
        .into_iter()
        .enumerate()
    {
        let surface = surface(window);
        projected.push(surface.bounds().size());
        let mut input = raw(10.0 + frame as f64, vec![]);
        input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, window));
        let _ = parent.run(input, |parent| {
            let output = computer.run(parent, Some(&surface), permission(), hit, |ui| {
                let button = ui.add_sized([140.0, 36.0], egui::Button::new("Travel"));
                canonical.push((ui.max_rect(), button.rect));
            });
            assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
            assert!(!output.primitives.is_empty());
        });
    }
    assert_ne!(projected[0], projected[1]);
    assert_eq!(canonical.len(), 2);
    assert_eq!(canonical[0], canonical[1]);
    assert_eq!(
        canonical[0].0,
        Rect::from_min_size(Pos2::ZERO, COMPUTER_CANVAS_SIZE)
    );
}

#[test]
fn computer_repeated_parent_pass_reuses_geometry_without_replaying_ui_effects() {
    let parent = context();
    let mut computer = CockpitComputer::default();
    let surface = surface(egui::vec2(1500.0, 960.0));
    let mut calls = 0;
    let _ = parent.run(raw(10.0, vec![]), |parent| {
        let first = computer.run(parent, Some(&surface), permission(), hit, |ui| {
            calls += 1;
            ui.label("Once per native input frame");
        });
        let second = computer.run(parent, Some(&surface), permission(), hit, |_| {
            calls += 1;
        });
        assert_eq!(first.primitives.len(), second.primitives.len());
        assert!(!first.primitives.is_empty());
        let mut inactive = permission();
        inactive.active = false;
        let revoked = computer.run(parent, None, inactive, hit, |_| {
            calls += 1;
        });
        assert!(!revoked.pointer_on_computer && !revoked.wants_keyboard_input);
    });
    assert_eq!(calls, 1);
    assert!(!computer.wants_keyboard_input());
    assert!(!computer.input.pending.is_empty());
    let _ = parent.run(raw(11.0, vec![]), |parent| {
        let _ = computer.run(parent, Some(&surface), permission(), hit, |_| calls += 1);
    });
    assert_eq!(calls, 2);
    assert!(computer.input.pending.is_empty());
}
