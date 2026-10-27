//! Full-editor unified Models navigation and hidden-client ownership.
use super::*;
use crate::tests::visible_point;

fn frame(
    ctx: &egui::Context,
    editor: &mut Editor,
    time: &mut f64,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    *time += 0.02;
    let modifiers = events
        .iter()
        .rev()
        .find_map(|event| match event {
            egui::Event::Key { modifiers, .. }
            | egui::Event::PointerButton { modifiers, .. }
            | egui::Event::MouseWheel { modifiers, .. } => Some(*modifiers),
            _ => None,
        })
        .unwrap_or_default();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1500.0, 960.0),
            )),
            time: Some(*time),
            predicted_dt: 0.02,
            focused: true,
            modifiers,
            events,
            ..Default::default()
        },
        |ctx| eframe::App::update(editor, ctx, &mut eframe::Frame::_new_kittest()),
    )
}

fn click(ctx: &egui::Context, editor: &mut Editor, time: &mut f64, label: &str) {
    frame(ctx, editor, time, vec![]);
    let output = frame(ctx, editor, time, vec![]);
    click_point(ctx, editor, time, visible_point(&output, label));
}

fn click_point(ctx: &egui::Context, editor: &mut Editor, time: &mut f64, point: egui::Pos2) {
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
            "arkline-editor-model-browser-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&project).unwrap();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut editor = Editor::new(&cc, project.clone(), "ModelBrowserPilot".into());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while (!editor.client.ready() || editor.imports.saved.is_none())
            && std::time::Instant::now() < deadline
        {
            editor.client.poll();
            editor.imports.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            editor.client.ready(),
            "Disposable local worker did not become ready"
        );
        assert!(
            editor.imports.saved.is_some(),
            "Initial import baseline did not load"
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

#[derive(Debug, PartialEq)]
struct ImportState {
    index: import_document::AssetIndex,
    saved: Option<import_document::AssetIndex>,
    accepted_sha: Option<String>,
    staged_sha: Option<String>,
}
impl ImportState {
    fn capture(imports: &imports::Imports) -> Self {
        Self {
            index: imports.index.clone(),
            saved: imports.saved.clone(),
            accepted_sha: imports.accepted.as_ref().map(|asset| asset.sha256.clone()),
            staged_sha: imports.staged.as_ref().map(|asset| asset.sha256.clone()),
        }
    }
}

fn return_from_import_manager(ctx: &egui::Context, editor: &mut Editor, time: &mut f64) {
    frame(ctx, editor, time, vec![]);
    let output = frame(ctx, editor, time, vec![]);
    let points: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "Models" => {
                let rect = text.galley.rect.translate(text.pos.to_vec2());
                shape.clip_rect.contains_rect(rect).then_some(rect.center())
            }
            _ => None,
        })
        .collect();
    assert!(
        points.len() >= 2,
        "Import manager needs its Models return button alongside the primary tab"
    );
    let back = points
        .into_iter()
        .max_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap();
    click_point(ctx, editor, time, back);
}

#[test]
fn models_browser_and_import_manager_preserve_session_and_unsaved_assets() {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| {
            let ctx = egui::Context::default();
            let mut fixture = Fixture::new(&ctx);
            let assets_path = fixture._project.join("Assets");
            let editor = &mut fixture.editor;
            let mut time = 0.0;
            let generation = editor.client.session_generation();
            let status = editor.client.status().clone();
            let clock = editor.client.snapshot().unwrap().server_time_ms;
            let view = editor.client.debug_view_state().current;
            let scene = editor.client.scene_state();
            let dirty_sources = editor.project_workspace.has_dirty_buffers();
            let pending_source_save = editor.project_workspace.has_pending_save();

            // Keep both an accepted-but-unsaved revision and a distinct staged
            // candidate. Browsing and manager navigation must retain both.
            let accepted = import_document::Verified::decode(
                include_bytes!("../tests/fixtures/import/wedge.glb").to_vec(),
            )
            .unwrap();
            let staged = import_document::Verified::decode(
                include_bytes!("../tests/fixtures/import/spire.glb").to_vec(),
            )
            .unwrap();
            assert_ne!(accepted.sha256, staged.sha256);
            editor.imports.index = editor
                .imports
                .index
                .accept(
                    &accepted,
                    "Accepted source".into(),
                    "Accepted license".into(),
                )
                .unwrap();
            editor.imports.accepted = Some(accepted);
            editor.imports.staged = Some(staged);
            let imports_before = ImportState::capture(&editor.imports);
            assert_ne!(Some(&imports_before.index), imports_before.saved.as_ref());
            assert!(editor.model_browser.is_none(), "Browser should be lazy");

            click(&ctx, editor, &mut time, "Models");
            assert_eq!(editor.workspace, 8);
            let browser = editor.model_browser.as_ref().unwrap();
            let default_id = arkline_client::runtime_model_catalog()
                .into_iter()
                .find(|descriptor| descriptor.unavailable.is_none())
                .unwrap()
                .id;
            assert_eq!(browser.selected_id(), Some(default_id.as_str()));
            let selected = browser.selected_id().unwrap().to_owned();
            let output = frame(&ctx, editor, &mut time, vec![]);
            for label in ["Search models", "Refresh", "Import / manage…"] {
                visible_point(&output, label);
            }
            assert_eq!(ImportState::capture(&editor.imports), imports_before);

            // Flight input stays neutral while the client viewport is hidden.
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
            let mut output = frame(&ctx, editor, &mut time, vec![]);
            // Explicit preview opt-ins add dialog content. Reach the existing
            // connection row by real scrolling, never an offscreen click.
            let scroll_at = ctx
                .memory(|memory| memory.area_rect(egui::Id::new("Multiplayer session")))
                .unwrap()
                .center();
            for _ in 0..8 {
                let local_visible = output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "Local" && shape.clip_rect.contains_rect(text.visual_bounding_rect())));
                if local_visible {
                    break;
                }
                frame(
                    &ctx,
                    editor,
                    &mut time,
                    vec![
                        egui::Event::PointerMoved(scroll_at),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0., -100.),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
                for _ in 0..12 {
                    output = frame(&ctx, editor, &mut time, vec![]);
                }
            }
            for label in ["Multiplayer session", "Local", "Host", "Join"] {
                visible_point(&output, label);
            }
            editor.session_panel.open = false;

            click(&ctx, editor, &mut time, "Import / manage…");
            assert_eq!(editor.workspace, 5);
            assert_eq!(ImportState::capture(&editor.imports), imports_before);
            return_from_import_manager(&ctx, editor, &mut time);
            assert_eq!(editor.workspace, 8);
            assert_eq!(
                editor.model_browser.as_ref().unwrap().selected_id(),
                Some(selected.as_str())
            );
            assert_eq!(ImportState::capture(&editor.imports), imports_before);

            click(&ctx, editor, &mut time, "Play");
            assert_eq!(editor.workspace, 0);
            click(&ctx, editor, &mut time, "Models");
            assert_eq!(editor.workspace, 8);
            assert_eq!(
                editor.model_browser.as_ref().unwrap().selected_id(),
                Some(selected.as_str()),
                "Model selection should survive switching documents"
            );
            assert_eq!(ImportState::capture(&editor.imports), imports_before);
            assert_eq!(editor.client.session_generation(), generation);
            assert_eq!(editor.client.status(), &status);
            assert_eq!(editor.client.snapshot().unwrap().server_time_ms, clock);
            assert_eq!(editor.client.debug_view_state().current, view);
            assert_eq!(editor.client.scene_state(), scene);
            assert_eq!(editor.project_workspace.has_dirty_buffers(), dirty_sources);
            assert_eq!(
                editor.project_workspace.has_pending_save(),
                pending_source_save
            );
            assert!(
                !assets_path.exists(),
                "Model inspection must not publish unsaved assets"
            );
        });
        let _ = sender.send(result);
    });
    if let Err(panic) = receiver
        .recv_timeout(Duration::from_secs(12))
        .expect("Models navigation exceeded its twelve-second budget")
    {
        std::panic::resume_unwind(panic);
    }
}
