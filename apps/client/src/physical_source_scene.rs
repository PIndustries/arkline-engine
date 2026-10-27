//! Exact source presentation for the explicitly admitted single physical bay.
//! Rendering never advances machinery, predicts contact, or changes cargo.
use super::*;
use arkline_protocol::{
    carrier_inventory_geometry as geometry,
    drone_bay_geometry::BayPose,
    physical_cargo::{control::PhysicalControlPublicView, source::physical_source_configuration},
    CarrierInventoryAction, DroneClass,
};
use std::collections::{BTreeMap, BTreeSet};

const DRONE: &[u8] =
    include_bytes!("../../../assets/ships/pioneer/carrier-magazine-v1704/mining-drone.glb");
const DRONE_SOURCE: &[u8] =
    include_bytes!("../../../assets/ships/pioneer/carrier-magazine-v1704/mining-drone-source.json");

#[derive(Clone)]
pub(super) struct DroneGeometry {
    pub parts: Vec<ModelPart>,
    pub valves: Vec<(usize, f32)>,
    pub materials: Vec<(usize, [f32; 4])>,
}

/// Append after every pre-existing catalog slot. Source ranges preserve every
/// original triangle exactly once, including both explicitly moving inlet panels.
pub(super) fn append_drone(upload: &mut Vec<Vec<Vertex>>) -> Result<DroneGeometry, String> {
    let source = physical_source_configuration().ok_or("Physical source registry unavailable")?;
    if !source.validates_asset_bytes("drone_glb", DRONE)
        || !source.validates_asset_bytes("drone_source", DRONE_SOURCE)
    {
        return Err("Physical drone bytes do not match the measured source".into());
    }
    let primitives = ship_asset::decode_physical_drone(DRONE)?;
    let document: serde_json::Value =
        serde_json::from_slice(DRONE_SOURCE).map_err(|e| e.to_string())?;
    let ranges = document["parts"]
        .as_array()
        .ok_or("Missing drone source parts")?;
    let mut used: Vec<Vec<bool>> = primitives
        .iter()
        .map(|p| vec![false; p.vertices.len()])
        .collect();
    let mut geometry = DroneGeometry {
        parts: vec![],
        valves: vec![],
        materials: vec![],
    };
    let mut panel_ids = BTreeSet::new();
    for range in ranges {
        let group = range["group"].as_str().ok_or("Missing source group")?;
        if !group.starts_with("valve/") {
            continue;
        }
        let sign = match group {
            "valve/-1" => -1.,
            "valve/1" => 1.,
            _ => return Err("Unknown physical inlet group".into()),
        };
        if !panel_ids.insert(group) {
            return Err("Duplicate physical inlet group".into());
        }
        let index = range["batch"].as_u64().ok_or("Missing panel batch")? as usize;
        let start = range["vertex_start"]
            .as_u64()
            .ok_or("Missing panel range")? as usize;
        let count = range["vertex_count"]
            .as_u64()
            .ok_or("Missing panel count")? as usize;
        let end = start.checked_add(count).ok_or("Panel range overflow")?;
        if count == 0 || start % 3 != 0 || count % 3 != 0 {
            return Err("Invalid panel triangles".into());
        }
        let material = primitives
            .get(index)
            .ok_or("Panel primitive out of bounds")?;
        let vertices = material
            .vertices
            .get(start..end)
            .ok_or("Panel range out of bounds")?;
        let flags = used[index]
            .get_mut(start..end)
            .ok_or("Panel range out of bounds")?;
        if flags.iter().any(|x| *x) {
            return Err("Panel source overlap".into());
        }
        flags.fill(true);
        let mesh = upload.len();
        upload.push(vertices.to_vec());
        geometry.parts.push((
            mesh,
            Mat4::from_scale(Vec3::splat(100.)),
            material.color,
            material.emission,
        ));
        geometry
            .materials
            .push((mesh, [material.roughness, material.metallic, 0., 0.]));
        geometry.valves.push((mesh, sign));
    }
    if panel_ids.len() != 2 {
        return Err("Physical inlet panels missing".into());
    }
    for (material, flags) in primitives.iter().zip(used) {
        let vertices: Vec<_> = material
            .vertices
            .iter()
            .zip(flags)
            .filter_map(|(v, m)| (!m).then_some(*v))
            .collect();
        if vertices.is_empty() {
            continue;
        }
        let mesh = upload.len();
        upload.push(vertices);
        geometry.parts.push((
            mesh,
            Mat4::from_scale(Vec3::splat(100.)),
            material.color,
            material.emission,
        ));
        geometry
            .materials
            .push((mesh, [material.roughness, material.metallic, 0., 0.]));
    }
    let triangles: usize = geometry.parts.iter().map(|p| upload[p.0].len() / 3).sum();
    if triangles != 2064 {
        return Err("Physical drone triangle conservation failed".into());
    }
    Ok(geometry)
}

/// Reproduce only the source presentation sample, never advance authority.
fn validate_source_frame(view: &PhysicalControlPublicView) -> Result<(), String> {
    use arkline_protocol::physical_cargo::bay::PhysicalBayRoute as Route;
    let state = view.bay_state.as_ref().ok_or("Missing bay state")?;
    let registration = carrier_magazine_source::Source::registration(1704)?;
    let definition =
        geometry::stationary_fixture_for_registration(&registration).ok_or("Missing source")?;
    let (route, index) = match state.route {
        Some(r) => (r, usize::from(state.frame_index)),
        None => (
            if state.deployed {
                Route::Launch
            } else {
                Route::Return
            },
            6,
        ),
    };
    let frames = match route {
        Route::Service => definition
            .physical_inspection_service_route(0, DroneClass::Mining, 0)
            .ok_or("Missing service")?,
        _ => definition
            .bay_route(
                0,
                DroneClass::Mining,
                0,
                if route == Route::Launch {
                    CarrierInventoryAction::Launch
                } else {
                    CarrierInventoryAction::RecallToStorage
                },
            )
            .ok_or("Missing route")?
            .into_iter()
            .map(|frame| geometry::PhysicalServiceFrame {
                frame,
                drone_inlet_travel_mm: 0,
            })
            .collect(),
    };
    let mut current = frames
        .get(index)
        .ok_or("Invalid source frame index")?
        .clone();
    let mut inlet = f64::from(current.drone_inlet_travel_mm);
    if state.segment_elapsed_ms > 0 {
        if state.binding.segment_duration_ms == 0
            || state.segment_elapsed_ms >= state.binding.segment_duration_ms
        {
            return Err("Invalid source segment time".into());
        }
        let next = frames.get(index + 1).ok_or("Missing source endpoint")?;
        let u = state.segment_elapsed_ms as f64 / state.binding.segment_duration_ms as f64;
        let t = u * u * (3.0 - 2.0 * u);
        let mix = |a: BayPose, b: BayPose| -> Result<BayPose, String> {
            if a.orientation_xyzw != b.orientation_xyzw {
                return Err("Uncertified source rotation".into());
            }
            Ok(BayPose {
                position_m: std::array::from_fn(|i| {
                    a.position_m[i] + (b.position_m[i] - a.position_m[i]) * t
                }),
                orientation_xyzw: a.orientation_xyzw,
            })
        };
        current.frame.drone_pose = Some(mix(
            current.frame.drone_pose.ok_or("Missing drone")?,
            next.frame.drone_pose.ok_or("Missing next drone")?,
        )?);
        for (name, pose) in &mut current.frame.machinery {
            *pose = mix(
                *pose,
                next.frame
                    .machinery
                    .iter()
                    .find(|(n, _)| n == name)
                    .ok_or("Missing mechanism endpoint")?
                    .1,
            )?;
        }
        current.frame.milestones = Default::default();
        inlet += (f64::from(next.drone_inlet_travel_mm) - inlet) * t;
    }
    if current.frame != view.source_frame || inlet != view.drone_inlet_travel_mm {
        return Err("Source frame or inlet differs from authoritative route state".into());
    }
    Ok(())
}

#[derive(Clone)]
pub(super) struct Scene {
    pub owner: String,
    pub sample_time_ms: u64,
    pub drone_id: u64,
    pub view: PhysicalControlPublicView,
    roots: BTreeMap<String, BayPose>,
    drone_pose: BayPose,
    inlet_mm: f64,
    alive: bool,
}
impl Scene {
    pub fn from_view(
        snapshot: &Snapshot,
        view: &PhysicalControlPublicView,
        owner: &str,
    ) -> Result<Self, String> {
        crate::physical_cargo_ui::validate(view, snapshot, owner)?;
        let registration = carrier_magazine_source::Source::registration(1704)?;
        let definition = geometry::stationary_fixture_for_registration(&registration)
            .ok_or("Missing exact1704 geometry")?;
        validate_source_frame(view)?;
        let mut cassettes: Vec<_> = definition
            .cassettes
            .iter()
            .map(|c| (c.cassette_id, c.reference_pose))
            .collect();
        let mut cassette_ids = BTreeSet::new();
        for (id, pose) in &view.source_frame.cassette_poses {
            if !cassette_ids.insert(*id) || !pose.valid() {
                return Err("Invalid physical cassette roots".into());
            }
            let row = cassettes
                .iter_mut()
                .find(|(i, _)| i == id)
                .ok_or("Unknown physical cassette")?;
            row.1 = *pose;
        }
        let mut roots: BTreeMap<_, _> = definition
            .idle_roots(&cassettes)
            .ok_or("Frozen source roots unavailable")?
            .into_iter()
            .collect();
        let keys: BTreeSet<_> = view
            .source_frame
            .machinery
            .iter()
            .map(|(id, _)| id.clone())
            .collect();
        if keys.len() != view.source_frame.machinery.len() {
            return Err("Duplicate physical source root".into());
        }
        let launch = definition
            .bay_route(0, DroneClass::Mining, 0, CarrierInventoryAction::Launch)
            .ok_or("Missing physical launch")?;
        let service = definition
            .physical_inspection_service_route(0, DroneClass::Mining, 0)
            .ok_or("Missing physical service")?;
        let allowed = launch
            .iter()
            .map(|f| &f.machinery)
            .chain(service.iter().map(|f| &f.frame.machinery))
            .any(|row| {
                row.iter()
                    .map(|(id, _)| id.clone())
                    .collect::<BTreeSet<_>>()
                    == keys
            });
        if !allowed {
            return Err("Incomplete or unrelated physical motion roots".into());
        }
        for (id, pose) in &view.source_frame.machinery {
            if !pose.valid() || pose.position_m.iter().any(|v| !(*v as f32).is_finite()) {
                return Err("Invalid physical mechanism pose".into());
            }
            *roots.get_mut(id).ok_or("Unknown physical mechanism root")? = *pose;
        }
        if roots.len() != 500 {
            return Err("Physical complete source root count mismatch".into());
        }
        let drone_pose = view
            .source_frame
            .drone_pose
            .ok_or("Missing physical drone pose")?;
        if !drone_pose.valid()
            || !view.drone_inlet_travel_mm.is_finite()
            || !(0. ..=250.).contains(&view.drone_inlet_travel_mm)
        {
            return Err("Invalid physical body or inlet pose".into());
        }
        Ok(Self {
            view: view.clone(),
            owner: owner.into(),
            sample_time_ms: snapshot.server_time_ms,
            drone_id: view.state.binding.drone_id,
            roots,
            drone_pose,
            inlet_mm: view.drone_inlet_travel_mm,
            alive: view.state.projection.hit_points > 0,
        })
    }
    /// Fixed source aperture and stationary ray from the admitted bay certificate.
    /// No articulated aim, speculative cycle or moving-target beam.
    pub fn mining_beam(&self) -> Option<([f64; 3], [f64; 3])> {
        let p = &self.view.state.projection;
        let b = self.view.bay_state.as_ref()?;
        if !self.alive
            || self.sample_time_ms.saturating_sub(p.accepted_at_ms) > 1500
            || !p.mining
            || !p.target_locked
            || !p.signal_available
            || p.mining_cycle.is_none()
            || p.resource_remaining_cm3.0 == 0
            || !b.deployed
            || b.route.is_some()
            || b.held
        {
            return None;
        }
        let cert: serde_json::Value=serde_json::from_slice(include_bytes!("../../../crates/arkline-protocol/src/physical_cargo/bay/selected-bay-source-certificate.json")).ok()?;
        let point = |key: &str| -> Option<[f64; 3]> {
            let a = cert.get("mining_fixture")?.get(key)?.as_array()?;
            Some([
                a.first()?.as_f64()? / 1000.,
                a.get(1)?.as_f64()? / 1000.,
                a.get(2)?.as_f64()? / 1000.,
            ])
        };
        let local = point("muzzle_local_mm")?;
        let transformed =
            matrix(self.drone_pose).transform_point3(Vec3::from_array(local.map(|v| v as f32)));
        let start = point("muzzle_world_mm")?;
        if transformed.distance(Vec3::from_array(start.map(|v| v as f32))) > 0.00001 {
            return None;
        }
        Some((start, point("beam_target_near_mm")?))
    }
    pub fn matches(&self, pilot: &PilotView, sample_time: u64) -> bool {
        self.owner == pilot.name
            && self.sample_time_ms == sample_time
            && pilot.ship_class == ShipClass::Pioneer
    }
    /// Body metres: source ship and source drone share color/depth/ray geometry.
    pub fn parts(&self, include_drone: bool) -> Result<Vec<ModelPart>, String> {
        let g = physical_geometry()?;
        let carrier = g
            .carriers
            .iter()
            .filter_map(|c| c.as_ref().ok())
            .find(|c| c.registration.assembly_revision == 1704)
            .ok_or("Exact physical GPU catalog unavailable")?;
        let mut parts = Vec::with_capacity(carrier.parts.len() + 20);
        for &(mesh, model, color, emission) in carrier.parts.iter() {
            let model = if let Some(m) = carrier.moving.iter().find(|m| m.mesh == mesh) {
                matrix(
                    *self
                        .roots
                        .get(&m.group_id)
                        .ok_or("Missing physical GPU root")?,
                ) * matrix(m.reference_pose).inverse()
                    * model
            } else {
                model
            };
            if !model.is_finite() {
                return Err("Invalid physical GPU matrix".into());
            }
            parts.push((mesh, model, color, emission));
        }
        if include_drone && self.alive {
            let drone = g.physical_drone.as_ref().map_err(Clone::clone)?;
            for &(mesh, local, color, emission) in &drone.parts {
                let movement = drone.valves.iter().find(|(m, _)| *m == mesh).map_or(
                    Mat4::IDENTITY,
                    |(_, sign)| {
                        Mat4::from_translation(Vec3::Z * (*sign * self.inlet_mm as f32 / 1000.))
                    },
                );
                parts.push((
                    mesh,
                    matrix(self.drone_pose) * movement * local,
                    color,
                    emission,
                ));
            }
        }
        Ok(parts)
    }
}
fn matrix(pose: BayPose) -> Mat4 {
    Mat4::from_rotation_translation(
        Quat::from_array(pose.orientation_xyzw.map(|x| x as f32)).normalize(),
        Vec3::from_array(pose.position_m.map(|x| x as f32)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_physical_assets_preserve_triangle_partition_and_valves() {
        let registration = carrier_magazine_source::Source::registration(1704).unwrap();
        let source = carrier_magazine_source::Source::load_for_registration(&registration).unwrap();
        assert_eq!(source.reference_roots().len(), 500);
        assert_eq!(
            source
                .primitives
                .iter()
                .map(|p| p.vertices.len() / 3)
                .sum::<usize>(),
            199_632
        );
        assert_eq!(
            source
                .partition
                .moving
                .iter()
                .map(|p| p.vertices.len() / 3)
                .sum::<usize>(),
            62_220
        );
        assert_eq!(
            source
                .partition
                .fixed
                .iter()
                .map(|p| p.len() / 3)
                .sum::<usize>(),
            137_412
        );
        let mut upload = vec![];
        let drone = append_drone(&mut upload).unwrap();
        assert_eq!(upload.iter().map(|p| p.len() / 3).sum::<usize>(), 2064);
        assert_eq!(drone.valves.len(), 2);
        assert_eq!(drone.valves.iter().map(|(_, s)| *s).sum::<f32>(), 0.);
        for (mesh, sign) in drone.valves {
            assert_eq!(upload[mesh].len(), 36);
            let closed = Mat4::from_scale(Vec3::splat(100.));
            let open = Mat4::from_translation(Vec3::Z * sign * 0.25) * closed;
            for vertex in &upload[mesh] {
                assert!(
                    (open.transform_point3(vertex.p)
                        - closed.transform_point3(vertex.p)
                        - Vec3::Z * sign * 0.25)
                        .length()
                        < 1e-6
                );
            }
        }
        assert!(ship_asset::decode(DRONE).is_err()); // Legacy decoder contract retained.
        let mut invalid = DRONE.to_vec();
        invalid[100] ^= 1;
        assert!(ship_asset::decode_physical_drone(&invalid).is_err());
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "Explicit physical render CPU profiling, not gameplay timing"]
    fn physical_render_cpu_preparation_profile() {
        let response = physical_cargo_native_tests::initial_response_for_tests();
        let snapshot = response.snapshot.unwrap();
        let view = response.physical_cargo.unwrap();
        let scene = Scene::from_view(&snapshot, &view, &view.state.binding.pilot).unwrap();
        let parts = scene.parts(true).unwrap();
        let measure = |mut work: Box<dyn FnMut() + '_>| {
            let mut samples = Vec::new();
            for _ in 0..120 {
                let start = std::time::Instant::now();
                work();
                samples.push(start.elapsed().as_secs_f64() * 1000.);
            }
            samples.sort_by(f64::total_cmp);
            serde_json::json!({"median_ms":samples[60],"p95_ms":samples[114],"samples_ms":samples})
        };
        let resolved = measure(Box::new(|| {
            std::hint::black_box(scene.parts(true).unwrap());
        }));
        let materials = measure(Box::new(|| {
            for p in &parts {
                std::hint::black_box(resolved_material_parameters(p.0, Material::BareMetal));
            }
        }));
        let embedded = measure(Box::new(|| {
            for p in &parts {
                std::hint::black_box(embedded_primitive(p.0));
            }
        }));
        let glass = measure(Box::new(|| {
            for p in &parts {
                std::hint::black_box(is_registered_glass(p.0));
            }
        }));
        let output = std::env::var_os("ARKLINE_RENDER_CPU_OUTPUT").unwrap();
        std::fs::write(output, serde_json::to_vec_pretty(&serde_json::json!({"part_count":parts.len(),"scope":"CPU full physical source-part preparation and lookup; no GL, culling or FPS claim","parts":resolved,"materials":materials,"embedded":embedded,"glass":glass})).unwrap()).unwrap();
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn authority_scene_rejects_source_roots_clock_and_inlet_mismatch() {
        let response = physical_cargo_native_tests::initial_response_for_tests();
        let snapshot = response.snapshot.unwrap();
        let view = response.physical_cargo.unwrap();
        let owner = view.state.binding.pilot.clone();
        let scene = Scene::from_view(&snapshot, &view, &owner).unwrap();
        assert_eq!(scene.roots.len(), 500);
        assert!(scene.parts(true).unwrap().len() > scene.parts(false).unwrap().len());
        let mut bad = view.clone();
        bad.source_frame.machinery.pop();
        assert!(Scene::from_view(&snapshot, &bad, &owner).is_err());
        let mut bad = view.clone();
        bad.source_frame
            .machinery
            .push(bad.source_frame.machinery[0].clone());
        assert!(Scene::from_view(&snapshot, &bad, &owner).is_err());
        let mut bad = view.clone();
        bad.drone_inlet_travel_mm = 250.001;
        assert!(Scene::from_view(&snapshot, &bad, &owner).is_err());
        let mut bad = view.clone();
        bad.source_frame.machinery[0].1.position_m[0] += 0.001;
        assert!(Scene::from_view(&snapshot, &bad, &owner).is_err());
        let mut bad = view.clone();
        bad.drone_inlet_travel_mm = 1.;
        assert!(Scene::from_view(&snapshot, &bad, &owner).is_err());
        let mut bad = view.clone();
        bad.source_configuration_sha256 = "0".repeat(64);
        assert!(Scene::from_view(&snapshot, &bad, &owner).is_err());
        let mut stale = view.clone();
        stale.state.projection.accepted_at_ms = snapshot.server_time_ms + 1;
        assert!(Scene::from_view(&snapshot, &stale, &owner).is_err());
        let mut flight = FlightView::new(None);
        flight
            .observe_physical(&snapshot, Some(&view), &owner)
            .unwrap();
        assert!(flight.physical.is_some());
        assert!(flight
            .observe_physical(&snapshot, Some(&bad), &owner)
            .is_err());
        assert!(flight.physical.is_none());
        flight
            .observe_physical(&snapshot, Some(&view), &owner)
            .unwrap();
        flight.clear_physical();
        assert!(flight.physical.is_none());
    }
}
