use super::*;
/// Reactors remain running while parked/offline. Capacity bounds stored energy;
/// u128 intermediate arithmetic bounds arbitrarily long elapsed intervals safely.
pub(super) fn advance_state(
    power: &mut Option<PowerState>,
    class: ShipClass,
    now: u64,
) -> Result<()> {
    advance_state_with_load(power, class, now, 0)
}
pub(super) fn advance_state_with_load(
    power: &mut Option<PowerState>,
    class: ShipClass,
    now: u64,
    load: u64,
) -> Result<()> {
    let spec = power_spec(class);
    ensure!(
        load <= spec.regen_w,
        "combined circuit load exceeds supported reactor budget"
    );
    let state = power.get_or_insert(PowerState {
        charge_j: spec.capacity_j,
        capacity_j: spec.capacity_j,
        regen_w: spec.regen_w,
        updated_at_ms: now,
        regen_remainder_mj: 0,
    });
    ensure!(
        state.capacity_j == spec.capacity_j && state.regen_w == spec.regen_w,
        "power configuration mismatch"
    );
    ensure!(
        state.charge_j <= state.capacity_j && state.regen_remainder_mj < 1000,
        "invalid capacitor state"
    );
    if state.charge_j == state.capacity_j {
        return Ok(());
    }
    if now <= state.updated_at_ms {
        return Ok(());
    }
    let elapsed = now - state.updated_at_ms;
    let energy_mj = elapsed as u128 * state.regen_w.saturating_sub(load) as u128
        + state.regen_remainder_mj as u128;
    let charge = state.charge_j as u128 + energy_mj / 1000;
    state.charge_j = charge.min(state.capacity_j as u128) as u64;
    state.regen_remainder_mj = if state.charge_j == state.capacity_j {
        0
    } else {
        (energy_mj % 1000) as u64
    };
    state.updated_at_ms = now;
    Ok(())
}
pub(super) fn advance(world: &mut World, now: u64) -> Result<()> {
    for p in world.pilots.values_mut() {
        if p.view.ship_class == ShipClass::Pioneer {
            continue;
        }
        advance_state(&mut p.view.power, p.view.ship_class, now)?;
    }
    for account in world.accounts.values_mut() {
        for ship in &mut account.ships {
            if ship.ship_class == ShipClass::Pioneer {
                continue;
            }
            advance_state(&mut ship.power, ship.ship_class, now)?;
        }
    }
    Ok(())
}
/// All callers advance the reactor before admission. Debit only if fully funded.
pub(super) fn spend(power: &mut Option<PowerState>, cost: u64, now: u64) -> bool {
    let Some(state) = power else { return false };
    if state.charge_j < cost {
        return false;
    }
    // A full capacitor can retain its old timestamp without writing on every
    // idle snapshot. Reset it on the first debit so idle time cannot refill a debit.
    if state.charge_j == state.capacity_j {
        state.updated_at_ms = now;
        state.regen_remainder_mj = 0;
    }
    state.charge_j -= cost;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    fn charged_at(charge: u64, now: u64) -> Option<PowerState> {
        let spec = power_spec(ShipClass::Pioneer);
        Some(PowerState {
            charge_j: charge,
            capacity_j: spec.capacity_j,
            regen_w: spec.regen_w,
            updated_at_ms: now,
            regen_remainder_mj: 0,
        })
    }
    #[test]
    fn fractional_regeneration_is_independent_of_polling_frequency() {
        let mut fine = charged_at(10_000, 1000);
        let mut coarse = fine.clone();
        for now in 1001..=1500 {
            advance_state(&mut fine, ShipClass::Pioneer, now).unwrap();
        }
        advance_state(&mut coarse, ShipClass::Pioneer, 1500).unwrap();
        assert_eq!(fine, coarse);
        assert_eq!(fine.unwrap().charge_j, 10_600);
    }
    #[test]
    fn full_idle_time_cannot_refund_new_spending_and_extreme_elapsed_is_bounded() {
        let spec = power_spec(ShipClass::Pioneer);
        let mut state = charged_at(spec.capacity_j, 0);
        advance_state(&mut state, ShipClass::Pioneer, 1_000_000).unwrap();
        assert!(spend(&mut state, spec.warp_cost_j, 1_000_000));
        advance_state(&mut state, ShipClass::Pioneer, 1_000_000).unwrap();
        assert_eq!(
            state.as_ref().unwrap().charge_j,
            spec.capacity_j - spec.warp_cost_j
        );
        advance_state(&mut state, ShipClass::Pioneer, u64::MAX).unwrap();
        assert_eq!(state.as_ref().unwrap().charge_j, spec.capacity_j);
        assert_eq!(state.unwrap().regen_remainder_mj, 0);
    }
    #[test]
    fn missing_schema_initializes_at_current_time_and_future_timestamps_do_not_regen() {
        let mut state = None;
        advance_state(&mut state, ShipClass::Pioneer, 1234).unwrap();
        assert_eq!(state.as_ref().unwrap().updated_at_ms, 1234);
        assert!(spend(&mut state, 1000, 1234));
        let before = state.clone();
        advance_state(&mut state, ShipClass::Pioneer, 1233).unwrap();
        assert_eq!(state, before);
    }
    #[test]
    fn boarding_keeps_charge_with_each_physical_hull() {
        let mut server = Server::open(":memory:").unwrap();
        let mut session = None;
        assert!(
            server
                .handle_at(
                    &mut session,
                    Request::Connect {
                        supports_room_controls: false,
                        world_layout_version: 1,
                        supports_fleet_hangar: false,
                        supports_pioneer_interior: true,
                        supports_docked_traversal_v2: false,
                        protocol_version: PROTOCOL_VERSION,
                        pilot: "PowerPilot".into(),
                        resume_token: None
                    },
                    0
                )
                .ok
        );
        let json: String = server
            .db
            .query_row("SELECT json FROM world WHERE id=1", [], |row| row.get(0))
            .unwrap();
        let mut world: serde_json::Value = serde_json::from_str(&json).unwrap();
        world["pilots"]["PowerPilot"]["view"]["power"]["charge_j"] = serde_json::json!(1000);
        server
            .db
            .execute("UPDATE world SET json=?1 WHERE id=1", [world.to_string()])
            .unwrap();
        let other = server.handle_at(
            &mut session,
            Request::SelectShip {
                ship_class: ShipClass::Prospector,
            },
            0,
        );
        assert!(other.ok);
        assert_eq!(
            other.snapshot.unwrap().pilots[0]
                .power
                .as_ref()
                .unwrap()
                .charge_j,
            power_spec(ShipClass::Prospector).capacity_j
        );
        let parked = other
            .assets
            .unwrap()
            .ships
            .into_iter()
            .find(|s| s.ship_class == ShipClass::Pioneer)
            .unwrap();
        assert_eq!(parked.power.unwrap().charge_j, 1000);
        let back = server.handle_at(
            &mut session,
            Request::SelectShip {
                ship_class: ShipClass::Pioneer,
            },
            0,
        );
        assert!(back.ok);
        assert_eq!(
            back.snapshot.unwrap().pilots[0]
                .power
                .as_ref()
                .unwrap()
                .charge_j,
            1000
        );
        let later = server
            .handle_at(&mut session, Request::Snapshot, 500)
            .snapshot
            .unwrap();
        assert_eq!(
            later.pilots[0].power.as_ref().unwrap().charge_j,
            1600 - default_pioneer_exterior().load_w() / 2
        );
    }
}
