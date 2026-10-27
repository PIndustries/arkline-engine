//! Response-only projection. Durable ownership is not reconstructed from pixels.
use super::*;
pub(crate) fn public_view(
    w: &World,
    name: &str,
    now: u64,
) -> Result<Option<CarrierInventoryPublicState>> {
    let Some(s) = state(w, name) else {
        return Ok(None);
    };
    let mut motions = s
        .runs
        .values()
        .map(|r| CarrierInventoryMotionView {
            operation_id: r.operation_id,
            drone_id: r.drone_id,
            route: CarrierPresentationRoute::Hardware {
                route: r.kind.clone(),
                target_index: r.target_index,
            },
            // A reached transfer barrier is stationary, not a moving segment
            // whose clock keeps extending beyond its physical endpoint. The
            // private accepted latch segment/receipt is retained unchanged.
            source: if r.waiting_for_transfer {
                r.confirmed.clone()
            } else {
                r.source.clone()
            },
            confirmed: r.confirmed.clone(),
            started_at_ms: if r.waiting_for_transfer {
                r.confirmed_at_ms
            } else {
                r.started_at_ms
            },
            duration_ms: if r.waiting_for_transfer {
                1
            } else {
                r.duration_ms
            },
            confirmed_at_ms: r.confirmed_at_ms,
            held: r.held,
            hold_reason: r.hold_reason.clone(),
        })
        .collect::<Vec<_>>();
    for a in s.arrivals.values() {
        motions.push(CarrierInventoryMotionView {
            operation_id: a.operation_id,
            drone_id: a.drone_id,
            route: CarrierPresentationRoute::Approach {
                berth_id: a.berth_id,
                cassette_id: a.cassette_id,
                path: a.path.clone(),
                target_index: a.target_index,
            },
            source: CarrierMotionFrame {
                poses: vec![(CarrierPoseKey::Drone, a.path[a.target_index - 1])],
            },
            confirmed: CarrierMotionFrame {
                poses: vec![(CarrierPoseKey::Drone, a.confirmed)],
            },
            started_at_ms: a.started_at_ms,
            duration_ms: a.duration_ms,
            confirmed_at_ms: a.confirmed_at_ms,
            held: a.held,
            hold_reason: a.hold_reason.clone(),
        });
    }
    for d in s.departures.values() {
        let op = s
            .inventory
            .operation(d.operation_id)
            .context("Departure receipt missing")?;
        motions.push(CarrierInventoryMotionView {
            operation_id: d.operation_id,
            drone_id: d.drone_id,
            route: CarrierPresentationRoute::Departure {
                berth_id: op.berth_id.context("Departure berth missing")?,
                cassette_id: d.cassette_id,
            },
            source: CarrierMotionFrame {
                poses: vec![(CarrierPoseKey::Drone, d.source)],
            },
            confirmed: CarrierMotionFrame {
                poses: vec![(CarrierPoseKey::Drone, d.confirmed)],
            },
            started_at_ms: d.started_at_ms,
            duration_ms: d.duration_ms,
            confirmed_at_ms: d.confirmed_at_ms,
            held: d.held,
            hold_reason: d.hold_reason.clone(),
        });
    }
    let flight = w.pilots[name]
        .view
        .flight
        .as_ref()
        .context("Carrier presentation parent missing")?;
    let current_parent = BayPose {
        position_m: flight.position,
        orientation_xyzw: flight.orientation,
    };
    let registration = &s.inventory.binding.registration;
    let cameras = arkline_protocol::carrier_camera_devices::for_exact_source(
        registration.assembly_revision,
        &registration.assembly_source_sha256,
    )
    .map(|camera_def| {
        arkline_protocol::carrier_camera_devices::project(
            camera_def,
            &s.inventory.binding.vessel_id,
            current_parent,
            &s.machinery,
            w.pilots[name].view.power.as_ref(),
            now,
        )
    })
    .transpose()
    .map_err(anyhow::Error::msg)?;
    let view = CarrierInventoryPublicState {
        sample_time_ms: now,
        parent: current_parent,
        cameras,
        inventory: s.inventory.clone(),
        ore: s.ore.clone(),
        mining_blocks: s
            .mining_blocks
            .iter()
            .map(|(id, r)| (*id, r.clone()))
            .collect(),
        drone_poses: s.drone_poses.iter().map(|(i, p)| (*i, *p)).collect(),
        cassette_poses: s.cassette_poses.iter().map(|(i, p)| (*i, *p)).collect(),
        machinery: s.machinery.iter().map(|(i, p)| (i.clone(), *p)).collect(),
        motions,
    };
    view.validate_projection(&w.pilots[name].view)
        .map_err(anyhow::Error::msg)?;
    Ok(Some(view))
}
