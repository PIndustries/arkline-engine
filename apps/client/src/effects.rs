//! Short presentation cues derived from confirmed snapshot transitions.
use arkline_protocol::{Location, Snapshot};

pub const EFFECT_DURATION_MS: u64 = 2400;
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EffectKind {
    Mining { units: u64 },
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
        let kind = if before.location == Location::Belt
            && after.location == Location::Belt
            && after.ore > before.ore
        {
            Some(EffectKind::Mining {
                units: after.ore - before.ore,
            })
        } else if before.location == Location::Station && after.location == Location::Docked {
            Some(EffectKind::Docking)
        } else if before.location == Location::Docked && after.location == Location::Station {
            Some(EffectKind::Undocking)
        } else {
            None
        };
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
        self.events.iter().rev().find_map(|event| {
            let age = now.saturating_sub(event.started_ms);
            match event.kind {
                EffectKind::Mining { units } if age < EFFECT_DURATION_MS => {
                    Some((age as f32 / EFFECT_DURATION_MS as f32, units))
                }
                _ => None,
            }
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use arkline_protocol::*;
    fn snapshot(location: Location, ore: u64) -> Snapshot {
        Snapshot {
            server_time_ms: 100,
            pilots: vec![PilotView {
                name: "Pilot".into(),
                credits: 1000,
                ore,
                location,
            }],
            orders: vec![],
            belt_ore: INITIAL_BELT_ORE - ore,
            ledger_sequence: 0,
            ledger_hash: GENESIS_HASH.into(),
        }
    }
    #[test]
    fn confirmed_extraction_animates_once_and_expires() {
        let mut effects = Effects::default();
        let before = snapshot(Location::Belt, 0);
        let after = snapshot(Location::Belt, 25);
        effects.observe(&before, &after, "Pilot", 100);
        effects.observe(&after, &after, "Pilot", 101);
        assert_eq!(effects.events.len(), 1);
        assert_eq!(effects.mining(100), Some((0.0, 25)));
        assert_eq!(effects.mining(2500), None);
        effects.retain(2500);
        assert!(effects.events.is_empty());
    }
    #[test]
    fn purchases_and_arrival_do_not_look_like_extraction() {
        let mut effects = Effects::default();
        effects.observe(
            &snapshot(Location::Docked, 0),
            &snapshot(Location::Docked, 25),
            "Pilot",
            100,
        );
        effects.observe(
            &snapshot(Location::Station, 0),
            &snapshot(Location::Belt, 25),
            "Pilot",
            100,
        );
        assert!(effects.events.is_empty());
        effects.observe(
            &snapshot(Location::Station, 25),
            &snapshot(Location::Docked, 25),
            "Pilot",
            100,
        );
        assert_eq!(effects.events[0].kind, EffectKind::Docking);
        effects.clear();
        assert!(effects.events.is_empty());
    }
}
