//! A bounded building sandbox on a spherical surface, without rigid body physics.
use super::*;
fn geometry(
    f: &FlightState,
    class: ShipClass,
    planet: &PlanetDefinition,
    envelope: Option<super::docking::Envelope>,
) -> Option<([f64; 3], f64, f64)> {
    let delta = std::array::from_fn::<_, 3, _>(|i| f.position[i] - planet.center[i]);
    let distance = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
    let normal = if distance > 1.0 {
        delta.map(|v| v / distance)
    } else {
        [0.0, 1.0, 0.0]
    };
    if let Some(envelope) = envelope {
        let support = envelope.support_m(
            arkline_protocol::drone_bay_geometry::BayPose {
                position_m: f.position,
                orientation_xyzw: f.orientation,
            },
            normal,
        )?;
        return Some((normal, distance - planet.radius_m, support));
    }
    let spec = class.spec();
    let half = [spec.length_m / 2.0, spec.height_m / 2.0, spec.width_m / 2.0];
    let support = (0..3)
        .map(|axis| {
            let mut basis = [0.0; 3];
            basis[axis] = 1.0;
            let rotated = flight::rotate(f.orientation, basis);
            rotated
                .iter()
                .zip(normal)
                .map(|(a, b)| a * b)
                .sum::<f64>()
                .abs()
                * half[axis]
        })
        .sum::<f64>();
    Some((normal, distance - planet.radius_m, support))
}
pub(super) fn constrain(
    f: &mut FlightState,
    class: ShipClass,
    system: u32,
    planet: &PlanetDefinition,
    envelope: Option<super::docking::Envelope>,
) {
    if system != planet.system_id {
        return;
    }
    let Some((normal, altitude, support)) = geometry(f, class, planet, envelope) else {
        f.velocity = [0.; 3];
        f.acceleration = [0.; 3];
        f.angular_velocity = [0.; 3];
        f.mode = NavigationMode::Stop;
        return;
    };
    if altitude < support {
        for (i, n) in normal.iter().enumerate() {
            f.position[i] += n * (support - altitude);
        }
        let inward = f
            .velocity
            .iter()
            .zip(normal)
            .map(|(a, b)| a * b)
            .sum::<f64>();
        if inward < 0.0 {
            for (i, n) in normal.iter().enumerate() {
                f.velocity[i] -= n * inward;
            }
        }
    }
}
pub(super) fn update(w: &mut World) {
    let planet = w.planet();
    let envelopes = super::docking::envelopes(w);
    for p in w.pilots.values_mut() {
        p.view.surface = if p.view.system_id == planet.system_id
            && matches!(p.view.location, Location::Station | Location::Belt)
        {
            p.view.flight.as_ref().and_then(|f| {
                let (_, altitude, support) =
                    geometry(f, p.view.ship_class, &planet, *envelopes.get(&p.view.name)?)?;
                (altitude <= planet.atmosphere_height_m).then_some(SurfaceState {
                    planet_id: planet.id,
                    altitude_m: altitude,
                    ground_clearance_m: (altitude - support).max(0.0),
                    grounded: altitude - support <= 0.1,
                })
            })
        } else {
            None
        };
    }
}
pub(super) fn handles(r: &Request) -> bool {
    matches!(
        r,
        Request::SpawnSurfaceProp { .. }
            | Request::MoveSurfaceProp { .. }
            | Request::RemoveSurfaceProp { .. }
    )
}
fn placement(
    position: [f64; 3],
    kind: SurfacePropKind,
    ship: [f64; 3],
    class: ShipClass,
    planet: &PlanetDefinition,
) -> Result<[f64; 3]> {
    ensure!(
        position.iter().all(|v| v.is_finite()),
        "prop position must be finite"
    );
    ensure!(
        hull_surface_distance(class, position, ship) <= 500.0,
        "surface edit range is 500 metres"
    );
    let delta = std::array::from_fn::<_, 3, _>(|i| position[i] - planet.center[i]);
    let length = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
    ensure!(length > 1.0, "invalid surface point");
    let result = std::array::from_fn(|i| {
        planet.center[i] + delta[i] / length * (planet.radius_m + kind.size()[1] / 2.0)
    });
    ensure!(
        hull_surface_distance(class, result, ship) <= 500.0,
        "projected prop is outside edit range"
    );
    Ok(result)
}
pub(super) fn execute(w: &mut World, name: &str, r: Request) -> Result<()> {
    let planet = w.planet();
    let p = w.pilots.get(name).context("pilot missing")?;
    let class = p.view.ship_class;
    ensure!(
        p.view
            .surface
            .as_ref()
            .is_some_and(|s| s.ground_clearance_m <= 100.0),
        "surface building requires ship within 100 metres of ground"
    );
    let ship = p
        .view
        .flight
        .as_ref()
        .context("flight unavailable")?
        .position;
    match r {
        Request::SpawnSurfaceProp { kind, position } => {
            ensure!(
                w.surface_props.len() < 256
                    && w.surface_props.iter().filter(|p| p.owner == name).count() < 32,
                "surface prop limit reached"
            );
            let position = placement(position, kind, ship, class, &planet)?;
            let id = w.next_prop_id.max(1);
            w.next_prop_id = id.checked_add(1).context("prop ID exhausted")?;
            w.surface_props.push(SurfaceProp {
                id,
                owner: name.into(),
                system_id: planet.system_id,
                kind,
                position,
            });
        }
        Request::MoveSurfaceProp { id, position } => {
            let prop = w
                .surface_props
                .iter_mut()
                .find(|p| p.id == id)
                .context("prop not found")?;
            ensure!(prop.owner == name, "only owner may edit prop");
            ensure!(
                hull_surface_distance(class, prop.position, ship) <= 500.0,
                "existing prop outside edit range"
            );
            prop.position = placement(position, prop.kind, ship, class, &planet)?;
        }
        Request::RemoveSurfaceProp { id } => {
            let prop = w
                .surface_props
                .iter()
                .find(|p| p.id == id)
                .context("prop not found")?;
            ensure!(prop.owner == name, "only owner may remove prop");
            ensure!(
                hull_surface_distance(class, prop.position, ship) <= 500.0,
                "existing prop outside edit range"
            );
            w.surface_props.retain(|p| p.id != id);
        }
        _ => unreachable!(),
    }
    Ok(())
}
pub(super) fn validate(w: &World) -> Result<()> {
    let planet = w.planet();
    ensure!(w.surface_props.len() <= 256, "too many surface props");
    for (i, p) in w.surface_props.iter().enumerate() {
        ensure!(
            p.id > 0 && p.id < w.next_prop_id && w.surface_props[..i].iter().all(|q| q.id != p.id),
            "invalid prop identity"
        );
        ensure!(
            w.pilots.contains_key(&p.owner) && p.system_id == planet.system_id,
            "invalid prop ownership or system"
        );
        ensure!(
            p.position.iter().all(|v| v.is_finite())
                && (planet.altitude(p.position) - p.kind.size()[1] / 2.0).abs() < 0.01,
            "invalid ground prop position"
        );
        ensure!(
            w.surface_props
                .iter()
                .filter(|q| q.owner == p.owner)
                .count()
                <= 32,
            "too many owned props"
        );
    }
    Ok(())
}

#[cfg(test)]
mod envelope_tests {
    use super::*;
    #[test]
    fn source_envelope_ground_support_retains_offset_roll_and_unknown_failure() {
        let mut reg = arkline_protocol::carrier_inventory_geometry::stationary_fixture_definition()
            .unwrap()
            .topology
            .registration
            .clone();
        reg.assembly_revision = 1703;
        reg.assembly_source_sha256 =
            "79ab43221861f74383ca36ca6a742ff5588e08003eb7b10f9d8b6f0d5469664a".into();
        let planet = PlanetDefinition {
            id: 1,
            system_id: 1,
            name: "test".into(),
            center: [0.; 3],
            radius_m: 1000.,
            atmosphere_height_m: 100.,
            approach_position: [0., 1100., 0.],
        };
        let mut f = FlightState {
            docking: None,
            acceleration: [0.; 3],
            position: [0., 1008., 0.],
            velocity: [0., -1., 0.],
            heading: [1., 0., 0.],
            orientation: [0., 0., 0., 1.],
            angular_velocity: [0.; 3],
            mode: NavigationMode::Stop,
            updated_at_ms: 0,
            warp: None,
        };
        assert!(
            (geometry(
                &f,
                ShipClass::Pioneer,
                &planet,
                super::super::docking::source_envelope(ShipClass::Pioneer, Some(&reg)).unwrap()
            )
            .unwrap()
            .2 - 9.001)
                .abs()
                < 1e-10
        );
        assert_eq!(
            geometry(&f, ShipClass::Pioneer, &planet, None).unwrap().2,
            ShipClass::Pioneer.spec().height_m / 2.
        );
        constrain(
            &mut f,
            ShipClass::Pioneer,
            1,
            &planet,
            super::super::docking::source_envelope(ShipClass::Pioneer, Some(&reg)).unwrap(),
        );
        assert!((f.position[1] - 1009.001).abs() < 1e-10);
        assert_eq!(f.velocity, [0.; 3]);
        f.orientation = [1., 0., 0., 0.];
        constrain(
            &mut f,
            ShipClass::Pioneer,
            1,
            &planet,
            super::super::docking::source_envelope(ShipClass::Pioneer, Some(&reg)).unwrap(),
        );
        assert!((f.position[1] - 1015.1568223095056).abs() < 1e-10);
        reg.assembly_revision = 9999;
        assert!(super::super::docking::source_envelope(ShipClass::Pioneer, Some(&reg)).is_err());
    }
}
