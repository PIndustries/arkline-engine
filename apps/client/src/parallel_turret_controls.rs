//! Captain controls for physical guns; hiding a monitor never changes deployment.
use super::*;

impl Cockpit {
    pub(super) fn parallel_physical_turret_controls(&mut self, ui: &mut egui::Ui) {
        let Some(pilot) = self.own() else {
            return;
        };
        if pilot.carrier_inventory.is_none()
            && self
                .snapshot
                .as_ref()
                .is_none_or(|s| s.carrier_inventory_registration.is_none())
        {
            return;
        }
        if pilot.ship_class != ShipClass::Pioneer {
            return;
        }
        ui.label("PHYSICAL TURRETS");
        let Some(mechanisms) = &pilot.cockpit_mechanisms else {
            ui.small("Physical turret telemetry unavailable");
            return;
        };
        if !mechanisms.turrets.source_admitted {
            ui.small("Physical stow unavailable on this hull source");
            return;
        }
        let now = self.presentation_time();
        let Some(sample) = self.snapshot.as_ref().map(|s| s.server_time_ms) else {
            return;
        };
        let Ok(state) = arkline_protocol::parallel_turret_binding::confirmed_state(&pilot, sample)
        else {
            ui.small("Physical turret source/owner/sample unavailable");
            return;
        };
        let fresh = self.connected
            && self.snapshot.as_ref().is_some_and(|s| {
                now >= s.server_time_ms && now.saturating_sub(s.server_time_ms) <= 1500
            });
        let normal = matches!(
            pilot.location,
            Location::Station | Location::Belt | Location::Docked
        );
        let admitted = pilot.carrier_inventory.as_ref().is_some_and(|p| {
            p.validate_projection(&pilot)
                == Ok(arkline_protocol::CarrierInventoryAdmission::Enabled)
        });
        let ready = self.can_operate() && fresh && normal && admitted;
        if !admitted {
            ui.small("Inspection telemetry · controls await normal source admission");
        }
        ui.small(&state.status);
        if !fresh {
            ui.colored_label(AMBER, "Last received state · waiting for authority");
        }
        for (slot, a) in state.actuators.iter().enumerate() {
            let aim_status = state.aims.as_ref().map_or("AIM UNAVAILABLE", |aims| {
                let aim = &aims[slot];
                if !fresh || aim.sample_ms != sample || aim.validate_at(sample).is_err() {
                    return "STALE AIM";
                }
                use arkline_protocol::parallel_turret_aim::TurretAimStatus::*;
                match aim.status {
                    Unavailable => "NO AIM TARGET",
                    Aiming => "AIMING",
                    Ready => "AIM READY",
                    OutOfArc => "OUT OF ARC",
                    Obstructed => "OBSTRUCTED",
                    Held => "AIM HELD",
                }
            });
            ui.small(format!(
                "{} · {}% deployed{} · {}",
                if slot == 0 { "Port" } else { "Starboard" },
                (a.fraction_at(sample) * 100.).round() as u32,
                if a.blocked.is_some() { " · HELD" } else { "" },
                aim_status
            ));
            if let Some(reason) = &a.blocked {
                ui.colored_label(AMBER, reason);
            }
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    ready && state.pending_warp.is_none() && !state.operational_at(sample),
                    egui::Button::new("Deploy turrets"),
                )
                .clicked()
            {
                self.send(Request::SetTurretDeployment {
                    vessel_id: mechanisms.vessel_id.clone(),
                    deployed: true,
                });
            }
            if ui
                .add_enabled(
                    ready && !state.secure_at(sample),
                    egui::Button::new("Stow turrets"),
                )
                .clicked()
            {
                self.send(Request::SetTurretDeployment {
                    vessel_id: mechanisms.vessel_id.clone(),
                    deployed: false,
                });
            }
            if state.pending_warp.is_some()
                && ui
                    .add_enabled(ready, egui::Button::new("Cancel warp"))
                    .clicked()
            {
                self.send(Request::Navigate {
                    mode: NavigationMode::Stop,
                });
            }
        });
        ui.small("Stow stops firing. Warp also requires full carrier departure admission. Monitor controls are independent.");
    }
}
