//! Deterministic audio intent from confirmed state, without device or UI access.
use super::flight3d;
use super::sound_engine::{Cue, Mix, SpatialEvent, SpatialLoop, SpatialSound};
use arkline_protocol::drone_bay_geometry::BayPose;
use arkline_protocol::*;
use std::collections::{BTreeMap, BTreeSet};
const STALE_MS: u64 = 1500;

mod lifecycle;
mod spatial;

#[cfg(test)]
use spatial::{spatial_loops, supported_bay};

#[derive(Default)]
pub struct Scene {
    connected: bool,
    previous: Option<Sample>,
    spatial_previous: Option<SpatialSample>,
}

struct Sample {
    time: u64,
    audio_time: u64,
    mining_engaged: bool,
    pilot: PilotView,
    drone_health: BTreeMap<(String, u64), u32>,
    incoming: BTreeMap<String, u64>,
}

/// Actual listener/inspection-camera pose in world metres. Right is a unit
/// world-space vector; supplying no listener deliberately silences spatial audio.
#[derive(Clone, Copy, Debug)]
pub struct AudioListener {
    pub position_m: [f64; 3],
    pub right: [f64; 3],
}

struct SpatialSample {
    time: u64,
    pilot: PilotView,
    asteroid_ore: BTreeMap<u64, u64>,
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

fn constrained(p: &PilotView, id: u64) -> bool {
    p.drone_bay
        .as_ref()
        .is_some_and(|b| b.constrained(id).is_some())
}

#[cfg(test)]
pub(crate) mod tests;
