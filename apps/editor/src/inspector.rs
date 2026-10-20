//! Read-only inspection of the latest server snapshot, never predicted telemetry.
use arkline_protocol::{pioneer_room_catalog, AssetsView, ShipClass, Snapshot};
use eframe::egui::{self, Color32, RichText};

/// `assets` must belong to the selected pilot. Pass None for another pilot's selection.
/// The transport owner must clear both inputs when their source is no longer valid.
pub fn show(
    ui: &mut egui::Ui,
    snapshot: Option<&Snapshot>,
    assets: Option<&AssetsView>,
    pilot: &str,
) {
    ui.heading(RichText::new("Inspector").color(Color32::from_rgb(76, 224, 148)));
    ui.small("Read-only · latest server report");
    let Some(snapshot) = snapshot else {
        ui.separator();
        ui.label("No live snapshot. Connect a local play session to inspect it.");
        return;
    };
    ui.small(format!("Server time {} ms", snapshot.server_time_ms));
    let Some(p) = snapshot.pilots.iter().find(|p| p.name == pilot) else {
        ui.label(if pilot.is_empty() {
            "No pilot selected."
        } else {
            "Selected pilot is absent from this snapshot."
        });
        return;
    };
    ui.push_id(&p.name, |ui| {
        egui::CollapsingHeader::new("Pilot and vessel").default_open(true).show(ui, |ui| {
            row(ui,"Pilot",&p.name);
            row(ui,"Presence",if snapshot.online_pilots.contains(&p.name) { "Online" } else { "Offline" });
            row(ui,"Location",format!("{:?} · system {}",p.location,p.system_id));
            let spec=p.ship_class.spec();
            row(ui,"Ship",spec.name);
            row(ui,"Hull definition",format!("{} × {} × {} m",spec.length_m,spec.width_m,spec.height_m));
            row(ui,"Ore aboard",format!("{} / {} units",p.ore,spec.cargo_capacity));
            row(ui,"Credits",p.credits.to_string());
        });
        egui::CollapsingHeader::new("Position and motion").default_open(true).show(ui, |ui| {
            if let Some(a)=&p.station_avatar {
                if let Some(state)=&a.docked_traversal {
                    ui.small("Avatar: station-world coordinates (meters)");
                    row(ui,"Vessel",&state.binding.host_vessel_id);
                    row(ui,"Support",format!("{:?}",state.support));
                    vector(ui,"Feet",state.feet_m,"m");
                    row(ui,"Heading",format!("{:.3} rad",state.heading));
                    row(ui,"Moving",state.moving.to_string());
                    row(ui,"Grounded",state.grounded.to_string());
                    row(ui,"Seat transition",state.seat_transition.as_ref().map_or("None".into(),|phase|format!("{:?}",phase.phase)));
                    row(ui,"Avatar update",format!("{} ms",state.updated_at_ms));
                } else {
                    ui.small(if a.onboard_interior { "Avatar: deck-local coordinates (meters)" } else { "Avatar: hangar-local coordinates (meters)" });
                    if a.onboard_interior { row(ui,"Deck",a.interior_deck.to_string()); }
                    vector(ui,"Feet",a.position,"m");
                    row(ui,"Heading",format!("{:.3} rad",a.heading));
                    row(ui,"Moving",a.moving.to_string());
                    row(ui,"Seated",a.seated.to_string());
                    row(ui,"Jump start",a.jump_started_ms.map_or("Grounded".into(),|v|format!("{v} ms")));
                    row(ui,"Avatar update",format!("{} ms",a.updated_at_ms));
                }
            }
            if let Some(f)=&p.flight {
                ui.small("Ship: system-space coordinates");
                vector(ui,"Position",f.position,"m");
                vector(ui,"Velocity",f.velocity,"m/s");
                vector(ui,"Acceleration",f.acceleration,"m/s²");
                row(ui,"Speed",format!("{:.2} m/s",f.velocity.iter().fold(0_f64,|n,v|n.hypot(*v))));
                row(ui,"Navigation",format!("{:?}",f.mode));
                row(ui,"Flight update",format!("{} ms",f.updated_at_ms));
                if let Some(w)=&f.warp { row(ui,"Warp arrival",format!("{} ms server time",w.arrives_at_ms)); }
            } else { ui.small("No ship flight state reported."); }
        });
        egui::CollapsingHeader::new("Power").default_open(true).show(ui, |ui| {
            if let Some(power)=&p.power {
                row(ui,"Capacitor",format!("{} / {} J",power.charge_j,power.capacity_j));
                row(ui,"Regeneration rating",format!("{} W",power.regen_w));
                row(ui,"Reported at",format!("{} ms",power.updated_at_ms));
                ui.small("Regeneration is a rating; net consumption is not independently reported here.");
            } else { ui.label("No power telemetry reported."); }
        });
        ui.collapsing(format!("Mining modules ({})",p.mining_modules.len()),|ui| {
            for module in &p.mining_modules {
                ui.push_id(module.module_id,|ui| {
                    ui.strong(format!("Module {}",module.module_id));
                    row(ui,"Target",module.target_id.map_or("None".into(),|v|v.to_string()));
                    row(ui,"Cycle deadline",module.cycle_completes_at_ms.map_or("Idle".into(),|v|format!("{v} ms")));
                    if let Some(reason)=&module.stop_reason { row(ui,"Stopped",reason); }
                });
            }
        });
        ui.collapsing(format!("Drones ({})",p.drones.len()),|ui| {
            if p.drones.is_empty() { ui.small("No deployed fleet drones reported."); }
            for drone in &p.drones {
                ui.push_id(drone.id,|ui| {
                    ui.collapsing(format!("#{} · {:?}",drone.id,drone.class),|ui| {
                        row(ui,"Mode",format!("{:?}",drone.mode));
                        row(ui,"Hit points",drone.hit_points.to_string());
                        row(ui,"Reported signal",format!("{:.1}%",drone.signal_strength*100.));
                        vector(ui,"Position",drone.position,"m");
                        vector(ui,"Velocity",drone.velocity,"m/s");
                        row(ui,"Updated",format!("{} ms",drone.updated_at_ms));
                        if let Some(c)=&drone.mining_cycle { row(ui,"Mining deadline",format!("{} ms",c.completes_at_ms)); }
                        if let Some(t)=drone.attack_ready_at_ms { row(ui,"Attack ready",format!("{t} ms")); }
                    });
                });
            }
        });
        if p.ship_class == ShipClass::Pioneer {
            ui.collapsing("Pioneer airlock equipment",|ui| {
                let Some(room)=assets.and_then(|a|a.pioneer_room.as_ref()) else { ui.label("No private room telemetry available for this selection.");return; };
                row(ui,"Vessel ID",&room.vessel_id);
                row(ui,"Commissioned",room.commissioned.to_string());
                row(ui,"Breaker",if room.breaker_closed {"Closed"}else{"Open"});
                row(ui,"Light output",if room.light_on {"On"}else{"Off"});
                row(ui,"Emergency output",if room.emergency_light_on {"On"}else{"Off"});
                row(ui,"Reserve remaining",format!("{:.1} s",room.emergency_remaining_ms as f64/1000.));
                row(ui,"Vent command",format!("{:?}",room.vent_commanded));
                row(ui,"Vent output",if room.vent_running {"Running"}else{"Stopped"});
                row(ui,"Door output",if room.door_open {"Open"}else{"Closed"});
                row(ui,"Pressure",format!("{:.2} kPa",room.pressure_pa as f64/1000.));
                row(ui,"Room update",format!("{} ms",room.updated_at_ms));
                if let Some(reason)=&room.interlock { row(ui,"Interlock",reason); }
                let catalog=pioneer_room_catalog();
                ui.small(format!("Definition v{} · {}",catalog.schema_version,catalog.room_id));
                ui.small("Catalog ratings and connections describe the implemented controller; they are not independent sensor readings.");
                for device in catalog.equipment {
                    ui.push_id(device.id,|ui| {ui.collapsing(device.label,|ui| {
                        ui.monospace(device.id);
                        if let Some(pos)=device.position { vector(ui,"Deck-local mount",pos,"m"); } else { ui.small("Logical endpoint; no physical mount."); }
                        row(ui,"Operating load rating",device.active_load_w.map_or("Not separately metered".into(),|v|format!("{v} W")));
                        for port in device.ports { ui.small(format!("{} · {:?} {:?}",port.id,port.kind,port.direction)); }
                        for edge in catalog.connections.iter().filter(|c|c.from_equipment==device.id) {
                            ui.small(format!("{} -> {} / {} ({:?})",edge.from_port,edge.to_equipment,edge.to_port,edge.kind));
                        }
                    });});
                }
            });
        }
    });
}
fn row(ui: &mut egui::Ui, label: &str, value: impl AsRef<str>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("{label}: ")).color(Color32::from_gray(160)));
        ui.label(value.as_ref());
    });
}
fn vector(ui: &mut egui::Ui, label: &str, v: [f64; 3], unit: &str) {
    row(
        ui,
        label,
        format!("x {:.2} · y {:.2} · z {:.2} {unit}", v[0], v[1], v[2]),
    );
}
