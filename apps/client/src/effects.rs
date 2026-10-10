//! Short presentation cues derived from confirmed, explicitly targeted actions.
use arkline_protocol::{Asteroid, Location, PilotView, Snapshot, MINING_RANGE};

pub fn can_extract(pilot: &PilotView) -> bool {
    !matches!(
        pilot.location,
        Location::Docked | Location::InTransit { .. } | Location::Jumping { .. }
    ) && pilot.flight.is_some()
        && (!pilot.locked_asteroids.is_empty() || pilot.locked_asteroid.is_some())
}
pub fn can_extract_target(pilot: &PilotView, asteroid: &Asteroid) -> bool {
    can_extract(pilot)
        && pilot.system_id == asteroid.system_id
        && (pilot.locked_asteroids.contains(&asteroid.id)
            || pilot.locked_asteroid == Some(asteroid.id))
        && pilot.flight.as_ref().is_some_and(|flight| {
            let center = flight
                .position
                .iter()
                .zip(asteroid.position)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                .sqrt();
            (center - asteroid.radius).max(0.0) <= MINING_RANGE
        })
}
#[derive(Clone, Copy, Debug)]
pub struct ModuleCycle {
    pub target_id: u64,
    pub progress: f32,
    pub remaining_ms: u64,
}
/// Active state comes from the server, including periods awaiting the next cycle snapshot.
pub fn module_cycle(pilot: &PilotView, module_id: u8, now: u64) -> Option<ModuleCycle> {
    if !can_extract(pilot) {
        return None;
    }
    let module = pilot
        .mining_modules
        .iter()
        .find(|m| m.module_id == module_id)?;
    let target_id = module.target_id?;
    let start = module.cycle_started_at_ms?;
    let finish = module.cycle_completes_at_ms?;
    if module_id > 1
        || finish <= start
        || now < start
        || (!pilot.locked_asteroids.contains(&target_id)
            && pilot.locked_asteroid != Some(target_id))
    {
        return None;
    }
    Some(ModuleCycle {
        target_id,
        progress: (now.saturating_sub(start) as f32 / (finish - start) as f32).clamp(0.0, 1.0),
        remaining_ms: finish.saturating_sub(now),
    })
}
pub const EFFECT_DURATION_MS: u64 = 6000;
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EffectKind {
    Mining { units: u64, asteroid_id: u64 },
    Docking,
    Undocking,
}
#[derive(Clone, Copy, Debug)]
pub struct Effect {
    pub kind: EffectKind,
    pub started_ms: u64,
}
#[derive(Default)]
pub struct Effects {
    pub events: Vec<Effect>,
}
impl Effects {
    pub fn clear(&mut self) {
        self.events.clear();
    }
    pub fn retain(&mut self, now: u64) {
        self.events
            .retain(|e| now.saturating_sub(e.started_ms) < EFFECT_DURATION_MS);
    }
    pub fn observe(&mut self, previous: &Snapshot, next: &Snapshot, pilot: &str, now_ms: u64) {
        let before = previous.pilots.iter().find(|p| p.name == pilot);
        let after = next.pilots.iter().find(|p| p.name == pilot);
        let (Some(before), Some(after)) = (before, after) else {
            return;
        };
        if before.system_id != after.system_id {
            self.clear();
            return;
        }
        let mined = before.locked_asteroid.and_then(|id| {
            let before_rock = previous.asteroids.iter().find(|a| a.id == id)?;
            let after_rock = next.asteroids.iter().find(|a| a.id == id)?;
            let units = after.ore.checked_sub(before.ore).filter(|u| *u > 0)?;
            (can_extract_target(before, before_rock)
                && can_extract_target(after, after_rock)
                && before_rock.ore.saturating_sub(after_rock.ore) >= units)
                .then_some(EffectKind::Mining {
                    units,
                    asteroid_id: id,
                })
        });
        let kind = mined.or_else(|| {
            if before.location != Location::Docked && after.location == Location::Docked {
                Some(EffectKind::Docking)
            } else if before.location == Location::Docked && after.location != Location::Docked {
                Some(EffectKind::Undocking)
            } else {
                None
            }
        });
        self.retain(now_ms);
        if let Some(kind) = kind {
            self.events.push(Effect {
                kind,
                started_ms: now_ms,
            });
            if self.events.len() > 16 {
                self.events.remove(0);
            }
        }
    }
    pub fn mining(&self, now: u64) -> Option<(f32, u64)> {
        self.events.iter().rev().find_map(|e| match e.kind {
            EffectKind::Mining { units, .. }
                if now.saturating_sub(e.started_ms) < EFFECT_DURATION_MS =>
            {
                Some((
                    now.saturating_sub(e.started_ms) as f32 / EFFECT_DURATION_MS as f32,
                    units,
                ))
            }
            _ => None,
        })
    }
    pub fn mining_target(&self, now: u64) -> Option<u64> {
        self.events.iter().rev().find_map(|e| match e.kind {
            EffectKind::Mining { asteroid_id, .. }
                if now.saturating_sub(e.started_ms) < EFFECT_DURATION_MS =>
            {
                Some(asteroid_id)
            }
            _ => None,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use arkline_protocol::*;
    fn snapshot(ore: u64, remaining: u64) -> Snapshot {
        Snapshot {
            resource_jobs: vec![],
            fleet_links: vec![],
            remote_controls: vec![],
            surface_props: vec![],
            traffic: vec![],
            stored_ore: 0,
            server_time_ms: 100,
            online_pilots: vec![],
            asteroids: vec![Asteroid {
                id: 7,
                system_id: 1,
                name: "Target seven".into(),
                position: BELT_POSITION,
                radius: 100.0,
                ore: remaining,
            }],
            pilots: vec![PilotView {
                surface: None,
                power: None,
                mining_modules: vec![],
                ship_class: arkline_protocol::ShipClass::default(),
                name: "Pilot".into(),
                credits: 1000,
                ore,
                location: Location::Belt,
                system_id: 1,
                locked_asteroid: Some(7),
                target_lock: None,
                target_locks: vec![],
                locked_asteroids: vec![],
                drone: None,
                drones: vec![],
                flight: Some(FlightState {
                    warp: None,
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    angular_velocity: [0.0; 3],
                    position: BELT_POSITION,
                    velocity: [0.0; 3],
                    heading: [1.0, 0.0, 0.0],
                    mode: NavigationMode::Stop,
                    updated_at_ms: 100,
                }),
            }],
            orders: vec![],
            belt_ore: remaining,
            ledger_sequence: 0,
            ledger_hash: GENESIS_HASH.into(),
        }
    }
    #[test]
    fn separate_module_beams_use_each_confirmed_target() {
        let mut snap = snapshot(0, 100);
        let pilot = &mut snap.pilots[0];
        pilot.locked_asteroids = vec![7, 8];
        pilot.mining_modules = [7, 8]
            .into_iter()
            .enumerate()
            .map(|(slot, id)| MiningModule {
                module_id: slot as u8,
                target_id: Some(id),
                cycle_started_at_ms: Some(100),
                cycle_completes_at_ms: Some(6100),
                stop_reason: None,
            })
            .collect();
        assert_eq!(module_cycle(pilot, 0, 1000).unwrap().target_id, 7);
        assert_eq!(module_cycle(pilot, 1, 1000).unwrap().target_id, 8);
        pilot.locked_asteroids.retain(|id| *id != 8);
        assert!(module_cycle(pilot, 1, 1000).is_none());
        assert!(module_cycle(pilot, 0, 1000).is_some());
    }

    #[test]
    fn active_module_visuals_precede_rewards_and_stop_on_server_state() {
        let mut snap = snapshot(0, 25);
        let pilot = &mut snap.pilots[0];
        pilot.mining_modules = vec![MiningModule {
            module_id: 0,
            target_id: Some(7),
            cycle_started_at_ms: Some(100),
            cycle_completes_at_ms: Some(6100),
            stop_reason: None,
        }];
        let cycle = module_cycle(pilot, 0, 3100).unwrap();
        assert_eq!(cycle.target_id, 7);
        assert_eq!(cycle.progress, 0.5);
        assert_eq!(cycle.remaining_ms, 3000);
        assert_eq!(pilot.ore, 0);
        assert!(module_cycle(pilot, 1, 3100).is_none());
        assert!(
            module_cycle(pilot, 0, 6200).is_some(),
            "active server state survives pending cycle snapshot"
        );
        pilot.mining_modules[0].target_id = None;
        assert!(module_cycle(pilot, 0, 3200).is_none());
        pilot.mining_modules[0].target_id = Some(7);
        pilot.location = Location::Docked;
        assert!(module_cycle(pilot, 0, 3200).is_none());
    }
    #[test]
    fn confirmed_target_extraction_animates_once_and_expires() {
        let mut effects = Effects::default();
        let before = snapshot(0, 25);
        let after = snapshot(25, 0);
        effects.observe(&before, &after, "Pilot", 100);
        effects.observe(&after, &after, "Pilot", 101);
        assert_eq!(effects.events.len(), 1);
        assert_eq!(effects.mining(100), Some((0.0, 25)));
        assert_eq!(effects.mining_target(100), Some(7));
        assert_eq!(effects.mining_target(6100), None);
        effects.retain(6100);
        assert!(effects.events.is_empty());
    }
    #[test]
    fn unrelated_ore_updates_and_changed_targets_do_not_create_beams() {
        let before = snapshot(0, 100);
        let mut after = snapshot(25, 100);
        let mut effects = Effects::default();
        effects.observe(&before, &after, "Pilot", 100);
        assert!(effects.events.is_empty());
        after.asteroids[0].ore = 75;
        after.pilots[0].locked_asteroid = Some(8);
        effects.observe(&before, &after, "Pilot", 100);
        assert!(effects.events.is_empty());
        after.pilots[0].locked_asteroid = Some(7);
        after.pilots[0].system_id = 2;
        effects.observe(&before, &after, "Pilot", 100);
        assert!(effects.events.is_empty());
    }
    #[test]
    fn surface_range_and_system_gate_the_exact_target() {
        let mut state = snapshot(0, 100);
        let rock = &state.asteroids[0];
        let pilot = &mut state.pilots[0];
        pilot.flight.as_mut().unwrap().position =
            [rock.position[0] + rock.radius + MINING_RANGE, 0.0, 0.0];
        assert!(can_extract_target(pilot, rock));
        pilot.flight.as_mut().unwrap().position[0] += 0.1;
        assert!(!can_extract_target(pilot, rock));
        pilot.flight.as_mut().unwrap().position = rock.position;
        pilot.location = Location::Docked;
        assert!(!can_extract_target(pilot, rock));
        pilot.location = Location::Jumping {
            gate_id: 101,
            arrives_at_ms: 1000,
        };
        assert!(!can_extract_target(pilot, rock));
    }
}
