//! Actual accepted Response replay through the production captain workspace.
//! Fixture orders stay disabled; this is UI evidence, not normal world admission.
use super::*;
use sha2::{Digest, Sha256};

unsafe fn pointer(
    app: &mut CockpitCapture,
    gl: &glow::Context,
    target: &CachedFeed,
    response: &arkline_protocol::Response,
    path: &std::path::Path,
    labels: &[(String, egui::Pos2)],
    text: &str,
) {
    let pos = labels
        .iter()
        .find(|(s, _)| s == text)
        .unwrap_or_else(|| panic!("Fully visible {text} missing: {labels:?}"))
        .1;
    for pressed in [true, false] {
        app.capture_input(
            gl,
            target,
            response.clone(),
            path,
            Some(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]),
        );
    }
}

#[test]
#[ignore = "Opt-in actual accepted cap7 Responses and native software EGL capture"]
fn native_carrier_inventory_captain_workspace() {
    let renderer_source = std::env::var("ARKLINE_CAPTURE_SOURCE")
        .expect("set the exact renderer commit before starting native capture");
    assert!(
        renderer_source.len() == 40 && renderer_source.bytes().all(|c| c.is_ascii_hexdigit()),
        "ARKLINE_CAPTURE_SOURCE must be a full commit hash"
    );
    let input =
        std::path::PathBuf::from(std::env::var_os("ARKLINE_CARRIER_PUBLIC_RESPONSES").unwrap());
    let output = std::path::PathBuf::from(std::env::var_os("ARKLINE_CARRIER_UI_CAPTURE").unwrap());
    std::fs::create_dir(&output).unwrap();
    let receipt_bytes = std::fs::read(input.join("receipt.json")).unwrap();
    let receipt: serde_json::Value = serde_json::from_slice(&receipt_bytes).unwrap();
    let rows = receipt["frames"].as_array().unwrap();
    let on_foot = std::env::var("ARKLINE_CARRIER_ONFOOT").as_deref() == Ok("1");
    let name = if on_foot {
        assert_eq!(
            hex::encode(Sha256::digest(&receipt_bytes)),
            "7c3729e6e622309c0f20951c38ddb7b07e9f9e8c9cfb143f1c761a856bb687b5"
        );
        assert_eq!(
            receipt["producer_commit"],
            "847b20d2bb1f7c85815faea64f604161b0febee8"
        );
        assert_eq!(receipt["result"], "passed");
        assert_eq!(receipt["sample_count"], 31);
        "Pilot"
    } else {
        receipt["pilot"].as_str().unwrap()
    };
    let indices: Vec<usize> = std::env::var("ARKLINE_CARRIER_UI_INDICES")
        .unwrap_or_else(|_| "0,85,127,155,271".into())
        .split(',')
        .map(|i| i.parse().unwrap())
        .collect();
    let widths: Vec<i32> = std::env::var("ARKLINE_CARRIER_UI_WIDTHS")
        .unwrap_or_else(|_| "900,1280".into())
        .split(',')
        .map(|i| i.parse().unwrap())
        .collect();
    let mut captures = Vec::new();
    unsafe {
        let (_egl, gl) = crate::flight3d::exterior_gpu_tests::Egl::create();
        for width in widths {
            let height = if width == 900 { 650 } else { 800 };
            let target = CachedFeed::create(&gl, 98700 + width as u64, width, height).unwrap();
            for index in &indices {
                let row = rows
                    .iter()
                    .find(|row| row["index"].as_u64() == Some(*index as u64))
                    .unwrap_or_else(|| &rows[*index]);
                let bytes = std::fs::read(input.join(row["file"].as_str().unwrap())).unwrap();
                assert_eq!(hex::encode(Sha256::digest(&bytes)), row["response_sha256"]);
                let recorded: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                let wrapped = recorded.get("response").is_some();
                if wrapped {
                    assert_eq!(
                        hex::encode(Sha256::digest(&receipt_bytes)),
                        std::env::var("ARKLINE_CARRIER_EXPECTED_RECEIPT_SHA256").unwrap_or_else(
                            |_| "5d92db8b549bde42b591934c7c92951ec157c7f7530d7a41646108f9a556c022"
                                .into()
                        )
                    );
                    let producer = std::env::var("ARKLINE_CARRIER_EXPECTED_PRODUCER")
                        .unwrap_or_else(|_| "847b20d2bb1f7c85815faea64f604161b0febee8".into());
                    assert_eq!(receipt["producer_commit"], producer);
                    assert_eq!(recorded["producer_commit"], producer);
                    assert_eq!(recorded["pilot"], name);
                    assert_eq!(recorded["format_version"], 1);
                }
                let response: arkline_protocol::Response = serde_json::from_value(if wrapped {
                    recorded["response"].clone()
                } else {
                    recorded.clone()
                })
                .unwrap();
                let snapshot = response.snapshot.as_ref().unwrap();
                let pilot = snapshot.pilots.iter().find(|p| p.name == name).unwrap();
                let public = pilot.carrier_inventory.as_ref().unwrap();
                assert_eq!(public.sample_time_ms, snapshot.server_time_ms);
                assert_eq!(
                    public.validate_projection(pilot),
                    Ok(arkline_protocol::CarrierInventoryAdmission::StationaryFixture)
                );
                if wrapped {
                    assert_eq!(
                        recorded["binding"],
                        serde_json::to_value(public.binding()).unwrap()
                    );
                    assert_eq!(recorded["sample_time_ms"], public.sample_time_ms);
                    assert_eq!(public.binding().registration.assembly_revision, 1703);
                    assert_eq!(
                        public.binding().registration.assembly_source_sha256,
                        "79ab43221861f74383ca36ca6a742ff5588e08003eb7b10f9d8b6f0d5469664a"
                    );
                    assert_eq!(
                        snapshot.carrier_inventory_registration.as_ref(),
                        Some(&public.binding().registration)
                    );
                    let projected = serde_json::to_value(public).unwrap();
                    for key in [
                        "parent",
                        "inventory",
                        "ore",
                        "drone_poses",
                        "cassette_poses",
                        "machinery",
                    ] {
                        assert_eq!(
                            recorded[key], projected[key],
                            "actual wrapper/projection {key}"
                        );
                    }
                    // The documentary wrapper records compact private hardware
                    // runs, not the public route/source/departure wire type.
                    let runs = recorded["motions"].as_array().unwrap();
                    let hardware: Vec<_> = public
                        .motions
                        .iter()
                        .filter(|m| {
                            matches!(
                                m.route,
                                arkline_protocol::CarrierPresentationRoute::Hardware { .. }
                            )
                        })
                        .collect();
                    assert_eq!(runs.len(), hardware.len());
                    let mut ids = std::collections::BTreeSet::new();
                    for run in runs {
                        let id = run["operation_id"].as_u64().unwrap();
                        assert!(ids.insert(id), "duplicate recorded physical run");
                        let motion = hardware.iter().find(|m| m.operation_id == id).unwrap();
                        assert_eq!(run["drone_id"], motion.drone_id);
                        assert_eq!(run["confirmed_at_ms"], motion.confirmed_at_ms);
                        assert_eq!(run["held"], motion.held);
                        let mut drone = None;
                        let mut cassettes = Vec::new();
                        let mut mechanisms = Vec::new();
                        for (key, pose) in &motion.confirmed.poses {
                            match key {
                                arkline_protocol::CarrierPoseKey::Drone => {
                                    assert!(drone.replace(*pose).is_none());
                                }
                                arkline_protocol::CarrierPoseKey::Cassette { cassette_id } => {
                                    cassettes.push((*cassette_id, *pose))
                                }
                                arkline_protocol::CarrierPoseKey::Mechanism { semantic_id } => {
                                    mechanisms.push((semantic_id.clone(), *pose))
                                }
                            }
                        }
                        assert_eq!(run["drone_pose"], serde_json::to_value(drone).unwrap());
                        assert_eq!(
                            run["cassette_poses"],
                            serde_json::to_value(cassettes).unwrap()
                        );
                        assert_eq!(run["machinery"], serde_json::to_value(mechanisms).unwrap());
                    }
                    assert!(public
                        .cameras
                        .as_ref()
                        .is_some_and(|c| c.devices.len() == 10));
                }
                if on_foot {
                    assert!(!wrapped, "on-foot evidence uses exact raw Responses");
                    let avatar = pilot.station_avatar.as_ref().expect("actual owner on foot");
                    let traversal = avatar.docked_traversal.as_ref().unwrap();
                    assert!(
                        !traversal.moving,
                        "settled after real walking, not a mid-stride claim"
                    );
                    assert_eq!(public.binding().registration.assembly_revision, 1703);
                    assert_eq!(
                        public.binding().registration.assembly_source_sha256,
                        "79ab43221861f74383ca36ca6a742ff5588e08003eb7b10f9d8b6f0d5469664a"
                    );
                    assert_eq!(
                        snapshot.carrier_inventory_registration.as_ref(),
                        Some(&public.binding().registration)
                    );
                    assert!(public
                        .cameras
                        .as_ref()
                        .is_some_and(|c| c.devices.len() == 10));
                    assert!(public.inventory.deployments.iter().any(|b| b.batch_id == 1));
                }
                let mounted = wrapped || on_foot;
                let selected = public
                    .inventory
                    .operations
                    .last()
                    .map(|o| o.drone_id)
                    .unwrap_or_else(|| public.inventory.entries.first().unwrap().drone_id);
                let mut app = CockpitCapture::new_for_pilot(gl.clone(), response.clone(), name);
                app.freeze_presentation = false;
                if on_foot {
                    // Exercise the supported standalone client entry route. This
                    // is not a claim to render the separate editor toolbar.
                    app.cockpit.embedded = false;
                    app.cockpit.identity_collapsed = false;
                    assert!(!app.cockpit.can_fly());
                } else {
                    app.cockpit.services_open = true;
                    app.cockpit.service_tab = crate::ServiceTab::Drones;
                }
                app.cockpit.selected_drone = Some(selected);
                let camera = app.cockpit.camera_source;
                let file = format!("carrier-{width}-{index:04}.png");
                let path = output.join(&file);
                let mut labels = Vec::new();
                for _ in 0..18 {
                    labels = app.capture_input(&gl, &target, response.clone(), &path, Some(vec![]));
                }
                if on_foot {
                    assert!(!app.cockpit.services_open);
                    pointer(
                        &mut app,
                        &gl,
                        &target,
                        &response,
                        &path,
                        &labels,
                        "Operations",
                    );
                    for _ in 0..18 {
                        labels =
                            app.capture_input(&gl, &target, response.clone(), &path, Some(vec![]));
                    }
                    pointer(&mut app, &gl, &target, &response, &path, &labels, "Drones");
                    for _ in 0..18 {
                        labels =
                            app.capture_input(&gl, &target, response.clone(), &path, Some(vec![]));
                    }
                    assert!(app.cockpit.service_tab == crate::ServiceTab::Drones);
                    assert!(
                        !app.cockpit.can_fly(),
                        "read-only feed does not grant seated control"
                    );
                    assert_eq!(
                        serde_json::to_value(&app.cockpit.snapshot).unwrap(),
                        serde_json::to_value(&response.snapshot).unwrap()
                    );
                }
                for wanted in [
                    "PORT",
                    "STARBOARD",
                    "Hide monitor",
                    "Next IDs",
                    "Back to Flight",
                ] {
                    assert!(
                        labels.iter().any(|(s, _)| s == wanted),
                        "visible {wanted} at {width}: {labels:?}"
                    );
                }
                assert!(
                    labels.iter().any(|(s, _)| s.contains("INSPECTION ONLY")),
                    "No playable source claim"
                );
                if mounted {
                    assert!(
                        labels.iter().any(|(s, _)| s == "Crane port 0"),
                        "actual selected device visible"
                    );
                    assert!(
                        labels
                            .iter()
                            .any(|(s, _)| s.contains("Mounted crane camera")),
                        "whole feed caption visible: {labels:?}"
                    );
                    let renderer = app
                        .cockpit
                        .flight_view
                        .renderer
                        .as_ref()
                        .unwrap()
                        .lock()
                        .unwrap();
                    let key = renderer
                        .workspace_feed_key
                        .expect("actual mounted feed cache");
                    assert!(
                        renderer
                            .feed_cache
                            .iter()
                            .any(|f| f.key == key && f.captured_ui_ms.is_some()),
                        "actual GL feed must be captured, not just a label"
                    );
                } else {
                    assert!(
                        labels
                            .iter()
                            .any(|(s, _)| s.contains("Carrier feed unavailable")),
                        "No substitute legacy feed"
                    );
                }
                assert!(
                    labels
                        .iter()
                        .filter(|(s, _)| s.starts_with('#') && s.contains("HP"))
                        .count()
                        >= 5,
                    "five actual visible IDs at{width}: {labels:?}"
                );
                assert!(
                    labels
                        .iter()
                        .any(|(s, _)| s.starts_with(&format!("#{selected} ·"))),
                    "selected actual status visible: {labels:?}"
                );
                assert!(
                    app._requests
                        .try_iter()
                        .all(|r| matches!(r, Request::Snapshot)),
                    "painting cannot order gameplay"
                );
                captures.push(serde_json::json!({"file":file,"png_sha256":hex::encode(Sha256::digest(std::fs::read(&path).unwrap())),"authority_sha256":row["response_sha256"],"authority_index":index,"viewport":[width,height],"selected_id":selected,"location":public.inventory.entry(selected).unwrap().location,"normal_commands_enabled":false,"on_foot":on_foot,"monitor":if mounted { "actual source-mounted crane feed" } else { "explicitly unavailable pending carrier renderer integration" }}));
                if *index == indices[0] || (mounted && *index == 350) {
                    let read_only = output.join(format!("carrier-{width}-{index:04}-pointer.png"));
                    let mut labels = labels;
                    if mounted {
                        let before_key = app
                            .cockpit
                            .flight_view
                            .renderer
                            .as_ref()
                            .unwrap()
                            .lock()
                            .unwrap()
                            .workspace_feed_key;
                        pointer(
                            &mut app,
                            &gl,
                            &target,
                            &response,
                            &read_only,
                            &labels,
                            "Crane port 0",
                        );
                        let menu = app.capture_input(
                            &gl,
                            &target,
                            response.clone(),
                            &read_only,
                            Some(vec![]),
                        );
                        pointer(
                            &mut app,
                            &gl,
                            &target,
                            &response,
                            &read_only,
                            &menu,
                            "Crane port 1",
                        );
                        for _ in 0..4 {
                            labels = app.capture_input(
                                &gl,
                                &target,
                                response.clone(),
                                &read_only,
                                Some(vec![]),
                            );
                        }
                        assert!(labels.iter().any(|(s, _)| s == "Crane port 1"));
                        let after_key = app
                            .cockpit
                            .flight_view
                            .renderer
                            .as_ref()
                            .unwrap()
                            .lock()
                            .unwrap()
                            .workspace_feed_key;
                        assert!(
                            after_key.is_some() && after_key != before_key,
                            "selected real device changes capture identity"
                        );
                        let detail_path =
                            output.join(format!("carrier-{width}-{index:04}-camera-details.png"));
                        let caption = labels
                            .iter()
                            .find(|(s, _)| s.contains("Mounted crane camera"))
                            .unwrap()
                            .1;
                        app.capture_input(
                            &gl,
                            &target,
                            response.clone(),
                            &detail_path,
                            Some(vec![egui::Event::PointerMoved(caption)]),
                        );
                        let mut detail_labels = Vec::new();
                        // Real hover delay on the normal frame clock, not a direct handler.
                        for _ in 0..45 {
                            detail_labels = app.capture_input(
                                &gl,
                                &target,
                                response.clone(),
                                &detail_path,
                                Some(vec![]),
                            );
                        }
                        assert!(detail_labels.iter().any(|(s,_)| s == "Unmetered · health not simulated"),
                            "explicit supply/health limitation reachable by real hover: {detail_labels:?}");
                        captures.push(serde_json::json!({"file":detail_path.file_name().unwrap().to_str().unwrap(),
                            "png_sha256":hex::encode(Sha256::digest(std::fs::read(&detail_path).unwrap())),
                            "authority_sha256":row["response_sha256"],"authority_index":index,"viewport":[width,height],
                            "actual_pointer":"hover mounted-camera primary caption","unmetered_health_details_visible":true}));
                    }
                    pointer(
                        &mut app,
                        &gl,
                        &target,
                        &response,
                        &read_only,
                        &labels,
                        "Hide monitor",
                    );
                    let hidden =
                        app.capture_input(&gl, &target, response.clone(), &read_only, Some(vec![]));
                    assert!(!hidden.iter().any(|(s, _)| s.contains(if mounted {
                        "Mounted crane camera"
                    } else {
                        "Carrier feed unavailable"
                    })));
                    if mounted {
                        assert!(
                            app.cockpit
                                .flight_view
                                .renderer
                                .as_ref()
                                .unwrap()
                                .lock()
                                .unwrap()
                                .workspace_feed_key
                                .is_none(),
                            "hiding retires only the workspace image"
                        );
                    }
                    pointer(
                        &mut app,
                        &gl,
                        &target,
                        &response,
                        &read_only,
                        &hidden,
                        "Hide monitor",
                    );
                    for _ in 0..3 {
                        let current = app.capture_input(
                            &gl,
                            &target,
                            response.clone(),
                            &read_only,
                            Some(vec![]),
                        );
                        pointer(
                            &mut app, &gl, &target, &response, &read_only, &current, "Next IDs",
                        );
                    }
                    let last =
                        app.capture_input(&gl, &target, response.clone(), &read_only, Some(vec![]));
                    assert!(
                        last.iter().any(|(s, _)| s.starts_with("#20 M")),
                        "all20IDs reachable"
                    );
                    let label = last
                        .iter()
                        .find(|(s, _)| s.starts_with("#20 M"))
                        .unwrap()
                        .0
                        .clone();
                    pointer(&mut app, &gl, &target, &response, &read_only, &last, &label);
                    assert_eq!(app.cockpit.selected_drone, Some(20));
                    let last =
                        app.capture_input(&gl, &target, response.clone(), &read_only, Some(vec![]));
                    assert_eq!(app.cockpit.camera_source, camera);
                    assert_eq!(serde_json::to_value(&app.cockpit.snapshot).unwrap(),
                        serde_json::to_value(&response.snapshot).unwrap(),
                        "camera/hide/paging must preserve the exact accepted jobs and operation clocks");
                    assert!(
                        app._requests
                            .try_iter()
                            .all(|r| matches!(r, Request::Snapshot)),
                        "view actions cannot cancel jobs"
                    );
                    captures.push(serde_json::json!({"file":read_only.file_name().unwrap().to_str().unwrap(),"png_sha256":hex::encode(Sha256::digest(std::fs::read(&read_only).unwrap())),"authority_sha256":row["response_sha256"],"authority_index":index,"viewport":[width,height],"selected_id":20,"actual_pointer":"select another actual camera when available, hide/reopen monitor, page through finite roster, select20","captain_camera_unchanged":true,"no_gameplay_requests":true}));
                    let back = output.join(format!("carrier-{width}-{index:04}-back.png"));
                    pointer(
                        &mut app,
                        &gl,
                        &target,
                        &response,
                        &back,
                        &last,
                        "Back to Flight",
                    );
                    assert!(!app.cockpit.services_open);
                    if on_foot {
                        assert!(
                            !app.cockpit.identity_collapsed,
                            "expanded user preference remains intact"
                        );
                        assert!(
                            !app.cockpit.identity_status_collapsed(),
                            "Back restores the expanded status presentation"
                        );
                        let mut current = Vec::new();
                        for _ in 0..6 {
                            current = app.capture_input(
                                &gl,
                                &target,
                                response.clone(),
                                &back,
                                Some(vec![]),
                            );
                        }
                        assert!(current.iter().any(|(s, _)| s == "Hide status"));
                        pointer(
                            &mut app,
                            &gl,
                            &target,
                            &response,
                            &back,
                            &current,
                            "Operations",
                        );
                        let reopened =
                            output.join(format!("carrier-{width}-{index:04}-reopened.png"));
                        for _ in 0..6 {
                            current = app.capture_input(
                                &gl,
                                &target,
                                response.clone(),
                                &reopened,
                                Some(vec![]),
                            );
                        }
                        pointer(
                            &mut app, &gl, &target, &response, &reopened, &current, "Drones",
                        );
                        for _ in 0..6 {
                            current = app.capture_input(
                                &gl,
                                &target,
                                response.clone(),
                                &reopened,
                                Some(vec![]),
                            );
                        }
                        assert!(app.cockpit.identity_status_collapsed());
                        assert!(!app.cockpit.identity_collapsed);
                        assert!(current.iter().any(|(s, _)| s == "Show status"));
                        assert!(current
                            .iter()
                            .any(|(s, _)| s.contains("Mounted crane camera")));
                        let window = app
                            .ctx
                            .memory(|m| {
                                m.area_rect(
                                    egui::Id::new("cockpit_services_overlay")
                                        .with(app.cockpit.layout.generation),
                                )
                            })
                            .unwrap();
                        let point = egui::pos2(window.right() - 14., window.top() + 18.);
                        let closed = output.join(format!("carrier-{width}-{index:04}-closed.png"));
                        for pressed in [true, false] {
                            app.capture_input(
                                &gl,
                                &target,
                                response.clone(),
                                &closed,
                                Some(vec![
                                    egui::Event::PointerMoved(point),
                                    egui::Event::PointerButton {
                                        pos: point,
                                        button: egui::PointerButton::Primary,
                                        pressed,
                                        modifiers: egui::Modifiers::NONE,
                                    },
                                ]),
                            );
                        }
                        assert!(!app.cockpit.services_open, "real title Close must dismiss");
                        for _ in 0..6 {
                            current = app.capture_input(
                                &gl,
                                &target,
                                response.clone(),
                                &closed,
                                Some(vec![]),
                            );
                        }
                        assert!(!app.cockpit.identity_status_collapsed());
                        assert!(!app.cockpit.identity_collapsed);
                        assert!(current.iter().any(|(s, _)| s == "Hide status"));
                        assert_eq!(
                            serde_json::to_value(&app.cockpit.snapshot).unwrap(),
                            serde_json::to_value(&response.snapshot).unwrap()
                        );
                        for image in [&back, &reopened, &closed] {
                            captures.push(serde_json::json!({"file":image.file_name().unwrap().to_str().unwrap(),"png_sha256":hex::encode(Sha256::digest(std::fs::read(image).unwrap())),"authority_sha256":row["response_sha256"],"authority_index":index,"viewport":[width,height],"actual_pointer":"Back restores expanded preference; Operations/Drones reopen compacts; title Close restores expanded preference", "accepted_snapshot_unchanged":true}));
                        }
                    }
                    assert_eq!(app.cockpit.camera_source, camera);
                    assert!(app
                        ._requests
                        .try_iter()
                        .all(|r| matches!(r, Request::Snapshot)));
                }
                app.painter.destroy();
            }
            target.destroy(&gl);
        }
    }
    std::fs::write(output.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"renderer_source":renderer_source,"authority_producer":receipt["producer_commit"],"source_receipt_sha256":hex::encode(Sha256::digest(receipt_bytes)),"on_foot_standalone_route":on_foot,"scope":"Actual accepted cap7 Response replay in production captain workspace; stationary inspection-only. Mounted-feed mode uses exact actual camera projections and selected GL capture; legacy mode remains explicitly unavailable. No normal command admission, continuous motion, full20 completion or hardware-performance claim.","captures":captures})).unwrap()).unwrap();
}
