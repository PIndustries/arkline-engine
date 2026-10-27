//! View-only source selection for the existing cockpit camera display.
use eframe::egui;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum CameraSource {
    #[default]
    Drones,
    /// Existing mining module identity: only slots 0 and 1 are offered.
    Turret(u8),
    AllTurrets,
    BayOverview,
    BayRack(u8),
}

impl CameraSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Drones => "Drone cameras",
            Self::Turret(0) => "Turret 1",
            Self::Turret(1) => "Turret 2",
            Self::Turret(_) => "Turret unavailable",
            Self::AllTurrets => "Both turrets",
            Self::BayOverview => "Bay overview",
            Self::BayRack(0) => "Recon rack",
            Self::BayRack(1) => "Mining rack",
            Self::BayRack(2) => "Combat rack",
            Self::BayRack(_) => "Bay camera unavailable",
        }
    }
}

/// Returns true for an explicit source choice, including reselecting a source.
/// This helper never selects a command target or issues a ship order.
pub(crate) fn selector(
    ui: &mut egui::Ui,
    source: &mut CameraSource,
    display_enabled: &mut bool,
    bay_available: bool,
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
            CameraSource::BayOverview,
            CameraSource::BayRack(0),
            CameraSource::BayRack(1),
            CameraSource::BayRack(2),
        ] {
            let available = bay_available
                || !matches!(option, CameraSource::BayOverview | CameraSource::BayRack(_));
            selected |= ui
                .add_enabled_ui(available, |ui| {
                    ui.selectable_value(source, option, option.label())
                        .clicked()
                })
                .inner;
        }
    });
    if matches!(*source, CameraSource::Turret(_) | CameraSource::AllTurrets) {
        ui.small("Turret cameras follow the rendered barrel pose. Module targets and extraction cycles come from authority feedback.");
    }
    if matches!(
        *source,
        CameraSource::BayOverview | CameraSource::BayRack(_)
    ) {
        ui.small("Fixed interior bay cameras show actual registered stock and handling machinery; they do not launch or recall drones.");
    }
    if !bay_available {
        ui.small("Physical bay cameras unavailable in this world or source registration.");
    }
    ui.small(
        "Camera choices only change the view; ship and drone orders use their existing controls.",
    );
    selected
}
