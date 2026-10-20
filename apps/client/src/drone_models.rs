//! Cached, presentation-only drone geometry in the existing flight scene units.
//!
//! +X is forward, +Y is up and +Z is starboard. Static components are baked into
//! five color/emission batches, shared by the runtime and inspection renderer.
use super::*;
use arkline_protocol::DroneClass;
use std::sync::OnceLock;

pub(super) const CLASSES: [DroneClass; 3] =
    [DroneClass::Recon, DroneClass::Mining, DroneClass::Attack];

pub(super) const fn class_index(class: DroneClass) -> usize {
    match class {
        DroneClass::Recon => 0,
        DroneClass::Mining => 1,
        DroneClass::Attack => 2,
    }
}

pub(super) struct DroneBatch {
    pub(super) vertices: Vec<Vertex>,
    pub(super) color: Vec3,
    pub(super) emission: f32,
}

pub(super) struct DroneModel {
    pub(super) batches: Vec<DroneBatch>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))] // Native inspection bounds.
    pub(super) min: Vec3,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(super) max: Vec3,
}

pub(super) fn model(class: DroneClass) -> &'static DroneModel {
    static MODELS: OnceLock<[DroneModel; 3]> = OnceLock::new();
    &MODELS.get_or_init(|| {
        let primitives = Primitives::new();
        CLASSES.map(|class| build(class, &primitives))
    })[class_index(class)]
}

const ARMOR: usize = 0;
const RECESS: usize = 1;
const FRAME: usize = 2;
const ACCENT: usize = 3;
const LIGHT: usize = 4;

struct Primitives {
    cube: Vec<Vertex>,
    armor: Vec<Vertex>,
    shell: Vec<Vertex>,
    cylinder: Vec<Vertex>,
}

impl Primitives {
    fn new() -> Self {
        Self {
            cube: cube(),
            armor: armor_block(),
            shell: industrial_shell(),
            cylinder: cylinder_x(10),
        }
    }
}

struct Builder {
    batches: Vec<DroneBatch>,
}

impl Builder {
    fn new(class: DroneClass) -> Self {
        let identification = match class {
            DroneClass::Recon => Vec3::new(0.32, 0.50, 0.46),
            DroneClass::Mining => Vec3::new(0.66, 0.43, 0.12),
            DroneClass::Attack => Vec3::new(0.50, 0.19, 0.13),
        };
        Self {
            batches: [
                (Vec3::new(0.12, 0.145, 0.16), 0.02),
                (Vec3::new(0.045, 0.055, 0.06), 0.0),
                (Vec3::new(0.055, 0.30, 0.205), 0.025),
                (identification, 0.02),
                (Vec3::new(0.14, 0.72, 0.50), 0.7),
            ]
            .into_iter()
            .map(|(color, emission)| DroneBatch {
                vertices: Vec::new(),
                color,
                emission,
            })
            .collect(),
        }
    }

    fn part(&mut self, source: &[Vertex], role: usize, at: Vec3, half: Vec3, rotation: Quat) {
        let matrix = Mat4::from_scale_rotation_translation(half, rotation, at);
        let output = &mut self.batches[role].vertices;
        for face in source.chunks_exact(3) {
            // Recompute each flat face normal after scaling/rotation. This is
            // equivalent to its inverse-transpose transform, including the
            // nonuniform scales used by armor plates and optical apertures.
            triangle(
                output,
                matrix.transform_point3(face[0].p),
                matrix.transform_point3(face[1].p),
                matrix.transform_point3(face[2].p),
            );
        }
    }

    fn finish(self) -> DroneModel {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for vertex in self.batches.iter().flat_map(|batch| &batch.vertices) {
            min = min.min(vertex.p);
            max = max.max(vertex.p);
        }
        DroneModel {
            batches: self.batches,
            min,
            max,
        }
    }
}

fn build(class: DroneClass, p: &Primitives) -> DroneModel {
    let mut b = Builder::new(class);
    common_frame(&mut b, p);
    match class {
        DroneClass::Recon => recon(&mut b, p),
        DroneClass::Mining => mining(&mut b, p),
        DroneClass::Attack => attack(&mut b, p),
    }
    b.finish()
}

fn common_frame(b: &mut Builder, p: &Primitives) {
    let q = Quat::IDENTITY;
    // An exposed cross member and lower tray keep the side pods visibly joined
    // to the core rather than reading as three disconnected colored boxes.
    b.part(
        &p.cube,
        FRAME,
        Vec3::new(-0.004, -0.0034, 0.0),
        Vec3::new(0.0025, 0.0013, 0.027),
        q,
    );
    b.part(
        &p.armor,
        ARMOR,
        Vec3::new(-0.003, -0.0078, 0.0),
        Vec3::new(0.015, 0.0012, 0.0065),
        q,
    );
    b.part(
        &p.armor,
        ARMOR,
        Vec3::new(-0.021, -0.0003, 0.0),
        Vec3::new(0.004, 0.0052, 0.011),
        q,
    );
    for side in [-1.0, 1.0] {
        b.part(
            &p.armor,
            FRAME,
            Vec3::new(-0.003, -0.0015, side * 0.012),
            Vec3::new(0.017, 0.0013, 0.0012),
            q,
        );
        b.part(
            &p.cube,
            RECESS,
            Vec3::new(-0.0242, -0.0002, side * 0.0062),
            Vec3::new(0.0008, 0.0027, 0.0032),
            q,
        );
        b.part(
            &p.cube,
            LIGHT,
            Vec3::new(-0.0249, -0.0002, side * 0.0062),
            Vec3::new(0.0001, 0.0018, 0.0021),
            q,
        );
    }
}

fn recon(b: &mut Builder, p: &Primitives) {
    let q = Quat::IDENTITY;
    // A low center body, forward optical head and isolated outboard sensors.
    b.part(
        &p.shell,
        RECESS,
        Vec3::new(-0.003, -0.0007, 0.0),
        Vec3::new(0.016, 0.0046, 0.0115),
        q,
    );
    for x in [-0.012, -0.002, 0.008] {
        b.part(
            &p.armor,
            ARMOR,
            Vec3::new(x, 0.0037, 0.0),
            Vec3::new(0.0043, 0.0014, 0.0088),
            q,
        );
    }
    b.part(
        &p.armor,
        ARMOR,
        Vec3::new(0.017, 0.0015, 0.0),
        Vec3::new(0.006, 0.0054, 0.0072),
        q,
    );
    b.part(
        &p.armor,
        FRAME,
        Vec3::new(0.0215, 0.0015, 0.0),
        Vec3::new(0.0014, 0.0048, 0.0064),
        q,
    );
    b.part(
        &p.cylinder,
        RECESS,
        Vec3::new(0.0232, 0.0015, 0.0),
        Vec3::new(0.0014, 0.0041, 0.0041),
        q,
    );
    b.part(
        &p.cylinder,
        LIGHT,
        Vec3::new(0.0245, 0.0015, 0.0),
        Vec3::new(0.0005, 0.0029, 0.0029),
        q,
    );
    for side in [-1.0, 1.0] {
        b.part(
            &p.shell,
            ARMOR,
            Vec3::new(-0.004, -0.0003, side * 0.0254),
            Vec3::new(0.012, 0.0032, 0.0046),
            q,
        );
        b.part(
            &p.cube,
            ACCENT,
            Vec3::new(-0.010, 0.0028, side * 0.0254),
            Vec3::new(0.0022, 0.0004, 0.0035),
            q,
        );
        b.part(
            &p.cylinder,
            LIGHT,
            Vec3::new(0.0078, -0.0003, side * 0.0254),
            Vec3::new(0.0004, 0.0015, 0.0021),
            q,
        );
        b.part(
            &p.cube,
            ACCENT,
            Vec3::new(0.013, 0.0052, side * 0.0052),
            Vec3::new(0.0025, 0.0007, 0.0007),
            q,
        );
    }
    b.part(
        &p.cube,
        FRAME,
        Vec3::new(-0.007, 0.0058, 0.0),
        Vec3::new(0.0006, 0.0022, 0.0006),
        q,
    );
    b.part(
        &p.armor,
        ACCENT,
        Vec3::new(-0.007, 0.0085, 0.0),
        Vec3::new(0.003, 0.0005, 0.0008),
        q,
    );
}

fn mining(b: &mut Builder, p: &Primitives) {
    let q = Quat::IDENTITY;
    // A protected central work head and broad braced pods. This static housing
    // suggests the existing mining laser; it adds no drill or moving mechanism.
    b.part(
        &p.shell,
        RECESS,
        Vec3::new(-0.002, -0.0003, 0.0),
        Vec3::new(0.019, 0.0064, 0.013),
        q,
    );
    b.part(
        &p.armor,
        ARMOR,
        Vec3::new(0.016, -0.001, 0.0),
        Vec3::new(0.009, 0.0063, 0.0085),
        q,
    );
    b.part(
        &p.armor,
        RECESS,
        Vec3::new(0.0238, -0.001, 0.0),
        Vec3::new(0.0012, 0.004, 0.0059),
        q,
    );
    b.part(
        &p.cylinder,
        LIGHT,
        Vec3::new(0.0247, -0.001, 0.0),
        Vec3::new(0.0003, 0.002, 0.0028),
        q,
    );
    b.part(
        &p.armor,
        ACCENT,
        Vec3::new(-0.004, 0.0075, 0.0),
        Vec3::new(0.011, 0.0015, 0.007),
        q,
    );
    for side in [-1.0, 1.0] {
        b.part(
            &p.armor,
            ARMOR,
            Vec3::new(-0.006, 0.0031, side * 0.0105),
            Vec3::new(0.011, 0.0031, 0.0032),
            q,
        );
        b.part(
            &p.cylinder,
            FRAME,
            Vec3::new(0.009, -0.001, side * 0.0092),
            Vec3::new(0.0013, 0.003, 0.003),
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        );
        b.part(
            &p.armor,
            FRAME,
            Vec3::new(0.005, -0.0048, side * 0.016),
            Vec3::new(0.0105, 0.0013, 0.002),
            Quat::from_rotation_y(side * -0.47),
        );
        b.part(
            &p.armor,
            ARMOR,
            Vec3::new(-0.003, -0.0005, side * 0.025),
            Vec3::new(0.013, 0.0047, 0.005),
            q,
        );
        for x in [-0.010, 0.004] {
            b.part(
                &p.cube,
                ACCENT,
                Vec3::new(x, 0.0042, side * 0.025),
                Vec3::new(0.0015, 0.0004, 0.0042),
                q,
            );
            b.part(
                &p.cube,
                RECESS,
                Vec3::new(x, -0.0048, side * 0.025),
                Vec3::new(0.0018, 0.0007, 0.0038),
                q,
            );
        }
        b.part(
            &p.cube,
            FRAME,
            Vec3::new(-0.004, 0.0064, side * 0.0094),
            Vec3::new(0.010, 0.001, 0.001),
            q,
        );
        b.part(
            &p.cube,
            LIGHT,
            Vec3::new(0.008, 0.0045, side * 0.0133),
            Vec3::new(0.0013, 0.0008, 0.001),
            q,
        );
    }
}

fn attack(b: &mut Builder, p: &Primitives) {
    let q = Quat::IDENTITY;
    // Paired forward housings and stepped shields read differently from the
    // recon optics and the mining cradle without changing attack simulation.
    b.part(
        &p.shell,
        RECESS,
        Vec3::new(-0.004, -0.0005, 0.0),
        Vec3::new(0.017, 0.0059, 0.011),
        q,
    );
    b.part(
        &p.armor,
        ARMOR,
        Vec3::new(-0.005, 0.0066, 0.0),
        Vec3::new(0.013, 0.0024, 0.004),
        q,
    );
    b.part(
        &p.cube,
        ACCENT,
        Vec3::new(-0.002, 0.0087, 0.0),
        Vec3::new(0.006, 0.0003, 0.0015),
        q,
    );
    for side in [-1.0, 1.0] {
        b.part(
            &p.armor,
            ARMOR,
            Vec3::new(-0.005, 0.003, side * 0.008),
            Vec3::new(0.011, 0.0031, 0.004),
            q,
        );
        b.part(
            &p.shell,
            ARMOR,
            Vec3::new(0.013, -0.001, side * 0.007),
            Vec3::new(0.011, 0.0036, 0.0045),
            q,
        );
        b.part(
            &p.cylinder,
            RECESS,
            Vec3::new(0.0237, -0.001, side * 0.007),
            Vec3::new(0.0013, 0.0029, 0.0029),
            q,
        );
        b.part(
            &p.cylinder,
            LIGHT,
            Vec3::new(0.0247, -0.001, side * 0.007),
            Vec3::new(0.0003, 0.0013, 0.0013),
            q,
        );
        b.part(
            &p.shell,
            RECESS,
            Vec3::new(-0.005, -0.0005, side * 0.025),
            Vec3::new(0.015, 0.0045, 0.005),
            q,
        );
        b.part(
            &p.armor,
            ARMOR,
            Vec3::new(0.009, 0.0015, side * 0.024),
            Vec3::new(0.010, 0.004, 0.005),
            q,
        );
        b.part(
            &p.cube,
            ACCENT,
            Vec3::new(0.010, 0.0053, side * 0.024),
            Vec3::new(0.004, 0.0003, 0.0038),
            q,
        );
        for x in [-0.013, 0.003] {
            b.part(
                &p.cube,
                FRAME,
                Vec3::new(x, -0.0004, side * 0.025),
                Vec3::new(0.0012, 0.0048, 0.005),
                q,
            );
        }
    }
}

/// A closed, low-resolution cylinder whose axis follows +X. Flat side normals
/// retain mechanical facets; explicit cap centers avoid degenerate pole faces.
fn cylinder_x(sides: usize) -> Vec<Vertex> {
    let mut vertices = Vec::with_capacity(sides * 12);
    let ring = |x: f32, i: usize| {
        let angle = i as f32 * std::f32::consts::TAU / sides as f32;
        Vec3::new(x, angle.cos(), angle.sin())
    };
    for i in 0..sides {
        let (a, b) = (ring(-1.0, i), ring(-1.0, i + 1));
        let (c, d) = (ring(1.0, i + 1), ring(1.0, i));
        triangle(&mut vertices, a, b, c);
        triangle(&mut vertices, a, c, d);
        triangle(&mut vertices, -Vec3::X, b, a);
        triangle(&mut vertices, Vec3::X, d, c);
    }
    vertices
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fingerprint(model: &DroneModel) -> u64 {
        let mut hash = 0xcbf29ce484222325_u64;
        for batch in &model.batches {
            for value in batch
                .color
                .to_array()
                .into_iter()
                .chain([batch.emission])
                .chain(
                    batch
                        .vertices
                        .iter()
                        .flat_map(|v| v.p.to_array().into_iter().chain(v.n.to_array())),
                )
            {
                hash ^= u64::from(value.to_bits());
                hash = hash.wrapping_mul(0x100000001b3);
            }
        }
        hash
    }

    #[test]
    fn geometry_is_finite_nondegenerate_and_has_transformed_unit_normals() {
        for class in CLASSES {
            for batch in &model(class).batches {
                assert_eq!(batch.vertices.len() % 3, 0);
                assert!(batch.color.is_finite());
                assert!(batch.emission.is_finite() && (0.0..=1.0).contains(&batch.emission));
                for face in batch.vertices.chunks_exact(3) {
                    let cross = (face[1].p - face[0].p).cross(face[2].p - face[0].p);
                    assert!(cross.length_squared() > 1e-20, "degenerate {class:?} face");
                    for vertex in face {
                        assert!(vertex.p.is_finite() && vertex.n.is_finite());
                        assert!((vertex.n.length() - 1.0).abs() < 1e-5);
                        assert!(vertex.n.dot(cross.normalize()) > 0.99999);
                    }
                }
            }
        }
    }

    #[test]
    fn all_classes_retain_legacy_render_envelope_and_bounded_batches() {
        let limit = Vec3::new(0.025, 0.009, 0.030);
        for class in CLASSES {
            let model = model(class);
            assert!(model.batches.len() <= 5);
            assert!(model.batches.iter().all(|batch| !batch.vertices.is_empty()));
            let triangles: usize = model
                .batches
                .iter()
                .map(|batch| batch.vertices.len() / 3)
                .sum();
            assert!((100..=1800).contains(&triangles), "{class:?}: {triangles}");
            assert!(
                (model.min + limit).abs().max_element() < 1e-6,
                "{class:?} min {:?}",
                model.min
            );
            assert!(
                (model.max - limit).abs().max_element() < 1e-6,
                "{class:?} max {:?}",
                model.max
            );
            for vertex in model.batches.iter().flat_map(|batch| &batch.vertices) {
                assert!((vertex.p.abs() - limit).max_element() <= 1e-6);
            }
        }
    }

    #[test]
    fn classes_are_distinct_deterministic_and_borrow_the_same_cache() {
        let primitives = Primitives::new();
        let hashes = CLASSES.map(|class| {
            assert!(std::ptr::eq(model(class), model(class)));
            assert_eq!(
                fingerprint(model(class)),
                fingerprint(&build(class, &primitives))
            );
            fingerprint(model(class))
        });
        assert_ne!(hashes[0], hashes[1]);
        assert_ne!(hashes[0], hashes[2]);
        assert_ne!(hashes[1], hashes[2]);
        // Distinction includes authored geometry, rather than only class tint.
        let counts = CLASSES.map(|class| {
            model(class)
                .batches
                .iter()
                .map(|batch| batch.vertices.len())
                .sum::<usize>()
        });
        assert_ne!(counts[0], counts[1]);
        assert_ne!(counts[1], counts[2]);
        assert_ne!(counts[0], counts[2]);
    }
}
