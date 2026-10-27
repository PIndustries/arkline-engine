//! Confirmed-state lifecycle, continuous loop mix and one-shot transition decisions.

use super::*;

impl Scene {
    pub fn reset(&mut self) {
        self.previous = None;
        self.spatial_previous = None;
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
                d.class == DroneClass::Mining && d.hit_points > 0 && d.signal_strength.is_finite() && d.signal_strength > 0.
                    && !constrained(p, d.id)
                    && matches!(d.mode,DroneMode::Mining{target_id} if confirmed.contains(&target_id))
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
    mechanical_transitions(old, p, previous.time, s.server_time_ms, cues);
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
        d.class == DroneClass::Mining
            && d.hit_points > 0
            && d.signal_strength.is_finite()
            && d.signal_strength > 0.
            && !constrained(old, d.id)
            && matches!(d.mode, DroneMode::Mining { .. })
            && d.mining_cycle.as_ref().is_some_and(|cycle| {
                cycle.completes_at_ms > previous.time
                    && cycle.completes_at_ms <= s.server_time_ms
                    && p.drones.iter().any(|next| {
                        next.id == d.id
                            && next.class == d.class
                            && next.mode == d.mode
                            && next.hit_points > 0
                            && next.signal_strength.is_finite()
                            && next.signal_strength > 0.
                            && !constrained(p, next.id)
                            && next.mining_cycle.as_ref().is_some_and(|advanced| {
                                advanced.started_at_ms >= cycle.completes_at_ms
                                    && advanced.completes_at_ms > cycle.completes_at_ms
                            })
                    })
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
    if p.drone_bay.is_none()
        && p.drones
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

// Snapshot edges only: no UI acknowledgement, local deadline completion, or
// continuous motor loop that could outlive a dropped/stale connection.
fn mechanical_transitions(
    old: &PilotView,
    current: &PilotView,
    before: u64,
    now: u64,
    cues: &mut Vec<Cue>,
) {
    fn traversal(pilot: &PilotView) -> Option<&DockedTraversalState> {
        pilot.station_avatar.as_ref()?.docked_traversal.as_ref()
    }
    let previous = traversal(old);
    let Some(next) = traversal(current) else {
        return;
    };
    if next.binding != next.actuators.binding {
        return;
    }
    // A seated pilot has no actuator clone. Only an explicitly new Leaving
    // sequence can be heard on that boundary; avatar spawning alone is silent.
    let compatible = previous
        .as_ref()
        .is_some_and(|p| p.binding == next.binding && p.actuators.binding == next.binding);
    let leaving_seat = previous.is_none()
        && old.station_avatar.is_none()
        && next
            .seat_transition
            .as_ref()
            .is_some_and(|t| t.phase == DockedSeatPhase::Leaving);
    if compatible || leaving_seat {
        if let Some(t) = &next.seat_transition {
            if t.started_at_ms >= before
                && t.started_at_ms <= now
                && now < t.ends_at_ms
                && previous.as_ref().and_then(|p| p.seat_transition.as_ref()) != Some(t)
            {
                cues.push(Cue::SeatMove);
            }
        }
    }
    let Some(previous) = previous.filter(|_| compatible) else {
        return;
    };
    let old = &previous.actuators;
    let next = &next.actuators;
    if let (Some(old), Some(next)) = (&old.hatches, &next.hatches) {
        for (old, next) in old.iter().zip(next) {
            if !old.fraction.is_finite()
                || !next.fraction.is_finite()
                || !(0.0..=1.0).contains(&old.fraction)
                || !(0.0..=1.0).contains(&next.fraction)
            {
                continue;
            }
            if next.blocked && !old.blocked {
                cues.push(Cue::HatchBlocked);
            } else if next.moving && (!old.moving || old.target_open != next.target_open) {
                cues.push(Cue::HatchMove);
            } else if old.moving && !next.moving {
                cues.push(
                    if !next.blocked && !next.target_open && next.fraction == 0.0 {
                        Cue::HatchLatch
                    } else {
                        Cue::HatchStop
                    },
                );
            }
        }
    }
    if old.lift.floor_station_m.is_finite() && next.lift.floor_station_m.is_finite() {
        if next.lift.moving && (!old.lift.moving || old.lift.target != next.lift.target) {
            cues.push(Cue::LiftMove);
        } else if old.lift.moving && !next.lift.moving {
            // Motor stopping is not a claim of arrival: no landing chime.
            cues.push(Cue::LiftStop);
        }
    }
}
