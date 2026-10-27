//! Explicit fresh-world gameplay design data. Geometry measures capacity; it
//! does not select these rates/prices or establish mass or pump performance.
//! No catalog/rating is registered by this module and no legacy price is read.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhysicalCargoForm {
    /// The quantity already IS cm³ of bulk material, not a count of ore items.
    BulkVolumeCm3,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredBulkPrice {
    pub credits_numerator: u64,
    pub volume_cm3_denominator: VolumeCm3,
}
impl AuthoredBulkPrice {
    fn valid(&self) -> bool {
        self.credits_numerator > 0 && self.volume_cm3_denominator.0 > 0
    }
    /// Buyers pay the rational quote rounded up; no floating credit creation.
    pub fn buy_quote_credits(&self, volume: VolumeCm3) -> Option<u64> {
        if !self.valid() {
            return None;
        }
        let value = u128::from(volume.0) * u128::from(self.credits_numerator);
        u64::try_from(value.div_ceil(u128::from(self.volume_cm3_denominator.0))).ok()
    }
    /// Sellers receive the rational quote rounded down. Repeated small sells
    /// cannot extract extra credits by rounding; actual trade authority is absent.
    pub fn sell_quote_credits(&self, volume: VolumeCm3) -> Option<u64> {
        if !self.valid() {
            return None;
        }
        let value = u128::from(volume.0) * u128::from(self.credits_numerator);
        u64::try_from(value / u128::from(self.volume_cm3_denominator.0)).ok()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalCargoMaterial {
    pub material_id: String,
    pub form: PhysicalCargoForm,
    pub mass: CargoMassKnowledge,
    /// None means trading is unavailable, NOT a zero price or a legacy fallback.
    pub authored_price: Option<AuthoredBulkPrice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredCargoOperatingRatings {
    pub rating_id: String,
    pub revision: u32,
    pub drone_extraction: AuthoredVolumeRate,
    pub ship_extraction: AuthoredVolumeRate,
    pub vessel_manifold: AuthoredVolumeRate,
    /// Explicit GAMEPLAY energy units per completed batch, not measured joules.
    /// Contact, due-time and actual power reservation remain authority-owned.
    pub drone_batch_energy_units: u64,
    pub ship_batch_energy_units: u64,
}
impl AuthoredCargoOperatingRatings {
    fn valid(&self) -> bool {
        identifier(&self.rating_id)
            && self.revision > 0
            && self.drone_extraction.valid()
            && self.ship_extraction.valid()
            && self.vessel_manifold.valid()
            && self.drone_batch_energy_units > 0
            && self.ship_batch_energy_units > 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalCargoCatalog {
    pub schema_version: u32,
    pub materials: Vec<PhysicalCargoMaterial>,
    pub operating_ratings: Vec<AuthoredCargoOperatingRatings>,
}
impl PhysicalCargoCatalog {
    pub fn valid_shape(&self) -> bool {
        let mut materials = BTreeSet::new();
        let mut ratings = BTreeSet::new();
        self.schema_version == PHYSICAL_CARGO_SCHEMA_VERSION
            && !self.materials.is_empty()
            && self.materials.len() <= 64
            && !self.operating_ratings.is_empty()
            && self.operating_ratings.len() <= 64
            && self.materials.iter().all(|m| {
                identifier(&m.material_id)
                    && materials.insert(&m.material_id)
                    && m.mass.valid()
                    && m.authored_price
                        .as_ref()
                        .is_none_or(AuthoredBulkPrice::valid)
            })
            && self
                .operating_ratings
                .iter()
                .all(|r| r.valid() && ratings.insert(&r.rating_id))
    }
    pub fn identity_sha256(&self) -> Option<String> {
        self.valid_shape().then(|| {
            hex::encode(Sha256::digest(
                serde_json::to_vec(self).expect("integer catalog"),
            ))
        })
    }
    /// Exact explicit selection only. This does not verify the source cells or
    /// install a world; final genesis must pin BOTH catalog and manifest hashes.
    pub fn bind_manifest(
        &self,
        material_id: &str,
        rating_id: &str,
        measurements: CargoSourceMeasurements,
    ) -> Option<PhysicalCargoDesignBinding> {
        let catalog_sha256 = self.identity_sha256()?;
        let material = self
            .materials
            .iter()
            .find(|m| m.material_id == material_id)?;
        let rating = self
            .operating_ratings
            .iter()
            .find(|r| r.rating_id == rating_id)?;
        let manifest = PhysicalCargoManifest {
            schema_version: PHYSICAL_CARGO_SCHEMA_VERSION,
            ship_source_sha256: measurements.ship_source_sha256,
            mining_drone_source_sha256: measurements.mining_drone_source_sha256,
            material_id: material.material_id.clone(),
            mass: material.mass.clone(),
            ship_hold: measurements.ship_hold,
            mining_drone_hold: measurements.mining_drone_hold,
            drone_extraction: rating.drone_extraction.clone(),
            ship_extraction: rating.ship_extraction.clone(),
            vessel_manifold: rating.vessel_manifold.clone(),
        };
        manifest
            .valid_shape()
            .then_some(PhysicalCargoDesignBinding {
                catalog_sha256,
                rating_id: rating.rating_id.clone(),
                manifest,
            })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CargoSourceMeasurements {
    pub ship_source_sha256: String,
    pub mining_drone_source_sha256: String,
    pub ship_hold: MeasuredCargoHold,
    pub mining_drone_hold: MeasuredCargoHold,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalCargoDesignBinding {
    pub catalog_sha256: String,
    pub rating_id: String,
    pub manifest: PhysicalCargoManifest,
}
impl PhysicalCargoDesignBinding {
    pub fn valid_against_catalog(&self, catalog: &PhysicalCargoCatalog) -> bool {
        let m = &self.manifest;
        catalog
            .bind_manifest(
                &m.material_id,
                &self.rating_id,
                CargoSourceMeasurements {
                    ship_source_sha256: m.ship_source_sha256.clone(),
                    mining_drone_source_sha256: m.mining_drone_source_sha256.clone(),
                    ship_hold: m.ship_hold.clone(),
                    mining_drone_hold: m.mining_drone_hold.clone(),
                },
            )
            .as_ref()
            == Some(self)
    }
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._/-".contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn catalog() -> PhysicalCargoCatalog {
        // SYNTHETIC ratings only: these numbers are not a proposed live balance.
        PhysicalCargoCatalog {
            schema_version: 1,
            materials: vec![PhysicalCargoMaterial {
                material_id: "fixture.bulk".into(),
                form: PhysicalCargoForm::BulkVolumeCm3,
                mass: CargoMassKnowledge::Unknown,
                authored_price: None,
            }],
            operating_ratings: vec![AuthoredCargoOperatingRatings {
                rating_id: "fixture.ratings".into(),
                revision: 1,
                drone_extraction: AuthoredVolumeRate {
                    volume_cm3: VolumeCm3(3),
                    interval_ms: 7,
                },
                ship_extraction: AuthoredVolumeRate {
                    volume_cm3: VolumeCm3(5),
                    interval_ms: 11,
                },
                vessel_manifold: AuthoredVolumeRate {
                    volume_cm3: VolumeCm3(2),
                    interval_ms: 13,
                },
                drone_batch_energy_units: 1,
                ship_batch_energy_units: 2,
            }],
        }
    }
    fn sources() -> CargoSourceMeasurements {
        let hold = MeasuredCargoHold {
            source_hold_id: "fixture.hold".into(),
            measurement_receipt_sha256: "a".repeat(64),
            usable_cells: vec![UsableCargoCellMm {
                min: [0; 3],
                max: [100; 3],
            }],
        };
        CargoSourceMeasurements {
            ship_source_sha256: "b".repeat(64),
            mining_drone_source_sha256: "c".repeat(64),
            ship_hold: hold.clone(),
            mining_drone_hold: hold,
        }
    }
    #[test]
    fn explicit_catalog_selection_has_no_default_rates_prices_or_mass() {
        let c = catalog();
        let b = c
            .bind_manifest("fixture.bulk", "fixture.ratings", sources())
            .unwrap();
        assert!(b.valid_against_catalog(&c));
        assert_eq!(b.manifest.drone_extraction.volume_cm3, VolumeCm3(3));
        assert!(b.manifest.mass.mass_mg_rounded_up(VolumeCm3(1)).is_none());
        assert!(c.materials[0].authored_price.is_none());
        assert!(c
            .bind_manifest("missing", "fixture.ratings", sources())
            .is_none());
        assert!(c
            .bind_manifest("fixture.bulk", "missing", sources())
            .is_none());
        let mut json = serde_json::to_value(&c).unwrap();
        json["operating_ratings"][0]
            .as_object_mut()
            .unwrap()
            .remove("vessel_manifold");
        assert!(serde_json::from_value::<PhysicalCargoCatalog>(json).is_err());
    }
    #[test]
    fn catalog_hash_pins_gameplay_ratings_and_never_blesses_modified_binding() {
        let c = catalog();
        let b = c
            .bind_manifest("fixture.bulk", "fixture.ratings", sources())
            .unwrap();
        let mut changed = c.clone();
        changed.operating_ratings[0].ship_batch_energy_units += 1;
        assert_ne!(c.identity_sha256(), changed.identity_sha256());
        assert!(!b.valid_against_catalog(&changed));
        let mut bad = b.clone();
        bad.manifest.vessel_manifold.interval_ms += 1;
        assert!(!bad.valid_against_catalog(&c));
        let mut changed = c.clone();
        changed
            .operating_ratings
            .push(changed.operating_ratings[0].clone());
        assert!(!changed.valid_shape());
        let mut changed = c.clone();
        changed.materials.push(changed.materials[0].clone());
        assert!(!changed.valid_shape());
        let mut changed = c.clone();
        changed.operating_ratings[0].drone_extraction.interval_ms = 0;
        assert!(!changed.valid_shape());
        let mut changed = c;
        changed.materials[0].mass = CargoMassKnowledge::AuthoredMilligramsPerCm3 {
            numerator: 1,
            denominator: 0,
        };
        assert!(!changed.valid_shape());
    }
    #[test]
    fn authored_rational_price_rounds_conservatively_and_refuses_overflow() {
        let price = AuthoredBulkPrice {
            credits_numerator: 5,
            volume_cm3_denominator: VolumeCm3(3),
        };
        assert_eq!(price.buy_quote_credits(VolumeCm3(1)), Some(2));
        assert_eq!(price.sell_quote_credits(VolumeCm3(1)), Some(1));
        assert_eq!(price.buy_quote_credits(VolumeCm3(3)), Some(5));
        assert_eq!(price.sell_quote_credits(VolumeCm3(3)), Some(5));
        assert_eq!(price.buy_quote_credits(VolumeCm3(0)), Some(0));
        assert_eq!(price.buy_quote_credits(VolumeCm3(u64::MAX)), None);
        let bad = AuthoredBulkPrice {
            volume_cm3_denominator: VolumeCm3(0),
            ..price
        };
        assert_eq!(bad.sell_quote_credits(VolumeCm3(1)), None);
    }
}
