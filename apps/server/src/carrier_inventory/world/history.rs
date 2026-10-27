//! Reopen verification binds every payload unit and pump cooldown to the retained
//! hash-linked economic history. Current physical eligibility is checked by the
//! runtime validator, not retroactively imposed on completed services.
use super::*;
pub(super) fn validate(w: &World, ledger: &[LedgerEntry]) -> Result<()> {
    let mut previous = GENESIS_HASH.to_owned();
    let mut timestamp = 0;
    for (index, e) in ledger.iter().enumerate() {
        ensure!(
            e.sequence == index as u64 + 1
                && e.previous_hash == previous
                && e.hash == e.calculate_hash()
                && e.timestamp_ms >= timestamp
                && e.ruleset_version == CARRIER_INVENTORY_RULESET_VERSION,
            "Carrier ledger chain/ruleset invalid"
        );
        if let EconomicEvent::CarrierInventory { record } = &e.event {
            let (pilot, vessel) = match record {
                CarrierInventoryLedgerEvent::Initialized { pilot, binding, .. } => {
                    (pilot, &binding.vessel_id)
                }
                CarrierInventoryLedgerEvent::Extracted {
                    pilot, vessel_id, ..
                }
                | CarrierInventoryLedgerEvent::Transferred {
                    pilot, vessel_id, ..
                }
                | CarrierInventoryLedgerEvent::Destroyed {
                    pilot, vessel_id, ..
                } => (pilot, vessel_id),
            };
            ensure!(
                state(w, pilot).is_some_and(|s| &s.inventory.binding.vessel_id == vessel),
                "Unowned carrier ledger event"
            );
        }
        previous = e.hash.clone();
        timestamp = e.timestamp_ms;
    }
    for (name, account) in &w.accounts {
        let s = account
            .carrier_inventory
            .as_ref()
            .context("Carrier history lacks owner")?;
        // ore, last successful cycle, last extraction time, destruction time
        let mut balances: BTreeMap<u64, (u64, u64, u64, Option<u64>)> = s
            .ore
            .holds
            .iter()
            .map(|h| (h.drone_id, (0, 0, s.inventory.initialized_at_ms, None)))
            .collect();
        let mut destroyed = BTreeMap::new();
        let mut receipts = BTreeSet::new();
        let mut lost = 0u64;
        let mut last_transfer = None;
        for e in ledger {
            let EconomicEvent::CarrierInventory { record } = &e.event else {
                ensure!(
                    !matches!(e.event, EconomicEvent::DroneCargo { .. }),
                    "Legacy cargo event in finite carrier history"
                );
                continue;
            };
            match record {
                CarrierInventoryLedgerEvent::Initialized { .. } => {}
                CarrierInventoryLedgerEvent::Extracted {
                    pilot,
                    vessel_id,
                    drone_id,
                    cycle_id,
                    target_id,
                    cycle_started_at_ms,
                    units,
                    job_id,
                } if pilot == name => {
                    ensure!(
                        vessel_id == &s.inventory.binding.vessel_id
                            && job_id.is_none()
                            && (1..=DRONE_MINING_YIELD).contains(units)
                            && w.asteroids.iter().any(|a| a.id == *target_id)
                            && cycle_started_at_ms
                                .checked_add(DRONE_MINING_CYCLE_MS)
                                .is_some_and(|t| t <= e.timestamp_ms),
                        "Invalid carrier extraction source/rate"
                    );
                    let b = balances
                        .get_mut(drone_id)
                        .context("Extraction foreign/non-Mining identity")?;
                    ensure!(
                        b.3.is_none() && *cycle_id > b.1 && *cycle_started_at_ms >= b.2,
                        "Reused/overlapping/dead carrier mining cycle"
                    );
                    b.0 = b.0.checked_add(*units).context("Carrier hold overflow")?;
                    ensure!(
                        b.0 <= MINING_DRONE_CARGO_CAPACITY,
                        "Carrier hold exceeds100"
                    );
                    b.1 = *cycle_id;
                    b.2 = e.timestamp_ms;
                }
                CarrierInventoryLedgerEvent::Transferred {
                    pilot,
                    vessel_id,
                    drone_id,
                    physical_operation_id,
                    berth_id,
                    service_id,
                    service_started_at_ms,
                    requested_units,
                    sequence,
                    units,
                } if pilot == name => {
                    let b = balances
                        .get_mut(drone_id)
                        .context("Transfer foreign/non-Mining identity")?;
                    let a = s
                        .ore
                        .services
                        .iter()
                        .find(|a| a.service_id == *service_id)
                        .context("Ledger transfer lost retained attempt")?;
                    ensure!(
                        vessel_id == &s.inventory.binding.vessel_id
                            && a.drone_id == *drone_id
                            && a.physical_operation_id == *physical_operation_id
                            && a.berth_id == *berth_id
                            && a.accepted_at_ms == *service_started_at_ms
                            && a.requested_units == *requested_units
                            && *service_started_at_ms >= b.2
                            && b.3.is_none()
                            && (1..=DRONE_DROPOFF_BATCH_UNITS).contains(units)
                            && service_started_at_ms
                                .checked_add(DRONE_DROPOFF_BATCH_MS)
                                .is_some_and(|t| t <= e.timestamp_ms)
                            && last_transfer.is_none_or(|last: u64| last
                                .checked_add(DRONE_DROPOFF_BATCH_MS)
                                .is_some_and(|t| t <= e.timestamp_ms)),
                        "Transfer binding/liveness/global cadence differs"
                    );
                    let r = a
                        .receipts
                        .iter()
                        .find(|r| r.sequence == *sequence)
                        .context("Transfer receipt missing")?;
                    ensure!(
                        r.units == *units
                            && r.committed_at_ms == e.timestamp_ms
                            && r.ledger_sequence == e.sequence
                            && r.ledger_hash == e.hash
                            && receipts.insert(e.sequence),
                        "Transfer receipt differs from ledger"
                    );
                    if *sequence == 1 {
                        ensure!(
                            b.0 == *requested_units,
                            "Service did not begin with actual entire payload"
                        );
                    }
                    b.0 = b.0.checked_sub(*units).context("Transfer duplicated ore")?;
                    last_transfer = Some(e.timestamp_ms);
                }
                CarrierInventoryLedgerEvent::Destroyed {
                    pilot,
                    vessel_id,
                    drone_id,
                    destroyed_at_ms,
                    lost_units,
                } if pilot == name => {
                    ensure!(
                        vessel_id == &s.inventory.binding.vessel_id
                            && s.inventory.entry(*drone_id).is_some()
                            && *destroyed_at_ms == e.timestamp_ms
                            && destroyed.insert(*drone_id, *destroyed_at_ms).is_none(),
                        "Foreign/duplicate destruction"
                    );
                    if let Some(b) = balances.get_mut(drone_id) {
                        ensure!(
                            b.3.is_none() && b.0 == *lost_units,
                            "Destruction sink differs from same-ID payload"
                        );
                        b.0 = 0;
                        b.3 = Some(*destroyed_at_ms);
                    } else {
                        ensure!(*lost_units == 0, "Non-Mining cargo loss");
                    }
                    lost = lost.checked_add(*lost_units).context("Sink overflow")?;
                }
                _ => {}
            }
        }
        ensure!(
            s.ore.lost_ore_units == lost
                && s.ore.last_committed_at_ms == last_transfer
                && receipts.len()
                    == s.ore
                        .services
                        .iter()
                        .map(|a| a.receipts.len())
                        .sum::<usize>(),
            "Retained sink/receipts/cooldown differs from history"
        );
        for h in &s.ore.holds {
            let b = balances[&h.drone_id];
            ensure!(
                h.ore_units == b.0 && h.destroyed_at_ms == b.3 && h.cycle_high_watermark >= b.1,
                "Carrier hold differs from ledger"
            );
            if let Some(c) = &h.cycle {
                ensure!(
                    c.cycle_id > b.1 && c.started_at_ms >= b.2,
                    "Pending cycle predates history"
                );
            }
        }
        for a in &s.ore.services {
            ensure!(
                a.accepted_at_ms >= s.inventory.initialized_at_ms,
                "Attempt predates source genesis"
            );
        }
        for e in &s.inventory.entries {
            ensure!(
                match (&e.location, destroyed.get(&e.drone_id)) {
                    (CarrierDroneLocation::Destroyed { at_ms }, Some(t)) =>
                        at_ms == t && e.hit_points == 0,
                    (CarrierDroneLocation::Destroyed { .. }, None) | (_, Some(_)) => false,
                    _ => e.hit_points > 0,
                },
                "Finite destruction differs from ledger"
            );
        }
    }
    let now = w
        .pilots
        .iter()
        .flat_map(|(name, p)| {
            let s = state(w, name).unwrap();
            [
                s.inventory.initialized_at_ms,
                // Idle physical samples can advance while flight/power and
                // inventory clocks remain unchanged. This is persisted time,
                // not wall time or an actuator advance during preflight.
                w.accounts[name]
                    .cockpit_mechanisms
                    .as_ref()
                    .map_or(0, |m| m.turrets.updated_at_ms),
                p.view.flight.as_ref().map_or(0, |f| f.updated_at_ms),
                p.view.power.as_ref().map_or(0, |p| p.updated_at_ms),
                p.view
                    .drones
                    .iter()
                    .map(|d| d.updated_at_ms)
                    .max()
                    .unwrap_or(0),
                s.inventory
                    .operations
                    .iter()
                    .map(|o| o.updated_at_ms)
                    .max()
                    .unwrap_or(0),
                s.runs
                    .values()
                    .map(|r| r.confirmed_at_ms)
                    .max()
                    .unwrap_or(0),
            ]
        })
        .chain(ledger.last().map(|e| e.timestamp_ms))
        .max()
        .unwrap_or(0);
    // Read-only preflight: reject corrupt physical/aim state before any schema
    // migration, restart hold or snapshot projection can rewrite the database.
    crate::turret_stow::validate(w, now)?;
    let snapshot = crate::project_snapshot(w, ledger, now, vec![])?;
    arkline_verifier::verify(
        &LedgerExport {
            protocol_version: PROTOCOL_VERSION,
            entries: ledger.to_vec(),
            snapshot,
        },
        None,
    )
    .context("Carrier full economic replay failed")?;
    Ok(())
}
