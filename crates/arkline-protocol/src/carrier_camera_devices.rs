//! Source-bound camera equipment projection. Supply is explicitly unmetered;
//! health/damage sensors are not simulated. Normal source admission is separate.
use crate::{drone_bay_geometry::BayPose, power_spec, PowerState, ShipClass};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const FIXTURE: &str = include_str!("carrier_camera_fixture.json");
const FIXTURE_SHA: &str = "431d77f32477688d9f4b054ca31c4f1cbe88991e478317552d164042ec857b9b";

const PARALLEL_FIXTURE: &str = include_str!("carrier_parallel_camera_fixture.json");
const PARALLEL_FIXTURE_SHA: &str =
    "425597be16b9c252d0071d5ab1e4d2afb03d5905dc0b2fbe99fe955c94824f0d";
pub const PARALLEL_CAMERA_SOURCE: &str =
    "79ab43221861f74383ca36ca6a742ff5588e08003eb7b10f9d8b6f0d5469664a";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraBoxPart {
    pub name: String,
    pub center_m: [f64; 3],
    pub half_m: [f64; 3],
    pub axes: [[f64; 3]; 3],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountedCameraDefinition {
    pub id: String,
    /// `hull.body` or an exact moving source mechanism semantic ID.
    pub parent_id: String,
    pub eye_m: [f64; 3],
    pub look_at_m: [f64; 3],
    pub vertical_fov_radians: f64,
    pub near_m: f64,
    pub far_m: f64,
    pub geometry: Vec<CameraBoxPart>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarrierCameraDefinition {
    pub assembly_revision: u32,
    pub source_sha256: String,
    pub device_revision: u32,
    pub devices: Vec<MountedCameraDefinition>,
}
/// Exact immutable source registry; unsupported/new loader sources stay absent
/// until their own moving brackets, apertures and swept routes have been proved.
pub fn for_exact_source(revision: u32, hash: &str) -> Option<&'static CarrierCameraDefinition> {
    if revision == 1703 && hash == PARALLEL_CAMERA_SOURCE {
        static PARALLEL: std::sync::OnceLock<CarrierCameraDefinition> = std::sync::OnceLock::new();
        return Some(PARALLEL.get_or_init(|| {
            assert_eq!(
                hex::encode(Sha256::digest(PARALLEL_FIXTURE.as_bytes())),
                PARALLEL_FIXTURE_SHA
            );
            serde_json::from_str(PARALLEL_FIXTURE).expect("compiled parallel source camera fixture")
        }));
    }
    static DEFINITION: std::sync::OnceLock<CarrierCameraDefinition> = std::sync::OnceLock::new();
    let definition = DEFINITION.get_or_init(|| {
        assert_eq!(hex::encode(Sha256::digest(FIXTURE.as_bytes())), FIXTURE_SHA);
        serde_json::from_str(FIXTURE).expect("compiled source camera fixture")
    });
    (definition.assembly_revision == revision && definition.source_sha256 == hash)
        .then_some(definition)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraSupplyModel {
    Unmetered,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraHealthModel {
    NotSimulated,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraAvailability {
    Available,
    MissingPower,
    InvalidPower,
    Unpowered,
    MissingParent,
    InvalidParent,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarrierCameraDeviceState {
    pub device_id: String,
    pub parent_id: String,
    /// Authoritative current parent world pose, not an extrapolated/documentary eye.
    pub parent_world_pose: Option<BayPose>,
    pub availability: CameraAvailability,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarrierCameraPublicState {
    pub vessel_id: String,
    pub assembly_revision: u32,
    pub source_sha256: String,
    pub device_revision: u32,
    pub definition_sha256: String,
    pub sampled_at_ms: u64,
    pub supply_model: CameraSupplyModel,
    pub health_model: CameraHealthModel,
    pub devices: Vec<CarrierCameraDeviceState>,
}

fn supply(power: Option<&PowerState>, now: u64) -> CameraAvailability {
    let Some(p) = power else {
        return CameraAvailability::MissingPower;
    };
    let spec = power_spec(ShipClass::Pioneer);
    if p.capacity_j != spec.capacity_j
        || p.regen_w != spec.regen_w
        || p.charge_j > p.capacity_j
        || p.regen_remainder_mj >= 1000
        || p.updated_at_ms > now
    {
        CameraAvailability::InvalidPower
    } else if p.charge_j == 0 {
        CameraAvailability::Unpowered
    } else {
        CameraAvailability::Available
    }
}
/// Invoke only AFTER validating the vessel's complete authoritative registration.
/// Hull and local mechanism roots must be from the SAME just-advanced snapshot.
/// This makes no electrical debit and never turns a source fixture into gameplay
/// admission. An absent source yields no camera projection, not a legacy fallback.
pub fn project(
    definition: &CarrierCameraDefinition,
    vessel_id: &str,
    hull_world: BayPose,
    mechanism_roots: &BTreeMap<String, BayPose>,
    power: Option<&PowerState>,
    now: u64,
) -> Result<CarrierCameraPublicState, String> {
    let expected = for_exact_source(definition.assembly_revision, &definition.source_sha256)
        .ok_or("camera source has not been registered")?;
    if definition != expected || vessel_id.is_empty() || vessel_id.len() > 160 {
        return Err("camera source/device/vessel definition mismatch".into());
    }
    let bus = supply(power, now);
    let devices = definition
        .devices
        .iter()
        .map(|device| {
            let parent = if device.parent_id == "hull.body" {
                hull_world.valid().then_some(hull_world)
            } else {
                mechanism_roots
                    .get(&device.parent_id)
                    .and_then(|p| hull_world.compose(*p))
            };
            let availability = if !hull_world.valid() {
                CameraAvailability::InvalidParent
            } else if parent.is_none() {
                if mechanism_roots.contains_key(&device.parent_id) {
                    CameraAvailability::InvalidParent
                } else {
                    CameraAvailability::MissingParent
                }
            } else {
                bus
            };
            CarrierCameraDeviceState {
                device_id: device.id.clone(),
                parent_id: device.parent_id.clone(),
                parent_world_pose: parent,
                availability,
            }
        })
        .collect();
    Ok(CarrierCameraPublicState {
        vessel_id: vessel_id.into(),
        assembly_revision: definition.assembly_revision,
        source_sha256: definition.source_sha256.clone(),
        device_revision: definition.device_revision,
        definition_sha256: if definition.assembly_revision == 1703 {
            PARALLEL_FIXTURE_SHA
        } else {
            FIXTURE_SHA
        }
        .into(),
        sampled_at_ms: now,
        supply_model: CameraSupplyModel::Unmetered,
        health_model: CameraHealthModel::NotSimulated,
        devices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn definition() -> &'static CarrierCameraDefinition {
        for_exact_source(
            1702,
            "57059d059a267da00ec2ae4d4a4dfd27393cd2280239bb241ffb751ea506b113",
        )
        .unwrap()
    }
    fn power() -> PowerState {
        let s = power_spec(ShipClass::Pioneer);
        PowerState {
            charge_j: s.capacity_j,
            capacity_j: s.capacity_j,
            regen_w: s.regen_w,
            updated_at_ms: 100,
            regen_remainder_mj: 0,
        }
    }
    fn pose() -> BayPose {
        BayPose {
            position_m: [10., 20., 30.],
            orientation_xyzw: [0., 0., 0., 1.],
        }
    }
    #[test]
    fn source_existence_does_not_invent_camera_power_or_health_sensor() {
        let p = project(
            definition(),
            "owned-vessel",
            pose(),
            &BTreeMap::new(),
            None,
            100,
        )
        .unwrap();
        assert_eq!(p.devices.len(), 12);
        assert!(p
            .devices
            .iter()
            .all(|d| d.availability == CameraAvailability::MissingPower));
        assert_eq!(p.supply_model, CameraSupplyModel::Unmetered);
        assert_eq!(p.health_model, CameraHealthModel::NotSimulated);
        let p = project(
            definition(),
            "owned-vessel",
            pose(),
            &BTreeMap::new(),
            Some(&power()),
            100,
        )
        .unwrap();
        assert!(p
            .devices
            .iter()
            .all(|d| d.availability == CameraAvailability::Available));
        assert!(p
            .devices
            .iter()
            .all(|d| d.parent_world_pose == Some(pose())));
    }
    #[test]
    fn depleted_bad_or_future_supply_and_invalid_parent_stay_unavailable() {
        for case in 0..3 {
            let mut power = power();
            match case {
                0 => power.charge_j = 0,
                1 => power.capacity_j += 1,
                _ => power.updated_at_ms = 101,
            }
            let p = project(
                definition(),
                "v",
                pose(),
                &BTreeMap::new(),
                Some(&power),
                100,
            )
            .unwrap();
            assert!(p
                .devices
                .iter()
                .all(|d| d.availability != CameraAvailability::Available));
        }
        let mut parent = pose();
        parent.position_m[0] = f64::NAN;
        let p = project(
            definition(),
            "v",
            parent,
            &BTreeMap::new(),
            Some(&power()),
            100,
        )
        .unwrap();
        assert!(p
            .devices
            .iter()
            .all(|d| d.availability == CameraAvailability::InvalidParent
                && d.parent_world_pose.is_none()));
    }
    #[test]
    fn new_source_or_unproved_crane_attachment_cannot_inherit_fixed_devices() {
        assert!(for_exact_source(1703, &definition().source_sha256).is_none());
        assert!(for_exact_source(1702, "new-source").is_none());
        let mut changed = definition().clone();
        changed.devices[0].parent_id = "unproved.loader".into();
        assert!(project(&changed, "v", pose(), &BTreeMap::new(), Some(&power()), 100).is_err());
    }
    #[test]
    fn parallel_source_requires_actual_parent_and_actual_supply() {
        let d = for_exact_source(1703, PARALLEL_CAMERA_SOURCE).unwrap();
        assert_eq!(d.devices.len(), 10);
        assert!(for_exact_source(1702, PARALLEL_CAMERA_SOURCE).is_none());
        assert!(for_exact_source(1703, "wrong").is_none());
        let empty = project(d, "owned", pose(), &BTreeMap::new(), Some(&power()), 100).unwrap();
        assert!(empty
            .devices
            .iter()
            .all(|d| d.availability == CameraAvailability::MissingParent));
        let mut roots = BTreeMap::new();
        for (i, device) in d.devices.iter().enumerate() {
            assert_eq!(device.parent_id, format!("cell{i:02}/loader/trolley"));
            roots.insert(
                device.parent_id.clone(),
                BayPose {
                    position_m: [i as f64, 7., 9.],
                    orientation_xyzw: [0., 0., 0., 1.],
                },
            );
        }
        let live = project(d, "owned", pose(), &roots, Some(&power()), 100).unwrap();
        assert_eq!(live.definition_sha256, PARALLEL_FIXTURE_SHA);
        assert_eq!(
            live.devices[0].parent_world_pose.unwrap().position_m,
            [10., 27., 39.]
        );
        assert!(live
            .devices
            .iter()
            .all(|d| d.availability == CameraAvailability::Available));
        let mut off = power();
        off.charge_j = 0;
        assert!(project(d, "owned", pose(), &roots, Some(&off), 100)
            .unwrap()
            .devices
            .iter()
            .all(|d| d.availability == CameraAvailability::Unpowered));
        roots.get_mut(&d.devices[0].parent_id).unwrap().position_m[2] += 3.;
        let moved = project(d, "owned", pose(), &roots, Some(&power()), 100).unwrap();
        assert_eq!(
            moved.devices[0].parent_world_pose.unwrap().position_m,
            [10., 27., 42.]
        );
        assert_eq!(moved.devices[1], live.devices[1]);
    }
}
