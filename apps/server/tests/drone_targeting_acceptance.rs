use arkline_protocol::*;
use arkline_server::Server;

fn ok(s: &mut Server, session: &mut Option<String>, request: Request, now: u64) -> Snapshot {
    let response = s.handle_at(session, request, now);
    assert!(response.ok, "{}", response.message);
    response.snapshot.unwrap()
}
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt()
}
fn setup() -> (Server, Option<String>, u64, Snapshot) {
    let mut server = Server::open(":memory:").unwrap();
    let mut session = None;
    ok(
        &mut server,
        &mut session,
        Request::Connect {
            supports_room_controls: false,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Acceptance".into(),
            resume_token: None,
        },
        0,
    );
    ok(&mut server, &mut session, Request::Undock, 0);
    let snapshot = ok(
        &mut server,
        &mut session,
        Request::Warp {
            destination: Destination::Belt,
        },
        0,
    );
    let Location::InTransit { arrives_at_ms, .. } = snapshot.pilots[0].location else {
        panic!("warp did not begin")
    };
    let snapshot = ok(&mut server, &mut session, Request::Snapshot, arrives_at_ms);
    (server, session, arrives_at_ms, snapshot)
}
#[test]
fn pending_and_complete_locks_share_cap_and_targeted_unlock_isolates_miners() {
    let (mut server, mut session, now, initial) = setup();
    let position = initial.pilots[0].flight.as_ref().unwrap().position;
    let mut rocks: Vec<_> = initial
        .asteroids
        .iter()
        .filter(|a| {
            a.system_id == HOME_SYSTEM_ID && distance(position, a.position) - a.radius <= LOCK_RANGE
        })
        .collect();
    rocks.sort_by(|a, b| {
        (distance(position, a.position) - a.radius)
            .total_cmp(&(distance(position, b.position) - b.radius))
    });
    assert!(rocks.len() >= 6);
    assert!(distance(position, rocks[1].position) - rocks[1].radius <= MINING_RANGE);
    let ids: Vec<_> = rocks.iter().map(|a| a.id).collect();
    let mut pending = initial.clone();
    for id in &ids[..5] {
        pending = ok(
            &mut server,
            &mut session,
            Request::LockAsteroid { id: *id },
            now,
        );
    }
    assert_eq!(pending.pilots[0].target_locks.len(), 5);
    // Duplicate requests cannot refresh acquisition or consume a second slot.
    let duplicate = ok(
        &mut server,
        &mut session,
        Request::LockAsteroid { id: ids[0] },
        now,
    );
    assert_eq!(
        duplicate.pilots[0].target_locks,
        pending.pilots[0].target_locks
    );
    assert!(
        !server
            .handle_at(&mut session, Request::LockAsteroid { id: ids[5] }, now)
            .ok
    );
    let ready = pending.pilots[0]
        .target_locks
        .iter()
        .map(|l| l.completes_at_ms)
        .max()
        .unwrap();
    let locked = ok(&mut server, &mut session, Request::Snapshot, ready);
    assert_eq!(locked.pilots[0].locked_asteroids.len(), 5);
    assert!(
        !server
            .handle_at(&mut session, Request::LockAsteroid { id: ids[5] }, ready)
            .ok
    );
    for module_id in MINING_MODULE_IDS {
        ok(
            &mut server,
            &mut session,
            Request::StartMining {
                module_id,
                target_id: ids[module_id as usize],
            },
            ready,
        );
    }
    let spec = mining_spec(ShipClass::Pioneer);
    let mined = ok(
        &mut server,
        &mut session,
        Request::Snapshot,
        ready + spec.cycle_ms,
    );
    assert_eq!(mined.pilots[0].ore, 2 * spec.yield_units);
    for id in &ids[..2] {
        let before = initial.asteroids.iter().find(|a| a.id == *id).unwrap().ore;
        let after = mined.asteroids.iter().find(|a| a.id == *id).unwrap().ore;
        assert_eq!(
            before - after,
            spec.yield_units,
            "each module must deplete its own asteroid"
        );
    }
    let unlocked = ok(
        &mut server,
        &mut session,
        Request::UnlockAsteroid { id: ids[0] },
        ready + spec.cycle_ms,
    );
    assert!(unlocked.pilots[0].mining_modules[0].target_id.is_none());
    assert_eq!(unlocked.pilots[0].mining_modules[1].target_id, Some(ids[1]));
    let mixed = ok(
        &mut server,
        &mut session,
        Request::LockAsteroid { id: ids[5] },
        ready + spec.cycle_ms,
    );
    assert_eq!(
        mixed.pilots[0].locked_asteroids.len() + mixed.pilots[0].target_locks.len(),
        5
    );
    assert!(
        !server
            .handle_at(
                &mut session,
                Request::LockAsteroid { id: ids[0] },
                ready + spec.cycle_ms
            )
            .ok
    );
    let later = ok(
        &mut server,
        &mut session,
        Request::Snapshot,
        ready + 2 * spec.cycle_ms,
    );
    assert_eq!(later.pilots[0].ore, 3 * spec.yield_units);
    assert_eq!(
        later.asteroids.iter().find(|a| a.id == ids[0]).unwrap().ore,
        mined.asteroids.iter().find(|a| a.id == ids[0]).unwrap().ore
    );
    let export = server
        .handle_at(
            &mut session,
            Request::ExportLedger,
            ready + 2 * spec.cycle_ms,
        )
        .ledger
        .unwrap();
    arkline_verifier::verify(&export, None).unwrap();
}
#[test]
fn recon_drone_moves_in_world_returns_and_rejects_invalid_orders_without_economics() {
    let (mut server, mut session, now, initial) = setup();
    assert!(!server.handle_at(&mut session, Request::RecallDrone, now).ok);
    let launched = ok(&mut server, &mut session, Request::LaunchDrone, now);
    let start = launched.pilots[0].drone.as_ref().unwrap().position;
    assert!(!server.handle_at(&mut session, Request::LaunchDrone, now).ok);
    let ship = launched.pilots[0].flight.as_ref().unwrap().position;
    let target = [ship[0], ship[1] + 1500.0, ship[2]];
    ok(
        &mut server,
        &mut session,
        Request::DroneNavigate { position: target },
        now,
    );
    for position in [
        [f64::NAN, 0.0, 0.0],
        [f64::INFINITY, 0.0, 0.0],
        [ship[0], ship[1] + DRONE_MAX_RANGE + 1.0, ship[2]],
    ] {
        assert!(
            !server
                .handle_at(&mut session, Request::DroneNavigate { position }, now)
                .ok
        );
    }
    // An unauthenticated caller cannot take over the connected pilot's drone.
    assert!(!server.handle_at(&mut None, Request::DroneHold, now).ok);
    let mut state = launched;
    for step in 1..=10 {
        state = ok(
            &mut server,
            &mut session,
            Request::Snapshot,
            now + step * 100,
        );
    }
    let moved = state.pilots[0].drone.as_ref().unwrap();
    assert!(
        distance(start, moved.position) > 100.0,
        "drone must actually travel"
    );
    assert!(
        distance(start, moved.position) <= DRONE_SPEED + 0.01,
        "drone exceeded one-second speed bound"
    );
    assert!(distance(moved.position, target) < distance(start, target));
    let held = ok(&mut server, &mut session, Request::DroneHold, now + 1000).pilots[0]
        .drone
        .clone()
        .unwrap();
    let still = ok(&mut server, &mut session, Request::Snapshot, now + 1100).pilots[0]
        .drone
        .clone()
        .unwrap();
    assert_eq!(held.position, still.position);
    ok(&mut server, &mut session, Request::RecallDrone, now + 1100);
    for step in 12..=200 {
        state = ok(
            &mut server,
            &mut session,
            Request::Snapshot,
            now + step * 100,
        );
        if state.pilots[0].drone.is_none() {
            break;
        }
    }
    assert!(
        state.pilots[0].drone.is_none(),
        "recall must return drone to bay"
    );
    assert_eq!(state.ledger_hash, initial.ledger_hash);
    assert_eq!(state.pilots[0].ore, initial.pilots[0].ore);
    assert_eq!(state.pilots[0].credits, initial.pilots[0].credits);
    assert_eq!(state.asteroids, initial.asteroids);
}

#[test]
fn recon_returns_after_signal_loss_and_cannot_survive_departure() {
    let (mut server, mut session, mut now, initial) = setup();
    ok(&mut server, &mut session, Request::LaunchDrone, now);
    let position = initial.pilots[0].flight.as_ref().unwrap().position;
    ok(
        &mut server,
        &mut session,
        Request::Navigate {
            mode: NavigationMode::FlyTo {
                position: [position[0], position[1] + 30_000.0, position[2]],
            },
        },
        now,
    );
    let mut state;
    let deadline = now + 120_000;
    loop {
        now += 100;
        state = ok(&mut server, &mut session, Request::Snapshot, now);
        if state.pilots[0]
            .drone
            .as_ref()
            .is_some_and(|d| d.mode == DroneMode::Returning && d.signal_strength == 0.0)
        {
            break;
        }
        assert!(
            now < deadline,
            "out-of-range drone did not begin autonomous return"
        );
    }
    assert_eq!(state.ledger_hash, initial.ledger_hash);
    let departing = ok(
        &mut server,
        &mut session,
        Request::Warp {
            destination: Destination::Station,
        },
        now,
    );
    assert!(departing.pilots[0].drone.is_none());
    assert!(!server.handle_at(&mut session, Request::LaunchDrone, now).ok);
    assert_eq!(departing.ledger_hash, initial.ledger_hash);
}
