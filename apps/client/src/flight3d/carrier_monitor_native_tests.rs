//! Explicit GPU gate using immutable actual authority Response envelopes.
use super::*;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[test]
#[ignore = "Explicit EGL + exact actual1703 camera Response receipt; writes native inspection evidence"]
fn native_actual_carrier_camera_response_feed() {
    let directory = PathBuf::from(
        std::env::var("ARKLINE_MOUNTED_CAMERA_RESPONSES")
            .expect("actual Response receipt directory"),
    );
    let output = PathBuf::from(
        std::env::var("ARKLINE_MOUNTED_CAMERA_OUTPUT").expect("isolated evidence output directory"),
    );
    assert!(output.is_dir());
    let receipt_bytes = std::fs::read(directory.join("receipt.json")).unwrap();
    let receipt: serde_json::Value = serde_json::from_slice(&receipt_bytes).unwrap();
    let pilot_name = receipt["pilot"].as_str().unwrap();
    let rows = receipt["frames"].as_array().unwrap();
    assert!(!rows.is_empty());
    let selected = "pioneer.carrier.camera.loader.port.0";
    let mut state = State::default();
    let mut observations = Vec::new();
    let mut parent_positions = std::collections::BTreeSet::new();
    let mut active_emissions = std::collections::BTreeSet::new();
    unsafe {
        let (_egl, gl) = super::super::exterior_gpu_tests::Egl::create();
        let mut renderer = Renderer::new(gl.clone()).unwrap();
        let target = CachedFeed::create(&gl, u64::MAX - 1, 640, 360).unwrap();
        let info = || egui::PaintCallbackInfo {
            viewport: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640., 360.)),
            clip_rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640., 360.)),
            pixels_per_point: 1.,
            screen_size_px: [640, 360],
        };
        let mut previous_key = None;
        for (index, row) in rows.iter().enumerate() {
            let path = row["file"].as_str().unwrap();
            assert!(!path.contains("..") && !path.starts_with('/'));
            let bytes = std::fs::read(directory.join(path)).unwrap();
            let hash = hex::encode(Sha256::digest(&bytes));
            assert_eq!(Some(hash.as_str()), row["response_sha256"].as_str());
            let envelope: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(envelope["producer_commit"], receipt["producer_commit"]);
            let response: arkline_protocol::Response =
                serde_json::from_value(envelope["response"].clone()).unwrap();
            let snapshot = response.snapshot.as_ref().unwrap();
            let before = serde_json::to_vec(snapshot).unwrap();
            let pilot = snapshot
                .pilots
                .iter()
                .find(|p| p.name == pilot_name)
                .unwrap();
            let public = pilot.carrier_inventory.as_ref().unwrap();
            let warning = crate::carrier_warning_adapter::visuals(pilot, public);
            let geometry = flight_geometry().unwrap();
            let catalog = geometry
                .carriers
                .iter()
                .filter_map(|c| c.as_ref().ok())
                .find(|c| c.registration == public.binding().registration)
                .unwrap();
            assert_eq!(catalog.warning_beacons.len(), 10);
            let body = carrier_body_parts(pilot, snapshot.server_time_ms).unwrap();
            assert_eq!(body.len(), catalog.parts.len());
            for &(mesh, cell) in &catalog.warning_beacons {
                let visual = warning[cell].expect(
                    "actual validated camera bus and source door travel must resolve beacon",
                );
                let part = body.iter().find(|part| part.0 == mesh).unwrap();
                assert_eq!(part.2, Vec3::from_array(visual.color));
                assert_eq!(part.3, visual.emission);
            }
            for &(mesh, _, color, emission) in catalog.parts.iter() {
                if !catalog
                    .warning_beacons
                    .iter()
                    .any(|(slot, _)| *slot == mesh)
                {
                    let part = body.iter().find(|part| part.0 == mesh).unwrap();
                    assert_eq!(
                        (part.2, part.3),
                        (color, emission),
                        "warning must not flash unrelated hardware"
                    );
                }
            }
            let mut operating_cells = Vec::new();
            for motion in &public.motions {
                if let arkline_protocol::CarrierPresentationRoute::Hardware {
                    route:
                        arkline_protocol::CarrierRouteKind::Stage {
                            destination_berth, ..
                        },
                    ..
                } = motion.route
                {
                    assert!(
                        !motion.held,
                        "this actual prefix is qualified as unheld Stage"
                    );
                    let cell = usize::from(destination_berth);
                    assert_eq!(
                        warning[cell].unwrap().mode,
                        crate::carrier_warning_geometry::WarningMode::Operating
                    );
                    active_emissions.insert(warning[cell].unwrap().emission.to_bits());
                    operating_cells.push(cell);
                }
            }
            if matches!(row["elapsed_ms"].as_u64(), Some(35_000 | 40_000 | 45_000)) {
                operating_cells.sort_unstable();
                assert_eq!(
                    operating_cells,
                    (0..10).collect::<Vec<_>>(),
                    "actual Stage landmarks must drive all ten beacons"
                );
            }
            let device = public
                .cameras
                .as_ref()
                .unwrap()
                .devices
                .iter()
                .find(|d| d.device_id == selected)
                .unwrap();
            assert_eq!(
                device.availability,
                arkline_protocol::carrier_camera_devices::CameraAvailability::Available
            );
            let parent = device.parent_world_pose.unwrap();
            parent_positions.insert(parent.position_m.map(f64::to_bits));
            let (frame, key) = resolve_frame(
                &mut state,
                snapshot,
                pilot_name,
                snapshot.server_time_ms,
                true,
                false,
                selected,
                true,
                [640, 360],
                Default::default(),
            )
            .unwrap()
            .unwrap();
            assert_eq!(frame.now, snapshot.server_time_ms);
            assert_eq!(frame.snapshot_time, snapshot.server_time_ms);
            assert!(!frame.cockpit && frame.drone_camera && frame.camera_drone.is_none());
            if let Some(old) = previous_key {
                assert_eq!(key, old, "motion must reuse one cache allocation");
            }
            previous_key = Some(key);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            gl.viewport(0, 0, 640, 360);
            flush_pending(&mut renderer, &state.pending);
            renderer.prepare_workspace_feed(key);
            renderer.feed_source_clocks = vec![(key, frame.now)];
            renderer.paint_cached_feed_on_surface(
                &frame,
                key,
                info(),
                1,
                Some(target.framebuffer),
                None,
                Some((index as u64, 1000 + index as u64 * 100, &[key])),
            );
            assert_eq!(renderer.feed_cache.len(), 1);
            assert_eq!(renderer.feed_cache[0].painted_ms, Some(frame.now));
            assert!(renderer.feed_cache.len() <= MAX_CAMERA_CAPTURE_SOURCES);
            let mut pixels = vec![0; 640 * 360 * 4];
            gl.read_pixels(
                0,
                0,
                640,
                360,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            assert_eq!(gl.get_error(), glow::NO_ERROR);
            let distinct = pixels
                .chunks_exact(4)
                .map(|p| [p[0], p[1], p[2]])
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            assert!(distinct > 8, "blank or uniform native image");
            if index == 0 || index + 1 == rows.len() {
                let mut flipped = pixels.clone();
                for y in 0..360 {
                    flipped[y * 640 * 4..(y + 1) * 640 * 4]
                        .copy_from_slice(&pixels[(359 - y) * 640 * 4..(360 - y) * 640 * 4]);
                }
                image::save_buffer(
                    output.join(format!("mounted-crane-{index:05}-NATIVE.png")),
                    &flipped,
                    640,
                    360,
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
            assert_eq!(
                serde_json::to_vec(snapshot).unwrap(),
                before,
                "view must not mutate gameplay"
            );
            observations.push(serde_json::json!({"file":path,"response_sha256":hash,"sample_ms":frame.now,"device":selected,"parent_world":parent,"eye_world_m":frame.space.world(frame.eye),"capture_key":key,"cache_entries":renderer.feed_cache.len(),"distinct_rgb":distinct,"actual_operating_beacon_cells":operating_cells,"beacon_catalog_entries":catalog.warning_beacons.len(),"actual_beacon_emissions":warning.iter().map(|v|v.map(|v|v.emission)).collect::<Vec<_>>()}));
            if index + 1 == rows.len() {
                assert!(resolve_frame(
                    &mut state,
                    snapshot,
                    pilot_name,
                    snapshot.server_time_ms,
                    true,
                    false,
                    selected,
                    false,
                    [640, 360],
                    Default::default()
                )
                .unwrap()
                .is_none());
                flush_pending(&mut renderer, &state.pending);
                assert!(renderer.feed_cache.is_empty());
                let (reopened, new_key) = resolve_frame(
                    &mut state,
                    snapshot,
                    pilot_name,
                    snapshot.server_time_ms,
                    true,
                    false,
                    selected,
                    true,
                    [640, 360],
                    Default::default(),
                )
                .unwrap()
                .unwrap();
                assert_ne!(new_key, key);
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
                flush_pending(&mut renderer, &state.pending);
                renderer.prepare_workspace_feed(new_key);
                renderer.feed_source_clocks = vec![(new_key, reopened.now)];
                renderer.paint_cached_feed_on_surface(
                    &reopened,
                    new_key,
                    info(),
                    1,
                    Some(target.framebuffer),
                    None,
                    Some((10000, 100000, &[new_key])),
                );
                assert_eq!(renderer.feed_cache.len(), 1);
                assert!(resolve_frame(
                    &mut state,
                    snapshot,
                    pilot_name,
                    snapshot.server_time_ms,
                    false,
                    false,
                    selected,
                    true,
                    [640, 360],
                    Default::default()
                )
                .is_err());
                flush_pending(&mut renderer, &state.pending);
                assert!(renderer.feed_cache.is_empty());
            }
        }
        renderer.destroy();
        target.destroy(&gl);
    }
    let moving = parent_positions.len() > 1;
    let report = serde_json::json!({"qualification":"Actual powered mounted-device native source feed via existing Renderer/cache; sampled Response landmarks, not complete operation coverage or network video","input_receipt_sha256":hex::encode(Sha256::digest(&receipt_bytes)),"producer_commit":receipt["producer_commit"],"actual_parent_motion_observed":moving,"distinct_parent_positions":parent_positions.len(),"distinct_active_beacon_intensities":active_emissions.len(),"hidden_reopen_and_disconnect_passed":true,"unchanged_gameplay_snapshots":true,"observations":observations});
    std::fs::write(
        output.join("mounted-native-receipt.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    if std::env::var_os("ARKLINE_CAMERA_REQUIRE_MOVING").is_some() {
        assert!(
            moving,
            "actual receipt contains no moving-parent landmark; do not invent motion"
        );
    }
    if std::env::var_os("ARKLINE_WARNING_REQUIRE_PULSE").is_some() {
        assert!(
            active_emissions.len() > 1,
            "actual receipt does not span multiple beacon pulse intensities; do not invent a clock"
        );
    }
}
