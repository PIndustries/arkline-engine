//! Exact1703 visual consumer; not moving-carrier admission. The current public
//! projection deliberately requires a stationary docking host, so it cannot
//! produce powered plumes. Keep that guard until authority admits travel; never
//! synthesize acceleration or relax projection checks to make a screenshot.
use super::*;
use crate::parallel_thruster_geometry::{
    reaction_outward, Outward, Plume, SourceGeometry, SOURCE_SHA256,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Unavailable {
    MissingCarrier,
    StaleDisconnectedOrPaused,
    UnsupportedSource,
    InvalidProjection,
    InstalledCatalogMismatch,
    MissingFlight,
    DockingGroundedOrWarp,
    InvalidFlightSample,
    /// Current stationary-only public admission necessarily reaches this gate.
    NoConfirmedThrustInAcceptedProjection,
    UnconfirmedPowerOrDrive,
    InvalidParent,
}

fn source_geometry(
    advertised: &arkline_protocol::CarrierInventoryRegistration,
    installed: &arkline_protocol::CarrierInventoryRegistration,
) -> Option<SourceGeometry> {
    if advertised != installed || advertised.assembly_revision != 1703 {
        return None;
    }
    SourceGeometry::for_verified_glb_sha256(&advertised.assembly_source_sha256)
}

fn fresh(sample: u64, snapshot: u64, now: u64) -> bool {
    sample <= snapshot && snapshot <= now && now - sample <= 500
}

fn normal_space_sample(
    location: Location,
    flight: &arkline_protocol::FlightState,
    grounded: bool,
    snapshot: u64,
    now: u64,
) -> Result<(), Unavailable> {
    if !matches!(location, Location::Station | Location::Belt)
        || flight.docking.as_ref().is_some_and(|d| {
            d.phase != arkline_protocol::DockingPhase::Free
                || d.clamps != 0.
                || d.segment.is_some()
                || d.hold_reason.is_some()
                || d.departing
        })
        || flight.warp.is_some()
        || grounded
    {
        return Err(Unavailable::DockingGroundedOrWarp);
    }
    if !fresh(flight.updated_at_ms, snapshot, now)
        || !flight.acceleration.iter().all(|v| v.is_finite())
    {
        return Err(Unavailable::InvalidFlightSample);
    }
    Ok(())
}

/// No fallback is returned: an advertised but unverified carrier must remain
/// dark rather than using buried legacy class-only engine/reaction anchors.
pub(super) fn for_pilot(frame: &Frame, pilot: &PilotView) -> Result<Vec<ModelPart>, Unavailable> {
    let public = pilot
        .carrier_inventory
        .as_ref()
        .ok_or(Unavailable::MissingCarrier)?;
    if !frame.presentation_live()
        || public.sample_time_ms != frame.snapshot_time
        || !fresh(public.sample_time_ms, frame.snapshot_time, frame.now)
    {
        return Err(Unavailable::StaleDisconnectedOrPaused);
    }
    let registration = &public.binding().registration;
    if registration.assembly_revision != 1703
        || registration.assembly_source_sha256 != SOURCE_SHA256
    {
        return Err(Unavailable::UnsupportedSource);
    }
    public
        .validate_projection(pilot)
        .map_err(|_| Unavailable::InvalidProjection)?;
    let catalog = flight_geometry().map_err(|_| Unavailable::InstalledCatalogMismatch)?;
    let installed = catalog
        .carriers
        .iter()
        .filter_map(|c| c.as_ref().ok())
        .find(|c| c.registration == *registration)
        .ok_or(Unavailable::InstalledCatalogMismatch)?;
    let source = source_geometry(registration, &installed.registration)
        .ok_or(Unavailable::InstalledCatalogMismatch)?;
    let flight = pilot.flight.as_ref().ok_or(Unavailable::MissingFlight)?;
    normal_space_sample(
        pilot.location.clone(),
        flight,
        pilot.surface.as_ref().is_some_and(|s| s.grounded),
        frame.snapshot_time,
        frame.now,
    )?;
    if flight.acceleration == [0.; 3] {
        return Err(Unavailable::NoConfirmedThrustInAcceptedProjection);
    }
    // Sanity/freshness only: this capacitor report is NOT a metered propulsion
    // circuit or a force/energy estimate. Do not use nav enabled/powered flags.
    let power = pilot
        .power
        .as_ref()
        .ok_or(Unavailable::UnconfirmedPowerOrDrive)?;
    if !fresh(power.updated_at_ms, frame.snapshot_time, frame.now)
        || power.capacity_j == 0
        || power.charge_j > power.capacity_j
        || power.regen_remainder_mj >= 1000
    {
        return Err(Unavailable::UnconfirmedPowerOrDrive);
    }
    let world = frame
        .exterior
        .as_deref()
        .ok_or(Unavailable::UnconfirmedPowerOrDrive)?;
    let mut vessels = world
        .vessels
        .iter()
        .filter(|v| v.vessel_id == public.binding().vessel_id);
    let vessel = vessels.next().ok_or(Unavailable::UnconfirmedPowerOrDrive)?;
    if vessels.next().is_some()
        || vessel.pilot != pilot.name
        || vessel.ship_class != pilot.ship_class
        || vessel.drive != arkline_protocol::ExteriorDriveState::Active
    {
        return Err(Unavailable::UnconfirmedPowerOrDrive);
    }
    // Confirmed parent, not heading-only or extrapolated orientation. Do not
    // apply source attitude twice or scale the ship by legacy class dimensions.
    let orientation =
        Quat::from_array(public.parent.orientation_xyzw.map(|v| v as f32)).normalize();
    let acceleration =
        orientation.inverse() * Vec3::from_array(flight.acceleration.map(|v| v as f32));
    if !acceleration.is_finite() {
        return Err(Unavailable::InvalidFlightSample);
    }
    let parent = frame.ship_mount(pilot).ok_or(Unavailable::InvalidParent)?;
    if !parent.is_finite() {
        return Err(Unavailable::InvalidParent);
    }
    let pulse = 1.0
        + 0.10
            * ((frame.now % 260) as f32 / 260.0 * std::f32::consts::TAU + lane(&pilot.name)).sin();
    Ok(draws(&source, parent, acceleration, pulse))
}

fn plume_model(parent: Mat4, plume: Plume, radius_layer: f32) -> Mat4 {
    let direction: Outward = plume.outward;
    // Every body-metre value is converted exactly once; parent is already scene
    // space from the confirmed world pose, without class scale or extrapolation.
    parent
        * transform(
            Vec3::from_array(plume.center_m.map(|v| (v / 100.) as f32)),
            Vec3::new(
                (plume.half_length_m / 100.) as f32,
                (plume.radius_m / 100.) as f32 * radius_layer,
                (plume.radius_m / 100.) as f32 * radius_layer,
            ),
            Quat::from_rotation_arc(
                Vec3::X,
                Vec3::from_array(direction.direction().map(|v| v as f32)),
            ),
        )
}

fn draws(source: &SourceGeometry, parent: Mat4, acceleration: Vec3, pulse: f32) -> Vec<ModelPart> {
    if !acceleration.is_finite() || !parent.is_finite() || !pulse.is_finite() {
        return Vec::new();
    }
    let mut result = Vec::new();
    let thrust = (acceleration.x.max(0.) / arkline_protocol::SHIP_ACCELERATION as f32)
        .clamp(0., 1.)
        .sqrt();
    if thrust * 1.8 > 0.001 {
        for (width, brightness) in [(0.58, 0.10), (0.31, 0.62), (0.16, 1.0)] {
            for plume in source.main(
                f64::from(thrust) * 4. * 72. / 7.,
                f64::from(pulse),
                width / 0.58,
            ) {
                result.push((
                    2,
                    plume_model(parent, plume, 1.),
                    Vec3::new(0.16, 0.48, 0.95) * brightness,
                    thrust * 1.8,
                ));
            }
        }
    }
    for axis in 0..3 {
        let component = acceleration[axis];
        let Some(outward) = reaction_outward(axis, f64::from(component)) else {
            continue;
        };
        let strength = (component.abs() / arkline_protocol::SHIP_ACCELERATION as f32)
            .clamp(0., 1.)
            .sqrt();
        for plume in source.reaction(outward, f64::from(strength)) {
            for (width, brightness) in [(1., 0.13), (0.40, 0.85)] {
                result.push((
                    2,
                    plume_model(parent, plume, width),
                    Vec3::new(0.22, 0.58, 1.) * brightness,
                    2.2 * strength,
                ));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_installed_registration_not_revision_or_class_selects_effects() {
        let installed = carrier_magazine_source::Source::registration(1703).unwrap();
        assert!(source_geometry(&installed, &installed).is_some());
        for revision in [1701, 1702] {
            let old = carrier_magazine_source::Source::registration(revision).unwrap();
            assert!(source_geometry(&old, &old).is_none());
            assert!(source_geometry(&installed, &old).is_none());
        }
        let mut wrong = installed.clone();
        wrong.assembly_source_sha256 = "0".repeat(64);
        assert!(source_geometry(&wrong, &installed).is_none());
        wrong = installed.clone();
        wrong.assembly_revision = 9999;
        assert!(source_geometry(&wrong, &installed).is_none());
        assert!(!fresh(1001, 1000, 1000));
        assert!(!fresh(1000, 1000, 999));
        assert!(!fresh(499, 1000, 1000));
        assert!(fresh(500, 1000, 1000));
    }

    #[test]
    fn plume_tip_parent_rotation_and_metre_scale_apply_once_all_axes() {
        let source = SourceGeometry::for_verified_glb_sha256(SOURCE_SHA256).unwrap();
        let parent = Mat4::from_rotation_translation(
            Quat::from_rotation_z(0.73) * Quat::from_rotation_y(-0.42),
            Vec3::new(12., -4., 8.),
        );
        for pulse in [0.9, 1., 1.1] {
            for length in [1e-8, 4., 41.] {
                for plume in source.main(length, pulse, 1.) {
                    let matrix = plume_model(parent, plume, 1.);
                    let tip = matrix.transform_point3(-Vec3::X);
                    let expected = parent
                        .transform_point3(Vec3::from_array(plume.tip_m.map(|v| (v / 100.) as f32)));
                    assert!((tip - expected).length() < 3e-6);
                    assert!((plume.tip_m[0] + 35.9).abs() < 1e-10);
                    let end = matrix.transform_point3(Vec3::X);
                    assert!(
                        ((end - tip).length() - plume.half_length_m as f32 * 2. / 100.).abs()
                            < 3e-6
                    );
                }
            }
        }
        for outward in [
            Outward::Forward,
            Outward::Up,
            Outward::Down,
            Outward::Port,
            Outward::Starboard,
        ] {
            for plume in source.reaction(outward, 1.) {
                for layer in [1., 0.4] {
                    let matrix = plume_model(parent, plume, layer);
                    let expected = parent
                        .transform_point3(Vec3::from_array(plume.tip_m.map(|v| (v / 100.) as f32)));
                    assert!((matrix.transform_point3(-Vec3::X) - expected).length() < 3e-6);
                    assert!(
                        (matrix.transform_vector3(Vec3::Y).length()
                            - (plume.radius_m / 100.) as f32 * layer)
                            .abs()
                            < 1e-6
                    );
                }
            }
        }
    }

    #[test]
    fn only_finite_confirmed_acceleration_generates_normal_space_plumes() {
        let source = SourceGeometry::for_verified_glb_sha256(SOURCE_SHA256).unwrap();
        assert!(draws(&source, Mat4::IDENTITY, Vec3::ZERO, 1.).is_empty());
        assert!(draws(&source, Mat4::IDENTITY, Vec3::splat(f32::NAN), 1.).is_empty());
        assert!(draws(&source, Mat4::IDENTITY, Vec3::X, f32::NAN).is_empty());
        let main = draws(
            &source,
            Mat4::IDENTITY,
            Vec3::X * arkline_protocol::SHIP_ACCELERATION as f32,
            1.,
        );
        assert_eq!(main.len(), 6);
        let braking = draws(
            &source,
            Mat4::IDENTITY,
            -Vec3::X * arkline_protocol::SHIP_ACCELERATION as f32,
            1.,
        );
        assert_eq!(braking.len(), 2);
        for (_, model, _, _) in braking {
            assert!(model.transform_point3(-Vec3::X).x > 0.35);
        }
        // Rotation uses the accepted parent, not a second extrapolated attitude.
        let attitude = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let local = attitude.inverse() * (attitude * Vec3::X * 5.);
        assert!((local - Vec3::X * 5.).length() < 1e-5);
    }
    #[test]
    fn normal_space_sample_rejects_stale_future_ground_dock_and_jump() {
        let mut flight: arkline_protocol::FlightState = serde_json::from_value(serde_json::json!({
            "position":[0.,0.,0.],"velocity":[0.,0.,0.],"heading":[1.,0.,0.],
            "orientation":[0.,0.,0.,1.],"angular_velocity":[0.,0.,0.],
            "acceleration":[1.,0.,0.],"mode":{"mode":"stop"},"updated_at_ms":1000
        }))
        .unwrap();
        assert!(normal_space_sample(Location::Station, &flight, false, 1000, 1000).is_ok());
        assert_eq!(
            normal_space_sample(Location::Docked, &flight, false, 1000, 1000),
            Err(Unavailable::DockingGroundedOrWarp)
        );
        assert_eq!(
            normal_space_sample(Location::Belt, &flight, true, 1000, 1000),
            Err(Unavailable::DockingGroundedOrWarp)
        );
        assert_eq!(
            normal_space_sample(
                Location::Jumping {
                    gate_id: 1,
                    arrives_at_ms: 2000
                },
                &flight,
                false,
                1000,
                1000
            ),
            Err(Unavailable::DockingGroundedOrWarp)
        );
        assert_eq!(
            normal_space_sample(Location::Station, &flight, false, 1000, 1501),
            Err(Unavailable::InvalidFlightSample)
        );
        flight.updated_at_ms = 1001;
        assert_eq!(
            normal_space_sample(Location::Station, &flight, false, 1000, 1000),
            Err(Unavailable::InvalidFlightSample)
        );
        flight.updated_at_ms = 1000;
        flight.docking = Some(arkline_protocol::DockingState {
            version: arkline_protocol::DOCKING_VERSION,
            host_vessel_id: "test".into(),
            berth_id: 0,
            operation_id: 1,
            phase: arkline_protocol::DockingPhase::Free,
            segment: None,
            clamps: 0.,
            updated_at_ms: 1000,
            hold_reason: None,
            departing: false,
        });
        assert!(normal_space_sample(Location::Station, &flight, false, 1000, 1000).is_ok());
        for phase in [
            arkline_protocol::DockingPhase::Approaching,
            arkline_protocol::DockingPhase::Aligning,
            arkline_protocol::DockingPhase::Berthing,
            arkline_protocol::DockingPhase::Clamping,
            arkline_protocol::DockingPhase::Secured,
            arkline_protocol::DockingPhase::Releasing,
            arkline_protocol::DockingPhase::Departing,
            arkline_protocol::DockingPhase::Braking,
            arkline_protocol::DockingPhase::Held,
        ] {
            flight.docking.as_mut().unwrap().phase = phase;
            assert_eq!(
                normal_space_sample(Location::Station, &flight, false, 1000, 1000),
                Err(Unavailable::DockingGroundedOrWarp)
            );
        }
        flight.docking = None;
        flight.acceleration[1] = f64::NAN;
        assert_eq!(
            normal_space_sample(Location::Station, &flight, false, 1000, 1000),
            Err(Unavailable::InvalidFlightSample)
        );
    }
}
