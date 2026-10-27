//! Accounting of the existing scalar electrical model, not new engineering ratings.
//! Rates describe the right-hand model state at sampled_at_ms. They are not energy
//! integrated over a time window. Instantaneous command debits remain separate joules.
use crate::{
    power_spec, PioneerRoomState, PowerState, ShipClass, VentMode, ROOM_CONTROLLER_W,
    ROOM_EMERGENCY_MS, ROOM_EMERGENCY_W, ROOM_LIGHT_W, ROOM_VENT_W,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const POWER_ACCOUNTING_SCHEMA_VERSION: u32 = 1;

/// The exact existing Pioneer reservations, shared by integration and telemetry.
/// Exterior reservations remain enabled even when emitted lamps are presently off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PowerLoadBudget {
    pub exterior_reserved_w: u64,
    pub room_controller_w: u64,
    pub room_light_w: u64,
    pub room_vent_w: u64,
    pub emergency_recharge_w: u64,
}
impl PowerLoadBudget {
    pub fn pioneer(
        room: Option<&PioneerRoomState>,
        room_powered: bool,
        exterior_reserved_w: u64,
    ) -> Self {
        let mut budget = Self {
            exterior_reserved_w,
            ..Self::default()
        };
        if let Some(r) = room.filter(|r| room_powered && r.commissioned && r.breaker_closed) {
            budget.room_controller_w = ROOM_CONTROLLER_W;
            budget.room_light_w = if r.light_commanded { ROOM_LIGHT_W } else { 0 };
            budget.room_vent_w = if r.vent_commanded != VentMode::Hold && !r.door_open {
                ROOM_VENT_W
            } else {
                0
            };
            budget.emergency_recharge_w = if r.emergency_remaining_ms < ROOM_EMERGENCY_MS {
                ROOM_EMERGENCY_W
            } else {
                0
            };
        }
        budget
    }
    pub fn total_reserved_w(self) -> Option<u64> {
        self.exterior_reserved_w
            .checked_add(self.room_controller_w)?
            .checked_add(self.room_light_w)?
            .checked_add(self.room_vent_w)?
            .checked_add(self.emergency_recharge_w)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerLoadRow {
    pub component_id: String,
    pub label: String,
    /// Existing scalar-model reservation. Not a measured delivered-power claim.
    pub reserved_w: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnmeteredPowerSystem {
    pub component_id: String,
    pub label: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerDebitRatings {
    pub warp_j: u64,
    pub mining_cycle_j: u64,
    pub drone_mining_cycle_j: u64,
    pub drone_attack_j: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmergencyReserveAccounting {
    pub component_id: String,
    pub remaining_ms: u64,
    pub capacity_ms: u64,
    /// Existing independent reserve output, not another main-bus load.
    pub output_w: u64,
    /// Already included in main-bus reservations, never counted again.
    pub recharge_reserved_w: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerAccountingView {
    pub schema_version: u32,
    pub owner_pilot: String,
    pub vessel_id: String,
    pub ship_class: ShipClass,
    pub sampled_at_ms: u64,
    pub charge_j: u64,
    pub capacity_j: u64,
    pub reactor_rating_w: u64,
    pub loads: Vec<PowerLoadRow>,
    pub accounted_reserved_w: u64,
    /// Nominal reactor rating minus accounted reservations; not measured output.
    pub net_available_charge_w: u64,
    /// Right-hand scalar charging rate, zero at capacity. Fractional carry and
    /// later state transitions are handled by the authoritative integrator.
    pub stored_charge_rate_w: u64,
    pub instantaneous_debits: PowerDebitRatings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emergency_reserve: Option<EmergencyReserveAccounting>,
    /// No rating is known for these systems. Absence of metering is never zero W.
    pub unmetered: Vec<UnmeteredPowerSystem>,
}
impl PowerAccountingView {
    pub fn validate_for(
        &self,
        owner: &str,
        class: ShipClass,
        power: &PowerState,
    ) -> Result<(), String> {
        let spec = power_spec(class);
        if self.schema_version != POWER_ACCOUNTING_SCHEMA_VERSION
            || self.owner_pilot != owner
            || self.vessel_id.is_empty()
            || self.vessel_id.len() > 256
            || self.ship_class != class
            || self.charge_j != power.charge_j
            || self.capacity_j != power.capacity_j
            || self.reactor_rating_w != power.regen_w
            || self.capacity_j != spec.capacity_j
            || self.reactor_rating_w != spec.regen_w
            || self.charge_j > self.capacity_j
            || power.regen_remainder_mj >= 1000
            || self.loads.len() > 64
            || self.unmetered.len() > 32
        {
            return Err("power accounting identity, schema or capacitor mismatch".into());
        }
        let mut ids = BTreeSet::new();
        let mut total = 0u64;
        for row in &self.loads {
            if row.component_id.is_empty()
                || row.component_id.len() > 128
                || row.label.is_empty()
                || row.label.len() > 160
                || !ids.insert(row.component_id.as_str())
            {
                return Err("invalid or duplicate power component".into());
            }
            total = total
                .checked_add(row.reserved_w)
                .ok_or("power accounting overflow")?;
        }
        for row in &self.unmetered {
            if row.component_id.is_empty()
                || row.component_id.len() > 128
                || row.label.is_empty()
                || row.label.len() > 160
                || !ids.insert(row.component_id.as_str())
            {
                return Err("invalid or duplicate unmetered component".into());
            }
        }
        if let Some(reserve) = &self.emergency_reserve {
            let recharge = self
                .loads
                .iter()
                .find(|row| row.component_id == "pioneer.deck1.airlock.emergency")
                .map(|row| row.reserved_w);
            if class != ShipClass::Pioneer
                || reserve.component_id != "pioneer.deck1.airlock.emergency"
                || reserve.capacity_ms != ROOM_EMERGENCY_MS
                || reserve.remaining_ms > reserve.capacity_ms
                || ![0, ROOM_EMERGENCY_W].contains(&reserve.output_w)
                || (reserve.remaining_ms == 0 && reserve.output_w != 0)
                || recharge != Some(reserve.recharge_reserved_w)
                || ![0, ROOM_EMERGENCY_W].contains(&reserve.recharge_reserved_w)
            {
                return Err("invalid independent emergency reserve accounting".into());
            }
        }
        let net = self
            .reactor_rating_w
            .checked_sub(total)
            .ok_or("unsupported electrical overload")?;
        if total != self.accounted_reserved_w
            || net != self.net_available_charge_w
            || self.stored_charge_rate_w
                != if self.charge_j == self.capacity_j {
                    0
                } else {
                    net
                }
            || self.instantaneous_debits
                != (PowerDebitRatings {
                    warp_j: spec.warp_cost_j,
                    mining_cycle_j: spec.mining_cycle_cost_j,
                    drone_mining_cycle_j: spec.drone_mining_cycle_cost_j,
                    drone_attack_j: spec.drone_attack_cost_j,
                })
        {
            return Err("power accounting sum, rate or debit mismatch".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_budget_exactly_matches_every_legacy_room_load_combination() {
        for mask in 0..128 {
            for vent in [VentMode::Hold, VentMode::Equalize, VentMode::Exhaust] {
                let mut r = PioneerRoomState::uncommissioned("owned".into());
                r.commissioned = mask & 1 != 0;
                r.breaker_closed = mask & 2 != 0;
                r.light_commanded = mask & 4 != 0;
                r.door_open = mask & 8 != 0;
                r.emergency_remaining_ms = if mask & 16 != 0 { 0 } else { ROOM_EMERGENCY_MS };
                r.vent_commanded = vent;
                let powered = mask & 32 != 0;
                let ext = if mask & 64 != 0 { 1066 } else { 0 };
                let legacy = ext
                    + if powered && r.commissioned && r.breaker_closed {
                        4 + if r.light_commanded { 24 } else { 0 }
                            + if vent != VentMode::Hold && !r.door_open {
                                96
                            } else {
                                0
                            }
                            + if r.emergency_remaining_ms < ROOM_EMERGENCY_MS {
                                10
                            } else {
                                0
                            }
                    } else {
                        0
                    };
                assert_eq!(
                    PowerLoadBudget::pioneer(Some(&r), powered, ext).total_reserved_w(),
                    Some(legacy)
                );
            }
        }
        assert_eq!(
            PowerLoadBudget::pioneer(None, false, 1066).total_reserved_w(),
            Some(1066)
        );
    }
    #[test]
    fn impossible_overflow_is_not_saturated_into_plausible_telemetry() {
        let mut r = PioneerRoomState::uncommissioned("owned".into());
        r.commissioned = true;
        r.breaker_closed = true;
        assert_eq!(
            PowerLoadBudget::pioneer(Some(&r), true, u64::MAX).total_reserved_w(),
            None
        );
    }

    fn reading() -> (PowerAccountingView, PowerState) {
        let spec = power_spec(ShipClass::Pioneer);
        let power = PowerState {
            charge_j: 1000,
            capacity_j: spec.capacity_j,
            regen_w: spec.regen_w,
            updated_at_ms: 0,
            regen_remainder_mj: 0,
        };
        let view = PowerAccountingView {
            schema_version: 1,
            owner_pilot: "Pilot".into(),
            vessel_id: "actual-owned-vessel".into(),
            ship_class: ShipClass::Pioneer,
            sampled_at_ms: 100,
            charge_j: power.charge_j,
            capacity_j: power.capacity_j,
            reactor_rating_w: power.regen_w,
            loads: vec![PowerLoadRow {
                component_id: "known.branch".into(),
                label: "Known reservation".into(),
                reserved_w: 28,
            }],
            accounted_reserved_w: 28,
            net_available_charge_w: 1172,
            stored_charge_rate_w: 1172,
            instantaneous_debits: PowerDebitRatings {
                warp_j: 30000,
                mining_cycle_j: 4000,
                drone_mining_cycle_j: 1500,
                drone_attack_j: 2000,
            },
            emergency_reserve: None,
            unmetered: vec![UnmeteredPowerSystem {
                component_id: "unknown.motor".into(),
                label: "Unmetered motor".into(),
            }],
        };
        (view, power)
    }
    #[test]
    fn projection_rejects_mismatched_identity_schema_arithmetic_and_duplicate_components() {
        let (view, power) = reading();
        assert!(view
            .validate_for("Pilot", ShipClass::Pioneer, &power)
            .is_ok());
        assert!(view
            .validate_for("Other", ShipClass::Pioneer, &power)
            .is_err());
        for case in 0..8 {
            let mut v = view.clone();
            match case {
                0 => v.schema_version = 2,
                1 => v.accounted_reserved_w += 1,
                2 => v.net_available_charge_w += 1,
                3 => v.stored_charge_rate_w = 0,
                4 => v.instantaneous_debits.warp_j += 1,
                5 => v.loads.push(v.loads[0].clone()),
                6 => v.unmetered[0].component_id = v.loads[0].component_id.clone(),
                _ => v.charge_j += 1,
            }
            assert!(
                v.validate_for("Pilot", ShipClass::Pioneer, &power).is_err(),
                "case{case}"
            );
        }
    }
    #[test]
    fn full_capacitor_separates_available_rate_from_right_hand_stored_rate() {
        let (mut view, mut power) = reading();
        power.charge_j = power.capacity_j;
        view.charge_j = power.charge_j;
        assert!(view
            .validate_for("Pilot", ShipClass::Pioneer, &power)
            .is_err());
        view.stored_charge_rate_w = 0;
        assert!(view
            .validate_for("Pilot", ShipClass::Pioneer, &power)
            .is_ok());
        assert_eq!(view.net_available_charge_w, 1172);
        let bytes = serde_json::to_vec(&view).unwrap();
        assert_eq!(
            serde_json::from_slice::<PowerAccountingView>(&bytes).unwrap(),
            view
        );
    }

    #[test]
    fn emergency_output_is_independent_and_recharge_must_match_main_bus_row() {
        let (mut v, p) = reading();
        v.loads[0].component_id = "pioneer.deck1.airlock.emergency".into();
        v.loads[0].reserved_w = 10;
        v.accounted_reserved_w = 10;
        v.net_available_charge_w = 1190;
        v.stored_charge_rate_w = 1190;
        v.emergency_reserve = Some(EmergencyReserveAccounting {
            component_id: "pioneer.deck1.airlock.emergency".into(),
            remaining_ms: 1000,
            capacity_ms: ROOM_EMERGENCY_MS,
            output_w: 0,
            recharge_reserved_w: 10,
        });
        assert!(v.validate_for("Pilot", ShipClass::Pioneer, &p).is_ok());
        v.emergency_reserve.as_mut().unwrap().recharge_reserved_w = 0;
        assert!(v.validate_for("Pilot", ShipClass::Pioneer, &p).is_err());
        v.loads[0].reserved_w = 0;
        v.accounted_reserved_w = 0;
        v.net_available_charge_w = 1200;
        v.stored_charge_rate_w = 1200;
        v.emergency_reserve.as_mut().unwrap().output_w = 10;
        assert!(v.validate_for("Pilot", ShipClass::Pioneer, &p).is_ok());
        v.emergency_reserve.as_mut().unwrap().remaining_ms = 0;
        assert!(v.validate_for("Pilot", ShipClass::Pioneer, &p).is_err());
    }
}
