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
            let origin = self
                .selected_drone_view(pilot)
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
    pub(super) fn fleet(p: &PilotView) -> Vec<&DroneView> {
        if p.drones.is_empty() {
            p.drone.iter().collect()
        } else {
            p.drones.iter().collect()
        }
    }
    pub(super) fn selected_drone_view<'a>(&self, p: &'a PilotView) -> Option<&'a DroneView> {
        let fleet = Self::fleet(p);
        self.selected_drone
            .and_then(|id| fleet.iter().copied().find(|d| d.id == id))
            .or_else(|| fleet.first().copied())
    }
    pub(super) fn drone_controls(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.own() else { return };
        let available = self.can_fly();
        ui.small("DRONE FLEET / 5 PER CLASS");
        ui.horizontal_wrapped(|ui| {
            for (class, label) in [
                (DroneClass::Recon, "Recon"),
                (DroneClass::Mining, "Mining"),
                (DroneClass::Attack, "Attack"),
            ] {
                let count = Self::fleet(&p).iter().filter(|d| d.class == class).count();
                if ui
                    .add_enabled(
                        available && count < 5,
                        egui::Button::new(format!("Launch {label} {count}/5")).small(),
                    )
                    .clicked()
                {
                    self.send(Request::LaunchDroneClass { class });
                }
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("fleet_list")
            .max_height(160.0)
            .show(ui, |ui| {
                for drone in Self::fleet(&p) {
                    let selected = self
                        .selected_drone_view(&p)
                        .is_some_and(|d| d.id == drone.id);
                    if ui
                        .selectable_label(
                            selected,
                            format!(
                                "#{:02} {:?} · {} · {}% signal · {} HP",
                                drone.id,
                                drone.class,
                                drone_mode_label(&drone.mode),
                                (drone.signal_strength * 100.0).round(),
                                drone.hit_points
                            ),
                        )
                        .clicked()
                    {
                        self.selected_drone = Some(drone.id);
                    }
                }
            });
        let Some(drone) = self.selected_drone_view(&p) else {
            ui.small("No deployed drones. Launch a class above.");
            return;
        };
        let id = drone.id;
        let controlled = available && drone.signal_strength > 0.0;
        ui.small(format!(
            "Selected #{id} · {} from ship",
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
                self.send(Request::RecallDroneById { id });
            }
            if ui
                .add_enabled(controlled, egui::Button::new("Hold drone").small())
                .clicked()
            {
                self.send(Request::DroneHoldById { id });
            }
            let destination = self.drone_destination(&p);
            if ui
                .add_enabled(
                    controlled && destination.is_some(),
                    egui::Button::new("Drone to selected").small(),
                )
                .clicked()
            {
                self.send(Request::DroneNavigateById {
                    id,
                    position: destination.unwrap(),
                });
            }
            if drone.class == DroneClass::Mining {
                let target = self.module_target(&p);
                if ui
                    .add_enabled(
                        controlled && target.is_some(),
                        egui::Button::new("Mine locked target").small(),
                    )
                    .clicked()
                {
                    self.send(Request::AssignMiningDrone {
                        id,
                        target_id: target.unwrap(),
                    });
                }
            }
        });
        if drone.class == DroneClass::Attack {
            let enemies: Vec<_> = self
                .snapshot
                .as_ref()
                .into_iter()
                .flat_map(|s| &s.pilots)
                .filter(|other| other.name != self.pilot && other.system_id == p.system_id)
                .flat_map(|other| {
                    Self::fleet(other)
                        .into_iter()
                        .map(|d| (other.name.clone(), d.id))
                })
                .collect();
            egui::ComboBox::from_id_salt("attack_drone_target")
                .selected_text(
                    self.drone_attack_target
                        .as_ref()
                        .map_or("Choose hostile drone".into(), |(name, id)| {
                            format!("{name} #{id}")
                        }),
                )
                .show_ui(ui, |ui| {
                    for target in &enemies {
                        ui.selectable_value(
                            &mut self.drone_attack_target,
                            Some(target.clone()),
                            format!("{} #{}", target.0, target.1),
                        );
                    }
                });
            let target = self
                .drone_attack_target
                .clone()
                .filter(|t| enemies.contains(t));
            if ui
                .add_enabled(
                    controlled && target.is_some(),
                    egui::Button::new("Attack selected drone").small(),
                )
                .clicked()
            {
                let (target_pilot, target_drone_id) = target.unwrap();
                self.send(Request::AssignAttackDrone {
                    id,
                    target_pilot,
                    target_drone_id,
                });
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.drone_camera, "Drone screen");
            ui.selectable_value(&mut self.drone_orbit, false, "POV");
            ui.selectable_value(&mut self.drone_orbit, true, "Orbit camera");
        });
        if drone.signal_strength <= 0.0 {
            ui.colored_label(AMBER, "Signal lost · recall available");
        }
        ui.small(
            "10 km control radius · mining needs a confirmed lock · attacks damage drones only",
        );
    }
}
fn drone_mode_label(mode: &DroneMode) -> &'static str {
    match mode {
        DroneMode::Hold => "Holding",
        DroneMode::FlyTo { .. } => "Flying",
        DroneMode::Returning => "Returning",
        DroneMode::Mining { .. } => "Mining",
        DroneMode::Attacking { .. } => "Attacking",
    }
}
