//! Exact source/owner/sample binding for physical turret telemetry. This admits
//! only the proved well kinematics, never normal carrier travel or firing.
use crate::{
    carrier_inventory_geometry, CarrierInventoryRegistration, PilotView, ShipClass,
    TurretDeployment,
};
pub const SOURCE_SHA256: &str = "79ab43221861f74383ca36ca6a742ff5588e08003eb7b10f9d8b6f0d5469664a";
pub fn exact_registration(registration: &CarrierInventoryRegistration) -> bool {
    registration.assembly_revision == 1703
        && registration.assembly_source_sha256 == SOURCE_SHA256
        && carrier_inventory_geometry::stationary_fixture_for_registration(registration).is_some()
}
pub fn confirmed_state(pilot: &PilotView, sample_ms: u64) -> Result<&TurretDeployment, String> {
    let public = pilot
        .carrier_inventory
        .as_ref()
        .ok_or("turret carrier projection missing")?;
    if pilot.ship_class != ShipClass::Pioneer
        || public.sample_time_ms != sample_ms
        || !exact_registration(&public.binding().registration)
    {
        return Err("turret source/sample binding mismatch".into());
    }
    public.validate_projection(pilot)?;
    let mechanisms = pilot
        .cockpit_mechanisms
        .as_ref()
        .ok_or("turret telemetry missing")?;
    if mechanisms.vessel_id != public.binding().vessel_id || !mechanisms.turrets.source_admitted {
        return Err("physical turret owner/source unavailable".into());
    }
    if mechanisms.turrets.updated_at_ms != sample_ms {
        return Err("turret pose clock is not the accepted carrier sample".into());
    }
    mechanisms.turrets.validate_at(sample_ms)?;
    Ok(&mechanisms.turrets)
}

/// Conservative full rigid aim/drop/shutter union from the independently
/// reviewed bank-v7-turret-domains.json, not the old disabled candidate box.
/// This source-qualified geometry lookup is NOT normal travel admission.
pub fn sweep(
    registration: &CarrierInventoryRegistration,
    slot: usize,
    from: f64,
    to: f64,
) -> Option<crate::PioneerDockedBox> {
    if !exact_registration(registration) {
        return None;
    }
    crate::turret_stow_geometry::mount(slot, from)?;
    crate::turret_stow_geometry::mount(slot, to)?;
    let (lo, hi) = match slot {
        0 => (
            [5.5071776904944185, 2.5711776904944186, -10.492822309505582],
            [16.31, 15.156822309505582, 0.09282230950558201],
        ),
        1 => (
            [5.5071776904944185, 2.5711776904944186, -0.09282230950558201],
            [16.31, 15.156822309505582, 10.492822309505582],
        ),
        _ => return None,
    };
    Some(crate::PioneerDockedBox {
        center_m: std::array::from_fn(|i| (lo[i] + hi[i]) * 0.5),
        half_m: std::array::from_fn(|i| (hi[i] - lo[i]) * 0.5),
        axes: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_reviewed_union_is_exact_source_only_and_old_admission_stays_off() {
        let mut registration = carrier_inventory_geometry::stationary_fixture_definition()
            .unwrap()
            .topology
            .registration
            .clone();
        assert!(sweep(&registration, 0, 1., 0.).is_none());
        registration.assembly_revision = 1703;
        registration.assembly_source_sha256 = SOURCE_SHA256.into();
        for slot in 0..2 {
            let b = sweep(&registration, slot, 1., 0.).unwrap();
            assert!(b.half_m[0] > 5.4 && b.half_m[1] > 6.2 && b.half_m[2] > 5.2);
        }
        assert!(sweep(&registration, 2, 1., 0.).is_none());
        assert!(sweep(&registration, 0, f64::NAN, 0.).is_none());
        assert!(crate::turret_stow_geometry::sweep(0, 1., 0.).is_none());
    }
}
