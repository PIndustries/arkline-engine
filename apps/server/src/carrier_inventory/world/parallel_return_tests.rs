//! Zero-payload stationary1703 recovery. This is not ore service, travel
//! admission, or a replacement for the frozen100ms deployment benchmark.
use super::*;

fn launch_one() -> Fixture {
    let mut f = parallel_tests::fixture();
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 1,
        drone_id: 1,
        action: CarrierInventoryAction::Launch,
    });
    for _ in 0..60 {
        f.coarse_step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        if s.runs.is_empty() && s.departures.is_empty() {
            return f;
        }
    }
    panic!("Actual launch/departure failed to clear");
}

#[test]
fn actual_1703_zero_payload_capture_reopens_approach_and_hardware_same_identity() {
    let mut f = launch_one();
    // Synthetic loaded-state negative, not an extraction/service gameplay proof.
    // It must fail at the explicit payload gate without deleting a unit of ore.
    let mut loaded = f.world();
    let loaded_state = loaded
        .accounts
        .get_mut("Pilot")
        .unwrap()
        .carrier_inventory
        .as_mut()
        .unwrap();
    loaded_state
        .ore
        .holds
        .iter_mut()
        .find(|h| h.drone_id == 1)
        .unwrap()
        .ore_units = 1;
    assert!(loaded_state.ore.validate_shape(&loaded_state.inventory));
    let loaded_before = serde_json::to_vec(&loaded).unwrap();
    let error = execute(
        &mut loaded,
        "Pilot",
        &CarrierInventoryRequest::Begin {
            binding: f.binding(),
            operation_id: 2,
            drone_id: 1,
            action: CarrierInventoryAction::RecallToStorage,
        },
        f.now,
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("Loaded return requires physical ore service"));
    assert_eq!(serde_json::to_vec(&loaded).unwrap(), loaded_before);
    let original = state(&f.world(), "Pilot").unwrap().inventory.roster.clone();
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 2,
        drone_id: 1,
        action: CarrierInventoryAction::RecallToStorage,
    });
    let mut interrupted = BTreeSet::new();
    for tick in 0..200 {
        f.coarse_step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        if s.inventory.operation(2).unwrap().phase == CarrierOperationPhase::Completed {
            break;
        }
        let domain = if s.arrivals.contains_key(&2) {
            Some("approach")
        } else if s.runs.contains_key(&2) {
            Some("capture")
        } else {
            None
        };
        if let Some(domain) = domain.filter(|d| interrupted.insert(*d)) {
            f.request(CarrierInventoryRequest::Hold {
                binding: f.binding(),
                operation_id: 2,
            });
            let held = state(&f.world(), "Pilot").unwrap().clone();
            f.reopen();
            f.coarse_step();
            let current = state(&f.world(), "Pilot").unwrap().clone();
            assert_eq!(held.drone_poses, current.drone_poses, "{domain}");
            assert_eq!(held.cassette_poses, current.cassette_poses, "{domain}");
            assert_eq!(held.machinery, current.machinery, "{domain}");
            f.snapshot_valid();
            f.request(CarrierInventoryRequest::Resume {
                binding: f.binding(),
                operation_id: 2,
            });
        }
        if tick % 20 == 0 {
            f.snapshot_valid();
        }
    }
    f.snapshot_valid();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(interrupted, BTreeSet::from(["approach", "capture"]));
    assert_eq!(s.inventory.roster, original);
    assert_eq!(
        s.inventory.entry(1).unwrap().location,
        CarrierDroneLocation::Ready { berth_id: 0 }
    );
    assert_eq!(s.inventory.entry(1).unwrap().hit_points, 100);
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    assert!(s.runs.is_empty() && s.arrivals.is_empty() && s.inventory.reservations.is_empty());
    assert_eq!(s.ore.ore_units(), Some(0));
}

#[test]
fn actual_1703_reverse_exchange_reopens_all_three_legs_and_keeps_full_ownership() {
    let mut f = launch_one();
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 2,
        drone_id: 2,
        action: CarrierInventoryAction::Stage { berth_id: 0 },
    });
    for _ in 0..180 {
        f.coarse_step();
        if state(&f.world(), "Pilot").unwrap().runs.is_empty()
            && state(&f.world(), "Pilot")
                .unwrap()
                .inventory
                .operation(2)
                .unwrap()
                .phase
                == CarrierOperationPhase::Completed
        {
            break;
        }
    }
    assert_eq!(
        state(&f.world(), "Pilot")
            .unwrap()
            .inventory
            .entry(2)
            .unwrap()
            .location,
        CarrierDroneLocation::Ready { berth_id: 0 }
    );
    let request = CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 3,
        drone_id: 2,
        action: CarrierInventoryAction::StoreReady { slot_id: 0 },
    };
    f.request(request.clone());
    let mut interrupted = BTreeSet::new();
    for tick in 0..220 {
        f.coarse_step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        let op = s.inventory.operation(3).unwrap();
        let x = op.stage.as_ref().unwrap();
        assert!(x.loaded_handoff_at_ms.is_none());
        if op.phase == CarrierOperationPhase::Completed {
            break;
        }
        assert_eq!(
            s.inventory
                .reservations
                .iter()
                .filter(|r| r.operation_id == 3)
                .count(),
            6
        );
        assert_eq!(
            s.inventory.entry(2).unwrap().location,
            CarrierDroneLocation::Loading { operation_id: 3 }
        );
        assert_eq!(
            s.runs[&3].kind.stage_segment(),
            Some(CarrierStageSegment::Full)
        );
        if s.inventory.cassettes.iter().any(|c| {
            matches!(
                c.location,
                CarrierCassetteLocation::InLoader {
                    operation_id: 3,
                    ..
                }
            )
        }) && interrupted.insert(x.completed_legs)
        {
            f.request(CarrierInventoryRequest::Hold {
                binding: f.binding(),
                operation_id: 3,
            });
            let held = state(&f.world(), "Pilot").unwrap().clone();
            f.reopen();
            f.coarse_step();
            let now = state(&f.world(), "Pilot").unwrap().clone();
            assert_eq!(held.machinery, now.machinery);
            assert_eq!(held.cassette_poses, now.cassette_poses);
            assert_eq!(held.drone_poses, now.drone_poses);
            assert_eq!(held.inventory.reservations, now.inventory.reservations);
            f.snapshot_valid();
            f.request(CarrierInventoryRequest::Resume {
                binding: f.binding(),
                operation_id: 3,
            });
        }
        if tick % 20 == 0 {
            f.snapshot_valid();
        }
    }
    f.snapshot_valid();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(interrupted, BTreeSet::from([0, 1, 2]));
    assert_eq!(
        s.inventory.entry(2).unwrap().location,
        CarrierDroneLocation::Protected { slot_id: 0 }
    );
    assert!(s.runs.is_empty() && s.inventory.reservations.is_empty());
    assert!(s
        .inventory
        .docked_cassette(CarrierCassetteDock::Ready { berth_id: 0 })
        .unwrap()
        .drone_id
        .is_none());
    let before = s.inventory.clone();
    f.request(request);
    assert_eq!(state(&f.world(), "Pilot").unwrap().inventory, before);
}

#[test]
fn actual_1703_one_recall_all_returns_twenty_with_local_storage_and_restart() {
    let mut f = parallel_tests::fixture();
    let original = state(&f.world(), "Pilot").unwrap().inventory.roster.clone();
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    for _ in 0..250 {
        f.coarse_step();
        if state(&f.world(), "Pilot")
            .unwrap()
            .inventory
            .active_deployment()
            .is_none()
        {
            break;
        }
    }
    assert!(state(&f.world(), "Pilot")
        .unwrap()
        .inventory
        .active_deployment()
        .is_none());
    assert_eq!(f.world().pilots["Pilot"].view.drones.len(), 20);
    let began = f.now;
    let request = CarrierInventoryRequest::RecallAll {
        binding: f.binding(),
        batch_id: 1,
    };
    f.request(request.clone());
    let mut restarted = false;
    let mut max_capture = 0;
    let mut max_storage = 0;
    for tick in 0..1800 {
        f.coarse_step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert_eq!(s.inventory.roster, original);
        assert_eq!(s.inventory.entries.len(), 20);
        assert_eq!(s.inventory.cassettes.len(), 20);
        assert!(s.inventory.entries.iter().all(|e| e.hit_points == 100));
        assert_eq!(s.ore.ore_units(), Some(0));
        max_capture = max_capture.max(
            s.arrivals.len()
                + s.runs
                    .values()
                    .filter(|r| {
                        matches!(
                            r.kind,
                            CarrierRouteKind::Bay {
                                action: CarrierInventoryAction::RecallToStorage,
                                ..
                            }
                        )
                    })
                    .count(),
        );
        max_storage = max_storage.max(
            s.runs
                .values()
                .filter(|r| {
                    matches!(
                        r.kind,
                        CarrierRouteKind::Stage {
                            direction: CarrierStageDirection::ToProtected,
                            ..
                        }
                    )
                })
                .count(),
        );
        if tick % 50 == 0 {
            eprintln!(
                "1703 Recall {}ms; free {} arrivals {} runs {} protected {}",
                f.now - began,
                w.pilots["Pilot"].view.drones.len(),
                s.arrivals.len(),
                s.runs.len(),
                s.inventory
                    .entries
                    .iter()
                    .filter(|e| matches!(e.location, CarrierDroneLocation::Protected { .. }))
                    .count()
            );
            f.snapshot_valid();
        }
        if !restarted
            && s.inventory.operations.iter().any(|op| {
                op.stage.as_ref().is_some_and(|x| {
                    x.direction == CarrierStageDirection::ToProtected && x.completed_legs == 1
                }) && !op.phase.terminal()
            })
        {
            f.request(CarrierInventoryRequest::HoldRecall {
                binding: f.binding(),
                batch_id: 1,
            });
            let held = state(&f.world(), "Pilot").unwrap().clone();
            f.reopen();
            f.coarse_step();
            let current = state(&f.world(), "Pilot").unwrap().clone();
            assert_eq!(held.machinery, current.machinery);
            assert_eq!(held.cassette_poses, current.cassette_poses);
            assert_eq!(held.drone_poses, current.drone_poses);
            assert_eq!(held.inventory.reservations, current.inventory.reservations);
            f.snapshot_valid();
            f.request(CarrierInventoryRequest::ResumeRecall {
                binding: f.binding(),
                batch_id: 1,
            });
            restarted = true;
        }
        if s.inventory.active_recall().is_none() {
            break;
        }
    }
    f.snapshot_valid();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(restarted && max_capture > 1 && max_storage > 1);
    assert_eq!(s.inventory.recalls[0].phase, CarrierRecallPhase::Completed);
    assert_eq!(
        s.inventory
            .entries
            .iter()
            .filter(|e| matches!(e.location, CarrierDroneLocation::Protected { .. }))
            .count(),
        10
    );
    assert_eq!(
        s.inventory
            .entries
            .iter()
            .filter(|e| matches!(e.location, CarrierDroneLocation::Ready { .. }))
            .count(),
        10
    );
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    assert!(
        s.runs.is_empty()
            && s.arrivals.is_empty()
            && s.departures.is_empty()
            && s.inventory.reservations.is_empty()
            && s.inventory.cleanups.is_empty()
    );
    assert_eq!(s.ore.lost_ore_units, 0);
    assert_eq!(w.pilots["Pilot"].view.ore, 0);
    assert_eq!(w.pilots["Pilot"].view.credits, STARTER_CREDITS);
    eprintln!("1703 full20 Recall completed after {}ms at1s cadence; max captures {max_capture}, max storage {max_storage}", f.now-began);
    let before = s.inventory.clone();
    f.request(request);
    assert_eq!(state(&f.world(), "Pilot").unwrap().inventory, before);
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    assert_eq!(state(&f.world(), "Pilot").unwrap().inventory, before);
}

#[test]
fn actual_1703_capture_death_releases_entry_or_retains_only_empty_hardware_cleanup() {
    for during_hardware in [false, true] {
        let mut f = parallel_tests::fixture();
        f.request(CarrierInventoryRequest::Begin {
            binding: f.binding(),
            operation_id: 1,
            drone_id: 1,
            action: CarrierInventoryAction::Launch,
        });
        for _ in 0..100 {
            f.coarse_step();
            let w = f.world();
            let s = state(&w, "Pilot").unwrap();
            if s.runs.is_empty() && s.departures.is_empty() {
                break;
            }
        }
        f.request(CarrierInventoryRequest::Begin {
            binding: f.binding(),
            operation_id: 2,
            drone_id: 1,
            action: CarrierInventoryAction::RecallToStorage,
        });
        if during_hardware {
            for _ in 0..100 {
                f.coarse_step();
                if state(&f.world(), "Pilot").unwrap().runs.contains_key(&2) {
                    break;
                }
            }
        }
        let mut w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert_eq!(s.runs.contains_key(&2), during_hardware);
        assert_eq!(s.arrivals.contains_key(&2), !during_hardware);
        // Explicit post-combat death injection, not a newly enabled damage source.
        w.pilots
            .get_mut("Pilot")
            .unwrap()
            .view
            .drones
            .iter_mut()
            .find(|d| d.id == 1)
            .unwrap()
            .hit_points = 0;
        let events = finish(&mut w, f.now).unwrap();
        assert_eq!(events.len(), 1);
        w.pilots
            .get_mut("Pilot")
            .unwrap()
            .view
            .drones
            .retain(|d| d.hit_points > 0);
        drones::sync(w.pilots.get_mut("Pilot").unwrap());
        validate(&w).unwrap();
        assert!(finish(&mut w, f.now).unwrap().is_empty());
        assert!(state(&w, "Pilot").unwrap().arrivals.is_empty());
        assert_eq!(
            state(&w, "Pilot").unwrap().runs.contains_key(&2),
            during_hardware
        );
        {
            let tx = f.server.db.transaction().unwrap();
            let mut entries = crate::load_entries(&tx, false).unwrap();
            for event in events {
                crate::append_event(
                    &tx,
                    &mut entries,
                    event,
                    f.now,
                    CARRIER_INVENTORY_RULESET_VERSION,
                )
                .unwrap();
            }
            tx.execute(
                "UPDATE world SET json=?1",
                [serde_json::to_string(&w).unwrap()],
            )
            .unwrap();
            tx.commit().unwrap();
        }
        f.reopen();
        f.snapshot_valid();
        if during_hardware {
            let held = state(&f.world(), "Pilot").unwrap().machinery.clone();
            f.coarse_step();
            assert_eq!(state(&f.world(), "Pilot").unwrap().machinery, held);
            f.request(CarrierInventoryRequest::Resume {
                binding: f.binding(),
                operation_id: 2,
            });
            for _ in 0..100 {
                f.coarse_step();
                f.snapshot_valid();
                if state(&f.world(), "Pilot").unwrap().runs.is_empty() {
                    break;
                }
            }
        }
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert!(matches!(
            s.inventory.entry(1).unwrap().location,
            CarrierDroneLocation::Destroyed { .. }
        ));
        assert_eq!(
            s.inventory.operation(2).unwrap().phase,
            CarrierOperationPhase::Destroyed
        );
        assert!(
            s.runs.is_empty()
                && s.inventory.cleanups.is_empty()
                && s.inventory.reservations.is_empty()
        );
        assert!(s.inventory.cassettes.iter().all(|c| c.drone_id != Some(1)));
        assert_eq!(s.inventory.entries.len(), 20);
        assert_eq!(s.inventory.cassettes.len(), 20);
        assert_eq!(s.ore.lost_ore_units, 0);
    }
}
