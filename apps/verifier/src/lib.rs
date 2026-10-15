//! Independent replay of Arkline economic records. No server or database code.
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
                entry.ruleset_version == RULESET_VERSION,
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
                EconomicEvent::ShipBoarded {
                    pilot,
                    from_ship_class,
                    ship_class,
                    system_id,
                    stored_units,
                    loaded_units,
                } => {
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
    let total_ore = accounts
        .values()
        .try_fold(add(belt, stored_ore)?, |sum, a| add(sum, a.ore))?;
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
    fn fixture() -> LedgerExport {
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
