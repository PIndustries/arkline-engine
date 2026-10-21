//! Context and ownership over the bindings used by the real movement samplers.
use super::*;
use crate::control_bindings as bindings;

impl Cockpit {
    pub(super) fn open_controls_reference(&mut self) {
        self.controls_reference_open = true;
        self.release_workspace_input();
    }
    pub(super) fn controls_context(&self) -> &'static str {
        if self.ship_observation {
            "Observation camera"
        } else if self.manual_remote.is_some() {
            "Remote vessel"
        } else if self.own().is_some_and(|p| p.station_avatar.is_some()) {
            "On foot"
        } else if self.own().is_some() {
            "Pilot seat"
        } else {
            "Waiting for vessel"
        }
    }
    pub(super) fn controls_reference(&mut self, ctx: &egui::Context) {
        if !self.controls_reference_open {
            return;
        }
        self.release_workspace_input();
        let mut open = true;
        let response = egui::Window::new("Controls & ownership")
            .id(egui::Id::new("controls_ownership_reference"))
            .order(egui::Order::Foreground)
            .open(&mut open).collapsible(false)
            .default_width(480.0).max_width((ctx.screen_rect().width()-24.0).max(200.0))
            .max_height((ctx.available_rect().height()-24.0).max(160.0))
            .constrain_to(ctx.screen_rect()).vscroll(true)
            .show(ctx, |ui| {
                ui.heading(self.controls_context());
                ui.small("Ship and walking input is suspended while this reference is open. Close it and release held keys before returning to world controls.");
                if !self.connected { ui.colored_label(AMBER, "Disconnected · controls unavailable; last known state shown"); }
                else if self.embedded_paused { ui.colored_label(AMBER, "Paused · resume simulation to move or operate equipment"); }
                else if !self.operations_fresh() { ui.colored_label(AMBER, "Stale authority state · wait for a fresh world update"); }
                else if self.request_pending { ui.colored_label(AMBER, "Command pending · wait for confirmation"); }
                let pilot = self.own();
                let job = pilot.as_ref().and_then(|p| self.snapshot.as_ref()?.resource_jobs.iter()
                    .find(|j| j.pilot == p.name && !j.phase.is_terminal()));
                if let Some(job) = job {
                    ui.separator();
                    ui.strong(format!("Mining routine #{} · controller {}", job.id, job.controller));
                    ui.label(&job.message);
                    ui.colored_label(AMBER, "Manual ship commands cancel this mining routine. Walking, viewing telemetry and changing views do not take over.");
                }
                ui.separator();
                if self.ship_observation {
                    ui.label("This camera observes the ship. Your body remains in place; observation does not grant pilot authority.");
                    ui.label("Return to the body view to walk, or physically return to the pilot seat for manual flight.");
                } else if pilot.as_ref().is_some_and(|p| p.station_avatar.is_some()) {
                    ui.label("These keys move your body; manual ship controls remain locked while on foot.");
                    ui.label(self.on_foot_look_hint());
                    if self.first_person_hud_active() {
                        ui.small("Escape, interface interaction or focus loss releases mouse-look. Click clear world space to capture again; instrument and menu clicks keep the cursor.");
                    }
                    ui.label("Use the nearby movement hints for this body. The same flight keys have different meanings while walking.");
                    ui.monospace(format!("{} · press/release the displayed nearby action", bindings::INTERACT.name()));
                    if pilot.as_ref().is_some_and(|p| p.location != Location::Docked) {
                        ui.label("Interior walking follows the ship. Boarding hatches and the exterior lift stay locked in space.");
                    }
                    ui.small("F only operates the exact visible, reachable, unambiguous prompt. Door, seat and lift interlocks still apply; use explicit buttons when no shortcut is offered.");
                } else if let Some(pilot) = pilot {
                    if pilot.location == Location::Docked {
                        ui.label("Docked · flight movement is unavailable. Use the pilot-seat and boarding controls to enter the interior.");
                    } else if matches!(pilot.location, Location::InTransit {..} | Location::Jumping {..}) {
                        ui.label("In transit · manual flight inputs are unavailable during this stage.");
                    }
                    egui::Grid::new("flight_binding_reference").striped(true).show(ui, |ui| {
                        for axis in bindings::FLIGHT_TRANSLATION.into_iter().chain(bindings::FLIGHT_ROTATION) {
                            ui.monospace(axis.keys_label()); ui.label(axis.label); ui.end_row();
                        }
                    });
                    ui.small(format!("Hold {} over the cockpit viewport · steer with the mouse. Escape releases mouse capture.", bindings::look_button_label()));
                    ui.small(format!("Shift suppresses ship rotation; translation keys still command thrust. Shift + {} can therefore take over mining autopilot.", bindings::FLIGHT_TRANSLATION[0].positive.name()));
                    ui.small("Seated head look is a view action; it does not grant or change movement authority. Use the cockpit view's own look hints.");
                    ui.small(format!("{} · press/release the displayed nearby seat action when available", bindings::INTERACT.name()));
                } else { ui.label("Connect and wait for the authoritative vessel snapshot."); }
                ui.separator();
                ui.strong("Interface and input ownership");
                ui.small("Editor free camera has separate view controls; this reference describes player movement.");
                ui.label("Text fields, panel search, physical computer input and editor/session dialogs own their input. World movement is released while those controls are active.");
                ui.small(format!("Management: Cmd/Ctrl+{} focuses panel search. {} leaves text editing first, then returns to Flight. Back to Flight is always available.", bindings::PANEL_SEARCH.name(), bindings::WORKSPACE_BACK.name()));
                if ui.button("Close controls reference").clicked() { self.controls_reference_open = false; }
            });
        if let Some(response) = response {
            ctx.move_to_top(response.response.layer_id);
        }
        self.controls_reference_open &= open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_reference_context_and_open_preserve_authority() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        let before = serde_json::to_value(&cockpit.snapshot).unwrap();
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        assert_eq!(cockpit.controls_context(), "Pilot seat");
        cockpit.ship_observation = true;
        assert_eq!(cockpit.controls_context(), "Observation camera");
        cockpit.open_controls_reference();
        assert!(cockpit.controls_reference_open && cockpit.workspace_input_waiting_release);
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
        assert!(rx.try_recv().is_err());
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            cockpit.controls_reference(ctx)
        });
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn controls_reference_blocks_underlying_management_shortcuts() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.services_open = true;
        cockpit.service_tab = ServiceTab::Cargo;
        cockpit.controls_reference_open = true;
        cockpit.embedded_input = true;
        for (key, modifiers) in [(crate::control_bindings::PANEL_SEARCH, egui::Modifiers::COMMAND),
            (crate::control_bindings::WORKSPACE_BACK, egui::Modifiers::NONE)] {
            let _ = ctx.run(egui::RawInput { focused:true,
                events:vec![egui::Event::Key {key, physical_key:None, pressed:true, repeat:false, modifiers}],
                ..Default::default() }, |ctx| {
                    cockpit.operations_keyboard_navigation(ctx);
                    egui::CentralPanel::default().show(ctx, |ui| cockpit.operations_navigation(ui));
                });
            assert!(cockpit.services_open && cockpit.service_tab == ServiceTab::Cargo);
            assert!(ctx.memory(|m| m.focused().is_none()));
        }
    }

}
