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
struct RecordedMotion {
    operation_id: u64,
    drone_id: u64,
    confirmed_at_ms: u64,
    held: bool,
    drone_pose: Option<BayPose>,
    cassette_poses: Vec<(u64, BayPose)>,
    machinery: Vec<(String, BayPose)>,
}
#[derive(Deserialize)]
struct Recorded {
    format_version: u32,
    producer_commit: String,
    sample_time_ms: u64,
    pilot: String,
    response: Response,
    parent: BayPose,
    binding: CarrierInventoryBinding,
    inventory: CarrierInventoryState,
    ore: serde_json::Value,
    drone_poses: Vec<(u64, BayPose)>,
    cassette_poses: Vec<(u64, BayPose)>,
    machinery: Vec<(String, BayPose)>,
    motions: Vec<RecordedMotion>,
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
fn native_carrier_magazine_recording() {
    unsafe {
        const W: usize = 960;
        const H: usize = 640;
        let path = |k| PathBuf::from(std::env::var_os(k).expect(k));
        let input = path("ARKLINE_CARRIER_RECORDING");
        let output = path("ARKLINE_CARRIER_CAPTURE");
        std::fs::create_dir(&output).unwrap();
        let receipt_bytes = std::fs::read(input.join("receipt.json")).unwrap();
        let receipt: serde_json::Value = serde_json::from_slice(&receipt_bytes).unwrap();
        let pilot_name = std::env::var("ARKLINE_CARRIER_PILOT")
            .unwrap_or_else(|_| receipt["pilot"].as_str().unwrap().to_owned());
        let stride: usize = std::env::var("ARKLINE_CAPTURE_STRIDE")
            .unwrap_or_else(|_| "2".into())
            .parse()
            .unwrap();
        assert!(stride > 0);
        let first = receipt["frames"]
            .as_array()
            .unwrap()
            .first()
            .expect("recorded frames");
        let first_bytes = std::fs::read(input.join(first["file"].as_str().unwrap())).unwrap();
        assert_eq!(
            hex::encode(Sha256::digest(&first_bytes)),
            first["response_sha256"]
        );
        let first_recorded: Recorded = serde_json::from_slice(&first_bytes).unwrap();
        let registration = &first_recorded.binding.registration;
        let definition =
            arkline_protocol::carrier_inventory_geometry::stationary_fixture_for_registration(
                registration,
            )
            .expect("exact recorded stationary source");
        let source =
            crate::flight3d::carrier_magazine_source::Source::load_for_registration(registration)
                .unwrap();
        let installed_bytes =
            crate::flight3d::carrier_magazine_source::Source::installed_bytes_for_registration(
                registration,
            )
            .unwrap();
        let (_context, gl) = Egl::create();
        let target = CachedFeed::create(&gl, 84001, W as i32, H as i32).unwrap();
        let mut gpu = PreviewGpu::new(gl.clone()).unwrap();
        let diagnostic = std::env::var_os("ARKLINE_CARRIER_DIAGNOSTIC_LIGHTING").is_some();
        if diagnostic {
            // Extra diagnostic only: original matched production-lighting
            // evidence is retained separately. Mesh/materials are unchanged.
            let program = compile(&gl,
                r#"#version 150
in vec3 position;in vec3 normal;uniform mat4 vp;out vec3 n;out vec3 world;
void main(){world=position;n=normal;gl_Position=vp*vec4(position,1.0);}"#,
                r#"#version 150
in vec3 n;in vec3 world;uniform vec3 base;uniform vec3 emission;uniform vec3 eye;uniform float roughness;uniform float metallic;out vec4 color;
void main(){vec3 normal=normalize(n);if(!gl_FrontFacing)normal=-normal;vec3 l=normalize(vec3(0.4,0.9,0.6));vec3 fill=normalize(vec3(-0.4,0.3,-0.8));vec3 v=normalize(eye-world);float diffuse=max(dot(normal,l),0.0);float spec=pow(max(dot(normal,normalize(l+v)),0.0),mix(80.0,3.0,roughness));vec3 lit=base*(0.65+1.1*diffuse+0.65*max(dot(normal,fill),0.0))*(1.0-0.4*metallic)+mix(vec3(0.04),base,metallic)*spec+emission;color=vec4(pow(lit/(lit+vec3(1.0)),vec3(1.0/2.2)),1.0);}"#).unwrap();
            gl.delete_program(gpu.program);
            gpu.program = program;
        }
        let interior_fov = std::env::var("ARKLINE_CARRIER_INTERIOR_FOV_DEGREES")
            .ok()
            .map(|v| {
                let fov = v.parse::<f32>().unwrap();
                assert!(fov.is_finite() && (20.0..=110.0).contains(&fov));
                fov
            });
        let mut cameras = vec![
            ("starboard-ready", [-12., 18., 79.], [-10., 1., 23.], 36.),
            ("port-ready", [-12., 18., -79.], [-10., 1., -23.], 36.),
            (
                "magazine-interior",
                [-10., 7.2, -19.8],
                [-10., 4.8, -28.0],
                26.,
            ),
        ];
        // Documentary inspection parameters affect only the camera. Every
        // exact value is retained in the receipt; no source part is hidden.
        let camera_vector = |key: &str| -> Option<[f32; 3]> {
            std::env::var(key).ok().map(|raw| {
                let values: Vec<f32> = raw.split(',').map(|v| v.parse().unwrap()).collect();
                let v: [f32; 3] = values.try_into().expect("three camera coordinates");
                assert!(v.iter().all(|x| x.is_finite()));
                v
            })
        };
        if let Some(camera) = cameras.iter_mut().find(|c| c.0 == "magazine-interior") {
            if let Some(v) = camera_vector("ARKLINE_CARRIER_INTERIOR_EYE") {
                camera.1 = v;
            }
            if let Some(v) = camera_vector("ARKLINE_CARRIER_INTERIOR_LOOK") {
                camera.2 = v;
            }
            if let Ok(v) = std::env::var("ARKLINE_CARRIER_INTERIOR_HALF_WIDTH") {
                camera.3 = v.parse::<f32>().unwrap();
                assert!(camera.3.is_finite() && camera.3 > 0.);
            }
        }
        if std::env::var_os("ARKLINE_CARRIER_WIDE").is_some() {
            cameras.push(("whole-carrier", [-78., 60., -90.], [-3., 2., 0.], 63.));
        }
        let selected: Option<BTreeSet<usize>> = std::env::var("ARKLINE_CAPTURE_INDICES")
            .ok()
            .map(|raw| raw.split(',').map(|v| v.parse().unwrap()).collect());
        let mounted_device = std::env::var("ARKLINE_CARRIER_MOUNTED_DEVICE").ok();
        if mounted_device.is_some() {
            cameras.retain(|c| c.0 != "magazine-interior");
        }
        let mut captures = Vec::new();
        let mut geometry = ImportedGeometry::decode(installed_bytes).unwrap();
        for (position, row) in receipt["frames"].as_array().unwrap().iter().enumerate() {
            let bytes = std::fs::read(input.join(row["file"].as_str().unwrap())).unwrap();
            let hash = hex::encode(Sha256::digest(&bytes));
            assert_eq!(hash, row["response_sha256"]);
            let recorded: Recorded = serde_json::from_slice(&bytes).unwrap();
            let snapshot = recorded.response.snapshot.as_ref().unwrap();
            assert_eq!(recorded.sample_time_ms, snapshot.server_time_ms);
            assert_eq!(recorded.format_version, 1);
            assert!(!recorded.producer_commit.is_empty());
            assert_eq!(
                recorded.producer_commit,
                receipt["producer_commit"]
                    .as_str()
                    .expect("exact authority producer")
            );
            assert_eq!(recorded.pilot, pilot_name);
            assert_eq!(
                recorded.binding.registration,
                definition.topology.registration
            );
            assert_eq!(recorded.inventory.binding, recorded.binding);
            assert!(recorded.inventory.validate_shape(&definition.topology));
            let pilot = snapshot
                .pilots
                .iter()
                .find(|p| p.name == pilot_name)
                .unwrap();
            let flight = pilot.flight.as_ref().unwrap();
            assert_eq!(
                recorded.parent,
                BayPose {
                    position_m: flight.position,
                    orientation_xyzw: flight.orientation
                }
            );
            assert!(flight.velocity.iter().all(|v| v.abs() < 1e-8));
            if let Some(public) = &pilot.carrier_inventory {
                public
                    .validate_projection(pilot)
                    .expect("actual public projection binding");
                assert_eq!(public.sample_time_ms, recorded.sample_time_ms);
                assert_eq!(public.binding(), &recorded.binding);
                assert_eq!(public.parent, recorded.parent);
                assert_eq!(public.inventory, recorded.inventory);
                assert_eq!(serde_json::to_value(&public.ore).unwrap(), recorded.ore);
                assert_eq!(public.drone_poses, recorded.drone_poses);
                assert_eq!(public.cassette_poses, recorded.cassette_poses);
                assert_eq!(public.machinery, recorded.machinery);
            } else {
                assert!(
                    mounted_device.is_none() && registration.assembly_revision != 1703,
                    "new source/mounted inspection requires actual public roots"
                );
            }
            if selected
                .as_ref()
                .is_some_and(|indices| !indices.contains(&position))
                || (selected.is_none()
                    && position % stride != 0
                    && position + 1 != receipt["frames"].as_array().unwrap().len())
            {
                continue;
            }
            let roots: BTreeMap<_, _> = recorded.machinery.iter().cloned().collect();
            assert_eq!(roots.len(), recorded.machinery.len());
            let cassettes: BTreeMap<_, _> = recorded.cassette_poses.iter().copied().collect();
            assert_eq!(cassettes.len(), 20);
            assert_eq!(cassettes.len(), recorded.cassette_poses.len());
            let drones: BTreeMap<_, _> = recorded.drone_poses.iter().copied().collect();
            assert_eq!(drones.len(), recorded.drone_poses.len());
            for motion in &recorded.motions {
                assert!(recorded.inventory.operations.iter().any(|o| o.operation_id
                    == motion.operation_id
                    && o.drone_id == motion.drone_id));
                if motion.held {
                    assert!(motion.confirmed_at_ms <= recorded.sample_time_ms);
                } else {
                    assert_eq!(motion.confirmed_at_ms, recorded.sample_time_ms);
                }
                if let Some(pose) = motion.drone_pose {
                    assert_eq!(drones.get(&motion.drone_id), Some(&pose));
                }
                for (id, pose) in &motion.cassette_poses {
                    assert_eq!(cassettes.get(id), Some(pose));
                }
                for (id, pose) in &motion.machinery {
                    assert_eq!(roots.get(id), Some(pose));
                }
            }
            let mut parts = source.sample(&roots).unwrap();
            let mut rendered = BTreeSet::new();
            for entry in &recorded.inventory.entries {
                if matches!(entry.location, CarrierDroneLocation::Destroyed { .. }) {
                    continue;
                }
                let pose = *drones
                    .get(&entry.drone_id)
                    .expect("actual authority body pose");
                assert!(pose.valid());
                assert!(rendered.insert(entry.drone_id));
                if entry.location.has_active_space_body() {
                    let live = pilot
                        .drones
                        .iter()
                        .find(|d| d.id == entry.drone_id)
                        .unwrap();
                    assert_eq!(live.class, entry.class);
                    assert_eq!(live.hit_points, entry.hit_points);
                    let delta = recorded.parent.transform_vector(pose.position_m).unwrap();
                    for (i, value) in delta.iter().enumerate() {
                        let expected = recorded.parent.position_m[i] + value;
                        let tolerance =
                            expected.abs().max(live.position[i].abs()) * f64::EPSILON * 8. + 1e-6;
                        assert!(
                            (expected - live.position[i]).abs() <= tolerance,
                            "mixed authority body pose"
                        );
                    }
                    let expected = glam::DQuat::from_array(recorded.parent.orientation_xyzw)
                        * glam::DQuat::from_array(pose.orientation_xyzw);
                    let actual = glam::DQuat::from_array(live.orientation_xyzw.unwrap());
                    assert!((expected.dot(actual).abs() - 1.).abs() < 1e-9);
                } else {
                    let cassette = recorded
                        .inventory
                        .cassettes
                        .iter()
                        .find(|c| c.drone_id == Some(entry.drone_id))
                        .unwrap();
                    assert_eq!(
                        cassettes.get(&cassette.cassette_id),
                        Some(&pose),
                        "stored/loading body must follow actual cassette, not preferred slot"
                    );
                }
                parts.extend(drone_parts(entry.class, pose));
            }
            assert_eq!(rendered.len(), drones.len());
            assert_eq!(
                rendered.len(),
                recorded
                    .inventory
                    .entries
                    .iter()
                    .filter(|e| !matches!(e.location, CarrierDroneLocation::Destroyed { .. }))
                    .count()
            );
            geometry.parts = parts;
            gpu.replace(&geometry).unwrap();
            let mut row_cameras = cameras.clone();
            let mounted = mounted_device.as_ref().map(|selected| {
                use arkline_protocol::carrier_camera_devices::{
                    for_exact_source, CameraAvailability,
                };
                let public = pilot
                    .carrier_inventory
                    .as_ref()
                    .expect("actual carrier camera projection");
                public
                    .validate_projection(pilot)
                    .expect("actual source/clock/power/root camera binding");
                assert_eq!(public.sample_time_ms, recorded.sample_time_ms);
                let cameras = public
                    .cameras
                    .as_ref()
                    .expect("actual installed camera state");
                let camera_definition = for_exact_source(
                    registration.assembly_revision,
                    &registration.assembly_source_sha256,
                )
                .unwrap();
                let device = camera_definition
                    .devices
                    .iter()
                    .find(|d| &d.id == selected)
                    .unwrap();
                let state = cameras
                    .devices
                    .iter()
                    .find(|d| &d.device_id == selected)
                    .unwrap();
                assert_eq!(state.availability, CameraAvailability::Available);
                let parent = state.parent_world_pose.unwrap();
                let body_q = glam::DQuat::from_array(recorded.parent.orientation_xyzw);
                let parent_q = glam::DQuat::from_array(parent.orientation_xyzw);
                let to_body = |point: [f64; 3]| {
                    let world = glam::DVec3::from_array(parent.position_m)
                        + parent_q * glam::DVec3::from_array(point);
                    (body_q.conjugate()
                        * (world - glam::DVec3::from_array(recorded.parent.position_m)))
                    .as_vec3()
                    .to_array()
                };
                let forward = (glam::DVec3::from_array(device.look_at_m)
                    - glam::DVec3::from_array(device.eye_m))
                .normalize();
                let up = forward.cross(glam::DVec3::Y).normalize().cross(forward);
                let up = (body_q.conjugate() * parent_q * up).as_vec3();
                let eye = to_body(device.eye_m);
                let look = to_body(device.look_at_m);
                row_cameras.push(("crane-mounted", eye, look, 0.));
                (
                    up,
                    device.vertical_fov_radians as f32,
                    device.near_m as f32,
                    device.far_m as f32,
                )
            });
            for &(name, eye, look, half_width) in &row_cameras {
                let eye = Vec3::from_array(eye);
                let half_height = half_width * H as f32 / W as f32;
                let perspective_fov = if name == "crane-mounted" {
                    mounted.map(|m| m.1.to_degrees())
                } else if name == "magazine-interior" {
                    interior_fov
                } else {
                    None
                };
                let projection = if let Some(fov) = perspective_fov {
                    if name == "crane-mounted" {
                        let m = mounted.unwrap();
                        Mat4::perspective_rh_gl(m.1, 16. / 9., m.2, m.3)
                    } else {
                        Mat4::perspective_rh_gl(fov.to_radians(), W as f32 / H as f32, 0.02, 500.)
                    }
                } else {
                    Mat4::orthographic_rh_gl(
                        -half_width,
                        half_width,
                        -half_height,
                        half_height,
                        0.02,
                        500.,
                    )
                };
                let up = if name == "crane-mounted" {
                    mounted.unwrap().0
                } else {
                    Vec3::Y
                };
                let vp = projection * Mat4::look_at_rh(eye, Vec3::from_array(look), up);
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
                if name == "crane-mounted" {
                    gl.viewport(0, 50, W as i32, 540);
                } else {
                    gl.viewport(0, 0, W as i32, H as i32);
                }
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
                captures.push(serde_json::json!({"file":file,"index":row["index"],"elapsed_ms":row["elapsed_ms"],"sample_time_ms":recorded.sample_time_ms,"authority_sha256":hash,"png_sha256":hex::encode(Sha256::digest(std::fs::read(output.join(&file)).unwrap())),"camera":name,"mounted_device":if name == "crane-mounted" {mounted_device.as_deref()} else {None},"mounted_viewport":if name == "crane-mounted" {Some([0,50,960,540])} else {None},"up":up.to_array(),"eye_m":eye.to_array(),"look_m":look,"half_width_m":half_width,"projection":if perspective_fov.is_some(){"perspective"}else{"orthographic"},"vertical_fov_degrees":perspective_fov,"ship_ore":pilot.ore,"ore":recorded.ore,"inventory":recorded.inventory,"rendered_ids":rendered,"hull_hidden":false}));
            }
        }
        std::fs::write(output.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"scope":"Native imported-source documentary of unchanged committed stationary fixture; not captain POV, normal flyable admission, audio or performance acceptance","renderer_source":std::env::var("ARKLINE_CAPTURE_SOURCE").unwrap(),"source_receipt_sha256":hex::encode(Sha256::digest(receipt_bytes)),"assembly":definition.topology.registration,"size":[W,H],"diagnostic_lighting":diagnostic,"captures":captures})).unwrap()).unwrap();
        gpu.clear();
        gl.delete_program(gpu.program);
        target.destroy(&gl);
    }
}
