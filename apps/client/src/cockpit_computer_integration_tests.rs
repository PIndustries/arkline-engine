//! Real App input journeys through the current scene's physical glass ray map.
//! The secondary context is observed for widget coordinates; it is never painted
//! into the parent as an overlay. These tests do not open a network connection.
use super::*;

struct Journey {
    ctx: egui::Context,
    cockpit: Cockpit,
    frame: u64,
    size: Vec2,
}

impl Journey {
    fn new(size: Vec2) -> Self {
        let ctx = egui::Context::default();
        ctx.options_mut(|options| options.max_passes = std::num::NonZeroUsize::MIN);
        let cockpit = super::tests::view_test_cockpit(&ctx);
        let mut journey = Self {
            ctx,
            cockpit,
            frame: 0,
            size,
        };
        for _ in 0..4 {
            journey.step(vec![]);
        }
        assert!(journey.cockpit.flight_view.computer_surface().is_some());
        // Enter the detail view through the current compact console's actual
        // physical widget, retaining the projection, hit and click assertions.
        let focus = journey.require_label("DETAILS");
        journey.click(focus);
        for _ in 0..4 {
            journey.step(vec![]);
        }
        assert!(journey.cockpit.cockpit_console_detail);
        journey
    }

    fn step(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        self.step_focused(events, true)
    }

    fn step_focused(&mut self, events: Vec<egui::Event>, focused: bool) -> egui::FullOutput {
        self.frame += 1;
        // These synthetic input journeys use a fresh fixture snapshot. Keep
        // wall-clock CPU rendering time from turning them into stale-data tests.
        self.cockpit.observed = Instant::now();
        self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, self.size)),
                time: Some(self.frame as f64 / 60.0),
                focused,
                events,
                ..Default::default()
            },
            |ctx| eframe::App::update(&mut self.cockpit, ctx, &mut eframe::Frame::_new_kittest()),
        )
    }

    fn label(&self, wanted: &str) -> Option<Pos2> {
        let point = self
            .cockpit
            .computer_ui_labels
            .iter()
            .find(|(text, _)| text == wanted)?
            .1;
        let surface = self.cockpit.flight_view.computer_surface()?;
        let projected = surface.project(point)?;
        self.cockpit
            .flight_view
            .computer_hit(&self.ctx, projected)?;
        Some(projected)
    }

    fn require_label(&self, wanted: &str) -> Pos2 {
        self.label(wanted).unwrap_or_else(|| {
            panic!(
                "physical computer label {wanted:?} is not visible/hittable; actual labels: {:?}",
                self.cockpit.computer_ui_labels,
            )
        })
    }

    fn click(&mut self, point: Pos2) {
        self.step(vec![egui::Event::PointerMoved(point), button(point, true)]);
        self.step(vec![button(point, false)]);
    }

    fn scroll_point(&self) -> Pos2 {
        let surface = self.cockpit.flight_view.computer_surface().unwrap();
        [
            egui::pos2(420.0, 300.0),
            egui::pos2(300.0, 280.0),
            egui::pos2(420.0, 240.0),
        ]
        .into_iter()
        .filter_map(|point| surface.project(point))
        .find(|point| {
            self.cockpit
                .flight_view
                .computer_hit(&self.ctx, *point)
                .is_some()
        })
        .expect("content has a visible exact scene hit")
    }

    fn scroll_to_label(&mut self, wanted: &str) -> Pos2 {
        for _ in 0..80 {
            self.step(vec![]);
            if let Some(point) = self.label(wanted) {
                return point;
            }
            let point = self.scroll_point();
            self.step(vec![
                egui::Event::PointerMoved(point),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -24.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        self.require_label(wanted)
    }

    fn focus_instruction(&mut self) {
        self.cockpit.service_tab = ServiceTab::ShipComputer;
        let mut orders_opened = false;
        for _ in 0..80 {
            self.step(vec![]);
            if !orders_opened {
                if let Some(point) = self.label("Ship computer · text & voice orders") {
                    self.click(point);
                    orders_opened = true;
                    continue;
                }
            }
            if self.label("Submit instruction").is_some()
                && self.label("Gather 100 ore and bring it back").is_some()
            {
                let point = self.require_label("Gather 100 ore and bring it back");
                self.click(point);
                assert!(self.cockpit.cockpit_computer.wants_keyboard_input());
                assert!(
                    !self.ctx.wants_keyboard_input(),
                    "focus belongs to the secondary context"
                );
                return;
            }
            let point = self.scroll_point();
            self.step(vec![
                egui::Event::PointerMoved(point),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -24.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        panic!("instruction text field never scrolled into the physical display");
    }
}

fn button(pos: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    }
}

fn key(key: egui::Key, pressed: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

// Called by the original baseline test names in lib.rs.
pub(super) fn ai_console_input_journey() {
    let mut journey = Journey::new(egui::vec2(1106.0, 768.0));
    journey.focus_instruction();
    let output = journey.step(vec![egui::Event::Text("Gather 20 ore".into())]);
    assert_eq!(journey.cockpit.ai_prompt, "Gather 20 ore");
    assert!(journey.cockpit.cockpit_computer.wants_keyboard_input());
    assert!(journey.cockpit.controls.is_empty());
    assert!(
        !output.shapes.iter().any(|shape| matches!(
            &shape.shape, egui::Shape::Text(text) if text.galley.text() == "Gather 20 ore"
        )),
        "computer text must stay off the parent's 2D paint list"
    );
    let ime = output
        .platform_output
        .ime
        .expect("focused text field provides a projected IME caret");
    assert!(ime.rect.is_finite() && ime.cursor_rect.is_finite());
    assert!(egui::Rect::from_min_size(Pos2::ZERO, journey.size).contains(ime.cursor_rect.center()));
}

pub(super) fn fleet_command_input_journey() {
    let mut journey = Journey::new(egui::vec2(1500.0, 1000.0));
    let (tx, rx) = mpsc::channel();
    journey.cockpit.commands = Some(tx);
    let point = journey.require_label("Drones");
    journey.click(point);
    assert!(journey.cockpit.service_tab == ServiceTab::Drones);
    let point = journey.scroll_to_label("Launch Recon 0/5");
    journey.step(vec![
        egui::Event::PointerMoved(point),
        button(point, true),
        key(egui::Key::W, true),
    ]);
    assert!(
        journey.cockpit.controls.is_empty(),
        "pressing a console control cannot also thrust"
    );
    journey.step(vec![button(point, false), key(egui::Key::W, false)]);
    assert!(matches!(
        rx.try_recv().unwrap(),
        Request::LaunchDroneClass {
            class: DroneClass::Recon
        }
    ));
    assert!(
        rx.try_recv().is_err(),
        "one physical release issues one command"
    );
}

#[test]
fn computer_text_focus_is_revoked_before_unfocused_text_and_flight_keys() {
    let mut journey = Journey::new(egui::vec2(1500.0, 1000.0));
    journey.focus_instruction();
    journey.step(vec![egui::Event::Text("Keep".into())]);
    assert_eq!(journey.cockpit.ai_prompt, "Keep");
    journey.step_focused(
        vec![
            egui::Event::Text(" discard".into()),
            key(egui::Key::W, true),
        ],
        false,
    );
    assert_eq!(journey.cockpit.ai_prompt, "Keep");
    assert!(!journey.cockpit.cockpit_computer.wants_keyboard_input());
    assert!(journey.cockpit.controls.is_empty());
}

#[test]
fn changing_presentation_revokes_computer_capture_and_old_surface() {
    let mut journey = Journey::new(egui::vec2(1500.0, 1000.0));
    journey.focus_instruction();
    journey.cockpit.view_mode = ViewMode::External;
    journey.step(vec![egui::Event::Text("discard".into())]);
    assert!(journey.cockpit.flight_view.computer_surface().is_none());
    assert!(!journey.cockpit.cockpit_computer.wants_keyboard_input());
    assert!(journey.cockpit.ai_prompt.is_empty());
}

#[test]
fn ime_projection_rejects_invalid_canonical_geometry() {
    let journey = Journey::new(egui::vec2(1500.0, 1000.0));
    let surface = journey.cockpit.flight_view.computer_surface().unwrap();
    let rect = egui::Rect::from_min_max(egui::pos2(200.0, 200.0), egui::pos2(400.0, 280.0));
    let caret = egui::Rect::from_min_max(egui::pos2(220.0, 230.0), egui::pos2(220.0, 250.0));
    let projected = project_computer_ime(
        &surface,
        egui::output::IMEOutput {
            rect,
            cursor_rect: caret,
        },
    )
    .expect("finite canvas rectangle and zero-width caret are valid");
    assert!(projected.rect.is_finite() && projected.cursor_rect.is_finite());
    assert_ne!(projected.rect, rect);
    let invalid = egui::Rect::from_min_size(egui::pos2(f32::NAN, 0.0), egui::vec2(10.0, 10.0));
    assert!(project_computer_ime(
        &surface,
        egui::output::IMEOutput {
            rect: invalid,
            cursor_rect: caret
        }
    )
    .is_none());
    let outside = egui::Rect::from_min_size(egui::pos2(900.0, 0.0), egui::vec2(10.0, 10.0));
    assert!(project_computer_ime(
        &surface,
        egui::output::IMEOutput {
            rect: outside,
            cursor_rect: caret
        }
    )
    .is_none());
}

#[test]
fn one_copy_event_forwards_once_even_when_parent_repeats_its_layout_pass() {
    const TEXT: &str = "One clipboard delivery";
    fn copies(output: &egui::FullOutput) -> Vec<&str> {
        output
            .platform_output
            .commands
            .iter()
            .filter_map(|command| {
                if let egui::OutputCommand::CopyText(text) = command {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect()
    }
    let mut journey = Journey::new(egui::vec2(1500.0, 1000.0));
    journey.focus_instruction();
    journey.step(vec![egui::Event::Text(TEXT.into())]);
    // Select through the actual TextEdit keyboard path, then issue one native
    // Copy event. No test hook injects an output command.
    journey.step(vec![
        egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        },
        egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        },
    ]);
    let output = journey.step(vec![egui::Event::Copy]);
    assert_eq!(
        copies(&output),
        vec![TEXT],
        "one native Copy produces one parent command"
    );
    let next = journey.step(vec![]);
    assert!(
        copies(&next).is_empty(),
        "commands cannot leak into a later input frame"
    );

    journey
        .ctx
        .options_mut(|options| options.max_passes = std::num::NonZeroUsize::new(2).unwrap());
    journey.frame += 1;
    let mut passes = 0;
    let repeated = journey.ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, journey.size)),
            time: Some(journey.frame as f64 / 60.0),
            focused: true,
            events: vec![egui::Event::Copy],
            ..Default::default()
        },
        |ctx| {
            passes += 1;
            eframe::App::update(
                &mut journey.cockpit,
                ctx,
                &mut eframe::Frame::_new_kittest(),
            );
            if passes == 1 {
                ctx.request_discard("exercise cached computer output on a second parent pass");
            } else {
                let surface = journey.cockpit.flight_view.computer_surface().unwrap();
                let hit = journey.cockpit.flight_view.computer_hit_tester(ctx);
                let cached = journey.cockpit.cockpit_computer.run(
                    ctx,
                    Some(&surface),
                    flight3d::cockpit_computer::ComputerPermission {
                        active: true,
                        pointer: true,
                        keyboard: true,
                    },
                    hit,
                    |_| panic!("a cached helper pass must not replay widgets"),
                );
                assert!(cached.platform_output.commands.is_empty());
                assert!(cached.platform_output.events.is_empty());
                assert!(cached.platform_output.ime.is_some());
            }
        },
    );
    assert_eq!(
        passes, 2,
        "the regression must actually run a repeated parent layout pass"
    );
    assert_eq!(
        copies(&repeated),
        vec![TEXT],
        "cached secondary output must not replay CopyText"
    );
    assert!(journey.cockpit.cockpit_computer.wants_keyboard_input());
    assert!(
        repeated.platform_output.ime.is_some(),
        "persistent projected IME state survives the repeated pass"
    );
}

#[test]
fn compact_control_guidance_is_accessible_through_real_glass_hover() {
    let mut journey = Journey::new(egui::vec2(1280., 800.));
    let compact = journey.require_label("Flight view");
    journey.click(compact);
    assert!(!journey.cockpit.cockpit_console_detail);
    let (tx, rx) = mpsc::channel();
    journey.cockpit.commands = Some(tx);
    let before = serde_json::to_value(&journey.cockpit.snapshot).unwrap();
    let help = "Approach the station docking port to dock";
    let canvas =
        egui::Rect::from_min_size(Pos2::ZERO, flight3d::cockpit_computer::COMPUTER_CANVAS_SIZE);
    for label in ["DOCK", "STOP"] {
        journey.step(vec![egui::Event::PointerMoved(egui::pos2(10., 10.))]);
        let point = journey.require_label(label);
        journey.step(vec![egui::Event::PointerMoved(point)]);
        for _ in 0..50 {
            journey.step(vec![]);
        }
        let text = journey
            .cockpit
            .cockpit_computer
            .last_shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(t) = &shape.shape {
                    (t.galley.job.text == help).then_some((shape, t))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("Guidance missing through actual {label} glass hover"));
        assert!(canvas.contains_rect(text.1.visual_bounding_rect()));
        assert!(text
            .0
            .clip_rect
            .contains_rect(text.1.visual_bounding_rect()));
    }
    let detail = journey.require_label("DETAILS");
    journey.click(detail);
    assert!(journey.cockpit.cockpit_console_detail);
    for _ in 0..4 { journey.step(vec![]); }
    assert!(
        journey
            .cockpit
            .computer_ui_labels
            .iter()
            .any(|(text, _)| text == help),
        "Detailed view retains inline guidance"
    );
    assert!(
        rx.try_recv().is_err(),
        "Hover and local view changes must not dispatch ship orders"
    );
    assert_eq!(
        serde_json::to_value(&journey.cockpit.snapshot).unwrap(),
        before
    );
}

// Isolate the deadlock regression in a process we can kill and reap. A timeout
// thread would leave a permanently blocked egui thread alive in the test suite.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn physical_computer_press_finishes_within_a_reaped_process_deadline() {
    struct ReapedChild(std::process::Child);
    impl Drop for ReapedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = ReapedChild(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cockpit_computer_integration_tests::physical_computer_press_process_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("ARKLINE_COMPUTER_PRESS_TEST_CHILD", "1")
            .stdin(std::process::Stdio::null())
            .spawn()
            .expect("start physical press regression process"),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if let Some(status) = child
            .0
            .try_wait()
            .expect("poll physical press regression process")
        {
            assert!(
                status.success(),
                "physical press regression child failed: {status}"
            );
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "physical glass press exceeded 20s; child is killed and reaped on scope exit"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "invoked by the bounded parent regression process"]
fn physical_computer_press_process_helper() {
    assert_eq!(
        std::env::var("ARKLINE_COMPUTER_PRESS_TEST_CHILD").as_deref(),
        Ok("1")
    );
    let mut journey = Journey::new(egui::vec2(1500.0, 1000.0));
    let point = journey.require_label("Drones");
    journey.click(point);
    assert!(
        journey.cockpit.service_tab == ServiceTab::Drones,
        "real glass press/release must reach the actual secondary widget"
    );
}
