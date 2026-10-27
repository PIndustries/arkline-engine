//! Server-confirmed target rack and utility drone controls.
use super::*;
use arkline_protocol::{
    DroneBayBatchKind, DroneBayBatchMemberStatus, DroneBayPhase, DroneBayPresence, DroneBayRequest,
    DroneBayState, DroneStockStatus,
};
pub(super) fn supported_drone_cargo(
    pilot: &PilotView,
) -> Result<Option<&arkline_protocol::DroneCargoPublicState>, &'static str> {
    match pilot.drone_cargo.as_ref() {
        None => Ok(None),
        Some(cargo) if flight3d::ore_receiver_view_available(pilot) => Ok(Some(cargo)),
        Some(_) => {
            Err("Ore receiver telemetry unavailable · invalid source, vessel or cargo accounting")
        }
    }
}

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
    /// Clear only an empty, confirmed legacy fleet. Physical stock keeps its
    /// selected identity while Stored or Destroyed; never switch to another drone.
    pub(super) fn reconcile_empty_legacy_drone_selection(&mut self) {
        if !self.connected || !self.live_command_snapshot_fresh() {
            return;
        }
        let Some(pilot) = self.own() else { return };
        if self
            .snapshot
            .as_ref()
            .is_some_and(|s| s.carrier_inventory_registration.is_some())
            || pilot.carrier_inventory.is_some()
            || self
                .assets
                .as_ref()
                .is_some_and(|a| a.carrier_inventory.is_some())
            || pilot.drone_bay.is_some()
            || self.assets.as_ref().is_some_and(|a| a.drone_bay.is_some())
            || !Self::fleet(&pilot).is_empty()
        {
            return;
        }
        if let Some(id) = self.selected_drone.take() {
            self.notice = format!("Drone #{id} is no longer deployed. Selection cleared; this legacy world has no physical bay storage.");
        }
    }

    fn drone_world_mode(&self, ui: &mut egui::Ui, pilot: &PilotView) {
        if flight3d::bay_view_available(pilot) {
            ui.colored_label(TEAL, "PHYSICAL BAY · full launch / recovery");
        } else if pilot.drone_bay.is_some()
            || self.assets.as_ref().is_some_and(|a| a.drone_bay.is_some())
        {
            ui.colored_label(
                AMBER,
                "Physical bay telemetry unavailable for this vessel · no legacy fallback",
            );
        } else {
            let revision = self
                .snapshot
                .as_ref()
                .and_then(|s| s.world_layout.as_ref())
                .map(|l| format!("{:?}", l.revision))
                .unwrap_or_else(|| "LegacyV1".into());
            ui.colored_label(
                AMBER,
                format!("LEGACY DRONE MODE · {revision} · no physical bay"),
            );
            ui.label("These legacy drones launch outside the ship and disappear when returning nearby. They are not travelling into stored cradles.");
            ui.small(if self.embedded {
                "For the physical sequence: Editor Session → Physical flagship preview. This creates or reopens a separate world; your current saved world is not converted."
            } else {
                "This is the standalone game. For the physical sequence, open Arkline Editor → Session → Physical flagship preview. Existing worlds are not converted."
            });
        }
    }

    /// Public stock remains viewable without private command capability. Never
    /// substitute the controller's inventory for a remotely controlled vessel.
    fn drone_batch_controls(
        &mut self,
        ui: &mut egui::Ui,
        public: &arkline_protocol::DroneBayPublicState,
        state: Option<&DroneBayState>,
        ready: bool,
        idle: bool,
        cargo_supported: bool,
    ) {
        let selected_class = ui
            .ctx()
            .data_mut(|data| {
                data.get_temp::<DroneClass>(egui::Id::new((
                    "bay_roster_class",
                    self.controlled_name(),
                )))
            })
            .unwrap_or_else(|| {
                self.selected_drone
                    .and_then(|id| public.cradles.iter().find(|entry| entry.drone_id == id))
                    .map_or(DroneClass::Mining, |entry| entry.class)
            });
        let next = state.and_then(|s| s.batches.as_ref()?.validated_next_id(&s.binding));
        if let Some(batch) = public.batch.as_ref().filter(|b| b.active()) {
            let done = batch
                .members
                .iter()
                .filter(|m| {
                    matches!(
                        m.status,
                        DroneBayBatchMemberStatus::Launched | DroneBayBatchMemberStatus::Stored
                    )
                })
                .count();
            ui.strong(format!(
                "{} · {done}/{} successful{}",
                match batch.kind {
                    DroneBayBatchKind::DeployClass { .. } => "Deploying class",
                    DroneBayBatchKind::RecallAll => "Recall All",
                },
                batch.members.len(),
                if batch.held { " · HELD" } else { "" }
            ));

            {
                if ui
                    .add_enabled(
                        ready && !self.request_pending,
                        egui::Button::new(if batch.held {
                            "Resume queue"
                        } else {
                            "Hold queue"
                        }),
                    )
                    .clicked()
                {
                    let binding = public.binding.clone();
                    let batch_id = batch.batch_id;
                    self.send(Request::DroneBay {
                        request: if batch.held {
                            DroneBayRequest::ResumeBatch { binding, batch_id }
                        } else {
                            DroneBayRequest::HoldBatch { binding, batch_id }
                        },
                    });
                }
                if ui
                    .add_enabled(
                        ready
                            && !self.request_pending
                            && batch.phase == arkline_protocol::DroneBayBatchPhase::Running,
                        egui::Button::new("Cancel remaining"),
                    )
                    .clicked()
                {
                    self.send(Request::DroneBay {
                        request: DroneBayRequest::CancelBatch {
                            binding: public.binding.clone(),
                            batch_id: batch.batch_id,
                        },
                    });
                }
            }
        } else {
            let job_active = self.snapshot.as_ref().is_some_and(|s| {
                s.resource_jobs
                    .iter()
                    .any(|j| j.pilot == self.controlled_name() && !j.phase.is_terminal())
            });
            if job_active {
                ui.colored_label(AMBER, "Mining routine owns this vessel · stop the routine before starting a manual drone queue.");
            }
            let ready = ready && !job_active;
            {
                for (class, label) in [
                    (DroneClass::Recon, "Recon"),
                    (DroneClass::Mining, "Mining"),
                    (DroneClass::Attack, "Combat"),
                ] {
                    if class != selected_class {
                        continue;
                    }
                    let count = public
                        .cradles
                        .iter()
                        .filter(|c| c.class == class && c.presence == DroneBayPresence::Stored)
                        .count();
                    if ui
                        .add_enabled(
                            ready && idle && next.is_some() && count > 0 && !self.request_pending,
                            egui::Button::new(format!("Deploy {label} ({count})")),
                        )
                        .clicked()
                    {
                        self.send(Request::DroneBay {
                            request: DroneBayRequest::BeginBatch {
                                binding: public.binding.clone(),
                                batch_id: next.unwrap(),
                                batch_kind: DroneBayBatchKind::DeployClass { class },
                            },
                        });
                    }
                }
                let count = public
                    .cradles
                    .iter()
                    .filter(|c| c.presence == DroneBayPresence::Active)
                    .count();
                if ui
                    .add_enabled(
                        ready && idle && next.is_some() && count > 0 && !self.request_pending,
                        egui::Button::new(if cargo_supported {
                            format!("Recall & stow ({count})")
                        } else {
                            format!("Recall All ({count})")
                        }),
                    )
                    .clicked()
                {
                    self.send(Request::DroneBay {
                        request: DroneBayRequest::BeginBatch {
                            binding: public.binding.clone(),
                            batch_id: next.unwrap(),
                            batch_kind: DroneBayBatchKind::RecallAll,
                        },
                    });
                }
            }
        }
    }
    fn drone_cargo_controls(&mut self, ui: &mut egui::Ui, pilot: &PilotView, ready: bool) {
        use arkline_protocol::{DroneCargoRequest, DroneDropoffPhase};
        let Ok(Some(cargo)) = supported_drone_cargo(pilot) else {
            return;
        };
        let private = (pilot.name == self.pilot)
            .then(|| self.assets.as_ref()?.drone_cargo.as_ref())
            .flatten();
        let next = private.and_then(|private| cargo.validated_next_id(private, pilot));
        let job_active = self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot
                .resource_jobs
                .iter()
                .any(|job| job.pilot == pilot.name && !job.phase.is_terminal())
        });
        let ready = ready && next.is_some() && !self.request_pending && !job_active;
        if next.is_none() {
            ui.small("Ore commands unavailable · matching owner journal required");
        }
        if job_active {
            ui.small("Mining routine owns ore handling · stop it before manual orders");
        }
        let batch_owned = pilot.drone_bay.as_ref().and_then(|bay| bay.batch.as_ref()).filter(|batch| batch.active())
            .is_some_and(|batch| cargo.operations.iter().any(|operation| {
                matches!(operation.followup, arkline_protocol::DroneDropoffFollowup::RecallAll { batch_id } if batch_id == batch.batch_id)
                    && (operation.active() || cargo.cleanup.as_ref().is_some_and(|cleanup| cleanup.operation_id == operation.operation_id))
            }));
        if batch_owned {
            ui.small(
                "Recall & stow owns this transfer · use queue Hold / Resume / Cancel remaining",
            );
            return;
        }
        if let Some(cleanup) = &cargo.cleanup {
            ui.colored_label(
                AMBER,
                format!(
                    "Ore receiver cleanup #{} · {}",
                    cleanup.operation_id, cleanup.reason
                ),
            );
            if ui
                .add_enabled(
                    ready && cleanup.held,
                    egui::Button::new("Resume receiver cleanup"),
                )
                .clicked()
            {
                self.send(Request::DroneCargo {
                    request: DroneCargoRequest::Resume {
                        binding: cargo.binding.clone(),
                        operation_id: cleanup.operation_id,
                    },
                });
            }
            return;
        }
        let operation = self
            .selected_drone
            .and_then(|id| {
                cargo
                    .operations
                    .iter()
                    .find(|operation| operation.drone_id == id && operation.active())
            })
            .or_else(|| {
                cargo.receiver_operation.and_then(|id| {
                    cargo
                        .operations
                        .iter()
                        .find(|operation| operation.operation_id == id)
                })
            });
        if let Some(operation) = operation {
            ui.small(format!(
                "DRONE #{} → SHIP · {} / {} confirmed",
                operation.drone_id, operation.committed_units, operation.requested_units
            ));
            {
                let held = operation.phase == DroneDropoffPhase::Held;
                if ui
                    .add_enabled(
                        ready,
                        egui::Button::new(if held {
                            "Resume drop-off"
                        } else {
                            "Hold drop-off"
                        }),
                    )
                    .clicked()
                {
                    let binding = cargo.binding.clone();
                    let operation_id = operation.operation_id;
                    self.send(Request::DroneCargo {
                        request: if held {
                            DroneCargoRequest::Resume {
                                binding,
                                operation_id,
                            }
                        } else {
                            DroneCargoRequest::Hold {
                                binding,
                                operation_id,
                            }
                        },
                    });
                }
                if ui
                    .add_enabled(ready, egui::Button::new("Cancel drop-off"))
                    .clicked()
                {
                    self.send(Request::DroneCargo {
                        request: DroneCargoRequest::Cancel {
                            binding: cargo.binding.clone(),
                            operation_id: operation.operation_id,
                        },
                    });
                }
            }
            ui.small(&operation.message);
        } else {
            let selected = self.selected_drone.and_then(|id| {
                cargo
                    .holds
                    .iter()
                    .find(|hold| hold.drone_id == id && hold.ore_units > 0)
            });
            let bay_idle = pilot.drone_bay.as_ref().is_some_and(|bay| {
                bay.machinery.cleanup.is_none()
                    && bay.machinery.handling_operation.is_none()
                    && !bay.batch.as_ref().is_some_and(|batch| batch.active())
            });
            if ui
                .add_enabled(
                    ready && selected.is_some() && bay_idle,
                    egui::Button::new("Drop ore & resume"),
                )
                .clicked()
            {
                self.send(Request::DroneCargo {
                    request: DroneCargoRequest::BeginDropoff {
                        binding: cargo.binding.clone(),
                        operation_id: next.unwrap(),
                        drone_id: selected.unwrap().drone_id,
                        resume_mining: true,
                    },
                });
            }
        }
    }
    fn drone_bay_five_rows(
        &mut self,
        ui: &mut egui::Ui,
        pilot: &PilotView,
        public: &arkline_protocol::DroneBayPublicState,
    ) {
        let cargo = supported_drone_cargo(pilot);
        let tab_id = egui::Id::new(("bay_roster_class", &pilot.name));
        let mut class = ui
            .ctx()
            .data_mut(|data| data.get_temp::<DroneClass>(tab_id))
            .unwrap_or_else(|| {
                self.selected_drone
                    .and_then(|id| public.cradles.iter().find(|entry| entry.drone_id == id))
                    .map_or(DroneClass::Mining, |entry| entry.class)
            });
        ui.horizontal(|ui| {
            ui.spacing_mut().button_padding.y = 2.0;
            for (value, label) in [
                (DroneClass::Mining, "Mining"),
                (DroneClass::Recon, "Recon"),
                (DroneClass::Attack, "Combat"),
            ] {
                ui.selectable_value(&mut class, value, label);
            }
        });
        ui.ctx().data_mut(|data| data.insert_temp(tab_id, class));
        let mut entries: Vec<_> = public
            .cradles
            .iter()
            .filter(|entry| entry.class == class)
            .collect();
        entries.sort_by_key(|entry| entry.home_cradle);
        let row_spacing = ui.spacing().item_spacing;
        ui.spacing_mut().item_spacing.y = 2.0;
        for entry in entries {
            let queued = public
                .batch
                .as_ref()
                .filter(|batch| batch.active())
                .and_then(|batch| {
                    batch
                        .members
                        .iter()
                        .find(|member| member.drone_id == entry.drone_id)
                })
                .is_some_and(|member| member.status == DroneBayBatchMemberStatus::Pending);
            let dropoff = cargo
                .as_ref()
                .ok()
                .and_then(|cargo| *cargo)
                .and_then(|cargo| {
                    cargo.operations.iter().find(|operation| {
                        operation.drone_id == entry.drone_id && operation.active()
                    })
                });
            let phase = if cargo.is_err() {
                "Ore telemetry unavailable"
            } else if let Some(operation) = dropoff {
                operation.phase.label()
            } else if queued {
                "Queued"
            } else {
                public
                    .constrained(entry.drone_id)
                    .map(|motion| motion.phase.label())
                    .unwrap_or(match entry.presence {
                        DroneBayPresence::Stored => "Stored",
                        DroneBayPresence::Active => Self::fleet(pilot)
                            .iter()
                            .find(|drone| drone.id == entry.drone_id)
                            .map_or("Active · telemetry unavailable", |drone| {
                                drone_mode_label(&drone.mode)
                            }),
                        DroneBayPresence::Destroyed => "Destroyed",
                    })
            };
            let cycle = bay_row_cycle(pilot, entry.drone_id, phase, self.drone_telemetry_time);
            let cycle_detail = cycle.as_ref().map_or_else(String::new, |cycle| {
                format!(
                    " · {} · {}",
                    cycle.label,
                    self.drone_telemetry_time.availability.label()
                )
            });
            let payload = match &cargo {
                Ok(Some(cargo)) => cargo
                    .holds
                    .iter()
                    .find(|hold| hold.drone_id == entry.drone_id)
                    .map_or_else(
                        || "—".into(),
                        |hold| format!("{}/{} ore", hold.ore_units, hold.capacity_units),
                    ),
                Ok(None) => "n/r".into(),
                Err(_) => "?".into(),
            };
            let short_phase = match phase {
                "Queued for ore receiver" => "Queued ore",
                "Approaching ore receiver" => "Approach",
                "Aligning for ore transfer" => "Align",
                "Connecting ore coupler" => "Couple",
                "Transferring confirmed ore" => "Transfer",
                "Disconnecting ore coupler" => "Disconnect",
                "Clearing ore receiver" => "Egress",
                "Ore drop-off held" => "Held ore",
                "Ore telemetry unavailable" => "Unavailable",
                "Releasing cradle" => "Release",
                "Transferring to launch throat" => "To throat",
                "Opening bay door" => "Door opening",
                "Clearing drone bay" => "Departing",
                "Returning to drone bay" => "Returning",
                "Waiting for drone bay" => "Bay queue",
                "Aligning with drone bay" => "Bay align",
                "Entering drone bay" => "Entering",
                "Returning to home cradle" => "To cradle",
                "Securing home cradle" => "Securing",
                "Docked in home cradle" => "Stored",
                "Drone bay operation held" => "Held bay",
                "Drone bay operation cancelled" => "Cancelled",
                "Drone destroyed" => "Destroyed",
                other => other,
            };
            let cycle_label = cycle.as_ref().map(|cycle| {
                let percent = cycle.label.split(" / ").next().unwrap_or("?");
                let state = match self.drone_telemetry_time.availability {
                    crate::drone_telemetry::Availability::Live if cycle.fraction >= 1. => "Await",
                    crate::drone_telemetry::Availability::Live => "Mine",
                    crate::drone_telemetry::Availability::Paused => "Paused",
                    _ => "Last",
                };
                format!("{state} {percent}")
            });
            let short_phase = cycle_label.as_deref().unwrap_or(short_phase);
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.), egui::Sense::hover());
            let response = ui.interact(
                rect,
                ui.make_persistent_id(("fleet-drone", &pilot.name, entry.drone_id)),
                egui::Sense::click(),
            );
            let selected = self.selected_drone == Some(entry.drone_id);
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    ui.is_enabled(),
                    selected,
                    format!(
                        "Drone #{} · home {} · {} HP · ore {} · {}{}",
                        entry.drone_id,
                        entry.home_cradle + 1,
                        entry.hit_points,
                        payload,
                        phase,
                        cycle_detail
                    ),
                )
            });
            if selected || response.hovered() || response.has_focus() {
                ui.painter()
                    .rect_filled(rect, 3., ui.visuals().selection.bg_fill);
            }
            let font = egui::FontId::proportional(13.);
            let color = ui.visuals().text_color();
            let hp_rect =
                egui::Rect::from_min_size(rect.min + egui::vec2(100., 5.), egui::vec2(42., 16.));
            ui.painter()
                .rect_filled(hp_rect, 2., ui.visuals().widgets.inactive.bg_fill);
            let mut fill = hp_rect;
            fill.max.x =
                fill.min.x + fill.width() * entry.hit_points as f32 / DRONE_HIT_POINTS as f32;
            ui.painter()
                .rect_filled(fill, 2., TEAL.gamma_multiply(0.35));
            for (x, text) in [
                (
                    4.,
                    format!("#{} · home {}", entry.drone_id, entry.home_cradle + 1),
                ),
                (102., format!("{} HP", entry.hit_points)),
                (150., payload.clone()),
                (230., short_phase.to_owned()),
            ] {
                let galley = ui.painter().layout_no_wrap(text, font.clone(), color);
                ui.painter()
                    .with_clip_rect(rect.intersect(ui.clip_rect()))
                    .galley(
                        egui::pos2(rect.left() + x, rect.center().y - galley.size().y * 0.5),
                        galley,
                        color,
                    );
            }
            if let Some(cycle) = &cycle {
                let track = egui::Rect::from_min_max(
                    egui::pos2(rect.left() + 230., rect.bottom() - 3.),
                    egui::pos2(rect.right() - 4., rect.bottom() - 1.),
                );
                if track.width() > 0. {
                    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
                    painter.rect_filled(track, 1., ui.visuals().widgets.inactive.bg_fill);
                    let mut fill = track;
                    fill.max.x = fill.min.x + fill.width() * cycle.fraction;
                    let color = if self.drone_telemetry_time.availability
                        == crate::drone_telemetry::Availability::Live
                    {
                        TEAL
                    } else {
                        AMBER
                    };
                    painter.rect_filled(fill, 1., color);
                }
            }
            if response
                .clone()
                .on_hover_text(format!(
                    "Drone #{} · home {} · {} HP · ore {} · {}{}",
                    entry.drone_id,
                    entry.home_cradle + 1,
                    entry.hit_points,
                    payload,
                    phase,
                    cycle_detail
                ))
                .clicked()
            {
                self.selected_drone = Some(entry.drone_id);
            }
        }
        ui.spacing_mut().item_spacing = row_spacing;
    }
    fn drone_bay_controls(
        &mut self,
        ui: &mut egui::Ui,
        pilot: &PilotView,
        available: bool,
    ) -> bool {
        // Controller capability can identify a feature realm, but its private
        // stock must never be displayed or spent as the viewed worker's stock.
        let advertised = pilot.drone_bay.is_some()
            || self.assets.as_ref().is_some_and(|a| a.drone_bay.is_some());
        if !advertised {
            return false;
        }
        if !flight3d::bay_view_available(pilot) {
            ui.colored_label(AMBER, "Drone bay capability unavailable for this viewed vessel · missing, unsupported or inconsistent telemetry. Legacy spawning is disabled.");
            return true;
        }
        let public = pilot.drone_bay.as_ref().unwrap();
        let cargo = supported_drone_cargo(pilot);
        if let Err(reason) = cargo {
            ui.colored_label(AMBER, reason);
        }
        let private = bay_private_state(pilot, &self.pilot, self.assets.as_ref()).cloned();
        let stationary = bay_carrier_stopped(pilot);
        if !stationary {
            ui.colored_label(
                AMBER,
                "STOP the carrier and wait for zero motion before bay handling.",
            );
        }
        if let Err(reason) = &private {
            ui.colored_label(AMBER, *reason);
        }
        let stored = public
            .cradles
            .iter()
            .filter(|e| e.presence == DroneBayPresence::Stored)
            .count();
        let active = public
            .cradles
            .iter()
            .filter(|e| e.presence == DroneBayPresence::Active)
            .count();
        let destroyed = public.cradles.len() - stored - active;
        let context = if self.embedded && self.embedded_paused {
            " · PAUSED — Resume simulation"
        } else if pilot.location == Location::Docked && pilot.station_avatar.is_none() {
            " · DOCKED — undock for orders"
        } else {
            ""
        };
        ui.strong(format!(
            "PHYSICAL BAY{context} · {stored} stored · {active} active · {destroyed} destroyed / 15"
        ));

        let state = private.as_ref().ok();
        let ready = available && stationary && state.is_some();
        let batch_active = public.batch.as_ref().is_some_and(|b| b.active());
        let idle = public.machinery.handling_operation.is_none()
            && public.machinery.cleanup.is_none()
            && !batch_active
            && cargo.as_ref().is_ok_and(|cargo| {
                cargo.is_none_or(|cargo| {
                    cargo.cleanup.is_none()
                        && !cargo.operations.iter().any(|operation| operation.active())
                })
            });
        let batch_supported = public.batch_capability == Some(1);
        let next = state.and_then(bay_next_operation);
        let capacity = cargo_capacity(pilot.ship_class);
        let hold_text = match cargo {
            Ok(Some(cargo)) => format!(
                "SHIP {} / {} ore · free {} · incoming reserved {} · available {}",
                pilot.ore,
                capacity,
                capacity.saturating_sub(pilot.ore),
                cargo.incoming_reserved_units,
                cargo
                    .available_for_new_ore(pilot.ore, capacity)
                    .expect("validated headroom")
            ),
            _ => format!(
                "SHIP {} / {} ore · physically free {}",
                pilot.ore,
                capacity,
                capacity.saturating_sub(pilot.ore)
            ),
        };
        ui.add(
            egui::ProgressBar::new(pilot.ore as f32 / capacity.max(1) as f32)
                .desired_height(18.)
                .text(hold_text),
        );
        let cargo_service_active = cargo.as_ref().is_ok_and(|cargo| {
            cargo.is_some_and(|cargo| {
                cargo.cleanup.is_some()
                    || cargo.operations.iter().any(|operation| operation.active())
            })
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().button_padding.y = 2.0;
            if !batch_active {
                self.drone_cargo_controls(ui, pilot, available && stationary);
            }
            if batch_supported && (batch_active || !cargo_service_active) {
                self.drone_batch_controls(
                    ui,
                    public,
                    state,
                    ready,
                    idle,
                    matches!(cargo, Ok(Some(_))),
                );
            }
        });
        let panel_width = ui.available_width();
        let controls_width = (panel_width * 0.47).clamp(310.0, 390.0);
        let monitor_width = (panel_width - controls_width - ui.spacing().item_spacing.x).max(180.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(controls_width);
                self.drone_bay_five_rows(ui, pilot, public);
            });
            ui.vertical(|ui| {
                ui.set_width(monitor_width);
                let receiver_key = ui.make_persistent_id(("ore_receiver_monitor", &pilot.name));
                let mut receiver_view = ui
                    .ctx()
                    .data_mut(|data| data.get_temp::<bool>(receiver_key))
                    .unwrap_or(false);
                let receiver_available = matches!(cargo, Ok(Some(_)));
                ui.horizontal(|ui| {
                    ui.spacing_mut().button_padding = egui::vec2(4., 2.);
                    ui.spacing_mut().item_spacing.x = 4.;
                    ui.small("VIEW ONLY");
                    if ui
                        .selectable_label(!receiver_view && !self.bay_monitor_doorway, "Inside bay")
                        .clicked()
                    {
                        receiver_view = false;
                        self.bay_monitor_doorway = false;
                    }
                    if ui
                        .selectable_label(!receiver_view && self.bay_monitor_doorway, "Doorway")
                        .clicked()
                    {
                        receiver_view = false;
                        self.bay_monitor_doorway = true;
                    }
                    if receiver_available
                        && ui.selectable_label(receiver_view, "Ore receiver").clicked()
                    {
                        receiver_view = true;
                    }
                });
                ui.ctx()
                    .data_mut(|data| data.insert_temp(receiver_key, receiver_view));
                if receiver_view && !receiver_available {
                    ui.colored_label(
                        AMBER,
                        "Selected ore receiver view unavailable · waiting for validated telemetry",
                    );
                    return;
                }
                if let Some(snapshot) = &self.snapshot {
                    let now = self
                        .timeline_origin
                        .map_or(snapshot.server_time_ms, |origin| {
                            origin.saturating_add(self.observed.elapsed().as_millis() as u64)
                        });
                    self.flight_view.bay_operations_camera_ui(
                        ui,
                        snapshot,
                        &pilot.name,
                        flight3d::BayMonitorOptions {
                            now,
                            connected: self.connected,
                            paused: self.embedded_paused || self.frozen_time.is_some(),
                            camera: if receiver_view {
                                flight3d::BayMonitorCamera::OreReceiver
                            } else if self.bay_monitor_doorway {
                                flight3d::BayMonitorCamera::Doorway
                            } else {
                                flight3d::BayMonitorCamera::Wide
                            },
                        },
                    );
                }
            });
        });
        if !batch_active {
            if let Some(batch) = &public.batch {
                let successful = batch
                    .members
                    .iter()
                    .filter(|m| {
                        matches!(
                            m.status,
                            DroneBayBatchMemberStatus::Launched | DroneBayBatchMemberStatus::Stored
                        )
                    })
                    .count();
                let cancelled = batch
                    .members
                    .iter()
                    .filter(|m| m.status == DroneBayBatchMemberStatus::Cancelled)
                    .count();
                let destroyed = batch
                    .members
                    .iter()
                    .filter(|m| m.status == DroneBayBatchMemberStatus::Destroyed)
                    .count();
                ui.label(format!("Last queue #{} · {:?} · {successful} successful · {cancelled} cancelled · {destroyed} destroyed", batch.batch_id, batch.phase));
                ui.small(&batch.message);
            }
        }
        ui.separator();
        if !batch_supported {
            ui.small("Individual physical operations · Deploy class / Recall All require the separate Physical flagship preview. This saved world is not migrated.");
        }
        if state.is_some() && next.is_none() {
            ui.colored_label(
                AMBER,
                "Operation counter unavailable or inconsistent · new launch/recovery disabled",
            );
        }
        if let Some(cleanup) = &public.machinery.cleanup {
            ui.colored_label(
                AMBER,
                format!(
                    "Empty hardware cleanup #{} · {} · {}",
                    cleanup.operation_id,
                    if cleanup.held { "Held" } else { "In progress" },
                    cleanup.reason
                ),
            );
            ui.small("Cleanup returns hardware only; destroyed drones stay destroyed.");
            if batch_active {
                ui.small("Use the queue Hold / Resume control above for this reserved hardware.");
            }
            if !batch_active
                && ui
                    .add_enabled(
                        ready && cleanup.held && !batch_active && !self.request_pending,
                        egui::Button::new("Resume hardware cleanup"),
                    )
                    .clicked()
            {
                self.send(Request::DroneBay {
                    request: DroneBayRequest::Resume {
                        binding: public.binding.clone(),
                        operation_id: cleanup.operation_id,
                    },
                });
            }
        }
        if !batch_supported {
            ui.horizontal_wrapped(|ui| {
                for (class, label) in [
                    (DroneClass::Recon, "Recon"),
                    (DroneClass::Mining, "Mining"),
                    (DroneClass::Attack, "Attack"),
                ] {
                    let stored = public
                        .cradles
                        .iter()
                        .filter(|e| e.class == class && e.presence == DroneBayPresence::Stored)
                        .count();
                    let candidate = state
                        .and_then(|s| {
                            s.stock.iter().find(|e| {
                                e.class == class
                                    && matches!(
                                        e.status,
                                        DroneStockStatus::Stored { hit_points: 1.. }
                                    )
                            })
                        })
                        .map(|e| e.drone_id);
                    if ui
                        .add_enabled(
                            ready
                                && idle
                                && next.is_some()
                                && candidate.is_some()
                                && !self.request_pending,
                            egui::Button::new(format!("Launch {label} · {stored} stored / 5"))
                                .small(),
                        )
                        .clicked()
                    {
                        self.send(Request::DroneBay {
                            request: DroneBayRequest::Launch {
                                binding: public.binding.clone(),
                                drone_id: candidate.unwrap(),
                                operation_id: next.unwrap(),
                            },
                        });
                    }
                }
            });
        }
        if let Some(op) = public
            .machinery
            .handling_operation
            .and_then(|id| state.and_then(|s| s.operations.iter().find(|op| op.operation_id == id)))
        {
            ui.strong(format!(
                "Now handling drone #{} · {}",
                op.drone_id,
                op.phase.label()
            ));
            if !batch_active {
                ui.label(&op.message);
            }
            if !batch_active {
                ui.horizontal_wrapped(|ui| {
                    let held = op.phase == DroneBayPhase::Held;
                    if ui
                        .add_enabled(
                            ready && !self.request_pending,
                            egui::Button::new(if held {
                                "Resume bay operation"
                            } else {
                                "Hold bay operation"
                            }),
                        )
                        .clicked()
                    {
                        let request = if held {
                            DroneBayRequest::Resume {
                                binding: public.binding.clone(),
                                operation_id: op.operation_id,
                            }
                        } else {
                            DroneBayRequest::Hold {
                                binding: public.binding.clone(),
                                operation_id: op.operation_id,
                            }
                        };
                        self.send(Request::DroneBay { request });
                    }
                    if ui
                        .add_enabled(
                            ready && !self.request_pending,
                            egui::Button::new("Cancel bay operation"),
                        )
                        .clicked()
                    {
                        self.send(Request::DroneBay {
                            request: DroneBayRequest::Cancel {
                                binding: public.binding.clone(),
                                operation_id: op.operation_id,
                            },
                        });
                    }
                });
            }
        }
        if let Some(batch) = &public.batch {
            ui.label(&batch.message);
        }
        ui.collapsing("How deployment and recovery work", |ui| {
            ui.small("Five per class · retained IDs, hull and home cradles. No free replacements or automatic repairs.");
            ui.small("One drone uses the bay at a time. Cancel remaining leaves the current accepted route to finish safely; held work still needs Resume.");
            ui.small("A drone is Stored only after its home cradle and machinery are secure. This monitor never redirects the captain camera.");
            ui.small("Launch: cradle → transfer → doorway → deployed. Recovery: approach → doorway → home cradle → secure → Stored. Status comes from authority, never a countdown.");
        });
        let Some(entry) = self
            .selected_drone
            .and_then(|id| public.cradles.iter().find(|e| e.drone_id == id))
        else {
            if let Some(id) = self.selected_drone {
                ui.colored_label(AMBER,format!("Selected drone #{id} is not in this vessel's retained roster · choose a roster row"));
            } else {
                ui.small("Select a stored or active drone to inspect its retained identity and operations.");
            }
            return true;
        };
        ui.strong(format!(
            "BAY ID #{} · home {} · {} HP",
            entry.drone_id,
            entry.home_cradle + 1,
            entry.hit_points
        ));
        if entry.presence == DroneBayPresence::Stored {
            let rack = match entry.class {
                DroneClass::Recon => "Recon rack",
                DroneClass::Mining => "Mining rack",
                DroneClass::Attack => "Combat rack",
            };
            ui.label(format!("Docked in home cradle {} · choose Bay overview or {rack} to inspect stored drone #{}.",entry.home_cradle+1,entry.drone_id));
        }
        if entry.presence == DroneBayPresence::Destroyed {
            ui.label("Destroyed · identity and cradle retained; cannot launch or recover");
            return true;
        }
        if state
            .and_then(|s| s.active_operation(entry.drone_id))
            .is_some()
        {
            ui.label("This drone is on the current physical route. Hold, Resume and Cancel are above the bay monitor.");
        } else {
            let stored = entry.presence == DroneBayPresence::Stored;
            let loaded = cargo
                .as_ref()
                .ok()
                .and_then(|cargo| *cargo)
                .is_some_and(|cargo| {
                    cargo
                        .holds
                        .iter()
                        .any(|hold| hold.drone_id == entry.drone_id && hold.ore_units > 0)
                });
            if loaded {
                ui.small("Drop off ore first, or use Recall & stow to unload then stow.");
            }
            if ui
                .add_enabled(
                    ready
                        && idle
                        && next.is_some()
                        && public.constrained(entry.drone_id).is_none()
                        && !loaded
                        && !self.request_pending,
                    egui::Button::new(if stored {
                        "Launch selected stored drone"
                    } else {
                        "Recover selected drone"
                    }),
                )
                .clicked()
            {
                let request = if stored {
                    DroneBayRequest::Launch {
                        binding: public.binding.clone(),
                        drone_id: entry.drone_id,
                        operation_id: next.unwrap(),
                    }
                } else {
                    DroneBayRequest::Recover {
                        binding: public.binding.clone(),
                        drone_id: entry.drone_id,
                        operation_id: next.unwrap(),
                    }
                };
                self.send(Request::DroneBay { request });
            }
        }
        true
    }
    pub(super) fn drone_controls(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.own() else {
            ui.heading("Drone operations");
            ui.label("Waiting for the owned ship and deployed drone state.");
            return;
        };
        if self.carrier_inventory_controls(ui, &p) {
            return;
        }
        let fresh = self.live_command_snapshot_fresh();
        let available = self.can_fly() && fresh;
        let physical_bay = flight3d::bay_view_available(&p);
        if !flight3d::bay_view_available(&p) {
            ui.heading("Drone operations");
        }
        if !flight3d::bay_view_available(&p) {
            self.drone_world_mode(ui, &p);
        }
        if p.drone_bay.is_none() {
            ui.label(format!(
                "{} deployed · owner {}",
                Self::fleet(&p).len(),
                p.name
            ));
        }
        if !self.connected {
            ui.colored_label(AMBER, "Disconnected · last known fleet; orders unavailable");
        } else if !fresh {
            ui.colored_label(AMBER, "Fleet telemetry stale · waiting for authority");
        }
        if self.embedded && self.embedded_paused {
            if !physical_bay {
                ui.colored_label(
                    MUTED,
                    "Simulation paused · choose Resume simulation to issue orders",
                );
            }
        } else if p.station_avatar.is_some() {
            ui.colored_label(MUTED, "On foot · return to the pilot seat for manual drone orders. Fleet and cameras remain viewable.");
        } else if p.location == Location::Docked {
            if !physical_bay {
                ui.colored_label(
                    MUTED,
                    "Docked · undock before deploying or directing drones",
                );
            }
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
        let finite_bay = ui
            .scope(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.spacing_mut().button_padding.y = 2.0;
                self.drone_bay_controls(ui, &p, available)
            })
            .inner;
        if !finite_bay {
            ui.small(
                "Legacy launch capacity: five deployed drones per class; there is no stored bay inventory.",
            );
        }
        if finite_bay {
            ui.collapsing("Authority and ship power details", |ui| {
                self.manual_operations_summary(ui);
            });
        } else {
            self.manual_operations_summary(ui);
        }
        let power = power_spec(p.ship_class);
        if let Some(capacitor) = &p.power {
            ui.label(format!(
                "Shared ship capacitor · {} / {} J",
                capacitor.charge_j, capacitor.capacity_j
            ));
        } else {
            ui.label("Ship capacitor telemetry unavailable");
        }
        ui.small(if matches!(supported_drone_cargo(&p), Ok(Some(_))) {
            "Drones draw from the ship capacitor. Mining-drone ore is held separately until a confirmed receiver transfer."
        } else { "Drones draw from the ship capacitor; separate drone batteries are not reported. Legacy worlds do not report drone cargo holds." });
        ui.small(format!(
            "Owner capacitor: {} J / mining cycle · {} J / weapon shot",
            power.drone_mining_cycle_cost_j, power.drone_attack_cost_j
        ));
        if !finite_bay {
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
        }
        ui.separator();
        let bay_available = self
            .snapshot
            .as_ref()
            .and_then(|s| s.pilots.iter().find(|p| p.name == self.pilot))
            .is_some_and(flight3d::bay_view_available);
        ui.small("OPTIONAL COCKPIT SCREEN SOURCE · independent of the bay monitor above");
        let camera_chosen = camera_sources::selector(
            ui,
            &mut self.camera_source,
            &mut self.drone_camera,
            bay_available,
        );
        if p.location == Location::Docked || p.station_avatar.is_some() {
            ui.colored_label(MUTED, if finite_bay {
                "The independent bay monitor is available from the pilot seat. These optional source controls affect the cockpit multipurpose display, not ship orders."
            } else {
                "Legacy cockpit camera video appears in free flight. This saved world has no physical bay camera."
            });
        }
        if camera_chosen && !matches!(self.camera_source, camera_sources::CameraSource::Drones) {
            self.remote_ship = None;
        }
        if self.remote_ship.is_some() {
            ui.colored_label(AMBER, "Worker camera overrides the selected source. Choose a ship camera to switch away from the worker view.");
        }
        let Some(drone) = self.selected_drone_view(&p) else {
            if finite_bay {
                return;
            }
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
        let controlled = available
            && drone.signal_strength > 0.0
            && (!finite_bay
                || (flight3d::bay_view_available(&p)
                    && p.drone_bay.as_ref().is_some_and(|bay| {
                        bay.constrained(id).is_none()
                            && !bay.batch.as_ref().is_some_and(|b| b.active())
                    })));
        ui.add_space(6.0);
        ui.strong(format!("SELECTED DRONE #{id}"));
        let readout = crate::drone_telemetry::DroneReadout::new(drone, self.drone_telemetry_time);
        ui.colored_label(
            MUTED,
            format!(
                "{} · confirmed snapshot {} ms",
                self.drone_telemetry_time.availability.label(),
                self.drone_telemetry_time.server_ms
            ),
        );
        if let Some(cycle) = &readout.cycle {
            ui.add(egui::ProgressBar::new(cycle.fraction).text(cycle.label.clone()));
        } else {
            ui.label("No confirmed mining cycle");
        }
        egui::Grid::new("selected-drone-authority")
            .num_columns(2)
            .min_col_width(0.0)
            .max_col_width(((ui.available_width() - 20.0) / 2.0).max(1.0))
            .spacing(egui::vec2(20.0, 7.0))
            .show(ui, |ui| {
                for (label, value) in [
                    ("Class", format!("{:?}", drone.class)),
                    ("Confirmed task", readout.task.clone()),
                    ("Assignment", drone_assignment(&drone.mode)),
                    (
                        "Signal",
                        if drone.signal_strength.is_finite() {
                            format!("{:.0}%", drone.signal_strength.clamp(0.0, 1.0) * 100.0)
                        } else {
                            "Unavailable".into()
                        },
                    ),
                    ("Hull", format!("{} HP", readout.hit_points)),
                    (
                        "Speed",
                        readout
                            .speed
                            .map_or_else(|| "Unavailable".into(), |v| format!("{v:.1} m/s")),
                    ),
                    (
                        "Heading",
                        readout.heading_yaw_pitch_deg.map_or_else(
                            || "Unavailable".into(),
                            |[yaw, pitch]| format!("Yaw {yaw:+.1}° · pitch {pitch:+.1}°"),
                        ),
                    ),
                    ("Orientation", drone_orientation_label(drone).into()),
                    ("Cargo", "No separate drone cargo reported".into()),
                    ("Energy", "Ship-supplied capacitor; no drone battery".into()),
                    (
                        "Power / thrust",
                        "Per-drone power and thrust unmodeled".into(),
                    ),
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
            if !finite_bay {
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
        if matches!(self.camera_source, camera_sources::CameraSource::Drones) {
            ui.separator();
            ui.small("DRONE CAMERA OPTIONS / VIEW ONLY");
            ui.horizontal_wrapped(|ui| {
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
            ui.small("Select a fleet row to switch drone cameras. Split view shows at most four cached feeds.");
            if self.remote_ship.is_some() {
                ui.colored_label(AMBER, "Worker feed overrides drone screen.");
                if ui.small_button("Restore drone feed").clicked() {
                    self.remote_ship = None;
                }
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

// A row's service/queue phase takes precedence over any retained mining intent.
// Never advance this gauge from presentation time or borrow a peer's cycle.
fn bay_row_cycle(
    pilot: &PilotView,
    drone_id: u64,
    phase: &str,
    time: crate::drone_telemetry::TelemetryTime,
) -> Option<crate::drone_telemetry::CycleReadout> {
    if phase != "Mining" {
        return None;
    }
    Cockpit::fleet(pilot)
        .iter()
        .find(|drone| drone.id == drone_id)
        .and_then(|drone| crate::drone_telemetry::DroneReadout::new(drone, time).cycle)
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
                orientation_xyzw: None,
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

/// A private journal is usable only for the viewed owner and the same validated
/// public stock. A partial/stale private response must not authorize a new ID.
fn bay_private_state<'a>(
    pilot: &PilotView,
    owner: &str,
    assets: Option<&'a AssetsView>,
) -> Result<&'a DroneBayState, &'static str> {
    if pilot.name != owner {
        return Err("View only · private bay capability belongs to another vessel owner");
    }
    let public = pilot
        .drone_bay
        .as_ref()
        .ok_or("Public drone bay unavailable")?;
    let state = assets
        .and_then(|a| a.drone_bay.as_ref())
        .ok_or("View only · private bay journal unavailable")?;
    if !arkline_protocol::drone_bay_schema_supported(state.schema_version)
        || state.schema_version != public.schema_version
        || state.batches.as_ref().map(|j| j.operations.last())
            != public.batch_capability.map(|_| public.batch.as_ref())
        || state
            .batches
            .as_ref()
            .is_some_and(|j| j.validated_next_id(&state.binding).is_none())
        || state.binding != public.binding
        || state.stock.len() != public.cradles.len()
        || state.handling_operation != public.machinery.handling_operation
        || state.cleanup != public.machinery.cleanup
    {
        return Err("View only · private bay journal does not match this vessel snapshot");
    }
    let mut operations = std::collections::BTreeSet::new();
    if state.operations.iter().any(|op| {
        op.operation_id == 0
            || op.operation_id > state.high_watermark
            || op.binding != state.binding
            || !operations.insert(op.operation_id)
    }) {
        return Err("View only · private operation IDs or vessel binding are inconsistent");
    }
    let mut ids = std::collections::BTreeSet::new();
    for entry in &state.stock {
        if !ids.insert(entry.drone_id)
            || !public.cradles.iter().any(|c| {
                c.drone_id == entry.drone_id
                    && c.class == entry.class
                    && c.home_cradle == entry.home_cradle
                    && match entry.status {
                        DroneStockStatus::Stored { hit_points } => {
                            c.presence == DroneBayPresence::Stored && c.hit_points == hit_points
                        }
                        DroneStockStatus::Active => c.presence == DroneBayPresence::Active,
                        DroneStockStatus::Destroyed { .. } => {
                            c.presence == DroneBayPresence::Destroyed
                        }
                    }
            })
        {
            return Err("View only · private and public drone identities disagree");
        }
    }
    if public.motions.iter().any(|m| {
        !state.operations.iter().any(|op| {
            op.operation_id == m.operation_id
                && op.drone_id == m.drone_id
                && op.phase == m.phase
                && op.binding == public.binding
        })
    }) {
        return Err("View only · operation journal is awaiting the current motion phase");
    }
    if state
        .operations
        .iter()
        .filter(|op| !op.phase.terminal())
        .any(|op| {
            !public.motions.iter().any(|m| {
                m.operation_id == op.operation_id
                    && m.drone_id == op.drone_id
                    && m.phase == op.phase
            })
        })
        || state.cleanup.as_ref().is_some_and(|cleanup| {
            !state.operations.iter().any(|op| {
                op.operation_id == cleanup.operation_id
                    && op.phase == DroneBayPhase::Destroyed
                    && op.binding == public.binding
            })
        })
    {
        return Err("View only · retained operation does not match the current bay phase");
    }
    Ok(state)
}
fn bay_next_operation(state: &DroneBayState) -> Option<u64> {
    let next = state.high_watermark.checked_add(1)?;
    let mut seen = std::collections::BTreeSet::new();
    state
        .operations
        .iter()
        .all(|op| {
            op.operation_id > 0
                && op.operation_id < next
                && op.binding == state.binding
                && seen.insert(op.operation_id)
        })
        .then_some(next)
}
fn drone_orientation_label(drone: &DroneView) -> &'static str {
    match drone.orientation_xyzw {
        Some(orientation_xyzw)
            if (arkline_protocol::drone_bay_geometry::BayPose {
                position_m: drone.position,
                orientation_xyzw,
            })
            .valid() =>
        {
            "Full attitude reported"
        }
        Some(_) => "Attitude unavailable · invalid telemetry",
        None => "Heading only; roll unmodeled",
    }
}
fn bay_carrier_stopped(pilot: &PilotView) -> bool {
    pilot.flight.as_ref().is_some_and(|f| {
        matches!(f.mode, NavigationMode::Stop)
            && f.warp.is_none()
            && f.velocity
                .iter()
                .chain(&f.angular_velocity)
                .all(|v| v.is_finite() && v.abs() <= 1e-6)
    })
}

#[cfg(test)]
mod bay_operation_tests {
    use super::*;
    use arkline_protocol::{drone_bay_layout as geometry, *};

    fn cargo_fixture(ctx: &egui::Context) -> Cockpit {
        use arkline_protocol::drone_dropoff_geometry as receiver;
        let mut c = batch_fixture(ctx);
        active(&mut c, None);
        let pilot = &mut c.snapshot.as_mut().unwrap().pilots[0];
        pilot.ore = 300;
        pilot.drones[0].id = 106;
        pilot.drones[0].class = DroneClass::Mining;
        let bay = pilot.drone_bay.as_mut().unwrap();
        bay.cradles[0].presence = DroneBayPresence::Stored;
        bay.cradles[5].presence = DroneBayPresence::Active;
        let binding = supported_drone_cargo_registration().bind("owned-pioneer".into());
        let holds: Vec<_> = (106..111)
            .map(|drone_id| DroneOreHoldView {
                drone_id,
                ore_units: if drone_id == 106 { 75 } else { 0 },
                capacity_units: 100,
                cycle_reserved_units: 0,
            })
            .collect();
        pilot.drone_cargo = Some(DroneCargoPublicState {
            schema_version: DRONE_CARGO_SCHEMA_VERSION,
            binding: binding.clone(),
            holds: holds.clone(),
            incoming_reserved_units: 75,
            operations: vec![],
            receiver_operation: None,
            cleanup: None,
            motion: None,
            groups: receiver::mechanism_ids()
                .into_iter()
                .zip(receiver::reference_poses())
                .map(|(id, pose)| DroneBayMechanismView {
                    semantic_id: id.semantic_id().into(),
                    confirmed_pose: pose,
                    segment: None,
                })
                .collect(),
        });
        let assets = c.assets.as_mut().unwrap();
        let stock = &mut assets.drone_bay.as_mut().unwrap().stock;
        stock[0].status = DroneStockStatus::Stored { hit_points: 37 };
        stock[5].status = DroneStockStatus::Active;
        assets.drone_cargo = Some(DroneCargoState {
            schema_version: DRONE_CARGO_SCHEMA_VERSION,
            binding,
            initialized_at_ms: 1000,
            holds: holds
                .into_iter()
                .map(|hold| DroneOreHold {
                    destroyed_at_ms: None,
                    intent_generation: 0,
                    mining_intent: None,
                    drone_id: hold.drone_id,
                    ore_units: hold.ore_units,
                    cycle_high_watermark: 0,
                    cycle: None,
                })
                .collect(),
            lost_ore_units: 0,
            high_watermark: 0,
            operations: vec![],
            receiver_operation: None,
            cleanup: None,
        });
        c.selected_drone = Some(106);
        assert!(supported_drone_cargo(&c.own().unwrap()).unwrap().is_some());
        c
    }
    #[test]
    fn bay_row_cycles_use_exact_identity_confirmed_time_and_service_precedence() {
        use crate::drone_telemetry::{Availability, TelemetryTime};
        let ctx = egui::Context::default();
        let mut c = cargo_fixture(&ctx);
        let pilot = &mut c.snapshot.as_mut().unwrap().pilots[0];
        pilot.drones[0].mode = DroneMode::Mining { target_id: 1 };
        pilot.drones[0].mining_cycle = Some(DroneMiningCycle {
            started_at_ms: 1000,
            completes_at_ms: 9000,
        });
        let before = serde_json::to_value(&*pilot).unwrap();
        for availability in [
            Availability::Live,
            Availability::Paused,
            Availability::Stale,
            Availability::Disconnected,
        ] {
            let time = TelemetryTime {
                server_ms: 5000,
                availability,
            };
            let cycle = bay_row_cycle(pilot, 106, "Mining", time).unwrap();
            assert_eq!(cycle.fraction, 0.5);
            assert_eq!(cycle.label, "50% / MINING CYCLE");
            assert!(bay_row_cycle(pilot, 107, "Mining", time).is_none());
            for phase in [
                "Stored",
                "Destroyed",
                "Queued",
                "Transferring confirmed ore",
                "Ore drop-off held",
            ] {
                assert!(bay_row_cycle(pilot, 106, phase, time).is_none());
            }
        }
        assert!(bay_row_cycle(pilot, 106, "Mining", TelemetryTime::default()).is_none());
        let finished = bay_row_cycle(
            pilot,
            106,
            "Mining",
            TelemetryTime {
                server_ms: 9000,
                availability: Availability::Live,
            },
        )
        .unwrap();
        assert_eq!(finished.label, "100% / AWAITING AUTHORITY");
        assert_eq!(serde_json::to_value(&*pilot).unwrap(), before);
    }

    #[test]
    fn bay_row_cycle_gauge_keeps_five_row_geometry_and_selection() {
        use crate::drone_telemetry::{Availability, TelemetryTime};
        let ctx = egui::Context::default();
        let mut c = cargo_fixture(&ctx);
        c.snapshot.as_mut().unwrap().pilots[0].drones[0].mode = DroneMode::Mining { target_id: 1 };
        c.snapshot.as_mut().unwrap().pilots[0].drones[0].mining_cycle = Some(DroneMiningCycle {
            started_at_ms: 1000,
            completes_at_ms: 9000,
        });
        let pilot = c.own().unwrap();
        let public = pilot.drone_bay.as_ref().unwrap();
        let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        for (availability, expected) in [
            (Availability::Live, "Mine 50%"),
            (Availability::Paused, "Paused 50%"),
            (Availability::Stale, "Last 50%"),
            (Availability::Disconnected, "Last 50%"),
        ] {
            c.drone_telemetry_time = TelemetryTime {
                server_ms: 5000,
                availability,
            };
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(900., 650.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.set_width(350.);
                        c.drone_bay_five_rows(ui, &pilot, public);
                    });
                },
            );
            let labels: Vec<_> = output
                .shapes
                .into_iter()
                .filter_map(|shape| match shape.shape {
                    egui::Shape::Text(text)
                        if shape.clip_rect.contains_rect(text.visual_bounding_rect()) =>
                    {
                        Some((
                            text.galley.job.text.clone(),
                            text.visual_bounding_rect().center(),
                        ))
                    }
                    _ => None,
                })
                .collect();
            assert!(labels.iter().any(|(text, _)| text == expected));
            let rows: Vec<_> = labels
                .iter()
                .filter(|(text, _)| text.contains(" · home "))
                .collect();
            assert_eq!(rows.len(), 5);
            assert!(rows
                .windows(2)
                .all(|pair| (pair[1].1.y - pair[0].1.y - 28.).abs() < 0.1));
            assert_eq!(c.selected_drone, Some(106));
            assert!(rx.try_recv().is_err());
            assert_eq!(
                serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
                before
            );
        }
    }

    #[test]
    fn physical_docked_and_paused_context_remains_visible_without_duplicate_warning_rows() {
        for paused in [false, true] {
            let ctx = egui::Context::default();
            let mut c = cargo_fixture(&ctx);
            c.snapshot.as_mut().unwrap().pilots[0].location = Location::Docked;
            c.embedded = paused;
            c.embedded_paused = paused;
            let labels = render(&ctx, &mut c, vec![]);
            assert!(labels.iter().any(|(text, _)| text.contains(if paused {
                "PAUSED — Resume simulation"
            } else {
                "DOCKED — undock for orders"
            })));
            assert!(!labels
                .iter()
                .any(|(text, _)| text.starts_with("Docked · undock before")
                    || text.starts_with("Simulation paused · choose")));
            let (tx, rx) = std::sync::mpsc::channel();
            c.commands = Some(tx);
            click(&ctx, &mut c, "Drop ore & resume");
            assert!(rx.try_recv().is_err());
            assert!(!c.request_pending);
        }
    }

    #[test]
    fn receiver_monitor_is_view_only_and_never_substitutes_invalid_telemetry() {
        let ctx = egui::Context::default();
        let mut c = cargo_fixture(&ctx);
        c.camera_source = camera_sources::CameraSource::AllTurrets;
        let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        click(&ctx, &mut c, "Ore receiver");
        assert_eq!(c.camera_source, camera_sources::CameraSource::AllTurrets);
        assert_eq!(c.selected_drone, Some(106));
        assert!(rx.try_recv().is_err());
        assert_eq!(
            serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
            before
        );
        c.snapshot.as_mut().unwrap().pilots[0]
            .drone_cargo
            .as_mut()
            .unwrap()
            .binding
            .registration
            .receiver_revision = 999;
        assert!(render(&ctx, &mut c, vec![])
            .iter()
            .any(|(text, _)| text.contains("Selected ore receiver view unavailable")));
        click(&ctx, &mut c, "Inside bay");
        assert!(!render(&ctx, &mut c, vec![])
            .iter()
            .any(|(text, _)| text.contains("Selected ore receiver view unavailable")));
        assert!(rx.try_recv().is_err());
        assert_eq!(c.selected_drone, Some(106));
    }

    #[test]
    fn cargo_rows_and_dropoff_use_real_payload_and_owned_identified_request() {
        let ctx = egui::Context::default();
        let mut c = cargo_fixture(&ctx);
        let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
        let labels = render(&ctx, &mut c, vec![]);
        for expected in [
            "75/100 ore",
            "incoming reserved 75 · available 625",
            "Recall & stow (1)",
        ] {
            assert!(
                labels.iter().any(|(text, _)| text.contains(expected)),
                "missing {expected}"
            );
        }
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        click(&ctx, &mut c, "Drop ore & resume");
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::DroneCargo {
                request: DroneCargoRequest::BeginDropoff {
                    drone_id: 106,
                    operation_id: 1,
                    resume_mining: true,
                    ..
                }
            }
        ));
        click(&ctx, &mut c, "Drop ore & resume");
        assert!(rx.try_recv().is_err());
        assert_eq!(c.selected_drone, Some(106));
        assert_eq!(
            serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
            before
        );
    }
    #[test]
    fn cargo_invalid_foreign_stale_and_loaded_recovery_fail_closed() {
        for gate in [
            "foreign",
            "stale",
            "counter",
            "source",
            "headroom",
            "loaded-recover",
        ] {
            let ctx = egui::Context::default();
            let mut c = cargo_fixture(&ctx);
            match gate {
                "foreign" => c.pilot = "OtherOwner".into(),
                "stale" => {
                    c.observed = Instant::now() - Duration::from_secs(10);
                    c.frozen_time = Some(1000);
                }
                "counter" => {
                    c.assets
                        .as_mut()
                        .unwrap()
                        .drone_cargo
                        .as_mut()
                        .unwrap()
                        .high_watermark = 9
                }
                "source" => {
                    c.snapshot.as_mut().unwrap().pilots[0]
                        .drone_cargo
                        .as_mut()
                        .unwrap()
                        .binding
                        .registration
                        .receiver_revision = 999
                }
                "headroom" => c.snapshot.as_mut().unwrap().pilots[0].ore = 999,
                _ => {}
            }
            let (tx, rx) = std::sync::mpsc::channel();
            c.commands = Some(tx);
            let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
            if matches!(gate, "source" | "headroom") {
                assert!(render(&ctx, &mut c, vec![])
                    .iter()
                    .any(|(text, _)| text.contains("Ore receiver telemetry unavailable")));
            } else if gate == "foreign" {
                // A remote owner cannot borrow the controller journal even with identical vessel bytes.
                let pilot = c.snapshot.as_ref().unwrap().pilots[0].clone();
                let _ = ctx.run(Default::default(), |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| c.drone_cargo_controls(ui, &pilot, true));
                });
            } else {
                click(
                    &ctx,
                    &mut c,
                    if gate == "loaded-recover" {
                        "Recover selected drone"
                    } else {
                        "Drop ore & resume"
                    },
                );
            }
            assert!(rx.try_recv().is_err(), "{gate}");
            assert!(!c.request_pending);
            assert_eq!(
                serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
                before
            );
        }
    }
    #[test]
    fn cargo_hold_resume_cancel_and_cleanup_keep_retained_operation_identity() {
        for action in [
            "Hold drop-off",
            "Resume drop-off",
            "Cancel drop-off",
            "Resume receiver cleanup",
        ] {
            let ctx = egui::Context::default();
            let mut c = cargo_fixture(&ctx);
            let cargo = c.snapshot.as_mut().unwrap().pilots[0]
                .drone_cargo
                .as_mut()
                .unwrap();
            let held = action == "Resume drop-off";
            let cleanup = action == "Resume receiver cleanup";
            let operation = DroneDropoffOperation {
                operation_id: 1,
                binding: cargo.binding.clone(),
                drone_id: 106,
                requested_resume_mining: false,
                cancel_requested: cleanup,
                followup: DroneDropoffFollowup::Hold,
                requested_units: 75,
                committed_units: 0,
                remaining_units: 75,
                phase: if cleanup {
                    DroneDropoffPhase::Cancelled
                } else if held {
                    DroneDropoffPhase::Held
                } else {
                    DroneDropoffPhase::Queued
                },
                held_phase: held.then_some(DroneDropoffPhase::Queued),
                accepted_at_ms: 1000,
                updated_at_ms: 1000,
                next_batch_at_ms: None,
                message: "Retained operation fixture".into(),
                receipts: vec![],
            };
            cargo.operations.push(operation.clone());
            if cleanup {
                cargo.cleanup = Some(DroneBayCleanupView {
                    operation_id: 1,
                    held: true,
                    reason: "Safe disconnect required".into(),
                });
            }
            let private = c.assets.as_mut().unwrap().drone_cargo.as_mut().unwrap();
            private.high_watermark = 1;
            private.operations.push(operation);
            private.cleanup = cargo.cleanup.clone();
            assert!(supported_drone_cargo(&c.own().unwrap()).unwrap().is_some());
            let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            c.commands = Some(tx);
            let labels = render(&ctx, &mut c, vec![]);
            assert!(
                !labels
                    .iter()
                    .any(|(text, _)| text.starts_with("Deploy ")
                        || text.starts_with("Recall & stow (")),
                "active service exposes its own controls, not idle queue choices"
            );
            click(&ctx, &mut c, action);
            let Request::DroneCargo { request } = rx.try_recv().unwrap() else {
                panic!("expected identified cargo operation");
            };
            match action {
                "Hold drop-off" => assert!(matches!(
                    request,
                    DroneCargoRequest::Hold {
                        operation_id: 1,
                        ..
                    }
                )),
                "Cancel drop-off" => assert!(matches!(
                    request,
                    DroneCargoRequest::Cancel {
                        operation_id: 1,
                        ..
                    }
                )),
                _ => assert!(matches!(
                    request,
                    DroneCargoRequest::Resume {
                        operation_id: 1,
                        ..
                    }
                )),
            }
            if cleanup {
                assert!(!render(&ctx, &mut c, vec![])
                    .iter()
                    .any(|(text, _)| text == "Cancel drop-off"));
            }
            assert_eq!(
                serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
                before
            );
        }
    }
    fn queued_cargo_operation(c: &mut Cockpit, cleanup: bool) {
        let cargo = c.snapshot.as_mut().unwrap().pilots[0]
            .drone_cargo
            .as_mut()
            .unwrap();
        let operation = DroneDropoffOperation {
            operation_id: 1,
            binding: cargo.binding.clone(),
            drone_id: 106,
            requested_resume_mining: false,
            cancel_requested: cleanup,
            followup: DroneDropoffFollowup::Hold,
            requested_units: 75,
            committed_units: 0,
            remaining_units: 75,
            phase: if cleanup {
                DroneDropoffPhase::Cancelled
            } else {
                DroneDropoffPhase::Queued
            },
            held_phase: None,
            accepted_at_ms: 1000,
            updated_at_ms: 1000,
            next_batch_at_ms: None,
            message: "Queued service".into(),
            receipts: vec![],
        };
        cargo.operations.push(operation.clone());
        if cleanup {
            cargo.cleanup = Some(DroneBayCleanupView {
                operation_id: 1,
                held: true,
                reason: "Safe disconnect required".into(),
            });
        }
        let private = c.assets.as_mut().unwrap().drone_cargo.as_mut().unwrap();
        private.high_watermark = 1;
        private.operations.push(operation);
        private.cleanup = cargo.cleanup.clone();
        assert!(supported_drone_cargo(&c.own().unwrap()).unwrap().is_some());
    }
    #[test]
    fn receiver_queue_and_cleanup_readiness_preserve_stop_neutral_and_actual_job_target() {
        let ctx = egui::Context::default();
        for cleanup in [false, true] {
            let mut c = cargo_fixture(&ctx);
            queued_cargo_operation(&mut c, cleanup);
            assert_eq!(
                c.bay_maneuver_reason().is_some(),
                cleanup,
                "queued-only must not hold carrier motion"
            );
            let mut worker = c.own().unwrap();
            worker.name = "worker".into();
            worker.drone_cargo = None;
            worker.drone_bay = None;
            c.snapshot.as_mut().unwrap().pilots.push(worker);
            let job = |pilot| Request::StartResourceJob {
                target_units: 10,
                pilot,
            };
            assert!(c.bay_command_reason(&job(None)).is_some());
            assert!(c.bay_command_reason(&job(Some("worker".into()))).is_none());
            let (tx, rx) = std::sync::mpsc::channel();
            c.commands = Some(tx);
            c.ai_pin = Some(("retain pending AI".into(), 7));
            let before = serde_json::to_value(&c.snapshot).unwrap();
            c.send(job(None));
            assert!(rx.try_recv().is_err());
            assert!(!c.request_pending);
            assert_eq!(c.ai_pin, Some(("retain pending AI".into(), 7)));
            assert_eq!(serde_json::to_value(&c.snapshot).unwrap(), before);
            for request in [
                Request::Navigate {
                    mode: NavigationMode::Stop,
                },
                Request::PilotInput {
                    translation: [0.; 3],
                    rotation: [0.; 3],
                },
            ] {
                assert!(c.bay_command_reason(&request).is_none());
            }
        }
    }
    #[test]
    fn exchange_incoming_headroom_is_not_physical_free_space_and_legacy_is_unchanged() {
        use crate::exchange_readiness::ExchangeAction;
        let ctx = egui::Context::default();
        let mut c = cargo_fixture(&ctx);
        c.snapshot.as_mut().unwrap().pilots[0].location = Location::Docked;
        c.snapshot.as_mut().unwrap().pilots[0].credits = 10000;
        c.snapshot.as_mut().unwrap().orders = vec![
            SellOrder {
                id: 1,
                seller: "other".into(),
                units: 700,
                unit_price: 1,
            },
            SellOrder {
                id: 2,
                seller: c.pilot.clone(),
                units: 626,
                unit_price: 1,
            },
        ];
        assert!(c
            .exchange_action_reason(ExchangeAction::Buy {
                order_id: 1,
                units: 625
            })
            .is_none());
        assert!(c
            .exchange_action_reason(ExchangeAction::Buy {
                order_id: 1,
                units: 626
            })
            .is_some());
        assert!(c
            .exchange_action_reason(ExchangeAction::Cancel { order_id: 2 })
            .is_some());
        c.snapshot.as_mut().unwrap().orders[1].units = 625;
        assert!(c
            .exchange_action_reason(ExchangeAction::Cancel { order_id: 2 })
            .is_none());
        c.snapshot.as_mut().unwrap().pilots[0].drone_cargo = None;
        c.snapshot.as_mut().unwrap().orders[1].units = 700;
        assert!(c
            .exchange_action_reason(ExchangeAction::Buy {
                order_id: 1,
                units: 700
            })
            .is_none());
        assert!(c
            .exchange_action_reason(ExchangeAction::Cancel { order_id: 2 })
            .is_none());
    }

    #[test]
    fn recall_service_uses_owning_queue_controls_not_direct_dropoff_commands() {
        let ctx = egui::Context::default();
        let mut c = cargo_fixture(&ctx);
        queued_cargo_operation(&mut c, false);
        let pilot = &mut c.snapshot.as_mut().unwrap().pilots[0];
        pilot.drone_cargo.as_mut().unwrap().operations[0].followup =
            DroneDropoffFollowup::RecallAll { batch_id: 1 };
        let bay = pilot.drone_bay.as_mut().unwrap();
        let batch = DroneBayBatchOperation {
            batch_id: 1,
            binding: bay.binding.clone(),
            kind: DroneBayBatchKind::RecallAll,
            phase: DroneBayBatchPhase::Running,
            held: false,
            members: vec![DroneBayBatchMember {
                drone_id: 106,
                home_cradle: 5,
                operation_id: None,
                service_operation_ids: vec![1],
                status: DroneBayBatchMemberStatus::Servicing,
            }],
            current_operation_id: None,
            current_service_operation_id: Some(1),
            accepted_at_ms: 1000,
            updated_at_ms: 1000,
            message: "Unload before stow".into(),
        };
        assert!(batch.validate_shape(&bay.binding));
        bay.batch = Some(batch.clone());
        let assets = c.assets.as_mut().unwrap();
        assets.drone_cargo.as_mut().unwrap().operations[0].followup =
            DroneDropoffFollowup::RecallAll { batch_id: 1 };
        let journal = assets.drone_bay.as_mut().unwrap().batches.as_mut().unwrap();
        journal.high_watermark = 1;
        journal.operations.push(batch);
        let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
        let labels = render(&ctx, &mut c, vec![]);
        assert!(labels
            .iter()
            .any(|(text, _)| text.contains("Recall All · 0/1 successful")));
        for forbidden in [
            "Hold drop-off",
            "Resume drop-off",
            "Cancel drop-off",
            "Resume receiver cleanup",
        ] {
            assert!(!labels.iter().any(|(text, _)| text == forbidden));
        }
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        click(&ctx, &mut c, "Hold queue");
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::DroneBay {
                request: DroneBayRequest::HoldBatch { batch_id: 1, .. }
            }
        ));
        assert!(rx.try_recv().is_err());
        assert_eq!(
            serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
            before
        );
    }
    fn batch_fixture(ctx: &egui::Context) -> Cockpit {
        let mut c = fixture(ctx);
        let public = c.snapshot.as_mut().unwrap().pilots[0]
            .drone_bay
            .as_mut()
            .unwrap();
        public.schema_version = arkline_protocol::DRONE_BAY_SCHEMA_VERSION;
        public.batch_capability = Some(1);
        let private = c.assets.as_mut().unwrap().drone_bay.as_mut().unwrap();
        private.schema_version = arkline_protocol::DRONE_BAY_SCHEMA_VERSION;
        private.batches = Some(arkline_protocol::DroneBayBatchJournal::default());
        c
    }
    #[test]
    fn batch_buttons_send_identified_roster_intent_once_without_mutating_stock() {
        for recall in [false, true] {
            let ctx = egui::Context::default();
            let mut c = batch_fixture(&ctx);
            if recall {
                active(&mut c, None);
            }
            let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            c.commands = Some(tx);
            if !recall {
                click(&ctx, &mut c, "Mining");
            }
            let label = if recall {
                "Recall All (1)"
            } else {
                "Deploy Mining (5)"
            };
            click(&ctx, &mut c, label);
            let request = rx.try_recv().unwrap();
            assert!(
                matches!(
                    request,
                    Request::DroneBay {
                        request: DroneBayRequest::BeginBatch {
                            batch_id: 1,
                            batch_kind: DroneBayBatchKind::RecallAll,
                            ..
                        }
                    }
                ) == recall
            );
            if !recall {
                assert!(matches!(
                    request,
                    Request::DroneBay {
                        request: DroneBayRequest::BeginBatch {
                            batch_id: 1,
                            batch_kind: DroneBayBatchKind::DeployClass {
                                class: DroneClass::Mining
                            },
                            ..
                        }
                    }
                ));
            }
            click(&ctx, &mut c, label);
            assert!(rx.try_recv().is_err());
            assert_eq!(
                serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
                before
            );
        }
    }
    fn attach_active_batch(c: &mut Cockpit, held: bool) {
        let state = c.assets.as_mut().unwrap().drone_bay.as_mut().unwrap();
        let batch = arkline_protocol::DroneBayBatchOperation {
            batch_id: 1,
            binding: state.binding.clone(),
            kind: DroneBayBatchKind::DeployClass {
                class: DroneClass::Recon,
            },
            phase: arkline_protocol::DroneBayBatchPhase::Running,
            held,
            current_service_operation_id: None,
            members: vec![arkline_protocol::DroneBayBatchMember {
                service_operation_ids: vec![],
                drone_id: 101,
                home_cradle: 0,
                operation_id: Some(8),
                status: DroneBayBatchMemberStatus::Active,
            }],
            current_operation_id: Some(8),
            accepted_at_ms: 1000,
            updated_at_ms: 1000,
            message: "Actual accepted route".into(),
        };
        state.batches = Some(arkline_protocol::DroneBayBatchJournal {
            version: 1,
            high_watermark: 1,
            operations: vec![batch.clone()],
        });
        c.snapshot.as_mut().unwrap().pilots[0]
            .drone_bay
            .as_mut()
            .unwrap()
            .batch = Some(batch);
    }
    #[test]
    fn queue_controls_own_current_route_without_individual_resume_or_stock_mutation() {
        for (held, label) in [
            (false, "Hold queue"),
            (true, "Resume queue"),
            (true, "Cancel remaining"),
        ] {
            let ctx = egui::Context::default();
            let mut c = batch_fixture(&ctx);
            active(
                &mut c,
                Some(if held {
                    DroneBayPhase::Held
                } else {
                    DroneBayPhase::TransferringOut
                }),
            );
            attach_active_batch(&mut c, held);
            assert!(flight3d::bay_view_available(&c.own().unwrap()));
            let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            c.commands = Some(tx);
            let labels = render(&ctx, &mut c, vec![]);
            assert!(!labels
                .iter()
                .any(|(s, _)| s == "Resume bay operation" || s == "Hold bay operation"));
            click(&ctx, &mut c, label);
            let Request::DroneBay { request } = rx.try_recv().unwrap() else {
                panic!("expected batch command")
            };
            match label {
                "Hold queue" => assert!(matches!(
                    request,
                    DroneBayRequest::HoldBatch { batch_id: 1, .. }
                )),
                "Resume queue" => assert!(matches!(
                    request,
                    DroneBayRequest::ResumeBatch { batch_id: 1, .. }
                )),
                _ => assert!(matches!(
                    request,
                    DroneBayRequest::CancelBatch { batch_id: 1, .. }
                )),
            }
            click(&ctx, &mut c, label);
            assert!(rx.try_recv().is_err());
            assert_eq!(
                serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
                before
            );
        }
    }
    #[test]
    fn batch_commands_reject_stale_or_mismatched_private_journal() {
        for stale in [false, true] {
            let ctx = egui::Context::default();
            let mut c = batch_fixture(&ctx);
            if stale {
                c.timeline_origin = Some(6000);
            } else {
                c.assets
                    .as_mut()
                    .unwrap()
                    .drone_bay
                    .as_mut()
                    .unwrap()
                    .batches
                    .as_mut()
                    .unwrap()
                    .high_watermark = 3;
            }
            let (tx, rx) = std::sync::mpsc::channel();
            c.commands = Some(tx);
            click(&ctx, &mut c, "Mining");
            click(&ctx, &mut c, "Deploy Mining (5)");
            assert!(rx.try_recv().is_err());
        }
    }
    #[test]
    fn legacy_empty_selection_clears_without_changing_stock_or_switching_drone() {
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx);
        c.assets = None;
        let pilot = &mut c.snapshot.as_mut().unwrap().pilots[0];
        pilot.drone_bay = None;
        pilot.drone = None;
        pilot.drones.clear();
        c.selected_drone = Some(5);
        let before = serde_json::to_value(&c.snapshot).unwrap();
        c.reconcile_empty_legacy_drone_selection();
        assert_eq!(c.selected_drone, None);
        assert!(c.notice.contains("Selection cleared"));
        assert_eq!(serde_json::to_value(&c.snapshot).unwrap(), before);
        c.selected_drone = Some(5);
        c.connected = false;
        c.reconcile_empty_legacy_drone_selection();
        assert_eq!(
            c.selected_drone,
            Some(5),
            "disconnect is not a recovery receipt"
        );
        c.connected = true;
        c.timeline_origin = Some(4000);
        c.reconcile_empty_legacy_drone_selection();
        assert_eq!(
            c.selected_drone,
            Some(5),
            "stale telemetry cannot retire selection"
        );
    }

    #[test]
    fn five_home_rows_switch_class_without_changing_identity_or_commands() {
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx);
        let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        for (label, first) in [("Mining", 106), ("Combat", 111), ("Recon", 101)] {
            click(&ctx, &mut c, label);
            let labels = render(&ctx, &mut c, vec![]);
            let rows: Vec<_> = labels
                .iter()
                .filter(|(text, _)| text.starts_with('#') && text.contains(" · home "))
                .collect();
            assert_eq!(rows.len(), 5);
            assert!(
                rows.windows(2)
                    .all(|rows| rows[1].1.y - rows[0].1.y <= 28.1),
                "five actual home rows must remain compact without font scaling"
            );
            for (index, (text, _)) in rows.iter().enumerate() {
                assert!(text.starts_with(&format!("#{} ·", first + index as u64)));
            }
            assert_eq!(c.selected_drone, Some(101));
            assert!(rx.try_recv().is_err());
        }
        assert_eq!(
            serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
            before
        );
    }

    #[test]
    fn physical_stored_selection_and_captain_camera_survive_bay_monitor_choices() {
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx);
        c.camera_source = camera_sources::CameraSource::AllTurrets;
        c.drone_camera = false;
        let before = serde_json::to_value(&c.snapshot).unwrap();
        c.reconcile_empty_legacy_drone_selection();
        assert_eq!(c.selected_drone, Some(101));
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        click(&ctx, &mut c, "Doorway");
        assert!(c.bay_monitor_doorway);
        click(&ctx, &mut c, "Inside bay");
        assert!(!c.bay_monitor_doorway);
        assert_eq!(c.camera_source, camera_sources::CameraSource::AllTurrets);
        assert!(!c.drone_camera);
        assert_eq!(c.selected_drone, Some(101));
        assert!(rx.try_recv().is_err());
        assert_eq!(serde_json::to_value(&c.snapshot).unwrap(), before);
    }

    fn fixture(ctx: &egui::Context) -> Cockpit {
        let mut c = crate::tests::view_test_cockpit(ctx);
        let p = &mut c.snapshot.as_mut().unwrap().pilots[0];
        p.flight.as_mut().unwrap().docking = Some(serde_json::from_value(serde_json::json!({
            "version":1,"host_vessel_id":"owned-pioneer","berth_id":0,"operation_id":0,
            "phase":"free","segment":null,"clamps":0.,"updated_at_ms":1000,"hold_reason":null,"departing":false
        })).unwrap());
        let binding = DroneBayBinding {
            vessel_id: "owned-pioneer".into(),
            geometry_revision: geometry::DRONE_BAY_REGISTRATION_REVISION,
            source_sha256: geometry::DRONE_BAY_SOURCE_SHA256.into(),
        };
        let cradles: Vec<_> = geometry::home_poses()
            .into_iter()
            .enumerate()
            .map(|(i, body_pose)| DroneCradleView {
                drone_id: 101 + i as u64,
                class: DroneBayState::expected_class(i as u8).unwrap(),
                hit_points: 37,
                home_cradle: i as u8,
                presence: DroneBayPresence::Stored,
                body_pose,
            })
            .collect();
        let stock = cradles
            .iter()
            .map(|e| DroneStockEntry {
                drone_id: e.drone_id,
                class: e.class,
                home_cradle: e.home_cradle,
                status: DroneStockStatus::Stored {
                    hit_points: e.hit_points,
                },
            })
            .collect();
        p.drone_bay = Some(DroneBayPublicState {
            batch_capability: None,
            batch: None,
            schema_version: DRONE_BAY_LEGACY_SCHEMA_VERSION,
            binding: binding.clone(),
            cradles,
            motions: vec![],
            machinery: DroneBayMachineryView {
                cleanup: None,
                handling_operation: None,
                door_fraction: 0.,
                door_segment: None,
                groups: geometry::mechanism_ids()
                    .into_iter()
                    .zip(geometry::reference_poses())
                    .map(|(id, pose)| DroneBayMechanismView {
                        semantic_id: id.into(),
                        confirmed_pose: pose,
                        segment: None,
                    })
                    .collect(),
            },
        });
        c.assets = Some(
            serde_json::from_value(
                serde_json::json!({"ships":[],"storage":[],"corporation":null,"invitations":[]}),
            )
            .unwrap(),
        );
        c.assets.as_mut().unwrap().drone_bay = Some(DroneBayState {
            batches: None,
            cleanup: None,
            schema_version: DRONE_BAY_LEGACY_SCHEMA_VERSION,
            binding,
            stock,
            initialization: DroneBayInitialization {
                fixture_version: 1,
                seeded_at_ms: 1,
                first_drone_id: 101,
            },
            high_watermark: 8,
            operations: vec![],
            handling_operation: None,
        });
        c.selected_drone = Some(101);
        assert!(flight3d::bay_view_available(&c.own().unwrap()));
        c
    }
    fn render(
        ctx: &egui::Context,
        c: &mut Cockpit,
        events: Vec<egui::Event>,
    ) -> Vec<(String, Pos2)> {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(800., 1000.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("bay_operations_test_workspace")
                        .show(ui, |ui| c.drone_controls(ui));
                });
            },
        )
        .shapes
        .into_iter()
        .filter_map(|s| match s.shape {
            egui::Shape::Text(t) if s.clip_rect.contains_rect(t.visual_bounding_rect()) => {
                Some((t.galley.job.text.clone(), t.visual_bounding_rect().center()))
            }
            _ => None,
        })
        .collect()
    }
    fn click(ctx: &egui::Context, c: &mut Cockpit, label: &str) {
        render(ctx, c, vec![]);
        let mut labels = render(ctx, c, vec![]);
        for _ in 0..20 {
            if labels.iter().any(|(s, _)| s == label) {
                break;
            }
            labels = render(
                ctx,
                c,
                vec![
                    egui::Event::PointerMoved(Pos2::new(780., 900.)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0., -200.),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            for _ in 0..4 {
                labels = render(ctx, c, vec![]);
            }
        }
        // Wheel scrolling is animated: wait for the actual widget position to
        // settle before a real pointer gesture, not an offscreen guessed click.
        for _ in 0..40 {
            labels = render(ctx, c, vec![]);
        }
        let pos = labels
            .iter()
            .find(|(s, _)| s == label)
            .unwrap_or_else(|| panic!("missing fully visible {label}: {labels:?}"))
            .1;
        render(ctx, c, vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            render(
                ctx,
                c,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
    }
    fn active(c: &mut Cockpit, phase: Option<DroneBayPhase>) {
        let p = &mut c.snapshot.as_mut().unwrap().pilots[0];
        let bay = p.drone_bay.as_mut().unwrap();
        bay.cradles[0].presence = DroneBayPresence::Active;
        p.drones=vec![serde_json::from_value(serde_json::json!({"id":101,"class":"recon","hit_points":37,"signal_strength":1.,"position":BELT_POSITION,"velocity":[0.,0.,0.],"heading":[1.,0.,0.],"mode":{"kind":"hold"},"updated_at_ms":1000})).unwrap()];
        let state = c.assets.as_mut().unwrap().drone_bay.as_mut().unwrap();
        state.stock[0].status = DroneStockStatus::Active;
        if let Some(phase) = phase {
            bay.motions.push(DroneBayMotionView {
                drone_id: 101,
                operation_id: 8,
                phase,
                frame: DroneBayMotionFrame::Body,
                confirmed_pose: bay.cradles[0].body_pose,
                segment: None,
                updated_at_ms: 1000,
            });
            bay.machinery.handling_operation = Some(8);
            state.handling_operation = Some(8);
            state.operations.push(DroneBayOperation {
                operation_id: 8,
                drone_id: 101,
                kind: DroneBayOperationKind::Launch,
                binding: state.binding.clone(),
                phase,
                held_phase: Some(DroneBayPhase::TransferringOut),
                accepted_at_ms: 1000,
                updated_at_ms: 1000,
                job_id: None,
                message: "Clearance hold".into(),
            });
        }
        assert!(flight3d::bay_view_available(&c.own().unwrap()));
    }
    #[test]
    fn stored_launch_uses_retained_id_hp_and_checked_operation_without_mutation() {
        for label in [
            "Launch Recon · 5 stored / 5",
            "Launch selected stored drone",
        ] {
            let ctx = egui::Context::default();
            let mut c = fixture(&ctx);
            let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            c.commands = Some(tx);
            let labels = render(&ctx, &mut c, vec![]);
            assert!(labels
                .iter()
                .any(|(s, _)| s.contains("15 stored · 0 active · 0 destroyed / 15")));
            for expected in ["#101 · home 1", "37 HP", "Stored", "n/r"] {
                assert!(
                    labels.iter().any(|(text, _)| text.contains(expected)),
                    "missing {expected}"
                );
            }
            assert!(!labels
                .iter()
                .any(|(s, _)| s.contains("inventory is not reported")));
            assert!(labels
                .iter()
                .any(|(s, _)| s
                    .contains("Docked in home cradle 1 · choose Bay overview or Recon rack")));
            click(&ctx, &mut c, label);
            assert_eq!(
                c.selected_drone,
                Some(101),
                "command must preserve selected identity"
            );
            assert!(matches!(
                rx.try_recv().unwrap(),
                Request::DroneBay {
                    request: DroneBayRequest::Launch {
                        drone_id: 101,
                        operation_id: 9,
                        ..
                    }
                }
            ));
            click(&ctx, &mut c, label);
            assert!(
                rx.try_recv().is_err(),
                "pending click must not reuse operation ID"
            );
            assert_eq!(
                serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
                before
            );
        }
    }
    #[test]
    fn free_recovery_and_held_operation_controls_use_exact_identified_receipts() {
        for (phase, label) in [
            (None, "Recover selected drone"),
            (Some(DroneBayPhase::Held), "Resume bay operation"),
            (Some(DroneBayPhase::TransferringOut), "Hold bay operation"),
            (Some(DroneBayPhase::Held), "Cancel bay operation"),
        ] {
            let ctx = egui::Context::default();
            let mut c = fixture(&ctx);
            active(&mut c, phase);
            let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            c.commands = Some(tx);
            click(&ctx, &mut c, label);
            let Request::DroneBay { request } = rx.try_recv().unwrap() else {
                panic!("not identified bay command")
            };
            assert_eq!(request.binding().vessel_id, "owned-pioneer");
            match (label, request) {
                (
                    "Recover selected drone",
                    DroneBayRequest::Recover {
                        drone_id: 101,
                        operation_id: 9,
                        ..
                    },
                )
                | (
                    "Resume bay operation",
                    DroneBayRequest::Resume {
                        operation_id: 8, ..
                    },
                )
                | (
                    "Hold bay operation",
                    DroneBayRequest::Hold {
                        operation_id: 8, ..
                    },
                )
                | (
                    "Cancel bay operation",
                    DroneBayRequest::Cancel {
                        operation_id: 8, ..
                    },
                ) => {}
                other => panic!("wrong receipt {other:?}"),
            }
            assert_eq!(
                serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
                before
            );
        }
    }
    #[test]
    fn finite_launch_fails_closed_for_authority_counter_owner_and_source_gaps() {
        for gate in [
            "stale",
            "offline",
            "paused",
            "pending",
            "foreign",
            "foreign-no-public",
            "private-missing",
            "binding",
            "overflow",
            "duplicate-next",
            "moving",
            "docked",
            "on-foot",
            "source",
        ] {
            let ctx = egui::Context::default();
            let mut c = fixture(&ctx);
            match gate {
                "stale" => c.observed = Instant::now() - Duration::from_secs(10),
                "offline" => c.connected = false,
                "paused" => c.embedded_paused = true,
                "pending" => c.request_pending = true,
                "foreign" | "foreign-no-public" => {
                    c.pilot = "different owner".into();
                    c.manual_remote = Some("pilot".into());
                    c.snapshot.as_mut().unwrap().remote_controls.push(RemoteControlLease { controller:c.pilot.clone(), pilot:"pilot".into() });
                    if gate=="foreign-no-public" {c.snapshot.as_mut().unwrap().pilots[0].drone_bay=None;}
                },
                "private-missing" => c.assets = None,
                "binding" => {
                    c.assets
                        .as_mut()
                        .unwrap()
                        .drone_bay
                        .as_mut()
                        .unwrap()
                        .binding
                        .vessel_id = "other".into()
                }
                "overflow" => {
                    c.assets
                        .as_mut()
                        .unwrap()
                        .drone_bay
                        .as_mut()
                        .unwrap()
                        .high_watermark = u64::MAX
                }
                "duplicate-next" => {
                    active(&mut c, Some(DroneBayPhase::Held));
                    c.assets
                        .as_mut()
                        .unwrap()
                        .drone_bay
                        .as_mut()
                        .unwrap()
                        .high_watermark = 7;
                }
                "moving" => {
                    c.snapshot.as_mut().unwrap().pilots[0]
                        .flight
                        .as_mut()
                        .unwrap()
                        .velocity[0] = 1.
                }
                "docked" => c.snapshot.as_mut().unwrap().pilots[0].location = Location::Docked,
                "on-foot" => c.snapshot.as_mut().unwrap().pilots[0].station_avatar=Some(serde_json::from_value(serde_json::json!({"position":[0.,0.,0.],"heading":0.,"moving":false,"updated_at_ms":1000})).unwrap()),
                "source" => {
                    c.snapshot.as_mut().unwrap().pilots[0]
                        .drone_bay
                        .as_mut()
                        .unwrap()
                        .binding
                        .geometry_revision = 999
                }
                _ => unreachable!(),
            }
            let (tx, rx) = std::sync::mpsc::channel();
            c.commands = Some(tx);
            let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
            let labels = render(&ctx, &mut c, vec![]);
            if matches!(gate, "source" | "foreign-no-public") {
                assert!(labels
                    .iter()
                    .any(|(s, _)| s.contains("capability unavailable for this viewed vessel")));
                assert!(
                    !labels.iter().any(|(s, _)| s.contains("destroyed / 15")),
                    "must not show another vessel's stock"
                );
            } else {
                let label = &labels
                    .iter()
                    .find(|(s, _)| s.starts_with("Launch Recon"))
                    .expect("disabled class control stays visible")
                    .0;
                click(&ctx, &mut c, label);
            }
            assert!(rx.try_recv().is_err(), "gate {gate}");
            assert!(
                !labels.iter().any(|(s, _)| s == "Launch Recon 0/5"),
                "no legacy fallback for {gate}"
            );
            assert_eq!(
                serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
                before
            );
        }
    }
    #[test]
    fn batch_destroyed_cleanup_uses_queue_resume_and_retains_terminal_outcome() {
        let ctx = egui::Context::default();
        let mut c = batch_fixture(&ctx);
        active(&mut c, Some(DroneBayPhase::Held));
        attach_active_batch(&mut c, true);
        let cleanup = DroneBayCleanupView {
            operation_id: 8,
            held: true,
            reason: "Blocked hardware".into(),
        };
        let p = &mut c.snapshot.as_mut().unwrap().pilots[0];
        p.drones.clear();
        let bay = p.drone_bay.as_mut().unwrap();
        bay.motions.clear();
        bay.cradles[0].presence = DroneBayPresence::Destroyed;
        bay.cradles[0].hit_points = 0;
        bay.machinery.handling_operation = None;
        bay.machinery.cleanup = Some(cleanup.clone());
        bay.batch.as_mut().unwrap().members[0].status = DroneBayBatchMemberStatus::Destroyed;
        let state = c.assets.as_mut().unwrap().drone_bay.as_mut().unwrap();
        state.handling_operation = None;
        state.cleanup = Some(cleanup);
        state.stock[0].status = DroneStockStatus::Destroyed { at_ms: 1001 };
        state.operations[0].phase = DroneBayPhase::Destroyed;
        state.batches.as_mut().unwrap().operations[0] = bay.batch.clone().unwrap();
        assert!(flight3d::bay_view_available(&c.own().unwrap()));
        let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        let labels = render(&ctx, &mut c, vec![]);
        assert!(!labels.iter().any(|(s, _)| s == "Resume hardware cleanup"));
        click(&ctx, &mut c, "Resume queue");
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::DroneBay {
                request: DroneBayRequest::ResumeBatch { batch_id: 1, .. }
            }
        ));
        assert_eq!(
            serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
            before
        );
        c.request_pending = false;
        let bay = c.snapshot.as_mut().unwrap().pilots[0]
            .drone_bay
            .as_mut()
            .unwrap();
        bay.machinery.cleanup = None;
        let batch = bay.batch.as_mut().unwrap();
        batch.phase = arkline_protocol::DroneBayBatchPhase::Completed;
        batch.held = false;
        batch.current_operation_id = None;
        let state = c.assets.as_mut().unwrap().drone_bay.as_mut().unwrap();
        state.cleanup = None;
        state.batches.as_mut().unwrap().operations[0] = batch.clone();
        let labels = render(&ctx, &mut c, vec![]);
        assert!(labels.iter().any(|(s, _)| s
            .contains("Last queue #1 · Completed · 0 successful · 0 cancelled · 1 destroyed")));
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn terminal_cleanup_resume_never_revives_destroyed_stock() {
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx);
        active(&mut c, Some(DroneBayPhase::Held));
        let cleanup = DroneBayCleanupView {
            operation_id: 8,
            held: true,
            reason: "Hardware obstruction".into(),
        };
        let p = &mut c.snapshot.as_mut().unwrap().pilots[0];
        p.drones.clear();
        let bay = p.drone_bay.as_mut().unwrap();
        bay.motions.clear();
        bay.cradles[0].presence = DroneBayPresence::Destroyed;
        bay.cradles[0].hit_points = 0;
        bay.machinery.handling_operation = None;
        bay.machinery.cleanup = Some(cleanup.clone());
        let state = c.assets.as_mut().unwrap().drone_bay.as_mut().unwrap();
        state.handling_operation = None;
        state.cleanup = Some(cleanup);
        state.stock[0].status = DroneStockStatus::Destroyed { at_ms: 1001 };
        state.operations[0].phase = DroneBayPhase::Destroyed;
        assert!(flight3d::bay_view_available(&c.own().unwrap()));
        let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        let labels = render(&ctx, &mut c, vec![]);
        assert!(labels
            .iter()
            .any(|(s, _)| s.contains("Destroyed · identity")));
        assert!(!labels
            .iter()
            .any(|(s, _)| s == "Cancel bay operation" || s == "Launch selected stored drone"));
        click(&ctx, &mut c, "Resume hardware cleanup");
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::DroneBay {
                request: DroneBayRequest::Resume {
                    operation_id: 8,
                    ..
                }
            }
        ));
        assert_eq!(
            serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
            before
        );
    }
    #[test]
    fn class_launch_skips_destroyed_identity_without_replacement_or_healing() {
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx);
        let bay = c.snapshot.as_mut().unwrap().pilots[0]
            .drone_bay
            .as_mut()
            .unwrap();
        bay.cradles[0].presence = DroneBayPresence::Destroyed;
        bay.cradles[0].hit_points = 0;
        c.assets.as_mut().unwrap().drone_bay.as_mut().unwrap().stock[0].status =
            DroneStockStatus::Destroyed { at_ms: 1000 };
        let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        click(&ctx, &mut c, "Launch Recon · 4 stored / 5");
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::DroneBay {
                request: DroneBayRequest::Launch {
                    drone_id: 102,
                    operation_id: 9,
                    ..
                }
            }
        ));
        assert_eq!(
            serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
            before
        );
    }
    #[test]
    fn operation_counter_rejects_overflow_reuse_and_duplicate_journal_ids() {
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx);
        active(&mut c, Some(DroneBayPhase::Held));
        let state = c.assets.as_mut().unwrap().drone_bay.as_mut().unwrap();
        assert_eq!(bay_next_operation(state), Some(9));
        state.high_watermark = 7;
        assert_eq!(bay_next_operation(state), None);
        state.high_watermark = u64::MAX;
        assert_eq!(bay_next_operation(state), None);
        state.high_watermark = 8;
        state.operations.push(state.operations[0].clone());
        assert_eq!(bay_next_operation(state), None);
    }
    #[test]
    fn constrained_active_entity_cannot_receive_navigation_mining_or_attack_orders() {
        for (class, index, label) in [
            (DroneClass::Recon, 0, "Drone to selected"),
            (DroneClass::Mining, 5, "Mine locked target"),
            (DroneClass::Attack, 10, "Attack selected drone"),
        ] {
            for held in [false, true] {
                let ctx = egui::Context::default();
                let mut c = fixture(&ctx);
                active(&mut c, held.then_some(DroneBayPhase::Held));
                let id = 101 + index as u64;
                let p = &mut c.snapshot.as_mut().unwrap().pilots[0];
                p.locked_asteroids = vec![7];
                let bay = p.drone_bay.as_mut().unwrap();
                bay.cradles[0].presence = DroneBayPresence::Stored;
                bay.cradles[index].presence = DroneBayPresence::Active;
                p.drones[0].id = id;
                p.drones[0].class = class;
                if let Some(m) = bay.motions.first_mut() {
                    m.drone_id = id;
                    m.confirmed_pose = bay.cradles[index].body_pose;
                }
                let state = c.assets.as_mut().unwrap().drone_bay.as_mut().unwrap();
                state.stock[0].status = DroneStockStatus::Stored { hit_points: 37 };
                state.stock[index].status = DroneStockStatus::Active;
                if let Some(op) = state.operations.first_mut() {
                    op.drone_id = id;
                }
                c.selected_drone = Some(id);
                if class == DroneClass::Attack {
                    let mut enemy = c.own().unwrap();
                    enemy.name = "enemy".into();
                    enemy.drone_bay = None;
                    enemy.drones[0].id = 900;
                    c.snapshot.as_mut().unwrap().pilots.push(enemy);
                    c.drone_attack_target = Some(("enemy".into(), 900));
                }
                assert!(flight3d::bay_view_available(&c.own().unwrap()));
                let (tx, rx) = std::sync::mpsc::channel();
                c.commands = Some(tx);
                let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
                click(&ctx, &mut c, label);
                if held {
                    assert!(rx.try_recv().is_err(), "constrained {class:?}");
                } else {
                    match rx.try_recv().unwrap_or_else(|error| panic!("free {class:?} order missing: {error}; ready={} pending={} destination={:?}",c.can_fly(),c.request_pending,c.drone_destination(&c.own().unwrap()))) {
                        Request::DroneNavigateById { id: actual, .. }
                        | Request::AssignMiningDrone { id: actual, .. }
                        | Request::AssignAttackDrone { id: actual, .. } => assert_eq!(actual, id),
                        other => panic!("wrong free-flight order {other:?}"),
                    }
                }
                assert_eq!(
                    serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
                    before
                );
            }
        }
    }
    #[test]
    fn orientation_label_distinguishes_valid_invalid_and_legacy_attitude() {
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx);
        active(&mut c, None);
        let drone = &mut c.snapshot.as_mut().unwrap().pilots[0].drones[0];
        drone.orientation_xyzw = None;
        assert_eq!(
            drone_orientation_label(drone),
            "Heading only; roll unmodeled"
        );
        drone.orientation_xyzw = Some([0., 0., 0., 1.]);
        assert_eq!(drone_orientation_label(drone), "Full attitude reported");
        drone.orientation_xyzw = Some([0.; 4]);
        assert_eq!(
            drone_orientation_label(drone),
            "Attitude unavailable · invalid telemetry"
        );
        drone.orientation_xyzw = Some([0., f64::NAN, 0., 1.]);
        assert_eq!(
            drone_orientation_label(drone),
            "Attitude unavailable · invalid telemetry"
        );
    }
    #[test]
    fn frozen_stale_bay_commands_are_disabled_while_view_only_selection_remains_available() {
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx);
        c.observed = Instant::now() - Duration::from_secs(10);
        c.frozen_time = Some(1000);
        assert_eq!(c.presentation_time(), 1000);
        assert!(!c.live_command_snapshot_fresh());
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
        let labels = render(&ctx, &mut c, vec![]);
        assert!(labels
            .iter()
            .any(|(s, _)| s.contains("Fleet telemetry stale")));
        click(&ctx, &mut c, "Launch Recon · 5 stored / 5");
        assert!(rx.try_recv().is_err());
        assert!(!c.request_pending);
        click(&ctx, &mut c, "Bay overview");
        assert_eq!(c.camera_source, camera_sources::CameraSource::BayOverview);
        assert_eq!(c.selected_drone, Some(101));
        assert!(rx.try_recv().is_err());
        assert!(!c.request_pending);
        assert_eq!(
            serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
            before
        );
    }
    #[test]
    fn stored_bay_workspace_shrink_keeps_real_title_close_inside_viewport() {
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx);
        c.identity_collapsed = true;
        c.services_open = true;
        c.service_tab = ServiceTab::Drones;
        let id = egui::Id::new("cockpit_services_overlay").with(c.layout.generation);
        let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        let frame = |c: &mut Cockpit, size: Vec2, events| {
            c.observed = Instant::now();
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, size)),
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ctx| eframe::App::update(c, ctx, &mut eframe::Frame::_new_kittest()),
            )
        };
        for size in [
            Vec2::new(1280., 800.),
            Vec2::new(900., 650.),
            Vec2::new(800., 600.),
        ] {
            let mut output = frame(&mut c, size, vec![]);
            for _ in 0..7 {
                output = frame(&mut c, size, vec![]);
            }
            let rect = ctx.memory(|m| m.area_rect(id)).unwrap();
            assert!(
                rect.right() <= size.x - 8.01 + 0.1 && rect.left() >= 7.9,
                "window={rect:?}, size={size:?}, labels={:?}",
                output
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Text(t) if t.visual_bounding_rect().width() > 500. =>
                            Some((t.galley.job.text.clone(), t.visual_bounding_rect())),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            );
            // egui's real title Close is two diagonal line segments, not a
            // text glyph. Verify their complete painted bounds, including the
            // stroke, stay inside both the actual clip and the viewport.
            let close_region = egui::Rect::from_min_max(
                Pos2::new(rect.right() - 32., rect.top()),
                Pos2::new(rect.right(), rect.top() + 34.),
            );
            let close_lines: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::LineSegment { points, stroke } = &s.shape {
                        let bounds = egui::Rect::from_two_pos(points[0], points[1]);
                        if close_region.contains_rect(bounds)
                            && bounds.width() > 4.
                            && bounds.height() > 4.
                        {
                            return Some((bounds.expand(stroke.width / 2.), s.clip_rect));
                        }
                    }
                    None
                })
                .collect();
            assert_eq!(close_lines.len(), 2, "real Close X strokes at {size:?}");
            for (paint, clip) in close_lines {
                assert!(
                    clip.contains_rect(paint),
                    "Close paint {paint:?} clipped by {clip:?}"
                );
                assert!(egui::Rect::from_min_size(Pos2::ZERO, size).contains_rect(paint));
            }
            for label in ["Operations · Drones", "Back to Flight", "Search panels"] {
                assert!(output.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text==label && s.clip_rect.contains_rect(t.visual_bounding_rect()))),"fully visible {label}");
            }
        }
        let rect = ctx.memory(|m| m.area_rect(id)).unwrap();
        let pos = Pos2::new(rect.right() - 14., rect.top() + 18.);
        let size = Vec2::new(800., 600.);
        frame(&mut c, size, vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            frame(
                &mut c,
                size,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
        assert!(
            !c.services_open,
            "actual title Close must work after shrink"
        );
        assert!(rx.try_recv().is_err());
        assert_eq!(
            serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
            before
        );
    }
    #[test]
    fn legacy_launch_remains_legacy_without_feature_advertisement() {
        let ctx = egui::Context::default();
        let mut c = crate::tests::view_test_cockpit(&ctx);
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        click(&ctx, &mut c, "Launch Recon 0/5");
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::LaunchDroneClass {
                class: DroneClass::Recon
            }
        ));
    }
}
