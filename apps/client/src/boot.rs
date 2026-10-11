//! Presentation-only relay startup. Completion requires a real authenticated snapshot.
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
use web_time::Instant;

pub struct Boot {
    started: Instant,
    pub visible: bool,
    connecting: bool,
    failed: bool,
    ready_since: Option<Instant>,
    entered_at: Option<Instant>,
}
#[derive(PartialEq, Debug)]
pub enum Action {
    Wait,
    Enter,
    Retry,
    Back,
}
impl Default for Boot {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            visible: true,
            connecting: false,
            failed: false,
            ready_since: None,
            entered_at: None,
        }
    }
}
impl Boot {
    pub fn paint_handoff(&self, ctx: &egui::Context) {
        let Some(started) = self.entered_at else {
            return;
        };
        let t = (started.elapsed().as_secs_f32() / 0.8).min(1.0);
        if t >= 1.0 {
            return;
        }
        let rect = ctx.screen_rect();
        let opening = t * t * (3.0 - 2.0 * t);
        let height = rect.height() * 0.5 * (1.0 - opening);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("relay_arrival"),
        ));
        let top = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), height));
        let bottom = egui::Rect::from_min_size(
            Pos2::new(rect.left(), rect.bottom() - height),
            Vec2::new(rect.width(), height),
        );
        for panel in [top, bottom] {
            painter.rect_filled(panel, 0.0, Color32::from_rgb(1, 6, 5));
        }
        for y in [top.bottom(), bottom.top()] {
            painter.line_segment(
                [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                Stroke::new(2.0, super::TEAL.gamma_multiply(1.0 - t)),
            );
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
    pub fn connect(&mut self) {
        *self = Self {
            connecting: true,
            ..Self::default()
        };
    }
    pub fn fail(&mut self) {
        self.failed = true;
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        ready: bool,
        connected: bool,
        notice: &str,
    ) -> Action {
        if !self.visible {
            return Action::Enter;
        }
        let elapsed = self.started.elapsed().as_secs_f32();
        if ready {
            self.ready_since.get_or_insert_with(Instant::now);
        } else {
            self.ready_since = None;
        }
        let arrival_finished = !self.connecting
            || self
                .ready_since
                .is_some_and(|t| t.elapsed().as_secs_f32() >= 0.75);
        if arrival_finished && should_enter(elapsed, self.connecting, ready, self.failed) {
            self.visible = false;
            self.entered_at = Some(Instant::now());
            return Action::Enter;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
        let mut action = Action::Wait;
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Color32::from_rgb(1, 6, 5))).show(ctx, |ui| {
            let rect = ui.max_rect();
            let painter = ui.painter_at(rect);
            let center = rect.center() - Vec2::new(0.0, rect.height() * 0.10);
            let radius = (rect.height() * 0.16).clamp(45.0, 130.0);
            let green = if self.failed { super::AMBER } else { super::TEAL };
            let pulse = if ready { ((elapsed - 2.0).max(0.0) * 1.5).min(1.0) } else { 0.0 };
            // Deterministic relay particles and a scrolling grid, with bounded geometry.
            for n in 0..64 {
                let angle = n as f32 * 2.399963;
                let travel = (elapsed * 0.17 + n as f32 / 64.0).fract();
                let distance = radius * (1.4 + travel * 5.0);
                let direction = Vec2::angled(angle);
                let start = center + direction * distance;
                painter.line_segment([start, start + direction * (3.0 + travel * 30.0)], Stroke::new(1.0, green.gamma_multiply(0.08 + travel * 0.28)));
            }
            for n in 0..18 {
                let y = rect.top() + ((n as f32 * 48.0 + elapsed * 14.0) % rect.height());
                painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(0.5, green.gamma_multiply(0.06)));
            }
            for ring in 0..3 {
                let r = radius * (0.72 + ring as f32 * 0.25) * (1.0 + pulse * 0.4);
                let phase = elapsed * if ring % 2 == 0 { 0.65 } else { -0.45 };
                for segment in 0..6 {
                    let start = phase + segment as f32 * std::f32::consts::TAU / 6.0;
                    let points = (0..20).map(|i| center + Vec2::angled(start + i as f32 * 0.037) * r).collect();
                    painter.add(egui::Shape::line(points, Stroke::new(if ring == 1 { 2.0 } else { 1.0 }, green.gamma_multiply(0.45 + ring as f32 * 0.2))));
                }
            }
            painter.text(center - Vec2::new(0.0, 13.0), egui::Align2::CENTER_CENTER, "P.", egui::FontId::proportional(radius * 0.52), green);
            painter.text(center + Vec2::new(0.0, 27.0), egui::Align2::CENTER_CENTER, "I N D U S T R I E S", egui::FontId::monospace(12.0), green);
            let width = rect.width().min(440.0) - 32.0;
            let content = egui::Rect::from_min_size(Pos2::new(center.x - width / 2.0, center.y + radius * 1.5), Vec2::new(width, 210.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("A R K L I N E").size(25.0).color(green));
                    ui.small(format!("{}  /  EARLY ACCESS", super::BUILD_VERSION.trim()));
                    ui.add_space(12.0);
                    let stage = stage_label(elapsed, self.connecting, connected, ready, self.failed);
                    ui.colored_label(green, stage);
                    ui.add_space(8.0);
                    for (label, done) in [("FLIGHT INTERFACE", true), ("RELAY / PILOT SESSION", connected && elapsed >= 1.1), ("AUTHORITATIVE WORLD", ready && elapsed >= 2.2)] {
                        ui.horizontal(|ui| {
                            ui.colored_label(if done { green } else { super::MUTED }, if done { "[ OK ]" } else { "[ -- ]" });
                            ui.small(label);
                        });
                    }
                    if self.failed {
                        ui.colored_label(super::AMBER, notice);
                        ui.horizontal(|ui| {
                            if ui.button("Retry relay").clicked() { action = Action::Retry; }
                            if ui.button("Pilot / server settings").clicked() { action = Action::Back; }
                        });
                    } else if ready || !self.connecting {
                        if ui.small_button(if ready { "Enter cockpit" } else { "Continue" }).clicked() { action = Action::Enter; }
                    } else {
                        ui.small("Waiting for the server. No flight commands are sent during startup.");
                        if ui.small_button("Pilot / server settings").clicked() { action = Action::Back; }
                    }
                });
            });
        });
        if action == Action::Enter {
            self.entered_at = Some(Instant::now());
        }
        if matches!(action, Action::Enter | Action::Back) {
            self.visible = false;
        }
        action
    }
}
fn stage_label(
    elapsed: f32,
    trying: bool,
    connected: bool,
    ready: bool,
    failed: bool,
) -> &'static str {
    if failed {
        "RELAY CONNECTION INTERRUPTED"
    } else if elapsed < 0.65 || !trying {
        "INITIALIZING FLIGHT INTERFACE"
    } else if !connected {
        "ESTABLISHING FTL RELAY CONNECTION"
    } else if elapsed < 1.5 {
        "FTL RELAY LINK CONFIRMED"
    } else if elapsed < 2.2 {
        "PILOT SESSION ACCEPTED"
    } else if ready {
        "WORLD SYNCHRONIZED · ENTERING COCKPIT"
    } else {
        "RECEIVING WORLD TELEMETRY"
    }
}
fn should_enter(elapsed: f32, connecting: bool, ready: bool, failed: bool) -> bool {
    !failed
        && if connecting {
            ready && elapsed >= 3.2
        } else {
            elapsed >= 1.8
        }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stages_never_claim_an_unconfirmed_world_or_session() {
        assert_eq!(
            stage_label(30.0, true, false, false, false),
            "ESTABLISHING FTL RELAY CONNECTION"
        );
        assert_eq!(
            stage_label(30.0, true, true, false, false),
            "RECEIVING WORLD TELEMETRY"
        );
        assert_eq!(
            stage_label(30.0, true, true, true, false),
            "WORLD SYNCHRONIZED · ENTERING COCKPIT"
        );
        assert_eq!(
            stage_label(30.0, true, true, true, true),
            "RELAY CONNECTION INTERRUPTED"
        );
    }
    #[test]
    fn animation_cannot_complete_a_missing_or_failed_connection() {
        assert!(!should_enter(1000.0, true, false, false));
        assert!(!should_enter(1000.0, true, true, true));
        assert!(!should_enter(1.0, true, true, false));
        assert!(should_enter(3.2, true, true, false));
        assert!(should_enter(1.8, false, false, false));
    }
}
