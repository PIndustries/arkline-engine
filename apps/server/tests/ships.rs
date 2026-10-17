use arkline_protocol::*;
use arkline_server::Server;

#[test]
fn docked_hull_selection_persists_without_changing_economy_and_rejects_space() {
    let path = std::env::temp_dir().join(format!(
        "arkline-ships-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    let token;
    let selected;
    {
        let mut server = Server::open(&path).unwrap();
        let mut session = None;
        let response = server.handle_at(
            &mut session,
            Request::Connect {
                supports_room_controls: false,
                world_layout_version: 1,
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
                protocol_version: PROTOCOL_VERSION,
                pilot: "Shipwright".into(),
                resume_token: None,
            },
            0,
        );
        assert!(response.ok);
        token = response.resume_token.unwrap();
        let initial = response.snapshot.unwrap();
        assert_eq!(initial.pilots[0].ship_class, ShipClass::Pioneer);
        for (index, ship_class) in ALL_SHIP_CLASSES.into_iter().enumerate() {
            let response = server.handle_at(&mut session, Request::SelectShip { ship_class }, 1);
            assert!(response.ok, "{}", response.message);
            let snapshot = response.snapshot.unwrap();
            assert_eq!(snapshot.pilots[0].ship_class, ship_class);
            assert_eq!(snapshot.pilots[0].credits, initial.pilots[0].credits);
            assert_eq!(snapshot.pilots[0].ore, initial.pilots[0].ore);
            assert_eq!(
                snapshot.ledger_sequence,
                initial.ledger_sequence + index as u64
            );
            assert_eq!(snapshot.stored_ore, 0);
            assert_eq!(
                response.assets.unwrap().ships.len(),
                ALL_SHIP_CLASSES.len() - 1
            );
        }
        selected = server
            .handle_at(&mut session, Request::Undock, 2)
            .snapshot
            .unwrap();
        let rejected = server.handle_at(
            &mut session,
            Request::SelectShip {
                ship_class: ShipClass::Pioneer,
            },
            3,
        );
        assert!(!rejected.ok);
        assert_eq!(
            server
                .handle_at(&mut session, Request::Snapshot, 3)
                .snapshot
                .unwrap()
                .pilots[0]
                .ship_class,
            ShipClass::Monolith
        );
    }
    {
        let mut server = Server::open(&path).unwrap();
        let response = server.handle_at(
            &mut None,
            Request::Connect {
                supports_room_controls: false,
                world_layout_version: 1,
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
                protocol_version: PROTOCOL_VERSION,
                pilot: "Shipwright".into(),
                resume_token: Some(token),
            },
            4,
        );
        assert!(response.ok);
        let snapshot = response.snapshot.unwrap();
        assert_eq!(snapshot.pilots[0].ship_class, ShipClass::Monolith);
        assert_eq!(snapshot.pilots[0].location, Location::Station);
        assert_eq!(snapshot.pilots[0].credits, selected.pilots[0].credits);
        assert_eq!(snapshot.ledger_hash, selected.ledger_hash);
    }
    std::fs::remove_file(&path).unwrap();
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    std::fs::remove_file(lock).unwrap();
}

#[test]
fn legacy_pilot_defaults_to_pioneer_and_dimensions_are_meters() {
    let pilot: PilotView = serde_json::from_value(
        serde_json::json!({"name":"Legacy","credits":1000,"ore":4,"location":{"kind":"docked"}}),
    )
    .unwrap();
    assert_eq!(pilot.ship_class, ShipClass::Pioneer);
    assert_eq!(
        ALL_SHIP_CLASSES.map(|s| s.spec().length_m),
        [72.0, 180.0, 420.0, 1100.0, 640.0, 1800.0, 5200.0]
    );
    for class in ALL_SHIP_CLASSES {
        let spec = class.spec();
        assert!(
            spec.length_m > spec.width_m && spec.width_m > spec.height_m && spec.height_m > 0.0
        );
    }
    assert_eq!(
        STATION_CENTER_POSITION[0] + STATION_DIAMETER_M / 2.0,
        STATION_POSITION[0]
    );
}
