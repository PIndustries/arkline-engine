use super::*;
use arkline_protocol::independent_bay_geometry::{IndependentBayGeometry, IndependentBayMechanism};
fn pose(x: f64) -> BayPose {
    BayPose {
        position_m: [x, 0., 0.],
        orientation_xyzw: [0., 0., 0., 1.],
    }
}
fn frame(
    x: f64,
    hardware: f64,
    phase: IndependentBayPhase,
    milestones: IndependentBayMilestones,
) -> IndependentBayKeyframe {
    IndependentBayKeyframe {
        label: format!("Fixture {x}/{hardware}"),
        phase,
        drone_pose: pose(x),
        mechanism_poses: vec![("fixture/cradle".into(), pose(hardware))],
        milestones,
    }
}
fn fixture(
    action: IndependentBayAction,
) -> (
    IndependentBayMotionStart,
    IndependentBayDefinition,
    Vec<IndependentBayKeyframe>,
) {
    let registration = IndependentBayRegistration {
        schema_version: INDEPENDENT_BAY_SCHEMA_VERSION,
        host: arkline_protocol::drone_bay_definition::PIONEER_BAY_REG12.registration(),
        assembly_revision: 1,
        assembly_source_sha256: "1".repeat(64),
    };
    let safe = IndependentBayMilestones {
        hardware_safe: true,
        ..Default::default()
    };
    let supported = IndependentBayMilestones {
        supported: true,
        clamped: true,
        hardware_safe: true,
        ..Default::default()
    };
    let clear = IndependentBayMilestones {
        exterior_clear: true,
        hardware_safe: true,
        ..Default::default()
    };
    let route = match action {
        IndependentBayAction::Launch => vec![
            frame(0., 0., IndependentBayPhase::Preparing, supported),
            frame(
                2.,
                2.,
                IndependentBayPhase::Launching,
                IndependentBayMilestones {
                    supported: true,
                    ..safe
                },
            ),
            frame(10., 2., IndependentBayPhase::Departing, clear),
            frame(10., 0., IndependentBayPhase::Securing, clear),
        ],
        IndependentBayAction::ReturnToStorage => vec![
            frame(10., 0., IndependentBayPhase::Approaching, clear),
            frame(
                2.,
                2.,
                IndependentBayPhase::Supporting,
                IndependentBayMilestones {
                    supported: true,
                    ..safe
                },
            ),
            frame(0., 0., IndependentBayPhase::Securing, supported),
        ],
        IndependentBayAction::ReturnToUnload { .. } => vec![
            frame(10., 0., IndependentBayPhase::Approaching, clear),
            frame(
                8.,
                1.,
                IndependentBayPhase::Coupling,
                IndependentBayMilestones {
                    coupled: true,
                    ..safe
                },
            ),
            frame(8., 0., IndependentBayPhase::Uncoupling, safe),
            frame(10., 0., IndependentBayPhase::Departing, clear),
        ],
    };
    let definition = IndependentBayDefinition {
        registration: registration.clone(),
        deployed_hardware_bounds_m: [[-5.; 3], [5.; 3]],
        carrier_bounds_m: [[-5.; 3], [5.; 3]],
        bays: vec![IndependentBayGeometry {
            bay_id: 5,
            compatible_classes: vec![DroneClass::Mining],
            preferred_home: 5,
            ore_transfer_capable: true,
            stored_pose: pose(0.),
            support_pose: pose(2.),
            contact_pose: pose(8.),
            gate_pose: pose(10.),
            clear_pose: pose(10.),
            mechanisms: vec![IndependentBayMechanism {
                semantic_id: "fixture/cradle".into(),
                reference_pose: pose(0.),
                local_bounds_m: [[-0.1; 3], [0.1; 3]],
            }],
            working_bounds_m: [[-1.; 3], [11.; 3]],
            drone_bounds_local_m: [[-2.5, -0.9, -3.], [2.5, 0.9, 3.]],
        }],
    };
    let start = IndependentBayMotionStart {
        binding: registration.bind("fixture-vessel".into()),
        operation_id: 1,
        drone_id: 6,
        bay_id: 5,
        action,
        parent: pose(1000.),
        system_id: 1,
        class: DroneClass::Mining,
        actual_body_pose: route[0].drone_pose,
        actual_mechanism_poses: route[0].mechanism_poses.clone(),
        now: 0,
    };
    (start, definition, route)
}
#[test]
fn unregistered_candidates_cannot_create_runtime_or_snap_to_entry() {
    let (start, definition, route) = fixture(IndependentBayAction::Launch);
    assert!(IndependentBayRuntime::begin(start.clone()).is_err());
    let mut bad = start.clone();
    bad.actual_body_pose = pose(99.);
    assert!(IndependentBayRuntime::from_route(bad, &definition, route.clone()).is_err());
    let mut bad_hardware = start.clone();
    bad_hardware.actual_mechanism_poses[0].1.position_m[0] += 0.1;
    assert!(IndependentBayRuntime::from_route(bad_hardware, &definition, route.clone()).is_err());
    let mut bad = definition.clone();
    bad.bays[0].compatible_classes = vec![DroneClass::Recon];
    assert!(IndependentBayRuntime::from_route(start, &bad, route).is_err());
}
#[test]
fn continuous_same_entity_route_requires_post_combat_finalization_not_timer_capture() {
    let (start, definition, route) = fixture(IndependentBayAction::Launch);
    let parent = start.parent;
    let mut run = IndependentBayRuntime::from_route(start, &definition, route.clone()).unwrap();
    let mut before = run.confirmed.clone();
    let mut finalized = false;
    for now in (100..=12000).step_by(100) {
        let result = run.advance(now, true, parent, 1).unwrap();
        assert!(distance(before.drone_pose, run.confirmed.drone_pose) <= 4.000001);
        assert_eq!(run.drone_id, 6);
        assert_eq!(run.parent, parent);
        if result == IndependentBayMotionProgress::FinalizationRequired {
            finalized = true;
            break;
        }
        run.validate_expected(&route, parent, 1).unwrap();
        assert!(run
            .view(run.route[run.target_index].phase, false)
            .unwrap()
            .valid_shape());
        before = run.confirmed.clone();
    }
    assert!(finalized);
    assert!(run.reached.hardware_safe && run.reached.exterior_clear);
    assert_eq!(run.confirmed.drone_pose, pose(10.));
    assert!(run.validate_expected(&route, parent, 1).is_err()); // Never save pending-finalize state.
}
#[test]
fn hold_resume_restart_no_catchup_and_repeated_resume_does_not_reset_clock() {
    let (start, definition, route) = fixture(IndependentBayAction::Launch);
    let parent = start.parent;
    let mut run = IndependentBayRuntime::from_route(start, &definition, route.clone()).unwrap();
    run.advance(500, true, parent, 1).unwrap();
    let at = run.confirmed.clone();
    run.hold("Disconnected").unwrap();
    assert_eq!(
        run.advance(100000, true, parent, 1).unwrap(),
        IndependentBayMotionProgress::Held
    );
    assert_eq!(run.confirmed, at);
    let mut reopened: IndependentBayRuntime =
        serde_json::from_str(&serde_json::to_string(&run).unwrap()).unwrap();
    reopened.validate_expected(&route, parent, 1).unwrap();
    assert!(reopened.resume_validated(100000).unwrap());
    assert_eq!(reopened.confirmed, at);
    let deadline = (reopened.started_at_ms, reopened.duration_ms);
    assert!(!reopened.resume_validated(100001).unwrap());
    assert_eq!((reopened.started_at_ms, reopened.duration_ms), deadline);
    assert_eq!(
        reopened.advance(103000, true, parent, 1).unwrap(),
        IndependentBayMotionProgress::Held
    );
    assert_eq!(reopened.confirmed, at);
}
#[test]
fn transfer_barrier_is_pose_stable_and_never_becomes_stored() {
    let action = IndependentBayAction::ReturnToUnload {
        resume_mining: true,
    };
    let (start, definition, route) = fixture(action);
    let parent = start.parent;
    let mut run = IndependentBayRuntime::from_route(start, &definition, route.clone()).unwrap();
    for now in (100..=2000).step_by(100) {
        if run.advance(now, true, parent, 1).unwrap()
            == IndependentBayMotionProgress::TransferBarrier
        {
            break;
        }
    }
    assert!(run.waiting_for_transfer);
    let at = run.confirmed.clone();
    for now in (2100..=4000).step_by(100) {
        assert_eq!(
            run.advance(now, true, parent, 1).unwrap(),
            IndependentBayMotionProgress::TransferBarrier
        );
        assert_eq!(run.confirmed, at);
    }
    assert!(run
        .view(IndependentBayPhase::WaitingForManifold, false)
        .unwrap()
        .valid_shape());
    run.release_transfer_barrier(4000).unwrap();
    let mut done = false;
    for now in (4100..=9000).step_by(100) {
        if run.advance(now, true, parent, 1).unwrap()
            == IndependentBayMotionProgress::FinalizationRequired
        {
            done = true;
            break;
        }
    }
    assert!(done);
    assert!(run.reached.exterior_clear);
    assert!(!run.reached.clamped);
}
#[test]
fn persisted_route_cursor_clock_pose_and_parent_tampering_fail_closed() {
    let (start, definition, route) = fixture(IndependentBayAction::Launch);
    let parent = start.parent;
    let mut run = IndependentBayRuntime::from_route(start, &definition, route.clone()).unwrap();
    run.advance(500, true, parent, 1).unwrap();
    run.hold("Restart").unwrap();
    let mut bad = run.clone();
    bad.route.last_mut().unwrap().drone_pose = pose(999.);
    assert!(bad.validate_expected(&route, parent, 1).is_err());
    let mut bad = run.clone();
    bad.confirmed.drone_pose = pose(99.);
    assert!(bad.validate_expected(&route, parent, 1).is_err());
    let mut bad = run.clone();
    bad.duration_ms += 1;
    assert!(bad.validate_expected(&route, parent, 1).is_err());
    let mut bad = run.clone();
    bad.segment_source.drone_pose.position_m[1] += 1.;
    assert!(bad.validate_expected(&route, parent, 1).is_err());
    let mut bad = run.clone();
    bad.reached.hardware_safe = false;
    assert!(bad.validate_expected(&route, parent, 1).is_err());
    assert!(run.validate_expected(&route, pose(1001.), 1).is_err());
    assert!(run.validate_expected(&route, parent, 2).is_err());
}
#[test]
fn scoped_obstruction_holds_pose_and_non_mining_bay_cannot_offer_transfer() {
    let (start, definition, route) = fixture(IndependentBayAction::Launch);
    let parent = start.parent;
    let mut run = IndependentBayRuntime::from_route(start, &definition, route).unwrap();
    let initial = run.confirmed.clone();
    assert_eq!(
        run.advance(100, false, parent, 1).unwrap(),
        IndependentBayMotionProgress::Held
    );
    assert_eq!(run.confirmed, initial);
    let (start, mut definition, route) = fixture(IndependentBayAction::ReturnToUnload {
        resume_mining: false,
    });
    definition.bays[0].ore_transfer_capable = false;
    assert!(IndependentBayRuntime::from_route(start, &definition, route).is_err());
}

#[test]
fn coupled_resume_refreshes_pose_clock_but_obstruction_prevents_transfer() {
    let (start, definition, route) = fixture(IndependentBayAction::ReturnToUnload {
        resume_mining: true,
    });
    let parent = start.parent;
    let mut run = IndependentBayRuntime::from_route(start, &definition, route.clone()).unwrap();
    for now in (100..=2000).step_by(100) {
        if run.advance(now, true, parent, 1).unwrap()
            == IndependentBayMotionProgress::TransferBarrier
        {
            break;
        }
    }
    assert!(run.waiting_for_transfer);
    let at = run.confirmed.clone();
    run.hold("Disconnected at coupler").unwrap();
    run.resume_validated(100000).unwrap();
    assert_eq!(run.confirmed, at);
    assert_eq!(
        run.advance(100100, true, parent, 1).unwrap(),
        IndependentBayMotionProgress::TransferBarrier
    );
    run.validate_expected(&route, parent, 1).unwrap();
    assert_eq!(
        run.advance(100200, false, parent, 1).unwrap(),
        IndependentBayMotionProgress::Held
    );
    assert_eq!(run.confirmed, at);
    assert!(run.waiting_for_transfer);
    assert!(run.release_transfer_barrier(100200).is_err());
}
