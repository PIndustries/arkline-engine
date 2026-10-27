use super::*;
fn snapshot(t: u64) -> Snapshot {
    serde_json::from_value(serde_json::json!({"server_time_ms":t,"pilots":[{"name":"Pilot","credits":1000,"ore":0,"location":{"kind":"station"},"flight":{"position":[0,0,0],"velocity":[300,0,0],"acceleration":[150,0,0],"heading":[1,0,0],"mode":{"mode":"manual"},"updated_at_ms":t}}],"orders":[],"belt_ore":100000,"ledger_sequence":1,"ledger_hash":GENESIS_HASH})).unwrap()
}
fn drone(id: u64) -> DroneView {
    serde_json::from_value(serde_json::json!({"id":id,"position":[0,0,0],"velocity":[0,0,0],"heading":[1,0,0],"mode":{"kind":"hold"},"updated_at_ms":0,"hit_points":100})).unwrap()
}
fn bay_snapshot(t: u64) -> Snapshot {
    let mut s = snapshot(t);
    let p = &mut s.pilots[0];
    p.ship_class = ShipClass::Pioneer;
    let f = p.flight.as_mut().unwrap();
    f.acceleration = [0.; 3];
    f.velocity = [0.; 3];
    f.mode = NavigationMode::Stop;
    f.docking=Some(serde_json::from_value(serde_json::json!({"version":1,"host_vessel_id":"audio-vessel","berth_id":0,"operation_id":0,"phase":"free","segment":null,"clamps":0.,"updated_at_ms":t,"hold_reason":null,"departing":false})).unwrap());
    p.drone_bay = Some(DroneBayPublicState {
        batch: None,
        batch_capability: None,
        schema_version: DRONE_BAY_LEGACY_SCHEMA_VERSION,
        binding: DroneBayBinding {
            vessel_id: "audio-vessel".into(),
            geometry_revision: drone_bay_layout::DRONE_BAY_REGISTRATION_REVISION,
            source_sha256: drone_bay_layout::DRONE_BAY_SOURCE_SHA256.into(),
        },
        cradles: drone_bay_layout::home_poses()
            .into_iter()
            .enumerate()
            .map(|(i, body_pose)| DroneCradleView {
                drone_id: i as u64 + 1,
                class: DroneBayState::expected_class(i as u8).unwrap(),
                hit_points: 100,
                home_cradle: i as u8,
                presence: DroneBayPresence::Stored,
                body_pose,
            })
            .collect(),
        motions: vec![],
        machinery: DroneBayMachineryView {
            cleanup: None,
            handling_operation: None,
            door_fraction: 0.,
            door_segment: None,
            groups: drone_bay_layout::mechanism_ids()
                .into_iter()
                .zip(drone_bay_layout::reference_poses())
                .map(|(id, pose)| DroneBayMechanismView {
                    semantic_id: id.into(),
                    confirmed_pose: pose,
                    segment: None,
                })
                .collect(),
        },
    });
    s
}
pub(crate) fn cargo_mining_snapshot() -> Snapshot {
    use arkline_protocol::{drone_dropoff_geometry as receiver, *};
    let mut s = bay_snapshot(1000);
    let p = &mut s.pilots[0];
    let mut d = drone(6);
    d.class = DroneClass::Mining;
    d.signal_strength = 1.;
    d.updated_at_ms = 1000;
    d.mode = DroneMode::Mining { target_id: 1 };
    d.mining_cycle = Some(DroneMiningCycle {
        started_at_ms: 1000,
        completes_at_ms: 1100,
    });
    p.drones = vec![d];
    let bay = p.drone_bay.as_mut().unwrap();
    bay.schema_version = DRONE_BAY_SCHEMA_VERSION;
    bay.batch_capability = Some(DRONE_BAY_BATCH_VERSION);
    bay.cradles[5].presence = DroneBayPresence::Active;
    p.drone_cargo = Some(DroneCargoPublicState {
        schema_version: DRONE_CARGO_SCHEMA_VERSION,
        binding: supported_drone_cargo_registration().bind("audio-vessel".into()),
        holds: (6..11)
            .map(|drone_id| DroneOreHoldView {
                drone_id,
                ore_units: 0,
                capacity_units: 100,
                cycle_reserved_units: 0,
            })
            .collect(),
        incoming_reserved_units: 0,
        operations: vec![],
        receiver_operation: None,
        cleanup: None,
        motion: None,
        groups: receiver::mechanism_ids()
            .into_iter()
            .zip(receiver::reference_poses())
            .map(|(id, pose)| DroneBayMechanismView {
                semantic_id: id.semantic_id().into(),
                confirmed_pose: pose,
                segment: None,
            })
            .collect(),
    });
    p.drone_cargo
        .as_ref()
        .unwrap()
        .validate_projection(p)
        .unwrap();
    s.asteroids = vec![Asteroid {
        id: 1,
        system_id: p.system_id,
        name: "cargo ore".into(),
        position: [10., 0., 0.],
        radius: 2.,
        ore: 100,
    }];
    s
}
#[test]
fn drone_owned_yield_makes_spatial_hit_but_transfer_and_invalid_state_do_not() {
    for case in ["yield", "transfer", "invalid", "no-cycle", "no-depletion"] {
        let mut before = cargo_mining_snapshot();
        if case == "transfer" {
            let cargo = before.pilots[0].drone_cargo.as_mut().unwrap();
            cargo.holds[0].ore_units = 10;
            cargo.incoming_reserved_units = 10;
        }
        let mut scene = Scene::default();
        scene.update_spatial(Some(&before), "Pilot", true, 1000, Some(listener()), false);
        let mut after = before.clone();
        after.server_time_ms = 1100;
        let p = &mut after.pilots[0];
        p.drones[0].updated_at_ms = 1100;
        if case != "no-cycle" {
            p.drones[0].mining_cycle = Some(DroneMiningCycle {
                started_at_ms: 1100,
                completes_at_ms: 9100,
            });
        }
        let cargo = p.drone_cargo.as_mut().unwrap();
        if case == "transfer" {
            p.ore += 10;
            cargo.holds[0].ore_units = 0;
            cargo.incoming_reserved_units = 0;
        } else {
            cargo.holds[0].ore_units = 10;
            cargo.incoming_reserved_units = 10;
        }
        if case == "invalid" {
            cargo.binding.registration.receiver_revision = 999;
        }
        if case != "no-depletion" {
            after.asteroids[0].ore = 90;
        }
        let cues = scene
            .update_spatial(Some(&after), "Pilot", true, 1100, Some(listener()), false)
            .1;
        let hits = cues
            .iter()
            .filter(|cue| {
                matches!(
                    cue,
                    Cue::Spatial {
                        event: SpatialEvent::MiningImpact,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(hits, usize::from(case == "yield"), "{case}");
    }
}
fn listener() -> AudioListener {
    AudioListener {
        position_m: [-25., -1., -3.5],
        right: [0., 0., 1.],
    }
}
fn launch_audio_drone(s: &mut Snapshot) {
    let p = &mut s.pilots[0];
    let b = p.drone_bay.as_mut().unwrap();
    let home = b.cradles[5].body_pose;
    b.cradles[5].presence = DroneBayPresence::Active;
    b.machinery.handling_operation = Some(1);
    b.motions.push(DroneBayMotionView {
        drone_id: 6,
        operation_id: 1,
        phase: DroneBayPhase::ReleasingCradle,
        frame: DroneBayMotionFrame::Body,
        confirmed_pose: home,
        segment: None,
        updated_at_ms: s.server_time_ms,
    });
    let mut d = drone(6);
    d.class = DroneClass::Mining;
    d.updated_at_ms = s.server_time_ms;
    d.position = home.position_m;
    d.orientation_xyzw = Some(home.orientation_xyzw);
    p.drones.push(d);
}
#[test]
fn bay_spatial_audio_uses_real_segments_and_deduplicates_contacts() {
    let mut scene = Scene::default();
    let mut s = bay_snapshot(1000);
    assert!(supported_bay(&s.pilots[0]).is_some());
    scene.update_spatial(Some(&s), "Pilot", true, 1000, Some(listener()), false);
    s.server_time_ms = 1100;
    launch_audio_drone(&mut s);
    let cues = scene
        .update_spatial(Some(&s), "Pilot", true, 1100, Some(listener()), false)
        .1;
    assert_eq!(
        cues.iter()
            .filter(|c| matches!(
                c,
                Cue::Spatial {
                    event: SpatialEvent::Release,
                    ..
                }
            ))
            .count(),
        1
    );
    assert!(scene
        .update_spatial(Some(&s), "Pilot", true, 1100, Some(listener()), false)
        .1
        .is_empty());
    let group = &mut s.pilots[0].drone_bay.as_mut().unwrap().machinery.groups[5];
    let mut target = group.confirmed_pose;
    target.position_m[2] -= 1.;
    group.segment = Some(drone_bay_geometry::BayMotionSegment {
        source: group.confirmed_pose,
        target,
        started_at_ms: 1100,
        duration_ms: 1000,
    });
    s.server_time_ms = 1200;
    let mix = scene
        .update_spatial(Some(&s), "Pilot", true, 1200, Some(listener()), false)
        .0;
    assert!(mix
        .spatial
        .iter()
        .any(|e| e.sound == SpatialSound::Handling && e.gain > 0.));
    let mut endpoint_mix = Mix::default();
    spatial_loops(&s, &s.pilots[0], 2200, listener(), &mut endpoint_mix);
    assert!(
        endpoint_mix.spatial.iter().all(|e| e.gain == 0.),
        "Accepted segment endpoints cannot keep a motor running"
    );
    s.server_time_ms = 1300;
    let b = s.pilots[0].drone_bay.as_mut().unwrap();
    b.motions[0].phase = DroneBayPhase::Held;
    b.machinery.groups[5].segment = None;
    let mix = scene
        .update_spatial(Some(&s), "Pilot", true, 1300, Some(listener()), false)
        .0;
    assert!(mix.spatial.iter().all(|e| e.gain == 0.));
    s.server_time_ms = 1400;
    let p = &mut s.pilots[0];
    p.drones.clear();
    let b = p.drone_bay.as_mut().unwrap();
    b.cradles[5].presence = DroneBayPresence::Stored;
    b.motions.clear();
    b.machinery.handling_operation = None;
    let cues = scene
        .update_spatial(Some(&s), "Pilot", true, 1400, Some(listener()), false)
        .1;
    assert_eq!(
        cues.iter()
            .filter(|c| matches!(
                c,
                Cue::Spatial {
                    event: SpatialEvent::Secure,
                    ..
                }
            ))
            .count(),
        1
    );
    assert!(scene
        .update_spatial(Some(&s), "Pilot", true, 1400, Some(listener()), false)
        .1
        .is_empty());
}
#[test]
fn bay_spatial_pause_stale_reconnect_wrong_source_never_burst() {
    for reset in 0..4 {
        let mut scene = Scene::default();
        let mut s = bay_snapshot(1000);
        scene.update_spatial(Some(&s), "Pilot", true, 1000, Some(listener()), false);
        match reset {
            0 => {
                scene.update_spatial(Some(&s), "Pilot", true, 1000, Some(listener()), true);
            }
            1 => {
                scene.update_spatial(Some(&s), "Pilot", true, 3000, Some(listener()), false);
            }
            2 => {
                scene.update_spatial(None, "Pilot", false, 1100, Some(listener()), false);
            }
            _ => {
                scene.update_spatial(Some(&s), "Pilot", true, 1100, None, false);
            }
        }
        s.server_time_ms = 3100;
        launch_audio_drone(&mut s);
        let (mix, cues) =
            scene.update_spatial(Some(&s), "Pilot", true, 3100, Some(listener()), false);
        assert!(mix.spatial.iter().all(|e| e.gain == 0.));
        assert!(!cues.iter().any(|c| matches!(c, Cue::Spatial { .. })));
        s.server_time_ms = 3200;
        s.pilots[0]
            .drone_bay
            .as_mut()
            .unwrap()
            .binding
            .source_sha256 = "wrong".into();
        let (mix, cues) =
            scene.update_spatial(Some(&s), "Pilot", true, 3200, Some(listener()), false);
        assert!(mix.spatial.iter().all(|e| e.gain == 0.));
        assert!(!cues.iter().any(|c| matches!(c, Cue::Spatial { .. })));
    }
}
#[test]
fn spatial_listener_rotates_panning_and_attenuates_real_distance() {
    let left = AudioListener {
        position_m: [0.; 3],
        right: [0., 0., 1.],
    };
    let (gain, pan) = left.spatial([0., 0., -10.], 1.).unwrap();
    assert!(pan < -0.99 && gain > 0.8);
    let rotated = AudioListener {
        right: [0., 0., -1.],
        ..left
    };
    assert!(rotated.spatial([0., 0., -10.], 1.).unwrap().1 > 0.99);
    assert!(left.spatial([0., 0., 1000.], 1.).unwrap().0 < 0.001);
    assert!(AudioListener {
        right: [0.; 3],
        ..left
    }
    .spatial([0.; 3], 1.)
    .is_none());
    assert!(left.spatial([f64::NAN, 0., 0.], 1.).is_none());
}
#[test]
fn spatial_mining_needs_confirmed_yield_depletion_and_free_same_id() {
    let mut scene = Scene::default();
    let mut s = snapshot(1000);
    let mut d = drone(6);
    d.class = DroneClass::Mining;
    d.signal_strength = 1.;
    d.mode = DroneMode::Mining { target_id: 1 };
    d.updated_at_ms = 1000;
    d.mining_cycle = Some(DroneMiningCycle {
        started_at_ms: 900,
        completes_at_ms: 1200,
    });
    s.pilots[0].drones.push(d);
    s.asteroids.push(Asteroid {
        id: 1,
        system_id: HOME_SYSTEM_ID,
        name: "ore".into(),
        position: [10., 0., 0.],
        radius: 2.,
        ore: 100,
    });
    scene.update_spatial(Some(&s), "Pilot", true, 1000, Some(listener()), false);
    s.server_time_ms = 1100;
    assert!(!scene
        .update_spatial(Some(&s), "Pilot", true, 1100, Some(listener()), false)
        .1
        .iter()
        .any(|c| matches!(
            c,
            Cue::Spatial {
                event: SpatialEvent::MiningImpact,
                ..
            }
        )));
    s.server_time_ms = 1200;
    s.pilots[0].ore = 5;
    s.asteroids[0].ore = 95;
    s.pilots[0].drones[0].mining_cycle = Some(DroneMiningCycle {
        started_at_ms: 1200,
        completes_at_ms: 2200,
    });
    let cues = scene
        .update_spatial(Some(&s), "Pilot", true, 1200, Some(listener()), false)
        .1;
    assert_eq!(
        cues.iter()
            .filter(|c| matches!(
                c,
                Cue::Spatial {
                    event: SpatialEvent::MiningImpact,
                    ..
                }
            ))
            .count(),
        1
    );
    assert!(scene
        .update_spatial(Some(&s), "Pilot", true, 1200, Some(listener()), false)
        .1
        .is_empty());
    s.server_time_ms = 1300;
    s.pilots[0].ore = 10; // purchase/other work has no new due cycle
    assert!(!scene
        .update_spatial(Some(&s), "Pilot", true, 1300, Some(listener()), false)
        .1
        .iter()
        .any(|c| matches!(
            c,
            Cue::Spatial {
                event: SpatialEvent::MiningImpact,
                ..
            }
        )));
}
#[test]
fn spatial_two_miners_shared_target_never_attributes_unchanged_cycle() {
    let mut s = snapshot(1000);
    for id in [6, 7] {
        let mut d = drone(id);
        d.class = DroneClass::Mining;
        d.signal_strength = 1.;
        d.mode = DroneMode::Mining { target_id: 1 };
        d.updated_at_ms = 1000;
        d.position = [0., 0., if id == 6 { -10. } else { 10. }];
        d.mining_cycle = Some(DroneMiningCycle {
            started_at_ms: 900,
            completes_at_ms: 1200,
        });
        s.pilots[0].drones.push(d);
    }
    s.asteroids.push(Asteroid {
        id: 1,
        system_id: HOME_SYSTEM_ID,
        name: "shared ore".into(),
        position: [10., 0., 0.],
        radius: 2.,
        ore: 100,
    });
    let mut scene = Scene::default();
    let ear = AudioListener {
        position_m: [0.; 3],
        right: [0., 0., 1.],
    };
    scene.update_spatial(Some(&s), "Pilot", true, 1000, Some(ear), false);
    s.server_time_ms = 1200;
    s.pilots[0].ore = 5;
    s.asteroids[0].ore = 95;
    s.pilots[0].drones[1].mining_cycle = Some(DroneMiningCycle {
        started_at_ms: 1200,
        completes_at_ms: 2200,
    });
    let cues = scene
        .update_spatial(Some(&s), "Pilot", true, 1200, Some(ear), false)
        .1;
    let impacts: Vec<_> = cues
        .iter()
        .filter_map(|c| match c {
            Cue::Spatial {
                event: SpatialEvent::MiningImpact,
                pan,
                ..
            } => Some(*pan),
            _ => None,
        })
        .collect();
    assert_eq!(
        impacts.len(),
        1,
        "Only ID7 advanced; ID6's shared ore deltas are not its work receipt"
    );
    assert!(impacts[0] > 0.);
    s.server_time_ms = 1300;
    s.pilots[0].ore = 10;
    s.asteroids[0].ore = 90;
    assert!(!scene
        .update_spatial(Some(&s), "Pilot", true, 1300, Some(ear), false)
        .1
        .iter()
        .any(|c| matches!(
            c,
            Cue::Spatial {
                event: SpatialEvent::MiningImpact,
                ..
            }
        )));
}
#[test]
fn existing_drone_cycle_chime_requires_that_drone_to_advance() {
    let mut s = snapshot(1000);
    let mut d = drone(6);
    d.class = DroneClass::Mining;
    d.signal_strength = 1.;
    d.mode = DroneMode::Mining { target_id: 1 };
    d.mining_cycle = Some(DroneMiningCycle {
        started_at_ms: 900,
        completes_at_ms: 1200,
    });
    s.pilots[0].drones.push(d);
    let mut scene = Scene::default();
    scene.update(Some(&s), "Pilot", true, 1000);
    s.server_time_ms = 1200;
    s.pilots[0].ore = 5;
    assert!(
        !scene
            .update(Some(&s), "Pilot", true, 1200)
            .1
            .iter()
            .any(|c| matches!(c, Cue::MiningCycle)),
        "Shared cargo growth cannot make an unchanged drone timer a confirmed cycle"
    );
}
#[test]
fn same_time_accepted_bay_release_is_heard_once_not_lost_or_replayed() {
    let mut s = bay_snapshot(1000);
    let mut scene = Scene::default();
    scene.update_spatial(Some(&s), "Pilot", true, 1000, Some(listener()), false);
    launch_audio_drone(&mut s); // real request can retain the current simulation time
    let cues = scene
        .update_spatial(Some(&s), "Pilot", true, 1000, Some(listener()), false)
        .1;
    assert_eq!(
        cues.iter()
            .filter(|c| matches!(
                c,
                Cue::Spatial {
                    event: SpatialEvent::Release,
                    ..
                }
            ))
            .count(),
        1
    );
    for _ in 0..3 {
        assert!(!scene
            .update_spatial(Some(&s), "Pilot", true, 1000, Some(listener()), false)
            .1
            .iter()
            .any(|c| matches!(c, Cue::Spatial { .. })));
    }
}
#[test]
fn zero_signal_miner_has_no_work_loop_or_confirmed_impact() {
    let mut s = snapshot(1000);
    let mut d = drone(6);
    d.class = DroneClass::Mining;
    d.signal_strength = 0.;
    d.mode = DroneMode::Mining { target_id: 1 };
    d.updated_at_ms = 1000;
    d.mining_cycle = Some(DroneMiningCycle {
        started_at_ms: 900,
        completes_at_ms: 1200,
    });
    s.pilots[0].drones.push(d);
    s.asteroids.push(Asteroid {
        id: 1,
        system_id: HOME_SYSTEM_ID,
        name: "ore".into(),
        position: [10., 0., 0.],
        radius: 2.,
        ore: 100,
    });
    let mut scene = Scene::default();
    let mix = scene
        .update_spatial(Some(&s), "Pilot", true, 1000, Some(listener()), false)
        .0;
    assert!(!mix
        .spatial
        .iter()
        .any(|e| e.sound == SpatialSound::Mining && e.gain > 0.));
    s.server_time_ms = 1200;
    s.pilots[0].ore = 5;
    s.asteroids[0].ore = 95;
    s.pilots[0].drones[0].mining_cycle = Some(DroneMiningCycle {
        started_at_ms: 1200,
        completes_at_ms: 2200,
    });
    assert!(!scene
        .update_spatial(Some(&s), "Pilot", true, 1200, Some(listener()), false)
        .1
        .iter()
        .any(|c| matches!(
            c,
            Cue::Spatial {
                event: SpatialEvent::MiningImpact,
                ..
            }
        )));
}
fn mechanical_snapshot(t: u64) -> Snapshot {
    let mut s = snapshot(t);
    let binding = DockedTraversalBinding {
        host_vessel_id: "owned-pioneer".into(),
        registration_revision: PIONEER_DOCKED_REGISTRATION_REVISION,
        layout_sha256: "fixture".into(),
        fleet_layout: FleetLayoutRevision::LinearV1,
        fleet_hangar: false,
    };
    let hatch = DockedHatchState {
        fraction: 0.0,
        target_open: false,
        moving: false,
        blocked: false,
        updated_at_ms: t,
    };
    let traversal = DockedTraversalState {
        version: DOCKED_TRAVERSAL_VERSION,
        binding: binding.clone(),
        feet_m: [0.0; 3],
        heading: 0.0,
        support: DockedSupport::Deck1,
        surface_id: "deck1/floor/0".into(),
        moving: false,
        sprinting: false,
        grounded: true,
        vertical_velocity_mps: 0.0,
        updated_at_ms: t,
        seat_transition: None,
        actuators: DockedVesselActuators {
            binding,
            hatches: Some([hatch.clone(), hatch]),
            inner_hatch_open: false,
            outer_hatch_open: false,
            lift: DockedLiftState {
                floor_station_m: 12.68,
                target: DockedLiftStop::Ship,
                moving: false,
                updated_at_ms: t,
            },
            service_power_available: true,
        },
    };
    s.pilots[0].station_avatar = Some(StationAvatar {
        docked_traversal: Some(traversal),
        onboard_interior: true,
        interior_deck: 1,
        jump_started_ms: None,
        seated: false,
        position: [0.0; 3],
        heading: 0.0,
        moving: false,
        sprinting: false,
        updated_at_ms: t,
    });
    s
}
fn mechanism(s: &mut Snapshot) -> &mut DockedTraversalState {
    s.pilots[0]
        .station_avatar
        .as_mut()
        .unwrap()
        .docked_traversal
        .as_mut()
        .unwrap()
}
#[test]
fn mechanical_audio_edges_rejections_repeats_and_blocked_stop() {
    let mut scene = Scene::default();
    let mut s = mechanical_snapshot(1000);
    scene.update(Some(&s), "Pilot", true, 1000);
    s.server_time_ms = 1100;
    let a = &mut mechanism(&mut s).actuators;
    a.hatches.as_mut().unwrap()[0].moving = true;
    a.hatches.as_mut().unwrap()[0].target_open = true;
    a.lift.moving = true;
    assert!(matches!(
        scene.update(Some(&s), "Pilot", true, 1100).1.as_slice(),
        [Cue::HatchMove, Cue::LiftMove]
    ));
    assert!(scene.update(Some(&s), "Pilot", true, 1100).1.is_empty());
    s.server_time_ms = 1200; // repeated motion/rejected action causes no sound
    assert!(scene.update(Some(&s), "Pilot", true, 1200).1.is_empty());
    s.server_time_ms = 1300;
    let a = &mut mechanism(&mut s).actuators;
    a.hatches.as_mut().unwrap()[0].moving = false;
    a.hatches.as_mut().unwrap()[0].blocked = true;
    a.lift.moving = false;
    assert!(matches!(
        scene.update(Some(&s), "Pilot", true, 1300).1.as_slice(),
        [Cue::HatchBlocked, Cue::LiftStop]
    ));
    s.server_time_ms = 1400;
    let h = &mut mechanism(&mut s).actuators.hatches.as_mut().unwrap()[0];
    h.blocked = false;
    h.target_open = false;
    h.moving = true;
    h.fraction = 0.4;
    assert!(matches!(
        scene.update(Some(&s), "Pilot", true, 1400).1.as_slice(),
        [Cue::HatchMove]
    ));
    s.server_time_ms = 1500;
    let h = &mut mechanism(&mut s).actuators.hatches.as_mut().unwrap()[0];
    h.moving = false;
    h.fraction = 0.0;
    assert!(matches!(
        scene.update(Some(&s), "Pilot", true, 1500).1.as_slice(),
        [Cue::HatchLatch]
    ));
}
#[test]
fn mechanical_audio_rebind_stale_reconnect_and_initial_motion_are_silent() {
    let mut scene = Scene::default();
    let mut s = mechanical_snapshot(1000);
    mechanism(&mut s).actuators.lift.moving = true;
    assert!(matches!(
        scene.update(Some(&s), "Pilot", true, 1000).1.as_slice(),
        [Cue::Connect]
    ));
    s.server_time_ms = 1100;
    let t = mechanism(&mut s);
    t.binding.layout_sha256 = "other".into();
    t.actuators.binding = t.binding.clone();
    t.actuators.lift.moving = false;
    assert!(scene.update(Some(&s), "Pilot", true, 1100).1.is_empty());
    s.server_time_ms = 3000;
    mechanism(&mut s).actuators.lift.moving = true;
    assert!(scene.update(Some(&s), "Pilot", true, 3000).1.is_empty());
    scene.update(None, "Pilot", false, 3100);
    s.server_time_ms = 3200;
    assert!(matches!(
        scene.update(Some(&s), "Pilot", true, 3200).1.as_slice(),
        [Cue::Connect]
    ));
}
#[test]
fn mechanical_audio_explicit_seat_sequence_only_once_without_completion_guess() {
    let mut scene = Scene::default();
    let mut s = mechanical_snapshot(1000);
    s.pilots[0].station_avatar = None;
    scene.update(Some(&s), "Pilot", true, 1000);
    s = mechanical_snapshot(1100);
    mechanism(&mut s).seat_transition = Some(DockedSeatTransition {
        phase: DockedSeatPhase::Leaving,
        started_at_ms: 1050,
        ends_at_ms: 1950,
    });
    assert!(matches!(
        scene.update(Some(&s), "Pilot", true, 1100).1.as_slice(),
        [Cue::SeatMove]
    ));
    s.server_time_ms = 1200;
    assert!(scene.update(Some(&s), "Pilot", true, 1200).1.is_empty());
    s.server_time_ms = 2000;
    mechanism(&mut s).seat_transition = None;
    assert!(scene.update(Some(&s), "Pilot", true, 2000).1.is_empty());
    s.server_time_ms = 2100;
    mechanism(&mut s).seat_transition = Some(DockedSeatTransition {
        phase: DockedSeatPhase::Entering,
        started_at_ms: 2050,
        ends_at_ms: 2950,
    });
    assert!(matches!(
        scene.update(Some(&s), "Pilot", true, 2100).1.as_slice(),
        [Cue::SeatMove]
    ));
    s.server_time_ms = 2200;
    s.pilots[0].station_avatar = None; // cancelled/removed is not a successful latch
    assert!(scene.update(Some(&s), "Pilot", true, 2200).1.is_empty());
}
#[test]
fn connection_source_stale_and_reordered_snapshots_never_replay_history() {
    let mut scene = Scene::default();
    let mut s = snapshot(1000);
    let (mix, cues) = scene.update(Some(&s), "Pilot", true, 1000);
    assert!(mix.active && mix.engine > 0.0);
    assert!(matches!(cues.as_slice(), [Cue::Connect]));
    assert!(scene.update(Some(&s), "Pilot", true, 1100).1.is_empty());
    assert!(!scene.update(Some(&s), "Pilot", true, 2501).0.active);
    s.server_time_ms = 2600;
    s.pilots[0].location = Location::Docked;
    s.pilots[0].ore = 1000;
    assert!(scene.update(Some(&s), "Pilot", true, 2600).1.is_empty());
    s.server_time_ms = 2500;
    assert!(!scene.update(Some(&s), "Pilot", true, 2600).0.active);
    assert!(matches!(
        scene.update(None, "Pilot", false, 2700).1.as_slice(),
        [Cue::Disconnect]
    ));
    assert!(scene.update(None, "Pilot", false, 2701).1.is_empty());
    s.server_time_ms = 2800;
    assert!(matches!(
        scene.update(Some(&s), "Pilot", true, 2800).1.as_slice(),
        [Cue::Connect]
    ));
    s.pilots[0].name = "Worker".into();
    s.server_time_ms = 2900;
    s.pilots[0].location = Location::Station;
    assert!(scene.update(Some(&s), "Worker", true, 2900).1.is_empty());
    scene.reset();
    assert!(!scene.update(None, "Worker", false, 3000).0.active);
}
#[test]
fn coasting_is_quiet_but_real_braking_and_rotation_are_audible() {
    let mut scene = Scene::default();
    let mut s = snapshot(1000);
    let f = s.pilots[0].flight.as_mut().unwrap();
    f.acceleration = [0.0; 3];
    assert_eq!(scene.update(Some(&s), "Pilot", true, 1000).0.engine, 0.0);
    s.server_time_ms = 1100;
    s.pilots[0].flight.as_mut().unwrap().acceleration = [-60.0, 0.0, 0.0];
    assert!(scene.update(Some(&s), "Pilot", true, 1100).0.engine > 0.0);
    s.server_time_ms = 1200;
    let f = s.pilots[0].flight.as_mut().unwrap();
    f.acceleration = [0.0; 3];
    f.angular_velocity = [0.0, 0.0, MAX_ANGULAR_SPEED];
    assert!(scene.update(Some(&s), "Pilot", true, 1200).0.engine > 0.0);
    scene.reset();
    assert!(scene.update(Some(&s), "Pilot", true, 1200).1.is_empty());
}
#[test]
fn mining_needs_confirmed_deadline_and_gain_not_purchase_or_every_frame() {
    let mut scene = Scene::default();
    let mut s = snapshot(1000);
    let p = &mut s.pilots[0];
    p.ore = 975;
    p.locked_asteroids = vec![1];
    p.mining_modules = vec![MiningModule {
        module_id: 0,
        target_id: Some(1),
        cycle_started_at_ms: Some(0),
        cycle_completes_at_ms: Some(1200),
        stop_reason: None,
    }];
    p.power = Some(PowerState {
        charge_j: 30,
        capacity_j: 100,
        regen_w: 1,
        updated_at_ms: 1000,
        regen_remainder_mj: 0,
    });
    s.asteroids.push(Asteroid {
        id: 1,
        system_id: HOME_SYSTEM_ID,
        name: "Rock".into(),
        position: [0.0; 3],
        radius: 100.0,
        ore: 100,
    });
    assert!(scene.update(Some(&s), "Pilot", true, 1000).0.mining > 0.0);
    s.server_time_ms = 1200;
    s.pilots[0].ore = 1000;
    s.pilots[0].mining_modules.clear();
    s.pilots[0].power.as_mut().unwrap().charge_j = 20;
    let (mix, cues) = scene.update(Some(&s), "Pilot", true, 1200);
    assert_eq!(mix.mining, 0.0);
    assert_eq!(
        cues.iter()
            .filter(|c| matches!(c, Cue::MiningCycle))
            .count(),
        1
    );
    assert!(cues.iter().any(|c| matches!(c, Cue::CargoFull)));
    assert!(cues.iter().any(|c| matches!(c, Cue::LowPower)));
    assert!(scene.update(Some(&s), "Pilot", true, 1210).1.is_empty());
    s.server_time_ms = 1300;
    s.pilots[0].location = Location::Docked;
    s.pilots[0].ore = 0;
    scene.update(Some(&s), "Pilot", true, 1300);
    s.server_time_ms = 1400;
    s.pilots[0].ore = 20;
    assert!(!scene
        .update(Some(&s), "Pilot", true, 1400)
        .1
        .iter()
        .any(|c| matches!(c, Cue::MiningCycle)));
}
#[test]
fn warp_and_lock_follow_authoritative_transitions_not_predicted_arrival() {
    let mut scene = Scene::default();
    let mut s = snapshot(1000);
    s.pilots[0].location = Location::Docked;
    scene.update(Some(&s), "Pilot", true, 1000);
    s.server_time_ms = 1100;
    s.pilots[0].location = Location::Station;
    assert!(scene
        .update(Some(&s), "Pilot", true, 1100)
        .1
        .iter()
        .any(|c| matches!(c, Cue::Undock)));
    let path = plan_warp([0.0; 3], [2000.0, 0.0, 0.0], 1200);
    let arrival = path.arrives_at_ms;
    s.server_time_ms = 1200;
    s.pilots[0].location = Location::InTransit {
        destination: Destination::Belt,
        arrives_at_ms: arrival,
    };
    s.pilots[0].flight.as_mut().unwrap().warp = Some(path);
    let (mix, cues) = scene.update(Some(&s), "Pilot", true, 1200);
    assert!(mix.warp > 0.0);
    assert!(cues.iter().any(|c| matches!(c, Cue::WarpCharge)));
    for t in (1300..arrival).step_by(100) {
        s.server_time_ms = t;
        scene.update(Some(&s), "Pilot", true, t);
    }
    s.server_time_ms = arrival;
    let (mix, cues) = scene.update(Some(&s), "Pilot", true, arrival);
    assert_eq!(mix.warp, 0.0);
    assert!(!cues.iter().any(|c| matches!(c, Cue::WarpArrival)));
    s.server_time_ms += 1;
    s.pilots[0].location = Location::Belt;
    s.pilots[0].locked_asteroids = vec![1];
    let (_, cues) = scene.update(Some(&s), "Pilot", true, s.server_time_ms);
    assert!(cues.iter().any(|c| matches!(c, Cue::WarpArrival)));
    assert!(cues.iter().any(|c| matches!(c, Cue::Lock)));
}
#[test]
fn nearby_arrivals_are_local_confirmed_and_never_replayed() {
    for (distance, system, audible) in [
        (199_999.0, HOME_SYSTEM_ID, true),
        (200_001.0, HOME_SYSTEM_ID, false),
        (100.0, HOME_SYSTEM_ID + 1, false),
    ] {
        let mut scene = Scene::default();
        let mut s = snapshot(1000);
        let mut other = s.pilots[0].clone();
        other.name = "Visitor".into();
        other.system_id = system;
        other.flight.as_mut().unwrap().position = [distance, 0.0, 0.0];
        other.location = Location::InTransit {
            destination: Destination::Belt,
            arrives_at_ms: 1100,
        };
        s.pilots.push(other);
        scene.update(Some(&s), "Pilot", true, 1000);
        s.server_time_ms = 1100;
        s.pilots[1].location = Location::Belt;
        let cues = scene.update(Some(&s), "Pilot", true, 1100).1;
        assert_eq!(
            cues.iter()
                .filter(|c| matches!(c, Cue::WarpArrival))
                .count(),
            usize::from(audible)
        );
        assert!(scene.update(Some(&s), "Pilot", true, 1101).1.is_empty());
        scene.reset();
        assert!(scene.update(Some(&s), "Pilot", true, 1101).1.is_empty());
    }
    for reset in 0..3 {
        let mut scene = Scene::default();
        let mut s = snapshot(1000);
        let mut other = s.pilots[0].clone();
        other.name = "Visitor".into();
        other.location = Location::Jumping {
            gate_id: 1,
            arrives_at_ms: 1100,
        };
        s.pilots.push(other);
        scene.update(Some(&s), "Pilot", true, 1000);
        match reset {
            0 => {
                scene.update(Some(&s), "Pilot", true, 3000);
            }
            1 => {
                scene.update(None, "Pilot", false, 1050);
            }
            _ => {
                scene.update(Some(&s), "Visitor", true, 1050);
            }
        }
        s.server_time_ms = 1100;
        s.pilots[1].location = Location::Station;
        assert!(!scene
            .update(Some(&s), "Pilot", true, 1100)
            .1
            .iter()
            .any(|c| matches!(c, Cue::WarpArrival)));
    }
}
#[test]
fn warp_entry_live_clock_crossing_is_latched_without_initial_replay() {
    let mut s = snapshot(2500);
    let path = plan_warp([0.0; 3], [1_000_000.0, 0.0, 0.0], 1000);
    let launch = path.started_at_ms + path.warmup_ms;
    s.pilots[0].location = Location::InTransit {
        destination: Destination::Belt,
        arrives_at_ms: path.arrives_at_ms,
    };
    s.pilots[0].flight.as_mut().unwrap().warp = Some(path);
    let mut scene = Scene::default();
    assert!(!scene
        .update(Some(&s), "Pilot", true, launch - 1)
        .1
        .iter()
        .any(|c| matches!(c, Cue::WarpEnter)));
    assert_eq!(
        scene
            .update(Some(&s), "Pilot", true, launch)
            .1
            .iter()
            .filter(|c| matches!(c, Cue::WarpEnter))
            .count(),
        1
    );
    assert!(scene
        .update(Some(&s), "Pilot", true, launch + 1)
        .1
        .is_empty());
    // A clock correction cannot re-arm the same boundary.
    scene.update(Some(&s), "Pilot", true, launch - 1);
    assert!(scene
        .update(Some(&s), "Pilot", true, launch + 2)
        .1
        .is_empty());
    scene.reset();
    assert!(scene
        .update(Some(&s), "Pilot", true, launch + 3)
        .1
        .is_empty());
}
#[test]
fn mining_lifecycle_coalesces_modules_and_ignores_cycle_boundaries() {
    let mut s = snapshot(1000);
    s.pilots[0].locked_asteroids = vec![1];
    s.asteroids.push(Asteroid {
        id: 1,
        system_id: HOME_SYSTEM_ID,
        name: "Rock".into(),
        position: [0.0; 3],
        radius: 100.0,
        ore: 1000,
    });
    let mut scene = Scene::default();
    scene.update(Some(&s), "Pilot", true, 1000);
    s.server_time_ms = 1100;
    s.pilots[0].mining_modules = (0..2)
        .map(|module_id| MiningModule {
            module_id,
            target_id: Some(1),
            cycle_started_at_ms: Some(1100),
            cycle_completes_at_ms: Some(1200),
            stop_reason: None,
        })
        .collect();
    let cues = scene.update(Some(&s), "Pilot", true, 1100).1;
    assert_eq!(
        cues.iter()
            .filter(|c| matches!(c, Cue::MiningStart))
            .count(),
        1
    );
    s.server_time_ms = 1200;
    assert!(scene.update(Some(&s), "Pilot", true, 1200).1.is_empty());
    s.server_time_ms = 1250;
    for m in &mut s.pilots[0].mining_modules {
        m.cycle_started_at_ms = Some(1200);
        m.cycle_completes_at_ms = Some(1300);
    }
    assert!(scene.update(Some(&s), "Pilot", true, 1250).1.is_empty());
    s.server_time_ms = 1260;
    s.pilots[0].mining_modules.pop();
    assert!(scene.update(Some(&s), "Pilot", true, 1260).1.is_empty());
    s.server_time_ms = 1270;
    s.pilots[0].mining_modules.clear();
    assert_eq!(
        scene
            .update(Some(&s), "Pilot", true, 1270)
            .1
            .iter()
            .filter(|c| matches!(c, Cue::MiningStop))
            .count(),
        1
    );
    assert!(scene.update(Some(&s), "Pilot", true, 1271).1.is_empty());
}
#[test]
fn drone_launch_recall_and_shot_need_real_state_evidence() {
    let mut scene = Scene::default();
    let mut s = snapshot(1000);
    let mut victim = s.pilots[0].clone();
    victim.name = "Target".into();
    victim.drones = vec![drone(99)];
    s.pilots.push(victim);
    scene.update(Some(&s), "Pilot", true, 1000);
    let mut attacker = drone(1);
    attacker.class = DroneClass::Attack;
    attacker.mode = DroneMode::Attacking {
        target_pilot: "Target".into(),
        target_drone_id: 99,
    };
    attacker.attack_ready_at_ms = Some(1200);
    s.pilots[0].drones.push(attacker);
    s.server_time_ms = 1100;
    let cues = scene.update(Some(&s), "Pilot", true, 1100).1;
    assert!(cues.iter().any(|c| matches!(c, Cue::DroneLaunch)));
    assert!(!cues.iter().any(|c| matches!(c, Cue::DroneShot)));
    s.server_time_ms = 1200;
    s.pilots[0].drones[0].attack_ready_at_ms = Some(1300);
    assert!(!scene
        .update(Some(&s), "Pilot", true, 1200)
        .1
        .iter()
        .any(|c| matches!(c, Cue::DroneShot)));
    s.server_time_ms = 1300;
    s.pilots[0].drones[0].attack_ready_at_ms = Some(1400);
    s.pilots[1].drones[0].hit_points = 90;
    assert!(scene
        .update(Some(&s), "Pilot", true, 1300)
        .1
        .iter()
        .any(|c| matches!(c, Cue::DroneShot)));
    s.server_time_ms = 1400;
    s.pilots[0].drones[0].mode = DroneMode::Returning;
    assert!(scene
        .update(Some(&s), "Pilot", true, 1400)
        .1
        .iter()
        .any(|c| matches!(c, Cue::DroneRecall)));
    s.server_time_ms = 1500;
    s.pilots[0].drones.clear();
    assert!(!scene
        .update(Some(&s), "Pilot", true, 1500)
        .1
        .iter()
        .any(|c| matches!(c, Cue::DroneShot)));
}
