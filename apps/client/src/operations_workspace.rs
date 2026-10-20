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
                self.service_tab = ServiceTab::Navigation;
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
        if !self.connected {
            ui.colored_label(
                AMBER,
                "Disconnected · showing last received authority state; commands unavailable",
            );
        } else if self.embedded && self.embedded_paused {
            ui.colored_label(
                MUTED,
                "Simulation paused · values are frozen at the authority clock",
            );
        } else if self.snapshot.as_ref().is_none_or(|snapshot| {
            self.presentation_time()
                .saturating_sub(snapshot.server_time_ms)
                > 1500
        }) {
            ui.colored_label(
                AMBER,
                "Waiting for a current authority snapshot · shown values may be stale",
            );
        }
        if self.request_pending {
            ui.colored_label(AMBER, "Command pending · awaiting server acknowledgement");
        }
        if !self.notice.is_empty() {
            ui.small(format!("Last authority response: {}", self.notice));
        }
        ui.separator();
    }
}
