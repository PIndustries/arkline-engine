//! Frame-rate-independent interpolation of confirmed avatar orientation.
//! No predicted turning or gameplay state is written back to the server.

pub(super) fn heading(previous: Option<f64>, target: f64, frame_dt: f32) -> f64 {
    let Some(previous) = previous else {
        return target;
    };
    let delta = (target - previous + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
        - std::f64::consts::PI;
    let alpha = 1.0 - (-36.0 * f64::from(frame_dt.clamp(0.0, 0.1))).exp();
    (previous + delta * alpha + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
        - std::f64::consts::PI
}

#[cfg(test)]
mod tests {
    use super::*;
    fn arc(a: f64, b: f64) -> f64 {
        (a - b + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
    }
    #[test]
    fn crosses_pi_by_short_arc_and_reset_adopts_new_pose() {
        let start = std::f64::consts::PI - 0.01;
        let target = -std::f64::consts::PI + 0.01;
        let next = heading(Some(start), target, 1.0 / 60.0);
        assert!(arc(next, start) > 0.0 && arc(next, start) < 0.02);
        assert_eq!(heading(None, target, 0.0), target);
        assert!(arc(heading(Some(start), target, 0.0), start).abs() < 1e-12);
    }
    #[test]
    fn sparse_confirmed_turn_samples_do_not_snap_camera_between_frames() {
        // Server publishes a 2 rad/s turn at20Hz; render120Hz. The old path
        // jumped0.1rad (~55cm for a5.5m chase camera) each snapshot.
        let mut visual = 0.0;
        let mut maximum_step: f64 = 0.0;
        for frame in 1..=120 {
            let target = (frame / 6) as f64 * 0.1;
            let next = heading(Some(visual), target, 1.0 / 120.0);
            let step = arc(next, visual);
            assert!(step >= -1e-12);
            maximum_step = maximum_step.max(step);
            visual = next;
        }
        assert!(
            maximum_step < 0.04,
            "orientation still snaps: {maximum_step}"
        );
        assert!(2.0 - visual < 0.10, "smoothing adds excessive lag");
        println!("20Hz turn samples,120Hz render: raw step0.100000rad; smoothed max{maximum_step:.6}rad; final lag{:.6}rad",2.0-visual);
    }
    #[test]
    fn frame_rates_settle_equally_and_stopped_heading_stays_still() {
        let simulate = |fps| {
            let mut result = 0.0;
            for _ in 0..fps {
                result = heading(Some(result), 1.0, 1.0 / fps as f32);
            }
            result
        };
        assert!((simulate(30) - simulate(120)).abs() < 1e-6);
        assert!((simulate(60) - 1.0).abs() < 1e-6);
        assert!(arc(heading(Some(1.2), 1.2, 0.1), 1.2).abs() < 1e-12);
    }
}
