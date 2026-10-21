//! Focused workspaces over existing authoritative services and commands.
use super::*;

fn group(tab: ServiceTab) -> usize {
    match tab {
        ServiceTab::Navigation | ServiceTab::Settings => 0,
        ServiceTab::Hangar
        | ServiceTab::Modules
        | ServiceTab::Cargo
        | ServiceTab::Layouts
        | ServiceTab::Sandbox => 1,
        ServiceTab::ShipComputer
        | ServiceTab::Drones
        | ServiceTab::Activity
        | ServiceTab::Ledger => 2,
        ServiceTab::Station | ServiceTab::Exchange | ServiceTab::Corporation => 3,
    }
}

impl Cockpit {
    pub(super) fn operations_fresh(&self) -> bool {
        self.connected
            && self.snapshot.as_ref().is_some_and(|snapshot| {
                self.presentation_time()
                    .saturating_sub(snapshot.server_time_ms)
                    <= 1500
            })
    }

    pub(super) fn manual_operations_summary(&self, ui: &mut egui::Ui) {
        let Some(ship) = self.own() else {
            return;
        };
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("CONTROL OWNERSHIP");
                ui.label(format!("Ship · {}", ship.name));
            });
            if let Some(job) = self.snapshot.as_ref().and_then(|snapshot| {
                snapshot
                    .resource_jobs
                    .iter()
                    .find(|job| job.pilot == ship.name && !job.phase.is_terminal())
            }) {
                ui.colored_label(
                    AMBER,
                    format!(
                        "Routine #{} active · manual orders cancel this mining routine",
                        job.id
                    ),
                );
                ui.small("Viewing telemetry or selecting a drone does not take control.");
            } else {
                ui.small("Manual operations · no active mining routine reported for this ship");
            }
            let reason = if !self.connected {
                Some("Disconnected · commands unavailable")
            } else if self.embedded_paused {
                Some("Paused · resume simulation to issue commands")
            } else if !self.operations_fresh() {
                Some("Stale telemetry · waiting for authority before issuing commands")
            } else if self.request_pending {
                Some("Command pending · waiting for authority confirmation")
            } else if ship.station_avatar.is_some() {
                Some("On foot · manual ship orders require the pilot seat")
            } else {
                None
            };
            if let Some(reason) = reason {
                ui.colored_label(MUTED, reason);
            }
        });
    }
    pub(super) fn operations_route(&self) -> &'static str {
        ["Flight", "Ship", "Operations", "Station"][group(self.service_tab)]
    }

    pub(super) fn operations_title(&self) -> &'static str {
        match self.service_tab {
            ServiceTab::Navigation => "Flight · Navigation",
            ServiceTab::Settings => "Flight · Preferences",
            ServiceTab::Hangar => "Ship · Fleet",
            ServiceTab::Modules => "Ship · Modules",
            ServiceTab::Cargo => "Ship · Cargo & storage",
            ServiceTab::Layouts => "Ship · Layouts",
            ServiceTab::Sandbox => "Ship · Surface tools",
            ServiceTab::ShipComputer => "Operations · Mining",
            ServiceTab::Drones => "Operations · Drones",
            ServiceTab::Activity => "Operations · Activity",
            ServiceTab::Ledger => "Operations · Audit",
            ServiceTab::Station => "Station · Services",
            ServiceTab::Exchange => "Station · Exchange",
            ServiceTab::Corporation => "Station · Corporation",
        }
    }

    pub(super) fn operations_navigation(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("← Back to Flight").clicked() {
                self.services_open = false;
                self.overview_open = false;
                self.service_tab = ServiceTab::Navigation;
                self.release_workspace_input();
            }
            ui.separator();
            for (index, label, default) in [
                (0, "Flight", ServiceTab::Navigation),
                (1, "Ship", ServiceTab::Hangar),
                (2, "Operations", ServiceTab::ShipComputer),
                (3, "Station", ServiceTab::Station),
            ] {
                if ui
                    .selectable_label(group(self.service_tab) == index, label)
                    .clicked()
                {
                    self.service_tab = default;
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in [
                (ServiceTab::Navigation, "Navigation"),
                (ServiceTab::Settings, "Preferences"),
                (ServiceTab::Hangar, "Fleet"),
                (ServiceTab::Modules, "Modules"),
                (ServiceTab::Cargo, "Cargo & storage"),
                (ServiceTab::Layouts, "Layouts"),
                (ServiceTab::Sandbox, "Surface tools"),
                (ServiceTab::ShipComputer, "Mining"),
                (ServiceTab::Drones, "Drones"),
                (ServiceTab::Activity, "Activity"),
                (ServiceTab::Ledger, "Audit"),
                (ServiceTab::Station, "Services"),
                (ServiceTab::Exchange, "Exchange"),
                (ServiceTab::Corporation, "Corporation"),
            ] {
                if group(tab) == group(self.service_tab)
                    && ui
                        .selectable_label(self.service_tab == tab, label)
                        .clicked()
                {
                    self.service_tab = tab;
                    self.sound.cue(sound_engine::Cue::Ui);
                }
            }
        });
        // Mining owns its concise local authority state; avoid repeating its
        // pause, pending and response notices above the same summary.
        if self.service_tab != ServiceTab::ShipComputer {
            if !self.connected {
                ui.colored_label(
                    AMBER,
                    "Disconnected · showing last received state; controls unavailable",
                );
            } else if self.embedded && self.embedded_paused {
                ui.colored_label(
                    MUTED,
                    "Simulation paused · choose Resume simulation to operate ship controls",
                );
            } else if self.snapshot.as_ref().is_none_or(|snapshot| {
                self.presentation_time()
                    .saturating_sub(snapshot.server_time_ms)
                    > 1500
            }) {
                ui.colored_label(
                    AMBER,
                    "Waiting for a world update · shown values may be out of date",
                );
            }
            if self.request_pending {
                ui.colored_label(AMBER, "Command pending · awaiting confirmation");
            }
            if !self.notice.is_empty() {
                ui.small(format!("Last response: {}", self.notice));
            }
        }
        ui.separator();
    }
}
