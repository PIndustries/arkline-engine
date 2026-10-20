//! Snapshot-bound inspection of the authored Pioneer panes, without proxy geometry.
use super::*;
use arkline_protocol::ExteriorFixture;
use serde_json::Value;

const ASSET_HASH: &str = "8125644f007537437942d34169e527c08ddd2dfaed575c874e2263ff2b7cb123";
const GLASS_PRIMITIVE: usize = 6;
const GLASS_VERTICES: usize = 288;
const EXPORT: &str = include_str!("../../../assets/ships/pioneer/pioneer-export.json");
const MOUNTS: &str = include_str!("../../../assets/ships/pioneer/exterior-mounts.json");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WindowRange {
    fixture_id: &'static str,
    source_part: &'static str,
    first: usize,
    count: usize,
}
const WINDOWS: [WindowRange; 4] = [
    WindowRange {
        fixture_id: "pioneer.exterior.window.port.forward",
        source_part: "Bridge front glazing port",
        first: 72,
        count: 36,
    },
    WindowRange {
        fixture_id: "pioneer.exterior.window.starboard.forward",
        source_part: "Bridge front glazing starboard",
        first: 108,
        count: 36,
    },
    WindowRange {
        fixture_id: "pioneer.exterior.window.port.aft",
        source_part: "Bridge wrap glazing port",
        first: 216,
        count: 36,
    },
    WindowRange {
        fixture_id: "pioneer.exterior.window.starboard.aft",
        source_part: "Bridge wrap glazing starboard",
        first: 252,
        count: 36,
    },
];

fn documents(export: &str, mounts: &str) -> Result<(Value, Value), String> {
    if export.len() > 1024 * 1024 || mounts.len() > 64 * 1024 {
        return Err("Window source mapping exceeds its document budget".into());
    }
    let export = serde_json::from_str(export)
        .map_err(|error| format!("Invalid window export mapping: {error}"))?;
    let mounts = serde_json::from_str(mounts)
        .map_err(|error| format!("Invalid window mount mapping: {error}"))?;
    Ok((export, mounts))
}

fn unique<'a>(values: &'a [Value], field: &str, identity: &str) -> Result<&'a Value, String> {
    let mut matching = values
        .iter()
        .filter(|value| value[field].as_str() == Some(identity));
    let value = matching
        .next()
        .ok_or_else(|| format!("Window source mapping is missing {identity}"))?;
    if matching.next().is_some() {
        return Err(format!("Window source mapping repeats {identity}"));
    }
    Ok(value)
}

fn validate_mapping(
    asset_hash: &str,
    export: &Value,
    mounts: &Value,
    primitive: &ship_asset::Primitive,
) -> Result<[WindowRange; 4], String> {
    if asset_hash != ASSET_HASH
        || export["output"]["sha256"].as_str() != Some(ASSET_HASH)
        || mounts["target_export_sha256"].as_str() != Some(ASSET_HASH)
    {
        return Err("Window source mapping does not match the installed Pioneer GLB hash".into());
    }
    if export["schema"].as_str() != Some("arkline.pioneer-saved-master-export/v1")
        || mounts["schema"].as_str() != Some("pioneer-fixture-mount-recommendations/v1")
        || export["output"]["file"].as_str() != Some("pioneer.glb")
        || export["output"]["bytes"].as_u64() != Some(8_700_948)
        || export["output"]["primitives"].as_u64() != Some(9)
        || export["output"]["materials"].as_u64() != Some(9)
    {
        return Err("Window source mapping has an unsupported snapshot schema or layout".into());
    }
    if primitive.material_index != Some(GLASS_PRIMITIVE)
        || primitive.material_name != "Navigation glass"
        || primitive.vertices.len() != GLASS_VERTICES
    {
        return Err(
            "Window source mapping does not match the decoded Navigation glass batch".into(),
        );
    }
    let source_parts = export["source_parts"]
        .as_array()
        .ok_or("Window export mapping has no source-part list")?;
    let fixture_mounts = mounts["fixtures"]
        .as_array()
        .ok_or("Window mount mapping has no fixture list")?;
    let definition = arkline_protocol::default_pioneer_exterior();
    for expected in WINDOWS {
        let mount = unique(fixture_mounts, "fixture_id", expected.fixture_id)?;
        if mount["kind"].as_str() != Some("window")
            || mount["source_part"].as_str() != Some(expected.source_part)
        {
            return Err(format!(
                "Window fixture-to-source binding changed for {}",
                expected.fixture_id
            ));
        }
        let fixture = definition
            .fixtures
            .iter()
            .find(|fixture| fixture.id == expected.fixture_id)
            .ok_or("Window fixture is absent from the installed exterior definition")?;
        let position = mount["applied_position_body_m"]
            .as_array()
            .ok_or("Window mount has no body position")?;
        if fixture.kind != ExteriorFixtureKind::Window
            || position.len() != 3
            || position
                .iter()
                .zip(fixture.position_m)
                .any(|(actual, expected)| {
                    actual.as_f64().is_none_or(|actual| {
                        !actual.is_finite() || (actual - expected).abs() > 1e-8
                    })
                })
        {
            return Err(format!(
                "Window source mount differs from the installed fixture {}",
                expected.fixture_id
            ));
        }
        let part = unique(source_parts, "name", expected.source_part)?;
        if part["material"].as_str() != Some("Navigation glass")
            || part["render_material_index"].as_u64() != Some(GLASS_PRIMITIVE as u64)
            || part["render_primitive"].as_u64() != Some(GLASS_PRIMITIVE as u64)
            || part["first_index"].as_u64() != Some(expected.first as u64)
            || part["index_count"].as_u64() != Some(expected.count as u64)
            || part["retained_triangles"].as_u64() != Some(12)
            || part["dropped_triangles"].as_u64() != Some(0)
        {
            return Err(format!(
                "Window authored triangle range changed for {}",
                expected.fixture_id
            ));
        }
        let (first, count) = triangle_draw_range(
            i32::try_from(primitive.vertices.len())
                .map_err(|_| "Window vertex count exceeds the GPU range")?,
            Some((expected.first, expected.count)),
        )
        .ok_or("Window authored triangle range is invalid or exceeds the glass batch")?;
        if primitive.vertices[first as usize..(first + count) as usize]
            .iter()
            .any(|vertex| {
                !vertex.p.is_finite() || !vertex.n.is_finite() || vertex.n.length_squared() < 0.1
            })
        {
            return Err("Window authored triangle range contains invalid vertices".into());
        }
    }
    Ok(WINDOWS)
}

fn ranges() -> Result<&'static [WindowRange; 4], String> {
    static RANGES: OnceLock<Result<[WindowRange; 4], String>> = OnceLock::new();
    RANGES
        .get_or_init(|| {
            let (export, mounts) = documents(EXPORT, MOUNTS)?;
            let primitive = pioneer_asset()?
                .get(GLASS_PRIMITIVE)
                .ok_or("Installed Pioneer asset has no Navigation glass batch")?;
            validate_mapping(pioneer_asset_sha256(), &export, &mounts, primitive)
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub(super) fn draw(fixture: &ExteriorFixture) -> Result<InspectionDraw, String> {
    let mapping = ranges()?
        .iter()
        .find(|mapping| mapping.fixture_id == fixture.id)
        .ok_or("Window fixture has no hash-bound authored triangle mapping")?;
    if fixture.kind != ExteriorFixtureKind::Window {
        return Err("Authored window mapping cannot preview another fixture kind".into());
    }
    let primitive = &pioneer_asset()?[GLASS_PRIMITIVE];
    Ok(InspectionDraw {
        part: (
            10 + GLASS_PRIMITIVE,
            Mat4::IDENTITY,
            primitive.color,
            primitive.emission,
        ),
        material: Material::BareMetal,
        range: Some((mapping.first, mapping.count)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> (Value, Value, &'static ship_asset::Primitive) {
        let (export, mounts) = documents(EXPORT, MOUNTS).unwrap();
        (export, mounts, &pioneer_asset().unwrap()[GLASS_PRIMITIVE])
    }

    #[test]
    fn authored_window_ranges_reuse_exact_glass_vertices_materials_and_measured_bounds() {
        let (export, mounts, primitive) = fixtures();
        assert_eq!(
            validate_mapping(pioneer_asset_sha256(), &export, &mounts, primitive).unwrap(),
            WINDOWS
        );
        let definition = arkline_protocol::default_pioneer_exterior();
        for expected in WINDOWS {
            let fixture = definition
                .fixtures
                .iter()
                .find(|fixture| fixture.id == expected.fixture_id)
                .unwrap();
            let draw = draw(fixture).unwrap();
            assert_eq!(draw.range, Some((expected.first, expected.count)));
            assert_eq!(draw.part.0, 16);
            assert_eq!(draw.part.1, Mat4::IDENTITY);
            assert_eq!(draw.part.2, primitive.color);
            assert_eq!(draw.part.3, primitive.emission);
            assert_eq!(draw.material, Material::BareMetal);
            let selected = draw.selected_vertices(&primitive.vertices).unwrap();
            let original = &primitive.vertices[expected.first..expected.first + expected.count];
            assert!(std::ptr::eq(selected.as_ptr(), original.as_ptr()));
            let min = selected
                .iter()
                .map(|vertex| vertex.p)
                .fold(Vec3::splat(f32::INFINITY), Vec3::min);
            let max = selected
                .iter()
                .map(|vertex| vertex.p)
                .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
            let model = measure(vec![draw], 100.0).unwrap();
            assert_eq!(model.info.triangles, 12);
            assert_eq!(model.info.batches, 1);
            assert_eq!(model.min, min);
            assert_eq!(model.max, max);
            assert_eq!(model.info.min_m, (min * 100.0).to_array());
            assert_eq!(model.info.max_m, (max * 100.0).to_array());
            assert!(model.info.max_m[0] - model.info.min_m[0] < 3.0);
        }
    }

    #[test]
    fn window_mapping_rejects_asset_and_receipt_hash_mismatches() {
        let (mut export, mut mounts, primitive) = fixtures();
        assert!(
            validate_mapping(&"0".repeat(64), &export, &mounts, primitive)
                .unwrap_err()
                .contains("hash")
        );
        export["output"]["sha256"] = Value::String("0".repeat(64));
        assert!(validate_mapping(ASSET_HASH, &export, &mounts, primitive).is_err());
        export["output"]["sha256"] = ASSET_HASH.into();
        mounts["target_export_sha256"] = Value::String("0".repeat(64));
        assert!(validate_mapping(ASSET_HASH, &export, &mounts, primitive).is_err());
    }

    #[test]
    fn window_mapping_rejects_malformed_missing_duplicate_and_rebound_records() {
        assert!(documents("{", MOUNTS).is_err());
        assert!(documents(EXPORT, "{").is_err());
        let (_, mounts, primitive) = fixtures();
        let (mut export, _) = documents(EXPORT, MOUNTS).unwrap();
        export["source_parts"] = Value::Null;
        assert!(validate_mapping(ASSET_HASH, &export, &mounts, primitive).is_err());
        let (mut export, mut mounts) = documents(EXPORT, MOUNTS).unwrap();
        let index = export["source_parts"]
            .as_array()
            .unwrap()
            .iter()
            .position(|part| part["name"] == WINDOWS[0].source_part)
            .unwrap();
        let duplicate = export["source_parts"][index].clone();
        export["source_parts"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert!(validate_mapping(ASSET_HASH, &export, &mounts, primitive).is_err());
        let (export, _) = documents(EXPORT, MOUNTS).unwrap();
        let index = mounts["fixtures"]
            .as_array()
            .unwrap()
            .iter()
            .position(|part| part["fixture_id"] == WINDOWS[0].fixture_id)
            .unwrap();
        mounts["fixtures"][index]["source_part"] = "Another pane".into();
        assert!(validate_mapping(ASSET_HASH, &export, &mounts, primitive).is_err());
        mounts["fixtures"][index]["source_part"] = WINDOWS[0].source_part.into();
        mounts["fixtures"][index]["applied_position_body_m"][0] = 0.0.into();
        assert!(validate_mapping(ASSET_HASH, &export, &mounts, primitive).is_err());
    }

    #[test]
    fn window_mapping_and_measurement_fail_closed_on_changed_or_invalid_ranges() {
        let (export, mounts, primitive) = fixtures();
        let index = export["source_parts"]
            .as_array()
            .unwrap()
            .iter()
            .position(|part| part["name"] == WINDOWS[0].source_part)
            .unwrap();
        for (field, value) in [
            ("first_index", 75),
            ("first_index", u64::MAX),
            ("index_count", 35),
            ("index_count", 600),
            ("render_primitive", 5),
            ("render_material_index", 5),
        ] {
            let mut changed = export.clone();
            changed["source_parts"][index][field] = value.into();
            assert!(validate_mapping(ASSET_HASH, &changed, &mounts, primitive).is_err());
        }
        let fixture = arkline_protocol::default_pioneer_exterior()
            .fixtures
            .into_iter()
            .find(|fixture| fixture.id == WINDOWS[0].fixture_id)
            .unwrap();
        let mut draw = draw(&fixture).unwrap();
        for range in [(1, 36), (72, 35), (288, 36), (0, 0), (usize::MAX, 3)] {
            draw.range = Some(range);
            assert!(draw.selected_vertices(&primitive.vertices).is_err());
            assert!(measure(vec![draw], 100.0).is_err());
        }
        assert_eq!(triangle_draw_range(288, None), Some((0, 288)));
        assert_eq!(triangle_draw_range(288, Some((72, 36))), Some((72, 36)));
        assert_eq!(triangle_draw_range(-1, None), None);
        assert_eq!(triangle_draw_range(287, None), None);
    }
}
