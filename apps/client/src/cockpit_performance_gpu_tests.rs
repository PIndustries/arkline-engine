//! Explicit matched-source profiling. This measures native engine work, not OS frame pacing.
//! Kept under the existing route harness so .17 and .20 run the identical fixture.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

const PROFILE_PILOT: &str = "NativePioneerProfile";
const WARMUP_FRAMES: usize = 16;
const SAMPLE_FRAMES: usize = 48;
const DRAIN_FRAMES: usize = 8;

#[derive(Default)]
struct PassSamples {
    cpu_prepare_ms: Vec<f64>,
    cpu_submit_ms: Vec<f64>,
    gpu_ms: BTreeMap<u64, f64>,
    submissions: BTreeSet<u64>,
    candidate_draws: Vec<f64>,
    submitted_draws: Vec<f64>,
    candidate_triangles: Vec<f64>,
    submitted_triangles: Vec<f64>,
}

fn distribution(values: &[f64]) -> serde_json::Value {
    if values.is_empty() {
        return serde_json::json!({"available":false,"samples":0});
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let percentile = |fraction: f64| {
        sorted[((sorted.len() as f64 * fraction).ceil() as usize).saturating_sub(1)]
    };
    serde_json::json!({
        "available":true,"samples":sorted.len(),
        "mean":sorted.iter().sum::<f64>() / sorted.len() as f64,
        "median":percentile(0.5),"p95":percentile(0.95),"max":sorted[sorted.len()-1]
    })
}

fn view_metrics(h: &Harness, docked: bool) -> Vec<crate::ViewRenderMetrics> {
    if docked {
        h.cockpit.hangar_view.debug_view_render_metrics()
    } else {
        h.cockpit.flight_view.debug_view_render_metrics()
    }
}

fn record_gpu(
    passes: &mut BTreeMap<String, PassSamples>,
    samples: Vec<crate::ViewRenderMetrics>,
    first: u64,
    last: u64,
) {
    for sample in samples {
        if let (Some(frame), Some(ns)) = (sample.gpu_frame, sample.gpu_elapsed_ns) {
            if (first..=last).contains(&frame) {
                passes
                    .entry(sample.key)
                    .or_default()
                    .gpu_ms
                    .insert(frame, ns as f64 / 1e6);
            }
        }
    }
}

// Every replacement is confined to the freshly generated retained harness directory.
// This admits no source backup by opening it, and performs no command on a user world.
fn install_seed(h: &mut Harness, seed: &Path) {
    assert_eq!(
        seed.file_name().and_then(|s| s.to_str()),
        Some("authority-seed.sqlite")
    );
    assert!(
        seed.is_file()
            && !std::fs::symlink_metadata(seed)
                .unwrap()
                .file_type()
                .is_symlink()
    );
    drop(h.world.take());
    let target = h.directory.join("world.sqlite");
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", target.display()));
        assert!(
            !sidecar.exists() || sidecar.metadata().unwrap().len() == 0,
            "Closed fixture retained a nonempty SQLite sidecar: {}",
            sidecar.display()
        );
    }
    std::fs::copy(seed, &target).unwrap();
    let mut world = EditorWorld::open(&h.directory).unwrap();
    // Reopening restores no transport presence. Admit a new deterministic
    // profile pilot through the real capability/identity boundary instead of
    // pretending the seed's saved session is online. The seed pilot is retained.
    let mut session = None;
    let response = world.handle(
        &mut session,
        Request::Connect {
            supports_pioneer_interior: true,
            supports_docked_traversal_v2: true,
            world_layout_version: 1,
            supports_fleet_hangar: true,
            supports_room_controls: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: PROFILE_PILOT.into(),
            resume_token: None,
        },
    );
    assert!(
        response.ok,
        "Normal profiling admission failed: {}",
        response.message
    );
    assert_eq!(session.as_deref(), Some(PROFILE_PILOT));
    world.play();
    h.world = Some(world);
    h.session = session;
    h.cockpit.pilot = PROFILE_PILOT.into();
    h.accept(response, true);
    h.refresh();
    assert_eq!(h.cockpit.own().unwrap().name, PROFILE_PILOT);
    assert_eq!(h.cockpit.own().unwrap().ship_class, ShipClass::Pioneer);
    if let Some(evidence) = &h.evidence_directory {
        let mut fixture = authority_fixture(&h.directory);
        fixture["pilot"] = PROFILE_PILOT.into();
        std::fs::write(
            evidence.join("fixture.json"),
            serde_json::to_vec_pretty(&fixture).unwrap(),
        )
        .unwrap();
    }
    assert_eq!(h.cockpit.own().unwrap().location, Location::Docked);
    assert!(h.cockpit.own().unwrap().station_avatar.is_none());
}

unsafe fn profile_case(
    gl: Arc<glow::Context>,
    directory: &Path,
    seed: &Path,
    name: &str,
    free_flight: bool,
    feeds: bool,
) -> serde_json::Value {
    let case_dir = directory.join(name);
    std::fs::create_dir(&case_dir).unwrap();
    let mut h = Harness::new(gl, Some(case_dir.clone()));
    install_seed(&mut h, seed);
    h.input_height = 772.0;
    h.cockpit.services_open = false;
    h.cockpit.overview_open = false;
    h.cockpit.view_mode = crate::ViewMode::Cockpit;
    if free_flight {
        h.command(Request::Undock);
        h.command(Request::StartResourceJob {
            target_units: 5000,
            pilot: None,
        });
        for _ in 0..600 {
            h.advance_time(1000);
            h.refresh();
            let own = h.cockpit.own().unwrap();
            if own.drones.len() == 5 && own.ore > 0 {
                break;
            }
        }
        let own = h.cockpit.own().unwrap();
        assert_eq!(own.drones.len(), 5, "Five real deployed drones required");
        assert!(
            own.ore > 0,
            "Ordinary authority mining must have delivered real ore to the ship"
        );
        assert_ne!(own.location, Location::Docked);
    }
    h.cockpit.drone_camera = feeds;
    h.cockpit.drone_split = feeds;
    h.cockpit.service_tab = crate::ServiceTab::Drones;
    for _ in 0..WARMUP_FRAMES {
        h.render_frame(Vec::new(), false);
    }
    let before = serde_json::to_value(h.cockpit.own().unwrap()).unwrap();
    let before_job =
        serde_json::to_value(&h.cockpit.snapshot.as_ref().unwrap().resource_jobs).unwrap();
    let mut whole_calls_ms = Vec::new();
    let mut render_frame_ms = Vec::new();
    let mut draw_calls = Vec::new();
    let mut mesh_triangles = Vec::new();
    let mut passes = BTreeMap::<String, PassSamples>::new();
    let mut first = None;
    let mut last = 0;
    for _ in 0..SAMPLE_FRAMES {
        let started = Instant::now();
        let shot = h.render_frame(Vec::new(), false);
        whole_calls_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        render_frame_ms.push(shot.frame_wall_ms);
        assert!(
            shot.rgba.is_empty() && shot.depth.is_empty(),
            "No measured screenshot readback"
        );
        assert_eq!(shot.registered_route, !free_flight);
        let viewport = shot.viewport.expect("Actual client scene aperture");
        assert!(
            (viewport.width() - 1500.0).abs() < 0.1 && (viewport.height() - 638.0).abs() < 0.1,
            "Matched 1500x638 aperture required: {viewport:?}"
        );
        let own = h.cockpit.own().unwrap();
        if free_flight {
            assert_eq!(
                own.drones.len(),
                5,
                "A returned drone invalidates the five-feed comparison"
            );
        }
        let first = *first.get_or_insert(shot.frame);
        last = shot.frame;
        let metrics = view_metrics(&h, !free_flight);
        for metric in &metrics {
            if metric.frame == shot.frame {
                let pass = passes.entry(metric.key.clone()).or_default();
                if pass.submissions.insert(metric.frame) {
                    pass.cpu_prepare_ms
                        .push(metric.visibility.cpu_prepare_ns as f64 / 1e6);
                    pass.cpu_submit_ms.push(metric.cpu_submit_ns as f64 / 1e6);
                    pass.candidate_draws
                        .push(metric.visibility.candidate_draws as f64);
                    pass.submitted_draws
                        .push(metric.visibility.submitted_draws as f64);
                    pass.candidate_triangles
                        .push(metric.visibility.candidate_triangles as f64);
                    pass.submitted_triangles
                        .push(metric.visibility.submitted_triangles as f64);
                }
            }
        }
        record_gpu(&mut passes, metrics, first, last);
        let last_mesh_pass = if free_flight {
            h.cockpit.flight_view.debug_render_metrics()
        } else {
            h.cockpit.hangar_view.debug_render_metrics()
        };
        draw_calls.push(last_mesh_pass.draw_calls as f64);
        mesh_triangles.push(last_mesh_pass.triangles as f64);
    }
    let after = serde_json::to_value(h.cockpit.own().unwrap()).unwrap();
    // Give asynchronous queries additional ordinary frames to finish. Never wait
    // for query results and never count these drain frames as measured CPU work.
    for _ in 0..DRAIN_FRAMES {
        h.render_frame(Vec::new(), false);
        record_gpu(
            &mut passes,
            view_metrics(&h, !free_flight),
            first.unwrap(),
            last,
        );
    }
    let passes: serde_json::Map<String, serde_json::Value> = passes.into_iter().map(|(key,p)| {
        let gpu: Vec<_> = p.gpu_ms.values().copied().collect();
        (key, serde_json::json!({
            "fresh_submitted_frames":p.submissions.len(),"gpu_unique_completed_frames":p.gpu_ms.keys().collect::<Vec<_>>(),
            "cpu_prepare_ms":distribution(&p.cpu_prepare_ms),"cpu_submit_ms":distribution(&p.cpu_submit_ms),
            "gpu_elapsed_ms":distribution(&gpu),"candidate_draws":distribution(&p.candidate_draws),
            "submitted_draws":distribution(&p.submitted_draws),"candidate_triangles":distribution(&p.candidate_triangles),
            "submitted_triangles":distribution(&p.submitted_triangles)
        }))
    }).collect();
    let capture = if std::env::var_os("ARKLINE_COCKPIT_PROFILE_CAPTURE").is_some() {
        let shot = h.frame(Vec::new());
        Some(super::capture(
            Some(&case_dir),
            "outside-sample-window",
            &shot,
        ))
    } else {
        None
    };
    let mut fixture = authority_fixture(&h.directory);
    fixture["pilot"] = PROFILE_PILOT.into();
    let result = serde_json::json!({
        "name":name,"feeds_requested":feeds,"free_flight":free_flight,"pilot_before":before,"pilot_after":after,
        "job_before":before_job,"simulation_range_ms":[first,last],"whole_harness_call_wall_ms":distribution(&whole_calls_ms),
        "render_frame_reported_wall_ms":distribution(&render_frame_ms),
        "whole_call_intervals_over_33_3_ms":whole_calls_ms.iter().filter(|&&x|x>1000.0/30.0).count(),
        "whole_call_intervals_over_50_ms":whole_calls_ms.iter().filter(|&&x|x>50.0).count(),
        "last_mesh_pass_draw_calls":distribution(&draw_calls),"last_mesh_pass_triangles":distribution(&mesh_triangles),
        "passes":passes,"capture_outside_samples":capture,"retained_fixture":fixture
    });
    std::fs::write(
        case_dir.join("profile.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    result
}

#[test]
#[ignore = "Explicit matched-source native profiling; retained disposable authority; no runtime FPS claim"]
fn native_pioneer_cockpit_no_readback_profile() {
    unsafe {
        let directory = PathBuf::from(
            std::env::var_os("ARKLINE_COCKPIT_PROFILE_EVIDENCE")
                .expect("New retained evidence directory required"),
        );
        std::fs::create_dir(&directory).unwrap();
        let (_context, gl) = PrivateContext::create();
        let seed = directory.join("authority-seed.sqlite");
        if let Some(source) = std::env::var_os("ARKLINE_COCKPIT_PROFILE_SOURCE_BACKUP") {
            let source = PathBuf::from(source);
            assert_eq!(
                source.file_name().and_then(|s| s.to_str()),
                Some("authority-seed.sqlite")
            );
            assert!(
                source.is_file()
                    && !std::fs::symlink_metadata(&source)
                        .unwrap()
                        .file_type()
                        .is_symlink()
            );
            std::fs::copy(source, &seed).unwrap();
        } else {
            let seed_dir = directory.join("seed-fixture");
            std::fs::create_dir(&seed_dir).unwrap();
            let mut fixture = Harness::new(gl.clone(), Some(seed_dir));
            drop(fixture.world.take());
            std::fs::copy(fixture.directory.join("world.sqlite"), &seed).unwrap();
        }
        use sha2::{Digest, Sha256};
        let seed_hash = hex::encode(Sha256::digest(std::fs::read(&seed).unwrap()));
        let mut cases = Vec::new();
        for (name, space, feeds) in [
            ("docked-seated", false, false),
            ("mining-feeds-off", true, false),
            ("mining-five-feeds-on", true, true),
        ] {
            cases.push(profile_case(
                gl.clone(),
                &directory,
                &seed,
                name,
                space,
                feeds,
            ));
        }
        assert_eq!(
            hex::encode(Sha256::digest(std::fs::read(&seed).unwrap())),
            seed_hash,
            "Seed must remain unchanged"
        );
        let receipt = serde_json::json!({
            "build_version":crate::BUILD_VERSION.trim(),"source_label":std::env::var("ARKLINE_COCKPIT_PROFILE_SOURCE_LABEL").ok(),
            "input_pixels":[1500,772],"scene_pixels":[1500,638],"framebuffer_pixels":[WIDTH,HEIGHT],
            "warmup_frames":WARMUP_FRAMES,"measured_frames":SAMPLE_FRAMES,"nonblocking_query_drain_frames":DRAIN_FRAMES,
            "authority_seed":seed,"authority_seed_sha256":seed_hash,"cases":cases,
            "scope":"Private macOS CGL native engine path. No screenshot readback, glFinish or query-result wait in measured windows. This is not desktop display FPS or input tracking latency.",
            "confounds":["Whole harness wall includes synchronous authority advance and snapshot serialization, UI update/tessellation, driver submission and possible driver throttling; it does not isolate UI work.","CPU pass timing excludes prior canonical body/occlusion preparation in egui update.","GPU timers are asynchronous unique completed samples, and unsupported or pending samples remain unavailable.","Authority advances50simulationms per frame; orbital/mining state advances normally. This is not a wall-clock-paced server benchmark.","No OS event delivery, vsync presentation or live-user input timing is measured.","Last-mesh-pass counters are not guaranteed whole-frame sums across feeds.","Some camera modes currently share the drone:unbound timing key. Such a key represents only its last retained feed result, not all five feeds or total GPU time.","Main-pass timings/counters exclude separate physical-display callbacks and must not be reported as whole-frame GPU work."]
        });
        std::fs::write(
            directory.join("profile.json"),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
    }
}
