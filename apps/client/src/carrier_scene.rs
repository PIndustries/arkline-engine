//! One confirmed finite-roster ownership partition for all carrier render passes.
//! No home inference, spawning, extrapolation or inventory mutation.
use super::*;

pub(super) fn instances(
    pilot: &PilotView,
    space: Space,
    snapshot_time: u64,
) -> Option<Vec<(u64, arkline_protocol::DroneClass, Mat4)>> {
    let public = pilot.carrier_inventory.as_ref()?;
    if public.sample_time_ms != snapshot_time {
        return None;
    }
    public.validate_projection(pilot).ok()?;
    public
        .inventory
        .entries
        .iter()
        .filter(|e| {
            e.hit_points > 0
                && !matches!(
                    e.location,
                    arkline_protocol::CarrierDroneLocation::Destroyed { .. }
                )
        })
        .map(|entry| {
            let local = public
                .drone_poses
                .iter()
                .find(|(id, _)| *id == entry.drone_id)?
                .1;
            Some((
                entry.drone_id,
                entry.class,
                confirmed_instance(space, public.parent, local)?,
            ))
        })
        .collect()
}

fn confirmed_instance(
    space: Space,
    parent: arkline_protocol::drone_bay_geometry::BayPose,
    local: arkline_protocol::drone_bay_geometry::BayPose,
) -> Option<Mat4> {
    let world = parent.compose(local)?;
    let matrix = Mat4::from_rotation_translation(
        Quat::from_array(world.orientation_xyzw.map(|v| v as f32)).normalize(),
        space.scene(world.position_m),
    );
    matrix.is_finite().then_some(matrix)
}

#[test]
fn confirmed_parent_at_au_composes_once_before_scene_conversion() {
    use arkline_protocol::drone_bay_geometry::BayPose;
    let parent = BayPose {
        position_m: [149_597_870_700., -2_000., 800.],
        orientation_xyzw: [
            0.,
            0.,
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        ],
    };
    let local = BayPose {
        position_m: [4., 3., -22.8],
        orientation_xyzw: [0., 0., 0., 1.],
    };
    let space = Space {
        origin: parent.position_m,
    };
    let result = confirmed_instance(space, parent, local).unwrap();
    assert!(
        result
            .transform_point3(Vec3::ZERO)
            .distance(Vec3::new(-0.03, 0.04, -0.228))
            < 1e-6
    );
    assert!(result.transform_vector3(Vec3::X).distance(Vec3::Y) < 1e-6);
    let mut bad = local;
    bad.position_m[0] = f64::NAN;
    assert!(confirmed_instance(space, parent, bad).is_none());
}
