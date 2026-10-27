//! Flight definitions consumed by sampling and the controls reference.
//! Walking definitions below remain a preserved pending refactor; the walking sampler is unchanged.
use eframe::egui::{InputState, Key, PointerButton};

#[derive(Clone, Copy)]
pub(super) struct AxisBinding {
    pub(super) label: &'static str,
    pub(super) positive: Key,
    pub(super) negative: Key,
    pub(super) positive_alt: Option<Key>,
    pub(super) negative_alt: Option<Key>,
}
impl AxisBinding {
    pub(super) fn sample(&self, input: &InputState, include_pressed: bool) -> f64 {
        let down = |key| input.key_down(key) || (include_pressed && input.key_pressed(key));
        f64::from(down(self.positive) || self.positive_alt.is_some_and(down))
            - f64::from(down(self.negative) || self.negative_alt.is_some_and(down))
    }
    pub(super) fn keys_label(&self) -> String {
        let mut text = format!("{}/{}", key_label(self.positive), key_label(self.negative));
        if let (Some(positive), Some(negative)) = (self.positive_alt, self.negative_alt) {
            text.push_str(&format!(
                " ({}/{})",
                key_label(positive),
                key_label(negative)
            ));
        }
        text
    }
    #[cfg(test)]
    fn summary(&self) -> String {
        let mut text = format!(
            "{}/{} {}",
            key_label(self.positive),
            key_label(self.negative),
            self.label
        );
        if let (Some(positive), Some(negative)) = (self.positive_alt, self.negative_alt) {
            text.push_str(&format!(
                " ({}/{})",
                key_label(positive),
                key_label(negative)
            ));
        }
        text
    }
}

const fn axis(label: &'static str, positive: Key, negative: Key) -> AxisBinding {
    AxisBinding {
        label,
        positive,
        negative,
        positive_alt: None,
        negative_alt: None,
    }
}
pub(super) const FLIGHT_TRANSLATION: [AxisBinding; 3] = [
    axis("Forward / reverse thrust", Key::W, Key::S),
    axis("Up / down thrust", Key::Space, Key::C),
    axis("Right / left thrust", Key::D, Key::A),
];
pub(super) const FLIGHT_ROTATION: [AxisBinding; 3] = [
    axis("Roll right / left", Key::E, Key::Q),
    axis("Yaw left / right", Key::ArrowLeft, Key::ArrowRight),
    axis("Pitch up / down", Key::ArrowUp, Key::ArrowDown),
];
#[cfg(test)]
pub(super) const WALK_FORWARD: AxisBinding = AxisBinding {
    positive_alt: Some(Key::ArrowUp),
    negative_alt: Some(Key::ArrowDown),
    ..axis("move", Key::W, Key::S)
};
#[cfg(test)]
pub(super) const WALK_STRAFE: AxisBinding = axis("strafe", Key::E, Key::Q);
#[cfg(test)]
pub(super) const WALK_TURN: AxisBinding = AxisBinding {
    positive_alt: Some(Key::ArrowRight),
    negative_alt: Some(Key::ArrowLeft),
    ..axis("turn", Key::D, Key::A)
};
#[cfg(test)]
pub(super) const JUMP: Key = Key::Space;
pub(super) const INTERACT: Key = Key::F;
pub(super) const PANEL_SEARCH: Key = Key::K;
pub(super) const WORKSPACE_BACK: Key = Key::Escape;
/// First-person cursor ownership; does not replace editor or workspace Escape handling.
pub(super) const MOUSE_LOOK_RELEASE: Key = Key::Escape;
pub(super) const LOOK_BUTTON: PointerButton = PointerButton::Secondary;

pub(super) fn key_label(key: Key) -> String {
    match key {
        Key::ArrowUp => "↑".into(),
        Key::ArrowDown => "↓".into(),
        Key::ArrowLeft => "←".into(),
        Key::ArrowRight => "→".into(),
        _ => format!("{key:?}"),
    }
}
pub(super) fn look_button_label() -> &'static str {
    match LOOK_BUTTON {
        PointerButton::Secondary => "RMB",
        PointerButton::Primary => "LMB",
        PointerButton::Middle => "MMB",
        _ => "Pointer button",
    }
}
#[cfg(test)]
pub(super) fn walking_summary(sprint_supported: bool) -> String {
    let sprint = if sprint_supported {
        " · Shift sprint"
    } else {
        ""
    };
    format!(
        "{} · {} · {} · {} turn{} · {} jump",
        WALK_FORWARD.summary(),
        WALK_TURN.summary(),
        WALK_STRAFE.summary(),
        look_button_label(),
        sprint,
        key_label(JUMP)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_reference_axis_signs_alternates_and_or_sampling_match_input() {
        let ctx = eframe::egui::Context::default();
        for binding in FLIGHT_TRANSLATION
            .into_iter()
            .chain(FLIGHT_ROTATION)
            .chain([WALK_FORWARD, WALK_STRAFE, WALK_TURN])
        {
            for (key, expected) in [(binding.positive, 1.0), (binding.negative, -1.0)] {
                let _ = ctx.run(
                    eframe::egui::RawInput {
                        events: vec![eframe::egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: Default::default(),
                        }],
                        ..Default::default()
                    },
                    |ctx| {
                        ctx.input(|input| {
                            assert_eq!(binding.sample(input, false), expected);
                            assert_eq!(binding.sample(input, true), expected);
                        })
                    },
                );
                let _ = ctx.run(
                    eframe::egui::RawInput {
                        events: vec![eframe::egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed: false,
                            repeat: false,
                            modifiers: Default::default(),
                        }],
                        ..Default::default()
                    },
                    |_| {},
                );
            }
        }
        assert_eq!(FLIGHT_ROTATION[1].positive, Key::ArrowLeft);
        assert_eq!(WALK_TURN.positive, Key::D);
        let _ = ctx.run(
            eframe::egui::RawInput {
                events: vec![Key::W, Key::ArrowUp]
                    .into_iter()
                    .map(|key| eframe::egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Default::default(),
                    })
                    .collect(),
                ..Default::default()
            },
            |ctx| ctx.input(|input| assert_eq!(WALK_FORWARD.sample(input, false), 1.0)),
        );
    }
    #[test]
    fn controls_reference_labels_and_capability_help_derive_from_bindings() {
        assert_eq!(WALK_FORWARD.keys_label(), "W/S (↑/↓)");
        assert!(walking_summary(true).contains("W/S move"));
        assert!(walking_summary(true).contains("Shift sprint"));
        assert!(!walking_summary(false).contains("sprint"));
        assert!(walking_summary(false).contains(&format!("{} jump", key_label(JUMP))));
    }
}
