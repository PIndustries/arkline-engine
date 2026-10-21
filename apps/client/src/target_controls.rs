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
        let fresh = self.connected
            && self.snapshot.as_ref().is_some_and(|snapshot| {
                self.presentation_time()
                    .saturating_sub(snapshot.server_time_ms)
                    <= 1500
            });
        let commands_ready = self.can_operate() && fresh;
        if !confirmed.is_empty() || !pending.is_empty() {
            if !fresh {
                ui.colored_label(
                    AMBER,
                    "Target rack is last received state · waiting for authority",
                );
            }
        } else {
            ui.small("No confirmed target locks");
        }
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
                        .add_enabled(commands_ready, egui::Button::new("Unlock").small())
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
                        .add_enabled(commands_ready, egui::Button::new("Cancel").small())
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
        match self.selected_drone {
            Some(id) => fleet.into_iter().find(|drone| drone.id == id),
            None => fleet.first().copied(),
        }
    }
    pub(super) fn drone_controls(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.own() else {
            ui.heading("Drone operations");
            ui.label("Waiting for the owned ship and deployed drone state.");
            return;
        };
        let fresh = self.connected
            && self.snapshot.as_ref().is_some_and(|snapshot| {
                self.presentation_time()
                    .saturating_sub(snapshot.server_time_ms)
                    <= 1500
            });
        let available = self.can_fly() && fresh;
        ui.heading("Drone operations");
        ui.label(format!(
            "{} deployed · owner {}",
            Self::fleet(&p).len(),
            p.name
        ));
        if !self.connected {
            ui.colored_label(AMBER, "Disconnected · last known fleet; orders unavailable");
        } else if !fresh {
            ui.colored_label(AMBER, "Fleet telemetry stale · waiting for authority");
        }
        if self.embedded && self.embedded_paused {
            ui.colored_label(
                MUTED,
                "Simulation paused · choose Resume simulation to issue orders",
            );
        } else if p.station_avatar.is_some() {
            ui.colored_label(MUTED, "On foot · return to the pilot seat for manual drone orders. Fleet and cameras remain viewable.");
        } else if p.location == Location::Docked {
            ui.colored_label(
                MUTED,
                "Docked · undock before deploying or directing drones",
            );
        } else if matches!(
            p.location,
            Location::InTransit { .. } | Location::Jumping { .. }
        ) {
            ui.colored_label(
                MUTED,
                "Transit in progress · wait for authority before issuing drone orders",
            );
        }
        if p.flight.is_none() && p.station_avatar.is_none() && p.location != Location::Docked {
            ui.colored_label(MUTED, "Ship flight telemetry unavailable · waiting for authority before issuing drone orders");
        }
        if self.request_pending {
            ui.colored_label(AMBER, "Awaiting authority · previous command pending");
        }
        self.manual_operations_summary(ui);
        ui.small("Bay inventory is not reported. Launch limit: five deployed drones per class.");
        let power = power_spec(p.ship_class);
        if let Some(capacitor) = &p.power {
            ui.label(format!(
                "Shared ship capacitor · {} / {} J",
                capacitor.charge_j, capacitor.capacity_j
            ));
        } else {
            ui.label("Ship capacitor telemetry unavailable");
        }
        ui.small("Drones draw from the ship capacitor; separate drone batteries and cargo holds are not reported.");
        ui.small(format!(
            "Owner capacitor: {} J / mining cycle · {} J / weapon shot",
            power.drone_mining_cycle_cost_j, power.drone_attack_cost_j
        ));
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
                    if fleet_row_button(
                        ui,
                        &p.name,
                        drone.id,
                        selected,
                        &format!(
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
            if let Some(id) = self.selected_drone {
                ui.colored_label(
                    AMBER,
                    format!(
                        "Selected drone #{id} is no longer reported · choose a current drone above"
                    ),
                );
            } else {
                ui.label("No deployed drones reported for this ship.");
            }
            if available && Self::fleet(&p).is_empty() {
                ui.small("Choose a drone class above to request deployment.");
            } else if !available {
                ui.small("Deployment controls are unavailable in the current state; the reason is shown above.");
            }
            return;
        };
        let id = drone.id;
        let controlled = available && drone.signal_strength > 0.0;
        ui.add_space(6.0);
        ui.strong(format!("SELECTED DRONE #{id}"));
        egui::Grid::new("selected-drone-authority")
            .num_columns(2)
            .min_col_width(0.0)
            .max_col_width(((ui.available_width() - 20.0) / 2.0).max(1.0))
            .spacing(egui::vec2(20.0, 7.0))
            .show(ui, |ui| {
                for (label, value) in [
                    ("Class", format!("{:?}", drone.class)),
                    ("Actual state", drone_mode_label(&drone.mode).to_owned()),
                    ("Assignment", drone_assignment(&drone.mode)),
                    ("Signal", format!("{:.0}%", drone.signal_strength * 100.0)),
                    ("Hull", format!("{} HP", drone.hit_points)),
                    ("Cargo", "No separate drone cargo reported".into()),
                    ("Energy", "Shared ship capacitor".into()),
                ] {
                    ui.colored_label(MUTED, label);
                    ui.label(value);
                    ui.end_row();
                }
            });
        if let Some(flight) = &p.flight {
            ui.small(format!(
                "Selected #{id} · {} from ship",
                format_distance(distance_m(flight.position, drone.position))
            ));
        } else {
            ui.small(format!(
                "Selected #{id} · distance unavailable without the ship's flight position"
            ));
        }
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
            let orbit_target = self.selected_target().filter(|t| {
                matches!(
                    t,
                    NavigationTarget::Asteroid { .. } | NavigationTarget::Pilot { .. }
                )
            });
            if ui
                .add_enabled(
                    controlled && orbit_target.is_some(),
                    egui::Button::new("Orbit selected").small(),
                )
                .clicked()
            {
                self.send(Request::OrbitDrone {
                    id,
                    target: orbit_target.unwrap(),
                });
            }
            if drone.class == DroneClass::Mining {
                let target = self.module_target(&p);
                if p.ore >= cargo_capacity(p.ship_class) {
                    ui.colored_label(MUTED, "Ship hold full · mining assignment unavailable");
                } else if target.is_none() {
                    ui.small("Confirm an asteroid lock before assigning this mining drone");
                }
                if ui
                    .add_enabled(
                        controlled && target.is_some() && p.ore < cargo_capacity(p.ship_class),
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
        ui.separator();
        ui.small("LIVE CAMERA / VIEW ONLY");
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.drone_camera, "Drone screen");
            ui.selectable_value(&mut self.drone_split, false, "Single feed");
            ui.selectable_value(&mut self.drone_split, true, "Split feeds");
        });
        ui.horizontal_wrapped(|ui| {
            for (mode, label) in [
                (flight3d::DroneCameraMode::Pov, "POV"),
                (flight3d::DroneCameraMode::Raised, "Raised"),
                (flight3d::DroneCameraMode::TopDown, "Top down"),
                (flight3d::DroneCameraMode::Orbit, "Orbit camera"),
            ] {
                ui.selectable_value(&mut self.drone_camera_mode, mode, label);
            }
        });
        let mut ids: Vec<_> = Self::fleet(&p).iter().map(|d| d.id).collect();
        ids.sort_unstable();
        let feeds = feed_ids(&ids, self.selected_drone, self.drone_split);
        if self.drone_split && !ids.is_empty() {
            let page = ids
                .iter()
                .position(|id| Some(*id) == self.selected_drone)
                .unwrap_or(0)
                / 4;
            let pages = ids.len().div_ceil(4);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(page > 0, egui::Button::new("Previous feeds").small())
                    .clicked()
                {
                    self.selected_drone = ids.get((page - 1) * 4).copied();
                }
                ui.small(format!("Page {} / {pages}", page + 1));
                if ui
                    .add_enabled(page + 1 < pages, egui::Button::new("Next feeds").small())
                    .clicked()
                {
                    self.selected_drone = ids.get((page + 1) * 4).copied();
                }
            });
        }
        let feed_label = if self.remote_ship.is_some() {
            "Selected drone feeds · worker screen active"
        } else if !self.drone_camera {
            "Selected drone feeds · screen off"
        } else {
            "Drone screen"
        };
        ui.small(format!(
            "{feed_label}: {}",
            feeds
                .iter()
                .map(|id| format!("#{id}"))
                .collect::<Vec<_>>()
                .join(" · ")
        ));
        ui.small("Select a fleet row to switch cameras. Split view shows at most four live feeds.");
        if self.remote_ship.is_some() {
            ui.colored_label(AMBER, "Worker feed overrides drone screen.");
            if ui.small_button("Restore drone feed").clicked() {
                self.remote_ship = None;
            }
        }
        if drone.signal_strength <= 0.0 {
            ui.colored_label(
                AMBER,
                if available {
                    "Signal lost · recall available"
                } else {
                    "Signal lost · recall controls currently unavailable"
                },
            );
        }
        ui.small(
            "10 km control radius · mining needs a confirmed lock · attacks damage drones only",
        );
        if !self.notice.is_empty() {
            ui.small(format!("Last authority response: {}", self.notice));
        }
    }
}
// The actual interaction ID must follow the vessel and drone, not its row slot.
// A scoped push_id does not replace selectable_label's positional response ID.
fn fleet_row_button(
    ui: &mut egui::Ui,
    owner: &str,
    drone_id: u64,
    selected: bool,
    label: &str,
) -> egui::Response {
    let id = ui.make_persistent_id(("fleet-drone", owner, drone_id));
    let padding = ui.spacing().button_padding;
    let text = egui::WidgetText::from(label).into_galley(
        ui,
        None,
        (ui.available_width() - 2.0 * padding.x).max(40.0),
        egui::TextStyle::Button,
    );
    let size = egui::vec2(
        text.size().x + 2.0 * padding.x,
        (text.size().y + 2.0 * padding.y).max(ui.spacing().interact_size.y),
    );
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let response = ui.interact(rect, id, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    if ui.is_rect_visible(rect) {
        let visual = ui.style().interact_selectable(&response, selected);
        if selected || response.hovered() || response.has_focus() {
            ui.painter().rect(
                rect,
                visual.corner_radius,
                visual.weak_bg_fill,
                visual.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        ui.painter()
            .galley(rect.min + padding, text, visual.text_color());
    }
    response
}

fn drone_mode_label(mode: &DroneMode) -> &'static str {
    match mode {
        DroneMode::Hold => "Holding",
        DroneMode::Orbit { .. } => "Orbiting",
        DroneMode::FlyTo { .. } => "Flying",
        DroneMode::Returning => "Returning",
        DroneMode::Mining { .. } => "Mining",
        DroneMode::Attacking { .. } => "Attacking",
    }
}

/// Stable ID pages retain the selected drone and never render more than four feeds.
pub(super) fn feed_ids(ids: &[u64], selected: Option<u64>, split: bool) -> Vec<u64> {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let index = selected
        .and_then(|id| ids.iter().position(|value| *value == id))
        .unwrap_or(0);
    if split {
        ids.into_iter().skip(index / 4 * 4).take(4).collect()
    } else {
        ids.get(index).copied().into_iter().collect()
    }
}

#[cfg(test)]
mod feed_tests {
    use super::feed_ids;
    #[test]
    fn feed_pages_are_bounded_and_contain_selected_drone() {
        let ids: Vec<_> = (1..=15).rev().collect();
        for selected in 1..=15 {
            let feeds = feed_ids(&ids, Some(selected), true);
            assert!(feeds.len() <= 4);
            assert!(feeds.contains(&selected));
            assert_eq!(feed_ids(&ids, Some(selected), false), vec![selected]);
        }
        assert_eq!(feed_ids(&ids, Some(15), true), vec![13, 14, 15]);
    }
    #[test]
    fn destroyed_or_missing_selection_falls_back_without_duplicate_feeds() {
        assert_eq!(feed_ids(&[9, 3, 3, 7], Some(99), true), vec![3, 7, 9]);
        assert_eq!(feed_ids(&[9, 3, 7], Some(99), false), vec![3]);
        assert!(feed_ids(&[], Some(7), true).is_empty());
    }
}

fn drone_assignment(mode: &DroneMode) -> String {
    match mode {
        DroneMode::Mining { target_id } => format!("Asteroid #{target_id}"),
        DroneMode::Attacking {
            target_pilot,
            target_drone_id,
        } => format!("{target_pilot} · drone #{target_drone_id}"),
        DroneMode::Orbit { target } => format!("Orbit {target:?}"),
        DroneMode::FlyTo { position } => format!(
            "System position [{:.1}, {:.1}, {:.1}] m",
            position[0], position[1], position[2]
        ),
        DroneMode::Returning => "Return to owner ship".into(),
        DroneMode::Hold => "Hold current position".into(),
    }
}

#[cfg(test)]
mod fleet_identity_tests {
    use super::*;

    fn render(
        ctx: &egui::Context,
        cockpit: &mut Cockpit,
        events: Vec<egui::Event>,
    ) -> Vec<(String, Pos2)> {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(900.0, 1100.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| cockpit.drone_controls(ui));
            },
        );
        output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    Some((
                        text.galley.job.text.clone(),
                        text.visual_bounding_rect().center(),
                    ))
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn drone_fleet_identity_removal_or_reorder_during_click_never_selects_replacement() {
        for remove in [false, true] {
            let ctx = egui::Context::default();
            let mut cockpit = crate::tests::view_test_cockpit(&ctx);
            let drone = |id| DroneView {
                id,
                class: DroneClass::Recon,
                hit_points: 100,
                signal_strength: 1.0,
                mining_cycle: None,
                attack_ready_at_ms: None,
                position: BELT_POSITION,
                velocity: [0.0; 3],
                heading: [1.0, 0.0, 0.0],
                mode: DroneMode::Hold,
                updated_at_ms: 1000,
            };
            cockpit.snapshot.as_mut().unwrap().pilots[0].drones = vec![drone(1), drone(2)];
            cockpit.selected_drone = Some(1);
            let (tx, rx) = std::sync::mpsc::channel();
            cockpit.commands = Some(tx);
            render(&ctx, &mut cockpit, vec![]);
            let labels = render(&ctx, &mut cockpit, vec![]);
            let point = labels
                .iter()
                .find(|(text, _)| text.starts_with("#01 "))
                .unwrap()
                .1;
            let event = |pressed| {
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            };
            render(&ctx, &mut cockpit, event(true));
            let fleet = &mut cockpit.snapshot.as_mut().unwrap().pilots[0].drones;
            if remove {
                fleet.remove(0);
            } else {
                fleet.reverse();
            }
            let authority = serde_json::to_value(&cockpit.snapshot).unwrap();
            let labels = render(&ctx, &mut cockpit, event(false));
            assert_eq!(
                cockpit.selected_drone,
                Some(1),
                "replacement drone must not inherit a pressed row"
            );
            assert!(
                rx.try_recv().is_err(),
                "selection gesture must not dispatch an order"
            );
            assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), authority);
            if remove {
                assert!(labels
                    .iter()
                    .any(|(text, _)| text.contains("Selected drone #1 is no longer reported")));
            }
        }
    }
}
