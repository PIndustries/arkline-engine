//! Authoritative exterior branch commands and public effective telemetry.
use super::*;

pub(super) fn execute(
    w: &mut World,
    name: &str,
    enabled: bool,
    revision: u64,
    now: u64,
) -> Result<()> {
    ensure!(
        revision == w.exterior_definition.revision,
        "exterior definition revision conflict: expected {}, current {}",
        revision,
        w.exterior_definition.revision
    );
    let pilot = w.pilots.get(name).context("pilot missing")?;
    ensure!(
        pilot.view.ship_class == ShipClass::Pioneer,
        "exterior controller belongs to your active Pioneer"
    );
    ensure!(
        pilot
            .view
            .station_avatar
            .as_ref()
            .is_none_or(|a| a.onboard_interior),
        "operate exterior controls aboard your Pioneer"
    );
    w.accounts
        .get_mut(name)
        .context("account missing")?
        .pioneer_exterior_enabled = enabled;
    // The preceding advance used the previous circuit load. A full capacitor
    // deliberately keeps old time, so anchor it before changing future loads.
    if let Some(power) = &mut w.pilots.get_mut(name).unwrap().view.power {
        power.updated_at_ms = power.updated_at_ms.max(now);
    }
    Ok(())
}

pub(super) fn anchor(w: &mut World, now: u64) {
    for p in w
        .pilots
        .values_mut()
        .filter(|p| p.view.ship_class == ShipClass::Pioneer)
    {
        if let Some(power) = &mut p.view.power {
            power.updated_at_ms = power.updated_at_ms.max(now);
        }
    }
    for a in w.accounts.values_mut() {
        for ship in a
            .ships
            .iter_mut()
            .filter(|s| s.ship_class == ShipClass::Pioneer)
        {
            if let Some(power) = &mut ship.power {
                power.updated_at_ms = power.updated_at_ms.max(now);
            }
        }
    }
}

fn drive(p: &Pilot, now: u64) -> ExteriorDriveState {
    if p.view.location == Location::Docked {
        return ExteriorDriveState::Off;
    }
    let warp =
        matches!(p.view.location, Location::InTransit { arrives_at_ms, .. } if now < arrives_at_ms);
    let input = p
        .controls
        .as_ref()
        .is_some_and(|i| now < i.expires_at_ms && i.translation.iter().any(|v| v.abs() > 1e-6));
    let acceleration =
        p.view.flight.as_ref().is_some_and(|f| {
            f.updated_at_ms == now && f.acceleration.iter().any(|v| v.abs() > 1e-6)
        });
    if warp || input || acceleration {
        ExteriorDriveState::Active
    } else if p.view.power.as_ref().is_some_and(|p| p.charge_j > 0)
        && !matches!(p.view.location, Location::Jumping { .. })
    {
        ExteriorDriveState::Idle
    } else {
        ExteriorDriveState::Off
    }
}

pub(super) fn view(w: &World, now: u64) -> ExteriorWorldState {
    let load = w.exterior_definition.document.load_w();
    let vessels = w
        .accounts
        .iter()
        .map(|(name, a)| {
            let pilot = &w.pilots[name];
            let active = pilot.view.ship_class == ShipClass::Pioneer;
            let power = if active {
                pilot.view.power.as_ref()
            } else {
                a.ships
                    .iter()
                    .find(|s| s.ship_class == ShipClass::Pioneer)
                    .and_then(|s| s.power.as_ref())
            };
            let powered = a.pioneer_exterior_enabled && power.is_some_and(|p| p.charge_j > 0);
            ExteriorVesselState {
                pilot: name.clone(),
                ship_class: ShipClass::Pioneer,
                vessel_id: a
                    .vessel_id(ShipClass::Pioneer)
                    .expect("validated owned Pioneer identity")
                    .to_owned(),
                enabled: a.pioneer_exterior_enabled,
                powered,
                drive: if active {
                    drive(pilot, now)
                } else {
                    ExteriorDriveState::Off
                },
                load_w: if powered { load } else { 0 },
            }
        })
        .collect();
    ExteriorWorldState {
        definition: w.exterior_definition.clone(),
        vessels,
    }
}

pub(super) fn validate(w: &World) -> Result<()> {
    w.exterior_definition
        .document
        .validate()
        .map_err(anyhow::Error::msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connect(s: &mut Server, name: &str, now: u64) -> Option<String> {
        let mut session = None;
        let response = s.handle_at(
            &mut session,
            Request::Connect {
                supports_room_controls: true,
                world_layout_version: 1,
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
                supports_docked_traversal_v2: false,
                protocol_version: PROTOCOL_VERSION,
                pilot: name.into(),
                resume_token: None,
            },
            now,
        );
        assert!(response.ok, "{}", response.message);
        session
    }
    fn snapshot(s: &mut Server, session: &mut Option<String>, now: u64) -> Snapshot {
        let response = s.handle_at(session, Request::Snapshot, now);
        assert!(response.ok, "{}", response.message);
        response.snapshot.unwrap()
    }
    fn world(s: &Server) -> World {
        let raw: String =
            s.db.query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
                .unwrap();
        serde_json::from_str(&raw).unwrap()
    }
    fn save(s: &Server, w: &World) {
        s.db.execute(
            "UPDATE world SET json=?1 WHERE id=1",
            [serde_json::to_string(w).unwrap()],
        )
        .unwrap();
    }
    #[test]
    fn historical_exterior_world_is_rejected_without_retargeting_persisted_state() {
        let historical_hash = "f8f8cc65772c678a46606c27abe205d62c4e6ab37c350f79a0f3a4cdc0c6f402";
        assert_ne!(PIONEER_MATERIAL_SHA256, historical_hash);
        for editor_mode in [false, true] {
            let directory = std::env::temp_dir().join(format!(
                "arkline-old-exterior-{}-{}-{editor_mode}",
                std::process::id(),
                rand::random::<u64>()
            ));
            let database = directory.join("world.sqlite");
            if editor_mode {
                let mut editor = editor::EditorWorld::open(&directory).unwrap();
                let mut session = None;
                let response = editor.handle(
                    &mut session,
                    Request::Connect {
                        supports_room_controls: true,
                        world_layout_version: 1,
                        supports_fleet_hangar: false,
                        supports_pioneer_interior: true,
                        supports_docked_traversal_v2: false,
                        protocol_version: PROTOCOL_VERSION,
                        pilot: "HistoricalHash".into(),
                        resume_token: None,
                    },
                );
                assert!(response.ok, "{}", response.message);
                editor.exterior_set_enabled(&mut session, false, 0).unwrap();
                editor.step(50).unwrap();
            } else {
                let mut server = Server::open(&database).unwrap();
                let now = now_ms();
                let mut session = connect(&mut server, "HistoricalHash", now);
                let response = server.handle_at(
                    &mut session,
                    Request::SetPioneerExterior {
                        enabled: false,
                        expected_revision: 0,
                    },
                    now,
                );
                assert!(response.ok, "{}", response.message);
            }

            let db = Connection::open(&database).unwrap();
            let raw: String = db
                .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
                .unwrap();
            let mut historical: serde_json::Value = serde_json::from_str(&raw).unwrap();
            historical["exterior_definition"]["document"]["hull_asset_sha256"] =
                historical_hash.into();
            historical["exterior_definition"]["revision"] = 7.into();
            let before = historical.to_string();
            db.execute("UPDATE world SET json=?1 WHERE id=1", [&before])
                .unwrap();
            let clock_before: Option<String> = editor_mode.then(|| {
                db.query_row("SELECT time_ms FROM editor_clock WHERE id=1", [], |r| {
                    r.get(0)
                })
                .unwrap()
            });
            drop(db);

            let assets = directory.join("Assets");
            std::fs::create_dir(&assets).unwrap();
            let exterior_file = assets.join("Pioneer.exterior.json");
            let exterior_bytes =
                serde_json::to_vec(&historical["exterior_definition"]["document"]).unwrap();
            std::fs::write(&exterior_file, &exterior_bytes).unwrap();
            let surface_file = assets.join("Pioneer.surface.json");
            let mut surface = MaterialGraphDocument::pioneer_default();
            surface.binding.asset_sha256 = historical_hash.into();
            let surface_bytes = serde_json::to_vec(&surface).unwrap();
            std::fs::write(&surface_file, &surface_bytes).unwrap();

            let error = if editor_mode {
                editor::EditorWorld::open(&directory).err().expect(
                    "old accepted exterior binding must not be retargeted by opening the editor",
                )
            } else {
                Server::open(&database).err().expect(
                    "old accepted exterior binding must not be retargeted by server startup",
                )
            };
            assert!(error.to_string().contains("hull asset hash"), "{error}");
            let db = Connection::open(&database).unwrap();
            let after: String = db
                .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
                .unwrap();
            assert_eq!(after, before);
            if let Some(clock_before) = clock_before {
                let clock_after: String = db
                    .query_row("SELECT time_ms FROM editor_clock WHERE id=1", [], |r| {
                        r.get(0)
                    })
                    .unwrap();
                assert_eq!(clock_after, clock_before);
            }
            assert_eq!(std::fs::read(&exterior_file).unwrap(), exterior_bytes);
            assert_eq!(std::fs::read(&surface_file).unwrap(), surface_bytes);
            drop(db);
            std::fs::remove_dir_all(directory).unwrap();
        }
    }

    #[test]
    fn legacy_defaults_on_and_only_owned_active_circuit_can_be_operated() {
        let mut s = Server::open(":memory:").unwrap();
        let mut a = connect(&mut s, "A", 0);
        let mut b = connect(&mut s, "B", 0);
        let mut value = serde_json::to_value(world(&s)).unwrap();
        value.as_object_mut().unwrap().remove("exterior_definition");
        for a in value["accounts"].as_object_mut().unwrap().values_mut() {
            a.as_object_mut()
                .unwrap()
                .remove("pioneer_exterior_enabled");
        }
        s.db.execute("UPDATE world SET json=?1 WHERE id=1", [value.to_string()])
            .unwrap();
        let view = snapshot(&mut s, &mut a, 0).exterior.unwrap();
        assert_eq!(view.definition.revision, 0);
        assert!(view.vessels.iter().all(|v| v.enabled && v.powered));
        assert!(
            !s.handle_at(
                &mut None,
                Request::SetPioneerExterior {
                    enabled: false,
                    expected_revision: 0
                },
                0
            )
            .ok
        );
        assert!(
            !s.handle_at(
                &mut a,
                Request::SetPioneerExterior {
                    enabled: false,
                    expected_revision: 1
                },
                0
            )
            .ok
        );
        assert!(
            s.handle_at(
                &mut a,
                Request::SetPioneerExterior {
                    enabled: false,
                    expected_revision: 0
                },
                0
            )
            .ok
        );
        let view = snapshot(&mut s, &mut b, 0).exterior.unwrap();
        assert!(
            !view
                .vessels
                .iter()
                .find(|v| v.pilot == "A")
                .unwrap()
                .powered
        );
        assert!(
            view.vessels
                .iter()
                .find(|v| v.pilot == "B")
                .unwrap()
                .powered
        );
        assert!(
            s.handle_at(
                &mut a,
                Request::SelectShip {
                    ship_class: ShipClass::Prospector
                },
                0
            )
            .ok
        );
        assert!(
            !s.handle_at(
                &mut a,
                Request::SetPioneerExterior {
                    enabled: true,
                    expected_revision: 0
                },
                0
            )
            .ok
        );
        assert!(
            !snapshot(&mut s, &mut b, 0)
                .exterior
                .unwrap()
                .vessels
                .iter()
                .find(|v| v.pilot == "A")
                .unwrap()
                .enabled
        );
    }
    #[test]
    fn exterior_only_and_combined_room_load_use_one_cadence_independent_integrator() {
        for room in [false, true] {
            let mut s = Server::open(":memory:").unwrap();
            let mut session = connect(&mut s, "Energy", 0);
            if room {
                assert!(
                    s.handle_at(
                        &mut session,
                        Request::SetPioneerRoom {
                            action: PioneerRoomCommand::Breaker { closed: true }
                        },
                        0
                    )
                    .ok
                );
            }
            let mut initial = world(&s);
            let p = initial
                .pilots
                .get_mut("Energy")
                .unwrap()
                .view
                .power
                .as_mut()
                .unwrap();
            p.charge_j = 100_000;
            p.updated_at_ms = 0;
            p.regen_remainder_mj = 137;
            let mut fine = initial.clone();
            let mut coarse = initial.clone();
            power::advance(&mut coarse, 1000).unwrap();
            room_controls::advance(&mut coarse, 1000).unwrap();
            for now in 1..=1000 {
                power::advance(&mut fine, now).unwrap();
                room_controls::advance(&mut fine, now).unwrap();
            }
            assert_eq!(
                fine.pilots["Energy"].view.power,
                coarse.pilots["Energy"].view.power
            );
            let room_load = if room {
                ROOM_CONTROLLER_W + ROOM_LIGHT_W
            } else {
                0
            };
            assert_eq!(
                coarse.pilots["Energy"]
                    .view
                    .power
                    .as_ref()
                    .unwrap()
                    .charge_j,
                100_000 + 1200 - default_pioneer_exterior().load_w() - room_load
            );
            // A persisted interval is charged once after reopening/restoration.
            save(&s, &initial);
            let before = snapshot(&mut s, &mut session, 1000);
            let again = snapshot(&mut s, &mut session, 1000);
            assert_eq!(before.pilots[0].power, again.pilots[0].power);
        }
    }
    #[test]
    fn inactive_pioneer_keeps_its_owned_circuit_load_and_identity() {
        let mut s = Server::open(":memory:").unwrap();
        let mut session = connect(&mut s, "Parked", 0);
        let mut initial = world(&s);
        initial
            .pilots
            .get_mut("Parked")
            .unwrap()
            .view
            .power
            .as_mut()
            .unwrap()
            .charge_j = 100_000;
        save(&s, &initial);
        let changed = s.handle_at(
            &mut session,
            Request::SelectShip {
                ship_class: ShipClass::Prospector,
            },
            0,
        );
        assert!(changed.ok, "{}", changed.message);
        let later = s.handle_at(&mut session, Request::Snapshot, 1000);
        assert!(later.ok, "{}", later.message);
        let parked = later
            .assets
            .unwrap()
            .ships
            .into_iter()
            .find(|s| s.ship_class == ShipClass::Pioneer)
            .unwrap();
        assert_eq!(
            parked.power.unwrap().charge_j,
            100_000 + 1200 - default_pioneer_exterior().load_w()
        );
        let state = later.snapshot.unwrap().exterior.unwrap();
        assert_eq!(state.vessels[0].vessel_id, "v1:Parked:Pioneer");
        assert!(state.vessels[0].enabled && state.vessels[0].powered);
        assert_eq!(state.vessels[0].drive, ExteriorDriveState::Off);
    }
    #[test]
    fn drive_uses_fresh_input_and_warp_independently_of_exterior_enable() {
        let mut s = Server::open(":memory:").unwrap();
        let mut session = connect(&mut s, "Drive", 0);
        assert!(
            s.handle_at(
                &mut session,
                Request::SetPioneerExterior {
                    enabled: false,
                    expected_revision: 0
                },
                0
            )
            .ok
        );
        assert!(s.handle_at(&mut session, Request::Undock, 0).ok);
        let response = s.handle_at(
            &mut session,
            Request::PilotInput {
                translation: [1.0, 0.0, 0.0],
                rotation: [0.0; 3],
            },
            0,
        );
        assert!(response.ok, "{}", response.message);
        let view = response.snapshot.unwrap().exterior.unwrap();
        assert_eq!(view.vessels[0].drive, ExteriorDriveState::Active);
        assert!(!view.vessels[0].powered);
        let expired = snapshot(&mut s, &mut session, PILOT_INPUT_LEASE_MS + 1)
            .exterior
            .unwrap();
        // Expired manual input still leaves real braking in the freshly
        // advanced flight state. The branch switch cannot darken that activity.
        assert_eq!(expired.vessels[0].drive, ExteriorDriveState::Active);
        let mut residual = world(&s).pilots["Drive"].clone();
        residual.controls = None;
        let flight = residual.view.flight.as_mut().unwrap();
        flight.mode = NavigationMode::Stop;
        flight.acceleration = [1.0, 0.0, 0.0];
        flight.updated_at_ms = PILOT_INPUT_LEASE_MS;
        assert_eq!(
            drive(&residual, PILOT_INPUT_LEASE_MS + 1),
            ExteriorDriveState::Idle
        );
        residual.view.flight.as_mut().unwrap().updated_at_ms = PILOT_INPUT_LEASE_MS + 1;
        assert_eq!(
            drive(&residual, PILOT_INPUT_LEASE_MS + 1),
            ExteriorDriveState::Active
        );
        let warp = s.handle_at(
            &mut session,
            Request::Warp {
                destination: Destination::Belt,
            },
            PILOT_INPUT_LEASE_MS + 1,
        );
        assert!(warp.ok, "{}", warp.message);
        assert_eq!(
            warp.snapshot.unwrap().exterior.unwrap().vessels[0].drive,
            ExteriorDriveState::Active
        );
    }
}
