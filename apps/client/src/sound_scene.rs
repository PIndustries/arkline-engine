//! Deterministic audio intent from confirmed state, without device or UI access.
use super::sound_engine::{Cue, Mix};
use arkline_protocol::*;
use std::collections::{BTreeMap, BTreeSet};
const STALE_MS: u64 = 1500;
#[derive(Default)]
pub struct Scene {
    connected: bool,
    previous: Option<Sample>,
}
struct Sample {
    time: u64,
    audio_time: u64,
    mining_engaged: bool,
    pilot: PilotView,
    drone_health: BTreeMap<(String, u64), u32>,
    incoming: BTreeMap<String, u64>,
}
impl Scene {
    pub fn reset(&mut self) {
        self.previous = None;
    }
    /// `now_ms` is a live server-aligned clock, not the frozen visual presentation clock.
    pub fn update(
        &mut self,
        snapshot: Option<&Snapshot>,
        pilot: &str,
        connected: bool,
        now_ms: u64,
    ) -> (Mix, Vec<Cue>) {
        let mut cues = Vec::new();
        if connected != self.connected {
            cues.push(if connected {
                Cue::Connect
            } else {
                Cue::Disconnect
            });
            self.connected = connected;
            self.previous = None;
        }
        let Some(snapshot) =
            snapshot.filter(|s| connected && now_ms.saturating_sub(s.server_time_ms) <= STALE_MS)
        else {
            self.previous = None;
            return (Mix::default(), cues);
        };
        let Some(current) = snapshot.pilots.iter().find(|p| p.name == pilot) else {
            self.previous = None;
            return (Mix::default(), cues);
        };
        if self
            .previous
            .as_ref()
            .is_some_and(|p| p.pilot.name != pilot)
        {
            self.previous = None;
        }
        if let Some(previous) = &mut self.previous {
            if snapshot.server_time_ms < previous.time {
                return (Mix::default(), cues);
            }
            if snapshot.server_time_ms == previous.time {
                if warp_entry(previous, &previous.pilot, now_ms) {
                    cues.push(Cue::WarpEnter);
                }
                previous.audio_time = previous.audio_time.max(now_ms);
                return (loops(snapshot, &previous.pilot, now_ms, false), cues);
            }
            if snapshot.server_time_ms.saturating_sub(previous.time) <= STALE_MS {
                transitions(previous, snapshot, current, &mut cues);
                if warp_entry(previous, current, now_ms) {
                    cues.push(Cue::WarpEnter);
                }
                let engaged = loops(snapshot, current, snapshot.server_time_ms, true).mining > 0.0;
                if engaged != previous.mining_engaged {
                    cues.push(if engaged {
                        Cue::MiningStart
                    } else {
                        Cue::MiningStop
                    });
                }
            }
        }
        self.previous = Some(Sample {
            time: snapshot.server_time_ms,
            audio_time: self
                .previous
                .as_ref()
                .map_or(now_ms, |p| p.audio_time.max(now_ms)),
            mining_engaged: loops(snapshot, current, snapshot.server_time_ms, true).mining > 0.0,
            pilot: current.clone(),
            incoming: snapshot
                .pilots
                .iter()
                .filter_map(|p| match p.location {
                    Location::InTransit { arrives_at_ms, .. }
                    | Location::Jumping { arrives_at_ms, .. }
                        if p.name != pilot =>
                    {
                        Some((p.name.clone(), arrives_at_ms))
                    }
                    _ => None,
                })
                .take(256)
                .collect(),
            drone_health: snapshot
                .pilots
                .iter()
                .flat_map(|p| {
                    p.drones
                        .iter()
                        .map(move |d| ((p.name.clone(), d.id), d.hit_points))
                })
                .collect(),
        });
        (loops(snapshot, current, now_ms, false), cues)
    }
}
fn normal(p: &PilotView) -> bool {
    matches!(p.location, Location::Station | Location::Belt)
}
fn locks(p: &PilotView) -> BTreeSet<u64> {
    p.locked_asteroids
        .iter()
        .copied()
        .chain(p.locked_asteroid)
        .collect()
}
fn magnitude(v: [f64; 3]) -> f64 {
    v.iter().map(|v| v * v).sum::<f64>().sqrt()
}
fn warp_entry(previous: &Sample, current: &PilotView, now: u64) -> bool {
    let path = |p: &PilotView| {
        if !matches!(p.location, Location::InTransit { .. }) {
            return None;
        }
        p.flight.as_ref().and_then(|f| f.warp.as_ref()).map(|w| {
            (
                w.started_at_ms,
                w.started_at_ms.saturating_add(w.warmup_ms),
                w.arrives_at_ms,
            )
        })
    };
    path(current).is_some_and(|key| {
        path(&previous.pilot) == Some(key)
            && previous.audio_time < key.1
            && now >= key.1
            && now < key.2
    })
}
fn loops(snapshot: &Snapshot, p: &PilotView, now: u64, lifecycle: bool) -> Mix {
    let mut mix = Mix {
        active: true,
        docked: p.location == Location::Docked,
        ..Mix::default()
    };
    if normal(p) {
        if let Some(f) = &p.flight {
            let burn = (magnitude(f.acceleration) / ship_acceleration(p.ship_class, p.ore))
                .clamp(0.0, 1.0);
            let rotation = if matches!(f.mode, NavigationMode::Manual) {
                (magnitude(f.angular_velocity) / MAX_ANGULAR_SPEED).clamp(0.0, 1.0) * 0.25
            } else {
                0.0
            };
            mix.engine = (burn.sqrt() * 0.85 + rotation).clamp(0.0, 1.0) as f32;
        }
        let confirmed = locks(p);
        let modules = p
            .mining_modules
            .iter()
            .filter(|m| {
                let (Some(id), Some(start), Some(end)) =
                    (m.target_id, m.cycle_started_at_ms, m.cycle_completes_at_ms)
                else {
                    return false;
                };
                end > start
                    && now >= start
                    && (lifecycle || now < end)
                    && confirmed.contains(&id)
                    && snapshot.asteroids.iter().any(|a| {
                        a.id == id
                            && a.system_id == p.system_id
                            && a.ore > 0
                            && p.flight.as_ref().is_some_and(|f| {
                                let d = std::array::from_fn(|i| f.position[i] - a.position[i]);
                                magnitude(d) - a.radius <= MINING_RANGE
                            })
                    })
            })
            .count();
        let drones = p
            .drones
            .iter()
            .filter(|d| {
                matches!(d.mode,DroneMode::Mining{target_id} if confirmed.contains(&target_id))
                    && d.mining_cycle.as_ref().is_some_and(|c| {
                        now >= c.started_at_ms && (lifecycle || now < c.completes_at_ms)
                    })
            })
            .count();
        mix.mining = (modules as f32 * 0.35 + drones as f32 * 0.08).min(1.0);
    }
    if let Location::InTransit { arrives_at_ms, .. } = p.location {
        if now < arrives_at_ms {
            mix.warp = if let Some(w) = p.flight.as_ref().and_then(|f| f.warp.as_ref()) {
                let launch = w.started_at_ms.saturating_add(w.warmup_ms);
                if now < launch {
                    0.15 + 0.35
                        * (now.saturating_sub(w.started_at_ms) as f32 / w.warmup_ms.max(1) as f32)
                            .min(1.0)
                } else if w.arrival_ms > 0 && now >= w.arrives_at_ms.saturating_sub(w.arrival_ms) {
                    0.2 + 0.7 * ((w.arrives_at_ms - now) as f32 / w.arrival_ms as f32)
                } else {
                    0.9
                }
            } else {
                0.6
            };
        }
    }
    mix
}
fn transitions(previous: &Sample, s: &Snapshot, p: &PilotView, cues: &mut Vec<Cue>) {
    let old = &previous.pilot;
    if old.location == Location::Docked && normal(p) {
        cues.push(Cue::Undock);
    }
    if old.location != Location::Docked && p.location == Location::Docked {
        cues.push(Cue::Dock);
    }
    if !matches!(old.location, Location::InTransit { .. })
        && matches!(p.location, Location::InTransit { .. })
    {
        cues.push(Cue::WarpCharge);
    }
    if let Location::InTransit { arrives_at_ms, .. } = old.location {
        if normal(p) && s.server_time_ms >= arrives_at_ms {
            cues.push(Cue::WarpArrival);
        }
    }
    // Only confirmed nearby arrivals are audible; cancellations before the deadline
    // and arrivals observed without a preceding transit sample stay silent.
    if !cues.iter().any(|cue| matches!(cue, Cue::WarpArrival))
        && p.flight.as_ref().is_some_and(|listener| {
            s.pilots.iter().any(|other| {
                other.name != p.name
                    && other.system_id == p.system_id
                    && normal(other)
                    && previous
                        .incoming
                        .get(&other.name)
                        .is_some_and(|end| s.server_time_ms >= *end)
                    && other.flight.as_ref().is_some_and(|flight| {
                        magnitude(std::array::from_fn(|i| {
                            flight.position[i] - listener.position[i]
                        })) <= 200_000.0
                    })
            })
        })
    {
        cues.push(Cue::WarpArrival);
    }
    if !matches!(old.location, Location::Jumping { .. })
        && (matches!(p.location, Location::Jumping { .. }) || old.system_id != p.system_id)
    {
        cues.push(Cue::Jump);
    }
    if locks(p).difference(&locks(old)).next().is_some() {
        cues.push(Cue::Lock);
    }
    let previous_due = old.mining_modules.iter().any(|m| {
        m.target_id.is_some()
            && m.cycle_completes_at_ms
                .is_some_and(|end| end > previous.time && end <= s.server_time_ms)
    }) || old.drones.iter().any(|d| {
        d.mining_cycle.as_ref().is_some_and(|c| {
            c.completes_at_ms > previous.time && c.completes_at_ms <= s.server_time_ms
        })
    });
    if normal(old) && p.ore > old.ore && previous_due {
        cues.push(Cue::MiningCycle);
    }
    if p.ship_class == old.ship_class
        && old.ore < cargo_capacity(p.ship_class)
        && p.ore >= cargo_capacity(p.ship_class)
    {
        cues.push(Cue::CargoFull);
    }
    if let (Some(a), Some(b)) = (&old.power, &p.power) {
        if a.capacity_j > 0
            && b.capacity_j > 0
            && a.charge_j as f64 / a.capacity_j as f64 > 0.2
            && b.charge_j as f64 / b.capacity_j as f64 <= 0.2
        {
            cues.push(Cue::LowPower);
        }
    }
    if p.drones
        .iter()
        .any(|d| !old.drones.iter().any(|o| o.id == d.id))
    {
        cues.push(Cue::DroneLaunch);
    }
    if p.drones.iter().any(|d| {
        matches!(d.mode, DroneMode::Returning)
            && old
                .drones
                .iter()
                .any(|o| o.id == d.id && !matches!(o.mode, DroneMode::Returning))
    }) {
        cues.push(Cue::DroneRecall);
    }
    // Timer progress alone is not proof of firing: validate observable target damage.
    if p.drones.iter().any(|d| {
        let DroneMode::Attacking {
            target_pilot,
            target_drone_id,
        } = &d.mode
        else {
            return false;
        };
        let Some(old_d) = old.drones.iter().find(|o| o.id == d.id && o.mode == d.mode) else {
            return false;
        };
        let (Some(before), Some(after)) = (old_d.attack_ready_at_ms, d.attack_ready_at_ms) else {
            return false;
        };
        after > before
            && before <= s.server_time_ms
            && previous
                .drone_health
                .get(&(target_pilot.clone(), *target_drone_id))
                .is_some_and(|old_hp| {
                    s.pilots
                        .iter()
                        .find(|p| p.name == *target_pilot)
                        .and_then(|p| p.drones.iter().find(|d| d.id == *target_drone_id))
                        .is_some_and(|target| target.hit_points < *old_hp)
                })
    }) {
        cues.push(Cue::DroneShot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(t: u64) -> Snapshot {
        serde_json::from_value(serde_json::json!({"server_time_ms":t,"pilots":[{"name":"Pilot","credits":1000,"ore":0,"location":{"kind":"station"},"flight":{"position":[0,0,0],"velocity":[300,0,0],"acceleration":[150,0,0],"heading":[1,0,0],"mode":{"mode":"manual"},"updated_at_ms":t}}],"orders":[],"belt_ore":100000,"ledger_sequence":1,"ledger_hash":GENESIS_HASH})).unwrap()
    }
    fn drone(id: u64) -> DroneView {
        serde_json::from_value(serde_json::json!({"id":id,"position":[0,0,0],"velocity":[0,0,0],"heading":[1,0,0],"mode":{"kind":"hold"},"updated_at_ms":0,"hit_points":100})).unwrap()
    }
    #[test]
    fn connection_source_stale_and_reordered_snapshots_never_replay_history() {
        let mut scene = Scene::default();
        let mut s = snapshot(1000);
        let (mix, cues) = scene.update(Some(&s), "Pilot", true, 1000);
        assert!(mix.active && mix.engine > 0.0);
        assert!(matches!(cues.as_slice(), [Cue::Connect]));
        assert!(scene.update(Some(&s), "Pilot", true, 1100).1.is_empty());
        assert!(!scene.update(Some(&s), "Pilot", true, 2501).0.active);
        s.server_time_ms = 2600;
        s.pilots[0].location = Location::Docked;
        s.pilots[0].ore = 1000;
        assert!(scene.update(Some(&s), "Pilot", true, 2600).1.is_empty());
        s.server_time_ms = 2500;
        assert!(!scene.update(Some(&s), "Pilot", true, 2600).0.active);
        assert!(matches!(
            scene.update(None, "Pilot", false, 2700).1.as_slice(),
            [Cue::Disconnect]
        ));
        assert!(scene.update(None, "Pilot", false, 2701).1.is_empty());
        s.server_time_ms = 2800;
        assert!(matches!(
            scene.update(Some(&s), "Pilot", true, 2800).1.as_slice(),
            [Cue::Connect]
        ));
        s.pilots[0].name = "Worker".into();
        s.server_time_ms = 2900;
        s.pilots[0].location = Location::Station;
        assert!(scene.update(Some(&s), "Worker", true, 2900).1.is_empty());
        scene.reset();
        assert!(!scene.update(None, "Worker", false, 3000).0.active);
    }
    #[test]
    fn coasting_is_quiet_but_real_braking_and_rotation_are_audible() {
        let mut scene = Scene::default();
        let mut s = snapshot(1000);
        let f = s.pilots[0].flight.as_mut().unwrap();
        f.acceleration = [0.0; 3];
        assert_eq!(scene.update(Some(&s), "Pilot", true, 1000).0.engine, 0.0);
        s.server_time_ms = 1100;
        s.pilots[0].flight.as_mut().unwrap().acceleration = [-60.0, 0.0, 0.0];
        assert!(scene.update(Some(&s), "Pilot", true, 1100).0.engine > 0.0);
        s.server_time_ms = 1200;
        let f = s.pilots[0].flight.as_mut().unwrap();
        f.acceleration = [0.0; 3];
        f.angular_velocity = [0.0, 0.0, MAX_ANGULAR_SPEED];
        assert!(scene.update(Some(&s), "Pilot", true, 1200).0.engine > 0.0);
        scene.reset();
        assert!(scene.update(Some(&s), "Pilot", true, 1200).1.is_empty());
    }
    #[test]
    fn mining_needs_confirmed_deadline_and_gain_not_purchase_or_every_frame() {
        let mut scene = Scene::default();
        let mut s = snapshot(1000);
        let p = &mut s.pilots[0];
        p.ore = 975;
        p.locked_asteroids = vec![1];
        p.mining_modules = vec![MiningModule {
            module_id: 0,
            target_id: Some(1),
            cycle_started_at_ms: Some(0),
            cycle_completes_at_ms: Some(1200),
            stop_reason: None,
        }];
        p.power = Some(PowerState {
            charge_j: 30,
            capacity_j: 100,
            regen_w: 1,
            updated_at_ms: 1000,
            regen_remainder_mj: 0,
        });
        s.asteroids.push(Asteroid {
            id: 1,
            system_id: HOME_SYSTEM_ID,
            name: "Rock".into(),
            position: [0.0; 3],
            radius: 100.0,
            ore: 100,
        });
        assert!(scene.update(Some(&s), "Pilot", true, 1000).0.mining > 0.0);
        s.server_time_ms = 1200;
        s.pilots[0].ore = 1000;
        s.pilots[0].mining_modules.clear();
        s.pilots[0].power.as_mut().unwrap().charge_j = 20;
        let (mix, cues) = scene.update(Some(&s), "Pilot", true, 1200);
        assert_eq!(mix.mining, 0.0);
        assert_eq!(
            cues.iter()
                .filter(|c| matches!(c, Cue::MiningCycle))
                .count(),
            1
        );
        assert!(cues.iter().any(|c| matches!(c, Cue::CargoFull)));
        assert!(cues.iter().any(|c| matches!(c, Cue::LowPower)));
        assert!(scene.update(Some(&s), "Pilot", true, 1210).1.is_empty());
        s.server_time_ms = 1300;
        s.pilots[0].location = Location::Docked;
        s.pilots[0].ore = 0;
        scene.update(Some(&s), "Pilot", true, 1300);
        s.server_time_ms = 1400;
        s.pilots[0].ore = 20;
        assert!(!scene
            .update(Some(&s), "Pilot", true, 1400)
            .1
            .iter()
            .any(|c| matches!(c, Cue::MiningCycle)));
    }
    #[test]
    fn warp_and_lock_follow_authoritative_transitions_not_predicted_arrival() {
        let mut scene = Scene::default();
        let mut s = snapshot(1000);
        s.pilots[0].location = Location::Docked;
        scene.update(Some(&s), "Pilot", true, 1000);
        s.server_time_ms = 1100;
        s.pilots[0].location = Location::Station;
        assert!(scene
            .update(Some(&s), "Pilot", true, 1100)
            .1
            .iter()
            .any(|c| matches!(c, Cue::Undock)));
        let path = plan_warp([0.0; 3], [2000.0, 0.0, 0.0], 1200);
        let arrival = path.arrives_at_ms;
        s.server_time_ms = 1200;
        s.pilots[0].location = Location::InTransit {
            destination: Destination::Belt,
            arrives_at_ms: arrival,
        };
        s.pilots[0].flight.as_mut().unwrap().warp = Some(path);
        let (mix, cues) = scene.update(Some(&s), "Pilot", true, 1200);
        assert!(mix.warp > 0.0);
        assert!(cues.iter().any(|c| matches!(c, Cue::WarpCharge)));
        for t in (1300..arrival).step_by(100) {
            s.server_time_ms = t;
            scene.update(Some(&s), "Pilot", true, t);
        }
        s.server_time_ms = arrival;
        let (mix, cues) = scene.update(Some(&s), "Pilot", true, arrival);
        assert_eq!(mix.warp, 0.0);
        assert!(!cues.iter().any(|c| matches!(c, Cue::WarpArrival)));
        s.server_time_ms += 1;
        s.pilots[0].location = Location::Belt;
        s.pilots[0].locked_asteroids = vec![1];
        let (_, cues) = scene.update(Some(&s), "Pilot", true, s.server_time_ms);
        assert!(cues.iter().any(|c| matches!(c, Cue::WarpArrival)));
        assert!(cues.iter().any(|c| matches!(c, Cue::Lock)));
    }
    #[test]
    fn nearby_arrivals_are_local_confirmed_and_never_replayed() {
        for (distance, system, audible) in [
            (199_999.0, HOME_SYSTEM_ID, true),
            (200_001.0, HOME_SYSTEM_ID, false),
            (100.0, HOME_SYSTEM_ID + 1, false),
        ] {
            let mut scene = Scene::default();
            let mut s = snapshot(1000);
            let mut other = s.pilots[0].clone();
            other.name = "Visitor".into();
            other.system_id = system;
            other.flight.as_mut().unwrap().position = [distance, 0.0, 0.0];
            other.location = Location::InTransit {
                destination: Destination::Belt,
                arrives_at_ms: 1100,
            };
            s.pilots.push(other);
            scene.update(Some(&s), "Pilot", true, 1000);
            s.server_time_ms = 1100;
            s.pilots[1].location = Location::Belt;
            let cues = scene.update(Some(&s), "Pilot", true, 1100).1;
            assert_eq!(
                cues.iter()
                    .filter(|c| matches!(c, Cue::WarpArrival))
                    .count(),
                usize::from(audible)
            );
            assert!(scene.update(Some(&s), "Pilot", true, 1101).1.is_empty());
            scene.reset();
            assert!(scene.update(Some(&s), "Pilot", true, 1101).1.is_empty());
        }
        for reset in 0..3 {
            let mut scene = Scene::default();
            let mut s = snapshot(1000);
            let mut other = s.pilots[0].clone();
            other.name = "Visitor".into();
            other.location = Location::Jumping {
                gate_id: 1,
                arrives_at_ms: 1100,
            };
            s.pilots.push(other);
            scene.update(Some(&s), "Pilot", true, 1000);
            match reset {
                0 => {
                    scene.update(Some(&s), "Pilot", true, 3000);
                }
                1 => {
                    scene.update(None, "Pilot", false, 1050);
                }
                _ => {
                    scene.update(Some(&s), "Visitor", true, 1050);
                }
            }
            s.server_time_ms = 1100;
            s.pilots[1].location = Location::Station;
            assert!(!scene
                .update(Some(&s), "Pilot", true, 1100)
                .1
                .iter()
                .any(|c| matches!(c, Cue::WarpArrival)));
        }
    }
    #[test]
    fn warp_entry_live_clock_crossing_is_latched_without_initial_replay() {
        let mut s = snapshot(2500);
        let path = plan_warp([0.0; 3], [1_000_000.0, 0.0, 0.0], 1000);
        let launch = path.started_at_ms + path.warmup_ms;
        s.pilots[0].location = Location::InTransit {
            destination: Destination::Belt,
            arrives_at_ms: path.arrives_at_ms,
        };
        s.pilots[0].flight.as_mut().unwrap().warp = Some(path);
        let mut scene = Scene::default();
        assert!(!scene
            .update(Some(&s), "Pilot", true, launch - 1)
            .1
            .iter()
            .any(|c| matches!(c, Cue::WarpEnter)));
        assert_eq!(
            scene
                .update(Some(&s), "Pilot", true, launch)
                .1
                .iter()
                .filter(|c| matches!(c, Cue::WarpEnter))
                .count(),
            1
        );
        assert!(scene
            .update(Some(&s), "Pilot", true, launch + 1)
            .1
            .is_empty());
        // A clock correction cannot re-arm the same boundary.
        scene.update(Some(&s), "Pilot", true, launch - 1);
        assert!(scene
            .update(Some(&s), "Pilot", true, launch + 2)
            .1
            .is_empty());
        scene.reset();
        assert!(scene
            .update(Some(&s), "Pilot", true, launch + 3)
            .1
            .is_empty());
    }
    #[test]
    fn mining_lifecycle_coalesces_modules_and_ignores_cycle_boundaries() {
        let mut s = snapshot(1000);
        s.pilots[0].locked_asteroids = vec![1];
        s.asteroids.push(Asteroid {
            id: 1,
            system_id: HOME_SYSTEM_ID,
            name: "Rock".into(),
            position: [0.0; 3],
            radius: 100.0,
            ore: 1000,
        });
        let mut scene = Scene::default();
        scene.update(Some(&s), "Pilot", true, 1000);
        s.server_time_ms = 1100;
        s.pilots[0].mining_modules = (0..2)
            .map(|module_id| MiningModule {
                module_id,
                target_id: Some(1),
                cycle_started_at_ms: Some(1100),
                cycle_completes_at_ms: Some(1200),
                stop_reason: None,
            })
            .collect();
        let cues = scene.update(Some(&s), "Pilot", true, 1100).1;
        assert_eq!(
            cues.iter()
                .filter(|c| matches!(c, Cue::MiningStart))
                .count(),
            1
        );
        s.server_time_ms = 1200;
        assert!(scene.update(Some(&s), "Pilot", true, 1200).1.is_empty());
        s.server_time_ms = 1250;
        for m in &mut s.pilots[0].mining_modules {
            m.cycle_started_at_ms = Some(1200);
            m.cycle_completes_at_ms = Some(1300);
        }
        assert!(scene.update(Some(&s), "Pilot", true, 1250).1.is_empty());
        s.server_time_ms = 1260;
        s.pilots[0].mining_modules.pop();
        assert!(scene.update(Some(&s), "Pilot", true, 1260).1.is_empty());
        s.server_time_ms = 1270;
        s.pilots[0].mining_modules.clear();
        assert_eq!(
            scene
                .update(Some(&s), "Pilot", true, 1270)
                .1
                .iter()
                .filter(|c| matches!(c, Cue::MiningStop))
                .count(),
            1
        );
        assert!(scene.update(Some(&s), "Pilot", true, 1271).1.is_empty());
    }
    #[test]
    fn drone_launch_recall_and_shot_need_real_state_evidence() {
        let mut scene = Scene::default();
        let mut s = snapshot(1000);
        let mut victim = s.pilots[0].clone();
        victim.name = "Target".into();
        victim.drones = vec![drone(99)];
        s.pilots.push(victim);
        scene.update(Some(&s), "Pilot", true, 1000);
        let mut attacker = drone(1);
        attacker.class = DroneClass::Attack;
        attacker.mode = DroneMode::Attacking {
            target_pilot: "Target".into(),
            target_drone_id: 99,
        };
        attacker.attack_ready_at_ms = Some(1200);
        s.pilots[0].drones.push(attacker);
        s.server_time_ms = 1100;
        let cues = scene.update(Some(&s), "Pilot", true, 1100).1;
        assert!(cues.iter().any(|c| matches!(c, Cue::DroneLaunch)));
        assert!(!cues.iter().any(|c| matches!(c, Cue::DroneShot)));
        s.server_time_ms = 1200;
        s.pilots[0].drones[0].attack_ready_at_ms = Some(1300);
        assert!(!scene
            .update(Some(&s), "Pilot", true, 1200)
            .1
            .iter()
            .any(|c| matches!(c, Cue::DroneShot)));
        s.server_time_ms = 1300;
        s.pilots[0].drones[0].attack_ready_at_ms = Some(1400);
        s.pilots[1].drones[0].hit_points = 90;
        assert!(scene
            .update(Some(&s), "Pilot", true, 1300)
            .1
            .iter()
            .any(|c| matches!(c, Cue::DroneShot)));
        s.server_time_ms = 1400;
        s.pilots[0].drones[0].mode = DroneMode::Returning;
        assert!(scene
            .update(Some(&s), "Pilot", true, 1400)
            .1
            .iter()
            .any(|c| matches!(c, Cue::DroneRecall)));
        s.server_time_ms = 1500;
        s.pilots[0].drones.clear();
        assert!(!scene
            .update(Some(&s), "Pilot", true, 1500)
            .1
            .iter()
            .any(|c| matches!(c, Cue::DroneShot)));
    }
}
