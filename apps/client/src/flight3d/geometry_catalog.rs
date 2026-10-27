//! Immutable shared CPU geometry catalog and source/material resolution.
//! Renderer uploads, depth and pointer rays retain this single catalog and slot order.

use super::*;

#[derive(Clone)]
pub(super) struct BayMeshMount {
    pub(super) mesh: usize,
    pub(super) group_id: String,
    pub(super) reference_pose: arkline_protocol::drone_bay_geometry::BayPose,
}

#[derive(Clone)]
pub(super) struct ReceiverGeometry {
    pub(super) parts: Arc<Vec<ModelPart>>,
    pub(super) source_meshes: Vec<(usize, usize)>,
    pub(super) moving: Vec<BayMeshMount>,
}

#[derive(Clone)]
pub(super) struct CarrierGeometry {
    pub(super) registration: arkline_protocol::CarrierInventoryRegistration,
    pub(super) parts: Arc<Vec<ModelPart>>,
    pub(super) moving: Vec<BayMeshMount>,
    pub(super) references:
        std::collections::BTreeMap<String, arkline_protocol::drone_bay_geometry::BayPose>,
    pub(super) materials: Vec<(usize, [f32; 4])>,
    pub(super) glass_meshes: Vec<usize>,
    pub(super) warning_beacons: Vec<(usize, usize)>,
    pub(super) exterior_roles: Vec<(usize, usize)>,
}

#[derive(Clone)]
pub(super) struct FlightGeometry {
    pub(super) catalog: Arc<DepthMeshCatalog>,
    pub(super) station_batches: Vec<(usize, Vec3, f32)>,
    pub(super) drone_parts: [Arc<Vec<ModelPart>>; 3],
    pub(super) drone_materials: Vec<(usize, [f32; 4])>,
    pub(super) source_meshes: Vec<(usize, usize)>,
    pub(super) bay_meshes: Vec<BayMeshMount>,
    pub(super) docked_parts: Result<Arc<Vec<ModelPart>>, String>,
    pub(super) receiver: Result<ReceiverGeometry, String>,
    pub(super) carriers: Vec<Result<CarrierGeometry, String>>,
    pub(super) physical_drone: Result<physical_source_scene::DroneGeometry, String>,
}

pub(super) fn flight_geometry() -> Result<Arc<FlightGeometry>, String> {
    static GEOMETRY: std::sync::OnceLock<Result<Arc<FlightGeometry>, String>> =
        std::sync::OnceLock::new();
    GEOMETRY
        .get_or_init(|| {
            let mut upload = vec![
                cube(),
                torus(1.0, 0.12),
                sphere(false),
                rock_mesh(),
                hull(),
                sphere_with_resolution(false, 72, 48),
                torus(1.0, 0.008),
                surface_patch(),
                armor_block(),
                industrial_shell(),
            ];
            upload.extend(pioneer_asset()?.iter().map(|p| p.vertices.clone()));
            upload.push(hangar::station_floor_vertices());
            let mut station_batches = Vec::new();
            for (vertices, color, emission) in station_batches_geometry() {
                station_batches.push((upload.len(), color, emission));
                upload.push(vertices);
            }
            // Append after all existing uploads so ship, floor and station mesh
            // indices stay stable. Flight, inspection, depth and pointer rays
            // retain these exact same baked vertices and material tuples.
            let mut drone_materials = Vec::new();
            let drone_parts = drone_models::CLASSES.map(|class| {
                Arc::new(
                    drone_models::model(class)
                        .batches
                        .iter()
                        .map(|batch| {
                            let mesh = upload.len();
                            upload.push(batch.vertices.clone());
                            drone_materials
                                .push((mesh, [batch.roughness, batch.metallic, 0.0, 0.0]));
                            (mesh, Mat4::IDENTITY, batch.color, batch.emission)
                        })
                        .collect(),
                )
            });
            // Every original triangle belongs to exactly one fixed or moving
            // source mesh. Material provenance remains attached to each upload.
            let mut source_meshes = Vec::new();
            let mut bay_meshes = Vec::new();
            let docked_parts = match pioneer_bay_source::installed() {
                Ok(partition) => {
                    let source = pioneer_asset()?;
                    let mut parts = Vec::new();
                    for (primitive, vertices) in partition.fixed.into_iter().enumerate() {
                        if vertices.is_empty() {
                            continue;
                        }
                        let mesh = upload.len();
                        upload.push(vertices);
                        source_meshes.push((mesh, primitive));
                        let material = &source[primitive];
                        parts.push((mesh, Mat4::IDENTITY, material.color, material.emission));
                    }
                    for moving in partition.moving {
                        let mesh = upload.len();
                        upload.push(moving.vertices);
                        source_meshes.push((mesh, moving.primitive));
                        let material = &source[moving.primitive];
                        parts.push((mesh, Mat4::IDENTITY, material.color, material.emission));
                        bay_meshes.push(BayMeshMount {
                            mesh,
                            group_id: moving.group_id,
                            reference_pose: moving.reference_pose,
                        });
                    }
                    Ok(Arc::new(parts))
                }
                Err(error) => Err(error),
            };
            // Additive module uploads follow every existing mesh. Its own
            // registration failure must not disable legacy hull rendering.
            let receiver = ore_receiver_source::installed().map(|source| {
                let mut parts = Vec::new();
                let mut source_meshes = Vec::new();
                let mut moving = Vec::new();
                for part in &source.meshes {
                    let mesh = upload.len();
                    upload.push(part.vertices.clone());
                    let material = &source.primitives[part.primitive];
                    parts.push((mesh, Mat4::IDENTITY, material.color, material.emission));
                    source_meshes.push((mesh, part.primitive));
                    if let Some(group_id) = &part.group {
                        moving.push(BayMeshMount {
                            mesh,
                            group_id: group_id.clone(),
                            reference_pose: part.reference_pose,
                        });
                    }
                }
                ReceiverGeometry {
                    parts: Arc::new(parts),
                    source_meshes,
                    moving,
                }
            });
            // Strictly additive: every legacy GPU/depth/ray slot above is preserved.
            // Each exact carrier variant fails independently of legacy resources.
            let carriers = [1701, 1702, 1703]
                .into_iter()
                .map(|revision| append_carrier(&mut upload, revision))
                .collect();
            let physical_drone = Err("Physical source not requested".into());
            Ok(Arc::new(FlightGeometry {
                physical_drone,
                catalog: Arc::new(DepthMeshCatalog::new(
                    upload.into_iter().map(Arc::from).collect(),
                )),
                station_batches,
                drone_parts,
                drone_materials,
                source_meshes,
                bay_meshes,
                docked_parts,
                receiver,
                carriers,
            }))
        })
        .clone()
}

fn append_carrier(upload: &mut Vec<Vec<Vertex>>, revision: u32) -> Result<CarrierGeometry, String> {
    let registration = carrier_magazine_source::Source::registration(revision)?;
    let source = carrier_magazine_source::Source::load_for_registration(&registration)?;
    let references = source.reference_roots().clone();
    let mut parts = Vec::new();
    let mut moving = Vec::new();
    let mut materials = Vec::new();
    let mut glass_meshes = Vec::new();
    let mut exterior_roles = Vec::new();
    let mut append = |vertices: Vec<Vertex>, primitive: usize| {
        let mesh = upload.len();
        upload.push(vertices);
        let material = &source.primitives[primitive];
        if material.material_index == Some(6) && material.material_name == "Navigation glass" {
            glass_meshes.push(mesh);
        }
        if revision == 1703 {
            let role = match (material.material_index, material.material_name.as_str()) {
                (Some(4), "Drive emission") => Some(4),
                (Some(5), "Working lights") => Some(5),
                (Some(6), "Navigation glass") => Some(6),
                _ => None,
            };
            if let Some(role) = role {
                exterior_roles.push((mesh, role));
            }
        }
        materials.push((mesh, [material.roughness, material.metallic, 0.0, 0.0]));
        parts.push((
            mesh,
            Mat4::from_scale(Vec3::splat(100.0)),
            material.color,
            material.emission,
        ));
        mesh
    };
    for (primitive, vertices) in source.partition.fixed.into_iter().enumerate() {
        if !vertices.is_empty() {
            append(vertices, primitive);
        }
    }
    let mut warning_beacons = Vec::new();
    for group in source.partition.moving {
        let mesh = append(group.vertices, group.primitive);
        if registration.assembly_revision == 1703
            && registration.assembly_source_sha256
                == "79ab43221861f74383ca36ca6a742ff5588e08003eb7b10f9d8b6f0d5469664a"
            && group.primitive == 7
            && source.primitives[group.primitive].material_name == "Safety amber"
        {
            if let Some(cell) = group
                .group_id
                .strip_prefix("cell")
                .and_then(|s| s.strip_suffix("/loader/trolley"))
                .and_then(|s| s.parse::<usize>().ok())
            {
                if cell >= 10 {
                    return Err("Unexpected warning beacon cell".into());
                }
                warning_beacons.push((mesh, cell));
            }
        }
        moving.push(BayMeshMount {
            mesh,
            group_id: group.group_id,
            reference_pose: group.reference_pose,
        });
    }
    if registration.assembly_revision == 1703 {
        let cells: std::collections::BTreeSet<_> =
            warning_beacons.iter().map(|(_, c)| *c).collect();
        if warning_beacons.len() != 10 || cells != (0..10).collect() {
            return Err("Exact source warning beacon partition mismatch".into());
        }
    }
    Ok(CarrierGeometry {
        warning_beacons,
        exterior_roles,
        registration,
        parts: Arc::new(parts),
        moving,
        references,
        materials,
        glass_meshes,
    })
}

/// Explicit source extension. Legacy construction neither decodes nor uploads1704.
pub(super) fn physical_geometry() -> Result<Arc<FlightGeometry>, String> {
    static PHYSICAL: std::sync::OnceLock<Result<Arc<FlightGeometry>, String>> =
        std::sync::OnceLock::new();
    PHYSICAL
        .get_or_init(|| {
            let base = flight_geometry()?;
            let count = base.catalog.uploads().len();
            // Empty prefix reserves exact mesh IDs without copying any base vertices.
            let mut tail = vec![Vec::new(); count];
            let carrier = append_carrier(&mut tail, 1704)?;
            let drone = physical_source_scene::append_drone(&mut tail)?;
            let mut uploads = base.catalog.uploads().to_vec();
            uploads.extend(tail.into_iter().skip(count).map(Arc::from));
            let mut geometry = (*base).clone();
            geometry.catalog = Arc::new(DepthMeshCatalog::new(uploads));
            geometry.carriers.push(Ok(carrier));
            geometry.physical_drone = Ok(drone);
            Ok(Arc::new(geometry))
        })
        .clone()
}

/// Only an extended mesh ID requests the opt-in source catalog.
fn geometry_for_mesh(mesh: usize) -> Result<Arc<FlightGeometry>, String> {
    let base = flight_geometry()?;
    if mesh_index(mesh) < base.catalog.uploads().len() {
        Ok(base)
    } else {
        physical_geometry()
    }
}

#[cfg(any(not(target_arch = "wasm32"), test))]
pub(super) fn cached_drone_parts(
    class: arkline_protocol::DroneClass,
) -> Result<Arc<Vec<ModelPart>>, String> {
    Ok(flight_geometry()?.drone_parts[drone_models::class_index(class)].clone())
}

// Mesh slots 0..10 remain stable for procedural world geometry. Embedded
// primitive slots precede station batching, and are shared by every render pass.
pub(super) fn pioneer_asset() -> Result<&'static Vec<ship_asset::Primitive>, String> {
    static ASSET: std::sync::OnceLock<Result<Vec<ship_asset::Primitive>, String>> =
        std::sync::OnceLock::new();
    ASSET
        .get_or_init(|| {
            let parts =
                ship_asset::decode(include_bytes!("../../../../assets/ships/pioneer/pioneer.glb"))?;
            let mut lo = Vec3::splat(f32::INFINITY);
            let mut hi = Vec3::splat(f32::NEG_INFINITY);
            for p in &parts {
                for v in &p.vertices {
                    lo = lo.min(v.p);
                    hi = hi.max(v.p);
                }
            }
            let expected = arkline_protocol::PIONEER_BODY_BOUNDS_M.map(|v| Vec3::from_array(v.map(|n| n as f32)) / 100.);
            if (lo - expected[0]).abs().max_element() > 0.0003 || (hi - expected[1]).abs().max_element() > 0.0003
            {
                return Err(format!(
                    "Pioneer asset must match the reviewed raised-bridge body bounds; got {lo:?}..{hi:?}"
                ));
            }
            Ok(parts)
        })
        .as_ref()
        .map_err(Clone::clone)
}

// Ship-only role tags preserve shared primitive geometry without tinting station parts.
pub(super) const PAINT_ROLE_MASK: usize = 0x0f00_0000;

pub(super) const HULL_ROLE: usize = 1 << 24;

pub(super) const TRIM_ROLE: usize = 2 << 24;

pub(super) const EMISSION_ROLE: usize = 3 << 24;

pub(super) fn mesh_index(mesh: usize) -> usize {
    mesh & 0x00ff_ffff
}

pub(super) fn paint_role(mesh: usize) -> usize {
    match mesh & PAINT_ROLE_MASK {
        HULL_ROLE => HULL_ROLE,
        TRIM_ROLE => TRIM_ROLE,
        _ => match embedded_primitive(mesh).map(|p| p.material_name.as_str()) {
            Some("Graphite ceramic armor") => HULL_ROLE,
            Some("Emerald structural frames") => TRIM_ROLE,
            _ => 0,
        },
    }
}

pub(super) fn preview_paint(mesh: usize) -> bool {
    paint_role(mesh) != 0
}

pub(super) fn preview_material(
    mesh: usize,
    color: Vec3,
    emission: f32,
    preview: Option<crate::EmbeddedMaterialPreview>,
) -> (Vec3, f32) {
    let Some(preview) = preview else {
        return (color, emission);
    };
    let color = match paint_role(mesh) {
        HULL_ROLE => Vec3::from_array(preview.hull_rgb),
        TRIM_ROLE => Vec3::from_array(preview.trim_rgb),
        _ => color,
    };
    let emission = if embedded_primitive(mesh).is_some() || mesh & PAINT_ROLE_MASK == EMISSION_ROLE
    {
        emission * preview.emissive_strength
    } else {
        emission
    };
    (color, emission)
}

pub(super) fn docked_source_primitive(mesh: usize) -> Option<usize> {
    let mesh = mesh_index(mesh);
    if (10..19).contains(&mesh) {
        return Some(mesh - 10);
    }
    if mesh < 19 {
        return None;
    }
    let geometry = flight_geometry().ok()?;
    geometry
        .source_meshes
        .iter()
        .find_map(|&(slot, primitive)| (slot == mesh).then_some(primitive))
}

pub(super) fn embedded_primitive(mesh: usize) -> Option<&'static ship_asset::Primitive> {
    pioneer_asset().ok()?.get(docked_source_primitive(mesh)?)
}

pub(super) fn docked_pioneer_parts() -> Result<Arc<Vec<ModelPart>>, String> {
    flight_geometry()?.docked_parts.clone()
}

/// Body-metre geometry from accepted roots, shared by rendering/depth/rays.
/// Callers must validate feature ownership and sample authority before calling;
/// this resolver cannot invent a coupling phase or change ore inventory.
pub(super) fn receiver_body_parts(
    roots: &std::collections::BTreeMap<String, arkline_protocol::drone_bay_geometry::BayPose>,
) -> Result<Vec<ModelPart>, String> {
    use arkline_protocol::drone_dropoff_geometry as receiver;
    if roots.len() != receiver::DROPOFF_MECHANISM_COUNT
        || receiver::mechanism_ids()
            .iter()
            .any(|id| roots.get(id.semantic_id()).is_none_or(|p| !p.valid()))
    {
        return Err("Unconfirmed receiver mechanism roots".into());
    }
    let geometry = flight_geometry()?;
    let receiver = geometry.receiver.as_ref().map_err(Clone::clone)?;
    let matrix = |p: arkline_protocol::drone_bay_geometry::BayPose| {
        Mat4::from_rotation_translation(
            Quat::from_array(p.orientation_xyzw.map(|v| v as f32)).normalize(),
            Vec3::from_array(p.position_m.map(|v| v as f32)),
        )
    };
    Ok(receiver
        .parts
        .iter()
        .map(|&(mesh, model, color, emission)| {
            let model = receiver
                .moving
                .iter()
                .find(|m| m.mesh == mesh)
                .map_or(model, |mount| {
                    matrix(roots[&mount.group_id]) * matrix(mount.reference_pose).inverse() * model
                });
            (mesh, model, color, emission)
        })
        .collect())
}

pub(super) fn is_registered_glass(mesh: usize) -> bool {
    docked_source_primitive(mesh) == Some(6)
        || geometry_for_mesh(mesh).ok().is_some_and(|g| {
            g.carriers
                .iter()
                .filter_map(|v| v.as_ref().ok())
                .any(|c| c.glass_meshes.contains(&mesh_index(mesh)))
        })
}

/// Exact installed carrier roots in body metres. This is the common immutable
/// catalog resolver, not a separate cosmetic preview or an admission decision.
pub(super) fn carrier_body_parts(
    pilot: &PilotView,
    snapshot_time: u64,
) -> Result<Vec<ModelPart>, String> {
    let public = pilot
        .carrier_inventory
        .as_ref()
        .ok_or("Carrier projection missing")?;
    if public.sample_time_ms != snapshot_time {
        return Err("Carrier projection clock differs from accepted snapshot".into());
    }
    public.validate_projection(pilot)?;
    let geometry = flight_geometry()?;
    let carrier = geometry
        .carriers
        .iter()
        .filter_map(|c| c.as_ref().ok())
        .find(|c| c.registration == public.binding().registration)
        .ok_or("Exact installed carrier catalog unavailable")?;
    let roots: std::collections::BTreeMap<_, _> = public.machinery.iter().cloned().collect();
    if roots.len() != public.machinery.len()
        || roots.len() != carrier.references.len()
        || carrier.references.keys().any(|id| {
            roots
                .get(id)
                .is_none_or(|p| !p.valid() || p.position_m.iter().any(|v| !(*v as f32).is_finite()))
        })
    {
        return Err("Incomplete confirmed carrier roots".into());
    }
    let matrix = |p: arkline_protocol::drone_bay_geometry::BayPose| {
        Mat4::from_rotation_translation(
            Quat::from_array(p.orientation_xyzw.map(|v| v as f32)).normalize(),
            Vec3::from_array(p.position_m.map(|v| v as f32)),
        )
    };
    let warning = crate::carrier_warning_adapter::visuals(pilot, public);
    carrier
        .parts
        .iter()
        .map(|&(mesh, model, color, emission)| {
            let model = carrier
                .moving
                .iter()
                .find(|m| m.mesh == mesh)
                .map_or(model, |m| {
                    matrix(roots[&m.group_id]) * matrix(m.reference_pose).inverse() * model
                });
            if !model.is_finite() {
                return Err("Nonfinite carrier root transform".into());
            }
            let (color, emission) = if let Some((_, cell)) = carrier
                .warning_beacons
                .iter()
                .find(|(slot, _)| *slot == mesh)
            {
                warning[*cell].map_or((color, 0.0), |visual| {
                    (Vec3::from_array(visual.color), visual.emission)
                })
            } else {
                (color, emission)
            };
            Ok((mesh, model, color, emission))
        })
        .collect()
}

/// Source PBR and authored drone material roles take precedence over the caller's
/// procedural fallback. Every canonical view uses the same exact mesh resolver.
pub(super) fn resolved_material_parameters(mesh: usize, fallback: Material) -> [f32; 4] {
    embedded_primitive(mesh).map_or_else(
        || {
            geometry_for_mesh(mesh)
                .ok()
                .and_then(|geometry| {
                    geometry
                        .drone_materials
                        .iter()
                        .find_map(|&(slot, parameters)| (slot == mesh).then_some(parameters))
                        .or_else(|| {
                            geometry
                                .carriers
                                .iter()
                                .filter_map(|c| c.as_ref().ok())
                                .flat_map(|c| &c.materials)
                                .find_map(|&(slot, parameters)| {
                                    (slot == mesh).then_some(parameters)
                                })
                        })
                        .or_else(|| {
                            geometry
                                .physical_drone
                                .as_ref()
                                .ok()?
                                .materials
                                .iter()
                                .find_map(|&(slot, p)| (slot == mesh).then_some(p))
                        })
                        .or_else(|| {
                            let receiver = geometry.receiver.as_ref().ok()?;
                            let primitive =
                                receiver
                                    .source_meshes
                                    .iter()
                                    .find_map(|&(slot, primitive)| {
                                        (slot == mesh).then_some(primitive)
                                    })?;
                            let source = ore_receiver_source::installed().ok()?;
                            let material = &source.primitives[primitive];
                            Some([material.roughness, material.metallic, 0.0, 0.0])
                        })
                })
                .unwrap_or_else(|| {
                    surface_roles::material(mesh)
                        .unwrap_or(fallback)
                        .parameters()
                })
        },
        |primitive| [primitive.roughness, primitive.metallic, 0.0, 0.0],
    )
}

pub(super) fn pioneer_asset_sha256() -> &'static str {
    use sha2::{Digest, Sha256};
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| {
        hex::encode(Sha256::digest(include_bytes!(
            "../../../../assets/ships/pioneer/pioneer.glb"
        )))
    })
}

pub(super) fn mesh_vertices(mesh: usize) -> Vec<Vertex> {
    if mesh_index(mesh) > hangar::station_floor_mesh_index() {
        if let Ok(geometry) = geometry_for_mesh(mesh) {
            if let Some(vertices) = geometry.catalog.uploads().get(mesh_index(mesh)) {
                return vertices.to_vec();
            }
        }
    }
    if mesh_index(mesh) == hangar::station_floor_mesh_index() {
        return hangar::station_floor_vertices();
    }
    if let Some(primitive) = embedded_primitive(mesh) {
        return primitive.vertices.clone();
    }
    match mesh_index(mesh) {
        0 => cube(),
        1 => torus(1.0, 0.12),
        2 => sphere(false),
        8 => armor_block(),
        9 => industrial_shell(),
        _ => hull(),
    }
}

pub(super) fn station_batches_geometry() -> Vec<(Vec<Vertex>, Vec3, f32)> {
    let parts = station_parts(Space {
        origin: STATION_CENTER_POSITION,
    });
    let mut groups = std::collections::BTreeMap::<[u32; 4], (Vec<Vertex>, Vec3, f32)>::new();
    for (mesh, model, color, emission) in parts {
        let normal = model.inverse().transpose();
        let entry = groups
            .entry([
                color.x.to_bits(),
                color.y.to_bits(),
                color.z.to_bits(),
                emission.to_bits(),
            ])
            .or_insert((Vec::new(), color, emission));
        entry
            .0
            .extend(mesh_vertices(mesh).into_iter().map(|v| Vertex {
                p: model.transform_point3(v.p),
                n: normal.transform_vector3(v.n).normalize(),
            }));
    }
    groups.into_values().collect()
}

/// Exact installed source material provenance; never legacy primitive indexing.
pub(super) fn carrier_exterior_material_role(mesh: usize) -> Option<usize> {
    let geometry = flight_geometry().ok()?;
    geometry
        .carriers
        .iter()
        .filter_map(|v| v.as_ref().ok())
        .filter(|c| c.registration.assembly_revision == 1703)
        .flat_map(|c| c.exterior_roles.iter())
        .find(|(slot, _)| *slot == mesh_index(mesh))
        .map(|(_, role)| *role)
}

#[cfg(test)]
mod physical_lazy_tests {
    use super::*;

    #[test]
    fn legacy_catalog_excludes_physical_tail_and_extension_preserves_all_slots() {
        let base = flight_geometry().unwrap();
        assert_eq!(base.carriers.len(), 3);
        assert!(base.physical_drone.is_err());
        assert!(base
            .carriers
            .iter()
            .filter_map(|c| c.as_ref().ok())
            .all(|c| c.registration.assembly_revision < 1704));
        let physical = physical_geometry().unwrap();
        let count = base.catalog.uploads().len();
        for (old, extended) in base
            .catalog
            .uploads()
            .iter()
            .zip(physical.catalog.uploads())
        {
            assert!(
                Arc::ptr_eq(old, extended),
                "Base vertices copied or reordered"
            );
        }
        assert_eq!(
            physical.catalog.uploads()[count..]
                .iter()
                .map(|v| v.len() / 3)
                .sum::<usize>(),
            201_696
        );
        assert_eq!(physical.carriers.len(), 4);
        assert!(physical.physical_drone.is_ok());
        assert_eq!(base.catalog.uploads().len(), count);
        assert!(Arc::ptr_eq(&base, &flight_geometry().unwrap()));
        assert!(Arc::ptr_eq(&physical, &physical_geometry().unwrap()));
    }

    #[test]
    fn lazy_tail_matches_eager_source_parts_and_materials_exactly() {
        let base = flight_geometry().unwrap();
        let count = base.catalog.uploads().len();
        let mut eager = vec![Vec::new(); count];
        let carrier = append_carrier(&mut eager, 1704).unwrap();
        let drone = physical_source_scene::append_drone(&mut eager).unwrap();
        let physical = physical_geometry().unwrap();
        let actual = physical.carriers.last().unwrap().as_ref().unwrap();
        assert_eq!(*carrier.parts, *actual.parts);
        assert_eq!(carrier.materials, actual.materials);
        assert_eq!(drone.parts, physical.physical_drone.as_ref().unwrap().parts);
        assert_eq!(
            drone.materials,
            physical.physical_drone.as_ref().unwrap().materials
        );
        for (expected, actual) in eager[count..]
            .iter()
            .zip(&physical.catalog.uploads()[count..])
        {
            assert_eq!(expected.len(), actual.len());
            for (a, b) in expected.iter().zip(actual.iter()) {
                assert_eq!(a.p, b.p);
                assert_eq!(a.n, b.n);
            }
        }
    }
}
