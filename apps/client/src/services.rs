//! Private station, inventory and corporation views from authenticated server assets.
use super::*;
impl Cockpit {
    pub(super) fn station_panel(&mut self, ui: &mut egui::Ui) {
        let Some(pilot) = self.own() else { return };
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
            pilot.ore, CARGO_CAPACITY, pilot.credits
        ));
        if ui
            .add_enabled(
                self.can_operate() && pilot.location == Location::Docked,
                egui::Button::new("Undock active vessel"),
            )
            .clicked()
        {
            self.send(Request::Undock);
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
        let Some(pilot) = self.own() else { return };
        ui.heading("Owned ship hangar");
        ui.horizontal(|ui| {
            ui.strong(pilot.ship_class.spec().name);
            ui.colored_label(self.accent(), "ACTIVE");
            ui.small(format!("{} ore aboard", pilot.ore));
        });
        self.power_gauge(ui);
        ui.small("Each ship retains its cargo and capacitor charge. Board a stored ship only while docked at its station.");
        let Some(assets) = self.assets.clone() else {
            ui.colored_label(AMBER, "Waiting for authenticated hangar inventory…");
            return;
        };
        if assets.ships.is_empty() {
            ui.label("No inactive ships in your hangars.");
        }
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
                let enabled = self.can_operate()
                    && pilot.location == Location::Docked
                    && pilot.system_id == ship.system_id;
                if ui
                    .add_enabled(enabled, egui::Button::new("Board ship"))
                    .on_hover_text("Requires docking at the ship's station")
                    .clicked()
                {
                    self.send(Request::SelectShip {
                        ship_class: ship.ship_class,
                    });
                }
            });
            if let Some(power) = &ship.power {
                ui.small(format!(
                    "Capacitor {:.1}/{:.1} kJ · +{} W",
                    power.charge_j as f64 / 1000.0,
                    power.capacity_j as f64 / 1000.0,
                    power.regen_w
                ));
            } else {
                ui.small("Stored capacitor data unavailable");
            }
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
                    Vec2::new(rect.width() * (spec.length_m / 1100.0) as f32, 5.0),
                ),
                1.0,
                self.accent(),
            );
        }
        ui.separator();
        ui.small(
            "Hull bars share one scale. Preview hulls share flight tuning and cargo capacity.",
        );
    }
    pub(super) fn cargo_panel(&mut self, ui: &mut egui::Ui) {
        let Some(pilot) = self.own() else { return };
        ui.heading("Personal inventory");
        ui.small("Your server-confirmed holdings · accessible from any location");
        let escrow = self.snapshot.as_ref().map_or(0, |s| {
            s.orders
                .iter()
                .filter(|o| o.seller == pilot.name)
                .map(|o| o.units)
                .sum::<u64>()
        });
        egui::Grid::new("personal-inventory")
            .num_columns(3)
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
        let docked = self.can_operate() && pilot.location == Location::Docked;
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::DragValue::new(&mut self.transfer_amount)
                    .range(1..=CARGO_CAPACITY)
                    .suffix(" ore"),
            );
            if ui
                .add_enabled(
                    docked && self.transfer_amount <= pilot.ore,
                    egui::Button::new("Store ore"),
                )
                .clicked()
            {
                self.send(Request::DepositOre {
                    units: self.transfer_amount,
                });
            }
            if ui
                .add_enabled(
                    docked
                        && self.transfer_amount <= stored
                        && pilot.ore.saturating_add(self.transfer_amount) <= CARGO_CAPACITY,
                    egui::Button::new("Load ore"),
                )
                .clicked()
            {
                self.send(Request::WithdrawOre {
                    units: self.transfer_amount,
                });
            }
        });
        if !docked {
            ui.small("Dock at this station to transfer. Board a stored ship to access its cargo.");
        }
    }
    pub(super) fn corporation_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Corporation");
        let Some(pilot) = self.own() else { return };
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
                            self.can_operate() && !self.invite_pilot.trim().is_empty(),
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
            let docked = self.can_operate() && pilot.location == Location::Docked;
            ui.small(format!(
                "{} station · Members deposit; owner withdraws into active cargo.",
                system_name(pilot.system_id)
            ));
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::DragValue::new(&mut self.transfer_amount)
                        .range(1..=CARGO_CAPACITY)
                        .suffix(" ore"),
                );
                if ui
                    .add_enabled(
                        docked && self.transfer_amount <= pilot.ore,
                        egui::Button::new("Donate ore"),
                    )
                    .clicked()
                {
                    self.send(Request::DepositCorporationOre {
                        units: self.transfer_amount,
                    });
                }
                if ui
                    .add_enabled(
                        docked
                            && owner
                            && self.transfer_amount <= stored
                            && pilot.ore.saturating_add(self.transfer_amount) <= CARGO_CAPACITY,
                        egui::Button::new("Withdraw ore"),
                    )
                    .clicked()
                {
                    self.send(Request::WithdrawCorporationOre {
                        units: self.transfer_amount,
                    });
                }
            });
            if !docked {
                ui.small("Dock at the station to transfer corporation ore.");
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
                        self.can_operate() && !self.corporation_name.trim().is_empty(),
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
                ui.horizontal(|ui| {
                    ui.label(&name);
                    if ui
                        .add_enabled(self.can_operate(), egui::Button::new("Accept invitation"))
                        .clicked()
                    {
                        self.send(Request::AcceptCorporation { corporation: name });
                    }
                });
            }
        }
    }
}
