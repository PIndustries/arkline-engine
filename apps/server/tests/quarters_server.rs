use arkline_protocol::*;
use arkline_server::Server;
fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Response {
    let out = s.handle_at(p, r, t);
    assert!(out.ok, "{}", out.message);
    out
}
fn a(s: &mut Server, t: u64) -> StationAvatar {
    ok(s, &mut None, Request::Snapshot, t)
        .snapshot
        .unwrap()
        .pilots[0]
        .station_avatar
        .clone()
        .unwrap()
}
fn walk(
    s: &mut Server,
    p: &mut Option<String>,
    t: &mut u64,
    forward: f64,
    strafe: f64,
    steps: u64,
) {
    for _ in 0..steps {
        ok(
            s,
            p,
            Request::StationWalk {
                forward,
                strafe,
                turn: 0.,
            },
            *t,
        );
        *t += 100;
        s.tick_at(*t).unwrap();
    }
    ok(
        s,
        p,
        Request::StationWalk {
            forward: 0.,
            strafe: 0.,
            turn: 0.,
        },
        *t,
    );
}
fn setup(path: &std::path::Path) -> (Server, Option<String>, Option<String>) {
    let mut s = Server::open(path).unwrap();
    let mut p = None;
    let token = ok(
        &mut s,
        &mut p,
        Request::Connect {
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Resident".into(),
            resume_token: None,
        },
        0,
    )
    .resume_token;
    ok(&mut s, &mut p, Request::DisembarkShip, 0);
    (s, p, token)
}
#[test]
fn doorway_is_open_but_walls_and_furniture_block_authoritative_steps() {
    let (mut s, mut p, _) = setup(std::path::Path::new(":memory:"));
    let mut t = 0;
    let q = quarters_layout(ShipClass::Pioneer);
    walk(&mut s, &mut p, &mut t, 1., 0., 10);
    walk(&mut s, &mut p, &mut t, 0., 1., 20);
    assert!(a(&mut s, t).position[2] <= q.min[2] - STATION_AVATAR_RADIUS_M + 1e-8);
    walk(&mut s, &mut p, &mut t, -1., 0., 10);
    walk(&mut s, &mut p, &mut t, 0., 1., 20);
    assert!(a(&mut s, t).position[2] > q.min[2] + 5.0);
    assert!(!s.handle_at(&mut p, Request::EnterShip, t).ok);
    walk(&mut s, &mut p, &mut t, 0., 1., 20);
    assert!(a(&mut s, t).position[2] <= q.max[2] - 0.6 + 1e-8);
    walk(&mut s, &mut p, &mut t, 0., -1., 6);
    walk(&mut s, &mut p, &mut t, -1., 0., 20);
    assert!(
        a(&mut s, t).position[0] >= -4.7 - 1e-8,
        "bed must stop entry"
    );
    let ledger = ok(&mut s, &mut p, Request::ExportLedger, t).ledger.unwrap();
    assert_eq!(ledger.entries.len(), 1);
}
#[test]
fn sofa_requires_proximity_survives_reconnect_and_walking_stands_in_place() {
    let (mut s, mut p, token) = setup(std::path::Path::new(":memory:"));
    let mut t = 0;
    assert!(
        !s.handle_at(&mut p, Request::StationSit { seated: true }, 0)
            .ok
    );
    walk(&mut s, &mut p, &mut t, 0., 1., 19);
    walk(&mut s, &mut p, &mut t, 1., 0., 11);
    ok(&mut s, &mut p, Request::StationSit { seated: true }, t);
    let seated = a(&mut s, t);
    assert!(seated.seated);
    assert_eq!(
        seated.position,
        quarters_layout(ShipClass::Pioneer).seat_point
    );
    s.disconnect(&mut p);
    ok(
        &mut s,
        &mut p,
        Request::Connect {
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Resident".into(),
            resume_token: token,
        },
        t,
    );
    assert!(a(&mut s, t).seated);
    ok(
        &mut s,
        &mut p,
        Request::StationWalk {
            forward: 0.,
            strafe: 1.,
            turn: 0.,
        },
        t,
    );
    let standing = a(&mut s, t);
    assert!(!standing.seated);
    assert_eq!(standing.position, seated.position);
}
#[test]
fn old_positions_in_new_obstacles_relocate_once_but_valid_quarters_positions_persist() {
    let path =
        std::env::temp_dir().join(format!("arkline-quarters-{}.sqlite", rand::random::<u64>()));
    let (s, _, _) = setup(&path);
    drop(s);
    let q = quarters_layout(ShipClass::Pioneer);
    let edit = |position: [f64; 3]| {
        let db = rusqlite::Connection::open(&path).unwrap();
        let raw: String = db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let mut w: serde_json::Value = serde_json::from_str(&raw).unwrap();
        w["pilots"]["Resident"]["view"]["station_avatar"]["position"] = serde_json::json!(position);
        db.execute("UPDATE world SET json=?1 WHERE id=1", [w.to_string()])
            .unwrap();
    };
    edit([-6., 0., q.max[2] - 3.]);
    let mut s = Server::open(&path).unwrap();
    assert_eq!(
        a(&mut s, 0).position,
        hangar_layout(ShipClass::Pioneer).boarding_point
    );
    drop(s);
    let clear = [0., 0., q.min[2] + 5.];
    edit(clear);
    for _ in 0..2 {
        let mut s = Server::open(&path).unwrap();
        assert_eq!(a(&mut s, 0).position, clear);
    }
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}.lock", path.display()));
}
