//! Derived interior openings shared by docked and flight Pioneer presentation.
//!
//! Canonical asset/catalog buffers remain untouched. Coordinates below are body
//! metres; returned vertices retain the renderer's 100 metres per scene unit.
use super::*;
use glam::DVec3;
use sha2::{Digest, Sha256};

const ASSET_SHA: &str = "8125644f007537437942d34169e527c08ddd2dfaed575c874e2263ff2b7cb123";
const RECEIPT_SHA: &str = "fadece504f99169f645559f5c903636b6555f8d8b3f3c3b856cc267b639d5429";
const RECEIPT: &[u8] = include_bytes!("../../../assets/ships/pioneer/pioneer-export.json");
const PRIMITIVES: [usize; 3] = [0, 1, 6];
const COUNTS: [usize; 9] = [44814, 156348, 27900, 52764, 1416, 3888, 288, 21168, 1812];
const MAX_ADDED_VERTICES: usize = 4096;
const MAX_POLYGON_VERTICES: usize = 16;
const MAX_FRAGMENTS: usize = 64;

struct SourcePart {
    id: &'static str,
    name: &'static str,
    primitive: usize,
    first: usize,
    count: usize,
    material: &'static str,
    material_id: &'static str,
    affected: std::ops::Range<usize>,
}

const SOURCES: [SourcePart; 3] = [
    SourcePart {
        id: "part:d9e8088471d3686fd783795df45dcc235b99ab7426033cf4d2a2cfcc5ade3309",
        name: "Pressure hull",
        primitive: 0,
        first: 18864,
        count: 13194,
        material: "Graphite ceramic armor",
        material_id: "material:5084bf82f73a59cdae623179d8113152515f5e5e6347c4df4a11ed8eaa478b57",
        affected: 31368..31443,
    },
    SourcePart {
        id: "part:68b45ea99185087e982152189d9fb7514c5ba1e3c156a25af5dc55011957b036",
        name: "Bridge wrap frame port 0",
        primitive: 1,
        first: 5796,
        count: 324,
        material: "Titanium machinery",
        material_id: "material:2e4b79a53f5d6a450c73bf2b573933f76a43373c0bdcc489c83a15237cfb346a",
        affected: 5796..6120,
    },
    SourcePart {
        id: "part:b200a9824e59e3c68bcb4a9726401968c5dc72286637d03904f40a58e81f60e3",
        name: "Bridge wrap glazing port",
        primitive: 6,
        first: 216,
        count: 36,
        material: "Navigation glass",
        material_id: "material:c0649776d3deed29903fbc269e3414985da7b0fda68653bd4f5adad51ed46a2f",
        affected: 216..252,
    },
];

// Explicit measured runtime refit: replace the source diagonal across the
// pilot's primary sightline with the shared perimeter post in pioneer_docked.
// Catalog/inspection uploads and the saved GLB remain byte-for-byte original.
const PILOT_BRACE: SourcePart = SourcePart {
    id: "part:8797073fe7367d18c774c7dcaa3f62e4588e0aebc9e78c3781d1e4b5b8cd8505",
    name: "Study05 cockpit diagonal brace starboard",
    primitive: 1,
    first: 112608,
    count: 324,
    material: "Titanium machinery",
    material_id: "material:2e4b79a53f5d6a450c73bf2b573933f76a43373c0bdcc489c83a15237cfb346a",
    affected: 112608..112932,
};

// Registration-5 cockpit prototype: this single measured source mullion is
// replaced by protocol-authored outboard uprights/transoms. Never remove by
// broad spatial clipping; the exact export identity and index span are guarded.
const CENTER_MULLION: SourcePart = SourcePart {
    id: "part:2edf5970536f9fa18916a20dd4ab3c43c403bf71ab9c3ed6121b7512aa994f2b",
    name: "Bridge center mullion",
    primitive: 1,
    first: 2880,
    count: 324,
    material: "Titanium machinery",
    material_id: "material:2e4b79a53f5d6a450c73bf2b573933f76a43373c0bdcc489c83a15237cfb346a",
    affected: 2880..3204,
};

/// Order is canonical primitives 0 (hull), 1 (machinery), 6 (glass).
/// A source mismatch disables this derived presentation rather than guessing.
pub(super) fn source_meshes() -> Result<[Vec<Vertex>; 3], String> {
    static MESHES: std::sync::OnceLock<Result<[Vec<Vertex>; 3], String>> =
        std::sync::OnceLock::new();
    MESHES.get_or_init(build).clone()
}

fn build() -> Result<[Vec<Vertex>; 3], String> {
    validate_receipt(RECEIPT, pioneer_asset_sha256())?;
    let asset = pioneer_asset()?;
    if asset.len() != COUNTS.len()
        || asset.iter().enumerate().any(|(index, part)| {
            part.vertices.len() != COUNTS[index] || part.material_index != Some(index)
        })
    {
        return Err("Docked Pioneer primitive identity/count mismatch".into());
    }
    let mut result: [Vec<Vertex>; 3] = std::array::from_fn(|_| Vec::new());
    for (slot, source) in SOURCES.iter().enumerate() {
        let part = &asset[source.primitive];
        if part.material_name != source.material
            || PRIMITIVES[slot] != source.primitive
            || source.affected.start % 3 != 0
            || source.affected.end % 3 != 0
            || source.affected.start < source.first
            || source.affected.end > source.first + source.count
        {
            return Err("Docked Pioneer material/source range mismatch".into());
        }
        result[slot] = derive(&part.vertices, source.affected.clone(), cuts(slot)?)?;
        if slot == 1 {
            let added = result[slot].len() - part.vertices.len();
            result[slot]
                .drain(PILOT_BRACE.first + added..PILOT_BRACE.first + added + PILOT_BRACE.count);
            // Descending original offsets: mullion precedes the existing portal
            // clipping range, so no generated-fragment offset applies here.
            result[slot].drain(CENTER_MULLION.first..CENTER_MULLION.first + CENTER_MULLION.count);
        }
    }
    Ok(result)
}

fn validate_receipt(bytes: &[u8], asset_sha: &str) -> Result<(), String> {
    if asset_sha != ASSET_SHA || hex::encode(Sha256::digest(bytes)) != RECEIPT_SHA {
        return Err("Docked Pioneer asset/receipt SHA mismatch".into());
    }
    let receipt: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("Docked Pioneer receipt: {error}"))?;
    validate_identity(&receipt)
}

fn validate_identity(receipt: &serde_json::Value) -> Result<(), String> {
    if receipt["schema"] != "arkline.pioneer-saved-master-export/v1"
        || receipt["source"]["sha256"]
            != "61eb70e1978668f5ca7256f237d9df14c47140b9bbbbce327875b68efcbe57c9"
        || receipt["output"]["sha256"] != ASSET_SHA
        || receipt["output"]["primitives"] != 9
        || receipt["output"]["materials"] != 9
        || receipt["output"]["triangles"] != 103466
        || receipt["coordinates"]["units"] != "body metres"
        || receipt["coordinates"]["blender_to_body"] != serde_json::json!(["x", "z", "-y"])
    {
        return Err("Docked Pioneer receipt schema/geometry mismatch".into());
    }
    let parts = receipt["source_parts"]
        .as_array()
        .filter(|parts| parts.len() == 686)
        .ok_or("Docked Pioneer source catalog mismatch")?;
    let materials = receipt["material_identity"]
        .as_array()
        .filter(|materials| materials.len() == 9)
        .ok_or("Docked Pioneer material catalog mismatch")?;
    for source in SOURCES.iter().chain([&PILOT_BRACE, &CENTER_MULLION]) {
        let matches: Vec<_> = parts
            .iter()
            .filter(|part| part["source_id"] == source.id)
            .collect();
        let Some(part) = matches.first().filter(|_| matches.len() == 1) else {
            return Err(format!("Docked Pioneer source identity: {}", source.name));
        };
        let material = &materials[source.primitive];
        if part["name"] != source.name
            || part["render_primitive"] != source.primitive
            || part["render_material_index"] != source.primitive
            || part["first_index"] != source.first
            || part["index_count"] != source.count
            || part["material"] != source.material
            || part["retained_triangles"] != source.count / 3
            || material["render_material_index"] != source.primitive
            || material["source_id"] != source.material_id
            || material["name"] != source.material
        {
            return Err(format!(
                "Docked Pioneer source/material binding: {}",
                source.name
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct CutBox {
    origin: DVec3,
    axes: [DVec3; 3],
    min: DVec3,
    max: DVec3,
}

impl CutBox {
    fn local(self, scene_point: DVec3) -> DVec3 {
        let body = scene_point * 100.0 - self.origin;
        DVec3::new(
            body.dot(self.axes[0]),
            body.dot(self.axes[1]),
            body.dot(self.axes[2]),
        )
    }

    fn distance(self, point: DVec3, plane: usize) -> f64 {
        let axis = plane / 2;
        let coordinate = self.local(point)[axis];
        if plane % 2 == 0 {
            coordinate - self.min[axis]
        } else {
            self.max[axis] - coordinate
        }
    }
}

fn cuts(slot: usize) -> Result<[CutBox; 2], String> {
    let primitive = *PRIMITIVES
        .get(slot)
        .ok_or("Unknown Pioneer source cut slot")?;
    let contract = arkline_protocol::pioneer_docked_source_cuts()
        .into_iter()
        .find(|cut| cut.primitive == primitive)
        .ok_or("Missing shared Pioneer source cut")?;
    let bounds: [arkline_protocol::PioneerDockedBox; 2] = contract
        .volumes
        .try_into()
        .map_err(|_| "Expected two bounded Pioneer source cuts")?;
    Ok(bounds.map(|b| CutBox {
        origin: DVec3::from_array(b.center_m),
        axes: b.axes.map(DVec3::from_array),
        min: -DVec3::from_array(b.half_m),
        max: DVec3::from_array(b.half_m),
    }))
}

#[derive(Clone, Copy)]
struct Corner {
    p: DVec3,
    // Keep the linear interpolant unnormalised through successive plane clips.
    // Normalising at each cut would change smooth source normal interpolation.
    n: DVec3,
    original: Option<Vertex>,
}

impl Corner {
    fn from_vertex(vertex: Vertex) -> Self {
        Self {
            p: vertex.p.as_dvec3(),
            n: vertex.n.as_dvec3(),
            original: Some(vertex),
        }
    }

    fn intersection(self, other: Self, distance: f64, other_distance: f64) -> Result<Self, String> {
        let fraction = distance / (distance - other_distance);
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
            return Err("Docked Pioneer invalid clip intersection".into());
        }
        Ok(Self {
            p: self.p.lerp(other.p, fraction),
            n: self.n.lerp(other.n, fraction),
            original: None,
        })
    }

    fn vertex(self) -> Result<Vertex, String> {
        let vertex = if let Some(original) = self.original {
            original
        } else {
            if !self.n.is_finite() || self.n.length_squared() <= 1e-20 {
                return Err("Docked Pioneer invalid interpolated normal".into());
            }
            Vertex {
                p: self.p.as_vec3(),
                n: self.n.normalize().as_vec3(),
            }
        };
        if !vertex.p.is_finite()
            || !vertex.n.is_finite()
            || (vertex.n.length_squared() - 1.0).abs() > 1e-4
        {
            return Err("Docked Pioneer nonfinite/nonunit vertex".into());
        }
        Ok(vertex)
    }
}

fn push_corner(polygon: &mut Vec<Corner>, corner: Corner) {
    if polygon
        .last()
        .is_none_or(|last| last.p.distance_squared(corner.p) > 1e-28)
    {
        polygon.push(corner);
    }
}

fn finish_polygon(polygon: &mut Vec<Corner>) {
    if polygon.len() > 1 && polygon[0].p.distance_squared(polygon.last().unwrap().p) <= 1e-28 {
        polygon.pop();
    }
}

fn split(
    polygon: &[Corner],
    cut: CutBox,
    plane: usize,
) -> Result<(Vec<Corner>, Vec<Corner>), String> {
    let mut inside = Vec::new();
    let mut outside = Vec::new();
    for index in 0..polygon.len() {
        let a = polygon[index];
        let b = polygon[(index + 1) % polygon.len()];
        let da = cut.distance(a.p, plane);
        let db = cut.distance(b.p, plane);
        if !da.is_finite() || !db.is_finite() {
            return Err("Docked Pioneer nonfinite clip plane".into());
        }
        if da >= 0.0 {
            push_corner(&mut inside, a);
        }
        if da <= 0.0 {
            push_corner(&mut outside, a);
        }
        if (da < 0.0 && db > 0.0) || (da > 0.0 && db < 0.0) {
            let intersection = a.intersection(b, da, db)?;
            push_corner(&mut inside, intersection);
            push_corner(&mut outside, intersection);
        }
    }
    finish_polygon(&mut inside);
    finish_polygon(&mut outside);
    if inside.len() > MAX_POLYGON_VERTICES || outside.len() > MAX_POLYGON_VERTICES {
        return Err("Docked Pioneer clip polygon budget exceeded".into());
    }
    Ok((inside, outside))
}

fn subtract(polygon: Vec<Corner>, cut: CutBox) -> Result<Vec<Vec<Corner>>, String> {
    // A separating box plane also preserves untouched triangles bit-for-bit.
    if (0..6).any(|plane| {
        polygon
            .iter()
            .all(|corner| cut.distance(corner.p, plane) <= 0.0)
    }) {
        return Ok(vec![polygon]);
    }
    let mut remaining = polygon;
    let mut outside = Vec::new();
    for plane in 0..6 {
        let (inside, fragment) = split(&remaining, cut, plane)?;
        if fragment.len() >= 3 {
            outside.push(fragment);
        }
        remaining = inside;
        if remaining.len() < 3 {
            break;
        }
    }
    Ok(outside)
}

fn derive(
    vertices: &[Vertex],
    affected: std::ops::Range<usize>,
    cuts: [CutBox; 2],
) -> Result<Vec<Vertex>, String> {
    if vertices.len() % 3 != 0
        || affected.end > vertices.len()
        || affected.start > affected.end
        || affected.start % 3 != 0
        || affected.end % 3 != 0
    {
        return Err("Docked Pioneer invalid triangle range".into());
    }
    let mut output = Vec::with_capacity(vertices.len() + 512);
    output.extend_from_slice(&vertices[..affected.start]);
    for source in vertices[affected.clone()].chunks_exact(3) {
        let source_normal = (source[1].p - source[0].p)
            .as_dvec3()
            .cross((source[2].p - source[0].p).as_dvec3());
        if !source_normal.is_finite() || source_normal.length_squared() <= 1e-20 {
            return Err("Docked Pioneer degenerate source triangle".into());
        }
        let mut fragments = vec![source.iter().copied().map(Corner::from_vertex).collect()];
        for cut in cuts {
            let mut next = Vec::new();
            for polygon in fragments {
                next.extend(subtract(polygon, cut)?);
            }
            if next.len() > MAX_FRAGMENTS {
                return Err("Docked Pioneer clip fragment budget exceeded".into());
            }
            fragments = next;
        }
        for polygon in fragments {
            for index in 1..polygon.len() - 1 {
                let exact_normal =
                    (polygon[index].p - polygon[0].p).cross(polygon[index + 1].p - polygon[0].p);
                // Coplanar boundary duplicates have no surface to triangulate.
                if exact_normal.length_squared() <= 1e-30 {
                    continue;
                }
                let triangle = [
                    polygon[0].vertex()?,
                    polygon[index].vertex()?,
                    polygon[index + 1].vertex()?,
                ];
                let normal = (triangle[1].p - triangle[0].p)
                    .as_dvec3()
                    .cross((triangle[2].p - triangle[0].p).as_dvec3());
                // Do not silently lose a positive-area sliver that collapses in
                // the renderer's f32 representation: disable the derived mesh.
                if !normal.is_finite()
                    || normal.length_squared() <= 1e-20
                    || normal.dot(source_normal) <= 0.0
                {
                    return Err("Docked Pioneer degenerate/reversed derived triangle".into());
                }
                output.extend(triangle);
                if output.len() > vertices.len() + MAX_ADDED_VERTICES {
                    return Err("Docked Pioneer derived vertex budget exceeded".into());
                }
            }
        }
    }
    output.extend_from_slice(&vertices[affected.end..]);
    if output.len() > vertices.len() + MAX_ADDED_VERTICES {
        return Err("Docked Pioneer derived vertex budget exceeded".into());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(vertex: &Vertex) -> [u32; 6] {
        let p = vertex.p.to_array();
        let n = vertex.n.to_array();
        [
            p[0].to_bits(),
            p[1].to_bits(),
            p[2].to_bits(),
            n[0].to_bits(),
            n[1].to_bits(),
            n[2].to_bits(),
        ]
    }

    fn identical(a: &[Vertex], b: &[Vertex]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| bits(a) == bits(b))
    }

    // Full triangle/OBB separating-axis test, independent of the clip planes.
    fn intersects(triangle: &[Vertex], bounds: CutBox) -> bool {
        let center = (bounds.min + bounds.max) * 0.5;
        let half = (bounds.max - bounds.min) * 0.5;
        let points: [DVec3; 3] =
            std::array::from_fn(|i| bounds.local(triangle[i].p.as_dvec3()) - center);
        let edges = [
            points[1] - points[0],
            points[2] - points[1],
            points[0] - points[2],
        ];
        let mut axes = vec![DVec3::X, DVec3::Y, DVec3::Z, edges[0].cross(edges[1])];
        for edge in edges {
            axes.extend([
                edge.cross(DVec3::X),
                edge.cross(DVec3::Y),
                edge.cross(DVec3::Z),
            ]);
        }
        axes.into_iter().all(|axis| {
            if axis.length_squared() < 1e-24 {
                return true;
            }
            let projections = points.map(|point| point.dot(axis));
            let radius = half.dot(axis.abs());
            projections.iter().copied().fold(f64::INFINITY, f64::min) <= radius
                && projections
                    .iter()
                    .copied()
                    .fold(f64::NEG_INFINITY, f64::max)
                    >= -radius
        })
    }

    #[test]
    fn exact_source_guards_reject_changed_assets_receipts_and_bindings() {
        validate_receipt(RECEIPT, ASSET_SHA).unwrap();
        assert!(validate_receipt(RECEIPT, "changed").is_err());
        let mut receipt: serde_json::Value = serde_json::from_slice(RECEIPT).unwrap();
        receipt["schema"] = "different".into();
        assert!(validate_identity(&receipt).is_err());
        let mut receipt: serde_json::Value = serde_json::from_slice(RECEIPT).unwrap();
        let parts = receipt["source_parts"].as_array_mut().unwrap();
        let part = parts
            .iter_mut()
            .find(|part| part["source_id"] == SOURCES[0].id)
            .unwrap();
        part["first_index"] = 18867.into();
        assert!(validate_identity(&receipt).is_err());
        assert!(validate_receipt(&serde_json::to_vec(&receipt).unwrap(), ASSET_SHA).is_err());
        let mut receipt: serde_json::Value = serde_json::from_slice(RECEIPT).unwrap();
        receipt["material_identity"][6]["source_id"] = "different".into();
        assert!(validate_identity(&receipt).is_err());
    }

    #[test]
    fn canonical_buffers_and_every_triangle_outside_identified_ranges_are_unchanged() {
        let asset = pioneer_asset().unwrap();
        let before: Vec<Vec<_>> = asset
            .iter()
            .map(|part| part.vertices.iter().map(bits).collect())
            .collect();
        let derived = source_meshes().unwrap();
        for (slot, source) in SOURCES.iter().enumerate() {
            let original = &asset[source.primitive].vertices;
            let prefix = if slot == 1 {
                [
                    &original[..CENTER_MULLION.first],
                    &original[CENTER_MULLION.first + CENTER_MULLION.count..source.affected.start],
                ]
                .concat()
            } else {
                original[..source.affected.start].to_vec()
            };
            assert!(identical(&prefix, &derived[slot][..prefix.len()]));
            let suffix = if slot == 1 {
                [
                    &original[source.affected.end..PILOT_BRACE.first],
                    &original[PILOT_BRACE.first + PILOT_BRACE.count..],
                ]
                .concat()
            } else {
                original[source.affected.end..].to_vec()
            };
            assert!(identical(
                &suffix,
                &derived[slot][derived[slot].len() - suffix.len()..]
            ));
            assert_ne!(original.len(), derived[slot].len());
            // The two broad hull caps retain surrounding hull surface.
            if slot == 0 {
                assert!(derived[slot].len() > original.len());
            }
        }
        for (primitive, original) in asset.iter().zip(before) {
            assert_eq!(
                primitive.vertices.iter().map(bits).collect::<Vec<_>>(),
                original
            );
        }
    }

    #[test]
    fn prototype_center_mullion_removes_only_its_measured_export_span() {
        let asset = pioneer_asset().unwrap();
        let original = &asset[1].vertices;
        let span = &original[CENTER_MULLION.affected.clone()];
        let min = span
            .iter()
            .fold(Vec3::splat(f32::INFINITY), |a, v| a.min(v.p * 100.));
        let max = span
            .iter()
            .fold(Vec3::splat(f32::NEG_INFINITY), |a, v| a.max(v.p * 100.));
        assert!(
            min.abs_diff_eq(Vec3::new(35.6, -0.64, -0.11), 0.0001),
            "{min:?}"
        );
        assert!(
            max.abs_diff_eq(Vec3::new(35.84, 2.71, 0.11), 0.0001),
            "{max:?}"
        );
        let source = &SOURCES[1];
        let mut previous = derive(original, source.affected.clone(), cuts(1).unwrap()).unwrap();
        let added = previous.len() - original.len();
        previous.drain(PILOT_BRACE.first + added..PILOT_BRACE.first + added + PILOT_BRACE.count);
        let before = previous.len();
        previous.drain(CENTER_MULLION.affected.clone());
        let actual = source_meshes().unwrap();
        assert_eq!(before - actual[1].len(), CENTER_MULLION.count);
        assert!(
            identical(&previous, &actual[1]),
            "No other runtime source triangle may change"
        );
        for property in ["first_index", "index_count", "render_primitive"] {
            let mut receipt: serde_json::Value = serde_json::from_slice(RECEIPT).unwrap();
            let part = receipt["source_parts"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|part| part["source_id"] == CENTER_MULLION.id)
                .unwrap();
            part[property] = 0.into();
            assert!(validate_identity(&receipt).is_err());
        }
    }

    #[test]
    fn derived_portals_and_floor_recesses_are_clear_through_all_materials() {
        let asset = pioneer_asset().unwrap();
        let derived = source_meshes().unwrap();
        let mut volumes = Vec::new();
        for slot in [0, 1] {
            let [mut door, mut recess] = cuts(slot).unwrap();
            // One-millimetre inset tests the promised clear volume, excluding
            // its deliberately retained boundary faces and f32 rounding.
            door.min += DVec3::splat(0.001);
            door.max -= DVec3::splat(0.001);
            recess.min += DVec3::splat(0.001);
            recess.max -= DVec3::splat(0.001);
            if slot == 1 {
                // The continuous outside threshold ends at lift near edge n=1.6.
                door.max.z = 1.6;
                recess.max.z = 1.6;
            }
            volumes.extend([door, recess]);
        }
        for (primitive_index, primitive) in asset.iter().enumerate() {
            let vertices = PRIMITIVES
                .iter()
                .position(|&index| index == primitive_index)
                .map_or(primitive.vertices.as_slice(), |slot| {
                    derived[slot].as_slice()
                });
            for (volume_index, volume) in volumes.iter().enumerate() {
                let hits: Vec<_> = vertices
                    .chunks_exact(3)
                    .enumerate()
                    .filter(|(_, triangle)| intersects(triangle, *volume))
                    .map(|(index, _)| index)
                    .collect();
                assert!(
                    hits.is_empty(),
                    "primitive {primitive_index}, volume {volume_index}: {hits:?}"
                );
            }
        }
    }

    #[test]
    fn deterministic_finite_unit_normals_valid_winding_and_bounded_geometry() {
        let first = build().unwrap();
        let second = build().unwrap();
        assert!(first[0].len() > COUNTS[0]);
        assert_eq!([first[1].len(), first[2].len()], [155862, 366]);
        for (slot, (a, b)) in first.iter().zip(&second).enumerate() {
            assert!(identical(a, b));
            assert_eq!(a.len() % 3, 0);
            assert!(a.len() <= COUNTS[PRIMITIVES[slot]] + MAX_ADDED_VERTICES);
            for triangle in a.chunks_exact(3) {
                let cross = (triangle[1].p - triangle[0].p).cross(triangle[2].p - triangle[0].p);
                assert!(cross.is_finite() && cross.length_squared() > 1e-20);
                for vertex in triangle {
                    assert!(vertex.p.is_finite() && vertex.n.is_finite());
                    assert!((vertex.n.length_squared() - 1.0).abs() < 1e-4);
                }
            }
        }
    }

    #[test]
    fn new_edges_interpolate_source_corner_normals_without_flattening() {
        let a = Corner::from_vertex(Vertex {
            p: Vec3::ZERO,
            n: Vec3::X,
        });
        let b = Corner::from_vertex(Vertex {
            p: Vec3::X,
            n: Vec3::Y,
        });
        let midpoint = a.intersection(b, -1.0, 1.0).unwrap();
        let quarter = a
            .intersection(midpoint, -1.0, 1.0)
            .unwrap()
            .vertex()
            .unwrap();
        let expected = Vec3::new(0.75, 0.25, 0.0).normalize();
        assert!((quarter.n - expected).length() < 1e-6);
        assert_eq!(quarter.p, Vec3::new(0.25, 0.0, 0.0));
        assert!(Corner {
            n: DVec3::ZERO,
            original: None,
            ..a
        }
        .vertex()
        .is_err());
        assert!(a.intersection(b, f64::NAN, 1.0).is_err());
        assert!(derive(
            &[Vertex {
                p: Vec3::ZERO,
                n: Vec3::Y
            }; 3],
            0..3,
            cuts(0).unwrap()
        )
        .is_err());
    }
}
