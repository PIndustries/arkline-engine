//! Compact flight workspace chrome. All readouts use authority snapshots;
//! opening a workspace never issues a gameplay command.
use super::*;

/// Label only: freshness and command permissions remain owned by the surrounding
/// workspace. A confirmed docking phase must not look like manual "Holding".
fn flight_status_label(location: &Location, flight: &FlightState) -> &'static str {
    if let Some(docking) = flight.docking.as_ref().filter(|d| d.phase.busy()) {
        return docking.phase.label();
    }
    match location {
        Location::InTransit { .. } => "Warp transit",
        Location::Jumping { .. } => "Gate transit",
        Location::Docked => "Docked",
        _ => match flight.mode {
            NavigationMode::Stop => "Holding",
            NavigationMode::Manual => "Manual flight",
            NavigationMode::Approach { .. } => "Approaching",
            NavigationMode::Orbit { .. } => "Orbiting",
            NavigationMode::KeepRange { .. } => "Keeping range",
            NavigationMode::FlyTo { .. } => "Flying to waypoint",
        },
    }
}

impl Cockpit {
    /// The physical display reserves room for scrollable content. Full search
    /// and workspace navigation belong to the expanded operations panel.
    pub(super) fn cockpit_detail_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in [
                (ServiceTab::Navigation, "Navigation"),
                (ServiceTab::Modules, "Modules"),
                (ServiceTab::Drones, "Drones"),
                (ServiceTab::ShipComputer, "Mining"),
                (ServiceTab::Cargo, "Cargo"),
                (ServiceTab::Displays, "Displays"),
            ] {
                ui.selectable_value(&mut self.service_tab, tab, label);
            }
        });
    }

    /// Camera presentation is independent of physical seat/body authority.
    /// Ship cameras observe snapshots while on foot; they never grant controls.
    pub(super) fn flight_workspace_view_selector(&mut self, ui: &mut egui::Ui) {
        use flight3d::hangar::OnFootView;
        let previous = self.view_mode;
        let previous_foot = self.hangar_view.on_foot_view;
        let previous_observation = self.ship_observation;
        let owned = self.own().is_some();
        let on_foot = self
            .own()
            .is_some_and(|pilot| pilot.station_avatar.is_some());
        ui.scope(|ui| {
            ui.set_max_width(if self.services_open {
                ui.available_width().min(440.0)
            } else {
                440.0
            });
            ui.horizontal_wrapped(|ui| {
                if on_foot {
                    let body_label = if self.ship_observation { "Body view" }
                        else if self.hangar_view.on_foot_view == OnFootView::FirstPerson { "Body · First person" }
                        else { "Body · Third person" };
                    egui::ComboBox::from_id_salt("body-view-mode").selected_text(body_label).show_ui(ui, |ui| {
                        for (view, label) in [(OnFootView::FirstPerson, "First person"), (OnFootView::ThirdPerson, "Third person")] {
                            if ui.selectable_label(!self.ship_observation && self.hangar_view.on_foot_view == view, label).clicked() {
                                self.hangar_view.on_foot_view = view;
                                self.ship_observation = false;
                            }
                        }
                    });
                }
                ui.add_enabled_ui(owned, |ui| {
                    for (mode, label, description) in [
                        (ViewMode::Tactical, "2D", "2D Tactical · local and system map"),
                        (ViewMode::Tactical3d, "3D", "3D Tactical · spatial navigation view"),
                        (ViewMode::External, "External", "3D External · observe the ship"),
                        (ViewMode::Cockpit, "Cockpit", "Cockpit · ship instrument view"),
                    ] {
                        if ui.selectable_label((!on_foot || self.ship_observation) && self.view_mode == mode, label).on_hover_text(description).clicked() {
                            self.view_mode = mode;
                            self.ship_observation = on_foot;
                        }
                    }
                });
            });
            if on_foot && self.ship_observation {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Return to body").clicked() { self.ship_observation = false; }
                    ui.small("Observing ship · controls locked").on_hover_text("Your body stays in place. Return to body to walk, or physically reseat for manual flight.");
                });
            }
            if (!on_foot || self.ship_observation) && self.view_mode == ViewMode::Tactical {
                ui.menu_button("Map options", |ui| {
                    let previous_scope = self.tactical_system;
                    ui.selectable_value(&mut self.tactical_system, false, "Local");
                    ui.selectable_value(&mut self.tactical_system, true, "System");
                    if previous_scope != self.tactical_system || ui.button("Reset map").clicked() {
                        self.tactical_pan = [0.0; 2]; self.tactical_zoom = 1.0;
                    }
                    if ui.button("Focus my ship").clicked() {
                        self.tactical_system = false; self.tactical_pan = [0.0; 2]; self.tactical_zoom = 1.0;
                    }
                    ui.small("Wheel: zoom at cursor · drag empty space: pan");
                });
            }
        });
        if self.view_mode != previous
            || self.hangar_view.on_foot_view != previous_foot
            || self.ship_observation != previous_observation
        {
            self.release_workspace_input();
        }
    }

    pub(super) fn flight_workspace_navigation(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("VIEW").small().color(MUTED));
            self.flight_workspace_view_selector(ui);
            if !self.embedded {
                ui.separator();
                self.flight_workspace_shortcuts(ui);
            }
        });
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

    pub(super) fn flight_workspace_status(&mut self, ui: &mut egui::Ui) {
        let compact_on_foot =
            ui.available_width() < 1000. && self.own().is_some_and(|p| p.station_avatar.is_some());
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if compact_on_foot {
                    "Controls"
                } else {
                    "Controls & ownership"
                })
                .on_hover_text("Controls & ownership")
                .clicked()
            {
                self.open_controls_reference();
            }
            if ui
                .selectable_label(
                    self.performance_open,
                    if compact_on_foot {
                        "Perf"
                    } else {
                        "Performance"
                    },
                )
                .on_hover_text("Performance")
                .clicked()
            {
                self.performance_open = !self.performance_open;
            }
            ui.separator();
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
            let age = self.timeline_origin.map_or(u64::MAX, |origin| {
                origin
                    .saturating_add(self.observed.elapsed().as_millis() as u64)
                    .saturating_sub(snapshot.server_time_ms)
            });
            let (authority, color) = if !self.connected {
                ("Disconnected · last known state".to_owned(), AMBER)
            } else if self.embedded_paused {
                ("Paused · resume simulation to continue".to_owned(), AMBER)
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
                if compact_on_foot {
                    "On foot · Flight locked"
                } else {
                    "On foot · manual flight locked"
                }
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
                ui.small(if compact_on_foot {
                    "Exterior locked"
                } else {
                    "Exterior access locked"
                })
                .on_hover_text("Exterior access locked while the ship is undocked");
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
                ui.small(flight_status_label(&pilot.location, flight));
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
                    let target = format!("Target {label} · {}", format_distance(range));
                    if compact_on_foot {
                        // Secondary target details use the actual remaining row
                        // width; critical control/lock status stays fully visible.
                        let remaining = ui.available_size_before_wrap().x.max(1.);
                        ui.add_sized(
                            [remaining, ui.text_style_height(&egui::TextStyle::Body)],
                            egui::Label::new(&target).truncate(),
                        )
                        .on_hover_text(&target);
                    } else {
                        ui.label(target);
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod visual_strip_tests {
    use super::*;
    #[test]
    fn docking_status_uses_confirmed_phase_without_changing_legacy_labels_or_state() {
        let ctx = egui::Context::default();
        let cockpit = crate::tests::view_test_cockpit(&ctx);
        let mut flight = cockpit.snapshot.as_ref().unwrap().pilots[0]
            .flight
            .clone()
            .unwrap();
        assert_eq!(flight_status_label(&Location::Station, &flight), "Holding");
        flight.docking = Some(arkline_protocol::DockingState {
            version: arkline_protocol::DOCKING_VERSION,
            host_vessel_id: "fixture".into(),
            berth_id: 0,
            operation_id: 7,
            phase: DockingPhase::Free,
            segment: None,
            clamps: 0.,
            updated_at_ms: 1000,
            hold_reason: None,
            departing: false,
        });
        for phase in [
            DockingPhase::Approaching,
            DockingPhase::Aligning,
            DockingPhase::Berthing,
            DockingPhase::Clamping,
            DockingPhase::Releasing,
            DockingPhase::Departing,
            DockingPhase::Braking,
            DockingPhase::Held,
        ] {
            flight.docking.as_mut().unwrap().phase = phase;
            let before = serde_json::to_vec(&flight).unwrap();
            assert_eq!(
                flight_status_label(&Location::Station, &flight),
                phase.label()
            );
            assert_eq!(serde_json::to_vec(&flight).unwrap(), before);
        }
        flight.docking.as_mut().unwrap().phase = DockingPhase::Secured;
        assert_eq!(flight_status_label(&Location::Docked, &flight), "Docked");
        flight.docking.as_mut().unwrap().phase = DockingPhase::Free;
        flight.mode = NavigationMode::Manual;
        assert_eq!(
            flight_status_label(&Location::Station, &flight),
            "Manual flight"
        );
    }

    #[test]
    fn compact_view_strip_retains_routes_with_readable_type_at_both_viewport_sizes() {
        for width in [1280.0, 3000.0] {
            let ctx = egui::Context::default();
            let mut cockpit = crate::tests::view_test_cockpit(&ctx);
            cockpit.theme.apply(&ctx);
            cockpit.snapshot.as_mut().unwrap().pilots[0].station_avatar = Some(serde_json::from_value(serde_json::json!({"position":[0,0,0],"heading":0,"moving":false,"updated_at_ms":1000})).unwrap());
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(width, 800.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        cockpit.flight_workspace_view_selector(ui);
                    });
                },
            );
            let labels: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape {
                        Some((t.galley.job.text.clone(), t.pos, t.galley.size()))
                    } else {
                        None
                    }
                })
                .collect();
            for label in ["2D", "3D", "External", "Cockpit"] {
                let (_, pos, size) = labels.iter().find(|(text, _, _)| text == label).unwrap();
                assert!(
                    pos.x + size.x <= 460.0 && pos.y + size.y <= 140.0,
                    "{label} outside compact strip at {width}"
                );
            }
            assert!(ctx.style().text_styles[&egui::TextStyle::Button].size >= 14.0);
            assert!(cockpit.snapshot.as_ref().unwrap().pilots[0]
                .station_avatar
                .is_some());
        }
    }
}
