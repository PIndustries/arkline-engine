//! Private station, inventory and corporation views from authenticated server assets.
use super::*;
impl Cockpit {
    fn service_authority_ready(&self) -> bool {
        self.can_operate()
            && self.controlled_name() == self.pilot
            && self
                .assets
                .as_ref()
                .and_then(|a| a.cargo_unloading.as_ref())
                .is_none_or(|s| {
                    s.schema_version == arkline_protocol::CARGO_UNLOADING_SCHEMA_VERSION
                })
            && self.snapshot.as_ref().is_some_and(|snapshot| {
                self.presentation_time()
                    .saturating_sub(snapshot.server_time_ms)
                    <= 1500
            })
    }

    fn unload_state(&self) -> Option<&arkline_protocol::CargoUnloadingState> {
        if self.controlled_name() != self.pilot {
            return None;
        }
        self.assets
            .as_ref()?
            .cargo_unloading
            .as_ref()
            .filter(|s| s.schema_version == arkline_protocol::CARGO_UNLOADING_SCHEMA_VERSION)
    }
    fn unload_busy(&self) -> bool {
        self.unload_state().is_some_and(|s| s.active().is_some())
    }
    fn unload_request(
        &self,
        units: u64,
        destination: arkline_protocol::CargoUnloadDestination,
    ) -> Request {
        if let Some(s) = self.unload_state() {
            Request::StartCargoUnload {
                operation_id: s.high_watermark.saturating_add(1),
                units,
                destination,
            }
        } else {
            match destination {
                arkline_protocol::CargoUnloadDestination::Personal => Request::DepositOre { units },
                arkline_protocol::CargoUnloadDestination::Corporation => {
                    Request::DepositCorporationOre { units }
                }
            }
        }
    }
    fn unloading_status(&mut self, ui: &mut egui::Ui) {
        let Some(op) = self.unload_state().and_then(|s| s.latest()).cloned() else {
            return;
        };
        ui.strong(format!("{} #{}", op.phase.label(), op.operation_id));
        ui.label(format!(
            "Confirmed {} / {} ore units · {} reserved in hold",
            op.committed_units, op.requested_units, op.reserved_units
        ));
        ui.add(
            egui::ProgressBar::new(op.committed_units as f32 / op.requested_units.max(1) as f32)
                .text("Confirmed batches only"),
        );
        ui.small(format!(
            "{} batch receipts · {:?} storage / {}",
            op.receipts.len(),
            op.destination,
            op.destination_owner
        ));
        ui.small(&op.message);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.service_authority_ready()
                        && op.phase == arkline_protocol::CargoUnloadPhase::Held,
                    egui::Button::new("Resume unloading"),
                )
                .clicked()
            {
                self.send(Request::ResumeCargoUnload {
                    operation_id: op.operation_id,
                });
            }
            if ui
                .add_enabled(
                    self.service_authority_ready() && op.phase.active(),
                    egui::Button::new("Cancel remaining unload"),
                )
                .clicked()
            {
                self.send(Request::CancelCargoUnload {
                    operation_id: op.operation_id,
                });
            }
        });
    }
    fn transfer_block_reason(&self, pilot: &PilotView) -> Option<&'static str> {
        if !self.connected {
            Some("Disconnected · reconnect before transferring ore.")
        } else if self.embedded && self.embedded_paused {
            Some("Choose Resume simulation to transfer ore. The Play workspace does not advance time.")
        } else if self.request_pending {
            Some("Awaiting authority · wait for the previous command before transferring ore.")
        } else if !self.service_authority_ready() {
            Some("Inventory may be stale · wait for a fresh authority snapshot before transferring ore.")
        } else if self.unload_busy() {
            Some("Cargo reserved · finish or cancel unloading before another transfer.")
        } else if pilot.location != Location::Docked {
            Some("Dock at this station to transfer ore.")
        } else {
            None
        }
    }
    pub(super) fn station_panel(&mut self, ui: &mut egui::Ui) {
        let Some(pilot) = self.own() else {
            ui.heading("Station services");
            ui.label("Waiting for the owned ship and its current system.");
            return;
        };
        ui.heading(format!(
            "{} · Arkline Station",
            system_name(pilot.system_id)
        ));
        if pilot.location != Location::Docked {
            ui.colored_label(
                AMBER,
                "Remote station services · Dock to board ships or transfer ore.",
            );
        } else {
            ui.colored_label(self.accent(), "DOCKED · STATION SERVICES AVAILABLE");
        }
        let spec = pilot.ship_class.spec();
        ui.separator();
        ui.strong(format!("Active vessel · {}", spec.name));
        ui.label(format!(
            "{} long · {} wide · {} high",
            format_distance(spec.length_m),
            format_distance(spec.width_m),
            format_distance(spec.height_m)
        ));
        ui.label(format!(
            "{} / {} ore aboard · {} credits",
            pilot.ore,
            cargo_capacity(pilot.ship_class),
            pilot.credits
        ));
        if pilot.location == Location::Docked {
            self.station_actions(ui);
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in [
                (ServiceTab::Hangar, "Ship Hangar"),
                (ServiceTab::Cargo, "Personal Inventory"),
                (ServiceTab::Corporation, "Corporation"),
                (ServiceTab::Exchange, "Exchange"),
            ] {
                if ui.button(label).clicked() {
                    self.service_tab = tab;
                }
            }
        });
        ui.small("Inventory stays accessible in flight. Stored ships and ore remain at their named stations.");
    }
    pub(super) fn hangar_panel(&mut self, ui: &mut egui::Ui) {
        let Some(pilot) = self.own() else {
            ui.heading("Owned ship hangar");
            ui.label("Waiting for the owned vessel and authenticated hangar inventory.");
            return;
        };
        ui.heading("Owned ship hangar");
        ui.horizontal_wrapped(|ui| {
            ui.strong(pilot.ship_class.spec().name);
            ui.colored_label(self.accent(), "ACTIVE");
            ui.small(format!("{} ore aboard", pilot.ore));
        });
        let active = pilot.ship_class.spec();
        ui.small(format!(
            "{} · {}/{} ore cargo",
            active
                .mining_tier
                .map_or("Utility hull".into(), |tier| format!(
                    "Mining tier {tier}/5"
                )),
            pilot.ore,
            active.cargo_capacity
        ));
        self.power_gauge(ui);
        self.ship_power_accounting(ui);
        ui.small("Each ship retains its cargo and capacitor charge.");
        if self.assets.as_ref().is_some_and(|a| a.hangar.is_some()) {
            ui.small("Leave your ship, then walk to the named vessel's boarding access in the physical dock. Boarding requires proximity.");
        } else {
            ui.small("Enter your current ship before switching to a stored hull at its station.");
        }
        let Some(mut assets) = self.assets.clone() else {
            ui.colored_label(AMBER, "Waiting for authenticated hangar inventory…");
            return;
        };
        if assets.ships.is_empty() {
            ui.label("No inactive ships in your hangars.");
        }
        assets
            .ships
            .sort_by_key(|ship| ship.ship_class.spec().mining_tier.unwrap_or(9));
        for ship in &assets.ships {
            let spec = ship.ship_class.spec();
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.strong(spec.name);
                ui.label(format!(
                    "{} station · {} ore",
                    system_name(ship.system_id),
                    ship.ore
                ));
                let enabled = self.service_authority_ready()
                    && pilot.location == Location::Docked
                    && pilot.system_id == ship.system_id
                    && pilot.station_avatar.is_none();
                if assets.hangar.is_some() {
                    ui.small("Board at physical berth");
                } else if ui
                    .add_enabled(enabled, egui::Button::new("Board ship"))
                    .on_hover_text("Requires docking at the ship's station")
                    .clicked()
                {
                    self.send(Request::SelectShip {
                        ship_class: ship.ship_class,
                    });
                }
            });
            ui.small(format!(
                "{} · {} ore cargo",
                spec.mining_tier
                    .map_or("Utility hull".into(), |tier| format!(
                        "Mining tier {tier}/5"
                    )),
                spec.cargo_capacity
            ));
            let extraction = mining_spec(ship.ship_class);
            let energy = power_spec(ship.ship_class);
            ui.small(format!(
                "Extraction: up to {} ore / {:.1}s per turret · {} J / cycle",
                extraction.yield_units,
                extraction.cycle_ms as f64 / 1000.0,
                energy.mining_cycle_cost_j
            ));
            if let Some(power) = &ship.power {
                ui.small(format!(
                    "Capacitor {:.1}/{:.1} kJ · reactor rating {} W",
                    power.charge_j as f64 / 1000.0,
                    power.capacity_j as f64 / 1000.0,
                    power.regen_w
                ));
            } else {
                ui.small("Stored capacitor data unavailable");
            }
            self.parked_power_accounting(ui, ship);
            ui.small(format!(
                "{} × {} × {}",
                format_distance(spec.length_m),
                format_distance(spec.width_m),
                format_distance(spec.height_m)
            ));
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 8.0), egui::Sense::hover());
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    rect.min,
                    Vec2::new(
                        rect.width() * (spec.length_m / ShipClass::Monolith.spec().length_m) as f32,
                        5.0,
                    ),
                ),
                1.0,
                self.accent(),
            );
        }
        ui.separator();
        ui.small(
            "Hull bars share one scale. Extraction, cargo and reactor specifications belong to the selected hull.",
        );
    }
    pub(super) fn cargo_panel(&mut self, ui: &mut egui::Ui) {
        let Some(pilot) = self.own() else {
            ui.heading("Cargo & storage");
            ui.label("Waiting for owned ship inventory. No cargo quantity is inferred.");
            return;
        };
        ui.heading("Cargo & storage");
        ui.label("Confirmed holdings · shared with ship instruments and the mining routine");
        let capacity = cargo_capacity(pilot.ship_class);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.strong(format!("ACTIVE HOLD · {}", pilot.ship_class.spec().name));
            ui.label(format!(
                "{} / {} ore · {} ore free",
                pilot.ore,
                capacity,
                capacity.saturating_sub(pilot.ore)
            ));
            ui.add(
                egui::ProgressBar::new(
                    (pilot.ore as f64 / capacity.max(1) as f64).clamp(0.0, 1.0) as f32
                )
                .text("Confirmed hold use"),
            );
            match target_controls::supported_drone_cargo(&pilot) {
                Ok(Some(cargo)) => {
                    ui.label(format!("DRONES → SHIP · {} incoming reserved · {} available for new ore", cargo.incoming_reserved_units,
                        cargo.available_for_new_ore(pilot.ore, capacity).expect("validated headroom")));
                    ui.small("Incoming reservation includes drone-held ore and accepted mining-cycle space; it is not extra ship-held ore.");
                }
                Err(reason) => { ui.colored_label(AMBER, reason); }
                Ok(None) => {}
            }
            if let Some(operation) = self.unload_state().and_then(|state| state.active()) {
                ui.small(format!("SHIP → STATION · {} units reserved for {:?} storage; already included in ship-held ore", operation.reserved_units, operation.destination));
            }
            if !self.connected {
                ui.colored_label(AMBER, "Disconnected · last received inventory");
            } else if self.embedded && self.embedded_paused {
                ui.colored_label(MUTED, "Simulation paused");
            }
        });
        // Active transfer controls must not be buried below the stored fleet.
        self.unloading_status(ui);
        let escrow = self.snapshot.as_ref().map_or(0, |s| {
            s.orders
                .iter()
                .filter(|o| o.seller == pilot.name)
                .map(|o| o.units)
                .sum::<u64>()
        });
        egui::Grid::new("personal-inventory")
            .num_columns(3)
            .min_col_width(0.0)
            .max_col_width(
                ((ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0).max(1.0),
            )
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Container");
                ui.strong("Location");
                ui.strong("Ore");
                ui.end_row();
                ui.label(format!("Active {} cargo", pilot.ship_class.spec().name));
                ui.label(system_name(pilot.system_id));
                ui.label(pilot.ore.to_string());
                ui.end_row();
                ui.label("Exchange escrow");
                ui.label("Open sell orders");
                ui.label(escrow.to_string());
                ui.end_row();
                if let Some(assets) = &self.assets {
                    for ship in &assets.ships {
                        ui.label(format!("Stored {} cargo", ship.ship_class.spec().name));
                        ui.label(system_name(ship.system_id));
                        ui.label(ship.ore.to_string());
                        ui.end_row();
                    }
                    for storage in &assets.storage {
                        ui.label("Personal station storage");
                        ui.label(system_name(storage.system_id));
                        ui.label(storage.ore.to_string());
                        ui.end_row();
                    }
                }
            });
        let Some(assets) = self.assets.clone() else {
            ui.colored_label(
                AMBER,
                "Waiting for authenticated station and ship inventory…",
            );
            return;
        };
        ui.separator();
        let stored = assets
            .storage
            .iter()
            .find(|s| s.system_id == pilot.system_id)
            .map_or(0, |s| s.ore);
        ui.label(format!(
            "{} station storage: {} ore",
            system_name(pilot.system_id),
            stored
        ));
        ui.small("Transfers move ore between active cargo and this station's personal storage.");
        let fresh = self.connected
            && self.snapshot.as_ref().is_some_and(|snapshot| {
                self.presentation_time()
                    .saturating_sub(snapshot.server_time_ms)
                    <= 1500
            });
        let docked = self.service_authority_ready()
            && fresh
            && !self.unload_busy()
            && pilot.location == Location::Docked;
        if !fresh {
            ui.colored_label(
                AMBER,
                "Inventory may be stale · transfers disabled until authority refreshes",
            );
        }
        if self.request_pending {
            ui.colored_label(AMBER, "Awaiting authority · transfer or command pending");
        }
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::DragValue::new(&mut self.transfer_amount)
                    .range(1..=cargo_capacity(pilot.ship_class))
                    .suffix(" ore"),
            );
            if ui
                .add_enabled(
                    docked && self.transfer_amount <= pilot.ore,
                    egui::Button::new("Store ore"),
                )
                .clicked()
            {
                self.send(self.unload_request(
                    self.transfer_amount,
                    arkline_protocol::CargoUnloadDestination::Personal,
                ));
            }
            if ui
                .add_enabled(
                    docked
                        && self.transfer_amount <= stored
                        && Self::incoming_ore_headroom(&pilot)
                            .is_ok_and(|available| self.transfer_amount <= available),
                    egui::Button::new("Load ore"),
                )
                .clicked()
            {
                self.send(Request::WithdrawOre {
                    units: self.transfer_amount,
                });
            }
        });
        if let Some(reason) = self.transfer_block_reason(&pilot) {
            ui.small(reason);
        }
    }
    pub(super) fn corporation_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Corporation");
        self.unloading_status(ui);
        let Some(pilot) = self.own() else {
            ui.label("Waiting for the authenticated pilot and corporation membership.");
            return;
        };
        let Some(assets) = self.assets.clone() else {
            ui.colored_label(AMBER, "Waiting for authenticated corporation information…");
            return;
        };
        if let Some(corp) = assets.corporation {
            ui.strong(&corp.name);
            let owner = corp.owner == pilot.name;
            ui.label(format!(
                "Owner: {} · Your role: {}",
                corp.owner,
                if owner { "Owner" } else { "Member" }
            ));
            ui.collapsing(format!("Members ({})", corp.members.len()), |ui| {
                for member in &corp.members {
                    ui.label(member);
                }
            });
            if owner {
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.invite_pilot)
                            .hint_text("Registered pilot name"),
                    );
                    if ui
                        .add_enabled(
                            self.service_authority_ready() && !self.invite_pilot.trim().is_empty(),
                            egui::Button::new("Invite pilot"),
                        )
                        .clicked()
                    {
                        self.send(Request::InviteCorporation {
                            pilot: self.invite_pilot.trim().into(),
                        });
                    }
                });
            }
            ui.separator();
            ui.strong("Corporation station storage");
            for storage in &corp.storage {
                ui.label(format!(
                    "{} · {} ore",
                    system_name(storage.system_id),
                    storage.ore
                ));
            }
            if corp.storage.is_empty() {
                ui.label("No ore in corporation storage.");
            }
            let stored = corp
                .storage
                .iter()
                .find(|s| s.system_id == pilot.system_id)
                .map_or(0, |s| s.ore);
            let docked = self.service_authority_ready()
                && !self.unload_busy()
                && pilot.location == Location::Docked;
            ui.small(format!(
                "{} station · Members deposit; owner withdraws into active cargo.",
                system_name(pilot.system_id)
            ));
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::DragValue::new(&mut self.transfer_amount)
                        .range(1..=cargo_capacity(pilot.ship_class))
                        .suffix(" ore"),
                );
                if ui
                    .add_enabled(
                        docked && self.transfer_amount <= pilot.ore,
                        egui::Button::new("Donate ore"),
                    )
                    .clicked()
                {
                    self.send(self.unload_request(
                        self.transfer_amount,
                        arkline_protocol::CargoUnloadDestination::Corporation,
                    ));
                }
                if ui
                    .add_enabled(
                        docked
                            && owner
                            && self.transfer_amount <= stored
                            && Self::incoming_ore_headroom(&pilot)
                                .is_ok_and(|available| self.transfer_amount <= available),
                        egui::Button::new("Withdraw ore"),
                    )
                    .clicked()
                {
                    self.send(Request::WithdrawCorporationOre {
                        units: self.transfer_amount,
                    });
                }
            });
            if let Some(reason) = self.transfer_block_reason(&pilot) {
                ui.small(reason);
            }
        } else {
            ui.label("You are not a member of a corporation.");
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.corporation_name)
                        .hint_text("Corporation name"),
                );
                if ui
                    .add_enabled(
                        self.service_authority_ready() && !self.corporation_name.trim().is_empty(),
                        egui::Button::new("Create corporation"),
                    )
                    .clicked()
                {
                    self.send(Request::CreateCorporation {
                        name: self.corporation_name.trim().into(),
                    });
                }
            });
            ui.separator();
            ui.strong("Invitations");
            if assets.invitations.is_empty() {
                ui.label("No pending invitations.");
            }
            for name in assets.invitations {
                ui.horizontal_wrapped(|ui| {
                    ui.label(&name);
                    if ui
                        .add_enabled(
                            self.service_authority_ready(),
                            egui::Button::new("Accept invitation"),
                        )
                        .clicked()
                    {
                        self.send(Request::AcceptCorporation { corporation: name });
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod unloading_tests {
    use super::*;
    fn cockpit() -> Cockpit {
        let mut c = crate::tests::view_test_cockpit(&egui::Context::default());
        c.connected = true;
        c.request_pending = false;
        c.embedded_paused = false;
        c.frozen_time = c.snapshot.as_ref().map(|s| s.server_time_ms);
        c.assets = Some(AssetsView {
            carrier_inventory: None,
            drone_bay: None,
            drone_cargo: None,
            cargo_unloading: Some(arkline_protocol::CargoUnloadingState::default()),
            pioneer_room: None,
            pioneer_room_layout: None,
            hangar: None,
            pioneer_interior_available: true,
            pioneer_interior_decks: 2,
            ships: vec![],
            storage: vec![],
            corporation: None,
            invitations: vec![],
        });
        c
    }
    #[test]
    fn unload_requests_use_confirmed_high_watermark_and_capability() {
        let mut c = cockpit();
        c.assets
            .as_mut()
            .unwrap()
            .cargo_unloading
            .as_mut()
            .unwrap()
            .high_watermark = 8;
        assert!(matches!(
            c.unload_request(57, arkline_protocol::CargoUnloadDestination::Personal),
            Request::StartCargoUnload {
                operation_id: 9,
                units: 57,
                ..
            }
        ));
        assert!(matches!(
            c.unload_request(57, arkline_protocol::CargoUnloadDestination::Personal),
            Request::StartCargoUnload {
                operation_id: 9,
                ..
            }
        ));
        c.assets.as_mut().unwrap().cargo_unloading = None;
        assert!(matches!(
            c.unload_request(57, arkline_protocol::CargoUnloadDestination::Personal),
            Request::DepositOre { units: 57 }
        ));
    }
    #[test]
    fn unknown_unload_schema_does_not_enable_commands() {
        let mut c = cockpit();
        assert!(c.service_authority_ready());
        c.assets
            .as_mut()
            .unwrap()
            .cargo_unloading
            .as_mut()
            .unwrap()
            .schema_version = 99;
        assert!(!c.service_authority_ready());
        assert!(c.unload_state().is_none());
    }
    #[test]
    fn remote_worker_view_cannot_use_owners_private_transfer_capability() {
        let mut c = cockpit();
        c.manual_remote = Some("Worker".into());
        c.snapshot
            .as_mut()
            .unwrap()
            .remote_controls
            .push(arkline_protocol::RemoteControlLease {
                controller: c.pilot.clone(),
                pilot: "Worker".into(),
            });
        assert_eq!(c.controlled_name(), "Worker");
        assert!(c.unload_state().is_none());
        assert!(!c.service_authority_ready());
    }
    #[test]
    fn active_transfer_controls_stay_above_long_inventory_without_scroll() {
        use arkline_protocol::{
            CargoUnloadDestination, CargoUnloadOperation, CargoUnloadPhase, StationStorage,
        };
        let ctx = egui::Context::default();
        let mut c = cockpit();
        let before = serde_json::to_value(&c.snapshot).unwrap();
        let assets = c.assets.as_mut().unwrap();
        assets.storage = (0..24)
            .map(|system_id| StationStorage { system_id, ore: 0 })
            .collect();
        let state = assets.cargo_unloading.as_mut().unwrap();
        state.high_watermark = 1;
        state.operations.push(CargoUnloadOperation {
            operation_id: 1,
            pilot: "pilot".into(),
            vessel_id: "fixture".into(),
            ship_class: ShipClass::Pioneer,
            system_id: HOME_SYSTEM_ID,
            berth_id: 0,
            destination: CargoUnloadDestination::Personal,
            destination_owner: "pilot".into(),
            requested_units: 100,
            committed_units: 25,
            reserved_units: 75,
            phase: CargoUnloadPhase::Held,
            next_batch_at_ms: 1500,
            job_id: None,
            message: "Held; confirmed quantities only".into(),
            receipts: vec![],
        });
        for _ in 0..2 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(600., 500.))),
                    focused: true,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| c.cargo_panel(ui));
                    });
                },
            );
            let visible: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape {
                        let point = t.pos + t.galley.rect.center().to_vec2();
                        s.clip_rect
                            .contains(point)
                            .then(|| t.galley.job.text.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            for label in [
                "Unloading held #1",
                "Resume unloading",
                "Cancel remaining unload",
            ] {
                assert!(
                    visible.contains(&label),
                    "active control hidden below inventory: {label}; {visible:?}"
                );
            }
        }
        assert_eq!(serde_json::to_value(&c.snapshot).unwrap(), before);
    }
}
