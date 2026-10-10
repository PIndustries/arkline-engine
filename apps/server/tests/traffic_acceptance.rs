use arkline_protocol::*;
use arkline_server::Server;
fn snapshot(server: &mut Server, now: u64) -> Snapshot {
    let r = server.handle_at(&mut None, Request::Snapshot, now);
    assert!(r.ok, "{}", r.message);
    r.snapshot.unwrap()
}
#[test]
fn traffic_is_deterministic_bounded_and_separate_from_players_and_economy() {
    let mut a = Server::open(":memory:").unwrap();
    let mut b = Server::open(":memory:").unwrap();
    let first = snapshot(&mut a, 1000);
    let independent = snapshot(&mut b, 1000);
    assert_eq!(first.traffic, independent.traffic);
    assert_eq!(first.traffic.len(), 9);
    assert!(first.pilots.is_empty());
    assert!(first.online_pilots.is_empty());
    let ids: std::collections::BTreeSet<_> = first.traffic.iter().map(|a| a.id).collect();
    assert_eq!(ids.len(), 9);
    for system in systems() {
        assert_eq!(
            first
                .traffic
                .iter()
                .filter(|a| a.system_id == system.id)
                .count(),
            3
        );
    }
    let later = snapshot(&mut a, 11_000);
    assert!(later
        .traffic
        .iter()
        .zip(&first.traffic)
        .any(|(a, b)| a.position != b.position));
    assert_eq!(later.ledger_hash, first.ledger_hash);
    assert_eq!(later.ledger_sequence, first.ledger_sequence);
    assert_eq!(later.asteroids, first.asteroids);
    assert_eq!(later.belt_ore, first.belt_ore);
    let export = a
        .handle_at(&mut None, Request::ExportLedger, 11_000)
        .ledger
        .unwrap();
    arkline_verifier::verify(&export, None).unwrap();
}
