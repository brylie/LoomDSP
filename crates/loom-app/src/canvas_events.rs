//! What a canvas event does to the patch: the canvas reports
//! `CanvasEvent`s and never edits the graph itself (ADR-0010).

use crate::{App, browser, gestures, probe_menu, scene, undo};
use iced::Point;
use loom_canvas::CanvasEvent;
use loom_core::{InputRef, ModuleId, OutputRef};

impl App {
    pub(crate) fn canvas_event(&mut self, event: CanvasEvent) {
        if event == CanvasEvent::PointerDown {
            self.matrix.focused = false;
            self.matrix.edit = None;
            self.matrix.add = None;
            return;
        }
        let is_unknown = |id: u32| {
            self.controller
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
            self.alerts.say("Cannot connect to an unknown module");
            self.gestures.cancel_drag();
            return;
        }
        let edits = undo::is_edit(&self.controller, &event);
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
                self.gestures.drag(gestures::AssignDrag { from, start, at, zoom });
            }
            CanvasEvent::AssignDrop { from, at } => {
                let landed = at.is_some() && !self.learn.is_on();
                if let Some((module, index)) = self.gestures.finish_drag(from, landed) {
                    let source = OutputRef {
                        module: ModuleId(from.module),
                        port: from.index,
                    };
                    self.assign(source, ModuleId(module), index);
                }
            }
            CanvasEvent::AssignCancel => self.gestures.cancel_drag(),
            CanvasEvent::GestureStart => {
                self.close_slider_gesture();
                self.edits.start_gesture();
                self.begin_edit();
            }
            CanvasEvent::GestureEnd => {
                if self.gestures.end_depth() {
                    self.refresh_values();
                }
                self.end_gesture();
            }
            CanvasEvent::ArrowGroupPending => self.arrow_group_pending = true,
            CanvasEvent::ArrowGroupResumed => {
                self.arrow_group_pending = false;
                if self.arrow_group_closed {
                    self.close_slider_gesture();
                    self.edits.start_gesture();
                    self.begin_edit();
                    self.arrow_group_closed = false;
                }
            }
            CanvasEvent::ArrowGroupEnd => {
                self.arrow_group_pending = false;
                if !self.arrow_group_closed {
                    self.end_gesture();
                }
                self.arrow_group_closed = false;
            }
            CanvasEvent::Select(ids) => self.selection = ids.into_iter().collect(),
            CanvasEvent::Move { ids, delta } => {
                for id in &ids {
                    if let Some(p) = self.positions.get_mut(id) {
                        *p += delta;
                    }
                }
                self.scene.move_modules(&ids, delta);
            }
            CanvasEvent::Resize { module, size } => {
                if let Some(tile) = self.scene.module(module).filter(|tile| tile.is_growable()) {
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
                            self.sizes.remove(&module);
                        } else {
                            self.sizes.insert(module, size);
                        }
                        self.refresh();
                    }
                }
            }
            CanvasEvent::Connect { from, to } => {
                let output = OutputRef {
                    module: ModuleId(from.module),
                    port: from.index,
                };
                let input = InputRef {
                    module: ModuleId(to.module),
                    port: to.index,
                };
                match self.controller.connect(output, input) {
                    Ok(_) => {
                        self.commit();
                    }
                    Err(e) => self.alerts.say(format!("error: {e}")),
                }
            }
            CanvasEvent::Assign { from, module, widget } => {
                if let Some((index, _, _)) = scene::control_param(&self.controller, module, widget) {
                    let source = OutputRef {
                        module: ModuleId(from.module),
                        port: from.index,
                    };
                    self.assign(source, ModuleId(module), index);
                }
            }
            CanvasEvent::DragDepth {
                module,
                widget,
                pinned,
                delta,
            } => self.drag_depth(module, widget, pinned, delta),
            CanvasEvent::Disconnect(port) => {
                let input = InputRef {
                    module: ModuleId(port.module),
                    port: port.index,
                };
                if self.controller.disconnect(input).is_ok() {
                    self.commit();
                }
            }
            CanvasEvent::SetControl {
                module,
                widget,
                position,
            } => {
                if let Some((index, kind, spec)) = scene::control_param(&self.controller, module, widget) {
                    let value = scene::control_value(kind, &spec, position);
                    self.set_param(module, index, value);
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
            CanvasEvent::ResetControl { module, widget } => {
                if let Some((index, _, spec)) = scene::control_param(&self.controller, module, widget) {
                    self.set_param(module, index, spec.default);
                }
            }
            CanvasEvent::Learn { module, widget } => {
                if let Some((index, _, _)) = scene::control_param(&self.controller, module, widget) {
                    self.arm((module, index));
                }
            }
            CanvasEvent::ProbeMenu {
                output,
                world,
                at,
                area,
            } => {
                self.close_slider_gesture();
                self.probe_menu = Some(probe_menu::ProbeMenu::open(output, world, at, area));
            }
            CanvasEvent::TileMenu { module, at, area } => {
                self.tile_menu = Some((
                    module,
                    Point::new(
                        at.x.min(area.x + area.width - 140.0).max(area.x),
                        at.y.min(area.y + area.height - 36.0).max(area.y),
                    ),
                ));
            }
            CanvasEvent::OpenBrowser { world, at, area } => {
                self.close_slider_gesture();
                self.browser = Some(browser::Browser::open(self.modules.clone(), world, at, area));
            }
            CanvasEvent::VisibleDisplays(ids) => {
                self.visible_displays = ids;
                self.sync_probes();
                self.refresh_values();
            }
            CanvasEvent::TraceDetail(full) => {
                self.full_traces = full;
                self.publish_watch();
            }
            CanvasEvent::ToggleRun => self.toggle_run(),
            CanvasEvent::Delete(ids) => {
                self.delete_modules(&ids);
            }
            // The app reads the clipboard first, in `update`.
            CanvasEvent::Paste { .. } => {}
        }
    }
}
