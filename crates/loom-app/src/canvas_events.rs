//! What a canvas event does to the patch: the canvas reports
//! `CanvasEvent`s and never edits the graph itself (ADR-0010).

use crate::edits::ArrowEvent;
use crate::overlays::{ContextMenu, Overlay};
use crate::{App, browser, gestures, probe_menu, scene, undo};
use loom_canvas::{CanvasEvent, ControlPart};
use loom_core::{InputRef, ModuleId, OutputRef};
use loom_ui_core::Point;

impl App {
    pub(crate) fn canvas_event(&mut self, event: CanvasEvent) {
        if event == CanvasEvent::PointerDown {
            self.session.matrix.focused = false;
            self.session.matrix.edit = None;
            self.session.matrix.add = None;
            return;
        }
        let is_unknown = |id: u32| {
            self.session
                .controller
                .graph()
                .node(ModuleId(id))
                .is_some_and(|node| node.unknown.is_some())
        };
        let forbidden = match &event {
            CanvasEvent::Connect { from, to } => is_unknown(from.module) || is_unknown(to.module),
            CanvasEvent::Assign { from, module, .. } => is_unknown(from.module) || is_unknown(*module),
            CanvasEvent::AssignDrop { from, .. } => is_unknown(from.module),
            _ => false,
        };
        if forbidden {
            self.alerts.warn("Cannot connect to an unknown module");
            self.session.gestures.cancel_drag();
            return;
        }
        let edits = undo::is_edit(&self.session.controller, &event);
        if edits {
            self.close_slider_gesture();
            self.edit(|app| app.apply_canvas_event(event));
        } else {
            self.apply_canvas_event(event);
        }
    }

    pub(crate) fn apply_canvas_event(&mut self, event: CanvasEvent) {
        match event {
            CanvasEvent::PointerDown => {}
            CanvasEvent::AssignDrag { from, start, at, zoom } => {
                self.session
                    .gestures
                    .drag(gestures::AssignDrag { from, start, at, zoom });
            }
            CanvasEvent::AssignDrop { from, at } => {
                let landed = at.is_some() && !self.session.learn.is_on();
                if let Some((module, index)) = self.session.gestures.finish_drag(from, landed) {
                    let source = OutputRef {
                        module: ModuleId(from.module),
                        port: from.index,
                    };
                    self.assign(source, ModuleId(module), index);
                }
            }
            CanvasEvent::AssignCancel => self.session.gestures.cancel_drag(),
            CanvasEvent::GestureStart => {
                self.close_slider_gesture();
                self.session.edits.start_gesture();
                self.begin_edit();
            }
            CanvasEvent::GestureEnd => {
                if self.session.gestures.end_depth() {
                    self.refresh_values();
                }
                self.end_gesture();
            }
            CanvasEvent::ArrowStep { at } => self.arrow_event(ArrowEvent::Step(at)),
            CanvasEvent::ArrowPressed { at } => {
                // A key press ends a screen reader's group, which the
                // canvas was shown as a released arrow group.
                self.reader_event(crate::edits::ReaderEvent::Close);
                self.arrow_event(ArrowEvent::Pressed(at));
            }
            CanvasEvent::ArrowReleased { at } => self.arrow_event(ArrowEvent::Released(at)),
            CanvasEvent::ArrowGroupEnd => {
                self.reader_event(crate::edits::ReaderEvent::Close);
                self.arrow_event(ArrowEvent::End);
            }
            CanvasEvent::ArrowGroupDue { at } => {
                self.reader_event(crate::edits::ReaderEvent::Due(at));
                self.arrow_event(ArrowEvent::Due(at));
            }
            CanvasEvent::Select(ids) => self.session.selection = ids.into_iter().collect(),
            CanvasEvent::Move { ids, delta } => {
                for id in &ids {
                    if let Some(p) = self.session.positions.get_mut(id) {
                        p.0 += delta.x;
                        p.1 += delta.y;
                    }
                }
                self.session.scene.move_modules(&ids, delta);
            }
            CanvasEvent::Resize { module, size } => {
                if let Some(tile) = self.session.scene.module(module).filter(|tile| tile.is_growable()) {
                    let min = tile.minimum_size();
                    let min_w = (min.width / loom_canvas::GRID).ceil() as u8;
                    let min_h = (min.height / loom_canvas::GRID).ceil() as u8;
                    let size = (size.0.clamp(min_w, min_w.max(64)), size.1.clamp(min_h, min_h.max(64)));
                    let current = (
                        (tile.width() / loom_canvas::GRID) as u8,
                        (tile.height() / loom_canvas::GRID) as u8,
                    );
                    if size != current {
                        if size == (min_w, min_h) {
                            self.session.sizes.remove(&module);
                        } else {
                            self.session.sizes.insert(module, size);
                        }
                        self.refresh();
                    }
                }
            }
            CanvasEvent::Connect { from, to } => {
                // A layer or select level holds only its members, unwired.
                if let Some(why) = self.session.hierarchy.level_refusal(&self.session.level) {
                    self.alerts.warn(why);
                    return;
                }
                let graph = self.session.controller.graph();
                let both_modules =
                    graph.node(ModuleId(from.module)).is_some() && graph.node(ModuleId(to.module)).is_some();
                if !both_modules {
                    // A port or sub-patch tile at one end: a boundary record.
                    self.connect_through_hierarchy(from, to);
                    return;
                }
                let output = OutputRef {
                    module: ModuleId(from.module),
                    port: from.index,
                };
                let input = InputRef {
                    module: ModuleId(to.module),
                    port: to.index,
                };
                let through_port = self.link_into_shown(to);
                match self.session.controller.connect(output, input) {
                    Ok(_) => {
                        // The record that held the old cable goes with it,
                        // after the connect, so a refusal changes nothing.
                        if through_port && let Some(to_end) = self.level_end(to) {
                            let level = self.session.level.clone();
                            self.change_hierarchy("connect", |h, _| {
                                h.disconnect(&level, &to_end);
                                Ok(())
                            });
                        }
                        self.commit();
                    }
                    Err(e) => self.alerts.warn(e.to_string()),
                }
            }
            CanvasEvent::Assign {
                from,
                module,
                widget,
                part,
            } => {
                if let Some(target) = self.control_target(module, widget, part) {
                    let source = OutputRef {
                        module: ModuleId(from.module),
                        port: from.index,
                    };
                    self.assign(source, target.module, target.index);
                }
            }
            CanvasEvent::DragDepth {
                module,
                widget,
                part,
                pinned,
                delta,
            } => self.drag_depth(module, widget, part, pinned, delta),
            CanvasEvent::Disconnect(port) => {
                let input = InputRef {
                    module: ModuleId(port.module),
                    port: port.index,
                };
                if self.session.controller.graph().node(input.module).is_none() || self.link_into_shown(port) {
                    // The cable is a boundary record: on a port or sub-patch
                    // tile, or into a module from a port.
                    self.disconnect_through_hierarchy(port);
                } else if self.session.controller.disconnect(input).is_ok() {
                    self.commit();
                }
            }
            CanvasEvent::SetControl {
                module,
                widget,
                part,
                position,
            } => {
                if let Some(target) = self.control_target(module, widget, part) {
                    let value = scene::control_value(target.kind, &target.spec, position);
                    self.set_param(target.module.0, target.index, value);
                }
            }
            CanvasEvent::TypedValue { module, widget, text } => {
                if let Some(target) = self.control_target(module, widget, ControlPart::Main) {
                    // The canvas checked the text before it sent it.
                    if let Err(why) = self.apply_typed_value(target.module.0, target.index, &text) {
                        self.alerts.warn(why);
                    }
                }
            }
            CanvasEvent::ButtonPressed { module, widget } => {
                self.apply_button(module, widget, loom_core::ButtonMode::press);
            }
            CanvasEvent::ButtonReleased { module, widget } => {
                self.apply_button(module, widget, |mode, _current| mode.release());
                self.release_hold(module, widget);
            }
            CanvasEvent::ButtonArrow { module, widget, high } => {
                self.apply_button(module, widget, |mode, current| mode.arrow(current, high));
            }
            CanvasEvent::ResetControl { module, widget, part } => {
                let parts: &[ControlPart] = if part == ControlPart::Both {
                    &[ControlPart::Main, ControlPart::Second]
                } else {
                    std::slice::from_ref(&part)
                };
                for &part in parts {
                    if let Some(target) = self.control_target(module, widget, part) {
                        self.set_param(target.module.0, target.index, target.spec.default);
                    }
                }
            }
            CanvasEvent::Learn { module, widget } => {
                // One press cannot say which of two values the player means
                // (midi-learn.md, Making a mapping).
                let target = self.control_target(module, widget, ControlPart::Main);
                if target.as_ref().is_some_and(|t| t.kind.is_two_numbers()) {
                    self.alerts
                        .say("This control has two values. Arm each one in the inspector.".to_string());
                } else if let Some(target) = target {
                    self.arm((target.module.0, target.index));
                }
            }
            CanvasEvent::ProbeMenu {
                output,
                world,
                at,
                area,
            } => {
                self.close_slider_gesture();
                self.overlays
                    .open(Overlay::Menu(ContextMenu::Probe(probe_menu::ProbeMenu::open(
                        output, world, at, area,
                    ))));
            }
            CanvasEvent::TileMenu { module, at, area } => {
                self.overlays.open(Overlay::Menu(ContextMenu::Tile {
                    module,
                    at: Point::new(
                        at.x.min(area.x + area.width - 140.0).max(area.x),
                        at.y.min(area.y + area.height - 36.0).max(area.y),
                    ),
                }));
            }
            CanvasEvent::OpenBrowser { world, at, area } => {
                self.close_slider_gesture();
                self.overlays.open(Overlay::Browser(browser::Browser::open(
                    self.modules.clone(),
                    world,
                    at,
                    area,
                )));
            }
            CanvasEvent::ClearClip { module, widget } => self.reset_clip((module, widget), false),
            CanvasEvent::ResetMeter { module, widget } => self.reset_clip((module, widget), true),
            CanvasEvent::VisibleDisplays(ids) => {
                self.session.visible_displays = ids;
                self.sync_probes();
                self.refresh_values();
            }
            CanvasEvent::TraceDetail(full) => {
                self.session.full_traces = full;
                self.publish_watch();
            }
            CanvasEvent::ToggleRun => self.toggle_run(),
            CanvasEvent::Delete(ids) => {
                self.delete_modules(&ids);
            }
            // The app reads the clipboard first, in `update`.
            CanvasEvent::Paste { .. } => {}
            CanvasEvent::OpenTile(id) => self.open_tile(id),
            // Each level keeps its camera for the session.
            CanvasEvent::Camera(camera) => {
                self.session.cameras.insert(self.session.level.clone(), camera);
            }
        }
    }
}
