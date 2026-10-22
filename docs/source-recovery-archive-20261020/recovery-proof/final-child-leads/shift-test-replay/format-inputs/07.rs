//! Real editor/embedded-worker input regression. No GL, window, OS input or public connection.
use super::*;
use crate::tests::visible_point;
use std::panic::{catch_unwind, AssertUnwindSafe};

struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "arkline-editor-shift-embedded-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn frame(
    ctx: &egui::Context,
    editor: &mut Editor,
    size: egui::Vec2,
    time: &mut f64,
    shift: bool,
    focused: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    *time += 0.02;
    catch_unwind(AssertUnwindSafe(|| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(*time),
                predicted_dt: 0.02,
                modifiers: egui::Modifiers {
                    shift,
                    ..Default::default()
                },
                focused,
                events,
                ..Default::default()
            },
            |ctx| {
                eframe::App::update(editor, ctx, &mut eframe::Frame::_new_kittest());
            },
        )
    }))
    .unwrap_or_else(|panic| {
        let message = panic.downcast_ref::<String>().map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied()).unwrap_or("non-string panic");
        panic!("Editor::update panic at {size:?}, Shift={shift}, focused={focused}, document={:?}, view={:?}, status={:?}: {message}",
            editor.shell.document, editor.client.debug_view_state(), editor.client.status());
    })
}

fn click(ctx: &egui::Context, editor: &mut Editor, size: egui::Vec2, time: &mut f64, label: &str) {
    frame(ctx, editor, size, time, false, true, vec![]);
    let output = frame(ctx, editor, size, time, false, true, vec![]);
    let point = visible_point(&output, label);
    for pressed in [true, false] {
        frame(
            ctx,
            editor,
            size,
            time,
            false,
            true,
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
}

fn visible_in(output: &egui::FullOutput, label: &str, area: egui::Rect) -> bool {
    output.shapes.iter().any(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.text() == label => {
            let rect = text.galley.rect.translate(text.pos.to_vec2());
            shape.clip_rect.contains_rect(rect) && area.contains_rect(rect)
        }
        _ => false,
    })
}

fn assert_windows(
    _ctx: &egui::Context,
    editor: &Editor,
    output: &egui::FullOutput,
    diagnostics: bool,
    size: egui::Vec2,
    state: &str,
    expect_console: bool,
) {
    assert!(
        editor.shell.navigation && editor.shell.properties,
        "{state}: editor panels were closed at {size:?}"
    );
    assert_eq!(
        editor.shell.diagnostics, diagnostics,
        "{state}: diagnostics visibility changed at {size:?}"
    );
    assert_eq!(
        editor.shell.document,
        Document::Scene,
        "{state}: input changed document"
    );
    assert!(
        !editor.client.free_camera_active(),
        "{state}: input switched editor camera mode"
    );
    assert_eq!(
        editor.client.debug_view_state().current,
        Some(arkline_client::EmbeddedView::Cockpit),
        "{state}: input switched the actual embedded view"
    );
    assert_eq!(
        editor.client.status(),
        &EmbeddedStatus::Running,
        "{state}: input interrupted the actual Play worker"
    );
    assert!(
        visible_in(output, "Navigator", editor.shell_rects[1]),
        "{state}: Navigator content disappeared at {size:?}"
    );
    assert!(
        visible_in(output, "Properties", editor.shell_rects[2]),
        "{state}: Properties content disappeared at {size:?}"
    );
    for label in [
        "▶  Play",
        "Ⅱ  Pause",
        "Step 50 ms",
        "■  Stop",
        "Save",
        "Layout",
        "Session",
    ] {
        assert!(
            visible_in(output, label, editor.shell_rects[0]),
            "{state}: toolbar {label:?} disappeared at {size:?}"
        );
    }
    if diagnostics {
        visible_point(output, "Hide diagnostics");
    }
    if !expect_console {
        return;
    }
    assert!(
        editor.client.computer_display_visible(),
        "{state}: embedded physical computer geometry disappeared at {size:?}"
    );
}

#[test]
fn actual_editor_embedded_play_keeps_windows_through_shift_and_text_focus() {
    let project = Project::new();
    let ctx = egui::Context::default();
    ctx.style_mut(|style| style.animation_time = 0.);
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut editor = Editor::new(&cc, project.0.clone(), "ShiftEmbeddedPilot".into());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !editor.client.ready() && std::time::Instant::now() < deadline {
        editor.client.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        editor.client.ready(),
        "disposable local worker did not become ready: {:?}",
        editor.client.status()
    );
    editor
        .client
        .set_debug_view(arkline_client::EmbeddedView::Cockpit)
        .unwrap();
    editor.client.set_free_camera(false);
    editor.navigate(Document::Scene);
    editor.session_panel.open = false;
    let mut time = 0.;
    click(
        &ctx,
        &mut editor,
        egui::vec2(1500., 960.),
        &mut time,
        "▶  Play",
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while editor.client.status() != &EmbeddedStatus::Running && std::time::Instant::now() < deadline
    {
        editor.client.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(editor.client.status(), &EmbeddedStatus::Running);
    let selected = (
        editor.selected_pilot.clone(),
        editor.selected_equipment.clone(),
        editor.inspector_tab,
    );
    let generation = editor.client.session_generation();
    for (size, diagnostics) in [
        (egui::vec2(1500., 960.), false),
        (egui::vec2(1500., 960.), true),
        (egui::vec2(1080., 720.), false),
    ] {
        editor.shell.navigation = true;
        editor.shell.properties = true;
        editor.shell.diagnostics = diagnostics;
        editor.bottom_tab = 1; // Project diagnostics preserve game input; this is not the command console.
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        for _ in 0..4 {
            frame(&ctx, &mut editor, size, &mut time, false, true, vec![]);
        }

        for (state, shift, focused, arrow) in [
            ("Shift pressed", true, true, None),
            ("Shift held", true, true, None),
            ("Shift + left", true, true, Some(egui::Key::ArrowLeft)),
            ("Shift released", false, true, None),
            ("Window focus lost", true, false, None),
            ("Window focus returned", false, true, None),
        ] {
            let modifiers = egui::Modifiers {
                shift,
                ..Default::default()
            };
            let mut events = Vec::new();
            if let Some(key) = arrow {
                events.push(egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                });
            }
            for pass in 0..2 {
                let output = frame(
                    &ctx,
                    &mut editor,
                    size,
                    &mut time,
                    shift,
                    focused,
                    if pass == 0 { events.clone() } else { vec![] },
                );
                assert_windows(&ctx, &editor, &output, diagnostics, size, state, true);
            }
            if let Some(key) = arrow {
                frame(
                    &ctx,
                    &mut editor,
                    size,
                    &mut time,
                    shift,
                    focused,
                    vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: false,
                        repeat: false,
                        modifiers,
                    }],
                );
            }
        }
        // Parent editor text focus must also suppress head-look. Secondary
        // computer text focus and ray clicks are covered by the client bridge
        // tests; a physical monitor no longer emits parent 2D text shapes.
        let search = egui::Id::new("editor_shift_focus_fixture");
        ctx.memory_mut(|memory| memory.request_focus(search));
        frame(&ctx, &mut editor, size, &mut time, true, true, vec![]);
        // Keep the real key held across the screen edge. Only the cockpit
        // surface may leave view; unrelated editor windows must stay open.
        ctx.memory_mut(|memory| memory.surrender_focus(search));
        for key in [egui::Key::ArrowLeft, egui::Key::ArrowRight] {
            let modifiers = egui::Modifiers {
                shift: true,
                ..Default::default()
            };
            for pass in 0..96 {
                let events = if pass == 0 {
                    vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    }]
                } else {
                    vec![]
                };
                let output = frame(&ctx, &mut editor, size, &mut time, true, true, events);
                assert_windows(
                    &ctx,
                    &editor,
                    &output,
                    diagnostics,
                    size,
                    "Held Shift head-look sweep",
                    false,
                );
            }
            frame(
                &ctx,
                &mut editor,
                size,
                &mut time,
                false,
                true,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            for pass in 0..24 {
                let output = frame(&ctx, &mut editor, size, &mut time, false, true, vec![]);
                assert_windows(
                    &ctx,
                    &editor,
                    &output,
                    diagnostics,
                    size,
                    "Released Shift after head-look sweep",
                    pass >= 20,
                );
            }
        }
        assert_eq!(editor.client.session_generation(), generation);
        assert_eq!(
            (
                editor.selected_pilot.clone(),
                editor.selected_equipment.clone(),
                editor.inspector_tab
            ),
            selected
        );
    }
    editor.client.on_exit();
}
