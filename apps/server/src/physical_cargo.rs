//! Opt-in physical-volume accounting inspection on the existing world/ledger
//! transaction boundary. Raw inspection remains separate from the fresh, gated
//! owner-bound controller, which uses existing Request/tick entries without
//! enabling public listeners or general gameplay.
//!
//! The eventual carrier adapter must supply real source-bound extraction and
//! socket eligibility. Inspection service events do not certify physical poses.
use crate::{append_event, load_entries, Connection, OwnershipLock, Server, World};
use anyhow::{ensure, Context, Result};
use arkline_protocol::{
    physical_cargo::{
        catalog::{PhysicalCargoCatalog, PhysicalCargoDesignBinding},
        ledger::{PhysicalCargoEvent, PHYSICAL_VOLUME_RULESET_VERSION},
        source::physical_source_configuration,
        PhysicalCargoAccount, VolumeCm3,
    },
    EconomicEvent, LedgerEntry, SceneLayoutRevision,
};
use arkline_verifier::physical_cargo::{replay, verify_account};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

pub mod bay;
pub mod control;

const DOWNGRADE_GUARD: &str = "CREATE TRIGGER preserve_physical_cargo_v1 BEFORE UPDATE OF json ON world
            WHEN json_extract(OLD.json,'$.physical_cargo') IS NOT NULL
            AND (json_extract(NEW.json,'$.physical_cargo') IS NULL OR
                 json_extract(NEW.json,'$.physical_cargo.genesis') IS NOT json_extract(OLD.json,'$.physical_cargo.genesis'))
            BEGIN SELECT RAISE(ABORT,'physical source and volume units cannot be discarded or rebound'); END;";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalCargoGenesis {
    pub source_configuration_sha256: String,
    pub design: PhysicalCargoDesignBinding,
    pub catalog: PhysicalCargoCatalog,
    pub mining_drone_ids: Vec<u64>,
    /// Fresh authored inspection resource, not converted from prototype asteroids.
    pub resource_cm3: VolumeCm3,
}
impl PhysicalCargoGenesis {
    fn event(&self) -> PhysicalCargoEvent {
        PhysicalCargoEvent::Initialized {
            source_configuration_sha256: self.source_configuration_sha256.clone(),
            design: Box::new(self.design.clone()),
            catalog: self.catalog.clone(),
            mining_drone_ids: self.mining_drone_ids.clone(),
            resource_cm3: self.resource_cm3,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PhysicalCargoState {
    pub genesis: PhysicalCargoGenesis,
    pub account: PhysicalCargoAccount,
    pub resource_remaining_cm3: VolumeCm3,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bay: Option<arkline_protocol::physical_cargo::bay::PhysicalBayState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<arkline_protocol::physical_cargo::control::PhysicalControlState>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhysicalCargoExport {
    pub entries: Vec<LedgerEntry>,
    pub genesis: PhysicalCargoGenesis,
    pub account: PhysicalCargoAccount,
    pub resource_remaining_cm3: VolumeCm3,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bay: Option<arkline_protocol::physical_cargo::bay::PhysicalBayState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<arkline_protocol::physical_cargo::control::PhysicalControlState>,
}
impl PhysicalCargoExport {
    pub fn verify(&self) -> Result<()> {
        let replayed = replay(&self.entries)?;
        ensure!(
            self.entries.first().is_some_and(|e| matches!(&e.event,
            EconomicEvent::PhysicalCargo { record } if record == &self.genesis.event())),
            "Physical genesis/export mismatch"
        );
        verify_account(&replayed, &self.account)?;
        ensure!(
            replayed.bay == self.bay && replayed.control == self.control,
            "Bay state differs from authoritative transcript"
        );
        ensure!(
            replayed.resource_remaining_cm3 == self.resource_remaining_cm3,
            "Physical source resource differs from replay"
        );
        Ok(())
    }
}

/// Deliberately owns but never exposes the ordinary Server transport interface.
/// The same ownership lock, SQLite world JSON and hash-linked ledger are retained.
pub struct PhysicalCargoInspection {
    server: Server,
}
impl PhysicalCargoInspection {
    pub fn open(path: impl AsRef<Path>, genesis: PhysicalCargoGenesis, now: u64) -> Result<Self> {
        let server = Server::open_realm_with_configuration(
            path,
            false,
            false,
            None,
            SceneLayoutRevision::LegacyV1,
            false,
            Some((genesis, now, false)),
        )?;
        ensure!(
            !server.physical_controlled,
            "Use explicit controlled-world factory"
        );
        Ok(Self { server })
    }
    /// An explicitly authorized inspection controller event. This is not a
    /// client request and cannot be used through normal gameplay transport.
    pub fn apply(&mut self, event: PhysicalCargoEvent, now: u64) -> Result<PhysicalCargoExport> {
        ensure!(
            !matches!(event, PhysicalCargoEvent::Initialized { .. }),
            "Cannot replace physical genesis"
        );
        apply(&mut self.server.db, &[event], now)?;
        self.export()
    }
    pub fn export(&mut self) -> Result<PhysicalCargoExport> {
        let tx = self.server.db.transaction()?;
        let world = read_world(&tx)?;
        let entries = load_entries(&tx, true)?;
        let out = export_state(&world, entries)?;
        out.verify()?;
        tx.commit()?;
        Ok(out)
    }
}
fn read_world(db: &Connection) -> Result<World> {
    Ok(serde_json::from_str(&db.query_row::<String, _, _>(
        "SELECT json FROM world WHERE id=1",
        [],
        |r| r.get(0),
    )?)?)
}
fn export_state(world: &World, entries: Vec<LedgerEntry>) -> Result<PhysicalCargoExport> {
    let s = world
        .physical_cargo
        .as_ref()
        .context("No physical cargo registration")?;
    ensure!(
        (s.control.is_some() || world.pilots.is_empty())
            && world.accounts.is_empty()
            && world.orders.is_empty()
            && world.carrier_inventory_registration.is_none()
            && world.drone_cargo_registration.is_none()
            && world.independent_bay_registration.is_none()
            && world.drone_bay_registration.is_none(),
        "Physical inspection cannot alias legacy/player authorities"
    );
    control::validate_world(world)?;
    Ok(PhysicalCargoExport {
        entries,
        genesis: s.genesis.clone(),
        account: s.account.clone(),
        resource_remaining_cm3: s.resource_remaining_cm3,
        bay: s.bay.clone(),
        control: s.control.clone(),
    })
}
pub(crate) fn refuse_existing(db: &Connection) -> Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='world')",
        [],
        |r| r.get(0),
    )?;
    if exists {
        let stamp: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM world WHERE json_type(json,'$.physical_cargo') IS NOT NULL AND json_type(json,'$.physical_cargo') != 'null')",[],|r|r.get(0))?;
        ensure!(!stamp,"Physical cargo inspection requires its explicit factory; normal gameplay admission is unavailable");
    }
    Ok(())
}

pub(crate) fn open_server(
    mut db: Connection,
    ownership_lock: Option<OwnershipLock>,
    genesis: PhysicalCargoGenesis,
    now: u64,
    controlled: bool,
) -> Result<Server> {
    let source =
        physical_source_configuration().context("Physical source certificate unavailable")?;
    ensure!(
        source.identity_sha256().as_ref() == Some(&genesis.source_configuration_sha256)
            && genesis.design.valid_against_catalog(&genesis.catalog),
        "Physical source/catalog mismatch"
    );
    // Validate complete genesis before creating or modifying any world/ledger rows.
    let mut probe = LedgerEntry {
        sequence: 1,
        ruleset_version: PHYSICAL_VOLUME_RULESET_VERSION,
        timestamp_ms: now,
        previous_hash: arkline_protocol::GENESIS_HASH.into(),
        event: EconomicEvent::PhysicalCargo {
            record: genesis.event(),
        },
        hash: String::new(),
    };
    probe.hash = probe.calculate_hash();
    replay(&[probe])?;
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='world')",
        [],
        |r| r.get(0),
    )?;
    if exists {
        let guard: String = db.query_row("SELECT sql FROM sqlite_master WHERE type='trigger' AND name='preserve_physical_cargo_v1'", [], |r|r.get(0)).context("Physical downgrade guard missing")?;
        ensure!(
            guard.trim().trim_end_matches(';') == DOWNGRADE_GUARD.trim().trim_end_matches(';'),
            "Physical downgrade guard was modified"
        );
        let world = read_world(&db)?;
        let state = world
            .physical_cargo
            .as_ref()
            .context("Existing world cannot be converted to physical units; use a fresh world")?;
        ensure!(
            state.genesis == genesis,
            "Existing physical genesis differs; no rebinding or migration"
        );
        let tx = db.transaction()?;
        let entries = load_entries(&tx, true)?;
        let export = export_state(&world, entries)?;
        export.verify()?;
        ensure!(
            if controlled {
                export.control.is_some() || export.entries.len() == 1
            } else {
                export.control.is_none()
            },
            "Physical factory flavor mismatch; no rejected-open restart"
        );
        ensure!(
            now >= export.entries.last().unwrap().timestamp_ms,
            "Restart clock moved backwards"
        );
        tx.commit()?;
    } else {
        let occupied: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name NOT GLOB 'sqlite_*')",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            !occupied,
            "Fresh physical factory requires an empty database, not an unrelated or partial schema"
        );
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(
            "CREATE TABLE world (id INTEGER PRIMARY KEY CHECK(id=1), json TEXT NOT NULL);
            CREATE TABLE ledger (sequence INTEGER PRIMARY KEY, json TEXT NOT NULL);",
        )?;
        let account =
            PhysicalCargoAccount::fresh(&genesis.design.manifest, &genesis.mining_drone_ids)
                .context("Invalid physical roster or manifest")?;
        let world = World {
            physical_cargo: Some(PhysicalCargoState {
                resource_remaining_cm3: genesis.resource_cm3,
                account,
                genesis: genesis.clone(),
                bay: None,
                control: None,
            }),
            ..World::default()
        };
        tx.execute(
            "INSERT INTO world(id,json) VALUES(1,?1)",
            [serde_json::to_string(&world)?],
        )?;
        append_event(
            &tx,
            &mut vec![],
            EconomicEvent::PhysicalCargo {
                record: genesis.event(),
            },
            now,
            PHYSICAL_VOLUME_RULESET_VERSION,
        )?;
        tx.execute_batch(DOWNGRADE_GUARD)?;
        tx.commit()?;
    }
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
    // Restart does not complete pending extraction or pump catch-up. Persist the
    // cancellation/hold through the SAME ledger transaction as the account state.
    let tx = db.transaction()?;
    let entries = load_entries(&tx, true)?;
    let prior = replay(&entries)?;
    tx.commit()?;
    let has_bay = read_world(&db)?
        .physical_cargo
        .as_ref()
        .is_some_and(|s| s.bay.is_some());
    let has_control = read_world(&db)?
        .physical_cargo
        .as_ref()
        .is_some_and(|s| s.control.is_some());
    if has_bay && !has_control && !controlled {
        bay::restart_db(&mut db, now)?;
    }
    let mut stop = Vec::new();
    for id in genesis.mining_drone_ids.iter().filter(|_| !has_bay) {
        let world = read_world(&db)?;
        if world
            .physical_cargo
            .as_ref()
            .unwrap()
            .account
            .drones
            .iter()
            .find(|h| h.drone_id == *id)
            .is_some_and(|h| h.claim.is_some())
        {
            stop.push(PhysicalCargoEvent::CancelClaim { drone_id: *id });
        }
    }
    if let Some(service) = prior.active_service.as_ref().filter(|_| !has_bay) {
        if !service.held {
            stop.push(PhysicalCargoEvent::ServiceHeld {
                service_id: service.service_id,
            });
        }
    }
    if !stop.is_empty() {
        apply(&mut db, &stop, now)?;
    }
    Ok(Server {
        physical_controlled: has_control,
        local_development: false,
        editor_room_layout: None,
        editor_time: None,
        db,
        auth: None,
        online_sessions: BTreeMap::new(),
        interior_incompatible: Default::default(),
        docked_incompatible: Default::default(),
        fleet_incompatible: Default::default(),
        room_incompatible: Default::default(),
        _ownership_lock: ownership_lock,
    })
}
fn apply(db: &mut Connection, requested: &[PhysicalCargoEvent], now: u64) -> Result<()> {
    ensure!(
        read_world(db)?
            .physical_cargo
            .as_ref()
            .is_some_and(|s| s.bay.is_none()),
        "Raw accounting cannot bypass the bound physical bay"
    );
    ensure!(
        !requested.iter().any(|e| matches!(
            e,
            PhysicalCargoEvent::BayBound { .. } | PhysicalCargoEvent::BayStep { .. }
        )),
        "Bay events require their executor"
    );
    apply_authorized(db, requested, now, None)
}
fn apply_authorized(
    db: &mut Connection,
    requested: &[PhysicalCargoEvent],
    now: u64,
    expected_bay: Option<arkline_protocol::physical_cargo::bay::PhysicalBayState>,
) -> Result<()> {
    apply_authorized_control(db, requested, now, expected_bay, None, None)
}
fn apply_authorized_control(
    db: &mut Connection,
    requested: &[PhysicalCargoEvent],
    now: u64,
    expected_bay: Option<arkline_protocol::physical_cargo::bay::PhysicalBayState>,
    expected_control: Option<arkline_protocol::physical_cargo::control::PhysicalControlState>,
    initial_token_hash: Option<String>,
) -> Result<()> {
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let mut world = read_world(&tx)?;
    let mut entries = load_entries(&tx, true)?;
    export_state(&world, entries.clone())?.verify()?;
    ensure!(
        entries.last().is_some_and(|last| now >= last.timestamp_ms),
        "Physical clock moved backwards"
    );
    let mut changed = false;
    for event in requested {
        // A retry must match the original complete operation identity, not only
        // amount. It returns the durable result without another event or debit.
        if let PhysicalCargoEvent::Transfer { request_id, .. } = event {
            if let Some(old) = entries.iter().find_map(|e| match &e.event {
                EconomicEvent::PhysicalCargo {
                    record: r @ PhysicalCargoEvent::Transfer { request_id: id, .. },
                } if id == request_id => Some(r),
                _ => None,
            }) {
                ensure!(
                    old == event,
                    "Transfer request ID reused with different content"
                );
                continue;
            }
        }
        append_event(
            &tx,
            &mut entries,
            EconomicEvent::PhysicalCargo {
                record: event.clone(),
            },
            now,
            PHYSICAL_VOLUME_RULESET_VERSION,
        )?;
        changed = true;
        let s = world.physical_cargo.as_mut().context("No physical state")?;
        let m = &s.genesis.design.manifest;
        let accepted = match *event {
            PhysicalCargoEvent::Initialized { .. } => false,
            PhysicalCargoEvent::BayBound { .. }
            | PhysicalCargoEvent::BayStep { .. }
            | PhysicalCargoEvent::WorldBound { .. }
            | PhysicalCargoEvent::WorldStep { .. } => true,
            PhysicalCargoEvent::Claim {
                drone_id,
                claim_id,
                volume_cm3,
                ..
            } => s
                .account
                .reserve_extraction(m, drone_id, claim_id, volume_cm3),
            PhysicalCargoEvent::Extract { drone_id, claim_id } => {
                let claimed = s
                    .account
                    .drones
                    .iter()
                    .find(|d| d.drone_id == drone_id)
                    .and_then(|d| d.claim.as_ref())
                    .context("Missing extraction claim")?
                    .volume_cm3;
                s.resource_remaining_cm3 = s
                    .resource_remaining_cm3
                    .checked_sub(claimed)
                    .context("Physical resource exhausted")?;
                s.account.complete_extraction(m, drone_id, claim_id)
            }
            PhysicalCargoEvent::CancelClaim { drone_id } => {
                s.account.cancel_extraction(m, drone_id)
            }
            PhysicalCargoEvent::Transfer {
                request_id,
                drone_id,
                volume_cm3,
                ..
            } => s.account.transfer(m, request_id, drone_id, volume_cm3, now),
            PhysicalCargoEvent::DroneLost { drone_id } => s.account.destroy_drone(m, drone_id),
            PhysicalCargoEvent::ServiceOpened { .. }
            | PhysicalCargoEvent::ServiceHeld { .. }
            | PhysicalCargoEvent::ServiceResumed { .. }
            | PhysicalCargoEvent::ServiceClosed { .. } => true,
        };
        ensure!(accepted, "Physical primitive rejected transition");
        ensure!(
            s.account.validate_roster(m, &s.genesis.mining_drone_ids),
            "Physical account/roster invalid"
        );
    }
    if let Some(bay) = expected_bay {
        world.physical_cargo.as_mut().unwrap().bay = Some(bay);
    }
    if let Some(control) = expected_control {
        world.physical_cargo.as_mut().unwrap().control = Some(control);
        control::project_world(&mut world, initial_token_hash)?;
    }
    export_state(&world, entries)?.verify()?;
    if !changed {
        tx.commit()?;
        return Ok(());
    }
    tx.execute(
        "UPDATE world SET json=?1 WHERE id=1",
        [serde_json::to_string(&world)?],
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests;
