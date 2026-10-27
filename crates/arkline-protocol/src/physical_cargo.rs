//! Fresh physical-volume accounting foundation. NOT installed by any factory.
//!
//! Legacy rules 1–3 quantities remain prototype units; there is deliberately no
//! conversion from them. Source certification and authored rates must be pinned
//! separately before this module can drive gameplay or claim an installed hold.
//! Authority still owns certificate/finite-roster binding, extraction deadlines,
//! target/power, first-transfer grant delay and actual stable socket contact.
//! This accounting core is not a replay verifier or a world factory.
pub mod bay;
pub mod catalog;
pub mod control;
pub mod ledger;
pub mod source;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const PHYSICAL_CARGO_SCHEMA_VERSION: u32 = 1;
pub const CUBIC_CENTIMETRES_PER_CUBIC_METRE: u64 = 1_000_000;

/// Exact 1 cm³ resolution. This is volume, never item count or kilograms.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VolumeCm3(pub u64);
impl VolumeCm3 {
    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }
    /// Conservative occupancy for an explicitly authored rational item-volume.
    /// Round the entire requested quantity up, never round each item down.
    pub fn for_items_rounded_up(count: u64, numerator_cm3: u64, denominator: u64) -> Option<Self> {
        if numerator_cm3 == 0 || denominator == 0 {
            return None;
        }
        let product = u128::from(count) * u128::from(numerator_cm3);
        let value = product.div_ceil(u128::from(denominator));
        u64::try_from(value).ok().map(Self)
    }
    /// Integer whole/fraction display parts; no floating arithmetic at admission.
    pub fn cubic_metre_parts(self) -> (u64, u64) {
        (
            self.0 / CUBIC_CENTIMETRES_PER_CUBIC_METRE,
            self.0 % CUBIC_CENTIMETRES_PER_CUBIC_METRE,
        )
    }
}

/// Source-certified disjoint usable cell, after walls, hardware, service and
/// access exclusions. Coordinates are integer millimetres in the named hold.
/// Shape validation alone is NOT a proof that a cell lies inside the model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsableCargoCellMm {
    pub min: [i64; 3],
    pub max: [i64; 3],
}
impl UsableCargoCellMm {
    pub fn capacity_rounded_down(&self) -> Option<VolumeCm3> {
        let mut cubic_mm = 1u128;
        for axis in 0..3 {
            let extent = i128::from(self.max[axis]) - i128::from(self.min[axis]);
            if extent <= 0 {
                return None;
            }
            cubic_mm = cubic_mm.checked_mul(u128::try_from(extent).ok()?)?;
        }
        u64::try_from(cubic_mm / 1_000).ok().map(VolumeCm3)
    }
    fn overlaps(&self, other: &Self) -> bool {
        (0..3).all(|i| self.min[i] < other.max[i] && other.min[i] < self.max[i])
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeasuredCargoHold {
    pub source_hold_id: String,
    pub measurement_receipt_sha256: String,
    pub usable_cells: Vec<UsableCargoCellMm>,
}
impl MeasuredCargoHold {
    pub fn capacity_cm3(&self) -> Option<VolumeCm3> {
        if self.source_hold_id.is_empty()
            || self.source_hold_id.len() > 128
            || !sha256_valid(&self.measurement_receipt_sha256)
            || self.usable_cells.is_empty()
            || self.usable_cells.len() > 256
        {
            return None;
        }
        let mut total = VolumeCm3(0);
        for (i, cell) in self.usable_cells.iter().enumerate() {
            if self.usable_cells[..i]
                .iter()
                .any(|other| cell.overlaps(other))
            {
                return None;
            }
            total = total.checked_add(cell.capacity_rounded_down()?)?;
        }
        (total.0 > 0).then_some(total)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CargoMassKnowledge {
    /// Unknown is not zero mass, not a density, and not a verified payload rating.
    Unknown,
    /// Explicit authored material assumption, not established by chamber geometry.
    AuthoredMilligramsPerCm3 { numerator: u64, denominator: u64 },
}
impl CargoMassKnowledge {
    pub fn mass_mg_rounded_up(&self, volume: VolumeCm3) -> Option<u64> {
        match self {
            Self::Unknown => None,
            Self::AuthoredMilligramsPerCm3 {
                numerator,
                denominator,
            } => VolumeCm3::for_items_rounded_up(volume.0, *numerator, *denominator).map(|v| v.0),
        }
    }
    fn valid(&self) -> bool {
        matches!(self, Self::Unknown)
            || matches!(self,
            Self::AuthoredMilligramsPerCm3 { numerator, denominator } if *numerator > 0 && *denominator > 0)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredVolumeRate {
    pub volume_cm3: VolumeCm3,
    pub interval_ms: u64,
}
impl AuthoredVolumeRate {
    fn valid(&self) -> bool {
        self.volume_cm3.0 > 0 && self.interval_ms > 0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalCargoManifest {
    pub schema_version: u32,
    pub ship_source_sha256: String,
    pub mining_drone_source_sha256: String,
    pub material_id: String,
    pub mass: CargoMassKnowledge,
    pub ship_hold: MeasuredCargoHold,
    pub mining_drone_hold: MeasuredCargoHold,
    /// Deliberately no default rates. These are authored volume/time design data,
    /// not a conversion of the prototype10/8s or25/500ms constants.
    pub drone_extraction: AuthoredVolumeRate,
    /// Per accepted ship-module extraction; authority owns module identity/cadence.
    pub ship_extraction: AuthoredVolumeRate,
    pub vessel_manifold: AuthoredVolumeRate,
}
impl PhysicalCargoManifest {
    pub fn valid_shape(&self) -> bool {
        self.schema_version == PHYSICAL_CARGO_SCHEMA_VERSION
            && sha256_valid(&self.ship_source_sha256)
            && sha256_valid(&self.mining_drone_source_sha256)
            && !self.material_id.is_empty()
            && self.material_id.len() <= 128
            && self.mass.valid()
            && self.ship_hold.capacity_cm3().is_some()
            && self.mining_drone_hold.capacity_cm3().is_some()
            && self.drone_extraction.valid()
            && self.ship_extraction.valid()
            && self.vessel_manifold.valid()
    }
    pub fn identity_sha256(&self) -> Option<String> {
        self.valid_shape().then(|| {
            hex::encode(Sha256::digest(
                serde_json::to_vec(self).expect("serializable integer manifest"),
            ))
        })
    }
}
fn sha256_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeExtractionClaim {
    pub claim_id: u64,
    pub volume_cm3: VolumeCm3,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneVolumeHold {
    pub drone_id: u64,
    pub payload_cm3: VolumeCm3,
    pub extracted_cm3: VolumeCm3,
    pub lost_cm3: VolumeCm3,
    pub destroyed: bool,
    pub claim_high_watermark: u64,
    pub claim: Option<VolumeExtractionClaim>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeTransferReceipt {
    pub request_id: u64,
    pub drone_id: u64,
    pub volume_cm3: VolumeCm3,
    pub committed_at_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalCargoAccount {
    pub manifest_sha256: String,
    pub ship_payload_cm3: VolumeCm3,
    pub extracted_to_ship_cm3: VolumeCm3,
    pub drones: Vec<DroneVolumeHold>,
    pub extracted_cm3: VolumeCm3,
    pub destroyed_cm3: VolumeCm3,
    pub exported_cm3: VolumeCm3,
    pub transfers: Vec<VolumeTransferReceipt>,
}
impl PhysicalCargoAccount {
    /// Fresh zero-balance creation only. Existing unit-count worlds are not input.
    pub fn fresh(manifest: &PhysicalCargoManifest, finite_mining_ids: &[u64]) -> Option<Self> {
        let out = Self {
            manifest_sha256: manifest.identity_sha256()?,
            ship_payload_cm3: VolumeCm3(0),
            extracted_to_ship_cm3: VolumeCm3(0),
            drones: finite_mining_ids
                .iter()
                .map(|id| DroneVolumeHold {
                    drone_id: *id,
                    payload_cm3: VolumeCm3(0),
                    extracted_cm3: VolumeCm3(0),
                    lost_cm3: VolumeCm3(0),
                    destroyed: false,
                    claim_high_watermark: 0,
                    claim: None,
                })
                .collect(),
            extracted_cm3: VolumeCm3(0),
            destroyed_cm3: VolumeCm3(0),
            exported_cm3: VolumeCm3(0),
            transfers: vec![],
        };
        out.validate(manifest).then_some(out)
    }
    /// Authority must supply its independently source-bound immutable roster.
    /// Local balance shape alone cannot detect removal of an empty identity.
    pub fn validate_roster(
        &self,
        manifest: &PhysicalCargoManifest,
        finite_mining_ids: &[u64],
    ) -> bool {
        let expected: BTreeSet<_> = finite_mining_ids.iter().copied().collect();
        self.validate(manifest)
            && expected.len() == finite_mining_ids.len()
            && expected == self.drones.iter().map(|h| h.drone_id).collect()
    }
    pub fn incoming_cm3(&self) -> Option<VolumeCm3> {
        self.drones.iter().try_fold(VolumeCm3(0), |sum, h| {
            sum.checked_add(h.payload_cm3)?
                .checked_add(h.claim.as_ref().map_or(VolumeCm3(0), |c| c.volume_cm3))
        })
    }
    pub fn free_ship_cm3(&self, manifest: &PhysicalCargoManifest) -> Option<VolumeCm3> {
        if manifest.identity_sha256().as_ref() != Some(&self.manifest_sha256) {
            return None;
        }
        manifest
            .ship_hold
            .capacity_cm3()?
            .checked_sub(self.ship_payload_cm3.checked_add(self.incoming_cm3()?)?)
    }
    pub fn validate(&self, manifest: &PhysicalCargoManifest) -> bool {
        let Some(hash) = manifest.identity_sha256() else {
            return false;
        };
        let Some(drone_capacity) = manifest.mining_drone_hold.capacity_cm3() else {
            return false;
        };
        if self.manifest_sha256 != hash
            || self.drones.len() > 512
            || self.free_ship_cm3(manifest).is_none()
        {
            return false;
        }
        let mut ids = BTreeSet::new();
        for h in &self.drones {
            if h.drone_id == 0
                || !ids.insert(h.drone_id)
                || h.payload_cm3
                    .checked_add(h.claim.as_ref().map_or(VolumeCm3(0), |c| c.volume_cm3))
                    .is_none_or(|v| v > drone_capacity)
                || (h.destroyed && (h.payload_cm3.0 != 0 || h.claim.is_some()))
                || h.claim.as_ref().is_some_and(|c| {
                    c.claim_id == 0
                        || c.claim_id != h.claim_high_watermark
                        || c.volume_cm3.0 == 0
                        || c.volume_cm3 > manifest.drone_extraction.volume_cm3
                })
            {
                return false;
            }
        }
        let accounted = self
            .drones
            .iter()
            .try_fold(self.ship_payload_cm3, |v, h| v.checked_add(h.payload_cm3))
            .and_then(|v| v.checked_add(self.destroyed_cm3))
            .and_then(|v| v.checked_add(self.exported_cm3));
        if accounted != Some(self.extracted_cm3) {
            return false;
        }
        let mut requests = BTreeSet::new();
        for (i, r) in self.transfers.iter().enumerate() {
            if r.request_id == 0
                || !requests.insert(r.request_id)
                || !ids.contains(&r.drone_id)
                || r.volume_cm3.0 == 0
                || r.volume_cm3 > manifest.vessel_manifold.volume_cm3
                || (i > 0
                    && self.transfers[i - 1]
                        .committed_at_ms
                        .checked_add(manifest.vessel_manifold.interval_ms)
                        .is_none_or(|due| r.committed_at_ms < due))
            {
                return false;
            }
        }
        let transferred = self
            .transfers
            .iter()
            .try_fold(VolumeCm3(0), |sum, r| sum.checked_add(r.volume_cm3));
        if transferred
            .and_then(|v| self.extracted_to_ship_cm3.checked_add(v))
            .and_then(|v| v.checked_sub(self.exported_cm3))
            != Some(self.ship_payload_cm3)
            || self
                .drones
                .iter()
                .try_fold(self.extracted_to_ship_cm3, |sum, h| {
                    sum.checked_add(h.extracted_cm3)
                })
                != Some(self.extracted_cm3)
            || self
                .drones
                .iter()
                .try_fold(VolumeCm3(0), |sum, h| sum.checked_add(h.lost_cm3))
                != Some(self.destroyed_cm3)
            || self.drones.iter().any(|h| {
                let transferred = self
                    .transfers
                    .iter()
                    .filter(|r| r.drone_id == h.drone_id)
                    .try_fold(VolumeCm3(0), |sum, r| sum.checked_add(r.volume_cm3));
                transferred
                    .and_then(|v| v.checked_add(h.payload_cm3))
                    .and_then(|v| v.checked_add(h.lost_cm3))
                    != Some(h.extracted_cm3)
                    || (!h.destroyed && h.lost_cm3.0 != 0)
            })
        {
            return false;
        }
        true
    }
    fn transact(
        &mut self,
        m: &PhysicalCargoManifest,
        f: impl FnOnce(&mut Self) -> Option<()>,
    ) -> bool {
        if !self.validate(m) {
            return false;
        }
        let mut next = self.clone();
        if f(&mut next).is_none() || !next.validate(m) {
            return false;
        }
        *self = next;
        true
    }
    pub fn reserve_extraction(
        &mut self,
        m: &PhysicalCargoManifest,
        drone_id: u64,
        claim_id: u64,
        volume: VolumeCm3,
    ) -> bool {
        if self
            .drones
            .iter()
            .find(|h| h.drone_id == drone_id)
            .is_some_and(|h| {
                !h.destroyed
                    && h.claim
                        .as_ref()
                        .is_some_and(|c| c.claim_id == claim_id && c.volume_cm3 == volume)
            })
        {
            return self.validate(m);
        }
        self.transact(m, |s| {
            let h = s.drones.iter_mut().find(|h| h.drone_id == drone_id)?;
            if h.destroyed
                || h.claim.is_some()
                || claim_id != h.claim_high_watermark.checked_add(1)?
                || volume.0 == 0
                || volume > m.drone_extraction.volume_cm3
            {
                return None;
            }
            h.claim_high_watermark = claim_id;
            h.claim = Some(VolumeExtractionClaim {
                claim_id,
                volume_cm3: volume,
            });
            Some(())
        })
    }
    pub fn complete_extraction(
        &mut self,
        m: &PhysicalCargoManifest,
        drone_id: u64,
        claim_id: u64,
    ) -> bool {
        // Authority must separately prove the actual source/target/power/deadline.
        self.transact(m, |s| {
            let h = s.drones.iter_mut().find(|h| h.drone_id == drone_id)?;
            let c = h.claim.take()?;
            if h.destroyed || c.claim_id != claim_id {
                return None;
            }
            h.payload_cm3 = h.payload_cm3.checked_add(c.volume_cm3)?;
            h.extracted_cm3 = h.extracted_cm3.checked_add(c.volume_cm3)?;
            s.extracted_cm3 = s.extracted_cm3.checked_add(c.volume_cm3)?;
            Some(())
        })
    }
    pub fn cancel_extraction(&mut self, m: &PhysicalCargoManifest, drone_id: u64) -> bool {
        self.transact(m, |s| {
            s.drones.iter_mut().find(|h| h.drone_id == drone_id)?.claim = None;
            Some(())
        })
    }
    pub fn extract_to_ship(&mut self, m: &PhysicalCargoManifest, volume: VolumeCm3) -> bool {
        self.transact(m, |s| {
            if volume.0 == 0 || volume > m.ship_extraction.volume_cm3 {
                return None;
            }
            s.ship_payload_cm3 = s.ship_payload_cm3.checked_add(volume)?;
            s.extracted_to_ship_cm3 = s.extracted_to_ship_cm3.checked_add(volume)?;
            s.extracted_cm3 = s.extracted_cm3.checked_add(volume)?;
            Some(())
        })
    }
    pub fn transfer(
        &mut self,
        m: &PhysicalCargoManifest,
        request_id: u64,
        drone_id: u64,
        volume: VolumeCm3,
        now: u64,
    ) -> bool {
        // A retry resolves before current stock/death/cooldown; it cannot debit twice.
        if let Some(r) = self.transfers.iter().find(|r| r.request_id == request_id) {
            return self.validate(m) && r.drone_id == drone_id && r.volume_cm3 == volume;
        }
        self.transact(m, |s| {
            let h = s.drones.iter_mut().find(|h| h.drone_id == drone_id)?;
            if h.destroyed || h.claim.is_some() {
                return None;
            }
            h.payload_cm3 = h.payload_cm3.checked_sub(volume)?;
            s.ship_payload_cm3 = s.ship_payload_cm3.checked_add(volume)?;
            s.transfers.push(VolumeTransferReceipt {
                request_id,
                drone_id,
                volume_cm3: volume,
                committed_at_ms: now,
            });
            Some(())
        })
    }
    pub fn destroy_drone(&mut self, m: &PhysicalCargoManifest, drone_id: u64) -> bool {
        self.transact(m, |s| {
            let h = s.drones.iter_mut().find(|h| h.drone_id == drone_id)?;
            s.destroyed_cm3 = s.destroyed_cm3.checked_add(h.payload_cm3)?;
            h.lost_cm3 = h.lost_cm3.checked_add(h.payload_cm3)?;
            h.payload_cm3 = VolumeCm3(0);
            h.claim = None;
            h.destroyed = true;
            Some(())
        })
    }
    pub fn export_from_ship(&mut self, m: &PhysicalCargoManifest, volume: VolumeCm3) -> bool {
        self.transact(m, |s| {
            if volume.0 == 0 {
                return None;
            }
            s.ship_payload_cm3 = s.ship_payload_cm3.checked_sub(volume)?;
            s.exported_cm3 = s.exported_cm3.checked_add(volume)?;
            Some(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest() -> PhysicalCargoManifest {
        // Synthetic measured boxes only; deliberately not a game-source registry.
        let hold = |id: &str, x| MeasuredCargoHold {
            source_hold_id: id.into(),
            measurement_receipt_sha256: "c".repeat(64),
            usable_cells: vec![UsableCargoCellMm {
                min: [0; 3],
                max: [x, 1000, 1000],
            }],
        };
        PhysicalCargoManifest {
            schema_version: 1,
            ship_source_sha256: "a".repeat(64),
            mining_drone_source_sha256: "b".repeat(64),
            material_id: "synthetic-bulk-fixture".into(),
            mass: CargoMassKnowledge::Unknown,
            ship_hold: hold("synthetic.ship", 10),
            mining_drone_hold: hold("synthetic.drone", 4),
            drone_extraction: AuthoredVolumeRate {
                volume_cm3: VolumeCm3(4000),
                interval_ms: 800,
            },
            ship_extraction: AuthoredVolumeRate {
                volume_cm3: VolumeCm3(60000),
                interval_ms: 1000,
            },
            vessel_manifold: AuthoredVolumeRate {
                volume_cm3: VolumeCm3(2500),
                interval_ms: 500,
            },
        }
    }
    #[test]
    fn geometry_capacity_is_disjoint_checked_and_rounds_down_not_up() {
        let mut m = manifest();
        assert_eq!(m.ship_hold.capacity_cm3(), Some(VolumeCm3(10_000)));
        m.ship_hold
            .usable_cells
            .push(m.ship_hold.usable_cells[0].clone());
        assert!(!m.valid_shape());
        assert_eq!(
            UsableCargoCellMm {
                min: [0; 3],
                max: [9, 9, 13]
            }
            .capacity_rounded_down(),
            Some(VolumeCm3(1))
        );
        // Arithmetic check of an unregistered exported-source candidate only.
        // This does not certify its actual walls/socket or install a capacity.
        assert_eq!(
            UsableCargoCellMm {
                min: [-1340, -129, -639],
                max: [-40, 409, 639]
            }
            .capacity_rounded_down(),
            Some(VolumeCm3(893833))
        );
        assert_eq!(
            UsableCargoCellMm {
                min: [i64::MIN; 3],
                max: [i64::MAX; 3]
            }
            .capacity_rounded_down(),
            None
        );
        assert_eq!(
            UsableCargoCellMm {
                min: [0; 3],
                max: [0, 1, 1]
            }
            .capacity_rounded_down(),
            None
        );
    }
    #[test]
    fn rational_items_round_up_overflow_fails_and_unknown_mass_is_not_zero() {
        assert_eq!(VolumeCm3::for_items_rounded_up(3, 1, 2), Some(VolumeCm3(2)));
        assert_eq!(VolumeCm3::for_items_rounded_up(u64::MAX, u64::MAX, 1), None);
        assert_eq!(VolumeCm3::for_items_rounded_up(1, 1, 0), None);
        assert_eq!(VolumeCm3(u64::MAX).checked_add(VolumeCm3(1)), None);
        assert_eq!(VolumeCm3(0).checked_sub(VolumeCm3(1)), None);
        assert_eq!(
            CargoMassKnowledge::Unknown.mass_mg_rounded_up(VolumeCm3(0)),
            None
        );
        assert_eq!(VolumeCm3(898560).cubic_metre_parts(), (0, 898560));
    }
    #[test]
    fn incoming_claims_payload_and_ship_extraction_share_one_volume_budget() {
        let m = manifest();
        let mut s = PhysicalCargoAccount::fresh(&m, &[1, 2]).unwrap();
        assert!(s.reserve_extraction(&m, 1, 1, VolumeCm3(4000)));
        let reserved = s.clone();
        assert!(s.reserve_extraction(&m, 1, 1, VolumeCm3(4000)));
        assert_eq!(s, reserved);
        assert!(s.reserve_extraction(&m, 2, 1, VolumeCm3(4000)));
        assert!(s.extract_to_ship(&m, VolumeCm3(2000)));
        let before = s.clone();
        assert!(!s.extract_to_ship(&m, VolumeCm3(1)));
        assert_eq!(s, before);
        assert!(s.complete_extraction(&m, 1, 1));
        assert_eq!(s.free_ship_cm3(&m), Some(VolumeCm3(0)));
        assert!(s.cancel_extraction(&m, 2));
        assert_eq!(s.free_ship_cm3(&m), Some(VolumeCm3(4000)));
        assert!(!s.reserve_extraction(&m, 2, 1, VolumeCm3(1))); // claim ID cannot be reused
        assert!(s.reserve_extraction(&m, 2, 2, VolumeCm3(4000)));
        assert!(s.complete_extraction(&m, 2, 2));
        assert!(!s.complete_extraction(&m, 2, 2));
        assert!(s.validate(&m));
    }
    #[test]
    fn partial_transfer_global_cooldown_retry_and_export_conserve_volume() {
        let m = manifest();
        let mut s = PhysicalCargoAccount::fresh(&m, &[1, 2]).unwrap();
        for id in [1, 2] {
            assert!(s.reserve_extraction(&m, id, 1, VolumeCm3(4000)));
            assert!(s.complete_extraction(&m, id, 1));
        }
        assert!(s.transfer(&m, 1, 1, VolumeCm3(2500), 100));
        let before = s.clone();
        assert!(!s.transfer(&m, 2, 2, VolumeCm3(2500), 599));
        assert_eq!(s, before);
        assert!(s.transfer(&m, 1, 1, VolumeCm3(2500), 9999));
        assert_eq!(s, before);
        assert!(s.transfer(&m, 2, 1, VolumeCm3(1500), 600));
        assert_eq!(s.free_ship_cm3(&m), Some(VolumeCm3(2000))); // transfer is not new volume
        assert!(s.export_from_ship(&m, VolumeCm3(1000)));
        assert_eq!(s.free_ship_cm3(&m), Some(VolumeCm3(3000)));
        assert!(s.transfer(&m, 3, 2, VolumeCm3(2500), 1100));
        assert!(s.destroy_drone(&m, 2));
        let lost = s.destroyed_cm3;
        assert!(s.destroy_drone(&m, 2));
        assert_eq!(s.destroyed_cm3, lost);
        assert!(s.transfer(&m, 3, 2, VolumeCm3(2500), 1200)); // historical retry after loss
        assert!(!s.transfer(&m, 4, 2, VolumeCm3(1), 1600));
        assert!(s.validate(&m));
        assert_eq!(lost, VolumeCm3(1500));
        let bytes = serde_json::to_vec(&s).unwrap();
        let restored: PhysicalCargoAccount = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(s, restored);
        assert!(restored.validate(&m));
        let mut wrong_owner = restored.clone();
        wrong_owner.transfers[0].drone_id = 2;
        assert!(
            !wrong_owner.validate(&m),
            "Conserved total cannot hide wrong transfer owner"
        );
        let mut phantom_receipt = restored;
        phantom_receipt.transfers.push(VolumeTransferReceipt {
            request_id: 9,
            drone_id: 1,
            volume_cm3: VolumeCm3(1),
            committed_at_ms: 1600,
        });
        assert!(!phantom_receipt.validate(&m));
    }
    #[test]
    fn malformed_manifest_owner_balance_and_cooldown_refuse_without_mutation() {
        let m = manifest();
        assert!(PhysicalCargoAccount::fresh(&m, &[1, 1]).is_none());
        let s = PhysicalCargoAccount::fresh(&m, &[1]).unwrap();
        for kind in 0..3 {
            let mut bad = s.clone();
            match kind {
                0 => bad.ship_payload_cm3 = VolumeCm3(1),
                1 => bad.manifest_sha256 = "0".repeat(64),
                _ => bad.drones[0].payload_cm3 = VolumeCm3(u64::MAX),
            };
            let before = bad.clone();
            assert!(!bad.reserve_extraction(&m, 1, 1, VolumeCm3(1)));
            assert_eq!(bad, before);
        }
        let mut low_rate = m.clone();
        low_rate.drone_extraction.volume_cm3 = VolumeCm3(1000);
        let mut oversized = PhysicalCargoAccount::fresh(&low_rate, &[1]).unwrap();
        oversized.drones[0].claim_high_watermark = 1;
        oversized.drones[0].claim = Some(VolumeExtractionClaim {
            claim_id: 1,
            volume_cm3: VolumeCm3(1001),
        });
        assert!(!oversized.validate(&low_rate));
        low_rate.ship_extraction.volume_cm3 = VolumeCm3(1000);
        let mut ship_limited = PhysicalCargoAccount::fresh(&low_rate, &[1]).unwrap();
        let before = ship_limited.clone();
        assert!(!ship_limited.extract_to_ship(&low_rate, VolumeCm3(1001)));
        assert_eq!(ship_limited, before);
        assert!(s.validate_roster(&m, &[1]));
        let mut omitted = s.clone();
        omitted.drones.clear();
        assert!(omitted.validate(&m));
        assert!(!omitted.validate_roster(&m, &[1]));
        let mut new = m.clone();
        new.vessel_manifold.volume_cm3 = VolumeCm3(1);
        assert!(!s.validate(&new));
        assert_eq!(crate::cargo_capacity(crate::ShipClass::Pioneer), 1000);
        assert_eq!(crate::MINING_DRONE_CARGO_CAPACITY, 100);
    }
    #[test]
    fn ten_incoming_holds_share_budget_and_one_global_transfer_clock() {
        let mut m = manifest();
        m.ship_hold.usable_cells[0].max[0] = 100; // synthetic0.1m³ fixture
        let ids: Vec<_> = (1..=10).collect();
        let mut s = PhysicalCargoAccount::fresh(&m, &ids).unwrap();
        for id in &ids {
            assert!(s.reserve_extraction(&m, *id, 1, VolumeCm3(4000)));
        }
        assert!(s.extract_to_ship(&m, VolumeCm3(60000)));
        let full = s.clone();
        assert!(!s.extract_to_ship(&m, VolumeCm3(1)));
        assert_eq!(s, full);
        for id in &ids {
            assert!(s.complete_extraction(&m, *id, 1));
        }
        assert!(s.transfer(&m, 1, 1, VolumeCm3(2500), 100));
        for id in &ids[1..] {
            let before = s.clone();
            assert!(!s.transfer(&m, *id, *id, VolumeCm3(2500), 100));
            assert_eq!(s, before, "No per-bay multiplication of vessel rate");
            assert!(s.transfer(&m, *id, *id, VolumeCm3(2500), 100 + (id - 1) * 500));
        }
        for id in &ids {
            assert!(s.transfer(&m, id + 10, *id, VolumeCm3(1500), 5100 + (id - 1) * 500));
        }
        assert_eq!(s.ship_payload_cm3, VolumeCm3(100000));
        assert_eq!(s.incoming_cm3(), Some(VolumeCm3(0)));
        assert!(s.validate_roster(&m, &ids));
        assert!(s.export_from_ship(&m, VolumeCm3(1)));
        assert_eq!(s.free_ship_cm3(&m), Some(VolumeCm3(1)));
        assert!(!s.reserve_extraction(&m, 1, 2, VolumeCm3(2)));
        assert!(s.reserve_extraction(&m, 1, 2, VolumeCm3(1)));
        let extracted = s.extracted_cm3;
        assert!(s.destroy_drone(&m, 1));
        assert_eq!(s.extracted_cm3, extracted); // a cancelled claim is not minted/lost payload
        assert_eq!(s.free_ship_cm3(&m), Some(VolumeCm3(1)));
        assert!(!s.complete_extraction(&m, 1, 2));
    }
}
