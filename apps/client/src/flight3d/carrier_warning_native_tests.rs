//! External diagnostic camera only: production-rendered actual lamp pulse.
//! The operator viewpoint is fixed; no source hardware/scene pose is edited.
use super::*;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

fn screen_pixel(vp: Mat4, point: Vec3) -> (usize, usize) {
    let ndc = vp.project_point3(point);
    assert!(ndc.x.abs() < 1. && ndc.y.abs() < 1.);
    (
        ((ndc.x + 1.) * 0.5 * 640.).floor() as usize,
        ((ndc.y + 1.) * 0.5 * 360.).floor() as usize,
    )
}
fn pixel_ray(frame: &Frame, x: usize, y: usize) -> Vec3 {
    let ndc = Vec3::new(
        (x as f32 + 0.5) / 640. * 2. - 1.,
        (y as f32 + 0.5) / 360. * 2. - 1.,
        1.,
    );
    (frame.vp.inverse().project_point3(ndc) - frame.eye).normalize()
}
#[test]
#[ignore = "Explicit EGL actual847b beacon sample proof; fixed external diagnostic viewpoint, not mounted feed"]
fn native_actual_amber_beacon_pixels_change_only_on_real_lamp() {
    let input = PathBuf::from(std::env::var("ARKLINE_MOUNTED_CAMERA_RESPONSES").unwrap());
    let output = PathBuf::from(std::env::var("ARKLINE_WARNING_PIXEL_OUTPUT").unwrap());
    assert!(output.is_dir());
    // The runner pins the source checkpoint used for this frozen test binary;
    // runtime git HEAD could instead describe a subsequently edited worktree.
    let renderer_commit = std::env::var("ARKLINE_TESTED_RENDERER_COMMIT")
        .expect("pin the exact compiled renderer commit in the evidence runner");
    assert!(renderer_commit.len() == 40 && renderer_commit.bytes().all(|b| b.is_ascii_hexdigit()));
    let executable_sha256 = hex::encode(Sha256::digest(
        std::fs::read(std::env::current_exe().unwrap()).unwrap(),
    ));
    let receipt_bytes = std::fs::read(input.join("receipt.json")).unwrap();
    let receipt: serde_json::Value = serde_json::from_slice(&receipt_bytes).unwrap();
    let pilot_name = receipt["pilot"].as_str().unwrap();
    let rows = receipt["frames"].as_array().unwrap();
    let mut inputs = Vec::new();
    for row in rows {
        let filename = row["file"].as_str().unwrap();
        assert!(!filename.contains("..") && !filename.starts_with('/'));
        let bytes = std::fs::read(input.join(filename)).unwrap();
        assert_eq!(
            hex::encode(Sha256::digest(&bytes)),
            row["response_sha256"].as_str().unwrap()
        );
        let envelope: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(envelope["producer_commit"], receipt["producer_commit"]);
        let response: arkline_protocol::Response =
            serde_json::from_value(envelope["response"].clone()).unwrap();
        let snapshot = response.snapshot.unwrap();
        let pilot = snapshot
            .pilots
            .iter()
            .find(|p| p.name == pilot_name)
            .unwrap();
        let public = pilot.carrier_inventory.as_ref().unwrap();
        let visual = crate::carrier_warning_adapter::visuals(pilot, public)[0].unwrap();
        assert_eq!(
            visual.mode,
            crate::carrier_warning_geometry::WarningMode::Operating
        );
        inputs.push((visual.emission, filename.to_owned(), snapshot));
    }
    let low = inputs
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.0.total_cmp(&b.0))
        .unwrap()
        .0;
    let high = inputs
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.0.total_cmp(&b.0))
        .unwrap()
        .0;
    assert!(
        inputs[high].0 > inputs[low].0,
        "actual input samples must contain distinct pulse phases"
    );
    let capture_sequence = std::env::var("ARKLINE_WARNING_CAPTURE_SEQUENCE").as_deref() == Ok("1");
    if capture_sequence {
        assert_eq!(inputs.len(), 11, "exact 35.0–36.0s sample set required");
        for pair in inputs.windows(2) {
            assert_eq!(pair[1].2.server_time_ms - pair[0].2.server_time_ms, 100);
        }
    }
    let mut state = State::default();
    let mut captures = Vec::new();
    let mut fixed_camera = None;
    let mut reference_model = None;
    let mut mask = Vec::new();
    let mut housing_pixels = Vec::new();
    unsafe {
        let (_egl, gl) = super::super::exterior_gpu_tests::Egl::create();
        let mut renderer = Renderer::new(gl.clone()).unwrap();
        let target = CachedFeed::create(&gl, u64::MAX - 2, 640, 360).unwrap();
        // Preserve accepted source order: the low/high chosen samples may have
        // different times, and the resolver correctly rejects backward replay.
        let mut selected = if capture_sequence {
            (0..inputs.len()).collect::<Vec<_>>()
        } else {
            vec![low, high]
        };
        selected.sort_unstable();
        for index in selected {
            let (emission, filename, snapshot) = &inputs[index];
            let before = serde_json::to_vec(snapshot).unwrap();
            let pilot = snapshot
                .pilots
                .iter()
                .find(|p| p.name == pilot_name)
                .unwrap();
            let public = pilot.carrier_inventory.as_ref().unwrap();
            let (mut frame, _) = resolve_frame(
                &mut state,
                snapshot,
                pilot_name,
                snapshot.server_time_ms,
                true,
                false,
                "pioneer.carrier.camera.loader.port.0",
                true,
                [640, 360],
                Default::default(),
            )
            .unwrap()
            .unwrap();
            let geometry = flight_geometry().unwrap();
            let catalog = geometry
                .carriers
                .iter()
                .filter_map(|c| c.as_ref().ok())
                .find(|c| c.registration == public.binding().registration)
                .unwrap();
            let mesh = catalog
                .warning_beacons
                .iter()
                .find(|(_, cell)| *cell == 0)
                .unwrap()
                .0;
            let parts = frame.canonical_pioneer_parts().unwrap();
            let part = parts.iter().find(|p| p.0 == mesh).unwrap();
            assert_eq!(part.3, *emission);
            let model = frame.pioneer_body_mount() * part.1;
            let vertices = &geometry.catalog.uploads()[mesh_index(mesh)];
            assert_eq!(vertices.len(), 36);
            if let Some(previous) = reference_model {
                assert_eq!(
                    model, previous,
                    "actual trolley/housing must remain fixed during lamp comparison"
                );
            } else {
                reference_model = Some(model);
            }
            let (lo, hi) = geometry.catalog.mesh_bounds(mesh).unwrap();
            let center = model.transform_point3((lo + hi) * 0.5);
            let (eye_world, target_world, up_world) = *fixed_camera.get_or_insert_with(|| {
                let eye = center + frame.rotation * Vec3::new(0., -0.0041, 0.006);
                (
                    frame.space.world(eye),
                    frame.space.world(center),
                    (frame.rotation * Vec3::Y).to_array(),
                )
            });
            // The ONLY altered view data is the explicitly external camera.
            frame.eye = frame.space.scene(eye_world);
            let look = frame.space.scene(target_world);
            let up = Vec3::from_array(up_world);
            let view = Mat4::look_at_rh(frame.eye, look, up);
            frame.vp = Mat4::perspective_rh_gl(40_f32.to_radians(), 16. / 9., 0.0001, 0.05) * view;
            frame.cockpit_vp = frame.vp;
            frame.planet_vp =
                Mat4::perspective_rh_gl(40_f32.to_radians(), 16. / 9., 1., 300_000.) * view;
            assert!(!frame.cockpit && frame.camera_drone.is_none());
            let nearest = |direction: Vec3| {
                parts
                    .iter()
                    .filter_map(|p| {
                        ray_mesh_hit(
                            frame.eye,
                            direction,
                            frame.pioneer_body_mount() * p.1,
                            &geometry.catalog.uploads()[mesh_index(p.0)],
                        )
                        .map(|t| (t, p.0))
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
            };
            assert_eq!(
                nearest((look - frame.eye).normalize()).unwrap().1,
                mesh,
                "actual opaque source must expose the lamp without hiding parts"
            );
            if mask.is_empty() {
                mask = (0..360)
                    .flat_map(|y| (0..640).map(move |x| (x, y)))
                    .map(|(x, y)| {
                        ray_mesh_hit(frame.eye, pixel_ray(&frame, x, y), model, vertices).is_some()
                    })
                    .collect();
                assert!(mask.iter().filter(|v| **v).count() > 100);
                for x in [-0.002, 0.002] {
                    let point = center + frame.rotation * Vec3::new(x, 0.0014, 0.);
                    let (px, py) = screen_pixel(frame.vp, point);
                    let hit = nearest(pixel_ray(&frame, px, py)).unwrap();
                    assert_ne!(hit.1, mesh);
                    assert!(
                        catalog
                            .moving
                            .iter()
                            .any(|m| m.mesh == hit.1 && m.group_id == "cell00/loader/trolley"),
                        "housing sample must hit real opaque trolley hardware"
                    );
                    housing_pixels.push((px, py));
                }
            }
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            gl.viewport(0, 0, 640, 360);
            gl.disable(glow::SCISSOR_TEST);
            gl.depth_range_f32(0., 1.);
            renderer.paint(&frame, Some(target.framebuffer));
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
            let mut flipped = pixels.clone();
            for y in 0..360 {
                flipped[y * 640 * 4..(y + 1) * 640 * 4]
                    .copy_from_slice(&pixels[(359 - y) * 640 * 4..(360 - y) * 640 * 4]);
            }
            let image_name = if capture_sequence {
                format!("amber-sequence-{index:05}-DIAGNOSTIC-EXTERNAL.png")
            } else {
                format!(
                    "amber-{}-DIAGNOSTIC-EXTERNAL.png",
                    if *emission == inputs[low].0 {
                        "low"
                    } else {
                        "high"
                    }
                )
            };
            image::save_buffer(
                output.join(&image_name),
                &flipped,
                640,
                360,
                image::ColorType::Rgba8,
            )
            .unwrap();
            let image_sha256 = hex::encode(Sha256::digest(
                std::fs::read(output.join(&image_name)).unwrap(),
            ));
            assert_eq!(serde_json::to_vec(snapshot).unwrap(), before);
            captures.push((*emission,pixels,serde_json::json!({"response":filename,"sample_ms":snapshot.server_time_ms,"emission":emission,"source_sha256":public.binding().registration.assembly_source_sha256,"lamp_mesh":mesh,"unchanged_source_part_count":parts.len(),"camera_eye_world":eye_world,"camera_target_world":target_world,"camera_up_world":up_world,"vfov_degrees":40.,"image":image_name,"image_sha256":image_sha256})));
        }
        renderer.destroy();
        target.destroy(&gl);
    }
    // Keep receipt images in actual clock order; choose extrema only for the
    // original pixel assertions, never reorder or interpolate the sequence.
    let low = &captures
        .iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap()
        .1;
    let high = &captures
        .iter()
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap()
        .1;
    let mut changed_lamp = 0;
    let mut changed_outside = 0;
    let mut low_sum = 0u64;
    let mut high_sum = 0u64;
    let dilated = |x: usize, y: usize| {
        (y.saturating_sub(2)..=(y + 2).min(359))
            .any(|yy| (x.saturating_sub(2)..=(x + 2).min(639)).any(|xx| mask[yy * 640 + xx]))
    };
    for y in 0..360 {
        for x in 0..640 {
            let i = (y * 640 + x) * 4;
            let changed = (0..3).any(|c| low[i + c].abs_diff(high[i + c]) > 3);
            if mask[y * 640 + x] {
                low_sum += u64::from(low[i]) + u64::from(low[i + 1]) + u64::from(low[i + 2]);
                high_sum += u64::from(high[i]) + u64::from(high[i + 1]) + u64::from(high[i + 2]);
                if changed {
                    changed_lamp += 1;
                }
            } else if changed && !dilated(x, y) {
                changed_outside += 1;
            }
        }
    }
    assert!(
        changed_lamp > 100 && high_sum > low_sum,
        "actual on-phase must visibly brighten actual lamp pixels"
    );
    assert_eq!(changed_outside, 0, "non-lamp scene pixels must not flash");
    for &(x, y) in &housing_pixels {
        let i = (y * 640 + x) * 4;
        assert_eq!(
            &low[i..i + 4],
            &high[i..i + 4],
            "opaque source housing must remain visible and unchanged"
        );
    }
    let report = serde_json::json!({"qualification":"Production renderer actual amber pulse pixels; explicitly fixed external diagnostic close-up, NOT a mounted camera feed; no hardware hidden/reposed or synthetic clock","capture_sequence":capture_sequence,"interpolated_frames":false,"tested_renderer_commit":renderer_commit,"test_executable_sha256":executable_sha256,"input_receipt_sha256":hex::encode(Sha256::digest(receipt_bytes)),"captures":captures.iter().map(|c|&c.2).collect::<Vec<_>>(),"changed_real_lamp_pixels":changed_lamp,"changed_outside_lamp_pixels":changed_outside,"opaque_housing_pixel_samples":housing_pixels,"gameplay_snapshots_unchanged":true});
    std::fs::write(
        output.join("amber-pixel-receipt.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}
