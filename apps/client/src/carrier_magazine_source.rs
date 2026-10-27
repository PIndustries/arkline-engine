//! Exact stationary-fixture source and machinery sampling. Normal world
//! registration is deliberately unavailable until full variant consumers pass.
use super::*;
use arkline_protocol::drone_bay_geometry::BayPose;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const SOURCE: &[u8] =
    include_bytes!("../../../assets/ships/pioneer/carrier-magazine-v1701/pioneer.glb");
const BINDING: &str =
    include_str!("../../../assets/ships/pioneer/carrier-magazine-v1701/runtime-binding.json");
const COMPOSED_SOURCE: &[u8] =
    include_bytes!("../../../assets/ships/pioneer/carrier-magazine-v1702/pioneer.glb");
const COMPOSED_BINDING: &str =
    include_str!("../../../assets/ships/pioneer/carrier-magazine-v1702/runtime-binding.json");
const PARALLEL_SOURCE: &[u8] =
    include_bytes!("../../../assets/ships/pioneer/carrier-magazine-v1703/pioneer.glb");
const PARALLEL_BINDING: &str =
    include_str!("../../../assets/ships/pioneer/carrier-magazine-v1703/runtime-binding.json");
const PHYSICAL_SOURCE: &[u8] =
    include_bytes!("../../../assets/ships/pioneer/carrier-magazine-v1704/pioneer.glb");
const PHYSICAL_BINDING: &str =
    include_str!("../../../assets/ships/pioneer/carrier-magazine-v1704/runtime-binding.json");
const PHYSICAL_BINDING_SHA: &str =
    "a76a0ac0b108a7905c7caa2a5cc76621c6682a21a72a3b15cc95792f54355534";
const PARALLEL_BINDING_SHA: &str =
    "fd3177e098549fae579bf70465581e8a5fbfe7adf040c9c6c5cd3707d4952776";
const COMPOSED_BINDING_SHA: &str =
    "583e8cb4dcc151ffedcd727f9f03498a892ad361cc9685554a2c103944ff0f7a";
const BINDING_SHA: &str = "dba386f0498cb34418a91ddd0a031e372b6aecb775e531da5fccbba26ffd38a0";

pub(super) struct Source {
    pub(super) primitives: Vec<ship_asset::Primitive>,
    pub(super) partition: pioneer_bay_source::Partition,
    references: BTreeMap<String, BayPose>,
}
impl Source {
    pub(super) fn registration(
        revision: u32,
    ) -> Result<arkline_protocol::CarrierInventoryRegistration, String> {
        // Only this embedded, immutable source is cached. Return an owned clone so
        // callers cannot alter the verified identity. Cache failure as well as
        // success: a bad compiled source must never become an admitted fixture.
        if revision == 1704 {
            static PHYSICAL_REGISTRATION: OnceLock<
                Result<arkline_protocol::CarrierInventoryRegistration, String>,
            > = OnceLock::new();
            return Self::cached_registration(&PHYSICAL_REGISTRATION, || {
                Self::registration_uncached(1704)
            });
        }
        Self::registration_uncached(revision)
    }
    fn cached_registration(
        cache: &OnceLock<Result<arkline_protocol::CarrierInventoryRegistration, String>>,
        initialize: impl FnOnce() -> Result<arkline_protocol::CarrierInventoryRegistration, String>,
    ) -> Result<arkline_protocol::CarrierInventoryRegistration, String> {
        cache.get_or_init(initialize).clone()
    }
    fn registration_uncached(
        revision: u32,
    ) -> Result<arkline_protocol::CarrierInventoryRegistration, String> {
        let mut registration =
            arkline_protocol::carrier_inventory_geometry::stationary_fixture_definition()
                .ok_or("Carrier source fixture unavailable")?
                .topology
                .registration
                .clone();
        registration.assembly_revision = revision;
        registration.assembly_source_sha256 = match revision {
            1701 => hex::encode(Sha256::digest(SOURCE)),
            1702 => hex::encode(Sha256::digest(COMPOSED_SOURCE)),
            1703 => hex::encode(Sha256::digest(PARALLEL_SOURCE)),
            1704 => hex::encode(Sha256::digest(PHYSICAL_SOURCE)),
            _ => return Err("Unsupported carrier source revision".into()),
        };
        arkline_protocol::carrier_inventory_geometry::stationary_fixture_for_registration(
            &registration,
        )
        .ok_or("Installed carrier source is not an exact compiled registration")?;
        Ok(registration)
    }
    #[cfg(test)]
    pub(super) fn installed_bytes_for_registration(
        registration: &arkline_protocol::CarrierInventoryRegistration,
    ) -> Result<&'static [u8], String> {
        if Self::registration(registration.assembly_revision)? != *registration {
            return Err("Installed carrier bytes do not match exact registration".into());
        }
        match registration.assembly_revision {
            1701 => Ok(SOURCE),
            1702 => Ok(COMPOSED_SOURCE),
            1703 => Ok(PARALLEL_SOURCE),
            1704 => Ok(PHYSICAL_SOURCE),
            _ => Err("Unsupported carrier source revision".into()),
        }
    }
    #[cfg(test)]
    pub(super) fn load() -> Result<Self, String> {
        let definition =
            arkline_protocol::carrier_inventory_geometry::stationary_fixture_definition()
                .ok_or("Carrier magazine fixture unavailable")?;
        Self::load_for_registration(&definition.topology.registration)
    }
    pub(super) fn load_for_registration(
        registration: &arkline_protocol::CarrierInventoryRegistration,
    ) -> Result<Self, String> {
        let definition =
            arkline_protocol::carrier_inventory_geometry::stationary_fixture_for_registration(
                registration,
            )
            .ok_or("Unsupported exact carrier source registration")?;
        let (bytes, binding, binding_sha) = match registration.assembly_revision {
            1701 => (SOURCE, BINDING, BINDING_SHA),
            1702 => (COMPOSED_SOURCE, COMPOSED_BINDING, COMPOSED_BINDING_SHA),
            1703 => (PARALLEL_SOURCE, PARALLEL_BINDING, PARALLEL_BINDING_SHA),
            1704 => (PHYSICAL_SOURCE, PHYSICAL_BINDING, PHYSICAL_BINDING_SHA),
            _ => return Err("Unsupported carrier source revision".into()),
        };
        Self::verify_source_bytes(registration, bytes, binding, binding_sha)?;
        let primitives = ship_asset::decode(bytes)?;
        let partition = pioneer_bay_source::partition(
            &primitives
                .iter()
                .map(|p| p.vertices.clone())
                .collect::<Vec<_>>(),
            binding,
            &definition.topology.registration.assembly_source_sha256,
            definition.topology.registration.assembly_revision,
        )?;
        let references: BTreeMap<_, _> = definition
            .mechanisms
            .iter()
            .map(|m| (m.semantic_id.clone(), m.reference_pose))
            .collect();
        if partition
            .moving
            .iter()
            .any(|m| references.get(&m.group_id) != Some(&m.reference_pose))
            || references
                .keys()
                .any(|id| !partition.moving.iter().any(|m| &m.group_id == id))
        {
            return Err("Carrier magazine source roots disagree with authority fixture".into());
        }
        Ok(Self {
            primitives,
            partition,
            references,
        })
    }
    // Independently verify supplied source bytes on every load. Registration
    // caching must not turn caller records or alternate bytes into trusted data.
    fn verify_source_bytes(
        registration: &arkline_protocol::CarrierInventoryRegistration,
        bytes: &[u8],
        binding: &str,
        binding_sha: &str,
    ) -> Result<(), String> {
        if hex::encode(Sha256::digest(bytes)) != registration.assembly_source_sha256
            || hex::encode(Sha256::digest(binding.as_bytes())) != binding_sha
        {
            return Err("Carrier magazine exact source binding mismatch".into());
        }
        Ok(())
    }
    pub(super) fn reference_roots(&self) -> &BTreeMap<String, BayPose> {
        &self.references
    }
    /// A single resolved vertex set is suitable for native color, depth and
    /// triangle-ray evidence. Every source triangle is retained exactly once.
    /// Values must be confirmed by authority before calling; this is not motion
    /// prediction, capture, cargo transfer or command admission.
    #[cfg(test)]
    pub(super) fn sample(
        &self,
        roots: &BTreeMap<String, BayPose>,
    ) -> Result<Vec<ship_asset::Primitive>, String> {
        if roots.len() != self.references.len()
            || self.references.keys().any(|id| {
                roots.get(id).is_none_or(|pose| {
                    !pose.valid() || pose.position_m.iter().any(|v| !(*v as f32).is_finite())
                })
            })
        {
            return Err("Incomplete or invalid carrier magazine mechanism roots".into());
        }
        let matrix = |pose: BayPose| {
            Mat4::from_rotation_translation(
                Quat::from_array(pose.orientation_xyzw.map(|x| x as f32)).normalize(),
                Vec3::from_array(pose.position_m.map(|x| x as f32)) / 100.,
            )
        };
        let mut result: Vec<_> = self
            .primitives
            .iter()
            .enumerate()
            .map(|(i, p)| ship_asset::Primitive {
                vertices: self.partition.fixed[i].clone(),
                material_index: p.material_index,
                material_name: p.material_name.clone(),
                color: p.color,
                emission: p.emission,
                emissive_color: p.emissive_color,
                roughness: p.roughness,
                metallic: p.metallic,
            })
            .collect();
        for moving in &self.partition.moving {
            let transform =
                matrix(roots[&moving.group_id]) * matrix(moving.reference_pose).inverse();
            result[moving.primitive]
                .vertices
                .extend(moving.vertices.iter().map(|v| Vertex {
                    p: transform.transform_point3(v.p),
                    n: transform.transform_vector3(v.n).normalize_or_zero(),
                }));
        }
        if result
            .iter()
            .flat_map(|p| &p.vertices)
            .any(|v| !v.p.is_finite() || !v.n.is_finite())
        {
            return Err("Nonfinite transformed carrier source vertex".into());
        }
        Ok(result)
    }
}

#[test]
fn exact_stationary_source_preserves_all_triangles_and_rejects_missing_roots() {
    let source = Source::load().unwrap();
    let sampled = source.sample(source.reference_roots()).unwrap();
    assert_eq!(
        sampled.iter().map(|p| p.vertices.len() / 3).sum::<usize>(),
        179_478
    );
    assert_eq!(source.reference_roots().len(), 330);
    let mut missing = source.reference_roots().clone();
    missing.pop_first();
    assert!(source.sample(&missing).is_err());
    let mut invalid = source.reference_roots().clone();
    invalid.values_mut().next().unwrap().position_m[0] = f64::NAN;
    assert!(source.sample(&invalid).is_err());
    invalid.values_mut().next().unwrap().position_m[0] = 1e100;
    assert!(source.sample(&invalid).is_err());
}

#[test]
fn confirmed_group_transform_is_applied_once_in_shared_vertex_space() {
    let source = Source::load().unwrap();
    let moving = source
        .partition
        .moving
        .iter()
        .find(|m| m.group_id == "b05/door/upper")
        .unwrap();
    let offset = source.partition.fixed[moving.primitive].len()
        + source
            .partition
            .moving
            .iter()
            .take_while(|m| !std::ptr::eq(*m, moving))
            .filter(|m| m.primitive == moving.primitive)
            .map(|m| m.vertices.len())
            .sum::<usize>();
    let rest = source.sample(source.reference_roots()).unwrap();
    for (original, resolved) in moving
        .vertices
        .iter()
        .zip(&rest[moving.primitive].vertices[offset..])
    {
        assert!((original.p - resolved.p).length() * 100.0 < 0.0001);
        assert!((original.n - resolved.n).length() < 1e-5);
    }
    let mut roots = source.reference_roots().clone();
    let reference = moving.reference_pose;
    let rotation =
        glam::DQuat::from_rotation_y(0.37) * glam::DQuat::from_array(reference.orientation_xyzw);
    let target = BayPose {
        position_m: std::array::from_fn(|i| reference.position_m[i] + [1.2, -0.4, 0.7][i]),
        orientation_xyzw: rotation.to_array(),
    };
    roots.insert(moving.group_id.clone(), target);
    let shifted = source.sample(&roots).unwrap();
    for (original, resolved) in moving
        .vertices
        .iter()
        .zip(&shifted[moving.primitive].vertices[offset..])
    {
        let relative = glam::DQuat::from_array(reference.orientation_xyzw).inverse()
            * (original.p.as_dvec3() * 100.0 - glam::DVec3::from_array(reference.position_m));
        let expected = glam::DVec3::from_array(target.position_m) + rotation * relative;
        assert!((expected - resolved.p.as_dvec3() * 100.0).length() < 0.0001);
    }
}

#[test]
fn composed_source_is_separate_and_exact_without_changing_historical_source() {
    let mut registration =
        arkline_protocol::carrier_inventory_geometry::stationary_fixture_definition()
            .unwrap()
            .topology
            .registration
            .clone();
    let old = Source::load_for_registration(&registration).unwrap();
    registration.assembly_revision = 1702;
    registration.assembly_source_sha256 = hex::encode(Sha256::digest(COMPOSED_SOURCE));
    let new = Source::load_for_registration(&registration).unwrap();
    assert_eq!(old.reference_roots(), new.reference_roots());
    assert_eq!(
        new.sample(new.reference_roots())
            .unwrap()
            .iter()
            .map(|p| p.vertices.len() / 3)
            .sum::<usize>(),
        182_176
    );
    assert_eq!(
        old.primitives
            .iter()
            .map(|p| p.vertices.len() / 3)
            .sum::<usize>(),
        179_478
    );
    registration.assembly_source_sha256 = hex::encode(Sha256::digest(SOURCE));
    assert!(Source::load_for_registration(&registration).is_err());
}

#[test]
fn carrier_catalog_appends_exact_variants_without_aliasing_legacy_slots() {
    let geometry = flight_geometry().unwrap();
    let receiver = geometry.receiver.as_ref().unwrap();
    let legacy_last = receiver.parts.iter().map(|p| p.0).max().unwrap();
    let mut last = legacy_last;
    for (revision, triangles, roots) in [
        (1701, 179_478, 330),
        (1702, 182_176, 330),
        (1703, 193_192, 480),
    ] {
        let carrier = geometry
            .carriers
            .iter()
            .filter_map(|v| v.as_ref().ok())
            .find(|v| v.registration.assembly_revision == revision)
            .unwrap();
        assert_eq!(carrier.parts.first().unwrap().0, last + 1);
        assert_eq!(carrier.references.len(), roots);
        assert_eq!(
            carrier.warning_beacons.len(),
            if revision == 1703 { 10 } else { 0 }
        );
        for (mesh, _) in &carrier.warning_beacons {
            assert_eq!(geometry.catalog.vertex_count(*mesh), 36);
        }

        assert_eq!(carrier.parts.len(), carrier.materials.len());
        let source = Source::load_for_registration(&carrier.registration).unwrap();
        assert_eq!(
            source
                .primitives
                .iter()
                .map(|p| p.vertices.len() / 3)
                .sum::<usize>(),
            triangles
        );
        assert_eq!(
            carrier
                .parts
                .iter()
                .map(|p| geometry.catalog.vertex_count(p.0) / 3)
                .sum::<usize>(),
            triangles
        );
        let expected = source
            .partition
            .fixed
            .iter()
            .filter(|v| !v.is_empty())
            .chain(source.partition.moving.iter().map(|m| &m.vertices));
        for (part, vertices) in carrier.parts.iter().zip(expected) {
            let uploaded = &geometry.catalog.uploads()[part.0];
            assert_eq!(uploaded.len(), vertices.len());
            for (a, b) in uploaded.iter().zip(vertices) {
                assert_eq!(
                    a.p.to_array().map(f32::to_bits),
                    b.p.to_array().map(f32::to_bits)
                );
                assert_eq!(
                    a.n.to_array().map(f32::to_bits),
                    b.n.to_array().map(f32::to_bits)
                );
            }
        }
        for &(mesh, params) in &carrier.materials {
            assert!(mesh > legacy_last);
            assert_eq!(
                resolved_material_parameters(mesh, Material::PaintedHull),
                params
            );
        }
        last = carrier.parts.last().unwrap().0;
    }
}

#[test]
#[ignore = "Opt-in exact source-bound public Response replay; no synthetic motion"]
fn actual_public_roots_resolve_same_catalog_for_color_depth_and_rays() {
    let input =
        std::path::PathBuf::from(std::env::var_os("ARKLINE_CARRIER_PUBLIC_RESPONSES").unwrap());
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(input.join("receipt.json")).unwrap()).unwrap();
    let geometry = flight_geometry().unwrap();
    for row in receipt["frames"].as_array().unwrap().iter().step_by(17) {
        let bytes = std::fs::read(input.join(row["file"].as_str().unwrap())).unwrap();
        assert_eq!(hex::encode(Sha256::digest(&bytes)), row["response_sha256"]);
        let response: arkline_protocol::Response = serde_json::from_slice(&bytes).unwrap();
        let snapshot = response.snapshot.unwrap();
        let pilot = snapshot
            .pilots
            .iter()
            .find(|p| p.name == receipt["pilot"].as_str().unwrap())
            .unwrap();
        let public = pilot.carrier_inventory.as_ref().unwrap();
        assert_eq!(public.sample_time_ms, snapshot.server_time_ms);
        let parts = carrier_body_parts(pilot, snapshot.server_time_ms).unwrap();
        let instances = carrier_scene::instances(
            pilot,
            Space {
                origin: public.parent.position_m,
            },
            snapshot.server_time_ms,
        )
        .unwrap();
        assert_eq!(instances.len(), 20);
        assert_eq!(
            instances
                .iter()
                .map(|i| i.0)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            20
        );
        assert!(
            carrier_scene::instances(pilot, Space::default(), snapshot.server_time_ms + 1)
                .is_none()
        );

        assert_eq!(
            parts
                .iter()
                .map(|p| geometry.catalog.vertex_count(p.0) / 3)
                .sum::<usize>(),
            179_478
        );
        for &(mesh, matrix, _, _) in &parts {
            let depth =
                cockpit_occlusion::DepthDraw::whole(&geometry.catalog, mesh, matrix).unwrap();
            assert_eq!(depth.mesh, mesh);
            assert_eq!(depth.model, matrix);
            for vertex in geometry.catalog.uploads()[mesh].iter().step_by(97) {
                assert!(matrix.transform_point3(vertex.p).is_finite());
            }
        }
        assert!(carrier_body_parts(pilot, snapshot.server_time_ms + 1).is_err());
        let source = Source::load_for_registration(&public.binding().registration).unwrap();
        let roots = public.machinery.iter().cloned().collect();
        let sampled = source.sample(&roots).unwrap();
        let carrier = geometry
            .carriers
            .iter()
            .filter_map(|v| v.as_ref().ok())
            .find(|c| c.registration == public.binding().registration)
            .unwrap();
        let mut offsets: Vec<_> = source.partition.fixed.iter().map(Vec::len).collect();
        for (group, mount) in source.partition.moving.iter().zip(&carrier.moving) {
            assert_eq!(group.group_id, mount.group_id);
            let part = parts.iter().find(|p| p.0 == mount.mesh).unwrap();
            for (index, vertex) in group.vertices.iter().enumerate().step_by(11) {
                let production = part.1.transform_point3(vertex.p);
                let documentary =
                    sampled[group.primitive].vertices[offsets[group.primitive] + index].p * 100.;
                assert!(
                    production.distance(documentary) < 0.0001,
                    "{}: {production:?} vs {documentary:?}",
                    group.group_id
                );
            }
            offsets[group.primitive] += group.vertices.len();
        }
        let mut bad = pilot.clone();
        bad.carrier_inventory.as_mut().unwrap().machinery.pop();
        assert!(carrier_body_parts(&bad, snapshot.server_time_ms).is_err());
        let mut foreign = pilot.clone();
        foreign
            .carrier_inventory
            .as_mut()
            .unwrap()
            .inventory
            .binding
            .registration
            .assembly_source_sha256 = "0".repeat(64);
        assert!(carrier_body_parts(&foreign, snapshot.server_time_ms).is_err());
    }
}

#[test]
fn parallel_carrier_source_uses_exact_idle_and_confirmed_local_loader_roots() {
    let registration = Source::registration(1703).unwrap();
    let definition =
        arkline_protocol::carrier_inventory_geometry::stationary_fixture_for_registration(
            &registration,
        )
        .unwrap();
    let source = Source::load_for_registration(&registration).unwrap();
    assert_eq!(source.reference_roots().len(), 480);
    assert_eq!(
        hex::encode(Sha256::digest(
            Source::installed_bytes_for_registration(&registration).unwrap()
        )),
        registration.assembly_source_sha256
    );
    let cassette_poses: Vec<_> = definition
        .cassettes
        .iter()
        .map(|c| (c.cassette_id, c.reference_pose))
        .collect();
    let idle: BTreeMap<_, _> = definition
        .idle_roots(&cassette_poses)
        .unwrap()
        .into_iter()
        .collect();
    assert_ne!(
        &idle,
        source.reference_roots(),
        "new shuttle has a source-authored non-reference rest pose"
    );
    let route = definition
        .stage_route(arkline_protocol::DroneClass::Mining, 0, 0, 10, 0)
        .unwrap();
    for index in [0, 20, 41] {
        let mut roots = idle.clone();
        for (id, pose) in &route[index].machinery {
            roots.insert(id.clone(), *pose);
        }
        let parts = source.sample(&roots).unwrap();
        assert_eq!(
            parts.iter().map(|p| p.vertices.len() / 3).sum::<usize>(),
            193_192
        );
        assert!(parts
            .iter()
            .flat_map(|p| &p.vertices)
            .all(|v| v.p.is_finite() && v.n.is_finite()));
    }
    let mut missing = idle.clone();
    missing.remove(idle.keys().next().unwrap());
    assert!(source.sample(&missing).is_err());
    let mut wrong = registration;
    wrong.assembly_source_sha256 = "0".repeat(64);
    assert!(Source::installed_bytes_for_registration(&wrong).is_err());
}

#[test]
fn physical_registration_cache_preserves_exact_identity_and_rejects_mutation() {
    let expected = Source::registration_uncached(1704).unwrap();
    assert_eq!(Source::registration(1704).unwrap(), expected);
    let mut altered = Source::registration(1704).unwrap();
    altered.assembly_source_sha256 = "0".repeat(64);
    assert!(Source::installed_bytes_for_registration(&altered).is_err());
    assert!(Source::load_for_registration(&altered).is_err());
    assert_eq!(Source::registration(1704).unwrap(), expected);
    altered = expected.clone();
    altered.assembly_revision = 1703;
    assert!(Source::installed_bytes_for_registration(&altered).is_err());
    assert!(Source::load_for_registration(&altered).is_err());
    altered = expected.clone();
    altered.host.source_sha256 = "0".repeat(64);
    assert!(Source::installed_bytes_for_registration(&altered).is_err());
    assert!(Source::load_for_registration(&altered).is_err());
    altered = expected.clone();
    altered.schema_version += 1;
    assert!(Source::installed_bytes_for_registration(&altered).is_err());
    assert!(Source::load_for_registration(&altered).is_err());
    for revision in [1701, 1702, 1703] {
        assert_eq!(
            Source::registration(revision),
            Source::registration_uncached(revision)
        );
    }
    assert!(Source::registration(1705).is_err());
}

#[test]
fn physical_registration_cache_does_not_skip_actual_source_byte_checks() {
    let registration = Source::registration(1704).unwrap();
    assert!(Source::verify_source_bytes(
        &registration,
        PHYSICAL_SOURCE,
        PHYSICAL_BINDING,
        PHYSICAL_BINDING_SHA
    )
    .is_ok());
    let mut altered = PHYSICAL_SOURCE.to_vec();
    altered[0] ^= 1;
    assert!(Source::verify_source_bytes(
        &registration,
        &altered,
        PHYSICAL_BINDING,
        PHYSICAL_BINDING_SHA
    )
    .is_err());
    assert!(Source::verify_source_bytes(
        &registration,
        PHYSICAL_SOURCE,
        "changed binding",
        PHYSICAL_BINDING_SHA
    )
    .is_err());
}

#[test]
fn physical_registration_cache_initialization_failure_stays_closed() {
    let cache = OnceLock::new();
    let expected = Err("Installed carrier source is not an exact compiled registration".into());
    assert_eq!(
        Source::cached_registration(&cache, || expected.clone()),
        expected
    );
    assert_eq!(
        Source::cached_registration(&cache, || panic!("failed cache retried")),
        expected
    );
}

#[test]
#[ignore = "bounded registration-only timing, not renderer frame rate"]
fn physical_registration_cache_timing() {
    use std::time::Instant;
    let expected = Source::registration_uncached(1704).unwrap();
    // Warm both fixture metadata and cache; compare repeated lookup cost, not
    // startup. The uncached branch is the unchanged pre-cache implementation.
    assert_eq!(Source::registration(1704).unwrap(), expected);
    let measure = |cached: bool, count: usize| {
        let mut samples = Vec::with_capacity(count);
        for _ in 0..count {
            let start = Instant::now();
            let result = if cached {
                Source::registration(1704)
            } else {
                Source::registration_uncached(1704)
            };
            let elapsed = start.elapsed().as_nanos();
            assert_eq!(std::hint::black_box(result).unwrap(), expected);
            samples.push(elapsed);
        }
        samples.sort_unstable();
        serde_json::json!({"calls": count, "total_ns": samples.iter().sum::<u128>(),
            "median_ns": samples[count / 2], "p95_ns": samples[(count * 95 / 100).min(count - 1)],
            "min_ns": samples[0], "max_ns": samples[count - 1]})
    };
    let report = serde_json::json!({"method": "warm registration-only; identical uncached implementation vs immutable cache; no renderer/FPS claim",
        "source_bytes": PHYSICAL_SOURCE.len(), "uncached": measure(false, 12), "cached": measure(true, 1000)});
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    if let Ok(path) = std::env::var("ARKLINE_REGISTRATION_TIMING_REPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}
