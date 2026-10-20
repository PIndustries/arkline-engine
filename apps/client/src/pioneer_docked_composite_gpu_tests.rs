//! Whole Cockpit/secondary-computer/registered-Hangar native composition. No OS window.
use super::*;
use sha2::{Digest, Sha256};
use std::ffi::{c_void, CString};
use std::path::Path;

#[link(name = "OpenGL", kind = "framework")]
unsafe extern "C" {
    fn CGLChoosePixelFormat(a: *const u32, f: *mut *mut c_void, n: *mut i32) -> i32;
    fn CGLCreateContext(f: *mut c_void, s: *mut c_void, c: *mut *mut c_void) -> i32;
    fn CGLGetCurrentContext() -> *mut c_void;
    fn CGLSetCurrentContext(c: *mut c_void) -> i32;
    fn CGLDestroyContext(c: *mut c_void) -> i32;
    fn CGLDestroyPixelFormat(f: *mut c_void) -> i32;
}
unsafe extern "C" {
    fn dlsym(handle: *mut c_void, name: *const std::ffi::c_char) -> *mut c_void;
}

struct PrivateContext {
    current: *mut c_void,
    previous: *mut c_void,
}
impl PrivateContext {
    unsafe fn create() -> (Self, Arc<glow::Context>) {
        let previous = CGLGetCurrentContext();
        let mut format = std::ptr::null_mut();
        let mut count = 0;
        assert_eq!(
            CGLChoosePixelFormat(
                [99, 0x3200, 73, 8, 24, 12, 24, 0].as_ptr(),
                &mut format,
                &mut count
            ),
            0
        );
        assert!(!format.is_null() && count > 0);
        let mut current = std::ptr::null_mut();
        let result = CGLCreateContext(format, std::ptr::null_mut(), &mut current);
        assert_eq!(CGLDestroyPixelFormat(format), 0);
        assert_eq!(result, 0);
        assert!(!current.is_null());
        let guard = Self { current, previous };
        assert_eq!(CGLSetCurrentContext(current), 0);
        let gl = Arc::new(glow::Context::from_loader_function(|name| {
            dlsym(
                (-2_isize) as *mut c_void,
                CString::new(name).unwrap().as_ptr(),
            )
            .cast()
        }));
        (guard, gl)
    }
}
impl Drop for PrivateContext {
    fn drop(&mut self) {
        unsafe {
            let _ = CGLSetCurrentContext(self.previous);
            let _ = CGLDestroyContext(self.current);
        }
    }
}

use arkline_protocol::{
    AssetsView, DockedLiftState, DockedLiftStop, DockedSupport, DockedTraversalState,
    DockedVesselActuators, PioneerRoomState, RoomLayoutState, StationAvatar,
    DOCKED_TRAVERSAL_VERSION,
};
use std::sync::mpsc;

const WIDTH: usize = 1500;
const HEIGHT: usize = 960;
const PARENT_TEXT: &str = "docked_composite_parent_text";

struct Shot {
    rgba: Vec<u8>,
    surface: Option<CockpitSurface>,
    labels: Vec<(String, egui::Pos2)>,
    parent_labels: Vec<(String, egui::Pos2)>,
    computer_vertices: usize,
    head: [f32; 2],
    depth_pixels: usize,
}
impl Shot {
    fn canonical_label(&self, label: &str) -> egui::Pos2 {
        self.labels
            .iter()
            .find(|(text, _)| text == label)
            .unwrap_or_else(|| {
                panic!(
                    "Real secondary-computer label {label:?} missing; actual labels {:?}",
                    self.labels.iter().map(|(text, _)| text).collect::<Vec<_>>()
                )
            })
            .1
    }
    fn parent_label(&self, label: &str) -> egui::Pos2 {
        self.parent_labels
            .iter()
            .find(|(text, _)| text == label)
            .unwrap_or_else(|| panic!("Visible actual parent control {label:?} missing"))
            .1
    }
    fn physical_label(&self, label: &str) -> egui::Pos2 {
        self.surface
            .as_ref()
            .expect("Physical computer surface")
            .project(self.canonical_label(label))
            .unwrap_or_else(|| {
                panic!("Label {label:?} must project onto its actual fitted physical backing")
            })
    }
}
struct Harness {
    gl: Arc<glow::Context>,
    ctx: egui::Context,
    cockpit: crate::Cockpit,
    painter: egui_glow::Painter,
    target: Option<CachedFeed>,
    time: f64,
    parent_text: String,
    parent_focus: bool,
}
impl Harness {
    unsafe fn new(gl: Arc<glow::Context>) -> (Self, mpsc::Receiver<arkline_protocol::Request>) {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.hangar_view = hangar::HangarView::new(Some(&gl));
        cockpit.embedded = true;
        cockpit.embedded_input = true;
        cockpit.embedded_paused = false;
        cockpit.focus_on_first_frame = false;
        cockpit.view_mode = crate::ViewMode::Cockpit;
        cockpit.services_open = false;
        cockpit.overview_open = false;
        cockpit.drone_camera = false;
        cockpit.service_tab = crate::ServiceTab::Corporation;
        cockpit.sound_settings.muted = true;
        let snapshot = cockpit.snapshot.as_mut().unwrap();
        snapshot.docked_traversal_capability =
            Some(arkline_protocol::docked_traversal_capability());
        snapshot.pilots[0].location = Location::Docked;
        snapshot.pilots[0].station_avatar = None;
        cockpit.assets = Some(AssetsView {
            pioneer_room: Some(PioneerRoomState::uncommissioned(
                "native-composite-owned-pioneer".into(),
            )),
            pioneer_room_layout: Some(RoomLayoutState {
                revision: 1,
                document: arkline_protocol::default_pioneer_room_layout(),
            }),
            hangar: None,
            pioneer_interior_available: true,
            pioneer_interior_decks: 2,
            ships: vec![],
            storage: vec![],
            corporation: None,
            invitations: vec![],
        });
        assert!(
            cockpit.verified_docked_binding().is_some(),
            "The full client must choose its real negotiated registered route"
        );
        assert!(cockpit.updates.is_none() && cockpit.local_server.is_none());
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let painter = egui_glow::Painter::new(gl.clone(), "", None, false).unwrap();
        let target = CachedFeed::create(&gl, 9030, WIDTH as i32, HEIGHT as i32).unwrap();
        (
            Self {
                gl,
                ctx,
                cockpit,
                painter,
                target: Some(target),
                time: 0.0,
                parent_text: String::new(),
                parent_focus: false,
            },
            rx,
        )
    }
    unsafe fn frame(
        &mut self,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
        focused: bool,
    ) -> Shot {
        self.time += 0.05;
        let now = 1000 + (self.time * 1000.0).round() as u64;
        self.cockpit.snapshot.as_mut().unwrap().server_time_ms = now;
        self.cockpit.frozen_time = Some(now);
        self.cockpit.timeline_origin = Some(now);
        self.cockpit.observed = crate::Instant::now();
        let source = serde_json::to_vec(&self.cockpit.snapshot).unwrap();
        let output = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(WIDTH as f32, HEIGHT as f32),
                )),
                time: Some(self.time),
                predicted_dt: 0.05,
                focused,
                modifiers,
                events,
                ..Default::default()
            },
            |ctx| {
                if self.parent_focus {
                    egui::Area::new(egui::Id::new("docked_composite_parent_area"))
                        .order(egui::Order::Foreground)
                        .fixed_pos(egui::pos2(8.0, 8.0))
                        .show(ctx, |ui| {
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut self.parent_text)
                                    .id(egui::Id::new(PARENT_TEXT))
                                    .desired_width(240.0),
                            );
                            if !response.has_focus() {
                                response.request_focus();
                            }
                        });
                }
                eframe::App::update(&mut self.cockpit, ctx, &mut eframe::Frame::_new_kittest());
            },
        );
        assert_eq!(
            serde_json::to_vec(&self.cockpit.snapshot).unwrap(),
            source,
            "UI input cannot mutate received authority data"
        );
        let parent_labels = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => {
                    let point = text.pos + text.galley.rect.center().to_vec2();
                    shape
                        .clip_rect
                        .contains(point)
                        .then(|| (text.galley.text().to_owned(), point))
                }
                _ => None,
            })
            .collect();
        let surface = self.cockpit.hangar_view.computer_surface();
        let labels = self.cockpit.computer_ui_labels.clone();
        let computer_vertices = self
            .cockpit
            .cockpit_computer
            .debug_primitives()
            .iter()
            .map(|primitive| match &primitive.primitive {
                egui::epaint::Primitive::Mesh(mesh) => mesh.vertices.len(),
                _ => 0,
            })
            .sum();
        let primitives = self
            .ctx
            .tessellate(output.shapes.clone(), output.pixels_per_point);
        let callbacks = primitives
            .iter()
            .filter(|primitive| matches!(primitive.primitive, egui::epaint::Primitive::Callback(_)))
            .count();
        assert!(
            callbacks >= 1,
            "Full registered scene callback must survive composition"
        );
        let target = self.target.as_ref().unwrap();
        self.gl
            .bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
        self.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
        self.gl.disable(glow::SCISSOR_TEST);
        self.gl.color_mask(true, true, true, true);
        self.gl.depth_mask(true);
        self.gl.clear_depth_f32(1.0);
        self.gl.clear_color(0.0, 0.0, 0.0, 1.0);
        self.gl
            .clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        self.painter.paint_and_update_textures(
            [WIDTH as u32, HEIGHT as u32],
            output.pixels_per_point,
            &primitives,
            &output.textures_delta,
        );
        assert_eq!(
            self.gl.get_error(),
            glow::NO_ERROR,
            "Whole Cockpit, scene, physical display and secondary-context texture composition"
        );
        self.gl
            .bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target.framebuffer));
        let mut rgba = vec![0; WIDTH * HEIGHT * 4];
        self.gl.read_pixels(
            0,
            0,
            WIDTH as i32,
            HEIGHT as i32,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut rgba)),
        );
        let mut depth = vec![0; WIDTH * HEIGHT * 4];
        self.gl.read_pixels(
            0,
            0,
            WIDTH as i32,
            HEIGHT as i32,
            glow::DEPTH_COMPONENT,
            glow::FLOAT,
            glow::PixelPackData::Slice(Some(&mut depth)),
        );
        assert_eq!(self.gl.get_error(), glow::NO_ERROR);
        let depth_pixels = depth
            .chunks_exact(4)
            .filter(|bytes| {
                let d = f32::from_ne_bytes((*bytes).try_into().unwrap());
                assert!(d.is_finite() && (0.0..=1.0).contains(&d));
                d < 1.0 - 1e-7
            })
            .count();
        Shot {
            rgba,
            surface,
            labels,
            parent_labels,
            computer_vertices,
            head: self.cockpit.hangar_view.head_look_angles(),
            depth_pixels,
        }
    }
    unsafe fn settle(&mut self) -> Shot {
        for _ in 0..4 {
            self.frame(vec![], egui::Modifiers::NONE, true);
        }
        self.frame(vec![], egui::Modifiers::NONE, true)
    }
    unsafe fn click(&mut self, point: egui::Pos2) -> Shot {
        self.frame(
            vec![egui::Event::PointerMoved(point)],
            egui::Modifiers::NONE,
            true,
        );
        self.frame(pointer(point, true), egui::Modifiers::NONE, true);
        self.frame(pointer(point, false), egui::Modifiers::NONE, true)
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.cockpit.hangar_view.destroy();
        self.cockpit.flight_view.destroy();
        self.painter.destroy();
        if let Some(target) = self.target.take() {
            unsafe {
                target.destroy(&self.gl);
            }
        }
    }
}
fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
fn key(key: egui::Key, pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers,
    }
}
fn surface_samples(shot: &Shot) -> Vec<u8> {
    let surface = shot.surface.as_ref().unwrap();
    let canvas = surface.canvas_rect().shrink(8.0);
    let mut result = Vec::new();
    for y in 0..96 {
        for x in 0..128 {
            let point = canvas.min
                + canvas.size() * egui::vec2((x as f32 + 0.5) / 128.0, (y as f32 + 0.5) / 96.0);
            if let Some(screen) = surface.project(point) {
                let x = screen.x.floor() as isize;
                let y = screen.y.floor() as isize;
                if x >= 0 && x < WIDTH as isize && y >= 0 && y < HEIGHT as isize {
                    let offset = ((HEIGHT - 1 - y as usize) * WIDTH + x as usize) * 4;
                    result.extend_from_slice(&shot.rgba[offset..offset + 4]);
                }
            }
        }
    }
    result
}
fn capture(directory: Option<&Path>, name: &str, shot: &Shot) -> serde_json::Value {
    let top_down = shot
        .rgba
        .chunks_exact(WIDTH * 4)
        .rev()
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    let png = format!("{name}.png");
    if let Some(directory) = directory {
        image::save_buffer_with_format(
            directory.join(&png),
            &top_down,
            WIDTH as u32,
            HEIGHT as u32,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .unwrap();
    }
    let samples = shot.surface.as_ref().map(|_| surface_samples(shot));
    serde_json::json!({"png":directory.map(|_|png),"width":WIDTH,"height":HEIGHT,"orientation":"top-left RGBA8","rgba_sha256":hex::encode(Sha256::digest(top_down)),"physical_computer_present":shot.surface.is_some(),"computer_canonical_vertices":shot.computer_vertices,"computer_lit_samples":samples.map(|pixels|pixels.chunks_exact(4).filter(|pixel|pixel[..3].iter().any(|value|*value>70)).count()),"head_look_rad":shot.head,"occupied_depth_pixels":shot.depth_pixels})
}

#[test]
#[ignore = "Explicit whole Cockpit + registered docked computer native macOS composition and physical input; no window"]
fn native_pioneer_docked_composite_computer_pixels_input_and_focus() {
    unsafe {
        let directory = std::env::var_os("ARKLINE_PIONEER_DOCKED_COMPOSITE_EVIDENCE")
            .map(std::path::PathBuf::from);
        if let Some(directory) = &directory {
            std::fs::create_dir(directory).unwrap();
        }
        let (_context, gl) = PrivateContext::create();
        let (mut harness, requests) = Harness::new(gl.clone());
        let binding = harness.cockpit.verified_docked_binding().unwrap();
        let baseline = harness.settle();
        assert!(baseline.computer_vertices > 100);
        assert!(baseline.depth_pixels > 1000);
        let samples = surface_samples(&baseline);
        let lit = samples
            .chunks_exact(4)
            .filter(|pixel| pixel[..3].iter().any(|value| *value > 70))
            .count();
        assert!(lit>60,"Real secondary-context computer content must be visible on its physical central backing, lit samples {lit}");
        assert!(baseline.labels.iter().any(|(text, _)| text == "UNDOCK"));
        assert!(baseline.labels.iter().any(|(text, _)| text == "Corp"));
        let mut captures = vec![capture(
            directory.as_deref(),
            "composed-pilot-seat",
            &baseline,
        )];
        let button = baseline.physical_label("UNDOCK");
        let canonical = baseline.canonical_label("UNDOCK");
        let hit = harness
            .cockpit
            .hangar_view
            .computer_hit_tester(&harness.ctx)(button)
        .expect("Same physical button plane must pass actual depth/layer pointer guard");
        assert!(hit.distance(canonical) < 0.5);
        let hovered = harness.frame(
            vec![egui::Event::PointerMoved(button)],
            egui::Modifiers::NONE,
            true,
        );
        assert!(requests.try_recv().is_err(), "Hover cannot send a command");
        let pressed = harness.frame(pointer(button, true), egui::Modifiers::NONE, true);
        assert!(
            requests.try_recv().is_err(),
            "Press cannot execute the real button before release"
        );
        let released = harness.frame(pointer(button, false), egui::Modifiers::NONE, true);
        let request = requests
            .try_recv()
            .expect("Physical computer button release must emit its actual UI request");
        assert!(matches!(request, arkline_protocol::Request::Undock));
        assert!(requests.try_recv().is_err());
        captures.push(capture(
            directory.as_deref(),
            "computer-undock-hover",
            &hovered,
        ));
        captures.push(capture(
            directory.as_deref(),
            "computer-undock-pressed",
            &pressed,
        ));
        captures.push(capture(
            directory.as_deref(),
            "computer-undock-released",
            &released,
        ));
        // Acknowledge only the isolated transport flag, without executing the
        // command or changing any received world/ship state.
        harness.cockpit.request_pending = false;
        let corp = harness.settle();
        let hint = corp.physical_label("Corporation name");
        harness.click(hint);
        let typed = harness.frame(
            vec![egui::Event::Text("Native cockpit text".into())],
            egui::Modifiers::NONE,
            true,
        );
        assert_eq!(harness.cockpit.corporation_name, "Native cockpit text");
        assert!(harness.cockpit.cockpit_computer.wants_keyboard_input());
        captures.push(capture(
            directory.as_deref(),
            "physical-computer-typing",
            &typed,
        ));
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let (viewport, viewport_layer) = harness.cockpit.hangar_view.input_surface().unwrap();
        // The top-left status card owns its pixels. Find an actual unobstructed
        // viewport point instead of treating that overlay as scene input.
        let scene_pointer = (1..9)
            .flat_map(|y| {
                (1..9).map(move |x| {
                    egui::pos2(
                        viewport.left() + viewport.width() * x as f32 / 10.,
                        viewport.top() + viewport.height() * y as f32 / 10.,
                    )
                })
            })
            .find(|point| {
                harness.ctx.layer_id_at(*point) == Some(viewport_layer)
                    && !typed
                        .surface
                        .as_ref()
                        .is_some_and(|surface| surface.contains(*point))
            })
            .expect("An actual viewport-owned point outside physical UI must exist");
        let before = typed.head;
        let guarded = harness.frame(
            vec![
                egui::Event::PointerMoved(scene_pointer),
                key(egui::Key::ArrowLeft, true, shift),
            ],
            shift,
            true,
        );
        assert_eq!(
            guarded.head, before,
            "Secondary TextEdit keyboard ownership must freeze head look"
        );
        harness.frame(
            vec![key(egui::Key::ArrowLeft, false, egui::Modifiers::NONE)],
            egui::Modifiers::NONE,
            true,
        );
        captures.push(capture(
            directory.as_deref(),
            "computer-text-blocks-headlook",
            &guarded,
        ));
        let text_before = harness.cockpit.corporation_name.clone();
        let lost = harness.frame(
            vec![
                egui::Event::Text(" must not arrive".into()),
                key(egui::Key::ArrowRight, true, shift),
            ],
            shift,
            false,
        );
        assert_eq!(harness.cockpit.corporation_name, text_before);
        assert_eq!(lost.head, before);
        assert!(!harness.cockpit.cockpit_computer.wants_keyboard_input());
        captures.push(capture(directory.as_deref(), "window-focus-lost", &lost));
        harness.frame(
            vec![key(egui::Key::ArrowRight, false, egui::Modifiers::NONE)],
            egui::Modifiers::NONE,
            true,
        );
        // A real parent TextEdit owns keyboard input before Cockpit::update.
        harness.parent_focus = true;
        harness.frame(vec![], egui::Modifiers::NONE, true);
        let parent_before = harness.cockpit.hangar_view.head_look_angles();
        let parent = harness.frame(
            vec![
                egui::Event::Text("Parent fixture text".into()),
                egui::Event::PointerMoved(scene_pointer),
                key(egui::Key::ArrowLeft, true, shift),
            ],
            shift,
            true,
        );
        assert!(harness
            .ctx
            .memory(|memory| memory.has_focus(egui::Id::new(PARENT_TEXT))));
        assert_eq!(harness.parent_text, "Parent fixture text");
        assert_eq!(parent.head, parent_before);
        assert_eq!(harness.cockpit.corporation_name, text_before);
        captures.push(capture(directory.as_deref(), "parent-text-focus", &parent));
        harness.parent_focus = false;
        harness.ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        harness.frame(
            vec![key(egui::Key::ArrowLeft, false, egui::Modifiers::NONE)],
            egui::Modifiers::NONE,
            true,
        );
        assert_eq!(
            harness.ctx.layer_id_at(scene_pointer),
            Some(viewport_layer),
            "Positive headlook fixture must own the visible viewport pixels"
        );
        let look_before = harness.cockpit.hangar_view.head_look_angles();
        let mut looked = harness.frame(
            vec![
                egui::Event::PointerMoved(scene_pointer),
                key(egui::Key::ArrowLeft, true, shift),
            ],
            shift,
            true,
        );
        for _ in 0..2 {
            looked = harness.frame(vec![egui::Event::PointerMoved(scene_pointer)], shift, true);
        }
        assert!(
            looked.head[0] > look_before[0] + 0.01,
            "Owned viewport Shift+arrow must positively change head look"
        );
        captures.push(capture(directory.as_deref(), "composed-headlook", &looked));
        harness.frame(
            vec![key(egui::Key::ArrowLeft, false, egui::Modifiers::NONE)],
            egui::Modifiers::NONE,
            true,
        );
        for _ in 0..12 {
            harness.frame(
                vec![egui::Event::PointerMoved(scene_pointer)],
                egui::Modifiers::NONE,
                true,
            );
        }
        let ready = harness.settle();
        let left = harness.click(ready.parent_label("Leave pilot seat"));
        match requests
            .try_recv()
            .expect("Visible parent Leave pilot seat must emit its actual bound request")
        {
            arkline_protocol::Request::DockedTraversal {
                request: arkline_protocol::DockedTraversalRequest::LeaveSeat { binding: actual },
            } => assert_eq!(actual, binding),
            other => panic!("Unexpected seat request {other:?}"),
        }
        assert!(requests.try_recv().is_err());
        captures.push(capture(
            directory.as_deref(),
            "visible-bound-leave-seat",
            &left,
        ));
        // The isolated channel has no authority worker. Install a compatible
        // received state below; first leave a real physical gesture pending.
        harness.cockpit.request_pending = false;
        let seated_before_transition = harness.settle();
        let stale_button = seated_before_transition.physical_label("UNDOCK");
        let pending_press = harness.frame(pointer(stale_button, true), egui::Modifiers::NONE, true);
        assert!(requests.try_recv().is_err());
        captures.push(capture(
            directory.as_deref(),
            "computer-press-before-onfoot",
            &pending_press,
        ));
        let pose = arkline_protocol::docked_binding_pose(&binding).unwrap();
        let seat = arkline_protocol::seat_anchors(pose);
        let now = 1000 + (harness.time * 1000.0) as u64;
        let state = DockedTraversalState {
            sprinting: false,
            version: DOCKED_TRAVERSAL_VERSION,
            binding: binding.clone(),
            feet_m: seat.root_station_m,
            heading: seat.heading,
            support: DockedSupport::Cockpit,
            surface_id: "cockpit".into(),
            moving: false,
            grounded: true,
            vertical_velocity_mps: 0.0,
            updated_at_ms: now,
            seat_transition: None,
            actuators: DockedVesselActuators {
                hatches: None,
                binding: binding.clone(),
                inner_hatch_open: false,
                outer_hatch_open: false,
                lift: DockedLiftState {
                    floor_station_m: arkline_protocol::lift_geometry(pose).ship_floor_m,
                    target: DockedLiftStop::Ship,
                    moving: false,
                    updated_at_ms: now,
                },
                service_power_available: true,
            },
        };
        harness.cockpit.snapshot.as_mut().unwrap().pilots[0].station_avatar = Some(StationAvatar {
            sprinting: false,
            docked_traversal: Some(state),
            onboard_interior: false,
            interior_deck: 1,
            jump_started_ms: None,
            seated: false,
            position: arkline_protocol::hangar_layout(ShipClass::Pioneer).boarding_point,
            heading: 0.0,
            moving: false,
            updated_at_ms: now,
        });
        assert_eq!(
            harness.cockpit.verified_docked_binding().as_ref(),
            Some(&binding)
        );
        let released_after_transition =
            harness.frame(pointer(stale_button, false), egui::Modifiers::NONE, true);
        assert!(requests.try_recv().is_err(),"A physical press from the seated frame must not activate on release after the surface disappears");
        let onfoot = harness.settle();
        assert!(onfoot.depth_pixels > 1000);
        assert!(onfoot.surface.is_none(),"This upright first-person fixture looks above the actual low computer; old surface must be cleared");
        assert!(onfoot.labels.is_empty(),"Cached canonical vertices must not expose active computer labels without a physical surface");
        assert!(!harness.cockpit.cockpit_computer.wants_keyboard_input());
        assert!(harness
            .cockpit
            .hangar_view
            .computer_hit_tester(&harness.ctx)(stale_button)
        .is_none());
        assert!(!harness
            .ctx
            .memory(|memory| memory.has_focus(egui::Id::new(PARENT_TEXT))));
        assert_eq!(
            harness.cockpit.hangar_view.on_foot_view,
            hangar::OnFootView::FirstPerson
        );
        captures.push(capture(
            directory.as_deref(),
            "stale-release-after-onfoot",
            &released_after_transition,
        ));
        captures.push(capture(
            directory.as_deref(),
            "composed-onfoot-cockpit",
            &onfoot,
        ));
        harness.click(onfoot.parent_label("Third person"));
        assert_eq!(
            harness.cockpit.hangar_view.on_foot_view,
            hangar::OnFootView::ThirdPerson
        );
        let third_person = harness.settle();
        assert_eq!(
            harness.cockpit.hangar_view.on_foot_view,
            hangar::OnFootView::ThirdPerson,
            "Actual chosen third-person view must survive subsequent composed Cockpit updates"
        );
        captures.push(capture(
            directory.as_deref(),
            "composed-onfoot-third-person",
            &third_person,
        ));
        harness.click(third_person.parent_label("First person"));
        harness.settle();
        assert_eq!(
            harness.cockpit.hangar_view.on_foot_view,
            hangar::OnFootView::FirstPerson,
            "Actual chosen first-person view must also persist"
        );
        assert!(
            requests.try_recv().is_err(),
            "Typing, ownership and presentation changes must not issue unrelated commands"
        );
        if let Some(directory) = &directory {
            std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"schema_version":2,"scope":"Actual Cockpit::update + negotiated HangarView + render_cockpit_computer secondary context + physical GPU display/depth/layer input in private macOS CGL; no desktop window","source_glb_sha256":arkline_protocol::PIONEER_DOCKED_SOURCE_SHA256,"registration_revision":binding.registration_revision,"layout_sha256":binding.layout_sha256,"host_vessel_id":binding.host_vessel_id,"viewport_px":[WIDTH,HEIGHT],"center_computer_lit_samples":lit,"center_computer_canonical_vertices":baseline.computer_vertices,"physical_button_request":"Undock","physical_hover_and_press_send_nothing":true,"physical_release_sends_exactly_one_actual_ui_request":true,"visible_parent_leave_seat_sends_exact_bound_request":true,"seated_press_onfoot_release_sends_nothing":true,"missing_surface_clears_active_labels_keyboard_and_hit_guard":true,"onfoot_cached_canonical_vertices":onfoot.computer_vertices,"actual_first_third_person_selection_persists":true,"computer_typing":"Native cockpit text","secondary_text_owns_headlook":true,"parent_text_owns_keyboard_and_headlook":true,"window_focus_loss_cancels_computer_input":true,"positive_owned_headlook":true,"received_snapshot_unchanged_by_input":true,"captures":captures,"vendor":gl.get_parameter_string(glow::VENDOR),"renderer":gl.get_parameter_string(glow::RENDERER),"gl_version":gl.get_parameter_string(glow::VERSION),"limitations":["Undock and the bound LeaveSeat request were captured in an isolated in-memory command channel and were never executed","On-foot state is a deterministic compatible snapshot fixture; full authority walking/interlock journeys remain separate","Native offscreen composition proves neither OS focus/input delivery nor a packaged app launch","No network, local authority, audio device, project or canonical source asset was opened or modified"]})).unwrap()).unwrap();
        }
        drop(harness);
        assert_eq!(gl.get_error(), glow::NO_ERROR);
    }
}
