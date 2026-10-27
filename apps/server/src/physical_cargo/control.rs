//! Fresh, explicitly gated bridge to existing Server request and tick entry
//! points. No ordinary Server::open or network launcher admits this mode.
use super::*;
use anyhow::bail;
use arkline_protocol::{
    physical_cargo::{
        bay::PhysicalBayBinding,
        control::{
            PhysicalControlAction as Action, PhysicalControlBinding as Binding,
            PhysicalControlProjection as Projection, PhysicalControlPublicView,
            PhysicalControlState as State, PhysicalControlStep,
        },
    },
    DroneClass, Request, Response, ShipClass, PROTOCOL_VERSION,
};
mod projection;
mod reducer;
pub(super) use projection::{project_world, validate_world};

/// Existing commands, actor views and Server authority, restricted to the
/// source-certified one-bay fixed-world envelope. Does not expose a listener.
pub struct PhysicalControlledWorld {
    server: Server,
}
impl PhysicalControlledWorld {
    pub fn open(
        path: impl AsRef<Path>,
        genesis: PhysicalCargoGenesis,
        bay_binding: PhysicalBayBinding,
        binding: Binding,
        owner_token: &str,
        now: u64,
    ) -> Result<Self> {
        ensure!(
            owner_token.len() >= 16 && owner_token.len() <= 128,
            "Explicit local owner token required"
        );
        // Validate the complete new control policy without creating a database.
        // The real world is still admitted atomically by its existing transaction.
        let mut probe_entries = Vec::new();
        let mut previous = arkline_protocol::GENESIS_HASH.to_owned();
        for (index, record) in [
            genesis.event(),
            PhysicalCargoEvent::BayBound {
                binding: Box::new(bay_binding.clone()),
            },
            PhysicalCargoEvent::WorldBound {
                binding: Box::new(binding.clone()),
            },
        ]
        .into_iter()
        .enumerate()
        {
            let mut entry = LedgerEntry {
                sequence: index as u64 + 1,
                ruleset_version: PHYSICAL_VOLUME_RULESET_VERSION,
                timestamp_ms: binding.initial_power.updated_at_ms,
                previous_hash: previous,
                event: EconomicEvent::PhysicalCargo { record },
                hash: String::new(),
            };
            entry.hash = entry.calculate_hash();
            previous = entry.hash.clone();
            probe_entries.push(entry);
        }
        replay(&probe_entries)?;
        let path = path.as_ref();
        if !path.exists() || path == Path::new(":memory:") {
            ensure!(
                binding.initial_power.updated_at_ms == now,
                "Fresh control clock differs from power binding"
            );
        }
        let mut server = Server::open_realm_with_configuration(
            path,
            false,
            false,
            None,
            SceneLayoutRevision::LegacyV1,
            false,
            Some((genesis, now, true)),
        )?;
        let world = read_world(&server.db)?;
        let physical = world.physical_cargo.as_ref().context("No physical state")?;
        if let Some(existing) = &physical.control {
            ensure!(
                existing.binding == binding
                    && physical
                        .bay
                        .as_ref()
                        .is_some_and(|b| b.binding == bay_binding),
                "Controlled source/owner/policy cannot be rebound"
            );
            ensure!(
                world.pilots[&binding.pilot].token_hash == crate::token_hash(owner_token),
                "Owner credential mismatch"
            );
            advance(&mut server, Action::Restart, now)?;
        } else {
            let entries = load_entries(&server.db.transaction()?, true)?;
            ensure!(
                entries.len() == 1 && physical.bay.is_none(),
                "Only untouched fresh genesis can admit controlled actors"
            );
            ensure!(
                physical.genesis.mining_drone_ids == [binding.drone_id]
                    && bay_binding.drone_id == binding.drone_id,
                "Bound roster mismatch"
            );
            let mut bay_state = bay::initial(bay_binding.clone(), now);
            bay_state.world_power = true;
            let state = State {
                binding: binding.clone(),
                projection: Projection {
                    power: binding.initial_power.clone(),
                    hit_points: 100,
                    signal_available: true,
                    target_lock: None,
                    target_locked: false,
                    mining: false,
                    recalling: false,
                    resource_remaining_cm3: physical.resource_remaining_cm3,
                    accepted_at_ms: now,
                    mining_cycle: None,
                },
            };
            apply_authorized_control(
                &mut server.db,
                &[
                    PhysicalCargoEvent::BayBound {
                        binding: Box::new(bay_binding),
                    },
                    PhysicalCargoEvent::WorldBound {
                        binding: Box::new(binding),
                    },
                ],
                now,
                Some(bay_state),
                Some(state),
                Some(crate::token_hash(owner_token)),
            )?;
        }
        server.physical_controlled = true;
        Ok(Self { server })
    }
    pub fn handle_at(
        &mut self,
        session: &mut Option<String>,
        request: Request,
        now: u64,
    ) -> Response {
        self.server.handle_at(session, request, now)
    }
    pub fn tick_at(&mut self, now: u64) -> Result<()> {
        self.server.tick_at(now)
    }
    pub fn export(&mut self) -> Result<PhysicalCargoExport> {
        let tx = self.server.db.transaction()?;
        let world = read_world(&tx)?;
        let out = export_state(&world, load_entries(&tx, true)?)?;
        out.verify()?;
        tx.commit()?;
        Ok(out)
    }
    // Combat/signal fault inputs belong to authority, never the network allowlist.
    #[cfg(test)]
    fn actor_input(&mut self, action: Action, now: u64) -> Result<()> {
        advance(&mut self.server, action, now)
    }
}
fn advance(server: &mut Server, action: Action, now: u64) -> Result<()> {
    let tx = server.db.transaction()?;
    let world = read_world(&tx)?;
    let entries = load_entries(&tx, true)?;
    export_state(&world, entries.clone())?.verify()?;
    let prior = replay(&entries)?;
    let (control, bay, events) = reducer::reduce(&world, &prior, action, now)?;
    tx.commit()?;
    apply_authorized_control(&mut server.db, &events, now, Some(bay), Some(control), None)
}
pub(crate) fn tick(server: &mut Server, now: u64) -> Result<()> {
    let w = read_world(&server.db)?;
    let control = w
        .physical_cargo
        .as_ref()
        .and_then(|s| s.control.as_ref())
        .context("No controlled world")?;
    ensure!(
        now >= control.projection.accepted_at_ms,
        "World clock moved backwards"
    );
    if now == control.projection.accepted_at_ms {
        return Ok(());
    }
    advance(server, Action::Tick, now)
}
/// Narrow branch at the existing Server::execute entry. Unsupported actions
/// never reach legacy economy, movement, actor creation or simulation.
pub(crate) fn execute(
    server: &mut Server,
    session: &mut Option<String>,
    request: Request,
    now: u64,
) -> Result<Response> {
    let world = read_world(&server.db)?;
    validate_world(&world)?;
    let s = world.physical_cargo.as_ref().unwrap();
    let control = s.control.as_ref().context("No control binding")?;
    let b = &control.binding;
    let export = matches!(&request, Request::ExportLedger);
    let mut connected = None;
    let action = match request {
        Request::Connect {
            protocol_version,
            pilot,
            resume_token,
            ..
        } => {
            ensure!(
                protocol_version == PROTOCOL_VERSION && pilot == b.pilot,
                "Unsupported protocol or owner"
            );
            let token = resume_token.context("Explicit owner resume token required")?;
            ensure!(
                world.pilots[&pilot].token_hash == crate::token_hash(&token),
                "Invalid owner token"
            );
            connected = Some(pilot);
            None
        }
        request => {
            ensure!(
                session.as_ref() == Some(&b.pilot),
                "Connect as the bound owner before controls"
            );
            match request {
                Request::Snapshot | Request::ExportLedger => None,
                Request::LaunchDroneClass {
                    class: DroneClass::Mining,
                } => Some(Action::Launch),
                Request::AssignMiningDrone { id, target_id } => {
                    ensure!(
                        id == b.drone_id && target_id == b.target_id,
                        "Unregistered drone/target"
                    );
                    Some(Action::Assign)
                }
                Request::LockAsteroid { id } => {
                    ensure!(id == b.target_id, "Unregistered target");
                    Some(Action::Lock)
                }
                Request::UnlockAsteroid { id } => {
                    ensure!(id == b.target_id, "Unregistered target");
                    Some(Action::Unlock)
                }
                Request::UnlockTarget => Some(Action::Unlock),
                Request::RecallDroneById { id } => {
                    ensure!(id == b.drone_id, "Unowned drone");
                    Some(Action::Recall)
                }
                Request::RecallDrone => Some(Action::Recall),
                Request::DroneHoldById { id } => {
                    ensure!(id == b.drone_id, "Unowned drone");
                    Some(Action::Hold)
                }
                Request::DroneHold => Some(Action::Hold),
                _ => bail!("Control not admitted by the selected-bay source envelope"),
            }
        }
    };
    ensure!(
        now >= control.projection.accepted_at_ms,
        "Control clock moved backwards"
    );
    // Observing/connect never advances the physical clock or performs a cycle.
    if let Some(action) = action {
        advance(server, action, now)?;
    }
    let tx = server.db.transaction()?;
    let world = read_world(&tx)?;
    let entries = load_entries(&tx, true)?;
    export_state(&world, entries.clone())?.verify()?;
    let response = projection::response(&world, entries, now, export)?;
    tx.commit()?;
    if let Some(name) = connected {
        *session = Some(name);
    }
    Ok(response)
}

#[cfg(test)]
mod tests;
