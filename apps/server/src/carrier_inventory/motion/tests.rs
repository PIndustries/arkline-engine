use super::*;
use arkline_protocol::carrier_inventory_geometry::*;
fn pose(x: f64) -> BayPose {
    BayPose {
        position_m: [x, 0., 0.],
        orientation_xyzw: [0., 0., 0., 1.],
    }
}
fn fixture() -> (
    CarrierInventoryDefinition,
    CarrierMotionStart,
    Vec<CarrierInventoryKeyframe>,
) {
    let (topology, inventory) = crate::carrier_inventory::tests::fixture();
    let names = [
        ("empty", CarrierMechanismOwner::Cassette { cassette_id: 0 }),
        (
            "loaded",
            CarrierMechanismOwner::Cassette { cassette_id: 10 },
        ),
        ("loader", CarrierMechanismOwner::Loader { loader_id: 0 }),
    ];
    let definition = CarrierInventoryDefinition {
        topology,
        carrier_bounds_m: [[-1.; 3], [1.; 3]],
        deployed_hardware_bounds_m: [[-1.; 3], [1.; 3]],
        cassettes: vec![],
        docks: [
            (CarrierCassetteDock::Protected { slot_id: 0 }, 10.),
            (CarrierCassetteDock::Ready { berth_id: 0 }, 0.),
            (CarrierCassetteDock::TransitBuffer { buffer_id: 0 }, 5.),
        ]
        .into_iter()
        .map(|(dock, x)| CarrierDockGeometry {
            dock,
            cassette_pose: pose(x),
            working_bounds_m: [[-1.; 3], [1.; 3]],
        })
        .collect(),
        berths: vec![],
        mechanisms: names
            .into_iter()
            .map(|(name, owner)| CarrierMechanismGeometry {
                semantic_id: name.into(),
                owner,
                reference_pose: pose(0.),
                local_bounds_m: [[-0.1; 3], [0.1; 3]],
            })
            .collect(),
    };
    let route: Vec<_> = [(0., 10., 0u8), (5., 10., 1), (5., 0., 2), (10., 0., 3)]
        .into_iter()
        .enumerate()
        .map(|(i, (empty, loaded, legs))| CarrierInventoryKeyframe {
            label: format!("Synthetic ownership/motion endpoint{i}"),
            phase: CarrierOperationPhase::Loading,
            drone_pose: Some(pose(loaded)),
            handled_cassette_id: Some(if i == 2 { 10 } else { 0 }),
            carried_cassette_id: None,
            cassette_poses: vec![(0, pose(empty)), (10, pose(loaded))],
            docked_cassettes: vec![
                (
                    0,
                    match i {
                        0 => CarrierCassetteDock::Ready { berth_id: 0 },
                        1 | 2 => CarrierCassetteDock::TransitBuffer { buffer_id: 0 },
                        _ => CarrierCassetteDock::Protected { slot_id: 0 },
                    },
                ),
                (
                    10,
                    if i < 2 {
                        CarrierCassetteDock::Protected { slot_id: 0 }
                    } else {
                        CarrierCassetteDock::Ready { berth_id: 0 }
                    },
                ),
            ],
            machinery: vec![
                ("empty".into(), pose(empty)),
                ("loaded".into(), pose(loaded)),
                ("loader".into(), pose(if i == 3 { 0. } else { empty })),
            ],
            completed_stage_legs: legs,
            milestones: CarrierInventoryMilestones {
                drone_clamped: true,
                cassette_supported: true,
                cassette_locked: true,
                loader_clear: i > 0,
                hardware_safe: i == 3,
                ..Default::default()
            },
        })
        .collect();
    let start = CarrierMotionStart {
        binding: inventory.binding,
        operation_id: 1,
        drone_id: 11,
        class: DroneClass::Mining,
        kind: CarrierRouteKind::Stage {
            direction: CarrierStageDirection::ToReady,
            segment: CarrierStageSegment::Full,
            source_slot: 0,
            destination_berth: 0,
            loaded_cassette: 10,
            empty_cassette: 0,
        },
        parent: pose(3e11),
        system_id: 1,
        actual: CarrierMotionFrame::from(&route[0]),
        now: 100,
    };
    (definition, start, route)
}
#[test]
fn unsupported_source_does_not_accept_even_structurally_valid_route() {
    let (_, start, _) = fixture();
    assert!(CarrierMotionRuntime::begin(start).is_err());
}
#[test]
fn cassette_drone_and_groups_share_clock_hold_resume_and_finalization() {
    let (d, start, route) = fixture();
    let mut r = CarrierMotionRuntime::from_route(start, &d, route.clone()).unwrap();
    let first_end = r.started_at_ms + r.duration_ms;
    r.advance(1100, true).unwrap();
    let frozen = r.confirmed.clone();
    r.hold("Operator pause");
    assert_eq!(
        r.advance(100000, true).unwrap(),
        CarrierMotionProgress::Held
    );
    assert_eq!(r.confirmed, frozen);
    r.validate_expected(&route, r.parent, 1, &frozen).unwrap();
    r.resume_validated(100000).unwrap();
    assert_eq!(r.confirmed, frozen);
    assert!(r.started_at_ms + r.duration_ms > first_end);
    loop {
        let now = r.confirmed_at_ms + 100;
        let progress = r.advance(now, true).unwrap();
        if progress == CarrierMotionProgress::FinalizationRequired {
            break;
        }
    }
    assert_eq!(r.completed_stage_legs, 3);
    assert!(r.reached.loader_clear && r.reached.hardware_safe);
    assert_eq!(r.confirmed, CarrierMotionFrame::from(route.last().unwrap()));
    assert!(r
        .validate_expected(&route, r.parent, 1, &r.confirmed)
        .is_err());
}
#[test]
fn malformed_future_identity_actual_entry_and_component_clock_reject() {
    let (d, start, route) = fixture();
    let mut wrong = start.clone();
    wrong.actual.poses[1].1.position_m[0] += 1.;
    assert!(CarrierMotionRuntime::from_route(wrong, &d, route.clone()).is_err());
    let mut bad = route.clone();
    bad[1].machinery[0].0 = "foreign-side".into();
    assert!(CarrierMotionRuntime::from_route(start.clone(), &d, bad).is_err());
    let mut bad = route.clone();
    bad[1].cassette_poses.remove(0);
    assert!(CarrierMotionRuntime::from_route(start.clone(), &d, bad).is_err());
    let mut r = CarrierMotionRuntime::from_route(start, &d, route.clone()).unwrap();
    r.advance(1100, true).unwrap();
    let actual = r.confirmed.clone();
    r.confirmed.poses[1].1.position_m[0] += 0.1;
    assert!(r.validate_expected(&route, r.parent, 1, &actual).is_err());
    r.confirmed = actual;
    r.duration_ms /= 2;
    assert!(r
        .validate_expected(&route, r.parent, 1, &r.confirmed)
        .is_err());
}
#[test]
fn coupled_hold_long_resume_refreshes_clock_and_obstruction_blocks_transfer() {
    let (mut d, mut start, mut route) = fixture();
    start.kind = CarrierRouteKind::Bay {
        berth_id: 0,
        cassette_id: 0,
        action: CarrierInventoryAction::ReturnToUnload {
            resume_mining: true,
        },
    };
    d.mechanisms.retain(|g| g.semantic_id == "empty");
    for frame in &mut route {
        frame.cassette_poses.retain(|(id, _)| *id == 0);
        frame.docked_cassettes.clear();
        frame.machinery.retain(|(id, _)| id == "empty");
        frame.completed_stage_legs = 0;
        frame.handled_cassette_id = Some(0);
        frame.phase = CarrierOperationPhase::Servicing;
    }
    route.last_mut().unwrap().milestones.exterior_clear = true;
    start.actual = CarrierMotionFrame::from(&route[0]);
    route[1].milestones.ore_coupled = true;
    let mut r = CarrierMotionRuntime::from_route(start, &d, route.clone()).unwrap();
    loop {
        let now = r.confirmed_at_ms + 100;
        if r.advance(now, true).unwrap() == CarrierMotionProgress::TransferBarrier {
            break;
        }
    }
    let pose = r.confirmed.clone();
    r.hold("Held at mouth");
    r.resume_validated(100000).unwrap();
    assert_eq!(
        r.advance(100100, true).unwrap(),
        CarrierMotionProgress::TransferBarrier
    );
    assert_eq!(r.confirmed, pose);
    assert_eq!(
        r.advance(100200, false).unwrap(),
        CarrierMotionProgress::Held
    );
    assert!(r.release_transfer_barrier(100300).is_err());
    r.resume_validated(100400).unwrap();
    r.release_transfer_barrier(100400).unwrap();
    assert!(!r.waiting_for_transfer);
}

#[test]
fn stage_completion_requires_each_exact_dock_and_loader_handoff() {
    let (d, start, route) = fixture();
    let mut bad = route.clone();
    bad[0].completed_stage_legs = 3;
    assert!(CarrierMotionRuntime::from_route(start.clone(), &d, bad).is_err());
    let mut bad = route.clone();
    bad[1].docked_cassettes.retain(|(id, _)| *id != 0);
    assert!(CarrierMotionRuntime::from_route(start.clone(), &d, bad).is_err());
    let mut bad = route.clone();
    bad[2].milestones.loader_clear = false;
    assert!(CarrierMotionRuntime::from_route(start.clone(), &d, bad).is_err());
    let mut bad = route.clone();
    bad[1].handled_cassette_id = Some(10);
    assert!(CarrierMotionRuntime::from_route(start.clone(), &d, bad).is_err());
    let mut bad = route;
    bad[0].handled_cassette_id = None;
    assert!(CarrierMotionRuntime::from_route(start, &d, bad).is_err());
}

#[test]
fn exact_1702_head_death_extends_original_full_cleanup_without_pose_or_clock_reset() {
    let mut reg = geometry::stationary_fixture_definition()
        .unwrap()
        .topology
        .registration
        .clone();
    reg.assembly_revision = 1702;
    reg.assembly_source_sha256 =
        "57059d059a267da00ec2ae4d4a4dfd27393cd2280239bb241ffb751ea506b113".into();
    let d = geometry::stationary_fixture_for_registration(&reg).unwrap();
    let kind = CarrierRouteKind::Stage {
        direction: CarrierStageDirection::ToReady,
        segment: CarrierStageSegment::Head,
        source_slot: 0,
        destination_berth: 0,
        loaded_cassette: 10,
        empty_cassette: 0,
    };
    let path = kind.route(d, DroneClass::Mining).unwrap();
    let start = CarrierMotionStart {
        binding: reg.bind("fixture".into()),
        operation_id: 1,
        drone_id: 2,
        class: DroneClass::Mining,
        kind,
        parent: pose(0.),
        system_id: 1,
        actual: CarrierMotionFrame::from(&path[0]),
        now: 1000,
    };
    for final_boundary in [false, true] {
        let mut r = CarrierMotionRuntime::from_route(start.clone(), d, path.clone()).unwrap();
        let mut now = 1000;
        loop {
            now += 1000;
            r.advance(now, true).unwrap();
            if (final_boundary && r.ready_to_finalize)
                || (!final_boundary && r.completed_stage_legs == 1)
            {
                break;
            }
        }
        let before = r.confirmed.clone();
        r.retain_full_destroyed_cleanup(d, now).unwrap();
        assert_eq!(r.confirmed, before);
        assert_eq!(r.kind.stage_segment(), Some(CarrierStageSegment::Full));
        assert!(!r.ready_to_finalize);
        for _ in 0..400 {
            now += 1000;
            r.advance(now, true).unwrap();
            if r.ready_to_finalize {
                break;
            }
        }
        assert!(r.ready_to_finalize && r.reached.hardware_safe);
        assert_eq!(r.completed_stage_legs, 3);
    }
}
