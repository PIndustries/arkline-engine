//! CPU calibration of the actual render instances; no GPU/native acceptance.
use super::*;
use arkline_protocol::{fleet_berth, fleet_hangar_layout, HangarBerth, HangarView as FleetView};
use sha2::{Digest, Sha256};

fn berth(class: ShipClass) -> HangarBerth {
    let (position, boarding_point) = fleet_berth(class);
    HangarBerth {
        id: format!("scale-fixture-{class:?}"),
        ship_class: class,
        position,
        yaw: 0.,
        boarding_point,
        active: class == ShipClass::Pioneer,
        can_board: true,
    }
}

fn close(actual: Vec3, expected: Vec3) {
    assert!(
        (actual - expected).abs().max_element() < 0.002,
        "{actual:?} != {expected:?}"
    );
}

fn points(parts: &[ModelPart]) -> Vec<Vec3> {
    parts
        .iter()
        .flat_map(|(mesh, model, _, _)| {
            mesh_vertices(*mesh)
                .into_iter()
                .map(|vertex| model.transform_point3(vertex.p))
        })
        .collect()
}

fn body(parts: &[ModelPart]) -> Vec<ModelPart> {
    // Embedded GLB slots only: exclude floor, fittings, avatars and dynamic turrets.
    parts
        .iter()
        .copied()
        .filter(|p| embedded_primitive(p.0).is_some())
        .collect()
}

#[test]
fn hangar_scale_actual_private_instance_has_meter_bounds_and_floor_clearance() {
    let asset = pioneer_asset().expect("fixture must use the embedded GLB, never fallback");
    let parts = body(&build_bay_parts(ShipClass::Pioneer));
    assert_eq!(parts.len(), asset.len());
    let (lo, hi) = parts_bounds(&parts);
    close(lo, Vec3::new(-36., 0., -19.));
    close(hi, Vec3::new(36., 18., 19.));
    close(hi - lo, Vec3::new(72., 18., 38.));
    let boarding = Vec3::from_array(
        hangar_layout(ShipClass::Pioneer)
            .boarding_point
            .map(|v| v as f32),
    );
    close(boarding, Vec3::new(0., 0., 22.));
    assert!((boarding.z - hi.z - 3.).abs() < 0.002);
}

#[test]
fn hangar_scale_actual_fleet_instances_keep_pioneer_size_in_mixed_hall() {
    for classes in [
        vec![ShipClass::Pioneer],
        vec![ShipClass::Pioneer, ShipClass::Monolith],
    ] {
        let fleet = FleetView {
            layout: fleet_hangar_layout(),
            berths: classes.into_iter().map(berth).collect(),
        };
        let parts = body(&fleet_hangar::parts(&fleet, Vec3::new(0., 1.65, 0.)));
        assert_eq!(parts.len(), pioneer_asset().unwrap().len());
        let (lo, hi) = parts_bounds(&parts);
        close(lo, Vec3::new(-36., 0., 6.));
        close(hi, Vec3::new(36., 18., 44.));
        close(hi - lo, Vec3::new(72., 18., 38.));
        close(
            Vec3::from_array(fleet.berths[0].boarding_point.map(|v| v as f32)),
            Vec3::ZERO,
        );
        assert!((lo.z - 6.).abs() < 0.002);
    }
}

#[test]
fn hangar_scale_yaw_preserves_intrinsic_dimensions_and_uses_negative_heading() {
    for yaw in [0., std::f64::consts::FRAC_PI_2, 0.37, -1.2] {
        let mut b = berth(ShipClass::Pioneer);
        b.yaw = yaw;
        let fleet = FleetView {
            layout: fleet_hangar_layout(),
            berths: vec![b.clone()],
        };
        let actual = body(&fleet_hangar::parts(&fleet, Vec3::ZERO));
        let center = Vec3::from_array(b.position.map(|v| v as f32));
        let inverse_heading = Quat::from_rotation_y(yaw as f32);
        let pts: Vec<_> = points(&actual)
            .into_iter()
            .map(|p| inverse_heading * (p - center))
            .collect();
        let lo = pts
            .iter()
            .copied()
            .fold(Vec3::splat(f32::INFINITY), Vec3::min);
        let hi = pts
            .iter()
            .copied()
            .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
        close(lo, Vec3::new(-36., -9., -19.));
        close(hi, Vec3::new(36., 9., 19.));
        if yaw == std::f64::consts::FRAC_PI_2 {
            let (lo, hi) = parts_bounds(&actual);
            close(hi - lo, Vec3::new(38., 18., 72.));
        }
    }
}

#[test]
fn hangar_scale_reference_avatar_and_perspective_landmarks_agree_in_meters() {
    let avatar: StationAvatar = serde_json::from_value(serde_json::json!({
        "position":[0,0,22], "heading":0, "moving":false, "updated_at_ms":0
    }))
    .unwrap();
    let (lo, hi) = parts_bounds(&avatar_parts(&avatar, Default::default(), None));
    assert!(lo.y.abs() < 0.01);
    assert!((hi.y - 1.8).abs() < 0.01);
    let vp = view_projection(Vec3::new(0., 9., 100.), Vec3::new(0., 9., 0.), 1280. / 720.);
    let project = |p| vp.project_point3(p);
    close(
        project(Vec3::new(0., 9., 0.)).truncate().extend(0.),
        Vec3::ZERO,
    );
    let one_meter_pixels =
        (project(Vec3::new(0., 10., 0.)).y - project(Vec3::new(0., 9., 0.)).y) * 360.;
    let analytical = 720. / (2. * 100. * (55f32.to_radians() / 2.).tan());
    assert!((one_meter_pixels - analytical).abs() < 0.001);
    assert!((one_meter_pixels - 6.9155).abs() < 0.002);
}

#[test]
#[ignore = "Writes a new CPU calibration evidence directory; set ARKLINE_SCALE_EVIDENCE_DIR"]
fn hangar_scale_export_cpu_evidence() {
    let dir = std::path::PathBuf::from(
        std::env::var_os("ARKLINE_SCALE_EVIDENCE_DIR").expect("set a fresh output directory"),
    );
    std::fs::create_dir(&dir).expect("output directory must not already exist");
    let private = body(&build_bay_parts(ShipClass::Pioneer));
    let fleet = FleetView {
        layout: fleet_hangar_layout(),
        berths: vec![berth(ShipClass::Pioneer), berth(ShipClass::Monolith)],
    };
    let fleet_body = body(&fleet_hangar::parts(&fleet, Vec3::new(0., 1.65, 0.)));
    let raw = ship_parts(ShipClass::Pioneer);
    let bounds = |parts: &[ModelPart]| {
        let (lo, hi) = parts_bounds(parts);
        serde_json::json!({"min_xyz":lo.to_array(), "max_xyz":hi.to_array(), "extent_xyz":(hi-lo).to_array()})
    };
    let source = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("read source commit");
    assert!(source.status.success());
    let report = serde_json::json!({
        "evidence":"CPU geometry/projection only; not a native screenshot or scale acceptance",
        "build":include_str!("../../../BUILD_VERSION").trim(),
        "source_commit":String::from_utf8(source.stdout).unwrap().trim(),
        "source_note":"source commit is informational; retain git diff and test receipt for an uncommitted build",
        "asset_sha256":hex::encode(Sha256::digest(include_bytes!("../../../assets/ships/pioneer/pioneer.glb"))),
        "decoded_renderer_units":bounds(&raw),
        "private_meters":bounds(&private), "mixed_fleet_pioneer_meters":bounds(&fleet_body),
        "body_only":true, "excluded":"articulated turrets, collision proxies and landing/scene fittings",
        "private_boarding_m":hangar_layout(ShipClass::Pioneer).boarding_point,
        "fleet_berths":fleet.berths,
        "private_instance_matrices":private.iter().map(|p|p.1.to_cols_array()).collect::<Vec<_>>(),
        "fleet_instance_matrices":fleet_body.iter().map(|p|p.1.to_cols_array()).collect::<Vec<_>>(),
        "orthographic_fixture":{"viewport_px":[1040,640],"pixels_per_meter":10,"axes":["XY side","ZY end","XZ top"],"grid_m":1},
        "perspective_landmark":{"eye_m":[0,9,100],"target_m":[0,9,0],"vertical_fov_deg":55,"near_m":0.08,"far_m":30000,"viewport_px":[1280,720],"one_meter_at_target_px":720./(200.*(55f32.to_radians()/2.).tan())}
    });
    std::fs::write(
        dir.join("scale.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    // Vertex projections, rather than a hand-authored box. Dense points preserve
    // openings/outline evidence without implying a shaded or occluded GPU result.
    for (name, axes, a, b) in [
        ("side", "XY", 0, 1),
        ("end", "ZY", 2, 1),
        ("top", "XZ", 0, 2),
    ] {
        let origin_y = if b == 2 { 350. } else { 450. };
        let mut svg = String::from("<svg xmlns='http://www.w3.org/2000/svg' width='1040' height='640' viewBox='0 0 1040 640'><rect width='1040' height='640' fill='#102126'/><defs><pattern id='m' width='10' height='10' patternUnits='userSpaceOnUse'><path d='M10 0H0V10' fill='none' stroke='#34474b' stroke-width='.5'/></pattern></defs><rect x='40' y='80' width='960' height='520' fill='url(#m)'/><g fill='#8ba9a4' opacity='.85'>");
        let projected: std::collections::BTreeSet<_> = points(&private)
            .into_iter()
            .map(|p| {
                (
                    ((520. + p[a] * 10.) * 100.).round() as i32,
                    ((origin_y - p[b] * 10.) * 100.).round() as i32,
                )
            })
            .collect();
        for (x, y) in projected {
            svg.push_str(&format!(
                "<circle cx='{:.2}' cy='{:.2}' r='.65'/>",
                x as f32 / 100.,
                y as f32 / 100.
            ));
        }
        let (lo, hi) = parts_bounds(&private);
        svg.push_str(&format!(
            "</g><rect x='{:.2}' y='{:.2}' width='{:.2}' height='{:.2}' fill='none' stroke='#e2ad64' stroke-dasharray='5 5'/><text x='520' y='{:.2}' text-anchor='middle' fill='#e2ad64' font-family='monospace' font-size='14'>Body bound {:.3} m x {:.3} m</text><g>",
            520. + lo[a] * 10., origin_y - hi[b] * 10.,
            (hi[a] - lo[a]) * 10., (hi[b] - lo[b]) * 10.,
            origin_y - hi[b] * 10. - 12., hi[a] - lo[a], hi[b] - lo[b],
        ));
        svg.push_str("</g><g fill='none' stroke='#76df8d'><rect x='80' y='432' width='6' height='18'/><rect x='100' y='440' width='10' height='10'/></g><g fill='white' font-family='monospace' font-size='14'>");
        svg.push_str(&format!("<text x='40' y='30'>Pioneer {name} ({axes}): CPU vertex projection, private bay, 10 px/m</text>"));
        svg.push_str("<text x='40' y='55'>1 m grid; green: 1.8 m reference and 1 m cube projection. No native acceptance.</text><text x='40' y='625'>Body only. Orthographic diagnostic; not a gameplay camera or GPU capture.</text></g></svg>");
        std::fs::write(dir.join(format!("{name}.svg")), svg).unwrap();
    }
    println!("CPU scale evidence: {}", dir.display());
}
