//! Body-view aim reference and the same authoritative nearby action used by F.
use super::*;

impl Cockpit {
    pub(super) fn on_foot_look_hint(&self) -> &'static str {
        if self.first_person_hud_active() {
            "Mouse look · Esc cursor"
        } else {
            "RMB look"
        }
    }

    pub(super) fn first_person_hud_active(&self) -> bool {
        self.connected
            && !self.ship_observation
            && self.manual_remote.is_none()
            && self.in_station_hangar()
            && self.hangar_view.on_foot_view == flight3d::hangar::OnFootView::FirstPerson
            && self
                .own()
                .is_some_and(|pilot| pilot.station_avatar.is_some())
    }

    pub(super) fn first_person_capture_allowed(&self, ctx: &egui::Context) -> bool {
        self.first_person_hud_active()
            && self.embedded_input
            && !self.boot.visible
            && !self.controls_reference_open
            && !self.performance_open
            && !self.services_open
            && self.service_tab != ServiceTab::Layouts
            && self.engineering_panel.is_none()
            && !self.cockpit_computer.wants_keyboard_input()
            && !ctx.wants_keyboard_input()
            && ctx.input(|input| input.focused && !input.viewport().minimized.unwrap_or(false))
    }

    pub(super) fn first_person_hud(&mut self, ctx: &egui::Context) {
        if !self.first_person_hud_active() {
            return;
        }
        if !self.first_person_capture_allowed(ctx) {
            self.hangar_view.release_first_person_mouse(ctx);
            self.nearby_shortcut.cancel();
            return;
        }
        let Some((viewport, _)) = self.hangar_view.input_surface() else {
            return;
        };
        let captured = self.hangar_view.first_person_mouse_captured();
        let color = if captured { self.accent() } else { MUTED };
        let center = viewport.center();
        // This is an aim reference. Nearby actions remain explicitly proximity-
        // based and retain their real range/obstruction checks; no fake picking.
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("body-aim-reference"),
        ));
        for (a, b) in [
            ((-7.0, 0.0), (-3.0, 0.0)),
            ((3.0, 0.0), (7.0, 0.0)),
            ((0.0, -7.0), (0.0, -3.0)),
            ((0.0, 3.0), (0.0, 7.0)),
        ] {
            painter.line_segment(
                [center + Vec2::new(a.0, a.1), center + Vec2::new(b.0, b.1)],
                egui::Stroke::new(1.0, color),
            );
        }
        // Registered Pioneer actions have one always-visible primary strip.
        // Keep only the aim reference here; legacy/standalone body contexts
        // retain their existing card and shortcut owner below.
        if self.docked_navigation_present() {
            return;
        }
        let status = if self.embedded && self.embedded_paused {
            "SIMULATION PAUSED"
        } else if self
            .own()
            .is_some_and(|pilot| pilot.location == Location::Docked)
        {
            "DOCKED INTERIOR"
        } else {
            "IN SPACE · EXTERIOR ACCESS LOCKED"
        };
        egui::Area::new(egui::Id::new("body-context-hud"))
            .order(egui::Order::Foreground)
            .sense(egui::Sense::hover())
            .fixed_pos(egui::pos2(
                (center.x - 220.0).max(viewport.left()),
                (viewport.bottom() - 110.0).max(viewport.top()),
            ))
            .constrain_to(viewport)
            .show(ctx, |ui| {
                ui.set_max_width(440.0);
                egui::Frame::new()
                    .fill(Color32::from_black_alpha(170))
                    .inner_margin(egui::Margin::same(8))
                    .corner_radius(4)
                    .show(ui, |ui| {
                        ui.small(status);
                        ui.small(if captured {
                            "MOUSE LOOK · Esc releases cursor"
                        } else {
                            "CURSOR FREE · click clear viewport to look"
                        });
                        // Exactly one prompt owner and one gesture advance in
                        // first person; the lower strip skips its duplicate.
                        self.nearby_interaction_prompt(ui);
                    });
            });
    }
}
