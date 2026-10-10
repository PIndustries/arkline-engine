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
        if p.location != Location::Docked {
            return Some("Dock the selected ship before starting.");
        }
        if p.ore != 0 {
            return Some("Deposit the selected ship's cargo first.");
        }
        if !Self::fleet(&p).is_empty() {
            return Some("Recall the selected ship's drones first.");
        }
        if self.resource_goal == 0 || self.resource_goal > cargo_capacity(p.ship_class) {
            return Some("Goal must fit the selected ship's cargo capacity.");
        }
        None
    }
    pub(super) fn invalidate_ai_session(&mut self) {
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
            self.ship_ai.request(
                &self.ai_prompt,
                &pilot,
                self.snapshot.as_ref().map_or(0, |s| s.server_time_ms),
            );
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
                        Some(ship_ai::AiIntent::ReportStatus) | None => {}
                    }
                }
            }
            self.speak_reply(&self.ai_message.clone());
        }
    }
    pub(super) fn resource_job_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ship computer");
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
            self.cancel_ai();
            self.ai_message.clear();
            self.voice_transcript.clear();
            self.spoken_job = None;
        }
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
            if self.remote_ship.is_some() && ui.button("Restore drone feed").clicked() {
                self.remote_ship = None;
            }
        });
        ui.small("Undock → belt → two turrets + mining drones → return → station storage.");
        let capacity = self
            .resource_ship()
            .map_or(1, |p| cargo_capacity(p.ship_class));
        ui.horizontal_wrapped(|ui| {
            ui.label("Ore goal");
            ui.add(
                egui::DragValue::new(&mut self.resource_goal)
                    .range(1..=capacity)
                    .suffix(" units"),
            );
            if ui
                .add_enabled(
                    self.can_operate() && self.resource_start_reason().is_none(),
                    egui::Button::new("Start gathering"),
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
        ui.small("Goal is a minimum; fixed cycles can overshoot. Only confirmed ore counts.");
        if let Some(job) = self.own_resource_job().cloned() {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(
                    if job.phase == ResourceJobPhase::Blocked {
                        AMBER
                    } else {
                        self.accent()
                    },
                    format!("Job #{} · {:?}", job.id, job.phase),
                );
                if ui
                    .add_enabled(
                        self.can_operate()
                            && self.resource_authorized()
                            && !job.phase.is_terminal(),
                        egui::Button::new("Cancel job"),
                    )
                    .clicked()
                {
                    self.cancel_ai();
                    self.send(Request::CancelResourceJob {
                        pilot: self.resource_pilot.clone(),
                    });
                }
            });
            if !self.connected {
                ui.colored_label(AMBER, "Last known job state · disconnected");
            }
            ui.label(&job.message);
            ui.small(format!(
                "Goal {} · confirmed mined {} · stored {}",
                job.target_units, job.mined_units, job.stored_units
            ));
            ui.add(
                egui::ProgressBar::new(
                    (job.stored_units as f64 / job.target_units.max(1) as f64).clamp(0., 1.) as f32,
                )
                .text("Confirmed storage"),
            );
        } else {
            ui.small("No job reported for this ship.");
        }
        ui.separator();
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
            if ui.add_enabled(self.can_operate() && valid,egui::Button::new("Grant fleet control")).clicked() { self.send(Request::SetFleetController{controller:Some(name)}); }
            if ui.add_enabled(self.can_operate() && controller.is_some(),egui::Button::new("Revoke control")).clicked() { self.send(Request::SetFleetController{controller:None}); }
            ui.small("Open a second cockpit as the worker and grant your controller pilot permission here. No ships or pilots are created by the computer.");
        });
        ui.small("Manual commands override jobs. A delegated worker may log out while its controller stays connected; controller logout, worker reconnect, revocation or server restart cancels the job.");
    }
}
