//! Exact stationary-fixture source and machinery sampling. Normal world
//! registration is deliberately unavailable until full variant consumers pass.
use super::*;
use arkline_protocol::drone_bay_geometry::BayPose;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const SOURCE: &[u8] =
    include_bytes!("../../../assets/ships/pioneer/independent-bays-v1601/pioneer.glb");
const BINDING: &str =
    include_str!("../../../assets/ships/pioneer/independent-bays-v1601/runtime-binding.json");
const BINDING_SHA: &str = "28ecbbd9fd59c9dfb5e8d2b23bfff5ad3cf13dcffb674e8458169c015c244eeb";

pub(super) struct Source {
    primitives: Vec<ship_asset::Primitive>,
    partition: pioneer_bay_source::Partition,
    references: BTreeMap<String, BayPose>,
}
impl Source {
    pub(super) fn load() -> Result<Self, String> {
        let definition =
            arkline_protocol::independent_bay_geometry::stationary_fixture_definition()
                .ok_or("Independent bay fixture unavailable")?;
        if hex::encode(Sha256::digest(SOURCE)) != definition.registration.assembly_source_sha256
            || hex::encode(Sha256::digest(BINDING.as_bytes())) != BINDING_SHA
        {
            return Err("Independent bay exact source binding mismatch".into());
        }
        let primitives = ship_asset::decode(SOURCE)?;
        let partition = pioneer_bay_source::partition(
            &primitives
                .iter()
                .map(|p| p.vertices.clone())
                .collect::<Vec<_>>(),
            BINDING,
            &definition.registration.assembly_source_sha256,
            definition.registration.assembly_revision,
        )?;
        let references: BTreeMap<_, _> = definition
            .bays
            .iter()
            .flat_map(|bay| {
                bay.mechanisms
                    .iter()
                    .map(|m| (m.semantic_id.clone(), m.reference_pose))
            })
            .collect();
        if partition
            .moving
            .iter()
            .any(|m| references.get(&m.group_id) != Some(&m.reference_pose))
            || references
                .keys()
                .any(|id| !partition.moving.iter().any(|m| &m.group_id == id))
        {
            return Err("Independent bay source roots disagree with authority fixture".into());
        }
        Ok(Self {
            primitives,
            partition,
            references,
        })
    }
    pub(super) fn reference_roots(&self) -> &BTreeMap<String, BayPose> {
        &self.references
    }
    /// A single resolved vertex set is suitable for native color, depth and
    /// triangle-ray evidence. Every source triangle is retained exactly once.
    /// Values must be confirmed by authority before calling; this is not motion
    /// prediction, capture, cargo transfer or command admission.
    pub(super) fn sample(
        &self,
        roots: &BTreeMap<String, BayPose>,
    ) -> Result<Vec<ship_asset::Primitive>, String> {
        if roots.len() != self.references.len()
            || self
                .references
                .keys()
                .any(|id| roots.get(id).is_none_or(|pose| !pose.valid()))
        {
            return Err("Incomplete or invalid independent bay mechanism roots".into());
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
        Ok(result)
    }
}

#[test]
fn exact_stationary_source_preserves_all_triangles_and_rejects_missing_roots() {
    let source = Source::load().unwrap();
    let sampled = source.sample(source.reference_roots()).unwrap();
    assert_eq!(
        sampled.iter().map(|p| p.vertices.len() / 3).sum::<usize>(),
        189_542
    );
    assert_eq!(source.reference_roots().len(), 175);
    let mut missing = source.reference_roots().clone();
    missing.pop_first();
    assert!(source.sample(&missing).is_err());
    let mut invalid = source.reference_roots().clone();
    invalid.values_mut().next().unwrap().position_m[0] = f64::NAN;
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
