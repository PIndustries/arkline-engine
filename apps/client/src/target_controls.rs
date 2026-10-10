//! Server-confirmed target rack and utility drone controls.
use super::*;
impl Cockpit {
    pub(super) fn confirmed_locks(p: &PilotView) -> Vec<u64> {
        if p.locked_asteroids.is_empty() {
            p.locked_asteroid.into_iter().collect()
        } else {
            p.locked_asteroids.clone()
        }
    }
    pub(super) fn pending_locks(p: &PilotView) -> Vec<TargetLock> {
        if p.target_locks.is_empty() {
            p.target_lock.iter().cloned().collect()
        } else {
            p.target_locks.clone()
        }
    }
    pub(super) fn asteroid_name(&self, id: u64) -> String {
        self.snapshot
            .as_ref()
            .and_then(|s| s.asteroids.iter().find(|a| a.id == id))
            .map_or_else(|| format!("Asteroid #{id}"), |a| a.name.clone())
    }
    pub(super) fn module_target(&self, p: &PilotView) -> Option<u64> {
        let locks = Self::confirmed_locks(p);
        if let SelectedObject::Asteroid(id) = self.selected_object {
            if locks.contains(&id) {
                return Some(id);
            }
        }
        self.mining_target
            .filter(|id| locks.contains(id))
            .or_else(|| locks.first().copied())
    }
    pub(super) fn target_chips(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.own() else { return };
        let confirmed = Self::confirmed_locks(&p);
        let pending = Self::pending_locks(&p);
        ui.small(format!(
            "TARGET RACK · {}/{}",
            confirmed.len() + pending.len(),
            MAX_TARGET_LOCKS
        ));
        for id in confirmed {
            ui.push_id(("locked", id), |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .selectable_label(
                            self.module_target(&p) == Some(id),
                            format!("Locked · {}", self.asteroid_name(id)),
                        )
                        .clicked()
                    {
                        self.mining_target = Some(id);
                        self.selected_object = SelectedObject::Asteroid(id);
                    }
                    if ui
                        .add_enabled(self.can_operate(), egui::Button::new("Unlock").small())
                        .clicked()
                    {
                        self.send(Request::UnlockAsteroid { id });
                    }
                })
            });
        }
        for lock in pending {
            ui.push_id(("acquiring", lock.asteroid_id), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.small(format!(
                        "Acquiring {} · {:.1}s",
                        self.asteroid_name(lock.asteroid_id),
                        lock.completes_at_ms
                            .saturating_sub(self.presentation_time())
                            as f64
                            / 1000.0
                    ));
                    if ui
                        .add_enabled(self.can_operate(), egui::Button::new("Cancel").small())
                        .clicked()
                    {
                        self.send(Request::UnlockAsteroid {
                            id: lock.asteroid_id,
                        });
                    }
                })
            });
        }
    }
    pub(super) fn drone_destination(&self, pilot: &PilotView) -> Option<[f64; 3]> {
        let mut destination = self.selected_position()?;
        if let Some(asteroid) = self.selected_asteroid() {
            let origin = pilot
                .drone
                .as_ref()
                .map(|d| d.position)
                .or_else(|| pilot.flight.as_ref().map(|f| f.position))?;
            let mut delta =
                std::array::from_fn::<_, 3, _>(|axis| origin[axis] - asteroid.position[axis]);
            let mut length = distance_m(origin, asteroid.position);
            if length < 0.001 {
                if let Some(flight) = &pilot.flight {
                    delta =
                        std::array::from_fn(|axis| flight.position[axis] - asteroid.position[axis]);
                    length = distance_m(flight.position, asteroid.position);
                }
            }
            if length < 0.001 {
                delta = [1.0, 0.0, 0.0];
                length = 1.0;
            }
            destination = std::array::from_fn(|axis| {
                asteroid.position[axis] + delta[axis] / length * (asteroid.radius + 100.0)
            });
        }
        pilot
            .flight
            .as_ref()
            .filter(|f| distance_m(f.position, destination) <= DRONE_MAX_RANGE)
            .map(|_| destination)
    }
    pub(super) fn drone_controls(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.own() else { return };
        ui.separator();
        ui.small("UTILITY DRONE");
        let available = self.can_fly();
        if let Some(drone) = &p.drone {
            ui.small(format!(
                "{} · {} from ship",
                match drone.mode {
                    DroneMode::Hold => "Holding",
                    DroneMode::FlyTo { .. } => "Flying to point",
                    DroneMode::Returning => "Returning",
                },
                format_distance(
                    p.flight
                        .as_ref()
                        .map_or(0.0, |f| distance_m(f.position, drone.position))
                )
            ));
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(available, egui::Button::new("Recall drone").small())
                    .clicked()
                {
                    self.send(Request::RecallDrone);
                }
                if ui
                    .add_enabled(available, egui::Button::new("Hold drone").small())
                    .clicked()
                {
                    self.send(Request::DroneHold);
                }
                let destination = self.drone_destination(&p);
                if ui
                    .add_enabled(
                        available && destination.is_some(),
                        egui::Button::new("Drone to selected").small(),
                    )
                    .clicked()
                {
                    self.send(Request::DroneNavigate {
                        position: destination.unwrap(),
                    });
                }
            });
            ui.checkbox(&mut self.drone_camera, "Drone camera on cockpit screen");
        } else if ui
            .add_enabled(available, egui::Button::new("Launch drone").small())
            .clicked()
        {
            self.send(Request::LaunchDrone);
        }
        ui.small(format!(
            "Control radius {} · navigation follows selected object",
            format_distance(DRONE_MAX_RANGE)
        ));
    }
}
