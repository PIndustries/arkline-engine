//! Versioned single-bay inspection vocabulary. No network command or normal
//! carrier registration admits this source. Positions are source-derived, never
//! supplied as acknowledgements by a caller.
mod certificate;
pub use certificate::{certified_binding, certified_inspection_binding, physical_definition};
pub fn binding_source_valid(binding: &PhysicalBayBinding) -> bool {
    certified_binding(binding)
}
use super::VolumeCm3;
use serde::{Deserialize, Serialize};

pub const PHYSICAL_BAY_VERSION: u32 = 1;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalBayBinding {
    pub version: u32,
    pub source_configuration_sha256: String,
    pub fixture_sha256: String,
    pub selected_bay_certificate_sha256: String,
    pub drone_id: u64,
    pub berth_id: u8,
    pub cassette_id: u64,
    /// Explicit inspection speed, not an engineering actuator rating.
    pub segment_duration_ms: u64,
    /// Explicit authored fixture energy units, not a measured battery rating.
    pub initial_energy_units: u64,
    /// Fixed inspection ore target in ship-relative millimetres.
    pub target_center_mm: [i64; 3],
    pub target_radius_mm: u32,
    pub mining_range_mm: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhysicalBayRoute {
    Launch,
    Service,
    Return,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum PhysicalBayCommand {
    Launch,
    Mine {
        volume_cm3: VolumeCm3,
    },
    Service,
    Return,
    Hold,
    Resume,
    Cancel,
    /// Inspection fault input, not a client-supplied positive clearance ack.
    SetAvailable {
        available: bool,
    },
    Damage {
        amount: u32,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum PhysicalBayStep {
    Command {
        expected_step_sequence: u64,
        command: PhysicalBayCommand,
    },
    /// Time accepted by authority, bounded to one source segment. No catch-up.
    Advance { elapsed_ms: u64 },
    /// Restart freezes the actual accepted state; no offline motion or pumping.
    Restart,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalBayState {
    pub binding: PhysicalBayBinding,
    pub step_sequence: u64,
    pub route: Option<PhysicalBayRoute>,
    pub frame_index: u16,
    pub segment_elapsed_ms: u64,
    pub deployed: bool,
    pub held: bool,
    pub available: bool,
    pub health: u32,
    pub energy_units: u64,
    /// Only an explicit WorldBound transcript enables real authoritative power.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub world_power: bool,
    pub mining_claim_id: Option<u64>,
    pub claim_high_watermark: u64,
    pub service_id: Option<u64>,
    pub service_high_watermark: u64,
    pub transfer_sequence: u64,
    pub request_high_watermark: u64,
    pub last_step_ms: u64,
}
