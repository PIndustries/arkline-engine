//! Experimental Pioneer physical display actuators. Fractions describe actual
//! hardware travel; no motor power, equipment mass or hydraulic model is implied.
use serde::{Deserialize, Serialize};
pub const COCKPIT_MECHANISM_SLOTS: usize = 7;
pub const COCKPIT_SCREEN_TRAVEL_MS: u64 = 1_200;
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
    pub vessel_id: String,
    /// Stable hardware slots 0..4; slot reassignment increments its generation.
    pub drone_ids: [Option<u64>; 5],
    pub assignment_generations: [u64; 5],
    /// The current hull can report more than five drones; never silently erase them.
    pub unassigned_drone_ids: Vec<u64>,
    /// Slots 0..4 lower from ceiling; slots 5..6 rise from console.
    pub screens: [CockpitScreenActuator; COCKPIT_MECHANISM_SLOTS],
    pub warp_stowed: bool,
    pub updated_at_ms: u64,
}
impl PioneerCockpitMechanisms {
    pub fn stowed(vessel_id: String, now: u64) -> Self {
        Self {
            vessel_id,
            drone_ids: [None; 5],
            assignment_generations: [0; 5],
            unassigned_drone_ids: Vec::new(),
            screens: std::array::from_fn(|_| CockpitScreenActuator::stowed(now)),
            warp_stowed: false,
            updated_at_ms: now,
        }
    }
    pub fn fractions_at(&self, now: u64) -> [f64; COCKPIT_MECHANISM_SLOTS] {
        std::array::from_fn(|slot| self.screens[slot].fraction_at(now))
    }
}
