//! Source-bound additive ore receiver. The registered revision12 hull is unchanged.
//! These metre-space poses do not move authority clocks or transfer inventory.
use crate::drone_bay_geometry::BayPose;

pub const DROPOFF_RECEIVER_REVISION: u32 = 1;
pub const DROPOFF_RECEIVER_SOURCE_SHA256: &str =
    "7101f1cfaf7ca4aa6bcae7ed8de966eb97dc47d00c087c644efc242be0f171ff";
pub const DROPOFF_BASE_SOURCE_SHA256: &str =
    "7970f096a6aac9d95b6250b75cd19ea2e54046c9deb77f3592b82efa879112fd";
pub const DROPOFF_MECHANISM_COUNT: usize = 5;
pub const DROPOFF_SOCKET_CLEAR_HALF_M: f64 = 0.25;
pub const DROPOFF_HEAD_HALF_M: f64 = 0.22;
pub const DROPOFF_INSERTION_M: f64 = 0.08;
/// Conservative outward rounding includes float32 exported vertices and all
/// extension/locking poses. This is added to, not substituted for, hull bounds.
pub const DROPOFF_MODULE_WORKING_BOUNDS_M: [[f64; 3]; 2] =
    [[-37.59, -1.81, 4.25], [-35.97, -0.19, 5.75]];
pub const DROPOFF_MODULE_STOWED_BOUNDS_M: [[f64; 3]; 2] =
    [[-36.59, -1.81, 4.25], [-35.97, -0.19, 5.75]];
/// Full5×6×1.8m Mining drone approach with0.25m conservative padding.
/// Admission must also check traffic/world hazards and actual incoming route.
pub const DROPOFF_APPROACH_SWEEP_M: [[f64; 3]; 2] = [[-52.75, -2.15, 1.75], [-37.25, 0.15, 8.25]];
pub const DROPOFF_SOCKET_CAVITY_M: [[f64; 3]; 2] = [[-37.80, -1.25, 4.75], [-37.50, -0.75, 5.25]];

const fn pose(position_m: [f64; 3], orientation_xyzw: [f64; 4]) -> BayPose {
    BayPose {
        position_m,
        orientation_xyzw,
    }
}
pub const fn gate_pose() -> BayPose {
    pose([-50.0, -1.0, 5.0], [0.0, 1.0, 0.0, 0.0])
}
pub const fn coupled_pose() -> BayPose {
    pose([-40.0, -1.0, 5.0], [0.0, 1.0, 0.0, 0.0])
}
pub const fn reference_pose() -> BayPose {
    pose([-36.5, -1.0, 5.0], [0.0, 0.0, 0.0, 1.0])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DropoffMechanismId {
    Stage1,
    Stage2,
    Stage3,
    PortLock,
    StarboardLock,
}
impl DropoffMechanismId {
    pub const fn semantic_id(self) -> &'static str {
        match self {
            Self::Stage1 => "stage/1",
            Self::Stage2 => "stage/2",
            Self::Stage3 => "stage/3",
            Self::PortLock => "latch/port",
            Self::StarboardLock => "latch/starboard",
        }
    }
}
pub const fn mechanism_ids() -> [DropoffMechanismId; DROPOFF_MECHANISM_COUNT] {
    [
        DropoffMechanismId::Stage1,
        DropoffMechanismId::Stage2,
        DropoffMechanismId::Stage3,
        DropoffMechanismId::PortLock,
        DropoffMechanismId::StarboardLock,
    ]
}
pub const fn reference_poses() -> [BayPose; DROPOFF_MECHANISM_COUNT] {
    [reference_pose(); DROPOFF_MECHANISM_COUNT]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropoffConfiguration {
    pub extension: f64,
    pub latch_closed: f64,
}
impl DropoffConfiguration {
    pub const STOWED: Self = Self {
        extension: 0.0,
        latch_closed: 0.0,
    };
    pub const EXTENDED: Self = Self {
        extension: 1.0,
        latch_closed: 0.0,
    };
    pub const COUPLED: Self = Self {
        extension: 1.0,
        latch_closed: 1.0,
    };
    pub fn valid(self) -> bool {
        self.extension.is_finite()
            && self.latch_closed.is_finite()
            && (0.0..=1.0).contains(&self.extension)
            && (0.0..=1.0).contains(&self.latch_closed)
            && (self.latch_closed == 0.0 || self.extension == 1.0)
    }
}
/// Resolved body roots. Consumers apply current*inverse(reference) exactly once;
/// all five then inherit the same authoritative full carrier quaternion.
pub fn mechanism_poses(config: DropoffConfiguration) -> Option<[BayPose; DROPOFF_MECHANISM_COUNT]> {
    if !config.valid() {
        return None;
    }
    let mut roots = reference_poses();
    for (index, root) in roots[..3].iter_mut().enumerate() {
        root.position_m[0] -= (index + 1) as f64 * config.extension / 3.0;
    }
    for (root, sign) in roots[3..].iter_mut().zip([-1.0, 1.0]) {
        root.position_m[0] -= config.extension;
        root.position_m[2] += sign * 0.03 * config.latch_closed;
    }
    Some(roots)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropoffKeyframe {
    Gate,
    Aligned,
    Extended,
    Coupled,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropoffResolvedKeyframe {
    pub drone_body_pose: BayPose,
    pub configuration: DropoffConfiguration,
    pub mechanism_roots: [BayPose; DROPOFF_MECHANISM_COUNT],
}
pub fn keyframe(stage: DropoffKeyframe) -> DropoffResolvedKeyframe {
    let configuration = match stage {
        DropoffKeyframe::Gate | DropoffKeyframe::Aligned => DropoffConfiguration::STOWED,
        DropoffKeyframe::Extended => DropoffConfiguration::EXTENDED,
        DropoffKeyframe::Coupled => DropoffConfiguration::COUPLED,
    };
    DropoffResolvedKeyframe {
        drone_body_pose: if stage == DropoffKeyframe::Gate {
            gate_pose()
        } else {
            coupled_pose()
        },
        configuration,
        mechanism_roots: mechanism_poses(configuration).expect("fixed valid authored keyframe"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_receipt_binds_every_group_and_conservative_envelope() {
        let receipt: serde_json::Value = serde_json::from_str(include_str!(
            "../../../assets/ships/pioneer/ore-receiver-v1/receipt.json"
        ))
        .unwrap();
        let proof: serde_json::Value = serde_json::from_str(include_str!(
            "../../../assets/ships/pioneer/ore-receiver-v1/clearance.json"
        ))
        .unwrap();
        assert_eq!(receipt["output"]["sha256"], DROPOFF_RECEIVER_SOURCE_SHA256);
        assert_eq!(receipt["base_glb_sha256"], DROPOFF_BASE_SOURCE_SHA256);
        assert_eq!(receipt["receiver_revision"], DROPOFF_RECEIVER_REVISION);
        assert_eq!(
            receipt["mechanism_reference_poses"]
                .as_object()
                .unwrap()
                .len(),
            DROPOFF_MECHANISM_COUNT
        );
        for id in mechanism_ids() {
            let root = &receipt["mechanism_reference_poses"][id.semantic_id()];
            assert_eq!(
                serde_json::from_value::<BayPose>(root.clone()).unwrap(),
                reference_pose()
            );
        }
        assert!(proof["hits"].as_array().unwrap().is_empty());
        assert!(proof["hardware_hits"].as_array().unwrap().is_empty());
        assert!(proof["legacy_15_route_pair_checks"].as_u64().unwrap() >= 400_000);
        assert!(!proof["negative_closed_socket_hits"]
            .as_array()
            .unwrap()
            .is_empty());
        for axis in 0..3 {
            assert!(
                DROPOFF_MODULE_WORKING_BOUNDS_M[0][axis]
                    < proof["expanded_module_bounds_m"][0][axis].as_f64().unwrap()
            );
            assert!(
                DROPOFF_MODULE_WORKING_BOUNDS_M[1][axis]
                    > proof["expanded_module_bounds_m"][1][axis].as_f64().unwrap()
            );
        }
    }
    #[test]
    fn extension_and_locking_are_continuous_and_fail_closed() {
        for step in 0..=100 {
            let t = f64::from(step) / 100.0;
            for c in [
                DropoffConfiguration {
                    extension: t,
                    latch_closed: 0.0,
                },
                DropoffConfiguration {
                    extension: 1.0,
                    latch_closed: t,
                },
            ] {
                assert!(mechanism_poses(c).unwrap().iter().all(|pose| pose.valid()));
            }
        }
        for c in [
            DropoffConfiguration {
                extension: f64::NAN,
                latch_closed: 0.0,
            },
            DropoffConfiguration {
                extension: 0.5,
                latch_closed: 0.1,
            },
            DropoffConfiguration {
                extension: 1.1,
                latch_closed: 0.0,
            },
            DropoffConfiguration {
                extension: 1.0,
                latch_closed: f64::INFINITY,
            },
        ] {
            assert!(mechanism_poses(c).is_none());
        }
        let final_roots = mechanism_poses(DropoffConfiguration::COUPLED).unwrap();
        assert_eq!(final_roots[2].position_m, [-37.5, -1.0, 5.0]);
        assert_eq!(final_roots[3].position_m, [-37.5, -1.0, 4.97]);
        assert_eq!(final_roots[4].position_m, [-37.5, -1.0, 5.03]);
    }
    #[test]
    fn socket_contact_uses_same_full_parent_transform() {
        let parent = pose([73.0, -12.0, 49.0], [0.5, 0.5, 0.5, 0.5]);
        let drone = parent.compose(coupled_pose()).unwrap();
        let local_mouth = pose([-2.5, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]);
        let actual = drone.compose(local_mouth).unwrap();
        let receiver = parent
            .compose(pose([-37.5, -1.0, 5.0], [0.0, 1.0, 0.0, 0.0]))
            .unwrap();
        assert_eq!(actual, receiver);
        assert_eq!(
            keyframe(DropoffKeyframe::Aligned).drone_body_pose,
            keyframe(DropoffKeyframe::Coupled).drone_body_pose
        );
    }
}
