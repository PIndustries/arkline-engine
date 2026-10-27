//! Presentation of the explicitly admitted one-bay physical world. This is not
//! legacy ore conversion, authority, motion prediction or normal world admission.
use super::*;
use arkline_protocol::physical_cargo::{
    bay::{binding_source_valid, PhysicalBayRoute},
    control::{PhysicalControlPublicView, PHYSICAL_CONTROL_VERSION},
    source::{
        physical_source_configuration, DRONE_GEOMETRIC_CAPACITY_CM3, SHIP_GEOMETRIC_CAPACITY_CM3,
    },
};

pub(crate) fn validate(
    v: &PhysicalControlPublicView,
    s: &Snapshot,
    owner: &str,
) -> Result<(), String> {
    let fail = || {
        "Physical bay response does not match its source, owner or authoritative snapshot"
            .to_owned()
    };
    let cfg = physical_source_configuration().ok_or_else(fail)?;
    let b = &v.state.binding;
    let p = &v.state.projection;
    let bay = v.bay_state.as_ref().ok_or_else(fail)?;
    let actor = s.pilots.iter().find(|a| a.name == owner).ok_or_else(fail)?;
    let target = s
        .asteroids
        .iter()
        .find(|a| a.id == b.target_id)
        .ok_or_else(fail)?;
    let flight = actor.flight.as_ref().ok_or_else(fail)?;
    let pose = v.source_frame.drone_pose.ok_or_else(fail)?;
    if b.version != PHYSICAL_CONTROL_VERSION
        || b.pilot != owner
        || s.pilots.len() != 1
        || s.asteroids.len() != 1
        || actor.ship_class != ShipClass::Pioneer
        || actor.location != Location::Belt
        || actor.system_id != b.system_id
        || target.system_id != b.system_id
        || actor.ore != 0
        || target.ore != 0
        || s.belt_ore != 0
        || s.stored_ore != 0
        || actor.power.as_ref() != Some(&p.power)
        || actor.target_lock != p.target_lock
        || actor.locked_asteroid != p.target_locked.then_some(b.target_id)
        || p.accepted_at_ms > s.server_time_ms
        || flight.updated_at_ms != p.accepted_at_ms
        || p.power.updated_at_ms > p.accepted_at_ms
        || p.power.charge_j > p.power.capacity_j
        || flight.position != [0.; 3]
        || flight.velocity != [0.; 3]
        || flight.orientation != [0., 0., 0., 1.]
        || flight.warp.is_some()
        || flight.docking.is_some()
        || cfg.identity_sha256().as_deref() != Some(v.source_configuration_sha256.as_str())
        || !binding_source_valid(&bay.binding)
        || bay.binding.source_configuration_sha256 != v.source_configuration_sha256
        || bay.binding.drone_id != b.drone_id
        // Held or unavailable machinery retains its last accepted source step
        // while fresh authoritative observations advance the control clock.
        || bay.last_step_ms > p.accepted_at_ms
        || (bay.last_step_ms < p.accepted_at_ms
            && !bay.held
            && bay.health > 0
            && bay.available)
        || !bay.world_power
        || bay.health != p.hit_points
        || bay.available != p.signal_available
        || v.ship_capacity_cm3 != SHIP_GEOMETRIC_CAPACITY_CM3
        || v.drone_capacity_cm3 != DRONE_GEOMETRIC_CAPACITY_CM3
        || v.ship_payload_cm3 > v.ship_capacity_cm3
        || v.drone_payload_cm3 > v.drone_capacity_cm3
        || !v.drone_inlet_travel_mm.is_finite()
        || !(0. ..=250.).contains(&v.drone_inlet_travel_mm)
        || target.position != bay.binding.target_center_mm.map(|x| x as f64 / 1000.)
        || target.radius != f64::from(bay.binding.target_radius_mm) / 1000.
        || !pose.valid()
        || actor.carrier_inventory.is_some()
        || s.carrier_inventory_registration.is_some()
    {
        return Err(fail());
    }
    let visible = bay.deployed || bay.route.is_some() || p.hit_points == 0;
    if actor.drones.len() != usize::from(visible) {
        return Err(fail());
    }
    if let Some(d) = actor.drones.first() {
        if d.id != b.drone_id
            || d.class != DroneClass::Mining
            || d.hit_points != p.hit_points
            || d.position != pose.position_m
            || d.orientation_xyzw != Some(pose.orientation_xyzw)
            || d.updated_at_ms != p.accepted_at_ms
            || d.mining_cycle != p.mining_cycle
        {
            return Err(fail());
        }
    }
    Ok(())
}

pub(crate) fn litres(cm3: u64) -> String {
    format!("{}.{:03} L", cm3 / 1000, cm3 % 1000)
}
fn status(v: &PhysicalControlPublicView) -> &'static str {
    let Some(b) = &v.bay_state else {
        return "Unavailable";
    };
    if b.health == 0 {
        return "Destroyed";
    }
    if !b.available {
        return "Signal unavailable";
    }
    if b.held {
        return "Held";
    }
    match b.route {
        Some(PhysicalBayRoute::Launch) => "Launching",
        Some(PhysicalBayRoute::Service) if v.source_frame.milestones.ore_coupled => {
            "Coupled / transferring"
        }
        Some(PhysicalBayRoute::Service) => "Ore transfer approach / separation",
        Some(PhysicalBayRoute::Return) => "Returning to protected storage",
        None if v.state.projection.mining => "Mining",
        None if b.deployed => "Deployed / holding",
        None => "Protected storage",
    }
}
impl Cockpit {
    pub(super) fn accept_physical_response(
        &mut self,
        incoming: Option<PhysicalControlPublicView>,
        snapshot: &Snapshot,
    ) -> Result<(), String> {
        if let Some(view) = incoming.as_ref() {
            if !self.physical_session {
                return Err("Physical source requires an explicit local preview session".into());
            }
            validate(view, snapshot, &self.pilot)?;
            if let (Some(old), Some(previous)) = (&self.physical_cargo, &self.snapshot) {
                if snapshot.server_time_ms < previous.server_time_ms
                    || snapshot.ledger_sequence < previous.ledger_sequence
                    || view.state.projection.accepted_at_ms < old.state.projection.accepted_at_ms
                    || (snapshot.ledger_sequence == previous.ledger_sequence
                        && snapshot.ledger_hash != previous.ledger_hash)
                    || (snapshot.ledger_sequence == previous.ledger_sequence
                        && serde_json::to_vec(view).ok() != serde_json::to_vec(old).ok())
                {
                    return Err("Stale or contradictory physical bay response rejected".into());
                }
            }
        } else if self.physical_session || self.physical_cargo.is_some() {
            return Err("Physical bay response lost its explicit source projection".into());
        }
        self.flight_view
            .observe_physical(snapshot, incoming.as_ref(), &self.pilot)?;
        self.physical_cargo = incoming;
        Ok(())
    }
    pub(super) fn physical_request_allowed(&self, request: &Request) -> bool {
        let Some(v) = &self.physical_cargo else {
            return !self.physical_session
                || matches!(request, Request::Snapshot | Request::ExportLedger);
        };
        if !matches!(request, Request::Snapshot | Request::ExportLedger)
            && self
                .performance_snapshot_received
                .is_none_or(|t| t.elapsed().as_millis() > 1500)
        {
            return false;
        }
        if self.controlled_name() != v.state.binding.pilot {
            return false;
        }
        match request {
            Request::Snapshot
            | Request::ExportLedger
            | Request::RecallDrone
            | Request::DroneHold
            | Request::UnlockTarget => true,
            Request::LaunchDroneClass {
                class: DroneClass::Mining,
            } => true,
            Request::AssignMiningDrone { id, target_id } => {
                *id == v.state.binding.drone_id && *target_id == v.state.binding.target_id
            }
            Request::LockAsteroid { id } | Request::UnlockAsteroid { id } => {
                *id == v.state.binding.target_id
            }
            Request::RecallDroneById { id } | Request::DroneHoldById { id } => {
                *id == v.state.binding.drone_id
            }
            _ => false,
        }
    }
    pub(super) fn physical_cargo_controls(&mut self, ui: &mut egui::Ui, pilot: &PilotView) {
        let Some(v) = self.physical_cargo.clone() else {
            return;
        };
        let fresh = self.connected
            && self
                .snapshot
                .as_ref()
                .is_some_and(|s| validate(&v, s, &self.pilot).is_ok())
            && self
                .performance_snapshot_received
                .is_some_and(|t| t.elapsed().as_millis() <= 1500);
        let ready = fresh && self.can_operate() && pilot.name == self.pilot;
        ui.horizontal(|ui| {
            ui.strong("PHYSICAL CARGO / BAY 01");
            if ui
                .small_button(if self.physical_panel_open {
                    "Hide monitor"
                } else {
                    "Show monitor"
                })
                .clicked()
            {
                self.physical_panel_open = !self.physical_panel_open;
            }
        });
        ui.small("Controlled source 1704 · one bay · fixed ship/target · mass rating unknown");
        if !self.physical_panel_open {
            ui.small("Monitor hidden. Machinery continues under authority.");
            return;
        }
        ui.colored_label(
            if fresh { TEAL } else { AMBER },
            if fresh {
                status(&v)
            } else {
                "Stale / disconnected · controls locked"
            },
        );
        for (label, amount, capacity) in [
            ("Ship hold", v.ship_payload_cm3.0, v.ship_capacity_cm3.0),
            (
                "Drone payload",
                v.drone_payload_cm3.0,
                v.drone_capacity_cm3.0,
            ),
        ] {
            ui.label(format!(
                "{label}: {} / {}",
                litres(amount),
                litres(capacity)
            ));
            ui.add(egui::ProgressBar::new(amount as f32 / capacity as f32).show_percentage());
        }
        ui.small(format!(
            "Target remaining: {} · accepted {} ms",
            litres(v.state.projection.resource_remaining_cm3.0),
            v.state.projection.accepted_at_ms
        ));
        ui.label(format!(
            "Capacitor: {} / {} J · drone health {}",
            v.state.projection.power.charge_j,
            v.state.projection.power.capacity_j,
            v.state.projection.hit_points
        ));
        ui.small(format!(
            "Drone inlet {:.1} / 250 mm · receiver coupling {}",
            v.drone_inlet_travel_mm,
            if v.source_frame.milestones.ore_coupled {
                "confirmed"
            } else {
                "not coupled"
            }
        ));
        let id = v.state.binding.drone_id;
        let target = v.state.binding.target_id;
        ui.horizontal_wrapped(|ui| {
            for (label, command) in [
                (
                    "Launch / resume",
                    Request::LaunchDroneClass {
                        class: DroneClass::Mining,
                    },
                ),
                ("Lock target", Request::LockAsteroid { id: target }),
                (
                    "Mine / resume",
                    Request::AssignMiningDrone {
                        id,
                        target_id: target,
                    },
                ),
                ("Hold", Request::DroneHoldById { id }),
                ("Recall", Request::RecallDroneById { id }),
            ] {
                if ui.add_enabled(ready, egui::Button::new(label)).clicked() {
                    self.send(command);
                }
            }
        });
        ui.small("Controls request authority. Showing or hiding this monitor never changes the operation.");
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn physical_volume_labels_do_not_round_or_relabel_prototype_units() {
        assert_eq!(litres(893833), "893.833 L");
        assert_eq!(litres(214121238), "214121.238 L");
        assert_eq!(litres(1), "0.001 L");
    }
    fn response() -> Response {
        crate::flight3d::physical_cargo_native_tests::initial_response_for_tests()
    }
    fn cockpit() -> Cockpit {
        let mut c = crate::tests::view_test_cockpit(&egui::Context::default());
        let r = response();
        let v = r.physical_cargo.unwrap();
        c.pilot = v.state.binding.pilot.clone();
        c.physical_session = true;
        c.snapshot = None;
        c.accept_physical_response(Some(v), r.snapshot.as_ref().unwrap())
            .unwrap();
        c.snapshot = r.snapshot;
        c.performance_snapshot_received = Some(Instant::now());
        c
    }
    #[test]
    fn exact_authority_view_rejects_wrong_source_owner_units_pose_and_clock() {
        let r = response();
        let v = r.physical_cargo.unwrap();
        let s = r.snapshot.unwrap();
        assert!(validate(&v, &s, &v.state.binding.pilot).is_ok());
        assert!(validate(&v, &s, "foreign").is_err());
        for case in 0..8 {
            let mut bad = v.clone();
            let mut snap = s.clone();
            match case {
                0 => bad.source_configuration_sha256 = "00".repeat(32),
                1 => bad.ship_capacity_cm3.0 += 1,
                2 => bad.drone_payload_cm3.0 = bad.drone_capacity_cm3.0 + 1,
                3 => bad.state.projection.power.charge_j += 1,
                4 => bad.bay_state = None,
                5 => snap.pilots[0].flight.as_mut().unwrap().position[0] += 1.,
                6 => bad.state.projection.accepted_at_ms = snap.server_time_ms + 1,
                _ => bad.drone_inlet_travel_mm = f64::NAN,
            }
            assert!(
                validate(&bad, &snap, &v.state.binding.pilot).is_err(),
                "case {case}"
            );
        }
    }
    #[test]
    fn stale_downgrade_and_same_ledger_mutation_fail_closed() {
        let mut c = cockpit();
        let s = c.snapshot.clone().unwrap();
        let v = c.physical_cargo.clone().unwrap();
        c.accept_physical_response(Some(v.clone()), &s).unwrap();
        let mut bad = v.clone();
        bad.ship_payload_cm3.0 += 1;
        assert!(c.accept_physical_response(Some(bad), &s).is_err());
        assert!(c.accept_physical_response(None, &s).is_err());
        let mut old = s.clone();
        old.ledger_sequence = old.ledger_sequence.saturating_sub(1);
        assert!(c.accept_physical_response(Some(v.clone()), &old).is_err());
        c.physical_session = false;
        assert!(c.accept_physical_response(Some(v), &s).is_err());
    }
    #[test]
    fn explicit_physical_session_rejects_first_missing_source_and_unconfirmed_actions() {
        let mut c = crate::tests::view_test_cockpit(&egui::Context::default());
        c.physical_session = true;
        c.physical_cargo = None;
        let r = response();
        assert!(c
            .accept_physical_response(None, r.snapshot.as_ref().unwrap())
            .is_err());
        assert!(!c.physical_request_allowed(&Request::DroneHold));
        assert!(c.physical_request_allowed(&Request::Snapshot));
    }
    #[test]
    fn physical_disconnect_and_rejection_remove_snapshot_instead_of_legacy_fallback() {
        for reconnect in [false, true] {
            let mut c = cockpit();
            let (tx, rx) = std::sync::mpsc::channel();
            c.updates = Some(rx);
            tx.send(if reconnect {
                Update::Reconnecting("test disconnect".into())
            } else {
                Update::Failure("test disconnect".into())
            })
            .unwrap();
            c.poll();
            assert!(c.snapshot.is_none());
            assert!(c.physical_cargo.is_none());
            assert!(!c.can_operate());
        }
        let mut c = cockpit();
        c.reject_world_layout("rejected physical source".into());
        assert!(c.snapshot.is_none());
        assert!(c.physical_cargo.is_none());
    }
    #[test]
    fn exact_control_allowlist_does_not_enable_flight_foreign_drone_or_stale_actions() {
        let mut c = cockpit();
        let b = c.physical_cargo.as_ref().unwrap().state.binding.clone();
        assert!(!c.can_fly());
        assert!(!c.can_warp());
        assert!(c.physical_request_allowed(&Request::AssignMiningDrone {
            id: b.drone_id,
            target_id: b.target_id
        }));
        assert!(!c.physical_request_allowed(&Request::AssignMiningDrone {
            id: b.drone_id + 1,
            target_id: b.target_id
        }));
        assert!(!c.physical_request_allowed(&Request::Undock));
        c.performance_snapshot_received = None;
        assert!(!c.physical_request_allowed(&Request::DroneHold));
        assert!(c.physical_request_allowed(&Request::Snapshot));
    }
}
