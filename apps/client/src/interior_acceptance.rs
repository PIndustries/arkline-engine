//! Deterministic full-app input acceptance, not an OS window or network acceptance.
use super::*;
use eframe::App;
struct Journey {
    ctx: egui::Context,
    app: Cockpit,
    frame: eframe::Frame,
    server: arkline_server::Server,
    session: Option<String>,
    now: u64,
    commands: mpsc::Receiver<Request>,
    updates: mpsc::Sender<Update>,
    held: Option<egui::Key>,
    emitted: usize,
}
impl Journey {
    fn new() -> Self {
        let ctx = egui::Context::default();
        let mut app = crate::tests::view_test_cockpit(&ctx);
        app.services_open = false;
        let mut server = arkline_server::Server::open(":memory:").unwrap();
        let mut session = None;
        let response = server.handle_at(
            &mut session,
            Request::Connect {
                supports_pioneer_interior: true, supports_docked_traversal_v2: false,
                world_layout_version: 1,
                supports_fleet_hangar: true,
                supports_room_controls: true,
                protocol_version: PROTOCOL_VERSION,
                pilot: "pilot".into(),
                resume_token: None,
            },
            1000,
        );
        assert!(response.ok);
        let (command_tx, commands) = mpsc::channel();
        app.commands = Some(command_tx);
        let (updates, rx) = mpsc::channel();
        app.updates = Some(rx);
        updates
            .send(Update::Response(Box::new(response), true))
            .unwrap();
        app.poll();
        Self {
            ctx,
            app,
            frame: eframe::Frame::_new_kittest(),
            server,
            session,
            now: 1000,
            commands,
            updates,
            held: None,
            emitted: 0,
        }
    }
    fn step(&mut self, events: Vec<egui::Event>) -> Vec<(String, Pos2)> {
        self.now += 25;
        self.server.tick_at(self.now).unwrap();
        let snapshot = self
            .server
            .handle_at(&mut self.session, Request::Snapshot, self.now);
        self.updates
            .send(Update::Response(Box::new(snapshot), false))
            .unwrap();
        let output = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(1280.0, 800.0),
                )),
                time: Some(self.now as f64 / 1000.0),
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| self.app.update(ctx, &mut self.frame),
        );
        let mut requests = self.commands.try_iter().collect::<Vec<_>>();
        // Match the worker's 20 Hz latest-value control cadence.
        if self.now % 50 == 0 {
            if let Some(control) = self.app.controls.take() {
                requests.push(control);
            }
        }
        for request in requests {
            let response = self
                .server
                .handle_at(&mut self.session, request.clone(), self.now);
            assert!(
                response.ok,
                "UI emitted rejected {request:?}: {}",
                response.message
            );
            self.emitted += 1;
            self.updates
                .send(Update::Response(Box::new(response), true))
                .unwrap();
        }
        self.app.poll();
        output
            .shapes
            .into_iter()
            .filter_map(|s| {
                if let egui::Shape::Text(t) = s.shape {
                    let pos = t.pos + t.galley.rect.center().to_vec2();
                    s.clip_rect
                        .contains(pos)
                        .then_some((t.galley.job.text.clone(), pos))
                } else {
                    None
                }
            })
            .collect()
    }
    fn click(&mut self, label: &str) {
        self.step(vec![]);
        let rows = self.step(vec![]);
        let point = rows
            .iter()
            .find(|(text, _)| text == label)
            .unwrap_or_else(|| panic!("button missing: {label}; {rows:?}"))
            .1;
        for pressed in [true, false] {
            self.step(vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]);
        }
    }
    fn key(&mut self, key: Option<egui::Key>) {
        let mut events = Vec::new();
        if self.held != key {
            for (k, pressed) in self
                .held
                .map(|k| (k, false))
                .into_iter()
                .chain(key.map(|k| (k, true)))
            {
                events.push(egui::Event::Key {
                    key: k,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: Default::default(),
                });
            }
            self.held = key;
        }
        self.step(events);
    }
    fn avatar(&self) -> StationAvatar {
        self.app.own().unwrap().station_avatar.unwrap()
    }
    fn go(&mut self, x: f64, z: f64) {
        // Camera remains +Z from initial entry. Inputs are real app keys, never Navigate/Walk injection.
        for axis in [0, 2] {
            let goal = if axis == 0 { x } else { z };
            let mut reached = false;
            for _ in 0..1600 {
                let error = goal - self.avatar().position[axis];
                if error.abs() < 0.11 {
                    self.key(None);
                    reached = true;
                    break;
                }
                self.key(Some(if axis == 0 {
                    if error > 0.0 {
                        egui::Key::A
                    } else {
                        egui::Key::D
                    }
                } else if error > 0.0 {
                    egui::Key::W
                } else {
                    egui::Key::S
                }));
            }
            assert!(
                reached,
                "keyboard stalled en route to ({x},{z}): {:?}, keyboard focus:{}",
                self.avatar(),
                self.ctx.wants_keyboard_input()
            );
        }
    }
}
#[test]
#[ignore = "Explicit full Cockpit input+authoritative simulated-clock journey; no native window/GPU"]
fn both_decks_pointer_keyboard_airlock_board_undock() {
    let mut j = Journey::new();
    j.click("Explore Pioneer interior");
    assert!(j.avatar().onboard_interior);
    for deck in [1, 2] {
        j.go(j.avatar().position[0], 7.0);
        for &(_, x, z) in pioneer_rooms(deck) {
            let door = x + 8.9333333333 / 2.0;
            j.go(door, 7.0);
            j.go(door, z + 2.7);
            j.go(door, 7.0);
        }
        j.go(2.0, 7.0);
        let camera_yaw = j.app.hangar_view.camera_yaw();
        j.click(if deck == 1 {
            "Use lift to deck 2"
        } else {
            "Use lift to deck 1"
        });
        assert_eq!(j.avatar().interior_deck, if deck == 1 { 2 } else { 1 });
        // Render the confirmed new floor; the lift must preserve the user camera yaw.
        j.step(vec![]);
        assert!((j.app.hangar_view.camera_yaw() - camera_yaw).abs() < 0.001);
    }
    j.go(PIONEER_INTERIOR_ENTRY[0], 7.0);
    j.go(PIONEER_INTERIOR_ENTRY[0], 3.0);
    j.click("Exit airlock to hangar");
    assert!(!j.avatar().onboard_interior);
    j.click("Enter ship");
    assert!(j.app.own().unwrap().station_avatar.is_none());
    j.click("Undock active vessel");
    assert_eq!(j.app.own().unwrap().location, Location::Station);
    println!("12 rooms / 2 lift transitions / airlock / boarding / undock passed with {} UI-emitted requests",j.emitted);
}

#[test]
fn on_foot_camera_buttons_and_ship_command_boundary() {
    use flight3d::hangar::OnFootView;
    let mut j = Journey::new();
    j.click("Explore Pioneer interior");
    let start = j.avatar();
    j.click("First person");
    assert_eq!(j.app.hangar_view.on_foot_view, OnFootView::FirstPerson);
    assert_eq!(j.avatar().position, start.position);
    for key in [
        egui::Key::W,
        egui::Key::S,
        egui::Key::ArrowUp,
        egui::Key::ArrowDown,
    ] {
        let before = j.avatar().position;
        for _ in 0..8 {
            j.key(Some(key));
        }
        j.key(None);
        assert!(distance_m(before, j.avatar().position) > 0.4);
        assert!(
            (j.avatar().heading - start.heading).abs() < 0.01,
            "walking/backpedalling must not spin camera"
        );
    }
    let before_turn = j.avatar();
    let (surface, layer) = j.app.hangar_view.input_surface().unwrap();
    let start_pointer = (2..10)
        .flat_map(|y| (2..10).map(move |x| Pos2::new(x as f32 * 100., y as f32 * 60.)))
        .find(|p| {
            surface.contains(*p)
                && j.ctx.layer_id_at(*p) == Some(layer)
                && j.ctx.layer_id_at(*p + Vec2::new(70., 0.)) == Some(layer)
        })
        .unwrap();
    j.step(vec![egui::Event::PointerMoved(start_pointer)]);
    j.step(vec![
        egui::Event::PointerMoved(start_pointer),
        egui::Event::PointerButton {
            pos: start_pointer,
            button: egui::PointerButton::Secondary,
            pressed: true,
            modifiers: Default::default(),
        },
    ]);
    let end_pointer = start_pointer + Vec2::new(70.0, 0.0);
    j.step(vec![egui::Event::PointerMoved(end_pointer)]);
    j.step(vec![egui::Event::PointerButton {
        pos: end_pointer,
        button: egui::PointerButton::Secondary,
        pressed: false,
        modifiers: Default::default(),
    }]);
    for _ in 0..40 {
        j.step(vec![]);
    }
    assert!(
        (j.avatar().heading - before_turn.heading).abs() > 0.1,
        "RMB must turn actual avatar, not detach third-person orbit: desired {}, before {}, after {} at {:?}", j.app.hangar_view.camera_yaw(), before_turn.heading, j.avatar().heading, start_pointer
    );
    assert!(distance_m(j.avatar().position, before_turn.position) < 0.001);
    j.click("Third person");
    assert_eq!(j.app.hangar_view.on_foot_view, OnFootView::ThirdPerson);
    for request in [
        Request::Undock,
        Request::Dock,
        Request::Navigate {
            mode: NavigationMode::Stop,
        },
        Request::PilotInput {
            translation: [1., 0., 0.],
            rotation: [0.; 3],
        },
        Request::SelectShip {
            ship_class: ShipClass::Hauler,
        },
        Request::StartMining {
            module_id: 0,
            target_id: 1,
        },
        Request::LaunchDrone,
        Request::StartResourceJob {
            target_units: 1,
            pilot: None,
        },
        Request::BeginRemoteControl {
            pilot: "worker".into(),
        },
    ] {
        j.app.send(request);
        assert!(
            j.commands.try_recv().is_err(),
            "ship command escaped on-foot guard"
        );
    }
    for tab in [
        ServiceTab::Navigation,
        ServiceTab::Modules,
        ServiceTab::Drones,
        ServiceTab::Hangar,
    ] {
        j.app.service_tab = tab;
        j.app.services_open = true;
        j.step(vec![]);
        let text = j
            .step(vec![])
            .into_iter()
            .map(|(text, _)| text)
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("ON FOOT"));
        assert!(!text.contains("Undock active vessel"));
        assert!(!text.contains("Activate turret"));
        let labels = j
            .step(vec![])
            .into_iter()
            .map(|(t, _)| t)
            .collect::<Vec<_>>();
        for forbidden in [
            "Flight", "Ship", "Modules", "Drones", "AI", "Surface", "M", "D", "H", "◎",
        ] {
            assert!(
                !labels.iter().any(|t| t == forbidden),
                "misleading on-foot launcher: {forbidden}"
            );
        }
        for retained in ["▣", "⌂", "L", "⇄", "C", "≡", "✓", "⚙"] {
            assert!(
                labels.iter().any(|t| t == retained),
                "missing on-foot service: {retained}; {labels:?}"
            );
        }
    }
}
