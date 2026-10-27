//! Exact 1703 Mining socket service. The body is flight-held inside the long
//! tunnel, NOT supported by the idle cradle. Only the actual cassette remains
//! supported/locked. No pressure seal, strength, propulsion or ore rate is
//! inferred. Authority owns stable actual pose/velocity, clearance, the shared
//! manifold and payload mutation; the inserted-and-latched frame is a barrier,
//! never permission to transfer while still approaching its target.
use super::*;

impl CarrierInventoryDefinition {
    /// Exact geometric service capability only. This does not reserve a berth,
    /// prove current capture, authorize a payload batch, or change old fixtures.
    pub fn supports_ore_service(&self, berth: u8, class: DroneClass) -> bool {
        self.parallel_service_route(berth, class, u64::from(berth))
            .is_some()
    }

    pub(super) fn parallel_service_route(
        &self,
        berth: u8,
        class: DroneClass,
        cassette: u64,
    ) -> Option<Vec<CarrierInventoryKeyframe>> {
        if self.topology.registration.assembly_revision != 1703
            || class != DroneClass::Mining
            || !self.has_complete_compound_collision()
        {
            return None;
        }
        // Launch's exact-definition, registered graph and compatible-class
        // guards authenticate both the source and the actual current cassette.
        let launch = self.bay_route(berth, class, cassette, CarrierInventoryAction::Launch)?;
        let idle = launch.last()?;
        let body_origin = launch.first()?.drone_pose?;
        let prefix = format!("b{berth:02}/receiver/");
        if self
            .mechanisms
            .iter()
            .filter(|m| m.semantic_id.starts_with(&prefix))
            .count()
            != 8
        {
            return None;
        }
        let sign = if berth < 5 { -1. } else { 1. };
        // x, door, lateral arm, axial insertion, socket jaw retention.
        let states = [
            ("external clear entry", 16., false, false, false, false),
            ("outer door opens", 16., true, false, false, false),
            (
                "flight-held socket alignment inside tunnel",
                5.9,
                true,
                false,
                false,
                false,
            ),
            (
                "receiver extends behind stationary tail",
                5.9,
                true,
                true,
                false,
                false,
            ),
            (
                "head inserts into actual socket",
                5.9,
                true,
                true,
                true,
                false,
            ),
            (
                "socket jaws contact: stable-pose transfer barrier",
                5.9,
                true,
                true,
                true,
                true,
            ),
            (
                "socket jaws release before withdrawal",
                5.9,
                true,
                true,
                true,
                false,
            ),
            (
                "head withdraws clear of socket",
                5.9,
                true,
                true,
                false,
                false,
            ),
            (
                "receiver stows before body departure",
                5.9,
                true,
                false,
                false,
                false,
            ),
            (
                "body departs through open tunnel",
                16.,
                true,
                false,
                false,
                false,
            ),
            (
                "outer door closes: external clear hardware safe",
                16.,
                false,
                false,
                false,
                false,
            ),
        ];
        let mut frames = Vec::with_capacity(states.len());
        for (index, (label, x, door, arm, insert, latch)) in states.into_iter().enumerate() {
            let mut f = idle.clone();
            f.label = format!("parallel Mining service/{label}");
            f.phase = if index == 10 {
                CarrierOperationPhase::Securing
            } else {
                CarrierOperationPhase::Servicing
            };
            let mut body = body_origin;
            body.position_m[2] += sign * x;
            f.drone_pose = Some(body);
            for (id, pose) in &mut f.machinery {
                if id == &format!("b{berth:02}/door/upper") {
                    pose.position_m[1] += if door { 1.35 } else { 0. };
                } else if id == &format!("b{berth:02}/door/lower") {
                    pose.position_m[1] += if door { -1.8 } else { 0. };
                } else if let Some(suffix) = id.strip_prefix(&prefix) {
                    let lateral = if let Some(stage) = suffix.strip_prefix("stage/") {
                        let stage: u8 = stage.parse().ok()?;
                        if !(1..=5).contains(&stage) {
                            return None;
                        }
                        if arm {
                            -3.65 * f64::from(stage) / 5.
                        } else {
                            0.
                        }
                    } else {
                        let jaw = if latch {
                            match suffix {
                                "latch/port" => -0.03,
                                "latch/starboard" => 0.03,
                                "head" => 0.,
                                _ => return None,
                            }
                        } else {
                            0.
                        };
                        pose.position_m[2] += if insert { sign * 0.08 } else { 0. };
                        if arm {
                            -3.65 + jaw
                        } else {
                            jaw
                        }
                    };
                    pose.position_m[0] += -sign * lateral;
                }
            }
            f.milestones.drone_clamped = false;
            f.milestones.door_clear = door;
            f.milestones.ore_coupled = arm && insert && latch;
            f.milestones.exterior_clear = x == 16.;
            f.milestones.hardware_safe = index == 10;
            frames.push(f);
        }
        Some(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_service_all_cells_actual_ids_barrier_and_full_stable_writer_coverage() {
        let d = &parallel_fixture().unwrap().definition;
        for bid in 0..10u8 {
            assert!(d.supports_ore_service(bid, DroneClass::Mining));
            for cid in [u64::from(bid), u64::from(bid) + 10] {
                let route = d
                    .bay_route(
                        bid,
                        DroneClass::Mining,
                        cid,
                        CarrierInventoryAction::ReturnToUnload {
                            resume_mining: false,
                        },
                    )
                    .unwrap();
                assert_eq!(
                    route,
                    d.bay_route(
                        bid,
                        DroneClass::Mining,
                        cid,
                        CarrierInventoryAction::ReturnToUnload {
                            resume_mining: true
                        }
                    )
                    .unwrap()
                );
                assert_eq!(route.len(), 11);
                let launch = d
                    .bay_route(bid, DroneClass::Mining, cid, CarrierInventoryAction::Launch)
                    .unwrap();
                let idle = launch.last().unwrap();
                assert_eq!(route[0].drone_pose, idle.drone_pose);
                assert_eq!(route[10].drone_pose, idle.drone_pose);
                assert_eq!(route[0].machinery, idle.machinery);
                assert_eq!(route[10].machinery, idle.machinery);
                let sign = if bid < 5 { -1. } else { 1. };
                for (index, f) in route.iter().enumerate() {
                    assert_eq!(f.cassette_poses, idle.cassette_poses);
                    assert_eq!(f.docked_cassettes, idle.docked_cassettes);
                    assert_eq!(f.handled_cassette_id, Some(cid));
                    assert_eq!(f.carried_cassette_id, None);
                    assert_eq!(f.completed_stage_legs, 0);
                    assert!(!f.milestones.drone_clamped);
                    assert!(
                        f.milestones.cassette_supported
                            && f.milestones.cassette_locked
                            && f.milestones.loader_clear
                    );
                    assert_eq!(f.milestones.ore_coupled, index == 5);
                    assert_eq!(f.milestones.hardware_safe, index == 10);
                    assert_eq!(f.milestones.exterior_clear, matches!(index, 0 | 1 | 9 | 10));
                    assert_eq!(f.machinery.len(), idle.machinery.len());
                    let body = f.drone_pose.unwrap();
                    for ((id, pose), (idle_id, idle_pose)) in
                        f.machinery.iter().zip(&idle.machinery)
                    {
                        assert_eq!(id, idle_id);
                        assert!(d
                            .mechanism_collision_parts(id)
                            .is_some_and(|p| !p.is_empty()));
                        // All unrelated stable cassette/dock/hatch roots are preserved.
                        if !id.starts_with(&format!("b{bid:02}/receiver/"))
                            && !id.starts_with(&format!("b{bid:02}/door/"))
                        {
                            assert_eq!(pose, idle_pose);
                        }
                        if index == 5 && id == &format!("b{bid:02}/receiver/head") {
                            assert!(
                                (pose.position_m[0] - idle_pose.position_m[0] - sign * 3.65).abs()
                                    < 1e-10
                            );
                            assert!(
                                (pose.position_m[2] - idle_pose.position_m[2] - sign * 0.08).abs()
                                    < 1e-10
                            );
                        }
                    }
                    if (2..=8).contains(&index) {
                        assert_eq!(body, route[5].drone_pose.unwrap());
                    }
                }
                // Full body pose stays fixed across every coupling/release edge.
                for pair in route[2..=8].windows(2) {
                    assert_eq!(pair[0].drone_pose, pair[1].drone_pose);
                }
                let encoded = serde_json::to_string(&route).unwrap();
                let reopened: Vec<CarrierInventoryKeyframe> =
                    serde_json::from_str(&encoded).unwrap();
                assert_eq!(route, reopened);
                for boundary in 0..route.len() {
                    assert_eq!(&route[boundary..], &reopened[boundary..]);
                }
                // Service exit is an exact continuous entry to capture/recall.
                let capture = d
                    .bay_route(
                        bid,
                        DroneClass::Mining,
                        cid,
                        CarrierInventoryAction::RecallToStorage,
                    )
                    .unwrap();
                assert_eq!(route[10].drone_pose, capture[0].drone_pose);
                assert_eq!(route[10].machinery, capture[0].machinery);
                assert_eq!(
                    launch,
                    d.bay_route(bid, DroneClass::Mining, cid, CarrierInventoryAction::Launch)
                        .unwrap()
                );
            }
        }
    }

    #[test]
    fn parallel_service_rejects_non_mining_cross_component_wrong_source_and_mutation() {
        let d = &parallel_fixture().unwrap().definition;
        for bid in 0..10u8 {
            for class in [DroneClass::Recon, DroneClass::Attack] {
                assert!(!d.supports_ore_service(bid, class));
                assert!(d
                    .parallel_service_route(bid, class, u64::from(bid))
                    .is_none());
            }
            for cid in [
                u64::from((bid + 1) % 10),
                u64::from((bid + 1) % 10) + 10,
                20,
                u64::MAX,
            ] {
                assert!(d
                    .parallel_service_route(bid, DroneClass::Mining, cid)
                    .is_none());
            }
        }
        assert!(!d.supports_ore_service(10, DroneClass::Mining));
        let mut changed = d.clone();
        changed.mechanisms[0].reference_pose.position_m[0] += 0.01;
        assert!(!changed.supports_ore_service(0, DroneClass::Mining));
        let mut wrong = d.clone();
        wrong.topology.registration.assembly_source_sha256 = "0".repeat(64);
        assert!(!wrong.supports_ore_service(0, DroneClass::Mining));
        for revision in [1701, 1702] {
            let mut old = d.clone();
            old.topology.registration.assembly_revision = revision;
            assert!(!old.supports_ore_service(0, DroneClass::Mining));
        }
        assert!(d.berths.iter().all(|b| !b.ore_transfer_capable));
    }
}

#[cfg(test)]
mod source_export {
    use super::*;
    #[test]
    #[ignore = "Explicit exact source route export for independent Python comparison"]
    fn export_parallel_service_routes_for_source_comparison() {
        let output =
            std::env::var("ARKLINE_SERVICE_ROUTE_EXPORT").expect("explicit new output path");
        assert!(!std::path::Path::new(&output).exists());
        let d = &parallel_fixture().unwrap().definition;
        let rows: Vec<_> = (0..10u8).flat_map(|bid| [u64::from(bid), u64::from(bid)+10].map(move |cid| {
            serde_json::json!({"berth":bid,"cassette":cid,"frames":d.parallel_service_route(bid,DroneClass::Mining,cid).unwrap()})
        })).collect();
        std::fs::write(output, serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
    }
}
