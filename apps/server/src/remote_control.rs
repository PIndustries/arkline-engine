//! Explicit worker-granted piloting leases. Authentication remains the controller's.
use super::*;
pub(super) fn validate_request(r: &Request) -> Result<()> {
    ensure!(
        r.is_remote_gameplay(),
        "command is not permitted through remote control"
    );
    Ok(())
}
fn neutralize(w: &mut World, pilot: &str) {
    if let Some(p) = w.pilots.get_mut(pilot) {
        p.controls = None;
        if let Some(f) = &mut p.view.flight {
            f.angular_velocity = [0.0; 3];
            if matches!(f.mode, NavigationMode::Manual) {
                f.mode = NavigationMode::Stop;
            }
        }
    }
}
pub(super) fn release_controller(w: &mut World, controller: &str) {
    if let Some(pilot) = w.remote_controls.remove(controller) {
        neutralize(w, &pilot);
    }
}
pub(super) fn release_worker(w: &mut World, pilot: &str) {
    let controllers = w
        .remote_controls
        .iter()
        .filter(|(_, p)| p.as_str() == pilot)
        .map(|(c, _)| c.clone())
        .collect::<Vec<_>>();
    for c in controllers {
        release_controller(w, &c);
    }
}
pub(super) fn begin(w: &mut World, controller: &str, pilot: &str, now: u64) -> Result<()> {
    ensure!(controller != pilot, "remote control requires another pilot");
    resource_jobs::authorize(w, controller, pilot)?;
    ensure!(
        !w.remote_controls
            .iter()
            .any(|(c, p)| c != controller && p == pilot),
        "worker already has an active controller"
    );
    if w.remote_controls
        .get(controller)
        .is_some_and(|p| p == pilot)
    {
        return Ok(());
    }
    release_controller(w, controller);
    resource_jobs::cancel(w, pilot, now, "Remote manual controller took over");
    neutralize(w, pilot);
    w.remote_controls.insert(controller.into(), pilot.into());
    Ok(())
}
pub(super) fn validate(w: &World) -> Result<()> {
    let mut workers = std::collections::BTreeSet::new();
    for (controller, pilot) in &w.remote_controls {
        ensure!(
            controller != pilot && workers.insert(pilot),
            "invalid remote control lease"
        );
        resource_jobs::authorize(w, controller, pilot)?;
    }
    Ok(())
}
