//! Display-only sampling. Commands and range gates always use the latest server snapshot.
use arkline_protocol::*;
use std::collections::{HashSet, VecDeque};

const INTERPOLATION_DELAY_MS: u64 = 400;
#[derive(Default)]
pub struct MotionHistory {
    samples: VecDeque<Snapshot>,
    direct_pilots: HashSet<String>,
}
impl MotionHistory {
    pub fn clear(&mut self) {
        self.samples.clear();
        self.direct_pilots.clear();
    }
    pub fn observe(&mut self, snapshot: &Snapshot) {
        // Keep a pilot's low-latency clock for the session. Switching back to a
        // larger delay at a Manual -> Stop boundary would replay old movement.
        for pilot in &snapshot.pilots {
            if pilot
                .flight
                .as_ref()
                .is_some_and(|f| f.mode == NavigationMode::Manual)
            {
                self.direct_pilots.insert(pilot.name.clone());
            }
        }
        self.samples.push_back(snapshot.clone());
        while self.samples.len() > 32 {
            self.samples.pop_front();
        }
    }
    pub fn sample(&self, latest: &Snapshot, now: u64) -> Snapshot {
        let mut result = latest.clone();
        for pilot in &mut result.pilots {
            let delay = if self.direct_pilots.contains(&pilot.name) {
                75
            } else {
                INTERPOLATION_DELAY_MS
            };
            let time = now.saturating_sub(delay);
            if !normal(pilot) {
                continue;
            }
            let mut samples = Vec::new();
            for snapshot in self.samples.iter().rev() {
                let Some(previous) = snapshot.pilots.iter().find(|p| p.name == pilot.name) else {
                    break;
                };
                if previous.system_id != pilot.system_id || !normal(previous) {
                    break;
                }
                samples.push((snapshot.server_time_ms, previous));
            }
            samples.reverse();
            let Some(&(last_time, last)) = samples.last() else {
                continue;
            };
            let Some(mut flight) = last.flight.clone() else {
                continue;
            };
            if let Some(pair) = samples
                .windows(2)
                .find(|p| p[0].0 <= time && time <= p[1].0)
            {
                let a = pair[0].1.flight.as_ref().unwrap();
                let b = pair[1].1.flight.as_ref().unwrap();
                let t =
                    (time - pair[0].0) as f64 / pair[1].0.saturating_sub(pair[0].0).max(1) as f64;
                flight.position =
                    std::array::from_fn(|i| a.position[i] + (b.position[i] - a.position[i]) * t);
                flight.velocity =
                    std::array::from_fn(|i| a.velocity[i] + (b.velocity[i] - a.velocity[i]) * t);
                let orientation = glam::DQuat::from_array(a.orientation)
                    .normalize()
                    .slerp(glam::DQuat::from_array(b.orientation).normalize(), t);
                flight.orientation = orientation.to_array();
                flight.angular_velocity = std::array::from_fn(|i| {
                    a.angular_velocity[i] + (b.angular_velocity[i] - a.angular_velocity[i]) * t
                });
                flight.heading =
                    std::array::from_fn(|i| a.heading[i] + (b.heading[i] - a.heading[i]) * t);
            } else if time >= last_time {
                flight.position = position(last, time);
            } else if let Some((_, first)) = samples.first() {
                flight = first.flight.clone().unwrap();
            }
            flight.updated_at_ms = now;
            pilot.flight = Some(flight);
        }
        result
    }
}
fn normal(pilot: &PilotView) -> bool {
    pilot.flight.is_some() && matches!(pilot.location, Location::Station | Location::Belt)
}
pub fn position(pilot: &PilotView, now: u64) -> [f64; 3] {
    let Some(flight) = &pilot.flight else {
        return STATION_POSITION;
    };
    if let Some(warp) = &flight.warp {
        if matches!(pilot.location, Location::InTransit { .. }) {
            return sample_warp(warp, now).0;
        }
    }
    let seconds = if normal(pilot) {
        now.saturating_sub(flight.updated_at_ms).min(350) as f64 / 1000.0
    } else {
        0.0
    };
    std::array::from_fn(|i| flight.position[i] + flight.velocity[i] * seconds)
}

pub fn warp_phase(pilot: &PilotView, now: u64) -> Option<(&'static str, f32)> {
    let path = pilot.flight.as_ref()?.warp.as_ref()?;
    if !matches!(pilot.location, Location::InTransit { .. }) || now >= path.arrives_at_ms {
        return None;
    }
    let elapsed = now.saturating_sub(path.started_at_ms);
    let phases = [
        ("WARP WARMUP", path.warmup_ms),
        ("WARP LAUNCH", path.launch_ms),
        ("WARP ACCELERATING", path.acceleration_ms),
        ("WARP CRUISE", path.cruise_ms),
        ("WARP DECELERATING", path.acceleration_ms),
        ("WARP ARRIVAL", path.arrival_ms),
    ];
    let mut start = 0;
    for (name, duration) in phases {
        if elapsed < start + duration {
            return Some((
                name,
                elapsed.saturating_sub(start) as f32 / duration.max(1) as f32,
            ));
        }
        start += duration;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(time: u64, x: f64) -> Snapshot {
        Snapshot {
            docked_traversal_capability: None,
            world_layout: None,
            exterior: None,
            resource_jobs: vec![],
            fleet_links: vec![],
            remote_controls: vec![],
            surface_props: vec![],
            traffic: vec![],
            stored_ore: 0,
            server_time_ms: time,
            online_pilots: vec![],
            asteroids: vec![],
            pilots: vec![PilotView {
                surface: None,
                station_avatar: None,
                power: None,
                mining_modules: vec![],
                ship_class: arkline_protocol::ShipClass::default(),
                name: "test".into(),
                credits: 10,
                ore: 0,
                system_id: 1,
                locked_asteroid: None,
                target_lock: None,
                target_locks: vec![],
                locked_asteroids: vec![],
                drones: vec![],
                drone: None,
                location: Location::Station,
                flight: Some(FlightState {
                    acceleration: [0.; 3],
                    position: [x, 0.0, 0.0],
                    velocity: [100.0, 0.0, 0.0],
                    heading: [1.0, 0.0, 0.0],
                    mode: NavigationMode::Stop,
                    updated_at_ms: time,
                    warp: None,
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    angular_velocity: [0.0; 3],
                }),
            }],
            orders: vec![],
            belt_ore: 0,
            ledger_sequence: 0,
            ledger_hash: String::new(),
        }
    }
    #[test]
    fn snapshots_interpolate_continuously_and_never_change_economic_data() {
        let mut history = MotionHistory::default();
        history.observe(&snapshot(1000, 0.0));
        let latest = snapshot(1350, 35.0);
        history.observe(&latest);
        let a = history.sample(&latest, 1550);
        let b = history.sample(&latest, 1560);
        assert!((position(&a.pilots[0], 1550)[0] - 15.0).abs() < 1e-9);
        assert!((position(&b.pilots[0], 1560)[0] - 16.0).abs() < 1e-9);
        assert_eq!(b.pilots[0].credits, latest.pilots[0].credits);
        assert_eq!(b.pilots[0].location, latest.pilots[0].location);
    }
    #[test]
    fn warp_moves_between_snapshots_and_preserves_au_precision() {
        let mut s = snapshot(1000, 0.0);
        let path = plan_warp(
            [AU_METERS * 8.0, 0.0, 0.0],
            [AU_METERS * 2.0, 0.0, 1000.0],
            1000,
        );
        s.pilots[0].location = Location::InTransit {
            destination: Destination::Belt,
            arrives_at_ms: path.arrives_at_ms,
        };
        s.pilots[0].flight.as_mut().unwrap().warp = Some(path.clone());
        let t = path.started_at_ms + path.warmup_ms + path.acceleration_ms / 2;
        assert_ne!(position(&s.pilots[0], t), position(&s.pilots[0], t + 16));
        assert_eq!(position(&s.pilots[0], path.arrives_at_ms), path.target);
        assert_eq!(warp_phase(&s.pilots[0], 1001).unwrap().0, "WARP WARMUP");
    }
    #[test]
    fn terminal_arrival_phase_uses_authoritative_deadline_and_keeps_moving() {
        let mut s = snapshot(1000, 0.0);
        let path = plan_warp([0.0; 3], [AU_METERS * 2.0, 0.0, 0.0], 1000);
        assert!(path.arrival_ms > 0);
        s.pilots[0].location = Location::InTransit {
            destination: Destination::Belt,
            arrives_at_ms: path.arrives_at_ms,
        };
        s.pilots[0].flight.as_mut().unwrap().warp = Some(path.clone());
        let launch = path.started_at_ms + path.warmup_ms;
        assert_eq!(
            warp_phase(&s.pilots[0], launch).unwrap(),
            ("WARP LAUNCH", 0.0)
        );
        assert_eq!(
            warp_phase(&s.pilots[0], launch + path.launch_ms).unwrap().0,
            "WARP ACCELERATING"
        );
        let start = path.arrives_at_ms - path.arrival_ms;
        assert_eq!(
            warp_phase(&s.pilots[0], start - 1).unwrap().0,
            "WARP DECELERATING"
        );
        assert_eq!(
            warp_phase(&s.pilots[0], start).unwrap(),
            ("WARP ARRIVAL", 0.0)
        );
        assert!(
            warp_phase(&s.pilots[0], start + path.arrival_ms / 2)
                .unwrap()
                .1
                > 0.49
        );
        let a = position(&s.pilots[0], start + 100);
        let b = position(&s.pilots[0], start + 116);
        assert!(b[0] > a[0]);
        assert!(path.target[0] - b[0] < path.arrival_distance_m);
        assert!(warp_phase(&s.pilots[0], path.arrives_at_ms).is_none());
        assert_eq!(position(&s.pilots[0], path.arrives_at_ms), path.target);
    }
    #[test]
    fn manual_orientation_uses_short_delay_and_shortest_quaternion_path() {
        let mut history = MotionHistory::default();
        let mut a = snapshot(1000, 0.0);
        a.pilots[0].flight.as_mut().unwrap().mode = NavigationMode::Manual;
        let mut b = a.clone();
        b.server_time_ms = 1100;
        b.pilots[0].flight.as_mut().unwrap().orientation =
            (-glam::DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)).to_array();
        history.observe(&a);
        history.observe(&b);
        let sample = history.sample(&b, 1125);
        let actual = glam::DQuat::from_array(sample.pilots[0].flight.as_ref().unwrap().orientation);
        let expected = glam::DQuat::from_rotation_x(std::f64::consts::FRAC_PI_4);
        assert!(actual.dot(expected).abs() > 0.999999);
    }
    #[test]
    fn manual_lease_expiry_never_rewinds_and_history_eviction_keeps_clock() {
        let mut history = MotionHistory::default();
        let mut latest = snapshot(1000, 0.0);
        latest.pilots[0].flight.as_mut().unwrap().mode = NavigationMode::Manual;
        history.observe(&latest);
        let mut previous = 0.0;
        for frame in 1..60 {
            latest = snapshot(1000 + frame * 50, frame as f64 * 5.0);
            history.observe(&latest);
            let shown = position(
                &history.sample(&latest, latest.server_time_ms).pilots[0],
                latest.server_time_ms,
            )[0];
            assert!(shown >= previous, "lease expiry rewound display");
            if frame > 2 {
                assert!((shown - (frame as f64 * 5.0 - 7.5)).abs() < 1e-9);
            }
            previous = shown;
        }
    }
    #[test]
    fn area_changes_do_not_lerp_across_universe() {
        let mut history = MotionHistory::default();
        history.observe(&snapshot(1000, 0.0));
        let mut next = snapshot(1350, AU_METERS * 8.0);
        next.pilots[0].system_id = 2;
        history.observe(&next);
        assert_eq!(
            position(&history.sample(&next, 1350).pilots[0], 1350)[0],
            AU_METERS * 8.0
        );
    }
}
