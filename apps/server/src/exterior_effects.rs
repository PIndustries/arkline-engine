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
    let automatic = p.view.flight.as_ref().is_some_and(|f| {
        f.updated_at_ms == now
            && !matches!(f.mode, NavigationMode::Manual | NavigationMode::Stop)
            && f.acceleration.iter().any(|v| v.abs() > 1e-6)
    });
    if warp || input || automatic {
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
                vessel_id: format!("v1:{name}:Pioneer"),
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
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
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
        assert_eq!(expired.vessels[0].drive, ExteriorDriveState::Idle);
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
