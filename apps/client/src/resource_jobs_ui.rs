//! The ship computer submits bounded intents; the server owns every job transition.
use super::*;
impl Cockpit {
    fn resource_fresh(&self) -> bool {
        self.connected
            && self
                .snapshot
                .as_ref()
                .is_some_and(|s| self.presentation_time().saturating_sub(s.server_time_ms) <= 1500)
    }
    fn resource_name(&self) -> &str {
        self.resource_pilot.as_deref().unwrap_or(&self.pilot)
    }
    fn resource_authorized(&self) -> bool {
        self.resource_name() == self.pilot
            || self.snapshot.as_ref().is_some_and(|s| {
                s.fleet_links
                    .iter()
                    .any(|l| l.pilot == self.resource_name() && l.controller == self.pilot)
            })
    }
    fn resource_ship(&self) -> Option<PilotView> {
        if !self.resource_authorized() {
            return None;
        }
        self.snapshot
            .as_ref()?
            .pilots
            .iter()
            .find(|p| p.name == self.resource_name())
            .cloned()
    }
    fn own_resource_job(&self) -> Option<&ResourceJob> {
        self.snapshot
            .as_ref()?
            .resource_jobs
            .iter()
            .find(|j| j.pilot == self.resource_name())
    }
    pub(super) fn resource_start_reason(&self) -> Option<&'static str> {
        if self.embedded && self.embedded_paused {
            return Some("Choose Resume simulation to operate the routine. The Play workspace only changes the interface.");
        }
        if self
            .own()
            .is_some_and(|pilot| pilot.station_avatar.is_some())
        {
            return Some(
                "Return to the pilot seat to assign a mining routine. Status remains visible on foot.",
            );
        }
        if self.manual_remote.is_some() {
            return Some("Return to your own ship before assigning AI jobs.");
        }
        if !self.connected {
            return Some("Offline: reconnect before starting a job.");
        }
        if !self.resource_fresh() {
            return Some("World state is stale: waiting for a fresh server snapshot.");
        }
        if self.request_pending {
            return Some("Waiting for server confirmation.");
        }
        let Some(p) = self.resource_ship() else {
            return Some("Worker state or explicit fleet permission unavailable.");
        };
        if self
            .own_resource_job()
            .is_some_and(|j| !j.phase.is_terminal())
        {
            return Some("A job is already running. Cancel it first.");
        }
        if p.station_avatar.is_some() {
            return Some("The selected ship's pilot must return to their seat before assigning a gathering job.");
        }
        if matches!(p.location, Location::Jumping { .. }) {
            return Some("Wait for gate transit to finish before assigning a haul.");
        }
        if self.resource_goal == 0 || self.resource_goal > arkline_protocol::MAX_RESOURCE_JOB_UNITS
        {
            return Some("Total goal must be between 1 and 1,000,000 ore.");
        }
        None
    }
    pub(super) fn invalidate_ai_session(&mut self) {
        self.ship_ai.clear_context();
        self.connection_generation = self.connection_generation.wrapping_add(1);
        self.cancel_ai();
    }
    pub(super) fn cancel_ai(&mut self) {
        self.voice.cancel();
        self.voice_pin = None;
        self.ship_ai.cancel_pending();
        self.ai_pin = None;
    }
    fn submit_ship_instruction(&mut self) {
        if self.voice_pin.take().is_some() {
            self.voice.cancel();
        }
        if !self.can_operate()
            || !self.resource_fresh()
            || self.ai_pin.is_some()
            || self.ai_prompt.trim().is_empty()
        {
            self.ai_message =
                "Instruction not sent: ship is unavailable or awaiting a response.".into();
            return;
        }
        if let Some(pilot) = self.resource_ship() {
            self.ai_pin = Some((pilot.name.clone(), self.connection_generation));
            if let Some(snapshot) = &self.snapshot {
                self.ship_ai.request_with_context(
                    &self.ai_prompt,
                    &pilot,
                    snapshot,
                    self.selected_target(),
                );
            }
        }
    }
    fn speak_reply(&mut self, text: &str) {
        if self.spoken_replies && !self.voice.is_listening() {
            self.voice.speak(text);
        }
    }
    pub(super) fn poll_voice(&mut self) {
        if self.voice_pin.as_ref().is_some_and(|(pilot, generation)| {
            !self.resource_fresh()
                || pilot != self.resource_name()
                || *generation != self.connection_generation
                || !self.resource_authorized()
        }) {
            self.voice.cancel();
            self.voice_pin = None;
            self.voice_transcript =
                "Voice order cancelled: connection or selected ship changed.".into();
        }
        while let Some(event) = self.voice.poll() {
            match event {
                voice::VoiceEvent::Ready => {}
                voice::VoiceEvent::Partial(text) => {
                    if self.voice_pin.is_some() {
                        self.voice_transcript = text;
                    }
                }
                voice::VoiceEvent::Final(text) => {
                    let valid = self.voice_pin.take().is_some_and(|(pilot, generation)| {
                        pilot == self.resource_name()
                            && generation == self.connection_generation
                            && self.resource_fresh()
                            && self.resource_authorized()
                    });
                    if valid && !text.trim().is_empty() {
                        self.voice_transcript = text.clone();
                        self.ai_prompt = text;
                        self.submit_ship_instruction();
                        self.speak_reply("Interpreting your order.");
                    }
                }
                voice::VoiceEvent::Error(error) => {
                    self.voice_pin = None;
                    self.voice_transcript = error;
                }
            }
        }
        if let Some(job) = self.own_resource_job().cloned() {
            let key = (job.id, job.phase);
            if self.spoken_job.as_ref() != Some(&key) {
                self.spoken_job = Some(key);
                self.speak_reply(&format!(
                    "{}. {}. {} ore stored.",
                    job.pilot, job.message, job.stored_units
                ));
            }
        }
    }
    pub(super) fn poll_ship_ai(&mut self) {
        if self.ai_pin.as_ref().is_some_and(|(pilot, generation)| {
            !self.resource_fresh()
                || pilot != self.resource_name()
                || *generation != self.connection_generation
                || !self.resource_authorized()
        }) {
            self.cancel_ai();
            self.ai_message = "Discarded reply: session, worker or permission changed.".into();
        }
        if let Some(reply) = self.ship_ai.poll() {
            let Some((pilot, generation)) = self.ai_pin.take() else {
                return;
            };
            if !self.resource_fresh()
                || pilot != self.resource_name()
                || generation != self.connection_generation
                || !self.resource_authorized()
            {
                return;
            }
            match reply {
                Err(error) => self.ai_message = error,
                Ok(reply) => {
                    self.ai_message = reply.message;
                    match reply.intent {
                        Some(ship_ai::AiIntent::GatherOre { target_units }) => {
                            self.resource_goal = target_units;
                            if let Some(reason) = self.resource_start_reason() {
                                self.ai_message
                                    .push_str(&format!("\nNot started: {reason}"));
                            } else {
                                self.send(Request::StartResourceJob {
                                    target_units,
                                    pilot: self.resource_pilot.clone(),
                                });
                            }
                        }
                        Some(ship_ai::AiIntent::CancelResourceJob) => {
                            if self.can_operate()
                                && self.resource_fresh()
                                && self
                                    .own_resource_job()
                                    .is_some_and(|j| !j.phase.is_terminal())
                            {
                                self.send(Request::CancelResourceJob {
                                    pilot: self.resource_pilot.clone(),
                                });
                            } else {
                                self.ai_message.push_str("\nNo cancellable job available, or server confirmation is pending.");
                            }
                        }
                        Some(ship_ai::AiIntent::OrbitDrone { id, target }) => {
                            self.send_ai_drone(Request::OrbitDrone { id, target })
                        }
                        Some(ship_ai::AiIntent::HoldDrone { id }) => {
                            self.send_ai_drone(Request::DroneHoldById { id })
                        }
                        Some(ship_ai::AiIntent::ReportStatus) | None => {}
                    }
                }
            }
            self.speak_reply(&self.ai_message.clone());
        }
    }
    fn send_ai_drone(&mut self, request: Request) {
        let same_target = self.controlled_name() == self.resource_name();
        let permitted = self.resource_name() == self.pilot
            || self.controlled_remote() == Some(self.resource_name());
        if self.can_operate() && self.resource_fresh() && same_target && permitted {
            self.send(request);
        } else {
            self.ai_message.push_str("\nNot sent: take manual control of the selected worker first (or return to your own ship for an own-ship order), and wait for server confirmation.");
        }
    }
    fn resource_authority_summary(&mut self, ui: &mut egui::Ui) {
        let ship = self.resource_ship();
        let job = self.own_resource_job().cloned();
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("ROUTINE STATUS");
                if let Some(job) = &job {
                    ui.colored_label(
                        if job.phase == ResourceJobPhase::Blocked {
                            AMBER
                        } else {
                            self.accent()
                        },
                        format!("#{} · {}", job.id, resource_phase_label(job.phase)),
                    );
                } else {
                    ui.colored_label(MUTED, "No routine assigned");
                }
            });
            if let Some(job) = &job {
                ui.add(
                    egui::Label::new(format!(
                        "{}: {}",
                        if job.phase == ResourceJobPhase::Blocked {
                            "Blocked"
                        } else {
                            "Authority stage"
                        },
                        job.message
                    ))
                    .wrap(),
                );
            }
            if !self.connected {
                ui.colored_label(AMBER, "Disconnected · last received state");
            } else if self.embedded && self.embedded_paused {
                ui.colored_label(MUTED, "Paused · authority clock is not advancing");
            } else if !self.resource_fresh() {
                ui.colored_label(AMBER, "Stale snapshot · waiting for authority");
            }
            egui::Grid::new("resource-authority-summary")
                .num_columns(2)
                .min_col_width(0.0)
                .max_col_width(((ui.available_width() - 18.0) / 2.0).max(1.0))
                .spacing(egui::vec2(18.0, 5.0))
                .show(ui, |ui| {
                    if let Some(job) = &job {
                        for (label, value) in [
                            ("Assigned goal", format!("{} ore", job.target_units)),
                            ("Delivered", format!("{} ore", job.stored_units)),
                        ] {
                            ui.colored_label(MUTED, label);
                            ui.strong(value);
                            ui.end_row();
                        }
                    }
                    if let Some(ship) = &ship {
                        ui.colored_label(MUTED, "Current haul / hold");
                        ui.strong(format!(
                            "{} / {} ore",
                            ship.ore,
                            cargo_capacity(ship.ship_class)
                        ));
                        ui.end_row();
                        ui.colored_label(MUTED, "Drones in space");
                        ui.strong(Self::fleet(ship).len().to_string());
                        ui.end_row();
                    } else {
                        ui.colored_label(MUTED, "Ship telemetry");
                        ui.label("Unavailable · selected ship state or permission missing");
                        ui.end_row();
                    }
                    if let Some(job) = &job {
                        ui.colored_label(
                            MUTED,
                            if job.phase.is_terminal() {
                                "Last assigned target"
                            } else {
                                "Intended target"
                            },
                        );
                        ui.label(job.target_id.map_or_else(
                            || "Awaiting assignment".into(),
                            |id| format!("Asteroid #{id}"),
                        ));
                        ui.end_row();
                    }
                });
            if let Some(job) = &job {
                ui.add_space(4.0);
                ui.add(
                    egui::ProgressBar::new(
                        (job.stored_units as f64 / job.target_units.max(1) as f64).clamp(0.0, 1.0)
                            as f32,
                    )
                    .text(format!(
                        "{} / {} ore delivered",
                        job.stored_units, job.target_units
                    )),
                );
                if !job.phase.is_terminal() {
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled(
                                self.can_operate()
                                    && self.resource_fresh()
                                    && self.resource_authorized(),
                                egui::Button::new("Cancel routine"),
                            )
                            .on_hover_text("Cancels automation; delivered ore remains in storage.")
                            .clicked()
                        {
                            self.cancel_ai();
                            self.send(Request::CancelResourceJob {
                                pilot: self.resource_pilot.clone(),
                            });
                        }
                    });
                }
            } else {
                ui.label("No routine reported. Set a goal below to request one.");
            }
            if self.request_pending {
                ui.colored_label(AMBER, "Awaiting authority · command not yet acknowledged");
            }
        });
    }

    fn resource_telemetry_details(&self, ui: &mut egui::Ui) {
        ui.collapsing("Telemetry details", |ui| {
            egui::Grid::new("resource-secondary-telemetry")
                .num_columns(2)
                .min_col_width(0.0)
                .max_col_width(((ui.available_width() - 18.0) / 2.0).max(1.0))
                .spacing(egui::vec2(18.0, 6.0))
                .show(ui, |ui| {
                    if let Some(job) = self.own_resource_job() {
                        ui.colored_label(MUTED, "Mined by this routine");
                        ui.label(format!("{} ore", job.mined_units));
                        ui.end_row();
                        ui.colored_label(MUTED, "Authority stage report");
                        ui.label(&job.message);
                        ui.end_row();
                    }
                    if let Some(ship) = self.resource_ship() {
                        ui.colored_label(MUTED, "Drone bay inventory");
                        ui.label("Not reported by authority");
                        ui.end_row();
                        ui.colored_label(MUTED, "Ship capacitor");
                        ui.label(ship.power.as_ref().map_or_else(
                            || "Not reported by authority".into(),
                            |power| format!("{} / {} J", power.charge_j, power.capacity_j),
                        ));
                        ui.end_row();
                        ui.colored_label(MUTED, "Location");
                        ui.label(format!(
                            "{} · {}",
                            system_name(ship.system_id),
                            location_label(&ship.location)
                        ));
                        ui.end_row();
                    }
                });
            if !self.notice.is_empty() {
                ui.small(format!("Last authority response: {}", self.notice));
            }
        });
    }
    pub(super) fn resource_job_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mining operations");
        let old = self.resource_pilot.clone();
        let workers: Vec<String> = self
            .snapshot
            .as_ref()
            .map(|s| {
                s.fleet_links
                    .iter()
                    .filter(|l| l.controller == self.pilot)
                    .map(|l| l.pilot.clone())
                    .collect()
            })
            .unwrap_or_default();
        egui::ComboBox::from_id_salt("resource worker")
            .selected_text(format!("Ship: {}", self.resource_name()))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.resource_pilot, None, "My ship");
                for worker in workers {
                    ui.selectable_value(&mut self.resource_pilot, Some(worker.clone()), worker);
                }
            });
        if old != self.resource_pilot {
            self.ship_ai.clear_context();
            self.cancel_ai();
            self.ai_message.clear();
            self.voice_transcript.clear();
            self.spoken_job = None;
        }
        self.resource_authority_summary(ui);
        ui.add_space(8.0);
        ui.strong("ASSIGNMENT");
        let capacity = self.resource_ship().map(|p| cargo_capacity(p.ship_class));
        ui.horizontal_wrapped(|ui| {
            let active = self
                .own_resource_job()
                .is_some_and(|job| !job.phase.is_terminal());
            ui.label(if active {
                "Next goal"
            } else {
                "Total ore goal"
            });
            ui.add(
                egui::DragValue::new(&mut self.resource_goal)
                    .range(1..=arkline_protocol::MAX_RESOURCE_JOB_UNITS)
                    .suffix(" units"),
            );
            if ui
                .add_enabled(
                    self.can_operate() && self.resource_start_reason().is_none(),
                    egui::Button::new("Start mining routine"),
                )
                .clicked()
            {
                self.cancel_ai();
                self.send(Request::StartResourceJob {
                    target_units: self.resource_goal,
                    pilot: self.resource_pilot.clone(),
                });
            }
        });
        if let Some(reason) = self.resource_start_reason() {
            ui.colored_label(MUTED, reason);
        }
        self.resource_telemetry_details(ui);
        ui.collapsing("Routine details", |ui| {
            ui.label("The ship selects an available local asteroid and returns to its starting-system station to deposit into personal storage.");
        if self
            .own_resource_job()
            .is_some_and(|job| !job.phase.is_terminal())
        {
            ui.small(
                "Next goal applies only to a new routine. The assigned goal above stays unchanged.",
            );
        }
        if let Some(capacity) = capacity {
            ui.small(format!(
                "{} ore per hold · approximately {} trips for the proposed goal",
                capacity,
                self.resource_goal.div_ceil(capacity.max(1))
            ));
        } else {
            ui.small("Hold capacity unavailable until the selected ship is reported.");
        }
        ui.small(
            "Goal includes ore aboard. Warp finishes and drones return before the haul proceeds.",
        );
        });
        ui.collapsing("Fleet & remote controls", |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        self.connected && self.resource_authorized(),
                        egui::Button::new("Watch ship"),
                    )
                    .clicked()
                {
                    self.remote_ship = Some(self.resource_name().to_owned());
                    self.view_mode = ViewMode::Cockpit;
                }
                if ui
                    .add_enabled(
                        self.can_operate()
                            && self.resource_fresh()
                            && self.resource_authorized()
                            && self.resource_name() != self.pilot,
                        egui::Button::new("Take manual control"),
                    )
                    .clicked()
                {
                    self.take_remote_control(self.resource_name().to_owned());
                }
                if (self.remote_ship.is_some() || self.manual_remote.is_some())
                    && ui.button("Return to my ship").clicked()
                {
                    self.return_to_my_ship();
                }
                if self.remote_ship.is_some() && ui.button("Close remote camera").clicked() {
                    self.remote_ship = None;
                }
                ui.small("Watch = camera only. Take manual control stops that ship's AI job.");
            });
        });
        ui.separator();
        ui.collapsing("Ship computer · text & voice orders", |ui| {
        ui.label("Ask the ship computer");
        ui.small(self.ship_ai.endpoint_label());
        ui.small(self.ship_ai.status());
        ui.add(
            egui::TextEdit::multiline(&mut self.ai_prompt)
                .hint_text("Gather 100 ore and bring it back")
                .desired_rows(2),
        );
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.can_operate()
                        && self.resource_fresh()
                        && self.resource_ship().is_some()
                        && !self.ai_prompt.trim().is_empty()
                        && self.ai_pin.is_none(),
                    egui::Button::new("Submit instruction"),
                )
                .clicked()
            {
                self.submit_ship_instruction();
            }
            if self.ai_pin.is_some() && ui.button("Cancel request").clicked() {
                self.cancel_ai();
                self.ai_message = "Request cancelled.".into();
            }
        });
        if !self.ai_message.is_empty() {
            ui.label(&self.ai_message);
        }
        ui.collapsing("Conversation", |ui| {
            for (question, answer) in self.ship_ai.conversation() {
                ui.label(format!("You: {question}"));
                ui.small(format!("Computer: {answer}"));
                ui.separator();
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    voice::Voice::supported()
                        && self.can_operate()
                        && self.resource_fresh()
                        && self.resource_ship().is_some()
                        && self.ai_pin.is_none()
                        && !self.voice.is_listening(),
                    egui::Button::new("Talk to ship"),
                )
                .clicked()
            {
                self.voice_pin =
                    Some((self.resource_name().to_owned(), self.connection_generation));
                self.voice_transcript.clear();
                self.spoken_replies = true;
                self.voice.start_listening();
            }
            if self.voice.is_listening() {
                if ui.button("Finish and send").clicked() {
                    self.voice.finish_listening();
                }
                if ui.button("Cancel recording").clicked() {
                    self.voice.cancel();
                    self.voice_pin = None;
                }
            }
            if ui
                .checkbox(&mut self.spoken_replies, "Spoken replies")
                .changed()
                && !self.spoken_replies
            {
                self.voice.stop_speaking();
            }
            if self.voice.is_speaking() && ui.button("Stop speech").clicked() {
                self.voice.stop_speaking();
            }
        });
        ui.small(self.voice.status());
        if !self.voice_transcript.is_empty() {
            ui.label(format!("Heard: {}", self.voice_transcript));
        }
        ui.small("Talk records up to 20 seconds. Final speech submits as an order. Cancel discards it. Uses the Mac's selected audio input/output; local speech recognition requires an installed language model.");
        ui.small("Supported instructions run directly after validation.");
        ui.collapsing("Fleet permission · this pilot",|ui| {
            let controller = self.snapshot.as_ref().and_then(|s|s.fleet_links.iter().find(|l|l.pilot==self.pilot)).map(|l|l.controller.clone());
            ui.label(format!("Controller: {}",controller.as_deref().unwrap_or("none")));
            ui.add(egui::TextEdit::singleline(&mut self.fleet_controller).hint_text("Existing controller pilot"));
            let name = self.fleet_controller.trim().to_owned();
            let valid = name != self.pilot && self.snapshot.as_ref().is_some_and(|s|s.pilots.iter().any(|p|p.name==name));
            if ui.add_enabled(self.can_operate() && self.resource_fresh() && valid,egui::Button::new("Grant fleet control")).clicked() { self.send(Request::SetFleetController{controller:Some(name)}); }
            if ui.add_enabled(self.can_operate() && self.resource_fresh() && controller.is_some(),egui::Button::new("Revoke control")).clicked() { self.send(Request::SetFleetController{controller:None}); }
            ui.small("Open a second cockpit as the worker and grant your controller pilot permission here. No ships or pilots are created by the computer.");
        });
        ui.small("Manual commands override jobs. A delegated worker may log out while its controller stays connected; controller logout, worker reconnect, revocation or server restart cancels the job.");
        });
    }
}

fn resource_phase_label(phase: ResourceJobPhase) -> &'static str {
    match phase {
        ResourceJobPhase::Undocking => "Undocking",
        ResourceJobPhase::TravelToBelt => "Travelling to belt",
        ResourceJobPhase::AcquiringTarget => "Acquiring target",
        ResourceJobPhase::Mining => "Mining",
        ResourceJobPhase::ReturningToStation => "Returning to station",
        ResourceJobPhase::Docking => "Docking",
        ResourceJobPhase::Depositing => "Depositing",
        ResourceJobPhase::Completed => "Completed",
        ResourceJobPhase::Blocked => "Blocked",
        ResourceJobPhase::Cancelled => "Cancelled",
    }
}
