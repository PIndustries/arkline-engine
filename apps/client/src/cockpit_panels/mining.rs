//! Mining module presentation and its existing command controls.

use super::*;

impl Cockpit {
    pub(crate) fn mining_module(&mut self, ui: &mut egui::Ui) {
        ui.small(
            egui::RichText::new("MINING MODULE")
                .strong()
                .color(self.accent()),
        );
        self.manual_operations_summary(ui);
        self.parallel_physical_turret_controls(ui);
        let fresh = self.operations_fresh();
        let seated = self
            .own()
            .is_some_and(|pilot| pilot.station_avatar.is_none());
        ui.add_enabled_ui(seated, |ui| self.target_chips(ui));
        let (status, progress) = self.mining_status();
        ui.small(status);
        if let Some(progress) = progress {
            ui.add(
                egui::ProgressBar::new(progress)
                    .desired_height(4.0)
                    .fill(self.accent()),
            );
        }
        if let Some(pilot) = self.own() {
            let spec = mining_spec(pilot.ship_class);
            let cost = power_spec(pilot.ship_class).mining_cycle_cost_j;
            ui.colored_label(
                if pilot.power.as_ref().is_some_and(|p| p.charge_j < cost) {
                    AMBER
                } else {
                    MUTED
                },
                format!(
                    "Capacitor {cost} J / turret cycle{}",
                    if pilot.power.as_ref().is_some_and(|p| p.charge_j < cost) {
                        " · low charge"
                    } else {
                        ""
                    }
                ),
            );
            ui.small(format!(
                "Up to {} ore / {:.1}s per turret · repeats until stopped",
                spec.yield_units,
                spec.cycle_ms as f64 / 1000.0
            ));
            if pilot.mining_modules.is_empty() {
                ui.small("Waiting for mining module state…");
            }
            for module in &pilot.mining_modules {
                ui.push_id(module.module_id, |ui| {
                    let active = module.target_id.is_some();
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(
                            if active { self.accent() } else { MUTED },
                            format!(
                                "{} TURRET {} · {}",
                                if active { "●" } else { "○" },
                                module.module_id + 1,
                                if module.module_id == 0 {
                                    "LEFT"
                                } else {
                                    "RIGHT"
                                }
                            ),
                        );
                        if active {
                            ui.small(self.asteroid_name(module.target_id.unwrap()));
                            let remaining = module
                                .cycle_completes_at_ms
                                .unwrap_or(0)
                                .saturating_sub(self.presentation_time());
                            ui.small(if remaining == 0 {
                                "Confirming cycle…".into()
                            } else {
                                format!("{:.1}s", remaining as f64 / 1000.0)
                            });
                            if ui
                                .add_enabled(
                                    self.can_operate() && seated && fresh,
                                    egui::Button::new("Stop turret").small(),
                                )
                                .clicked()
                            {
                                self.send(Request::StopMining {
                                    module_id: module.module_id,
                                });
                            }
                        } else if ui
                            .add_enabled(
                                self.can_mine(&pilot),
                                egui::Button::new("Activate turret").small(),
                            )
                            .clicked()
                        {
                            self.start_module(module.module_id);
                        }
                    });
                    if let (Some(start), Some(end)) =
                        (module.cycle_started_at_ms, module.cycle_completes_at_ms)
                    {
                        let progress = (self.presentation_time().saturating_sub(start) as f32
                            / end.saturating_sub(start).max(1) as f32)
                            .clamp(0.0, 1.0);
                        ui.add(
                            egui::ProgressBar::new(progress)
                                .desired_height(4.0)
                                .fill(self.accent()),
                        );
                    } else if let Some(reason) = &module.stop_reason {
                        ui.small(reason);
                    }
                });
            }
            if self.selected_asteroid().is_some()
                && ui
                    .add_enabled(self.can_lock_selected(), egui::Button::new("Lock").small())
                    .clicked()
            {
                if let Some(a) = self.selected_asteroid() {
                    self.send(Request::LockAsteroid { id: a.id });
                }
            }
        }
        if let Some(pilot) = self.own() {
            if (!Self::pending_locks(&pilot).is_empty()
                || !Self::confirmed_locks(&pilot).is_empty())
                && ui
                    .add_enabled(
                        self.can_operate() && seated && fresh,
                        egui::Button::new(
                            if Self::pending_locks(&pilot).len()
                                + Self::confirmed_locks(&pilot).len()
                                > 1
                            {
                                "Unlock all targets"
                            } else if pilot.target_lock.is_some() {
                                "Cancel lock"
                            } else {
                                "Unlock target"
                            },
                        )
                        .small(),
                    )
                    .clicked()
            {
                self.send(Request::UnlockTarget);
            }
        }
    }
}
