//! Nearby physical controls, not crosshair picking or local simulation.
use super::*;
use arkline_protocol::{
    camera_segment_station, dynamic_geometry, registered_geometry, PioneerDockedActuators,
    PioneerDockedGeometry, RoomLayoutDocument,
};

#[derive(Clone, PartialEq)]
pub(super) struct NearbyAction {
    pub(super) request: DockedTraversalRequest,
    pub(super) label: String,
    pub(super) blocked_reason: Option<&'static str>,
    pub(super) distance_m: f64,
    pub(super) target_id: String,
}

#[derive(Default)]
pub(super) struct NearbyGeometryCache {
    document: Option<RoomLayoutDocument>,
    geometry: Option<PioneerDockedGeometry>,
}
impl NearbyGeometryCache {
    fn get(&mut self, document: &RoomLayoutDocument) -> Option<&PioneerDockedGeometry> {
        if self.document.as_ref() != Some(document) {
            self.geometry = registered_geometry(document).ok();
            self.document = Some(document.clone());
        }
        self.geometry.as_ref()
    }
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// Control anchors are authoritative reach positions at floor level. The ray
/// ends above that SAME position, not behind a closed door or on the far side.
fn reachable_control(
    geometry: &PioneerDockedGeometry,
    dynamic: &PioneerDockedGeometry,
    pose: PioneerDockedPose,
    feet: [f64; 3],
    anchor: [f64; 3],
    range: f64,
) -> Option<f64> {
    let distance = distance(feet, anchor);
    if !distance.is_finite() || distance > range {
        return None;
    }
    let eye = [feet[0], feet[1] + 1.65, feet[2]];
    let control = [anchor[0], anchor[1] + 1.2, anchor[2]];
    camera_segment_station(geometry, dynamic, pose, eye, control, 0.01)
        .is_none()
        .then_some(distance)
}

/// Never choose a different ready action behind a closer blocked one. Nearby
/// controls within 25 cm of the same reach distance are ambiguous: use buttons.
pub(super) fn unambiguous_action(mut candidates: Vec<NearbyAction>) -> Option<NearbyAction> {
    candidates.sort_by(|a, b| a.distance_m.total_cmp(&b.distance_m));
    if candidates
        .get(1)
        .is_some_and(|next| next.distance_m - candidates[0].distance_m <= 0.25)
    {
        None
    } else {
        candidates.into_iter().next()
    }
}

impl Cockpit {
    pub(super) fn nearby_ship_action(&mut self) -> Option<NearbyAction> {
        let binding = self.verified_docked_binding()?;
        let pilot = self.own()?;
        let pose = docked_binding_pose(&binding).ok()?;
        let Some(state) = pilot
            .station_avatar
            .as_ref()
            .and_then(|avatar| avatar.docked_traversal.as_ref())
        else {
            if pilot.station_avatar.is_some() {
                return None;
            }
            let blocked = self.docked_leave_seat_block_reason();
            return Some(NearbyAction {
                request: DockedTraversalRequest::LeaveSeat { binding },
                label: "Leave pilot seat".into(),
                blocked_reason: blocked,
                distance_m: 0.0,
                target_id: "pilot-seat/leave".into(),
            });
        };
        let document = self
            .assets
            .as_ref()
            .and_then(|assets| assets.pioneer_room_layout.as_ref())
            .map(|layout| layout.document.clone())
            .unwrap_or_else(default_pioneer_room_layout);
        let dynamic = dynamic_geometry(
            pose,
            PioneerDockedActuators {
                hatch_fractions: Some(state.actuators.hatch_fractions()),
                room_door_open: self
                    .assets
                    .as_ref()
                    .and_then(|assets| assets.pioneer_room.as_ref())
                    .is_some_and(|room| room.door_open),
                inner_hatch_open: state.actuators.inner_hatch_open,
                outer_hatch_open: state.actuators.outer_hatch_open,
                lift_floor_station_m: state.actuators.lift.floor_station_m,
                chair_fold: state
                    .seat_transition
                    .as_ref()
                    .map_or(1.0, |phase| phase.chair_fold(self.presentation_time())),
            },
        )
        .ok()?;
        let geometry = self.nearby_geometry.get(&document)?;
        let mut candidates = Vec::new();
        if let Some(distance_m) = reachable_control(
            geometry,
            &dynamic,
            pose,
            state.feet_m,
            seat_anchors(pose).root_station_m,
            1.0,
        ) {
            candidates.push(NearbyAction {
                request: DockedTraversalRequest::TakeSeat {
                    binding: binding.clone(),
                },
                label: "Return to pilot seat".into(),
                blocked_reason: None,
                distance_m,
                target_id: "pilot-seat/take".into(),
            });
        }
        if pilot.location == Location::Docked {
            let anchors = hatch_anchors(pose);
            for (hatch, index, name, inside, outside) in [
                (
                    DockedHatch::Inner,
                    0,
                    "inner hatch",
                    anchors.inner_inside_station_m,
                    anchors.inner_outside_station_m,
                ),
                (
                    DockedHatch::Outer,
                    1,
                    "outer hatch",
                    anchors.outer_inside_station_m,
                    anchors.outer_outside_station_m,
                ),
            ] {
                let side = [("inside", inside), ("outside", outside)]
                    .into_iter()
                    .filter_map(|(side, anchor)| {
                        reachable_control(geometry, &dynamic, pose, state.feet_m, anchor, 3.0)
                            .map(|distance| (side, distance))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((side, distance_m)) = side {
                    let open = Self::boarding_hatch_open_intent(state, index);
                    candidates.push(NearbyAction {
                        request: DockedTraversalRequest::SetHatch {
                            binding: binding.clone(),
                            hatch,
                            open,
                        },
                        label: format!("{} {name}", if open { "Open" } else { "Close" }),
                        blocked_reason: None,
                        distance_m,
                        target_id: format!("boarding/{name}/{side}"),
                    });
                }
            }
            let lift = lift_geometry(pose);
            let calls = lift_call_anchors(pose);
            let current = &state.actuators.lift;
            let destination = if !current.moving
                && (current.floor_station_m - lift.ship_floor_m).abs() <= 1.0e-8
            {
                Some((
                    DockedLiftStop::Station,
                    "Lift to station",
                    calls.ship_station_m,
                    "ship",
                ))
            } else if !current.moving
                && (current.floor_station_m - lift.station_floor_m).abs() <= 1.0e-8
            {
                Some((
                    DockedLiftStop::Ship,
                    "Lift to ship",
                    calls.station_station_m,
                    "station",
                ))
            } else {
                None
            };
            if let Some((stop, label, anchor, side)) = destination {
                if let Some(distance_m) =
                    reachable_control(geometry, &dynamic, pose, state.feet_m, anchor, 3.0)
                {
                    candidates.push(NearbyAction {
                        request: DockedTraversalRequest::CallLift {
                            binding: binding.clone(),
                            stop,
                        },
                        label: label.into(),
                        blocked_reason: None,
                        distance_m,
                        target_id: format!("boarding/lift/{side}"),
                    });
                }
            }
        }
        // End the cache borrow before consulting the same readiness helpers as
        // the explicit buttons. No candidate submits or changes world state.
        for candidate in &mut candidates {
            candidate.blocked_reason = match candidate.request {
                DockedTraversalRequest::TakeSeat { .. } => self.docked_seat_block_reason(state),
                DockedTraversalRequest::SetHatch { hatch, open, .. } => {
                    self.boarding_hatch_block_reason(state, hatch, open)
                }
                DockedTraversalRequest::CallLift { .. } => self.boarding_lift_block_reason(state),
                _ => return None,
            };
        }
        unambiguous_action(candidates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearby_interaction_cpu_visibility_blocks_real_closed_hatch_and_range() {
        let pose = PioneerDockedPose::private();
        let document = default_pioneer_room_layout();
        let geometry = registered_geometry(&document).unwrap();
        let anchors = hatch_anchors(pose);
        let mut actuators = PioneerDockedActuators {
            hatch_fractions: Some([0.0, 0.0]),
            room_door_open: false,
            inner_hatch_open: false,
            outer_hatch_open: false,
            lift_floor_station_m: lift_geometry(pose).ship_floor_m,
            chair_fold: 1.0,
        };
        let dynamic = dynamic_geometry(pose, actuators).unwrap();
        assert!(
            reachable_control(
                &geometry,
                &dynamic,
                pose,
                anchors.inner_inside_station_m,
                anchors.inner_outside_station_m,
                3.0
            )
            .is_none(),
            "Closed physical hatch must obstruct the far-side control"
        );
        actuators.hatch_fractions = Some([1.0, 0.0]);
        actuators.inner_hatch_open = true;
        let dynamic = dynamic_geometry(pose, actuators).unwrap();
        assert!(reachable_control(
            &geometry,
            &dynamic,
            pose,
            anchors.inner_inside_station_m,
            anchors.inner_outside_station_m,
            3.0
        )
        .is_some());
        assert!(reachable_control(
            &geometry,
            &dynamic,
            pose,
            anchors.inner_inside_station_m,
            anchors.inner_outside_station_m,
            1.0
        )
        .is_none());
        assert!(reachable_control(
            &geometry,
            &dynamic,
            pose,
            [f64::NAN; 3],
            anchors.inner_outside_station_m,
            3.0
        )
        .is_none());
    }

    #[test]
    fn nearby_interaction_cache_rejects_invalid_document_instead_of_using_old_geometry() {
        let mut cache = NearbyGeometryCache::default();
        let mut document = default_pioneer_room_layout();
        assert!(cache.get(&document).is_some());
        document.rooms[0].clear_lwh_m[0] = -1.0;
        assert!(cache.get(&document).is_none());
        assert!(cache.get(&document).is_none());
        assert!(cache.get(&default_pioneer_room_layout()).is_some());
    }
}
