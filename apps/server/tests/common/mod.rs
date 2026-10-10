use arkline_protocol::*;
use arkline_server::Server;
/// Exercise actual fixed-yield cycles, retaining the authenticated connection.
pub fn mine_fixed_cycles(
    s: &mut Server,
    p: &mut Option<String>,
    cycles: u64,
    t: &mut u64,
) -> Snapshot {
    let snapshot = s.handle_at(p, Request::Snapshot, *t).snapshot.unwrap();
    let pilot = snapshot
        .pilots
        .iter()
        .find(|v| Some(&v.name) == p.as_ref())
        .unwrap();
    let target_id = pilot.locked_asteroid.expect("confirmed target required");
    let duration = mining_spec(pilot.ship_class).cycle_ms;
    let r = s.handle_at(
        p,
        Request::StartMining {
            module_id: 0,
            target_id,
        },
        *t,
    );
    assert!(r.ok, "{}", r.message);
    for _ in 0..cycles {
        *t += duration;
        s.tick_at(*t).unwrap();
    }
    let r = s.handle_at(p, Request::StopMining { module_id: 0 }, *t);
    assert!(r.ok, "{}", r.message);
    r.snapshot.unwrap()
}
