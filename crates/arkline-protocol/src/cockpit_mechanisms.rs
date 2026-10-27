//! Experimental Pioneer physical display actuators. Fractions describe actual
//! hardware travel; no motor power, equipment mass or hydraulic model is implied.
use serde::{Deserialize, Serialize};
/// Existing wire arrays retain seven entries for old snapshot compatibility.
pub const COCKPIT_MECHANISM_SLOTS: usize = 7;
pub const COCKPIT_EXTERNAL_SCREEN_SLOTS: usize = 2;
pub const COCKPIT_TOTAL_MECHANISM_SLOTS: usize = 9;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExteriorCameraSide {
    Port,
    Starboard,
}
impl ExteriorCameraSide {
    pub const fn hardware_slot(self) -> usize {
        match self {
            Self::Port => 7,
            Self::Starboard => 8,
        }
    }
    /// Component identity is scoped by PioneerCockpitMechanisms::vessel_id.
    pub const fn component_id(self) -> &'static str {
        match self {
            Self::Port => "pioneer.camera.exterior.port",
            Self::Starboard => "pioneer.camera.exterior.starboard",
        }
    }
    pub const fn from_hardware_slot(slot: usize) -> Option<Self> {
        match slot {
            7 => Some(Self::Port),
            8 => Some(Self::Starboard),
            _ => None,
        }
    }
}
fn default_external_screens() -> [CockpitScreenActuator; 2] {
    std::array::from_fn(|_| CockpitScreenActuator::stowed(0))
}
pub const COCKPIT_SCREEN_TRAVEL_MS: u64 = 1_200;
/// Owner-selected monitor policy; it never changes the underlying equipment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CockpitScreenMode {
    #[default]
    Auto,
    Open,
    Stowed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CockpitScreenTarget {
    Slot {
        slot: u8,
        expected_assignment_generation: Option<u64>,
    },
    All {
        expected_assignment_generations: [u64; 5],
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CockpitScreenActuator {
    /// 0 is stowed, 1 deployed. Interpolation is authorized only after sweep clearance.
    pub from_fraction: f64,
    pub target_deployed: bool,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    pub blocked: Option<String>,
}
impl CockpitScreenActuator {
    pub fn fraction_at(&self, now: u64) -> f64 {
        if self.blocked.is_some() {
            return self.from_fraction;
        }
        let target = if self.target_deployed { 1.0 } else { 0.0 };
        let t = if self.duration_ms == 0 {
            1.0
        } else {
            now.saturating_sub(self.started_at_ms) as f64 / self.duration_ms as f64
        }
        .clamp(0.0, 1.0);
        self.from_fraction + (target - self.from_fraction) * t
    }
    pub fn moving_at(&self, now: u64) -> bool {
        self.blocked.is_none()
            && (self.fraction_at(now) - f64::from(self.target_deployed)).abs() > 1e-9
    }
    pub fn stowed(now: u64) -> Self {
        Self {
            from_fraction: 0.0,
            target_deployed: false,
            started_at_ms: now,
            duration_ms: 0,
            blocked: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PioneerCockpitMechanisms {
    /// Physical guns, independent of monitor policy. Old saves remain raised.
    #[serde(
        default,
        skip_serializing_if = "crate::TurretDeployment::is_legacy_default"
    )]
    pub turrets: crate::TurretDeployment,
    pub vessel_id: String,
    /// Stable hardware slots 0..4; slot reassignment increments its generation.
    pub drone_ids: [Option<u64>; 5],
    pub assignment_generations: [u64; 5],
    /// The current hull can report more than five drones; never silently erase them.
    pub unassigned_drone_ids: Vec<u64>,
    /// Slots 0..4 lower from ceiling; slots 5..6 rise from console.
    pub screens: [CockpitScreenActuator; COCKPIT_MECHANISM_SLOTS],
    #[serde(default)]
    pub modes: [CockpitScreenMode; COCKPIT_MECHANISM_SLOTS],
    /// Appended hardware slots 7 port / 8 starboard; not drone assignments.
    #[serde(default = "default_external_screens")]
    pub external_screens: [CockpitScreenActuator; COCKPIT_EXTERNAL_SCREEN_SLOTS],
    #[serde(default)]
    pub external_modes: [CockpitScreenMode; COCKPIT_EXTERNAL_SCREEN_SLOTS],
    /// Retiring source identity must stow before reassignment, even in Open mode.
    #[serde(default)]
    pub retiring_drone_slots: [bool; 5],
    pub warp_stowed: bool,
    pub updated_at_ms: u64,
}
impl PioneerCockpitMechanisms {
    pub fn stowed(vessel_id: String, now: u64) -> Self {
        Self {
            turrets: crate::TurretDeployment::default(),
            vessel_id,
            drone_ids: [None; 5],
            assignment_generations: [0; 5],
            unassigned_drone_ids: Vec::new(),
            screens: std::array::from_fn(|_| CockpitScreenActuator::stowed(now)),
            modes: [CockpitScreenMode::Auto; COCKPIT_MECHANISM_SLOTS],
            external_screens: std::array::from_fn(|_| CockpitScreenActuator::stowed(now)),
            external_modes: [CockpitScreenMode::Auto; COCKPIT_EXTERNAL_SCREEN_SLOTS],
            retiring_drone_slots: [false; 5],
            warp_stowed: false,
            updated_at_ms: now,
        }
    }
    pub fn screen(&self, slot: usize) -> Option<&CockpitScreenActuator> {
        if slot < COCKPIT_MECHANISM_SLOTS {
            self.screens.get(slot)
        } else {
            self.external_screens
                .get(slot.checked_sub(COCKPIT_MECHANISM_SLOTS)?)
        }
    }
    pub fn screen_mut(&mut self, slot: usize) -> Option<&mut CockpitScreenActuator> {
        if slot < COCKPIT_MECHANISM_SLOTS {
            self.screens.get_mut(slot)
        } else {
            self.external_screens
                .get_mut(slot.checked_sub(COCKPIT_MECHANISM_SLOTS)?)
        }
    }
    pub fn mode(&self, slot: usize) -> Option<CockpitScreenMode> {
        if slot < COCKPIT_MECHANISM_SLOTS {
            self.modes.get(slot).copied()
        } else {
            self.external_modes
                .get(slot.checked_sub(COCKPIT_MECHANISM_SLOTS)?)
                .copied()
        }
    }
    pub fn mode_mut(&mut self, slot: usize) -> Option<&mut CockpitScreenMode> {
        if slot < COCKPIT_MECHANISM_SLOTS {
            self.modes.get_mut(slot)
        } else {
            self.external_modes
                .get_mut(slot.checked_sub(COCKPIT_MECHANISM_SLOTS)?)
        }
    }
    pub fn iter_screens(&self) -> impl Iterator<Item = &CockpitScreenActuator> {
        self.screens.iter().chain(&self.external_screens)
    }
    pub fn iter_screens_mut(&mut self) -> impl Iterator<Item = &mut CockpitScreenActuator> {
        self.screens.iter_mut().chain(&mut self.external_screens)
    }
    pub fn all_fractions_at(&self, now: u64) -> [f64; COCKPIT_TOTAL_MECHANISM_SLOTS] {
        std::array::from_fn(|slot| self.screen(slot).unwrap().fraction_at(now))
    }
    pub fn fractions_at(&self, now: u64) -> [f64; COCKPIT_MECHANISM_SLOTS] {
        std::array::from_fn(|slot| self.screens[slot].fraction_at(now))
    }
}

#[cfg(test)]
mod captain_tests {
    use super::*;
    #[test]
    fn older_mechanism_payload_defaults_to_auto_without_retirement() {
        let original = PioneerCockpitMechanisms::stowed("owned".into(), 1000);
        let mut value = serde_json::to_value(&original).unwrap();
        value.as_object_mut().unwrap().remove("modes");
        value
            .as_object_mut()
            .unwrap()
            .remove("retiring_drone_slots");
        let restored: PioneerCockpitMechanisms = serde_json::from_value(value).unwrap();
        assert_eq!(restored, original);
    }
    #[test]
    fn captain_mode_request_roundtrips_but_is_not_remote_gameplay() {
        let request = crate::Request::SetCockpitScreenMode {
            vessel_id: "owned".into(),
            target: CockpitScreenTarget::All {
                expected_assignment_generations: [1, 2, 3, 4, 5],
            },
            mode: CockpitScreenMode::Stowed,
        };
        assert!(!request.is_remote_gameplay());
        let json = serde_json::to_value(&request).unwrap();
        let decoded: crate::Request = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), json);
    }

    #[test]
    fn legacy_seven_arrays_admit_absent_external_pair_without_reindexing() {
        let original = PioneerCockpitMechanisms::stowed("owned".into(), 1000);
        let mut value = serde_json::to_value(&original).unwrap();
        value.as_object_mut().unwrap().remove("external_screens");
        value.as_object_mut().unwrap().remove("external_modes");
        let restored: PioneerCockpitMechanisms = serde_json::from_value(value).unwrap();
        assert_eq!(restored.screens, original.screens);
        assert_eq!(restored.modes, original.modes);
        assert_eq!(restored.external_modes, [CockpitScreenMode::Auto; 2]);
        assert_eq!(restored.all_fractions_at(u64::MAX), [0.; 9]);
        assert_eq!(restored.fractions_at(1000), [0.; 7]);
    }
    #[test]
    fn nine_slot_accessors_preserve_legacy_hardware_and_external_identity() {
        let mut state = PioneerCockpitMechanisms::stowed("owned".into(), 1000);
        for slot in 0..COCKPIT_TOTAL_MECHANISM_SLOTS {
            *state.mode_mut(slot).unwrap() = CockpitScreenMode::Open;
            state.screen_mut(slot).unwrap().target_deployed = true;
            assert_eq!(state.mode(slot), Some(CockpitScreenMode::Open));
            assert_eq!(state.screen(slot).unwrap().fraction_at(1000), 1.);
        }
        assert_eq!(state.all_fractions_at(1000), [1.; 9]);
        assert_eq!(state.iter_screens().count(), 9);
        assert!(state.screen(9).is_none());
        assert!(state.mode(usize::MAX).is_none());
        assert_eq!(ExteriorCameraSide::Port.hardware_slot(), 7);
        assert_eq!(ExteriorCameraSide::Starboard.hardware_slot(), 8);
        assert_ne!(
            ExteriorCameraSide::Port.component_id(),
            ExteriorCameraSide::Starboard.component_id()
        );
        assert!(ExteriorCameraSide::from_hardware_slot(0).is_none());
    }
}

#[cfg(test)]
mod turret_legacy_wire_tests {
    use super::*;
    #[test]
    fn old_cockpit_json_roundtrips_without_appending_turret_fields() {
        let mut old = serde_json::to_value(PioneerCockpitMechanisms::stowed(
            "legacy-vessel".into(),
            123,
        ))
        .unwrap();
        old.as_object_mut().unwrap().remove("turrets");
        let bytes = serde_json::to_vec(&old).unwrap();
        let decoded: PioneerCockpitMechanisms = serde_json::from_slice(&bytes).unwrap();
        assert!(decoded.turrets.is_legacy_default());
        assert_eq!(
            serde_json::to_vec(&serde_json::to_value(&decoded).unwrap()).unwrap(),
            bytes
        );
        let mut source = decoded.clone();
        source.turrets.source_admitted = true;
        assert!(serde_json::to_value(source)
            .unwrap()
            .get("turrets")
            .is_some());
        let mut nondefault = decoded;
        nondefault.turrets.status = "retained nondefault".into();
        assert!(serde_json::to_value(nondefault)
            .unwrap()
            .get("turrets")
            .is_some());
    }
}
