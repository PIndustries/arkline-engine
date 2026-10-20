//! Proposed CPU tests. Not executed here; root owns Cargo and GPU execution.
use super::*;

fn viewport() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 720.0))
}

fn projection() -> Mat4 {
    Mat4::perspective_rh_gl(COCKPIT_FOV, 4.0 / 3.0, 0.0002, 10.0)
        * Mat4::look_at_rh(Vec3::ZERO, Vec3::X, Vec3::Y)
}

fn face(mount: Mat4) -> DisplayFace {
    DisplayFace::new(
        ShipClass::Pioneer,
        3,
        mount,
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(840.0, 600.0)),
    )
    .unwrap()
}

fn triangle_mesh() -> Arc<[Vertex]> {
    // Actual triangle in x=0, spanning a full display in test coordinates.
    [
        Vec3::new(0.0, -10.0, -10.0),
        Vec3::new(0.0, 10.0, -10.0),
        Vec3::new(0.0, 0.0, 10.0),
    ]
    .map(|p| Vertex { p, n: Vec3::NEG_X })
    .into()
}

fn scene(blockers: &[(f32, bool)]) -> CockpitOcclusion {
    let catalog = Arc::new(DepthMeshCatalog::new(vec![triangle_mesh()]));
    let mut external = Vec::new();
    let mut own = Vec::new();
    for &(x, is_own) in blockers {
        let draw = DepthDraw::whole(&catalog, 0, Mat4::from_translation(Vec3::X * x)).unwrap();
        if is_own {
            own.push(draw);
        } else {
            external.push(draw);
        }
    }
    CockpitOcclusion::new(ShipClass::Pioneer, catalog, external, own).unwrap()
}

fn pointer(vp: Mat4, face: &DisplayFace) -> egui::Pos2 {
    project_point(
        vp,
        viewport(),
        face.mount
            .transform_point3(face.local_point(face.canvas.center()).unwrap()),
    )
    .unwrap()
}

#[test]
fn real_triangle_before_display_blocks_but_triangle_behind_does_not() {
    let face = face(Mat4::IDENTITY);
    let pointer = pointer(projection(), &face);
    let target = face.plane_x;
    for is_own in [false, true] {
        assert!(scene(&[(target - 0.01, is_own)])
            .pick(
                projection(),
                viewport(),
                pointer,
                std::slice::from_ref(&face)
            )
            .is_none());
        assert!(scene(&[(target + 0.01, is_own)])
            .pick(
                projection(),
                viewport(),
                pointer,
                std::slice::from_ref(&face)
            )
            .is_some());
    }
}

#[test]
fn bias_keeps_actual_opaque_backing_behind_rendered_content_and_input() {
    let face = face(Mat4::IDENTITY);
    let pointer = pointer(projection(), &face);
    let backing_front = cockpit_screens(ShipClass::Pioneer)[3].0.x - 0.025 - 0.006;
    assert!(face.plane_x < backing_front);
    assert!(scene(&[(backing_front, true)])
        .pick(projection(), viewport(), pointer, &[face])
        .is_some());
}

#[test]
fn nearest_actual_blocker_wins_independent_of_input_order() {
    let face = face(Mat4::IDENTITY);
    let pointer = pointer(projection(), &face);
    let target = face.plane_x;
    for entries in [
        [(target + 0.1, false), (target - 0.1, true)],
        [(target - 0.1, true), (target + 0.1, false)],
    ] {
        assert!(scene(&entries)
            .pick(
                projection(),
                viewport(),
                pointer,
                std::slice::from_ref(&face)
            )
            .is_none());
    }
}

#[test]
fn nearest_overlapping_display_wins_independent_of_submission_order() {
    let near = face(Mat4::IDENTITY);
    let pointer = pointer(projection(), &near);
    let mut far = near.clone();
    far.index = 5;
    far.plane_x += 0.1;
    for faces in [[far.clone(), near.clone()], [near.clone(), far.clone()]] {
        assert_eq!(
            scene(&[])
                .pick(projection(), viewport(), pointer, &faces)
                .unwrap()
                .index,
            3
        );
    }
}

#[test]
fn scene_ray_distance_survives_real_mount_scale_and_nonuniform_blocker_scale() {
    let rotation = Quat::from_rotation_x(0.6) * Quat::from_rotation_y(0.9);
    let origin = Vec3::new(0.17, -0.11, 0.08);
    let view = Mat4::look_at_rh(origin, origin + rotation * Vec3::X, rotation * Vec3::Y);
    let vp = Mat4::perspective_rh_gl(COCKPIT_FOV, 4.0 / 3.0, 0.0002, 10.0) * view;
    for scale in [0.01, 0.025] {
        let mount = Mat4::from_scale_rotation_translation(Vec3::splat(scale), rotation, origin);
        let face = face(mount);
        let pointer = pointer(vp, &face);
        let catalog = Arc::new(DepthMeshCatalog::new(vec![triangle_mesh()]));
        for (offset, blocked) in [(-0.01, true), (0.01, false)] {
            let model = mount
                * Mat4::from_translation(Vec3::X * (face.plane_x + offset))
                * Mat4::from_scale(Vec3::new(0.3, 1.7, 0.8));
            let external = vec![DepthDraw::whole(&catalog, 0, model).unwrap()];
            let scene =
                CockpitOcclusion::new(ShipClass::Pioneer, catalog.clone(), external, Vec::new())
                    .unwrap();
            assert_eq!(
                scene
                    .pick(vp, viewport(), pointer, std::slice::from_ref(&face))
                    .is_none(),
                blocked
            );
        }
    }
}

#[test]
fn blocker_clipped_before_cockpit_near_plane_does_not_reject_visible_content() {
    let face = face(Mat4::IDENTITY);
    let pointer = pointer(projection(), &face);
    assert!(scene(&[(0.00001, false)])
        .pick(projection(), viewport(), pointer, &[face])
        .is_some());
}

#[test]
fn malformed_triangle_ranges_and_nonfinite_transforms_fail_closed() {
    let face = face(Mat4::IDENTITY);
    let pointer = pointer(projection(), &face);
    let scene = scene(&[(0.3, false)]);
    let catalog = scene.catalog.clone();
    let mut malformed = scene.external[0].clone();
    malformed.vertices = 1..3;
    assert!(CockpitOcclusion::new(
        ShipClass::Pioneer,
        catalog.clone(),
        vec![malformed],
        Vec::new()
    )
    .is_none());
    assert!(DepthDraw::whole(&catalog, 0, Mat4::from_translation(Vec3::splat(f32::NAN))).is_none());
    let mut projective = Mat4::IDENTITY;
    projective.x_axis.w = 0.1;
    assert!(DepthDraw::whole(&catalog, 0, projective).is_none());
    assert!(scene
        .pick(projection(), viewport(), pointer, &[face])
        .is_none());
}

#[test]
fn world_positions_subtract_f64_origin_before_casting_to_scene_units() {
    let space = Space {
        origin: [1.0e12, -1.0e12, 1.0e12],
    };
    assert_eq!(
        space.scene([1.0e12 + 0.5, -1.0e12 + 0.25, 1.0e12 - 0.5]),
        Vec3::new(0.005, 0.0025, -0.005)
    );
}
