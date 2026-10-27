//! Exact bridge to existing authoritative world actor types. Positions are always
//! source-derived; legacy ore is zero, while typed volume is a bound sidecar.
use super::*;
use arkline_protocol::*;

fn actor_views(world: &World) -> Result<(PilotView, Asteroid)> {
    let physical = world
        .physical_cargo
        .as_ref()
        .context("No physical authority")?;
    let control = physical
        .control
        .as_ref()
        .context("No world control binding")?;
    let state = physical.bay.clone().context("No source bay")?;
    let now = control.projection.accepted_at_ms;
    let v = bay::view(state.clone())?;
    let pose = v
        .source_frame
        .drone_pose
        .context("Missing actual drone pose")?;
    let p = &control.projection;
    let b = &control.binding;
    let mut drones = vec![];
    // Stored identity remains in the exact bay account, not a free-space actor.
    if state.deployed || state.route.is_some() || p.hit_points == 0 {
        drones.push(DroneView {
            orientation_xyzw: Some(pose.orientation_xyzw),
            id: b.drone_id,
            class: DroneClass::Mining,
            hit_points: p.hit_points,
            signal_strength: if p.signal_available {
                drone_signal_strength(pose.position_m, [0.; 3])
            } else {
                0.
            },
            mining_cycle: p.mining_cycle.clone(),
            attack_ready_at_ms: None,
            position: pose.position_m,
            velocity: [0.; 3],
            heading: [1., 0., 0.],
            mode: if p.recalling || state.route.is_some() {
                DroneMode::Returning
            } else if p.mining {
                DroneMode::Mining {
                    target_id: b.target_id,
                }
            } else {
                DroneMode::Hold
            },
            updated_at_ms: now,
        });
    }
    let locks: Vec<_> = p.target_lock.clone().into_iter().collect();
    let locked = if p.target_locked {
        vec![b.target_id]
    } else {
        vec![]
    };
    let pilot = PilotView {
        carrier_inventory: None,
        drone_cargo: None,
        power_accounting: None,
        drone_bay: None,
        cockpit_mechanisms: None,
        reactor_configuration: None,
        station_avatar: None,
        surface: None,
        power: Some(p.power.clone()),
        drone: drones.first().cloned(),
        drones,
        target_locks: locks,
        locked_asteroids: locked,
        mining_modules: vec![],
        target_lock: p.target_lock.clone(),
        ship_class: ShipClass::Pioneer,
        name: b.pilot.clone(),
        credits: 0,
        ore: 0,
        location: Location::Belt,
        flight: Some(FlightState {
            docking: None,
            acceleration: [0.; 3],
            position: [0.; 3],
            velocity: [0.; 3],
            heading: [1., 0., 0.],
            orientation: [0., 0., 0., 1.],
            angular_velocity: [0.; 3],
            mode: NavigationMode::Stop,
            updated_at_ms: now,
            warp: None,
        }),
        system_id: b.system_id,
        locked_asteroid: p.target_locked.then_some(b.target_id),
    };
    let asteroid = Asteroid {
        id: b.target_id,
        system_id: b.system_id,
        name: "Source-certified bulk-volume target (cm³)".into(),
        position: state.binding.target_center_mm.map(|v| v as f64 / 1000.),
        radius: f64::from(state.binding.target_radius_mm) / 1000.,
        ore: 0,
    };
    Ok((pilot, asteroid))
}

pub(in crate::physical_cargo) fn project_world(
    world: &mut World,
    initial_token_hash: Option<String>,
) -> Result<()> {
    let (view, asteroid) = actor_views(world)?;
    let token = initial_token_hash
        .or_else(|| world.pilots.get(&view.name).map(|p| p.token_hash.clone()))
        .context("Owner credential binding missing")?;
    world.pilots.clear();
    world.pilots.insert(
        view.name.clone(),
        crate::Pilot {
            view,
            token_hash: token,
            identity: None,
            last_mine_ms: None,
            warp: None,
            controls: None,
            walk_input: None,
        },
    );
    world.asteroids = vec![asteroid];
    world.belt_ore = 0;
    Ok(())
}

pub(in crate::physical_cargo) fn validate_world(world: &World) -> Result<()> {
    let Some(physical) = world.physical_cargo.as_ref() else {
        return Ok(());
    };
    let Some(control) = physical.control.as_ref() else {
        ensure!(
            world.pilots.is_empty(),
            "Inspection cannot contain player actors"
        );
        return Ok(());
    };
    ensure!(
        world.pilots.len() == 1
            && world.asteroids.len() == 1
            && world.belt_ore == 0
            && world.remote_controls.is_empty()
            && world.fleet_links.is_empty()
            && world.resource_jobs.is_empty()
            && world.surface_props.is_empty()
            && world.cargo_unloading.is_empty(),
        "Unregistered moving/economic actors in fixed source world"
    );
    let (expected, asteroid) = actor_views(world)?;
    let actual = world
        .pilots
        .get(&control.binding.pilot)
        .context("Owner actor missing")?;
    ensure!(
        actual.view == expected && world.asteroids[0] == asteroid,
        "Actual actors/source/volume policy differ from bound authority"
    );
    ensure!(
        actual.identity.is_none()
            && actual.warp.is_none()
            && actual.controls.is_none()
            && actual.walk_input.is_none()
            && actual.token_hash.len() == 64,
        "Unsupported owner/control state"
    );
    ensure!(
        physical.bay.as_ref().is_some_and(|b| b.world_power),
        "Real-power mode missing"
    );
    Ok(())
}

pub(super) fn response(
    world: &World,
    entries: Vec<LedgerEntry>,
    now: u64,
    export: bool,
) -> Result<Response> {
    validate_world(world)?;
    let physical = world
        .physical_cargo
        .as_ref()
        .context("No physical authority")?;
    let state = physical.control.clone().context("No world controls")?;
    let v = bay::view(physical.bay.clone().context("No bay")?)?;
    let manifest = &physical.genesis.design.manifest;
    let snapshot = Snapshot {
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
        server_time_ms: now,
        online_pilots: vec![],
        asteroids: world.asteroids.clone(),
        pilots: world.pilots.values().map(|p| p.view.clone()).collect(),
        orders: vec![],
        belt_ore: 0,
        ledger_sequence: entries.last().map_or(0, |e| e.sequence),
        ledger_hash: entries
            .last()
            .map_or(arkline_protocol::GENESIS_HASH, |e| e.hash.as_str())
            .into(),
    };
    Ok(Response {
        physical_cargo: Some(PhysicalControlPublicView {
            bay_state: Some(v.state),
            source_configuration_sha256: physical.genesis.source_configuration_sha256.clone(),
            state,
            ship_payload_cm3: physical.account.ship_payload_cm3,
            ship_capacity_cm3: manifest
                .ship_hold
                .capacity_cm3()
                .context("Ship cell capacity")?,
            drone_payload_cm3: physical.account.drones[0].payload_cm3,
            drone_capacity_cm3: manifest
                .mining_drone_hold
                .capacity_cm3()
                .context("Drone cell capacity")?,
            source_frame: v.source_frame,
            drone_inlet_travel_mm: v.drone_inlet_travel_mm,
        }),
        auth: None,
        assets: None,
        ok: true,
        message: "Source-bound single-bay controls accepted; other capabilities closed".into(),
        resume_token: None,
        snapshot: Some(snapshot.clone()),
        ledger: export.then_some(LedgerExport {
            protocol_version: PROTOCOL_VERSION,
            entries,
            snapshot,
        }),
    })
}
