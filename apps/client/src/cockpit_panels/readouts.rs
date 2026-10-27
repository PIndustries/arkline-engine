//! Read-only operation, mining, power, and compact flight readouts.

use super::*;

impl Cockpit {
    pub(crate) fn operation_status(&self) -> (String, String, Option<f32>, Color32) {
        if !self.connected {
            return (
                if self.connecting {
                    "CONNECTING"
                } else {
                    "OFFLINE"
                }
                .into(),
                if self.notice.is_empty() {
                    "Waiting for a live server connection".into()
                } else {
                    self.notice.clone()
                },
                None,
                AMBER,
            );
        }
        if self.request_pending {
            return (
                "COMMAND PENDING".into(),
                "Awaiting server confirmation".into(),
                None,
                AMBER,
            );
        }
        let Some(pilot) = self.own() else {
            return (
                "SYNCHRONIZING".into(),
                "Waiting for vessel snapshot".into(),
                None,
                MUTED,
            );
        };
        if let Some(docking) = pilot
            .flight
            .as_ref()
            .and_then(|f| f.docking.as_ref())
            .filter(|d| d.phase.busy())
        {
            return (
                docking.phase.label().into(),
                docking.hold_reason.clone().unwrap_or_else(|| {
                    format!(
                        "Berth {} · operation {} · ship motion owned by station",
                        docking.berth_id + 1,
                        docking.operation_id
                    )
                }),
                docking
                    .segment
                    .as_ref()
                    .map(|s| s.progress(docking.updated_at_ms) as f32),
                self.accent(),
            );
        }
        if let Some(state) = pilot
            .station_avatar
            .as_ref()
            .and_then(|avatar| avatar.docked_traversal.as_ref())
        {
            return (
                format!("ON FOOT / {}", station_ui::docked_state_label(state)),
                if pilot.location == Location::Docked {
                    station_ui::docked_exit_guidance(state).into()
                } else {
                    "Autopilot continues · return to the cockpit chair to pilot · exterior access locked".into()
                },
                None,
                self.accent(),
            );
        }
        if let Some(avatar) = &pilot.station_avatar {
            let (title, guidance) = if avatar.onboard_interior {
                (
                    format!("ON FOOT / PIONEER DECK {}", avatar.interior_deck),
                    if avatar.interior_deck == 1 {
                        "Return to the airlock to leave; access-core lift leads to deck 2".into()
                    } else {
                        "Use the access-core lift to deck 1 for the airlock exit".into()
                    },
                )
            } else if self.in_captains_quarters() {
                (
                    "ON FOOT / CAPTAIN'S QUARTERS".into(),
                    "Return through the doorway to the hangar boarding marker".into(),
                )
            } else {
                (
                    "ON FOOT / STATION HANGAR".into(),
                    "Walk to the boarding marker to enter your ship".into(),
                )
            };
            return (title, guidance, None, self.accent());
        }
        let now = self.presentation_time();
        if let Location::InTransit {
            destination,
            arrives_at_ms,
        } = pilot.location
        {
            let destination = match destination {
                Destination::Belt => "asteroid belt".to_string(),
                Destination::Station => "station".to_string(),
                Destination::Gate { id } => gates()
                    .into_iter()
                    .find(|g| g.id == id)
                    .map(|g| g.name)
                    .unwrap_or_else(|| "stargate".into()),
            };
            if let Some((phase, progress)) = motion::warp_phase(&pilot, now) {
                return (
                    phase.into(),
                    format!(
                        "To {destination} · {} remaining · {:.1}s",
                        pilot
                            .flight
                            .as_ref()
                            .and_then(|f| f.warp.as_ref())
                            .map_or_else(
                                || "—".into(),
                                |path| format_distance(distance_m(
                                    motion::position(&pilot, now),
                                    path.target
                                ))
                            ),
                        arrives_at_ms.saturating_sub(now) as f32 / 1000.0
                    ),
                    Some(progress),
                    self.accent(),
                );
            }
            return (
                "ARRIVAL CHECK".into(),
                "Awaiting server-confirmed arrival".into(),
                Some(1.0),
                self.accent(),
            );
        }
        if let Location::Jumping {
            gate_id,
            arrives_at_ms,
        } = pilot.location
        {
            let remaining = arrives_at_ms.saturating_sub(now);
            let route = gates()
                .into_iter()
                .find(|g| g.id == gate_id)
                .map(|g| system_name(g.destination_system_id))
                .unwrap_or_else(|| "destination area".into());
            return (
                "STARGATE TRANSIT".into(),
                format!("To {route} · {:.1}s", remaining as f32 / 1000.0),
                Some(1.0 - remaining.min(JUMP_DURATION_MS) as f32 / JUMP_DURATION_MS as f32),
                self.accent(),
            );
        }
        if let Some(reason) = self.bay_maneuver_reason() {
            return ("DRONE BAY INTERLOCK".into(), reason.into(), None, AMBER);
        }
        if let Some(module) = pilot
            .mining_modules
            .iter()
            .filter(|m| m.target_id.is_some())
            .min_by_key(|m| m.cycle_completes_at_ms)
        {
            let count = pilot
                .mining_modules
                .iter()
                .filter(|m| m.target_id.is_some())
                .count();
            let remaining = module
                .cycle_completes_at_ms
                .unwrap_or(now)
                .saturating_sub(now);
            return (
                "MINING MODULES ACTIVE".into(),
                format!(
                    "{count} active turret(s) · next cycle {:.1}s · ore credited on completion",
                    remaining as f64 / 1000.0
                ),
                None,
                self.accent(),
            );
        }
        if self.mining_effect_target().is_some() {
            if let Some((progress, units)) = self.effects.mining(now) {
                return (
                    "MINING LASER ACTIVE".into(),
                    format!("+{units} ore confirmed; cargo already credited"),
                    Some(progress),
                    self.accent(),
                );
            }
        }
        if let Some(event) = self.effects.events.iter().rev().find(|event| {
            matches!(
                (event.kind, &pilot.location),
                (effects::EffectKind::Docking, Location::Docked)
                    | (effects::EffectKind::Undocking, Location::Station)
            )
        }) {
            return match event.kind {
                effects::EffectKind::Docking => (
                    "DOCKING CONFIRMED".into(),
                    "Station services available".into(),
                    Some(1.0),
                    self.accent(),
                ),
                _ => (
                    "UNDOCKED".into(),
                    "Navigation available".into(),
                    Some(1.0),
                    self.accent(),
                ),
            };
        }
        if pilot.location != Location::Docked {
            if let Some(flight) = &pilot.flight {
                let label = match flight.mode {
                    NavigationMode::Manual => "DIRECT FLIGHT / ASSISTED BRAKING",
                    NavigationMode::Stop => "STOP / BRAKING",
                    NavigationMode::Approach { .. } => "APPROACHING TARGET",
                    NavigationMode::Orbit { .. } => "ORBITING TARGET",
                    NavigationMode::KeepRange { .. } => "KEEPING RANGE",
                    NavigationMode::FlyTo { .. } => "FLYING TO POINT",
                };
                return (
                    label.into(),
                    format!(
                        "{:.0} m/s · server-authoritative flight",
                        distance_m(flight.velocity, [0.0; 3])
                    ),
                    Some(
                        (distance_m(flight.velocity, [0.0; 3]) / MAX_SHIP_SPEED).clamp(0.0, 1.0)
                            as f32,
                    ),
                    self.accent(),
                );
            }
        }
        match pilot.location {
            Location::Docked => (
                "DOCKED / SECURE".into(),
                "Station exchange ready".into(),
                None,
                self.accent(),
            ),
            Location::Station => (
                "FLIGHT READY".into(),
                "Select the asteroid belt to begin a route".into(),
                None,
                self.accent(),
            ),
            Location::Belt => (
                "EXTRACTION READY".into(),
                "Lock an asteroid, then activate a mining turret".into(),
                None,
                self.accent(),
            ),
            Location::InTransit { .. } | Location::Jumping { .. } => unreachable!(),
        }
    }
    pub(crate) fn mining_status(&self) -> (String, Option<f32>) {
        if let Some(v) = &self.physical_cargo {
            return (
                format!(
                    "Physical target: {} · {}",
                    crate::physical_cargo_ui::litres(v.state.projection.resource_remaining_cm3.0),
                    if v.state.projection.mining {
                        "mining"
                    } else if v.state.projection.target_locked {
                        "locked"
                    } else {
                        "awaiting lock"
                    }
                ),
                None,
            );
        }
        let Some(pilot) = self.own() else {
            return ("Select an asteroid and lock".into(), None);
        };
        if !self.connected {
            return ("Offline · last known state".into(), None);
        }
        if let Some(id) = self.module_target(&pilot) {
            let name = self.asteroid_name(id);
            if let Some(asteroid) = self
                .snapshot
                .as_ref()
                .and_then(|s| s.asteroids.iter().find(|a| a.id == id))
            {
                if asteroid.ore == 0 {
                    return (format!("Depleted · {name}"), None);
                }
                if !effects::can_extract_target(&pilot, asteroid) {
                    return (format!("Out of extraction range · {name}"), None);
                }
            }
            return (format!("Locked · {name}"), None);
        }
        if let Some(lock) = pilot.target_locks.first().or(pilot.target_lock.as_ref()) {
            let now = self.presentation_time();
            let remaining = lock.completes_at_ms.saturating_sub(now);
            return (
                format!(
                    "Acquiring {} · {}",
                    self.asteroid_name(lock.asteroid_id),
                    if remaining == 0 {
                        "confirming…".into()
                    } else {
                        format!("{:.1}s", remaining as f64 / 1000.0)
                    }
                ),
                Some(
                    (now.saturating_sub(lock.started_at_ms) as f32
                        / lock
                            .completes_at_ms
                            .saturating_sub(lock.started_at_ms)
                            .max(1) as f32)
                        .clamp(0.0, 1.0),
                ),
            );
        }
        ("Select an asteroid and lock".into(), None)
    }
    pub(crate) fn power_gauge(&self, ui: &mut egui::Ui) {
        let Some(pilot) = self.own() else { return };
        if let Some(power) = &pilot.power {
            let fraction = if power.capacity_j == 0 {
                0.0
            } else {
                (power.charge_j as f64 / power.capacity_j as f64).clamp(0.0, 1.0) as f32
            };
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_height(18.0)
                    .fill(self.accent())
                    .text(format!(
                        "Capacitor {:.1}/{:.1} kJ · regen rating {} W{}",
                        power.charge_j as f64 / 1000.0,
                        power.capacity_j as f64 / 1000.0,
                        power.regen_w,
                        if !self.connected { " · last known" } else if self.embedded_paused { " · paused" } else if !self.live_command_snapshot_fresh() { " · stale" } else { "" }
                    )),
            )
            .on_hover_text({let spec=power_spec(pilot.ship_class);format!("Server-confirmed energy. Warp {} J; turret cycle {} J; mining drone cycle {} J; drone weapon shot {} J.",spec.warp_cost_j,spec.mining_cycle_cost_j,spec.drone_mining_cycle_cost_j,spec.drone_attack_cost_j)});
        } else {
            ui.small("Capacitor data unavailable");
        }
    }
    /// Read-only content; physical placement and input authority remain external.
    pub(crate) fn compact_flight_summary(&self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("FLIGHT CONSOLE")
                .size(20.0)
                .color(MUTED),
        );
        let Some(pilot) = self.own() else {
            ui.label("Waiting for vessel telemetry");
            return;
        };
        let mode = pilot
            .flight
            .as_ref()
            .and_then(|f| f.docking.as_ref())
            .filter(|d| d.phase.busy())
            .map(|d| d.phase.label())
            .unwrap_or_else(|| match pilot.location {
                Location::Docked => "Docked",
                Location::InTransit { .. } => "Warp transit",
                Location::Jumping { .. } => "Gate transit",
                _ => pilot
                    .flight
                    .as_ref()
                    .map_or("Unavailable", |flight| match flight.mode {
                        NavigationMode::Stop => "Braking / holding",
                        NavigationMode::Manual => "Manual flight",
                        NavigationMode::Approach { .. } => "Approaching",
                        NavigationMode::Orbit { .. } => "Orbiting",
                        NavigationMode::KeepRange { .. } => "Keeping range",
                        NavigationMode::FlyTo { .. } => "Flying to waypoint",
                    }),
            });
        let speed: String = pilot.flight.as_ref().map_or_else(
            || "Unavailable".into(),
            |flight| {
                let velocity = flight.warp.as_ref().map_or(flight.velocity, |warp| {
                    sample_warp(warp, self.presentation_time()).1
                });
                let speed = distance_m(velocity, [0.0; 3]);
                if speed.is_finite() {
                    format_speed(speed)
                } else {
                    "Unavailable".into()
                }
            },
        );
        let job = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .resource_jobs
                .iter()
                .filter(|job| job.pilot == pilot.name)
                .max_by_key(|job| (!job.phase.is_terminal(), job.updated_at_ms, job.id))
        });
        let mining = job.map_or_else(
            || "No routine".into(),
            |job| resource_jobs_ui::resource_phase_label(job.phase).to_owned(),
        );
        let delivery = job.map_or_else(
            || "No target".into(),
            |job| format!("{} / {} ore", job.stored_units, job.target_units),
        );
        // Two compact readout groups leave the live control bank and view
        // unobstructed. The expanded pages retain detailed management.
        ui.columns(2, |columns| {
            egui::Frame::new()
                .fill(Color32::from_rgb(7, 17, 23))
                .inner_margin(egui::Margin::same(6))
                .show(&mut columns[0], |ui| {
                    ui.label(egui::RichText::new("SPEED").size(22.0).color(MUTED));
                    let width = ui
                        .painter()
                        .layout_no_wrap(speed.clone(), egui::FontId::monospace(44.0), self.accent())
                        .size()
                        .x;
                    let font = 44.0 * ((ui.available_width() - 2.0) / width.max(1.0)).min(1.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(speed)
                                .monospace()
                                .size(font)
                                .color(self.accent()),
                        )
                        .wrap_mode(egui::TextWrapMode::Extend),
                    );
                    ui.label(egui::RichText::new(mode).size(20.0));
                });
            egui::Frame::new()
                .fill(Color32::from_rgb(7, 17, 23))
                .inner_margin(egui::Margin::same(6))
                .show(&mut columns[1], |ui| {
                    ui.label(egui::RichText::new("Routine").size(22.0).color(MUTED));
                    let width = ui
                        .painter()
                        .layout_no_wrap(
                            mining.clone(),
                            egui::FontId::proportional(28.0),
                            self.accent(),
                        )
                        .size()
                        .x;
                    let font = 28.0 * ((ui.available_width() - 2.0) / width.max(1.0)).min(1.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(mining).size(font).color(self.accent()),
                        )
                        .wrap_mode(egui::TextWrapMode::Extend),
                    );
                    ui.label(egui::RichText::new("Delivered").size(22.0).color(MUTED));
                    let width = ui
                        .painter()
                        .layout_no_wrap(
                            delivery.clone(),
                            egui::FontId::monospace(28.0),
                            self.accent(),
                        )
                        .size()
                        .x;
                    let font = 28.0 * ((ui.available_width() - 2.0) / width.max(1.0)).min(1.0);
                    ui.add(
                        egui::Label::new(egui::RichText::new(delivery).monospace().size(font))
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                });
        });
        if let Some(v) = &self.physical_cargo {
            ui.label(
                egui::RichText::new(format!(
                    "Hold {} / {} · physical volume",
                    crate::physical_cargo_ui::litres(v.ship_payload_cm3.0),
                    crate::physical_cargo_ui::litres(v.ship_capacity_cm3.0)
                ))
                .size(22.0),
            );
            return;
        }
        ui.label(egui::RichText::new(format!(
            "Hold {} / {} ore · {} drones",
            pilot.ore,
            cargo_capacity(pilot.ship_class),
            Self::fleet(&pilot).len()
        )).size(22.0))
        .on_hover_text("Current ship ore and reported hold capacity; drones counts deployed craft, not craft in the bay. Power and regeneration ratings are available in the expanded console.");
    }
}
