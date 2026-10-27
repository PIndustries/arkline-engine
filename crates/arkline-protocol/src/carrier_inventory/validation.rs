use super::*;

impl CarrierInventoryState {
    /// Structural ownership validation only. The world validator must additionally
    /// bind genesis, exact registered source, motion clocks, bodies and cargo.
    pub fn validate_shape(&self, t: &CarrierInventoryTopology) -> bool {
        if !self.validate_deployments()
            || !self.validate_recalls()
            || self.schema_version != CARRIER_INVENTORY_SCHEMA_VERSION
            || !self.binding.well_formed()
            || self.binding.registration != t.registration
            || !t.valid_shape()
            || self.entries.is_empty()
            || self.entries.len() > t.capacity()
            || self.roster.len() != self.entries.len()
            || self.cassettes.len() != usize::from(t.cassette_count)
            || self.high_watermark != self.operations.len() as u64
            || !unique(self.entries.iter().map(|e| e.drone_id))
            || !unique(self.roster.iter().map(|e| e.drone_id))
            || !unique(self.cassettes.iter().map(|c| c.cassette_id))
            || !unique(self.cleanups.iter().map(|c| c.operation_id))
        {
            return false;
        }
        for identity in &self.roster {
            if identity.drone_id == 0
                || !self
                    .entry(identity.drone_id)
                    .is_some_and(|e| e.class == identity.class)
                || identity.preferred_magazine_slot.is_some_and(|id| {
                    !t.slot(id)
                        .is_some_and(|s| s.compatible_classes.contains(&identity.class))
                })
            {
                return false;
            }
        }
        let mut occupied = BTreeSet::new();
        let mut carried = BTreeSet::new();
        for cassette in &self.cassettes {
            if let Some(id) = cassette.drone_id {
                if !carried.insert(id)
                    || !self.entry(id).is_some_and(|e| {
                        !matches!(
                            e.location,
                            CarrierDroneLocation::Deployed | CarrierDroneLocation::Destroyed { .. }
                        )
                    })
                {
                    return false;
                }
            }
            match cassette.location {
                CarrierCassetteLocation::Docked { dock } => {
                    if !t.contains_dock(dock) || !occupied.insert(dock) {
                        return false;
                    }
                    if let Some(id) = cassette.drone_id {
                        if !t.accepts(dock, self.entry(id).unwrap().class) {
                            return false;
                        }
                    }
                    if let CarrierCassetteDock::TransitBuffer { buffer_id } = dock {
                        if !self.operations.iter().any(|o| {
                            self.retains_hardware(o)
                                && o.stage.as_ref().is_some_and(|x| {
                                    x.buffer_id == buffer_id
                                        && x.empty_cassette_id == cassette.cassette_id
                                })
                        }) {
                            return false;
                        }
                    }
                }
                CarrierCassetteLocation::InLoader {
                    operation_id,
                    from,
                    to,
                } => {
                    let Some(op) = self.operation(operation_id) else {
                        return false;
                    };
                    if !self.retains_hardware(op)
                        || !t.contains_dock(from)
                        || !t.contains_dock(to)
                        || from == to
                        || op.stage.is_none()
                    {
                        return false;
                    }
                    let exchange = op.stage.as_ref().unwrap();
                    let Some(expected) = exchange.current_leg() else {
                        return false;
                    };
                    if expected != (cassette.cassette_id, from, to)
                        || !self.reservations.iter().any(|r| {
                            r.operation_id == operation_id
                                && r.resource
                                    == (CarrierResource::Cassette {
                                        cassette_id: cassette.cassette_id,
                                    })
                        })
                    {
                        return false;
                    }
                }
            }
        }
        for entry in &self.entries {
            let destroyed = matches!(entry.location, CarrierDroneLocation::Destroyed { .. });
            if (destroyed && entry.hit_points != 0)
                || (!destroyed && !(1..=DRONE_HIT_POINTS).contains(&entry.hit_points))
            {
                return false;
            }
            if let CarrierDroneLocation::Destroyed { at_ms } = entry.location {
                if at_ms < self.initialized_at_ms {
                    return false;
                }
            }
            if let Some(dock) = entry.location.dock() {
                if !t.accepts(dock, entry.class)
                    || self
                        .docked_cassette(dock)
                        .is_none_or(|c| c.drone_id != Some(entry.drone_id))
                {
                    return false;
                }
            }
            if matches!(entry.location, CarrierDroneLocation::Loading { .. })
                && !carried.contains(&entry.drone_id)
            {
                return false;
            }
            if let Some(id) = entry.location.operation_id() {
                if !self.operation(id).is_some_and(|o| {
                    o.drone_id == entry.drone_id
                        && !o.phase.terminal()
                        && self.location_matches(o, entry)
                }) {
                    return false;
                }
            }
        }
        let mut live = BTreeSet::new();
        let mut services = BTreeSet::new();
        for (index, op) in self.operations.iter().enumerate() {
            let Some(entry) = self.entry(op.drone_id) else {
                return false;
            };
            if (matches!(op.action, CarrierInventoryAction::StoreReady { .. })
                && !matches!(self.binding.registration.assembly_revision, 1702 | 1703))
                || op.operation_id != index as u64 + 1
                || op.accepted_at_ms < self.initialized_at_ms
                || op.accepted_at_ms > op.updated_at_ms
                || op.message.len() > 512
                || (op.phase == CarrierOperationPhase::Held) != op.held_phase.is_some()
                || op
                    .held_phase
                    .is_some_and(|p| p == CarrierOperationPhase::Held || p.terminal())
                || op.berth_id.is_some_and(|id| {
                    !t.berth(id)
                        .is_some_and(|b| b.compatible_classes.contains(&entry.class))
                })
                || op.cargo_service_operation_ids.contains(&0)
                || !op
                    .cargo_service_operation_ids
                    .iter()
                    .all(|id| services.insert(*id))
                || !op
                    .cargo_service_operation_ids
                    .windows(2)
                    .all(|p| p[0] < p[1])
            {
                return false;
            }
            if op.phase == CarrierOperationPhase::Destroyed
                && !matches!(entry.location, CarrierDroneLocation::Destroyed { .. })
            {
                return false;
            }
            if !op.phase.terminal() {
                if !op.released_loaded_owner()
                    && (!live.insert(op.drone_id)
                        || matches!(entry.location, CarrierDroneLocation::Destroyed { .. }))
                {
                    return false;
                }
                if op.effective_phase() == CarrierOperationPhase::Queued {
                    if op.stage.is_some()
                        || op.berth_id.is_some()
                        || self
                            .reservations
                            .iter()
                            .any(|r| r.operation_id == op.operation_id)
                        || !queued_eligible(op.action, &entry.location, entry.class)
                    {
                        return false;
                    }
                } else if !op.released_loaded_owner() && !self.location_matches(op, entry) {
                    return false;
                }
            }
            if self.retains_hardware(op)
                && op.effective_phase() != CarrierOperationPhase::Queued
                && op.stage.is_none()
            {
                let Some(berth_id) = op.berth_id else {
                    return false;
                };
                if !self.reservations.iter().any(|r| {
                    r.operation_id == op.operation_id
                        && r.drone_id == op.drone_id
                        && r.resource == (CarrierResource::ReadyBerth { berth_id })
                }) {
                    return false;
                }
                if matches!(
                    op.action,
                    CarrierInventoryAction::Launch | CarrierInventoryAction::RecallToStorage
                ) {
                    let Some(cassette) =
                        self.docked_cassette(CarrierCassetteDock::Ready { berth_id })
                    else {
                        return false;
                    };
                    if !self.reservations.iter().any(|r| {
                        r.operation_id == op.operation_id
                            && r.drone_id == op.drone_id
                            && r.resource
                                == (CarrierResource::Cassette {
                                    cassette_id: cassette.cassette_id,
                                })
                    }) {
                        return false;
                    }
                }
            }
            if let Some(stage) = &op.stage {
                if !self.valid_stage(t, op, stage) {
                    return false;
                }
            } else if matches!(
                op.action,
                CarrierInventoryAction::Stage { .. } | CarrierInventoryAction::StoreReady { .. }
            ) && op.effective_phase() != CarrierOperationPhase::Queued
                && !matches!(
                    op.phase,
                    CarrierOperationPhase::Cancelled | CarrierOperationPhase::Destroyed
                )
            {
                return false;
            }
        }
        for cleanup in &self.cleanups {
            if cleanup.reason.len() > 512
                || !self.operation(cleanup.operation_id).is_some_and(|op| {
                    op.phase == CarrierOperationPhase::Destroyed
                        && (op.stage.is_some() || op.berth_id.is_some())
                        && matches!(
                            self.entry(op.drone_id).unwrap().location,
                            CarrierDroneLocation::Destroyed { .. }
                        )
                })
            {
                return false;
            }
        }
        let mut resources = BTreeSet::new();
        for reservation in &self.reservations {
            let Some(op) = self.operation(reservation.operation_id) else {
                return false;
            };
            if !resources.insert(reservation.resource)
                || op.drone_id != reservation.drone_id
                || !self.retains_hardware(op)
            {
                return false;
            }
            if let Some(stage) = &op.stage {
                if !stage.retained_resources().contains(&reservation.resource) {
                    return false;
                }
            } else {
                match reservation.resource {
                    CarrierResource::ReadyBerth { berth_id } if op.berth_id == Some(berth_id) => (),
                    CarrierResource::Cassette { cassette_id }
                        if op.berth_id.is_some_and(|berth_id| {
                            self.docked_cassette(CarrierCassetteDock::Ready { berth_id })
                                .is_some_and(|c| c.cassette_id == cassette_id)
                        }) => {}
                    _ => return false,
                }
            }
        }
        true
    }
    fn retains_hardware(&self, op: &CarrierInventoryOperation) -> bool {
        !op.phase.terminal()
            || self
                .cleanups
                .iter()
                .any(|c| c.operation_id == op.operation_id)
    }
    fn location_matches(&self, op: &CarrierInventoryOperation, entry: &CarrierDroneEntry) -> bool {
        let phase = op.effective_phase();
        match (&entry.location, op.action) {
            (
                CarrierDroneLocation::Loading { operation_id },
                CarrierInventoryAction::Stage { .. } | CarrierInventoryAction::StoreReady { .. },
            ) => {
                *operation_id == op.operation_id
                    && matches!(
                        phase,
                        CarrierOperationPhase::Loading | CarrierOperationPhase::Securing
                    )
            }
            (
                CarrierDroneLocation::Launching {
                    operation_id,
                    berth_id,
                },
                CarrierInventoryAction::Launch,
            ) => {
                *operation_id == op.operation_id
                    && op.berth_id == Some(*berth_id)
                    && matches!(
                        phase,
                        CarrierOperationPhase::Launching | CarrierOperationPhase::Securing
                    )
            }
            (
                CarrierDroneLocation::Returning {
                    operation_id,
                    berth_id,
                },
                CarrierInventoryAction::RecallToStorage
                | CarrierInventoryAction::ReturnToUnload { .. },
            ) => {
                *operation_id == op.operation_id
                    && op.berth_id == Some(*berth_id)
                    && matches!(
                        phase,
                        CarrierOperationPhase::Returning | CarrierOperationPhase::Securing
                    )
            }
            (
                CarrierDroneLocation::Servicing {
                    operation_id,
                    berth_id,
                },
                CarrierInventoryAction::ReturnToUnload { .. }
                | CarrierInventoryAction::RecallToStorage,
            ) => {
                *operation_id == op.operation_id
                    && op.berth_id == Some(*berth_id)
                    && matches!(
                        phase,
                        CarrierOperationPhase::Servicing | CarrierOperationPhase::Securing
                    )
                    && entry.class == DroneClass::Mining
            }
            _ => false,
        }
    }
    fn valid_stage(
        &self,
        t: &CarrierInventoryTopology,
        op: &CarrierInventoryOperation,
        x: &CarrierStageExchange,
    ) -> bool {
        let entry = self.entry(op.drone_id).unwrap();
        let expected_action = match x.direction {
            CarrierStageDirection::ToReady => CarrierInventoryAction::Stage {
                berth_id: x.destination_berth_id,
            },
            CarrierStageDirection::ToProtected => CarrierInventoryAction::StoreReady {
                slot_id: x.source_slot_id,
            },
        };
        if (!x.direction.is_forward()
            && !matches!(self.binding.registration.assembly_revision, 1702 | 1703))
            || op.action != expected_action
            || op.berth_id != Some(x.destination_berth_id)
            || x.loaded_handoff_at_ms.is_some_and(|at| {
                self.binding.registration.assembly_revision != 1702
                    || !x.direction.is_forward()
                    || at < op.accepted_at_ms
                    || at > op.updated_at_ms
                    || x.completed_legs < 2
                    || !matches!(
                        op.effective_phase(),
                        CarrierOperationPhase::Securing | CarrierOperationPhase::Completed
                    )
            })
            || x.completed_legs > 3
            || x.loaded_cassette_id == x.empty_cassette_id
            || t.stage_loader(x.source_slot_id, x.destination_berth_id, entry.class)
                != Some(x.loader_id)
            || t.loader(x.loader_id)
                .is_none_or(|l| l.transit_buffer_id != x.buffer_id)
            || self.cassette(x.loaded_cassette_id).is_none()
            || self.cassette(x.empty_cassette_id).is_none()
            || (op.phase == CarrierOperationPhase::Completed && x.completed_legs != 3)
        {
            return false;
        }
        if !self.retains_hardware(op) {
            return true;
        } // Historical receipt does not own today's cassette locations.
        let owned: BTreeSet<_> = self
            .reservations
            .iter()
            .filter(|r| r.operation_id == op.operation_id)
            .map(|r| r.resource)
            .collect();
        if owned != x.retained_resources().into_iter().collect() {
            return false;
        }
        let loaded = self.cassette(x.loaded_cassette_id).unwrap();
        let empty = self.cassette(x.empty_cassette_id).unwrap();
        if let Some(at) = x.loaded_handoff_at_ms {
            // While the tail is live, the released body must remain at the exact
            // accepted Ready handoff or belong to a later same-berth Launch.
            let subsequent_launch = |id: Option<u64>, completed: bool| {
                self.operations.iter().any(|next| {
                    next.operation_id > op.operation_id
                        && id.is_none_or(|id| id == next.operation_id)
                        && next.drone_id == op.drone_id
                        && next.action == CarrierInventoryAction::Launch
                        && next.berth_id == Some(x.destination_berth_id)
                        && next.accepted_at_ms >= at
                        && if completed {
                            next.phase == CarrierOperationPhase::Completed
                        } else {
                            !next.phase.terminal()
                                && next.effective_phase() != CarrierOperationPhase::Queued
                        }
                })
            };
            let released_location = match entry.location {
                CarrierDroneLocation::Ready { berth_id } => {
                    berth_id == x.destination_berth_id && loaded.drone_id == Some(op.drone_id)
                }
                CarrierDroneLocation::Launching {
                    operation_id,
                    berth_id,
                } => {
                    berth_id == x.destination_berth_id
                        && operation_id > op.operation_id
                        && subsequent_launch(Some(operation_id), false)
                        && loaded.drone_id == Some(op.drone_id)
                }
                CarrierDroneLocation::Deployed => subsequent_launch(None, true),
                CarrierDroneLocation::Destroyed { .. } => true,
                _ => false,
            };
            if !released_location {
                return false;
            }
        }
        let dead = op.phase == CarrierOperationPhase::Destroyed;
        if (x.loaded_handoff_at_ms.is_none()
            && loaded.drone_id != if dead { None } else { Some(op.drone_id) })
            || empty.drone_id.is_some()
        {
            return false;
        }
        let slot = x.loaded_source();
        let berth = x.loaded_destination();
        let buffer = CarrierCassetteDock::TransitBuffer {
            buffer_id: x.buffer_id,
        };
        let docked =
            |c: &CarrierCassette, dock| c.location == CarrierCassetteLocation::Docked { dock };
        let at_or_moving = |c: &CarrierCassette, from, to| {
            docked(c, from)
                || c.location
                    == CarrierCassetteLocation::InLoader {
                        operation_id: op.operation_id,
                        from,
                        to,
                    }
        };
        match x.completed_legs {
            0 => docked(loaded, slot) && at_or_moving(empty, berth, buffer),
            1 => docked(empty, buffer) && at_or_moving(loaded, slot, berth),
            2 => {
                (x.loaded_handoff_at_ms.is_some() || docked(loaded, berth))
                    && at_or_moving(empty, buffer, slot)
            }
            3 => (x.loaded_handoff_at_ms.is_some() || docked(loaded, berth)) && docked(empty, slot),
            _ => false,
        }
    }
}
fn queued_eligible(
    action: CarrierInventoryAction,
    location: &CarrierDroneLocation,
    class: DroneClass,
) -> bool {
    match action {
        CarrierInventoryAction::Stage { .. } => {
            matches!(location, CarrierDroneLocation::Protected { .. })
        }
        CarrierInventoryAction::StoreReady { .. } => {
            matches!(location, CarrierDroneLocation::Ready { .. })
        }
        CarrierInventoryAction::Launch => matches!(location, CarrierDroneLocation::Ready { .. }),
        CarrierInventoryAction::ReturnToUnload { .. } => {
            *location == CarrierDroneLocation::Deployed && class == DroneClass::Mining
        }
        CarrierInventoryAction::RecallToStorage => *location == CarrierDroneLocation::Deployed,
    }
}
