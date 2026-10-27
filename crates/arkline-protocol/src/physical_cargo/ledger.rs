//! Inspection-only physical-volume ledger. This is NOT the legacy economic
//! ledger and cannot reinterpret rulesets 1–3 or admit a playable ship source.
use super::{
    catalog::{PhysicalCargoCatalog, PhysicalCargoDesignBinding},
    VolumeCm3,
};
use serde::{Deserialize, Serialize};

pub const PHYSICAL_VOLUME_RULESET_VERSION: u32 = 4;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum PhysicalCargoEvent {
    Initialized {
        source_configuration_sha256: String,
        design: Box<PhysicalCargoDesignBinding>,
        catalog: PhysicalCargoCatalog,
        mining_drone_ids: Vec<u64>,
        resource_cm3: VolumeCm3,
    },
    /// Optional versioned physical mechanism binding. Accounting-only histories remain unchanged.
    BayBound {
        binding: Box<super::bay::PhysicalBayBinding>,
    },
    /// Fresh explicit owner/target binding using existing Pioneer joules.
    WorldBound {
        binding: Box<super::control::PhysicalControlBinding>,
    },
    WorldStep {
        step: Box<super::control::PhysicalControlStep>,
    },
    BayStep {
        step: super::bay::PhysicalBayStep,
    },
    Claim {
        drone_id: u64,
        claim_id: u64,
        volume_cm3: VolumeCm3,
        due_at_ms: u64,
    },
    Extract {
        drone_id: u64,
        claim_id: u64,
    },
    CancelClaim {
        drone_id: u64,
    },
    /// An inspection controller's accounting service, not geometric docking proof.
    ServiceOpened {
        drone_id: u64,
        service_id: u64,
        ready_at_ms: u64,
    },
    ServiceHeld {
        service_id: u64,
    },
    ServiceResumed {
        service_id: u64,
        ready_at_ms: u64,
    },
    Transfer {
        drone_id: u64,
        service_id: u64,
        sequence: u64,
        request_id: u64,
        volume_cm3: VolumeCm3,
    },
    ServiceClosed {
        service_id: u64,
    },
    DroneLost {
        drone_id: u64,
    },
}
