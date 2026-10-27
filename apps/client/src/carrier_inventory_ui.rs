//! Captain-facing finite inventory. Physical berths are not class quotas or inventory capacity.
use super::*;
use arkline_protocol::{carrier_inventory_geometry as geometry, *};

#[derive(Default, Debug, PartialEq, Eq)]
struct Counts {
    ready: usize,
    protected: usize,
    loading: usize,
    transit: usize,
    deployed: usize,
    destroyed: usize,
}
impl Counts {
    fn new(entries: &[CarrierDroneEntry]) -> Self {
        let mut c = Self::default();
        for e in entries {
            match e.location {
                CarrierDroneLocation::Ready { .. } => c.ready += 1,
                CarrierDroneLocation::Protected { .. } => c.protected += 1,
                CarrierDroneLocation::Loading { .. } => c.loading += 1,
                CarrierDroneLocation::Launching { .. }
                | CarrierDroneLocation::Returning { .. }
                | CarrierDroneLocation::Servicing { .. } => c.transit += 1,
                CarrierDroneLocation::Deployed => c.deployed += 1,
                CarrierDroneLocation::Destroyed { .. } => c.destroyed += 1,
            }
        }
        c
    }
}
fn location_label(location: &CarrierDroneLocation) -> String {
    match location {
        CarrierDroneLocation::Ready { berth_id } => format!("Ready · berth {}", berth_id + 1),
        CarrierDroneLocation::Protected { slot_id } => format!("Protected · slot {}", slot_id + 1),
        CarrierDroneLocation::Loading { .. } => "Loading · not ready".into(),
        CarrierDroneLocation::Launching { .. } => "Launching · not deployed".into(),
        CarrierDroneLocation::Returning { .. } => "Returning".into(),
        CarrierDroneLocation::Servicing { .. } => "Servicing · not stored".into(),
        CarrierDroneLocation::Deployed => "Deployed".into(),
        CarrierDroneLocation::Destroyed { .. } => "Destroyed".into(),
    }
}
fn command_ids(
    public: &CarrierInventoryPublicState,
    pilot: &PilotView,
    authenticated_owner: &str,
    assets: Option<&AssetsView>,
) -> Option<CarrierInventoryNextIds> {
    if pilot.name != authenticated_owner {
        return None;
    }
    public.validated_next_ids(assets?.carrier_inventory.as_ref()?, pilot)
}
fn deployment_hold_resume_ready(phase: CarrierDeploymentPhase, ready: bool) -> bool {
    ready && phase.active()
}
fn waiting_for_transfer(ore: &CarrierOreState, physical_operation_id: u64) -> bool {
    ore.waiting.iter().any(|id| {
        ore.services.iter().any(|service| {
            service.service_id == *id && service.physical_operation_id == physical_operation_id
        })
    })
}
fn recall_service_operation<'a>(
    state: &'a CarrierInventoryState,
    member: &CarrierRecallMember,
) -> Option<&'a CarrierInventoryOperation> {
    state.operation(member.service_operation_id?).filter(|op| {
        op.drone_id == member.drone_id
            && op.action
                == CarrierInventoryAction::ReturnToUnload {
                    resume_mining: false,
                }
    })
}
fn recall_ore_work(state: &CarrierInventoryState, batch: &CarrierRecallBatch) -> usize {
    batch
        .members
        .iter()
        .filter(|m| recall_service_operation(state, m).is_some_and(|op| !op.phase.terminal()))
        .count()
}
fn motion_owns_drone(motion: &CarrierInventoryMotionView) -> bool {
    motion
        .confirmed
        .poses
        .iter()
        .any(|(key, _)| matches!(key, CarrierPoseKey::Drone))
}
fn entry_status(public: &CarrierInventoryPublicState, entry: &CarrierDroneEntry) -> String {
    if let Some(motion) = public
        .motions
        .iter()
        .find(|m| m.drone_id == entry.drone_id && motion_owns_drone(m) && m.held)
    {
        return format!(
            "{} · motion held: {}",
            location_label(&entry.location),
            motion.hold_reason
        );
    }
    if public.motions.iter().any(|m| {
        m.drone_id == entry.drone_id
            && motion_owns_drone(m)
            && matches!(m.route, CarrierPresentationRoute::Departure { .. })
    }) {
        return "Deployed · clearing exit".into();
    }
    if let Some(op) = public.inventory.active_operation(entry.drone_id) {
        if op.phase == CarrierOperationPhase::Held {
            return format!("Held · {}", op.message);
        }
        if waiting_for_transfer(&public.ore, op.operation_id) {
            return "Coupled · waiting for shared transfer".into();
        }
        if public.ore.granted_service_id.is_some_and(|id| {
            public
                .ore
                .services
                .iter()
                .any(|s| s.service_id == id && s.physical_operation_id == op.operation_id)
        }) {
            return "Confirmed ore transfer".into();
        }
        if !op.message.is_empty() {
            return format!("{} · {}", location_label(&entry.location), op.message);
        }
    }
    location_label(&entry.location)
}

fn carrier_action_supported(
    public: &CarrierInventoryPublicState,
    class: DroneClass,
    action: CarrierInventoryAction,
) -> bool {
    let definition = geometry::lookup(&public.binding().registration)
        .or_else(|| geometry::stationary_fixture_for_registration(&public.binding().registration));
    definition.is_some_and(|d| {
        d.topology.ready_berths.iter().any(|b| {
            d.cassettes.iter().any(|c| {
                d.bay_route(b.berth_id, class, c.cassette_id, action)
                    .is_some()
            })
        })
    })
}

impl Cockpit {
    /// True also for invalid/missing viewed-vessel telemetry when the session advertises
    /// the capability: never fall through to free-spawn legacy controls.
    pub(super) fn carrier_inventory_controls(
        &mut self,
        ui: &mut egui::Ui,
        pilot: &PilotView,
    ) -> bool {
        if self.physical_cargo.is_some() {
            self.physical_cargo_controls(ui, pilot);
            return true;
        }
        let advertised = self
            .snapshot
            .as_ref()
            .is_some_and(|s| s.carrier_inventory_registration.is_some())
            || pilot.carrier_inventory.is_some()
            || self
                .assets
                .as_ref()
                .is_some_and(|a| a.carrier_inventory.is_some());
        if !advertised {
            return false;
        }
        let Some(public) = &pilot.carrier_inventory else {
            ui.colored_label(
                AMBER,
                "Carrier inventory unavailable for this viewed vessel · no legacy spawning",
            );
            return true;
        };
        let admission = match public.validate_projection(pilot) {
            Ok(a)
                if self
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| s.server_time_ms == public.sample_time_ms) =>
            {
                a
            }
            _ => {
                ui.colored_label(AMBER, "Carrier inventory unavailable · inconsistent source, identity or confirmed sample");
                return true;
            }
        };
        let Some(definition) = geometry::lookup(&public.binding().registration).or_else(|| {
            geometry::stationary_fixture_for_registration(&public.binding().registration)
        }) else {
            ui.colored_label(
                AMBER,
                "Carrier inventory unavailable · exact source definition missing",
            );
            return true;
        };
        let ids = command_ids(public, pilot, &self.pilot, self.assets.as_ref());
        let ready = ids.is_some() && self.can_fly() && self.live_command_snapshot_fresh();
        let job_busy = self.snapshot.as_ref().is_some_and(|s| {
            s.resource_jobs
                .iter()
                .any(|j| j.pilot == pilot.name && !j.phase.is_terminal())
        });
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 3.;
            ui.spacing_mut().button_padding.y = 2.;
            ui.spacing_mut().interact_size.y = 22.;
            let counts = Counts::new(&public.inventory.entries);
            let inspection = admission == CarrierInventoryAdmission::StationaryFixture;
            let summary = format!("{} IDs | Ready {}  Protected {}  Loading {}  Transit {}  Deployed {}  Destroyed {}",
                public.inventory.entries.len(), counts.ready, counts.protected, counts.loading, counts.transit, counts.deployed, counts.destroyed);
            ui.colored_label(if inspection { AMBER } else { TEAL }, summary)
                .on_hover_text(if inspection { "Stationary source fixture; normal orders unavailable. Physical berths do not set class quotas or inventory capacity." } else { "Actual finite roster locations; launching/returning/servicing are transit, not deployed." });
            if !self.connected || !self.live_command_snapshot_fresh() {
                ui.colored_label(AMBER, "LAST CONFIRMED · stale/disconnected; no inferred progress");
            } else if !inspection && !ready {
                ui.colored_label(AMBER, "Orders unavailable · requires fresh owned telemetry, seated unpaused flight and no pending request");
            }
            let capacity = cargo_capacity(pilot.ship_class);
            ui.horizontal(|ui| {
                let selected = self.selected_drone.and_then(|id| public.inventory.entry(id)).map(|entry| {
                    let held = public.inventory.active_operation(entry.drone_id).is_some_and(|o| o.phase == CarrierOperationPhase::Held)
                        || public.motions.iter().any(|m| m.drone_id == entry.drone_id && motion_owns_drone(m) && m.held);
                    (format!("#{} · {}{}", entry.drone_id, location_label(&entry.location), if held { " · HELD" } else { "" }), entry_status(public, entry))
                });
                let reserved = selected.as_ref().map_or(0., |(text, _)| ui.painter()
                    .layout_no_wrap(text.clone(), egui::TextStyle::Body.resolve(ui.style()), egui::Color32::WHITE).size().x + ui.spacing().item_spacing.x);
                ui.add(egui::ProgressBar::new(pilot.ore as f32 / capacity.max(1) as f32)
                    .desired_width((ui.available_width() - reserved).max(1.)).desired_height(16.).text(format!(
                    "SHIP {}/{} ore · incoming {} · available {}", pilot.ore, capacity,
                    public.ore.reserved_incoming().unwrap(), public.ore.available_ship_headroom(pilot.ore, capacity).unwrap())));
                if let Some((text, detail)) = selected { ui.label(text).on_hover_text(detail); }
            });
            if job_busy { ui.colored_label(AMBER, "Resource job owns this vessel · new manual deployment/return orders unavailable"); }
            self.carrier_deployment_controls(ui, public, ids, ready, job_busy);
            // Slot identities follow the source topology; neither side is tied to a drone class.
            for (side, label) in [(CarrierSide::Port, "PORT"), (CarrierSide::Starboard, "STARBOARD")] {
                ui.horizontal(|ui| {
                    ui.label(label);
                    for berth in definition.topology.ready_berths.iter().filter(|b| b.side == side) {
                        let occupant = public.inventory.docked_cassette(CarrierCassetteDock::Ready { berth_id: berth.berth_id })
                            .and_then(|c| c.drone_id).and_then(|id| public.inventory.entry(id));
                        let reserved = public.inventory.reservations.iter().find(|r| r.resource == (CarrierResource::ReadyBerth { berth_id: berth.berth_id }));
                        let text = if let Some(entry) = occupant {
                            format!("{} #{}{}", berth.berth_id + 1, entry.drone_id,
                                if matches!(entry.location, CarrierDroneLocation::Ready { .. }) { " ready" } else { " busy" })
                        } else if reserved.is_some() { format!("{} busy", berth.berth_id + 1) }
                        else { format!("{} empty", berth.berth_id + 1) };
                        let selected_id = occupant.map(|e| e.drone_id).or_else(|| reserved.map(|r| r.drone_id));
                        let response = ui.selectable_label(selected_id.is_some() && selected_id == self.selected_drone, text);
                        if response.on_hover_text("Actual ready berth occupancy/reservation; selecting never launches or loads").clicked() {
                            if let Some(id) = selected_id { self.selected_drone = Some(id); }
                        }
                    }
                });
            }
            let hidden_id = ui.make_persistent_id(("carrier_monitor_hidden", &pilot.name));
            let mut hidden = ui.ctx().data_mut(|d| d.get_temp::<bool>(hidden_id)).unwrap_or(false);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() * 0.53).clamp(340., 420.));
                    self.carrier_roster(ui, pilot, public);
                });
                ui.vertical(|ui| {
                    let device_key = ui.make_persistent_id(("carrier_camera_device", &public.binding().vessel_id));
                    let devices = public.cameras.as_ref().map(|c| c.devices.as_slice()).unwrap_or_default();
                    let mut selected = ui.ctx().data_mut(|d| d.get_temp::<String>(device_key)).unwrap_or_default();
                    if !devices.iter().any(|d| d.device_id == selected) {
                        selected = devices.first().map(|d| d.device_id.clone()).unwrap_or_default();
                    }
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut hidden, "Hide monitor").on_hover_text("View only; jobs and physical operations continue");
                    if !hidden && !devices.is_empty() {
                        let label = |id: &str| id.strip_prefix("pioneer.carrier.camera.loader.").unwrap_or(id).replace('.', " ");
                        egui::ComboBox::from_id_salt(device_key)
                            .width(ui.available_width().max(1.))
                            .selected_text(format!("Crane {}", label(&selected)))
                            .show_ui(ui, |ui| { for device in devices {
                                ui.selectable_value(&mut selected, device.device_id.clone(), format!("Crane {}", label(&device.device_id)));
                            }});
                    }
                    });
                    ui.ctx().data_mut(|d| d.insert_temp(device_key, selected.clone()));
                    if let Some(snapshot) = &self.snapshot {
                        let now = self.timeline_origin.map_or(snapshot.server_time_ms, |origin| origin.saturating_add(self.observed.elapsed().as_millis() as u64));
                        self.flight_view.carrier_operations_camera_ui(ui, snapshot, &pilot.name, now,
                            self.connected, self.embedded_paused || self.frozen_time.is_some(), &selected, hidden || devices.is_empty());
                    }
                    if !hidden && devices.is_empty() {
                        ui.small("PHYSICAL FEED · view only");
                        ui.label("Source-mounted camera telemetry unavailable");
                        ui.small("No substitute legacy image. Jobs continue while this view is hidden.");
                    }

                });
            });
            ui.ctx().data_mut(|d| d.insert_temp(hidden_id, hidden));
            self.carrier_selected_controls(ui, public, ids, ready, job_busy);
        });
        true
    }
    fn carrier_deployment_controls(
        &mut self,
        ui: &mut egui::Ui,
        public: &CarrierInventoryPublicState,
        ids: Option<CarrierInventoryNextIds>,
        ready: bool,
        job_busy: bool,
    ) {
        let state = &public.inventory;
        ui.horizontal_wrapped(|ui| {
            if let Some(batch) = state.active_recall() {
                let blocked = batch.members.iter().filter(|m| !m.blocked_reason.is_empty()).count();
                let ore_work = recall_ore_work(state, batch);
                let ore_label = if ore_work > 0 { format!(" · {ore_work} ore tasks") } else { String::new() };
                ui.label(format!("Recall all #{} · {:?} · {} IDs · {blocked} blocked{ore_label}{}",
                    batch.batch_id, batch.phase, batch.members.len(), if batch.held { " · HELD" } else { "" }))
                    .on_hover_text(format!("{ore_work} retained ore-return tasks. Unloading is a prefix, not a storage receipt."));
                let binding = public.binding().clone();
                if ui.add_enabled(ready && batch.phase.active(),
                    egui::Button::new(if batch.held { "Resume recall" } else { "Hold recall" })).clicked() {
                    self.send(Request::CarrierInventory { request: if batch.held {
                        CarrierInventoryRequest::ResumeRecall { binding: binding.clone(), batch_id: batch.batch_id }
                    } else { CarrierInventoryRequest::HoldRecall { binding: binding.clone(), batch_id: batch.batch_id } } });
                }
                if ui.add_enabled(ready && batch.phase == CarrierRecallPhase::Running,
                    egui::Button::new("Cancel recall remaining")).clicked() {
                    self.send(Request::CarrierInventory { request: CarrierInventoryRequest::CancelRecallRemaining { binding, batch_id: batch.batch_id } });
                }
            } else if let Some(batch) = state.active_deployment() {
                let out = batch.members.iter().filter(|m| m.launch_operation_id.and_then(|id| state.operation(id)).is_some_and(|o| o.phase == CarrierOperationPhase::Completed)).count();
                let blocked = batch.members.iter().filter(|m| !m.blocked_reason.is_empty()).count();
                ui.label(format!("Deploy all #{} · {out}/{} launched · {blocked} blocked{}", batch.batch_id, batch.members.len(), if batch.held { " · HELD" } else { "" }));
                let binding = public.binding().clone();
                if ui.add_enabled(deployment_hold_resume_ready(batch.phase, ready), egui::Button::new(if batch.held { "Resume" } else { "Hold" })).clicked() {
                    self.send(Request::CarrierInventory { request: if batch.held { CarrierInventoryRequest::ResumeDeployment { binding: binding.clone(), batch_id: batch.batch_id } } else { CarrierInventoryRequest::HoldDeployment { binding: binding.clone(), batch_id: batch.batch_id } } });
                }
                if ui.add_enabled(ready && batch.phase == CarrierDeploymentPhase::Running, egui::Button::new("Cancel remaining")).clicked() {
                    self.send(Request::CarrierInventory { request: CarrierInventoryRequest::CancelDeploymentRemaining { binding, batch_id: batch.batch_id } });
                }
            } else {
                if let Some(batch) = state.deployments.last() {
                    ui.label(format!("Last deployment #{} · {:?}", batch.batch_id, batch.phase));
                }
                let eligible = state.entries.iter().filter(|e| matches!(e.location, CarrierDroneLocation::Ready { .. } | CarrierDroneLocation::Protected { .. })).count();
                if ui.add_enabled(ready && !job_busy && eligible > 0, egui::Button::new(format!("Deploy all ({eligible})"))).on_hover_text("One retained order: launch ready drones and physically load protected inventory until its accepted finite roster is deployed").clicked() {
                    self.send(Request::CarrierInventory { request: CarrierInventoryRequest::DeployAll { binding: public.binding().clone(), batch_id: ids.unwrap().deployment_id } });
                }
            }
            if public.source_admission() == Some(CarrierInventoryAdmission::StationaryFixture) {
                ui.colored_label(AMBER, "INSPECTION ONLY").on_hover_text("Stationary source fixture; normal orders unavailable");
            }

        });
    }
    fn carrier_roster(
        &mut self,
        ui: &mut egui::Ui,
        pilot: &PilotView,
        public: &CarrierInventoryPublicState,
    ) {
        let mut entries: Vec<_> = public.inventory.entries.iter().collect();
        entries.sort_by_key(|e| e.drone_id);
        let key = ui.make_persistent_id(("carrier_roster_page", &pilot.name));
        let pages = entries.len().div_ceil(5).max(1);
        let mut page = ui
            .ctx()
            .data_mut(|d| d.get_temp::<usize>(key))
            .unwrap_or(0)
            .min(pages - 1);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(page > 0, egui::Button::new("Previous IDs"))
                .clicked()
            {
                page -= 1;
            }
            ui.label(format!("{}/{} · {} IDs", page + 1, pages, entries.len()));
            if ui
                .add_enabled(page + 1 < pages, egui::Button::new("Next IDs"))
                .clicked()
            {
                page += 1;
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(key, page));
        for entry in entries.into_iter().skip(page * 5).take(5) {
            let ore = public
                .ore
                .holds
                .iter()
                .find(|h| h.drone_id == entry.drone_id)
                .map_or_else(
                    || "—".into(),
                    |h| format!("{}/{} ore", h.ore_units, MINING_DRONE_CARGO_CAPACITY),
                );
            let status = entry_status(public, entry);
            let cycle = matches!(entry.location, CarrierDroneLocation::Deployed)
                .then(|| {
                    Cockpit::fleet(pilot)
                        .iter()
                        .find(|d| d.id == entry.drone_id)
                        .and_then(|d| {
                            crate::drone_telemetry::DroneReadout::new(d, self.drone_telemetry_time)
                                .cycle
                        })
                })
                .flatten();
            let class = match entry.class {
                DroneClass::Mining => "M",
                DroneClass::Recon => "R",
                DroneClass::Attack => "C",
            };
            let phase = match entry.location {
                CarrierDroneLocation::Ready { berth_id } => format!("Ready B{}", berth_id + 1),
                CarrierDroneLocation::Protected { slot_id } => format!("Store S{}", slot_id + 1),
                CarrierDroneLocation::Loading { .. } => "Loading".into(),
                CarrierDroneLocation::Launching { .. } => "Launching".into(),
                CarrierDroneLocation::Returning { .. } => "Returning".into(),
                CarrierDroneLocation::Servicing { .. } => "Servicing".into(),
                CarrierDroneLocation::Deployed => cycle.as_ref().map_or_else(
                    || "Deployed".into(),
                    |c| {
                        let tag = match self.drone_telemetry_time.availability {
                            crate::drone_telemetry::Availability::Live if c.fraction < 1. => "Mine",
                            crate::drone_telemetry::Availability::Live => "Await",
                            crate::drone_telemetry::Availability::Paused => "Paused",
                            _ => "Last",
                        };
                        format!("{tag} {}", c.label.split(" / ").next().unwrap_or("?"))
                    },
                ),
                CarrierDroneLocation::Destroyed { .. } => "Destroyed".into(),
            };
            let label = format!(
                "#{} {class} · {}HP · {} · {phase}",
                entry.drone_id, entry.hit_points, ore
            );
            ui.push_id(("carrier_drone", &pilot.name, entry.drone_id), |ui| {
                let response = ui.add_sized(
                    [ui.available_width(), 24.],
                    egui::Button::new(label).selected(self.selected_drone == Some(entry.drone_id)),
                );
                if response
                    .on_hover_text(format!(
                        "{status}{}",
                        cycle.as_ref().map_or_else(String::new, |c| format!(
                            " · {} · {}",
                            c.label,
                            self.drone_telemetry_time.availability.label()
                        ))
                    ))
                    .clicked()
                {
                    self.selected_drone = Some(entry.drone_id);
                }
            });
        }
    }
    fn carrier_tail_controls(
        &mut self,
        ui: &mut egui::Ui,
        public: &CarrierInventoryPublicState,
        drone_id: u64,
        ready: bool,
    ) {
        for tail in public
            .motions
            .iter()
            .filter(|m| m.drone_id == drone_id && !motion_owns_drone(m))
        {
            ui.horizontal_wrapped(|ui| {
                ui.small(format!(
                    "Hardware tail #{} · {}",
                    tail.operation_id,
                    if tail.held { "held" } else { "securing" }
                ))
                .on_hover_text(&tail.hold_reason);
                if public
                    .inventory
                    .deployment_owner(tail.operation_id)
                    .is_some()
                {
                    ui.small("Deploy all owns this hardware · use deployment controls");
                } else if public.inventory.recall_owner(tail.operation_id).is_some() {
                    ui.small("Recall all owns this hardware · use recall controls");
                } else if ui
                    .add_enabled(
                        ready,
                        egui::Button::new(format!(
                            "{} hardware #{}",
                            if tail.held { "Resume" } else { "Hold" },
                            tail.operation_id
                        )),
                    )
                    .clicked()
                {
                    let binding = public.binding().clone();
                    self.send(Request::CarrierInventory {
                        request: if tail.held {
                            CarrierInventoryRequest::Resume {
                                binding,
                                operation_id: tail.operation_id,
                            }
                        } else {
                            CarrierInventoryRequest::Hold {
                                binding,
                                operation_id: tail.operation_id,
                            }
                        },
                    });
                }
            });
        }
    }
    fn carrier_selected_controls(
        &mut self,
        ui: &mut egui::Ui,
        public: &CarrierInventoryPublicState,
        ids: Option<CarrierInventoryNextIds>,
        ready: bool,
        job_busy: bool,
    ) {
        let Some(entry) = self
            .selected_drone
            .and_then(|id| public.inventory.entry(id))
        else {
            ui.small("Select a stable drone ID; no replacement is selected automatically.");
            return;
        };
        ui.label(format!(
            "#{} · {}",
            entry.drone_id,
            entry_status(public, entry)
        ));
        self.carrier_tail_controls(ui, public, entry.drone_id, ready);
        if let Some(batch) = public
            .inventory
            .active_deployment()
            .filter(|b| b.members.iter().any(|m| m.drone_id == entry.drone_id))
        {
            if let Some(member) = batch.members.iter().find(|m| m.drone_id == entry.drone_id) {
                if !member.blocked_reason.is_empty() {
                    ui.colored_label(AMBER, &member.blocked_reason);
                }
            }
            ui.small("Owned by Deploy all · use its Hold / Resume / Cancel remaining controls");
            return;
        }
        if let Some(member) = public
            .inventory
            .active_recall()
            .and_then(|b| b.members.iter().find(|m| m.drone_id == entry.drone_id))
        {
            if !member.blocked_reason.is_empty() {
                ui.colored_label(AMBER, &member.blocked_reason);
            }
            if member.service_operation_id.is_some() {
                if let Some(op) = recall_service_operation(&public.inventory, member) {
                    ui.small(format!(
                        "Recall ore return #{} · {:?} · not a storage receipt",
                        op.operation_id, op.phase
                    ))
                    .on_hover_text(&op.message);
                } else {
                    ui.colored_label(AMBER, "Recall ore-return receipt unavailable");
                }
            }
            ui.small("Owned by Recall all · use its Hold / Resume / Cancel remaining controls");
            return;
        }
        if let Some(cleanup) = public.inventory.cleanups.iter().find(|cleanup| {
            public
                .inventory
                .operation(cleanup.operation_id)
                .is_some_and(|op| op.drone_id == entry.drone_id)
        }) {
            ui.colored_label(
                AMBER,
                format!("Retained hardware cleanup · {}", cleanup.reason),
            );
            if ui
                .add_enabled(
                    ready && cleanup.held,
                    egui::Button::new("Resume hardware cleanup"),
                )
                .clicked()
            {
                self.send(Request::CarrierInventory {
                    request: CarrierInventoryRequest::Resume {
                        binding: public.binding().clone(),
                        operation_id: cleanup.operation_id,
                    },
                });
            }
            return;
        }
        if let Some(op) = public
            .inventory
            .active_operation(entry.drone_id)
            .or_else(|| {
                public
                    .motions
                    .iter()
                    .find(|m| m.drone_id == entry.drone_id && motion_owns_drone(m))
                    .and_then(|m| public.inventory.operation(m.operation_id))
            })
        {
            let held = op.phase == CarrierOperationPhase::Held
                || public
                    .motions
                    .iter()
                    .any(|m| m.operation_id == op.operation_id && m.held);
            ui.horizontal(|ui| {
                let binding = public.binding().clone();
                if ui
                    .add_enabled(
                        ready,
                        egui::Button::new(if held {
                            "Resume operation"
                        } else {
                            "Hold operation"
                        }),
                    )
                    .clicked()
                {
                    self.send(Request::CarrierInventory {
                        request: if held {
                            CarrierInventoryRequest::Resume {
                                binding: binding.clone(),
                                operation_id: op.operation_id,
                            }
                        } else {
                            CarrierInventoryRequest::Hold {
                                binding: binding.clone(),
                                operation_id: op.operation_id,
                            }
                        },
                    });
                }
                if ui
                    .add_enabled(
                        ready && op.effective_phase() == CarrierOperationPhase::Queued,
                        egui::Button::new("Cancel queued operation"),
                    )
                    .clicked()
                {
                    self.send(Request::CarrierInventory {
                        request: CarrierInventoryRequest::Cancel {
                            binding,
                            operation_id: op.operation_id,
                        },
                    });
                }
            });
            return;
        }
        if public.inventory.active_deployment().is_some()
            || public.inventory.active_recall().is_some()
        {
            ui.small("Automatic queue owns pending inventory · no new individual orders");
            return;
        }
        if !carrier_action_supported(public, entry.class, CarrierInventoryAction::RecallToStorage) {
            ui.small("Return/unload/storage routes are unavailable in this source · buttons are not implemented capability claims");
        }
        ui.horizontal_wrapped(|ui| {
            for (label, action, eligible) in [
                (
                    "Deploy selected",
                    CarrierInventoryAction::Launch,
                    matches!(entry.location, CarrierDroneLocation::Ready { .. }),
                ),
                (
                    "Return to unload & resume",
                    CarrierInventoryAction::ReturnToUnload {
                        resume_mining: true,
                    },
                    matches!(entry.location, CarrierDroneLocation::Deployed)
                        && entry.class == DroneClass::Mining
                        && public
                            .ore
                            .holds
                            .iter()
                            .any(|h| h.drone_id == entry.drone_id && h.ore_units > 0),
                ),
                (
                    "Recall to storage",
                    CarrierInventoryAction::RecallToStorage,
                    matches!(entry.location, CarrierDroneLocation::Deployed),
                ),
            ] {
                let route_available = carrier_action_supported(public, entry.class, action);
                if ui
                    .add_enabled(
                        ready && !job_busy && eligible && route_available,
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    self.send(Request::CarrierInventory {
                        request: CarrierInventoryRequest::Begin {
                            binding: public.binding().clone(),
                            operation_id: ids.unwrap().operation_id,
                            drone_id: entry.drone_id,
                            action,
                        },
                    });
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(ctx: &egui::Context) -> Cockpit {
        fixture_for(ctx, geometry::stationary_fixture_definition().unwrap())
    }
    fn fixture_for(ctx: &egui::Context, def: &geometry::CarrierInventoryDefinition) -> Cockpit {
        let mut c = crate::tests::view_test_cockpit(ctx);
        let binding = def.topology.registration.bind("owned-pioneer".into());
        let entries: Vec<_> = def
            .cassettes
            .iter()
            .enumerate()
            .map(|(i, cassette)| CarrierDroneEntry {
                drone_id: i as u64 + 1,
                class: DroneClass::Mining,
                hit_points: 100,
                location: match cassette.initial_dock {
                    CarrierCassetteDock::Ready { berth_id } => {
                        CarrierDroneLocation::Ready { berth_id }
                    }
                    CarrierCassetteDock::Protected { slot_id } => {
                        CarrierDroneLocation::Protected { slot_id }
                    }
                    CarrierCassetteDock::TransitBuffer { .. } => {
                        panic!("buffer is not stock capacity")
                    }
                },
            })
            .collect();
        let inventory = CarrierInventoryState {
            schema_version: 1,
            binding: binding.clone(),
            initialized_at_ms: 1000,
            roster: entries
                .iter()
                .map(|e| CarrierRosterIdentity {
                    drone_id: e.drone_id,
                    class: e.class,
                    preferred_magazine_slot: if let CarrierDroneLocation::Protected { slot_id } =
                        e.location
                    {
                        Some(slot_id)
                    } else {
                        None
                    },
                })
                .collect(),
            cassettes: entries
                .iter()
                .zip(&def.cassettes)
                .map(|(e, s)| CarrierCassette {
                    cassette_id: s.cassette_id,
                    drone_id: Some(e.drone_id),
                    location: CarrierCassetteLocation::Docked {
                        dock: s.initial_dock,
                    },
                })
                .collect(),
            entries,
            high_watermark: 0,
            operations: vec![],
            reservations: vec![],
            cleanups: vec![],
            deployment_high_watermark: 0,
            deployments: vec![],
            recall_high_watermark: 0,
            recalls: vec![],
        };
        let ore = CarrierOreState {
            binding,
            holds: inventory
                .roster
                .iter()
                .map(|i| DroneOreHold {
                    drone_id: i.drone_id,
                    ore_units: 0,
                    destroyed_at_ms: None,
                    intent_generation: 0,
                    mining_intent: None,
                    cycle_high_watermark: 0,
                    cycle: None,
                })
                .collect(),
            lost_ore_units: 0,
            service_high_watermark: 0,
            services: vec![],
            waiting: vec![],
            granted_service_id: None,
            last_committed_at_ms: None,
        };
        let cassette_poses: Vec<_> = def
            .cassettes
            .iter()
            .map(|c| (c.cassette_id, c.reference_pose))
            .collect();
        let pilot = &mut c.snapshot.as_mut().unwrap().pilots[0];
        pilot.ship_class = ShipClass::Pioneer;
        pilot.drone_bay = None;
        pilot.drone_cargo = None;
        pilot.drones.clear();
        pilot.drone = None;
        let flight = pilot.flight.as_mut().unwrap();
        flight.position = [0.; 3];
        flight.orientation = [0., 0., 0., 1.];
        flight.velocity = [0.; 3];
        flight.acceleration = [0.; 3];
        flight.angular_velocity = [0.; 3];
        flight.mode = NavigationMode::Stop;
        flight.docking=Some(serde_json::from_value(serde_json::json!({"version":1,"host_vessel_id":"owned-pioneer","berth_id":0,"operation_id":0,"phase":"free","clamps":0.,"updated_at_ms":1000,"departing":false})).unwrap());
        let public = CarrierInventoryPublicState {
            mining_blocks: vec![],
            cameras: None,
            sample_time_ms: 1000,
            parent: arkline_protocol::drone_bay_geometry::BayPose {
                position_m: [0.; 3],
                orientation_xyzw: [0., 0., 0., 1.],
            },
            drone_poses: inventory
                .entries
                .iter()
                .zip(&def.cassettes)
                .map(|(e, s)| (e.drone_id, s.reference_pose))
                .collect(),
            machinery: def.idle_roots(&cassette_poses).unwrap(),
            cassette_poses,
            inventory: inventory.clone(),
            ore,
            motions: vec![],
        };
        assert_eq!(
            public.validate_projection(pilot),
            Ok(CarrierInventoryAdmission::StationaryFixture)
        );
        pilot.carrier_inventory = Some(public);
        c.snapshot.as_mut().unwrap().server_time_ms = 1000;
        c.assets = Some(
            serde_json::from_value(
                serde_json::json!({"ships":[],"storage":[],"corporation":null,"invitations":[]}),
            )
            .unwrap(),
        );
        c.assets.as_mut().unwrap().carrier_inventory = Some(inventory);
        c.selected_drone = Some(1);
        c
    }
    fn frame(
        ctx: &egui::Context,
        c: &mut Cockpit,
        events: Vec<egui::Event>,
    ) -> Vec<(String, Pos2)> {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(640., 650.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| c.drone_controls(ui));
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
    fn click(ctx: &egui::Context, c: &mut Cockpit, text: &str) {
        frame(ctx, c, vec![]);
        let labels = frame(ctx, c, vec![]);
        let pos = labels
            .iter()
            .find(|(s, _)| s == text)
            .unwrap_or_else(|| panic!("Missing {text}: {labels:?}"))
            .1;
        for pressed in [true, false] {
            frame(
                ctx,
                c,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    #[test]
    fn twenty_ids_are_separate_from_ten_berths_and_view_only_paging() {
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx);
        let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        let labels = frame(&ctx, &mut c, vec![]);
        assert!(labels
            .iter()
            .any(|(s, _)| s.contains("Ready 10  Protected 10  Loading 0  Transit 0  Deployed 0")));
        assert!(labels.iter().any(|(s, _)| s == "PORT"));
        assert!(labels.iter().any(|(s, _)| s == "STARBOARD"));
        for _ in 0..3 {
            click(&ctx, &mut c, "Next IDs");
        }
        assert!(frame(&ctx, &mut c, vec![])
            .iter()
            .any(|(s, _)| s.starts_with("#20 M")));
        let label = frame(&ctx, &mut c, vec![])
            .into_iter()
            .find(|(s, _)| s.starts_with("#20 M"))
            .unwrap()
            .0;
        click(&ctx, &mut c, &label);
        assert_eq!(c.selected_drone, Some(20));
        click(&ctx, &mut c, "Hide monitor");
        click(&ctx, &mut c, "Hide monitor");
        click(&ctx, &mut c, "Deploy all (20)");
        click(&ctx, &mut c, "Deploy all (20)");
        assert!(
            rx.try_recv().is_err(),
            "fixture inspection never enables normal actions"
        );
        c.reconcile_empty_legacy_drone_selection();
        assert_eq!(c.selected_drone, Some(20));
        assert_eq!(
            serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
            before
        );
    }
    #[test]
    fn missing_foreign_clock_and_invalid_source_never_fall_back_to_legacy_spawn() {
        for case in 0..5 {
            let ctx = egui::Context::default();
            let mut c = fixture(&ctx);
            match case {
                0 => c.snapshot.as_mut().unwrap().pilots[0].carrier_inventory = None,
                1 => {
                    c.snapshot.as_mut().unwrap().pilots[0]
                        .carrier_inventory
                        .as_mut()
                        .unwrap()
                        .inventory
                        .binding
                        .vessel_id = "foreign".into()
                }
                2 => c.snapshot.as_mut().unwrap().server_time_ms += 1,
                4 => {
                    let snapshot = c.snapshot.as_mut().unwrap();
                    snapshot.carrier_inventory_registration = Some(
                        snapshot.pilots[0]
                            .carrier_inventory
                            .as_ref()
                            .unwrap()
                            .binding()
                            .registration
                            .clone(),
                    );
                    snapshot.pilots[0].carrier_inventory = None;
                    c.assets = None;
                }
                _ => {
                    c.snapshot.as_mut().unwrap().pilots[0]
                        .carrier_inventory
                        .as_mut()
                        .unwrap()
                        .inventory
                        .binding
                        .registration
                        .assembly_revision += 1
                }
            }
            let labels = frame(&ctx, &mut c, vec![]);
            assert!(labels
                .iter()
                .any(|(s, _)| s.contains("Carrier inventory unavailable")));
            assert!(!labels
                .iter()
                .any(|(s, _)| s.starts_with("Deploy all") || s.starts_with("Launch")));
        }
    }
    #[test]
    fn launch_and_service_are_not_deployed_and_buffer_does_not_add_capacity() {
        let ctx = egui::Context::default();
        let c = fixture(&ctx);
        let mut entries = c
            .own()
            .unwrap()
            .carrier_inventory
            .unwrap()
            .inventory
            .entries;
        entries[0].location = CarrierDroneLocation::Launching {
            operation_id: 1,
            berth_id: 0,
        };
        entries[1].location = CarrierDroneLocation::Servicing {
            operation_id: 2,
            berth_id: 1,
        };
        entries[10].location = CarrierDroneLocation::Loading { operation_id: 3 };
        let counts = Counts::new(&entries);
        assert_eq!(
            counts,
            Counts {
                ready: 8,
                protected: 9,
                loading: 1,
                transit: 2,
                deployed: 0,
                destroyed: 0
            }
        );
        assert_eq!(entries.len(), 20);
    }
    #[test]
    fn fixture_and_foreign_private_journal_never_supply_command_ids() {
        let ctx = egui::Context::default();
        let c = fixture(&ctx);
        let pilot = c.own().unwrap();
        let public = pilot.carrier_inventory.as_ref().unwrap();
        assert!(command_ids(public, &pilot, &c.pilot, c.assets.as_ref()).is_none());
        assert!(command_ids(public, &pilot, "Different owner", c.assets.as_ref()).is_none());
        assert!(command_ids(public, &pilot, &c.pilot, None).is_none());
    }
    #[test]
    fn cancelling_deployment_can_resume_safe_hardware_but_terminal_cannot() {
        for phase in [
            CarrierDeploymentPhase::Running,
            CarrierDeploymentPhase::Cancelling,
        ] {
            assert!(deployment_hold_resume_ready(phase, true));
            assert!(!deployment_hold_resume_ready(phase, false));
        }
        for phase in [
            CarrierDeploymentPhase::Completed,
            CarrierDeploymentPhase::Cancelled,
        ] {
            assert!(!deployment_hold_resume_ready(phase, true));
        }
    }
    #[test]
    fn manifold_queue_resolves_service_identity_not_physical_operation_number() {
        let ctx = egui::Context::default();
        let c = fixture(&ctx);
        let mut ore = c.own().unwrap().carrier_inventory.unwrap().ore;
        ore.services.push(CarrierOreService {
            service_id: 1,
            physical_operation_id: 42,
            drone_id: 1,
            berth_id: 0,
            accepted_at_ms: 1000,
            closed_at_ms: None,
            requested_units: 25,
            receipts: vec![],
        });
        ore.waiting.push(1);
        assert!(waiting_for_transfer(&ore, 42));
        assert!(!waiting_for_transfer(&ore, 1));
        ore.waiting.clear();
        assert!(!waiting_for_transfer(&ore, 42));
    }
    #[test]
    fn empty_hardware_tail_is_not_the_drone_motion_owner() {
        let pose = arkline_protocol::drone_bay_geometry::BayPose {
            position_m: [0.; 3],
            orientation_xyzw: [0., 0., 0., 1.],
        };
        let mut motion = CarrierInventoryMotionView {
            operation_id: 7,
            drone_id: 2,
            route: CarrierPresentationRoute::Hardware {
                route: CarrierRouteKind::Stage {
                    direction: CarrierStageDirection::ToReady,
                    segment: CarrierStageSegment::EmptyTail,
                    source_slot: 0,
                    destination_berth: 0,
                    loaded_cassette: 1,
                    empty_cassette: 0,
                },
                target_index: 1,
            },
            source: CarrierMotionFrame { poses: vec![] },
            confirmed: CarrierMotionFrame {
                poses: vec![(CarrierPoseKey::Cassette { cassette_id: 0 }, pose)],
            },
            started_at_ms: 1000,
            duration_ms: 1000,
            confirmed_at_ms: 1200,
            held: true,
            hold_reason: "Empty tail held".into(),
        };
        assert!(!motion_owns_drone(&motion));
        motion.confirmed.poses.push((CarrierPoseKey::Drone, pose));
        assert!(motion_owns_drone(&motion));
    }
    #[test]
    fn all_count_labels_fit_one_line_at_native_narrow_content_width() {
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx);
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(636., 286.),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_width(620.);
                    assert_eq!(ui.available_width(), 620.);
                    c.drone_controls(ui);
                });
            },
        );
        let count = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text.starts_with("20 IDs | Ready") => {
                    Some((s.clip_rect, t))
                }
                _ => None,
            })
            .expect("actual finite counts");
        assert!(count.0.contains_rect(count.1.visual_bounding_rect()));
        let visible_rows = output.shapes.iter().filter(|s| matches!(&s.shape,
            egui::Shape::Text(t) if t.galley.job.text.starts_with('#') && t.galley.job.text.contains("HP") && s.clip_rect.contains_rect(t.visual_bounding_rect()))).count();
        assert_eq!(
            visible_rows, 5,
            "five exact IDs fit within native 620x270 content, without scrolling"
        );
        assert!(output.shapes.iter().any(|s| matches!(&s.shape,
            egui::Shape::Text(t) if t.galley.job.text == "INSPECTION ONLY" && s.clip_rect.contains_rect(t.visual_bounding_rect()))));
        assert_eq!(
            count.1.galley.rows.len(),
            1,
            "all counts stay on one line without smaller fonts"
        );
    }
    #[test]
    fn exact_1702_fixture_renders_inspection_without_1701_factory_panic() {
        let source: serde_json::Value = serde_json::from_str(include_str!(
            "../../../crates/arkline-protocol/src/carrier_inventory_geometry/stationary_v1702.json"
        ))
        .unwrap();
        let registration: CarrierInventoryRegistration =
            serde_json::from_value(source["definition"]["topology"]["registration"].clone())
                .unwrap();
        let definition = geometry::stationary_fixture_for_registration(&registration).unwrap();
        let ctx = egui::Context::default();
        let mut c = fixture_for(&ctx, definition);
        let labels = frame(&ctx, &mut c, vec![]);
        assert!(labels.iter().any(|(s, _)| s == "INSPECTION ONLY"));
        assert!(labels.iter().any(|(s, _)| s == "PORT"));
        let pilot = c.own().unwrap();
        let public = pilot.carrier_inventory.as_ref().unwrap();
        assert!(carrier_action_supported(
            public,
            DroneClass::Mining,
            CarrierInventoryAction::Launch
        ));
        // The pinned 1702 provider includes reverse ready capture, but no ore service.
        assert!(carrier_action_supported(
            public,
            DroneClass::Mining,
            CarrierInventoryAction::RecallToStorage
        ));
        assert!(!carrier_action_supported(
            public,
            DroneClass::Mining,
            CarrierInventoryAction::ReturnToUnload {
                resume_mining: true
            }
        ));
        assert!(command_ids(public, &pilot, &c.pilot, c.assets.as_ref()).is_none());
        c.snapshot.as_mut().unwrap().pilots[0]
            .carrier_inventory
            .as_mut()
            .unwrap()
            .inventory
            .binding
            .registration
            .assembly_source_sha256 = "a".repeat(64);
        assert!(frame(&ctx, &mut c, vec![])
            .iter()
            .any(|(s, _)| s.contains("Carrier inventory unavailable")));
    }
    #[test]
    fn isolated_tail_widget_resumes_exact_hardware_and_defers_to_deployment_owner() {
        // Exercise the already-admitted widget boundary, not normal fixture admission.
        // The full panel's fixture/foreign/stale gates remain independently tested.
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx);
        let mut public = c.own().unwrap().carrier_inventory.unwrap();
        let pose = public.cassette_poses[0].1;
        public.inventory.operations.push(CarrierInventoryOperation {
            recall_batch_id: None,
            operation_id: 7,
            drone_id: 1,
            action: CarrierInventoryAction::Stage { berth_id: 0 },
            phase: CarrierOperationPhase::Completed,
            held_phase: None,
            stage: None,
            berth_id: Some(0),
            accepted_at_ms: 1000,
            updated_at_ms: 1200,
            message: "Body handed off; empty tail held".into(),
            cargo_service_operation_ids: vec![],
        });
        public.motions.push(CarrierInventoryMotionView {
            operation_id: 7,
            drone_id: 1,
            route: CarrierPresentationRoute::Hardware {
                route: CarrierRouteKind::Stage {
                    direction: CarrierStageDirection::ToReady,
                    segment: CarrierStageSegment::EmptyTail,
                    source_slot: 0,
                    destination_berth: 0,
                    loaded_cassette: 1,
                    empty_cassette: 0,
                },
                target_index: 1,
            },
            source: CarrierMotionFrame {
                poses: vec![(CarrierPoseKey::Cassette { cassette_id: 0 }, pose)],
            },
            confirmed: CarrierMotionFrame {
                poses: vec![(CarrierPoseKey::Cassette { cassette_id: 0 }, pose)],
            },
            started_at_ms: 1000,
            duration_ms: 1000,
            confirmed_at_ms: 1200,
            held: true,
            hold_reason: "Empty tail held".into(),
        });
        let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        let render = |c: &mut Cockpit, public: &CarrierInventoryPublicState, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(640., 650.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        c.carrier_selected_controls(
                            ui,
                            public,
                            Some(CarrierInventoryNextIds {
                                operation_id: 8,
                                deployment_id: 1,
                            }),
                            true,
                            false,
                        )
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
            .collect::<Vec<_>>()
        };
        let labels = render(&mut c, &public, vec![]);
        assert!(
            labels.iter().any(|(s, _)| s == "Deploy selected"),
            "tail must not replace body launch controls"
        );
        let pos = labels
            .iter()
            .find(|(s, _)| s == "Resume hardware #7")
            .unwrap()
            .1;
        for _ in 0..2 {
            for pressed in [true, false] {
                render(
                    &mut c,
                    &public,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        }
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::CarrierInventory {
                request: CarrierInventoryRequest::Resume {
                    operation_id: 7,
                    ..
                }
            }
        ));
        assert!(
            rx.try_recv().is_err(),
            "pending repeat cannot queue another mutation"
        );
        assert_eq!(
            serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
            before
        );
        public.inventory.deployments.push(CarrierDeploymentBatch {
            batch_id: 1,
            members: vec![CarrierDeploymentMember {
                drone_id: 1,
                stage_operation_id: Some(7),
                launch_operation_id: None,
                cancelled: false,
                blocked_reason: String::new(),
            }],
            phase: CarrierDeploymentPhase::Running,
            held: true,
            accepted_at_ms: 1000,
            updated_at_ms: 1200,
        });
        let labels = render(&mut c, &public, vec![]);
        assert!(!labels.iter().any(|(s, _)| s == "Resume hardware #7"));
        assert!(labels
            .iter()
            .any(|(s, _)| s.contains("Deploy all owns this hardware")));
    }
    #[test]
    fn retained_recall_cancelling_resumes_once_without_deployment_or_child_commands() {
        recall_cancelling_routes_exact_batch(false);
    }
    #[test]
    fn retained_recall_ore_prefix_routes_batch_not_child_or_storage() {
        recall_cancelling_routes_exact_batch(true);
    }
    fn recall_cancelling_routes_exact_batch(service_prefix: bool) {
        // Widget routing only. Full-panel normal admission remains disabled for fixtures.
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx);
        let mut public = c.own().unwrap().carrier_inventory.unwrap();
        public.inventory.recalls.push(CarrierRecallBatch {
            batch_id: 1,
            phase: CarrierRecallPhase::Cancelling,
            held: true,
            accepted_at_ms: 1000,
            updated_at_ms: 1000,
            members: vec![CarrierRecallMember {
                drone_id: 1,
                accepted_location: CarrierDroneLocation::Deployed,
                service_operation_id: service_prefix.then_some(7),
                capture_operation_id: (!service_prefix).then_some(7),
                store_operation_id: None,
                cancelled: false,
                blocked_reason: "Securing actual ready berth".into(),
                completed_location: None,
            }],
        });
        if service_prefix {
            public.inventory.operations.push(CarrierInventoryOperation {
                recall_batch_id: Some(1),
                operation_id: 7,
                drone_id: 1,
                action: CarrierInventoryAction::ReturnToUnload {
                    resume_mining: false,
                },
                phase: CarrierOperationPhase::Held,
                held_phase: Some(CarrierOperationPhase::Servicing),
                stage: None,
                berth_id: Some(0),
                accepted_at_ms: 1000,
                updated_at_ms: 1000,
                message: "Waiting for confirmed shared ore transfer".into(),
                cargo_service_operation_ids: vec![],
            });
            assert_eq!(
                recall_ore_work(&public.inventory, &public.inventory.recalls[0]),
                1
            );
            assert_eq!(public.inventory.recall_owner(7).unwrap().batch_id, 1);
            public.inventory.operations[0].phase = CarrierOperationPhase::Completed;
            assert_eq!(
                recall_ore_work(&public.inventory, &public.inventory.recalls[0]),
                0
            );
            assert!(
                public.inventory.recalls[0].members[0]
                    .completed_location
                    .is_none(),
                "ore completion is not storage completion"
            );
            public.inventory.operations[0].phase = CarrierOperationPhase::Held;
        }
        let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        let render = |c: &mut Cockpit, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(900., 650.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        c.carrier_deployment_controls(ui, &public, None, true, false);
                        c.carrier_selected_controls(ui, &public, None, true, false);
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
            .collect::<Vec<_>>()
        };
        let labels = render(&mut c, vec![]);
        assert!(!labels.iter().any(|(s, _)| s.starts_with("Deploy all (")));
        assert!(!labels.iter().any(|(s, _)| s == "Deploy selected"));
        assert!(labels
            .iter()
            .any(|(s, _)| s.contains("Owned by Recall all")));
        if service_prefix {
            assert!(labels.iter().any(|(s, _)| s.contains("1 ore tasks")));
            assert!(labels
                .iter()
                .any(|(s, _)| s.contains("Recall ore return #7 · Held · not a storage receipt")));
            assert!(!labels
                .iter()
                .any(|(s, _)| s == "Resume operation" || s == "Recall to storage"));
        }
        let resume = labels.iter().find(|(s, _)| s == "Resume recall").unwrap().1;
        for _ in 0..2 {
            for pressed in [true, false] {
                render(
                    &mut c,
                    vec![
                        egui::Event::PointerMoved(resume),
                        egui::Event::PointerButton {
                            pos: resume,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        }
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::CarrierInventory {
                request: CarrierInventoryRequest::ResumeRecall { batch_id: 1, .. }
            }
        ));
        assert!(rx.try_recv().is_err());
        assert_eq!(
            serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
            before
        );
    }

    #[test]
    fn exact_1703_source_has_capture_and_service_routes_but_fixture_commands_stay_disabled() {
        let source: serde_json::Value = serde_json::from_str(include_str!(
            "../../../crates/arkline-protocol/src/carrier_inventory_geometry/stationary_v1703.json"
        ))
        .unwrap();
        let registration: CarrierInventoryRegistration =
            serde_json::from_value(source["definition"]["topology"]["registration"].clone())
                .unwrap();
        let definition = geometry::stationary_fixture_for_registration(&registration).unwrap();
        let ctx = egui::Context::default();
        let c = fixture_for(&ctx, definition);
        let pilot = c.own().unwrap();
        let public = pilot.carrier_inventory.as_ref().unwrap();
        assert!(carrier_action_supported(
            public,
            DroneClass::Mining,
            CarrierInventoryAction::Launch
        ));
        assert!(carrier_action_supported(
            public,
            DroneClass::Mining,
            CarrierInventoryAction::RecallToStorage
        ));
        assert!(carrier_action_supported(
            public,
            DroneClass::Mining,
            CarrierInventoryAction::ReturnToUnload {
                resume_mining: true
            }
        ));
        assert!(command_ids(public, &pilot, &c.pilot, c.assets.as_ref()).is_none());
    }
    #[test]
    #[ignore = "Opt-in exact immutable authority Responses for full workspace geometry; no GPU"]
    fn actual_carrier_workspace_idle_active_held_and_selected20_keep_width() {
        use sha2::{Digest, Sha256};
        let root =
            std::path::PathBuf::from(std::env::var_os("ARKLINE_CARRIER_EVIDENCE_ROOT").unwrap());
        let cases = [
            (
                "parallel-1703-847b-moving-camera-prefix/response-00000.json",
                "613f83cac7afb7d104296779c06a8ccdc188107f58bef940ac71851a15257d9e",
                1,
            ),
            (
                "parallel-1703-847b-moving-camera-prefix/response-00350.json",
                "9d82f0f0ac7d9e531d32dabfa4eafa02ee1ee8c8e49d5bfa086c0b432bdf494a",
                2,
            ),
            (
                "parallel-1703-847b-moving-camera-prefix/response-00350.json",
                "9d82f0f0ac7d9e531d32dabfa4eafa02ee1ee8c8e49d5bfa086c0b432bdf494a",
                20,
            ),
            (
                "public-loaded-restart-samples/response-1792995587537.json",
                "55827528ef5739138d5827ae789c5bf6f81a6f03cb5bc9400cb620227808828b",
                2,
            ),
            (
                "parallel-camera-onfoot-847b/response-1793007142435.json",
                "4a5bc5afdfafbc31c411f17ccb385b28614bf0997a4a5e86c26babdcfcc932bb",
                20,
            ),
        ];
        for width in [900., 1280.] {
            let mut widths = Vec::new();
            for (file, hash, selected) in cases {
                let bytes = std::fs::read(root.join(file)).unwrap();
                assert_eq!(hex::encode(Sha256::digest(&bytes)), hash);
                let raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                let response: Response =
                    serde_json::from_value(raw.get("response").unwrap_or(&raw).clone()).unwrap();
                let snapshot = response.snapshot.as_ref().unwrap();
                let pilot = snapshot.pilots.iter().find(|p| p.name == "Pilot").unwrap();
                assert_eq!(
                    pilot
                        .carrier_inventory
                        .as_ref()
                        .unwrap()
                        .validate_projection(pilot),
                    Ok(CarrierInventoryAdmission::StationaryFixture)
                );
                let ctx = egui::Context::default();
                ctx.style_mut(|s| s.animation_time = 0.);
                let mut c = crate::tests::view_test_cockpit(&ctx);
                c.pilot = "Pilot".into();
                c.snapshot = response.snapshot.clone();
                c.assets = response.assets.clone();
                c.connected = true;
                let on_foot = pilot.station_avatar.is_some();
                c.embedded = !on_foot;
                c.embedded_input = true;
                c.embedded_paused = false;
                c.identity_collapsed = !on_foot;
                c.services_open = true;
                c.service_tab = ServiceTab::Drones;
                c.selected_drone = Some(selected);
                if on_foot {
                    c.selected_object = crate::SelectedObject::Asteroid(7);
                }
                c.sound_settings.muted = true;
                let (tx, rx) = std::sync::mpsc::channel();
                c.commands = Some(tx);
                let mut shapes = Vec::new();
                for frame in 0..24 {
                    c.observed = crate::Instant::now();
                    c.timeline_origin = Some(snapshot.server_time_ms);
                    shapes = ctx
                        .run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    Pos2::ZERO,
                                    egui::vec2(width, if width == 900. { 650. } else { 800. }),
                                )),
                                time: Some(frame as f64 / 60.),
                                focused: true,
                                ..Default::default()
                            },
                            |ctx| {
                                eframe::App::update(&mut c, ctx, &mut eframe::Frame::_new_kittest())
                            },
                        )
                        .shapes;
                }
                let visible: Vec<_> = shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Text(t)
                            if s.clip_rect.contains_rect(t.visual_bounding_rect()) =>
                        {
                            Some(t)
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    visible
                        .iter()
                        .filter(|t| t.galley.job.text.starts_with('#')
                            && t.galley.job.text.contains("HP"))
                        .count(),
                    5,
                    "all five rows: {file}/{width}"
                );
                if on_foot {
                    let footer_labels = if width < 1000. {
                        [
                            "Controls",
                            "Perf",
                            "On foot · Flight locked",
                            "Exterior locked",
                        ]
                    } else {
                        [
                            "Controls & ownership",
                            "Performance",
                            "On foot · manual flight locked",
                            "Exterior access locked",
                        ]
                    };
                    let mut ys: Vec<f32> = footer_labels
                        .iter()
                        .map(|label| {
                            visible
                                .iter()
                                .find(|t| t.galley.job.text == *label)
                                .unwrap_or_else(|| panic!("missing footer {label}"))
                                .visual_bounding_rect()
                                .center()
                                .y
                        })
                        .collect();
                    for prefix in ["Hold ", "Target "] {
                        ys.push(
                            visible
                                .iter()
                                .find(|t| t.galley.job.text.starts_with(prefix))
                                .unwrap()
                                .visual_bounding_rect()
                                .center()
                                .y,
                        );
                    }
                    let target_label = visible
                        .iter()
                        .find(|t| t.galley.job.text.starts_with("Target "))
                        .unwrap();
                    assert_eq!(
                        target_label.galley.rows.len(),
                        1,
                        "long selected target must not grow footer"
                    );
                    let min = ys.iter().copied().fold(f32::INFINITY, f32::min);
                    let max = ys.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                    assert!(
                        max - min < 10.,
                        "actual on-foot footer must remain one line: {ys:?}"
                    );
                    assert!(
                        !c.identity_collapsed,
                        "temporary compact preserves expanded preference"
                    );
                }
                let counts = visible
                    .iter()
                    .find(|t| t.galley.job.text.starts_with("20 IDs | Ready"))
                    .unwrap();
                assert_eq!(counts.galley.rows.len(), 1);
                assert!(counts.galley.job.text.contains("Destroyed"));
                assert!(visible.iter().any(|t| t
                    .galley
                    .job
                    .text
                    .starts_with(&format!("#{selected} ·"))));
                let search_y = visible
                    .iter()
                    .find(|t| t.galley.job.text == "Search panels")
                    .unwrap()
                    .visual_bounding_rect()
                    .center()
                    .y;
                let clear_y = visible
                    .iter()
                    .find(|t| matches!(t.galley.job.text.as_str(), "Clear" | "Clear search"))
                    .unwrap()
                    .visual_bounding_rect()
                    .center()
                    .y;
                assert!((search_y - clear_y).abs() <= 2., "search remains one row");
                let rect = ctx
                    .memory(|m| {
                        m.area_rect(
                            egui::Id::new("cockpit_services_overlay").with(c.layout.generation),
                        )
                    })
                    .unwrap();
                widths.push(rect.width());
                assert_eq!(
                    serde_json::to_value(&c.snapshot).unwrap(),
                    serde_json::to_value(&response.snapshot).unwrap()
                );
                assert!(rx.try_iter().all(|r| matches!(r, Request::Snapshot)));
            }
            assert!(
                widths.iter().all(|w| (w - widths[0]).abs() <= 1.),
                "state cannot resize workspace: {widths:?}"
            );
        }
    }
}
