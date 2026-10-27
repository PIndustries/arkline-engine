//! Independent replay of Arkline economic records. No server or database code.
mod carrier;
pub mod physical_cargo;
use anyhow::{bail, ensure, Context, Result};
use arkline_protocol::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Checkpoint {
    pub sequence: u64,
    pub hash: String,
}

#[derive(Debug)]
pub struct Report {
    pub accounts: usize,
    pub trades: u64,
    pub total_credits: u64,
    pub belt_ore: u64,
    pub stored_ore: u64,
    pub head: Checkpoint,
}

#[derive(Default)]
struct Account {
    credits: u64,
    ore: u64,
    active_class: Option<ShipClass>,
    inactive: BTreeMap<usize, (u32, u64)>,
}

struct Corporation {
    owner: String,
    members: BTreeSet<String>,
    invitations: BTreeSet<String>,
    storage: BTreeMap<u32, u64>,
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 24
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
fn valid_system(id: u32) -> Result<()> {
    ensure!(
        systems().iter().any(|s| s.id == id),
        "unknown storage system"
    );
    Ok(())
}
fn hull_key(class: ShipClass) -> usize {
    ALL_SHIP_CLASSES.iter().position(|c| *c == class).unwrap()
}
fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).context("integer addition overflow")
}
fn sub(a: u64, b: u64) -> Result<u64> {
    a.checked_sub(b).context("insufficient balance or quantity")
}
fn cargo(accounts: &BTreeMap<String, Account>, name: &str) -> Result<u64> {
    Ok(accounts.get(name).context("unknown pilot")?.ore)
}

// These balances are reconstructed only from ledger events, independently of
// server state transitions. Claims are checked at the snapshot, never minted.
#[derive(Default)]
struct ReplayedDrone {
    ore: u64,
    last_cycle: u64,
    last_finished_at: u64,
    destroyed: bool,
}
struct ReplayedTransfer {
    drone: u64,
    started: u64,
    requested: u64,
    sequence: u64,
    committed: u64,
    last_time: u64,
    receipts: Vec<DroneOreTransferReceipt>,
}
#[derive(Default)]
struct DroneReplay {
    vessels: BTreeMap<(String, String), BTreeMap<u64, ReplayedDrone>>,
    transfers: BTreeMap<(String, String, u64), ReplayedTransfer>,
    last_receiver_batch: BTreeMap<(String, String), (u64, u64)>,
    lost: u64,
}
impl DroneReplay {
    fn pilot_ore(&self, pilot: &str) -> Result<u64> {
        self.vessels
            .iter()
            .filter(|((p, _), _)| p == pilot)
            .flat_map(|(_, ds)| ds.values())
            .try_fold(0, |sum, d| add(sum, d.ore))
    }
    fn total(&self) -> Result<u64> {
        self.vessels
            .values()
            .flat_map(|ds| ds.values())
            .try_fold(0, |sum, d| add(sum, d.ore))
    }
    fn apply(
        &mut self,
        record: &DroneCargoLedgerEvent,
        entry: &LedgerEntry,
        registration: &DroneCargoRegistration,
        accounts: &mut BTreeMap<String, Account>,
        belt: &mut u64,
    ) -> Result<()> {
        let now = entry.timestamp_ms;
        match record {
            DroneCargoLedgerEvent::Initialized {
                pilot,
                vessel_id,
                registration: declared,
                drone_ids,
                capacity_units,
            } => {
                ensure!(
                    accounts
                        .get(pilot)
                        .context("unknown cargo owner")?
                        .active_class
                        == Some(ShipClass::Pioneer),
                    "cargo requires Pioneer"
                );
                ensure!(
                    !vessel_id.is_empty() && vessel_id.len() <= 256,
                    "invalid cargo vessel"
                );
                ensure!(
                    declared == registration && *capacity_units == MINING_DRONE_CARGO_CAPACITY,
                    "cargo registration or capacity mismatch"
                );
                ensure!(
                    drone_ids.len() == DRONE_CLASS_CAP,
                    "incorrect finite cargo roster"
                );
                let mut drones = BTreeMap::new();
                for id in drone_ids {
                    ensure!(
                        *id > 0
                            && drones
                                .insert(
                                    *id,
                                    ReplayedDrone {
                                        last_finished_at: now,
                                        ..ReplayedDrone::default()
                                    }
                                )
                                .is_none(),
                        "duplicate or zero cargo drone"
                    );
                }
                ensure!(
                    !self
                        .vessels
                        .keys()
                        .any(|(p, v)| p == pilot || v == vessel_id),
                    "vessel cargo already initialized"
                );
                self.vessels
                    .insert((pilot.clone(), vessel_id.clone()), drones);
            }
            DroneCargoLedgerEvent::Extracted {
                pilot,
                vessel_id,
                drone_id,
                cycle_id,
                target_id,
                cycle_started_at_ms,
                units,
                job_id,
            } => {
                ensure!(
                    (1..=DRONE_MINING_YIELD).contains(units),
                    "invalid drone extraction quantity"
                );
                ensure!(
                    *target_id > 0
                        && asteroid_catalog(INITIAL_BELT_ORE)
                            .iter()
                            .any(|a| a.id == *target_id),
                    "unknown extraction target"
                );
                ensure!(
                    job_id.is_none_or(|id| id > 0),
                    "zero extraction job identity"
                );
                ensure!(
                    add(*cycle_started_at_ms, DRONE_MINING_CYCLE_MS)? <= now,
                    "extraction before declared cycle maturity"
                );
                let d = self
                    .vessels
                    .get_mut(&(pilot.clone(), vessel_id.clone()))
                    .context("uninitialized cargo vessel")?
                    .get_mut(drone_id)
                    .context("unowned cargo drone")?;
                ensure!(
                    !d.destroyed && *cycle_id > d.last_cycle,
                    "destroyed drone or reused extraction cycle"
                );
                ensure!(
                    *cycle_started_at_ms >= d.last_finished_at,
                    "overlapping or pre-initialization mining cycle"
                );
                d.last_finished_at = now;
                d.ore = add(d.ore, *units)?;
                ensure!(
                    d.ore <= MINING_DRONE_CARGO_CAPACITY,
                    "drone cargo capacity exceeded"
                );
                d.last_cycle = *cycle_id;
                *belt = sub(*belt, *units)?;
            }
            DroneCargoLedgerEvent::Transferred {
                pilot,
                vessel_id,
                drone_id,
                operation_id,
                operation_started_at_ms,
                requested_units,
                sequence,
                units,
            } => {
                ensure!(
                    *operation_id > 0 && (1..=DRONE_DROPOFF_BATCH_UNITS).contains(units),
                    "invalid drop-off batch"
                );
                ensure!(
                    (1..=MINING_DRONE_CARGO_CAPACITY).contains(requested_units),
                    "invalid drop-off request quantity"
                );
                ensure!(
                    add(*operation_started_at_ms, DRONE_DROPOFF_BATCH_MS)? <= now,
                    "drop-off before minimum initial spacing"
                );
                let d = self
                    .vessels
                    .get_mut(&(pilot.clone(), vessel_id.clone()))
                    .context("uninitialized transfer vessel")?
                    .get_mut(drone_id)
                    .context("unowned transfer drone")?;
                ensure!(!d.destroyed, "destroyed drone cannot transfer");
                let vessel_key = (pilot.clone(), vessel_id.clone());
                if let Some((last_operation, last_time)) = self.last_receiver_batch.get(&vessel_key)
                {
                    ensure!(
                        *operation_id >= *last_operation,
                        "older receiver operation reopened"
                    );
                    ensure!(
                        add(*last_time, DRONE_DROPOFF_BATCH_MS)? <= now,
                        "receiver batches accelerated across operations"
                    );
                }
                if !self
                    .transfers
                    .contains_key(&(pilot.clone(), vessel_id.clone(), *operation_id))
                {
                    ensure!(
                        *requested_units == d.ore,
                        "first transfer request differs from owned payload"
                    );
                }
                self.last_receiver_batch
                    .insert(vessel_key, (*operation_id, now));
                let op = self
                    .transfers
                    .entry((pilot.clone(), vessel_id.clone(), *operation_id))
                    .or_insert(ReplayedTransfer {
                        drone: *drone_id,
                        started: *operation_started_at_ms,
                        requested: *requested_units,
                        sequence: 0,
                        committed: 0,
                        last_time: *operation_started_at_ms,
                        receipts: vec![],
                    });
                ensure!(
                    op.drone == *drone_id
                        && op.started == *operation_started_at_ms
                        && op.requested == *requested_units,
                    "drop-off operation identity changed"
                );
                ensure!(
                    *sequence == add(op.sequence, 1)?,
                    "reused or skipped drop-off batch"
                );
                ensure!(
                    add(op.last_time, DRONE_DROPOFF_BATCH_MS)? <= now,
                    "accelerated drop-off batches"
                );
                op.committed = add(op.committed, *units)?;
                ensure!(
                    op.committed <= op.requested,
                    "drop-off exceeds immutable request"
                );
                op.sequence = *sequence;
                op.last_time = now;
                op.receipts.push(DroneOreTransferReceipt {
                    sequence: *sequence,
                    units: *units,
                    committed_at_ms: now,
                    ledger_sequence: entry.sequence,
                    ledger_hash: entry.hash.clone(),
                });
                d.ore = sub(d.ore, *units)?;
                let account = accounts.get_mut(pilot).context("unknown transfer owner")?;
                ensure!(
                    account.active_class == Some(ShipClass::Pioneer),
                    "transfer to wrong active hull"
                );
                account.ore = add(account.ore, *units)?;
            }
            DroneCargoLedgerEvent::Destroyed {
                pilot,
                vessel_id,
                drone_id,
                destroyed_at_ms,
                lost_units,
            } => {
                ensure!(*destroyed_at_ms == now, "destroyed timestamp mismatch");
                let d = self
                    .vessels
                    .get_mut(&(pilot.clone(), vessel_id.clone()))
                    .context("uninitialized destroyed vessel")?
                    .get_mut(drone_id)
                    .context("unowned destroyed drone")?;
                ensure!(
                    !d.destroyed && d.ore == *lost_units,
                    "duplicate destruction or incorrect loss"
                );
                self.lost = add(self.lost, *lost_units)?;
                d.ore = 0;
                d.destroyed = true;
            }
        }
        Ok(())
    }
    fn snapshot(&self, snapshot: &Snapshot, registration: &DroneCargoRegistration) -> Result<()> {
        let totals = snapshot
            .drone_cargo_totals
            .as_ref()
            .context("missing drone cargo totals")?;
        ensure!(
            totals.schema_version == DRONE_CARGO_SCHEMA_VERSION
                && totals.drone_ore_units == self.total()?
                && totals.lost_ore_units == self.lost,
            "drone cargo totals mismatch"
        );
        let mut seen = BTreeSet::new();
        for pilot in &snapshot.pilots {
            let cargo = pilot
                .drone_cargo
                .as_ref()
                .context("cargo world pilot lacks initialization")?;
            ensure!(
                cargo.schema_version == DRONE_CARGO_SCHEMA_VERSION
                    && &cargo.binding.registration == registration,
                "snapshot cargo registration mismatch"
            );
            ensure!(
                pilot.ship_class == ShipClass::Pioneer,
                "cargo on non-Pioneer hull"
            );
            let key = (pilot.name.clone(), cargo.binding.vessel_id.clone());
            let drones = self
                .vessels
                .get(&key)
                .context("snapshot cargo was not initialized")?;
            ensure!(seen.insert(key), "duplicate snapshot cargo vessel");
            let stock = pilot
                .drone_bay
                .as_ref()
                .context("missing cargo stock identity")?;
            ensure!(
                stock.binding == registration.base.bind(cargo.binding.vessel_id.clone()),
                "cargo stock vessel mismatch"
            );
            let actual_vessel = pilot
                .flight
                .as_ref()
                .and_then(|f| f.docking.as_ref())
                .context("missing physical cargo vessel")?;
            ensure!(
                actual_vessel.host_vessel_id == cargo.binding.vessel_id,
                "cargo actual vessel mismatch"
            );
            ensure!(
                stock.cradles.len() == DRONE_BAY_INITIAL_STOCK,
                "incomplete finite bay roster"
            );
            let mut stock_ids = BTreeSet::new();
            let mut homes = BTreeSet::new();
            for cradle in &stock.cradles {
                let home = usize::from(cradle.home_cradle);
                ensure!(
                    cradle.drone_id > 0
                        && stock_ids.insert(cradle.drone_id)
                        && homes.insert(home)
                        && home < DRONE_BAY_INITIAL_STOCK,
                    "invalid finite bay identity"
                );
                let expected = [DroneClass::Recon, DroneClass::Mining, DroneClass::Attack]
                    [home / DRONE_CLASS_CAP];
                ensure!(cradle.class == expected, "bay class/home identity mismatch");
                ensure!(
                    (cradle.presence == DroneBayPresence::Destroyed) == (cradle.hit_points == 0),
                    "bay destruction/health mismatch"
                );
            }
            ensure!(
                cargo.holds.len() == drones.len(),
                "snapshot cargo roster count mismatch"
            );
            let mut ids = BTreeSet::new();
            let mut incoming = 0;
            for hold in &cargo.holds {
                ensure!(ids.insert(hold.drone_id), "duplicate snapshot cargo drone");
                let d = drones
                    .get(&hold.drone_id)
                    .context("snapshot unowned cargo drone")?;
                let matches: Vec<_> = stock
                    .cradles
                    .iter()
                    .filter(|c| c.drone_id == hold.drone_id)
                    .collect();
                ensure!(matches.len() == 1, "ambiguous cargo stock identity");
                let cradle = matches[0];
                ensure!(
                    cradle.class == DroneClass::Mining,
                    "cargo on non-mining drone"
                );
                ensure!(
                    (cradle.presence == DroneBayPresence::Destroyed) == d.destroyed,
                    "destroyed cargo identity mismatch"
                );
                ensure!(
                    hold.ore_units == d.ore && hold.capacity_units == MINING_DRONE_CARGO_CAPACITY,
                    "snapshot drone balance or capacity mismatch"
                );
                ensure!(
                    hold.cycle_reserved_units <= DRONE_MINING_YIELD
                        && add(hold.ore_units, hold.cycle_reserved_units)?
                            <= MINING_DRONE_CARGO_CAPACITY,
                    "invalid cycle claim capacity"
                );
                ensure!(
                    cradle.presence == DroneBayPresence::Active
                        || (hold.ore_units == 0 && hold.cycle_reserved_units == 0),
                    "non-active drone retains cargo or claim"
                );
                incoming = add(incoming, add(hold.ore_units, hold.cycle_reserved_units)?)?;
            }
            let mut operation_ids = BTreeSet::new();
            for operation in &cargo.operations {
                ensure!(
                    operation.operation_id > 0 && operation_ids.insert(operation.operation_id),
                    "duplicate snapshot drop-off operation"
                );
                ensure!(
                    operation.binding == cargo.binding && drones.contains_key(&operation.drone_id),
                    "snapshot drop-off owner mismatch"
                );
                let replay = self.transfers.get(&(
                    pilot.name.clone(),
                    cargo.binding.vessel_id.clone(),
                    operation.operation_id,
                ));
                if let Some(replay) = replay {
                    ensure!(
                        operation.drone_id == replay.drone
                            && operation.accepted_at_ms == replay.started
                            && operation.requested_units == replay.requested,
                        "snapshot drop-off intent mismatch"
                    );
                    ensure!(
                        operation.committed_units == replay.committed
                            && operation.receipts == replay.receipts,
                        "snapshot drop-off receipt mismatch"
                    );
                } else {
                    ensure!(
                        operation.committed_units == 0 && operation.receipts.is_empty(),
                        "snapshot fabricated drop-off receipt"
                    );
                }
                ensure!(
                    add(operation.committed_units, operation.remaining_units)?
                        == operation.requested_units,
                    "snapshot drop-off quantity mismatch"
                );
            }
            ensure!(
                cargo.incoming_reserved_units == incoming,
                "incoming cargo claim mismatch"
            );
            ensure!(
                add(pilot.ore, incoming)? <= cargo_capacity(pilot.ship_class),
                "combined snapshot cargo capacity exceeded"
            );
        }
        ensure!(
            seen.len() == self.vessels.len(),
            "initialized cargo vessel missing from snapshot"
        );
        Ok(())
    }
}

/// Replay from genesis and compare only economic snapshot fields. Locations are
/// deliberately outside this audit: a valid ledger cannot prove a kill or mining.
pub fn verify(export: &LedgerExport, pinned: Option<&Checkpoint>) -> Result<Report> {
    ensure!(
        export.protocol_version == PROTOCOL_VERSION,
        "unsupported protocol version"
    );
    if let Some(pin) = pinned {
        if pin.sequence == 0 {
            ensure!(pin.hash == GENESIS_HASH, "invalid genesis checkpoint");
        } else {
            let index =
                usize::try_from(pin.sequence - 1).context("checkpoint sequence too large")?;
            let entry = export
                .entries
                .get(index)
                .context("rollback: export ends before pinned checkpoint")?;
            ensure!(
                entry.sequence == pin.sequence && entry.hash == pin.hash,
                "history rewrite: pinned checkpoint differs"
            );
        }
    }
    let carrier_registration = export.snapshot.carrier_inventory_registration.as_ref();
    if let Some(r) = carrier_registration {
        ensure!(
            r.well_formed()
                && carrier_inventory_geometry::stationary_fixture_for_registration(r).is_some(),
            "unsupported carrier registration"
        );
        ensure!(
            export.snapshot.drone_cargo_registration.is_none()
                && export.snapshot.drone_cargo_totals.is_none()
                && export
                    .snapshot
                    .pilots
                    .iter()
                    .all(|p| p.drone_cargo.is_none() && p.drone_bay.is_none()),
            "mixed carrier and legacy cargo world"
        );
    } else {
        ensure!(
            export
                .snapshot
                .pilots
                .iter()
                .all(|p| p.carrier_inventory.is_none()),
            "unregistered carrier snapshot"
        );
    }
    let registration = export.snapshot.drone_cargo_registration.as_ref();
    if let Some(registration) = registration {
        ensure!(registration.supported(), "unsupported cargo registration");
    } else {
        ensure!(
            export.snapshot.drone_cargo_totals.is_none()
                && export
                    .snapshot
                    .pilots
                    .iter()
                    .all(|p| p.drone_cargo.is_none()),
            "legacy world contains cargo state"
        );
    }
    let ruleset = if carrier_registration.is_some() {
        CARRIER_INVENTORY_RULESET_VERSION
    } else if registration.is_some() {
        DRONE_CARGO_RULESET_VERSION
    } else {
        RULESET_VERSION
    };
    let mut drones = DroneReplay::default();
    let mut carrier = carrier::CarrierReplay::default();
    // Before boarding events existed, hull choice was saved only in the world.
    // Infer that historical starting hull from the first boarding source, or the
    // final snapshot when the pilot has never boarded. Ownership of those free
    // preview hulls remains a migration assumption, not an independently proved event.
    let mut initial_classes: BTreeMap<String, ShipClass> = export
        .snapshot
        .pilots
        .iter()
        .map(|p| (p.name.clone(), p.ship_class))
        .collect();
    let mut first_boarding = BTreeSet::new();
    for entry in &export.entries {
        if let EconomicEvent::ShipBoarded {
            pilot,
            from_ship_class,
            ..
        } = &entry.event
        {
            if first_boarding.insert(pilot) {
                initial_classes.insert(pilot.clone(), *from_ship_class);
            }
        }
    }
    let mut accounts = BTreeMap::<String, Account>::new();
    let mut corporations = BTreeMap::<String, Corporation>::new();
    let mut personal = BTreeMap::<(String, u32), u64>::new();
    let mut orders = BTreeMap::<u64, SellOrder>::new();
    let mut used_orders = BTreeSet::new();
    let mut belt = INITIAL_BELT_ORE;
    let mut sequence = 0;
    let mut hash = GENESIS_HASH.to_string();
    let mut time = 0;
    let mut trades = 0;
    for entry in &export.entries {
        let apply = || -> Result<()> {
            ensure!(
                entry.sequence == add(sequence, 1)?,
                "missing or reordered sequence"
            );
            ensure!(
                entry.ruleset_version == ruleset,
                "unsupported ruleset version"
            );
            ensure!(entry.timestamp_ms >= time, "timestamp moved backwards");
            ensure!(entry.previous_hash == hash, "previous hash mismatch");
            ensure!(entry.calculate_hash() == entry.hash, "event hash mismatch");
            Ok(())
        };
        apply().with_context(|| format!("entry {}", entry.sequence))?;
        let apply_event = || -> Result<()> {
            match &entry.event {
                EconomicEvent::PhysicalCargo { .. } => {
                    bail!("physical-volume ruleset requires dedicated physical replay");
                }
                EconomicEvent::CarrierInventory { record } => {
                    carrier.apply(
                        record,
                        entry,
                        carrier_registration.context("carrier event in legacy world")?,
                        &mut drones,
                        &mut accounts,
                        &mut belt,
                    )?;
                }
                EconomicEvent::DroneCargo { record } => {
                    drones.apply(
                        record,
                        entry,
                        registration.context("cargo event in legacy world")?,
                        &mut accounts,
                        &mut belt,
                    )?;
                }
                EconomicEvent::ShipBoarded {
                    pilot,
                    from_ship_class,
                    ship_class,
                    system_id,
                    stored_units,
                    loaded_units,
                } => {
                    ensure!(
                        registration.is_none() && carrier_registration.is_none(),
                        "cargo world cannot change owned hull"
                    );
                    valid_system(*system_id)?;
                    ensure!(
                        from_ship_class != ship_class,
                        "boarding active hull is not a transfer"
                    );
                    let account = accounts.get_mut(pilot).context("unknown boarding pilot")?;
                    ensure!(
                        account.active_class.is_none_or(|c| c == *from_ship_class),
                        "boarding source hull mismatch"
                    );
                    ensure!(
                        *stored_units == account.ore,
                        "boarding stores incorrect cargo"
                    );
                    let prior = account.inactive.remove(&hull_key(*ship_class));
                    if let Some((station, ore)) = prior {
                        ensure!(
                            station == *system_id && ore == *loaded_units,
                            "boarding loads wrong hull cargo or station"
                        );
                    } else {
                        ensure!(*loaded_units == 0, "unrecorded hull cargo fabricated");
                        ensure!(
                            *system_id == HOME_SYSTEM_ID,
                            "unmoved starter hull boarded away from home"
                        );
                    }
                    ensure!(
                        *loaded_units <= cargo_capacity(*ship_class),
                        "boarded cargo over capacity"
                    );
                    ensure!(
                        account
                            .inactive
                            .insert(hull_key(*from_ship_class), (*system_id, *stored_units))
                            .is_none(),
                        "source hull already inactive"
                    );
                    account.active_class = Some(*ship_class);
                    account.ore = *loaded_units;
                }
                EconomicEvent::PersonalOreDeposited {
                    pilot,
                    system_id,
                    units,
                }
                | EconomicEvent::PersonalOreWithdrawn {
                    pilot,
                    system_id,
                    units,
                } => {
                    valid_system(*system_id)?;
                    ensure!(*units > 0, "zero storage transfer");
                    let account = accounts.get_mut(pilot).context("unknown storage pilot")?;
                    let storage = personal.entry((pilot.clone(), *system_id)).or_default();
                    if matches!(&entry.event, EconomicEvent::PersonalOreDeposited { .. }) {
                        account.ore = sub(account.ore, *units)?;
                        *storage = add(*storage, *units)?;
                    } else {
                        *storage = sub(*storage, *units)?;
                        account.ore = add(account.ore, *units)?;
                        ensure!(
                            account.ore
                                <= account
                                    .active_class
                                    .map(cargo_capacity)
                                    .unwrap_or(CARGO_CAPACITY),
                            "withdrawal exceeds cargo capacity"
                        );
                    }
                }
                EconomicEvent::CorporationCreated { corporation, owner } => {
                    ensure!(valid_name(corporation), "invalid corporation name");
                    ensure!(accounts.contains_key(owner), "unknown corporation owner");
                    ensure!(
                        !corporations.contains_key(corporation),
                        "duplicate corporation"
                    );
                    ensure!(
                        !corporations.values().any(|c| c.members.contains(owner)),
                        "pilot already belongs to corporation"
                    );
                    corporations.insert(
                        corporation.clone(),
                        Corporation {
                            owner: owner.clone(),
                            members: BTreeSet::from([owner.clone()]),
                            invitations: BTreeSet::new(),
                            storage: BTreeMap::new(),
                        },
                    );
                }
                EconomicEvent::CorporationInvited {
                    corporation,
                    pilot,
                    inviter,
                } => {
                    ensure!(accounts.contains_key(pilot), "unknown invitee");
                    ensure!(
                        !corporations.values().any(|c| c.members.contains(pilot)),
                        "invitee already belongs to corporation"
                    );
                    let corp = corporations
                        .get_mut(corporation)
                        .context("unknown corporation")?;
                    ensure!(&corp.owner == inviter, "invitation by nonowner");
                    ensure!(
                        corp.invitations.insert(pilot.clone()),
                        "duplicate invitation"
                    );
                }
                EconomicEvent::CorporationJoined { corporation, pilot } => {
                    ensure!(accounts.contains_key(pilot), "unknown joiner");
                    ensure!(
                        !corporations.values().any(|c| c.members.contains(pilot)),
                        "pilot already belongs to corporation"
                    );
                    let corp = corporations
                        .get_mut(corporation)
                        .context("unknown corporation")?;
                    ensure!(corp.invitations.remove(pilot), "joining without invitation");
                    corp.members.insert(pilot.clone());
                }
                EconomicEvent::CorporationOreDeposited {
                    corporation,
                    pilot,
                    system_id,
                    units,
                }
                | EconomicEvent::CorporationOreWithdrawn {
                    corporation,
                    pilot,
                    system_id,
                    units,
                } => {
                    valid_system(*system_id)?;
                    ensure!(*units > 0, "zero corporation transfer");
                    let corp = corporations
                        .get_mut(corporation)
                        .context("unknown corporation")?;
                    ensure!(
                        corp.members.contains(pilot),
                        "corporation storage by nonmember"
                    );
                    let account = accounts
                        .get_mut(pilot)
                        .context("unknown corporation pilot")?;
                    let storage = corp.storage.entry(*system_id).or_default();
                    if matches!(&entry.event, EconomicEvent::CorporationOreDeposited { .. }) {
                        account.ore = sub(account.ore, *units)?;
                        *storage = add(*storage, *units)?;
                    } else {
                        *storage = sub(*storage, *units)?;
                        account.ore = add(account.ore, *units)?;
                        ensure!(
                            account.ore
                                <= account
                                    .active_class
                                    .map(cargo_capacity)
                                    .unwrap_or(CARGO_CAPACITY),
                            "corporation withdrawal exceeds cargo capacity"
                        );
                    }
                }
                EconomicEvent::PilotCreated { pilot } => {
                    ensure!(
                        !pilot.is_empty()
                            && pilot.len() <= 24
                            && pilot
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
                        "invalid pilot name"
                    );
                    ensure!(!accounts.contains_key(pilot), "duplicate pilot creation");
                    accounts.insert(
                        pilot.clone(),
                        Account {
                            active_class: initial_classes.get(pilot).copied(),
                            credits: STARTER_CREDITS,
                            ore: 0,
                            ..Account::default()
                        },
                    );
                }
                EconomicEvent::OreMined { pilot, units } => {
                    ensure!(
                        *units > 0 && *units <= MAX_MINE_UNITS,
                        "invalid mining quantity"
                    );
                    ensure!(
                        add(cargo(&accounts, pilot)?, *units)?
                            <= accounts[pilot]
                                .active_class
                                .map(cargo_capacity)
                                .unwrap_or(CARGO_CAPACITY),
                        "cargo capacity exceeded"
                    );
                    belt = sub(belt, *units)?;
                    let account = accounts.get_mut(pilot).context("unknown miner")?;
                    account.ore = add(account.ore, *units)?;
                }
                EconomicEvent::SellPlaced {
                    order_id,
                    seller,
                    units,
                    unit_price,
                } => {
                    ensure!(
                        *order_id > 0 && used_orders.insert(*order_id),
                        "order ID zero or reused"
                    );
                    ensure!(
                        *units > 0 && *unit_price > 0,
                        "zero order quantity or price"
                    );
                    units
                        .checked_mul(*unit_price)
                        .context("order value overflow")?;
                    let account = accounts.get_mut(seller).context("unknown seller")?;
                    account.ore = sub(account.ore, *units)?;
                    orders.insert(
                        *order_id,
                        SellOrder {
                            id: *order_id,
                            seller: seller.clone(),
                            units: *units,
                            unit_price: *unit_price,
                        },
                    );
                }
                EconomicEvent::TradeExecuted {
                    order_id,
                    buyer,
                    units,
                } => {
                    let order = orders.get(order_id).context("unknown order")?.clone();
                    ensure!(
                        *units > 0 && *units <= order.units,
                        "invalid trade quantity"
                    );
                    ensure!(*buyer != order.seller, "self trade");
                    ensure!(
                        add(cargo(&accounts, buyer)?, *units)?
                            <= accounts[buyer]
                                .active_class
                                .map(cargo_capacity)
                                .unwrap_or(CARGO_CAPACITY),
                        "cargo capacity exceeded"
                    );
                    let price = units
                        .checked_mul(order.unit_price)
                        .context("trade value overflow")?;
                    let account = accounts.get_mut(buyer).context("unknown buyer")?;
                    account.credits = sub(account.credits, price)?;
                    account.ore = add(account.ore, *units)?;
                    let seller = accounts.get_mut(&order.seller).context("unknown seller")?;
                    seller.credits = add(seller.credits, price)?;
                    if order.units == *units {
                        orders.remove(order_id);
                    } else {
                        orders.get_mut(order_id).unwrap().units -= *units;
                    }
                    trades = add(trades, 1)?;
                }
                EconomicEvent::SellCancelled { order_id, seller } => {
                    let order = orders.get(order_id).context("unknown order")?.clone();
                    ensure!(*seller == order.seller, "cancellation by nonowner");
                    let account = accounts.get_mut(seller).context("unknown seller")?;
                    account.ore = add(account.ore, order.units)?;
                    ensure!(
                        account.ore
                            <= account
                                .active_class
                                .map(cargo_capacity)
                                .unwrap_or(CARGO_CAPACITY),
                        "cancel exceeds cargo capacity"
                    );
                    orders.remove(order_id);
                }
            }
            Ok(())
        };
        let mut apply_event = apply_event;
        apply_event()
            .with_context(|| format!("entry {} economic rule violation", entry.sequence))?;
        if registration.is_some() || carrier_registration.is_some() {
            for (name, account) in &accounts {
                ensure!(
                    add(account.ore, drones.pilot_ore(name)?)?
                        <= account
                            .active_class
                            .map(cargo_capacity)
                            .unwrap_or(CARGO_CAPACITY),
                    "combined ship and drone capacity exceeded"
                );
            }
        }
        sequence = entry.sequence;
        hash = entry.hash.clone();
        time = entry.timestamp_ms;
    }
    let snapshot = &export.snapshot;
    ensure!(
        snapshot.ledger_sequence == sequence && snapshot.ledger_hash == hash,
        "snapshot head mismatch"
    );
    ensure!(snapshot.belt_ore == belt, "snapshot belt mismatch");
    // Older exports omit individual rocks. New exports conserve the same global
    // reserve; this proves aggregate consistency, not which rock was mined.
    if !snapshot.asteroids.is_empty() {
        let mut ids = BTreeSet::new();
        let mut reserve = 0;
        for asteroid in &snapshot.asteroids {
            ensure!(ids.insert(asteroid.id), "duplicate asteroid ID");
            reserve = add(reserve, asteroid.ore)?;
        }
        ensure!(reserve == belt, "asteroid reserve mismatch");
    }
    ensure!(
        snapshot.pilots.len() == accounts.len(),
        "snapshot pilot count mismatch"
    );
    let mut seen = BTreeSet::new();
    for pilot in &snapshot.pilots {
        ensure!(seen.insert(&pilot.name), "duplicate snapshot pilot");
        let account = accounts
            .get(&pilot.name)
            .context("unknown snapshot pilot")?;
        ensure!(
            account.active_class.is_none_or(|c| c == pilot.ship_class),
            "snapshot active hull mismatch"
        );
        ensure!(
            account.credits == pilot.credits && account.ore == pilot.ore,
            "snapshot balance mismatch for {}",
            pilot.name
        );
    }
    ensure!(
        snapshot.orders.len() == orders.len(),
        "snapshot order count mismatch"
    );
    let mut seen = BTreeSet::new();
    for order in &snapshot.orders {
        ensure!(seen.insert(order.id), "duplicate snapshot order");
        ensure!(
            orders.get(&order.id) == Some(order),
            "snapshot order mismatch"
        );
    }
    let total_credits = accounts
        .values()
        .try_fold(0, |sum, a| add(sum, a.credits))?;
    let expected_credits = u64::try_from(accounts.len())?
        .checked_mul(STARTER_CREDITS)
        .context("total credit overflow")?;
    ensure!(
        total_credits == expected_credits,
        "credit conservation failed"
    );
    let mut stored_ore = 0;
    for account in accounts.values() {
        for (_, ore) in account.inactive.values() {
            stored_ore = add(stored_ore, *ore)?;
        }
    }
    for ore in personal.values() {
        stored_ore = add(stored_ore, *ore)?;
    }
    for corp in corporations.values() {
        for ore in corp.storage.values() {
            stored_ore = add(stored_ore, *ore)?;
        }
    }
    ensure!(
        snapshot.stored_ore == stored_ore,
        "snapshot stored ore mismatch"
    );
    if let Some(registration) = registration {
        ensure!(
            snapshot.server_time_ms >= time,
            "cargo snapshot predates ledger"
        );
        drones.snapshot(snapshot, registration)?;
    }
    if let Some(registration) = carrier_registration {
        ensure!(
            snapshot.server_time_ms >= time,
            "carrier snapshot predates ledger"
        );
        carrier.snapshot(snapshot, &drones, registration)?;
    }
    let total_ore = accounts.values().try_fold(
        add(add(belt, stored_ore)?, add(drones.total()?, drones.lost)?)?,
        |sum, a| add(sum, a.ore),
    )?;
    let total_ore = orders
        .values()
        .try_fold(total_ore, |sum, o| add(sum, o.units))?;
    if total_ore != INITIAL_BELT_ORE {
        bail!("ore conservation failed");
    }
    Ok(Report {
        accounts: accounts.len(),
        trades,
        total_credits,
        belt_ore: belt,
        stored_ore,
        head: Checkpoint { sequence, hash },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn append(export: &mut LedgerExport, event: EconomicEvent) {
        let mut entry = LedgerEntry {
            sequence: export.entries.len() as u64 + 1,
            ruleset_version: RULESET_VERSION,
            timestamp_ms: export.entries.last().map_or(0, |e| e.timestamp_ms + 1000),
            previous_hash: export.snapshot.ledger_hash.clone(),
            event,
            hash: String::new(),
        };
        entry.hash = entry.calculate_hash();
        export.snapshot.ledger_sequence = entry.sequence;
        export.snapshot.ledger_hash = entry.hash.clone();
        export.entries.push(entry);
    }
    fn assets_fixture() -> LedgerExport {
        let mut e = fixture();
        for event in [
            EconomicEvent::PersonalOreDeposited {
                pilot: "Alice".into(),
                system_id: 1,
                units: 3,
            },
            EconomicEvent::ShipBoarded {
                pilot: "Alice".into(),
                from_ship_class: ShipClass::Pioneer,
                ship_class: ShipClass::Prospector,
                system_id: 1,
                stored_units: 3,
                loaded_units: 0,
            },
            EconomicEvent::PersonalOreWithdrawn {
                pilot: "Alice".into(),
                system_id: 1,
                units: 2,
            },
            EconomicEvent::ShipBoarded {
                pilot: "Alice".into(),
                from_ship_class: ShipClass::Prospector,
                ship_class: ShipClass::Pioneer,
                system_id: 1,
                stored_units: 2,
                loaded_units: 3,
            },
            EconomicEvent::CorporationCreated {
                corporation: "Builders".into(),
                owner: "Alice".into(),
            },
            EconomicEvent::CorporationInvited {
                corporation: "Builders".into(),
                pilot: "Bob".into(),
                inviter: "Alice".into(),
            },
            EconomicEvent::CorporationJoined {
                corporation: "Builders".into(),
                pilot: "Bob".into(),
            },
            EconomicEvent::CorporationOreDeposited {
                corporation: "Builders".into(),
                pilot: "Alice".into(),
                system_id: 1,
                units: 1,
            },
            EconomicEvent::CorporationOreDeposited {
                corporation: "Builders".into(),
                pilot: "Bob".into(),
                system_id: 1,
                units: 2,
            },
            EconomicEvent::CorporationOreWithdrawn {
                corporation: "Builders".into(),
                pilot: "Alice".into(),
                system_id: 1,
                units: 1,
            },
        ] {
            append(&mut e, event);
        }
        e.snapshot.pilots[0].ore = 3;
        e.snapshot.pilots[1].ore = 2;
        e.snapshot.stored_ore = 5;
        e
    }
    #[test]
    fn inactive_personal_and_shared_cargo_replay_without_double_counting() {
        let mut e = assets_fixture();
        let report = verify(&e, None).unwrap();
        assert_eq!(report.stored_ore, 5);
        assert_eq!(report.total_credits, 2000);
        e.snapshot.stored_ore += 1;
        assert!(verify(&e, None).is_err());
    }
    #[test]
    fn rehashed_hull_cargo_fabrication_and_cross_station_load_are_rejected() {
        let mut e = assets_fixture();
        if let EconomicEvent::ShipBoarded { loaded_units, .. } = &mut e.entries[7].event {
            *loaded_units = 1;
        }
        rehash(&mut e);
        assert!(verify(&e, None).is_err());
        let mut e = assets_fixture();
        if let EconomicEvent::ShipBoarded { system_id, .. } = &mut e.entries[9].event {
            *system_id = 2;
        }
        rehash(&mut e);
        assert!(verify(&e, None).is_err());
    }
    #[test]
    fn corporation_permissions_and_personal_balances_are_replayed() {
        let mut e = assets_fixture();
        if let EconomicEvent::CorporationInvited { inviter, .. } = &mut e.entries[11].event {
            *inviter = "Bob".into();
        }
        rehash(&mut e);
        assert!(verify(&e, None).is_err());
        let mut e = assets_fixture();
        if let EconomicEvent::CorporationOreWithdrawn { pilot, .. } = &mut e.entries[15].event {
            *pilot = "Bob".into();
        }
        rehash(&mut e);
        assert!(verify(&e, None).is_err());
        let mut e = assets_fixture();
        if let EconomicEvent::PersonalOreWithdrawn { units, .. } = &mut e.entries[8].event {
            *units = 4;
        }
        rehash(&mut e);
        assert!(verify(&e, None).is_err());
        let mut e = fixture();
        append(
            &mut e,
            EconomicEvent::CorporationCreated {
                corporation: "Builders".into(),
                owner: "Alice".into(),
            },
        );
        append(
            &mut e,
            EconomicEvent::CorporationJoined {
                corporation: "Builders".into(),
                pilot: "Bob".into(),
            },
        );
        assert!(verify(&e, None).is_err());
    }
    #[test]
    fn individual_reserves_match_legacy_economic_total() {
        let mut export = fixture();
        export.snapshot.asteroids = asteroid_catalog(export.snapshot.belt_ore);
        verify(&export, None).unwrap();
        export.snapshot.asteroids[0].ore += 1;
        assert!(verify(&export, None).is_err());
        export.snapshot.asteroids[0].ore -= 1;
        export.snapshot.asteroids[1].id = export.snapshot.asteroids[0].id;
        assert!(verify(&export, None).is_err());
    }
    pub(super) fn fixture() -> LedgerExport {
        let events = vec![
            EconomicEvent::PilotCreated {
                pilot: "Alice".into(),
            },
            EconomicEvent::PilotCreated {
                pilot: "Bob".into(),
            },
            EconomicEvent::OreMined {
                pilot: "Alice".into(),
                units: 10,
            },
            EconomicEvent::SellPlaced {
                order_id: 1,
                seller: "Alice".into(),
                units: 10,
                unit_price: 5,
            },
            EconomicEvent::TradeExecuted {
                order_id: 1,
                buyer: "Bob".into(),
                units: 4,
            },
            EconomicEvent::SellCancelled {
                order_id: 1,
                seller: "Alice".into(),
            },
        ];
        let mut entries = Vec::new();
        let mut previous_hash = GENESIS_HASH.to_string();
        for (i, event) in events.into_iter().enumerate() {
            let mut entry = LedgerEntry {
                sequence: i as u64 + 1,
                ruleset_version: RULESET_VERSION,
                timestamp_ms: i as u64 * 1000,
                previous_hash,
                event,
                hash: String::new(),
            };
            entry.hash = entry.calculate_hash();
            previous_hash = entry.hash.clone();
            entries.push(entry);
        }
        LedgerExport {
            protocol_version: PROTOCOL_VERSION,
            snapshot: Snapshot {
                carrier_inventory_registration: None,
                drone_cargo_registration: None,
                drone_cargo_totals: None,
                walk_sprint_v1: false,
                docked_traversal_capability: None,
                world_layout: None,
                exterior: None,
                remote_controls: vec![],
                fleet_links: vec![],
                resource_jobs: vec![],
                surface_props: vec![],
                traffic: vec![],
                stored_ore: 0,
                asteroids: vec![],
                online_pilots: vec![],
                server_time_ms: 6000,
                pilots: vec![
                    PilotView {
                        carrier_inventory: None,
                        power_accounting: None,
                        drone_bay: None,
                        drone_cargo: None,
                        cockpit_mechanisms: None,
                        reactor_configuration: None,
                        station_avatar: None,
                        surface: None,
                        power: None,
                        drones: vec![],
                        target_locks: vec![],
                        locked_asteroids: vec![],
                        drone: None,
                        mining_modules: Vec::new(),
                        target_lock: None,
                        ship_class: arkline_protocol::ShipClass::default(),
                        system_id: HOME_SYSTEM_ID,
                        locked_asteroid: None,
                        name: "Alice".into(),
                        credits: 1020,
                        ore: 6,
                        location: Location::Docked,
                        flight: None,
                    },
                    PilotView {
                        carrier_inventory: None,
                        power_accounting: None,
                        drone_bay: None,
                        drone_cargo: None,
                        cockpit_mechanisms: None,
                        reactor_configuration: None,
                        station_avatar: None,
                        surface: None,
                        power: None,
                        drones: vec![],
                        target_locks: vec![],
                        locked_asteroids: vec![],
                        drone: None,
                        mining_modules: Vec::new(),
                        target_lock: None,
                        ship_class: arkline_protocol::ShipClass::default(),
                        system_id: HOME_SYSTEM_ID,
                        locked_asteroid: None,
                        name: "Bob".into(),
                        credits: 980,
                        ore: 4,
                        location: Location::Belt,
                        flight: None,
                    },
                ],
                orders: vec![],
                belt_ore: INITIAL_BELT_ORE - 10,
                ledger_sequence: 6,
                ledger_hash: previous_hash,
            },
            entries,
        }
    }
    fn rehash(export: &mut LedgerExport) {
        let mut previous = GENESIS_HASH.to_string();
        for entry in &mut export.entries {
            entry.previous_hash = previous;
            entry.hash = entry.calculate_hash();
            previous = entry.hash.clone();
        }
        export.snapshot.ledger_hash = previous;
    }
    #[test]
    fn successful_replay() {
        let export = fixture();
        let report = verify(&export, None).unwrap();
        assert_eq!(report.accounts, 2);
        assert_eq!(report.trades, 1);
        assert_eq!(report.total_credits, 2000);
        verify(&export, Some(&report.head)).unwrap();
    }
    #[test]
    fn tampered_event_hash() {
        let mut export = fixture();
        export.entries[2].event = EconomicEvent::OreMined {
            pilot: "Alice".into(),
            units: 11,
        };
        assert!(verify(&export, None)
            .unwrap_err()
            .to_string()
            .contains("entry 3"));
    }
    #[test]
    fn missing_and_reordered_events() {
        let mut missing = fixture();
        missing.entries.remove(2);
        rehash(&mut missing);
        assert!(verify(&missing, None).is_err());
        let mut reordered = fixture();
        reordered.entries.swap(2, 3);
        rehash(&mut reordered);
        assert!(verify(&reordered, None).is_err());
    }
    #[test]
    fn fabricated_credit_and_duplicate_identity() {
        let mut export = fixture();
        export.snapshot.pilots[0].credits += 1;
        assert!(verify(&export, None).is_err());
        let mut export = fixture();
        export.entries[1].event = EconomicEvent::PilotCreated {
            pilot: "Alice".into(),
        };
        rehash(&mut export);
        assert!(verify(&export, None).is_err());
    }
    #[test]
    fn overspend_and_oversell_even_when_rehashed() {
        let mut export = fixture();
        export.entries[3].event = EconomicEvent::SellPlaced {
            order_id: 1,
            seller: "Alice".into(),
            units: 10,
            unit_price: 1000,
        };
        rehash(&mut export);
        assert!(verify(&export, None).is_err());
        let mut export = fixture();
        export.entries[3].event = EconomicEvent::SellPlaced {
            order_id: 1,
            seller: "Alice".into(),
            units: 11,
            unit_price: 5,
        };
        rehash(&mut export);
        assert!(verify(&export, None).is_err());
    }
    #[test]
    fn snapshot_mismatch_and_duplicates() {
        let mut export = fixture();
        export.snapshot.belt_ore += 1;
        assert!(verify(&export, None).is_err());
        let mut export = fixture();
        export.snapshot.pilots[1] = export.snapshot.pilots[0].clone();
        assert!(verify(&export, None).is_err());
    }
    #[test]
    fn pinned_rollback_and_rewrite() {
        let mut export = fixture();
        let pin = verify(&export, None).unwrap().head;
        export.entries.pop();
        assert!(verify(&export, Some(&pin))
            .unwrap_err()
            .to_string()
            .contains("rollback"));
        let mut export = fixture();
        export.entries[5].timestamp_ms += 1;
        rehash(&mut export);
        assert!(verify(&export, None).is_ok());
        assert!(verify(&export, Some(&pin))
            .unwrap_err()
            .to_string()
            .contains("rewrite"));
    }
    #[test]
    fn reject_unknown_versions_backwards_time_and_bad_genesis() {
        let mut export = fixture();
        export.protocol_version += 1;
        assert!(verify(&export, None).is_err());
        let mut export = fixture();
        export.entries[0].ruleset_version += 1;
        rehash(&mut export);
        assert!(verify(&export, None).is_err());
        let mut export = fixture();
        export.entries[5].timestamp_ms = 1;
        rehash(&mut export);
        assert!(verify(&export, None).is_err());
        assert!(verify(
            &fixture(),
            Some(&Checkpoint {
                sequence: 0,
                hash: "wrong".into()
            })
        )
        .is_err());
    }
    #[test]
    fn replay_id_conflict_and_order_id_reuse() {
        let mut export = fixture();
        export.entries[5].event = EconomicEvent::SellPlaced {
            order_id: 1,
            seller: "Alice".into(),
            units: 1,
            unit_price: 1,
        };
        rehash(&mut export);
        assert!(verify(&export, None).is_err());
    }
}
