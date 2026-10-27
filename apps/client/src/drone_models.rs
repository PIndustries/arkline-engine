//! Cached, presentation-only drone geometry in the existing flight scene units.
//!
//! +X is forward, +Y is up and +Z is starboard. Static components are baked into
//! five semantic material batches (plus the Mining work head), shared by runtime
//! and inspection rendering. Roughness/metallicity are real shader parameters.
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
    pub(super) roughness: f32,
    pub(super) metallic: f32,
}

pub(super) struct DroneModel {
    pub(super) batches: Vec<DroneBatch>,
    /// Mining head-only trailing batches, baked at neutral body coordinates.
    /// Runtime rotates these about MINING_WORK_HEAD_PIVOT_M, never the body.
    pub(super) work_head_batch_start: Option<usize>,
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

/// Actual visible aperture plane in drone-local body metres (+X forward).
/// Rendering geometry below uses 0.01 scene units per metre; consumers must
/// transform this mount through the same full drone attitude as the body.
pub(super) const MINING_WORK_HEAD_PIVOT_M: [f64; 3] = [2.0, -0.1, 0.0];
pub(super) const MINING_WORK_HEAD_LOCAL_M: [f64; 3] = [0.45, 0.0, 0.0];
pub(super) const MINING_WORK_HEAD_RADIUS_M: f64 = 0.49;
pub(super) const MINING_WORK_HEAD_M: [f64; 3] = [2.45, -0.1, 0.0];

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
        let mining = class == DroneClass::Mining;
        Self {
            batches: [
                (
                    if mining {
                        Vec3::new(0.24, 0.27, 0.29)
                    } else {
                        Vec3::new(0.12, 0.145, 0.16)
                    },
                    0.02,
                ),
                (Vec3::new(0.045, 0.055, 0.06), 0.0),
                (
                    if mining {
                        Vec3::new(0.38, 0.42, 0.43)
                    } else {
                        Vec3::new(0.055, 0.30, 0.205)
                    },
                    0.025,
                ),
                (identification, 0.02),
                (Vec3::new(0.14, 0.72, 0.50), 0.7),
            ]
            .into_iter()
            .enumerate()
            .map(|(role, (color, emission))| DroneBatch {
                vertices: Vec::new(),
                color,
                emission,
                roughness: [0.68, 0.90, 0.28, 0.52, 0.24][role],
                metallic: [0.18, 0.02, 0.85, 0.12, 0.0][role],
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
            work_head_batch_start: None,
            min,
            max,
        }
    }
}

fn build(class: DroneClass, p: &Primitives) -> DroneModel {
    let mut b = Builder::new(class);
    common_frame(&mut b, p, class == DroneClass::Mining);
    match class {
        DroneClass::Recon => recon(&mut b, p),
        DroneClass::Mining => mining(&mut b, p),
        DroneClass::Attack => attack(&mut b, p),
    }
    let head_start = if class == DroneClass::Mining {
        let start = b.batches.len();
        let mut head = Builder::new(class);
        mining_work_head(&mut head, p);
        b.batches.extend(head.batches);
        Some(start)
    } else {
        None
    };
    let mut model = b.finish();
    model.work_head_batch_start = head_start;
    model
}

fn common_frame(b: &mut Builder, p: &Primitives, mining: bool) {
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
    if mining {
        mining_cargo_socket(b, p);
    } else {
        b.part(
            &p.armor,
            ARMOR,
            Vec3::new(-0.021, -0.0003, 0.0),
            Vec3::new(0.004, 0.0052, 0.011),
            q,
        );
    }
    for side in [-1.0, 1.0] {
        b.part(
            &p.armor,
            FRAME,
            Vec3::new(-0.003, -0.0015, side * 0.012),
            Vec3::new(0.017, 0.0013, 0.0012),
            q,
        );
        // Mining tail lamps must not overlap the cargo flange or share their
        // visible plane with an opaque backing. The housing meets the lens back
        // atX−2.48m; the unchanged lens front still establishesX−2.50m bounds.
        let tail_z = if mining { 0.0069 } else { 0.0062 };
        let (housing_x, housing_half_x) = if mining {
            (-0.0241, 0.0007)
        } else {
            (-0.0242, 0.0008)
        };
        b.part(
            &p.cube,
            RECESS,
            Vec3::new(housing_x, -0.0002, side * tail_z),
            Vec3::new(housing_half_x, 0.0027, 0.0032),
            q,
        );
        b.part(
            &p.cube,
            LIGHT,
            Vec3::new(-0.0249, -0.0002, side * tail_z),
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

/// Physical tail socket, distinct from the forward mining tool and side exhaust.
/// The registered receiver has a0.44m square insert in this0.50m opening. Its
/// tip reaches0.08m inside the mouth; the actual backplate lies0.30m inside.
/// No cargo quantity, hatch animation or material fill is inferred here.
pub(super) const MINING_CARGO_SOCKET_M: [f64; 3] = [-2.5, 0.0, 0.0];
const MINING_CARGO_SOCKET_HALF_OPENING_M: f32 = 0.25;
const MINING_CARGO_SOCKET_HALF_FLANGE_M: f32 = 0.36;
const MINING_CARGO_SOCKET_DEPTH_M: f32 = 0.30;

fn mining_cargo_socket(b: &mut Builder, p: &Primitives) {
    let q = Quat::IDENTITY;
    let mouth = MINING_CARGO_SOCKET_M[0] as f32 * 0.01;
    let inner = MINING_CARGO_SOCKET_HALF_OPENING_M * 0.01;
    let flange_back = mouth + 0.001;
    let bin_front = -0.017;
    let bin_center = (flange_back + bin_front) * 0.5;
    let bin_half_depth = (bin_front - flange_back) * 0.5;
    // Replace the old solid center cap, not a decorative ring pasted over it.
    // Four bin walls leave the entire receiver insertion volume genuinely clear.
    for side in [-1.0, 1.0] {
        b.part(
            &p.cube,
            ARMOR,
            Vec3::new(bin_center, side * (0.0052 + inner) * 0.5, 0.0),
            Vec3::new(bin_half_depth, (0.0052 - inner) * 0.5, 0.011),
            q,
        );
        b.part(
            &p.cube,
            ARMOR,
            Vec3::new(bin_center, 0.0, side * (0.011 + inner) * 0.5),
            Vec3::new(bin_half_depth, inner, (0.011 - inner) * 0.5),
            q,
        );
    }
    let outer = MINING_CARGO_SOCKET_HALF_FLANGE_M * 0.01;
    for side in [-1.0, 1.0] {
        b.part(
            &p.cube,
            FRAME,
            Vec3::new(mouth + 0.0005, side * (outer + inner) * 0.5, 0.0),
            Vec3::new(0.0005, (outer - inner) * 0.5, outer),
            q,
        );
        b.part(
            &p.cube,
            FRAME,
            Vec3::new(mouth + 0.0005, 0.0, side * (outer + inner) * 0.5),
            Vec3::new(0.0005, inner, (outer - inner) * 0.5),
            q,
        );
        // Broad ochre keying bars sit against the bin's actual rear face,
        // outside the flange: no floating strips in front of an empty gap.
        b.part(
            &p.cube,
            ACCENT,
            Vec3::new(flange_back - 0.00012, side * 0.00435, 0.0),
            Vec3::new(0.00012, 0.00035, 0.0085),
            q,
        );
    }
    b.part(
        &p.cube,
        RECESS,
        Vec3::new(
            mouth + MINING_CARGO_SOCKET_DEPTH_M * 0.01 + 0.0001,
            0.0,
            0.0,
        ),
        Vec3::new(0.0001, inner, inner),
        q,
    );
}

fn mining(b: &mut Builder, p: &Primitives) {
    let q = Quat::IDENTITY;
    let ring = annulus_x(10, 0.58);
    let optic = cylinder_x(8);
    // Layered pressure core, exposed metallic spine and replaceable ochre lid.
    // All authored details stay inside the unchanged registered drone envelope.
    b.part(
        &p.shell,
        RECESS,
        Vec3::new(-0.002, -0.0003, 0.0),
        Vec3::new(0.016, 0.0064, 0.013),
        q,
    );
    b.part(
        &p.armor,
        ARMOR,
        Vec3::new(0.008, -0.001, 0.0),
        Vec3::new(0.007, 0.0063, 0.0085),
        q,
    );
    b.part(
        &p.armor,
        ACCENT,
        Vec3::new(-0.004, 0.0075, 0.0),
        Vec3::new(0.011, 0.0015, 0.007),
        q,
    );
    // Fixed optics/brows lie outside the complete swept tool-head sphere.
    for side in [-1.0, 1.0] {
        b.part(
            &p.cube,
            ARMOR,
            Vec3::new(0.0245, 0.0048, side * 0.0064),
            Vec3::new(0.0005, 0.0006, 0.0014),
            q,
        );
        // Offset stereo inspection optics remain visibly separate from the tool.
        b.part(
            &optic,
            RECESS,
            Vec3::new(0.0215, 0.0038, side * 0.0064),
            Vec3::new(0.001, 0.0014, 0.0014),
            q,
        );
        b.part(
            &optic,
            LIGHT,
            Vec3::new(0.02255, 0.0038, side * 0.0064),
            Vec3::new(0.00005, 0.00075, 0.00075),
            q,
        );
        b.part(
            &p.armor,
            ARMOR,
            Vec3::new(-0.006, 0.0031, side * 0.0105),
            Vec3::new(0.011, 0.0031, 0.0032),
            q,
        );
        b.part(
            &p.armor,
            FRAME,
            Vec3::new(0.005, -0.0048, side * 0.016),
            Vec3::new(0.0105, 0.0013, 0.002),
            Quat::from_rotation_y(side * -0.47),
        );
        // Broad pods preserve the familiar class silhouette and exact outer width.
        b.part(
            &p.armor,
            ARMOR,
            Vec3::new(-0.003, -0.0005, side * 0.025),
            Vec3::new(0.013, 0.0047, 0.005),
            q,
        );
        // Aft-facing open nozzle collars with dark, recessed throats. These are
        // static authored housings, not a new propulsion/energy simulation.
        b.part(
            &ring,
            FRAME,
            Vec3::new(-0.0195, -0.0005, side * 0.025),
            Vec3::new(0.0035, 0.0037, 0.0037),
            q,
        );
        b.part(
            &p.cylinder,
            RECESS,
            Vec3::new(-0.0165, -0.0005, side * 0.025),
            Vec3::new(0.0002, 0.0022, 0.0022),
            q,
        );
        // Recessed service hatch surrounds, proud panels and tiny bare-metal
        // wear bars avoid coplanar decal flicker without claiming damage state.
        b.part(
            &p.cube,
            RECESS,
            Vec3::new(-0.003, 0.00425, side * 0.025),
            Vec3::new(0.0077, 0.00015, 0.0034),
            q,
        );
        b.part(
            &p.cube,
            ACCENT,
            Vec3::new(-0.003, 0.00448, side * 0.025),
            Vec3::new(0.0068, 0.00008, 0.0028),
            q,
        );
        for x in [-0.010, 0.004] {
            b.part(
                &p.cube,
                FRAME,
                Vec3::new(x, 0.0046, side * 0.025),
                Vec3::new(0.00045, 0.0002, 0.0033),
                q,
            );
            // Four real underside contact shoes meet the registered bottom
            // plane. They are contact geometry, not animated clamps or legs.
            b.part(
                &p.cube,
                FRAME,
                Vec3::new(x, -0.0067, side * 0.025),
                Vec3::new(0.0026, 0.0015, 0.0034),
                q,
            );
            b.part(
                &p.cube,
                RECESS,
                Vec3::new(x, -0.0086, side * 0.025),
                Vec3::new(0.0028, 0.0004, 0.0036),
                q,
            );
        }
        // Outer-side service seam and chevron-like class markings.
        b.part(
            &p.cube,
            RECESS,
            Vec3::new(-0.003, -0.0004, side * 0.0299),
            Vec3::new(0.007, 0.0022, 0.0001),
            q,
        );
        for x in [-0.007, -0.003, 0.001] {
            b.part(
                &p.cube,
                ACCENT,
                Vec3::new(x, -0.0004, side * 0.02999),
                Vec3::new(0.00065, 0.0018, 0.00001),
                Quat::from_rotation_z(-0.35),
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
    // Two lid seams and a short exposed edge give scale at normal bay distance.
    for x in [-0.010, 0.002] {
        b.part(
            &p.cube,
            RECESS,
            Vec3::new(x, 0.00891, 0.0),
            Vec3::new(0.0002, 0.00009, 0.0052),
            q,
        );
    }
    b.part(
        &p.cube,
        FRAME,
        Vec3::new(-0.013, 0.0088, 0.002),
        Vec3::new(0.0012, 0.0002, 0.002),
        q,
    );
}

/// Mining head is a small spherical cartridge around a real body-local pivot.
/// Every vertex lies inside its0.49m sphere, so any visual aim stays inside the
/// existing registered drone envelope. The emitter and mesh use one aim transform.
fn mining_work_head(b: &mut Builder, p: &Primitives) {
    let q = Quat::IDENTITY;
    let pivot = Vec3::from_array(MINING_WORK_HEAD_PIVOT_M.map(|v| (v * 0.01) as f32));
    let aperture = pivot + Vec3::from_array(MINING_WORK_HEAD_LOCAL_M.map(|v| (v * 0.01) as f32));
    let ball = ball_x(8);
    b.part(
        &ball,
        ARMOR,
        pivot,
        Vec3::splat((MINING_WORK_HEAD_RADIUS_M * 0.01) as f32),
        q,
    );
    // The ball's central forward hemisphere is physically open around the bore.
    // ball_x omits its forward cap; this sleeve/lens completes the tool cartridge.
    b.part(
        &annulus_x(8, 0.58),
        FRAME,
        pivot + Vec3::X * 0.0029,
        Vec3::new(0.0016, 0.0018, 0.0018),
        q,
    );
    let optic = cylinder_x(8);
    b.part(
        &optic,
        RECESS,
        aperture - Vec3::X * 0.0014,
        Vec3::new(0.0002, 0.00105, 0.00105),
        q,
    );
    b.part(
        &optic,
        LIGHT,
        aperture - Vec3::X * 0.0011,
        Vec3::new(0.0001, 0.0008, 0.0008),
        q,
    );
    b.part(
        &p.cube,
        ACCENT,
        pivot + Vec3::new(-0.001, 0.00426, 0.),
        Vec3::new(0.001, 0.00004, 0.0012),
        q,
    );
}

/// Faceted spherical rear housing, open on +X where the tool sleeve attaches.
fn ball_x(sides: usize) -> Vec<Vertex> {
    let mut out = Vec::new();
    let radial = (0.75_f32).sqrt();
    let ring = |x: f32, i: usize| {
        let a = i as f32 * std::f32::consts::TAU / sides as f32;
        Vec3::new(x, radial * a.cos(), radial * a.sin())
    };
    for i in 0..sides {
        let a = ring(-0.5, i);
        let b = ring(-0.5, i + 1);
        let c = ring(0.5, i + 1);
        let d = ring(0.5, i);
        triangle(&mut out, -Vec3::X, b, a);
        triangle(&mut out, a, b, c);
        triangle(&mut out, a, c, d);
        // Front annulus closes from the spherical rim to the narrow sleeve,
        // leaving the axial passage open rather than putting a cap in the ray.
        let e = Vec3::new(0.5, d.y * 0.42, d.z * 0.42);
        let f = Vec3::new(0.5, c.y * 0.42, c.z * 0.42);
        triangle(&mut out, d, c, f);
        triangle(&mut out, d, f, e);
    }
    out
}

/// Closed annular sleeve, with both inner and outer walls and open central bore.
/// Unlike a capped cylinder it does not obscure the recessed optical/nozzle face.
fn annulus_x(sides: usize, inner: f32) -> Vec<Vertex> {
    let mut vertices = Vec::with_capacity(sides * 24);
    let ring = |x: f32, radius: f32, i: usize| {
        let a = i as f32 * std::f32::consts::TAU / sides as f32;
        Vec3::new(x, radius * a.cos(), radius * a.sin())
    };
    for i in 0..sides {
        let a = ring(-1.0, 1.0, i);
        let b = ring(-1.0, 1.0, i + 1);
        let c = ring(1.0, 1.0, i + 1);
        let d = ring(1.0, 1.0, i);
        let e = ring(-1.0, inner, i);
        let f = ring(-1.0, inner, i + 1);
        let g = ring(1.0, inner, i + 1);
        let h = ring(1.0, inner, i);
        for face in [
            [a, b, c],
            [a, c, d],
            [e, h, g],
            [e, g, f],
            [a, e, f],
            [a, f, b],
            [d, c, g],
            [d, g, h],
        ] {
            triangle(&mut vertices, face[0], face[1], face[2]);
        }
    }
    vertices
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
    // Recess beneath the identification strip; coplanar color batches flicker.
    b.part(
        &p.armor,
        ARMOR,
        Vec3::new(-0.005, 0.0064, 0.0),
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
        // Keep the retaining bands proud of the pod skin on both side faces.
        b.part(
            &p.shell,
            RECESS,
            Vec3::new(-0.005, -0.0005, side * 0.025),
            Vec3::new(0.015, 0.0045, 0.0048),
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
                .chain([batch.emission, batch.roughness, batch.metallic])
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
                assert!(batch.roughness.is_finite() && (0.08..=1.0).contains(&batch.roughness));
                assert!(batch.metallic.is_finite() && (0.0..=1.0).contains(&batch.metallic));
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
            assert!(model.batches.len() <= if class == DroneClass::Mining { 10 } else { 5 });
            assert_eq!(
                model.work_head_batch_start,
                (class == DroneClass::Mining).then_some(5)
            );
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
    fn mining_work_head_is_a_recessed_aperture_at_the_shared_mount() {
        let mine = model(DroneClass::Mining);
        let mount = Vec3::from_array(MINING_WORK_HEAD_M.map(|v| (v * 0.01) as f32));
        assert!((mount - Vec3::new(0.0245, -0.001, 0.)).length() < 1e-8);
        // A ray along the exact tool axis must hit the recessed emissive lens,
        // not a capped collar or an opaque nose covering its working end.
        let origin = mount + Vec3::X * 0.01;
        let direction = -Vec3::X;
        let mut nearest = None;
        for (role, batch) in mine.batches.iter().enumerate() {
            for face in batch.vertices.chunks_exact(3) {
                let edge1 = face[1].p - face[0].p;
                let edge2 = face[2].p - face[0].p;
                let h = direction.cross(edge2);
                let det = edge1.dot(h);
                if det.abs() < 1e-12 {
                    continue;
                }
                let delta = origin - face[0].p;
                let u = delta.dot(h) / det;
                let q = delta.cross(edge1);
                let v = direction.dot(q) / det;
                let t = edge2.dot(q) / det;
                if (0.0..=1.0).contains(&u)
                    && v >= 0.0
                    && u + v <= 1.00001
                    && t >= 0.0
                    && nearest.is_none_or(|(old, _)| t < old)
                {
                    nearest = Some((t, role));
                }
            }
        }
        let (distance, role) = nearest.expect("visible work-head lens");
        assert_eq!(role, mine.work_head_batch_start.unwrap() + LIGHT);
        assert!((distance - 0.0110).abs() < 1e-6);
        // Collar's forward ring surrounds, but does not cap, that aperture.
        assert!(mine.batches[mine.work_head_batch_start.unwrap() + FRAME]
            .vertices
            .iter()
            .any(|v| (v.p.x - mount.x).abs() < 1e-7 && (v.p.y - mount.y).abs() > 0.001));
    }

    #[test]
    fn mining_contact_shoes_and_open_collars_stay_inside_registered_bounds() {
        let mine = model(DroneClass::Mining);
        for x in [-0.010_f32, 0.004] {
            for side in [-1.0_f32, 1.0] {
                let centre = Vec3::new(x, -0.009, side * 0.025);
                let contacts = mine.batches[RECESS]
                    .vertices
                    .iter()
                    .filter(|v| {
                        (v.p.y - centre.y).abs() < 1e-7
                            && (v.p.x - centre.x).abs() <= 0.00281
                            && (v.p.z - centre.z).abs() <= 0.00361
                    })
                    .count();
                assert!(
                    contacts >= 6,
                    "missing real underside contact shoe {centre:?}"
                );
            }
        }
        let ring = annulus_x(10, 0.58);
        assert_eq!(ring.len(), 10 * 8 * 3);
        for v in ring {
            let radius = Vec3::new(0., v.p.y, v.p.z).length();
            assert!((0.57999..=1.00001).contains(&radius));
        }
    }

    #[test]
    fn mining_head_full_sphere_sweep_preserves_registered_envelope() {
        let mine = model(DroneClass::Mining);
        let pivot = Vec3::from_array(MINING_WORK_HEAD_PIVOT_M.map(|v| (v * 0.01) as f32));
        let limit = Vec3::new(0.025, 0.009, 0.030);
        for vertex in mine.batches[mine.work_head_batch_start.unwrap()..]
            .iter()
            .flat_map(|b| &b.vertices)
        {
            let local = vertex.p - pivot;
            assert!(local.length() <= MINING_WORK_HEAD_RADIUS_M as f32 * 0.01 + 1e-7);
            for yaw in -12..=12 {
                for pitch in -6..=6 {
                    let q = Quat::from_rotation_y(yaw as f32 * std::f32::consts::PI / 12.)
                        * Quat::from_rotation_z(pitch as f32 * std::f32::consts::PI / 12.);
                    assert!(((pivot + q * local).abs() - limit).max_element() <= 1e-7);
                }
            }
        }
        // This is a geometric bound, not an assertion that every backward beam
        // direction clears the static hull; runtime checks actual occlusion.
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

    fn first_tail_hit(model: &DroneModel, origin: Vec3) -> Option<(f32, usize)> {
        let direction = Vec3::X;
        let mut nearest = None;
        for (role, batch) in model.batches.iter().enumerate() {
            for face in batch.vertices.chunks_exact(3) {
                let e1 = face[1].p - face[0].p;
                let e2 = face[2].p - face[0].p;
                let h = direction.cross(e2);
                let det = e1.dot(h);
                if det.abs() < 1e-12 {
                    continue;
                }
                let delta = origin - face[0].p;
                let u = delta.dot(h) / det;
                let q = delta.cross(e1);
                let v = direction.dot(q) / det;
                let t = e2.dot(q) / det;
                if (0.0..=1.0).contains(&u)
                    && v >= 0.0
                    && u + v <= 1.00001
                    && t >= 0.0
                    && nearest.is_none_or(|(old, _)| t < old)
                {
                    nearest = Some((t, role));
                }
            }
        }
        nearest
    }

    #[test]
    fn mining_cargo_socket_is_clear_to_real_backplate_and_matches_receiver() {
        use arkline_protocol::drone_bay_geometry::BayPose;
        let mine = model(DroneClass::Mining);
        let mouth = Vec3::from_array(MINING_CARGO_SOCKET_M.map(|v| v as f32 * 0.01));
        let world = BayPose {
            position_m: [-40., -1., 5.],
            orientation_xyzw: [0., 1., 0., 0.],
        }
        .compose(BayPose {
            position_m: MINING_CARGO_SOCKET_M,
            orientation_xyzw: [0., 0., 0., 1.],
        })
        .unwrap();
        assert_eq!(world.position_m, [-37.5, -1., 5.]);
        // Actual0.44m receiver center/edge/corner rays all reach the0.30m
        // backplate, leaving the required0.08m insertion unobstructed.
        for y in [-0.0022, 0., 0.0022] {
            for z in [-0.0022, 0., 0.0022] {
                let origin = mouth + Vec3::new(-0.001, y, z);
                let (distance, role) = first_tail_hit(mine, origin).unwrap();
                assert_eq!(role, RECESS, "receiver ray hit an opaque cap");
                assert!((distance - 0.004).abs() < 1e-6, "{y},{z}: {distance}");
                assert!(distance > 0.0018);
            }
        }
        let (flange_distance, role) =
            first_tail_hit(mine, mouth + Vec3::new(-0.001, 0.003, 0.)).unwrap();
        assert_eq!(role, FRAME);
        assert!((flange_distance - 0.001).abs() < 1e-6);
        // Negative control: reinserting the old solid aft cap must obstruct
        // the same test ray, not silently pass because only a collar was tested.
        let mut old = Builder::new(DroneClass::Mining);
        old.part(
            &Primitives::new().cube,
            ARMOR,
            Vec3::new(-0.021, 0., 0.),
            Vec3::new(0.004, 0.0052, 0.011),
            Quat::IDENTITY,
        );
        let (blocked, _) = first_tail_hit(&old.finish(), mouth - Vec3::X * 0.001).unwrap();
        assert!(blocked < 0.0018);
    }

    #[test]
    fn mining_socket_flange_and_keying_strips_have_real_bin_support() {
        let mine = model(DroneClass::Mining);
        let bin_rear = mine.batches[ARMOR]
            .vertices
            .iter()
            .filter(|v| v.p.x < -0.017 && v.p.y.abs() <= 0.0053 && v.p.z.abs() <= 0.0111)
            .map(|v| v.p.x)
            .reduce(f32::min)
            .unwrap();
        assert!((bin_rear - -0.024).abs() < 1e-7);
        let flange_back = mine.batches[FRAME]
            .vertices
            .iter()
            .filter(|v| v.p.x <= -0.0239 && v.p.y.abs() <= 0.00361 && v.p.z.abs() <= 0.00361)
            .map(|v| v.p.x)
            .reduce(f32::max)
            .unwrap();
        assert!(
            (flange_back - bin_rear).abs() < 1e-7,
            "flange must join bin, not float ahead"
        );
        for side in [-1.0, 1.0] {
            let bar_back = mine.batches[ACCENT]
                .vertices
                .iter()
                .filter(|v| {
                    v.p.x < -0.0235
                        && v.p.y * side >= 0.00399
                        && v.p.y * side <= 0.00471
                        && v.p.z.abs() <= 0.00851
                })
                .map(|v| v.p.x)
                .reduce(f32::max)
                .unwrap();
            assert!(
                (bar_back - bin_rear).abs() < 1e-7,
                "keying strip needs actual backing"
            );
            let (distance, role) =
                first_tail_hit(mine, Vec3::new(-0.03, side * 0.00435, 0.0)).unwrap();
            assert_eq!(role, ACCENT);
            assert!((distance - 0.00576).abs() < 1e-6);
        }
    }

    #[test]
    fn drone_semantic_materials_are_bounded_distinct_and_shared_with_head() {
        let mine = model(DroneClass::Mining);
        assert!(mine.batches[FRAME].metallic > mine.batches[ARMOR].metallic);
        assert!(mine.batches[FRAME].roughness < mine.batches[ARMOR].roughness);
        assert!(mine.batches[RECESS].roughness > mine.batches[ACCENT].roughness);
        for role in 0..5 {
            let head = &mine.batches[mine.work_head_batch_start.unwrap() + role];
            assert_eq!(head.roughness, mine.batches[role].roughness);
            assert_eq!(head.metallic, mine.batches[role].metallic);
        }
    }

    #[test]
    fn mining_tail_lenses_and_cargo_flange_have_no_coplanar_opaque_cover() {
        let mine = model(DroneClass::Mining);
        for side in [-1.0, 1.0] {
            // Nine actual triangle rays per visible lens, including its corners.
            // The old opaque housing on the same plane wins the first-hit test
            // by batch order, reproducing the native striped-panel defect.
            for y in [-0.0015, 0., 0.0015] {
                for z in [-0.0018, 0., 0.0018] {
                    let origin = Vec3::new(-0.026, -0.0002 + y, side * 0.0069 + z);
                    let (distance, role) = first_tail_hit(mine, origin).unwrap();
                    assert_eq!(role, LIGHT, "opaque geometry covers tail lens at{origin:?}");
                    assert!((distance - 0.001).abs() < 1e-6);
                }
            }
            // The flange side rails must remain visible, not intersect a lamp
            // housing that used to extend inward to|Z|0.30m (flange0.36m).
            for y in [-0.002, 0., 0.002] {
                let (distance, role) =
                    first_tail_hit(mine, Vec3::new(-0.026, y, side * 0.0033)).unwrap();
                assert_eq!(role, FRAME);
                assert!((distance - 0.001).abs() < 1e-6);
            }
        }
        let housing_vertices: Vec<_> = mine.batches[RECESS]
            .vertices
            .iter()
            .filter(|v| v.p.x < -0.023)
            .collect();
        assert!(!housing_vertices.is_empty());
        for vertex in housing_vertices {
            assert!(vertex.p.x >= -0.0248 - 1e-7);
            assert!(vertex.p.z.abs() >= 0.0037 - 1e-7);
        }
    }
    #[test]
    #[ignore = "Explicit source-bound body/socket geometry export for independent-bay proof"]
    fn export_independent_bay_drone_geometry() {
        let output =
            std::path::PathBuf::from(std::env::var_os("ARKLINE_BAY_DRONE_GEOMETRY").unwrap());
        assert!(!output.exists());
        let rows: Vec<_> = CLASSES.into_iter().map(|class| {
            let geometry = model(class);
            let batches: Vec<_> = geometry.batches.iter().map(|batch| {
                let vertices: Vec<_> = batch.vertices.iter().map(|v| {
                    serde_json::json!({"position_m":v.p.to_array().map(|x|f64::from(x)*100.),
                        "normal":v.n.to_array(),"position_bits":v.p.to_array().map(f32::to_bits)})
                }).collect();
                serde_json::json!({"vertices":vertices})
            }).collect();
            serde_json::json!({"class":class,"head_batch_start":geometry.work_head_batch_start,"batches":batches})
        }).collect();
        std::fs::write(
            output,
            serde_json::to_vec_pretty(&serde_json::json!({
            "scope":"actual production cached drone mesh; neutral work-head; no simulation state",
            "scene_to_metre_scale":100,"classes":rows}))
            .unwrap(),
        )
        .unwrap();
    }
}
