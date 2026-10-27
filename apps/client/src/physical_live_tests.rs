// Opt-in live, wall-clock EGL measurement. No documentary frame exports and no OS-window claim.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};

fn distribution(values: &[f64]) -> serde_json::Value {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    if v.is_empty() {
        return serde_json::json!({"count":0});
    }
    serde_json::json!({"count":v.len(),"median_ms":v[v.len()/2],"p95_ms":v[((v.len()-1) as f64*0.95).ceil() as usize],"max_ms":v[v.len()-1]})
}
fn rss_kib() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find(|s| s.starts_with("VmRSS:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}
fn labels(output: &egui::FullOutput) -> Vec<(String, egui::Pos2)> {
    output
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                let point = t.pos + t.galley.rect.center().to_vec2();
                s.clip_rect
                    .contains(point)
                    .then(|| (t.galley.job.text.clone(), point))
            } else {
                None
            }
        })
        .collect()
}
fn pointer(point: egui::Pos2, down: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(point),
        egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed: down,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

#[test]
#[ignore = "Live local bridge + actual EGL renderer/pointer benchmark; no OS-window or deployment acceptance"]
fn live_physical_bay_frame_and_bridge_measurement() {
    let out = PathBuf::from(
        std::env::var_os("ARKLINE_LIVE_OUTPUT").expect("explicit fresh evidence directory"),
    );
    std::fs::create_dir(&out).unwrap();
    let source = std::env::var("ARKLINE_CAPTURE_SOURCE").unwrap();
    let seconds = std::env::var("ARKLINE_LIVE_SECONDS")
        .unwrap_or_else(|_| "24".into())
        .parse::<f64>()
        .unwrap();
    let overall = Instant::now();
    let memory_before = rss_kib();
    let options = options_for_tests(out.join("world.sqlite"));
    let (commands, incoming, stop, worker) =
        crate::physical_cargo_session::live_bridge_for_tests(options);
    let (forward, receiver) = std::sync::mpsc::channel();
    let arrivals = Arc::new(Mutex::new(Vec::<(u64, f64, bool)>::new()));
    let acknowledgements = Arc::new(Mutex::new(Vec::<(bool, String, Option<u64>)>::new()));
    let proxy_done = Arc::new(AtomicBool::new(false));
    let proxy = {
        let arrivals = arrivals.clone();
        let acknowledgements = acknowledgements.clone();
        let done = proxy_done.clone();
        std::thread::spawn(move || {
            while let Ok(update) = incoming.recv() {
                if let crate::Update::PhysicalResponse(r, visible, _, _) = &update {
                    if *visible {
                        acknowledgements.lock().unwrap().push((
                            r.ok,
                            r.message.clone(),
                            r.snapshot.as_ref().map(|s| s.server_time_ms),
                        ));
                    }
                    if let Some(s) = &r.snapshot {
                        let elapsed = overall.elapsed().as_secs_f64() * 1000.;
                        arrivals
                            .lock()
                            .unwrap()
                            .push((s.server_time_ms, elapsed, *visible));
                    }
                }
                if forward.send(update).is_err() {
                    break;
                }
            }
            done.store(true, Ordering::Release);
        })
    };
    let mut frame_rows = vec![];
    let mut actions = vec![];
    let mut phases = vec![];
    let mut startup = None;
    let mut command_wait: Option<(String, Instant, usize)> = None;
    let mut completed_actions = 0usize;
    let mut failures = Vec::<String>::new();
    let mut physical_frames = 0usize;
    let mut acknowledged_for_paint: Option<(String, Instant)> = None;
    let mut last_clock = None;
    let mut last_frame = None;
    let mut released = None;
    let mut current_labels = vec![];
    let sequence = [
        (2., "Launch / resume"),
        (5., "Hide monitor"),
        (6., "Show monitor"),
        (10., "Hold"),
        (12., "Recall"),
    ];
    let mut next_action = 0;
    let mut action_start = None;
    let mut last_render = None;
    let mut last_physical: Option<Arc<crate::flight3d::physical_source_scene::Scene>> = None;
    unsafe {
        let (_egl, gl) = super::super::exterior_gpu_tests::Egl::create();
        let renderer_info = serde_json::json!({"vendor":gl.get_parameter_string(glow::VENDOR),"renderer":gl.get_parameter_string(glow::RENDERER),"version":gl.get_parameter_string(glow::VERSION)});
        let mut renderer = Renderer::new(gl.clone()).unwrap();
        renderer.profile_phase_sync = std::env::var("ARKLINE_RENDER_PHASE_SYNC").as_deref() == Ok("1");
        let target = CachedFeed::create(&gl, 170491, 960, 640).unwrap();
        let ctx = egui::Context::default();
        ctx.style_mut(|s| s.animation_time = 0.);
        let mut app = crate::tests::view_test_cockpit_with_profile(&ctx, out.join("client"));
        app.physical_session = true;
        app.pilot = OWNER.into();
        app.connected = false;
        app.connecting = true;
        app.snapshot = None;
        app.assets = None;
        app.physical_cargo = None;
        app.commands = Some(commands);
        app.updates = Some(receiver);
        app.flight_view = FlightView::new(Some(&gl));
        let mut painter = egui_glow::Painter::new(gl.clone(), "", None, false).unwrap();
        let render_started = Instant::now();
        loop {
            let begin = Instant::now();
            let wall = overall.elapsed().as_secs_f64() * 1000.;
            if render_started.elapsed().as_secs_f64() >= seconds {
                break;
            }
            let poll = Instant::now();
            app.poll();
            let poll_ms = poll.elapsed().as_secs_f64() * 1000.;
            if app.physical_cargo.is_some() && startup.is_none() {
                startup = Some(overall.elapsed().as_secs_f64() * 1000.);
                action_start = Some(Instant::now());
                phases.push(serde_json::json!({"stage":"first_authority_and_lazy_assets","wall_ms":startup,"rss_kib":rss_kib(),"poll_ms":poll_ms}));
            }
            if command_wait.is_some() && !app.request_pending {
                let (name, when, previous_acks) = command_wait.take().unwrap();
                let replies = acknowledgements.lock().unwrap();
                let reply = replies.get(previous_acks);
                let accepted = replies.len() == previous_acks + 1
                    && reply.is_some_and(|r| r.0)
                    && app.connected
                    && app.physical_cargo.is_some();
                actions.push(serde_json::json!({"event":"authority_consumed","name":name.clone(),"accepted":accepted,"reply":reply,"latency_ms":when.elapsed().as_secs_f64()*1000.,"clock":app.snapshot.as_ref().map(|s|s.server_time_ms),"held":app.physical_cargo.as_ref().and_then(|v|v.bay_state.as_ref().map(|b|b.held))}));
                if accepted {
                    completed_actions += 1;
                    acknowledged_for_paint = Some((name, when));
                } else {
                    failures.push(format!(
                        "{name}: missing, ambiguous, rejected or disconnected acknowledgement"
                    ));
                }
            }
            let mut events = vec![];
            let mut released_action = None;
            if let Some((name, point)) = released.take() {
                events = pointer(point, false);
                actions.push(
                    serde_json::json!({"event":"pointer_release","name":name,"wall_ms":wall}),
                );
                released_action = Some((
                    name,
                    Instant::now(),
                    acknowledgements.lock().unwrap().len(),
                    app.physical_panel_open,
                    app.request_pending,
                ));
            } else if let Some(start) = action_start {
                if next_action < sequence.len()
                    && start.elapsed().as_secs_f64() >= sequence[next_action].0
                    && !app.request_pending
                {
                    let name = sequence[next_action].1;
                    if let Some((_, point)) = current_labels.iter().find(|(text, _)| text == name) {
                        events = pointer(*point, true);
                        released = Some((name.to_owned(), *point));
                        next_action += 1;
                    }
                }
            }
            let update = Instant::now();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(960., 640.),
                    )),
                    time: Some(overall.elapsed().as_secs_f64()),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::Window::new("PIONEER · LIVE LOCAL BRIDGE")
                        .fade_in(false)
                        .fade_out(false)
                        .fixed_pos(egui::pos2(610., 12.))
                        .fixed_size(egui::vec2(335., 510.))
                        .resizable(false)
                        .collapsible(false)
                        .show(ctx, |ui| {
                            if let Some(pilot) = app.own() {
                                app.physical_cargo_controls(ui, &pilot);
                            } else {
                                ui.label(&app.notice);
                            }
                        });
                },
            );
            if let Some((name, when, previous_acks, was_open, was_pending)) = released_action {
                if name == "Launch / resume" || name == "Hold" || name == "Recall" {
                    let dispatched = !was_pending && app.request_pending;
                    actions.push(serde_json::json!({"event":"dispatch_observed","name":name,"request_pending":app.request_pending,"dispatched":dispatched}));
                    if dispatched {
                        command_wait = Some((name, when, previous_acks));
                    } else {
                        failures.push(format!("{name}: pointer release did not dispatch command"));
                    }
                } else {
                    let expected_open = name == "Show monitor";
                    let toggled = was_open != app.physical_panel_open
                        && app.physical_panel_open == expected_open
                        && app.request_pending == was_pending;
                    actions.push(serde_json::json!({"event":"monitor_toggle_observed","name":name,"was_open":was_open,"is_open":app.physical_panel_open,"request_pending_unchanged":app.request_pending==was_pending,"accepted":toggled}));
                    if toggled {
                        completed_actions += 1;
                    } else {
                        failures.push(format!(
                            "{name}: visibility/pending state did not match local-only toggle"
                        ));
                    }
                }
            }
            current_labels = labels(&output);
            let ui_ms = update.elapsed().as_secs_f64() * 1000.;
            let prepare = Instant::now();
            if let (Some(snapshot), Some(view)) = (&app.snapshot, &app.physical_cargo) {
                if !last_physical
                    .as_ref()
                    .zip(app.flight_view.physical.as_ref())
                    .is_some_and(|(a, b)| Arc::ptr_eq(a, b))
                    || last_frame.is_none()
                {
                    let mut frame = super::super::tests::frame_for_scene_clone_test();
                    let eye = Vec3::new(-40., 12., -61.) / 100.;
                    let aim = Vec3::new(-26., 0., -29.) / 100.;
                    let vp = Mat4::orthographic_rh_gl(-0.21, 0.21, -0.14, 0.14, 0.001, 8.)
                        * Mat4::look_at_rh(eye, aim, Vec3::Y);
                    frame.pioneer_parts = None;
                    frame.exterior = None;
                    frame.snapshot_time = snapshot.server_time_ms;
                    frame.now = snapshot.server_time_ms;
                    frame.own = OWNER.into();
                    frame.system_id = view.state.binding.system_id;
                    frame.ship_class = ShipClass::Pioneer;
                    frame.cockpit = false;
                    frame.connected = true;
                    frame.paused = false;
                    frame.space = Space { origin: [0.; 3] };
                    frame.eye = eye;
                    frame.vp = vp;
                    frame.cockpit_vp = vp;
                    frame.planet_vp = vp;
                    frame.scene = Arc::new(FrameScene {
                        physical: app.flight_view.physical.clone(),
                        carrier_inventory_registration: None,
                        world_layout: arkline_protocol::legacy_world_layout(),
                        traffic: snapshot.traffic.clone(),
                        surface_props: snapshot.surface_props.clone(),
                        asteroids: snapshot.asteroids.clone(),
                        gates: vec![],
                        pilots: snapshot.pilots.clone(),
                    });
                    last_frame = Some(frame);
                    last_physical = app.flight_view.physical.clone();
                }
            }
            let prepare_ms = prepare.elapsed().as_secs_f64() * 1000.;
            let paint = Instant::now();
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            gl.viewport(0, 0, 960, 640);
            gl.disable(glow::SCISSOR_TEST);
            if let Some(frame) = &last_frame {
                if frame.scene.physical.is_some() {
                    physical_frames += 1;
                }
                renderer.paint(frame, Some(target.framebuffer));
            }
            let renderer_submit_ms = paint.elapsed().as_secs_f64() * 1000.;
            let ui_paint = Instant::now();
            let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
            painter.paint_and_update_textures(
                [960, 640],
                output.pixels_per_point,
                &primitives,
                &output.textures_delta,
            );
            let ui_submit_ms = ui_paint.elapsed().as_secs_f64() * 1000.;
            let finish = Instant::now();
            gl.finish();
            let finish_wait_ms = finish.elapsed().as_secs_f64() * 1000.;
            let debug = renderer.debug_metrics.get();
            let view_metrics = renderer.view_profiler.borrow().samples().into_iter().map(|m| serde_json::json!({"key":m.key,"clock_ms":m.frame,"cpu_submit_ms":m.cpu_submit_ns as f64/1e6,"gpu_elapsed_ms":m.gpu_elapsed_ns.map(|n|n as f64/1e6),"gpu_clock_ms":m.gpu_frame,"candidate_draws":m.visibility.candidate_draws,"submitted_draws":m.visibility.submitted_draws,"candidate_triangles":m.visibility.candidate_triangles,"submitted_triangles":m.visibility.submitted_triangles,"cull_prepare_ms":m.visibility.cpu_prepare_ns as f64/1e6})).collect::<Vec<_>>();
            let paint_ms = paint.elapsed().as_secs_f64() * 1000.;
            if let Some((name, when)) = acknowledged_for_paint.take() {
                actions.push(serde_json::json!({"event":"authority_frame_finished","name":name,"latency_ms":when.elapsed().as_secs_f64()*1000.,"note":"EGL GL finish; not compositor display latency"}));
            }
            let clock = app.snapshot.as_ref().map(|s| s.server_time_ms);
            frame_rows.push(serde_json::json!({"frame_start_wall_ms":wall,"clock_ms":clock,"rendered_clock_ms":last_frame.as_ref().map(|f|f.snapshot_time),"frame_interval_ms":last_render.map(|i:Instant|begin.duration_since(i).as_secs_f64()*1000.),"poll_ms":poll_ms,"ui_ms":ui_ms,"prepare_ms":prepare_ms,"paint_finish_ms":paint_ms,"renderer_submit_ms":renderer_submit_ms,"ui_submit_ms":ui_submit_ms,"finish_wait_ms":finish_wait_ms,"draw_calls":debug.draw_calls,"triangles":debug.triangles,"view_metrics":view_metrics,"phase_synchronized":renderer.profile_phase_sync,"shadow_ms":renderer.profile_shadow.get().0,"shadow_draws":renderer.profile_shadow.get().1,"shadow_triangles":renderer.profile_shadow.get().2,"color_ms":renderer.profile_color_ms.get(),"producer_to_frame_ms":app.performance_snapshot_received.map(|when|when.elapsed().as_secs_f64()*1000.),"new_clock":clock!=last_clock,"held":app.physical_cargo.as_ref().and_then(|v|v.bay_state.as_ref().map(|b|b.held)),"pending":app.request_pending}));
            last_clock = clock;
            last_render = Some(begin);
            let duration = begin.elapsed();
            if duration < Duration::from_micros(16667) {
                std::thread::sleep(Duration::from_micros(16667) - duration);
            }
        }
        let final_clock = app.snapshot.as_ref().map(|s| s.server_time_ms);
        phases.push(serde_json::json!({"stage":"before_shutdown","clock_ms":final_clock,"rss_kib":rss_kib(),"has_physical":app.physical_cargo.is_some()}));
        let shutdown = Instant::now();
        stop.store(true, Ordering::Release);
        app.commands = None;
        app.updates = None;
        drop(worker);
        proxy.join().unwrap();
        let shutdown_ms = shutdown.elapsed().as_secs_f64() * 1000.;
        assert!(proxy_done.load(Ordering::Acquire));
        let metrics = |name: &str| {
            frame_rows
                .iter()
                .filter_map(|r| r[name].as_f64())
                .collect::<Vec<_>>()
        };
        let rows = arrivals.lock().unwrap().clone();
        let periodic = rows
            .iter()
            .filter(|(_, _, visible)| !*visible)
            .collect::<Vec<_>>();
        let intervals = periodic
            .windows(2)
            .map(|w| w[1].1 - w[0].1)
            .collect::<Vec<_>>();
        if startup.is_none() || physical_frames == 0 {
            failures.push("No accepted physical source frames rendered".into());
        }
        if completed_actions != sequence.len() || command_wait.is_some() || released.is_some() {
            failures.push(format!(
                "Action sequence incomplete: {completed_actions}/{} accepted",
                sequence.len()
            ));
        }
        let complete = failures.is_empty();
        let report = serde_json::json!({"result":if complete {"measured_complete"} else {"measured_incomplete"},"failures":failures,"completed_actions":completed_actions,"physical_frames":physical_frames,"source":source,"scope":"Live same-process authority bridge, actual EGL Renderer::paint and existing egui controls; no OS window, compositor, network, onboard cameras, or broad gameplay/FPS acceptance","profile":if cfg!(debug_assertions){"development/test with workspace package optimization"}else{"release"},"renderer":renderer_info,"dimensions":[960,640],"target_frame_ms":16.667,"startup_first_authority_ms":startup,"memory_before_kib":memory_before,"memory_end_kib":rss_kib(),"shutdown_ms":shutdown_ms,"simulation_final_ms":final_clock,"wall_total_ms":overall.elapsed().as_secs_f64()*1000.,"summary":{"frames":frame_rows.len(),"frame_interval":distribution(&metrics("frame_interval_ms")),"poll":distribution(&metrics("poll_ms")),"ui":distribution(&metrics("ui_ms")),"paint_finish":distribution(&metrics("paint_finish_ms")),"response_interval":distribution(&intervals)},"frame_rows":frame_rows,"bridge_arrivals":rows,"actions":actions,"phases":phases,"frame_exports":0,"per_frame_json_exports":0,"raw_authority_validation_skipped":false});
        std::fs::write(
            out.join("live-report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        painter.destroy();
        renderer.destroy();
        target.destroy(&gl);
        assert!(
            complete,
            "Live sequence incomplete; inspect saved live-report.json"
        );
    }
}

#[test]
#[ignore = "Renderer-only resolution scaling diagnostic, not a production quality change or FPS acceptance"]
fn physical_render_resolution_cost_profile() {
    let output = std::env::var_os("ARKLINE_RENDER_RESOLUTION_OUTPUT").unwrap();
    let response = super::initial_response_for_tests();
    let frame = super::physical_frame(&response);
    let mut rows = Vec::new();
    unsafe {
        let (_egl, gl) = super::super::exterior_gpu_tests::Egl::create();
        let mut renderer = Renderer::new(gl.clone()).unwrap();
        for (width, height) in [(480,320),(960,640),(1920,1280)] {
            let target = CachedFeed::create(&gl, 170491, width, height).unwrap();
            let mut samples = Vec::new();
            for index in 0..35 {
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
                gl.viewport(0,0,width,height);
                gl.disable(glow::SCISSOR_TEST);
                let start = Instant::now();
                renderer.paint(&frame, Some(target.framebuffer));
                gl.finish();
                if index>=5 { samples.push(start.elapsed().as_secs_f64()*1000.); }
            }
            let metrics = renderer.debug_metrics.get();
            rows.push(serde_json::json!({"dimensions":[width,height],"draw_calls":metrics.draw_calls,"triangles":metrics.triangles,"render_and_finish":distribution(&samples),"samples_ms":samples}));
            target.destroy(&gl);
        }
        std::fs::write(output, serde_json::to_vec_pretty(&serde_json::json!({"scope":"Renderer-only immutable initial physical source/frame at three diagnostic resolutions; no UI/bridge or production quality changes; no hardware or gameplay FPS claim","source":std::env::var("ARKLINE_CAPTURE_SOURCE").unwrap(),"renderer":gl.get_parameter_string(glow::RENDERER),"rows":rows})).unwrap()).unwrap();
        renderer.destroy();
    }
}
