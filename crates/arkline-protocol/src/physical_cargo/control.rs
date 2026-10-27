//! Explicit fresh-world control binding for the source-certified single bay.
//! This opts into existing Pioneer capacitor joules. Older inspection energy
//! units and prototype asteroid ore remain unchanged and are never converted.
use super::{bay::PhysicalBayStep, VolumeCm3};
use crate::{PowerState, TargetLock};
use serde::{Deserialize, Serialize};

pub const PHYSICAL_CONTROL_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalControlBinding {
    pub version: u32,
    pub pilot: String,
    pub drone_id: u64,
    pub target_id: u64,
    pub system_id: u32,
    /// Existing Pioneer power specification, not a new physical rating.
    pub initial_power: PowerState,
    pub cycle_cost_j: u64,
    /// This first source-fixed host explicitly admits no other electrical loads.
    pub frozen_load_w: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalControlProjection {
    pub power: PowerState,
    pub hit_points: u32,
    pub signal_available: bool,
    pub target_lock: Option<TargetLock>,
    pub target_locked: bool,
    pub mining: bool,
    pub mining_cycle: Option<crate::DroneMiningCycle>,
    pub recalling: bool,
    pub resource_remaining_cm3: VolumeCm3,
    pub accepted_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalControlState {
    pub binding: PhysicalControlBinding,
    pub projection: PhysicalControlProjection,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PhysicalControlAction {
    Launch,
    Lock,
    Unlock,
    Assign,
    Hold,
    Resume,
    Recall,
    Tick,
    Restart,
    /// Authority-side actor input, never an untrusted client command.
    Damage {
        amount: u32,
    },
    /// Authority-side signal fault input, never a positive client clearance ack.
    Signal {
        available: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalControlStep {
    pub action: PhysicalControlAction,
    pub before: PhysicalControlProjection,
    pub after: PhysicalControlProjection,
    /// Exact ordered mechanical steps; independent replay derives this list.
    pub bay_steps: Vec<PhysicalBayStep>,
}

/// Additive, explicit-unit projection for the gated source-bound controller.
/// Existing ore fields remain prototype units and are zero in this fresh mode.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhysicalControlPublicView {
    /// Additive mechanical status. Older receipts without it are not admitted by the physical client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bay_state: Option<super::bay::PhysicalBayState>,
    pub source_configuration_sha256: String,
    pub state: PhysicalControlState,
    pub ship_payload_cm3: VolumeCm3,
    pub ship_capacity_cm3: VolumeCm3,
    pub drone_payload_cm3: VolumeCm3,
    pub drone_capacity_cm3: VolumeCm3,
    pub source_frame: crate::carrier_inventory_geometry::CarrierInventoryKeyframe,
    pub drone_inlet_travel_mm: f64,
}
