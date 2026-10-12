//! Bounded first-room controller. No arbitrary programs or whole-ship air simulation.
use super::*;
pub(super) fn load_w(room: Option<&PioneerRoomState>) -> u64 {
    let Some(r) = room.filter(|r| r.commissioned && r.breaker_closed) else {
        return 0;
    };
    ROOM_CONTROLLER_W
        + if r.light_commanded { ROOM_LIGHT_W } else { 0 }
        + if r.vent_commanded != VentMode::Hold && !r.door_open {
            ROOM_VENT_W
        } else {
            0
        }
        + if r.emergency_remaining_ms < ROOM_EMERGENCY_MS {
            10
        } else {
            0
        }
}
fn doorway_occupied(p: &Pilot) -> bool {
    p.view.station_avatar.as_ref().is_some_and(|a| {
        a.onboard_interior && a.interior_deck == 1 && {
            let d = PIONEER_CONNECTED_ROOM;
            let r = STATION_AVATAR_RADIUS_M;
            a.position[0] > d.door_min[0] - r
                && a.position[0] < d.door_max[0] + r
                && a.position[2] > d.door_min[2] - r
                && a.position[2] < d.door_max[2] + r
        }
    })
}
fn outputs(r: &mut PioneerRoomState, powered: bool, occupied: bool) {
    let main = powered && r.breaker_closed;
    r.light_on = main && r.light_commanded;
    r.interlock = None;
    if r.door_commanded_open {
        if ROOM_PRESSURE_PA.abs_diff(r.pressure_pa) > ROOM_DOOR_DELTA_PA {
            r.interlock = Some("Pressure difference exceeds 5 kPa".into());
        } else if main || r.door_manual {
            r.door_open = true;
        } else {
            r.interlock = Some("Door motor has no power; manual actuation available".into());
        }
    } else if occupied {
        r.interlock = Some("Doorway occupied".into());
    } else if main || r.door_manual {
        r.door_open = false;
    } else {
        r.interlock = Some("Door motor has no power; manual actuation available".into());
    }
    r.vent_running = main && r.vent_commanded != VentMode::Hold && !r.door_open;
    if r.vent_commanded != VentMode::Hold && r.door_open {
        r.interlock = Some("Close the door before ventilation".into());
    }
    r.emergency_light_on = !main && r.emergency_remaining_ms > 0;
}
pub(super) fn advance(w: &mut World, now: u64) {
    for (name, a) in &mut w.accounts {
        let Some(r) = &mut a.pioneer_room else {
            continue;
        };
        let p = &w.pilots[name];
        let power = if p.view.ship_class == ShipClass::Pioneer {
            p.view.power.as_ref()
        } else {
            a.ships
                .iter()
                .find(|s| s.ship_class == ShipClass::Pioneer)
                .and_then(|s| s.power.as_ref())
        };
        let powered = power.is_some_and(|p| p.charge_j > 0);
        let elapsed = now.saturating_sub(r.updated_at_ms);
        if r.vent_running {
            let delta = (elapsed as u128 * 2).min(ROOM_PRESSURE_PA as u128) as u32;
            match r.vent_commanded {
                VentMode::Equalize => {
                    r.pressure_pa = r.pressure_pa.saturating_add(delta).min(ROOM_PRESSURE_PA)
                }
                VentMode::Exhaust => r.pressure_pa = r.pressure_pa.saturating_sub(delta),
                VentMode::Hold => {}
            }
        }
        if r.door_open {
            r.pressure_pa = ROOM_PRESSURE_PA;
        }
        if r.emergency_light_on {
            r.emergency_remaining_ms = r.emergency_remaining_ms.saturating_sub(elapsed);
        } else if powered && r.breaker_closed {
            r.emergency_remaining_ms = r
                .emergency_remaining_ms
                .saturating_add(elapsed)
                .min(ROOM_EMERGENCY_MS);
        }
        r.updated_at_ms = r.updated_at_ms.max(now);
        outputs(
            r,
            powered,
            p.view.ship_class == ShipClass::Pioneer && doorway_occupied(p),
        );
        if !a.room_compatible {
            r.interlock = Some(
                "Older session: door held open; reconnect all clients to enable room controls"
                    .into(),
            );
        }
    }
}
pub(super) fn execute(
    w: &mut World,
    name: &str,
    command: PioneerRoomCommand,
    now: u64,
) -> Result<()> {
    ensure!(
        w.accounts[name].room_compatible,
        "close older clients and reconnect all sessions before operating room controls"
    );
    let p = &w.pilots[name];
    ensure!(
        p.view.ship_class == ShipClass::Pioneer,
        "room controller belongs to your active Pioneer"
    );
    ensure!(
        p.view
            .station_avatar
            .as_ref()
            .is_none_or(|a| a.onboard_interior && a.interior_deck == 1),
        "use engineering aboard or inside Pioneer deck one"
    );
    if matches!(command, PioneerRoomCommand::Door { open: false, .. }) {
        ensure!(!doorway_occupied(p), "doorway occupied");
    }
    let occupied = doorway_occupied(p);
    let powered = p.view.power.as_ref().is_some_and(|p| p.charge_j > 0);
    let a = w.accounts.get_mut(name).unwrap();
    let r = a
        .pioneer_room
        .get_or_insert_with(|| PioneerRoomState::uncommissioned(format!("v1:{name}:Pioneer")));
    r.commissioned = true;
    r.updated_at_ms = r.updated_at_ms.max(now);
    match command {
        PioneerRoomCommand::Breaker { closed } => r.breaker_closed = closed,
        PioneerRoomCommand::Light { on } => r.light_commanded = on,
        PioneerRoomCommand::Vent { mode } => r.vent_commanded = mode,
        PioneerRoomCommand::Door { open, manual } => {
            r.door_commanded_open = open;
            r.door_manual = manual;
        }
    }
    outputs(r, powered, occupied);
    // Anchor full capacitor time before changing its subsequent room-load budget.
    if let Some(power) = &mut w.pilots.get_mut(name).unwrap().view.power {
        power.updated_at_ms = power.updated_at_ms.max(now);
    }
    Ok(())
}
pub(super) fn door_obstacle(w: &World, name: &str) -> Option<StationObstacle> {
    let p = &w.pilots[name];
    (p.view.ship_class == ShipClass::Pioneer
        && p.view
            .station_avatar
            .as_ref()
            .is_some_and(|a| a.onboard_interior && a.interior_deck == 1)
        && w.accounts[name]
            .pioneer_room
            .as_ref()
            .is_some_and(|r| !r.door_open))
    .then(|| {
        let d = PIONEER_CONNECTED_ROOM;
        StationObstacle {
            kind: StationObstacleKind::Wall,
            min: d.door_min,
            max: d.door_max,
        }
    })
}
pub(super) fn validate(w: &World) -> Result<()> {
    for (name, a) in &w.accounts {
        if let Some(r) = &a.pioneer_room {
            ensure!(
                r.commissioned
                    && r.vessel_id == format!("v1:{name}:Pioneer")
                    && r.pressure_pa <= ROOM_PRESSURE_PA
                    && r.emergency_remaining_ms <= ROOM_EMERGENCY_MS,
                "invalid Pioneer room state"
            );
        }
    }
    Ok(())
}

pub(super) fn legacy_recovery(w: &mut World, name: &str, now: u64) {
    if let Some(r) = &mut w.accounts.get_mut(name).unwrap().pioneer_room {
        // Compatibility recovery is an explicit scalar-model reset after authentication,
        // never a physical claim of instantaneous gas transfer.
        r.pressure_pa = ROOM_PRESSURE_PA;
        r.door_open = true;
        r.door_commanded_open = true;
        r.door_manual = true;
        r.vent_commanded = VentMode::Hold;
        r.vent_running = false;
        r.updated_at_ms = r.updated_at_ms.max(now);
        r.interlock = Some(
            "Older session: door held open; reconnect all clients to enable room controls".into(),
        );
    }
}
