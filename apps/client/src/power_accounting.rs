//! Read-only projection of the authority's existing electrical model.
//! No client-side consumption simulation, inferred motor ratings or mass totals.
use super::*;

fn confirmed(pilot: &PilotView, snapshot_time: u64) -> Result<&PowerAccountingView, &'static str> {
    let accounting = pilot
        .power_accounting
        .as_ref()
        .ok_or("Electrical accounting unavailable on this server")?;
    let power = pilot
        .power
        .as_ref()
        .ok_or("Capacitor telemetry unavailable")?;
    accounting
        .validate_for(&pilot.name, pilot.ship_class, power)
        .map_err(|_| "Electrical accounting identity or values unconfirmed")?;
    if accounting.sampled_at_ms != snapshot_time {
        return Err("Electrical accounting belongs to a different authority sample");
    }
    let bindings = [
        pilot
            .drone_bay
            .as_ref()
            .map(|s| s.binding.vessel_id.as_str()),
        pilot
            .cockpit_mechanisms
            .as_ref()
            .map(|s| s.vessel_id.as_str()),
        pilot
            .reactor_configuration
            .as_ref()
            .map(|s| s.vessel_id.as_str()),
        pilot
            .flight
            .as_ref()
            .and_then(|s| s.docking.as_ref())
            .map(|s| s.host_vessel_id.as_str()),
    ];
    if bindings
        .into_iter()
        .flatten()
        .any(|id| id != accounting.vessel_id)
    {
        return Err("Electrical accounting does not match the viewed physical vessel");
    }
    Ok(accounting)
}

impl Cockpit {
    /// Existing engineering/Fleet workspace only; no new physical screen or commands.
    pub(super) fn ship_power_accounting(&self, ui: &mut egui::Ui) {
        let Some(pilot) = self.own() else {
            ui.small("Electrical accounting · waiting for viewed vessel");
            return;
        };
        let sample = self.snapshot.as_ref().map(|s| s.server_time_ms);
        ui.collapsing("Electrical accounting · existing modeled circuits", |ui| {
            let Some(sample) = sample else { ui.label("Waiting for authority sample"); return; };
            let view = match confirmed(&pilot, sample) {
                Ok(view) => view,
                Err(reason) => { ui.colored_label(AMBER, reason); return; }
            };
            ui.small(if !self.connected {
                "Disconnected · last confirmed accounting, not live output"
            } else if self.embedded_paused {
                "Paused · confirmed model state, no wall-clock energy integration"
            } else if !self.live_command_snapshot_fresh() {
                "Stale · last confirmed accounting, not live output"
            } else {
                "Authority-confirmed model rates at this snapshot"
            });
            ui.small(format!("{} · {}", view.owner_pilot, view.vessel_id));
            ui.label(format!("Reactor rating: {} W", view.reactor_rating_w));
            ui.label(format!("Main-bus reservations: {} W", view.accounted_reserved_w));
            ui.label(format!("Net available charging: {} W", view.net_available_charge_w));
            ui.label(format!("Model capacitor storage: {} W{}", view.stored_charge_rate_w,
                if view.charge_j == view.capacity_j { " · capacitor full" } else { "" }));
            ui.small("Watts are model rates per simulation second, not measured consumption or energy over an interval. Paused samples do not charge by wall clock. Full capacity stops stored charging, not the reactor rating.");
            if let Some(reserve) = &view.emergency_reserve {
                ui.separator();
                ui.strong("Emergency reserve · independent output");
                ui.label(format!("Reserve: {:.1} / {:.1} simulation seconds · output {} W", reserve.remaining_ms as f64 / 1000., reserve.capacity_ms as f64 / 1000., reserve.output_w));
                ui.small(format!("Main-bus recharge reservation: {} W (already included above). Reserve output is not added to the main-bus total.", reserve.recharge_reserved_w));
            }
            ui.separator();
            ui.strong("Accounted main-bus reservations");
            for row in &view.loads {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{} W", row.reserved_w));
                    ui.label(&row.label).on_hover_text(&row.component_id);
                });
            }
            ui.small("Enabled exterior reservations remain accounted during a strobe's dark interval. Visual output is not a separate power meter.");
            ui.collapsing("Instantaneous debits · joules, not watts", |ui| {
                let d = &view.instantaneous_debits;
                for (label, joules) in [("Warp", d.warp_j), ("Turret mining cycle", d.mining_cycle_j),
                    ("Drone mining cycle", d.drone_mining_cycle_j), ("Drone weapon shot", d.drone_attack_j)] {
                    ui.label(format!("{label}: {joules} J per successful action"));
                }
                ui.small("Existing capacitor debits are separate from the continuous reservations above; they are not added again as watts.");
            });
            ui.collapsing("Not metered · unknown is not zero", |ui| {
                for row in &view.unmetered { ui.label(&row.label).on_hover_text(&row.component_id); }
                ui.small("No motor, propulsion or life-support consumption is inferred from animation. Physical kilogram mass and thermal accounting are not implemented.");
            });
        });
    }
}

impl Cockpit {
    pub(super) fn parked_power_accounting(&self, ui: &mut egui::Ui, ship: &HangarShip) {
        let Some(view) = ship.power_accounting.as_ref() else {
            ui.small("Stored-hull electrical breakdown unavailable");
            return;
        };
        let valid = ship.power.as_ref().is_some_and(|power| {
            view.validate_for(&self.pilot, ship.ship_class, power)
                .is_ok()
        }) && self
            .snapshot
            .as_ref()
            .is_some_and(|s| view.sampled_at_ms == s.server_time_ms);
        if !valid {
            ui.colored_label(
                AMBER,
                "Stored-hull electrical identity or sample unconfirmed",
            );
            return;
        }
        ui.small(format!("Owned parked hull · reserved {} W · net available {} W · storing {} W{}",
            view.accounted_reserved_w, view.net_available_charge_w, view.stored_charge_rate_w,
            if !self.connected { " · last confirmed" } else if self.embedded_paused { " · paused" } else if !self.live_command_snapshot_fresh() { " · stale" } else { "" }))
            .on_hover_text(format!("Actual hull: {}. Existing modeled reservations only; unmetered systems are not zero consumption.",view.vessel_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Cockpit {
        let mut c = crate::tests::view_test_cockpit(&egui::Context::default());
        let snapshot = c.snapshot.as_mut().unwrap();
        let p = snapshot
            .pilots
            .iter_mut()
            .find(|p| p.name == c.pilot)
            .unwrap();
        let spec = power_spec(p.ship_class);
        p.power = Some(PowerState {
            charge_j: 100_000,
            capacity_j: spec.capacity_j,
            regen_w: spec.regen_w,
            updated_at_ms: snapshot.server_time_ms,
            regen_remainder_mj: 0,
        });
        let vessel = p
            .cockpit_mechanisms
            .as_ref()
            .map(|s| s.vessel_id.clone())
            .or_else(|| {
                p.flight
                    .as_ref()
                    .and_then(|f| f.docking.as_ref())
                    .map(|d| d.host_vessel_id.clone())
            })
            .unwrap_or_else(|| "explicit-test-owned-hull".into());
        p.power_accounting = Some(PowerAccountingView {
            schema_version: POWER_ACCOUNTING_SCHEMA_VERSION,
            owner_pilot: p.name.clone(),
            vessel_id: vessel,
            ship_class: p.ship_class,
            sampled_at_ms: snapshot.server_time_ms,
            charge_j: 100_000,
            capacity_j: spec.capacity_j,
            reactor_rating_w: spec.regen_w,
            loads: vec![PowerLoadRow {
                component_id: "pioneer.deck1.airlock.light".into(),
                label: "Airlock main light".into(),
                reserved_w: 24,
            }],
            accounted_reserved_w: 24,
            net_available_charge_w: spec.regen_w - 24,
            stored_charge_rate_w: spec.regen_w - 24,
            instantaneous_debits: PowerDebitRatings {
                warp_j: spec.warp_cost_j,
                mining_cycle_j: spec.mining_cycle_cost_j,
                drone_mining_cycle_j: spec.drone_mining_cycle_cost_j,
                drone_attack_j: spec.drone_attack_cost_j,
            },
            emergency_reserve: None,
            unmetered: vec![UnmeteredPowerSystem {
                component_id: "ship.drone_bay.motors".into(),
                label: "Drone bay motors · not metered".into(),
            }],
        });
        c
    }
    #[test]
    fn accounting_requires_exact_viewed_owner_sample_capacitor_and_vessel() {
        let c = fixture();
        let t = c.snapshot.as_ref().unwrap().server_time_ms;
        let p = c.own().unwrap();
        assert!(confirmed(&p, t).is_ok());
        for change in 0..7 {
            let mut changed = p.clone();
            let v = changed.power_accounting.as_mut().unwrap();
            match change {
                0 => v.owner_pilot = "foreign".into(),
                1 => v.schema_version += 1,
                2 => v.sampled_at_ms += 1,
                3 => v.charge_j += 1,
                4 => v.accounted_reserved_w += 1,
                5 => v.instantaneous_debits.warp_j += 1,
                _ => {
                    v.vessel_id = "foreign-hull".into();
                    changed.reactor_configuration =
                        Some(PioneerReactorConfiguration::open("expected-hull".into(), t));
                }
            }
            assert!(confirmed(&changed, t).is_err(), "case{change}");
        }
        let mut old = p;
        old.power_accounting = None;
        assert!(confirmed(&old, t).is_err());
    }
    #[test]
    fn full_capacitor_keeps_rating_and_net_distinct_from_zero_stored_rate() {
        let mut c = fixture();
        let s = c.snapshot.as_mut().unwrap();
        let t = s.server_time_ms;
        let p = &mut s.pilots[0];
        let power = p.power.as_mut().unwrap();
        power.charge_j = power.capacity_j;
        let v = p.power_accounting.as_mut().unwrap();
        v.charge_j = v.capacity_j;
        v.stored_charge_rate_w = 0;
        let v = confirmed(p, t).unwrap();
        assert_eq!(v.reactor_rating_w, 1200);
        assert_eq!(v.net_available_charge_w, 1176);
        assert_eq!(v.stored_charge_rate_w, 0);
        assert_eq!(v.unmetered.len(), 1);
    }
    #[test]
    fn electrical_details_expand_with_real_clicks_without_gameplay_or_fabricated_loads() {
        for width in [900.0, 1280.0] {
            let ctx = egui::Context::default();
            let mut c = fixture();
            let (tx, rx) = std::sync::mpsc::channel();
            c.commands = Some(tx);
            let before = serde_json::to_value(&c.snapshot).unwrap();
            let mut tick = 0;
            let render = |c: &Cockpit, tick: &mut u64, events| {
                *tick += 1;
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            Pos2::ZERO,
                            Vec2::new(width, 650.),
                        )),
                        time: Some(*tick as f64 / 10.),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            egui::ScrollArea::vertical().show(ui, |ui| c.ship_power_accounting(ui));
                        });
                    },
                )
            };
            let visible = |out: &egui::FullOutput, prefix: &str| {
                out.shapes.iter().find_map(|shape| {
                    if let egui::Shape::Text(text) = &shape.shape {
                        let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                        (text.galley.text().starts_with(prefix)
                            && shape.clip_rect.contains_rect(rect))
                        .then_some(rect.center())
                    } else {
                        None
                    }
                })
            };
            let _ = render(&c, &mut tick, vec![]);
            let out = render(&c, &mut tick, vec![]);
            let point = visible(&out, "Electrical accounting").unwrap();
            for pressed in [true, false] {
                let _ = render(
                    &c,
                    &mut tick,
                    vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                );
            }
            let _ = render(&c, &mut tick, vec![]);
            let out = render(&c, &mut tick, vec![]);
            for text in [
                "Reactor rating: 1200 W",
                "Main-bus reservations: 24 W",
                "Net available charging: 1176 W",
                "Model capacitor storage: 1176 W",
                "Instantaneous debits",
                "Not metered",
            ] {
                assert!(visible(&out, text).is_some(), "missing {text} at{width}");
            }
            let point = visible(&out, "Not metered").unwrap();
            for pressed in [true, false] {
                let _ = render(
                    &c,
                    &mut tick,
                    vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                );
            }
            let out = render(&c, &mut tick, vec![]);
            assert!(visible(&out, "Drone bay motors · not metered").is_some());
            c.frozen_time = Some(1000);
            c.observed = Instant::now() - Duration::from_secs(3);
            let out = render(&c, &mut tick, vec![]);
            assert!(visible(&out, "Stale · last confirmed").is_some());
            assert_eq!(serde_json::to_value(&c.snapshot).unwrap(), before);
            assert!(rx.try_recv().is_err());
            assert!(!c.request_pending);
        }
    }
    #[test]
    fn remote_accounting_never_borrows_controller_public_or_private_power() {
        let mut c = fixture();
        let owner = c.pilot.clone();
        let mut worker = c.own().unwrap();
        worker.name = "Worker".into();
        worker.power_accounting = None;
        let s = c.snapshot.as_mut().unwrap();
        let time = s.server_time_ms;
        s.pilots.push(worker);
        s.remote_controls.push(RemoteControlLease {
            controller: owner,
            pilot: "Worker".into(),
        });
        c.manual_remote = Some("Worker".into());
        assert_eq!(c.own().unwrap().name, "Worker");
        assert!(confirmed(&c.own().unwrap(), time).is_err());
        let controller = c.snapshot.as_ref().unwrap().pilots[0]
            .power_accounting
            .clone()
            .unwrap();
        c.snapshot.as_mut().unwrap().pilots[1].power_accounting = Some(controller);
        assert!(confirmed(&c.own().unwrap(), time).is_err());
        let a = c.snapshot.as_mut().unwrap().pilots[1]
            .power_accounting
            .as_mut()
            .unwrap();
        a.owner_pilot = "Worker".into();
        a.vessel_id = "actual-worker-test-hull".into();
        assert_eq!(
            confirmed(&c.own().unwrap(), time).unwrap().vessel_id,
            "actual-worker-test-hull"
        );
    }
}
