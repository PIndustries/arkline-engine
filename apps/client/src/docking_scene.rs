//! World-space docking platforms and confirmed clamp travel. Never moves a ship.
use super::*;
use arkline_protocol::{docking_berth_position, docking_platform_boxes, DOCKING_BERTH_COUNT};
pub(super) fn parts(frame: &Frame) -> Vec<ModelPart> {
    let mut result = Vec::new();
    let mut box_at = |min: [f64; 3], max: [f64; 3], color: Vec3, emission: f32| {
        let center = std::array::from_fn(|i| (min[i] + max[i]) * 0.5);
        let half = Vec3::from_array(std::array::from_fn(|i| ((max[i] - min[i]) / 200.) as f32));
        result.push((
            0,
            transform(frame.space.scene(center), half, Quat::IDENTITY),
            color,
            emission,
        ));
    };
    for id in 0..DOCKING_BERTH_COUNT {
        for (index, (min, max)) in docking_platform_boxes(id).into_iter().enumerate() {
            box_at(
                min,
                max,
                if index == 0 {
                    Vec3::new(0.18, 0.25, 0.29)
                } else {
                    Vec3::new(0.30, 0.34, 0.36)
                },
                0.,
            );
        }
        let p = docking_berth_position(id);
        let clamps = frame
            .scene
            .pilots
            .iter()
            .filter_map(|p| p.flight.as_ref()?.docking.as_ref())
            .find(|d| d.berth_id == id)
            .map_or(0., |d| d.clamps);
        for x in [-22., 22.] {
            for side in [-1., 1.] {
                let z = p[2] + side * (24. - 4. * clamps);
                box_at(
                    [p[0] + x - 1.2, p[1] - 8., z - 0.7],
                    [p[0] + x + 1.2, p[1] - 3., z + 0.7],
                    Vec3::new(0.55, 0.36, 0.10),
                    0.,
                );
            }
        }
        for step in 0..9 {
            let x = p[0] + 60. + step as f64 * 25.;
            for side in [-1., 1.] {
                box_at(
                    [x - 0.5, 4., p[2] + side * 25. - 0.5],
                    [x + 0.5, 4.3, p[2] + side * 25. + 0.5],
                    Vec3::new(0.10, 0.65, 0.48),
                    1.,
                );
            }
        }
    }
    result
}
