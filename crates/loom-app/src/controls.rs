//! What controls on tiles and in the inspector do: turning a parameter,
//! ranges, buttons, modulation assignments and depth drags, and arming a
//! control for MIDI learn (`docs/design/midi-learn.md`,
//! `docs/design/modulation.md`).

use crate::{App, learn, modulation, scene};
use loom_core::{ModuleId, OutputRef};

impl App {
    /// Rebuilds what learn mode shows, after mappings or the armed
    /// parameter changed. A removed module disarms its parameter.
    pub(crate) fn refresh_learn(&mut self) {
        self.learn.refresh(&self.controller);
    }

    /// Turns learn mode on or off, or arms a parameter.
    pub(crate) fn learn_message(&mut self, message: learn::Message) {
        match message {
            learn::Message::Toggle if self.learn.is_on() => self.stop_learning(),
            learn::Message::Toggle => {
                self.learn.start();
                self.alerts.say("MIDI learn: click a control to map it".to_string());
                self.refresh_learn();
            }
            learn::Message::Arm(param) => {
                if self.learn.is_on() {
                    self.arm(param);
                }
            }
        }
    }

    /// Arms a parameter in learn mode, or disarms it if it is armed.
    pub(crate) fn arm(&mut self, param: learn::Param) {
        let (module, index) = param;
        let transient = self
            .controller
            .graph()
            .node(ModuleId(module))
            .and_then(|node| node.descriptor.params.get(index))
            .is_some_and(|spec| spec.is_transient());
        if transient {
            // A live gesture cannot be mapped (transient-params.md).
            self.alerts.say(format!(
                "MIDI learn: {} is a live control and cannot be mapped",
                learn::param_name(&self.controller, param)
            ));
            return;
        }
        if self.learn.is_armed(param) {
            self.learn.disarm();
            self.alerts.say("MIDI learn: click a control to map it".to_string());
        } else {
            let serial = self.controller.last_control_change().map(|seen| seen.serial);
            self.learn.arm(param, serial);
            self.alerts.say(format!(
                "Move a MIDI control to map it to {}",
                learn::param_name(&self.controller, param)
            ));
        }
        self.refresh_learn();
    }

    /// Follows control changes: mapped parameters on screen, and the armed
    /// parameter in learn mode.
    pub(crate) fn midi_arrived(&mut self) {
        if !self.controller.sync_midi().is_empty() {
            self.refresh_values();
        }
        if self.panic_midi_arrived() {
            return;
        }
        let Some(armed) = self.learn.armed() else { return };
        let Some(seen) = self.controller.last_control_change() else {
            return;
        };
        if Some(seen.serial) == armed.serial {
            return;
        }
        let (module, index) = armed.param;
        self.begin_edit();
        self.edits.touch(module, index);
        let learned = self.controller.learn_midi(seen, ModuleId(module), index);
        match learned {
            Ok((id, removed)) => {
                let mapping = self
                    .controller
                    .graph()
                    .midi_mappings()
                    .find(|(i, _)| *i == id)
                    .map(|(_, m)| *m);
                self.learn.disarm();
                self.commit();
                if let Some(mapping) = mapping {
                    self.alerts
                        .say(learn::mapped_status(&self.controller, mapping, &removed));
                }
            }
            Err(e) => {
                self.learn.disarm();
                self.alerts.say(format!("error: {e}"));
                self.refresh_learn();
            }
        }
        if !self.edits.gesture_open() {
            self.record();
        }
    }

    pub(crate) fn stop_learning(&mut self) {
        self.learn.stop();
        self.alerts.say("MIDI learn off".to_string());
        self.refresh_learn();
    }

    /// Sets a parameter as a knob turn does. A slider sends no start, so its
    /// first change opens the gesture and letting go closes it.
    pub(crate) fn turn_param(&mut self, module: u32, index: usize, value: f32) {
        self.close_pending_arrow_group();
        self.edits.start_slider_gesture();
        self.begin_edit();
        self.set_param(module, index, value);
    }

    /// Selects a parameter's control range, as one undo step. A smaller
    /// range clamps the value into it, a wider one keeps it (control-ranges.md,
    /// rule 6). The commit puts the bounds into the schedule for modulation
    /// and MIDI (rule 8).
    pub(crate) fn set_range(&mut self, module: u32, index: usize, range: usize) {
        let Some(range) = self
            .controller
            .graph()
            .node(ModuleId(module))
            .and_then(|n| n.descriptor.params.get(index))
            .and_then(|spec| spec.options.ranges.get(range))
            .copied()
        else {
            return;
        };
        self.close_slider_gesture();
        self.edit(|app| {
            if let Err(e) = app.controller.set_range(ModuleId(module), index, range) {
                app.alerts.say(format!("error: {e}"));
                return;
            }
            app.edits.touch(module, index);
            app.commit();
        });
        self.refresh_values();
    }

    /// Sets a parameter from a control on a tile or in the inspector.
    pub(crate) fn set_param(&mut self, module: u32, index: usize, value: f32) {
        if let Err(e) = self.controller.set_param(ModuleId(module), index, value) {
            self.alerts.say(format!("error: {e}"));
        }
        self.edits.touch(module, index);
        self.sync_audio_input();
        self.refresh_values();
    }

    /// Applies a button gesture. The canvas names the gesture. [`loom_core::ButtonMode`]
    /// names the value, from the mode parameter stored on the module. Toggle
    /// writes the latch and Momentary the transient hold.
    pub(crate) fn apply_button(
        &mut self,
        module: u32,
        widget: usize,
        change: impl Fn(loom_core::ButtonMode, f32) -> Option<f32>,
    ) {
        let edit = self.controller.graph().node(ModuleId(module)).and_then(|node| {
            let panel_widget = node.descriptor.panel.widgets.get(widget)?;
            let mode = node.descriptor.button_mode(panel_widget, &node.params)?;
            let target = node.descriptor.control_param(panel_widget, &node.params)?;
            let next = change(mode, node.params[target])?;
            Some((target, next))
        });
        if let Some((index, value)) = edit {
            self.set_param(module, index, value);
        }
    }

    /// Lets go of a button's transient hold, whatever the mode is now. The
    /// mode can change during a momentary press (inspector, undo, or a MIDI
    /// mapping), and the release then resolves to the latch. Without this
    /// the hold would stay high and come back with Momentary mode.
    pub(crate) fn release_hold(&mut self, module: u32, widget: usize) {
        let held = self.controller.graph().node(ModuleId(module)).and_then(|node| {
            let loom_core::WidgetKind::Button { hold, .. } = node.descriptor.panel.widgets.get(widget)?.kind else {
                return None;
            };
            let index = node.descriptor.param_index(hold)?;
            let default = node.descriptor.params[index].default;
            (node.params[index] != default).then_some((index, default))
        });
        if let Some((index, default)) = held {
            self.set_param(module, index, default);
        }
    }

    /// Assigns an output to a parameter with the default depth, after the
    /// others on that parameter. A drop on a tile control and **Add
    /// modulation** both come here. An output that already drives the
    /// parameter adds nothing (`docs/design/modulation.md`, Making an
    /// assignment).
    pub(crate) fn assign(&mut self, source: OutputRef, target: ModuleId, index: usize) {
        let graph = self.controller.graph();
        let Some(spec) = graph.node(target).and_then(|n| n.spec(index)) else {
            return;
        };
        let (from, to) = (
            modulation::source_name(graph, source),
            modulation::target_name(graph, target, index),
        );
        if modulation::exists(graph, source, target, index) {
            self.alerts.say(format!("{from} already modulates {to}"));
            return;
        }
        let depth = modulation::default_depth(&spec);
        match self.controller.add_assignment(source, target, index, depth) {
            Ok(_) => {
                if self.commit() {
                    self.alerts
                        .say(format!("{from} modulates {to}, {}", modulation::depth_text(depth)));
                }
            }
            Err(e) => self.alerts.say(format!("error: {e}")),
        }
    }

    /// One step of an arc drag. The first picks the assignment, by the
    /// spec's order; the rest of the drag keeps it.
    pub(crate) fn drag_depth(&mut self, module: u32, widget: usize, pinned: Option<loom_canvas::PortRef>, delta: f32) {
        let Some((param, _, _)) = scene::control_param(&self.controller, module, widget) else {
            return;
        };
        let graph = self.controller.graph();
        let picked = self.gestures.pick_depth(|| {
            let pinned = pinned.map(|p| OutputRef {
                module: ModuleId(p.module),
                port: p.index,
            });
            let selected = (self.selection.len() == 1)
                .then(|| self.selection.iter().next().map(|&id| ModuleId(id)))
                .flatten();
            match modulation::drag_target(graph, ModuleId(module), param, pinned, selected) {
                Ok(id) => Some(id),
                Err(why) => {
                    self.alerts.say(why.to_string());
                    None
                }
            }
        });
        let Some(id) = picked else { return };
        let Some(a) = self.controller.graph().assignment(id).copied() else {
            return;
        };
        match self.controller.set_assignment_depth(id, a.depth + delta) {
            Ok(depth) => {
                self.alerts.say(format!(
                    "{}: depth {}",
                    modulation::source_name(self.controller.graph(), a.source),
                    modulation::depth_text(depth)
                ));
            }
            Err(e) => self.alerts.say(format!("error: {e}")),
        }
        self.refresh_values();
    }
}
