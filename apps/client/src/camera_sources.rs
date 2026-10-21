//! View-only source selection for the existing cockpit camera display.
use eframe::egui;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum CameraSource {
    #[default]
    Drones,
    /// Existing mining module identity: only slots 0 and 1 are offered.
    Turret(u8),
    AllTurrets,
}

impl CameraSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Drones => "Drone cameras",
            Self::Turret(0) => "Turret 1",
            Self::Turret(1) => "Turret 2",
            Self::Turret(_) => "Turret unavailable",
            Self::AllTurrets => "Both turrets",
        }
    }
}

/// Returns true for an explicit source choice, including reselecting a source.
/// This helper never selects a command target or issues a ship order.
pub(crate) fn selector(
    ui: &mut egui::Ui,
    source: &mut CameraSource,
    display_enabled: &mut bool,
) -> bool {
    ui.small("CAMERA / VIEW ONLY");
    ui.checkbox(display_enabled, "Camera screen");
    let mut selected = false;
    ui.horizontal_wrapped(|ui| {
        for option in [
            CameraSource::Drones,
            CameraSource::Turret(0),
            CameraSource::Turret(1),
            CameraSource::AllTurrets,
        ] {
            selected |= ui.selectable_value(source, option, option.label()).clicked();
        }
    });
    if !matches!(*source, CameraSource::Drones) {
        ui.small("Turret cameras follow the rendered barrel pose. Module targets and extraction cycles come from authority feedback.");
    }
    ui.small("Camera choices only change the view; ship and drone orders use their existing controls.");
    selected
}
