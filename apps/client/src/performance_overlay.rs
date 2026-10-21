//! Read-only diagnostics projected from samples already observed by the client.
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use std::collections::VecDeque;

pub(crate) struct PerformanceSamples<'a> {
    /// Wall-clock intervals between client update calls, in seconds.
    pub frame_intervals_s: &'a VecDeque<f64>,
    /// Age of the last authority snapshot, not a measured network round trip.
    pub snapshot_age_ms: Option<u64>,
    pub connected: bool,
    pub paused: bool,
    pub session: &'a str,
    /// Last actual fixed-tick host duration and its simulation timestamp; cached.
    pub server_tick: Option<(u64, u64)>,
}

struct FrameStats {
    intervals_ms: Vec<f64>,
    mean_ms: f64,
    p95_ms: f64,
    worst_ms: f64,
    over_33_ms: usize,
    over_50_ms: usize,
}

impl FrameStats {
    fn from_samples(samples: &VecDeque<f64>) -> Option<Self> {
        let intervals_ms: Vec<_> = samples
            .iter()
            .copied()
            .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
            .map(|seconds| seconds * 1000.0)
            .collect();
        if intervals_ms.is_empty() {
            return None;
        }
        let mut sorted = intervals_ms.clone();
        sorted.sort_by(f64::total_cmp);
        let p95_index = ((sorted.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
        Some(Self {
            mean_ms: intervals_ms.iter().sum::<f64>() / intervals_ms.len() as f64,
            p95_ms: sorted[p95_index],
            worst_ms: *sorted.last().expect("nonempty measured samples"),
            over_33_ms: intervals_ms
                .iter()
                .filter(|&&ms| ms > 1000.0 / 30.0)
                .count(),
            over_50_ms: intervals_ms.iter().filter(|&&ms| ms > 50.0).count(),
            intervals_ms,
        })
    }
}

pub(crate) fn show(ctx: &egui::Context, open: &mut bool, samples: PerformanceSamples<'_>) {
    if !*open {
        return;
    }
    let accent = Color32::from_rgb(107, 226, 191);
    let warning = Color32::from_rgb(244, 186, 91);
    let window = egui::Window::new("Performance")
        .id(egui::Id::new("client-performance-overlay"))
        .open(open)
        .order(egui::Order::Foreground)
        .default_pos(egui::pos2((ctx.screen_rect().right() - 410.0).max(8.0), 108.0))
        .max_height((ctx.screen_rect().height() - 24.0).max(160.0))
        .vscroll(true)
        .default_width(360.0)
        .min_width(300.0)
        .resizable(false)
        .collapsible(true)
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(
                    if samples.connected { accent } else { warning },
                    if samples.connected { "CONNECTED" } else { "DISCONNECTED" },
                );
                if samples.paused {
                    ui.colored_label(warning, "SIMULATION PAUSED");
                }
                ui.label(samples.session);
            });
            if let Some(stats) = FrameStats::from_samples(samples.frame_intervals_s) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{:.0} FPS", 1000.0 / stats.mean_ms)).size(28.0).monospace().color(accent));
                    ui.label(format!("{:.1} ms average", stats.mean_ms));
                });
                ui.small("Client update cadence · wall-clock intervals");
                egui::Grid::new("client-performance-values")
                    .num_columns(2)
                    .show(ui, |ui| {
                        for (label, value) in [
                            ("95th percentile", format!("{:.1} ms", stats.p95_ms)),
                            ("Worst interval", format!("{:.1} ms", stats.worst_ms)),
                            ("Intervals > 33.3 ms", stats.over_33_ms.to_string()),
                            ("Hitches > 50 ms", stats.over_50_ms.to_string()),
                        ] {
                            ui.label(label);
                            ui.monospace(value);
                            ui.end_row();
                        }
                    });
                history(ui, &stats, accent, warning);
                ui.small(format!("{} observed intervals · oldest to newest", stats.intervals_ms.len()));
            } else {
                ui.label("Frame timing unavailable · waiting for measured intervals");
            }
            ui.separator();
            ui.label(match samples.snapshot_age_ms {
                Some(age) => format!("Last snapshot receipt: {age} ms ago{}", if samples.connected { "" } else { " · last received" }),
                None => "Last snapshot receipt: unavailable".into(),
            });
            ui.small("Receipt age is time since the last accepted snapshot, not ping or the age of a simulated event. Paused snapshots remain valid.");
            ui.label("Network RTT: not instrumented");
            ui.label(match samples.server_tick {
                Some((duration_us, time_ms)) => format!("Server tick work: {:.3} ms · sampled at sim {time_ms} ms", duration_us as f64 / 1000.0),
                None => "Server tick duration: unavailable in this session".into(),
            });
            ui.small("Tick work is a cached authority host-duration sample, not the simulation timestep or transport latency.");
            ui.label("Whole-frame GPU time: unavailable here");
            ui.small("Update intervals include idle/background scheduling and synchronization. They do not isolate CPU work, GPU work, or network latency. Editor render inspection has separate per-pass timing where supported.");
        });
    if let Some(window) = window { ctx.move_to_top(window.response.layer_id); }
}

fn history(ui: &mut egui::Ui, stats: &FrameStats, accent: Color32, warning: Color32) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 70.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, Color32::from_rgb(8, 17, 23));
    let plot = rect.shrink(5.0);
    let ceiling = stats.worst_ms.max(50.0);
    for (budget, color) in [
        (1000.0 / 60.0, Color32::from_gray(61)),
        (50.0, warning.gamma_multiply(0.4)),
    ] {
        let y = plot.bottom() - (budget / ceiling) as f32 * plot.height();
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            Stroke::new(1.0, color),
        );
    }
    let denominator = stats.intervals_ms.len().saturating_sub(1).max(1) as f32;
    let points: Vec<_> = stats
        .intervals_ms
        .iter()
        .enumerate()
        .map(|(index, &ms)| {
            egui::pos2(
                plot.left() + index as f32 / denominator * plot.width(),
                plot.bottom() - (ms / ceiling) as f32 * plot.height(),
            )
        })
        .collect();
    if points.len() > 1 {
        painter.add(egui::Shape::line(points, Stroke::new(1.5, accent)));
    } else if let Some(point) = points.first() {
        painter.circle_filled(*point, 2.0, accent);
    }
    ui.small(format!(
        "History scale: 0–{ceiling:.1} ms · guides: 16.7 / 50 ms"
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statistics_use_only_observed_positive_finite_intervals() {
        let samples = VecDeque::from([0.01, 0.02, 0.06, f64::NAN, 0.0, -1.0]);
        let stats = FrameStats::from_samples(&samples).unwrap();
        assert_eq!(stats.intervals_ms, vec![10.0, 20.0, 60.0]);
        assert_eq!(stats.mean_ms, 30.0);
        assert_eq!(stats.p95_ms, 60.0);
        assert_eq!(stats.worst_ms, 60.0);
        assert_eq!((stats.over_33_ms, stats.over_50_ms), (1, 1));
        assert!(FrameStats::from_samples(&VecDeque::new()).is_none());
    }
}
