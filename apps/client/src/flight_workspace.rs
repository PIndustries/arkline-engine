//! Compact flight workspace chrome. All readouts use authority snapshots;
//! opening a workspace never issues a gameplay command.
use super::*;

impl Cockpit {
    pub(super) fn flight_workspace_navigation(&mut self, ui: &mut egui::Ui) {
        let previous = self.view_mode;
        let previous_foot = self.hangar_view.on_foot_view;
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("VIEW").small().color(MUTED));
            // Reuse the canonical selector so Local/System map scope, reset,
            // ship focus and on-foot first/third-person controls stay reachable.
            self.view_selector(ui);
            if self.own().is_some_and(|pilot| pilot.station_avatar.is_some()) {
                ui.small("On foot · return to pilot seat for ship cameras")
                    .on_hover_text("Walk to the physical pilot chair and choose Return to pilot seat to use 2D, 3D and Cockpit ship views.");
            }
            if !self.embedded {
                ui.separator();
                self.flight_workspace_shortcuts(ui);
            }
        });
        if self.view_mode != previous || self.hangar_view.on_foot_view != previous_foot {
            self.release_workspace_input();
        }
    }

    pub(super) fn flight_workspace_shortcuts(&mut self, ui: &mut egui::Ui) {
        for (label, tab) in [
            ("Operations", ServiceTab::ShipComputer),
            ("Ship", ServiceTab::Cargo),
        ] {
            if ui.button(label).clicked() {
                self.service_tab = tab;
                self.services_open = true;
            }
        }
    }

    pub(super) fn flight_workspace_status(&self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let Some(snapshot) = self.snapshot.as_ref() else {
                ui.colored_label(
                    AMBER,
                    if self.connected {
                        "Synchronizing world"
                    } else {
                        "Disconnected · no world data"
                    },
                );
                return;
            };
            let age = self
                .presentation_time()
                .saturating_sub(snapshot.server_time_ms);
            let (authority, color) = if !self.connected {
                ("Disconnected · last known state".to_owned(), AMBER)
            } else if self.embedded_paused {
                ("Paused · simulation held".to_owned(), AMBER)
            } else if age > 1500 {
                (
                    format!("Stale · {:.1}s since world update", age as f64 / 1000.0),
                    AMBER,
                )
            } else {
                ("Live".to_owned(), self.accent())
            };
            ui.colored_label(color, authority);
            if self.request_pending {
                ui.small("Command pending");
            }
            let Some(pilot) = self.own() else {
                ui.small("Waiting for vessel");
                return;
            };
            ui.separator();
            let active_job = snapshot
                .resource_jobs
                .iter()
                .find(|job| job.pilot == pilot.name && !job.phase.is_terminal());
            let ownership = if pilot.station_avatar.is_some() {
                "On foot · manual flight locked"
            } else if self.manual_remote.is_some() {
                "Remote vessel control"
            } else if active_job.is_some() {
                "Pilot seat · mining autopilot"
            } else {
                "Pilot seat"
            };
            ui.label(ownership);
            if !self.embedded_input {
                ui.small("Viewport input inactive");
            }
            if pilot.location != Location::Docked && pilot.station_avatar.is_some() {
                ui.small("Exterior access locked");
            }
            ui.separator();
            if let Some(flight) = &pilot.flight {
                let velocity = flight.warp.as_ref().map_or(flight.velocity, |warp| {
                    arkline_protocol::sample_warp(warp, self.presentation_time()).1
                });
                let speed = velocity.iter().map(|v| v * v).sum::<f64>().sqrt();
                if speed.is_finite() {
                    ui.label(format!("{speed:.0} m/s"));
                }
                let mode = match &flight.mode {
                    NavigationMode::Stop => "Holding",
                    NavigationMode::Manual => "Manual flight",
                    NavigationMode::Approach { .. } => "Approaching",
                    NavigationMode::Orbit { .. } => "Orbiting",
                    NavigationMode::KeepRange { .. } => "Keeping range",
                    NavigationMode::FlyTo { .. } => "Flying to waypoint",
                };
                ui.small(match pilot.location {
                    Location::InTransit { .. } => "Warp transit",
                    Location::Jumping { .. } => "Gate transit",
                    Location::Docked => "Docked",
                    _ => mode,
                });
            } else if pilot.location == Location::Docked {
                ui.small("Docked");
            }
            ui.label(format!(
                "Hold {} / {}",
                pilot.ore,
                cargo_capacity(pilot.ship_class)
            ));
            if let Some(job) = active_job {
                ui.separator();
                ui.label(format!(
                    "Mining · stored {} / {}",
                    job.stored_units, job.target_units
                ));
                ui.small(match job.phase {
                    ResourceJobPhase::Undocking => "Undocking",
                    ResourceJobPhase::TravelToBelt => "Flying to belt",
                    ResourceJobPhase::AcquiringTarget => "Acquiring ore",
                    ResourceJobPhase::Mining => "Mining",
                    ResourceJobPhase::ReturningToStation => "Returning to station",
                    ResourceJobPhase::Docking => "Docking",
                    ResourceJobPhase::Depositing => "Depositing ore",
                    _ => "Routine updated",
                });
            }
            if let Some(position) = self.selected_position() {
                let label = match &self.selected_object {
                    SelectedObject::Station => "Station".to_owned(),
                    SelectedObject::Belt => "Asteroid belt".to_owned(),
                    SelectedObject::Asteroid(id) => format!("Asteroid #{id}"),
                    SelectedObject::Gate(id) => format!("Gate #{id}"),
                    SelectedObject::Planet(id) => format!("Planet #{id}"),
                    SelectedObject::Prop(id) => format!("Object #{id}"),
                    SelectedObject::Pilot(name) => name.clone(),
                };
                let range =
                    distance_m(motion::position(&pilot, self.presentation_time()), position);
                if range.is_finite() {
                    ui.separator();
                    ui.label(format!("Target {label} · {}", format_distance(range)));
                }
            }
        });
    }
}
