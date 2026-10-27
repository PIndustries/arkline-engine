//! Read-only reactor configuration. This schematic projects the same rigid
//! collar boxes as the world; it is not a reactor, thermal or motor simulation.
use super::*;
use arkline_protocol::{DockedTraversalState, PioneerReactorConfiguration};

#[derive(Default)]
pub(super) struct PoseSampler {
    owner: String,
    vessel: String,
    snapshot_ms: Option<u64>,
    state: Option<PioneerReactorConfiguration>,
    fraction: Option<f64>,
    available: bool,
}
impl PoseSampler {
    pub fn reset_session(&mut self) {
        *self = Self::default();
    }
    pub fn disconnect(&mut self) {
        self.available = false;
    }
    /// Sample only the confirmed authority timeline supplied by the caller.
    /// A paused timeline cannot move the collars; stale/disconnected views hold
    /// their last sample. Receipt order resolves distinct equal-time payloads.
    pub fn observe(&mut self, snapshot: &Snapshot, owner: &str, connected: bool, now: u64) {
        let state = snapshot
            .pilots
            .iter()
            .find(|pilot| pilot.name == owner && pilot.ship_class == ShipClass::Pioneer)
            .and_then(|pilot| pilot.reactor_configuration.as_ref())
            .filter(|state| {
                state.component_id == arkline_protocol::REACTOR_ASSEMBLY_COMPONENT_ID
                    && !state.vessel_id.is_empty()
                    && state.from_fraction.is_finite()
                    && (0.0..=1.0).contains(&state.from_fraction)
            });
        let vessel = state.map(|state| state.vessel_id.as_str());
        if self.owner != owner || vessel.is_some_and(|vessel| self.vessel != vessel) {
            *self = Self {
                owner: owner.into(),
                vessel: vessel.unwrap_or_default().into(),
                ..Default::default()
            };
        }
        if self
            .snapshot_ms
            .is_none_or(|accepted| snapshot.server_time_ms >= accepted)
        {
            self.snapshot_ms = Some(snapshot.server_time_ms);
            self.state = state.cloned();
            if self.state.is_none() {
                self.fraction = None;
            }
        }
        self.available = connected
            && self
                .snapshot_ms
                .is_some_and(|accepted| now.saturating_sub(accepted) <= 1500)
            && self.state.is_some();
        if self.available {
            self.fraction = self.state.as_ref().map(|state| state.fraction_at(now));
        }
    }
    pub fn state(&self) -> Option<&PioneerReactorConfiguration> {
        self.state.as_ref()
    }
    pub fn fraction(&self) -> Option<f64> {
        self.fraction
    }
    pub fn host_matches(&self, traversal: Option<&DockedTraversalState>) -> bool {
        self.state().is_some_and(|state| {
            traversal.is_none_or(|traversal| traversal.binding.host_vessel_id == state.vessel_id)
        })
    }
    pub fn status(&self) -> &'static str {
        if !self.available {
            return "CONFIGURATION UNAVAILABLE";
        }
        let (Some(state), Some(fraction)) = (&self.state, self.fraction) else {
            return "CONFIGURATION UNAVAILABLE";
        };
        if state.blocked.is_some() {
            "MOTION BLOCKED"
        } else if fraction <= 1e-6 {
            if state.target_compact {
                "COMPACTING"
            } else {
                "EXPANDED / NORMAL"
            }
        } else if fraction >= 1.0 - 1e-6 {
            if state.target_compact {
                "COMPACT / TRAVEL"
            } else {
                "EXPANDING"
            }
        } else if state.target_compact {
            "COMPACTING"
        } else {
            "EXPANDING"
        }
    }
}

#[derive(Clone)]
struct Schematic {
    core: arkline_protocol::PioneerDockedBox,
    collars: [arkline_protocol::PioneerDockedBox; 4],
    // One expanded-envelope projection for every pose: no auto-fit scaling.
    bounds: [f64; 4],
}
fn xy_bounds(bounds: arkline_protocol::PioneerDockedBox) -> [f64; 4] {
    let extent = |axis: usize| {
        (0..3)
            .map(|i| bounds.axes[i][axis].abs() * bounds.half_m[i])
            .sum::<f64>()
    };
    [
        bounds.center_m[0] - extent(0),
        bounds.center_m[1] - extent(1),
        bounds.center_m[0] + extent(0),
        bounds.center_m[1] + extent(1),
    ]
}
fn schematic(fraction: f64) -> Option<Schematic> {
    let core = arkline_protocol::reactor_core_box();
    let collars = [
        arkline_protocol::reactor_configuration_box(0, fraction)?,
        arkline_protocol::reactor_configuration_box(1, fraction)?,
        arkline_protocol::reactor_configuration_box(2, fraction)?,
        arkline_protocol::reactor_configuration_box(3, fraction)?,
    ];
    let mut bounds = xy_bounds(core);
    for slot in 0..4 {
        let expanded = xy_bounds(arkline_protocol::reactor_configuration_box(slot, 0.)?);
        bounds[0] = bounds[0].min(expanded[0]);
        bounds[1] = bounds[1].min(expanded[1]);
        bounds[2] = bounds[2].max(expanded[2]);
        bounds[3] = bounds[3].max(expanded[3]);
    }
    Some(Schematic {
        core,
        collars,
        bounds,
    })
}
fn fitted_text(
    painter: &egui::Painter,
    rect: egui::Rect,
    text: &str,
    size: f32,
    color: egui::Color32,
) {
    let mut size = size;
    let mut galley = painter.layout_no_wrap(text.into(), egui::FontId::monospace(size), color);
    for _ in 0..4 {
        let ink = galley.mesh_bounds;
        let fit = (rect.width() / ink.width().max(0.01))
            .min(rect.height() / ink.height().max(0.01))
            .min(1.);
        if fit >= 1. {
            break;
        }
        size *= fit * 0.99;
        galley = painter.layout_no_wrap(text.into(), egui::FontId::monospace(size), color);
    }
    let ink = galley.mesh_bounds;
    painter.galley(
        egui::pos2(rect.left(), rect.center().y - ink.height() * 0.5) - ink.min.to_vec2(),
        galley,
        color,
    );
}

pub(super) fn paint(
    ui: &egui::Ui,
    surface: &CockpitSurface,
    pose: &PoseSampler,
    own: &PilotView,
    authority: &str,
) {
    let model = pose.fraction().and_then(schematic);
    #[cfg(test)]
    ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("reactor_display_proof"), serde_json::json!({
        "slot":15,"fraction":pose.fraction(),"state":pose.state(),"authority":authority,
        "status":pose.status(),"read_only":true,"schematic_not_camera":true,
        "collars":model.as_ref().map(|model| model.collars.iter().enumerate().map(|(slot, bounds)| serde_json::json!({
            "component_id":arkline_protocol::REACTOR_COLLAR_COMPONENT_IDS[slot],
            "center_m":bounds.center_m,"half_m":bounds.half_m,
        })).collect::<Vec<_>>()),
        "core":model.as_ref().map(|model| serde_json::json!({"center_m":model.core.center_m,"half_m":model.core.half_m})),
    })));
    surface.paint(ui, |painter| {
        let canvas = surface.canvas_rect();
        let rect = canvas.shrink(canvas.height() * 0.035);
        let scale = canvas.height() / 180.;
        let row = |top: f32, bottom: f32| {
            egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.top() + rect.height() * top),
                egui::pos2(rect.right(), rect.top() + rect.height() * bottom),
            )
        };
        let color = if pose.available && authority == "LIVE" {
            super::super::TEAL
        } else {
            super::super::AMBER
        };
        fitted_text(
            painter,
            row(0., 0.105),
            "REACTOR / CONFIGURATION",
            17. * scale,
            color,
        );
        fitted_text(
            painter,
            row(0.115, 0.205),
            &format!("{authority} / {}", pose.status()),
            11. * scale,
            color,
        );
        if let Some(model) = &model {
            let chart = row(0.235, 0.66).shrink(2. * scale);
            let [x0, y0, x1, y1] = model.bounds;
            let pixels_per_m =
                (chart.width() as f64 / (x1 - x0)).min(chart.height() as f64 / (y1 - y0));
            let project = |x: f64, y: f64| {
                chart.center()
                    + egui::vec2(
                        ((x - (x0 + x1) * 0.5) * pixels_per_m) as f32,
                        (((y0 + y1) * 0.5 - y) * pixels_per_m) as f32,
                    )
            };
            let projected = |bounds| {
                let [left, bottom, right, top] = xy_bounds(bounds);
                egui::Rect::from_min_max(project(left, top), project(right, bottom))
            };
            let core = projected(model.core);
            painter.rect_filled(core, 0., egui::Color32::from_rgb(31, 57, 57));
            painter.rect_stroke(
                core,
                0.,
                egui::Stroke::new(1. * scale, super::super::MUTED),
                egui::StrokeKind::Inside,
            );
            for bounds in model.collars {
                // Conservative whole-collar envelope, not a filled solid door.
                painter.rect_stroke(
                    projected(bounds),
                    0.,
                    egui::Stroke::new(1.5 * scale, color),
                    egui::StrokeKind::Inside,
                );
            }
            fitted_text(
                painter,
                row(0.675, 0.735),
                &format!(
                    "4 COLLAR ENVELOPES / FIXED CORE / {:.0}%",
                    pose.fraction().unwrap_or(0.) * 100.
                ),
                9. * scale,
                super::super::MUTED,
            );
        } else {
            fitted_text(
                painter,
                row(0.3, 0.55),
                "NO CONFIRMED CONFIGURATION",
                13. * scale,
                color,
            );
        }
        let capacitor = own.power.as_ref().map_or_else(
            || "CAPACITOR DATA UNAVAILABLE".into(),
            |power| format!("CAPACITOR {} / {} J", power.charge_j, power.capacity_j),
        );
        fitted_text(
            painter,
            row(0.755, 0.835),
            &capacitor,
            12. * scale,
            egui::Color32::from_rgb(202, 228, 215),
        );
        let rating = own.power.as_ref().map_or_else(
            || "REGEN RATING UNAVAILABLE".into(),
            |power| format!("REGEN RATING {} W / READ ONLY", power.regen_w),
        );
        fitted_text(
            painter,
            row(0.845, 0.91),
            &rating,
            10. * scale,
            super::super::MUTED,
        );
        fitted_text(
            painter,
            row(0.925, 1.),
            "HEAT / FUEL / MOTOR / MASS: UNMODELED",
            9. * scale,
            super::super::AMBER,
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(ms: u64, state: Option<PioneerReactorConfiguration>) -> Snapshot {
        serde_json::from_value(serde_json::json!({"server_time_ms":ms,
            "pilots":[{"name":"owner","credits":0,"ore":0,"location":Location::Belt,
                "ship_class":ShipClass::Pioneer,"reactor_configuration":state}],
            "orders":[],"belt_ore":0,"ledger_sequence":0,"ledger_hash":"reactor-fixture"}))
        .unwrap()
    }
    fn compacting() -> PioneerReactorConfiguration {
        let mut state = PioneerReactorConfiguration::open("owned-vessel".into(), 1000);
        state.target_compact = true;
        state.duration_ms = 1200;
        state
    }
    #[test]
    fn authority_pause_stale_disconnect_and_absent_state_never_invent_motion() {
        let mut sampler = PoseSampler::default();
        let confirmed = snapshot(1000, Some(compacting()));
        sampler.observe(&confirmed, "owner", true, 1600);
        assert_eq!(sampler.fraction(), Some(0.5));
        for _ in 0..3 {
            sampler.observe(&confirmed, "owner", true, 1600);
        }
        assert_eq!(sampler.fraction(), Some(0.5));
        sampler.disconnect();
        assert_eq!(sampler.status(), "CONFIGURATION UNAVAILABLE");
        sampler.observe(&confirmed, "owner", false, 1800);
        assert_eq!(sampler.fraction(), Some(0.5));
        sampler.observe(&confirmed, "owner", true, 9000);
        assert_eq!(sampler.fraction(), Some(0.5));
        assert_eq!(sampler.status(), "CONFIGURATION UNAVAILABLE");
        sampler.observe(&snapshot(2200, Some(compacting())), "owner", true, 2200);
        assert_eq!(sampler.fraction(), Some(1.));
        sampler.observe(&snapshot(2300, None), "owner", true, 2300);
        assert_eq!(sampler.fraction(), None);
        assert_eq!(sampler.status(), "CONFIGURATION UNAVAILABLE");
    }
    #[test]
    fn equal_time_fifo_reversal_blockage_and_older_payload_are_explicit() {
        let mut sampler = PoseSampler::default();
        let a = compacting();
        let mut b = a.clone();
        b.from_fraction = 0.5;
        b.target_compact = false;
        b.started_at_ms = 1600;
        b.duration_ms = 600;
        sampler.observe(&snapshot(1600, Some(a.clone())), "owner", true, 1900);
        assert_eq!(sampler.fraction(), Some(0.75));
        sampler.observe(&snapshot(1600, Some(b.clone())), "owner", true, 1900);
        assert_eq!(sampler.fraction(), Some(0.25));
        assert_eq!(sampler.status(), "EXPANDING");
        sampler.observe(&snapshot(1600, Some(a.clone())), "owner", true, 1900);
        assert_eq!(
            sampler.fraction(),
            Some(0.75),
            "A→B→A is legitimate at one tick"
        );
        b.blocked = Some("occupied sweep".into());
        sampler.observe(&snapshot(2000, Some(b)), "owner", true, 2200);
        assert_eq!(sampler.fraction(), Some(0.5));
        assert_eq!(sampler.status(), "MOTION BLOCKED");
        sampler.observe(&snapshot(1500, Some(a)), "owner", true, 2200);
        assert_eq!(sampler.fraction(), Some(0.5));
        sampler.observe(&snapshot(1500, None), "owner", true, 2200);
        assert_eq!(
            sampler.fraction(),
            Some(0.5),
            "An older absent state is not a vessel-context switch"
        );
        sampler.observe(&snapshot(1500, Some(compacting())), "owner", true, 9000);
        assert_eq!(
            sampler.status(),
            "CONFIGURATION UNAVAILABLE",
            "Rejected old receipts cannot keep availability fresh"
        );
    }
    #[test]
    fn context_reset_and_invalid_component_cannot_reuse_another_reactor() {
        let mut sampler = PoseSampler::default();
        sampler.observe(&snapshot(2000, Some(compacting())), "owner", true, 2000);
        let mut other = PioneerReactorConfiguration::open("other-vessel".into(), 500);
        sampler.observe(&snapshot(500, Some(other.clone())), "owner", true, 500);
        assert_eq!(sampler.fraction(), Some(0.));
        assert_eq!(sampler.state().unwrap().vessel_id, "other-vessel");
        sampler.reset_session();
        assert_eq!(sampler.fraction(), None);
        sampler.observe(
            &snapshot(500, Some(other.clone())),
            "different-owner",
            true,
            500,
        );
        assert_eq!(sampler.fraction(), None);
        other.component_id = "unrecognized-component".into();
        sampler.observe(&snapshot(500, Some(other)), "owner", true, 500);
        assert_eq!(sampler.fraction(), None);
    }
    #[test]
    fn target_is_not_a_compact_or_open_completion_claim() {
        let mut sampler = PoseSampler::default();
        sampler.observe(&snapshot(1000, Some(compacting())), "owner", true, 1000);
        assert_eq!(sampler.status(), "COMPACTING");
        sampler.observe(&snapshot(1600, Some(compacting())), "owner", true, 1600);
        assert_eq!(sampler.status(), "COMPACTING");
        sampler.observe(&snapshot(2200, Some(compacting())), "owner", true, 2200);
        assert_eq!(sampler.status(), "COMPACT / TRAVEL");
        let mut opening = PioneerReactorConfiguration::open("owned-vessel".into(), 2200);
        opening.from_fraction = 1.;
        opening.duration_ms = 1200;
        sampler.observe(&snapshot(2200, Some(opening.clone())), "owner", true, 2200);
        assert_eq!(sampler.status(), "EXPANDING");
        sampler.observe(&snapshot(3400, Some(opening)), "owner", true, 3400);
        assert_eq!(sampler.status(), "EXPANDED / NORMAL");
    }
    #[test]
    fn schematic_reuses_shared_rigid_boxes_without_scale_or_core_changes() {
        let open = schematic(0.).unwrap();
        let compact = schematic(1.).unwrap();
        assert_eq!(open.core, compact.core);
        assert_eq!(open.bounds, compact.bounds);
        for fraction in [0., 0.25, 0.5, 0.75, 1.] {
            let sample = schematic(fraction).unwrap();
            assert_eq!(sample.bounds, open.bounds);
            for slot in 0..4 {
                assert_eq!(
                    sample.collars[slot],
                    arkline_protocol::reactor_configuration_box(slot, fraction).unwrap()
                );
                assert_eq!(sample.collars[slot].half_m, open.collars[slot].half_m);
                assert_eq!(sample.collars[slot].axes, open.collars[slot].axes);
            }
        }
        assert!(schematic(f64::NAN).is_none());
        assert!(schematic(1.01).is_none());
    }
}
