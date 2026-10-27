use super::*;
pub(crate) fn validate(w: &World) -> Result<()> {
    #[cfg(test)]
    crate::tick_profile::count("carrier_validate");
    let Some(def) = definition(w)? else {
        ensure!(
            w.carrier_inventory_loadout.is_none()
                && w.accounts.values().all(|a| a.carrier_inventory.is_none()),
            "Unstamped world cannot retain carrier inventory"
        );
        return Ok(());
    };
    ensure!(
        w.independent_bay_registration.is_none()
            && w.drone_cargo_registration.is_none()
            && w.drone_bay_registration.is_none()
            && w.drone_bay_batch_version.is_none(),
        "New carrier cannot alias legacy inventory/cargo authority"
    );
    ensure!(
        def.topology.valid_shape()
            && w.carrier_inventory_loadout
                .as_ref()
                .is_some_and(|l| l.len() == def.topology.capacity()),
        "Carrier creation loadout/source mismatch"
    );
    for (name, a) in &w.accounts {
        let s = state(w, name).context("Missing finite carrier initialization")?;
        let p = w.pilots.get(name).context("Orphan carrier account")?;
        let f = p.view.flight.as_ref().context("Carrier flight missing")?;
        ensure!(
            p.view.ship_class == ShipClass::Pioneer
                && f.mode == NavigationMode::Stop
                && f.velocity == [0.; 3]
                && f.acceleration == [0.; 3]
                && f.angular_velocity == [0.; 3],
            "Stationary source fixture carrier cannot move/change hull"
        );
        ensure!(
            s.inventory.binding.registration == def.topology.registration
                && a.vessel_id(ShipClass::Pioneer) == Some(s.inventory.binding.vessel_id.as_str())
                && s.inventory.validate_shape(&def.topology)
                && s.ore.validate_shape(&s.inventory),
            "Carrier inventory/cargo ownership invalid"
        );
        if !cargo::source_supports_service(def) {
            ensure!(
                s.ore
                    .holds
                    .iter()
                    .all(|h| h.ore_units == 0 && h.cycle.is_none() && h.mining_intent.is_none())
                    && s.ore.services.is_empty(),
                "This source launch milestone does not admit cargo extraction/service yet"
            );
        }
        cargo::validate(w, name, s)?;
        ensure!(
            a.drone_bays.is_empty()
                && a.drone_cargo.is_empty()
                && a.drone_bay_runtime.is_empty()
                && a.drone_receivers.is_empty()
                && a.independent_bays.is_none(),
            "Carrier cannot retain legacy physical authorities"
        );
        let expected: BTreeSet<_> = def.cassettes.iter().map(|c| c.cassette_id).collect();
        ensure!(
            s.inventory
                .cassettes
                .iter()
                .map(|c| c.cassette_id)
                .collect::<BTreeSet<_>>()
                == expected
                && s.cassette_poses.keys().copied().collect::<BTreeSet<_>>() == expected,
            "Cassette set differs from authored identities"
        );
        ensure!(
            s.drone_poses.len() == s.inventory.entries.len()
                && s.inventory.entries.iter().all(|e| s
                    .drone_poses
                    .get(&e.drone_id)
                    .is_some_and(|p| p.valid() && p.position_m.iter().all(|x| x.abs() <= 1e6))),
            "Missing or malformed retained drone poses"
        );
        for c in &s.inventory.cassettes {
            let original = def
                .cassettes
                .iter()
                .find(|x| x.cassette_id == c.cassette_id)
                .unwrap();
            let pose = s.cassette_poses[&c.cassette_id];
            ensure!(
                pose.valid() && pose.orientation_xyzw == original.reference_pose.orientation_xyzw,
                "Cassette source attitude changed"
            );
            match c.location {
                CarrierCassetteLocation::Docked { dock } => {
                    ensure!(
                        def.topology
                            .same_dock_component(original.initial_dock, dock),
                        "Cassette crossed unsupported loader component"
                    );
                    ensure!(
                        def.docks
                            .iter()
                            .find(|d| d.dock == dock)
                            .is_some_and(|d| d.cassette_pose == pose),
                        "Docked cassette does not match actual source support"
                    );
                }
                CarrierCassetteLocation::InLoader {
                    operation_id,
                    from,
                    to,
                } => {
                    ensure!(
                        def.topology
                            .same_dock_component(original.initial_dock, from)
                            && def.topology.same_dock_component(original.initial_dock, to)
                            && s.runs.contains_key(&operation_id),
                        "Unsupported cassette component/loader history"
                    );
                }
            }
        }
        let idle = def
            .idle_roots(
                &s.cassette_poses
                    .iter()
                    .map(|(i, p)| (*i, *p))
                    .collect::<Vec<_>>(),
            )
            .context("Invalid actual cassette source poses")?;
        ensure!(
            s.machinery.len() == idle.len()
                && idle
                    .iter()
                    .all(|(id, _)| s.machinery.get(id).is_some_and(|p| p.valid())),
            "Missing/orphan hardware root"
        );
        for op in &s.inventory.operations {
            let expected = (!op.phase.terminal()
                && op.effective_phase() != CarrierOperationPhase::Queued)
                || s.inventory
                    .cleanups
                    .iter()
                    .any(|c| c.operation_id == op.operation_id);
            ensure!(
                (s.runs.contains_key(&op.operation_id)
                    || s.arrivals.contains_key(&op.operation_id))
                    == expected
                    && !(s.runs.contains_key(&op.operation_id)
                        && s.arrivals.contains_key(&op.operation_id)),
                "Physical operation/runtime ownership is not bijective"
            );
        }
        for (id, a) in &s.arrivals {
            ensure!(
                *id == a.operation_id
                    && !s.departures.contains_key(&a.drone_id)
                    && !s
                        .runs
                        .values()
                        .any(|r| r.drone_id == a.drone_id && r.kind.writes_drone()),
                "Approach overlaps another body owner"
            );
            a.validate(w, name, s)?;
        }
        for (id, d) in &s.departures {
            ensure!(
                *id == d.drone_id
                    && !s
                        .runs
                        .values()
                        .any(|r| r.drone_id == *id && r.kind.writes_drone()),
                "Departure overlaps hardware ownership"
            );
            d.validate(s, def, parent(w, name)?)?;
        }
        let mut owned = BTreeSet::new();
        for (id, r) in &s.runs {
            let op = s
                .inventory
                .operation(*id)
                .context("Orphan carrier runtime")?;
            let e = s.inventory.entry(op.drone_id).unwrap();
            ensure!(
                r.operation_id == *id
                    && r.drone_id == op.drone_id
                    && r.class == e.class
                    && r.binding == s.inventory.binding
                    && r.kind == run_kind(&s.inventory, *id)?,
                "Runtime binding/class/action differs from retained owner"
            );
            let cleanup = s.inventory.cleanups.iter().find(|c| c.operation_id == *id);
            ensure!(
                (op.phase == CarrierOperationPhase::Destroyed) == cleanup.is_some(),
                "Runtime terminal/cleanup mismatch"
            );
            if let Some(cleanup) = cleanup {
                ensure!(cleanup.held == r.held, "Cleanup hold mismatch");
            } else {
                ensure!(
                    !op.phase.terminal() && (op.phase == CarrierOperationPhase::Held) == r.held,
                    "Runtime live/hold mismatch"
                );
            }
            #[cfg(test)]
            crate::tick_profile::count("expected_route_rebuild_validate");
            r.validate_expected(
                &route(def, &r.kind, r.class)?,
                parent(w, name)?,
                p.view.system_id,
                &actual(s, &r.confirmed, r.drone_id)?,
            )?;
            if let Some(x) = &op.stage {
                ensure!(
                    x.completed_legs == r.completed_stage_legs
                        && x.loaded_handoff_at_ms
                            .is_none_or(|at| at <= r.started_at_ms),
                    "Stage cursor/handoff clock differs from retained motion"
                );
            }
            let target_phase = r.route[r.target_index].phase;
            ensure!(
                op.phase.terminal() || op.effective_phase() == target_phase,
                "Receipt phase differs from actual motion target"
            );
            for (k, _) in &r.confirmed.poses {
                if let CarrierPoseKey::Mechanism { semantic_id } = k {
                    ensure!(
                        owned.insert(semantic_id.clone()),
                        "Two routes own one hardware root"
                    );
                }
            }
        }
        for (id, pose) in idle {
            if !owned.contains(&id) {
                ensure!(
                    s.machinery[&id] == pose,
                    "Idle hardware changed without retained route"
                );
            }
        }
        for e in &s.inventory.entries {
            let live = p.view.drones.iter().find(|d| d.id == e.drone_id);
            let expects = matches!(
                e.location,
                CarrierDroneLocation::Launching { .. }
                    | CarrierDroneLocation::Returning { .. }
                    | CarrierDroneLocation::Servicing { .. }
                    | CarrierDroneLocation::Deployed
            );
            ensure!(
                live.is_some() == expects,
                "Finite inventory/live drone presence mismatch"
            );
            if let Some(d) = live {
                ensure!(
                    d.mode == DroneMode::Hold
                        || (cargo::source_supports_service(def)
                            && e.location == CarrierDroneLocation::Deployed
                            && matches!(d.mode, DroneMode::Mining { .. })),
                    "Unsupported free carrier navigation mode"
                );
                ensure!(
                    d.class == e.class && d.hit_points == e.hit_points,
                    "Finite identity class/HP mismatch"
                );
                if s.runs.values().any(|r| r.drone_id == e.drone_id)
                    || s.departures.contains_key(&e.drone_id)
                    || s.arrivals.values().any(|a| a.drone_id == e.drone_id)
                {
                    let expected = parent(w, name)?
                        .compose(s.drone_poses[&e.drone_id])
                        .context("Actual drone pose transform")?;
                    ensure!(
                        expected
                            .position_m
                            .iter()
                            .zip(d.position)
                            .all(|(a, b)| (a - b).abs() <= 0.001)
                            && d.orientation_xyzw
                                .is_some_and(|q| q == expected.orientation_xyzw
                                    || (s.arrivals.values().any(|a| a.drone_id == d.id)
                                        && carrier_composed_attitude_matches(
                                            q,
                                            expected.orientation_xyzw
                                        ))),
                        "Live drone differs from retained source frame"
                    );
                }
            } else if matches!(
                e.location,
                CarrierDroneLocation::Ready { .. } | CarrierDroneLocation::Protected { .. }
            ) {
                let c = s
                    .inventory
                    .cassettes
                    .iter()
                    .find(|c| c.drone_id == Some(e.drone_id))
                    .context("Stored body cassette missing")?;
                ensure!(
                    s.drone_poses[&e.drone_id] == s.cassette_poses[&c.cassette_id],
                    "Stored drone is not on its actual cassette"
                );
            }
        }
        ensure!(
            p.view
                .drones
                .iter()
                .all(|d| s.inventory.entry(d.id).is_some()),
            "Unrecorded active drone"
        );
    }
    Ok(())
}
pub(crate) fn validate_history(w: &World, db: &rusqlite::Connection) -> Result<()> {
    if w.carrier_inventory_registration.is_none() || w.accounts.is_empty() {
        return Ok(());
    }
    let mut query = db.prepare("SELECT json FROM ledger ORDER BY sequence")?;
    let ledger = query
        .query_map([], |r| r.get::<_, String>(0))?
        .map(|r| Ok(serde_json::from_str::<LedgerEntry>(&r?)?))
        .collect::<Result<Vec<_>>>()?;
    for (name, a) in &w.accounts {
        let s = a
            .carrier_inventory
            .as_ref()
            .context("Missing finite initialization")?;
        let init = ledger
            .iter()
            .filter_map(|e| match &e.event {
                EconomicEvent::CarrierInventory {
                    record:
                        CarrierInventoryLedgerEvent::Initialized {
                            pilot,
                            binding,
                            roster,
                            capacity_units_per_mining_drone,
                        },
                } if pilot == name => Some((e, binding, roster, capacity_units_per_mining_drone)),
                _ => None,
            })
            .collect::<Vec<_>>();
        ensure!(
            init.len() == 1,
            "Finite inventory genesis must occur exactly once"
        );
        let (e, binding, roster, capacity) = init[0];
        ensure!(
            e.ruleset_version == CARRIER_INVENTORY_RULESET_VERSION
                && e.timestamp_ms == s.inventory.initialized_at_ms
                && *binding == s.inventory.binding
                && *roster == s.inventory.roster
                && *capacity == MINING_DRONE_CARGO_CAPACITY,
            "Carrier genesis ledger differs from durable inventory"
        );
    }
    history::validate(w, &ledger)
}
pub(crate) fn preflight_existing(db: &rusqlite::Connection) -> Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='world')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let json: Option<String> = db
        .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
        .optional()?;
    let Some(json) = json else { return Ok(()) };
    let value: serde_json::Value = serde_json::from_str(&json)?;
    let marked = value
        .get("carrier_inventory_registration")
        .is_some_and(|x| !x.is_null())
        || value
            .get("carrier_inventory_loadout")
            .is_some_and(|x| !x.is_null())
        || value
            .get("accounts")
            .and_then(|x| x.as_object())
            .is_some_and(|a| {
                a.values()
                    .any(|a| a.get("carrier_inventory").is_some_and(|x| !x.is_null()))
            });
    if !marked {
        return Ok(());
    }
    let world: World = serde_json::from_value(value)?;
    validate(&world)?;
    validate_history(&world, db)
}
use rusqlite::OptionalExtension;
