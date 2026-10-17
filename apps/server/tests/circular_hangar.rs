use arkline_protocol::*;
use arkline_server::editor::EditorWorld;
use rusqlite::Connection;
use serde_json::Value;
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "arkline-circular-hangar-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn seed_avatar(&self, position: [f64; 3], heading: f64) {
        // Offline disposable fixture preparation, never a gameplay command or a user world.
        let db = Connection::open(self.0.join("world.sqlite")).unwrap();
        let raw: String = db
            .query_row("SELECT json FROM world WHERE id=1", [], |row| row.get(0))
            .unwrap();
        let mut world: Value = serde_json::from_str(&raw).unwrap();
        world["pilots"]["Walker"]["view"]["station_avatar"] = serde_json::to_value(StationAvatar {
            onboard_interior: false,
            interior_deck: 1,
            jump_started_ms: None,
            seated: false,
            position,
            heading,
            moving: false,
            updated_at_ms: 0,
        })
        .unwrap();
        world["pilots"]["Walker"]["walk_input"] = Value::Null;
        db.execute("UPDATE world SET json=?1 WHERE id=1", [world.to_string()])
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn accepted(response: Response) -> Response {
    assert!(response.ok, "{}", response.message);
    response
}
fn call(world: &mut EditorWorld, session: &mut Option<String>, request: Request) -> Response {
    accepted(world.handle(session, request))
}
fn connect(
    world: &mut EditorWorld,
    session: &mut Option<String>,
    name: &str,
    token: Option<String>,
) -> Response {
    call(
        world,
        session,
        Request::Connect {
            world_layout_version: WORLD_LAYOUT_SCHEMA_VERSION,
            supports_room_controls: true,
            supports_fleet_hangar: true,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: name.into(),
            resume_token: token,
        },
    )
}
fn walker(snapshot: &Snapshot) -> &PilotView {
    snapshot
        .pilots
        .iter()
        .find(|pilot| pilot.name == "Walker")
        .unwrap()
}
fn avatar(world: &mut EditorWorld, session: &mut Option<String>) -> StationAvatar {
    walker(&call(world, session, Request::Snapshot).snapshot.unwrap())
        .station_avatar
        .clone()
        .unwrap()
}
fn walk(world: &mut EditorWorld, session: &mut Option<String>, steps: usize) {
    for _ in 0..steps {
        call(
            world,
            session,
            Request::StationWalk {
                forward: 1.0,
                strafe: 0.0,
                turn: 0.0,
            },
        );
        world.step(250).unwrap();
        assert!(fleet_walkable(
            FleetLayoutRevision::CircularV1,
            avatar(world, session).position
        ));
    }
}

#[test]
fn circular_snapshot_uses_shared_poses_and_rejects_foreign_boarding_without_economic_change() {
    let fixture = Fixture::new();
    let mut world = EditorWorld::open(&fixture.0).unwrap();
    let mut owner = None;
    let connected = connect(&mut world, &mut owner, "Walker", None);
    let hangar = connected.assets.unwrap().hangar.unwrap();
    assert_eq!(hangar.layout_revision, FleetLayoutRevision::CircularV1);
    assert_eq!(hangar.berths.len(), ALL_SHIP_CLASSES.len());
    for berth in &hangar.berths {
        let expected = fleet_berth_pose(FleetLayoutRevision::CircularV1, berth.ship_class);
        assert_eq!(berth.position, expected.position);
        assert_eq!(berth.boarding_point, expected.boarding_point);
        assert_eq!(berth.yaw, expected.yaw);
        assert!(berth.position[1] - berth.ship_class.spec().height_m / 2.0 >= 3.999);
    }
    let mut other = None;
    let outsider = connect(&mut world, &mut other, "Other", None);
    let foreign = outsider.assets.unwrap().hangar.unwrap().berths[0]
        .id
        .clone();
    call(&mut world, &mut owner, Request::DisembarkShip);
    let before = call(&mut world, &mut owner, Request::ExportLedger)
        .ledger
        .unwrap();
    assert!(
        !world
            .handle(&mut owner, Request::BoardShip { id: foreign })
            .ok
    );
    assert!(
        !world
            .handle(
                &mut owner,
                Request::BoardShip {
                    id: "forged-berth".into()
                }
            )
            .ok
    );
    let after = call(&mut world, &mut owner, Request::ExportLedger)
        .ledger
        .unwrap();
    assert_eq!(
        serde_json::to_value(&before.entries).unwrap(),
        serde_json::to_value(&after.entries).unwrap()
    );
    arkline_verifier::verify(&after, None).unwrap();
}

#[test]
fn real_walking_stops_at_the_rotated_hull_service_boundary() {
    let fixture = Fixture::new();
    let token = {
        let mut world = EditorWorld::open(&fixture.0).unwrap();
        let mut session = None;
        let token = connect(&mut world, &mut session, "Walker", None).resume_token;
        call(&mut world, &mut session, Request::DisembarkShip);
        let start = avatar(&mut world, &mut session);
        assert_eq!(start.position, [0.0; 3]);
        assert!((start.heading - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        token
    };
    for class in [
        ShipClass::Pioneer,
        ShipClass::Prospector,
        ShipClass::Monolith,
    ] {
        let pose = fleet_berth_pose(FleetLayoutRevision::CircularV1, class);
        let heading = (pose.yaw + std::f64::consts::FRAC_PI_2).rem_euclid(std::f64::consts::TAU);
        fixture.seed_avatar(pose.boarding_point, heading);
        let mut world = EditorWorld::open(&fixture.0).unwrap();
        let mut session = None;
        connect(&mut world, &mut session, "Walker", token.clone());
        walk(&mut world, &mut session, 16);
        let stopped = avatar(&mut world, &mut session);
        let toward_hull = (stopped.position[0] - pose.boarding_point[0]) * heading.cos()
            + (stopped.position[2] - pose.boarding_point[2]) * heading.sin();
        // Each boarding marker is six metres from the near body face, at its actual yaw.
        assert!(
            toward_hull > 5.3 && toward_hull <= 5.700_001,
            "{class:?}: {toward_hull}"
        );
    }
}

#[test]
fn concourse_void_edge_is_authoritative_and_valid_position_survives_restart() {
    let fixture = Fixture::new();
    let token = {
        let mut world = EditorWorld::open(&fixture.0).unwrap();
        connect(&mut world, &mut None, "Walker", None).resume_token
    };
    let revision = FleetLayoutRevision::CircularV1;
    let geometry = fleet_layout_geometry(revision);
    let pioneer = fleet_berth_pose(revision, ShipClass::Pioneer);
    let radius = geometry.concourse_radius_m + pioneer.platform_radius_m;
    // Middle of the empty four-degree gap after the Pioneer platform's angular wedge.
    let theta = std::f64::consts::FRAC_PI_2
        + (pioneer.platform_radius_m / radius).asin()
        + 2f64.to_radians();
    let distance = geometry.concourse_radius_m - 0.5;
    fixture.seed_avatar(
        [
            geometry.center[0] + theta.cos() * distance,
            0.0,
            geometry.center[2] + theta.sin() * distance,
        ],
        theta,
    );
    let stopped = {
        let mut world = EditorWorld::open(&fixture.0).unwrap();
        let mut session = None;
        connect(&mut world, &mut session, "Walker", token.clone());
        walk(&mut world, &mut session, 12);
        avatar(&mut world, &mut session)
    };
    let radial =
        (stopped.position[0] - geometry.center[0]).hypot(stopped.position[2] - geometry.center[2]);
    assert!(radial <= geometry.concourse_radius_m - STATION_AVATAR_RADIUS_M + 0.000_01);
    let mut world = EditorWorld::open(&fixture.0).unwrap();
    let mut session = None;
    connect(&mut world, &mut session, "Walker", token);
    let restored = avatar(&mut world, &mut session);
    assert_eq!(restored.position, stopped.position);
    assert!(!restored.moving);
    assert!(restored.jump_started_ms.is_none());
}

#[test]
fn every_catalog_berth_can_board_by_stable_identity_and_restart_preserves_assets() {
    let fixture = Fixture::new();
    let (token, ids) = {
        let mut world = EditorWorld::open(&fixture.0).unwrap();
        let response = connect(&mut world, &mut None, "Walker", None);
        (
            response.resume_token,
            response
                .assets
                .unwrap()
                .hangar
                .unwrap()
                .berths
                .into_iter()
                .map(|berth| (berth.ship_class, berth.id))
                .collect::<Vec<_>>(),
        )
    };
    for class in ALL_SHIP_CLASSES {
        let pose = fleet_berth_pose(FleetLayoutRevision::CircularV1, class);
        fixture.seed_avatar(
            pose.boarding_point,
            (pose.yaw + std::f64::consts::FRAC_PI_2).rem_euclid(std::f64::consts::TAU),
        );
        let mut world = EditorWorld::open(&fixture.0).unwrap();
        let mut session = None;
        let response = connect(&mut world, &mut session, "Walker", token.clone());
        let hangar = response.assets.unwrap().hangar.unwrap();
        assert!(hangar.berths.iter().all(|berth| ids
            .iter()
            .any(|(class, id)| berth.ship_class == *class && berth.id == *id)));
        let id = ids
            .iter()
            .find(|(candidate, _)| *candidate == class)
            .unwrap()
            .1
            .clone();
        let boarded = call(&mut world, &mut session, Request::BoardShip { id });
        assert_eq!(walker(&boarded.snapshot.unwrap()).ship_class, class);
        assert!(walker(
            &call(&mut world, &mut session, Request::Snapshot)
                .snapshot
                .unwrap()
        )
        .station_avatar
        .is_none());
        let ledger = call(&mut world, &mut session, Request::ExportLedger)
            .ledger
            .unwrap();
        arkline_verifier::verify(&ledger, None).unwrap();
    }
}
