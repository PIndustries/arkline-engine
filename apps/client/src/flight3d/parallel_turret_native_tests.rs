//! Actual accepted stow Responses, production renderer, external diagnostic view.
//! This is NOT a firing/aim-authority or normal-flight admission test.
use super::*;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[test]
#[ignore = "Explicit EGL exact 3aa2 49-row physical stow proof"]
fn native_actual_turrets_drop_close_and_redeploy() {
    let input = PathBuf::from(std::env::var("ARKLINE_TURRET_RESPONSES").unwrap());
    let output = PathBuf::from(std::env::var("ARKLINE_TURRET_NATIVE_OUTPUT").unwrap());
    assert!(output.is_dir());
    let renderer_commit = std::env::var("ARKLINE_TESTED_RENDERER_COMMIT").unwrap();
    assert!(renderer_commit.len() == 40 && renderer_commit.bytes().all(|b| b.is_ascii_hexdigit()));
    let receipt_bytes = std::fs::read(input.join("receipt.json")).unwrap();
    assert_eq!(
        hex::encode(Sha256::digest(&receipt_bytes)),
        "6bf4a88806f0da3544c661225b3462c606fe89b11ebb64ce878409943a34fdfa"
    );
    let receipt: serde_json::Value = serde_json::from_slice(&receipt_bytes).unwrap();
    let name = receipt["pilot"].as_str().unwrap();
    let rows = receipt["frames"].as_array().unwrap();
    assert_eq!(rows.len(), 49);
    let mut state = State::default();
    let mut captures = Vec::new();
    let mut muzzle_heights = Vec::new();
    let mut previous_clock = None;
    let mut endpoint_pixels = Vec::new();
    unsafe {
        let (_egl, gl) = super::super::exterior_gpu_tests::Egl::create();
        let mut renderer = Renderer::new(gl.clone()).unwrap();
        let target = CachedFeed::create(&gl, u64::MAX - 3, 640, 360).unwrap();
        for (index, row) in rows.iter().enumerate() {
            let file = row["file"].as_str().unwrap();
            assert!(!file.contains("..") && !file.starts_with('/'));
            let bytes = std::fs::read(input.join(file)).unwrap();
            assert_eq!(
                hex::encode(Sha256::digest(&bytes)),
                row["response_sha256"].as_str().unwrap()
            );
            let envelope: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(envelope["producer_commit"], receipt["producer_commit"]);
            let response: arkline_protocol::Response =
                serde_json::from_value(envelope["response"].clone()).unwrap();
            let snapshot = response.snapshot.unwrap();
            let before = serde_json::to_vec(&snapshot).unwrap();
            let pilot = snapshot.pilots.iter().find(|p| p.name == name).unwrap();
            let deployment = &pilot.cockpit_mechanisms.as_ref().unwrap().turrets;
            assert!(
                deployment.aims.is_none(),
                "historical stow rows confer no aim authority"
            );
            assert_eq!(snapshot.server_time_ms, row["sample_ms"].as_u64().unwrap());
            if let Some(last) = previous_clock {
                assert_eq!(snapshot.server_time_ms - last, 100);
            }
            previous_clock = Some(snapshot.server_time_ms);
            let (mut frame, _) = resolve_frame(
                &mut state,
                &snapshot,
                name,
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
            let mount = frame.ship_mount(pilot).unwrap();
            // Only camera extrinsics change. All opaque hull, wells and actors remain.
            frame.eye = mount.transform_point3(Vec3::new(0.20, 0.18, -0.22));
            let look = mount.transform_point3(Vec3::new(0.125, 0.087, -0.052));
            let view = Mat4::look_at_rh(
                frame.eye,
                look,
                mount.transform_vector3(Vec3::Y).normalize(),
            );
            frame.vp = Mat4::perspective_rh_gl(45_f32.to_radians(), 16. / 9., 0.0001, 1200.) * view;
            frame.cockpit_vp = frame.vp;
            frame.planet_vp =
                Mat4::perspective_rh_gl(45_f32.to_radians(), 16. / 9., 1., 300_000.) * view;
            renderer.prepare_turrets(&frame);
            for slot in 0..2 {
                let raw = renderer
                    .turrets
                    .get(&(name.to_owned(), slot))
                    .unwrap()
                    .rotation;
                let resolve = || {
                    parallel_turret_pose::resolve(
                        pilot,
                        frame.space,
                        frame.snapshot_time,
                        frame.now,
                        slot,
                        raw,
                        frame.connected,
                        frame.scene.carrier_inventory_registration.as_ref(),
                    )
                    .unwrap()
                };
                let rig = resolve();
                assert_eq!(
                    rig.parts.len(),
                    11,
                    "one nine-part gun and two shutters, not duplicate guns"
                );
                assert!(!rig.can_fire && rig.beam_radius_m.is_none());
                // The same source resolver supplies color, shadow and picking instances.
                assert_eq!(rig.parts, resolve().parts);
                let feed = turret_camera_frame(&frame, pilot, slot, raw, 16. / 9.).unwrap();
                assert!((feed.eye - (rig.muzzle + rig.up * 0.022)).length() < 1e-6);
                for (mesh, model, _, _) in &rig.parts {
                    let geometry = flight_geometry().unwrap();
                    let vertices = &geometry.catalog.uploads()[mesh_index(*mesh)];
                    let center = model.transform_point3(Vec3::ZERO);
                    let origin = center + model.transform_vector3(Vec3::Y) * 3.;
                    assert!(
                        ray_mesh_hit(origin, (center - origin).normalize(), *model, vertices)
                            .is_some(),
                        "world-space picking must intersect the same physical instance"
                    );
                }
                if slot == 0 {
                    muzzle_heights.push(mount.inverse().transform_point3(rig.muzzle).y);
                }
            }
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            gl.viewport(0, 0, 640, 360);
            gl.disable(glow::SCISSOR_TEST);
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
            if [0, 24, 48].contains(&index) {
                endpoint_pixels.push(pixels.clone());
            }
            let mut flipped = pixels.clone();
            for y in 0..360 {
                flipped[y * 640 * 4..(y + 1) * 640 * 4]
                    .copy_from_slice(&pixels[(359 - y) * 640 * 4..(360 - y) * 640 * 4]);
            }
            let png = format!("turret-{index:05}-EXTERNAL-DIAGNOSTIC.png");
            image::save_buffer(
                output.join(&png),
                &flipped,
                640,
                360,
                image::ColorType::Rgba8,
            )
            .unwrap();
            captures.push(serde_json::json!({"file":png,"sha256":hex::encode(Sha256::digest(std::fs::read(output.join(&png)).unwrap())),
                "sample_ms":snapshot.server_time_ms,"response_sha256":row["response_sha256"],"fractions":row["fractions"]}));
            assert_eq!(serde_json::to_vec(&snapshot).unwrap(), before);
        }
        target.destroy(&gl);
        renderer.destroy();
    }
    let measured_drop = muzzle_heights[0] - muzzle_heights[24];
    let expected_drop = arkline_protocol::turret_stow_geometry::DROP_M as f32 / 100.;
    eprintln!(
        "actual muzzle scene heights raised={} stowed={} redeployed={}; drop={} expected={}",
        muzzle_heights[0], muzzle_heights[24], muzzle_heights[48], measured_drop, expected_drop
    );
    assert!((measured_drop - expected_drop).abs() < 1e-6);
    assert!((muzzle_heights[0] - muzzle_heights[48]).abs() < 1e-6);
    assert_ne!(
        endpoint_pixels[0], endpoint_pixels[1],
        "actual stow must change native pixels"
    );
    assert_ne!(endpoint_pixels[1], endpoint_pixels[2]);
    std::fs::write(output.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "scope":"Actual 3aa2 accepted physical stow; external diagnostic camera, no firing or normal admission",
        "renderer_commit":renderer_commit,"executable_sha256":hex::encode(Sha256::digest(std::fs::read(std::env::current_exe().unwrap()).unwrap())),
        "source_receipt_sha256":hex::encode(Sha256::digest(&receipt_bytes)),"frames":captures
    })).unwrap()).unwrap();
}
