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
            ROOM_EMERGENCY_W
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
/// Integrate the finite controller transitions, never an unbounded per-ms loop.
/// Pressure-opening, reserve full/empty and first available capacitor joule are
/// the only load/output discontinuities under the fixed sub-reactor room budget.
fn advance_pair(
    r: &mut PioneerRoomState,
    power: &mut Option<PowerState>,
    occupied: bool,
    now: u64,
    exterior_load: u64,
) -> Result<()> {
    let mut cursor = r.updated_at_ms;
    power::advance_state_with_load(power, ShipClass::Pioneer, cursor, 0)?;
    for _ in 0..8 {
        let powered = power.as_ref().is_some_and(|p| p.charge_j > 0);
        outputs(r, powered, occupied);
        if r.door_open {
            r.pressure_pa = ROOM_PRESSURE_PA;
        }
        if cursor >= now {
            return Ok(());
        }
        let mut dt = now - cursor;
        if !powered {
            let state = power.as_ref().unwrap();
            dt = dt.min(
                (1000 - state.regen_remainder_mj)
                    .div_ceil(state.regen_w - exterior_load)
                    .max(1),
            );
        }
        if powered && r.breaker_closed && r.emergency_remaining_ms < ROOM_EMERGENCY_MS {
            dt = dt.min(ROOM_EMERGENCY_MS - r.emergency_remaining_ms);
        } else if r.emergency_light_on {
            dt = dt.min(r.emergency_remaining_ms);
        }
        if r.vent_running && r.vent_commanded == VentMode::Equalize && r.door_commanded_open {
            let needed = (ROOM_PRESSURE_PA - ROOM_DOOR_DELTA_PA).saturating_sub(r.pressure_pa);
            if needed > 0 {
                dt = dt.min((needed as u64).div_ceil(2));
            }
        }
        let load = exterior_load + if powered { load_w(Some(r)) } else { 0 };
        power::advance_state_with_load(power, ShipClass::Pioneer, cursor + dt, load)?;
        if r.vent_running {
            let delta = (dt as u128 * 2).min(ROOM_PRESSURE_PA as u128) as u32;
            match r.vent_commanded {
                VentMode::Equalize => {
                    r.pressure_pa = r.pressure_pa.saturating_add(delta).min(ROOM_PRESSURE_PA)
                }
                VentMode::Exhaust => r.pressure_pa = r.pressure_pa.saturating_sub(delta),
                VentMode::Hold => {}
            }
        }
        if r.emergency_light_on {
            r.emergency_remaining_ms = r.emergency_remaining_ms.saturating_sub(dt);
        } else if powered && r.breaker_closed {
            r.emergency_remaining_ms = r
                .emergency_remaining_ms
                .saturating_add(dt)
                .min(ROOM_EMERGENCY_MS);
        }
        cursor += dt;
        r.updated_at_ms = cursor;
        if let Some(p) = power.as_mut().filter(|p| p.charge_j == p.capacity_j) {
            p.updated_at_ms = cursor;
        }
    }
    bail!("room integration exceeded finite transition budget")
}
pub(super) fn advance(w: &mut World, now: u64) -> Result<()> {
    exterior_effects::validate(w)?;
    let exterior_load = w.exterior_definition.document.load_w();
    for (name, a) in &mut w.accounts {
        let p = w.pilots.get_mut(name).unwrap();
        let active = p.view.ship_class == ShipClass::Pioneer;
        let occupied = active && doorway_occupied(p);
        let power = if active {
            &mut p.view.power
        } else {
            &mut a
                .ships
                .iter_mut()
                .find(|s| s.ship_class == ShipClass::Pioneer)
                .context("owned Pioneer missing")?
                .power
        };
        let load = if a.pioneer_exterior_enabled {
            exterior_load
        } else {
            0
        };
        let Some(r) = &mut a.pioneer_room else {
            power::advance_state_with_load(power, ShipClass::Pioneer, now, load)?;
            continue;
        };
        advance_pair(r, power, occupied, now, load)?;
        if !a.room_compatible {
            r.interlock = Some(
                "Older session: door held open; reconnect all clients to enable room controls"
                    .into(),
            );
        }
    }
    Ok(())
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

#[cfg(test)]
mod integration_tests {
    use super::*;
    fn fixture() -> (PioneerRoomState, Option<PowerState>) {
        let mut r = PioneerRoomState::uncommissioned("v1:Test:Pioneer".into());
        r.commissioned = true;
        r.breaker_closed = true;
        r.door_open = false;
        r.door_commanded_open = true;
        r.vent_commanded = VentMode::Equalize;
        r.pressure_pa = 81_000;
        r.emergency_remaining_ms = ROOM_EMERGENCY_MS - 1000;
        let spec = power_spec(ShipClass::Pioneer);
        let p = Some(PowerState {
            charge_j: 100_000,
            capacity_j: spec.capacity_j,
            regen_w: spec.regen_w,
            updated_at_ms: 0,
            regen_remainder_mj: 137,
        });
        (r, p)
    }
    #[test]
    fn catalog_operating_loads_match_controller_budget() {
        let (mut r, _) = fixture();
        r.emergency_remaining_ms = ROOM_EMERGENCY_MS - 1;
        let catalog_w = pioneer_room_catalog()
            .equipment
            .iter()
            .filter_map(|e| e.active_load_w)
            .sum::<u64>();
        assert_eq!(load_w(Some(&r)), catalog_w);
    }
    #[test]
    fn reserve_and_pressure_load_boundaries_are_polling_independent() {
        let (mut coarse, mut cp) = fixture();
        let (mut fine, mut fp) = (coarse.clone(), cp.clone());
        advance_pair(&mut coarse, &mut cp, false, 20_000, 0).unwrap();
        for t in 1..=20_000 {
            advance_pair(&mut fine, &mut fp, false, t, 0).unwrap();
        }
        assert_eq!(coarse, fine);
        assert_eq!(cp, fp);
        // 20 s reactor production - 28 W baseline - 96 W for 7.5 s - 10 W for 1 s.
        assert_eq!(cp.as_ref().unwrap().charge_j, 122_710);
        assert_eq!(cp.as_ref().unwrap().regen_remainder_mj, 137);
        assert!(coarse.door_open && !coarse.vent_running);
        advance_pair(&mut coarse, &mut cp, false, 1_000_000, 0).unwrap();
        for t in (21_000..=1_000_000).step_by(1000) {
            advance_pair(&mut fine, &mut fp, false, t, 0).unwrap();
        }
        assert_eq!(coarse, fine);
        assert_eq!(cp, fp);
    }
    #[test]
    fn persisted_elapsed_interval_matches_continuous_ticks_and_is_bounded() {
        let (mut r, mut p) = fixture();
        advance_pair(&mut r, &mut p, false, 500, 0).unwrap();
        let json = serde_json::to_string(&(r.clone(), p.clone())).unwrap();
        let (mut restored, mut rp): (PioneerRoomState, Option<PowerState>) =
            serde_json::from_str(&json).unwrap();
        advance_pair(&mut restored, &mut rp, false, 20_000, 0).unwrap();
        for t in (600..=20_000).step_by(100) {
            advance_pair(&mut r, &mut p, false, t, 0).unwrap();
        }
        assert_eq!((restored, rp), (r, p));
        let (mut r, mut p) = fixture();
        advance_pair(&mut r, &mut p, false, u64::MAX, 0).unwrap();
        assert_eq!(r.updated_at_ms, u64::MAX);
        assert_eq!(p.as_ref().unwrap().charge_j, p.as_ref().unwrap().capacity_j);
    }
    #[test]
    fn power_recovery_and_emergency_expiry_are_cadence_independent() {
        for blackout in [false, true] {
            let (mut coarse, mut cp) = fixture();
            coarse.breaker_closed = !blackout;
            coarse.emergency_remaining_ms = 5;
            cp.as_mut().unwrap().charge_j = 0;
            cp.as_mut().unwrap().regen_remainder_mj = 0;
            let (mut fine, mut fp) = (coarse.clone(), cp.clone());
            advance_pair(&mut coarse, &mut cp, false, 50, 0).unwrap();
            for t in 1..=50 {
                advance_pair(&mut fine, &mut fp, false, t, 0).unwrap();
            }
            assert_eq!(coarse, fine);
            assert_eq!(cp, fp);
        }
    }
}
