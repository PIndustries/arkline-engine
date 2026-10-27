//! Drone responsibility facade. Keep parent call sites and simulation ordering stable.

use super::*;

mod commands;
mod lifecycle;
mod motion;
mod simulation;

pub(super) fn sync(p: &mut Pilot) {
    lifecycle::sync(p);
}

pub(super) fn clear(p: &mut Pilot) {
    lifecycle::clear(p);
}

pub(super) fn ensure_fleets(world: &mut World) -> Result<()> {
    lifecycle::ensure_fleets(world)
}

pub(super) fn handles(r: &Request) -> bool {
    commands::handles(r)
}

pub(super) fn execute(world: &mut World, name: &str, request: Request, now: u64) -> Result<()> {
    commands::execute(world, name, request, now)
}

pub(super) fn invalidate(world: &mut World) {
    lifecycle::invalidate(world);
}

pub(super) fn advance(world: &mut World, now: u64) -> Result<Vec<EconomicEvent>> {
    simulation::advance(world, now)
}

pub(super) fn validate(world: &World) -> Result<()> {
    lifecycle::validate(world)
}
