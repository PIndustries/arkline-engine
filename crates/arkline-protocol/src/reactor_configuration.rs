//! RECONSTRUCTED from retained API/behavior notes, not recovered original bytes.
//! Zero is expanded/normal configuration; one is compact/travel configuration.
//! Expanded does not imply a maintenance isolation or reactor shutdown.
use serde::{Deserialize, Serialize};

pub const REACTOR_CONFIGURATION_TRAVEL_MS: u64 = 1200;
pub const REACTOR_ASSEMBLY_COMPONENT_ID: &str = "pioneer.reactor.assembly";
pub const REACTOR_COLLAR_COMPONENT_IDS: [&str; 4] = [
    "pioneer.reactor.collar.0",
    "pioneer.reactor.collar.1",
    "pioneer.reactor.collar.2",
    "pioneer.reactor.collar.3",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PioneerReactorConfiguration {
    pub vessel_id: String,
    pub component_id: String,
    pub from_fraction: f64,
    pub target_compact: bool,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    pub blocked: Option<String>,
    pub updated_at_ms: u64,
}
impl PioneerReactorConfiguration {
    pub fn open(vessel_id: String, now: u64) -> Self {
        Self {
            vessel_id,
            component_id: REACTOR_ASSEMBLY_COMPONENT_ID.into(),
            from_fraction: 0.,
            target_compact: false,
            started_at_ms: now,
            duration_ms: 0,
            blocked: None,
            updated_at_ms: now,
        }
    }
    pub fn fraction_at(&self, now: u64) -> f64 {
        if self.blocked.is_some() {
            return self.from_fraction;
        }
        let target = f64::from(self.target_compact);
        if self.duration_ms == 0 {
            return target;
        }
        let progress =
            (now.saturating_sub(self.started_at_ms) as f64 / self.duration_ms as f64).clamp(0., 1.);
        self.from_fraction + (target - self.from_fraction) * progress
    }
    pub fn moving_at(&self, now: u64) -> bool {
        self.blocked.is_none()
            && (self.fraction_at(now) - f64::from(self.target_compact)).abs() > 1e-9
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synchronized_fraction_is_bounded_and_reversible() {
        let mut s = PioneerReactorConfiguration::open("owned".into(), 1000);
        s.target_compact = true;
        s.duration_ms = REACTOR_CONFIGURATION_TRAVEL_MS;
        assert_eq!(s.fraction_at(0), 0.);
        assert_eq!(s.fraction_at(1600), 0.5);
        assert!(s.moving_at(1600));
        assert_eq!(s.fraction_at(u64::MAX), 1.);
        assert!(!s.moving_at(2200));
        s.from_fraction = 0.5;
        s.target_compact = false;
        s.started_at_ms = 1600;
        s.duration_ms = 600;
        assert_eq!(s.fraction_at(1900), 0.25);
        assert_eq!(s.fraction_at(2200), 0.);
    }
    #[test]
    fn blocked_configuration_never_advances_from_retained_fraction() {
        let mut s = PioneerReactorConfiguration::open("owned".into(), 1000);
        s.from_fraction = 0.5;
        s.target_compact = true;
        s.blocked = Some("occupied envelope".into());
        assert_eq!(s.fraction_at(u64::MAX), 0.5);
        assert!(!s.moving_at(u64::MAX));
    }
    #[test]
    fn serialized_configuration_retains_component_identity_and_time() {
        let s = PioneerReactorConfiguration::open("owned".into(), 1000);
        let bytes = serde_json::to_vec(&s).unwrap();
        let restored: PioneerReactorConfiguration = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored, s);
        assert_eq!(restored.component_id, REACTOR_ASSEMBLY_COMPONENT_ID);
    }
}
