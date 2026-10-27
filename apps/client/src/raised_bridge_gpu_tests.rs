//! Before/after registered whole-vessel evidence, using the production renderer.
use super::*;

#[test]
#[ignore = "Explicit registered bridge whole-hull and pilot native GPU evidence"]
fn native_raised_bridge_whole_hull_and_pilot_evidence() {
    unsafe {
        let directory = std::path::PathBuf::from(
            std::env::var_os("ARKLINE_RAISED_BRIDGE_EVIDENCE").expect("new evidence directory"),
        );
        std::fs::create_dir(&directory).unwrap();
        let (_context, gl) = PrivateContext::create();
        let mut h = Harness::new(gl.clone());
        let cache = SceneCache::new(arkline_protocol::default_pioneer()).unwrap();
        let pose = PioneerDockedPose::private();
        let dynamic = dynamic_geometry(pose, actuator_pose(None, pose, None, 1000)).unwrap();
        let parts = body_parts(&cache, &dynamic).unwrap();
        let mut captures = Vec::new();
        for (name, eye, up, half_width) in [
            ("whole-hull-quarter", Vec3::new(70., 45., 65.), Vec3::Y, 48.),
            ("whole-hull-port", Vec3::new(0., 8., -100.), Vec3::Y, 43.),
            ("whole-hull-top", Vec3::new(0., 100., 0.), -Vec3::Z, 43.),
        ] {
            let target = Vec3::ZERO;
            let half_height = half_width * HEIGHT as f32 / WIDTH as f32;
            let vp = Mat4::orthographic_rh_gl(
                -half_width,
                half_width,
                -half_height,
                half_height,
                0.1,
                200.,
            ) * Mat4::look_at_rh(eye, target, up);
            for _ in 0..3 {
                gl.bind_framebuffer(
                    glow::FRAMEBUFFER,
                    Some(h.target.as_ref().unwrap().framebuffer),
                );
                gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
                gl.disable(glow::SCISSOR_TEST);
                gl.color_mask(true, true, true, true);
                gl.depth_mask(true);
                gl.clear_depth_f32(1.);
                gl.clear_color(0.012, 0.020, 0.027, 1.);
                gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
                paint_scene(
                    &mut h.view.renderer.as_ref().unwrap().lock().unwrap(),
                    vp,
                    eye,
                    &parts,
                    1000,
                    true,
                    0,
                );
            }
            let mut pixels = vec![0; WIDTH * HEIGHT * 4];
            gl.read_pixels(
                0,
                0,
                WIDTH as i32,
                HEIGHT as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            assert_eq!(gl.get_error(), glow::NO_ERROR);
            let pixels = pixels
                .chunks_exact(WIDTH * 4)
                .rev()
                .flatten()
                .copied()
                .collect::<Vec<_>>();
            image::save_buffer_with_format(
                directory.join(format!("{name}.png")),
                &pixels,
                WIDTH as u32,
                HEIGHT as u32,
                image::ColorType::Rgba8,
                image::ImageFormat::Png,
            )
            .unwrap();
            captures.push(serde_json::json!({"png":format!("{name}.png"),"view":"full installed runtime hull and fittings; external inspection camera, no cutaway","body_eye_m":eye.to_array(),"draw_count":parts.len(),"rgba_sha256":hex::encode(Sha256::digest(&pixels))}));
        }
        let seated = pilot(None);
        for (name, yaw, pitch) in [
            ("pilot-forward", 0., 0.),
            ("pilot-port", 0.30, 0.),
            ("pilot-starboard", -0.30, 0.),
            ("pilot-overhead", 0., 0.25),
            ("pilot-port-horizontal", std::f32::consts::FRAC_PI_2, 0.),
            (
                "pilot-starboard-horizontal",
                -std::f32::consts::FRAC_PI_2,
                0.,
            ),
        ] {
            h.view.pitch = 0.;
            for _ in 0..8 {
                h.view.head_look = [yaw, pitch];
                h.frame(&seated, 1000, vec![]);
            }
            h.view.head_look = [yaw, pitch];
            let shot = h.frame(&seated, 1000, vec![]);
            assert!(shot.occupied > 1000);
            captures.push(capture(Some(&directory), name, &shot));
        }
        let root = arkline_protocol::pilot_joints(1.0).root_m;
        for (name, heading) in [
            ("standing-forward", 0.),
            ("standing-port-horizontal", -std::f64::consts::FRAC_PI_2),
            ("standing-starboard-horizontal", std::f64::consts::FRAC_PI_2),
        ] {
            let standing = pilot(Some(supported_state_at(
                root,
                heading,
                arkline_protocol::DockedSupport::Cockpit,
            )));
            h.view.yaw = heading as f32;
            h.view.pitch = 0.;
            h.view.head_look = [0.; 2];
            let shot = h.settle(&standing, 1000);
            assert!((shot.direction.0 - heading as f32).abs() < 1e-4);
            assert!(shot.direction.1.abs() < 1e-4);
            captures.push(capture(Some(&directory), name, &shot));
        }
        std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"scope":"actual registered engine rendering; deterministic in-memory fixture, not real player session or performance proof","capability":arkline_protocol::docked_traversal_capability(),"source_sha256":arkline_protocol::PIONEER_DOCKED_SOURCE_SHA256,"pilot_seated_body_eye_m":arkline_protocol::pilot_joints(0.).eye_m,"pilot_standing_body_root_m":root,"renderer":gl.get_parameter_string(glow::RENDERER),"captures":captures})).unwrap()).unwrap();
    }
}

#[test]
#[ignore = "Explicit supported raised-bridge access stair native GPU evidence"]
fn native_raised_bridge_supported_access_evidence() {
    unsafe {
        let directory = std::path::PathBuf::from(
            std::env::var_os("ARKLINE_BRIDGE_ACCESS_EVIDENCE").expect("new evidence directory"),
        );
        std::fs::create_dir(&directory).unwrap();
        let (_context, gl) = PrivateContext::create();
        let mut h = Harness::new(gl.clone());
        let geometry = registered_geometry(&arkline_protocol::default_pioneer()).unwrap();
        let mut captures = Vec::new();
        for (name, id, heading) in [
            ("stairs-lower-approach", "bridge/stairs/base", 0.),
            ("stairs-mid-lower", "bridge/stairs/landing/0", 2.74),
            ("stairs-mid-upper", "bridge/stairs/landing/1", -0.40),
            ("stairs-upper-to-bridge", "bridge/stairs/landing/2", 0.),
            ("stairs-upper-descent", "bridge/stairs/landing/2", -2.74),
        ] {
            let surface = geometry
                .supports
                .iter()
                .find(|surface| surface.id == id)
                .expect("raised bridge has this actual registered support");
            // The base also overlaps the lower connector on its centerline.
            // Stand on the actual first-flight lane, not the tied center-floor
            // support, so this view proves the named stair support precisely.
            let mut feet = surface.center_m;
            if id == "bridge/stairs/base" {
                feet[2] = geometry
                    .supports
                    .iter()
                    .find(|s| s.id == "bridge/stairs/0/tread/1")
                    .unwrap()
                    .center_m[2];
            }
            let state =
                supported_state_at(feet, heading, arkline_protocol::DockedSupport::CoreStairs);
            assert_eq!(state.surface_id, id);
            let fixture = pilot(Some(state));
            h.view.yaw = heading as f32;
            h.view.pitch = if name == "stairs-upper-descent" {
                0.60
            } else {
                0.15
            };
            h.view.head_look = [0.; 2];
            let shot = h.settle(&fixture, 1000);
            assert!(shot.occupied > 1000);
            let mut record = capture(Some(&directory), name, &shot);
            record["support_id"] = serde_json::json!(id);
            record["body_feet_m"] = serde_json::json!(feet);
            captures.push(record);
        }
        let landing = geometry
            .supports
            .iter()
            .find(|surface| surface.id == "bridge/stairs/landing/0")
            .unwrap();
        let foyer = geometry
            .supports
            .iter()
            .find(|surface| surface.id == "bridge/lower-service-foyer")
            .unwrap();
        for (name, body, heading) in [
            (
                "lower-corridor-under-landing",
                [
                    landing.center_m[0],
                    arkline_protocol::deck_floor_y(2).unwrap(),
                    0.,
                ],
                std::f64::consts::PI,
            ),
            (
                "lower-foyer-towards-stairs",
                foyer.center_m,
                std::f64::consts::PI,
            ),
        ] {
            let state =
                supported_state_at(body, heading, arkline_protocol::DockedSupport::Connector);
            let support_id = state.surface_id.clone();
            let fixture = pilot(Some(state));
            h.view.yaw = heading as f32;
            h.view.pitch = 0.;
            h.view.head_look = [0.; 2];
            let shot = h.settle(&fixture, 1000);
            assert!(shot.occupied > 1000);
            let mut record = capture(Some(&directory), name, &shot);
            record["support_id"] = serde_json::json!(support_id);
            record["body_feet_m"] = serde_json::json!(body);
            captures.push(record);
        }
        std::fs::write(directory.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "scope": "actual first-person production renderer at collision-clear registered stair and lower corridor supports; in-memory view fixtures, not a walked server journey or hull-cutaway",
            "capability": arkline_protocol::docked_traversal_capability(),
            "source_sha256": arkline_protocol::PIONEER_DOCKED_SOURCE_SHA256,
            "renderer": gl.get_parameter_string(glow::RENDERER), "captures": captures
        })).unwrap()).unwrap();
    }
}
