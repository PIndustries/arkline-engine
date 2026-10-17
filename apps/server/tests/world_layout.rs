use arkline_protocol::*;
use arkline_server::{editor::EditorWorld, Server};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::PathBuf;

/// Every test owns its entire directory, including SQLite sidecars and lock.
struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "arkline-world-layout-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir(&directory).unwrap();
        Self(directory)
    }

    fn database(&self) -> PathBuf {
        self.0.join("world.sqlite")
    }

    fn world(&self) -> Value {
        let database = Connection::open(self.database()).unwrap();
        let raw: String = database
            .query_row("SELECT json FROM world WHERE id=1", [], |row| row.get(0))
            .unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    fn edit(&self, edit: impl FnOnce(&mut Value)) {
        let mut world = self.world();
        edit(&mut world);
        let database = Connection::open(self.database()).unwrap();
        database
            .execute("UPDATE world SET json=?1 WHERE id=1", [world.to_string()])
            .unwrap();
    }

    /// Compare durable SQL contents, rather than SQLite/WAL file bytes, which
    /// may change when a read connection checkpoints the database.
    fn durable_state(&self) -> Value {
        let database = Connection::open(self.database()).unwrap();
        let world: String = database
            .query_row("SELECT json FROM world WHERE id=1", [], |row| row.get(0))
            .unwrap();
        let ledger = database
            .prepare("SELECT sequence,json FROM ledger ORDER BY sequence")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let audit = database
            .prepare("SELECT pilot,issuer,subject,linked_at_ms FROM identity_audit ORDER BY pilot")
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let schema = database
            .prepare("SELECT name,type,sql FROM sqlite_master ORDER BY name,type")
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let realm: String = database
            .query_row("SELECT realm FROM identity_realm WHERE id=1", [], |row| {
                row.get(0)
            })
            .unwrap();
        let has_clock: bool = database
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='editor_clock')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let clock: Option<String> = has_clock.then(|| {
            database
                .query_row("SELECT time_ms FROM editor_clock WHERE id=1", [], |row| {
                    row.get(0)
                })
                .unwrap()
        });
        json!({
            "world": world,
            "ledger": ledger,
            "identity_audit": audit,
            "schema": schema,
            "realm": realm,
            "clock": clock,
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn connect(name: &str, token: Option<String>, capability: u32) -> Request {
    serde_json::from_value(json!({
        "command": "connect",
        "protocol_version": PROTOCOL_VERSION,
        "world_layout_version": capability,
        "pilot": name,
        "resume_token": token,
        "supports_room_controls": true,
        "supports_fleet_hangar": false,
        "supports_pioneer_interior": true,
    }))
    .unwrap()
}

fn accepted(response: Response) -> Response {
    assert!(response.ok, "{}", response.message);
    response
}

fn call(editor: &mut EditorWorld, session: &mut Option<String>, request: Request) -> Response {
    accepted(editor.handle(session, request))
}

fn snapshot(editor: &mut EditorWorld) -> Snapshot {
    call(editor, &mut None, Request::Snapshot).snapshot.unwrap()
}

fn layout(snapshot: &Snapshot) -> WorldLayoutDefinition {
    snapshot.world_layout.clone().unwrap_or_default()
}

fn preserved_coordinates_and_holdings(world: &Value) -> Value {
    let pilots = world["pilots"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, pilot)| {
            (
                name.clone(),
                json!({
                    "token_hash": pilot["token_hash"],
                    "identity": pilot["identity"],
                    "location": pilot["view"]["location"],
                    "system_id": pilot["view"]["system_id"],
                    "ship_class": pilot["view"]["ship_class"],
                    "credits": pilot["view"]["credits"],
                    "ore": pilot["view"]["ore"],
                    "flight_position": pilot["view"]["flight"]["position"],
                    "flight_orientation": pilot["view"]["flight"]["orientation"],
                    "flight_warp": pilot["view"]["flight"]["warp"],
                    "warp": pilot["warp"],
                    "avatar": pilot["view"]["station_avatar"],
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let accounts = world["accounts"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, account)| {
            (
                name.clone(),
                json!({
                    "ships": account["ships"],
                    "vessel_ids": account["vessel_ids"],
                    "storage": account["storage"],
                    "corporation": account["corporation"],
                    "fleet_hangar": account["fleet_hangar"],
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    json!({
        "pilots": pilots,
        "accounts": accounts,
        "surface_props": world["surface_props"],
        "next_prop_id": world["next_prop_id"],
        "corporations": world["corporations"],
        "orders": world["orders"],
        "belt_ore": world["belt_ore"],
        "asteroids": world["asteroids"],
    })
}

#[test]
fn absent_legacy_revision_preserves_populated_absolute_coordinates_and_ledger_on_reopen() {
    let fixture = Fixture::new();
    let mut editor =
        EditorWorld::open_with_creation_scene(&fixture.0, SceneLayoutRevision::LegacyV1).unwrap();
    let mut builder = None;
    let token = call(&mut editor, &mut builder, connect("Builder", None, 0)).resume_token;
    call(&mut editor, &mut builder, Request::Undock);
    fixture.edit(|world| {
        world["pilots"]["Builder"]["view"]["flight"]["position"] = json!([0.0, -119_950.0, 0.0]);
    });
    call(&mut editor, &mut builder, Request::Snapshot);
    call(
        &mut editor,
        &mut builder,
        Request::SpawnSurfaceProp {
            kind: SurfacePropKind::Crate,
            position: [40.0, -120_000.0, 0.0],
        },
    );
    let mut flyer = None;
    call(&mut editor, &mut flyer, connect("Flyer", None, 0));
    call(&mut editor, &mut flyer, Request::Undock);
    call(
        &mut editor,
        &mut flyer,
        Request::Warp {
            destination: Destination::Belt,
        },
    );
    let mut walker = None;
    call(&mut editor, &mut walker, connect("Walker", None, 0));
    call(&mut editor, &mut walker, Request::DisembarkShip);
    let before_export = call(&mut editor, &mut builder, Request::ExportLedger)
        .ledger
        .unwrap();
    arkline_verifier::verify(&before_export, None).unwrap();
    drop(editor);

    // Model a pre-layout save explicitly; no new layout field is needed to
    // preserve its original physical coordinates or private hangar layout.
    fixture.edit(|world| {
        world.as_object_mut().unwrap().remove("scene_layout");
    });
    let before = fixture.world();
    assert_eq!(before["surface_props"].as_array().unwrap().len(), 1);
    assert!(!before["pilots"]["Flyer"]["warp"].is_null());
    assert!(!before["pilots"]["Walker"]["view"]["station_avatar"].is_null());
    assert!(!before["accounts"]["Builder"]["vessel_ids"].is_null());
    let preserved = preserved_coordinates_and_holdings(&before);
    let ledger = fixture.durable_state()["ledger"].clone();

    for _ in 0..3 {
        let mut editor = EditorWorld::open(&fixture.0).unwrap();
        assert!(editor.is_paused());
        assert_eq!(layout(&snapshot(&mut editor)), legacy_world_layout());
        let current = fixture.world();
        assert!(current.get("scene_layout").is_none());
        assert_eq!(
            fixture.durable_state()["realm"],
            json!("local-development-v1")
        );
        assert_eq!(preserved_coordinates_and_holdings(&current), preserved);
        assert_eq!(fixture.durable_state()["ledger"], ledger);
        drop(editor);
    }

    let mut editor = EditorWorld::open(&fixture.0).unwrap();
    builder = None;
    call(&mut editor, &mut builder, connect("Builder", token, 0));
    let after_export = call(&mut editor, &mut builder, Request::ExportLedger)
        .ledger
        .unwrap();
    arkline_verifier::verify(&after_export, None).unwrap();
    assert_eq!(
        serde_json::to_value(&after_export.entries).unwrap(),
        serde_json::to_value(&before_export.entries).unwrap()
    );
    assert_eq!(
        after_export.snapshot.ledger_hash,
        before_export.snapshot.ledger_hash
    );
}

#[test]
fn ordinary_and_local_servers_create_legacy_worlds_for_capability_zero_clients() {
    for local in [false, true] {
        let fixture = Fixture::new();
        let mut server = if local {
            Server::open_local_development(&fixture.0).unwrap()
        } else {
            Server::open(fixture.database()).unwrap()
        };
        let response = accepted(server.handle_at(&mut None, connect("OldClient", None, 0), 0));
        assert_eq!(layout(&response.snapshot.unwrap()), legacy_world_layout());
        assert!(fixture.world().get("scene_layout").is_none());
        assert_eq!(
            fixture.durable_state()["realm"],
            json!(if local {
                "local-development-v1"
            } else {
                "legacy"
            })
        );
    }
}

#[test]
fn explicit_legacy_editor_creation_stays_legacy_when_opened_with_default() {
    let fixture = Fixture::new();
    let mut editor =
        EditorWorld::open_with_creation_scene(&fixture.0, SceneLayoutRevision::LegacyV1).unwrap();
    let token = call(&mut editor, &mut None, connect("LegacyEditor", None, 0)).resume_token;
    assert_eq!(layout(&snapshot(&mut editor)), legacy_world_layout());
    assert_eq!(
        fixture.durable_state()["realm"],
        json!("local-development-v1")
    );
    drop(editor);

    let mut editor = EditorWorld::open(&fixture.0).unwrap();
    let response = call(&mut editor, &mut None, connect("LegacyEditor", token, 0));
    assert_eq!(layout(&response.snapshot.unwrap()), legacy_world_layout());
    assert!(fixture.world().get("scene_layout").is_none());
    assert_eq!(
        fixture.durable_state()["realm"],
        json!("local-development-v1")
    );
}

#[test]
fn fresh_editor_persists_orbital_descriptor_and_creation_override_cannot_relocate_it() {
    let fixture = Fixture::new();
    let mut editor = EditorWorld::open(&fixture.0).unwrap();
    let expected = SceneLayoutRevision::OrbitalV1.definition();
    let first = snapshot(&mut editor).world_layout.unwrap();
    assert_eq!(first, expected);
    first.validate().unwrap();
    assert_eq!(fixture.world()["scene_layout"], json!("orbital_v1"));
    assert_eq!(
        fixture.durable_state()["realm"],
        json!("local-development-orbital-v1")
    );
    let token = call(
        &mut editor,
        &mut None,
        connect("OrbitalEditor", None, WORLD_LAYOUT_SCHEMA_VERSION),
    )
    .resume_token;
    let before = fixture.durable_state();
    let time = editor.time();
    drop(editor);

    for revision in [
        SceneLayoutRevision::LegacyV1,
        SceneLayoutRevision::OrbitalV1,
    ] {
        let mut editor = EditorWorld::open_with_creation_scene(&fixture.0, revision).unwrap();
        assert!(editor.is_paused());
        assert_eq!(editor.time(), time);
        assert_eq!(snapshot(&mut editor).world_layout.unwrap(), expected);
        assert_eq!(fixture.durable_state(), before);
        drop(editor);
    }
    let mut editor = EditorWorld::open(&fixture.0).unwrap();
    let response = call(
        &mut editor,
        &mut None,
        connect("OrbitalEditor", token, WORLD_LAYOUT_SCHEMA_VERSION),
    );
    assert_eq!(response.snapshot.unwrap().world_layout.unwrap(), expected);
}

#[test]
fn orbital_admission_rejects_old_clients_without_creating_pilots_or_mutating_durable_state() {
    let fixture = Fixture::new();
    let mut editor = EditorWorld::open(&fixture.0).unwrap();
    let mut owner = None;
    let token = call(
        &mut editor,
        &mut owner,
        connect("Owner", None, WORLD_LAYOUT_SCHEMA_VERSION),
    )
    .resume_token;
    let before = fixture.durable_state();

    for (name, supplied, mut session) in [
        ("OldClient", None, None),
        ("Owner", token.clone(), None),
        ("OldClient", None, owner.clone()),
    ] {
        let previous = session.clone();
        let response = editor.handle(&mut session, connect(name, supplied, 0));
        assert!(!response.ok);
        assert!(
            response.message.to_lowercase().contains("layout"),
            "{}",
            response.message
        );
        assert!(response.resume_token.is_none());
        assert!(response.snapshot.is_none());
        assert_eq!(session, previous);
        assert_eq!(fixture.durable_state(), before);
    }
    assert!(fixture.world()["pilots"].get("OldClient").is_none());

    // The rejected name is still available to a compatible client, and the
    // existing owner's saved credential remains valid after every rejection.
    let mut compatible = None;
    call(
        &mut editor,
        &mut compatible,
        connect("OldClient", None, WORLD_LAYOUT_SCHEMA_VERSION),
    );
    assert_eq!(compatible.as_deref(), Some("OldClient"));
    call(
        &mut editor,
        &mut None,
        connect("Owner", token, WORLD_LAYOUT_SCHEMA_VERSION),
    );
}

#[test]
fn omitted_capability_is_legacy_and_cannot_enter_orbital_world() {
    let fixture = Fixture::new();
    let mut editor = EditorWorld::open(&fixture.0).unwrap();
    let before = fixture.durable_state();
    let request = serde_json::from_value(json!({
        "command": "connect",
        "protocol_version": PROTOCOL_VERSION,
        "pilot": "PreLayoutClient",
        "resume_token": null,
    }))
    .unwrap();
    let mut session = None;
    let response = editor.handle(&mut session, request);
    assert!(!response.ok);
    assert!(session.is_none());
    assert_eq!(fixture.durable_state(), before);
}

#[test]
fn unsupported_future_capability_is_rejected_without_mutation_in_both_layouts() {
    for revision in [
        SceneLayoutRevision::LegacyV1,
        SceneLayoutRevision::OrbitalV1,
    ] {
        let fixture = Fixture::new();
        let mut editor = EditorWorld::open_with_creation_scene(&fixture.0, revision).unwrap();
        let before = fixture.durable_state();
        for capability in [WORLD_LAYOUT_SCHEMA_VERSION + 1, u32::MAX] {
            let mut session = None;
            let response = editor.handle(&mut session, connect("FutureClient", None, capability));
            assert!(!response.ok);
            assert!(response.message.contains("layout"), "{}", response.message);
            assert!(session.is_none());
            assert_eq!(fixture.durable_state(), before);
        }
    }
}

#[test]
fn account_admission_forwards_layout_capability_before_binding_identity_or_creating_pilot() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use rsa::{pkcs1::EncodeRsaPrivateKey, traits::PublicKeyParts, RsaPrivateKey};

    let fixture = Fixture::new();
    // Ordinary account realms retain legacy geometry. A future capability must
    // still be forwarded and rejected: hardcoding zero during AccountConnect
    // translation would incorrectly admit it and bind an account identity.
    let mut server = Server::open(fixture.database()).unwrap();
    let key = RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).unwrap();
    let config = serde_json::from_value(json!({
        "issuer": "https://layout-issuer.example.test/",
        "audience": "arkline:layout-fixture",
        "required_scopes": ["arkline.play"],
        "allowed_client_ids": ["layout-test-client"],
        "allowed_algorithms": ["RS256"],
        "max_lifetime_seconds": 300,
        "clock_skew_seconds": 5,
    }))
    .unwrap();
    let jwks = serde_json::to_vec(&json!({"keys": [{
        "kty": "RSA", "alg": "RS256", "use": "sig", "kid": "layout-fixture",
        "n": URL_SAFE_NO_PAD.encode(key.n().to_bytes_be()),
        "e": URL_SAFE_NO_PAD.encode(key.e().to_bytes_be()),
    }]}))
    .unwrap();
    server.configure_auth(config, &jwks, true).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut header = Header::new(Algorithm::RS256);
    header.typ = Some("at+jwt".into());
    header.kid = Some("layout-fixture".into());
    let token = encode(
        &header,
        &json!({
            "iss": "https://layout-issuer.example.test/", "sub": "layout-account",
            "aud": "arkline:layout-fixture", "client_id": "layout-test-client",
            "jti": "layout-fixture", "iat": now - 10, "exp": now + 120,
            "scope": "arkline.play",
        }),
        &EncodingKey::from_rsa_der(key.to_pkcs1_der().unwrap().as_bytes()),
    )
    .unwrap();
    let account_request = |capability| {
        serde_json::from_value(json!({
            "command": "account_connect", "access_token": token,
            "pilot": "AccountPilot", "protocol_version": PROTOCOL_VERSION,
            "world_layout_version": capability,
            "supports_room_controls": true, "supports_fleet_hangar": false,
            "supports_pioneer_interior": true,
        }))
        .unwrap()
    };
    let before = fixture.durable_state();
    for capability in [WORLD_LAYOUT_SCHEMA_VERSION + 1, u32::MAX] {
        let (mut session, mut expiry) = (None, None);
        let response =
            server.handle_authenticated(&mut session, &mut expiry, account_request(capability));
        assert!(!response.ok);
        assert!(response.message.contains("layout"), "{}", response.message);
        assert!(session.is_none());
        assert!(expiry.is_none());
        assert_eq!(fixture.durable_state(), before);
    }
    let (mut session, mut expiry) = (None, None);
    let response = accepted(server.handle_authenticated(
        &mut session,
        &mut expiry,
        account_request(WORLD_LAYOUT_SCHEMA_VERSION),
    ));
    assert_eq!(session.as_deref(), Some("AccountPilot"));
    assert_eq!(expiry, Some(now + 120));
    assert_eq!(
        response.snapshot.unwrap().world_layout.unwrap(),
        SceneLayoutRevision::LegacyV1.definition()
    );
    assert_eq!(
        fixture.world()["pilots"]["AccountPilot"]["identity"]["subject"],
        json!("layout-account")
    );
    assert_eq!(
        fixture.durable_state()["identity_audit"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let after = fixture.durable_state();
    let response = server.handle_authenticated(
        &mut session,
        &mut expiry,
        account_request(WORLD_LAYOUT_SCHEMA_VERSION + 1),
    );
    assert!(!response.ok);
    assert_eq!(session.as_deref(), Some("AccountPilot"));
    assert_eq!(expiry, Some(now + 120));
    assert_eq!(fixture.durable_state(), after);
}

#[test]
fn orbital_surface_altitude_projection_and_restart_use_the_persisted_planet() {
    let fixture = Fixture::new();
    let mut editor = EditorWorld::open(&fixture.0).unwrap();
    let mut session = None;
    let token = call(
        &mut editor,
        &mut session,
        connect("SurfaceBuilder", None, WORLD_LAYOUT_SCHEMA_VERSION),
    )
    .resume_token;
    call(&mut editor, &mut session, Request::Undock);
    let descriptor = snapshot(&mut editor).world_layout.unwrap();
    let planet = descriptor.planet(1).unwrap();
    let ship = [
        planet.center[0],
        planet.center[1] + planet.radius_m + 50.0,
        planet.center[2],
    ];
    fixture.edit(|world| {
        world["pilots"]["SurfaceBuilder"]["view"]["flight"]["position"] = json!(ship);
    });
    let current = call(&mut editor, &mut session, Request::Snapshot)
        .snapshot
        .unwrap();
    let own = current
        .pilots
        .iter()
        .find(|pilot| pilot.name == "SurfaceBuilder")
        .unwrap();
    let surface = own.surface.as_ref().unwrap();
    assert_eq!(surface.planet_id, 1);
    assert!((surface.altitude_m - 50.0).abs() < 0.001);
    let placed = call(
        &mut editor,
        &mut session,
        Request::SpawnSurfaceProp {
            kind: SurfacePropKind::Crate,
            position: [ship[0] + 40.0, planet.center[1] + planet.radius_m, ship[2]],
        },
    )
    .snapshot
    .unwrap();
    assert_eq!(placed.surface_props.len(), 1);
    let prop = &placed.surface_props[0];
    assert!((planet.altitude(prop.position) - prop.kind.size()[1] / 2.0).abs() < 0.001);
    assert!((planet_altitude(prop.position) - prop.kind.size()[1] / 2.0).abs() > 1_000_000.0);
    let saved = fixture.world()["surface_props"].clone();
    let ledger = fixture.durable_state()["ledger"].clone();
    drop(editor);

    let mut editor = EditorWorld::open(&fixture.0).unwrap();
    let recovered = call(
        &mut editor,
        &mut None,
        connect("SurfaceBuilder", token, WORLD_LAYOUT_SCHEMA_VERSION),
    )
    .snapshot
    .unwrap();
    assert_eq!(recovered.world_layout.unwrap(), descriptor);
    assert_eq!(recovered.surface_props, placed.surface_props);
    assert_eq!(fixture.world()["surface_props"], saved);
    assert_eq!(fixture.durable_state()["ledger"], ledger);
}

#[test]
fn unsupported_persisted_revision_fails_open_without_rewriting_world_or_ledger() {
    for editor_realm in [false, true] {
        let fixture = Fixture::new();
        if editor_realm {
            let mut editor = EditorWorld::open(&fixture.0).unwrap();
            call(
                &mut editor,
                &mut None,
                connect("Existing", None, WORLD_LAYOUT_SCHEMA_VERSION),
            );
        } else {
            let mut server = Server::open_local_development(&fixture.0).unwrap();
            accepted(server.handle_at(&mut None, connect("Existing", None, 0), 0));
        }
        fixture.edit(|world| world["scene_layout"] = json!("future_revision_v99"));
        let before = fixture.durable_state();
        let error = if editor_realm {
            match EditorWorld::open(&fixture.0) {
                Ok(_) => panic!("unsupported editor layout was accepted"),
                Err(error) => error.to_string(),
            }
        } else {
            match Server::open_local_development(&fixture.0) {
                Ok(_) => panic!("unsupported server layout was accepted"),
                Err(error) => error.to_string(),
            }
        };
        assert!(error.contains("future_revision_v99"), "{error}");
        assert_eq!(fixture.durable_state(), before);
        assert_eq!(fixture.world()["pilots"].as_object().unwrap().len(), 1);
    }
}

#[test]
fn orbital_marker_and_stored_revision_must_agree_before_any_reopen_writes() {
    for tampering in ["old_marker", "legacy_revision", "missing_revision"] {
        let fixture = Fixture::new();
        let mut editor = EditorWorld::open(&fixture.0).unwrap();
        call(
            &mut editor,
            &mut None,
            connect("Existing", None, WORLD_LAYOUT_SCHEMA_VERSION),
        );
        drop(editor);
        match tampering {
            "old_marker" => {
                Connection::open(fixture.database())
                    .unwrap()
                    .execute(
                        "UPDATE identity_realm SET realm='local-development-v1' WHERE id=1",
                        [],
                    )
                    .unwrap();
            }
            "legacy_revision" => {
                fixture.edit(|world| world["scene_layout"] = json!("legacy_v1"));
            }
            "missing_revision" => {
                fixture.edit(|world| {
                    world.as_object_mut().unwrap().remove("scene_layout");
                });
            }
            _ => unreachable!(),
        }
        let before = fixture.durable_state();
        for attempt in [
            EditorWorld::open(&fixture.0).map(drop),
            Server::open_local_development(&fixture.0).map(drop),
            Server::open(fixture.database()).map(drop),
        ] {
            assert!(attempt.is_err(), "accepted tampering: {tampering}");
            assert_eq!(fixture.durable_state(), before, "{tampering}");
        }
    }
}

#[test]
fn injecting_orbital_geometry_into_legacy_realm_does_not_relabel_or_migrate_it() {
    for local in [false, true] {
        let fixture = Fixture::new();
        let mut server = if local {
            Server::open_local_development(&fixture.0).unwrap()
        } else {
            Server::open(fixture.database()).unwrap()
        };
        accepted(server.handle_at(&mut None, connect("Existing", None, 0), 0));
        drop(server);
        fixture.edit(|world| world["scene_layout"] = json!("orbital_v1"));
        let before = fixture.durable_state();
        let attempt = if local {
            Server::open_local_development(&fixture.0)
        } else {
            Server::open(fixture.database())
        };
        match attempt {
            Ok(_) => panic!("orbital geometry was accepted under a legacy marker"),
            Err(error) => assert!(error.to_string().contains("scene layout"), "{error}"),
        }
        assert_eq!(fixture.durable_state(), before);
        assert!(EditorWorld::open(&fixture.0).is_err());
        assert_eq!(fixture.durable_state(), before);
    }
}
