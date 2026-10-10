use arkline_protocol::*;
use arkline_server::Server;
fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Snapshot {
    let r = s.handle_at(p, r, t);
    assert!(r.ok, "{}", r.message);
    r.snapshot.unwrap()
}
fn connect(s: &mut Server, name: &str) -> Option<String> {
    let mut p = None;
    ok(
        s,
        &mut p,
        Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            pilot: name.into(),
            resume_token: None,
        },
        0,
    );
    ok(s, &mut p, Request::Undock, 0);
    p
}
fn pilot<'a>(s: &'a Snapshot, name: &str) -> &'a PilotView {
    s.pilots.iter().find(|p| p.name == name).unwrap()
}
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt()
}
#[test]
fn full_fleet_caps_ids_and_control_ownership_are_server_enforced() {
    let mut s = Server::open(":memory:").unwrap();
    let mut a = connect(&mut s, "Alpha");
    let mut b = connect(&mut s, "Beta");
    let initial = ok(&mut s, &mut a, Request::Snapshot, 0);
    for class in [DroneClass::Recon, DroneClass::Mining, DroneClass::Attack] {
        for _ in 0..DRONE_CLASS_CAP {
            ok(&mut s, &mut a, Request::LaunchDroneClass { class }, 0);
        }
        assert!(
            !s.handle_at(&mut a, Request::LaunchDroneClass { class }, 0)
                .ok
        );
    }
    let state = ok(&mut s, &mut a, Request::Snapshot, 0);
    let fleet = &pilot(&state, "Alpha").drones;
    assert_eq!(fleet.len(), MAX_DRONES);
    let ids: std::collections::BTreeSet<_> = fleet.iter().map(|d| d.id).collect();
    assert_eq!(ids.len(), MAX_DRONES);
    let foreign = fleet[0].id;
    for request in [
        Request::DroneHoldById { id: foreign },
        Request::RecallDroneById { id: foreign },
        Request::DroneNavigateById {
            id: foreign,
            position: pilot(&state, "Beta").flight.as_ref().unwrap().position,
        },
    ] {
        assert!(!s.handle_at(&mut b, request, 0).ok);
    }
    let other = ok(
        &mut s,
        &mut b,
        Request::LaunchDroneClass {
            class: DroneClass::Recon,
        },
        0,
    );
    assert!(!ids.contains(&pilot(&other, "Beta").drones[0].id));
    assert_eq!(other.ledger_hash, initial.ledger_hash);
    assert_eq!(pilot(&other, "Alpha").ore, 0);
}
#[test]
fn mining_drone_approaches_then_repeats_real_economic_cycles() {
    let mut s = Server::open(":memory:").unwrap();
    let mut p = connect(&mut s, "Miner");
    let state = ok(
        &mut s,
        &mut p,
        Request::Warp {
            destination: Destination::Belt,
        },
        0,
    );
    let Location::InTransit { arrives_at_ms, .. } = pilot(&state, "Miner").location else {
        panic!()
    };
    let mut t = arrives_at_ms;
    let state = ok(&mut s, &mut p, Request::Snapshot, t);
    let pos = pilot(&state, "Miner").flight.as_ref().unwrap().position;
    let rock = state
        .asteroids
        .iter()
        .filter(|a| {
            a.system_id == HOME_SYSTEM_ID
                && distance(pos, a.position) - a.radius > 1000.0
                && distance(pos, a.position) - a.radius < LOCK_RANGE
        })
        .min_by(|a, b| {
            (distance(pos, a.position) - a.radius)
                .total_cmp(&(distance(pos, b.position) - b.radius))
        })
        .unwrap()
        .clone();
    let state = ok(
        &mut s,
        &mut p,
        Request::LaunchDroneClass {
            class: DroneClass::Mining,
        },
        t,
    );
    let id = pilot(&state, "Miner").drones[0].id;
    assert!(
        !s.handle_at(
            &mut p,
            Request::AssignMiningDrone {
                id,
                target_id: rock.id
            },
            t
        )
        .ok
    );
    let state = ok(&mut s, &mut p, Request::LockAsteroid { id: rock.id }, t);
    t = pilot(&state, "Miner").target_locks[0].completes_at_ms;
    let state = ok(
        &mut s,
        &mut p,
        Request::AssignMiningDrone {
            id,
            target_id: rock.id,
        },
        t,
    );
    let start = pilot(&state, "Miner").drones[0].position;
    let deadline = t + 120_000;
    let mut state = state;
    while pilot(&state, "Miner").ore < 2 * DRONE_MINING_YIELD {
        t += 100;
        state = ok(&mut s, &mut p, Request::Snapshot, t);
        assert!(t < deadline, "mining drone did not complete two cycles");
    }
    let owner = pilot(&state, "Miner");
    assert_eq!(owner.ore, 2 * DRONE_MINING_YIELD);
    assert!(distance(owner.drones[0].position, start) > 0.0);
    assert_eq!(
        rock.ore
            - state
                .asteroids
                .iter()
                .find(|a| a.id == rock.id)
                .unwrap()
                .ore,
        2 * DRONE_MINING_YIELD
    );
    assert_eq!(owner.credits, 1000);
    assert!(owner.mining_modules.iter().all(|m| m.target_id.is_none()));
    ok(&mut s, &mut p, Request::DroneHoldById { id }, t);
    let later = ok(&mut s, &mut p, Request::Snapshot, t + DRONE_MINING_CYCLE_MS);
    assert_eq!(pilot(&later, "Miner").ore, 2 * DRONE_MINING_YIELD);
    arkline_verifier::verify(
        &s.handle_at(&mut p, Request::ExportLedger, t + DRONE_MINING_CYCLE_MS)
            .ledger
            .unwrap(),
        None,
    )
    .unwrap();
}
#[test]
fn attack_drone_damages_only_assigned_foreign_drone_and_destroys_it() {
    let mut s = Server::open(":memory:").unwrap();
    let mut a = connect(&mut s, "Alpha");
    let mut b = connect(&mut s, "Beta");
    let state = ok(
        &mut s,
        &mut a,
        Request::LaunchDroneClass {
            class: DroneClass::Attack,
        },
        0,
    );
    let attacker = pilot(&state, "Alpha").drones[0].id;
    let state = ok(
        &mut s,
        &mut b,
        Request::LaunchDroneClass {
            class: DroneClass::Recon,
        },
        0,
    );
    let victim = pilot(&state, "Beta").drones[0].id;
    let state = ok(
        &mut s,
        &mut b,
        Request::LaunchDroneClass {
            class: DroneClass::Recon,
        },
        0,
    );
    let innocent = pilot(&state, "Beta").drones[1].id;
    let head = state.ledger_hash.clone();
    assert!(
        !s.handle_at(
            &mut a,
            Request::AssignAttackDrone {
                id: attacker,
                target_pilot: "Alpha".into(),
                target_drone_id: attacker
            },
            0
        )
        .ok
    );
    assert!(
        !s.handle_at(
            &mut b,
            Request::AssignAttackDrone {
                id: victim,
                target_pilot: "Alpha".into(),
                target_drone_id: attacker
            },
            0
        )
        .ok
    );
    ok(
        &mut s,
        &mut a,
        Request::AssignAttackDrone {
            id: attacker,
            target_pilot: "Beta".into(),
            target_drone_id: victim,
        },
        0,
    );
    let mut damaged = false;
    let mut state = state;
    for step in 1..=600 {
        state = ok(&mut s, &mut a, Request::Snapshot, step * 100);
        let fleet = &pilot(&state, "Beta").drones;
        assert_eq!(
            fleet.iter().find(|d| d.id == innocent).unwrap().hit_points,
            DRONE_HIT_POINTS
        );
        match fleet.iter().find(|d| d.id == victim) {
            Some(d) => damaged |= d.hit_points < DRONE_HIT_POINTS,
            None => break,
        }
    }
    assert!(damaged);
    assert!(!pilot(&state, "Beta").drones.iter().any(|d| d.id == victim));
    assert_eq!(state.ledger_hash, head);
    assert_eq!(pilot(&state, "Alpha").credits, 1000);
    assert_eq!(pilot(&state, "Beta").credits, 1000);
}

#[test]
fn signal_loss_forces_return_and_denies_control_until_recovered() {
    let mut s = Server::open(":memory:").unwrap();
    let mut p = connect(&mut s, "Owner");
    let initial = ok(
        &mut s,
        &mut p,
        Request::LaunchDroneClass {
            class: DroneClass::Recon,
        },
        0,
    );
    let owner = pilot(&initial, "Owner");
    let id = owner.drones[0].id;
    let pos = owner.flight.as_ref().unwrap().position;
    assert!(owner.drones[0].signal_strength > 0.9);
    ok(
        &mut s,
        &mut p,
        Request::Navigate {
            mode: NavigationMode::FlyTo {
                position: [pos[0], pos[1] + 20_000.0, pos[2]],
            },
        },
        0,
    );
    let mut now = 0;
    let mut state;
    loop {
        now += 100;
        state = ok(&mut s, &mut p, Request::Snapshot, now);
        let d = &pilot(&state, "Owner").drones[0];
        assert!(
            (d.signal_strength
                - drone_signal_strength(
                    d.position,
                    pilot(&state, "Owner").flight.as_ref().unwrap().position
                ))
            .abs()
                < 1e-9
        );
        if d.signal_strength == 0.0 {
            assert!(matches!(d.mode, DroneMode::Returning));
            break;
        }
        assert!(now < 90_000);
    }
    let before = pilot(&state, "Owner").drones[0].position;
    assert!(!s.handle_at(&mut p, Request::DroneHoldById { id }, now).ok);
    assert!(
        !s.handle_at(
            &mut p,
            Request::DroneNavigateById { id, position: pos },
            now
        )
        .ok
    );
    ok(
        &mut s,
        &mut p,
        Request::Navigate {
            mode: NavigationMode::Stop,
        },
        now,
    );
    // Recalling remains allowed with no signal; the autonomous drone travels back.
    ok(&mut s, &mut p, Request::RecallDroneById { id }, now);
    let deadline = now + 180_000;
    let mut moved = false;
    loop {
        now += 100;
        state = ok(&mut s, &mut p, Request::Snapshot, now);
        match pilot(&state, "Owner").drones.first() {
            None => break,
            Some(d) => moved |= distance(before, d.position) > 100.0,
        }
        assert!(now < deadline);
    }
    assert!(moved);
    assert_eq!(state.ledger_hash, initial.ledger_hash);
}
