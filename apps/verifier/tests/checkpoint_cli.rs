use arkline_protocol::*;
use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn checkpoint_advances_only_after_full_verification() {
    let directory = std::env::temp_dir().join(format!(
        "arkline-verifier-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    let input = directory.join("export.json");
    let pin = directory.join("pin.json");
    let saved = directory.join("saved.json");
    let mut export = LedgerExport {
        protocol_version: PROTOCOL_VERSION,
        entries: vec![],
        snapshot: Snapshot {
            world_layout: None,
            exterior: None,
            remote_controls: vec![],
            fleet_links: vec![],
            resource_jobs: vec![],
            surface_props: vec![],
            traffic: vec![],
            stored_ore: 0,
            asteroids: vec![],
            online_pilots: vec![],
            server_time_ms: 0,
            pilots: vec![],
            orders: vec![],
            belt_ore: INITIAL_BELT_ORE,
            ledger_sequence: 0,
            ledger_hash: GENESIS_HASH.into(),
        },
    };
    fs::write(&input, serde_json::to_vec(&export).unwrap()).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_arkline-verifier"))
            .arg("--file")
            .arg(&input)
            .arg("--checkpoint")
            .arg(&pin)
            .arg("--save")
            .arg(&saved)
            .output()
            .unwrap()
    };
    let first = run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let trusted = fs::read(&pin).unwrap();
    let verified_export = fs::read(&saved).unwrap();
    export.snapshot.belt_ore -= 1;
    fs::write(&input, serde_json::to_vec(&export).unwrap()).unwrap();
    assert!(!run().status.success());
    assert_eq!(fs::read(&pin).unwrap(), trusted);
    assert_eq!(fs::read(&saved).unwrap(), verified_export);
    let mut entry = LedgerEntry {
        sequence: 1,
        ruleset_version: RULESET_VERSION,
        timestamp_ms: 1,
        previous_hash: GENESIS_HASH.into(),
        event: EconomicEvent::PilotCreated {
            pilot: "Alice".into(),
        },
        hash: String::new(),
    };
    entry.hash = entry.calculate_hash();
    export.snapshot.belt_ore = INITIAL_BELT_ORE;
    export.snapshot.pilots.push(PilotView {
        station_avatar: None,
        surface: None,
        power: None,
        drones: vec![],
        target_locks: vec![],
        locked_asteroids: vec![],
        drone: None,
        mining_modules: Vec::new(),
        target_lock: None,
        ship_class: arkline_protocol::ShipClass::default(),
        system_id: HOME_SYSTEM_ID,
        locked_asteroid: None,
        name: "Alice".into(),
        credits: STARTER_CREDITS,
        ore: 0,
        location: Location::Docked,
        flight: None,
    });
    export.snapshot.ledger_sequence = 1;
    export.snapshot.ledger_hash = entry.hash.clone();
    export.entries.push(entry);
    fs::write(&input, serde_json::to_vec(&export).unwrap()).unwrap();
    assert!(run().status.success());
    let advanced = fs::read(&pin).unwrap();
    assert_ne!(advanced, trusted);
    fs::write(&input, verified_export).unwrap();
    let rollback = run();
    assert!(!rollback.status.success());
    assert!(String::from_utf8_lossy(&rollback.stderr).contains("rollback"));
    assert_eq!(fs::read(&pin).unwrap(), advanced);
    fs::remove_dir_all(directory).unwrap();
}
