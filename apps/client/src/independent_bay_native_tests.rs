//! Native documentary evidence of unchanged committed stationary fixture rows.
//! Not general gameplay admission and not a replacement for captain view.
use super::*;
use crate::flight3d::exterior_gpu_tests::Egl;
use arkline_protocol::{drone_bay_geometry::BayPose, *};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Deserialize)]
struct LaneMechanisms {
    bay_id: u8,
    groups: Vec<IndependentBayMechanismView>,
}
#[derive(Deserialize)]
struct Recorded {
    response: Response,
    sample_time_ms: u64,
    binding: IndependentBayBinding,
    stock: Vec<DroneStockEntry>,
    state: IndependentBayState,
    cargo: DroneCargoState,
    lane_mechanisms: Vec<LaneMechanisms>,
    motions: Vec<IndependentBayMotionView>,
}
fn matrix(p: BayPose) -> Mat4 {
    Mat4::from_rotation_translation(
        Quat::from_array(p.orientation_xyzw.map(|v| v as f32)).normalize(),
        Vec3::from_array(p.position_m.map(|v| v as f32)) / 100.,
    )
}
fn drone_parts(class: DroneClass, pose: BayPose) -> Vec<ship_asset::Primitive> {
    let transform = matrix(pose);
    crate::flight3d::drone_models::model(class)
        .batches
        .iter()
        .enumerate()
        .map(|(i, b)| ship_asset::Primitive {
            vertices: b
                .vertices
                .iter()
                .map(|v| Vertex {
                    p: transform.transform_point3(v.p),
                    n: transform.transform_vector3(v.n).normalize_or_zero(),
                })
                .collect(),
            material_index: None,
            material_name: format!("Actual {class:?} material {i}"),
            color: b.color,
            emission: b.emission,
            emissive_color: b.color,
            roughness: b.roughness,
            metallic: b.metallic,
        })
        .collect()
}

#[test]
#[ignore = "Opt-in source-bound stationary independent-bay documentary capture"]
fn native_independent_bay_recording() {
    unsafe {
        const W: usize = 960;
        const H: usize = 640;
        let path = |k| PathBuf::from(std::env::var_os(k).expect(k));
        let input = path("ARKLINE_INDEPENDENT_RECORDING");
        let output = path("ARKLINE_INDEPENDENT_CAPTURE");
        std::fs::create_dir(&output).unwrap();
        let receipt_bytes = std::fs::read(input.join("receipt.json")).unwrap();
        let receipt: serde_json::Value = serde_json::from_slice(&receipt_bytes).unwrap();
        let pilot_name = std::env::var("ARKLINE_INDEPENDENT_PILOT")
            .unwrap_or_else(|_| receipt["pilot"].as_str().unwrap().to_owned());
        let stride: usize = std::env::var("ARKLINE_CAPTURE_STRIDE")
            .unwrap_or_else(|_| "2".into())
            .parse()
            .unwrap();
        assert!(stride > 0);
        let definition =
            arkline_protocol::independent_bay_geometry::stationary_fixture_definition().unwrap();
        let source = crate::flight3d::independent_bay_source::Source::load().unwrap();
        let (_context, gl) = Egl::create();
        let target = CachedFeed::create(&gl, 84001, W as i32, H as i32).unwrap();
        let mut gpu = PreviewGpu::new(gl.clone()).unwrap();
        let mut cameras = vec![
            ("bay-contact", [-34., 5., -40.], [-26., 0., -26.], 14.),
            ("port-bank", [-14., 17., -79.], [-12., 0., -23.], 35.),
        ];
        if std::env::var_os("ARKLINE_INDEPENDENT_ALL_BANKS").is_some() {
            cameras = vec![
                (
                    "mining-bank",
                    [-14.0, 17.0, -79.0],
                    [-12.0, 0.0, -23.0],
                    35.0,
                ),
                ("combat-bank", [-14.0, 17.0, 79.0], [-12.0, 0.0, 23.0], 35.0),
                ("recon-bank", [-87.0, 22.0, 0.0], [-39.0, 8.0, 0.0], 25.0),
            ];
        }
        if std::env::var_os("ARKLINE_INDEPENDENT_DETAIL").is_some() {
            cameras.push((
                "socket-detail",
                [-23.2, 0.6, -24.9],
                [-26.0, 0.0, -26.3],
                2.25,
            ));
        }
        let selected: Option<BTreeSet<usize>> = std::env::var("ARKLINE_CAPTURE_INDICES")
            .ok()
            .map(|raw| raw.split(',').map(|v| v.parse().unwrap()).collect());
        let mut captures = Vec::new();
        let mut geometry = ImportedGeometry::decode(include_bytes!(
            "../../../assets/ships/pioneer/independent-bays-v1601/pioneer.glb"
        ))
        .unwrap();
        for (position, row) in receipt["frames"].as_array().unwrap().iter().enumerate() {
            if selected
                .as_ref()
                .is_some_and(|indices| !indices.contains(&position))
                || (selected.is_none()
                    && position % stride != 0
                    && position + 1 != receipt["frames"].as_array().unwrap().len())
            {
                continue;
            }
            let bytes = std::fs::read(input.join(row["file"].as_str().unwrap())).unwrap();
            let hash = hex::encode(Sha256::digest(&bytes));
            assert_eq!(hash, row["response_sha256"]);
            let recorded: Recorded = serde_json::from_slice(&bytes).unwrap();
            let snapshot = recorded.response.snapshot.as_ref().unwrap();
            assert_eq!(recorded.sample_time_ms, snapshot.server_time_ms);
            assert_eq!(recorded.binding.registration, definition.registration);
            assert_eq!(recorded.state.binding, recorded.binding);
            let pilot = snapshot
                .pilots
                .iter()
                .find(|p| p.name == pilot_name)
                .unwrap();
            let flight = pilot.flight.as_ref().unwrap();
            assert!(flight.velocity.iter().all(|v| v.abs() < 1e-8));
            assert!(flight.angular_velocity.iter().all(|v| v.abs() < 1e-8));
            let mut roots = BTreeMap::new();
            let mut lanes = BTreeSet::new();
            for lane in &recorded.lane_mechanisms {
                assert!(lanes.insert(lane.bay_id));
                for group in &lane.groups {
                    assert_eq!(
                        group.pose.sample(recorded.sample_time_ms),
                        Some(group.pose.confirmed)
                    );
                    assert!(roots
                        .insert(group.semantic_id.clone(), group.pose.confirmed)
                        .is_none());
                }
            }
            assert_eq!(lanes.len(), 15);
            for motion in &recorded.motions {
                assert!(motion.valid_shape());
                if motion.phase == IndependentBayPhase::Held {
                    assert!(motion.confirmed_at_ms <= recorded.sample_time_ms);
                    assert!(motion.drone.segment.is_none());
                    assert!(motion
                        .mechanisms
                        .iter()
                        .all(|group| group.pose.segment.is_none()));
                } else {
                    assert_eq!(motion.confirmed_at_ms, recorded.sample_time_ms);
                }
                assert_eq!(
                    motion.parent,
                    BayPose {
                        position_m: flight.position,
                        orientation_xyzw: flight.orientation
                    }
                );
                for group in &motion.mechanisms {
                    assert_eq!(roots.get(&group.semantic_id), Some(&group.pose.confirmed));
                }
                if !motion.cleanup_only {
                    let live = pilot
                        .drones
                        .iter()
                        .find(|d| d.id == motion.drone_id)
                        .expect("same-ID live authority body");
                    let delta = motion
                        .parent
                        .transform_vector(motion.drone.confirmed.position_m)
                        .unwrap();
                    for (i, value) in delta.iter().enumerate() {
                        let expected = motion.parent.position_m[i] + value;
                        let roundoff =
                            expected.abs().max(live.position[i].abs()) * f64::EPSILON * 8.0 + 1e-6;
                        assert!(
                            (expected - live.position[i]).abs() <= roundoff,
                            "mixed wrapper/Response body pose"
                        );
                    }
                    let expected = glam::DQuat::from_array(motion.parent.orientation_xyzw)
                        * glam::DQuat::from_array(motion.drone.confirmed.orientation_xyzw);
                    let actual = glam::DQuat::from_array(
                        live.orientation_xyzw.expect("physical full attitude"),
                    );
                    assert!(
                        (expected.dot(actual).abs() - 1.0).abs() < 1e-9,
                        "mixed wrapper/Response attitude"
                    );
                }
            }
            let mut parts = source.sample(&roots).unwrap();
            let mut rendered = BTreeSet::new();
            for lane in &recorded.state.lanes {
                if let Some(id) = lane.stored_drone_id {
                    let stock = recorded.stock.iter().find(|s| s.drone_id == id).unwrap();
                    assert!(matches!(stock.status, DroneStockStatus::Stored { .. }));
                    assert!(rendered.insert(id));
                    let bay = definition
                        .bays
                        .iter()
                        .find(|b| b.bay_id == lane.bay_id)
                        .unwrap();
                    assert!(bay.compatible_classes.contains(&stock.class));
                    parts.extend(drone_parts(stock.class, bay.stored_pose));
                }
            }
            for drone in &pilot.drones {
                assert!(rendered.insert(drone.id));
                let stock = recorded
                    .stock
                    .iter()
                    .find(|s| s.drone_id == drone.id)
                    .unwrap();
                assert_eq!(stock.status, DroneStockStatus::Active);
                assert_eq!(stock.class, drone.class);
                let pose = if let Some(motion) = recorded
                    .motions
                    .iter()
                    .find(|m| m.drone_id == drone.id && !m.cleanup_only)
                {
                    assert!(motion.valid_shape());
                    if motion.phase == IndependentBayPhase::Held {
                        assert!(motion.confirmed_at_ms <= recorded.sample_time_ms);
                        assert!(motion.drone.segment.is_none());
                        assert!(motion
                            .mechanisms
                            .iter()
                            .all(|group| group.pose.segment.is_none()));
                    } else {
                        assert_eq!(motion.confirmed_at_ms, recorded.sample_time_ms);
                    }
                    assert_eq!(
                        motion.drone.sample(recorded.sample_time_ms),
                        Some(motion.drone.confirmed)
                    );
                    motion.drone.confirmed
                } else {
                    let parent = glam::DQuat::from_array(flight.orientation);
                    let delta = glam::DVec3::from_array(std::array::from_fn(|i| {
                        drone.position[i] - flight.position[i]
                    }));
                    BayPose {
                        position_m: (parent.inverse() * delta).to_array(),
                        orientation_xyzw: (parent.inverse()
                            * glam::DQuat::from_array(
                                drone.orientation_xyzw.expect("actual physical attitude"),
                            ))
                        .to_array(),
                    }
                };
                parts.extend(drone_parts(drone.class, pose));
            }
            assert_eq!(
                rendered.len(),
                recorded
                    .stock
                    .iter()
                    .filter(|s| !matches!(s.status, DroneStockStatus::Destroyed { .. }))
                    .count()
            );
            geometry.parts = parts;
            gpu.replace(&geometry).unwrap();
            for &(name, eye, look, half_width) in &cameras {
                let eye = Vec3::from_array(eye);
                let half_height = half_width * H as f32 / W as f32;
                let vp = Mat4::orthographic_rh_gl(
                    -half_width,
                    half_width,
                    -half_height,
                    half_height,
                    0.02,
                    500.,
                ) * Mat4::look_at_rh(eye, Vec3::from_array(look), Vec3::Y);
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
                gl.viewport(0, 0, W as i32, H as i32);
                gl.disable(glow::SCISSOR_TEST);
                gpu.paint(vp, eye);
                assert_eq!(gl.get_error(), glow::NO_ERROR);
                let mut pixels = vec![0; W * H * 4];
                gl.read_pixels(
                    0,
                    0,
                    W as i32,
                    H as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut pixels)),
                );
                let top: Vec<_> = pixels
                    .chunks_exact(W * 4)
                    .rev()
                    .flatten()
                    .copied()
                    .collect();
                let file = format!("{name}-{position:05}.png");
                image::save_buffer_with_format(
                    output.join(&file),
                    &top,
                    W as u32,
                    H as u32,
                    image::ColorType::Rgba8,
                    image::ImageFormat::Png,
                )
                .unwrap();
                captures.push(serde_json::json!({"file":file,"index":row["index"],"elapsed_ms":row["elapsed_ms"],"sample_time_ms":recorded.sample_time_ms,"authority_sha256":hash,"png_sha256":hex::encode(Sha256::digest(std::fs::read(output.join(&file)).unwrap())),"camera":name,"eye_m":eye.to_array(),"look_m":look,"ship_ore":pilot.ore,"cargo":recorded.cargo,"rendered_ids":rendered,"hull_hidden":false}));
            }
        }
        std::fs::write(output.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"scope":"Native imported-source documentary of unchanged committed stationary fixture; not captain POV, normal flyable admission, audio or performance acceptance","renderer_source":std::env::var("ARKLINE_CAPTURE_SOURCE").unwrap(),"source_receipt_sha256":hex::encode(Sha256::digest(receipt_bytes)),"assembly":definition.registration,"size":[W,H],"captures":captures})).unwrap()).unwrap();
        gpu.clear();
        gl.delete_program(gpu.program);
        target.destroy(&gl);
    }
}
