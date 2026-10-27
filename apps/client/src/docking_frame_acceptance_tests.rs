//! Client transform acceptance only; these fixtures do not grant walking or
//! complete docking. Real command/traversal acceptance belongs to server tests.
use super::*;
use arkline_protocol::{
    DockingPhase, DockingSegment, DockingState, PioneerDockedPose, SceneLayoutRevision,
};

fn fixture() -> Snapshot {
    let ctx = egui::Context::default();
    let cockpit = crate::tests::view_test_cockpit(&ctx);
    let mut snapshot = cockpit.snapshot.unwrap();
    snapshot.world_layout = Some(SceneLayoutRevision::DockingV1.definition());
    let pilot = &mut snapshot.pilots[0];
    pilot.ship_class = ShipClass::Pioneer;
    pilot.location = Location::Station;
    let flight = pilot.flight.as_mut().unwrap();
    flight.position = [12_000., 35., -700.];
    flight.orientation = [0., 0., 0., 1.];
    flight.docking = Some(DockingState {
        version: arkline_protocol::DOCKING_VERSION,
        host_vessel_id: "frame-fixture".into(),
        berth_id: 2,
        operation_id: 4,
        phase: DockingPhase::Aligning,
        segment: Some(DockingSegment {
            source_m: flight.position,
            target_m: [12_250., 42., -620.],
            initial_velocity_mps: [2., 0., 0.],
            source_orientation: flight.orientation,
            target_orientation: [0., 1., 0., 0.],
            started_at_ms: 1000,
            duration_ms: 3000,
        }),
        clamps: 0.,
        updated_at_ms: 2000,
        hold_reason: None,
        departing: false,
    });
    snapshot.server_time_ms = 2000;
    snapshot
}

fn near_matrix(actual: Mat4, expected: Mat4) {
    for (a, b) in actual
        .to_cols_array()
        .into_iter()
        .zip(expected.to_cols_array())
    {
        assert!((a - b).abs() < 0.0002, "matrix differs: {a} vs {b}");
    }
}

#[test]
fn docking_registered_eyes_parts_and_projection_share_one_rigid_body_transform() {
    let snapshot = fixture();
    let pilot = &snapshot.pilots[0];
    let before = serde_json::to_vec(&snapshot).unwrap();
    let pose = PioneerDockedPose::private();
    let registration = hangar::pioneer_docked_scene::body_frame(pose);
    let canonical = hangar::pioneer_docked_scene::default_body_parts().unwrap();
    let registered: Vec<_> = canonical
        .iter()
        .map(|&(mesh, model, color, emission)| (mesh, registration * model, color, emission))
        .collect();
    let docking = pilot.flight.as_ref().unwrap().docking.as_ref().unwrap();
    let segment = docking.segment.as_ref().unwrap();
    for fraction in [0., 0.25, 0.5, 0.75, 1.] {
        let body_eye = Vec3::from_array(
            arkline_protocol::pilot_joints(fraction)
                .eye_m
                .map(|v| v as f32),
        );
        let local_eye = registration.transform_point3(body_eye);
        let local_look = registration.transform_point3(body_eye + Vec3::X);
        for now in [2000, 2075, 2150, 100_000] {
            let sampled = segment.sample(now.min(2150));
            let rotation = Quat::from_array(sampled.2.map(|v| v as f32));
            let frame = moving_interior_frame(
                &snapshot,
                pilot,
                now,
                true,
                16. / 9.,
                pose,
                local_eye,
                local_look,
                &registered,
            )
            .unwrap();
            assert_eq!(frame.space.origin, sampled.0);
            assert!(frame.eye.distance(rotation * body_eye / 100.) < 0.00001);
            assert!(frame.rotation.abs_diff_eq(rotation, 0.000001));
            let expected_view = Mat4::look_at_rh(
                rotation * body_eye / 100.,
                rotation * (body_eye + Vec3::X) / 100.,
                rotation * Vec3::Y,
            );
            near_matrix(
                frame.cockpit_vp,
                Mat4::perspective_rh_gl(55_f32.to_radians(), 16. / 9., 0.0002, 10.) * expected_view,
            );
            let carried = frame.pioneer_parts.as_ref().unwrap();
            assert_eq!(carried.len(), canonical.len());
            for (actual, expected) in carried.iter().zip(canonical.iter()) {
                assert_eq!(
                    (actual.0, actual.2, actual.3),
                    (expected.0, expected.2, expected.3)
                );
                near_matrix(actual.1, expected.1);
            }
            // Ship translation is applied once by the space origin, never baked
            // into the body-local camera or copied into individual mesh matrices.
            let world_eye = frame.space.world(frame.eye);
            let rotated = rotation * body_eye;
            for axis in 0..3 {
                assert!((world_eye[axis] - sampled.0[axis] - rotated[axis] as f64).abs() < 0.0001);
            }
        }
    }
    assert_eq!(serde_json::to_vec(&snapshot).unwrap(), before);
}

#[test]
fn secured_docking_frame_retains_confirmed_pose_without_phase_or_clock_motion() {
    let mut snapshot = fixture();
    let pilot = &mut snapshot.pilots[0];
    pilot.location = Location::Docked;
    let flight = pilot.flight.as_mut().unwrap();
    flight.position = [14_000., 19., 800.];
    flight.orientation = [0., 1., 0., 0.];
    let docking = flight.docking.as_mut().unwrap();
    docking.phase = DockingPhase::Secured;
    docking.segment = None;
    docking.clamps = 1.;
    let pilot = &snapshot.pilots[0];
    let pose = PioneerDockedPose::private();
    let registration = hangar::pioneer_docked_scene::body_frame(pose);
    let eye = registration.transform_point3(cockpit_fit::eye_body_meters(ShipClass::Pioneer));
    let look = eye + registration.transform_vector3(Vec3::X);
    let a = moving_interior_frame(&snapshot, pilot, 2000, true, 1.5, pose, eye, look, &[]).unwrap();
    let b =
        moving_interior_frame(&snapshot, pilot, 999_000, false, 1.5, pose, eye, look, &[]).unwrap();
    assert_eq!(a.space, b.space);
    assert_eq!(a.eye, b.eye);
    assert_eq!(a.rotation, b.rotation);
    near_matrix(a.cockpit_vp, b.cockpit_vp);
    assert!(
        !b.connected,
        "a carried frame must not invent a live connection"
    );
    assert_eq!(
        pilot
            .flight
            .as_ref()
            .unwrap()
            .docking
            .as_ref()
            .unwrap()
            .phase,
        DockingPhase::Secured
    );
}
