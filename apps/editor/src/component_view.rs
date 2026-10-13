//! Candidate engineering viewport. These boxes visualize the local design document;
//! they are not meshes, installed equipment, or authoritative runtime geometry.
use crate::engineering_document::{Component, PartClass, HALF_EXTENT};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

#[derive(Debug, Clone)]
pub struct ComponentView {
    yaw: f32,
    pitch: f32,
    zoom: f32,
}
impl Default for ComponentView {
    fn default() -> Self {
        Self {
            yaw: -0.65,
            pitch: 0.42,
            zoom: 1.0,
        }
    }
}
#[derive(Clone, Copy, Debug)]
struct Projected {
    point: Pos2,
    depth: f32,
}

impl ComponentView {
    /// Draw a rotatable, zoomable candidate-box view and update `selected` from clicks.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        components: &[Component],
        selected: &mut Option<u64>,
    ) {
        ui.label("Candidate design boxes · 72 × 18 × 38 m envelope");
        ui.small("Illustrative axis-aligned boxes only; no real equipment meshes or authoritative runtime geometry.");
        let width = ui.available_width().max(160.0);
        let height = (ui.available_height() - 30.0).clamp(180.0, 520.0);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::click_and_drag());
        if response.dragged() {
            let delta = response.drag_delta();
            self.yaw = (self.yaw + delta.x * 0.008).rem_euclid(std::f32::consts::TAU);
            self.pitch = (self.pitch - delta.y * 0.008).clamp(-1.1, 1.1);
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll != 0.0 {
                self.zoom = (self.zoom * (scroll * 0.001).exp()).clamp(0.55, 2.5);
            }
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, Color32::from_rgb(11, 22, 26));
        draw_envelope(&painter, rect, self);
        let valid: Vec<_> = components.iter().filter(|c| valid_box(c)).collect();
        if valid.len() != components.len() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Correct invalid component dimensions to display boxes",
                egui::FontId::proportional(13.0),
                Color32::LIGHT_RED,
            );
        }
        let project = |p: [f64; 3]| project_point(p, rect, self);
        // Back-to-front ordering makes overlapping candidate boxes legible.
        let mut ordered = valid.clone();
        ordered.sort_by(|a, b| {
            project(a.center_m)
                .depth
                .total_cmp(&project(b.center_m).depth)
        });
        for c in ordered {
            let pts = corners(c).map(&project);
            let color = if *selected == Some(c.id) {
                Color32::from_rgb(193, 255, 183)
            } else {
                match c.class {
                    PartClass::Structure => Color32::from_rgb(57, 182, 112),
                    PartClass::Equipment => Color32::from_rgb(75, 157, 220),
                }
            };
            draw_box(&painter, &pts, color, *selected == Some(c.id));
            painter.text(
                project(c.center_m).point,
                egui::Align2::CENTER_CENTER,
                c.id.to_string(),
                egui::FontId::monospace(11.0),
                color,
            );
        }
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                *selected = valid
                    .iter()
                    .filter_map(|c| {
                        let projected = corners(c).map(&project);
                        let hull = convex_hull(projected.iter().map(|p| p.point).collect());
                        point_in_polygon(pos, &hull).then(|| (c.id, project(c.center_m).depth))
                    })
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|hit| hit.0);
            }
        }
        painter.text(
            rect.left_top() + Vec2::splat(8.0),
            egui::Align2::LEFT_TOP,
            "Y ↑",
            egui::FontId::monospace(10.0),
            Color32::GRAY,
        );
        painter.text(
            rect.right_bottom() - Vec2::splat(8.0),
            egui::Align2::RIGHT_BOTTOM,
            "Drag: orbit · scroll: zoom · click: select",
            egui::FontId::monospace(10.0),
            Color32::LIGHT_GRAY,
        );
    }
}
fn valid_box(c: &Component) -> bool {
    c.center_m
        .iter()
        .chain(c.extent_m.iter())
        .all(|v| v.is_finite())
        && c.extent_m
            .iter()
            .enumerate()
            .all(|(a, e)| *e >= 0.001 && c.center_m[a].abs() + *e / 2. <= HALF_EXTENT[a])
}
fn project_point(p: [f64; 3], rect: Rect, state: &ComponentView) -> Projected {
    let (sy, cy) = state.yaw.sin_cos();
    let (sp, cp) = state.pitch.sin_cos();
    let x = p[0] as f32;
    let y = p[1] as f32;
    let z = p[2] as f32;
    let xr = x * cy + z * sy;
    let zr = -x * sy + z * cy;
    let yr = y * cp - zr * sp;
    let depth = zr * cp + y * sp;
    let scale = (rect.width() / 104.0).min(rect.height() / 60.0) * state.zoom;
    Projected {
        point: egui::pos2(rect.center().x + xr * scale, rect.center().y - yr * scale),
        depth,
    }
}
fn corners(c: &Component) -> [[f64; 3]; 8] {
    std::array::from_fn(|i| {
        std::array::from_fn(|a| {
            c.center_m[a]
                + if (i >> a) & 1 == 0 {
                    -c.extent_m[a] / 2.
                } else {
                    c.extent_m[a] / 2.
                }
        })
    })
}
fn draw_box(p: &egui::Painter, pts: &[Projected; 8], color: Color32, selected: bool) {
    for i in 0..8 {
        for bit in [1, 2, 4] {
            if i & bit == 0 {
                let a = pts[i];
                let b = pts[i | bit];
                let near = (a.depth + b.depth) * 0.5 > 0.;
                let stroke = if selected {
                    Stroke::new(2.2, color)
                } else {
                    Stroke::new(
                        if near { 1.5 } else { 0.8 },
                        if near {
                            color
                        } else {
                            color.gamma_multiply(0.5)
                        },
                    )
                };
                p.line_segment([a.point, b.point], stroke);
            }
        }
    }
}
fn draw_envelope(p: &egui::Painter, rect: Rect, state: &ComponentView) {
    let dummy = Component {
        id: 0,
        name: String::new(),
        class: PartClass::Structure,
        material: String::new(),
        mass_kg: 1.,
        center_m: [0.; 3],
        extent_m: [
            HALF_EXTENT[0] * 2.,
            HALF_EXTENT[1] * 2.,
            HALF_EXTENT[2] * 2.,
        ],
    };
    let pts = corners(&dummy).map(|v| project_point(v, rect, state));
    for i in 0..8 {
        for bit in [1, 2, 4] {
            if i & bit == 0 {
                p.line_segment(
                    [pts[i].point, pts[i | bit].point],
                    Stroke::new(1., Color32::from_gray(75)),
                );
            }
        }
    }
}
fn convex_hull(mut points: Vec<Pos2>) -> Vec<Pos2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup_by(|a, b| (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01);
    if points.len() <= 2 {
        return points;
    }
    fn cross(o: Pos2, a: Pos2, b: Pos2) -> f32 {
        (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
    }
    let mut lo = Vec::new();
    for p in &points {
        while lo.len() >= 2 && cross(lo[lo.len() - 2], lo[lo.len() - 1], *p) <= 0. {
            lo.pop();
        }
        lo.push(*p);
    }
    let mut hi = Vec::new();
    for p in points.iter().rev() {
        while hi.len() >= 2 && cross(hi[hi.len() - 2], hi[hi.len() - 1], *p) <= 0. {
            hi.pop();
        }
        hi.push(*p);
    }
    lo.pop();
    hi.pop();
    lo.extend(hi);
    lo
}
fn point_in_polygon(p: Pos2, poly: &[Pos2]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[j];
        if ((a.y > p.y) != (b.y > p.y)) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;
    fn component(id: u64, center: [f64; 3]) -> Component {
        Component {
            id,
            name: id.to_string(),
            class: PartClass::Structure,
            material: String::new(),
            mass_kg: 1.,
            center_m: center,
            extent_m: [4.; 3],
        }
    }
    #[test]
    fn center_projects_to_view_center() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(500., 300.));
        let p = project_point([0.; 3], rect, &ComponentView::default());
        assert!((p.point.x - 250.).abs() < 0.01 && (p.point.y - 150.).abs() < 0.01);
    }
    #[test]
    fn nearer_overlapping_box_wins_pick_depth() {
        let state = ComponentView::default();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(500., 300.));
        let a = component(1, [0., 0., -2.]);
        let b = component(2, [0., 0., 2.]);
        let click = project_point([0.; 3], rect, &state).point;
        let winner = [&a, &b]
            .iter()
            .filter_map(|c| {
                let h = convex_hull(
                    corners(c)
                        .iter()
                        .map(|p| project_point(*p, rect, &state).point)
                        .collect(),
                );
                point_in_polygon(click, &h)
                    .then(|| (c.id, project_point(c.center_m, rect, &state).depth))
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        assert_eq!(winner.0, 2);
    }
    #[test]
    fn hull_contains_projected_box_center() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(500., 300.));
        let c = component(7, [4., 2., -3.]);
        let s = ComponentView::default();
        let h = convex_hull(
            corners(&c)
                .iter()
                .map(|p| project_point(*p, rect, &s).point)
                .collect(),
        );
        assert!(point_in_polygon(
            project_point(c.center_m, rect, &s).point,
            &h
        ));
    }
    #[test]
    fn invalid_box_is_not_renderable() {
        let mut c = component(9, [0.; 3]);
        c.center_m[1] = f64::NAN;
        assert!(!valid_box(&c));
    }
}
