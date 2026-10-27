//! Small source-mounted amber beacon presentation. Actual door/loader state is
//! supplied by authority; this never starts jobs or invents machinery progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WarningMode {
    Secured,
    Operating,
    HeldFault,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WarningVisual {
    pub mode: WarningMode,
    pub emission: f32,
    /// Rotating light direction only; rigid hardware has no motor/rotation root.
    pub rotating_light_phase_radians: f64,
    pub color: [f32; 3],
}
pub(crate) struct Definition<'a> {
    pub device_id: &'a str,
    pub source_sha256: &'a str,
    pub revision: u32,
    pub parent_id: &'a str,
}
pub(crate) struct ConfirmedWarning<'a> {
    pub vessel_id: &'a str,
    pub device_id: &'a str,
    pub source_sha256: &'a str,
    pub revision: u32,
    pub parent_id: &'a str,
    pub sampled_at_ms: u64,
    /// Existing actual bus availability, explicitly unmetered. No watt/debit claim.
    pub supply_available: bool,
    pub door_fraction: f64,
    pub door_moving: bool,
    pub loader_active: bool,
    pub held_fault: bool,
}
/// Hardware geometry remains visible when dark. Only this small amber emitter
/// pulses; never flash the captain screen or global/cockpit lighting.
pub(crate) fn visual(
    definition: &Definition<'_>,
    state: &ConfirmedWarning<'_>,
    expected_vessel_id: &str,
    now: u64,
    connected: bool,
) -> Option<WarningVisual> {
    if !connected
        || !state.supply_available
        || state.vessel_id != expected_vessel_id
        || state.device_id != definition.device_id
        || state.source_sha256 != definition.source_sha256
        || state.revision != definition.revision
        || state.parent_id != definition.parent_id
        || state.sampled_at_ms > now
        || now - state.sampled_at_ms > 1500
        || !state.door_fraction.is_finite()
        || !(0.0..=1.0).contains(&state.door_fraction)
    {
        return None;
    }
    let mode = if state.held_fault {
        WarningMode::HeldFault
    } else if state.door_moving || state.loader_active || state.door_fraction > 1e-6 {
        WarningMode::Operating
    } else {
        WarningMode::Secured
    };
    let pulse = now % 1000 < 300;
    let emission = match mode {
        WarningMode::Secured => 0.,
        WarningMode::Operating => {
            if pulse {
                2.
            } else {
                0.15
            }
        }
        WarningMode::HeldFault => {
            if pulse {
                2.
            } else {
                0.7
            }
        }
    };
    Some(WarningVisual {
        mode,
        emission,
        color: [1., 0.42, 0.03],
        rotating_light_phase_radians: if mode == WarningMode::Operating {
            (now % 1600) as f64 / 1600. * std::f64::consts::TAU
        } else {
            0.
        },
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn definition() -> Definition<'static> {
        Definition {
            device_id: "pioneer.carrier.warning.loader.port.0",
            source_sha256: "test-fixture-only",
            revision: 1,
            parent_id: "cell00/loader/trolley",
        }
    }
    fn state() -> ConfirmedWarning<'static> {
        ConfirmedWarning {
            vessel_id: "owned",
            device_id: definition().device_id,
            source_sha256: definition().source_sha256,
            revision: 1,
            parent_id: definition().parent_id,
            sampled_at_ms: 1000,
            supply_available: true,
            door_fraction: 0.,
            door_moving: false,
            loader_active: false,
            held_fault: false,
        }
    }
    #[test]
    fn actual_motion_or_open_door_drives_warning_not_elapsed_job_timer() {
        let d = definition();
        let mut s = state();
        assert_eq!(
            visual(&d, &s, "owned", 1000, true).unwrap().mode,
            WarningMode::Secured
        );
        s.door_moving = true;
        assert_eq!(
            visual(&d, &s, "owned", 1000, true).unwrap().mode,
            WarningMode::Operating
        );
        s.door_moving = false;
        s.door_fraction = 0.7;
        assert_eq!(
            visual(&d, &s, "owned", 1000, true).unwrap().mode,
            WarningMode::Operating
        );
        s.door_fraction = 0.;
        s.loader_active = true;
        assert_eq!(
            visual(&d, &s, "owned", 1000, true).unwrap().mode,
            WarningMode::Operating
        );
        s.held_fault = true;
        assert_eq!(
            visual(&d, &s, "owned", 1000, true).unwrap().mode,
            WarningMode::HeldFault
        );
        s.held_fault = false;
        s.loader_active = false;
        assert_eq!(visual(&d, &s, "owned", 1200, true).unwrap().emission, 0.);
    }
    #[test]
    fn missing_power_stale_wrong_source_parent_or_vessel_cannot_flash() {
        let d = definition();
        for case in 0..8 {
            let mut s = state();
            match case {
                0 => s.supply_available = false,
                1 => s.vessel_id = "other",
                2 => s.source_sha256 = "other",
                3 => s.parent_id = "obsolete.shared.gantry",
                4 => s.revision = 2,
                5 => s.door_fraction = f64::NAN,
                6 => s.sampled_at_ms = 3000,
                _ => s.device_id = "other",
            };
            assert!(visual(&d, &s, "owned", 1000, true).is_none());
        }
        assert!(visual(&d, &state(), "owned", 2501, true).is_none());
        assert!(visual(&d, &state(), "owned", 1000, false).is_none());
    }
    #[test]
    fn repeat_is_deterministic_rotation_and_flash_are_simulation_clock_only() {
        let d = definition();
        let mut s = state();
        s.loader_active = true;
        assert_eq!(
            visual(&d, &s, "owned", 1100, true),
            visual(&d, &s, "owned", 1100, true)
        );
        assert_ne!(
            visual(&d, &s, "owned", 1100, true)
                .unwrap()
                .rotating_light_phase_radians,
            visual(&d, &s, "owned", 1400, true)
                .unwrap()
                .rotating_light_phase_radians
        );
        assert!(
            visual(&d, &s, "owned", 1100, true).unwrap().emission
                > visual(&d, &s, "owned", 1400, true).unwrap().emission
        );
    }
}
