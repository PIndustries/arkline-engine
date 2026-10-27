//! Ruleset three changes identity only; economic bounds reuse the old replay.
//! Recognizing a source for economic replay does not admit its runtime routes,
//! prove physical contact, or enable the normal-world feature.
use super::*;
type VesselKey = (String, String);
type Genesis = (CarrierInventoryBinding, Vec<CarrierRosterIdentity>, u64);
type ServiceKey = (String, String, u64);
#[derive(Default)]
pub(super) struct CarrierReplay {
    genesis: BTreeMap<VesselKey, Genesis>,
    services: BTreeMap<ServiceKey, (u64, u8)>,
    losses: BTreeMap<String, u64>,
    destroyed: BTreeMap<ServiceKey, u64>,
}
impl CarrierReplay {
    #[allow(clippy::too_many_arguments)] // Keep the legacy replay inputs unchanged.
    pub(super) fn apply(
        &mut self,
        record: &CarrierInventoryLedgerEvent,
        entry: &LedgerEntry,
        registration: &CarrierInventoryRegistration,
        drones: &mut DroneReplay,
        accounts: &mut BTreeMap<String, Account>,
        belt: &mut u64,
    ) -> Result<()> {
        let converted = match record {
            CarrierInventoryLedgerEvent::Initialized {
                pilot,
                binding,
                roster,
                capacity_units_per_mining_drone,
            } => {
                let def =
                    carrier_inventory_geometry::stationary_fixture_for_registration(registration)
                        .context("unsupported carrier source")?;
                ensure!(
                    binding.well_formed() && &binding.registration == registration,
                    "carrier genesis source mismatch"
                );
                ensure!(
                    accounts
                        .get(pilot)
                        .context("unknown carrier owner")?
                        .active_class
                        == Some(ShipClass::Pioneer),
                    "carrier requires Pioneer"
                );
                ensure!(
                    *capacity_units_per_mining_drone == MINING_DRONE_CARGO_CAPACITY
                        && roster.len() == def.topology.capacity(),
                    "invalid finite carrier capacity"
                );
                let mut ids = BTreeSet::new();
                for (identity, cassette) in roster.iter().zip(&def.cassettes) {
                    ensure!(
                        identity.drone_id > 0
                            && ids.insert(identity.drone_id)
                            && !self.genesis.values().any(|(_, existing, _)| existing
                                .iter()
                                .any(|r| r.drone_id == identity.drone_id)),
                        "duplicate or zero carrier identity"
                    );
                    ensure!(
                        def.topology.accepts(cassette.initial_dock, identity.class),
                        "roster class incompatible with source dock"
                    );
                    ensure!(
                        identity.preferred_magazine_slot.is_none_or(|id| def
                            .topology
                            .slot(id)
                            .is_some_and(|s| s.compatible_classes.contains(&identity.class))),
                        "invalid roster storage preference"
                    );
                }
                ensure!(
                    !self
                        .genesis
                        .keys()
                        .any(|(p, v)| p == pilot || v == &binding.vessel_id),
                    "duplicate carrier initialization"
                );
                let key = (pilot.clone(), binding.vessel_id.clone());
                drones.vessels.insert(
                    key.clone(),
                    roster
                        .iter()
                        .filter(|r| r.class == DroneClass::Mining)
                        .map(|r| {
                            (
                                r.drone_id,
                                ReplayedDrone {
                                    last_finished_at: entry.timestamp_ms,
                                    ..Default::default()
                                },
                            )
                        })
                        .collect(),
                );
                self.genesis
                    .insert(key, (binding.clone(), roster.clone(), entry.timestamp_ms));
                return Ok(());
            }
            CarrierInventoryLedgerEvent::Extracted {
                pilot,
                vessel_id,
                drone_id,
                cycle_id,
                target_id,
                cycle_started_at_ms,
                units,
                job_id,
            } => DroneCargoLedgerEvent::Extracted {
                pilot: pilot.clone(),
                vessel_id: vessel_id.clone(),
                drone_id: *drone_id,
                cycle_id: *cycle_id,
                target_id: *target_id,
                cycle_started_at_ms: *cycle_started_at_ms,
                units: *units,
                job_id: *job_id,
            },
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
            } => {
                let def =
                    carrier_inventory_geometry::stationary_fixture_for_registration(registration)
                        .context("unsupported carrier source")?;
                ensure!(
                    *physical_operation_id > 0 && def.topology.berth(*berth_id).is_some(),
                    "invalid physical receiver identity"
                );
                let balance = drones
                    .vessels
                    .get(&(pilot.clone(), vessel_id.clone()))
                    .and_then(|v| v.get(drone_id))
                    .context("unowned carrier service drone")?;
                ensure!(
                    *service_started_at_ms >= balance.last_finished_at,
                    "service predates cargo acquisition"
                );
                let key = (pilot.clone(), vessel_id.clone(), *service_id);
                let identity = (*physical_operation_id, *berth_id);
                ensure!(
                    self.services.get(&key).is_none_or(|old| *old == identity),
                    "service physical binding changed"
                );
                self.services.insert(key, identity);
                DroneCargoLedgerEvent::Transferred {
                    pilot: pilot.clone(),
                    vessel_id: vessel_id.clone(),
                    drone_id: *drone_id,
                    operation_id: *service_id,
                    operation_started_at_ms: *service_started_at_ms,
                    requested_units: *requested_units,
                    sequence: *sequence,
                    units: *units,
                }
            }
            CarrierInventoryLedgerEvent::Destroyed {
                pilot,
                vessel_id,
                drone_id,
                destroyed_at_ms,
                lost_units,
            } => {
                let key = (pilot.clone(), vessel_id.clone());
                let (_, roster, _) = self
                    .genesis
                    .get(&key)
                    .context("uninitialized destroyed carrier")?;
                let identity = roster
                    .iter()
                    .find(|r| r.drone_id == *drone_id)
                    .context("unowned destroyed carrier drone")?;
                let dead_key = (pilot.clone(), vessel_id.clone(), *drone_id);
                ensure!(
                    *destroyed_at_ms == entry.timestamp_ms
                        && !self.destroyed.contains_key(&dead_key),
                    "duplicate destruction or timestamp mismatch"
                );
                if identity.class != DroneClass::Mining {
                    ensure!(*lost_units == 0, "non-mining identity claimed cargo loss");
                    self.destroyed.insert(dead_key, *destroyed_at_ms);
                    return Ok(());
                }
                self.destroyed.insert(dead_key, *destroyed_at_ms);
                *self.losses.entry(pilot.clone()).or_default() =
                    add(*self.losses.get(pilot).unwrap_or(&0), *lost_units)?;
                DroneCargoLedgerEvent::Destroyed {
                    pilot: pilot.clone(),
                    vessel_id: vessel_id.clone(),
                    drone_id: *drone_id,
                    destroyed_at_ms: *destroyed_at_ms,
                    lost_units: *lost_units,
                }
            }
        };
        // Only non-initialization variants enter the unchanged legacy arithmetic.
        drones.apply(
            &converted,
            entry,
            &supported_drone_cargo_registration(),
            accounts,
            belt,
        )
    }
    pub(super) fn snapshot(
        &self,
        snapshot: &Snapshot,
        drones: &DroneReplay,
        registration: &CarrierInventoryRegistration,
    ) -> Result<()> {
        let def = carrier_inventory_geometry::stationary_fixture_for_registration(registration)
            .context("unsupported carrier source")?;
        let mut seen = BTreeSet::new();
        let mut seen_transfers = BTreeSet::new();
        for pilot in &snapshot.pilots {
            ensure!(
                pilot.ship_class == ShipClass::Pioneer
                    && pilot.drone_cargo.is_none()
                    && pilot.drone_bay.is_none(),
                "mixed carrier hull or legacy state"
            );
            let public = pilot
                .carrier_inventory
                .as_ref()
                .context("missing carrier snapshot")?;
            let inventory = &public.inventory;
            let ore = &public.ore;
            let key = (pilot.name.clone(), inventory.binding.vessel_id.clone());
            let (binding, roster, initialized) = self
                .genesis
                .get(&key)
                .context("uninitialized snapshot carrier")?;
            ensure!(
                seen.insert(key.clone())
                    && &inventory.binding == binding
                    && &inventory.roster == roster
                    && inventory.initialized_at_ms == *initialized,
                "snapshot genesis identity mismatch"
            );
            ensure!(
                inventory.validate_shape(&def.topology) && ore.validate_shape(inventory),
                "malformed carrier snapshot"
            );
            ensure!(
                public.sample_time_ms <= snapshot.server_time_ms
                    && public.sample_time_ms >= *initialized,
                "invalid carrier sample time"
            );
            let actual = pilot
                .flight
                .as_ref()
                .and_then(|f| f.docking.as_ref())
                .context("missing carrier host")?;
            ensure!(
                actual.host_vessel_id == binding.vessel_id,
                "carrier host identity mismatch"
            );
            for identity in &inventory.entries {
                let dead = self.destroyed.get(&(
                    pilot.name.clone(),
                    binding.vessel_id.clone(),
                    identity.drone_id,
                ));
                ensure!(
                    match (&identity.location, dead) {
                        (CarrierDroneLocation::Destroyed { at_ms }, Some(t)) =>
                            at_ms == t && identity.hit_points == 0,
                        (CarrierDroneLocation::Destroyed { .. }, None) | (_, Some(_)) => false,
                        _ => identity.hit_points > 0,
                    },
                    "snapshot carrier death differs from ledger"
                );
            }
            let balances = drones
                .vessels
                .get(&key)
                .context("missing carrier balances")?;
            ensure!(
                ore.holds.len() == balances.len()
                    && ore.lost_ore_units == *self.losses.get(&pilot.name).unwrap_or(&0),
                "carrier balances or sink mismatch"
            );
            for hold in &ore.holds {
                let replay = balances
                    .get(&hold.drone_id)
                    .context("fabricated carrier hold")?;
                ensure!(
                    hold.ore_units == replay.ore
                        && hold.destroyed_at_ms
                            == self
                                .destroyed
                                .get(&(
                                    pilot.name.clone(),
                                    binding.vessel_id.clone(),
                                    hold.drone_id
                                ))
                                .copied()
                        && hold.destroyed_at_ms.is_some() == replay.destroyed
                        && hold.cycle_high_watermark >= replay.last_cycle,
                    "carrier hold differs from replay"
                );
                if let Some(cycle) = &hold.cycle {
                    ensure!(
                        cycle.cycle_id > replay.last_cycle
                            && cycle.started_at_ms >= replay.last_finished_at
                            && cycle.started_at_ms <= snapshot.server_time_ms
                            && asteroid_catalog(INITIAL_BELT_ORE)
                                .iter()
                                .any(|a| a.id == cycle.target_id)
                            && cycle.job_id.is_none_or(|id| id > 0),
                        "invalid pending carrier cycle"
                    );
                }
            }
            for service in &ore.services {
                let k = (
                    pilot.name.clone(),
                    binding.vessel_id.clone(),
                    service.service_id,
                );
                if let Some(replay) = drones.transfers.get(&k) {
                    ensure!(
                        seen_transfers.insert(k.clone())
                            && self.services.get(&k)
                                == Some(&(service.physical_operation_id, service.berth_id))
                            && service.drone_id == replay.drone
                            && service.accepted_at_ms == replay.started
                            && service.requested_units == replay.requested
                            && service.receipts == replay.receipts,
                        "carrier receipt identity mismatch"
                    );
                } else {
                    ensure!(service.receipts.is_empty(), "fabricated carrier receipt");
                }
            }
            ensure!(
                add(
                    pilot.ore,
                    ore.reserved_incoming()
                        .context("carrier reservation overflow")?
                )? <= cargo_capacity(pilot.ship_class),
                "carrier headroom exceeded"
            );
        }
        ensure!(
            seen.len() == self.genesis.len() && seen_transfers.len() == drones.transfers.len(),
            "carrier journal omitted from snapshot"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Harness {
        carrier: CarrierReplay,
        drones: DroneReplay,
        accounts: BTreeMap<String, Account>,
        belt: u64,
        registration: CarrierInventoryRegistration,
    }
    impl Harness {
        fn new() -> Self {
            Self {
                carrier: Default::default(),
                drones: Default::default(),
                accounts: BTreeMap::from([(
                    "Alice".into(),
                    Account {
                        active_class: Some(ShipClass::Pioneer),
                        ..Default::default()
                    },
                )]),
                belt: INITIAL_BELT_ORE,
                registration: carrier_inventory_geometry::stationary_fixture_definition()
                    .unwrap()
                    .topology
                    .registration
                    .clone(),
            }
        }
        fn init(&self, mining: usize) -> CarrierInventoryLedgerEvent {
            CarrierInventoryLedgerEvent::Initialized {
                pilot: "Alice".into(),
                binding: self.registration.bind("vessel".into()),
                roster: (1..=20)
                    .map(|id| CarrierRosterIdentity {
                        drone_id: id,
                        class: if id as usize <= mining {
                            DroneClass::Mining
                        } else {
                            DroneClass::Attack
                        },
                        preferred_magazine_slot: None,
                    })
                    .collect(),
                capacity_units_per_mining_drone: 100,
            }
        }
        fn apply(&mut self, record: CarrierInventoryLedgerEvent, time: u64) -> Result<()> {
            let mut entry = LedgerEntry {
                sequence: time + 1,
                ruleset_version: 3,
                timestamp_ms: time,
                previous_hash: GENESIS_HASH.into(),
                event: EconomicEvent::CarrierInventory {
                    record: record.clone(),
                },
                hash: String::new(),
            };
            entry.hash = entry.calculate_hash();
            self.carrier.apply(
                &record,
                &entry,
                &self.registration,
                &mut self.drones,
                &mut self.accounts,
                &mut self.belt,
            )
        }
        fn extract(id: u64, cycle: u64, start: u64) -> CarrierInventoryLedgerEvent {
            CarrierInventoryLedgerEvent::Extracted {
                pilot: "Alice".into(),
                vessel_id: "vessel".into(),
                drone_id: id,
                cycle_id: cycle,
                target_id: asteroid_catalog(INITIAL_BELT_ORE)[0].id,
                cycle_started_at_ms: start,
                units: 10,
                job_id: None,
            }
        }
        fn transfer(id: u64, service: u64, start: u64) -> CarrierInventoryLedgerEvent {
            CarrierInventoryLedgerEvent::Transferred {
                pilot: "Alice".into(),
                vessel_id: "vessel".into(),
                drone_id: id,
                physical_operation_id: service,
                berth_id: (id - 1) as u8,
                service_id: service,
                service_started_at_ms: start,
                requested_units: 10,
                sequence: 1,
                units: 10,
            }
        }
    }
    #[test]
    fn twenty_explicit_miners_and_mixed_rosters_replay() {
        for mining in [0, 5, 10, 20] {
            let mut h = Harness::new();
            h.apply(h.init(mining), 0).unwrap();
            for id in 1..=mining as u64 {
                h.apply(Harness::extract(id, 1, 0), 8000).unwrap();
            }
            assert_eq!(h.drones.total().unwrap(), mining as u64 * 10);
            assert_eq!(h.belt, INITIAL_BELT_ORE - mining as u64 * 10);
        }
    }
    #[test]
    fn invalid_roster_source_and_capacity_rejected() {
        for case in 0..5 {
            let mut h = Harness::new();
            let mut event = h.init(20);
            if let CarrierInventoryLedgerEvent::Initialized {
                roster,
                binding,
                capacity_units_per_mining_drone,
                ..
            } = &mut event
            {
                match case {
                    0 => roster[1].drone_id = 1,
                    1 => roster[0].drone_id = 0,
                    2 => roster.push(roster[0].clone()),
                    3 => binding.registration.assembly_source_sha256 = "a".repeat(64),
                    _ => *capacity_units_per_mining_drone = 101,
                }
            }
            assert!(h.apply(event, 0).is_err());
        }
    }
    #[test]
    fn fabricated_nonmining_fast_reused_and_dead_yield_rejected() {
        for case in 0..5 {
            let mut h = Harness::new();
            h.apply(h.init(10), 0).unwrap();
            h.apply(Harness::extract(1, 1, 0), 8000).unwrap();
            let (event, time) = match case {
                0 => (Harness::extract(21, 1, 0), 8000),
                1 => (Harness::extract(11, 1, 0), 8000),
                2 => (Harness::extract(1, 2, 8000), 15999),
                3 => (Harness::extract(1, 1, 8000), 16000),
                _ => {
                    h.apply(
                        CarrierInventoryLedgerEvent::Destroyed {
                            pilot: "Alice".into(),
                            vessel_id: "vessel".into(),
                            drone_id: 1,
                            destroyed_at_ms: 8000,
                            lost_units: 10,
                        },
                        8000,
                    )
                    .unwrap();
                    (Harness::extract(1, 2, 8000), 16000)
                }
            };
            assert!(h.apply(event, time).is_err());
        }
    }
    #[test]
    fn global_receiver_spacing_retry_and_physical_binding() {
        for case in 0..4 {
            let mut h = Harness::new();
            h.apply(h.init(20), 0).unwrap();
            for id in 1..=2 {
                h.apply(Harness::extract(id, 1, 0), 8000).unwrap();
            }
            h.apply(Harness::transfer(1, 1, 8000), 8500).unwrap();
            match case {
                0 => {
                    h.apply(Harness::transfer(2, 2, 8500), 9000).unwrap();
                    assert_eq!(h.accounts["Alice"].ore, 20);
                }
                1 => assert!(h.apply(Harness::transfer(2, 2, 8000), 8999).is_err()),
                2 => assert!(h.apply(Harness::transfer(1, 1, 8000), 9000).is_err()),
                _ => {
                    let mut e = Harness::transfer(1, 1, 8000);
                    if let CarrierInventoryLedgerEvent::Transferred { berth_id, .. } = &mut e {
                        *berth_id = 2
                    };
                    assert!(h.apply(e, 9000).is_err());
                }
            }
        }
    }
    #[test]
    fn nonmining_death_has_zero_loss_and_is_not_reusable() {
        let mut h = Harness::new();
        h.apply(h.init(5), 0).unwrap();
        let event = CarrierInventoryLedgerEvent::Destroyed {
            pilot: "Alice".into(),
            vessel_id: "vessel".into(),
            drone_id: 20,
            destroyed_at_ms: 1,
            lost_units: 0,
        };
        h.apply(event.clone(), 1).unwrap();
        assert!(h.apply(event, 1).is_err());
        assert_eq!(h.drones.lost, 0);
        let mut h = Harness::new();
        h.apply(h.init(5), 0).unwrap();
        assert!(h
            .apply(
                CarrierInventoryLedgerEvent::Destroyed {
                    pilot: "Alice".into(),
                    vessel_id: "vessel".into(),
                    drone_id: 20,
                    destroyed_at_ms: 1,
                    lost_units: 1
                },
                1
            )
            .is_err());
    }
    #[test]
    fn finite_ids_cannot_be_duplicated_across_vessels() {
        let mut h = Harness::new();
        h.apply(h.init(20), 0).unwrap();
        h.accounts.insert(
            "Bob".into(),
            Account {
                active_class: Some(ShipClass::Pioneer),
                ..Default::default()
            },
        );
        let mut event = h.init(20);
        if let CarrierInventoryLedgerEvent::Initialized { pilot, binding, .. } = &mut event {
            *pilot = "Bob".into();
            binding.vessel_id = "second-vessel".into();
        }
        assert!(h.apply(event, 0).is_err());
    }
    #[test]
    fn mining_hold_capacity_and_loss_are_conserved() {
        let mut h = Harness::new();
        h.apply(h.init(20), 0).unwrap();
        for cycle in 1..=10 {
            h.apply(Harness::extract(1, cycle, (cycle - 1) * 8000), cycle * 8000)
                .unwrap();
        }
        assert_eq!(h.drones.total().unwrap(), 100);
        assert!(h.apply(Harness::extract(1, 11, 80000), 88000).is_err());
        let mut h = Harness::new();
        h.apply(h.init(20), 0).unwrap();
        h.apply(Harness::extract(1, 1, 0), 8000).unwrap();
        assert!(h
            .apply(
                CarrierInventoryLedgerEvent::Destroyed {
                    pilot: "Alice".into(),
                    vessel_id: "vessel".into(),
                    drone_id: 1,
                    destroyed_at_ms: 8000,
                    lost_units: 9
                },
                8000
            )
            .is_err());
    }
    #[test]
    fn composed_source_keeps_mining_and_vessel_global_transfer_bounds() {
        let mut h = Harness::new();
        h.registration.assembly_revision = 1702;
        h.registration.assembly_source_sha256 =
            "57059d059a267da00ec2ae4d4a4dfd27393cd2280239bb241ffb751ea506b113".into();
        h.apply(h.init(20), 0).unwrap();
        for id in 1..=20 {
            h.apply(Harness::extract(id, 1, 0), 8000).unwrap();
        }
        assert_eq!(h.drones.total().unwrap(), 200);
        h.apply(Harness::transfer(1, 1, 8000), 8500).unwrap();
        assert!(h.apply(Harness::transfer(2, 2, 8000), 8999).is_err());
        h.apply(Harness::transfer(2, 2, 8500), 9000).unwrap();
        assert_eq!(h.accounts["Alice"].ore, 20);
        assert!(h.apply(Harness::extract(20, 2, 8000), 15999).is_err());
    }
    #[test]
    fn legacy_five_mining_rule_not_relaxed() {
        for count in [5, 10, 20] {
            let mut h = Harness::new();
            let registration = supported_drone_cargo_registration();
            let record = DroneCargoLedgerEvent::Initialized {
                pilot: "Alice".into(),
                vessel_id: "legacy".into(),
                registration: registration.clone(),
                drone_ids: (1..=count).collect(),
                capacity_units: 100,
            };
            let entry = LedgerEntry {
                sequence: 1,
                ruleset_version: 2,
                timestamp_ms: 0,
                previous_hash: GENESIS_HASH.into(),
                event: EconomicEvent::DroneCargo {
                    record: record.clone(),
                },
                hash: String::new(),
            };
            assert_eq!(
                h.drones
                    .apply(&record, &entry, &registration, &mut h.accounts, &mut h.belt)
                    .is_ok(),
                count == 5
            );
        }
    }
}

#[cfg(test)]
mod export_tests {
    use super::*;
    fn fixture() -> LedgerExport {
        fixture_for_definition(carrier_inventory_geometry::stationary_fixture_definition().unwrap())
    }
    fn fixture_for_definition(
        def: &carrier_inventory_geometry::CarrierInventoryDefinition,
    ) -> LedgerExport {
        let binding = def.topology.registration.bind("vessel".into());
        let roster: Vec<_> = def
            .cassettes
            .iter()
            .enumerate()
            .map(|(i, c)| CarrierRosterIdentity {
                drone_id: i as u64 + 1,
                class: DroneClass::Mining,
                preferred_magazine_slot: match c.initial_dock {
                    CarrierCassetteDock::Protected { slot_id } => Some(slot_id),
                    _ => None,
                },
            })
            .collect();
        let inventory = CarrierInventoryState {
            schema_version: 1,
            binding: binding.clone(),
            initialized_at_ms: 0,
            roster: roster.clone(),
            entries: roster
                .iter()
                .zip(&def.cassettes)
                .map(|(r, c)| CarrierDroneEntry {
                    drone_id: r.drone_id,
                    class: r.class,
                    hit_points: DRONE_HIT_POINTS,
                    location: match c.initial_dock {
                        CarrierCassetteDock::Ready { berth_id } => {
                            CarrierDroneLocation::Ready { berth_id }
                        }
                        CarrierCassetteDock::Protected { slot_id } => {
                            CarrierDroneLocation::Protected { slot_id }
                        }
                        _ => unreachable!(),
                    },
                })
                .collect(),
            cassettes: roster
                .iter()
                .zip(&def.cassettes)
                .map(|(r, c)| CarrierCassette {
                    cassette_id: c.cassette_id,
                    drone_id: Some(r.drone_id),
                    location: CarrierCassetteLocation::Docked {
                        dock: c.initial_dock,
                    },
                })
                .collect(),
            recall_high_watermark: 0,
            recalls: vec![],
            deployment_high_watermark: 0,
            deployments: vec![],
            high_watermark: 0,
            operations: vec![],
            reservations: vec![],
            cleanups: vec![],
        };
        let ore = CarrierOreState {
            binding: binding.clone(),
            holds: roster
                .iter()
                .map(|r| DroneOreHold {
                    drone_id: r.drone_id,
                    ore_units: 0,
                    destroyed_at_ms: None,
                    intent_generation: 0,
                    mining_intent: None,
                    cycle_high_watermark: 0,
                    cycle: None,
                })
                .collect(),
            lost_ore_units: 0,
            service_high_watermark: 0,
            services: vec![],
            waiting: vec![],
            granted_service_id: None,
            last_committed_at_ms: None,
        };
        let public = CarrierInventoryPublicState {
            sample_time_ms: 0,
            cameras: None,
            parent: def.cassettes[0].reference_pose,
            inventory,
            ore,
            mining_blocks: vec![],
            drone_poses: vec![],
            cassette_poses: vec![],
            machinery: vec![],
            motions: vec![],
        };
        let mut e = crate::tests::fixture();
        e.snapshot.pilots.truncate(1);
        let p = &mut e.snapshot.pilots[0];
        p.ore = 0;
        p.credits = STARTER_CREDITS;
        p.ship_class = ShipClass::Pioneer;
        p.carrier_inventory = Some(public);
        p.flight=Some(serde_json::from_value(serde_json::json!({"position":[0,0,0],"velocity":[0,0,0],"heading":[1,0,0],"mode":{"mode":"stop"},"updated_at_ms":0,"docking":{"version":1,"host_vessel_id":"vessel","berth_id":0,"operation_id":0,"phase":"secured","segment":null,"clamps":1.0,"updated_at_ms":0,"hold_reason":null,"departing":false}})).unwrap());
        e.snapshot.carrier_inventory_registration = Some(binding.registration.clone());
        e.snapshot.belt_ore = INITIAL_BELT_ORE;
        e.snapshot.server_time_ms = 0;
        e.entries.clear();
        for event in [
            EconomicEvent::PilotCreated {
                pilot: "Alice".into(),
            },
            EconomicEvent::CarrierInventory {
                record: CarrierInventoryLedgerEvent::Initialized {
                    pilot: "Alice".into(),
                    binding,
                    roster,
                    capacity_units_per_mining_drone: 100,
                },
            },
        ] {
            let mut entry = LedgerEntry {
                sequence: e.entries.len() as u64 + 1,
                ruleset_version: 3,
                timestamp_ms: 0,
                previous_hash: e
                    .entries
                    .last()
                    .map_or(GENESIS_HASH.to_string(), |e| e.hash.clone()),
                event,
                hash: String::new(),
            };
            entry.hash = entry.calculate_hash();
            e.entries.push(entry);
        }
        e.snapshot.ledger_sequence = 2;
        e.snapshot.ledger_hash = e.entries[1].hash.clone();
        e
    }
    #[test]
    fn exact_composed_source_replays_without_runtime_admission() {
        let mut registration = carrier_inventory_geometry::stationary_fixture_definition()
            .unwrap()
            .topology
            .registration
            .clone();
        registration.assembly_revision = 1702;
        registration.assembly_source_sha256 =
            "57059d059a267da00ec2ae4d4a4dfd27393cd2280239bb241ffb751ea506b113".into();
        let definition =
            carrier_inventory_geometry::stationary_fixture_for_registration(&registration).unwrap();
        let export = fixture_for_definition(definition);
        verify(&export, None).unwrap();
        assert!(carrier_inventory_geometry::lookup(&registration).is_none());
        for host in [false, true] {
            let mut foreign = export.clone();
            let registration = foreign
                .snapshot
                .carrier_inventory_registration
                .as_mut()
                .unwrap();
            if host {
                registration.host.source_sha256 = "a".repeat(64);
            } else {
                registration.assembly_source_sha256 = "a".repeat(64);
            }
            assert!(
                carrier_inventory_geometry::stationary_fixture_for_registration(registration)
                    .is_none()
            );
            assert!(verify(&foreign, None).is_err());
        }
        let mut rewritten = export.clone();
        if let EconomicEvent::CarrierInventory {
            record: CarrierInventoryLedgerEvent::Initialized { binding, .. },
        } = &mut rewritten.entries[1].event
        {
            binding.registration.assembly_source_sha256 = "a".repeat(64);
        }
        rewritten.entries[1].hash = rewritten.entries[1].calculate_hash();
        rewritten.snapshot.ledger_hash = rewritten.entries[1].hash.clone();
        assert!(verify(&rewritten, None).is_err());
    }
    #[test]
    fn genesis_export_and_empty_registered_world_verify() {
        let mut e = fixture();
        verify(&e, None).unwrap();
        e.entries.clear();
        e.snapshot.pilots.clear();
        e.snapshot.ledger_sequence = 0;
        e.snapshot.ledger_hash = GENESIS_HASH.into();
        verify(&e, None).unwrap();
    }
    #[test]
    fn all_twenty_mining_export_replays_and_bounds_snapshot_claims() {
        let mut e = fixture();
        for id in 1..=20 {
            let record = CarrierInventoryLedgerEvent::Extracted {
                pilot: "Alice".into(),
                vessel_id: "vessel".into(),
                drone_id: id,
                cycle_id: 1,
                target_id: asteroid_catalog(INITIAL_BELT_ORE)[0].id,
                cycle_started_at_ms: 0,
                units: 10,
                job_id: None,
            };
            let mut entry = LedgerEntry {
                sequence: e.entries.len() as u64 + 1,
                ruleset_version: 3,
                timestamp_ms: 8000,
                previous_hash: e.entries.last().unwrap().hash.clone(),
                event: EconomicEvent::CarrierInventory { record },
                hash: String::new(),
            };
            entry.hash = entry.calculate_hash();
            e.entries.push(entry);
        }
        let public = e.snapshot.pilots[0].carrier_inventory.as_mut().unwrap();
        for d in &mut public.inventory.entries {
            d.location = CarrierDroneLocation::Deployed;
        }
        for c in &mut public.inventory.cassettes {
            c.drone_id = None;
        }
        for h in &mut public.ore.holds {
            h.ore_units = 10;
            h.cycle_high_watermark = 1;
        }
        public.sample_time_ms = 8000;
        e.snapshot.server_time_ms = 8000;
        e.snapshot.belt_ore -= 200;
        e.snapshot.ledger_sequence = 22;
        e.snapshot.ledger_hash = e.entries.last().unwrap().hash.clone();
        verify(&e, None).unwrap();
        let public = e.snapshot.pilots[0].carrier_inventory.as_mut().unwrap();
        public.ore.holds[19].ore_units = 11;
        assert!(verify(&e, None).is_err());
    }
    #[test]
    fn malformed_snapshot_and_mixed_versions_rejected() {
        for case in 0..8 {
            let mut e = fixture();
            match case {
                0 => e.snapshot.carrier_inventory_registration = None,
                1 => {
                    e.snapshot.drone_cargo_registration = Some(supported_drone_cargo_registration())
                }
                2 => {
                    e.snapshot.pilots[0]
                        .carrier_inventory
                        .as_mut()
                        .unwrap()
                        .inventory
                        .roster[0]
                        .class = DroneClass::Attack
                }
                3 => {
                    e.snapshot.pilots[0]
                        .carrier_inventory
                        .as_mut()
                        .unwrap()
                        .ore
                        .holds[0]
                        .ore_units = 10
                }
                4 => {
                    e.snapshot.pilots[0]
                        .carrier_inventory
                        .as_mut()
                        .unwrap()
                        .inventory
                        .binding
                        .vessel_id = "fabricated".into()
                }
                5 => {
                    e.snapshot.pilots[0]
                        .carrier_inventory
                        .as_mut()
                        .unwrap()
                        .ore
                        .holds
                        .pop();
                }
                6 => e.entries[0].ruleset_version = 2,
                _ => {
                    e.snapshot.pilots[0]
                        .carrier_inventory
                        .as_mut()
                        .unwrap()
                        .ore
                        .lost_ore_units = 10
                }
            };
            assert!(verify(&e, None).is_err(), "case {case}");
        }
    }
}
