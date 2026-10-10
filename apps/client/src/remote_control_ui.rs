//! Player identity never changes when a camera or control link targets another ship.
use super::*;
impl Cockpit {
    pub(super) fn controlled_remote(&self) -> Option<&str> {
        let target = self.manual_remote.as_deref()?;
        (self.connected
            && self.snapshot.as_ref().is_some_and(|s| {
                s.remote_controls
                    .iter()
                    .any(|l| l.controller == self.pilot && l.pilot == target)
            }))
        .then_some(target)
    }
    pub(super) fn controlled_name(&self) -> &str {
        self.controlled_remote().unwrap_or(&self.pilot)
    }
    pub(super) fn clear_ship_input(&mut self) {
        self.controls.clear();
        self.pilot_input.relinquish(&self.controls);
        self.cancel_ai();
        self.effects.clear();
        self.selected_drone = None;
        self.selected_object = SelectedObject::Station;
        self.mining_target = None;
        self.tactical_system = false;
    }
    pub(super) fn return_to_my_ship(&mut self) {
        let release = self.manual_remote.is_some()
            || self
                .snapshot
                .as_ref()
                .is_some_and(|s| s.remote_controls.iter().any(|l| l.controller == self.pilot));
        self.clear_ship_input();
        self.manual_remote = None;
        self.remote_ship = None;
        self.resource_pilot = None;
        self.drone_camera = false;
        self.service_tab = ServiceTab::Navigation;
        // Release remains sendable while another command is awaiting a reply.
        // It is ordered after that command; fresh controls stay gated until acknowledgement.
        if release && self.connected {
            if let Some(tx) = &self.commands {
                self.request_pending = tx.send(Request::EndRemoteControl).is_ok();
            }
        }
    }
    pub(super) fn take_remote_control(&mut self, pilot: String) {
        if pilot == self.pilot || !self.can_operate() {
            return;
        }
        self.clear_ship_input();
        self.send(Request::BeginRemoteControl {
            pilot: pilot.clone(),
        });
        self.manual_remote = Some(pilot);
        self.remote_ship = None;
        self.view_mode = ViewMode::Cockpit;
        self.service_tab = ServiceTab::Navigation;
    }
    pub(super) fn sync_remote_control(&mut self) {
        if self.manual_remote.is_some()
            && (!self.connected || (!self.request_pending && self.controlled_remote().is_none()))
        {
            self.return_to_my_ship();
            self.notice = "Remote control ended. You are back in your own ship.".into();
        }
        if self.remote_ship.as_ref().is_some_and(|target| {
            target != &self.pilot
                && self.snapshot.as_ref().is_some_and(|s| {
                    !s.fleet_links
                        .iter()
                        .any(|l| l.controller == self.pilot && &l.pilot == target)
                })
        }) {
            self.remote_ship = None;
        }
    }
    pub(super) fn ship_connection_panel(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new(format!("PLAYER  {}", self.pilot)).strong());
        if let Some(target) = self.manual_remote.clone() {
            ui.colored_label(
                AMBER,
                egui::RichText::new(format!("REMOTE CONTROL  →  {target}")).strong(),
            );
            ui.small(if self.controlled_remote().is_some() { "Flight, modules and drones control this remote ship. Your player identity stays yours." } else { "Requesting control · flight input paused" });
        } else if let Some(target) = &self.remote_ship {
            ui.colored_label(AMBER, format!("WATCHING  {target}  ·  CAMERA ONLY"));
            ui.small(format!("You are flying your own ship: {}", self.pilot));
        } else {
            ui.colored_label(
                self.accent(),
                format!("YOUR SHIP  {}  ·  LOCAL CONTROLS", self.pilot),
            );
        }
        if (self.manual_remote.is_some() || self.remote_ship.is_some())
            && ui
                .add(egui::Button::new("Return to my ship").min_size(Vec2::new(155.0, 28.0)))
                .clicked()
        {
            self.return_to_my_ship();
        }
    }
}
