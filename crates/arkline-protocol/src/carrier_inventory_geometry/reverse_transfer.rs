//! Source-only Ready -> Protected exchange. Geometry is the exact reverse of
//! the admitted-for-inspection Stage sweep; control events are directional.
//! This module never admits a source or changes stock, HP, cargo, IDs or clocks.
//! The authority reserves BOTH cassettes, the drone, both docks and the loader /
//! buffer for the entire route. There is deliberately no early handoff API.
use super::*;

impl CarrierInventoryDefinition {
    /// Empty Protected -> buffer, loaded Ready -> Protected, empty buffer ->
    /// Ready. Arguments are CURRENT cassette identities, not home positions.
    /// Only the exact composed 1702 stationary fixture supports this operation.
    pub fn reverse_stage_route(
        &self,
        class: DroneClass,
        source_berth: u8,
        destination_slot: u16,
        loaded_cassette_id: u64,
        empty_cassette_id: u64,
    ) -> Option<Vec<CarrierInventoryKeyframe>> {
        if self.topology.registration.assembly_revision == 1703 {
            return self.parallel_reverse_stage_route(
                class,
                source_berth,
                destination_slot,
                loaded_cassette_id,
                empty_cassette_id,
            );
        }
        if self.topology.registration.assembly_revision != 1702 {
            return None;
        }
        // stage_route validates exact source/definition equality, class, side,
        // finite distinct cassette identities and the source-owned route.
        let forward = self.stage_route(
            class,
            destination_slot,
            source_berth,
            loaded_cassette_id,
            empty_cassette_id,
        )?;
        let loader_id = self
            .topology
            .stage_loader(destination_slot, source_berth, class)?;
        let buffer_id = self
            .topology
            .loaders
            .iter()
            .find(|l| l.loader_id == loader_id)?
            .transit_buffer_id;
        let targets = [
            (
                empty_cassette_id,
                CarrierCassetteDock::TransitBuffer { buffer_id },
            ),
            (
                loaded_cassette_id,
                CarrierCassetteDock::Protected {
                    slot_id: destination_slot,
                },
            ),
            (
                empty_cassette_id,
                CarrierCassetteDock::Ready {
                    berth_id: source_berth,
                },
            ),
        ];
        let mut result = Vec::with_capacity(forward.len());
        let mut completed = 0usize;
        for (reverse_index, original) in forward.iter().rev().enumerate() {
            let source_index = forward.len() - 1 - reverse_index;
            let mut frame = original.clone();
            frame.phase = CarrierOperationPhase::Loading;
            frame.label = if reverse_index == 0 {
                "reverse exchange entry: loaded Ready, empty Protected".into()
            } else {
                reverse_event(&forward[source_index + 1].label)?
            };
            // Endpoint facts, not forward execution history. A cassette can be
            // supported and crane-latched simultaneously during pickup/drop.
            frame.docked_cassettes = frame
                .cassette_poses
                .iter()
                .filter_map(|(id, pose)| {
                    self.docks
                        .iter()
                        .find(|dock| dock.cassette_pose == *pose)
                        .map(|dock| (*id, dock.dock))
                })
                .collect();
            // The last two forward entry poses had no handled body. In reverse
            // these are cleanup AFTER the empty cassette has reached Ready.
            if frame.handled_cassette_id.is_none() {
                frame.handled_cassette_id = Some(empty_cassette_id);
            }
            let handled_dock = frame
                .docked_cassettes
                .iter()
                .find(|(id, _)| Some(*id) == frame.handled_cassette_id)
                .map(|(_, dock)| *dock);
            frame.milestones.cassette_supported = handled_dock.is_some();
            frame.milestones.cassette_locked = handled_dock.is_some_and(|dock| {
                let prefix = match dock {
                    CarrierCassetteDock::Ready { berth_id } => {
                        format!("dock/ready/{berth_id}/latch/")
                    }
                    CarrierCassetteDock::Protected { slot_id } => {
                        format!("dock/protected/{slot_id}/latch/")
                    }
                    CarrierCassetteDock::TransitBuffer { buffer_id } => {
                        format!("dock/buffer/{buffer_id}/latch/")
                    }
                };
                let latches: Vec<_> = self
                    .mechanisms
                    .iter()
                    .filter(|m| m.semantic_id.starts_with(&prefix))
                    .collect();
                latches.len() == 2
                    && latches.iter().all(|m| {
                        frame
                            .machinery
                            .iter()
                            .any(|(name, pose)| name == &m.semantic_id && pose == &m.reference_pose)
                    })
            });
            if completed < targets.len()
                && frame.handled_cassette_id == Some(targets[completed].0)
                && frame.docked_cassettes.contains(&targets[completed])
                && frame.milestones.cassette_supported
                && frame.milestones.cassette_locked
                && frame.milestones.loader_clear
                && frame.carried_cassette_id.is_none()
                && !frame.milestones.crane_latched
            {
                completed += 1;
                frame.label = format!(
                    "reverse leg {completed}: destination supported, locked and loader clear"
                );
            }
            frame.completed_stage_legs = completed as u8;
            frame.milestones.hardware_safe = source_index == 0 && completed == 3;
            if frame.milestones.hardware_safe {
                frame.label =
                    "reverse exchange complete: loaded Protected, empty Ready, hardware safe"
                        .into();
            }
            result.push(frame);
        }
        let end = result.last()?;
        (completed == 3
            && end.milestones.hardware_safe
            && end.milestones.loader_clear
            && end.docked_cassettes.contains(&targets[1])
            && end.docked_cassettes.contains(&targets[2]))
        .then_some(result)
    }
}

/// Reverse the action of the edge arriving at the NEXT forward pose, rather
/// than reusing a forward label on an unrelated reverse pickup/drop event.
fn reverse_event(forward_edge: &str) -> Option<String> {
    let event = match forward_edge {
        "exchange complete loader clear hardware safe" => "leave safe rest",
        "loader returns to safe rest" => "loader leaves rest toward source row",
        "empty trolley exits reserve row" => "empty trolley enters Protected source row",
        "hoist clear of locked cassette" => "hoist approaches supported locked source cassette",
        "eye shoes released after dock lock" => "eye shoes secure before source dock release",
        "cassette seats on actual dock pads" => "crane lifts cassette clear of source pads",
        "carried cassette to destination row" => "carried cassette leaves source row for aisle",
        "carried cassette along aisle" => "carried cassette traverses aisle",
        "carried cassette to aisle" => "carried cassette approaches destination row",
        "supported cassette lifted clear" => "cassette seats on actual destination pads",
        "eye shoes secured" => "eye shoes release after destination lock",
        "pin enters source eye" => "hoist withdraws from locked destination eye",
        "pin approach above source eye" => "hoist clears locked destination",
        "empty trolley to source row" => "empty trolley leaves destination row",
        "empty trolley along aisle" => "empty trolley returns along aisle",
        "inner transfer hatch closed" => "inner transfer hatch opens for loaded Ready pickup",
        "inner transfer hatch open" => "inner transfer hatch closes after empty Ready delivery",
        "loaded cassette supported and locked in ready berth" => "loaded Ready pickup approach",
        "empty cassette supported and locked in transit buffer" => "empty buffer pickup approach",
        s if s.ends_with(" dock locked") => {
            return Some(format!(
                "reverse exchange: {} dock unlocks; crane retains any handled load",
                s.strip_suffix(" dock locked")?
            ));
        }
        s if s.ends_with(" dock released") => {
            return Some(format!(
                "reverse exchange: {} dock locks; crane retains any handled load",
                s.strip_suffix(" dock released")?
            ));
        }
        _ => return None,
    };
    Some(format!("reverse exchange: {event}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_all_fifty_routes_all_classes_preserve_every_source_pose_and_direct_handoffs() {
        let d = &composed_fixture().unwrap().definition;
        for slot in 0..10 {
            for berth in (slot / 5 * 5) as u8..(slot / 5 * 5 + 5) as u8 {
                for class in [DroneClass::Mining, DroneClass::Recon, DroneClass::Attack] {
                    // Non-home IDs, including Ready-origin ID loaded and
                    // Protected-origin ID empty, must remain actual identities.
                    let loaded = u64::from(slot / 5 * 5 + (slot + 2) % 5);
                    let empty = u64::from(slot / 5 * 5 + (slot + 3) % 5) + 10;
                    let forward = d.stage_route(class, slot, berth, loaded, empty).unwrap();
                    let reverse = d
                        .reverse_stage_route(class, berth, slot, loaded, empty)
                        .unwrap();
                    assert_eq!(forward.len(), reverse.len());
                    let mut legs = 0;
                    let mut boundaries = Vec::new();
                    for (index, (actual, source)) in
                        reverse.iter().zip(forward.iter().rev()).enumerate()
                    {
                        assert_eq!(actual.drone_pose, source.drone_pose);
                        assert_eq!(actual.cassette_poses, source.cassette_poses);
                        assert_eq!(actual.machinery, source.machinery);
                        assert_eq!(actual.carried_cassette_id, source.carried_cassette_id);
                        assert_eq!(
                            actual.milestones.crane_latched,
                            actual.carried_cassette_id.is_some()
                        );
                        assert!(actual.milestones.drone_clamped);
                        assert_eq!(actual.phase, CarrierOperationPhase::Loading);
                        assert_eq!(actual.milestones.hardware_safe, index + 1 == reverse.len());
                        assert!(
                            actual.completed_stage_legs >= legs
                                && actual.completed_stage_legs <= legs + 1
                        );
                        for (cid, dock) in &actual.docked_cassettes {
                            let pose = actual
                                .cassette_poses
                                .iter()
                                .find(|(id, _)| id == cid)
                                .unwrap()
                                .1;
                            assert_eq!(
                                pose,
                                d.docks
                                    .iter()
                                    .find(|g| &g.dock == dock)
                                    .unwrap()
                                    .cassette_pose
                            );
                        }
                        if actual.completed_stage_legs != legs {
                            assert!(
                                actual.milestones.cassette_supported
                                    && actual.milestones.cassette_locked
                            );
                            assert!(
                                actual.milestones.loader_clear && !actual.milestones.crane_latched
                            );
                            assert!(actual.carried_cassette_id.is_none());
                            let (cid, target) = match actual.completed_stage_legs {
                                1 => (
                                    empty,
                                    CarrierCassetteDock::TransitBuffer {
                                        buffer_id: (slot / 5) as u8,
                                    },
                                ),
                                2 => (loaded, CarrierCassetteDock::Protected { slot_id: slot }),
                                3 => (empty, CarrierCassetteDock::Ready { berth_id: berth }),
                                _ => panic!("invalid reverse leg"),
                            };
                            assert_eq!(actual.handled_cassette_id, Some(cid));
                            assert!(actual.docked_cassettes.contains(&(cid, target)));
                            boundaries.push(index);
                            legs = actual.completed_stage_legs;
                        }
                    }
                    assert_eq!(legs, 3);
                    assert_eq!(boundaries, [16, 33, 50]);
                    // Source timers may interpolate every adjacent segment:
                    // any moving body is latched at BOTH segment endpoints.
                    for pair in reverse.windows(2) {
                        for cid in [loaded, empty] {
                            let a = pair[0]
                                .cassette_poses
                                .iter()
                                .find(|(id, _)| *id == cid)
                                .unwrap()
                                .1;
                            let b = pair[1]
                                .cassette_poses
                                .iter()
                                .find(|(id, _)| *id == cid)
                                .unwrap()
                                .1;
                            if a != b {
                                assert_eq!(pair[0].carried_cassette_id, Some(cid));
                                assert_eq!(pair[1].carried_cassette_id, Some(cid));
                            }
                        }
                        if pair[0].carried_cassette_id.is_none()
                            && pair[1].carried_cassette_id.is_some()
                        {
                            // Grasp before unlock: both source dock and crane
                            // own the load during the pickup overlap.
                            assert!(
                                pair[0].milestones.cassette_supported
                                    && pair[0].milestones.cassette_locked
                            );
                            assert!(
                                pair[1].milestones.cassette_supported
                                    && pair[1].milestones.cassette_locked
                            );
                        }
                        if pair[0].carried_cassette_id.is_some()
                            && pair[1].carried_cassette_id.is_none()
                        {
                            assert!(
                                pair[1].milestones.cassette_supported
                                    && pair[1].milestones.cassette_locked
                            );
                        }
                    }
                    // Calling reverse must not mutate the immutable forward proof.
                    assert_eq!(
                        forward,
                        d.stage_route(class, slot, berth, loaded, empty).unwrap()
                    );
                }
            }
        }
    }

    #[test]
    fn reverse_rejects_old_unknown_tampered_cross_side_and_nonfinite_requests() {
        let old = stationary_fixture_definition().unwrap();
        assert!(old
            .reverse_stage_route(DroneClass::Mining, 0, 0, 10, 0)
            .is_none());
        let d = &composed_fixture().unwrap().definition;
        for (berth, slot, loaded, empty) in [
            (0, 5, 10, 0),
            (5, 0, 10, 0),
            (10, 0, 10, 0),
            (0, 10, 10, 0),
            (0, 0, 10, 10),
            (0, 0, 20, 0),
            (0, 0, 10, u64::MAX),
            (0, 0, 15, 0),
            (0, 0, 10, 5),
        ] {
            assert!(d
                .reverse_stage_route(DroneClass::Mining, berth, slot, loaded, empty)
                .is_none());
        }
        let mut bad = d.clone();
        bad.topology.registration.assembly_source_sha256 = "0".repeat(64);
        assert!(bad
            .reverse_stage_route(DroneClass::Mining, 0, 0, 10, 0)
            .is_none());
        let mut bad = d.clone();
        bad.docks[0].cassette_pose.position_m[0] += 0.001;
        assert!(bad
            .reverse_stage_route(DroneClass::Mining, 0, 0, 10, 0)
            .is_none());
        assert!(lookup(&d.topology.registration).is_none());
    }

    #[test]
    fn forward_reverse_forward_sequences_keep_roots_and_actual_ids_continuous() {
        let d = &composed_fixture().unwrap().definition;
        for slot in 0..10 {
            let berth = (slot / 5 * 5 + (slot + 1) % 5) as u8;
            let loaded = u64::from(slot) + 10;
            let empty = u64::from(berth);
            let forward = d
                .stage_route(DroneClass::Mining, slot, berth, loaded, empty)
                .unwrap();
            let reverse = d
                .reverse_stage_route(DroneClass::Mining, berth, slot, loaded, empty)
                .unwrap();
            for (a, b) in [
                (forward.last().unwrap(), reverse.first().unwrap()),
                (reverse.last().unwrap(), forward.first().unwrap()),
            ] {
                assert_eq!(a.drone_pose, b.drone_pose);
                assert_eq!(a.cassette_poses, b.cassette_poses);
                assert_eq!(a.machinery, b.machinery);
            }
            // Both cassette identities survive all six pickup/drop handoffs;
            // no home-CID reset and no third/unowned cassette writer appears.
            for f in forward.iter().chain(reverse.iter()) {
                assert_eq!(f.cassette_poses.len(), 2);
                assert!(f
                    .cassette_poses
                    .iter()
                    .all(|(id, _)| [loaded, empty].contains(id)));
                assert!(f
                    .machinery
                    .iter()
                    .filter(|(name, _)| name.starts_with('c'))
                    .all(|(name, _)| {
                        name.starts_with(&format!("c{loaded:02}/"))
                            || name.starts_with(&format!("c{empty:02}/"))
                    }));
            }
        }
    }
}
