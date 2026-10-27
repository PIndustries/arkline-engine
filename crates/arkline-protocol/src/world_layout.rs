//! Immutable realm geometry revisions. Missing persisted/snapshot data means the
//! original layout; a new editor realm opts into OrbitalV1 explicitly.
use crate::{
    HOME_SYSTEM_ID, PLANET_APPROACH_POSITION, PLANET_ATMOSPHERE_HEIGHT_M, PLANET_CENTER_POSITION,
    PLANET_RADIUS_M, SPACE_BOUND, STATION_POSITION,
};
use serde::{Deserialize, Serialize};

/// Highest advertised world-feature capability, not the serialized immutable
/// geometry schema. Version3 adds registered persistent physical drone bays;
/// Version4 adds fresh-only durable bay batches. DockingV1's layout definition
/// deliberately remains schema2.
pub const WORLD_LAYOUT_SCHEMA_VERSION: u32 = 7;
pub const ORBITAL_PLANET_CENTER_DISTANCE_M: f64 = 23_182_219.830_937_64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneLayoutRevision {
    #[default]
    LegacyV1,
    OrbitalV1,
    DockingV1,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FleetLayoutRevision {
    #[default]
    LinearV1,
    CircularV1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanetDefinition {
    pub id: u64,
    pub system_id: u32,
    pub name: String,
    pub center: [f64; 3],
    pub radius_m: f64,
    pub atmosphere_height_m: f64,
    pub approach_position: [f64; 3],
}

impl PlanetDefinition {
    pub fn altitude(&self, position: [f64; 3]) -> f64 {
        position
            .iter()
            .zip(self.center)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt()
            - self.radius_m
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id == 0 || self.system_id == 0 || self.name.is_empty() || self.name.len() > 128 {
            return Err("Invalid planet identity in world layout".into());
        }
        if !self
            .center
            .iter()
            .chain(self.approach_position.iter())
            .all(|v| v.is_finite() && v.abs() <= SPACE_BOUND)
            || !self.radius_m.is_finite()
            || self.radius_m <= 0.0
            || !self.atmosphere_height_m.is_finite()
            || self.atmosphere_height_m < 0.0
            || self.radius_m + self.atmosphere_height_m > SPACE_BOUND
        {
            return Err("Invalid finite planet geometry in world layout".into());
        }
        if (self.altitude(self.approach_position) - 5_000.0).abs() > 0.001 {
            return Err("Planet approach must remain five kilometres above its surface".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldLayoutDefinition {
    pub schema_version: u32,
    pub revision: SceneLayoutRevision,
    pub planets: Vec<PlanetDefinition>,
    pub fleet_layout: FleetLayoutRevision,
}

impl Default for WorldLayoutDefinition {
    fn default() -> Self {
        legacy_world_layout()
    }
}

impl WorldLayoutDefinition {
    pub fn planet(&self, id: u64) -> Option<&PlanetDefinition> {
        self.planets.iter().find(|planet| planet.id == id)
    }

    pub fn is_legacy(&self) -> bool {
        self.revision.is_legacy()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != self.revision.required_capability_version().max(1) {
            return Err(
                "Unsupported world layout schema; update the client before entering this realm"
                    .into(),
            );
        }
        if self.planets.len() != 1 {
            return Err("Supported world layouts require exactly one planet".into());
        }
        for planet in &self.planets {
            planet.validate()?;
        }
        // These revisions are immutable definitions, not an arbitrary scene
        // editing protocol. A new definition needs its own revision/capability.
        if self != &self.revision.definition() {
            return Err("World layout does not match its supported immutable revision".into());
        }
        Ok(())
    }
}

impl SceneLayoutRevision {
    pub fn is_legacy(&self) -> bool {
        matches!(self, Self::LegacyV1)
    }

    pub fn required_capability_version(self) -> u32 {
        match self {
            Self::LegacyV1 => 0,
            Self::OrbitalV1 => 1,
            Self::DockingV1 => 2,
        }
    }

    pub fn definition(self) -> WorldLayoutDefinition {
        let (center, approach_position, fleet_layout) = match self {
            Self::LegacyV1 => (
                PLANET_CENTER_POSITION,
                PLANET_APPROACH_POSITION,
                FleetLayoutRevision::LinearV1,
            ),
            Self::OrbitalV1 | Self::DockingV1 => {
                let center = [
                    STATION_POSITION[0],
                    STATION_POSITION[1] - ORBITAL_PLANET_CENTER_DISTANCE_M,
                    STATION_POSITION[2],
                ];
                let approach = [center[0], center[1] + PLANET_RADIUS_M + 5_000.0, center[2]];
                (center, approach, FleetLayoutRevision::CircularV1)
            }
        };
        WorldLayoutDefinition {
            schema_version: self.required_capability_version().max(1),
            revision: self,
            planets: vec![PlanetDefinition {
                id: 1,
                system_id: HOME_SYSTEM_ID,
                name: "Verdant".into(),
                center,
                radius_m: PLANET_RADIUS_M,
                atmosphere_height_m: PLANET_ATMOSPHERE_HEIGHT_M,
                approach_position,
            }],
            fleet_layout,
        }
    }
}

pub fn legacy_world_layout() -> WorldLayoutDefinition {
    SceneLayoutRevision::LegacyV1.definition()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_definition_retains_exact_anchors_and_default() {
        let layout = legacy_world_layout();
        assert_eq!(
            SceneLayoutRevision::default(),
            SceneLayoutRevision::LegacyV1
        );
        assert_eq!(layout.planets[0].center, [0.0, -6_120_000.0, 0.0]);
        assert_eq!(layout.planets[0].approach_position, [0.0, -115_000.0, 0.0]);
        assert_eq!(layout.planets[0].radius_m, 6_000_000.0);
        assert_eq!(layout.fleet_layout, FleetLayoutRevision::LinearV1);
        layout.validate().unwrap();
    }

    #[test]
    fn orbital_definition_measures_thirty_degrees_and_five_km_approach() {
        let layout = SceneLayoutRevision::OrbitalV1.definition();
        layout.validate().unwrap();
        let planet = &layout.planets[0];
        let distance = planet.altitude(STATION_POSITION) + planet.radius_m;
        let diameter = 2.0 * (planet.radius_m / distance).asin().to_degrees();
        assert!((diameter - 30.0).abs() < 1e-10);
        assert!((planet.altitude(planet.approach_position) - 5_000.0).abs() < 1e-8);
        assert_eq!(planet.radius_m, 6_000_000.0);
        assert_eq!(planet.atmosphere_height_m, 60_000.0);
        assert_eq!(layout.fleet_layout, FleetLayoutRevision::CircularV1);
    }

    #[test]
    fn definitions_round_trip_and_reject_unknown_or_mutated_geometry() {
        for revision in [
            SceneLayoutRevision::LegacyV1,
            SceneLayoutRevision::OrbitalV1,
            SceneLayoutRevision::DockingV1,
        ] {
            let layout = revision.definition();
            let decoded: WorldLayoutDefinition =
                serde_json::from_str(&serde_json::to_string(&layout).unwrap()).unwrap();
            assert_eq!(decoded, layout);
            decoded.validate().unwrap();
            let mut invalid = layout.clone();
            invalid.schema_version += 1;
            assert!(invalid.validate().is_err());
            let mut invalid = layout.clone();
            invalid.planets.push(invalid.planets[0].clone());
            assert!(invalid
                .validate()
                .unwrap_err()
                .contains("exactly one planet"));
            let mut invalid = layout;
            invalid.planets[0].center[0] += 1.0;
            assert!(invalid.validate().is_err());
        }
        assert!(serde_json::from_str::<SceneLayoutRevision>("\"unknown_revision\"").is_err());
    }
}
