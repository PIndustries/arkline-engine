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
        hierarchy: vec![category.into()],
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
            Source::Snapshot(bytes) => Verified::decode_preview(bytes.as_ref().clone())
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

#[test]
fn same_id_import_availability_refresh_restores_selected_verified_geometry_and_tree() {
    let loads = Arc::new(AtomicUsize::new(0));
    let counted = loads.clone();
    let mut browser = ModelBrowser::with_worker(Vec::new(), move |work| {
        if matches!(work, Work::Load { .. }) {
            counted.fetch_add(1, Ordering::SeqCst);
        }
        answer(work)
    });
    settle(&mut browser);
    let verified = Verified::decode(WEDGE.to_vec()).unwrap();
    let id = format!("import:accepted:7:{}", verified.sha256);
    let mut snapshot = BrowserImportSnapshot {
        id: id.clone(),
        name: "Accepted asset #7 · revision 1".into(),
        source: "Accepted import".into(),
        identity: verified.sha256.clone(),
        notes: vec!["Accepted content saved".into()],
        verified: None,
        unavailable: Some("Accepted geometry unavailable. File is temporarily unavailable.".into()),
    };
    browser.sync_imports(vec![snapshot.clone()]);
    browser.select(browser.imports[0].clone(), false);
    browser.rebuild_tree();
    assert!(matches!(browser.state, LoadState::Unavailable(_)));
    assert!(browser.loaded.is_none());
    let unavailable = find_node(
        &browser.tree.as_ref().unwrap().roots,
        &["Imported snapshots", &snapshot.name],
    );
    assert!(unavailable.entry.as_ref().unwrap().unavailable.is_some());

    // Restoring bytes does not change the accepted content ID or source hash.
    // The cached tree and current selection must nevertheless become available.
    snapshot.verified = Some(verified.clone());
    snapshot.unavailable = None;
    browser.sync_imports(vec![snapshot.clone()]);
    settle(&mut browser);
    browser.rebuild_tree();
    browser.update_preview(None);
    assert_eq!(browser.selected_id(), Some(id.as_str()));
    assert_eq!(browser.state, LoadState::Ready);
    let selected = browser.selected.as_ref().unwrap();
    assert!(selected.unavailable.is_none());
    assert_eq!(selected.identity, verified.sha256);
    assert!(matches!(&selected.model, Source::Verified(_)));
    match browser.loaded.as_ref().unwrap() {
        LoadedModel::Imported(restored) => {
            assert!(Arc::ptr_eq(&restored.geometry, &verified.geometry));
            assert!(Arc::ptr_eq(&restored.bytes, &verified.bytes));
        }
        _ => panic!("Restored import must use the original verified source geometry"),
    }
    let restored = find_node(
        &browser.tree.as_ref().unwrap().roots,
        &["Imported snapshots", &snapshot.name],
    )
    .entry
    .as_ref()
    .unwrap();
    assert_eq!(restored.id, id);
    assert!(restored.unavailable.is_none());
    assert!(matches!(&restored.model, Source::Verified(_)));
    assert_eq!(browser.uploaded_epoch, Some(browser.epoch));
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    browser.destroy();
}

#[test]
fn same_id_import_metadata_refresh_preserves_ready_geometry_cache_and_preview_epoch() {
    let loads = Arc::new(AtomicUsize::new(0));
    let counted = loads.clone();
    let mut browser = ModelBrowser::with_worker(Vec::new(), move |work| {
        if matches!(work, Work::Load { .. }) {
            counted.fetch_add(1, Ordering::SeqCst);
        }
        answer(work)
    });
    settle(&mut browser);
    browser.select(
        entry("cached:other", "Cached fixture", "Project", SPIRE),
        false,
    );
    settle(&mut browser);
    let cached_geometry = match &browser.cache.front().unwrap().1 {
        LoadedModel::Imported(verified) => verified.geometry.clone(),
        _ => panic!("Expected the existing decoded fixture in the cache"),
    };
    let verified = Verified::decode(WEDGE.to_vec()).unwrap();
    let id = format!("import:accepted:7:{}", verified.sha256);
    let mut snapshot = BrowserImportSnapshot {
        id: id.clone(),
        name: "Accepted asset #7 · revision 1".into(),
        source: "Accepted import".into(),
        identity: verified.sha256.clone(),
        notes: vec![
            "Source: fixture.glb".into(),
            "Accepted content unsaved".into(),
        ],
        verified: Some(verified.clone()),
        unavailable: None,
    };
    browser.sync_imports(vec![snapshot.clone()]);
    browser.select(browser.imports[0].clone(), false);
    browser.rebuild_tree();
    assert_eq!(
        find_node(
            &browser.tree.as_ref().unwrap().roots,
            &["Imported snapshots", &snapshot.name],
        )
        .entry
        .as_ref()
        .unwrap()
        .notes,
        snapshot.notes,
    );
    browser.update_preview(None);
    let epoch = browser.epoch;
    let uploaded_epoch = browser.uploaded_epoch;
    assert_eq!(uploaded_epoch, Some(epoch));
    let preview = browser.preview.as_ref().unwrap() as *const ModelPreview;
    let cache_ids = browser
        .cache
        .iter()
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();

    // Saving or editing accepted metadata keeps the same content identity. It
    // should refresh visible metadata without resetting the selected preview.
    snapshot.name = "Accepted asset #7 · revision 2".into();
    snapshot.source = "Accepted import · project".into();
    snapshot.notes = vec![
        "Source: fixture.glb".into(),
        "License: project license".into(),
        "Accepted content saved".into(),
    ];
    browser.sync_imports(vec![snapshot.clone()]);
    browser.rebuild_tree();
    browser.update_preview(None);
    assert_eq!(browser.selected_id(), Some(id.as_str()));
    assert_eq!(browser.state, LoadState::Ready);
    let selected = browser.selected.as_ref().unwrap();
    assert_eq!(selected.name, snapshot.name);
    assert_eq!(selected.source, snapshot.source);
    assert_eq!(selected.identity, verified.sha256);
    assert_eq!(selected.notes, snapshot.notes);
    let tree_entry = find_node(
        &browser.tree.as_ref().unwrap().roots,
        &["Imported snapshots", &snapshot.name],
    )
    .entry
    .as_ref()
    .unwrap();
    assert_eq!(tree_entry.name, snapshot.name);
    assert_eq!(tree_entry.source, snapshot.source);
    assert_eq!(tree_entry.notes, snapshot.notes);
    assert!(matches!(&tree_entry.model, Source::Verified(_)));
    match browser.loaded.as_ref().unwrap() {
        LoadedModel::Imported(current) => {
            assert!(Arc::ptr_eq(&current.geometry, &verified.geometry));
            assert!(Arc::ptr_eq(&current.bytes, &verified.bytes));
        }
        _ => panic!("Metadata refresh must preserve the selected verified geometry"),
    }
    assert_eq!(browser.epoch, epoch);
    assert_eq!(browser.uploaded_epoch, uploaded_epoch);
    assert_eq!(
        browser.preview.as_ref().unwrap() as *const ModelPreview,
        preview
    );
    assert_eq!(
        browser
            .cache
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>(),
        cache_ids,
    );
    match &browser.cache.front().unwrap().1 {
        LoadedModel::Imported(cached) => assert!(Arc::ptr_eq(&cached.geometry, &cached_geometry)),
        _ => panic!("Metadata refresh must retain cached geometry"),
    }
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    browser.destroy();
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
            browser.tree_open.insert(vec!["Models".into()]);
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

#[test]
fn calibration_snapshot_previews_twelve_triangles_but_stage_button_cannot_emit_action() {
    use sha2::{Digest, Sha256};
    const CALIBRATION: &[u8] = include_bytes!("../../../assets/examples/calibration-cube.glb");
    let project = std::env::temp_dir().join(format!(
        "arkline-model-browser-preview-only-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&project).unwrap();
    let mut imports = Imports::open(&project);
    let deadline = Instant::now() + Duration::from_secs(2);
    while imports.saved.is_none() {
        imports.poll();
        assert!(
            Instant::now() < deadline,
            "Initial import baseline did not load"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let before = (imports.index.clone(), imports.saved.clone());
    let mut browser = ModelBrowser::with_worker(Vec::new(), answer);
    settle(&mut browser);
    assert!(browser.open_snapshot("Calibration cube".into(), CALIBRATION.to_vec()));
    settle(&mut browser);
    assert_eq!(browser.state, LoadState::Ready);
    match browser.loaded.as_ref().unwrap() {
        LoadedModel::Imported(verified) => {
            assert_eq!(verified.geometry.report.triangles, 12);
            assert!(verified.geometry.uses_generated_normals());
            assert_eq!(verified.bytes.as_slice(), CALIBRATION);
            assert_eq!(
                verified.sha256,
                format!("{:x}", Sha256::digest(CALIBRATION))
            );
            assert!(verified
                .geometry
                .report
                .notes
                .iter()
                .any(|note| note.contains("generated flat face normals")));
        }
        _ => panic!("Expected the calibration snapshot's actual imported geometry"),
    }
    let ctx = egui::Context::default();
    let mut time = 0.0;
    for size in [egui::vec2(1280.0, 900.0), egui::vec2(800.0, 600.0)] {
        let mut draw = |events| {
            time += 0.02;
            let mut action = None;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    time: Some(time),
                    predicted_dt: 0.02,
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ctx| action = browser.show(ctx, None, &imports),
            );
            (output, action)
        };
        let _ = draw(vec![]);
        let (output, action) = draw(vec![]);
        assert!(action.is_none());
        visible(&output, crate::import_document::PREVIEW_ONLY_REASON);
        visible(&output, "Fit model");
        visible(&output, "Reset view");
        let point = visible(&output, "Stage selected…").center();
        for pressed in [true, false] {
            let (_, action) = draw(vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            assert!(
                action.is_none(),
                "Preview-only geometry must not produce a staging action"
            );
        }
    }
    assert_eq!((imports.index.clone(), imports.saved.clone()), before);
    assert!(imports.accepted.is_none() && imports.staged.is_none());
    assert!(!project.join("Assets").exists());
    browser.destroy();
    imports.destroy();
}

fn nested_entry(id: &str, name: &str, category: &str, parents: &[&str], bytes: &[u8]) -> Entry {
    let mut model = entry(id, name, category, bytes);
    model.hierarchy = parents.iter().map(|label| (*label).into()).collect();
    model
}

fn find_node<'a>(nodes: &'a [tree::Node], path: &[&str]) -> &'a tree::Node {
    let (label, rest) = path.split_first().expect("a tree path is not empty");
    let node = nodes
        .iter()
        .find(|node| node.label == *label)
        .unwrap_or_else(|| panic!("Missing tree path {path:?}"));
    if rest.is_empty() {
        node
    } else {
        find_node(&node.children, rest)
    }
}

fn collect_entries<'a>(nodes: &'a [tree::Node], entries: &mut Vec<&'a Entry>) {
    for node in nodes {
        if let Some(entry) = &node.entry {
            entries.push(entry);
        }
        collect_entries(&node.children, entries);
    }
}

#[test]
fn hierarchy_groups_686_components_under_pioneer_without_losing_provenance() {
    let mut models = Vec::new();
    for object in 0..686 {
        let mut part = nested_entry(
            &format!("pioneer/source-object/{object}"),
            &format!("Hull panel {object:03}"),
            "Ship components",
            &["Ships", "Pioneer", "Source parts", "Hull panels"],
            WEDGE,
        );
        part.identity = format!("assets/ships/pioneer/pioneer.glb#source-object={object}");
        part.source = "Runtime GLB source".into();
        part.notes = vec![
            format!("Source object {object}"),
            format!("Material primitive {} · Hull paint", object % 9),
        ];
        models.push(part);
    }
    // A selectable whole-model node must merge with its parent branch even when
    // the component entries precede that whole model in the source catalog.
    models.push(nested_entry(
        "ship:Pioneer",
        "Pioneer",
        "Ships",
        &["Ships"],
        WEDGE,
    ));
    models.push(nested_entry(
        "drone:Recon",
        "Recon",
        "Drones",
        &["Drones"],
        SPIRE,
    ));
    let tree = tree::Tree::build(&models, "", None);
    assert_eq!(tree.matches, 688);
    assert_eq!(
        tree.roots.len(),
        2,
        "Components belong beneath their ship, not at the root"
    );
    let ships = find_node(&tree.roots, &["Ships"]);
    assert!(ships.entry.is_none());
    assert_eq!(ships.matches, 687);
    let pioneer = find_node(&tree.roots, &["Ships", "Pioneer"]);
    assert_eq!(pioneer.entry.as_ref().unwrap().id, "ship:Pioneer");
    assert_eq!(pioneer.children.len(), 1);
    let parts = find_node(
        &tree.roots,
        &["Ships", "Pioneer", "Source parts", "Hull panels"],
    );
    assert_eq!(parts.matches, 686);
    assert_eq!(parts.children.len(), 686);
    let last = parts
        .children
        .iter()
        .find(|node| {
            node.entry
                .as_ref()
                .is_some_and(|entry| entry.id == "pioneer/source-object/685")
        })
        .unwrap();
    let part = last.entry.as_ref().unwrap();
    assert_eq!(
        last.path,
        vec![
            "Ships",
            "Pioneer",
            "Source parts",
            "Hull panels",
            "Hull panel 685"
        ]
    );
    assert_eq!(
        part.hierarchy,
        vec!["Ships", "Pioneer", "Source parts", "Hull panels"]
    );
    assert_eq!(
        part.identity,
        "assets/ships/pioneer/pioneer.glb#source-object=685"
    );
    assert_eq!(part.source, "Runtime GLB source");
    assert_eq!(
        part.notes,
        vec!["Source object 685", "Material primitive 1 · Hull paint"]
    );
    let mut leaves = Vec::new();
    collect_entries(&tree.roots, &mut leaves);
    assert_eq!(
        leaves
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        models.len()
    );
}

#[test]
fn hierarchy_search_retains_ancestors_and_distinct_same_named_source_parts() {
    let mut first = nested_entry(
        "source-object:4",
        "Hull panel",
        "Ship components",
        &["Ships", "Pioneer", "Components"],
        WEDGE,
    );
    first.identity = "pioneer.glb#source-object=4;material-primitive=4".into();
    first.notes = vec!["Material primitive 4 · Inner coating".into()];
    let mut second = nested_entry(
        "source-object:5",
        "Hull panel",
        "Ship components",
        &["Ships", "Pioneer", "Components"],
        WEDGE,
    );
    second.identity = "pioneer.glb#source-object=5;material-primitive=5".into();
    second.notes = vec!["Material primitive 5 · Outer coating".into()];
    let models = vec![
        first,
        second,
        nested_entry("ship:Pioneer", "Pioneer", "Ships", &["Ships"], WEDGE),
        nested_entry("drone:Recon", "Recon", "Drones", &["Drones"], SPIRE),
    ];
    let matching_ship = tree::Tree::build(&models, "pioneer", None);
    assert_eq!(
        matching_ship.matches, 3,
        "A parent-name search should find its source parts"
    );
    let component_filter = tree::Tree::build(&models, "pioneer", Some("Ship components"));
    assert_eq!(component_filter.matches, 2);
    let pioneer = find_node(&component_filter.roots, &["Ships", "Pioneer"]);
    assert!(
        pioneer.entry.is_none(),
        "Category filtering must not make the excluded whole model selectable"
    );
    let parts = find_node(&component_filter.roots, &["Ships", "Pioneer", "Components"]);
    assert_eq!(
        parts.children.len(),
        2,
        "Equal display names must not collapse different source identities"
    );
    let mut entries = Vec::new();
    collect_entries(&component_filter.roots, &mut entries);
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["source-object:4", "source-object:5"])
    );
    let material = tree::Tree::build(&models, "material-primitive=5", None);
    assert_eq!(material.matches, 1);
    let part = find_node(
        &material.roots,
        &["Ships", "Pioneer", "Components", "Hull panel"],
    );
    assert_eq!(part.entry.as_ref().unwrap().id, "source-object:5");
    assert_eq!(
        part.entry.as_ref().unwrap().notes,
        vec!["Material primitive 5 · Outer coating"]
    );
    assert!(tree::Tree::build(&models, "no such source", None)
        .roots
        .is_empty());
}

#[test]
fn clear_filters_reopens_selected_ancestors_without_reloading_geometry() {
    let loads = Arc::new(AtomicUsize::new(0));
    let counted = loads.clone();
    let mut browser = ModelBrowser::with_worker(Vec::new(), move |work| {
        if matches!(work, Work::Load { .. }) {
            counted.fetch_add(1, Ordering::SeqCst);
        }
        answer(work)
    });
    settle(&mut browser);
    let part = nested_entry(
        "selected:panel",
        "Hull panel",
        "Ship components",
        &["Ships", "Pioneer", "Components"],
        WEDGE,
    );
    browser.runtime = vec![
        part.clone(),
        nested_entry("drone:Recon", "Recon", "Drones", &["Drones"], SPIRE),
    ];
    browser.select(part, false);
    settle(&mut browser);
    let geometry = match browser.loaded.as_ref().unwrap() {
        LoadedModel::Imported(verified) => verified.geometry.clone(),
        _ => panic!("Expected the selected fixture's decoded source geometry"),
    };
    let epoch = browser.epoch;
    browser.search = "  NO SUCH MODEL  ".into();
    browser.category = Some("Drones".into());
    browser.tree_open.clear();
    browser.rebuild_tree();
    assert_eq!(browser.tree.as_ref().unwrap().matches, 0);
    browser.clear_filters();
    browser.rebuild_tree();
    assert!(browser.search.is_empty() && browser.category.is_none());
    for path in [
        vec!["Ships".into()],
        vec!["Ships".into(), "Pioneer".into()],
        vec!["Ships".into(), "Pioneer".into(), "Components".into()],
    ] {
        assert!(
            browser.tree_open.contains(&path),
            "Selected branch should reopen: {path:?}"
        );
    }
    assert_eq!(browser.tree.as_ref().unwrap().matches, 2);
    assert_eq!(browser.selected_id(), Some("selected:panel"));
    assert_eq!(browser.epoch, epoch);
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    match browser.loaded.as_ref().unwrap() {
        LoadedModel::Imported(verified) => assert!(Arc::ptr_eq(&verified.geometry, &geometry)),
        _ => panic!("Clearing filters must preserve the selected geometry"),
    }
}

fn visible_count(output: &egui::FullOutput, label: &str) -> usize {
    output
        .shapes
        .iter()
        .filter(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == label => shape
                .clip_rect
                .contains_rect(text.galley.rect.translate(text.pos.to_vec2())),
            _ => false,
        })
        .count()
}

fn click_browser_label(
    ctx: &egui::Context,
    browser: &mut ModelBrowser,
    imports: &Imports,
    size: egui::Vec2,
    time: &mut f64,
    label: &str,
) {
    let _ = ui_frame(ctx, browser, imports, size, time, vec![]);
    let output = ui_frame(ctx, browser, imports, size, time, vec![]);
    let point = visible(&output, label).center();
    for pressed in [true, false] {
        let _ = ui_frame(
            ctx,
            browser,
            imports,
            size,
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

#[test]
fn real_hierarchy_collapses_expands_and_search_recovers_selected_part_at_compact_sizes() {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| {
            let project = std::env::temp_dir().join(format!(
                "arkline-model-hierarchy-ui-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&project).unwrap();
            let mut imports = Imports::open(&project);
            let loads = Arc::new(AtomicUsize::new(0));
            let counted = loads.clone();
            let mut browser = ModelBrowser::with_worker(Vec::new(), move |work| {
                if matches!(work, Work::Load { .. }) {
                    counted.fetch_add(1, Ordering::SeqCst);
                }
                answer(work)
            });
            settle(&mut browser);
            browser.runtime = vec![
                nested_entry("ship:Pioneer", "Pioneer", "Ships", &["Ships"], WEDGE),
                nested_entry(
                    "part:hull",
                    "Hull panel",
                    "Ship components",
                    &["Ships", "Pioneer", "Components"],
                    WEDGE,
                ),
                nested_entry(
                    "part:engine",
                    "Engine pod",
                    "Ship components",
                    &["Ships", "Pioneer", "Components"],
                    SPIRE,
                ),
            ];
            let ctx = egui::Context::default();
            let mut time = 0.0;
            for size in [egui::vec2(1080.0, 720.0), egui::vec2(800.0, 600.0)] {
                imports.poll();
                browser.tree_open =
                    BTreeSet::from([vec!["Ships".into()], vec!["Ships".into(), "Pioneer".into()]]);
                click_browser_label(&ctx, &mut browser, &imports, size, &mut time, "Components");
                assert!(browser.tree_open.contains(&vec![
                    "Ships".into(),
                    "Pioneer".into(),
                    "Components".into()
                ]));
                click_browser_label(&ctx, &mut browser, &imports, size, &mut time, "Hull panel");
                settle(&mut browser);
                assert_eq!(browser.selected_id(), Some("part:hull"));
                let output = ui_frame(&ctx, &mut browser, &imports, size, &mut time, vec![]);
                assert_eq!(
                    visible_count(&output, "Hull panel"),
                    2,
                    "Selected part appears in the tree and preview heading"
                );
                let loaded = loads.load(Ordering::SeqCst);
                click_browser_label(&ctx, &mut browser, &imports, size, &mut time, "Ships");
                assert!(!browser.tree_open.contains(&vec!["Ships".into()]));
                let output = ui_frame(&ctx, &mut browser, &imports, size, &mut time, vec![]);
                assert_eq!(
                    visible_count(&output, "Hull panel"),
                    1,
                    "Collapsing the tree must preserve the selected preview heading"
                );
                assert_eq!(browser.selected_id(), Some("part:hull"));
                browser.search = "  ENGINE POD  ".into();
                let output = ui_frame(&ctx, &mut browser, &imports, size, &mut time, vec![]);
                visible(&output, "Engine pod");
                assert_eq!(browser.tree.as_ref().unwrap().matches, 1);
                assert_eq!(browser.selected_id(), Some("part:hull"));
                browser.search = "no such part".into();
                let output = ui_frame(&ctx, &mut browser, &imports, size, &mut time, vec![]);
                visible(&output, "No models match this search.");
                click_browser_label(
                    &ctx,
                    &mut browser,
                    &imports,
                    size,
                    &mut time,
                    "Clear filters",
                );
                assert!(browser.search.is_empty());
                assert!(browser.tree_open.contains(&vec![
                    "Ships".into(),
                    "Pioneer".into(),
                    "Components".into()
                ]));
                let output = ui_frame(&ctx, &mut browser, &imports, size, &mut time, vec![]);
                assert_eq!(visible_count(&output, "Hull panel"), 2);
                visible(&output, "Fit model");
                visible(&output, "Reset view");
                assert_eq!(browser.selected_id(), Some("part:hull"));
                assert_eq!(
                    loads.load(Ordering::SeqCst),
                    loaded,
                    "Tree and filter interactions must not reload selected geometry"
                );
            }
            assert!(!project.join("Assets").exists());
            browser.destroy();
            imports.destroy();
        });
        let _ = sender.send(result);
    });
    if let Err(panic) = receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("Hierarchy UI interaction exceeded ten seconds")
    {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn constructor_prefers_whole_pioneer_and_expands_its_source_branch() {
    // Clone real descriptor metadata so this test stays independent of backend
    // representation fields while deliberately putting an unrelated model first.
    let seed = runtime_model_catalog()
        .into_iter()
        .next()
        .expect("Runtime catalog is not empty");
    let mut unrelated = seed.clone();
    unrelated.id = "unrelated:first".into();
    unrelated.name = "Unrelated fixture".into();
    unrelated.category = "Other models".into();
    unrelated.hierarchy = vec!["Other models".into()];
    unrelated.unavailable = None;
    let mut component = seed.clone();
    component.id = "pioneer/source-object/fixture".into();
    component.name = "Hull panel".into();
    component.category = "Ship components".into();
    component.hierarchy = vec![
        "Ships".into(),
        "Pioneer".into(),
        "Source parts".into(),
        "Hull panels".into(),
    ];
    component.unavailable = None;
    let mut pioneer = seed;
    pioneer.id = "ship/pioneer".into();
    pioneer.name = "Pioneer".into();
    pioneer.category = "Ships".into();
    pioneer.hierarchy = vec!["Ships".into()];
    pioneer.unavailable = None;
    let mut browser =
        ModelBrowser::with_worker(vec![unrelated, component, pioneer], |work| match work {
            Work::Scan => Ok(Answer::Scanned(sources::SourceCatalog::default())),
            Work::Load { .. } => Verified::decode_preview(WEDGE.to_vec())
                .map(|verified| Answer::Loaded(LoadedModel::Imported(verified))),
        });
    settle(&mut browser);
    assert_eq!(browser.selected_id(), Some("ship/pioneer"));
    assert_eq!(browser.state, LoadState::Ready);
    for path in [
        vec!["Ships".into()],
        vec!["Ships".into(), "Pioneer".into()],
        vec!["Ships".into(), "Pioneer".into(), "Source parts".into()],
    ] {
        assert!(
            browser.tree_open.contains(&path),
            "Initial Pioneer branch should be expanded: {path:?}"
        );
    }
    assert!(
        !browser.tree_open.contains(&vec![
            "Ships".into(),
            "Pioneer".into(),
            "Source parts".into(),
            "Hull panels".into()
        ]),
        "Individual source-name groups should remain collapsed initially"
    );
    browser.rebuild_tree();
    let pioneer = find_node(&browser.tree.as_ref().unwrap().roots, &["Ships", "Pioneer"]);
    assert_eq!(pioneer.entry.as_ref().unwrap().id, "ship/pioneer");
    assert_eq!(pioneer.matches, 2);
}
