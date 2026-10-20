//! Additive, negotiated docked traversal. Canonical feet never change coordinate frames.
use crate::{FleetLayoutRevision, PioneerDockedSupport, RoomLayoutDocument};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const DOCKED_TRAVERSAL_VERSION: u32 = 2;
pub const DOCKED_SEAT_TRANSITION_MS: u64 = 900;
pub type DockedSupport = PioneerDockedSupport;

/// Binds a command to one owned vessel, source registration and exact live layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockedTraversalBinding {
    pub host_vessel_id: String,
    pub registration_revision: u32,
    pub layout_sha256: String,
    pub fleet_layout: FleetLayoutRevision,
    pub fleet_hangar: bool,
}

pub fn docked_layout_sha256(document: &RoomLayoutDocument) -> String {
    // Documents contain bounded, validated finite geometry. Serializing the same
    // shared struct gives client and authority the same content binding.
    hex::encode(Sha256::digest(
        serde_json::to_vec(document).expect("room layout is serializable"),
    ))
}

/// One rigid pose definition shared by authority, first-person rendering and controls.
pub fn docked_binding_pose(
    binding: &DockedTraversalBinding,
) -> Result<crate::PioneerDockedPose, String> {
    if binding.registration_revision != crate::PIONEER_DOCKED_REGISTRATION_REVISION
        || binding.host_vessel_id.is_empty()
        || binding.host_vessel_id.len() > 128
        || binding.layout_sha256.len() != 64
        || !binding
            .layout_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("Docked traversal registration or layout binding is invalid".into());
    }
    Ok(if binding.fleet_hangar {
        crate::PioneerDockedPose::fleet(binding.fleet_layout)
    } else {
        crate::PioneerDockedPose::private()
    })
}

pub fn docked_traversal_binding(
    host_vessel_id: String,
    document: &RoomLayoutDocument,
    fleet_layout: FleetLayoutRevision,
    fleet_hangar: bool,
) -> DockedTraversalBinding {
    DockedTraversalBinding {
        host_vessel_id,
        registration_revision: crate::PIONEER_DOCKED_REGISTRATION_REVISION,
        layout_sha256: docked_layout_sha256(document),
        fleet_layout,
        fleet_hangar,
    }
}

pub fn docked_traversal_capability() -> DockedTraversalCapability {
    DockedTraversalCapability {
        version: DOCKED_TRAVERSAL_VERSION,
        registration_revision: crate::PIONEER_DOCKED_REGISTRATION_REVISION,
        source_sha256: crate::PIONEER_DOCKED_SOURCE_SHA256.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockedTraversalCapability {
    pub version: u32,
    pub registration_revision: u32,
    pub source_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockedHatch {
    Inner,
    Outer,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockedLiftStop {
    Ship,
    Station,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockedSeatPhase {
    Leaving,
    Entering,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockedSeatTransition {
    pub phase: DockedSeatPhase,
    pub started_at_ms: u64,
    pub ends_at_ms: u64,
}
impl DockedSeatTransition {
    fn progress(&self, now_ms: u64) -> f64 {
        (now_ms.saturating_sub(self.started_at_ms) as f64
            / self.ends_at_ms.saturating_sub(self.started_at_ms).max(1) as f64)
            .clamp(0.0, 1.0)
    }
    pub fn standing_fraction(&self, now_ms: u64) -> f64 {
        let t = self.progress(now_ms);
        match self.phase {
            DockedSeatPhase::Leaving => ((t - 0.5) * 2.0).clamp(0.0, 1.0),
            DockedSeatPhase::Entering => (1.0 - t * 2.0).clamp(0.0, 1.0),
        }
    }
    pub fn chair_fold(&self, now_ms: u64) -> f64 {
        let t = self.progress(now_ms);
        match self.phase {
            DockedSeatPhase::Leaving => (t * 2.0).clamp(0.0, 1.0),
            DockedSeatPhase::Entering => (2.0 - t * 2.0).clamp(0.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockedLiftState {
    /// Absolute station-meter support height; never a body/deck-local offset.
    pub floor_station_m: f64,
    pub target: DockedLiftStop,
    pub moving: bool,
    pub updated_at_ms: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockedVesselActuators {
    pub binding: DockedTraversalBinding,
    pub inner_hatch_open: bool,
    pub outer_hatch_open: bool,
    pub lift: DockedLiftState,
    /// Station docking service availability, independent of ship capacitor load.
    pub service_power_available: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockedTraversalState {
    pub version: u32,
    pub binding: DockedTraversalBinding,
    /// Canonical feet in one station-world meter frame for every support region.
    pub feet_m: [f64; 3],
    pub heading: f64,
    pub support: DockedSupport,
    pub surface_id: String,
    pub moving: bool,
    pub grounded: bool,
    pub vertical_velocity_mps: f64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seat_transition: Option<DockedSeatTransition>,
    /// Snapshot clone; the authoritative copy is persisted per owned vessel.
    pub actuators: DockedVesselActuators,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DockedTraversalRequest {
    LeaveSeat {
        binding: DockedTraversalBinding,
    },
    TakeSeat {
        binding: DockedTraversalBinding,
    },
    Walk {
        binding: DockedTraversalBinding,
        forward: f64,
        strafe: f64,
        turn: f64,
    },
    Jump {
        binding: DockedTraversalBinding,
    },
    SetHatch {
        binding: DockedTraversalBinding,
        hatch: DockedHatch,
        open: bool,
    },
    CallLift {
        binding: DockedTraversalBinding,
        stop: DockedLiftStop,
    },
}
impl DockedTraversalRequest {
    pub fn binding(&self) -> &DockedTraversalBinding {
        match self {
            Self::LeaveSeat { binding }
            | Self::TakeSeat { binding }
            | Self::Walk { binding, .. }
            | Self::Jump { binding }
            | Self::SetHatch { binding, .. }
            | Self::CallLift { binding, .. } => binding,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_content_binding_is_deterministic_and_changes_with_geometry() {
        let original = crate::default_pioneer_room_layout();
        assert_eq!(
            docked_layout_sha256(&original),
            docked_layout_sha256(&original.clone())
        );
        let mut changed = original.clone();
        changed.rooms[0].name.push_str(" revised");
        assert_ne!(
            docked_layout_sha256(&original),
            docked_layout_sha256(&changed)
        );
    }
    #[test]
    fn old_wire_admission_and_avatar_default_out_of_v2() {
        let connect: crate::Request = serde_json::from_value(serde_json::json!({"command":"connect", "protocol_version":1, "pilot":"Legacy", "resume_token":null})).unwrap();
        assert!(matches!(
            connect,
            crate::Request::Connect {
                supports_docked_traversal_v2: false,
                ..
            }
        ));
        let account: crate::Request = serde_json::from_value(serde_json::json!({"command":"account_connect", "protocol_version":1, "pilot":"Legacy", "access_token":"test"})).unwrap();
        assert!(matches!(
            account,
            crate::Request::AccountConnect {
                supports_docked_traversal_v2: false,
                ..
            }
        ));
        let avatar: crate::StationAvatar = serde_json::from_value(
            serde_json::json!({"position":[0,0,22], "heading":0, "moving":false,"updated_at_ms":0}),
        )
        .unwrap();
        assert!(avatar.docked_traversal.is_none());
        let snapshot: crate::Snapshot = serde_json::from_value(serde_json::json!({"server_time_ms":0,"pilots":[],"orders":[],"belt_ore":100000,"ledger_sequence":0,"ledger_hash":crate::GENESIS_HASH})).unwrap();
        assert!(snapshot.docked_traversal_capability.is_none());
    }
    #[test]
    fn seat_joint_phases_are_bounded_and_docked_commands_never_delegate() {
        let phase = DockedSeatTransition {
            phase: DockedSeatPhase::Leaving,
            started_at_ms: 100,
            ends_at_ms: 100 + DOCKED_SEAT_TRANSITION_MS,
        };
        assert_eq!(
            (phase.standing_fraction(100), phase.chair_fold(100)),
            (0.0, 0.0)
        );
        assert_eq!(
            (phase.standing_fraction(550), phase.chair_fold(550)),
            (0.0, 1.0)
        );
        assert_eq!(
            (phase.standing_fraction(1000), phase.chair_fold(1000)),
            (1.0, 1.0)
        );
        let mut enter = phase;
        enter.phase = DockedSeatPhase::Entering;
        assert_eq!(
            (enter.standing_fraction(100), enter.chair_fold(100)),
            (1.0, 1.0)
        );
        assert_eq!(
            (enter.standing_fraction(550), enter.chair_fold(550)),
            (0.0, 1.0)
        );
        assert_eq!(
            (enter.standing_fraction(1000), enter.chair_fold(1000)),
            (0.0, 0.0)
        );
        let request = crate::Request::DockedTraversal {
            request: DockedTraversalRequest::LeaveSeat {
                binding: docked_traversal_binding(
                    "v1:Test:Pioneer".into(),
                    &crate::default_pioneer_room_layout(),
                    FleetLayoutRevision::LinearV1,
                    false,
                ),
            },
        };
        assert!(!request.is_remote_gameplay());
    }
}
