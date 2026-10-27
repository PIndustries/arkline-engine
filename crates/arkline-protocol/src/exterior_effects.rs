//! Bounded Pioneer exterior effects. Wattages are gameplay assumptions. Drive
//! telemetry describes existing flight; this does not add a propulsion model.
use crate::{ShipClass, PIONEER_MATERIAL_SHA256};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const EXTERIOR_SCHEMA_VERSION: u32 = 1;
pub const MAX_EXTERIOR_FIXTURES: usize = 32;
/// The room's worst case is 134 W; both circuits share the 1,200 W reactor.
pub const MAX_EXTERIOR_LOAD_W: u64 = 1066;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExteriorFixtureKind {
    Navigation,
    Strobe,
    Window,
    Panel,
    EngineGlow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExteriorPulse {
    pub period_ms: u64,
    pub on_ms: u64,
    pub phase_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExteriorFixture {
    pub id: String,
    pub kind: ExteriorFixtureKind,
    /// Canonical body metres: X length, Y height, Z beam (72 × 18 × 38 m).
    pub position_m: [f64; 3],
    pub color: [f32; 3],
    pub emission: f32,
    pub light_range_m: f32,
    pub light_intensity: f32,
    /// Reserved while enabled, including a strobe's dark interval.
    pub load_w: u64,
    pub pulse: Option<ExteriorPulse>,
}
impl ExteriorFixture {
    /// One shared simulation-clock multiplier for emitted and illuminated pixels.
    pub fn output(&self, vessel: &ExteriorVesselState, time_ms: u64) -> f32 {
        if self.kind == ExteriorFixtureKind::EngineGlow {
            return match vessel.drive {
                ExteriorDriveState::Active => 1.0,
                ExteriorDriveState::Idle if vessel.powered => 0.2,
                _ => 0.0,
            };
        }
        if !vessel.powered {
            return 0.0;
        }
        let on = self.pulse.is_none_or(|p| {
            p.period_ms > 0
                && ((time_ms as u128 + p.phase_ms as u128) % p.period_ms as u128) < p.on_ms as u128
        });
        if on {
            1.0
        } else {
            0.0
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExteriorDefinition {
    pub schema_version: u32,
    /// An authored definition never silently follows an unrelated replacement hull.
    pub hull_asset_sha256: String,
    pub fixtures: Vec<ExteriorFixture>,
}
impl ExteriorDefinition {
    pub fn load_w(&self) -> u64 {
        self.fixtures
            .iter()
            .fold(0u64, |sum, f| sum.saturating_add(f.load_w))
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != EXTERIOR_SCHEMA_VERSION {
            return Err("unsupported exterior schema version".into());
        }
        if self.hull_asset_sha256 != PIONEER_MATERIAL_SHA256 {
            return Err("exterior hull asset hash does not match the installed Pioneer; explicit rebinding is required".into());
        }
        if self.fixtures.is_empty() || self.fixtures.len() > MAX_EXTERIOR_FIXTURES {
            return Err("exterior requires 1 to 32 fixtures".into());
        }
        let mut ids = BTreeSet::new();
        for f in &self.fixtures {
            if f.id.is_empty()
                || f.id.len() > 96
                || !f
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
                || !ids.insert(&f.id)
            {
                return Err("exterior fixture IDs must be unique bounded identifiers".into());
            }
            if !f
                .position_m
                .iter()
                .zip([38.0, 11.0, 21.0])
                .all(|(x, b)| x.is_finite() && x.abs() <= b)
            {
                return Err("exterior mount exceeds canonical body envelope allowance".into());
            }
            if !f
                .color
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                || !f.emission.is_finite()
                || !(0.0..=16.0).contains(&f.emission)
                || !f.light_range_m.is_finite()
                || !(0.0..=30.0).contains(&f.light_range_m)
                || !f.light_intensity.is_finite()
                || !(0.0..=8.0).contains(&f.light_intensity)
            {
                return Err("invalid exterior color, emission or local illumination".into());
            }
            match (f.kind, f.pulse) {
                (ExteriorFixtureKind::Strobe, Some(p))
                    if (200..=10000).contains(&p.period_ms)
                        && (50..=1000).contains(&p.on_ms)
                        && p.on_ms <= p.period_ms
                        && p.phase_ms < p.period_ms => {}
                (ExteriorFixtureKind::Strobe, _) => {
                    return Err("strobe requires a bounded pulse interval".into())
                }
                (_, Some(_)) => return Err("only strobe fixtures support pulses".into()),
                _ => {}
            }
            if f.kind == ExteriorFixtureKind::EngineGlow && f.load_w != 0 {
                return Err("drive glow is telemetry, not an exterior electrical load".into());
            }
        }
        if self.load_w() > MAX_EXTERIOR_LOAD_W {
            return Err("combined Pioneer room and exterior load exceeds 1200 W".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExteriorDefinitionState {
    pub revision: u64,
    pub document: ExteriorDefinition,
}
impl Default for ExteriorDefinitionState {
    fn default() -> Self {
        Self {
            revision: 0,
            document: default_pioneer_exterior(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExteriorDriveState {
    #[default]
    Off,
    Idle,
    Active,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExteriorVesselState {
    /// Exact source-specific fixture positions, not installation or power proof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_definition: Option<crate::carrier_exterior::CarrierExteriorDefinition>,
    pub pilot: String,
    pub ship_class: ShipClass,
    pub vessel_id: String,
    pub enabled: bool,
    pub powered: bool,
    pub drive: ExteriorDriveState,
    pub load_w: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExteriorWorldState {
    pub definition: ExteriorDefinitionState,
    pub vessels: Vec<ExteriorVesselState>,
}

/// Mounts are measured against the published Study05 editable master.
/// See assets/ships/pioneer/exterior-mounts.json for source surfaces and limits.
pub fn default_pioneer_exterior() -> ExteriorDefinition {
    let mut fixtures = Vec::new();
    let mut add = |id: &str, kind, position_m, color, load_w, pulse| {
        fixtures.push(ExteriorFixture {
            id: format!("pioneer.exterior.{id}"),
            kind,
            position_m,
            color,
            emission: 2.0,
            light_range_m: 6.0,
            light_intensity: 0.65,
            load_w,
            pulse,
        })
    };
    use ExteriorFixtureKind::*;
    add(
        "nav.port",
        Navigation,
        [-24.0, 2.8, -18.95],
        [1.0, 0.04, 0.02],
        4,
        None,
    );
    add(
        "nav.starboard",
        Navigation,
        [-24.0, 2.8, 18.95],
        [0.04, 1.0, 0.15],
        4,
        None,
    );
    add(
        "nav.aft",
        Navigation,
        [-36.05, 4.0, 0.0],
        [0.9, 0.95, 1.0],
        4,
        None,
    );
    for (id, position, phase) in [
        ("strobe.dorsal", [-8.0, 6.3, 0.0], 0),
        ("strobe.ventral", [0.0, -9.1, 0.0], 500),
    ] {
        add(
            id,
            Strobe,
            position,
            [0.85, 0.93, 1.0],
            8,
            Some(ExteriorPulse {
                period_ms: 1000,
                on_ms: 100,
                phase_ms: phase,
            }),
        );
    }
    for (id, position) in [
        ("window.port.forward", [35.7025, 9.56, -1.25]),
        ("window.starboard.forward", [35.7025, 9.56, 1.25]),
        ("window.port.aft", [31.0, 9.865, -2.9025]),
        ("window.starboard.aft", [31.0, 9.865, 2.9025]),
    ] {
        add(id, Window, position, [1.0, 0.78, 0.43], 12, None);
    }
    add(
        "panel.port",
        Panel,
        [36.04, -2.3, -6.5],
        [0.15, 0.65, 1.0],
        6,
        None,
    );
    add(
        "panel.starboard",
        Panel,
        [36.04, -2.3, 6.5],
        [0.15, 0.65, 1.0],
        6,
        None,
    );
    add(
        "drive.port",
        EngineGlow,
        [-33.28, 0.0, -12.5],
        [0.15, 0.5, 1.0],
        0,
        None,
    );
    add(
        "drive.starboard",
        EngineGlow,
        [-33.28, 0.0, 12.5],
        [0.15, 0.5, 1.0],
        0,
        None,
    );
    // Authored recessed panes carry the visible surface. These are restrained
    // presentation values, while the existing Window circuit still reserves 12 W.
    for fixture in &mut fixtures {
        if fixture.kind == Window {
            fixture.color = [0.10, 0.42, 0.50];
            fixture.emission = 0.025;
            fixture.light_intensity = 0.08;
            fixture.light_range_m = 2.0;
        }
    }
    ExteriorDefinition {
        schema_version: EXTERIOR_SCHEMA_VERSION,
        hull_asset_sha256: PIONEER_MATERIAL_SHA256.into(),
        fixtures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn historical_pioneer_exterior_hash_is_preserved_and_rejected_after_model_adoption() {
        let historical_hash = "f8f8cc65772c678a46606c27abe205d62c4e6ab37c350f79a0f3a4cdc0c6f402";
        let mut historical = default_pioneer_exterior();
        historical.validate().unwrap();
        assert_ne!(historical.hull_asset_sha256, historical_hash);
        historical.hull_asset_sha256 = historical_hash.into();
        let serialized = serde_json::to_vec(&historical).unwrap();
        let restored: ExteriorDefinition = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(restored, historical);
        assert_eq!(restored.hull_asset_sha256, historical_hash);
        assert!(restored.validate().unwrap_err().contains("hull asset hash"));
        assert_eq!(serde_json::to_vec(&restored).unwrap(), serialized);
    }

    #[test]
    fn definition_bounds_identity_hash_and_versions_are_enforced() {
        let d = default_pioneer_exterior();
        d.validate().unwrap();
        assert_eq!(d.load_w(), 88);
        for change in 0..6 {
            let mut bad = d.clone();
            match change {
                0 => bad.schema_version = 2,
                1 => bad.hull_asset_sha256 = "0".repeat(64),
                2 => bad.fixtures[0].position_m[0] = f64::NAN,
                3 => bad.fixtures[0].load_w = 1200,
                4 => bad.fixtures[0].id = bad.fixtures[1].id.clone(),
                _ => bad.fixtures[3].pulse.as_mut().unwrap().period_ms = 0,
            }
            assert!(bad.validate().is_err());
        }
        let mut value = serde_json::to_value(d).unwrap();
        value["unknown"] = true.into();
        assert!(serde_json::from_value::<ExteriorDefinition>(value).is_err());
    }
    #[test]
    fn pulse_uses_only_simulation_time_and_drive_does_not_follow_branch_switch() {
        let d = default_pioneer_exterior();
        let mut v = ExteriorVesselState {
            source_definition: None,
            pilot: "Test".into(),
            ship_class: ShipClass::Pioneer,
            vessel_id: "v1:Test:Pioneer".into(),
            enabled: true,
            powered: true,
            drive: ExteriorDriveState::Idle,
            load_w: d.load_w(),
        };
        let strobe = &d.fixtures[3];
        assert_eq!(strobe.output(&v, 50), 1.0);
        assert_eq!(strobe.output(&v, 100), 0.0);
        v.powered = false;
        v.enabled = false;
        v.drive = ExteriorDriveState::Active;
        assert_eq!(strobe.output(&v, 50), 0.0);
        assert_eq!(d.fixtures[11].output(&v, 50), 1.0);
    }
}
