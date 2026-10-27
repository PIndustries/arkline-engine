//! Independent inspection-volume replay over the existing economic envelope.
//! No server code or PhysicalCargoAccount transition implementation is called.
//! Accounting service events do not certify geometric contact or normal travel.
use anyhow::{bail, ensure, Context, Result};
use arkline_protocol::{
    physical_cargo::{
        catalog::{PhysicalCargoCatalog, PhysicalCargoDesignBinding},
        ledger::{PhysicalCargoEvent, PHYSICAL_VOLUME_RULESET_VERSION},
        source::physical_source_configuration,
        PhysicalCargoAccount, VolumeCm3, VolumeTransferReceipt,
    },
    EconomicEvent, LedgerEntry, GENESIS_HASH,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
mod bay;
mod control;
use arkline_protocol::physical_cargo::bay::PhysicalBayState;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalClaim {
    pub claim_id: u64,
    pub volume_cm3: VolumeCm3,
    pub due_at_ms: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalDrone {
    pub extracted_cm3: VolumeCm3,
    pub lost_cm3: VolumeCm3,
    pub claim_high_watermark: u64,
    pub payload_cm3: VolumeCm3,
    pub destroyed: bool,
    pub claim: Option<PhysicalClaim>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalService {
    pub service_id: u64,
    pub drone_id: u64,
    pub ready_at_ms: u64,
    pub held: bool,
    pub next_sequence: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalReplay {
    /// Independently reconstructed optional versioned mechanism transcript.
    pub bay: Option<PhysicalBayState>,
    /// Optional explicit owner/target/real-power adapter, independently replayed.
    pub control: Option<arkline_protocol::physical_cargo::control::PhysicalControlState>,
    pub source_configuration_sha256: String,
    pub design: PhysicalCargoDesignBinding,
    pub catalog: PhysicalCargoCatalog,
    pub initial_resource_cm3: VolumeCm3,
    pub resource_remaining_cm3: VolumeCm3,
    pub ship_payload_cm3: VolumeCm3,
    pub lost_cm3: VolumeCm3,
    pub drones: BTreeMap<u64, PhysicalDrone>,
    pub used_claim_ids: BTreeSet<(u64, u64)>,
    pub used_service_ids: BTreeSet<u64>,
    pub active_service: Option<PhysicalService>,
    pub transfers: Vec<VolumeTransferReceipt>,
    pub head_sequence: u64,
    pub head_hash: String,
    pub last_timestamp_ms: u64,
}
fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).context("physical integer overflow")
}
fn sub(a: u64, b: u64) -> Result<u64> {
    a.checked_sub(b).context("physical insufficient volume")
}

impl PhysicalReplay {
    fn incoming(&self) -> Result<u64> {
        self.drones.values().try_fold(0, |sum, drone| {
            add(
                add(sum, drone.payload_cm3.0)?,
                drone.claim.as_ref().map_or(0, |c| c.volume_cm3.0),
            )
        })
    }
    fn claims(&self) -> Result<u64> {
        self.drones.values().try_fold(0, |sum, drone| {
            add(sum, drone.claim.as_ref().map_or(0, |c| c.volume_cm3.0))
        })
    }
    fn invariants(&self) -> Result<()> {
        let manifest = &self.design.manifest;
        let ship_capacity = manifest
            .ship_hold
            .capacity_cm3()
            .context("invalid ship cells")?
            .0;
        let drone_capacity = manifest
            .mining_drone_hold
            .capacity_cm3()
            .context("invalid drone cells")?
            .0;
        ensure!(
            add(self.ship_payload_cm3.0, self.incoming()?)? <= ship_capacity,
            "combined physical ship capacity exceeded"
        );
        ensure!(
            self.claims()? <= self.resource_remaining_cm3.0,
            "unbacked resource claims"
        );
        let mut live = self.ship_payload_cm3.0;
        for drone in self.drones.values() {
            ensure!(
                add(
                    drone.payload_cm3.0,
                    drone.claim.as_ref().map_or(0, |c| c.volume_cm3.0)
                )? <= drone_capacity,
                "physical drone capacity exceeded"
            );
            ensure!(
                !drone.destroyed || (drone.payload_cm3.0 == 0 && drone.claim.is_none()),
                "destroyed drone retains payload or claim"
            );
            live = add(live, drone.payload_cm3.0)?;
        }
        ensure!(
            add(add(live, self.lost_cm3.0)?, self.resource_remaining_cm3.0)?
                == self.initial_resource_cm3.0,
            "physical volume conservation mismatch"
        );
        if let Some(service) = &self.active_service {
            let drone = self
                .drones
                .get(&service.drone_id)
                .context("service drone missing")?;
            ensure!(
                !drone.destroyed && drone.claim.is_none(),
                "service identity invalid"
            );
        }
        Ok(())
    }
    fn apply(&mut self, event: &PhysicalCargoEvent, now: u64) -> Result<()> {
        let m = &self.design.manifest;
        match event {
            PhysicalCargoEvent::Initialized { .. } => bail!("physical genesis repeated"),
            PhysicalCargoEvent::BayBound { .. }
            | PhysicalCargoEvent::BayStep { .. }
            | PhysicalCargoEvent::WorldBound { .. }
            | PhysicalCargoEvent::WorldStep { .. } => {
                bail!("mechanism event bypassed transcript gate")
            }
            PhysicalCargoEvent::Claim {
                drone_id,
                claim_id,
                volume_cm3,
                due_at_ms,
            } => {
                ensure!(
                    *claim_id > 0 && !self.used_claim_ids.contains(&(*drone_id, *claim_id)),
                    "physical claim ID reused"
                );
                ensure!(
                    volume_cm3.0 > 0 && *volume_cm3 <= m.drone_extraction.volume_cm3,
                    "extraction exceeds explicit authored batch"
                );
                ensure!(
                    *due_at_ms == add(now, m.drone_extraction.interval_ms)?,
                    "extraction due time mismatch"
                );
                ensure!(
                    self.active_service
                        .as_ref()
                        .is_none_or(|s| s.drone_id != *drone_id),
                    "serviced drone cannot extract"
                );
                let drone = self
                    .drones
                    .get_mut(drone_id)
                    .context("unknown physical drone")?;
                ensure!(
                    !drone.destroyed
                        && drone.claim.is_none()
                        && *claim_id == add(drone.claim_high_watermark, 1)?,
                    "drone cannot reserve extraction"
                );
                drone.claim_high_watermark = *claim_id;
                drone.claim = Some(PhysicalClaim {
                    claim_id: *claim_id,
                    volume_cm3: *volume_cm3,
                    due_at_ms: *due_at_ms,
                });
                self.used_claim_ids.insert((*drone_id, *claim_id));
            }
            PhysicalCargoEvent::Extract { drone_id, claim_id } => {
                let drone = self
                    .drones
                    .get_mut(drone_id)
                    .context("unknown physical drone")?;
                ensure!(!drone.destroyed, "destroyed extraction drone");
                let claim = drone.claim.take().context("extraction has no live claim")?;
                ensure!(
                    claim.claim_id == *claim_id && now >= claim.due_at_ms,
                    "extraction claim or due time mismatch"
                );
                self.resource_remaining_cm3.0 =
                    sub(self.resource_remaining_cm3.0, claim.volume_cm3.0)?;
                drone.payload_cm3.0 = add(drone.payload_cm3.0, claim.volume_cm3.0)?;
                drone.extracted_cm3.0 = add(drone.extracted_cm3.0, claim.volume_cm3.0)?;
            }
            PhysicalCargoEvent::CancelClaim { drone_id } => {
                let drone = self
                    .drones
                    .get_mut(drone_id)
                    .context("unknown physical drone")?;
                ensure!(
                    !drone.destroyed && drone.claim.take().is_some(),
                    "no cancellable claim"
                );
            }
            PhysicalCargoEvent::ServiceOpened {
                drone_id,
                service_id,
                ready_at_ms,
            } => {
                ensure!(
                    self.active_service.is_none(),
                    "shared manifold already assigned"
                );
                ensure!(
                    *service_id > 0 && !self.used_service_ids.contains(service_id),
                    "physical service ID reused"
                );
                ensure!(
                    *ready_at_ms == add(now, m.vessel_manifold.interval_ms)?,
                    "first transfer delay mismatch"
                );
                let drone = self
                    .drones
                    .get(drone_id)
                    .context("unknown physical drone")?;
                ensure!(
                    !drone.destroyed && drone.claim.is_none() && drone.payload_cm3.0 > 0,
                    "no serviceable payload"
                );
                self.active_service = Some(PhysicalService {
                    service_id: *service_id,
                    drone_id: *drone_id,
                    ready_at_ms: *ready_at_ms,
                    held: false,
                    next_sequence: 1,
                });
                self.used_service_ids.insert(*service_id);
            }
            PhysicalCargoEvent::ServiceHeld { service_id } => {
                let service = self
                    .active_service
                    .as_mut()
                    .context("no active physical service")?;
                ensure!(
                    service.service_id == *service_id && !service.held,
                    "service cannot hold"
                );
                service.held = true;
            }
            PhysicalCargoEvent::ServiceResumed {
                service_id,
                ready_at_ms,
            } => {
                let service = self
                    .active_service
                    .as_mut()
                    .context("no active physical service")?;
                ensure!(
                    service.service_id == *service_id && service.held,
                    "service cannot resume"
                );
                ensure!(
                    *ready_at_ms == add(now, m.vessel_manifold.interval_ms)?,
                    "resume transfer delay mismatch"
                );
                service.ready_at_ms = *ready_at_ms;
                service.held = false;
            }
            PhysicalCargoEvent::Transfer {
                drone_id,
                service_id,
                sequence,
                request_id,
                volume_cm3,
            } => {
                ensure!(
                    *request_id > 0 && !self.transfers.iter().any(|r| r.request_id == *request_id),
                    "transfer request ID repeated"
                );
                let service = self
                    .active_service
                    .as_mut()
                    .context("no active physical service")?;
                ensure!(
                    service.service_id == *service_id
                        && service.drone_id == *drone_id
                        && !service.held
                        && now >= service.ready_at_ms
                        && *sequence == service.next_sequence,
                    "transfer owner, hold, sequence or due time mismatch"
                );
                ensure!(
                    volume_cm3.0 > 0 && *volume_cm3 <= m.vessel_manifold.volume_cm3,
                    "transfer exceeds explicit shared manifold batch"
                );
                let drone = self
                    .drones
                    .get_mut(drone_id)
                    .context("unknown physical drone")?;
                ensure!(
                    !drone.destroyed && drone.claim.is_none(),
                    "invalid transfer drone"
                );
                drone.payload_cm3.0 = sub(drone.payload_cm3.0, volume_cm3.0)?;
                self.ship_payload_cm3.0 = add(self.ship_payload_cm3.0, volume_cm3.0)?;
                service.next_sequence = add(service.next_sequence, 1)?;
                // Never replay missed batches or let a late tick accumulate throughput.
                service.ready_at_ms = add(now, m.vessel_manifold.interval_ms)?;
                self.transfers.push(VolumeTransferReceipt {
                    request_id: *request_id,
                    drone_id: *drone_id,
                    volume_cm3: *volume_cm3,
                    committed_at_ms: now,
                });
            }
            PhysicalCargoEvent::ServiceClosed { service_id } => {
                ensure!(
                    self.active_service
                        .as_ref()
                        .is_some_and(|s| s.service_id == *service_id),
                    "wrong service close"
                );
                self.active_service = None;
            }
            PhysicalCargoEvent::DroneLost { drone_id } => {
                let drone = self
                    .drones
                    .get_mut(drone_id)
                    .context("unknown physical drone")?;
                ensure!(!drone.destroyed, "physical loss repeated");
                self.lost_cm3.0 = add(self.lost_cm3.0, drone.payload_cm3.0)?;
                drone.lost_cm3.0 = add(drone.lost_cm3.0, drone.payload_cm3.0)?;
                drone.payload_cm3 = VolumeCm3(0);
                drone.claim = None;
                drone.destroyed = true;
                if self
                    .active_service
                    .as_ref()
                    .is_some_and(|s| s.drone_id == *drone_id)
                {
                    self.active_service = None;
                }
            }
        }
        self.invariants()
    }
}

/// Reconstruct from exact ruleset-4 genesis. Legacy events/worlds are rejected,
/// not converted; a valid result proves accounting, not physical contact.
pub fn replay(entries: &[LedgerEntry]) -> Result<PhysicalReplay> {
    let first = entries.first().context("physical ledger is empty")?;
    let EconomicEvent::PhysicalCargo {
        record:
            PhysicalCargoEvent::Initialized {
                source_configuration_sha256,
                design,
                catalog,
                mining_drone_ids,
                resource_cm3,
            },
    } = &first.event
    else {
        bail!("physical initialization must be first");
    };
    let source =
        physical_source_configuration().context("physical source certificate unavailable")?;
    ensure!(
        source.identity_sha256().as_ref() == Some(source_configuration_sha256),
        "physical source configuration mismatch"
    );
    ensure!(
        design.valid_against_catalog(catalog),
        "physical catalog or authored design mismatch"
    );
    let measured = source
        .measurements()
        .context("physical source measurements unavailable")?;
    let m = &design.manifest;
    ensure!(
        m.ship_source_sha256 == measured.ship_source_sha256
            && m.mining_drone_source_sha256 == measured.mining_drone_source_sha256
            && m.ship_hold == measured.ship_hold
            && m.mining_drone_hold == measured.mining_drone_hold,
        "physical manifest differs from certified source cells"
    );
    ensure!(
        !mining_drone_ids.is_empty() && mining_drone_ids.len() <= 20,
        "invalid finite physical roster size"
    );
    let mut drones = BTreeMap::new();
    for id in mining_drone_ids {
        ensure!(
            *id > 0 && drones.insert(*id, PhysicalDrone::default()).is_none(),
            "duplicate or zero physical drone ID"
        );
    }
    let mut state = PhysicalReplay {
        bay: None,
        control: None,
        source_configuration_sha256: source_configuration_sha256.clone(),
        design: (**design).clone(),
        catalog: catalog.clone(),
        initial_resource_cm3: *resource_cm3,
        resource_remaining_cm3: *resource_cm3,
        ship_payload_cm3: VolumeCm3(0),
        lost_cm3: VolumeCm3(0),
        drones,
        used_claim_ids: BTreeSet::new(),
        used_service_ids: BTreeSet::new(),
        active_service: None,
        transfers: vec![],
        head_sequence: 0,
        head_hash: GENESIS_HASH.into(),
        last_timestamp_ms: 0,
    };
    let mut pending: VecDeque<PhysicalCargoEvent> = VecDeque::new();
    let mut pending_timestamp = None;
    for (index, entry) in entries.iter().enumerate() {
        ensure!(
            entry.ruleset_version == PHYSICAL_VOLUME_RULESET_VERSION,
            "unsupported physical ruleset (no legacy conversion)"
        );
        ensure!(
            entry.sequence == add(state.head_sequence, 1)?,
            "physical ledger reordered or missing"
        );
        ensure!(
            entry.previous_hash == state.head_hash && entry.hash == entry.calculate_hash(),
            "physical ledger hash mismatch"
        );
        ensure!(
            entry.timestamp_ms >= state.last_timestamp_ms,
            "physical timestamp moved backwards"
        );
        let EconomicEvent::PhysicalCargo { record } = &entry.event else {
            bail!("legacy event in physical ledger");
        };
        if index != 0 {
            if let Some(expected) = pending.pop_front() {
                ensure!(
                    pending_timestamp == Some(entry.timestamp_ms) && *record == expected,
                    "physical bay required economic suffix missing, altered or delayed"
                );
                if !matches!(record, PhysicalCargoEvent::BayStep { .. }) {
                    state
                        .apply(record, entry.timestamp_ms)
                        .with_context(|| format!("physical bay suffix entry {}", entry.sequence))?;
                }
                if pending.is_empty() {
                    pending_timestamp = None;
                }
            } else {
                match record {
                    PhysicalCargoEvent::WorldBound { binding } => {
                        state.control = Some(control::bind(&state, binding, entry.timestamp_ms)?);
                        state.bay.as_mut().expect("validated world bay").world_power = true;
                    }
                    PhysicalCargoEvent::WorldStep { step } => {
                        let (next_control, next_bay, events) =
                            control::step(&state, step, entry.timestamp_ms)?;
                        state.control = Some(next_control);
                        state.bay = Some(next_bay);
                        pending = events.into();
                        if !pending.is_empty() {
                            pending_timestamp = Some(entry.timestamp_ms);
                        }
                    }
                    PhysicalCargoEvent::BayBound { binding } => {
                        ensure!(
                            state.control.is_none(),
                            "raw bay binding after world binding"
                        );
                        state.bay = Some(bay::bind(&state, binding, entry.timestamp_ms)?);
                    }
                    PhysicalCargoEvent::BayStep { step } => {
                        ensure!(
                            state.control.is_none(),
                            "raw bay step bypassed world authority"
                        );
                        let old = state
                            .bay
                            .as_ref()
                            .context("physical bay step before binding")?;
                        let (next, events) = bay::step(&state, old, step, entry.timestamp_ms)?;
                        state.bay = Some(next);
                        pending = events.into();
                        if !pending.is_empty() {
                            pending_timestamp = Some(entry.timestamp_ms);
                        }
                    }
                    _ => {
                        ensure!(
                            state.bay.is_none(),
                            "unacknowledged accounting event after physical bay binding"
                        );
                        state
                            .apply(record, entry.timestamp_ms)
                            .with_context(|| format!("physical entry {}", entry.sequence))?;
                    }
                }
            }
        }
        state.invariants()?;
        if pending.is_empty() {
            if let Some(control) = &state.control {
                ensure!(
                    control.projection.resource_remaining_cm3 == state.resource_remaining_cm3,
                    "world target differs from volume ledger"
                );
                ensure!(state.bay.as_ref().is_some_and(|b| b.world_power
                    && b.health == control.projection.hit_points), "world actor differs from mechanism");
            }
        }
        state.head_sequence = entry.sequence;
        state.head_hash = entry.hash.clone();
        state.last_timestamp_ms = entry.timestamp_ms;
    }
    ensure!(
        pending.is_empty(),
        "physical bay transaction ended before required economic suffix"
    );
    Ok(state)
}

/// Compare an independently reconstructed replay with the authority's persisted
/// account. This deliberately does not invoke account validation or mutations.
pub fn verify_account(replayed: &PhysicalReplay, account: &PhysicalCargoAccount) -> Result<()> {
    ensure!(
        Some(&account.manifest_sha256) == replayed.design.manifest.identity_sha256().as_ref(),
        "account manifest mismatch"
    );
    ensure!(
        account.ship_payload_cm3 == replayed.ship_payload_cm3
            && account.destroyed_cm3 == replayed.lost_cm3
            && account.extracted_cm3.0
                == sub(
                    replayed.initial_resource_cm3.0,
                    replayed.resource_remaining_cm3.0
                )?
            && account.extracted_to_ship_cm3.0 == 0
            && account.exported_cm3.0 == 0,
        "authority physical totals differ from replay"
    );
    ensure!(
        account.transfers == replayed.transfers,
        "authority physical transfer receipts differ from replay"
    );
    ensure!(
        account.drones.len() == replayed.drones.len(),
        "authority physical roster differs from replay"
    );
    let mut seen = BTreeSet::new();
    for hold in &account.drones {
        ensure!(
            seen.insert(hold.drone_id),
            "duplicate authority physical identity"
        );
        let drone = replayed
            .drones
            .get(&hold.drone_id)
            .context("unknown authority physical identity")?;
        ensure!(
            hold.payload_cm3 == drone.payload_cm3
                && hold.extracted_cm3 == drone.extracted_cm3
                && hold.lost_cm3 == drone.lost_cm3
                && hold.destroyed == drone.destroyed
                && hold.claim_high_watermark == drone.claim_high_watermark,
            "authority physical drone differs from replay"
        );
        ensure!(
            hold.claim.as_ref().map(|c| (c.claim_id, c.volume_cm3))
                == drone.claim.as_ref().map(|c| (c.claim_id, c.volume_cm3)),
            "authority physical claim differs from replay"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arkline_protocol::physical_cargo::{
        catalog::{AuthoredCargoOperatingRatings, PhysicalCargoForm, PhysicalCargoMaterial},
        AuthoredVolumeRate, CargoMassKnowledge,
    };
    fn catalog() -> PhysicalCargoCatalog {
        PhysicalCargoCatalog {
            schema_version: 1,
            materials: vec![PhysicalCargoMaterial {
                material_id: "inspection.bulk".into(),
                form: PhysicalCargoForm::BulkVolumeCm3,
                mass: CargoMassKnowledge::Unknown,
                authored_price: None,
            }],
            operating_ratings: vec![AuthoredCargoOperatingRatings {
                rating_id: "inspection.synthetic".into(),
                revision: 1,
                drone_extraction: AuthoredVolumeRate {
                    volume_cm3: VolumeCm3(100_000),
                    interval_ms: 10,
                },
                ship_extraction: AuthoredVolumeRate {
                    volume_cm3: VolumeCm3(1),
                    interval_ms: 10,
                },
                vessel_manifold: AuthoredVolumeRate {
                    volume_cm3: VolumeCm3(60_000),
                    interval_ms: 5,
                },
                drone_batch_energy_units: 1,
                ship_batch_energy_units: 1,
            }],
        }
    }
    pub(super) fn init() -> PhysicalCargoEvent {
        let source = physical_source_configuration().unwrap();
        let catalog = catalog();
        PhysicalCargoEvent::Initialized {
            source_configuration_sha256: source.identity_sha256().unwrap(),
            design: Box::new(
                catalog
                    .bind_manifest(
                        "inspection.bulk",
                        "inspection.synthetic",
                        source.measurements().unwrap(),
                    )
                    .unwrap(),
            ),
            catalog,
            mining_drone_ids: vec![1, 2],
            resource_cm3: VolumeCm3(1_000_000),
        }
    }
    pub(super) fn append(entries: &mut Vec<LedgerEntry>, time: u64, event: PhysicalCargoEvent) {
        let mut entry = LedgerEntry {
            sequence: entries.len() as u64 + 1,
            ruleset_version: PHYSICAL_VOLUME_RULESET_VERSION,
            timestamp_ms: time,
            previous_hash: entries.last().map_or(GENESIS_HASH, |e| &e.hash).into(),
            event: EconomicEvent::PhysicalCargo { record: event },
            hash: String::new(),
        };
        entry.hash = entry.calculate_hash();
        entries.push(entry);
    }
    pub(super) fn rehash(entries: &mut [LedgerEntry]) {
        let mut previous = GENESIS_HASH.to_owned();
        for entry in entries {
            entry.previous_hash = previous;
            entry.hash = entry.calculate_hash();
            previous = entry.hash.clone();
        }
    }
    fn valid_ledger() -> Vec<LedgerEntry> {
        let mut entries = vec![];
        append(&mut entries, 100, init());
        append(
            &mut entries,
            100,
            PhysicalCargoEvent::Claim {
                drone_id: 1,
                claim_id: 1,
                volume_cm3: VolumeCm3(100_000),
                due_at_ms: 110,
            },
        );
        append(
            &mut entries,
            110,
            PhysicalCargoEvent::Extract {
                drone_id: 1,
                claim_id: 1,
            },
        );
        append(
            &mut entries,
            110,
            PhysicalCargoEvent::ServiceOpened {
                drone_id: 1,
                service_id: 1,
                ready_at_ms: 115,
            },
        );
        append(
            &mut entries,
            115,
            PhysicalCargoEvent::Transfer {
                drone_id: 1,
                service_id: 1,
                sequence: 1,
                request_id: 1,
                volume_cm3: VolumeCm3(60_000),
            },
        );
        append(
            &mut entries,
            116,
            PhysicalCargoEvent::ServiceHeld { service_id: 1 },
        );
        append(
            &mut entries,
            200,
            PhysicalCargoEvent::ServiceResumed {
                service_id: 1,
                ready_at_ms: 205,
            },
        );
        append(
            &mut entries,
            205,
            PhysicalCargoEvent::Transfer {
                drone_id: 1,
                service_id: 1,
                sequence: 2,
                request_id: 2,
                volume_cm3: VolumeCm3(40_000),
            },
        );
        append(
            &mut entries,
            206,
            PhysicalCargoEvent::ServiceClosed { service_id: 1 },
        );
        append(
            &mut entries,
            207,
            PhysicalCargoEvent::Claim {
                drone_id: 2,
                claim_id: 1,
                volume_cm3: VolumeCm3(100_000),
                due_at_ms: 217,
            },
        );
        append(
            &mut entries,
            217,
            PhysicalCargoEvent::Extract {
                drone_id: 2,
                claim_id: 1,
            },
        );
        append(
            &mut entries,
            218,
            PhysicalCargoEvent::DroneLost { drone_id: 2 },
        );
        entries
    }
    #[test]
    fn physical_replay_conserves_real_units_and_checks_exact_account() {
        let entries = valid_ledger();
        let replayed = replay(&entries).unwrap();
        assert_eq!(replayed.ship_payload_cm3, VolumeCm3(100_000));
        assert_eq!(replayed.lost_cm3, VolumeCm3(100_000));
        assert_eq!(replayed.resource_remaining_cm3, VolumeCm3(800_000));
        assert!(replayed.drones[&2].destroyed);
        let m = &replayed.design.manifest;
        let mut account = PhysicalCargoAccount::fresh(m, &[1, 2]).unwrap();
        assert!(account.reserve_extraction(m, 1, 1, VolumeCm3(100_000)));
        assert!(account.complete_extraction(m, 1, 1));
        assert!(account.transfer(m, 1, 1, VolumeCm3(60_000), 115));
        assert!(account.transfer(m, 2, 1, VolumeCm3(40_000), 205));
        assert!(account.reserve_extraction(m, 2, 1, VolumeCm3(100_000)));
        assert!(account.complete_extraction(m, 2, 1));
        assert!(account.destroy_drone(m, 2));
        verify_account(&replayed, &account).unwrap();
        account.drones[0].claim_high_watermark += 1;
        assert!(verify_account(&replayed, &account).is_err());
        let bytes = serde_json::to_vec(&entries).unwrap();
        let restored = serde_json::from_slice::<Vec<LedgerEntry>>(&bytes).unwrap();
        assert_eq!(replay(&restored).unwrap(), replayed);
    }
    #[test]
    fn physical_replay_rejects_rehashed_semantic_tampering() {
        for case in 0..13 {
            let mut entries = valid_ledger();
            match case {
                0 => entries[1].ruleset_version = 3,
                1 => entries[2].timestamp_ms = 109,
                2 => entries[4].timestamp_ms = 114,
                3 => entries[7].timestamp_ms = 204,
                4 => {
                    if let EconomicEvent::PhysicalCargo {
                        record: PhysicalCargoEvent::Transfer { sequence, .. },
                    } = &mut entries[7].event
                    {
                        *sequence = 1;
                    }
                }
                5 => {
                    if let EconomicEvent::PhysicalCargo {
                        record: PhysicalCargoEvent::Transfer { request_id, .. },
                    } = &mut entries[7].event
                    {
                        *request_id = 1;
                    }
                }
                6 => {
                    if let EconomicEvent::PhysicalCargo {
                        record: PhysicalCargoEvent::Claim { volume_cm3, .. },
                    } = &mut entries[1].event
                    {
                        *volume_cm3 = VolumeCm3(100_001);
                    }
                }
                7 => {
                    if let EconomicEvent::PhysicalCargo {
                        record: PhysicalCargoEvent::Transfer { volume_cm3, .. },
                    } = &mut entries[4].event
                    {
                        *volume_cm3 = VolumeCm3(60_001);
                    }
                }
                8 => {
                    if let EconomicEvent::PhysicalCargo {
                        record:
                            PhysicalCargoEvent::Initialized {
                                source_configuration_sha256,
                                ..
                            },
                    } = &mut entries[0].event
                    {
                        *source_configuration_sha256 = "0".repeat(64);
                    }
                }
                9 => {
                    if let EconomicEvent::PhysicalCargo {
                        record:
                            PhysicalCargoEvent::Initialized {
                                mining_drone_ids, ..
                            },
                    } = &mut entries[0].event
                    {
                        *mining_drone_ids = vec![1, 1];
                    }
                }
                10 => {
                    if let EconomicEvent::PhysicalCargo {
                        record: PhysicalCargoEvent::Initialized { resource_cm3, .. },
                    } = &mut entries[0].event
                    {
                        *resource_cm3 = VolumeCm3(99_999);
                    }
                }
                11 => {
                    entries[1].event = EconomicEvent::PilotCreated {
                        pilot: "Pilot".into(),
                    }
                }
                _ => entries[3].event = entries[0].event.clone(),
            }
            rehash(&mut entries);
            assert!(replay(&entries).is_err(), "tamper case {case} accepted");
        }
    }
    #[test]
    fn physical_replay_rejects_oversized_source_cells_and_rate_overflow() {
        let mut entries = vec![];
        let mut genesis = init();
        if let PhysicalCargoEvent::Initialized {
            design, catalog, ..
        } = &mut genesis
        {
            let mut measured = physical_source_configuration()
                .unwrap()
                .measurements()
                .unwrap();
            measured.mining_drone_hold.usable_cells[0].max[0] += 1;
            **design = catalog
                .bind_manifest("inspection.bulk", "inspection.synthetic", measured)
                .unwrap();
        }
        append(&mut entries, 100, genesis);
        assert!(replay(&entries).is_err());
        let mut entries = vec![];
        append(&mut entries, u64::MAX - 2, init());
        append(
            &mut entries,
            u64::MAX - 2,
            PhysicalCargoEvent::Claim {
                drone_id: 1,
                claim_id: 1,
                volume_cm3: VolumeCm3(1),
                due_at_ms: u64::MAX,
            },
        );
        assert!(replay(&entries).is_err());
    }
    #[test]
    fn physical_replay_rejects_held_transfer_repeat_loss_and_shared_manifold() {
        let mut held = valid_ledger();
        held.truncate(6);
        append(
            &mut held,
            500,
            PhysicalCargoEvent::Transfer {
                drone_id: 1,
                service_id: 1,
                sequence: 2,
                request_id: 2,
                volume_cm3: VolumeCm3(40_000),
            },
        );
        assert!(replay(&held).is_err());
        let mut repeated_loss = valid_ledger();
        append(
            &mut repeated_loss,
            300,
            PhysicalCargoEvent::DroneLost { drone_id: 2 },
        );
        assert!(replay(&repeated_loss).is_err());
        let mut second = valid_ledger();
        second.truncate(4);
        append(
            &mut second,
            110,
            PhysicalCargoEvent::ServiceOpened {
                drone_id: 1,
                service_id: 2,
                ready_at_ms: 115,
            },
        );
        assert!(replay(&second).is_err());
    }
    #[test]
    fn physical_replay_rejects_hash_sequence_and_time_rewrites() {
        let original = valid_ledger();
        let mut bad = original.clone();
        bad[3].hash = "a".repeat(64);
        assert!(replay(&bad).is_err());
        let mut bad = original.clone();
        bad[3].sequence += 1;
        rehash(&mut bad);
        assert!(replay(&bad).is_err());
        let mut bad = original;
        bad[8].timestamp_ms = 100;
        rehash(&mut bad);
        assert!(replay(&bad).is_err());
    }
    #[test]
    fn physical_replay_capacity_limits_are_geometry_not_authored_rate() {
        let mut genesis = init();
        if let PhysicalCargoEvent::Initialized {
            design,
            catalog,
            resource_cm3,
            ..
        } = &mut genesis
        {
            catalog.operating_ratings[0].drone_extraction.volume_cm3 = VolumeCm3(1_000_000);
            catalog.operating_ratings[0].vessel_manifold.volume_cm3 = VolumeCm3(1_000_000);
            **design = catalog
                .bind_manifest(
                    "inspection.bulk",
                    "inspection.synthetic",
                    physical_source_configuration()
                        .unwrap()
                        .measurements()
                        .unwrap(),
                )
                .unwrap();
            *resource_cm3 = VolumeCm3(214_121_239);
        }
        let mut oversized = vec![];
        append(&mut oversized, 100, genesis.clone());
        append(
            &mut oversized,
            100,
            PhysicalCargoEvent::Claim {
                drone_id: 1,
                claim_id: 1,
                volume_cm3: VolumeCm3(893_834),
                due_at_ms: 110,
            },
        );
        assert!(replay(&oversized).is_err());
        let mut filled = vec![];
        append(&mut filled, 100, genesis);
        let mut remaining = 214_121_238u64;
        let mut id = 0;
        let mut now = 100;
        while remaining > 0 {
            id += 1;
            let volume = VolumeCm3(remaining.min(893_833));
            append(
                &mut filled,
                now,
                PhysicalCargoEvent::Claim {
                    drone_id: 1,
                    claim_id: id,
                    volume_cm3: volume,
                    due_at_ms: now + 10,
                },
            );
            now += 10;
            append(
                &mut filled,
                now,
                PhysicalCargoEvent::Extract {
                    drone_id: 1,
                    claim_id: id,
                },
            );
            append(
                &mut filled,
                now,
                PhysicalCargoEvent::ServiceOpened {
                    drone_id: 1,
                    service_id: id,
                    ready_at_ms: now + 5,
                },
            );
            now += 5;
            append(
                &mut filled,
                now,
                PhysicalCargoEvent::Transfer {
                    drone_id: 1,
                    service_id: id,
                    sequence: 1,
                    request_id: id,
                    volume_cm3: volume,
                },
            );
            append(
                &mut filled,
                now,
                PhysicalCargoEvent::ServiceClosed { service_id: id },
            );
            remaining -= volume.0;
        }
        let state = replay(&filled).unwrap();
        assert_eq!(state.ship_payload_cm3, VolumeCm3(214_121_238));
        assert_eq!(state.resource_remaining_cm3, VolumeCm3(1));
        append(
            &mut filled,
            now,
            PhysicalCargoEvent::Claim {
                drone_id: 1,
                claim_id: id + 1,
                volume_cm3: VolumeCm3(1),
                due_at_ms: now + 10,
            },
        );
        assert!(replay(&filled).is_err());
    }
}
