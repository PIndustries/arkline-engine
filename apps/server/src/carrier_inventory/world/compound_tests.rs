//! Continuous narrowphase regressions for exact source-bound rigid solids.
use super::*;
use crate::independent_bays::independent_bay_sweeps_clear;
fn definition(revision: u32) -> &'static CarrierInventoryDefinition {
    let mut registration = geometry::stationary_fixture_definition()
        .unwrap()
        .topology
        .registration
        .clone();
    if revision == 1703 {
        registration.assembly_revision = 1703;
        registration.assembly_source_sha256 =
            "79ab43221861f74383ca36ca6a742ff5588e08003eb7b10f9d8b6f0d5469664a".into();
    }
    geometry::stationary_fixture_for_registration(&registration).unwrap()
}
fn run(def: &CarrierInventoryDefinition) -> CarrierMotionRuntime {
    let kind = CarrierRouteKind::Stage {
        direction: CarrierStageDirection::ToReady,
        segment: CarrierStageSegment::Full,
        source_slot: 0,
        destination_berth: 0,
        loaded_cassette: 10,
        empty_cassette: 0,
    };
    let route = kind.route(def, DroneClass::Mining).unwrap();
    CarrierMotionRuntime::from_route(
        CarrierMotionStart {
            binding: CarrierInventoryBinding {
                vessel_id: "fixture-vessel".into(),
                registration: def.topology.registration.clone(),
            },
            operation_id: 1,
            drone_id: 2,
            class: DroneClass::Mining,
            kind,
            parent: BayPose {
                position_m: [0.; 3],
                orientation_xyzw: [0., 0., 0., 1.],
            },
            system_id: 1,
            actual: CarrierMotionFrame::from(&route[0]),
            now: 0,
        },
        def,
        route,
    )
    .unwrap()
}
fn source_body(def: &CarrierInventoryDefinition, berth: u8, id: &str) -> CarrierSweptBody {
    let route = def
        .stage_route(
            DroneClass::Mining,
            u16::from(berth),
            berth,
            u64::from(berth) + 10,
            u64::from(berth),
        )
        .unwrap();
    assert_eq!(route[6].label, "empty cassette translates to lower pocket");
    let segment = BayMotionSegment {
        source: route[5].machinery.iter().find(|(n, _)| n == id).unwrap().1,
        target: route[6].machinery.iter().find(|(n, _)| n == id).unwrap().1,
        started_at_ms: 0,
        duration_ms: 5400,
    };
    let key = CarrierPoseKey::Mechanism {
        semantic_id: id.into(),
    };
    CarrierSweptBody::new(
        IndependentBaySweptBody {
            segment,
            local_bounds_m: body_bounds(def, &key).unwrap(),
        },
        Some(
            def.mechanism_collision_parts(id)
                .unwrap()
                .iter()
                .map(|part| IndependentBaySweptBody {
                    segment,
                    local_bounds_m: part.local_bounds_m,
                })
                .collect(),
        ),
    )
}
#[test]
fn exact_failed_receiver_cassette_union_is_refined_without_ignoring_real_solids() {
    let def = definition(1703);
    let receiver = source_body(def, 0, "b00/receiver/stage/1");
    let cassette = source_body(def, 1, "c01/cassette/base");
    assert!(!independent_bay_sweeps_clear(
        receiver.union.body,
        cassette.union.body,
        500,
        600,
        0.001
    ));
    assert!(compounds_clear(&receiver, &cassette, 500, 600, 0.001));
    let mut oversized = cassette.clone();
    oversized.leaves = Some(vec![oversized.union.clone()]);
    assert!(
        !compounds_clear(&receiver, &oversized, 500, 600, 0.001),
        "solid-filled negative control must block"
    );
    let actual_solid = cassette.parts()[0].body;
    let obstruction = CarrierSweptBody::single(actual_solid);
    assert!(
        !compounds_clear(&cassette, &obstruction, 500, 600, 0.001),
        "no free-body/asteroid exemption inside actual solid"
    );
    for leaf in cassette.parts() {
        assert_eq!(
            leaf.body.segment, cassette.union.body.segment,
            "one accepted clock for every leaf"
        );
    }
}
#[test]
fn cassette_identity_proxy_requires_complete_source_confirmed_and_target_solid_coverage() {
    let def = definition(1703);
    let original = run(def);
    let resolved = bodies(def, &original).unwrap();
    let cassette_keys = original
        .confirmed
        .poses
        .iter()
        .filter(|(k, _)| matches!(k, CarrierPoseKey::Cassette { .. }))
        .count();
    assert_eq!(
        resolved.len(),
        original.confirmed.poses.len() - cassette_keys
    );
    let key = CarrierPoseKey::Mechanism {
        semantic_id: "c00/cassette/base".into(),
    };
    for missing in 0..3 {
        let mut bad = original.clone();
        match missing {
            0 => bad.source.poses.retain(|(k, _)| *k != key),
            1 => bad.confirmed.poses.retain(|(k, _)| *k != key),
            2 => bad.route[bad.target_index]
                .machinery
                .retain(|(id, _)| id != "c00/cassette/base"),
            _ => unreachable!(),
        }
        assert!(bodies(def, &bad).is_err(), "missing frame {missing}");
    }
    let mut foreign = def.clone();
    foreign.mechanisms[0].local_bounds_m[0][0] -= 1.;
    assert!(!foreign.has_complete_compound_collision());
    assert!(bodies(&foreign, &original).is_err());
}
#[test]
fn historical_source_keeps_identical_aggregate_body_shapes() {
    let def = definition(1701);
    let r = run(def);
    assert!(!def.has_complete_compound_collision());
    let actual = bodies(def, &r).unwrap();
    assert_eq!(actual.len(), r.confirmed.poses.len());
    for (body, (key, _)) in actual.iter().zip(&r.confirmed.poses) {
        assert!(body.leaves.is_none());
        assert_eq!(
            body.union.body.local_bounds_m,
            body_bounds(def, key).unwrap()
        );
    }
}

fn resegment(
    body: CarrierSweptBody,
    change: impl Fn(BayMotionSegment) -> BayMotionSegment,
) -> CarrierSweptBody {
    let map = |body: IndependentBaySweptBody| IndependentBaySweptBody {
        segment: change(body.segment),
        ..body
    };
    CarrierSweptBody::new(
        map(body.union.body),
        body.leaves
            .map(|parts| parts.into_iter().map(|p| map(p.body)).collect()),
    )
}
#[test]
fn compound_leaf_clocks_preserve_different_start_times_and_held_peer_pose() {
    let def = definition(1703);
    let ours = resegment(source_body(def, 0, "c00/cassette/base"), |mut s| {
        s.started_at_ms = 1000;
        s
    });
    let peer = resegment(source_body(def, 1, "c01/cassette/base"), |mut s| {
        s.started_at_ms = 700;
        s
    });
    assert!(compounds_clear(&ours, &peer, 1000, 6000, 0.001));
    assert!(peer.union.prepared.get().is_some());
    let stopped = peer.union.body.segment.sample(1900).unwrap().0;
    // Changed accepted state creates a new immutable context, never mutates a
    // prepared shape or reuses a prior moving segment for the held body.
    let peer = resegment(peer, |_| stationary(stopped, 1900));
    assert!(peer.union.prepared.get().is_none());
    assert!(peer.parts().iter().all(|p| p.prepared.get().is_none()));
    assert!(compounds_clear(&ours, &peer, 1900, 6400, 0.001));
    for leaf in peer.parts() {
        assert_eq!(leaf.body.segment.sample(1900).unwrap().0, stopped);
        assert_eq!(leaf.body.segment.sample(6400).unwrap().0, stopped);
    }
}

fn owner_with_runs(runs: BTreeMap<u64, CarrierMotionRuntime>) -> OwnedCarrier {
    let binding = runs.values().next().unwrap().binding.clone();
    OwnedCarrier {
        inventory: CarrierInventoryState {
            schema_version: CARRIER_INVENTORY_SCHEMA_VERSION,
            binding: binding.clone(),
            initialized_at_ms: 0,
            roster: vec![],
            entries: vec![],
            cassettes: vec![],
            recall_high_watermark: 0,
            recalls: vec![],
            deployment_high_watermark: 0,
            deployments: vec![],
            high_watermark: 0,
            operations: vec![],
            reservations: vec![],
            cleanups: vec![],
        },
        ore: CarrierOreState {
            binding,
            holds: vec![],
            lost_ore_units: 0,
            service_high_watermark: 0,
            services: vec![],
            waiting: vec![],
            granted_service_id: None,
            last_committed_at_ms: None,
        },
        mining_blocks: BTreeMap::new(),
        cassette_poses: BTreeMap::new(),
        drone_poses: BTreeMap::new(),
        machinery: BTreeMap::new(),
        runs,
        departures: BTreeMap::new(),
        arrivals: BTreeMap::new(),
    }
}
#[test]
fn borrowed_peer_shapes_are_lazy_and_errors_remain_at_original_peer() {
    let def = definition(1703);
    let good = run(def);
    let mut bad = good.clone();
    bad.source.poses.retain(|(key, _)| {
        *key != CarrierPoseKey::Mechanism {
            semantic_id: "c00/cassette/base".into(),
        }
    });
    let owner = owner_with_runs(BTreeMap::from([(1, good), (2, bad)]));
    let context = PeerSweeps::new(def, &owner);
    assert!(context.runs.values().all(|slot| slot.get().is_none()));
    let first = context.run(1).unwrap();
    assert!(!first.is_empty());
    assert!(std::ptr::eq(
        first.as_ptr(),
        context.run(1).unwrap().as_ptr()
    ));
    assert!(
        context.runs[&2].get().is_none(),
        "unqueried invalid peer stays unexamined"
    );
    let expected = bodies(def, &owner.runs[&2]).err().unwrap().to_string();
    for _ in 0..2 {
        assert_eq!(context.run(2).err().unwrap().to_string(), expected);
        assert!(context.runs[&2].get().is_none(), "failure is not cached");
    }
}
#[test]
fn new_owner_snapshot_rebuilds_warmed_peer_shapes_and_motion() {
    let def = definition(1703);
    let owner = owner_with_runs(BTreeMap::from([(1, run(def))]));
    let context = PeerSweeps::new(def, &owner);
    let old = context.run(1).unwrap();
    for body in old {
        let _ = body.union.shape();
    }
    let mut changed = owner.clone();
    let r = changed.runs.get_mut(&1).unwrap();
    r.held = true;
    r.confirmed_at_ms = 7_000;
    for (_, pose) in &mut r.confirmed.poses {
        pose.position_m[0] += 0.125;
    }
    let fresh = PeerSweeps::new(def, &changed);
    assert!(fresh.runs[&1].get().is_none());
    let rebuilt = fresh.run(1).unwrap();
    let direct = bodies(def, &changed.runs[&1]).unwrap();
    assert_eq!(rebuilt.len(), direct.len());
    assert_ne!(rebuilt[0].union.body.segment, old[0].union.body.segment);
    for (a, b) in rebuilt.iter().zip(&direct) {
        assert_eq!(a.union.body.segment, b.union.body.segment);
        assert_eq!(a.union.body.local_bounds_m, b.union.body.local_bounds_m);
        assert!(a.union.prepared.get().is_none());
        for (a, b) in a.parts().iter().zip(b.parts()) {
            assert_eq!(a.body.segment, b.body.segment);
            assert_eq!(a.body.local_bounds_m, b.body.local_bounds_m);
        }
    }
}

#[test]
fn warm_peer_queries_preserve_early_block_and_later_error_order() {
    let def = definition(1703);
    let world = World {
        carrier_inventory_registration: Some(def.topology.registration.clone()),
        ..World::default()
    };
    let first = run(def);
    let mut second = first.clone();
    second.operation_id = 2;
    let mut invalid = first.clone();
    invalid.operation_id = 3;
    invalid.source.poses.retain(|(key, _)| {
        *key != CarrierPoseKey::Mechanism {
            semantic_id: "c00/cassette/base".into(),
        }
    });
    let owner = owner_with_runs(BTreeMap::from([(1, first), (2, second), (3, invalid)]));
    let context = PeerSweeps::new(def, &owner);
    // The identical first two bodies obstruct one another. The malformed third
    // peer must not be inspected merely because it exists in the snapshot.
    for id in [2, 1, 2, 1] {
        let r = &owner.runs[&id];
        let reference = clear(&world, "unused-before-block", &owner, r, 100).unwrap();
        assert!(!reference);
        assert_eq!(
            clear_with_peers(&world, "unused-before-block", &context, r, 100).unwrap(),
            reference
        );
        assert!(context.runs[&3].get().is_none());
        let ours = [bodies(def, r).unwrap()[0].union.body];
        let reference = clear_bodies(
            &world,
            "unused-before-block",
            &owner,
            99,
            None,
            r.parent,
            &ours,
            0,
            100,
        )
        .unwrap();
        assert!(!reference);
        assert_eq!(
            clear_bodies_with_peers(
                &world,
                "unused-before-block",
                &context,
                99,
                None,
                r.parent,
                &ours,
                0,
                100
            )
            .unwrap(),
            reference
        );
        assert!(context.runs[&3].get().is_none());
    }
    let mut changed = owner.clone();
    changed.runs.remove(&2);
    let fresh = PeerSweeps::new(def, &changed);
    // Once the earlier obstruction is absent, the original malformed-peer error
    // is reached. A warm success or failure from another snapshot is not reused.
    for _ in 0..2 {
        let r = &changed.runs[&1];
        let reference = clear(&world, "unused-before-error", &changed, r, 100)
            .unwrap_err()
            .to_string();
        assert_eq!(
            clear_with_peers(&world, "unused-before-error", &fresh, r, 100)
                .unwrap_err()
                .to_string(),
            reference
        );
        assert!(fresh.runs[&3].get().is_none());
    }
}

#[test]
fn one_dynamic_hazard_per_query_matches_fresh_hazard_for_every_body() {
    let def = definition(1703);
    let ours = bodies(def, &run(def)).unwrap();
    for (x, radius) in [(0., 1.), (100., 2.), (-20., 0.5), (0., f64::NAN)] {
        let body = IndependentBaySweptBody {
            segment: stationary(
                BayPose {
                    position_m: [x, 0., 0.],
                    orientation_xyzw: [0., 0., 0., 1.],
                },
                0,
            ),
            local_bounds_m: [[-radius; 3], [radius; 3]],
        };
        let shared = CarrierSweptBody::single(body);
        for (start, end) in [(0, 100), (500, 600), (600, 500)] {
            for a in &ours {
                assert_eq!(
                    compounds_clear(a, &shared, start, end, 0.001),
                    compounds_clear(a, &CarrierSweptBody::single(body), start, end, 0.001)
                );
            }
        }
    }
}
