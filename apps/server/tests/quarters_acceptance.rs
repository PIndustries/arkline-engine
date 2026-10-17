use arkline_protocol::*;
use arkline_server::Server;

fn command(s: &mut Server, pilot: &mut Option<String>, request: Request, now: u64) -> Response {
    let r = s.handle_at(pilot, request, now);
    assert!(r.ok, "{}", r.message);
    r
}
fn current(s: &mut Server, now: u64) -> PilotView {
    command(s, &mut None, Request::Snapshot, now)
        .snapshot
        .unwrap()
        .pilots
        .remove(0)
}
fn walk_to(s: &mut Server, pilot: &mut Option<String>, now: &mut u64, target: [f64; 3]) {
    for _ in 0..300 {
        let avatar = current(s, *now).station_avatar.unwrap();
        let dx = target[0] - avatar.position[0];
        let dz = target[2] - avatar.position[2];
        if dx.hypot(dz) < 0.04 {
            command(
                s,
                pilot,
                Request::StationWalk {
                    forward: 0.,
                    strafe: 0.,
                    turn: 0.,
                },
                *now,
            );
            return;
        }
        let (sin, cos) = avatar.heading.sin_cos();
        // Convert world displacement to current avatar-local walking axes.
        command(
            s,
            pilot,
            Request::StationWalk {
                forward: ((dx * cos + dz * sin) / (WALK_SPEED_MPS * 0.1)).clamp(-1., 1.),
                strafe: ((-dx * sin + dz * cos) / (WALK_SPEED_MPS * 0.1)).clamp(-1., 1.),
                turn: 0.,
            },
            *now,
        );
        *now += 100;
        s.tick_at(*now).unwrap();
    }
    panic!("walk route blocked before {target:?}");
}
#[test]
fn walk_quarters_and_return_to_board_without_moving_ship_or_economy() {
    for class in [ShipClass::Pioneer, ShipClass::Monolith] {
        let mut s = Server::open(":memory:").unwrap();
        let mut pilot = None;
        let mut now = 0;
        command(
            &mut s,
            &mut pilot,
            Request::Connect {
                supports_room_controls: false,
                world_layout_version: 1,
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
                protocol_version: PROTOCOL_VERSION,
                pilot: "Resident".into(),
                resume_token: None,
            },
            now,
        );
        command(
            &mut s,
            &mut pilot,
            Request::SelectShip { ship_class: class },
            now,
        );
        let before = command(&mut s, &mut pilot, Request::ExportLedger, now)
            .ledger
            .unwrap();
        let ship_position = current(&mut s, now).flight.unwrap().position;
        command(&mut s, &mut pilot, Request::DisembarkShip, now);
        let room = quarters_layout(class);
        let aisle = [0., 0., room.min[2] + 3.4];
        walk_to(&mut s, &mut pilot, &mut now, aisle);
        let desk_access = [-4., 0., room.min[2] + 3.4];
        walk_to(&mut s, &mut pilot, &mut now, desk_access);
        let view = current(&mut s, now);
        assert_eq!(view.flight.unwrap().position, ship_position);
        assert_eq!(view.location, Location::Docked);
        assert!(!s.handle_at(&mut pilot, Request::EnterShip, now).ok);
        assert!(!s.handle_at(&mut pilot, Request::Undock, now).ok);
        // Ordinary services remain ordinary authenticated requests, not a new
        // server proximity capability merely because a world screen is visible.
        assert!(command(&mut s, &mut pilot, Request::Snapshot, now)
            .assets
            .is_some());
        assert!(
            !s.handle_at(&mut pilot, Request::StationSit { seated: true }, now)
                .ok
        );
        walk_to(&mut s, &mut pilot, &mut now, aisle);
        let seat_aisle = [0., 0., room.seat_point[2]];
        walk_to(&mut s, &mut pilot, &mut now, seat_aisle);
        walk_to(&mut s, &mut pilot, &mut now, room.seat_point);
        command(
            &mut s,
            &mut pilot,
            Request::StationSit { seated: true },
            now,
        );
        assert!(current(&mut s, now).station_avatar.unwrap().seated);
        command(
            &mut s,
            &mut pilot,
            Request::StationWalk {
                forward: 0.,
                strafe: 0.,
                turn: 0.,
            },
            now,
        );
        assert!(
            current(&mut s, now).station_avatar.unwrap().seated,
            "neutral input must not undo sitting"
        );
        command(
            &mut s,
            &mut pilot,
            Request::StationSit { seated: false },
            now,
        );
        walk_to(&mut s, &mut pilot, &mut now, seat_aisle);
        walk_to(
            &mut s,
            &mut pilot,
            &mut now,
            hangar_layout(class).boarding_point,
        );
        command(&mut s, &mut pilot, Request::EnterShip, now);
        assert!(current(&mut s, now).station_avatar.is_none());
        command(&mut s, &mut pilot, Request::Undock, now);
        assert_eq!(current(&mut s, now).location, Location::Station);
        let after = command(&mut s, &mut pilot, Request::ExportLedger, now)
            .ledger
            .unwrap();
        assert_eq!(before.entries.len(), after.entries.len());
        arkline_verifier::verify(&after, None).unwrap();
    }
}
