use arkline_protocol::*;
use arkline_server::Server;
fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Response {
    let r = s.handle_at(p, r, t);
    assert!(r.ok, "{}", r.message);
    r
}
fn connect(s: &mut Server, p: &mut Option<String>, token: Option<String>, t: u64) -> Response {
    ok(
        s,
        p,
        Request::Connect {
            supports_room_controls: true,
            world_layout_version: 1,
            supports_fleet_hangar: true,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Engineer".into(),
            resume_token: token,
        },
        t,
    )
}
fn room(s: &mut Server, p: &mut Option<String>, t: u64) -> PioneerRoomState {
    ok(s, p, Request::Snapshot, t)
        .assets
        .unwrap()
        .pioneer_room
        .unwrap()
}
fn command(
    s: &mut Server,
    p: &mut Option<String>,
    a: PioneerRoomCommand,
    t: u64,
) -> PioneerRoomState {
    ok(s, p, Request::SetPioneerRoom { action: a }, t)
        .assets
        .unwrap()
        .pioneer_room
        .unwrap()
}
fn path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("arkline-room-{}.sqlite", rand::random::<u64>()))
}
#[test]
fn breaker_emergency_manual_door_restore_and_pressure_interlock() {
    let mut s = Server::open(path()).unwrap();
    let mut p = None;
    connect(&mut s, &mut p, None, 0);
    assert!(!room(&mut s, &mut p, 0).commissioned);
    let r = command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Breaker { closed: true },
        0,
    );
    assert!(r.light_on && !r.emergency_light_on);
    command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Door {
            open: false,
            manual: false,
        },
        0,
    );
    command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Vent {
            mode: VentMode::Exhaust,
        },
        0,
    );
    let r = room(&mut s, &mut p, 10_000);
    assert_eq!(r.pressure_pa, 81_000);
    assert!(r.vent_running);
    let r = command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Door {
            open: true,
            manual: true,
        },
        10_000,
    );
    assert!(!r.door_open && r.door_commanded_open && r.interlock.unwrap().contains("Pressure"));
    let r = command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Breaker { closed: false },
        10_000,
    );
    assert!(!r.light_on && !r.vent_running && r.emergency_light_on);
    let r = room(&mut s, &mut p, 11_000);
    assert_eq!(r.emergency_remaining_ms, ROOM_EMERGENCY_MS - 1000);
    assert_eq!(r.pressure_pa, 81_000);
    command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Vent {
            mode: VentMode::Equalize,
        },
        11_000,
    );
    let r = command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Breaker { closed: true },
        11_000,
    );
    assert!(r.light_on && r.vent_running && !r.emergency_light_on);
    let r = room(&mut s, &mut p, 21_000);
    assert_eq!(r.pressure_pa, ROOM_PRESSURE_PA);
    assert!(r.door_open);
    assert_eq!(r.emergency_remaining_ms, ROOM_EMERGENCY_MS);
    command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Breaker { closed: false },
        21_000,
    );
    let r = command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Door {
            open: false,
            manual: true,
        },
        21_000,
    );
    assert!(!r.door_open);
    let r = command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Door {
            open: true,
            manual: false,
        },
        21_000,
    );
    assert!(!r.door_open);
    assert!(
        command(
            &mut s,
            &mut p,
            PioneerRoomCommand::Door {
                open: true,
                manual: true
            },
            21_000
        )
        .door_open
    );
    assert!(!room(&mut s, &mut p, 21_000 + ROOM_EMERGENCY_MS).emergency_light_on);
}
#[test]
fn authoritative_door_collision_access_and_non_remote_commands() {
    let mut s = Server::open(path()).unwrap();
    let mut p = None;
    connect(&mut s, &mut p, None, 0);
    let request = Request::SetPioneerRoom {
        action: PioneerRoomCommand::Breaker { closed: true },
    };
    assert!(!request.is_remote_gameplay());
    assert!(!s.handle_at(&mut None, request.clone(), 0).ok);
    command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Breaker { closed: true },
        0,
    );
    ok(&mut s, &mut p, Request::DisembarkShip, 0);
    assert!(!s.handle_at(&mut p, request.clone(), 0).ok);
    ok(&mut s, &mut p, Request::EnterInterior, 0);
    command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Door {
            open: false,
            manual: false,
        },
        0,
    );
    // Entry faces +Z; walk into the closed physical doorway for two seconds.
    for i in 0..4 {
        ok(
            &mut s,
            &mut p,
            Request::StationWalk {
                forward: 1.,
                strafe: 0.,
                turn: 0.,
            },
            i * 500,
        );
        s.tick_at((i + 1) * 500).unwrap();
    }
    let r = ok(&mut s, &mut p, Request::Snapshot, 2000);
    let a = r.snapshot.unwrap().pilots[0]
        .station_avatar
        .clone()
        .unwrap();
    assert!(a.position[2] <= PIONEER_CONNECTED_ROOM.door_min[2] - STATION_AVATAR_RADIUS_M + 1e-8);
    command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Door {
            open: true,
            manual: false,
        },
        2000,
    );
    ok(
        &mut s,
        &mut p,
        Request::StationWalk {
            forward: 1.,
            strafe: 0.,
            turn: 0.,
        },
        2000,
    );
    s.tick_at(2100).unwrap();
    assert!(
        !s.handle_at(
            &mut p,
            Request::SetPioneerRoom {
                action: PioneerRoomCommand::Door {
                    open: false,
                    manual: false
                }
            },
            2100
        )
        .ok
    );
    assert!(room(&mut s, &mut p, 2100).door_open);
}
#[test]
fn persisted_per_vessel_and_load_reduces_capacitor_regeneration() {
    let path = path();
    let mut s = Server::open(&path).unwrap();
    let mut p = None;
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + 86_400_000;
    let token = connect(&mut s, &mut p, None, t).resume_token;
    command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Breaker { closed: true },
        t,
    );
    command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Door {
            open: false,
            manual: false,
        },
        t,
    );
    command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Vent {
            mode: VentMode::Equalize,
        },
        t,
    );
    ok(&mut s, &mut p, Request::Undock, t);
    let r = ok(
        &mut s,
        &mut p,
        Request::Warp {
            destination: Destination::Belt,
        },
        t,
    );
    let initial = r.snapshot.unwrap().pilots[0]
        .power
        .clone()
        .unwrap()
        .charge_j;
    let r = ok(&mut s, &mut p, Request::Snapshot, t + 1000);
    assert_eq!(
        r.snapshot.unwrap().pilots[0]
            .power
            .as_ref()
            .unwrap()
            .charge_j,
        initial + power_spec(ShipClass::Pioneer).regen_w
            - default_pioneer_exterior().load_w()
            - ROOM_LIGHT_W
            - ROOM_VENT_W
            - ROOM_CONTROLLER_W
    );
    let before = room(&mut s, &mut p, t + 1000);
    drop(s);
    let mut s = Server::open(&path).unwrap();
    let mut p = None;
    connect(&mut s, &mut p, token, t + 1000);
    assert_eq!(room(&mut s, &mut p, t + 1000), before);
}

#[test]
fn equipment_stays_with_pioneer_and_other_pilots_cannot_address_it() {
    let mut s = Server::open(path()).unwrap();
    let mut p = None;
    let token = connect(&mut s, &mut p, None, 0).resume_token;
    let before = command(&mut s, &mut p, PioneerRoomCommand::Light { on: false }, 0);
    let mut other = None;
    ok(
        &mut s,
        &mut other,
        Request::Connect {
            supports_room_controls: true,
            world_layout_version: 1,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Other".into(),
            resume_token: None,
        },
        0,
    );
    command(
        &mut s,
        &mut other,
        PioneerRoomCommand::Breaker { closed: true },
        0,
    );
    assert_eq!(room(&mut s, &mut p, 0), before);
    // Legacy selection is used only to exercise custody without walking a long berth.
    ok(
        &mut s,
        &mut p,
        Request::Connect {
            supports_room_controls: true,
            world_layout_version: 1,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Engineer".into(),
            resume_token: token,
        },
        0,
    );
    ok(
        &mut s,
        &mut p,
        Request::SelectShip {
            ship_class: ShipClass::Prospector,
        },
        0,
    );
    assert!(
        !s.handle_at(
            &mut p,
            Request::SetPioneerRoom {
                action: PioneerRoomCommand::Breaker { closed: true }
            },
            0
        )
        .ok
    );
    assert!(ok(&mut s, &mut p, Request::Snapshot, 0)
        .assets
        .unwrap()
        .pioneer_room
        .is_none());
    ok(
        &mut s,
        &mut p,
        Request::SelectShip {
            ship_class: ShipClass::Pioneer,
        },
        0,
    );
    assert_eq!(room(&mut s, &mut p, 0), before);
}

#[test]
fn legacy_recovery_requires_auth_and_mixed_sessions_cannot_reclose_invisible_door() {
    let mut s = Server::open(path()).unwrap();
    let mut p = None;
    let token = connect(&mut s, &mut p, None, 0).resume_token;
    command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Breaker { closed: true },
        0,
    );
    command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Door {
            open: false,
            manual: false,
        },
        0,
    );
    command(
        &mut s,
        &mut p,
        PioneerRoomCommand::Vent {
            mode: VentMode::Exhaust,
        },
        0,
    );
    let before = room(&mut s, &mut p, 10000);
    assert_eq!(before.pressure_pa, 81000);
    let legacy = |token| Request::Connect {
        supports_room_controls: false,
        world_layout_version: 1,
        supports_fleet_hangar: true,
        supports_pioneer_interior: true,
        protocol_version: PROTOCOL_VERSION,
        pilot: "Engineer".into(),
        resume_token: token,
    };
    let mut old = None;
    assert!(
        !s.handle_at(&mut old, legacy(Some("wrong".into())), 10000)
            .ok
    );
    assert_eq!(room(&mut s, &mut p, 10000), before);
    let r = ok(&mut s, &mut old, legacy(token.clone()), 10000)
        .assets
        .unwrap()
        .pioneer_room
        .unwrap();
    assert!(r.door_open);
    assert_eq!(r.pressure_pa, ROOM_PRESSURE_PA);
    assert_eq!(r.vent_commanded, VentMode::Hold);
    connect(&mut s, &mut p, token.clone(), 10000);
    let close = Request::SetPioneerRoom {
        action: PioneerRoomCommand::Door {
            open: false,
            manual: true,
        },
    };
    assert!(!s.handle_at(&mut p, close.clone(), 10000).ok);
    s.disconnect(&mut old);
    connect(&mut s, &mut p, token.clone(), 10000);
    assert!(!s.handle_at(&mut p, close.clone(), 10000).ok);
    s.disconnect(&mut p);
    connect(&mut s, &mut p, token, 10000);
    assert!(
        !ok(&mut s, &mut p, close, 10000)
            .assets
            .unwrap()
            .pioneer_room
            .unwrap()
            .door_open
    );
}
