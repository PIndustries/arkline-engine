//! Exact 1703-only reverse routes. Neither legacy reverse behavior nor normal
//! admission is extended by shape/capacity similarity. Runtime owns zero-payload
//! admission, source-bound clocks, identity/HP persistence and all six resources.
//! Retain the full exchange writer set until the final safe-rest frame: no
//! leg-two release, no new ore service and no semantic-only cassette relocation.
use super::*;

impl CarrierInventoryDefinition {
    fn parallel_reverse_component(&self, class: DroneClass, berth: u8, cid: u64) -> bool {
        if self.topology.registration.assembly_revision != 1703
            || !self.has_complete_compound_collision()
            || self
                .topology
                .stage_loader(u16::from(berth), berth, class)
                .is_none()
        {
            return false;
        }
        // Reuse the authoritative registered graph, never a same-side shortcut
        // or a permanent home-role rule. Actual cassette roles may exchange.
        self.cassette_reaches_dock(cid, CarrierCassetteDock::Ready { berth_id: berth })
    }

    pub(super) fn parallel_capture_route(
        &self,
        berth: u8,
        class: DroneClass,
        cassette: u64,
    ) -> Option<Vec<CarrierInventoryKeyframe>> {
        if !self.parallel_reverse_component(class, berth, cassette) {
            return None;
        }
        let mut frames = self.bay_route(berth, class, cassette, CarrierInventoryAction::Launch)?;
        if frames.len() != 7 {
            return None;
        }
        frames.reverse();
        let labels = [
            "parallel capture entry: exterior body, hardware secured",
            "outer door opens before supported capture",
            "empty cradle extends within tunnel for capture",
            "returning body reaches supported capture position",
            "cradle clamps secure returning body",
            "captured body retracts into Ready cassette",
            "Ready capture complete: door closed, hardware safe",
        ];
        for (index, frame) in frames.iter_mut().enumerate() {
            frame.label = labels[index].into();
            frame.phase = CarrierOperationPhase::Returning;
            frame.milestones.hardware_safe = index == labels.len() - 1;
        }
        Some(frames)
    }

    pub(super) fn parallel_reverse_stage_route(
        &self,
        class: DroneClass,
        source_berth: u8,
        destination_slot: u16,
        loaded: u64,
        empty: u64,
    ) -> Option<Vec<CarrierInventoryKeyframe>> {
        if destination_slot != u16::from(source_berth)
            || loaded == empty
            || !self.parallel_reverse_component(class, source_berth, loaded)
            || !self.parallel_reverse_component(class, source_berth, empty)
        {
            return None;
        }
        let forward = self.stage_route(class, destination_slot, source_berth, loaded, empty)?;
        if forward.len() != 42 {
            return None;
        }
        let targets = [
            (
                empty,
                CarrierCassetteDock::TransitBuffer {
                    buffer_id: source_berth,
                },
            ),
            (
                loaded,
                CarrierCassetteDock::Protected {
                    slot_id: destination_slot,
                },
            ),
            (
                empty,
                CarrierCassetteDock::Ready {
                    berth_id: source_berth,
                },
            ),
        ];
        let mut completed = 0usize;
        let mut result = Vec::with_capacity(forward.len());
        for (index, original) in forward.iter().rev().enumerate() {
            let source_index = forward.len() - 1 - index;
            let mut frame = original.clone();
            frame.phase = CarrierOperationPhase::Loading;
            frame.label = if index == 0 {
                "parallel reverse entry: loaded Ready, empty Protected".into()
            } else {
                reverse_parallel_event(&forward[source_index + 1].label)?.into()
            };
            // Recompute actual endpoint docking/support, including the overlap
            // while the original conservative forward map still omitted a dock.
            frame.docked_cassettes = frame
                .cassette_poses
                .iter()
                .filter_map(|(cid, pose)| {
                    self.docks
                        .iter()
                        .find(|d| d.cassette_pose == *pose)
                        .map(|d| (*cid, d.dock))
                })
                .collect();
            if frame.handled_cassette_id.is_none() {
                frame.handled_cassette_id = Some(empty);
            }
            let handled_dock = frame
                .docked_cassettes
                .iter()
                .find(|(cid, _)| Some(*cid) == frame.handled_cassette_id)
                .map(|(_, dock)| *dock);
            frame.milestones.cassette_supported = handled_dock.is_some();
            frame.milestones.cassette_locked = handled_dock.is_some_and(|dock| {
                let latches: Vec<_> = self
                    .mechanisms
                    .iter()
                    .filter(|m| {
                        m.owner == CarrierMechanismOwner::Dock { dock }
                            && m.semantic_id.contains("/latch/")
                    })
                    .collect();
                // Protected floor/support parts are not latch evidence. Local
                // buffer latch roots are cellXX/buffer/latch, not old dock/buffer.
                latches.len() == 2
                    && latches.iter().all(|m| {
                        frame
                            .machinery
                            .iter()
                            .any(|(g, pose)| g == &m.semantic_id && pose == &m.reference_pose)
                    })
            });
            if completed < targets.len()
                && frame.handled_cassette_id == Some(targets[completed].0)
                && frame.docked_cassettes.contains(&targets[completed])
                && frame.milestones.cassette_supported
                && frame.milestones.cassette_locked
                && frame.milestones.loader_clear
                && !frame.milestones.crane_latched
                && frame.carried_cassette_id.is_none()
            {
                completed += 1;
                frame.label = format!("parallel reverse leg {completed}: supported, locked, released and loader clear");
            }
            frame.completed_stage_legs = completed as u8;
            frame.milestones.hardware_safe = source_index == 0 && completed == 3;
            if frame.milestones.hardware_safe {
                frame.label =
                    "parallel reverse complete: loaded Protected, empty Ready, hardware safe"
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

/// An edge's reverse action, not its obsolete forward pickup/drop label.
fn reverse_parallel_event(edge: &str) -> Option<&'static str> {
    Some(match edge {
        "local exchange complete lower launch corridor clear" => "leave local safe rest",
        "empty lower buffer resets" => "lower buffer jaws open before empty return",
        "hoist returns local safe rest" => "empty hoist approaches Protected cassette",
        "hoist lifts clear" => "hoist lowers into locked Protected cassette eyes",
        "protected hoist latches release" => "hoist retains empty before Protected unlock",
        "protected dock 0.0" => "Protected dock unlocks while hoist retains empty",
        "empty cassette seats protected pads" => "empty cassette lifts clear of Protected supports",
        "protected supports extend below lifted cassette" => {
            "Protected supports retract beneath retained empty"
        }
        "protected floor closes below lifted cassette" => {
            "Protected floor opens beneath retained empty"
        }
        "empty cassette lifts through open protected shelf" => {
            "empty cassette lowers through open shelf onto buffer pads"
        }
        "lower buffer releases" => "lower buffer locks before hoist release",
        "lower cassette eye latches engage" => "hoist releases empty after lower buffer lock",
        "pin enters lower empty cassette eyes" => "hoist withdraws clear of buffer cassette",
        "empty hoist aligns lower buffer shaft" => "empty hoist leaves lower buffer shaft",
        "protected floor slides into closed ready roof pocket" => {
            "Protected floor closes with empty safely below"
        }
        "protected pads and tracks retract" => "Protected supports extend for loaded arrival",
        "loaded ready locked but launch corridor remains blocked" => {
            "loaded Ready pickup preparation"
        }
        "ready hatch closes before protected opening" => {
            "Ready hatch opens after Protected floor closes"
        }
        "hoist clears ready shaft" => "hoist enters locked loaded Ready cassette eyes",
        "ready supported release hoist" => "hoist retains loaded before Ready unlock",
        "ready dock 0.0" => "Ready dock unlocks while hoist retains loaded",
        "loaded cassette lowers into ready" => "loaded cassette lifts out of Ready shaft",
        "loaded cassette translates to ready shaft" => {
            "loaded cassette translates to Protected shaft"
        }
        "loaded cassette lifts off pads" => "loaded cassette seats on Protected supports",
        "protected dock 1.0" => "Protected dock locks before loaded hoist release",
        "protected eye latches engage" => "hoist releases loaded after Protected lock",
        "pin enters protected eyes" => "hoist withdraws clear of loaded Protected cassette",
        "empty hoist to protected row" => "hoist returns to local Ready rest",
        "ready transfer hatch opens" => "Ready transfer hatch closes before shuttle return",
        "empty secured lower buffer" => "empty lower buffer pickup preparation",
        "shuttle returns under ready floor" => "lowered open shuttle approaches buffer cassette",
        "shuttle releases supported cassette" => "shuttle rises to supported buffer cassette",
        "shuttle clamps release after buffer locks" => "shuttle retains empty before buffer unlock",
        "lower buffer locks" => "lower buffer unlocks while shuttle retains empty",
        "empty cassette seats lower buffer" => "shuttle lifts empty clear of buffer pads",
        "empty cassette translates to lower pocket" => "shuttle carries empty into Ready",
        "lower buffer jaws open before arrival" => "vacated lower buffer jaws close",
        "shuttle lifts empty clear of ready pads" => "shuttle seats empty on Ready supports",
        "ready dock 1.0" => "Ready dock locks before shuttle release",
        "shuttle side clamps retain fixed cassette channels" => {
            "shuttle releases empty after Ready lock"
        }
        "shuttle touches fixed base" => "shuttle lowers clear into safe idle",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes() -> [DroneClass; 3] {
        [DroneClass::Mining, DroneClass::Recon, DroneClass::Attack]
    }

    fn assert_physical_equal(a: &CarrierInventoryKeyframe, b: &CarrierInventoryKeyframe) {
        assert_eq!(a.drone_pose, b.drone_pose);
        assert_eq!(a.cassette_poses, b.cassette_poses);
        assert_eq!(a.machinery, b.machinery);
        assert_eq!(a.carried_cassette_id, b.carried_cassette_id);
    }

    #[test]
    fn parallel_reverse_all_cells_classes_current_ids_exact_geometry_and_directed_handoffs() {
        let d = &parallel_fixture().unwrap().definition;
        for bid in 0..10u8 {
            for class in classes() {
                for (loaded, empty) in [
                    (u64::from(bid) + 10, u64::from(bid)),
                    (u64::from(bid), u64::from(bid) + 10),
                ] {
                    let original = d
                        .stage_route(class, u16::from(bid), bid, loaded, empty)
                        .unwrap();
                    let reverse = d
                        .reverse_stage_route(class, bid, u16::from(bid), loaded, empty)
                        .unwrap();
                    assert_eq!(reverse.len(), 42);
                    let owned: Vec<_> = original[0].machinery.iter().map(|(g, _)| g).collect();
                    for group in &owned {
                        assert!(!d.mechanism_collision_parts(group).unwrap().is_empty());
                    }
                    let mut legs = 0;
                    let mut boundaries = Vec::new();
                    for (index, (frame, forward)) in
                        reverse.iter().zip(original.iter().rev()).enumerate()
                    {
                        assert_physical_equal(frame, forward);
                        assert_eq!(frame.phase, CarrierOperationPhase::Loading);
                        assert_eq!(
                            frame.machinery.iter().map(|(g, _)| g).collect::<Vec<_>>(),
                            owned
                        );
                        assert!(frame.milestones.drone_clamped);
                        assert!(!frame.milestones.ore_coupled);
                        assert_eq!(
                            frame.milestones.crane_latched,
                            frame.carried_cassette_id.is_some()
                        );
                        assert_eq!(frame.milestones.hardware_safe, index == 41);
                        assert_eq!(frame.cassette_poses.len(), 2);
                        assert!(frame
                            .cassette_poses
                            .iter()
                            .all(|(cid, _)| [loaded, empty].contains(cid)));
                        // A pose at a protected seat is supported only while
                        // its real retracting floor/pad roots remain extended.
                        for (_, dock) in &frame.docked_cassettes {
                            for m in d.mechanisms.iter().filter(|m| {
                                m.owner == CarrierMechanismOwner::Dock { dock: *dock }
                                    && !m.semantic_id.contains("/latch/")
                            }) {
                                assert!(frame
                                    .machinery
                                    .contains(&(m.semantic_id.clone(), m.reference_pose)));
                            }
                        }
                        for (cid, dock) in &frame.docked_cassettes {
                            assert_eq!(
                                frame
                                    .cassette_poses
                                    .iter()
                                    .find(|(id, _)| id == cid)
                                    .unwrap()
                                    .1,
                                d.docks
                                    .iter()
                                    .find(|g| &g.dock == dock)
                                    .unwrap()
                                    .cassette_pose
                            );
                        }
                        if frame.completed_stage_legs != legs {
                            assert_eq!(frame.completed_stage_legs, legs + 1);
                            let expected = match frame.completed_stage_legs {
                                1 => (empty, CarrierCassetteDock::TransitBuffer { buffer_id: bid }),
                                2 => (
                                    loaded,
                                    CarrierCassetteDock::Protected {
                                        slot_id: u16::from(bid),
                                    },
                                ),
                                3 => (empty, CarrierCassetteDock::Ready { berth_id: bid }),
                                _ => panic!("invalid reverse leg"),
                            };
                            assert_eq!(frame.handled_cassette_id, Some(expected.0));
                            assert!(frame.docked_cassettes.contains(&expected));
                            assert!(
                                frame.milestones.cassette_supported
                                    && frame.milestones.cassette_locked
                                    && frame.milestones.loader_clear
                                    && !frame.milestones.crane_latched
                            );
                            legs = frame.completed_stage_legs;
                            boundaries.push(index);
                        }
                    }
                    assert_eq!(boundaries, [13, 27, 41]);
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
                            for f in pair {
                                assert!(
                                    f.milestones.cassette_supported && f.milestones.cassette_locked
                                );
                            }
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
                    assert_eq!(
                        original,
                        d.stage_route(class, u16::from(bid), bid, loaded, empty)
                            .unwrap()
                    );
                    assert!(d
                        .stage_handoff_route(class, u16::from(bid), bid, loaded, empty)
                        .is_none());
                }
            }
        }
    }

    #[test]
    fn parallel_capture_all_cells_classes_and_both_cids_are_exact_reverse_launch() {
        let d = &parallel_fixture().unwrap().definition;
        for bid in 0..10u8 {
            for class in classes() {
                for cid in [u64::from(bid), u64::from(bid) + 10] {
                    let launch = d
                        .bay_route(bid, class, cid, CarrierInventoryAction::Launch)
                        .unwrap();
                    let capture = d
                        .bay_route(bid, class, cid, CarrierInventoryAction::RecallToStorage)
                        .unwrap();
                    assert_eq!(capture.len(), 7);
                    for (group, _) in &capture[0].machinery {
                        assert!(!d.mechanism_collision_parts(group).unwrap().is_empty());
                    }
                    for (index, (frame, source)) in
                        capture.iter().zip(launch.iter().rev()).enumerate()
                    {
                        assert_physical_equal(frame, source);
                        assert_eq!(frame.phase, CarrierOperationPhase::Returning);
                        assert_eq!(
                            frame.milestones.drone_clamped,
                            source.milestones.drone_clamped
                        );
                        assert_eq!(frame.milestones.door_clear, source.milestones.door_clear);
                        assert_eq!(frame.milestones.hardware_safe, index == 6);
                        assert!(!frame.milestones.ore_coupled);
                        assert_eq!(frame.completed_stage_legs, 0);
                        assert_eq!(frame.docked_cassettes, source.docked_cassettes);
                    }
                    assert!(
                        capture[0].milestones.exterior_clear
                            && !capture[0].milestones.drone_clamped
                    );
                    assert!(!capture[3].milestones.drone_clamped);
                    assert!(capture[4].milestones.drone_clamped);
                    assert!(
                        capture[6].milestones.cassette_supported
                            && capture[6].milestones.cassette_locked
                            && capture[6].milestones.loader_clear
                            && !capture[6].milestones.door_clear
                    );
                    assert_eq!(
                        launch,
                        d.bay_route(bid, class, cid, CarrierInventoryAction::Launch)
                            .unwrap()
                    );
                }
            }
        }
    }

    #[test]
    fn parallel_capture_reverse_stage_restage_sequences_are_continuous_and_restart_deterministic() {
        let d = &parallel_fixture().unwrap().definition;
        // A serialized definition/snapshot is only a provider restart hook;
        // authority separately proves clocks, saved ownership, HP and resources.
        let restored: CarrierInventoryDefinition =
            serde_json::from_slice(&serde_json::to_vec(d).unwrap()).unwrap();
        for bid in 0..10u8 {
            for class in classes() {
                for (loaded, empty) in [
                    (u64::from(bid), u64::from(bid) + 10),
                    (u64::from(bid) + 10, u64::from(bid)),
                ] {
                    let capture = d
                        .bay_route(bid, class, loaded, CarrierInventoryAction::RecallToStorage)
                        .unwrap();
                    let reverse = d
                        .reverse_stage_route(class, bid, u16::from(bid), loaded, empty)
                        .unwrap();
                    let restage = d
                        .stage_route(class, u16::from(bid), bid, loaded, empty)
                        .unwrap();
                    let a = capture.last().unwrap();
                    let b = reverse.first().unwrap();
                    assert_eq!(a.drone_pose, b.drone_pose);
                    for pair in &a.cassette_poses {
                        assert!(b.cassette_poses.contains(pair));
                    }
                    for pair in &a.machinery {
                        assert!(
                            b.machinery.contains(pair),
                            "capture/reverse seam {:?}",
                            pair.0
                        );
                    }
                    assert_physical_equal(reverse.last().unwrap(), restage.first().unwrap());
                    let mut all_poses: Vec<_> = d
                        .cassettes
                        .iter()
                        .map(|c| (c.cassette_id, c.reference_pose))
                        .collect();
                    for pair in &reverse.last().unwrap().cassette_poses {
                        all_poses
                            .iter_mut()
                            .find(|(cid, _)| cid == &pair.0)
                            .unwrap()
                            .1 = pair.1;
                    }
                    let idle = d.idle_roots(&all_poses).unwrap();
                    for pair in &reverse.last().unwrap().machinery {
                        assert!(idle.contains(pair));
                    }
                    let replay = restored
                        .reverse_stage_route(class, bid, u16::from(bid), loaded, empty)
                        .unwrap();
                    assert_eq!(reverse, replay);
                    let snapshot: Vec<CarrierInventoryKeyframe> =
                        serde_json::from_slice(&serde_json::to_vec(&reverse).unwrap()).unwrap();
                    assert_eq!(snapshot, replay);
                    for boundary in [0, 5, 13, 27, 40, 41] {
                        assert_eq!(snapshot[boundary..], replay[boundary..]);
                    }
                }
            }
        }
    }

    #[test]
    fn parallel_reverse_rejects_wrong_source_cross_component_invalid_ids_and_tampering() {
        let d = &parallel_fixture().unwrap().definition;
        for bid in 0..10u8 {
            let foreign = u64::from((bid / 5) * 5 + (bid + 1) % 5);
            for (loaded, empty) in [
                (foreign, u64::from(bid)),
                (u64::from(bid) + 10, foreign + 10),
                (u64::from(bid), u64::from(bid)),
                (20, u64::from(bid)),
                (u64::MAX, u64::from(bid)),
            ] {
                assert!(d
                    .reverse_stage_route(DroneClass::Mining, bid, u16::from(bid), loaded, empty)
                    .is_none());
            }
            for cid in [foreign, foreign + 10, 20, u64::MAX] {
                assert!(d
                    .bay_route(
                        bid,
                        DroneClass::Mining,
                        cid,
                        CarrierInventoryAction::RecallToStorage
                    )
                    .is_none());
            }
            assert!(d
                .reverse_stage_route(
                    DroneClass::Mining,
                    bid,
                    u16::from((bid + 1) % 10),
                    u64::from(bid) + 10,
                    u64::from(bid)
                )
                .is_none());
        }
        for berth in [10, u8::MAX] {
            assert!(d
                .reverse_stage_route(DroneClass::Mining, berth, u16::from(berth), 10, 0)
                .is_none());
            assert!(d
                .bay_route(
                    berth,
                    DroneClass::Mining,
                    0,
                    CarrierInventoryAction::RecallToStorage
                )
                .is_none());
        }
        let mut forged = d.clone();
        forged.topology.registration.assembly_source_sha256 = "0".repeat(64);
        assert!(forged
            .reverse_stage_route(DroneClass::Mining, 0, 0, 10, 0)
            .is_none());
        assert!(forged
            .bay_route(
                0,
                DroneClass::Mining,
                0,
                CarrierInventoryAction::RecallToStorage
            )
            .is_none());
        let mut forged = d.clone();
        forged.mechanisms[0].reference_pose.position_m[0] += 0.001;
        assert!(forged
            .reverse_stage_route(DroneClass::Mining, 0, 0, 10, 0)
            .is_none());
        assert!(forged
            .bay_route(
                0,
                DroneClass::Mining,
                0,
                CarrierInventoryAction::RecallToStorage
            )
            .is_none());
        for old in [
            &fixture().unwrap().definition,
            &composed_fixture().unwrap().definition,
        ] {
            assert!(old
                .parallel_capture_route(0, DroneClass::Mining, 0)
                .is_none());
            assert!(old
                .parallel_reverse_stage_route(DroneClass::Mining, 0, 0, 10, 0)
                .is_none());
        }
        assert!(lookup(&d.topology.registration).is_none());
        assert!(d.berths.iter().all(|berth| !berth.ore_transfer_capable));
    }
}
