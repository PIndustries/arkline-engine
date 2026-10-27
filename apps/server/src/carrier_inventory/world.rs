//! Durable, transaction-owned carrier inventory. Production lookup remains
//! fail-closed until the composed source and travel consumers are admitted.
use super::motion::*;
use super::*;
use crate::{drone_bay, drones, World};
use anyhow::{bail, Context};
use arkline_protocol::carrier_inventory_geometry::{self as geometry, CarrierInventoryDefinition};
use arkline_protocol::drone_bay_geometry::{BayMotionSegment, BayPose};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
mod arrival;
pub(crate) mod cargo;
mod departure;
mod deployment;
mod history;
mod persistence;
mod presentation;
mod recall;
pub(crate) mod service;
pub(crate) use presentation::public_view;
mod simulation;
pub(crate) use persistence::{preflight_existing, validate, validate_history};
pub(crate) use simulation::{advance, dispatch, finish};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct OwnedCarrier {
    pub inventory: CarrierInventoryState,
    pub ore: CarrierOreState,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub mining_blocks: BTreeMap<u64, String>,
    /// Actual confirmed body-space roots, including idle portable cassettes.
    pub cassette_poses: BTreeMap<u64, BayPose>,
    pub drone_poses: BTreeMap<u64, BayPose>,
    pub machinery: BTreeMap<String, BayPose>,
    pub runs: BTreeMap<u64, CarrierMotionRuntime>,
    pub departures: BTreeMap<u64, departure::Departure>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub arrivals: BTreeMap<u64, arrival::Arrival>,
}
pub(crate) fn definition(w: &World) -> Result<Option<&'static CarrierInventoryDefinition>> {
    let Some(reg) = w.carrier_inventory_registration.as_ref() else {
        return Ok(None);
    };
    if let Some(d) = geometry::lookup(reg) {
        return Ok(Some(d));
    }
    #[cfg(test)]
    if let Some(d) = geometry::stationary_fixture_for_registration(reg) {
        return Ok(Some(d));
    }
    bail!("Carrier inventory source is not admitted; no automatic save migration")
}
pub(crate) fn state<'a>(w: &'a World, name: &str) -> Option<&'a OwnedCarrier> {
    w.carrier_inventory_registration.as_ref()?;
    w.accounts.get(name)?.carrier_inventory.as_ref()
}
fn parent(w: &World, name: &str) -> Result<BayPose> {
    drone_bay::runtime::parent(w, name)
}
pub(crate) fn constrained(w: &World, name: &str, id: u64) -> bool {
    state(w, name).is_some_and(|s| {
        s.inventory.active_operation(id).is_some() || s.departures.contains_key(&id)
    })
}
/// Necessary inventory security, never sufficient source/travel admission.
/// Immutable losses do not invent a replacement drone or prevent safe departure
/// after all retained hardware has actually finished its cleanup.
pub(crate) fn inventory_secured(w: &World, name: &str) -> Result<()> {
    validate(w)?;
    let def = definition(w)?.context("Carrier source unavailable")?;
    let s = state(w, name).context("Carrier ownership unavailable")?;
    ensure!(
        s.inventory.entries.iter().all(|e| matches!(
            e.location,
            CarrierDroneLocation::Ready { .. }
                | CarrierDroneLocation::Protected { .. }
                | CarrierDroneLocation::Destroyed { .. }
        )),
        "Every surviving drone must be secured Ready or Protected"
    );
    ensure!(
        s.runs.is_empty()
            && s.arrivals.is_empty()
            && s.departures.is_empty()
            && s.inventory.reservations.is_empty()
            && s.inventory.cleanups.is_empty()
            && s.inventory.operations.iter().all(|o| o.phase.terminal())
            && s.inventory.active_deployment().is_none()
            && s.inventory.active_recall().is_none(),
        "Carrier machinery, cleanup and queued work must finish before departure"
    );
    ensure!(
        s.inventory.cassettes.iter().all(|c| matches!(
            c.location,
            CarrierCassetteLocation::Docked {
                dock: CarrierCassetteDock::Ready { .. } | CarrierCassetteDock::Protected { .. }
            }
        )),
        "Every cassette must be secured at a capacity dock, not a transit buffer"
    );
    ensure!(
        s.ore.granted_service_id.is_none()
            && s.ore.waiting.is_empty()
            && s.ore.services.iter().all(|a| a.closed_at_ms.is_some())
            && s.ore
                .holds
                .iter()
                .all(|h| h.ore_units == 0 && h.cycle.is_none() && h.mining_intent.is_none()),
        "Unload payload and clear every manifold/extraction claim before departure"
    );
    let idle: BTreeMap<_, _> = def
        .idle_roots(
            &s.cassette_poses
                .iter()
                .map(|(id, pose)| (*id, *pose))
                .collect::<Vec<_>>(),
        )
        .context("Exact secured source roots unavailable")?
        .into_iter()
        .collect();
    ensure!(
        idle == s.machinery,
        "Carrier machinery is not at exact secured source roots"
    );
    Ok(())
}
/// Shared secure-only gate used by departure and certified station refinement.
/// This does not authorize normal source lookup, movement, docking, or a debit.
pub(crate) fn secured_for_departure(w: &World, name: &str, now: u64) -> Result<()> {
    inventory_secured(w, name)?;
    crate::turret_stow::confirmed_secure(w, name, now)
}
/// An automatic finite deployment may keep working while its owner walks the
/// unchanged enclosed crew core. This grants no boarding/outer machinery access.
pub(crate) fn enclosed_crew_during_deployment(w: &World, name: &str) -> bool {
    let Ok(Some(def)) = definition(w) else {
        return false;
    };
    state(w, name).is_some_and(|s| {
        s.inventory.binding.registration == def.topology.registration
            && (s.inventory.active_deployment().is_some() || s.inventory.active_recall().is_some())
            && w.accounts
                .get(name)
                .and_then(|a| a.vessel_id(ShipClass::Pioneer))
                == Some(s.inventory.binding.vessel_id.as_str())
            && parent(w, name).is_ok()
    })
}
pub(crate) fn initialize(w: &mut World, name: &str, now: u64) -> Result<Option<EconomicEvent>> {
    let Some(def) = definition(w)? else {
        return Ok(None);
    };
    ensure!(
        w.accounts[name].carrier_inventory.is_none(),
        "Carrier initialization cannot refill inventory"
    );
    let loadout = w
        .carrier_inventory_loadout
        .as_ref()
        .context("Fresh carrier needs explicit finite loadout")?;
    ensure!(
        loadout.len() == def.topology.capacity() && def.cassettes.len() == loadout.len(),
        "Source capacity/loadout mismatch"
    );
    let vessel_id = w.accounts[name]
        .vessel_id(ShipClass::Pioneer)
        .context("Carrier vessel missing")?
        .to_owned();
    let binding = CarrierInventoryBinding {
        vessel_id,
        registration: def.topology.registration.clone(),
    };
    let mut entries = Vec::new();
    let mut roster = Vec::new();
    let mut cassettes = Vec::new();
    let mut cassette_poses = BTreeMap::new();
    let mut drone_poses = BTreeMap::new();
    for (c, class) in def.cassettes.iter().zip(loadout) {
        ensure!(
            def.topology.accepts(c.initial_dock, *class),
            "Loadout class incompatible with source dock"
        );
        let id = w.next_drone_id;
        w.next_drone_id = id.checked_add(1).context("Finite drone ID overflow")?;
        let (location, preferred_magazine_slot) = match c.initial_dock {
            CarrierCassetteDock::Ready { berth_id } => {
                (CarrierDroneLocation::Ready { berth_id }, None)
            }
            CarrierCassetteDock::Protected { slot_id } => {
                (CarrierDroneLocation::Protected { slot_id }, Some(slot_id))
            }
            CarrierCassetteDock::TransitBuffer { .. } => {
                bail!("Transit buffer cannot initialize stock")
            }
        };
        entries.push(CarrierDroneEntry {
            drone_id: id,
            class: *class,
            hit_points: DRONE_HIT_POINTS,
            location,
        });
        roster.push(CarrierRosterIdentity {
            drone_id: id,
            class: *class,
            preferred_magazine_slot,
        });
        cassettes.push(CarrierCassette {
            cassette_id: c.cassette_id,
            drone_id: Some(id),
            location: CarrierCassetteLocation::Docked {
                dock: c.initial_dock,
            },
        });
        cassette_poses.insert(c.cassette_id, c.reference_pose);
        // Portable cassette body origin is the source-defined clamped drone origin.
        drone_poses.insert(id, c.reference_pose);
    }
    let machinery = def
        .idle_roots(
            &cassette_poses
                .iter()
                .map(|(i, p)| (*i, *p))
                .collect::<Vec<_>>(),
        )
        .context("Source idle roots unavailable")?
        .into_iter()
        .collect();
    let inventory = CarrierInventoryState {
        schema_version: CARRIER_INVENTORY_SCHEMA_VERSION,
        binding: binding.clone(),
        initialized_at_ms: now,
        roster: roster.clone(),
        entries,
        cassettes,
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
            .filter(|e| e.class == DroneClass::Mining)
            .map(|e| DroneOreHold {
                drone_id: e.drone_id,
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
    ensure!(
        inventory.validate_shape(&def.topology) && ore.validate_shape(&inventory),
        "Invalid source-derived fresh carrier inventory"
    );
    w.accounts.get_mut(name).unwrap().carrier_inventory = Some(OwnedCarrier {
        inventory,
        ore,
        mining_blocks: BTreeMap::new(),
        cassette_poses,
        drone_poses,
        machinery,
        runs: BTreeMap::new(),
        departures: BTreeMap::new(),
        arrivals: BTreeMap::new(),
    });
    Ok(Some(EconomicEvent::CarrierInventory {
        record: CarrierInventoryLedgerEvent::Initialized {
            pilot: name.into(),
            binding,
            roster,
            capacity_units_per_mining_drone: MINING_DRONE_CARGO_CAPACITY,
        },
    }))
}
fn actual(
    s: &OwnedCarrier,
    expected: &CarrierMotionFrame,
    drone_id: u64,
) -> Result<CarrierMotionFrame> {
    Ok(CarrierMotionFrame {
        poses: expected
            .poses
            .iter()
            .map(|(k, _)| {
                let pose = match k {
                    CarrierPoseKey::Drone => s.drone_poses.get(&drone_id),
                    CarrierPoseKey::Cassette { cassette_id } => s.cassette_poses.get(cassette_id),
                    CarrierPoseKey::Mechanism { semantic_id } => s.machinery.get(semantic_id),
                }
                .context("Actual source-owned pose missing")?;
                Ok((k.clone(), *pose))
            })
            .collect::<Result<_>>()?,
    })
}
fn apply_frame(s: &mut OwnedCarrier, drone_id: u64, frame: &CarrierMotionFrame) {
    for (k, p) in &frame.poses {
        match k {
            CarrierPoseKey::Drone => {
                s.drone_poses.insert(drone_id, *p);
            }
            CarrierPoseKey::Cassette { cassette_id } => {
                s.cassette_poses.insert(*cassette_id, *p);
            }
            CarrierPoseKey::Mechanism { semantic_id } => {
                s.machinery.insert(semantic_id.clone(), *p);
            }
        }
    }
}
fn run_kind(s: &CarrierInventoryState, id: u64) -> Result<CarrierRouteKind> {
    let op = s.operation(id).context("Missing operation")?;
    if let Some(x) = &op.stage {
        return Ok(CarrierRouteKind::Stage {
            direction: x.direction,
            segment: if x.loaded_handoff_at_ms.is_some() {
                CarrierStageSegment::EmptyTail
            } else if x.direction.is_forward()
                && s.binding.registration.assembly_revision == 1702
                && op.phase != CarrierOperationPhase::Destroyed
            {
                CarrierStageSegment::Head
            } else {
                CarrierStageSegment::Full
            },
            source_slot: x.source_slot_id,
            destination_berth: x.destination_berth_id,
            loaded_cassette: x.loaded_cassette_id,
            empty_cassette: x.empty_cassette_id,
        });
    }
    let berth_id = op.berth_id.context("Bay operation lacks berth")?;
    let cassette = s
        .docked_cassette(CarrierCassetteDock::Ready { berth_id })
        .context("Ready cassette missing")?;
    Ok(CarrierRouteKind::Bay {
        berth_id,
        cassette_id: cassette.cassette_id,
        action: op.action,
    })
}
fn route(
    def: &CarrierInventoryDefinition,
    kind: &CarrierRouteKind,
    class: DroneClass,
) -> Result<Vec<geometry::CarrierInventoryKeyframe>> {
    kind.route(def, class)
        .context("Exact source route unavailable")
}
fn phase(s: &mut OwnedCarrier, id: u64, value: CarrierOperationPhase, now: u64, message: &str) {
    let op = s
        .inventory
        .operations
        .iter_mut()
        .find(|o| o.operation_id == id)
        .unwrap();
    op.phase = value;
    op.held_phase = None;
    op.updated_at_ms = now;
    op.message = message.into();
}
pub(crate) fn execute(
    w: &mut World,
    name: &str,
    request: &CarrierInventoryRequest,
    now: u64,
) -> Result<()> {
    let def = definition(w)?.context("Carrier inventory unavailable in this world")?;
    if deployment::execute(w, name, request, now)? || recall::execute(w, name, request, now)? {
        return validate(w);
    }
    let mut s = state(w, name).context("Owned carrier missing")?.clone();
    let (binding, id) = match request {
        CarrierInventoryRequest::Begin {
            binding,
            operation_id,
            ..
        }
        | CarrierInventoryRequest::Hold {
            binding,
            operation_id,
        }
        | CarrierInventoryRequest::Resume {
            binding,
            operation_id,
        }
        | CarrierInventoryRequest::Cancel {
            binding,
            operation_id,
        } => (binding, *operation_id),
        _ => unreachable!(),
    };
    ensure!(
        *binding == s.inventory.binding,
        "Actual vessel/source binding mismatch"
    );
    let physical_owned = s.runs.contains_key(&id)
        || s.arrivals.contains_key(&id)
        || s.departures.values().any(|d| d.operation_id == id);
    ensure!(
        !s.inventory
            .deployment_owner(id)
            .is_some_and(|b| b.phase.active())
            && s.inventory.recall_owner(id).is_none()
            || (!physical_owned
                && s.inventory
                    .operation(id)
                    .is_some_and(|o| o.phase.terminal())),
        "Automatic inventory queue owns this physical child; use its controls"
    );
    match request {
        CarrierInventoryRequest::RecallAll { .. }
        | CarrierInventoryRequest::HoldRecall { .. }
        | CarrierInventoryRequest::ResumeRecall { .. }
        | CarrierInventoryRequest::CancelRecallRemaining { .. }
        | CarrierInventoryRequest::DeployAll { .. }
        | CarrierInventoryRequest::HoldDeployment { .. }
        | CarrierInventoryRequest::ResumeDeployment { .. }
        | CarrierInventoryRequest::CancelDeploymentRemaining { .. } => unreachable!(),
        CarrierInventoryRequest::Begin {
            drone_id, action, ..
        } => {
            if let Some(op) = s.inventory.operation(id) {
                ensure!(
                    op.drone_id == *drone_id && op.action == *action,
                    "Operation payload conflict"
                );
                return Ok(());
            }
            ensure!(
                s.inventory.active_deployment().is_none() && s.inventory.active_recall().is_none(),
                "Automatic queue owns pending inventory; use its retained controls"
            );
            ensure!(
                !w.resource_jobs.contains_key(name),
                "Stop the resource routine before manual carrier actions"
            );
            if matches!(
                action,
                CarrierInventoryAction::Stage { .. } | CarrierInventoryAction::StoreReady { .. }
            ) {
                if matches!(action, CarrierInventoryAction::StoreReady { .. }) {
                    ensure!(
                        matches!(def.topology.registration.assembly_revision, 1702 | 1703),
                        "Protected exchange source is not admitted"
                    );
                    ensure!(
                        s.ore
                            .holds
                            .iter()
                            .find(|h| h.drone_id == *drone_id)
                            .is_none_or(|h| h.ore_units == 0 && h.cycle.is_none()),
                        "Unload ore before protected storage"
                    );
                }
                begin_stage(&mut s.inventory, &def.topology, request, now)?;
            } else {
                ensure!(
                    matches!(
                        action,
                        CarrierInventoryAction::Launch
                            | CarrierInventoryAction::RecallToStorage
                            | CarrierInventoryAction::ReturnToUnload { .. }
                    ),
                    "Ore service source is not admitted"
                );
                ensure!(
                    !s.departures.contains_key(drone_id),
                    "Wait for the existing physical departure to clear before Recall"
                );
                if *action == CarrierInventoryAction::RecallToStorage {
                    ensure!(
                        !s.inventory
                            .operations
                            .iter()
                            .any(|o| o.drone_id == *drone_id && !o.phase.terminal()),
                        "Finish this identity's retained empty hardware cleanup before Recall"
                    );
                }
                ensure!(
                    s.inventory.next_operation_id() == Some(id)
                        && s.inventory.active_operation(*drone_id).is_none(),
                    "Expected next operation for idle identity"
                );
                let e = s
                    .inventory
                    .entry(*drone_id)
                    .context("Unknown finite drone")?;
                ensure!(
                    match action {
                        CarrierInventoryAction::Launch =>
                            matches!(e.location, CarrierDroneLocation::Ready { .. }),
                        CarrierInventoryAction::RecallToStorage =>
                            matches!(e.location, CarrierDroneLocation::Deployed)
                                && matches!(
                                    def.topology.registration.assembly_revision,
                                    1702 | 1703
                                ),
                        CarrierInventoryAction::ReturnToUnload { .. } =>
                            e.location == CarrierDroneLocation::Deployed
                                && e.class == DroneClass::Mining
                                && cargo::source_supports_service(def),
                        _ => false,
                    },
                    "Action requires actual Ready launch or source-admitted Deployed capture"
                );
                let service = matches!(action, CarrierInventoryAction::ReturnToUnload { .. });
                ensure!(
                    s.ore
                        .holds
                        .iter()
                        .find(|h| h.drone_id == *drone_id)
                        .is_none_or(|h| if service {
                            h.ore_units > 0
                        } else {
                            h.ore_units == 0 && h.cycle.is_none()
                        }),
                    "Loaded return requires physical ore service; service requires actual payload"
                );
                if service {
                    let h = s
                        .ore
                        .holds
                        .iter_mut()
                        .find(|h| h.drone_id == *drone_id)
                        .context("Mining hold absent")?;
                    h.cycle = None;
                    if matches!(
                        action,
                        CarrierInventoryAction::ReturnToUnload {
                            resume_mining: false
                        }
                    ) {
                        cargo::invalidate(h)?;
                    }
                    let d = w
                        .pilots
                        .get_mut(name)
                        .unwrap()
                        .view
                        .drones
                        .iter_mut()
                        .find(|d| d.id == *drone_id)
                        .context("Service body absent")?;
                    d.mode = DroneMode::Hold;
                    d.mining_cycle = None;
                    d.velocity = [0.; 3];
                }
                s.mining_blocks.remove(drone_id);
                s.inventory.operations.push(CarrierInventoryOperation {
                    recall_batch_id: None,
                    operation_id: id,
                    drone_id: *drone_id,
                    action: *action,
                    phase: CarrierOperationPhase::Queued,
                    held_phase: None,
                    stage: None,
                    berth_id: None,
                    accepted_at_ms: now,
                    updated_at_ms: now,
                    message: "Queued for source-bound physical route".into(),
                    cargo_service_operation_ids: vec![],
                });
                s.inventory.high_watermark = id;
            }
        }
        CarrierInventoryRequest::Hold { .. } => {
            service::close_attempt(&mut s, id, now)?;
            for d in s.departures.values_mut().filter(|d| d.operation_id == id) {
                d.held = true;
                d.hold_reason = "Pilot held exterior departure".into();
            }
            if let Some(a) = s.arrivals.get_mut(&id) {
                a.hold("Pilot held exterior approach");
            }
            hold_stage(&mut s.inventory, id, now)?;
            if let Some(r) = s.runs.get_mut(&id) {
                r.hold("Pilot held carrier hardware");
            }
        }
        CarrierInventoryRequest::Resume { .. } => {
            let op = s.inventory.operation(id).context("Unknown operation")?;
            let has_departure = s.departures.values().any(|d| d.operation_id == id);
            if op.phase.terminal()
                && !s.inventory.cleanups.iter().any(|c| c.operation_id == id)
                && !has_departure
            {
                return Ok(());
            }
            for d in s.departures.values_mut().filter(|d| d.operation_id == id) {
                d.resume(now)?;
            }
            if let Some(a) = s.arrivals.get_mut(&id) {
                a.resume(now)?;
            }
            if let Some(r) = s.runs.get_mut(&id) {
                if r.held {
                    r.resume_validated(now)?;
                }
            }
            resume_stage(&mut s.inventory, id, now)?;
        }
        CarrierInventoryRequest::Cancel { .. } => {
            let op = s.inventory.operation(id).context("Unknown operation")?;
            if op.phase.terminal() {
                return Ok(());
            }
            ensure!(
                op.effective_phase() == CarrierOperationPhase::Queued,
                "Accepted physical route cannot be discarded; Hold or Resume to finish securing"
            );
            phase(
                &mut s,
                id,
                CarrierOperationPhase::Cancelled,
                now,
                "Cancelled before physical reservation",
            );
        }
    }
    service::reconcile(&mut s, now)?;
    for body in &mut w.pilots.get_mut(name).unwrap().view.drones {
        if s.departures.get(&body.id).is_some_and(|d| d.held) {
            body.velocity = [0.; 3];
        }
    }
    w.accounts.get_mut(name).unwrap().carrier_inventory = Some(s);
    simulation::admit(w, name, now)?;
    validate(w)
}
pub(crate) fn stop_all(w: &mut World, name: &str, now: u64, reason: &str) -> Result<()> {
    let Some(mut s) = state(w, name).cloned() else {
        return Ok(());
    };
    for d in s.departures.values_mut() {
        d.held = true;
        d.hold_reason = reason.into();
    }
    for a in s.arrivals.values_mut() {
        a.hold(reason);
    }
    for batch in &mut s.inventory.deployments {
        if batch.phase.active() {
            batch.held = true;
            batch.updated_at_ms = now;
        }
    }
    for batch in &mut s.inventory.recalls {
        if batch.phase.active() {
            batch.held = true;
            batch.updated_at_ms = now;
        }
    }
    let ids: BTreeSet<_> = s
        .inventory
        .operations
        .iter()
        .filter(|o| !o.phase.terminal())
        .map(|o| o.operation_id)
        .chain(s.runs.keys().copied())
        .collect();
    for id in ids {
        service::close_attempt(&mut s, id, now)?;
        hold_stage(&mut s.inventory, id, now)?;
        if let Some(r) = s.runs.get_mut(&id) {
            r.hold(reason);
        }
    }
    cargo::stop(&mut s)?;
    for d in &mut w.pilots.get_mut(name).unwrap().view.drones {
        if d.class == DroneClass::Mining {
            d.mode = DroneMode::Hold;
            d.mining_cycle = None;
            d.velocity = [0.; 3];
        }
        if s.departures.get(&d.id).is_some_and(|r| r.held) {
            d.velocity = [0.; 3];
        }
    }
    w.accounts.get_mut(name).unwrap().carrier_inventory = Some(s);
    Ok(())
}
pub(crate) fn authorize(w: &World, name: &str, request: &Request) -> Result<()> {
    if state(w, name).is_none() {
        return Ok(());
    }
    match request {
        Request::Snapshot
        | Request::ExportLedger
        | Request::CarrierInventory { .. }
        | Request::Connect { .. } => Ok(()),
        Request::Navigate {
            mode: NavigationMode::Stop,
        } => Ok(()),
        Request::Warp { .. }
        | Request::Jump { .. }
        | Request::Dock
        | Request::Undock
        | Request::Navigate { .. }
        | Request::PilotInput { .. }
        | Request::SelectShip { .. }
        | Request::BoardShip { .. }
        | Request::StartResourceJob { .. } => {
            bail!("Stationary carrier source fixture: travel and routines not yet admitted")
        }
        Request::IndependentBays { .. }
        | Request::DroneBay { .. }
        | Request::DroneCargo { .. }
        | Request::LaunchDrone
        | Request::LaunchDroneClass { .. }
        | Request::RecallDrone
        | Request::RecallDroneById { .. } => {
            bail!("Use identified carrier inventory actions; legacy drone spawning is unavailable")
        }
        Request::AssignMiningDrone { id, .. } | Request::DroneHoldById { id } => {
            let s = state(w, name).unwrap();
            let def = definition(w)?.context("Carrier source unavailable")?;
            ensure!(
                cargo::source_supports_service(def)
                    && s.inventory
                        .entry(*id)
                        .is_some_and(|e| e.location == CarrierDroneLocation::Deployed
                            && e.class == DroneClass::Mining
                            && e.hit_points > 0)
                    && !constrained(w, name, *id)
                    && s.inventory.active_deployment().is_none()
                    && s.inventory.active_recall().is_none(),
                "Mining command needs a free live Mining identity and finished queue/departure"
            );
            Ok(())
        }
        Request::DroneNavigateById { .. }
        | Request::AssignAttackDrone { .. }
        | Request::OrbitDrone { .. }
        | Request::DroneNavigate { .. }
        | Request::DroneHold => {
            bail!("Free-drone commands await the carrier cargo/return integration gate")
        }
        Request::StartMining { .. } => {
            ensure!(
                crate::turret_stow::enabled(w, name),
                "Carrier turret source unavailable"
            );
            Ok(())
        }
        Request::Mine { .. } | Request::MineAsteroid { .. } => {
            bail!("Carrier fixture mining awaits ruleset3 extraction gate")
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
