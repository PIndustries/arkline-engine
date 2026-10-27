//! Navigation controls and local contact overview presentation.

use super::*;

impl Cockpit {
    pub(crate) fn navigation_controls(&mut self, ui: &mut egui::Ui) {
        if let Some(docking) = self
            .own()
            .and_then(|p| p.flight.as_ref()?.docking.clone())
            .filter(|d| d.phase.busy())
        {
            ui.strong(docking.phase.label());
            if let Some(reason) = &docking.hold_reason {
                ui.small(reason);
            }
            let held = docking.phase == DockingPhase::Held;
            if ui
                .add_enabled(
                    self.can_operate() && self.live_command_snapshot_fresh(),
                    egui::Button::new(if held {
                        "Resume docking"
                    } else {
                        "Hold docking"
                    }),
                )
                .clicked()
            {
                self.send(if held {
                    Request::ResumeDocking {
                        operation_id: docking.operation_id,
                    }
                } else {
                    Request::CancelDocking {
                        operation_id: docking.operation_id,
                    }
                });
            }
        }
        let readiness = self.navigation_order_reason();
        let fresh = readiness.is_none();
        if let Some(reason) = readiness {
            ui.colored_label(AMBER, reason);
        }
        if let Some(reason) = self.bay_departure_reason() {
            ui.colored_label(AMBER, reason);
        }
        ui.add_enabled_ui(fresh, |ui| {
        if matches!(
            self.selected_object,
            SelectedObject::Planet(_) | SelectedObject::Prop(_)
        ) {
            if let Some(mut position) = self.selected_position() {
                let planet = matches!(self.selected_object, SelectedObject::Planet(_));
                if !planet {
                    position[1] += 100.0;
                }
                if ui
                    .add_enabled(
                        self.can_maneuver(),
                        egui::Button::new(if planet {
                            "Approach planet"
                        } else {
                            "Fly above prop"
                        }),
                    )
                    .clicked()
                {
                    self.send(Request::Navigate {
                        mode: NavigationMode::FlyTo { position },
                    });
                }
            }
        }
        let target = self.selected_target();
        let enabled = self.can_maneuver() && target.is_some();
        if let Some(own) = self.own() {
            if let Some(position) = self.selected_position() {
                let own_position = motion::position(&own, self.presentation_time());
                ui.colored_label(
                    self.accent(),
                    if let Some(asteroid) = self.selected_asteroid() {
                        format!(
                            "Surface {}",
                            format_distance(
                                (distance_m(own_position, position) - asteroid.radius).max(0.0)
                            )
                        )
                    } else {
                        format!(
                            "Range {}",
                            format_distance(distance_m(own_position, position))
                        )
                    },
                );
            }
        }
        ui.horizontal(|ui| {
            ui.label("Range");
            egui::ComboBox::from_id_salt("nav-range")
                .selected_text(format_distance(self.navigation_range))
                .show_ui(ui, |ui| {
                    for range in [100.0, 500.0, 1000.0] {
                        ui.selectable_value(
                            &mut self.navigation_range,
                            range,
                            format_distance(range),
                        );
                    }
                });
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(enabled, egui::Button::new("Approach"))
                .clicked()
            {
                self.send(Request::Navigate {
                    mode: NavigationMode::Approach {
                        target: target.clone().unwrap(),
                    },
                });
            }
            if ui
                .add_enabled(enabled, egui::Button::new("Orbit"))
                .clicked()
            {
                self.send(Request::Navigate {
                    mode: NavigationMode::Orbit {
                        target: target.clone().unwrap(),
                        radius: self.navigation_range,
                    },
                });
            }
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(enabled, egui::Button::new("Keep range"))
                .clicked()
            {
                self.send(Request::Navigate {
                    mode: NavigationMode::KeepRange {
                        target: target.clone().unwrap(),
                        distance: self.navigation_range,
                    },
                });
            }
            if ui
                .add_enabled(self.can_fly(), egui::Button::new("Stop"))
                .clicked()
            {
                self.send(Request::Navigate {
                    mode: NavigationMode::Stop,
                });
            }
        });
        let destination = match self.selected_object {
            SelectedObject::Station => Some(Destination::Station),
            SelectedObject::Belt | SelectedObject::Asteroid(_) => Some(Destination::Belt),
            SelectedObject::Gate(id) => Some(Destination::Gate { id }),
            _ => None,
        };
        if let Some(destination) = destination {
            if let Some(pilot) = self.own() {
                let cost = power_spec(pilot.ship_class).warp_cost_j;
                ui.colored_label(
                    if self.warp_power_ready() {
                        MUTED
                    } else {
                        AMBER
                    },
                    format!(
                        "Warp {:.1} kJ{}",
                        cost as f64 / 1000.0,
                        if self.warp_power_ready() {
                            ""
                        } else {
                            " · insufficient charge; recharging"
                        }
                    ),
                );
            }
            if ui
                .add_enabled(
                    self.can_warp(),
                    egui::Button::new(
                        if matches!(self.selected_object, SelectedObject::Asteroid(_)) {
                            "Warp to belt"
                        } else {
                            "Warp to selected"
                        },
                    ),
                )
                .clicked()
            {
                self.send(Request::Warp { destination });
            }
        }
        if self.selected_object == SelectedObject::Station {
            let can_dock = self.own().is_some_and(|p| self.can_dock(&p));
            if ui
                .add_enabled(can_dock, egui::Button::new("Dock"))
                .clicked()
            {
                self.send(Request::Dock);
            }
            ui.small(format!(
                "Docking port access: {} from hull envelope",
                format_distance(DOCK_RANGE)
            ));
        }
        if let Some(asteroid) = self.selected_asteroid().cloned() {
            if ui.small_button("Mining modules").clicked() {
                self.service_tab = ServiceTab::Modules;
                self.services_open = true;
            }
            let locked = self
                .own()
                .is_some_and(|p| Self::confirmed_locks(&p).contains(&asteroid.id));
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        self.can_lock_selected(),
                        egui::Button::new(if locked { "Locked" } else { "Lock target" }),
                    )
                    .clicked()
                {
                    self.send(Request::LockAsteroid { id: asteroid.id });
                }
            });
            ui.small(format!(
                "Surface range ≤ {} to extract",
                format_distance(MINING_RANGE)
            ));
        }
        if let Some(gate) = self.selected_gate() {
            if ui
                .add_enabled(
                    self.can_jump_selected(),
                    egui::Button::new(format!(
                        "Jump to {}",
                        system_name(gate.destination_system_id)
                    )),
                )
                .clicked()
            {
                self.send(Request::Jump { gate_id: gate.id });
            }
            ui.small(format!(
                "Gate activation range {}",
                format_distance(GATE_RANGE)
            ));
        }
        if matches!(&self.selected_object, SelectedObject::Pilot(name) if name == self.controlled_name())
        {
            ui.small("Select another vessel or a location to navigate.");
        }
        ui.small(
            if self
                .own()
                .is_some_and(|pilot| pilot.station_avatar.is_some())
            {
                "Read-only navigation · selecting a contact does not move your body or ship"
            } else {
                "Ctrl+Space: stop · RMB: aim · Release controls: assisted braking"
            },
        );
        });
    }
    pub(crate) fn overview(&mut self, ui: &mut egui::Ui) {
        let system = self.current_system();
        ui.label(
            egui::RichText::new(system_name(system))
                .size(19.0)
                .color(self.accent())
                .strong(),
        );
        ui.small(if !self.connected {
            "DISCONNECTED · last reported contacts"
        } else if self.embedded_paused {
            "PAUSED · last reported local contacts"
        } else if !self.live_command_snapshot_fresh() {
            "STALE · waiting for current contact telemetry"
        } else {
            "LOCAL CONTACTS · authoritative snapshot"
        });
        ui.separator();
        ui.label(
            egui::RichText::new("SELECTED OBJECT")
                .size(11.0)
                .color(MUTED),
        );
        ui.small(format!(
            "Selection · {}",
            overview_contacts::identity(&self.selected_object)
        ));
        match self.selected_object.clone() {
            SelectedObject::Station => {
                ui.heading("Arkline Station");
                ui.small(format!(
                    "{} span · {} high",
                    format_distance(STATION_DIAMETER_M),
                    format_distance(STATION_HEIGHT_M)
                ));
                ui.small("Range is measured to the docking port");
            }
            SelectedObject::Planet(id) => {
                if let Some(planet) = self.scene_planets().iter().find(|p| p.id == id) {
                    ui.heading(&planet.name);
                    ui.small("Approach in normal flight, then descend continuously through the atmosphere.");
                }
                self.surface_status(ui);
            }
            SelectedObject::Prop(id) => {
                if let Some(prop) = self
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.surface_props.iter().find(|p| p.id == id))
                {
                    ui.heading(format!("{:?} #{id}", prop.kind));
                    ui.small(format!("Owned by {}", prop.owner));
                }
                if ui.button("Surface sandbox").clicked() {
                    self.service_tab = ServiceTab::Sandbox;
                    self.services_open = true;
                }
            }
            SelectedObject::Belt => {
                ui.heading("Asteroid field");
                ui.small("Select an individual asteroid, lock it, then extract.");
            }
            SelectedObject::Asteroid(id) => {
                if let Some(a) = self.selected_asteroid() {
                    ui.heading(&a.name);
                    if let Some(v) = self
                        .physical_cargo
                        .as_ref()
                        .filter(|v| v.state.binding.target_id == id)
                    {
                        ui.colored_label(
                            if v.state.projection.resource_remaining_cm3.0 > 0 {
                                self.accent()
                            } else {
                                AMBER
                            },
                            format!(
                                "{} remaining · #{}",
                                crate::physical_cargo_ui::litres(
                                    v.state.projection.resource_remaining_cm3.0
                                ),
                                id
                            ),
                        );
                    } else {
                        ui.colored_label(
                            if a.ore > 0 { self.accent() } else { AMBER },
                            format!("{} ore remaining · #{}", a.ore, id),
                        );
                    }
                    if self
                        .own()
                        .is_some_and(|p| Self::confirmed_locks(&p).contains(&id))
                    {
                        ui.colored_label(self.accent(), "TARGET LOCK CONFIRMED");
                    }
                }
            }
            SelectedObject::Gate(_) => {
                if let Some(g) = self.selected_gate() {
                    ui.heading(&g.name);
                    ui.small(format!("Route to {}", system_name(g.destination_system_id)));
                }
            }
            SelectedObject::Pilot(name) => {
                ui.heading(&name);
                if let Some(p) = self
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.pilots.iter().find(|p| p.name == name))
                {
                    ui.small(location_label(&p.location));
                    ui.small(format!(
                        "{} · {} hull",
                        p.ship_class.spec().name,
                        format_distance(p.ship_class.spec().length_m)
                    ));
                    let online = self
                        .snapshot
                        .as_ref()
                        .is_some_and(|s| s.online_pilots.contains(&name));
                    ui.small(if online {
                        "Connected"
                    } else {
                        "Disconnected; vessel persists"
                    });
                }
            }
        }
        self.navigation_controls(ui);
        ui.separator();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.overview_filter, OverviewFilter::All, "All");
            ui.selectable_value(&mut self.overview_filter, OverviewFilter::Mining, "Mining");
            ui.selectable_value(&mut self.overview_filter, OverviewFilter::Travel, "Travel");
            ui.selectable_value(&mut self.overview_filter, OverviewFilter::Pilots, "Pilots");
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.overview_query)
                    .hint_text("Search contacts · name, type or ID")
                    .char_limit(128)
                    .desired_width(240.0),
            );
            if ui
                .add_enabled(!self.overview_query.is_empty(), egui::Button::new("Clear"))
                .clicked()
            {
                self.overview_query.clear();
            }
        });
        let mut rows: Vec<(SelectedObject, String, &str, Option<[f64; 3]>)> = vec![];
        if matches!(
            self.overview_filter,
            OverviewFilter::All | OverviewFilter::Travel
        ) {
            rows.push((
                SelectedObject::Station,
                "Arkline Station".into(),
                "Station",
                Some(STATION_POSITION),
            ));
            rows.push((
                SelectedObject::Belt,
                "Asteroid field".into(),
                "Field",
                Some(BELT_POSITION),
            ));
            for planet in self
                .scene_planets()
                .iter()
                .filter(|p| p.system_id == system)
            {
                rows.push((
                    SelectedObject::Planet(planet.id),
                    planet.name.clone(),
                    "Planet",
                    Some(planet.approach_position),
                ));
            }
            for gate in gates().into_iter().filter(|g| g.system_id == system) {
                rows.push((
                    SelectedObject::Gate(gate.id),
                    gate.name,
                    "Gate",
                    Some(gate.position),
                ));
            }
        }
        if self.overview_filter == OverviewFilter::All {
            if let Some(snapshot) = &self.snapshot {
                for prop in snapshot
                    .surface_props
                    .iter()
                    .filter(|p| p.system_id == system)
                {
                    rows.push((
                        SelectedObject::Prop(prop.id),
                        format!("{:?} #{}", prop.kind, prop.id),
                        "Prop",
                        Some(prop.position),
                    ));
                }
            }
        }
        if matches!(
            self.overview_filter,
            OverviewFilter::All | OverviewFilter::Mining
        ) {
            if let Some(s) = &self.snapshot {
                for a in s.asteroids.iter().filter(|a| a.system_id == system) {
                    rows.push((
                        SelectedObject::Asteroid(a.id),
                        a.name.clone(),
                        if self.physical_cargo.as_ref().is_some_and(|v| {
                            v.state.binding.target_id == a.id
                                && v.state.projection.resource_remaining_cm3.0 > 0
                        }) {
                            "Bulk volume"
                        } else if a.ore > 0 {
                            "Ore"
                        } else {
                            "Empty"
                        },
                        Some(a.position),
                    ));
                }
            }
        }
        if matches!(
            self.overview_filter,
            OverviewFilter::All | OverviewFilter::Pilots
        ) {
            if let Some(s) = &self.snapshot {
                for p in s.pilots.iter().filter(|p| p.system_id == system) {
                    rows.push((
                        SelectedObject::Pilot(p.name.clone()),
                        p.name.clone(),
                        if p.location == Location::Docked {
                            "Docked"
                        } else {
                            "Ship"
                        },
                        p.flight
                            .as_ref()
                            .map(|_| motion::position(p, self.presentation_time())),
                    ));
                }
            }
        }
        let total = rows.len();
        let selected_in_category = rows.iter().any(|row| row.0 == self.selected_object);
        rows.retain(|(object, name, kind, _)| {
            overview_contacts::matches(&self.overview_query, object, name, kind)
        });
        ui.horizontal_wrapped(|ui| {
            ui.small(format!("{} / {} contacts", rows.len(), total));
            if !rows.iter().any(|row| row.0 == self.selected_object) {
                ui.small(if selected_in_category {
                    "Selection hidden by search · unchanged"
                } else {
                    "Selection outside this category or current local snapshot · unchanged"
                });
            }
        });
        if rows.is_empty() {
            ui.colored_label(
                MUTED,
                if self.overview_query.trim().is_empty() {
                    "No contacts reported in this category."
                } else {
                    "No matching contacts · clear search or choose another category."
                },
            );
        }
        let own_position = self
            .own()
            .map(|p| motion::position(&p, self.presentation_time()));
        rows.sort_by(|a, b| {
            let da = own_position
                .zip(a.3)
                .map_or(f64::INFINITY, |(own, p)| distance_m(own, p));
            let db = own_position
                .zip(b.3)
                .map_or(f64::INFINITY, |(own, p)| distance_m(own, p));
            let ordering = da.total_cmp(&db).then_with(|| a.1.cmp(&b.1));
            if self.overview_nearest_first {
                ordering
            } else {
                ordering.reverse()
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("overview-table")
            .show(ui, |ui| {
                egui::Grid::new("overview-grid")
                    .striped(true)
                    .spacing([8.0, 3.0])
                    .show(ui, |ui| {
                        ui.small("NAME");
                        ui.small("TYPE");
                        if ui
                            .small_button(if self.overview_nearest_first {
                                "RANGE ↑"
                            } else {
                                "RANGE ↓"
                            })
                            .on_hover_text("Sort by distance")
                            .clicked()
                        {
                            self.overview_nearest_first = !self.overview_nearest_first;
                        }
                        ui.end_row();
                        for (object, name, kind, position) in rows {
                            if overview_contacts::button(
                                ui,
                                &object,
                                self.selected_object == object,
                                &name,
                            )
                            .clicked()
                            {
                                self.selected_object = object;
                            }
                            ui.small(kind);
                            ui.small(
                                own_position
                                    .zip(position)
                                    .map(|(a, b)| format_distance(distance_m(a, b)))
                                    .unwrap_or_else(|| "—".into()),
                            );
                            ui.end_row();
                        }
                    });
            });
    }
}
