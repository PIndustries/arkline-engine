//! Exact, source-bound moving-mesh partition. Removing a registered dynamic range
//! from the fixed hull is paired with retaining its byte-identical moving mesh.
use super::*;
use arkline_protocol::drone_bay_geometry::BayPose;
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Deserialize)]
struct RootPose {
    translation: [f64; 3],
    rotation_xyzw: [f64; 4],
}
#[derive(Deserialize)]
struct SourceRange {
    source_id: String,
    render_primitive: usize,
    first_index: usize,
    index_count: usize,
}
#[derive(Deserialize)]
struct Group {
    id: String,
    reference_pose_body_m: RootPose,
    source_parts: Vec<SourceRange>,
}
#[derive(Deserialize)]
struct Binding {
    schema: String,
    registration_revision: u32,
    glb_sha256: String,
    source_triangle_count: usize,
    dynamic_triangle_count: usize,
    static_triangle_count: usize,
    dynamic_groups: Vec<Group>,
}

pub(super) struct MovingSourceMesh {
    pub group_id: String,
    pub primitive: usize,
    pub reference_pose: BayPose,
    pub vertices: Vec<Vertex>,
}
pub(super) struct Partition {
    pub fixed: Vec<Vec<Vertex>>,
    pub moving: Vec<MovingSourceMesh>,
}

/// The caller validates the compiled receipt hash and full saved-master mapping
/// first. These checks additionally prevent overlap, arbitrary deletion, bad
/// triangle ranges and mismatched source registration during runtime assembly.
pub(super) fn partition(
    source: &[Vec<Vertex>],
    document: &str,
    expected_hash: &str,
    expected_revision: u32,
) -> Result<Partition, String> {
    let binding: Binding =
        serde_json::from_str(document).map_err(|e| format!("Invalid bay source binding: {e}"))?;
    if binding.schema != "arkline.pioneer-drone-bay-runtime-binding/v1"
        || binding.glb_sha256 != expected_hash
        || binding.registration_revision != expected_revision
    {
        return Err("Bay moving geometry does not match the installed source".into());
    }
    if source.iter().any(|v| v.len() % 3 != 0) {
        return Err("Incomplete source triangle".into());
    }
    let total: usize = source.iter().map(|v| v.len() / 3).sum();
    if binding.source_triangle_count != total
        || binding
            .dynamic_triangle_count
            .checked_add(binding.static_triangle_count)
            != Some(total)
    {
        return Err("Bay source triangle accounting mismatch".into());
    }
    let mut used: Vec<Vec<bool>> = source.iter().map(|v| vec![false; v.len()]).collect();
    let mut group_ids = BTreeSet::new();
    let mut part_ids = BTreeSet::new();
    let mut moving = Vec::new();
    for group in binding.dynamic_groups {
        let reference = BayPose {
            position_m: group.reference_pose_body_m.translation,
            orientation_xyzw: group.reference_pose_body_m.rotation_xyzw,
        };
        if group.id.is_empty()
            || !group_ids.insert(group.id.clone())
            || !reference.valid()
            || group.source_parts.is_empty()
        {
            return Err("Invalid or duplicate bay mechanism identity/reference".into());
        }
        let mut batches: Vec<Vec<Vertex>> = source.iter().map(|_| Vec::new()).collect();
        for part in group.source_parts {
            if part.source_id.is_empty()
                || !part_ids.insert(part.source_id)
                || part.index_count == 0
                || part.first_index % 3 != 0
                || part.index_count % 3 != 0
            {
                return Err("Invalid or duplicate moving source part".into());
            }
            let end = part
                .first_index
                .checked_add(part.index_count)
                .ok_or("Moving source range overflow")?;
            let vertices = source
                .get(part.render_primitive)
                .and_then(|v| v.get(part.first_index..end))
                .ok_or("Moving source range outside primitive")?;
            let flags = &mut used[part.render_primitive][part.first_index..end];
            if flags.iter().any(|v| *v) {
                return Err("Moving source ranges overlap".into());
            }
            flags.fill(true);
            batches[part.render_primitive].extend_from_slice(vertices);
        }
        for (primitive, vertices) in batches
            .into_iter()
            .enumerate()
            .filter(|(_, v)| !v.is_empty())
        {
            moving.push(MovingSourceMesh {
                group_id: group.id.clone(),
                primitive,
                reference_pose: reference,
                vertices,
            });
        }
    }
    let fixed: Vec<Vec<Vertex>> = source
        .iter()
        .zip(used)
        .map(|(vertices, flags)| {
            vertices
                .iter()
                .zip(flags)
                .filter_map(|(v, moving)| (!moving).then_some(*v))
                .collect()
        })
        .collect();
    let moving_count: usize = moving.iter().map(|m| m.vertices.len() / 3).sum();
    let fixed_count: usize = fixed.iter().map(|m| m.len() / 3).sum();
    if moving_count != binding.dynamic_triangle_count
        || fixed_count != binding.static_triangle_count
        || moving_count + fixed_count != total
    {
        return Err("Bay partition lost or duplicated source geometry".into());
    }
    Ok(Partition { fixed, moving })
}

pub(super) fn installed() -> Result<Partition, String> {
    pioneer_source_snapshot::validate_installed()?;
    let vertices = pioneer_asset()?
        .iter()
        .map(|p| p.vertices.clone())
        .collect::<Vec<_>>();
    partition(
        &vertices,
        include_str!("../../../assets/ships/pioneer/drone-bay-runtime-binding.json"),
        arkline_protocol::PIONEER_DOCKED_SOURCE_SHA256,
        arkline_protocol::PIONEER_DOCKED_REGISTRATION_REVISION,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding() -> serde_json::Value {
        serde_json::json!({"schema":"arkline.pioneer-drone-bay-runtime-binding/v1","registration_revision":12,"glb_sha256":"exact","source_triangle_count":3,"dynamic_triangle_count":1,"static_triangle_count":2,"dynamic_groups":[{"id":"door/upper","reference_pose_body_m":{"translation":[1,2,3],"rotation_xyzw":[0,0,0,1]},"source_parts":[{"source_id":"part:door","render_primitive":0,"first_index":3,"index_count":3}]}]})
    }
    fn source() -> Vec<Vec<Vertex>> {
        vec![
            (0..6)
                .map(|i| Vertex {
                    p: Vec3::new(i as f32, 0., 0.),
                    n: Vec3::Y,
                })
                .collect(),
            vec![
                Vertex {
                    p: Vec3::Z,
                    n: Vec3::X
                };
                3
            ],
        ]
    }
    #[test]
    fn partition_preserves_exact_vertices_and_material_identity() {
        let input = source();
        let output = partition(&input, &binding().to_string(), "exact", 12).unwrap();
        assert_eq!(output.fixed.len(), 2);
        assert_eq!(output.fixed[0].len(), 3);
        assert_eq!(output.fixed[1].len(), 3);
        assert_eq!(output.moving.len(), 1);
        let moving = &output.moving[0];
        assert_eq!(moving.group_id, "door/upper");
        assert_eq!(moving.primitive, 0);
        assert_eq!(moving.reference_pose.position_m, [1., 2., 3.]);
        for (a, b) in output.fixed[0]
            .iter()
            .chain(&moving.vertices)
            .zip(&input[0])
        {
            assert_eq!(a.p, b.p);
            assert_eq!(a.n, b.n);
        }
    }
    #[test]
    fn source_partition_rejects_wrong_identity_overlap_and_accounting() {
        let input = source();
        assert!(partition(&input, &binding().to_string(), "wrong", 12).is_err());
        assert!(partition(&input, &binding().to_string(), "exact", 11).is_err());
        let mut duplicate = binding();
        let group = duplicate["dynamic_groups"][0].clone();
        duplicate["dynamic_groups"]
            .as_array_mut()
            .unwrap()
            .push(group);
        assert!(partition(&input, &duplicate.to_string(), "exact", 12).is_err());
        let mut overlap = binding();
        let mut part = overlap["dynamic_groups"][0]["source_parts"][0].clone();
        part["source_id"] = "part:other".into();
        overlap["dynamic_groups"][0]["source_parts"]
            .as_array_mut()
            .unwrap()
            .push(part);
        assert!(partition(&input, &overlap.to_string(), "exact", 12).is_err());
        for (key, value) in [
            ("first_index", 1),
            ("index_count", 30),
            ("render_primitive", 99),
        ] {
            let mut invalid = binding();
            invalid["dynamic_groups"][0]["source_parts"][0][key] = value.into();
            assert!(partition(&input, &invalid.to_string(), "exact", 12).is_err());
        }
        let mut missing = binding();
        missing["dynamic_triangle_count"] = 0.into();
        assert!(partition(&input, &missing.to_string(), "exact", 12).is_err());
    }
    #[test]
    fn installed_bay_partition_conserves_all_source_triangles() {
        let source = pioneer_asset().unwrap();
        let partition = installed().unwrap();
        assert_eq!(partition.moving.len(), 43);
        for (primitive, input) in source.iter().enumerate() {
            let bits = |v: &Vertex| {
                [
                    v.p.x.to_bits(),
                    v.p.y.to_bits(),
                    v.p.z.to_bits(),
                    v.n.x.to_bits(),
                    v.n.y.to_bits(),
                    v.n.z.to_bits(),
                ]
            };
            let mut expected = input.vertices.iter().map(bits).collect::<Vec<_>>();
            let mut actual = partition.fixed[primitive]
                .iter()
                .map(bits)
                .chain(
                    partition
                        .moving
                        .iter()
                        .filter(|m| m.primitive == primitive)
                        .flat_map(|m| m.vertices.iter().map(bits)),
                )
                .collect::<Vec<_>>();
            expected.sort_unstable();
            actual.sort_unstable();
            assert_eq!(
                actual, expected,
                "primitive{primitive} must conserve every exact source vertex"
            );
        }
    }
}
