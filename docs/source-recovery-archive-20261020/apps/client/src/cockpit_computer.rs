//! Frozen prototype: fixed-canvas computer input and main-painter texture bridge.
//! Install as a child of `flight3d` to use the existing CockpitSurface visibility.
//! This module produces canvas-space triangles. It never projects or paints them.
use super::CockpitSurface;
use eframe::egui::{self, epaint, Context, Event, Key, Modifiers, PointerButton, Pos2, Rect};
use std::collections::HashMap;

pub(crate) const COMPUTER_CANVAS_SIZE: egui::Vec2 = egui::vec2(840.0, 600.0);
const OUTSIDE: Pos2 = egui::pos2(-10_000.0, -10_000.0);

#[derive(Clone, Copy)]
pub(crate) struct ComputerPermission {
    /// False when the computer is inactive, including non-cockpit/free-camera modes.
    pub active: bool,
    /// Caller checks parent layers and editor dialogs. The per-event hit callback
    /// additionally rejects nearer world occluders for that exact coordinate.
    pub pointer: bool,
    /// False while another editor text field, modal, or command console owns input.
    pub keyboard: bool,
}

#[derive(Clone, Default)]
pub(crate) struct ComputerOutput {
    /// Positions and clip rectangles remain in COMPUTER_CANVAS_SIZE coordinates.
    /// Texture IDs already refer to the parent's existing painter namespace.
    pub primitives: Vec<egui::ClippedPrimitive>,
    pub wants_keyboard_input: bool,
    pub pointer_on_computer: bool,
    /// IME rectangles remain canonical; integration must project them before
    /// handing platform output to the native backend. No native window is opened.
    pub platform_output: egui::PlatformOutput,
    pub diagnostics: Vec<String>,
}

#[derive(Default)]
struct InputBridge {
    keyboard_owned: bool,
    buttons: Vec<PointerButton>,
    keys: Vec<Key>,
    touches: Vec<(egui::TouchDeviceId, egui::TouchId)>,
    pending: Vec<Event>,
    window_pointer: Option<Pos2>,
}

impl InputBridge {
    fn release_all(&mut self, events: &mut Vec<Event>) {
        // Never release at the last in-bounds coordinate: that could activate
        // the original button after its glass plane moved or became occluded.
        events.push(Event::PointerMoved(OUTSIDE));
        for button in self.buttons.drain(..) {
            events.push(Event::PointerButton {
                pos: OUTSIDE,
                button,
                pressed: false,
                modifiers: Modifiers::NONE,
            });
        }
        for (device_id, id) in self.touches.drain(..) {
            events.push(Event::Touch {
                device_id,
                id,
                phase: egui::TouchPhase::Cancel,
                pos: OUTSIDE,
                force: None,
            });
        }
        for key in self.keys.drain(..) {
            events.push(Event::Key {
                key,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE,
            });
        }
        events.push(Event::Ime(egui::ImeEvent::Disabled));
        events.push(Event::PointerGone);
        self.keyboard_owned = false;
    }

    fn map(
        &mut self,
        source: &egui::RawInput,
        permission: ComputerPermission,
        mut hit: impl FnMut(Pos2) -> Option<Pos2>,
        pointer: Option<Pos2>,
    ) -> (egui::RawInput, bool) {
        let mut events = std::mem::take(&mut self.pending);
        let mut available = permission.active && permission.keyboard && source.focused;
        let mut pointer_available = permission.active && permission.pointer && source.focused;
        if !available || !pointer_available {
            self.release_all(&mut events);
        }
        let map = |point, allowed: bool, hit: &mut dyn FnMut(Pos2) -> Option<Pos2>| {
            allowed.then(|| hit(point)).flatten()
        };
        let has_pointer_events = source.events.iter().any(|event| {
            matches!(
                event,
                Event::PointerMoved(_)
                    | Event::PointerButton { .. }
                    | Event::PointerGone
                    | Event::Touch { .. }
            )
        });
        // Parent hover_pos is the FINAL position in this batch. Replay pointer
        // events from the prior position so an inside release followed by an
        // outside move (or touch PointerGone) still completes its valid click.
        let initial_pointer = if has_pointer_events {
            self.window_pointer
        } else {
            pointer
        };
        let mut pointer_hit =
            initial_pointer.and_then(|point| map(point, pointer_available, &mut hit));
        if !has_pointer_events
            && pointer_hit.is_none()
            && (!self.buttons.is_empty() || !self.touches.is_empty())
        {
            self.release_all(&mut events);
        }
        for event in &source.events {
            match event {
                Event::WindowFocused(focused) => {
                    available = permission.active && permission.keyboard && *focused;
                    pointer_available = permission.active && permission.pointer && *focused;
                    if !*focused {
                        self.release_all(&mut events);
                        pointer_hit = None;
                    }
                    events.push(Event::WindowFocused(*focused));
                }
                Event::PointerMoved(point) => {
                    pointer_hit = map(*point, pointer_available, &mut hit);
                    if pointer_hit.is_none()
                        && (!self.buttons.is_empty() || !self.touches.is_empty())
                    {
                        self.release_all(&mut events);
                    }
                    events.push(Event::PointerMoved(pointer_hit.unwrap_or(OUTSIDE)));
                }
                Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } => {
                    let point = map(*pos, pointer_available, &mut hit);
                    pointer_hit = point;
                    if *pressed {
                        if let Some(pos) = point.filter(|_| available) {
                            self.keyboard_owned = true;
                            if !self.buttons.contains(button) {
                                self.buttons.push(*button);
                            }
                            events.push(Event::PointerMoved(pos));
                            events.push(Event::PointerButton {
                                pos,
                                button: *button,
                                pressed: true,
                                modifiers: *modifiers,
                            });
                        } else {
                            self.release_all(&mut events);
                        }
                    } else if self.buttons.contains(button) && point.is_none() {
                        self.release_all(&mut events);
                    } else if self.buttons.contains(button) {
                        self.buttons.retain(|held| held != button);
                        // Move first as well as setting the release coordinate: egui
                        // must not use its old hover location for click completion.
                        let pos = point.expect("captured release has an accepted ray hit");
                        events.push(Event::PointerMoved(pos));
                        events.push(Event::PointerButton {
                            pos,
                            button: *button,
                            pressed: false,
                            modifiers: *modifiers,
                        });
                    }
                }
                Event::Touch {
                    device_id,
                    id,
                    phase,
                    pos,
                    force,
                } => {
                    let point = map(*pos, pointer_available, &mut hit);
                    pointer_hit = point;
                    let identity = (*device_id, *id);
                    let captured = self.touches.contains(&identity);
                    if captured && (point.is_none() || *phase == egui::TouchPhase::Cancel) {
                        self.release_all(&mut events);
                        continue;
                    }
                    match (*phase, point) {
                        (egui::TouchPhase::Start, Some(pos)) if available => {
                            self.keyboard_owned = true;
                            if !captured {
                                self.touches.push(identity);
                            }
                            events.push(Event::Touch {
                                device_id: *device_id,
                                id: *id,
                                phase: *phase,
                                pos,
                                force: *force,
                            });
                        }
                        (egui::TouchPhase::Move, Some(pos)) if captured => {
                            events.push(Event::Touch {
                                device_id: *device_id,
                                id: *id,
                                phase: *phase,
                                pos,
                                force: *force,
                            });
                        }
                        _ if captured => {
                            self.touches.retain(|touch| *touch != identity);
                            events.push(Event::Touch {
                                device_id: *device_id,
                                id: *id,
                                phase: if point.is_some() && *phase == egui::TouchPhase::End {
                                    egui::TouchPhase::End
                                } else {
                                    egui::TouchPhase::Cancel
                                },
                                pos: point.unwrap_or(OUTSIDE),
                                force: *force,
                            });
                        }
                        (egui::TouchPhase::Start, _) => self.release_all(&mut events),
                        _ => {}
                    }
                }
                Event::PointerGone => {
                    pointer_hit = None;
                    if !self.buttons.is_empty() || !self.touches.is_empty() {
                        self.release_all(&mut events);
                    }
                    events.push(Event::PointerMoved(OUTSIDE));
                    events.push(Event::PointerGone);
                }
                Event::MouseWheel { .. } if pointer_hit.is_some() => events.push(event.clone()),
                Event::Key {
                    key,
                    pressed: false,
                    ..
                } if self.keys.contains(key) => {
                    self.keys.retain(|held| held != key);
                    events.push(event.clone());
                }
                Event::Key {
                    key, pressed: true, ..
                } if available && self.keyboard_owned => {
                    if !self.keys.contains(key) {
                        self.keys.push(*key);
                    }
                    events.push(event.clone());
                }
                Event::Copy | Event::Cut | Event::Paste(_) | Event::Text(_) | Event::Ime(_)
                    if available && self.keyboard_owned =>
                {
                    events.push(event.clone())
                }
                // Raw relative motion, pinch zoom, screenshots, file drops and
                // accessibility actions are not blindly injected into this UI.
                _ => {}
            }
        }
        // Hover follows the current ray even if the camera moved without a
        // PointerMoved event. A missing surface also cancels captured gestures.
        self.window_pointer = pointer;
        pointer_hit = pointer.and_then(|point| map(point, pointer_available, &mut hit));
        events.push(Event::PointerMoved(pointer_hit.unwrap_or(OUTSIDE)));
        if !pointer_available
            || (pointer_hit.is_none() && (!self.buttons.is_empty() || !self.touches.is_empty()))
        {
            self.release_all(&mut events);
        }
        if pointer_hit.is_none() {
            events.push(Event::PointerGone);
        }
        let focused = available && self.keyboard_owned;
        let canvas = Rect::from_min_size(Pos2::ZERO, COMPUTER_CANVAS_SIZE);
        let mut raw = egui::RawInput {
            screen_rect: Some(canvas),
            max_texture_side: source.max_texture_side,
            time: source.time,
            predicted_dt: source.predicted_dt,
            modifiers: if focused {
                source.modifiers
            } else {
                Modifiers::NONE
            },
            events,
            focused,
            system_theme: source.system_theme,
            ..Default::default()
        };
        let viewport = raw.viewports.entry(raw.viewport_id).or_default();
        viewport.inner_rect = Some(canvas);
        viewport.outer_rect = Some(canvas);
        viewport.native_pixels_per_point = Some(1.0);
        viewport.focused = Some(focused);
        (raw, pointer_hit.is_some())
    }
}

pub(crate) struct CockpitComputer {
    context: Context,
    input: InputBridge,
    /// Secondary managed ID -> parent managed ID. Managed(0) is never assumed shared.
    textures: HashMap<egui::TextureId, egui::TextureId>,
    parent: Option<Context>,
    last_frame: Option<(egui::ViewportId, u64)>,
    cached: ComputerOutput,
    #[cfg(test)]
    pub(crate) last_shapes: Vec<egui::epaint::ClippedShape>,
}

impl Default for CockpitComputer {
    fn default() -> Self {
        let context = Context::default();
        // UI side effects execute once. Layout changes settle on the next frame.
        context.options_mut(|options| {
            options.max_passes = std::num::NonZeroUsize::MIN;
            options.zoom_with_keyboard = false;
        });
        context.set_pixels_per_point(1.0);
        Self {
            context,
            input: InputBridge::default(),
            textures: HashMap::new(),
            parent: None,
            last_frame: None,
            cached: ComputerOutput::default(),
            #[cfg(test)]
            last_shapes: Vec::new(),
        }
    }
}

impl CockpitComputer {
    #[cfg(test)]
    pub(crate) fn debug_primitives(&self) -> &[egui::ClippedPrimitive] {
        &self.cached.primitives
    }

    /// Use for early shortcut/head-look gates before this frame's screen layout.
    /// The caller must also require current cockpit mode and keyboard permission.
    pub(crate) fn wants_keyboard_input(&self) -> bool {
        self.input.keyboard_owned && self.context.wants_keyboard_input()
    }

    /// Revoke immediately on a scene/permission change, including a repeated
    /// parent layout pass. Releases are delivered on the next secondary pass.
    pub(crate) fn cancel_input(&mut self) {
        let mut releases = Vec::new();
        self.input.release_all(&mut releases);
        self.input.pending.extend(releases);
        self.context.memory_mut(|memory| {
            if let Some(focused) = memory.focused() {
                memory.surrender_focus(focused);
            }
        });
        self.cached.wants_keyboard_input = false;
        self.cached.pointer_on_computer = false;
        self.cached.platform_output = egui::PlatformOutput::default();
    }

    /// Call during the parent pass, before its end_pass/texture upload. Construct
    /// surface with this exact canvas size; bounds are never used for UI layout.
    /// `hit` must return a canonical point only when this glass is the nearest
    /// accepted surface, using the same current-frame scene/mount as rendering.
    /// Keep one helper for one parent Context; reset it when that parent changes.
    pub(crate) fn run(
        &mut self,
        parent: &Context,
        surface: Option<&CockpitSurface>,
        mut permission: ComputerPermission,
        mut hit: impl FnMut(Pos2) -> Option<Pos2>,
        mut show: impl FnMut(&mut egui::Ui),
    ) -> ComputerOutput {
        let (mut source, pointer, time) = parent.input(|input| {
            let mut raw = input.raw.clone();
            raw.events = input.events.clone(); // Respect keys consumed by editor widgets.
            (raw, input.pointer.hover_pos(), input.time)
        });
        // A hidden/offscreen computer still receives release processing, but
        // cannot silently keep a text editor swallowing the player's input.
        permission.keyboard &= surface.is_some();
        permission.pointer &= surface.is_some();
        // eframe supplies an explicit monotonic time. Repeated parent layout
        // passes for that input may reuse cached triangles, never UI commands.
        let frame = (parent.viewport_id(), time.to_bits());
        if self.last_frame == Some(frame) {
            let pointer_on_computer = permission.active
                && permission.pointer
                && source.focused
                && pointer
                    .and_then(&mut hit)
                    .filter(|point| {
                        point.is_finite()
                            && Rect::from_min_size(Pos2::ZERO, COMPUTER_CANVAS_SIZE)
                                .contains(*point)
                    })
                    .is_some();
            if !permission.active
                || !permission.keyboard
                || !permission.pointer
                || !source.focused
                || (!pointer_on_computer
                    && (!self.input.buttons.is_empty() || !self.input.touches.is_empty()))
            {
                self.cancel_input();
            }
            self.cached.wants_keyboard_input = self.wants_keyboard_input();
            self.cached.pointer_on_computer = pointer_on_computer;
            return self.cached.clone();
        }
        self.last_frame = Some(frame);
        source.time = Some(time);
        self.parent.get_or_insert_with(|| parent.clone());
        self.context.set_os(parent.os());
        self.context.set_theme(parent.theme());
        self.context.set_style(parent.style());
        // set_fonts compares definitions and rebuilds only when they change.
        // Atlas growth/compaction remains wholly owned by this secondary context.
        let definitions = parent.fonts(|fonts| fonts.lock().fonts.definitions().clone());
        self.context.set_fonts(definitions);
        let (raw, pointer_on_computer) = self.input.map(
            &source,
            permission,
            |point| {
                hit(point).filter(|point| {
                    point.is_finite()
                        && Rect::from_min_size(Pos2::ZERO, COMPUTER_CANVAS_SIZE).contains(*point)
                })
            },
            pointer,
        );
        if !self.input.keyboard_owned {
            self.context.memory_mut(|memory| {
                if let Some(focused) = memory.focused() {
                    memory.surrender_focus(focused);
                }
            });
        }
        let output = self.context.run(raw, |context| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(context, |ui| {
                    ui.set_min_size(COMPUTER_CANVAS_SIZE);
                    ui.set_max_size(COMPUTER_CANVAS_SIZE);
                    show(ui);
                });
        });
        if permission.active {
            if let Some(viewport) = output.viewport_output.get(&egui::ViewportId::ROOT) {
                if viewport.repaint_delay != std::time::Duration::MAX {
                    parent.request_repaint_after(viewport.repaint_delay);
                }
            }
        }
        let mut diagnostics = Vec::new();
        let manager = parent.tex_manager();
        {
            let mut manager = manager.write();
            for (source_id, delta) in output.textures_delta.set {
                if let Some(parent_id) = self.textures.get(&source_id).copied() {
                    manager.set(parent_id, delta);
                } else if delta.pos.is_none() {
                    let parent_id = manager.alloc(
                        format!("cockpit-computer-{source_id:?}"),
                        delta.image,
                        delta.options,
                    );
                    self.textures.insert(source_id, parent_id);
                } else {
                    diagnostics.push(format!(
                        "Unmapped partial computer texture update: {source_id:?}"
                    ));
                }
            }
        }
        #[cfg(test)]
        {
            self.last_shapes = output.shapes.clone();
        }
        let mut primitives = self
            .context
            .tessellate(output.shapes, output.pixels_per_point);
        primitives.retain_mut(|primitive| {
            let epaint::Primitive::Mesh(mesh) = &mut primitive.primitive else {
                diagnostics.push(
                    "Computer paint callbacks require a separate explicit world-space adapter."
                        .into(),
                );
                return false;
            };
            if let Some(parent_id) = self.textures.get(&mesh.texture_id) {
                mesh.texture_id = *parent_id;
                true
            } else if matches!(mesh.texture_id, egui::TextureId::User(_)) {
                true // Explicit native user textures already registered with the main painter.
            } else {
                diagnostics.push(format!(
                    "Unmapped computer mesh texture: {:?}",
                    mesh.texture_id
                ));
                false
            }
        });
        // Parent TextureManager queues these frees in its FullOutput; standard
        // eframe handling frees them AFTER painting the current triangles.
        {
            let mut manager = manager.write();
            for source_id in output.textures_delta.free {
                if let Some(parent_id) = self.textures.remove(&source_id) {
                    manager.free(parent_id);
                }
            }
        }
        let wants_keyboard_input = self.wants_keyboard_input();
        let platform_output = output.platform_output;
        if wants_keyboard_input || pointer_on_computer {
            parent.output_mut(|parent_output| {
                parent_output
                    .commands
                    .extend(platform_output.commands.iter().cloned());
                if pointer_on_computer {
                    parent_output.cursor_icon = platform_output.cursor_icon;
                    parent_output.mutable_text_under_cursor =
                        platform_output.mutable_text_under_cursor;
                }
            });
        }
        self.cached = ComputerOutput {
            primitives,
            wants_keyboard_input,
            pointer_on_computer,
            platform_output,
            diagnostics,
        };
        self.cached.clone()
    }

    /// Call only after the last frame using these triangles has been painted.
    pub(crate) fn clear(&mut self) {
        if let Some(parent) = &self.parent {
            let manager = parent.tex_manager();
            let mut manager = manager.write();
            for (_, parent_id) in self.textures.drain() {
                manager.free(parent_id);
            }
        }
        self.cached = ComputerOutput::default();
        self.last_frame = None;
        self.input = InputBridge::default();
        self.context = Context::default();
        self.context.options_mut(|options| {
            options.max_passes = std::num::NonZeroUsize::MIN;
            options.zoom_with_keyboard = false;
        });
        self.context.set_pixels_per_point(1.0);
        self.parent = None;
    }
}

impl Drop for CockpitComputer {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
#[path = "cockpit_computer_tests.rs"]
mod tests;
