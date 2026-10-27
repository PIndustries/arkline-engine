//! Isolated numerical experiments. No connection to gameplay, world saves, or production.
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use std::{collections::VecDeque, time::Instant};
const DT: f64 = 0.02;
const MAX_SECONDS: f64 = 600.;
const MAX_POSITION: f64 = 1_000_000.;
const HISTORY: usize = 1200;
#[derive(Clone, Copy, Debug, PartialEq)]
struct Parameters {
    mass: f64,
    thrust: f64,
    gravity: f64,
    expansion: f64,
    points: usize,
}
impl Default for Parameters {
    fn default() -> Self {
        Self {
            mass: 1000.,
            thrust: 12_000.,
            gravity: 9.81,
            expansion: 0.08,
            points: 160,
        }
    }
}
impl Parameters {
    fn bounded(self) -> Self {
        let d = Self::default();
        let bound = |x: f64, low, high, fallback| {
            if x.is_finite() {
                x.clamp(low, high)
            } else {
                fallback
            }
        };
        Self {
            mass: bound(self.mass, 1., 100_000., d.mass),
            thrust: bound(self.thrust, 0., 1_000_000., d.thrust),
            gravity: bound(self.gravity, 0., 20., d.gravity),
            expansion: bound(self.expansion, -0.05, 1., d.expansion),
            points: self.points.clamp(16, 512),
        }
    }
    fn acceleration(self) -> f64 {
        self.thrust / self.mass - self.gravity
    }
}
#[derive(Clone, Copy, Debug)]
struct Sample {
    time: f64,
    height: f64,
    velocity: f64,
}
struct Experiment {
    parameters: Parameters,
    ticks: u64,
    height: f64,
    velocity: f64,
    remainder: f64,
    playing: bool,
    limit: bool,
    seed: Vec<[f64; 2]>,
    history: VecDeque<Sample>,
}
impl Experiment {
    fn new(parameters: Parameters) -> Self {
        let parameters = parameters.bounded();
        let mut seed = Vec::with_capacity(parameters.points);
        // A fixed integer generator makes every Reset exactly reproducible.
        let mut state = 0x4152_4b4c_494e_4531u64;
        let mut random = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 32) as u32) as f64 / u32::MAX as f64
        };
        for _ in 0..parameters.points {
            let angle = random() * std::f64::consts::TAU;
            let radius = random().sqrt() * 100.;
            seed.push([radius * angle.cos(), radius * angle.sin()]);
        }
        Self {
            parameters,
            ticks: 0,
            height: 0.,
            velocity: 0.,
            remainder: 0.,
            playing: false,
            limit: false,
            seed,
            history: VecDeque::from([Sample {
                time: 0.,
                height: 0.,
                velocity: 0.,
            }]),
        }
    }
    fn time(&self) -> f64 {
        self.ticks as f64 * DT
    }
    fn step(&mut self) {
        if self.limit {
            return;
        }
        let acceleration = self.parameters.acceleration();
        // Exact constant-acceleration integration, sampled at fixed 20 ms intervals.
        let next_height = self.height + self.velocity * DT + 0.5 * acceleration * DT * DT;
        let next_velocity = self.velocity + acceleration * DT;
        if next_height.abs() > MAX_POSITION
            || !next_height.is_finite()
            || !next_velocity.is_finite()
            || self.time() + DT > MAX_SECONDS + 1e-9
        {
            self.limit = true;
            self.playing = false;
            self.remainder = 0.;
            return;
        }
        self.height = next_height;
        self.velocity = next_velocity;
        self.ticks += 1;
        self.history.push_back(Sample {
            time: self.time(),
            height: self.height,
            velocity: self.velocity,
        });
        while self.history.len() > HISTORY {
            self.history.pop_front();
        }
    }
    fn advance(&mut self, wall_seconds: f64, time_scale: f64) {
        if !self.playing || !wall_seconds.is_finite() {
            return;
        }
        let speed = if time_scale.is_finite() {
            time_scale.clamp(0.1, 10.)
        } else {
            1.
        };
        self.remainder += wall_seconds.clamp(0., 0.25) * speed;
        // At most 125 simulation steps; stalled rendering never creates unbounded work.
        let count = ((self.remainder + 1e-10) / DT).floor().min(125.) as usize;
        self.remainder -= count as f64 * DT;
        self.remainder = self.remainder.max(0.);
        for _ in 0..count {
            self.step();
            if self.limit {
                break;
            }
        }
    }
    fn pause(&mut self) {
        self.playing = false;
        self.remainder = 0.;
    }
    fn expansion_scale(&self) -> f64 {
        (1. + self.parameters.expansion * self.time()).max(0.)
    }
    fn extent(&self) -> f64 {
        self.seed
            .iter()
            .map(|p| p[0].hypot(p[1]))
            .fold(0., f64::max)
            * self.expansion_scale()
    }
}
pub struct PhysicsLab {
    draft: Parameters,
    experiment: Experiment,
    speed: f64,
    observed: Instant,
}
impl Default for PhysicsLab {
    fn default() -> Self {
        Self {
            draft: Parameters::default(),
            experiment: Experiment::new(Parameters::default()),
            speed: 1.,
            observed: Instant::now(),
        }
    }
}
impl PhysicsLab {
    pub fn show(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.observed).as_secs_f64();
        self.observed = now;
        // Returning from another workspace must not advance an unseen experiment.
        if elapsed <= 0.25 {
            self.experiment.advance(elapsed, self.speed);
        } else {
            self.experiment.pause();
        }
        egui::CentralPanel::default().show(ctx,|ui| {
            ui.heading("Local physics lab");
            ui.label("Isolated experiments · no gameplay, saved-world, or public-server changes");
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!self.experiment.limit,egui::Button::new(if self.experiment.playing{"Pause"}else{"Play"})).clicked(){if self.experiment.playing{self.experiment.pause();}else{self.experiment.playing=true;}}
                if ui.add_enabled(!self.experiment.playing&&!self.experiment.limit,egui::Button::new("Step 20 ms")).clicked(){self.experiment.step();}
                if ui.button("Reset experiment").clicked(){self.experiment=Experiment::new(self.experiment.parameters);}
                ui.label("Time speed");ui.add(egui::Slider::new(&mut self.speed,0.1..=10.).suffix("×").logarithmic(true));
                ui.strong(if self.experiment.limit{"SAFETY LIMIT · RESET REQUIRED"}else if self.experiment.playing{"RUNNING"}else{"PAUSED"});
            });
            ui.separator();
            egui::ScrollArea::vertical().id_salt("physics_lab_scroll").show(ui,|ui| {
                if ui.available_width() > 900. {
                    ui.columns(2, |columns| {
                        thrust_parameters(&mut columns[0], &mut self.draft);
                        cloud_parameters(&mut columns[1], &mut self.draft);
                    });
                } else {
                    thrust_parameters(ui, &mut self.draft);
                    ui.add_space(6.);
                    cloud_parameters(ui, &mut self.draft);
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Apply parameters and reset").clicked(){self.draft=self.draft.bounded();self.experiment=Experiment::new(self.draft);}
                    if self.draft!=self.experiment.parameters {ui.colored_label(Color32::YELLOW,"Pending changes · current run still uses applied parameters");}
                    else {ui.label("Parameters applied");}
                });
                let e=&self.experiment;
                ui.horizontal_wrapped(|ui| {
                    for metric in [format!("Elapsed {:.2} s",e.time()),format!("Acceleration {:.3} m/s²",e.parameters.acceleration()),format!("Velocity {:.3} m/s",e.velocity),format!("Height {:.3} m",e.height),format!("Points {}",e.seed.len()),format!("Extent radius {:.2} m",e.extent())] {ui.label(RichText::new(metric).monospace());ui.separator();}
                });
                ui.small("Limits: 600 simulated seconds, ±1,000,000 m displacement, 125 fixed steps per frame. Long absence pauses the lab.");
                let width=ui.available_width();
                if width>=800. {
                    ui.columns(2,|columns|{draw_cloud(&mut columns[0],e);draw_graph(&mut columns[1],e);});
                } else {draw_cloud(ui,e);draw_graph(ui,e);}
            });
        });
        if self.experiment.playing {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}
fn thrust_parameters(ui: &mut egui::Ui, draft: &mut Parameters) {
    let width = (ui.available_width() - 14.).max(240.);
    ui.group(|ui| {
        ui.set_width(width);
        ui.vertical(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.strong("1D thrust and uniform gravity");
            egui::Grid::new("physics_parameters")
                .num_columns(2)
                .max_col_width((width - 110.).max(100.))
                .show(ui, |ui| {
                    ui.label("Mass (kg)");
                    ui.add(
                        egui::DragValue::new(&mut draft.mass)
                            .range(1. ..=100_000.)
                            .speed(10.),
                    );
                    ui.end_row();
                    ui.label("Upward thrust (N)");
                    ui.add(
                        egui::DragValue::new(&mut draft.thrust)
                            .range(0. ..=1_000_000.)
                            .speed(100.),
                    );
                    ui.end_row();
                    ui.label("Downward gravity (m/s²)");
                    ui.add(
                        egui::DragValue::new(&mut draft.gravity)
                            .range(0. ..=20.)
                            .speed(0.01),
                    );
                    ui.end_row();
                });
            ui.add_space(4.);
            ui.small(
                "a = F / m − g. Constant mass, no drag, fuel, ground collision or orbital gravity.",
            );
            ui.small("Height is signed displacement from the start; negative values are allowed.");
        });
    });
}
fn cloud_parameters(ui: &mut egui::Ui, draft: &mut Parameters) {
    let width = (ui.available_width() - 14.).max(240.);
    ui.group(|ui| {
        ui.set_width(width);
        ui.vertical(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.strong("Seeded kinematic point cloud");
            egui::Grid::new("cloud_parameters").num_columns(2).max_col_width((width - 110.).max(100.)).show(ui, |ui| {
                ui.label("Linear scale rate (/s)"); ui.add(egui::DragValue::new(&mut draft.expansion).range(-0.05..=1.).speed(0.005)); ui.end_row();
                ui.label("Points"); ui.add(egui::DragValue::new(&mut draft.points).range(16..=512)); ui.end_row();
            });
            ui.add_space(4.);
            ui.small("r(t) = r₀ × max(0, 1 + rate × t). Initial radius ≤100 m.");
            ui.small("Kinematic scaling only: no cosmology, particle forces, collisions or gravity coupling.");
        });
    });
}
fn canvas(ui: &mut egui::Ui, title: &str) -> (Rect, egui::Painter) {
    ui.strong(title);
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(40.), 280.),
        Sense::hover(),
    );
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 4., Color32::from_rgb(5, 15, 15));
    (rect.shrink(16.), painter)
}
fn draw_cloud(ui: &mut egui::Ui, e: &Experiment) {
    let (rect, p) = canvas(ui, "Point cloud · adaptive scale, origin-centered");
    // Bounds expand only beyond the initial 100 m reference; metrics make rescaling explicit.
    let radius = e.extent().max(100.);
    let pixels = rect.width().min(rect.height()) * 0.46;
    let center = rect.center();
    p.line_segment(
        [
            Pos2::new(rect.left(), center.y),
            Pos2::new(rect.right(), center.y),
        ],
        Stroke::new(1., Color32::DARK_GRAY),
    );
    p.line_segment(
        [
            Pos2::new(center.x, rect.top()),
            Pos2::new(center.x, rect.bottom()),
        ],
        Stroke::new(1., Color32::DARK_GRAY),
    );
    let reference = (100. / radius) as f32 * pixels;
    p.circle_stroke(
        center,
        reference,
        Stroke::new(1., Color32::from_rgb(90, 120, 110)),
    );
    for point in &e.seed {
        let factor = e.expansion_scale() / radius * pixels as f64;
        p.circle_filled(
            center + Vec2::new((point[0] * factor) as f32, (-point[1] * factor) as f32),
            2.,
            Color32::from_rgb(60, 230, 155),
        );
    }
    p.text(
        rect.left_top(),
        egui::Align2::LEFT_TOP,
        format!("Outer scale {:.1} m · gray ring 100 m", radius),
        egui::FontId::monospace(11.),
        Color32::LIGHT_GRAY,
    );
}
fn draw_graph(ui: &mut egui::Ui, e: &Experiment) {
    let (rect, p) = canvas(ui, "Height history · last 24 simulated seconds");
    let max_height = e.history.iter().map(|s| s.height.abs()).fold(1., f64::max);
    let start = e.history.front().map_or(0., |s| s.time);
    let span = (e.time() - start).max(1.);
    let y = rect.center().y;
    p.line_segment(
        [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
        Stroke::new(1., Color32::DARK_GRAY),
    );
    let points: Vec<_> = e
        .history
        .iter()
        .map(|s| {
            Pos2::new(
                rect.left() + ((s.time - start) / span) as f32 * rect.width(),
                y - (s.height / max_height) as f32 * rect.height() * 0.42,
            )
        })
        .collect();
    if points.len() > 1 {
        p.add(egui::Shape::line(
            points,
            Stroke::new(2., Color32::from_rgb(60, 230, 155)),
        ));
    }
    let velocity = e.history.back().map_or(0., |s| s.velocity);
    p.text(
        rect.left_top(),
        egui::Align2::LEFT_TOP,
        format!("Vertical ±{max_height:.1} m · velocity {velocity:.2} m/s"),
        egui::FontId::monospace(11.),
        Color32::LIGHT_GRAY,
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parameter_groups_wrap_and_stack_without_horizontal_overflow() {
        for width in [760., 1162.] {
            let ctx = egui::Context::default();
            let mut lab = PhysicsLab::default();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 1000.))),
                    ..Default::default()
                },
                |ctx| lab.show(ctx),
            );
            let mut gravity_bottom = None;
            let mut equation_top = None;
            let mut cloud_top = None;
            let mut height_bottom = None;
            for shape in output.shapes {
                if let egui::Shape::Text(t) = shape.shape {
                    let text = t.galley.text();
                    if text.starts_with("a =")
                        || text.starts_with("r(t)")
                        || text.starts_with("Kinematic scaling")
                        || text.starts_with("Height is")
                    {
                        assert!(
                            t.pos.x >= 0. && t.pos.x + t.galley.size().x <= width,
                            "overflow at {width}: {text}"
                        );
                    }
                    if text == "Downward gravity (m/s²)" {
                        gravity_bottom = Some(t.pos.y + t.galley.size().y);
                    }
                    if text.starts_with("a =") {
                        equation_top = Some(t.pos.y);
                    }
                    if text.starts_with("Height is") {
                        height_bottom = Some(t.pos.y + t.galley.size().y);
                    }
                    if text == "Seeded kinematic point cloud" {
                        cloud_top = Some(t.pos.y);
                    }
                }
            }
            assert!(
                equation_top.unwrap() > gravity_bottom.unwrap(),
                "explanation must be below parameter grid"
            );
            if width < 900. {
                assert!(
                    cloud_top.unwrap() > height_bottom.unwrap(),
                    "narrow layout must stack groups"
                );
            }
        }
    }
    #[test]
    fn visible_step_button_advances_one_fixed_step() {
        let ctx = egui::Context::default();
        let mut lab = PhysicsLab::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1200., 800.));
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| lab.show(ctx),
        );
        let point = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == "Step 20 ms" => {
                    Some(t.pos + Vec2::new(10., 6.))
                }
                _ => None,
            })
            .expect("step button rendered");
        for pressed in [true, false] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                    ..Default::default()
                },
                |ctx| lab.show(ctx),
            );
        }
        assert_eq!(lab.experiment.ticks, 1);
        assert!(!lab.experiment.playing);
    }
    #[test]
    fn fixed_step_matches_force_over_mass_minus_gravity() {
        let mut e = Experiment::new(Parameters {
            mass: 2.,
            thrust: 30.,
            gravity: 5.,
            ..Default::default()
        });
        for _ in 0..50 {
            e.step();
        }
        assert!((e.time() - 1.).abs() < 1e-10);
        assert!((e.velocity - 10.).abs() < 1e-10);
        assert!((e.height - 5.).abs() < 1e-10);
    }
    #[test]
    fn pause_and_reset_are_deterministic() {
        let mut a = Experiment::new(Default::default());
        let b = Experiment::new(Default::default());
        assert_eq!(a.seed, b.seed);
        a.advance(0.25, 10.);
        assert_eq!(a.ticks, 0);
        a.playing = true;
        a.advance(0.1, 1.);
        a.pause();
        let before = (a.ticks, a.height, a.velocity);
        a.advance(100., 10.);
        assert_eq!(before, (a.ticks, a.height, a.velocity));
    }
    #[test]
    fn render_cadence_does_not_change_fixed_step_result() {
        let mut a = Experiment::new(Default::default());
        let mut b = Experiment::new(Default::default());
        a.playing = true;
        b.playing = true;
        for _ in 0..100 {
            a.advance(0.01, 1.);
        }
        for _ in 0..10 {
            b.advance(0.1, 1.);
        }
        assert_eq!(a.ticks, b.ticks);
        assert_eq!(a.height, b.height);
        assert_eq!(a.velocity, b.velocity);
    }
    #[test]
    fn extremes_remain_finite_and_work_is_bounded() {
        let mut e = Experiment::new(Parameters {
            mass: 0.,
            thrust: f64::INFINITY,
            gravity: f64::NAN,
            expansion: 100.,
            points: usize::MAX,
        });
        e.playing = true;
        e.advance(1000., 1000.);
        assert_eq!(e.ticks, 125);
        assert!(e.height.is_finite());
        assert_eq!(e.seed.len(), 512);
        for _ in 0..500 {
            e.advance(0.25, 10.);
        }
        assert!(e.limit);
        assert!(!e.playing);
        assert!(e.height.abs() <= MAX_POSITION);
        assert!(e.extent().is_finite());
        assert!(e.history.len() <= HISTORY);
    }
    #[test]
    fn cloud_uses_declared_linear_scale() {
        let mut e = Experiment::new(Parameters {
            expansion: 0.5,
            ..Default::default()
        });
        let initial = e.extent();
        for _ in 0..50 {
            e.step();
        }
        assert!((e.extent() - initial * 1.5).abs() < 1e-9);
    }
}
