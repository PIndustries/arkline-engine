use arkline_protocol::*;
use arkline_server::Server;
struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "arkline-surface-{}-{}.sqlite",
            std::process::id(),
            rand::random::<u64>()
        )))
    }
    fn edit(&self, f: impl FnOnce(&mut serde_json::Value)) {
        let db = rusqlite::Connection::open(&self.0).unwrap();
        let raw: String = db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let mut w = serde_json::from_str(&raw).unwrap();
        f(&mut w);
        db.execute("UPDATE world SET json=?1 WHERE id=1", [w.to_string()])
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        for s in [".lock", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{s}", self.0.display()));
        }
    }
}
fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Response {
    let r = s.handle_at(p, r, t);
    assert!(r.ok, "{}", r.message);
    r
}
fn login(s: &mut Server, p: &mut Option<String>, name: &str) -> String {
    ok(
        s,
        p,
        Request::Connect {
            supports_room_controls: false,
            world_layout_version: 1,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: name.into(),
            resume_token: None,
        },
        0,
    )
    .resume_token
    .unwrap()
}
#[test]
fn continuous_descent_ground_contact_and_owned_props_persist_without_economy_changes() {
    let f = Fixture::new();
    let mut s = Server::open(&f.0).unwrap();
    let mut a = None;
    let token = login(&mut s, &mut a, "Builder");
    let mut b = None;
    login(&mut s, &mut b, "Visitor");
    for p in [&mut a, &mut b] {
        ok(&mut s, p, Request::Undock, 0);
    }
    f.edit(|w| {
        for name in ["Builder", "Visitor"] {
            w["pilots"][name]["view"]["flight"]["position"] =
                serde_json::json!([0.0, -119900.0, 0.0]);
        }
    });
    let baseline = ok(&mut s, &mut a, Request::Snapshot, 0).snapshot.unwrap();
    // Actual authoritative controls descend from100m, rather than a transition teleport.
    for t in (100..=3000).step_by(100) {
        ok(
            &mut s,
            &mut a,
            Request::PilotInput {
                translation: [0.0, -1.0, 0.0],
                rotation: [0.0; 3],
            },
            t,
        );
    }
    let state = ok(&mut s, &mut a, Request::Snapshot, 3100)
        .snapshot
        .unwrap();
    let pilot = state.pilots.iter().find(|p| p.name == "Builder").unwrap();
    let surface = pilot.surface.as_ref().unwrap();
    assert!(surface.grounded);
    assert!((surface.altitude_m - ShipClass::Pioneer.spec().height_m / 2.0).abs() < 0.01);
    assert!(pilot.flight.as_ref().unwrap().velocity[1] >= -0.001);
    let point = [50.0, -120000.0, 0.0];
    let spawned = ok(
        &mut s,
        &mut a,
        Request::SpawnSurfaceProp {
            kind: SurfacePropKind::Crate,
            position: point,
        },
        3100,
    )
    .snapshot
    .unwrap();
    let id = spawned.surface_props[0].id;
    assert!((planet_altitude(spawned.surface_props[0].position) - 4.0).abs() < 0.01);
    assert!(
        !s.handle_at(&mut b, Request::RemoveSurfaceProp { id }, 3100)
            .ok
    );
    assert!(
        !s.handle_at(
            &mut a,
            Request::MoveSurfaceProp {
                id,
                position: [f64::NAN, 0.0, 0.0]
            },
            3100
        )
        .ok
    );
    assert!(
        !s.handle_at(
            &mut a,
            Request::MoveSurfaceProp {
                id,
                position: [5000.0, -120000.0, 0.0]
            },
            3100
        )
        .ok
    );
    let changed = ok(
        &mut s,
        &mut a,
        Request::MoveSurfaceProp {
            id,
            position: [70.0, -120000.0, 0.0],
        },
        3100,
    )
    .snapshot
    .unwrap();
    assert_eq!(changed.ledger_hash, baseline.ledger_hash);
    assert_eq!(changed.belt_ore, baseline.belt_ore);
    drop(s);
    let mut s = Server::open(&f.0).unwrap();
    a = None;
    let recovered = ok(
        &mut s,
        &mut a,
        Request::Connect {
            supports_room_controls: false,
            world_layout_version: 1,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Builder".into(),
            resume_token: Some(token),
        },
        4000,
    )
    .snapshot
    .unwrap();
    assert_eq!(recovered.surface_props, changed.surface_props);
    let empty = ok(&mut s, &mut a, Request::RemoveSurfaceProp { id }, 4000)
        .snapshot
        .unwrap();
    assert!(empty.surface_props.is_empty());
    assert_eq!(empty.ledger_hash, baseline.ledger_hash);
}
#[test]
fn old_hangar_gains_new_hulls_once_and_larger_cargo_replays() {
    let f = Fixture::new();
    let mut s = Server::open(&f.0).unwrap();
    let mut p = None;
    let token = login(&mut s, &mut p, "Miner");
    drop(s);
    f.edit(|w| {
        let a = &mut w["accounts"]["Miner"];
        a.as_object_mut().unwrap().remove("hull_catalog_version");
        a["ships"].as_array_mut().unwrap().retain(|h| {
            !matches!(
                h["ship_class"].as_str(),
                Some("borealis" | "stratum" | "monolith")
            )
        });
    });
    for _ in 0..3 {
        let mut s = Server::open(&f.0).unwrap();
        p = None;
        let r = ok(
            &mut s,
            &mut p,
            Request::Connect {
                supports_room_controls: false,
                world_layout_version: 1,
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
                protocol_version: PROTOCOL_VERSION,
                pilot: "Miner".into(),
                resume_token: Some(token.clone()),
            },
            0,
        );
        let ships = r.assets.unwrap().ships;
        assert_eq!(ships.len(), 6);
        assert!(ships
            .iter()
            .all(|h| h.ore == 0 && h.system_id == HOME_SYSTEM_ID));
    }
    let mut s = Server::open(&f.0).unwrap();
    p = None;
    ok(
        &mut s,
        &mut p,
        Request::Connect {
            supports_room_controls: false,
            world_layout_version: 1,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Miner".into(),
            resume_token: Some(token),
        },
        0,
    );
    for class in MINING_SHIP_CLASSES {
        let r = ok(&mut s, &mut p, Request::SelectShip { ship_class: class }, 0);
        assert_eq!(r.snapshot.unwrap().pilots[0].ship_class, class);
    }
    let export = ok(&mut s, &mut p, Request::ExportLedger, 0);
    arkline_verifier::verify(&export.ledger.unwrap(), None).unwrap();
    assert!(MINING_SHIP_CLASSES
        .windows(2)
        .all(|pair| cargo_capacity(pair[0]) < cargo_capacity(pair[1])));
    assert!(MINING_SHIP_CLASSES.windows(2).all(|pair| {
        let a = mining_spec(pair[0]);
        let b = mining_spec(pair[1]);
        a.yield_units * b.cycle_ms < b.yield_units * a.cycle_ms
    }));
}
#[test]
fn prospector_fills_its_own_capacity_and_ledger_verifier_accepts_over_legacy_limit() {
    fill_prospector(false);
    fill_prospector(true);
}
fn fill_prospector(legacy: bool) {
    let f = Fixture::new();
    let mut s = Server::open(&f.0).unwrap();
    let mut p = None;
    login(&mut s, &mut p, "Extractor");
    if legacy {
        // Reproduce a historical non-ledger hull choice before station assets.
        f.edit(|w| {
            w.as_object_mut().unwrap().remove("accounts");
            let view = &mut w["pilots"]["Extractor"]["view"];
            view["ship_class"] = serde_json::json!("prospector");
            view.as_object_mut().unwrap().remove("power");
            view["mining_modules"] = serde_json::json!([]);
        });
    } else {
        ok(
            &mut s,
            &mut p,
            Request::SelectShip {
                ship_class: ShipClass::Prospector,
            },
            0,
        );
    }
    ok(&mut s, &mut p, Request::Undock, 0);
    let warp = ok(
        &mut s,
        &mut p,
        Request::Warp {
            destination: Destination::Belt,
        },
        0,
    )
    .snapshot
    .unwrap();
    let Location::InTransit { arrives_at_ms, .. } = warp.pilots[0].location else {
        panic!()
    };
    let mut t = arrives_at_ms;
    let mut snapshot = ok(&mut s, &mut p, Request::Snapshot, t).snapshot.unwrap();
    while snapshot.pilots[0].ore < cargo_capacity(ShipClass::Prospector) {
        let pos = snapshot.pilots[0].flight.as_ref().unwrap().position;
        let rock = snapshot
            .asteroids
            .iter()
            .filter(|a| a.system_id == HOME_SYSTEM_ID && a.ore > 0)
            .min_by(|a, b| {
                let d = |q: [f64; 3]| q.iter().zip(pos).map(|(x, y)| (x - y).powi(2)).sum::<f64>();
                d(a.position).total_cmp(&d(b.position))
            })
            .unwrap()
            .clone();
        let pending = ok(&mut s, &mut p, Request::LockAsteroid { id: rock.id }, t)
            .snapshot
            .unwrap();
        t = pending.pilots[0]
            .target_locks
            .iter()
            .find(|l| l.asteroid_id == rock.id)
            .unwrap()
            .completes_at_ms;
        ok(
            &mut s,
            &mut p,
            Request::StartMining {
                module_id: 0,
                target_id: rock.id,
            },
            t,
        );
        loop {
            t += mining_spec(ShipClass::Prospector).cycle_ms;
            s.tick_at(t).unwrap();
            snapshot = ok(&mut s, &mut p, Request::Snapshot, t).snapshot.unwrap();
            if snapshot.pilots[0].mining_modules[0].target_id.is_none() {
                break;
            }
        }
    }
    assert_eq!(snapshot.pilots[0].ore, 2500);
    assert_eq!(snapshot.belt_ore, INITIAL_BELT_ORE - 2500);
    assert!(snapshot.pilots[0].mining_modules[0]
        .stop_reason
        .as_ref()
        .unwrap()
        .contains("Cargo"));
    let export = ok(&mut s, &mut p, Request::ExportLedger, t).ledger.unwrap();
    arkline_verifier::verify(&export, None).unwrap();
}
#[test]
fn every_hull_launches_clear_of_station_and_can_dock_from_its_scaled_berth() {
    let f = Fixture::new();
    let mut s = Server::open(&f.0).unwrap();
    let mut p = None;
    login(&mut s, &mut p, "Dockmaster");
    for class in ALL_SHIP_CLASSES {
        ok(&mut s, &mut p, Request::SelectShip { ship_class: class }, 0);
        let snapshot = ok(&mut s, &mut p, Request::Undock, 0).snapshot.unwrap();
        let position = snapshot.pilots[0].flight.as_ref().unwrap().position;
        assert!(position[0] - hull_radius_m(class) >= STATION_POSITION[0] + 99.9);
        assert!(hull_surface_distance(class, position, STATION_POSITION) <= DOCK_RANGE);
        ok(&mut s, &mut p, Request::Dock, 0);
    }
}
#[test]
fn capital_surface_tools_measure_from_hull_and_prop_limit_is_authoritative() {
    let f = Fixture::new();
    let mut s = Server::open(&f.0).unwrap();
    let mut p = None;
    login(&mut s, &mut p, "CapitalBuilder");
    ok(
        &mut s,
        &mut p,
        Request::SelectShip {
            ship_class: ShipClass::Monolith,
        },
        0,
    );
    ok(&mut s, &mut p, Request::Undock, 0);
    f.edit(|w| {
        w["pilots"]["CapitalBuilder"]["view"]["flight"]["position"] =
            serde_json::json!([0.0, -119300.0, 0.0]);
    });
    for i in 0..32 {
        ok(
            &mut s,
            &mut p,
            Request::SpawnSurfaceProp {
                kind: SurfacePropKind::Pylon,
                position: [i as f64 * 20.0, -120000.0, 0.0],
            },
            0,
        );
    }
    assert!(
        !s.handle_at(
            &mut p,
            Request::SpawnSurfaceProp {
                kind: SurfacePropKind::Crate,
                position: [0.0, -120000.0, 0.0]
            },
            0
        )
        .ok
    );
    let state = ok(&mut s, &mut p, Request::Snapshot, 0).snapshot.unwrap();
    assert_eq!(state.surface_props.len(), 32);
    assert_eq!(state.pilots[0].ore, 0);
    assert_eq!(state.ledger_sequence, 2);
}

#[test]
fn station_is_above_atmosphere_and_planet_approach_is_five_kilometres_up() {
    assert!(planet_altitude(STATION_POSITION) > PLANET_ATMOSPHERE_HEIGHT_M);
    assert!(planet_altitude(STATION_CENTER_POSITION) > PLANET_ATMOSPHERE_HEIGHT_M);
    assert!((planet_altitude(PLANET_APPROACH_POSITION) - 5000.0).abs() < 0.001);
}
