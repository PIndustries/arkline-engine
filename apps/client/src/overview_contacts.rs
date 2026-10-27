//! Search and stable contact identity over the real local snapshot inventory.
use super::*;
pub(super) fn identity(object: &SelectedObject) -> String {
    match object {
        SelectedObject::Station => "station".into(),
        SelectedObject::Belt => "belt".into(),
        SelectedObject::Planet(id) => format!("planet:{id}"),
        SelectedObject::Prop(id) => format!("prop:{id}"),
        SelectedObject::Asteroid(id) => format!("asteroid:{id}"),
        SelectedObject::Gate(id) => format!("gate:{id}"),
        SelectedObject::Pilot(name) => format!("pilot:{name}"),
    }
}
pub(super) fn matches(query: &str, object: &SelectedObject, name: &str, kind: &str) -> bool {
    let haystack = format!("{name} {kind} {}", identity(object)).to_lowercase();
    query
        .split_whitespace()
        .all(|word| haystack.contains(&word.to_lowercase()))
}
pub(super) fn button(
    ui: &mut egui::Ui,
    object: &SelectedObject,
    selected: bool,
    name: &str,
) -> egui::Response {
    // push_id alone does not stabilize egui's positional interaction IDs.
    // Bind the actual pointer/focus response to the contact's semantic identity.
    let id = ui.make_persistent_id(("contact-row", identity(object)));
    let padding = ui.spacing().button_padding;
    let text = egui::WidgetText::from(name).into_galley(
        ui,
        None,
        (ui.available_width().min(260.0) - 2.0 * padding.x).max(40.0),
        egui::TextStyle::Button,
    );
    let size = egui::vec2(
        text.size().x + 2.0 * padding.x,
        (text.size().y + 2.0 * padding.y).max(ui.spacing().interact_size.y),
    );
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let response = ui.interact(rect, id, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            name,
        )
    });
    if ui.is_rect_visible(rect) {
        let visual = ui.style().interact_selectable(&response, selected);
        if selected || response.hovered() || response.has_focus() {
            ui.painter().rect(
                rect,
                visual.corner_radius,
                visual.weak_bg_fill,
                visual.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        ui.painter()
            .galley(rect.min + padding, text, visual.text_color());
    }
    response
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overview_contacts_searches_real_identity_and_never_substitutes_selection() {
        let selected = SelectedObject::Asteroid(7);
        assert!(matches("ROCK asteroid:7", &selected, "Test rock", "Ore"));
        assert!(!matches("asteroid:8", &selected, "Test rock", "Ore"));
        assert!(!matches("no such contact", &selected, "Test rock", "Ore"));
        assert!(matches("", &selected, "Test rock", "Ore"));
        assert!(matches!(selected, SelectedObject::Asteroid(7)));
    }
    #[test]
    fn overview_contacts_reordering_between_press_and_release_cannot_select_another_row() {
        let ctx = egui::Context::default();
        let a = SelectedObject::Asteroid(7);
        let b = SelectedObject::Asteroid(8);
        let render = |reverse: bool, events| {
            let mut responses = Vec::new();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(600., 400.))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        for (object, name) in if reverse {
                            [(&b, "Beta"), (&a, "Alpha")]
                        } else {
                            [(&a, "Alpha"), (&b, "Beta")]
                        } {
                            let response = button(ui, object, false, name);
                            responses.push((
                                name,
                                response.id,
                                response.rect.center(),
                                response.clicked(),
                            ));
                        }
                    });
                },
            );
            responses
        };
        let first = render(false, vec![]);
        let point = first[0].2;
        let event = |pressed| {
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        render(false, event(true));
        let next = render(true, event(false));
        assert_eq!(first[0].1, next[1].1);
        assert_eq!(first[1].1, next[0].1);
        assert!(
            !next.iter().find(|row| row.0 == "Beta").unwrap().3,
            "release must not select the contact newly occupying the pressed row"
        );
    }
}
