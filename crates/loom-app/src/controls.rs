//! What controls on tiles and in the inspector do: turning a parameter,
//! ranges, buttons, modulation assignments and depth drags, and arming a
//! control for MIDI learn (`docs/design/midi-learn.md`,
//! `docs/design/modulation.md`).

use crate::{App, Message, learn, modulation, scene};
use loom_core::{MidiMappingOptions, ModuleId, OutputRef};
use std::time::Instant;

impl App {
    /// Rebuilds what learn mode shows, after mappings or the armed
    /// parameter changed. A removed module disarms its parameter.
    pub(crate) fn refresh_learn(&mut self) {
        self.session.learn.refresh(&self.session.controller);
    }

    /// Turns learn mode on or off, or arms a parameter.
    pub(crate) fn learn_message(&mut self, message: learn::Message) {
        match message {
            learn::Message::Toggle if self.session.learn.is_on() => self.stop_learning(),
            learn::Message::Toggle => {
                self.session.learn.start();
                self.alerts.say("MIDI learn: click a control to map it".to_string());
                self.refresh_learn();
            }
            learn::Message::Arm(param) => {
                if self.session.learn.is_on() {
                    self.arm(param);
                }
            }
            learn::Message::Unmap { module, index } => {
                self.close_slider_gesture();
                self.edit(|app| match app.session.controller.unmap_midi(ModuleId(module), index) {
                    Ok(Some(mapping)) => {
                        app.commit();
                        app.alerts.say(format!(
                            "Removed {} from {}",
                            mapping.control,
                            learn::param_name(&app.session.controller, (module, index))
                        ));
                    }
                    Ok(None) => {}
                    Err(e) => app.alerts.warn(e.to_string()),
                });
            }
            learn::Message::MidiOptionsRows { module, index } => self.session.learn.toggle_options((module, index)),
            learn::Message::MidiOptions { module, index, options } => self.set_midi_options(module, index, options),
            learn::Message::MidiRangeText {
                module,
                index,
                low,
                high,
            } => self.session.learn.set_range_draft(Some(learn::RangeDraft {
                param: (module, index),
                low,
                high,
                error: None,
            })),
            learn::Message::MidiRangeSubmit { module, index } => self.submit_midi_range(module, index),
        }
    }

    /// Arms a parameter in learn mode, or disarms it if it is armed.
    pub(crate) fn arm(&mut self, param: learn::Param) {
        let (module, index) = param;
        let transient = self
            .session
            .controller
            .graph()
            .node(ModuleId(module))
            .and_then(|node| node.descriptor.params.get(index))
            .is_some_and(|spec| spec.is_transient());
        if transient {
            // A live gesture cannot be mapped (transient-params.md).
            self.alerts.warn(format!(
                "MIDI learn: {} is a live control and cannot be mapped",
                learn::param_name(&self.session.controller, param)
            ));
            return;
        }
        if self.session.learn.is_armed(param) {
            self.session.learn.disarm();
            self.alerts.say("MIDI learn: click a control to map it".to_string());
        } else {
            let serial = self.session.controller.last_control_change().map(|seen| seen.serial);
            self.session.learn.arm(param, serial);
            self.alerts.say(format!(
                "Move a MIDI control to map it to {}",
                learn::param_name(&self.session.controller, param)
            ));
        }
        self.refresh_learn();
    }

    /// Follows control changes: mapped parameters on screen, and the armed
    /// parameter in learn mode.
    pub(crate) fn midi_arrived(&mut self) {
        let moved = !self.session.controller.sync_midi().is_empty();
        if self.refresh_ghosts() || moved {
            self.refresh_values();
        }
        if self.panic_midi_arrived() {
            return;
        }
        let Some(armed) = self.session.learn.armed() else {
            return;
        };
        let Some(seen) = self.session.controller.last_control_change() else {
            return;
        };
        if Some(seen.serial) == armed.serial {
            return;
        }
        let (module, index) = armed.param;
        self.begin_edit();
        self.session.edits.touch(module, index);
        let learned = self.session.controller.learn_midi(seen, ModuleId(module), index);
        match learned {
            Ok((id, removed)) => {
                let mapping = self
                    .session
                    .controller
                    .graph()
                    .midi_mappings()
                    .find(|(i, _)| *i == id)
                    .map(|(_, m)| *m);
                self.session.learn.disarm();
                self.commit();
                if let Some(mapping) = mapping {
                    self.alerts
                        .say(learn::mapped_status(&self.session.controller, mapping, &removed));
                }
            }
            Err(e) => {
                self.session.learn.disarm();
                self.alerts.warn(e.to_string());
                self.refresh_learn();
            }
        }
        if !self.session.edits.gesture_open() {
            self.record();
        }
    }

    /// Brings the ghost marks in line with the engine. Returns true when what
    /// shows changed.
    pub(crate) fn refresh_ghosts(&mut self) -> bool {
        let waiting = self.session.controller.midi_waiting();
        self.session.learn.update_ghosts(&waiting, Instant::now())
    }

    /// Changes the options of a parameter's mapping. One undo step.
    pub(crate) fn set_midi_options(&mut self, module: u32, index: usize, options: MidiMappingOptions) {
        self.close_slider_gesture();
        self.edit(|app| {
            match app
                .session
                .controller
                .set_midi_options(ModuleId(module), index, options)
            {
                Ok(Some(_)) => {
                    app.commit();
                }
                Ok(None) => {}
                Err(e) => app.alerts.warn(e.to_string()),
            }
        });
    }

    /// Enter in a mapping's range fields: a range that passes becomes the
    /// mapping's. Otherwise the text stays with the reason, as for a typed
    /// value (panel-widgets.md, Typing a value).
    pub(crate) fn submit_midi_range(&mut self, module: u32, index: usize) {
        let param = (module, index);
        let Some(draft) = self.session.learn.range_draft(param).cloned() else {
            return;
        };
        let current = self
            .session
            .controller
            .graph()
            .midi_mapping_of(ModuleId(module), index)
            .map(|(_, mapping)| mapping.options);
        let Some(current) = current else {
            self.session.learn.set_range_draft(None);
            return;
        };
        let number = |text: &str| text.trim().replace(',', ".").parse::<f32>().ok();
        let options = number(&draft.low)
            .zip(number(&draft.high))
            .map(|(low, high)| MidiMappingOptions { low, high, ..current });
        let error = match options.map(|options| (options, options.check())) {
            Some((options, Ok(()))) => {
                self.session.learn.set_range_draft(None);
                self.set_midi_options(module, index, options);
                return;
            }
            Some((_, Err(reason))) => reason.to_string(),
            None => "Not a number".to_string(),
        };
        self.session.learn.set_range_draft(Some(learn::RangeDraft {
            error: Some(error),
            ..draft
        }));
    }

    pub(crate) fn stop_learning(&mut self) {
        self.session.learn.stop();
        self.alerts.say("MIDI learn off".to_string());
        self.refresh_learn();
    }

    /// Sets a parameter as a knob turn does. A slider sends no start, so its
    /// first change opens the gesture and letting go closes it.
    pub(crate) fn turn_param(&mut self, module: u32, index: usize, value: f32) {
        self.close_pending_arrow_group();
        self.session.edits.start_slider_gesture();
        self.begin_edit();
        self.set_param(module, index, value);
    }

    /// Selects a parameter's control range, as one undo step. A smaller
    /// range clamps the value into it, a wider one keeps it (control-ranges.md,
    /// rule 6). The commit puts the bounds into the schedule for modulation
    /// and MIDI (rule 8).
    pub(crate) fn set_range(&mut self, module: u32, index: usize, range: usize) {
        let Some(range) = self
            .session
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
            if let Err(e) = app.session.controller.set_range(ModuleId(module), index, range) {
                app.alerts.warn(e.to_string());
                return;
            }
            app.session.edits.touch(module, index);
            let partner = app
                .session
                .controller
                .graph()
                .node(ModuleId(module))
                .and_then(|n| n.descriptor.range_partner(index));
            if let Some(partner) = partner {
                app.session.edits.touch(module, partner);
            }
            app.commit();
        });
        self.refresh_values();
    }

    /// Sets a parameter from typed text, and returns why not if the text is
    /// not a value. It parses with the parameter's whole range. A value outside
    /// the selected control range selects the smallest range that holds it, as
    /// when a patch loads, or the full range if none does (panel-widgets.md,
    /// Typing a value). Call it inside one undo step.
    pub(crate) fn apply_typed_value(&mut self, module: u32, index: usize, text: &str) -> Result<(), String> {
        let id = ModuleId(module);
        let node = self.session.controller.graph().node(id).ok_or("no such module")?;
        let spec = node.descriptor.params.get(index).ok_or("no such parameter")?;
        let value = spec.parse_value_text(text).map_err(|why| why.to_string())?;
        let outside = node
            .spec(index)
            .is_some_and(|selected| !(selected.min..=selected.max).contains(&value));
        // A range widget's two parameters share a range, so it must hold both
        // (control-ranges.md, Ranges on a range widget).
        let partner = node.descriptor.range_partner(index).map(|p| node.params[p]);
        let wider = (outside && !spec.options.ranges.is_empty()).then(|| {
            let holds =
                |range: &loom_core::ParamRange| range.contains(value) && partner.is_none_or(|v| range.contains(v));
            let listed = spec.options.ranges.iter().copied().find(holds);
            listed.unwrap_or(loom_core::ParamRange::new(spec.min, spec.max))
        });
        if let Some(range) = wider {
            if let Err(e) = self.session.controller.set_range(id, index, range) {
                self.alerts.warn(e.to_string());
            }
            self.set_param(module, index, value);
            // The commit puts the bounds into the schedule (control-ranges.md,
            // rule 8).
            self.commit();
        } else {
            self.set_param(module, index, value);
        }
        Ok(())
    }

    /// Why text typed on a canvas control is not a value, if it is not.
    pub(crate) fn typed_check(&self, module: u32, widget: usize, text: &str) -> Result<(), String> {
        let (index, _, _) =
            crate::scene::control_param(&self.session.controller, module, widget).ok_or("no such control")?;
        let node = self
            .session
            .controller
            .graph()
            .node(ModuleId(module))
            .ok_or("no such module")?;
        let spec = node.descriptor.params.get(index).ok_or("no such parameter")?;
        spec.parse_value_text(text).map(|_| ()).map_err(|why| why.to_string())
    }

    /// Drops the inspector value field's draft: Escape, Tab, or focus moved.
    pub(crate) fn clear_value_draft(&mut self) {
        self.session.value_draft = None;
    }

    /// Records which text field has focus. A draft ends when its own field
    /// loses focus, even to another field. A value field that has just got
    /// focus selects its text, so a typed number replaces the value, as Enter
    /// on a canvas control does (panel-widgets.md, Typing a value).
    pub(crate) fn text_focus_changed(&mut self, focus: crate::editing::TextFocus) -> iced::Task<Message> {
        if let Some(draft) = &self.session.value_draft
            && focus.field != Some(value_field_id(draft.module, draft.index))
        {
            self.session.value_draft = None;
        }
        let gained = focus
            .field
            .clone()
            .filter(|field| self.text_focus.field.as_ref() != Some(field) && self.is_value_field(field));
        self.text_focus = focus;
        gained.map_or_else(iced::Task::none, crate::editing::select_all_text)
    }

    /// True if this widget ID is a value field of the selected module.
    fn is_value_field(&self, field: &iced::advanced::widget::Id) -> bool {
        self.session.selection.iter().any(|&module| {
            let params = self
                .session
                .controller
                .graph()
                .node(ModuleId(module))
                .map_or(0, |node| node.descriptor.params.len());
            (0..params).any(|index| value_field_id(module, index) == *field)
        })
    }

    /// Enter in an inspector value field. After a commit the field selects its
    /// new text, so the next number replaces it. Bad text stays as it is, to be
    /// corrected.
    pub(crate) fn value_submit(&mut self, module: u32, index: usize) -> iced::Task<Message> {
        self.submit_value_draft(module, index);
        if self.session.value_draft.is_some() {
            return iced::Task::none();
        }
        crate::editing::select_all_text(value_field_id(module, index))
    }

    /// Tab in a text field on the main view: it closes the field, as on the
    /// canvas. The canvas has taken the same key and moved its focus. The matrix
    /// and the windows over the canvas have their own use of the keys.
    pub(crate) fn text_field_tab(&mut self) -> iced::Task<Message> {
        if !self.text_focus.any || self.matrix_has_keys() || !self.no_overlay() || self.tab_moves_between_fields() {
            return iced::Task::none();
        }
        self.text_focus = crate::editing::TextFocus::default();
        self.session.value_draft = None;
        crate::editing::unfocus_text_fields()
    }

    /// Typing in an inspector value field keeps the text as a draft, with the
    /// reason if it is not a value yet.
    pub(crate) fn type_value_draft(&mut self, module: u32, index: usize, text: String) {
        let error = self
            .session
            .controller
            .graph()
            .node(ModuleId(module))
            .and_then(|node| node.descriptor.params.get(index))
            .and_then(|spec| spec.parse_value_text(&text).err())
            .map(|why| why.to_string());
        self.session.value_draft = Some(ValueDraft {
            module,
            index,
            text,
            error,
        });
    }

    /// Enter in an inspector value field: commits a draft that is a value, as
    /// one undo step. With no draft, or the text the field already showed,
    /// nothing changes, because that text is rounded. Bad text stays.
    pub(crate) fn submit_value_draft(&mut self, module: u32, index: usize) {
        let Some(draft) = self
            .session
            .value_draft
            .clone()
            .filter(|d| d.module == module && d.index == index)
        else {
            return;
        };
        let shown = self.session.controller.graph().node(ModuleId(module)).and_then(|node| {
            Some(crate::param::format(
                node.descriptor.params.get(index)?,
                node.params[index],
            ))
        });
        if shown.as_deref() == Some(draft.text.as_str()) {
            self.session.value_draft = None;
            return;
        }
        if draft.error.is_some() {
            return;
        }
        self.close_slider_gesture();
        self.edit(|app| {
            let _ = app.apply_typed_value(module, index, &draft.text);
        });
        self.session.value_draft = None;
    }

    /// Sets a parameter from a control on a tile or in the inspector.
    pub(crate) fn set_param(&mut self, module: u32, index: usize, value: f32) {
        if let Err(e) = self.session.controller.set_param(ModuleId(module), index, value) {
            self.alerts.warn(e.to_string());
        }
        self.session.edits.touch(module, index);
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
        let edit = self.session.controller.graph().node(ModuleId(module)).and_then(|node| {
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
        let held = self.session.controller.graph().node(ModuleId(module)).and_then(|node| {
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
        let graph = self.session.controller.graph();
        let Some(spec) = graph.node(target).and_then(|n| n.spec(index)) else {
            return;
        };
        let (from, to) = (
            modulation::source_name(graph, source),
            modulation::target_name(graph, target, index),
        );
        if modulation::exists(graph, source, target, index) {
            self.alerts.warn(format!("{from} already modulates {to}"));
            return;
        }
        let depth = modulation::default_depth(&spec);
        match self.session.controller.add_assignment(source, target, index, depth) {
            Ok(_) => {
                if self.commit() {
                    self.alerts
                        .say(format!("{from} modulates {to}, {}", modulation::depth_text(depth)));
                }
            }
            Err(e) => self.alerts.warn(e.to_string()),
        }
    }

    /// One step of an arc drag. The first picks the assignment, by the
    /// spec's order; the rest of the drag keeps it.
    pub(crate) fn drag_depth(
        &mut self,
        module: u32,
        widget: usize,
        part: loom_canvas::ControlPart,
        pinned: Option<loom_canvas::PortRef>,
        delta: f32,
    ) {
        let Some((param, _, _)) = scene::control_part(&self.session.controller, module, widget, part) else {
            return;
        };
        let graph = self.session.controller.graph();
        let picked = self.session.gestures.pick_depth(|| {
            let pinned = pinned.map(|p| OutputRef {
                module: ModuleId(p.module),
                port: p.index,
            });
            let selected = (self.session.selection.len() == 1)
                .then(|| self.session.selection.iter().next().map(|&id| ModuleId(id)))
                .flatten();
            match modulation::drag_target(graph, ModuleId(module), param, pinned, selected) {
                Ok(id) => Some(id),
                Err(why) => {
                    self.alerts.warn(why.to_string());
                    None
                }
            }
        });
        let Some(id) = picked else { return };
        let Some(a) = self.session.controller.graph().assignment(id).copied() else {
            return;
        };
        match self.session.controller.set_assignment_depth(id, a.depth + delta) {
            Ok(depth) => {
                self.alerts.say(format!(
                    "{}: depth {}",
                    modulation::source_name(self.session.controller.graph(), a.source),
                    modulation::depth_text(depth)
                ));
            }
            Err(e) => self.alerts.warn(e.to_string()),
        }
        self.refresh_values();
    }
}

/// Text typed into an inspector value field that is not committed yet
/// (panel-widgets.md, Typing a value).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ValueDraft {
    pub module: u32,
    pub index: usize,
    pub text: String,
    /// Why the text is not a value, if it is not.
    pub error: Option<String>,
}

/// The widget ID of an inspector value field. Each parameter has its own, so a
/// draft can tell its own field from another.
pub(crate) fn value_field_id(module: u32, index: usize) -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::from(format!("value-{module}-{index}"))
}

/// The inspector value field's Escape, heard whichever widget used it. It
/// drops a draft.
pub(crate) fn value_draft_event(
    event: iced::Event,
    _status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    use iced::keyboard::{self, key::Named};
    match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(Named::Escape),
            ..
        }) => Some(Message::Inspector(crate::inspector::Message::ValueDraftClear)),
        _ => None,
    }
}

/// The events that can move text focus, heard whichever widget used them: a
/// press, Escape, which a text field takes as giving up focus, and Tab.
pub(crate) fn text_focus_event(
    event: iced::Event,
    _status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    use iced::keyboard::{self, key::Named};
    match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(Named::Tab),
            ..
        }) => Some(Message::TextFieldTab),
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(Named::Escape),
            ..
        })
        | iced::Event::Mouse(iced::mouse::Event::ButtonPressed(_)) => Some(Message::TextFocusCheck),
        _ => None,
    }
}
