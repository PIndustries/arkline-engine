//! Static ship, cockpit and station construction recipes.
//! Cache identity, source fallback, ordered parts and materials are shared by every caller.

use super::*;

// One recipe supplies the live enclosure/console draws and native inspection.
// These coordinates precede the class-specific cockpit_fit mount.
pub(super) fn cached_cockpit_parts(class: ShipClass) -> &'static [(ModelPart, Material)] {
    static MODELS: [std::sync::OnceLock<Vec<(ModelPart, Material)>>; 7] =
        [const { std::sync::OnceLock::new() }; 7];
    let index = arkline_protocol::ALL_SHIP_CLASSES
        .iter()
        .position(|c| *c == class)
        .unwrap();
    MODELS[index].get_or_init(|| {
        let mut parts = MaterialParts(Vec::new());
        let steel = Vec3::new(0.055, 0.08, 0.09);
        let glow = Vec3::new(0.08, 0.62, 0.43);
        // Forward is local +X; glazing is open between structural pillars.
        for side in [-1.0, 1.0] {
            if class != ShipClass::Pioneer {
                parts.draw(
                    0,
                    transform(
                        Vec3::new(0.88, 0.03, side * cockpit_span(class)),
                        Vec3::new(0.025, 0.62, 0.018),
                        Quat::from_rotation_x(side * 0.22),
                    ),
                    steel,
                    0.0,
                );
            }
            parts.draw(
                0,
                transform(
                    Vec3::new(0.63, -0.38, side * 0.42),
                    Vec3::new(0.35, 0.15, 0.13),
                    Quat::IDENTITY,
                ),
                steel,
                0.0,
            );
            for i in 0..6 {
                parts.draw(
                    0,
                    transform(
                        Vec3::new(0.77, -0.34, side * (0.22 + i as f32 * 0.04)),
                        Vec3::splat(0.013),
                        Quat::IDENTITY,
                    ),
                    glow,
                    0.8,
                );
            }
        }
        if class != ShipClass::Pioneer {
            parts.draw(
                0,
                transform(
                    Vec3::new(0.75, 0.57, 0.0),
                    Vec3::new(0.065, 0.035, 0.57),
                    Quat::IDENTITY,
                ),
                steel,
                0.0,
            );
            parts.draw(
                0,
                transform(
                    Vec3::new(0.82, -0.50, 0.0),
                    Vec3::new(0.22, 0.13, 0.58),
                    Quat::IDENTITY,
                ),
                steel,
                0.0,
            );
        }
        for (index, (center, size)) in cockpit_screens(class).into_iter().enumerate() {
            if index == 0 {
                continue;
            } // Transparent canopy telemetry preserves forward visibility.
            parts.draw(
                0,
                transform(center, size, Quat::IDENTITY),
                glow * 0.23,
                0.25,
            );
            parts.draw_material(
                0,
                transform(
                    center - Vec3::X * 0.025,
                    Vec3::new(0.006, size.y - 0.008, size.z - 0.008),
                    Quat::IDENTITY,
                ),
                Vec3::new(0.004, 0.012, 0.010),
                0.1,
                Material::Glass,
            );
        }
        parts.0
    })
}

struct MaterialParts(Vec<(ModelPart, Material)>);

impl MaterialParts {
    fn draw(&mut self, mesh: usize, model: Mat4, color: Vec3, emission: f32) {
        self.draw_material(mesh, model, color, emission, Material::PaintedHull);
    }
    fn draw_material(
        &mut self,
        mesh: usize,
        model: Mat4,
        color: Vec3,
        emission: f32,
        material: Material,
    ) {
        self.0.push(((mesh, model, color, emission), material));
    }
}

pub(super) fn ship_parts(class: ShipClass) -> Vec<ModelPart> {
    cached_ship_parts(class).to_vec()
}

pub(super) fn cached_ship_parts(class: ShipClass) -> &'static [ModelPart] {
    static MODELS: [std::sync::OnceLock<Vec<ModelPart>>; 7] =
        [const { std::sync::OnceLock::new() }; 7];
    MODELS[match class {
        ShipClass::Pioneer => 0,
        ShipClass::Prospector => 1,
        ShipClass::Hauler => 2,
        ShipClass::Vanguard => 3,
        ShipClass::Borealis => 4,
        ShipClass::Stratum => 5,
        ShipClass::Monolith => 6,
    }]
    .get_or_init(|| build_ship_parts(class))
}

pub(super) fn build_ship_parts(class: ShipClass) -> Vec<ModelPart> {
    if class == ShipClass::Pioneer {
        return match pioneer_asset() {
            Ok(parts) => parts
                .iter()
                .enumerate()
                .map(|(i, p)| (10 + i, Mat4::IDENTITY, p.color, p.emission))
                .collect(),
            Err(_) => pioneer_construction_model(),
        };
    }

    // Original procedural interpretation of the construction-fleet dossiers.
    // Model-space proportions are fitted to measured public bounds below.
    let mut parts = Parts(Vec::new());
    let graphite = Vec3::new(0.075, 0.095, 0.105);
    let plate = Vec3::new(0.15, 0.18, 0.19);
    let edge = Vec3::new(0.24, 0.28, 0.29);
    let green = Vec3::new(0.025, 0.28, 0.15);
    let amber = Vec3::new(0.72, 0.39, 0.09);
    let glass = Vec3::new(0.04, 0.30, 0.27);
    let twin = matches!(class, ShipClass::Prospector | ShipClass::Borealis);
    let sections = match class {
        ShipClass::Pioneer => 3,
        ShipClass::Prospector => 3,
        ShipClass::Hauler => 5,
        ShipClass::Vanguard => 5,
        ShipClass::Borealis => 4,
        ShipClass::Stratum => 6,
        ShipClass::Monolith => 8,
    };
    let industrial = matches!(class, ShipClass::Stratum | ShipClass::Monolith);
    // Box helper takes half-extents, preserving the existing mesh convention.
    let mut block = |p: Vec3, size: Vec3, color: Vec3, emission: f32| {
        let mesh = if emission == 0.0 && size.min_element() > 0.06 {
            8
        } else {
            0
        };
        parts.draw(mesh, transform(p, size, Quat::IDENTITY), color, emission);
    };
    if twin {
        // Two long extraction booms and a rear bridge; an open central channel.
        for side in [-1.0, 1.0] {
            block(
                Vec3::new(0.0, 0.0, side * 0.72),
                Vec3::new(1.85, 0.32, 0.29),
                graphite,
                0.0,
            );
            block(
                Vec3::new(1.72, 0.03, side * 0.72),
                Vec3::new(0.24, 0.24, 0.23),
                green,
                0.0,
            );
        }
        block(
            Vec3::new(-1.05, 0.04, 0.0),
            Vec3::new(0.48, 0.30, 0.80),
            plate,
            0.0,
        );
        // Crossbeam under the unchanged inner turret pedestals.
        block(
            Vec3::new(0.65, -0.02, 0.0),
            Vec3::new(0.38, 0.13, 0.76),
            graphite,
            0.0,
        );
    } else {
        block(
            Vec3::new(0.0, -0.13, 0.0),
            Vec3::new(1.78, 0.16, 0.32),
            graphite,
            0.0,
        );
        block(
            Vec3::new(1.50, 0.02, 0.0),
            Vec3::new(0.34, 0.29, 0.48),
            plate,
            0.0,
        );
        block(
            Vec3::new(1.49, 0.29, 0.0),
            Vec3::new(0.23, 0.10, 0.38),
            green,
            0.0,
        );
    }
    // Broad non-emissive identification bands and structural longitudinal rails.
    for side in [-1.0, 1.0] {
        for y in [-0.34, 0.34] {
            block(
                Vec3::new(-0.08, y, side * 0.94),
                Vec3::new(1.74, 0.035, 0.045),
                edge,
                0.0,
            );
        }
        for x in [-1.58, 1.20] {
            block(
                Vec3::new(x, 0.0, side * 0.89),
                Vec3::new(0.11, 0.36, 0.13),
                green,
                0.0,
            );
        }
        for i in 0..sections {
            let x = -1.35 + i as f32 * 2.55 / sections as f32;
            let half = 0.92 / sections as f32;
            let z = if twin { 0.92 } else { 0.60 };
            block(
                Vec3::new(x, 0.0, side * z),
                Vec3::new(
                    half,
                    if industrial { 0.23 } else { 0.29 },
                    if twin { 0.11 } else { 0.28 },
                ),
                if class == ShipClass::Hauler {
                    plate
                } else {
                    graphite * 1.45
                },
                0.0,
            );
            block(
                Vec3::new(x - half, 0.0, side * (z + 0.12)),
                Vec3::new(0.018, 0.33, 0.035),
                edge,
                0.0,
            );
            // Alternate lamps leave a bounded geometry budget for cargo tops.
            if i % 2 == 0 {
                block(
                    Vec3::new(x, -0.27, side * (z + 0.12)),
                    Vec3::new(0.033, 0.012, 0.018),
                    amber,
                    0.45,
                );
            }
        }
        // Engine housings and recessed luminous exhaust faces; no drill shapes.
        block(
            Vec3::new(-1.94, -0.02, side * 0.59),
            Vec3::new(0.27, 0.26, 0.24),
            graphite,
            0.0,
        );
        block(
            Vec3::new(-2.215, -0.02, side * 0.59),
            Vec3::new(0.012, 0.17, 0.16),
            green,
            0.9,
        );
    }
    if !twin && class != ShipClass::Hauler {
        for i in 0..sections {
            let x = -1.35 + i as f32 * 2.55 / sections as f32;
            block(
                Vec3::new(x, if industrial { 0.15 } else { 0.22 }, 0.0),
                Vec3::new(
                    0.88 / sections as f32,
                    0.12,
                    if industrial { 0.48 } else { 0.40 },
                ),
                if i % 3 == 0 { green } else { plate },
                0.0,
            );
        }
    }
    let tower_x = if industrial {
        -1.18
    } else if twin {
        -0.85
    } else {
        0.86
    };
    let tower_levels = match class {
        ShipClass::Monolith => 3,
        ShipClass::Stratum | ShipClass::Vanguard => 2,
        _ => 1,
    };
    for level in 0..tower_levels {
        let y = 0.42 + level as f32 * 0.13;
        let scale = 1.0 - level as f32 * 0.18;
        block(
            Vec3::new(tower_x, y, 0.0),
            Vec3::new(0.34 * scale, 0.065, 0.33 * scale),
            graphite,
            0.0,
        );
        block(
            Vec3::new(tower_x + 0.345 * scale, y, 0.0),
            Vec3::new(0.008, 0.027, 0.25 * scale),
            glass,
            0.25,
        );
    }
    if class == ShipClass::Hauler {
        // Exposed top cargo cassettes differ from the extraction-boom silhouettes.
        for i in 0..4 {
            block(
                Vec3::new(-1.12 + i as f32 * 0.48, 0.35, 0.0),
                Vec3::new(0.20, 0.12, 0.52),
                plate,
                0.0,
            );
        }
    }
    if class == ShipClass::Vanguard {
        block(
            Vec3::new(-0.35, 0.39, 0.0),
            Vec3::new(1.1, 0.07, 0.31),
            green,
            0.0,
        );
    }
    // End of helper borrow: diagonal structural braces use rotated cube beams.
    if industrial || class == ShipClass::Borealis {
        for side in [-1.0, 1.0] {
            for i in 0..sections {
                let x = -1.40 + i as f32 * 2.60 / sections as f32;
                let a = Vec3::new(x, -0.28, side * if twin { 1.07 } else { 0.96 });
                let b = Vec3::new(
                    x + 2.35 / sections as f32,
                    0.28,
                    side * if twin { 1.07 } else { 0.96 },
                );
                let delta = b - a;
                parts.draw(
                    0,
                    transform(
                        (a + b) * 0.5,
                        Vec3::new(delta.length() * 0.5, 0.018, 0.022),
                        Quat::from_rotation_arc(Vec3::X, delta.normalize()),
                    ),
                    edge,
                    0.0,
                );
            }
        }
    }
    // Blunt armored brows: optical turrets remain the only extraction tools.
    for z in if twin { vec![-0.72, 0.72] } else { vec![0.0] } {
        parts.draw(
            0,
            transform(
                Vec3::new(1.71, 0.26, z),
                Vec3::new(0.23, 0.04, if twin { 0.22 } else { 0.54 }),
                Quat::from_rotation_z(-0.16),
            ),
            green,
            0.0,
        );
    }
    // Fit measured vertices, including all appendages, to the public meter spec.
    let (min, max) = parts_bounds(&parts.0);
    // Keep a solid service core at the fitted center for all six reaction axes.
    // Segmented cargo bays must not leave an axis nozzle firing from empty space.
    // Its lower edge joins the keel/crossbeam; all vertices stay inside the
    // measured envelope, preserving the public dimensions and turret supports.
    let center = (min + max) * 0.5;
    let bottom = -0.20;
    let top = center.y + 0.10;
    parts.draw(
        0,
        transform(
            Vec3::new(center.x, (bottom + top) * 0.5, center.z),
            Vec3::new(0.50, (top - bottom) * 0.5, 0.34),
            Quat::IDENTITY,
        ),
        graphite,
        0.0,
    );
    let spec = class.spec();
    let dimensions = Vec3::new(
        spec.length_m as f32,
        spec.height_m as f32,
        spec.width_m as f32,
    ) / 100.0;
    let fit =
        Mat4::from_scale(dimensions / (max - min)) * Mat4::from_translation(-(min + max) * 0.5);
    for (mesh, model, color, emission) in &mut parts.0 {
        *model = fit * *model;
        // Roles assigned inside the ship builder, never inferred from scene colors.
        if *emission > 0.0 {
            *mesh |= EMISSION_ROLE;
        } else if *emission == 0.0 {
            if *color == graphite || *color == plate {
                *mesh |= HULL_ROLE;
            } else if *color == green {
                *mesh |= TRIM_ROLE;
            }
        }
    }
    parts.0
}

/// Enclosed cutter silhouette derived from Pioneer's concept: sloped bridge,
/// shoulder armor, recessed service cassettes and rear drive housings.
/// Still a procedural exterior; no claim of fitted structural/interior geometry.
pub(super) fn pioneer_construction_model() -> Vec<ModelPart> {
    let mut p = Parts(Vec::new());
    let dark = Vec3::new(0.055, 0.070, 0.075);
    let steel = Vec3::new(0.15, 0.18, 0.19);
    let trim = Vec3::new(0.25, 0.28, 0.29);
    let green = Vec3::new(0.018, 0.20, 0.085);
    let mut part = |mesh, pos, size, color, emission| {
        p.draw(mesh, transform(pos, size, Quat::IDENTITY), color, emission)
    };
    part(9, Vec3::ZERO, Vec3::new(1.92, 0.42, 0.87), dark, 0.0);
    // Nose is a continuous faceted pressure-body reservation, not stacked crates.
    part(
        9,
        Vec3::new(1.20, 0.10, 0.0),
        Vec3::new(0.80, 0.37, 0.88),
        dark,
        0.0,
    );
    part(
        8,
        Vec3::new(1.992, 0.10, 0.0),
        Vec3::new(0.014, 0.19, 0.47),
        trim,
        0.0,
    );
    part(
        8,
        Vec3::new(2.010, 0.11, 0.0),
        Vec3::new(0.008, 0.15, 0.42),
        Vec3::new(0.025, 0.16, 0.17),
        0.0,
    );
    for z in [-0.22, 0.0, 0.22] {
        part(
            0,
            Vec3::new(2.020, 0.11, z),
            Vec3::new(0.006, 0.16, 0.008),
            trim,
            0.0,
        );
    }
    for side in [-1.0, 1.0] {
        // Armored green collars and inset side service panels.
        for x in [-1.40, 0.82] {
            part(
                8,
                Vec3::new(x, 0.0, side * 0.78),
                Vec3::new(0.22, 0.44, 0.21),
                green,
                0.0,
            );
            part(
                8,
                Vec3::new(x, 0.03, side * 0.988),
                Vec3::new(0.14, 0.29, 0.018),
                steel,
                0.0,
            );
            for y in [-0.21, 0.21] {
                part(
                    0,
                    Vec3::new(x, y, side * 1.01),
                    Vec3::new(0.09, 0.012, 0.012),
                    trim,
                    0.0,
                );
            }
        }
        for x in [-0.81, -0.18, 0.45] {
            part(
                8,
                Vec3::new(x, -0.015, side * 0.871),
                Vec3::new(0.285, 0.285, 0.055),
                steel,
                0.0,
            );
            part(
                8,
                Vec3::new(x, -0.015, side * 0.928),
                Vec3::new(0.24, 0.235, 0.014),
                dark,
                0.0,
            );
            part(
                0,
                Vec3::new(x, -0.292, side * 0.937),
                Vec3::new(0.17, 0.006, 0.010),
                Vec3::new(0.55, 0.30, 0.06),
                0.4,
            );
        }
        // Short service rails stay attached to the hull instead of framing empty air.
        for y in [-0.32, 0.32] {
            part(
                8,
                Vec3::new(-0.16, y, side * 0.92),
                Vec3::new(0.92, 0.018, 0.025),
                trim,
                0.0,
            );
        }
        part(
            8,
            Vec3::new(-1.85, -0.02, side * 0.55),
            Vec3::new(0.30, 0.28, 0.27),
            dark,
            0.0,
        );
        part(
            8,
            Vec3::new(-2.15, -0.02, side * 0.55),
            Vec3::new(0.016, 0.22, 0.21),
            steel,
            0.0,
        );
        part(
            8,
            Vec3::new(-2.17, -0.02, side * 0.55),
            Vec3::new(0.008, 0.16, 0.15),
            green,
            0.75,
        );
        part(
            8,
            Vec3::new(1.35, 0.36, side * 0.55),
            Vec3::new(0.33, 0.06, 0.19),
            green,
            0.0,
        );
        part(
            0,
            Vec3::new(1.72, -0.18, side * 0.57),
            Vec3::new(0.04, 0.012, 0.020),
            Vec3::new(0.50, 0.65, 0.63),
            0.7,
        );
    }
    for x in [-0.88, -0.25, 0.38] {
        part(
            8,
            Vec3::new(x, 0.423, 0.0),
            Vec3::new(0.285, 0.018, 0.57),
            steel,
            0.0,
        );
        part(
            8,
            Vec3::new(x, 0.444, 0.0),
            Vec3::new(0.235, 0.006, 0.48),
            dark,
            0.0,
        );
    }
    let (min, max) = parts_bounds(&p.0);
    let spec = ShipClass::Pioneer.spec();
    let extent = Vec3::new(
        spec.length_m as f32,
        spec.height_m as f32,
        spec.width_m as f32,
    ) / 100.0;
    let fit = Mat4::from_scale(extent / (max - min)) * Mat4::from_translation(-(min + max) * 0.5);
    for (_, m, _, _) in &mut p.0 {
        *m = fit * *m;
    }
    // Keep the emergency procedural model on the same editor finish path as
    // the authored GLB. The normal model receives these tags while importing
    // its named material roles; this fallback has no named materials.
    for (mesh, _, color, emission) in &mut p.0 {
        if *emission > 0.0 {
            *mesh |= EMISSION_ROLE;
        } else if *color == dark || *color == steel {
            *mesh |= HULL_ROLE;
        } else if *color == trim || *color == green {
            *mesh |= TRIM_ROLE;
        }
    }
    p.0
}

pub(super) fn station_parts(space: Space) -> Vec<(usize, Mat4, Vec3, f32)> {
    let mut parts = Parts(Vec::new());
    let center = Mat4::from_translation(space.scene(STATION_CENTER_POSITION));
    let steel = Vec3::new(0.19, 0.25, 0.29);
    let window = Vec3::new(0.14, 0.43, 0.37);
    let radius = STATION_DIAMETER_M as f32 / 200.0;
    let half_height = STATION_HEIGHT_M as f32 / 200.0;
    parts.draw(
        0,
        center * Mat4::from_scale(Vec3::new(11.0, half_height, 11.0)),
        steel,
        0.0,
    );
    // Habitat rings: the large rings sit above/below the docking plane. Their
    // 1.12 torus extent exactly matches the advertised twenty-kilometer span.
    for (y, r) in [(-15.0, radius / 1.12), (0.0, 68.0), (15.0, radius / 1.12)] {
        parts.draw(
            1,
            center * transform(Vec3::new(0.0, y, 0.0), Vec3::new(r, 5.0, r), Quat::IDENTITY),
            steel,
            0.0,
        );
        for i in 0..32 {
            let angle = i as f32 * std::f32::consts::TAU / 32.0;
            let rotation = Quat::from_rotation_y(angle);
            let p = rotation * Vec3::new(r, 0.0, 0.0) + Vec3::Y * (y + 0.7);
            parts.draw(
                0,
                center * transform(p, Vec3::new(1.5, 0.12, 0.4), rotation),
                window,
                0.55,
            );
        }
        for i in 0..8 {
            let rotation = Quat::from_rotation_y(i as f32 * std::f32::consts::TAU / 8.0);
            parts.draw(
                0,
                center
                    * transform(
                        rotation * Vec3::new(r * 0.52, y, 0.0),
                        Vec3::new(r * 0.44, 0.65, 1.0),
                        rotation,
                    ),
                steel * 0.8,
                0.0,
            );
        }
    }
    for i in 0..8 {
        let y = -24.0 + i as f32 * 6.5;
        parts.draw(
            1,
            center * transform(Vec3::Y * y, Vec3::new(22.0, 3.0, 22.0), Quat::IDENTITY),
            steel * 1.15,
            0.0,
        );
        for side in [-1.0, 1.0] {
            parts.draw(
                0,
                center
                    * transform(
                        Vec3::new(0.0, y, side * 24.0),
                        Vec3::new(5.0, 1.7, 4.0),
                        Quat::IDENTITY,
                    ),
                steel,
                0.0,
            );
        }
    }
    // Recessed +Z-facing port. Nothing occupies the authoritative berth volume
    // x/y +/-450m, z+1000m around STATION_POSITION, or the docked port point.
    let port = Mat4::from_translation(space.scene(STATION_POSITION));
    for y in [-5.5, 5.5] {
        parts.draw(
            0,
            port * transform(
                Vec3::new(-6.0, y, -6.0),
                Vec3::new(6.0, 0.4, 6.0),
                Quat::IDENTITY,
            ),
            steel,
            0.0,
        );
        parts.draw(
            0,
            port * transform(
                Vec3::new(-5.8, y, 0.1),
                Vec3::new(5.5, 0.07, 0.07),
                Quat::IDENTITY,
            ),
            window,
            0.8,
        );
    }
    parts.draw(
        0,
        port * transform(
            Vec3::new(-12.0, 0.0, -6.0),
            Vec3::new(0.4, 5.5, 6.0),
            Quat::IDENTITY,
        ),
        steel,
        0.0,
    );
    for i in 0..8 {
        parts.draw(
            0,
            port * transform(
                Vec3::new(-1.0 - i as f32 * 1.3, -5.0, -2.0),
                Vec3::new(0.1, 0.1, 1.3),
                Quat::IDENTITY,
            ),
            Vec3::new(0.48, 0.32, 0.07),
            0.4,
        );
    }
    parts.0
}
