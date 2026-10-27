//! Source-bound Pioneer drop-well kinematics in body metres. Admission must match
//! the actual cut hull receipt; old intact decks cannot use this motion path.
use crate::{PioneerDockedBox, PIONEER_MINING_TURRET_MOUNTS_M};
pub const DROP_M: f64 = 2.0;
pub const LOWER_START: f64 = 0.75;
pub const HATCH_CLOSE_START: f64 = 0.15;
pub const HATCH_TRAVEL_M: f64 = 1.30;
/// Deliberately false until the candidate hull and swept-clearance receipt are
/// adopted together. A legacy, uncut source never becomes an invisible recess.
pub const SOURCE_ADMITTED: bool = false;
pub const ADMITTED_ASSEMBLY_SHA256: &str = "UNADMITTED_PENDING_COMPOSED_SOURCE_PROOF";
pub fn carriage_fraction(fraction: f64) -> f64 {
    ((fraction - HATCH_CLOSE_START) / (LOWER_START - HATCH_CLOSE_START)).clamp(0., 1.)
}
pub fn drop_at(fraction: f64) -> f64 {
    (1. - carriage_fraction(fraction)) * DROP_M
}
pub fn hatch_fraction(fraction: f64) -> f64 {
    (fraction / HATCH_CLOSE_START).clamp(0., 1.)
}
pub fn aim_fraction(fraction: f64) -> f64 {
    ((fraction - LOWER_START) / (1. - LOWER_START)).clamp(0., 1.)
}
pub fn mount(slot: usize, fraction: f64) -> Option<[f64; 3]> {
    if !fraction.is_finite() || !(0. ..=1.).contains(&fraction) {
        return None;
    }
    let mut p = *PIONEER_MINING_TURRET_MOUNTS_M.get(slot)?;
    p[1] -= drop_at(fraction);
    Some(p)
}
pub fn hatch_box(slot: usize, side: f64, fraction: f64) -> Option<PioneerDockedBox> {
    let p = mount(slot, 1.)?;
    if side.abs() != 1. || !fraction.is_finite() || !(0. ..=1.).contains(&fraction) {
        return None;
    }
    Some(PioneerDockedBox {
        center_m: [
            12.85,
            9.08,
            p[2] + side * (0.56 + HATCH_TRAVEL_M * hatch_fraction(fraction)),
        ],
        half_m: [3.46, 0.05, 0.55],
        axes: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
    })
}
pub fn sweep(slot: usize, from: f64, to: f64) -> Option<PioneerDockedBox> {
    if !SOURCE_ADMITTED {
        return None;
    }
    candidate_sweep(slot, from, to)
}
/// Conservative full mechanism envelope includes both sliding shutters and all
/// permitted above-deck aiming poses. Static source clearance is proved offline;
/// this swept volume is reserved against transient bodies at runtime.
pub fn candidate_sweep(slot: usize, from: f64, to: f64) -> Option<PioneerDockedBox> {
    let p = mount(slot, from)?;
    mount(slot, to)?;
    Some(PioneerDockedBox {
        center_m: [12.8, 9.5, p[2]],
        half_m: [3.7, 2.5, 3.0],
        axes: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shutter_waits_for_carriage_and_aim_homes_before_lowering() {
        for i in 0..=1000 {
            let f = i as f64 / 1000.;
            if drop_at(f) > 1e-9 {
                assert_eq!(aim_fraction(f), 0.);
            }
            if hatch_fraction(f) < 1. - 1e-9 {
                assert_eq!(drop_at(f), DROP_M);
            }
        }
        assert!((mount(0, 0.).unwrap()[1] - 7.864).abs() < 1e-12);
    }
    #[test]
    fn invalid_source_and_parameters_fail_closed() {
        assert!(sweep(0, 1., 0.).is_none());
        assert!(candidate_sweep(2, 1., 0.).is_none());
        assert!(candidate_sweep(0, f64::NAN, 0.).is_none());
    }
}
