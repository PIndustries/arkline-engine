use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
const WEDGE: &[u8] = include_bytes!("../tests/fixtures/import/wedge.glb");
const SPIRE: &[u8] = include_bytes!("../tests/fixtures/import/spire.glb");

fn entry(id: &str, name: &str, category: &str, bytes: &[u8]) -> Entry {
    Entry {
        id: id.into(),
        name: name.into(),
        category: category.into(),
        source: "Test project".into(),
        identity: format!("Assets/{id}.glb"),
        notes: Vec::new(),
        unavailable: None,
        spec: None,
        model: Source::Snapshot(Arc::new(bytes.to_vec())),
    }
}
fn answer(work: Work) -> Result<Answer, String> {
    match work {
        Work::Scan => Ok(Answer::Scanned(sources::SourceCatalog::default())),
        Work::Load { entry } => match entry.model {
            Source::Snapshot(bytes) => Verified::decode(bytes.as_ref().clone())
                .map(|verified| Answer::Loaded(LoadedModel::Imported(verified))),
            _ => Err("Unexpected test source".into()),
        },
    }
}
fn settle(browser: &mut ModelBrowser) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while browser.active.is_some() || browser.waiting.is_some() || browser.refresh_requested {
        browser.poll();
        assert!(
            Instant::now() < deadline,
            "Model worker exceeded three seconds"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn metadata_search_is_case_insensitive_and_category_switches_keep_selection() {
    let mut browser = ModelBrowser::with_worker(Vec::new(), answer);
    settle(&mut browser);
    browser.runtime = vec![
        entry("pioneer", "Pioneer", "Ships", WEDGE),
        entry("recon", "Recon drone", "Drones", SPIRE),
    ];
    browser.select(browser.runtime[0].clone(), false);
    settle(&mut browser);
    let selected = browser.selected_id().unwrap().to_owned();
    for (search, category, expected) in [
        ("PIONEER", None, 1),
        ("assets/RECON", None, 1),
        ("test project", Some("Drones"), 1),
        ("", Some("Ships"), 1),
        ("missing model", None, 0),
    ] {
        let search = search.to_lowercase();
        assert_eq!(
            browser
                .entries()
                .filter(|entry| entry.matches(&search, category))
                .count(),
            expected
        );
        assert_eq!(browser.selected_id(), Some(selected.as_str()));
    }
}

#[test]
fn repeated_selection_uses_cache_and_invalid_or_unsupported_selection_clears_geometry() {
    let loads = Arc::new(AtomicUsize::new(0));
    let counted = loads.clone();
    let mut browser = ModelBrowser::with_worker(Vec::new(), move |work| {
        if matches!(work, Work::Load { .. }) {
            counted.fetch_add(1, Ordering::SeqCst);
        }
        answer(work)
    });
    settle(&mut browser);
    let valid = entry("valid", "Valid model", "Project", WEDGE);
    browser.select(valid.clone(), false);
    settle(&mut browser);
    browser.select(valid.clone(), false);
    browser.poll();
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert!(matches!(browser.loaded, Some(LoadedModel::Imported(_))));
    let invalid = entry("invalid", "Broken model", "Project", b"not a GLB");
    browser.select(invalid, false);
    assert!(browser.loaded.is_none());
    assert_eq!(browser.state, LoadState::Loading);
    settle(&mut browser);
    assert!(matches!(browser.state, LoadState::Error(_)));
    assert!(browser.loaded.is_none());
    let mut source = entry("source", "Model.blend", "Source files", &[]);
    source.unavailable = Some("BLEND source files cannot be previewed".into());
    browser.select(source, false);
    assert!(matches!(browser.state, LoadState::Unavailable(_)));
    assert!(browser.loaded.is_none());
    browser.select(valid, false);
    assert_eq!(browser.state, LoadState::Ready);
    assert_eq!(
        loads.load(Ordering::SeqCst),
        2,
        "Cached selection must not decode again"
    );
}

#[test]
fn rapid_selection_keeps_only_latest_request_and_discards_stale_results() {
    let (started, first_started) = mpsc::channel();
    let (release, first_release) = mpsc::channel();
    let loads = Arc::new(AtomicUsize::new(0));
    let counted = loads.clone();
    let mut browser = ModelBrowser::with_worker(Vec::new(), move |work| {
        if let Work::Load { entry } = &work {
            counted.fetch_add(1, Ordering::SeqCst);
            if entry.id == "first" {
                started.send(()).unwrap();
                first_release.recv_timeout(Duration::from_secs(2)).unwrap();
            }
        }
        answer(work)
    });
    settle(&mut browser);
    browser.select(entry("first", "First", "Project", WEDGE), false);
    first_started
        .recv_timeout(Duration::from_secs(2))
        .expect("First selection did not reach worker");
    browser.select(entry("discard", "Discarded", "Project", b"broken"), false);
    browser.select(entry("latest", "Latest", "Project", SPIRE), false);
    release.send(()).unwrap();
    settle(&mut browser);
    assert_eq!(browser.selected_id(), Some("latest"));
    assert_eq!(browser.state, LoadState::Ready);
    assert_eq!(
        loads.load(Ordering::SeqCst),
        2,
        "Intermediate selection must never decode"
    );
    match browser.loaded.as_ref().unwrap() {
        LoadedModel::Imported(verified) => {
            assert_eq!(verified.geometry.report.min_m, [-7.0, 1.0, 3.0])
        }
        _ => panic!("Expected actual imported spire geometry"),
    }
    assert!(browser.cache.iter().all(|(id, _)| id == "latest"));
}

#[test]
fn import_snapshots_and_guarded_files_snapshot_stay_separate_and_cache_is_bounded() {
    let mut browser = ModelBrowser::with_worker(Vec::new(), answer);
    settle(&mut browser);
    let verified = Verified::decode(WEDGE.to_vec()).unwrap();
    browser.sync_imports(vec![BrowserImportSnapshot {
        id: "accepted:fixture".into(),
        name: "Accepted fixture".into(),
        source: "Accepted import".into(),
        identity: verified.sha256.clone(),
        notes: vec!["Unsaved".into()],
        verified: Some(verified.clone()),
        unavailable: None,
    }]);
    assert!(browser.open_snapshot("Files fixture".into(), SPIRE.to_vec()));
    settle(&mut browser);
    assert_eq!(browser.imports.len(), 1);
    assert_eq!(browser.imports[0].identity, verified.sha256);
    assert_eq!(
        browser.snapshot.as_ref().unwrap().source,
        "Files · guarded snapshot"
    );
    for i in 0..5 {
        browser.select(
            entry(&format!("cache-{i}"), "Fixture", "Project", WEDGE),
            false,
        );
        settle(&mut browser);
    }
    assert_eq!(browser.cache.len(), 3);
    browser.select(browser.imports[0].clone(), false);
    assert_eq!(browser.state, LoadState::Ready);
    browser.sync_imports(Vec::new());
    assert!(browser.loaded.is_none());
    assert!(matches!(browser.state, LoadState::Unavailable(_)));
}

fn visible(output: &egui::FullOutput, label: &str) -> egui::Rect {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                let rect = text.galley.rect.translate(text.pos.to_vec2());
                shape.clip_rect.contains_rect(rect).then_some(rect)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Visible {label:?} not found"))
}
fn ui_frame(
    ctx: &egui::Context,
    browser: &mut ModelBrowser,
    imports: &Imports,
    size: egui::Vec2,
    time: &mut f64,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    *time += 0.02;
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(*time),
            predicted_dt: 0.02,
            events,
            ..Default::default()
        },
        |ctx| {
            browser.show(ctx, None, imports);
        },
    )
}

#[test]
fn real_list_click_selects_and_no_results_clear_control_recovers_at_compact_sizes() {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| {
            let project = std::env::temp_dir().join(format!(
                "arkline-model-browser-ui-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&project).unwrap();
            let mut imports = Imports::open(&project);
            let mut browser = ModelBrowser::with_worker(Vec::new(), answer);
            settle(&mut browser);
            browser.runtime = vec![
                entry("alpha", "Alpha fixture", "Models", WEDGE),
                entry("beta", "Beta fixture", "Models", SPIRE),
            ];
            let ctx = egui::Context::default();
            let mut time = 0.0;
            for size in [egui::vec2(1080.0, 720.0), egui::vec2(800.0, 600.0)] {
                imports.poll();
                ui_frame(&ctx, &mut browser, &imports, size, &mut time, vec![]);
                let output = ui_frame(&ctx, &mut browser, &imports, size, &mut time, vec![]);
                let point = visible(&output, "Beta fixture").center();
                for pressed in [true, false] {
                    ui_frame(
                        &ctx,
                        &mut browser,
                        &imports,
                        size,
                        &mut time,
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
                settle(&mut browser);
                assert_eq!(browser.selected_id(), Some("beta"));
                browser.search = "does not exist".into();
                let output = ui_frame(&ctx, &mut browser, &imports, size, &mut time, vec![]);
                visible(&output, "No models match this search.");
                let point = visible(&output, "Clear filters").center();
                for pressed in [true, false] {
                    ui_frame(
                        &ctx,
                        &mut browser,
                        &imports,
                        size,
                        &mut time,
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
                assert!(browser.search.is_empty());
                assert_eq!(browser.selected_id(), Some("beta"));
                let output = ui_frame(&ctx, &mut browser, &imports, size, &mut time, vec![]);
                visible(&output, "Fit model");
                visible(&output, "Reset view");
            }
            browser.destroy();
            imports.destroy();
        });
        let _ = sender.send(result);
    });
    if let Err(panic) = receiver
        .recv_timeout(Duration::from_secs(8))
        .expect("Browser UI test exceeded eight seconds")
    {
        std::panic::resume_unwind(panic);
    }
}
