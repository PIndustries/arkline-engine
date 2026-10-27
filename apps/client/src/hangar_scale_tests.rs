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
    close(lo, Vec3::new(-36., 2.40, -19.));
    close(hi, Vec3::new(36., 22.75, 19.));
    close(hi - lo, Vec3::new(72., 20.35, 38.));
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
            layout_revision: arkline_protocol::FleetLayoutRevision::LinearV1,
            layout: fleet_hangar_layout(),
            berths: classes.into_iter().map(berth).collect(),
        };
        let parts = body(&fleet_hangar::parts(&fleet, Vec3::new(0., 1.65, 0.)));
        assert_eq!(parts.len(), pioneer_asset().unwrap().len());
        let (lo, hi) = parts_bounds(&parts);
        close(lo, Vec3::new(-36., 2.40, 6.));
        close(hi, Vec3::new(36., 22.75, 44.));
        close(hi - lo, Vec3::new(72., 20.35, 38.));
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
            layout_revision: arkline_protocol::FleetLayoutRevision::LinearV1,
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
        close(hi, Vec3::new(36., 11.35, 19.));
        if yaw == std::f64::consts::FRAC_PI_2 {
            let (lo, hi) = parts_bounds(&actual);
            close(hi - lo, Vec3::new(38., 20.35, 72.));
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
        layout_revision: arkline_protocol::FleetLayoutRevision::LinearV1,
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
    export_circular_layout(&dir);
    println!("CPU scale evidence: {}", dir.display());
}

fn export_circular_layout(directory: &std::path::Path) {
    use arkline_protocol::{
        fleet_berth_pose, fleet_layout_geometry, FleetLayoutRevision, ALL_SHIP_CLASSES,
    };
    let revision = FleetLayoutRevision::CircularV1;
    let geometry = fleet_layout_geometry(revision);
    let mut measurements = Vec::new();
    let scale = 430. / geometry.outer_radius_m;
    let pixel = |x: f32, z: f32| {
        (
            600. + (x as f64 - geometry.center[0]) * scale,
            500. + (z as f64 - geometry.center[2]) * scale,
        )
    };
    let mut svg = String::from("<svg xmlns='http://www.w3.org/2000/svg' width='1200' height='1040' viewBox='0 0 1200 1040'><rect width='1200' height='1040' fill='#102126'/><g fill='none' stroke='#667e87'>");
    svg.push_str("<circle cx='600' cy='500' r='430' stroke-width='3'/>");
    svg.push_str(&format!(
        "<circle cx='600' cy='500' r='{:.3}' fill='#1a3438'/>",
        geometry.concourse_radius_m * scale
    ));
    for class in ALL_SHIP_CLASSES {
        let pose = fleet_berth_pose(revision, class);
        let berth = HangarBerth {
            id: format!("CPU-catalog-{class:?}"),
            ship_class: class,
            position: pose.position,
            yaw: pose.yaw,
            boarding_point: pose.boarding_point,
            active: class == ShipClass::Pioneer,
            can_board: false,
        };
        let body = fleet_hangar::hull_parts(&berth).collect::<Vec<_>>();
        let mut complete = body.clone();
        for yaw in 0..16 {
            for pitch in [
                -std::f32::consts::FRAC_PI_2,
                -0.7,
                0.,
                0.7,
                std::f32::consts::FRAC_PI_2,
            ] {
                let rotation = Quat::from_rotation_y(yaw as f32 * std::f32::consts::TAU / 16.)
                    * Quat::from_rotation_z(pitch);
                for slot in 0..2 {
                    complete.extend(turret_parts(class, rotation, slot).into_iter().map(
                        |(mesh, model, color, emission)| {
                            (
                                mesh,
                                fleet_hangar::hull_transform(&berth) * model,
                                color,
                                emission,
                            )
                        },
                    ));
                }
            }
        }
        let (body_min, body_max) = parts_bounds(&body);
        let (full_min, full_max) = parts_bounds(&complete);
        let full_points = points(&complete);
        let radius = full_points
            .iter()
            .map(|p| (p.x - pose.position[0] as f32).hypot(p.z - pose.position[2] as f32))
            .fold(0., f32::max);
        measurements.push(serde_json::json!({
            "ship_class":class, "pose":berth, "platform_radius_m":pose.platform_radius_m,
            "body_world_min_m":body_min.to_array(), "body_world_max_m":body_max.to_array(),
            "body_intrinsic_dimensions_m":[class.spec().length_m,class.spec().height_m,class.spec().width_m],
            "complete_aim_sweep_min_m":full_min.to_array(), "complete_aim_sweep_max_m":full_max.to_array(),
            "complete_aim_sweep_radial_extent_m":radius,
            "platform_radial_clearance_m":pose.platform_radius_m - radius as f64,
            "roof_clearance_m":geometry.ceiling_y - full_max.y as f64,
            "aim_samples":{"yaw_count":16,"pitch_radians":[-std::f64::consts::FRAC_PI_2,-0.7,0.,0.7,std::f64::consts::FRAC_PI_2],"turret_slots":2}
        }));
        let (cx, cy) = pixel(pose.position[0] as f32, pose.position[2] as f32);
        let (bx, by) = pixel(pose.boarding_point[0] as f32, pose.boarding_point[2] as f32);
        svg.push_str(&format!("<path d='M600 500L{bx:.3} {by:.3}' stroke='#9da476' stroke-width='{:.3}'/><circle cx='{cx:.3}' cy='{cy:.3}' r='{:.3}' fill='#183035'/>", 2. * geometry.bridge_half_width_m * scale, pose.platform_radius_m * scale));
        let outline = convex_xz(points(&body));
        let polygon = outline
            .iter()
            .map(|p| {
                let (x, y) = pixel(p[0], p[1]);
                format!("{x:.3},{y:.3}")
            })
            .collect::<Vec<_>>()
            .join(" ");
        svg.push_str(&format!("<polygon points='{polygon}' fill='#507b7b' stroke='#b9d9d3' stroke-width='1'/><circle cx='{bx:.3}' cy='{by:.3}' r='2' fill='#73e79e'/><text x='{:.3}' y='{:.3}' fill='#d8e6e4' stroke='none' font-family='monospace' font-size='12'>{}</text>",cx + 8.,cy - 6.,class.spec().name));
    }
    svg.push_str("</g><g fill='#e0ebe8' font-family='monospace' font-size='16'><text x='30' y='30'>Circular catalog: measured CPU hull footprints in metres</text><text x='30' y='56'>Diagnostic only. Not native pixels, GPU lighting, owned inventory or boarding acceptance.</text>");
    svg.push_str(&format!("<text x='30' y='990'>Drum radius {:.2} m · roof {:.2} m · hover gap {:.2} m · 8 m bridges</text><text x='30' y='1018'>Actual rendered body vertices form each outline; JSON includes full articulated turret sweeps.</text></g></svg>",geometry.outer_radius_m,geometry.ceiling_y,geometry.hover_gap_m));
    let report = serde_json::json!({
        "evidence":"CPU geometry and sampled articulated bounds only; no native/GPU acceptance",
        "layout_revision":revision, "center_m":geometry.center,
        "concourse_radius_m":geometry.concourse_radius_m,"drum_radius_m":geometry.outer_radius_m,
        "ceiling_y_m":geometry.ceiling_y,"hover_gap_m":geometry.hover_gap_m,
        "bridge_half_width_m":geometry.bridge_half_width_m,"catalog_fixture_only":true,
        "measurements":measurements
    });
    std::fs::write(
        directory.join("circular-layout.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    std::fs::write(directory.join("circular-layout.svg"), svg).unwrap();
}

fn convex_xz(points: Vec<Vec3>) -> Vec<[f32; 2]> {
    let mut points: Vec<_> = points.into_iter().map(|p| [p.x, p.z]).collect();
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then_with(|| a[1].total_cmp(&b[1])));
    points.dedup();
    let cross = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let mut hull = Vec::new();
    for point in &points {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], *point) <= 0. {
            hull.pop();
        }
        hull.push(*point);
    }
    let lower = hull.len();
    for point in points.iter().rev().skip(1) {
        while hull.len() > lower && cross(hull[hull.len() - 2], hull[hull.len() - 1], *point) <= 0.
        {
            hull.pop();
        }
        hull.push(*point);
    }
    hull.pop();
    hull
}
