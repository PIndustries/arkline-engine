//! Vessel-owned physical gun travel and deferred warp interlock. Screen policy
//! never enters the security decision. A pending destination is not a debit.
use super::*;
use arkline_protocol::drone_bay_geometry::BayPose;

/// Only the new, exact composed assembly may enable the physical path. This
/// intentionally leaves every unregistered / legacy world on its old behavior.
pub(super) fn enabled(w: &World, name: &str) -> bool {
    let Some(registration) = w.carrier_inventory_registration.as_ref() else {
        return false;
    };
    arkline_protocol::parallel_turret_binding::exact_registration(registration)
        && carrier_inventory::world::definition(w)
            .ok()
            .flatten()
            .is_some()
        && carrier_inventory::world::state(w, name).is_some_and(|s| {
            s.inventory.binding.registration == *registration
                && w.accounts
                    .get(name)
                    .and_then(|a| a.vessel_id(ShipClass::Pioneer))
                    == Some(s.inventory.binding.vessel_id.as_str())
        })
        && w.pilots
            .get(name)
            .is_some_and(|p| p.view.ship_class == ShipClass::Pioneer)
}

/// Persisted corrupt numeric state is rejected, never clamped into secure pose.
pub(super) fn validate(w: &World, now: u64) -> Result<()> {
    for (name, account) in &w.accounts {
        if let Some(m) = &account.cockpit_mechanisms {
            m.turrets.validate_at(now).map_err(anyhow::Error::msg)?;
            if m.turrets.source_admitted {
                ensure!(
                    enabled(w, name),
                    "Persisted turret source/owner unavailable"
                );
                ensure!(
                    account.vessel_id(ShipClass::Pioneer) == Some(m.vessel_id.as_str()),
                    "Persisted turret vessel binding mismatch"
                );
                ensure!(
                    m.turrets.aims.as_ref().is_none_or(|aims| aims
                        .iter()
                        .all(|aim| aim.sample_ms == m.turrets.updated_at_ms)),
                    "Persisted turret aim clock differs from checked physical sample"
                );
            }
        }
    }
    Ok(())
}

/// Read-only gate for all non-deferred travel paths. Normal carrier admission
/// remains independently blocked by carrier_inventory::world::authorize.
pub(super) fn authorize(w: &World, name: &str, request: &Request, now: u64) -> Result<()> {
    if !enabled(w, name) {
        return Ok(());
    }
    let s = state(w, name).context("physical turret telemetry unavailable")?;
    s.validate_at(now).map_err(anyhow::Error::msg)?;
    if matches!(request, Request::Jump { .. }) {
        carrier_inventory::world::secured_for_departure(w, name, now)?;
        ensure!(
            s.pending_warp.is_none(),
            "cancel pending warp before gate jump"
        );
        ensure!(
            (0..2).all(|slot| !obstructed(w, name, slot, 0., 0., now)),
            "secured turret envelope obstructed"
        );
    }
    if s.pending_warp.is_some()
        && matches!(request,Request::Navigate{mode} if *mode!=NavigationMode::Stop)
    {
        bail!("STOP cancels pending warp before a new navigation intent");
    }
    if s.pending_warp.is_some()
        && matches!(
            request,
            Request::PilotInput { .. } | Request::SelectShip { .. } | Request::BoardShip { .. }
        )
    {
        bail!("STOP cancels pending warp before changing vessel motion/ownership");
    }
    Ok(())
}

/// Caller first validates the complete inventory/source runtime. Exterior
/// approaches are keyed by operation, unlike departures keyed by drone.
pub(super) fn source_bound_body(s: &carrier_inventory::world::OwnedCarrier, id: u64) -> bool {
    s.arrivals.values().all(|a| a.drone_id != id)
        && !s.departures.contains_key(&id)
        && s.inventory.entry(id).is_some_and(|e| {
            matches!(
                e.location,
                CarrierDroneLocation::Ready { .. } | CarrierDroneLocation::Protected { .. }
            ) || (matches!(
                e.location,
                CarrierDroneLocation::Loading { .. }
                    | CarrierDroneLocation::Launching { .. }
                    | CarrierDroneLocation::Returning { .. }
                    | CarrierDroneLocation::Servicing { .. }
            ) && s
                .runs
                .values()
                .any(|r| r.drone_id == id && r.binding == s.inventory.binding))
        })
}

fn registered_drone_radius() -> f64 {
    // Registered source body [2.5,.9,3]m; all corners, not a 3m half-axis.
    [2.5_f64, 0.9, 3.0]
        .iter()
        .map(|v| v * v)
        .sum::<f64>()
        .sqrt()
}

fn state<'a>(w: &'a World, name: &str) -> Option<&'a TurretDeployment> {
    let p = w.pilots.get(name)?;
    if p.view.ship_class != ShipClass::Pioneer {
        return None;
    }
    w.accounts
        .get(name)?
        .cockpit_mechanisms
        .as_ref()
        .map(|m| &m.turrets)
}
fn store(w: &mut World, name: &str, s: TurretDeployment) {
    if let Some(m) = w
        .accounts
        .get_mut(name)
        .and_then(|a| a.cockpit_mechanisms.as_mut())
    {
        m.turrets = s.clone();
    }
    if let Some(m) = w
        .pilots
        .get_mut(name)
        .and_then(|p| p.view.cockpit_mechanisms.as_mut())
    {
        m.turrets = s;
    }
}
/// Shared source bounds include the complete home/retract path. Missing or
/// invalid registration is obstruction, never implicit clearance.
pub(super) fn obstructed(w: &World, name: &str, slot: usize, from: f64, to: f64, now: u64) -> bool {
    let Some(registration) = w.carrier_inventory_registration.as_ref() else {
        return true;
    };
    let Some(bounds) =
        arkline_protocol::parallel_turret_binding::sweep(registration, slot, from, to)
    else {
        return true;
    };
    if carrier_inventory::world::validate(w).is_err() {
        return true;
    }
    let p = &w.pilots[name].view;
    // Reserve a conservative reachable ball for the whole motor interval, and
    // the entire elapsed authority gap. This intentionally over-holds near
    // moving peers until source-specific route reservations are integrated.
    let horizon = state(w, name).map_or(TURRET_TRAVEL_MS, |s| {
        now.saturating_sub(s.updated_at_ms).max(TURRET_TRAVEL_MS)
    }) as f64
        / 1000.;
    let Some(f) = &p.flight else {
        return true;
    };
    let inverse = BayPose {
        position_m: [0.; 3],
        orientation_xyzw: [
            -f.orientation[0],
            -f.orientation[1],
            -f.orientation[2],
            f.orientation[3],
        ],
    };
    let sphere_hits = |position: [f64; 3], radius: f64| {
        let Some(local) =
            inverse.transform_vector(std::array::from_fn(|i| position[i] - f.position[i]))
        else {
            return true;
        };
        let delta: [f64; 3] = std::array::from_fn(|i| local[i] - bounds.center_m[i]);
        let squared = bounds
            .axes
            .iter()
            .zip(bounds.half_m)
            .map(|(axis, half)| {
                let along: f64 = (0..3).map(|i| delta[i] * axis[i]).sum();
                (along.abs() - half).max(0.).powi(2)
            })
            .sum::<f64>();
        squared <= radius * radius
    };
    if w.asteroids.iter().any(|a| {
        a.system_id == p.system_id
            && sphere_hits(
                a.position,
                a.radius
                    + flight::length(f.velocity) * horizon
                    + ship_acceleration(p.ship_class, 0) * horizon * horizon * 0.5,
            )
    }) {
        return true;
    }
    for (other_name, other) in &w.pilots {
        if other_name != name && other.view.system_id == p.system_id {
            if let Some(flight) = &other.view.flight {
                // Shared source-qualified owner envelope; never approximate an
                // advertised carrier peer with the legacy ShipClass radius.
                let peer_radius = match docking::owner_envelope(w, other_name) {
                    Ok(Some(envelope)) => match envelope.origin_radius_m() {
                        Some(radius) if radius.is_finite() && radius > 0. => radius,
                        _ => return true,
                    },
                    Ok(None) => hull_radius_m(other.view.ship_class),
                    Err(_) => return true,
                };
                if sphere_hits(
                    flight.position,
                    peer_radius
                        + (flight::length(flight.velocity) + flight::length(f.velocity)) * horizon
                        + (ship_acceleration(other.view.ship_class, 0)
                            + ship_acceleration(p.ship_class, 0))
                            * horizon
                            * horizon
                            * 0.5,
                ) {
                    return true;
                }
            }
        }
        if other.view.system_id == p.system_id
            && other.view.drones.iter().any(|d| {
                // Exact validated source-bound owned routes (including held
                // poses) are separated by the complete1703 turret union proof.
                // Free/deployed bodies retain conservative reachable bounds.
                if other_name == name
                    && carrier_inventory::world::state(w, name)
                        .is_some_and(|s| source_bound_body(s, d.id))
                {
                    return false;
                }
                sphere_hits(
                    d.position,
                    registered_drone_radius()
                        + (drone_speed(d.class).max(flight::length(d.velocity))
                            + flight::length(f.velocity)
                            + other
                                .view
                                .flight
                                .as_ref()
                                .map_or(0., |f| flight::length(f.velocity)))
                            * horizon
                        + ship_acceleration(p.ship_class, 0) * horizon * horizon * 0.5,
                )
            })
        {
            return true;
        }
        if let Some(traversal) = other
            .view
            .station_avatar
            .as_ref()
            .and_then(|a| a.docked_traversal.as_ref())
        {
            if w.accounts[name].vessel_id(ShipClass::Pioneer)
                == Some(traversal.binding.host_vessel_id.as_str())
            {
                let Ok(pose) = docked_binding_pose(&traversal.binding) else {
                    return true;
                };
                if arkline_protocol::pioneer_docked::box_intersects_capsule(
                    bounds,
                    pose.station_to_body(traversal.feet_m),
                    1.8,
                    0.3,
                ) {
                    return true;
                }
            }
        }
    }
    false
}

pub(super) fn set_deployment(
    w: &mut World,
    name: &str,
    vessel: &str,
    deployed: bool,
    now: u64,
) -> Result<()> {
    ensure!(
        enabled(w, name),
        "Physical turret source is not admitted in this world"
    );
    let p = &w.pilots.get(name).context("pilot missing")?.view;
    ensure!(
        p.ship_class == ShipClass::Pioneer,
        "physical turret controls require Pioneer"
    );
    ensure!(
        w.accounts
            .get(name)
            .and_then(|a| a.vessel_id(ShipClass::Pioneer))
            == Some(vessel),
        "turret vessel ownership changed"
    );
    ensure!(
        !matches!(
            p.location,
            Location::InTransit { .. } | Location::Jumping { .. }
        ),
        "turrets must stay secured during transit"
    );
    if let Some(avatar) = &p.station_avatar {
        ensure!(
            avatar
                .docked_traversal
                .as_ref()
                .is_some_and(|t| t.binding.host_vessel_id == vessel),
            "board the owned ship before operating turrets"
        );
    }
    let mut s = state(w, name)
        .context("turret telemetry unavailable")?
        .clone();
    ensure!(
        !deployed || s.pending_warp.is_none(),
        "cancel pending warp before deploying turrets"
    );
    if !deployed {
        mining::stop_all(w.pilots.get_mut(name).unwrap(), "Turrets stowing");
    }
    s.validate_at(now).map_err(anyhow::Error::msg)?;
    s.command(deployed, now, |slot, from, to| {
        obstructed(w, name, slot, from, to, now)
    });
    store(w, name, s);
    Ok(())
}

/// Legacy mining path selector. Advertised source guns must use the separate
/// authoritative aim/line-of-fire path, even when public projection is absent.
pub(super) fn mining_aim_available(p: &PilotView, world_advertised: bool) -> bool {
    p.carrier_inventory.is_none()
        && !(p.ship_class == ShipClass::Pioneer
            && (world_advertised
                || p.cockpit_mechanisms
                    .as_ref()
                    .is_some_and(|m| m.turrets.source_admitted)))
}

pub(super) fn can_fire(p: &PilotView, now: u64) -> bool {
    p.ship_class != ShipClass::Pioneer
        || p.cockpit_mechanisms
            .as_ref()
            .is_none_or(|m| !m.turrets.source_admitted || m.turrets.operational_at(now))
}

/// Necessary physical/resource condition, never sufficient normal admission.
pub(super) fn inventory_departure_clear(w: &World, name: &str) -> Result<()> {
    carrier_inventory::world::inventory_secured(w, name)
}
/// Security is evaluated at the last accepted sample, never a predicted motor
/// endpoint. Every caller still retains its independent movement admission gate.
pub(super) fn confirmed_secure(w: &World, name: &str, now: u64) -> Result<()> {
    ensure!(
        enabled(w, name),
        "Exact source-owned turret controls unavailable"
    );
    let s = state(w, name).context("Physical turret telemetry unavailable")?;
    s.validate_at(now).map_err(anyhow::Error::msg)?;
    ensure!(
        s.source_admitted && s.secure_at(s.updated_at_ms),
        "Physical turrets have not confirmed a secured endpoint"
    );
    ensure!(
        (0..2).all(|slot| !obstructed(w, name, slot, 0., 0., now)),
        "Secured turret envelope obstructed"
    );
    Ok(())
}
fn departure_ready(w: &World, name: &str, destination: Destination) -> Result<()> {
    let request = Request::Warp { destination };
    cargo_unloading::authorize(w, name, &request)?;
    drone_bay::authorize(w, name, &request)?;
    drone_cargo::authorize(w, name, &request)?;
    independent_bays::world::authorize(w, name, &request)?;
    carrier_inventory::world::authorize(w, name, &request)?;
    carrier_inventory::world::validate(w)?;
    inventory_departure_clear(w, name)?;
    let p = w.pilots.get(name).context("pilot missing")?;
    ensure!(p.view.station_avatar.is_none(), "board ship before warp");
    ensure!(!docking::busy(p), "docking authority owns vessel motion");
    ensure!(
        matches!(p.view.location, Location::Station | Location::Belt),
        "must be in space and not in transit"
    );
    if let Destination::Gate { id } = destination {
        ensure!(
            universe::gate(id)?.system_id == p.view.system_id,
            "gate is in another system"
        );
    }
    ensure!(
        flight::distance(
            p.view
                .flight
                .as_ref()
                .context("flight unavailable")?
                .position,
            flight::berth(name, destination, p.view.ship_class)
        ) > 100.,
        "already at destination"
    );
    ensure!(
        p.view
            .power
            .as_ref()
            .is_some_and(|pwr| pwr.charge_j >= power_spec(p.view.ship_class).warp_cost_j),
        "insufficient capacitor energy for warp reservation"
    );
    Ok(())
}
fn launch(w: &mut World, name: &str, destination: Destination, now: u64) -> Result<()> {
    departure_ready(w, name, destination)?;
    carrier_inventory::world::secured_for_departure(w, name, now)?;
    let p = w.pilots.get_mut(name).unwrap();
    ensure!(
        power::spend(
            &mut p.view.power,
            power_spec(p.view.ship_class).warp_cost_j,
            now
        ),
        "insufficient capacitor energy for warp reservation"
    );
    universe::clear_locks(p);
    flight::begin_warp(p, destination, now);
    p.view.location = Location::InTransit {
        destination,
        arrives_at_ms: p
            .view
            .flight
            .as_ref()
            .unwrap()
            .warp
            .as_ref()
            .unwrap()
            .arrives_at_ms,
    };
    let mut s = state(w, name).unwrap().clone();
    s.pending_warp = None;
    s.status = "Warp initiated; turrets secured".into();
    store(w, name, s);
    Ok(())
}
/// Returns an explicit receipt; queueing never reports that warp has started.
pub(super) fn request_warp(
    w: &mut World,
    name: &str,
    destination: Destination,
    now: u64,
) -> Result<&'static str> {
    ensure!(
        enabled(w, name),
        "Physical turret source is not admitted in this world"
    );
    departure_ready(w, name, destination)?;
    let mut s = state(w, name)
        .context("physical turret telemetry unavailable")?
        .clone();
    s.validate_at(now).map_err(anyhow::Error::msg)?;
    s.pending_warp = Some(destination);
    mining::stop_all(w.pilots.get_mut(name).unwrap(), "Securing turrets for warp");
    s.command(false, now, |slot, from, to| {
        obstructed(w, name, slot, from, to, now)
    });
    let secure = s.secure_at(now);
    store(w, name, s);
    if secure {
        ensure!(
            (0..2).all(|slot| !obstructed(w, name, slot, 0., 0., now)),
            "Secured turret envelope obstructed; clear it before warp"
        );
        launch(w, name, destination, now)?;
        Ok("Warp initiated; turrets secured")
    } else {
        Ok("Securing turrets for warp; STOP cancels launch")
    }
}

/// Call before mining tick/commands; no queued launch is dispatched by reads.
pub(super) fn advance(w: &mut World, now: u64) {
    let names: Vec<_> = w.pilots.keys().cloned().collect();
    for name in names {
        if !enabled(w, &name) {
            continue;
        }
        let Some(mut s) = state(w, &name).cloned() else {
            continue;
        };
        if s.validate_at(now).is_err() {
            continue;
        }
        s.source_admitted = true;
        s.advance(now, |slot, from, to| {
            obstructed(w, &name, slot, from, to, now)
        });
        if !s.operational_at(now) {
            mining::stop_all(
                w.pilots.get_mut(&name).unwrap(),
                "Physical turrets unavailable",
            );
        }
        store(w, &name, s);
    }
}
/// Tick-only dispatch, after all movement/bay/cargo reconciliation and before
/// persistence. Every original departure gate is evaluated again at this point.
pub(super) fn dispatch(w: &mut World, now: u64) {
    let pending: Vec<_> = w
        .pilots
        .keys()
        .filter_map(|name| {
            if !enabled(w, name) {
                return None;
            }
            let s = state(w, name)?;
            s.secure_at(now)
                .then(|| s.pending_warp.map(|d| (name.clone(), d)))
                .flatten()
        })
        .collect();
    for (name, destination) in pending {
        if (0..2).any(|slot| obstructed(w, &name, slot, 0., 0., now)) {
            cancel(
                w,
                &name,
                "Warp canceled: secured turret envelope obstructed",
            );
            continue;
        }
        if let Err(error) = launch(w, &name, destination, now) {
            cancel(w, &name, &format!("Warp canceled: {error}"));
        }
    }
}
pub(super) fn cancel(w: &mut World, name: &str, reason: &str) {
    if let Some(mut s) = state(w, name).cloned() {
        if s.pending_warp.take().is_some() {
            s.status = reason.into();
            store(w, name, s);
        }
    }
}
pub(super) fn hold(w: &mut World, name: &str, now: u64, reason: &str) {
    if let Some(mut s) = state(w, name).cloned().filter(|s| s.source_admitted) {
        s.hold(now, reason);
        store(w, name, s);
    }
}
pub(super) fn stop_on_restart(w: &mut World, now: u64) {
    for a in w.accounts.values_mut() {
        if let Some(m) = a
            .cockpit_mechanisms
            .as_mut()
            .filter(|m| m.turrets.source_admitted)
        {
            m.turrets.hold(
                now,
                "Server restarted; turret motion held and warp canceled",
            );
        }
    }
    for (name, p) in &mut w.pilots {
        if let Some(m) = p.view.cockpit_mechanisms.as_mut() {
            if let Some(saved) = w
                .accounts
                .get(name)
                .and_then(|a| a.cockpit_mechanisms.as_ref())
            {
                m.turrets = saved.turrets.clone();
            }
        }
    }
}
/// Later navigation and ownership intent replace a queued launch before it can
/// execute. Stow and display commands do not silently cancel travel.
pub(super) fn before_request(w: &mut World, name: &str, request: &Request, now: u64) {
    if matches!(
        request,
        Request::Navigate {
            mode: NavigationMode::Stop
        }
    ) {
        hold(
            w,
            name,
            now,
            "Pilot STOP; turret motion held and warp canceled",
        );
        return;
    }
    if matches!(
        request,
        Request::Navigate { .. }
            | Request::PilotInput { .. }
            | Request::Dock
            | Request::Undock
            | Request::Jump { .. }
            | Request::SelectShip { .. }
            | Request::BoardShip { .. }
            | Request::Connect { .. }
            | Request::CancelResourceJob { .. }
    ) {
        cancel(w, name, "Pending warp canceled by later pilot intent");
    }
}
