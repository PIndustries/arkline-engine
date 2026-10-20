//! Full-editor navigation and hidden-client ownership for the runtime model viewer.
use super::*;
use crate::tests::visible_point;

fn frame(
    ctx: &egui::Context,
    editor: &mut Editor,
    time: &mut f64,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    *time += 0.02;
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1500.0, 960.0),
            )),
            time: Some(*time),
            predicted_dt: 0.02,
            focused: true,
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(editor, ctx, &mut eframe::Frame::_new_kittest()),
    )
}

fn click(ctx: &egui::Context, editor: &mut Editor, time: &mut f64, label: &str) {
    frame(ctx, editor, time, vec![]);
    let output = frame(ctx, editor, time, vec![]);
    let point = visible_point(&output, label);
    for pressed in [true, false] {
        frame(
            ctx,
            editor,
            time,
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

struct Fixture {
    editor: Editor,
    _project: PathBuf,
}
impl Fixture {
    fn new(ctx: &egui::Context) -> Self {
        let project = std::env::temp_dir().join(format!(
            "arkline-editor-drone-gallery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&project).unwrap();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut editor = Editor::new(&cc, project.clone(), "DroneGalleryPilot".into());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !editor.client.ready() && std::time::Instant::now() < deadline {
            editor.client.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            editor.client.ready(),
            "Disposable local worker did not become ready"
        );
        Self {
            editor,
            _project: project,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        eframe::App::on_exit(&mut self.editor, None);
    }
}

#[test]
fn model_menu_opens_drone_workspace_and_preserves_session_on_round_trip() {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| {
            let ctx = egui::Context::default();
            let mut fixture = Fixture::new(&ctx);
            let editor = &mut fixture.editor;
            let mut time = 0.0;
            let generation = editor.client.session_generation();
            let status = editor.client.status().clone();
            let clock = editor.client.snapshot().unwrap().server_time_ms;
            let view = editor.client.debug_view_state().current;
            let scene = editor.client.scene_state();
            assert!(
                editor.drone_gallery.is_none(),
                "Viewer renderer should be lazy"
            );
            click(&ctx, editor, &mut time, "Model");
            click(&ctx, editor, &mut time, "Drone models");
            assert_eq!(editor.workspace, 8);
            assert!(editor.drone_gallery.is_some());
            let output = frame(&ctx, editor, &mut time, vec![]);
            for label in ["Drone models", "Recon", "Mining", "Attack", "Reset framing"] {
                visible_point(&output, label);
            }
            click(&ctx, editor, &mut time, "Attack");
            assert_eq!(
                editor.drone_gallery.as_ref().unwrap().selected_class(),
                arkline_protocol::DroneClass::Attack
            );
            // Held flight keys belong to this document while its viewport is
            // hidden. The existing blocked-input path and poll_hidden both run.
            for pressed in [true, false] {
                frame(
                    &ctx,
                    editor,
                    &mut time,
                    vec![egui::Event::Key {
                        key: egui::Key::ArrowLeft,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: egui::Modifiers {
                            shift: pressed,
                            ..Default::default()
                        },
                    }],
                );
            }
            assert!(!editor.client.computer_wants_keyboard_input());
            assert_eq!(editor.client.session_generation(), generation);
            assert_eq!(editor.client.status(), &status);
            assert_eq!(editor.client.snapshot().unwrap().server_time_ms, clock);
            assert_eq!(editor.client.debug_view_state().current, view);
            assert_eq!(editor.client.scene_state(), scene);
            editor.session_panel.open = true;
            for _ in 0..2 {
                frame(&ctx, editor, &mut time, vec![]);
            }
            let output = frame(&ctx, editor, &mut time, vec![]);
            for label in ["Multiplayer session", "Local", "Host", "Join"] {
                visible_point(&output, label);
            }
            editor.session_panel.open = false;
            click(&ctx, editor, &mut time, "Play view");
            assert_eq!(editor.workspace, 0);
            click(&ctx, editor, &mut time, "Model");
            click(&ctx, editor, &mut time, "Drone models");
            assert_eq!(editor.workspace, 8);
            assert_eq!(
                editor.drone_gallery.as_ref().unwrap().selected_class(),
                arkline_protocol::DroneClass::Attack,
                "Viewer selection should survive switching documents"
            );
            assert_eq!(editor.client.session_generation(), generation);
        });
        let _ = sender.send(result);
    });
    if let Err(panic) = receiver
        .recv_timeout(Duration::from_secs(12))
        .expect("Drone workspace navigation exceeded its twelve-second budget")
    {
        std::panic::resume_unwind(panic);
    }
}
