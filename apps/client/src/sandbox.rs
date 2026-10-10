//! Surface tools operate only on confirmed local terrain state and owned props.
use super::*;
impl Cockpit {
    pub(super) fn surface_status(&self, ui: &mut egui::Ui) {
        let Some(pilot) = self.own() else { return };
        if let Some(surface) = &pilot.surface {
            let planet = planets().into_iter().find(|p| p.id == surface.planet_id);
            let name = planet.as_ref().map_or("Planet", |p| p.name.as_str());
            let state = if surface.grounded {
                "LANDED"
            } else if planet
                .as_ref()
                .is_some_and(|p| surface.altitude_m <= p.atmosphere_height_m)
            {
                "ATMOSPHERE"
            } else {
                "ORBITAL SPACE"
            };
            ui.colored_label(
                self.accent(),
                format!(
                    "{name} · {state} · altitude {} · ground {}{}",
                    format_distance(surface.altitude_m.max(0.0)),
                    format_distance(surface.ground_clearance_m.max(0.0)),
                    if self.connected { "" } else { " · last known" }
                ),
            );
        }
    }
    fn surface_edit_ready(&self) -> bool {
        self.can_fly()
            && self
                .own()
                .is_some_and(|p| p.surface.is_some_and(|s| s.ground_clearance_m <= 100.0))
    }
    fn prop_position(&self, kind: SurfacePropKind) -> Option<[f64; 3]> {
        let pilot = self.own()?;
        let flight = pilot.flight?;
        let surface = pilot.surface?;
        let planet = planets().into_iter().find(|p| p.id == surface.planet_id)?;
        let offset = [
            flight.position[0] + self.prop_offset[0] - planet.center[0],
            flight.position[1] - planet.center[1],
            flight.position[2] + self.prop_offset[2] - planet.center[2],
        ];
        let length = distance_m(offset, [0.0; 3]);
        if length < 1.0 {
            return None;
        }
        let radius = planet.radius_m + kind.size()[1] * 0.5;
        let point = std::array::from_fn(|i| planet.center[i] + offset[i] / length * radius);
        (hull_surface_distance(pilot.ship_class, flight.position, point) <= 500.0).then_some(point)
    }
    pub(super) fn sandbox_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Surface sandbox");
        self.surface_status(ui);
        ui.small(
            "Descend to ≤100 m ground clearance. Edit owned props within 500 m of hull; maximum 32 props.",
        );
        if !self.surface_edit_ready() {
            ui.colored_label(
                AMBER,
                "Surface tools unavailable here. Approach the planet, then descend.",
            );
        }
        ui.horizontal_wrapped(|ui| {
            for (kind, label) in [
                (SurfacePropKind::Crate, "Crate"),
                (SurfacePropKind::Pylon, "Pylon"),
                (SurfacePropKind::Platform, "Platform"),
            ] {
                ui.selectable_value(&mut self.prop_kind, kind, label);
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (axis, label, range) in [(0, "East", -250.0..=250.0), (2, "North", -250.0..=250.0)]
            {
                ui.label(label);
                ui.add(
                    egui::DragValue::new(&mut self.prop_offset[axis])
                        .speed(1.0)
                        .range(range)
                        .suffix(" m"),
                );
            }
        });
        let own = self.own();
        let owned: Vec<_> = self.snapshot.as_ref().map_or(vec![], |s| {
            s.surface_props
                .iter()
                .filter(|p| p.owner == self.pilot)
                .cloned()
                .collect()
        });
        let position = self.prop_position(self.prop_kind);
        if ui
            .add_enabled(
                self.surface_edit_ready() && owned.len() < 32 && position.is_some(),
                egui::Button::new("Spawn selected prop"),
            )
            .clicked()
        {
            self.send(Request::SpawnSurfaceProp {
                kind: self.prop_kind,
                position: position.unwrap(),
            });
        }
        ui.separator();
        ui.small(format!("Owned props: {}/32", owned.len()));
        for prop in owned {
            ui.push_id(prop.id, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .selectable_label(
                            self.selected_object == SelectedObject::Prop(prop.id),
                            format!("{:?} #{}", prop.kind, prop.id),
                        )
                        .clicked()
                    {
                        self.selected_object = SelectedObject::Prop(prop.id);
                    }
                    let local = own.as_ref().is_some_and(|p| {
                        p.system_id == prop.system_id
                            && p.flight.as_ref().is_some_and(|f| {
                                hull_surface_distance(p.ship_class, f.position, prop.position)
                                    <= 500.0
                            })
                    });
                    let destination = self.prop_position(prop.kind);
                    if ui
                        .add_enabled(
                            self.surface_edit_ready() && local && destination.is_some(),
                            egui::Button::new("Move here").small(),
                        )
                        .clicked()
                    {
                        self.send(Request::MoveSurfaceProp {
                            id: prop.id,
                            position: destination.unwrap(),
                        });
                    }
                    if ui
                        .add_enabled(
                            self.surface_edit_ready() && local,
                            egui::Button::new("Delete prop").small(),
                        )
                        .clicked()
                    {
                        self.send(Request::RemoveSurfaceProp { id: prop.id });
                    }
                })
            });
        }
    }
}
