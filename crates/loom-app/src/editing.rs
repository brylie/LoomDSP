//! Edits and undo: grouping changes into undo steps, committing them to
//! the engine, and the clipboard commands (`docs/design/undo.md`).

use crate::{App, Message, RightTab, alerts, browser, clipboard, undo};
use iced::advanced::widget::operation::{self, Focusable, Outcome};
use iced::advanced::widget::{Id, Operation};
use iced::{Point, Task};
use loom_core::{LoomError, ModuleId};
use std::collections::BTreeSet;

/// True when a text field or text box in the window has focus. Iced's
/// text widgets have no undo of their own, so while one is being edited the
/// undo shortcuts do nothing rather than change the patch behind it
/// (undo.md, Gestures).
pub(crate) fn text_field_focused() -> Task<bool> {
    text_focus().map(|focus| focus.any)
}

/// Focuses a text field, then tells the app. A focus task sends no message, and
/// the canvas must know a text field has the keys (panel-widgets.md, Keys in a
/// text field). So use this wherever a task focuses a field.
pub(crate) fn focus_text_field(id: &'static str) -> Task<crate::Message> {
    iced::widget::operation::focus(id).chain(text_focus().map(crate::Message::TextFocus))
}

/// Which text field has focus, as the widget tree knows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TextFocus {
    /// A text field or text box has focus.
    pub any: bool,
    /// The ID of the one that has it, if it has one.
    pub field: Option<iced::advanced::widget::Id>,
}

/// Asks the widget tree which text field has focus. Iced's text fields do not
/// say when focus moves, and a press can move it, so the app asks after the
/// press has reached the widgets.
pub(crate) fn text_focus() -> Task<TextFocus> {
    use iced::advanced::widget::operation::{Focusable, Outcome};
    use iced::advanced::widget::{Id, Operation};

    /// Records the focused widget. Only text fields and text boxes are
    /// focusable in this app.
    struct Find(TextFocus);

    impl Operation<TextFocus> for Find {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<TextFocus>)) {
            operate(self);
        }

        fn focusable(&mut self, id: Option<&Id>, _bounds: iced::Rectangle, state: &mut dyn Focusable) {
            if state.is_focused() {
                self.0 = TextFocus {
                    any: true,
                    field: id.cloned(),
                };
            }
        }

        fn finish(&self) -> Outcome<TextFocus> {
            Outcome::Some(self.0.clone())
        }
    }

    iced::advanced::widget::operate(Find(TextFocus::default()))
}

/// Selects all the text in a field, so what is typed next replaces it.
pub(crate) fn select_all_text(id: iced::advanced::widget::Id) -> Task<crate::Message> {
    iced::advanced::widget::operate(iced::advanced::widget::operation::text_input::select_all(id))
}

/// Takes focus from every text field.
pub(crate) fn unfocus_text_fields() -> Task<crate::Message> {
    iced::advanced::widget::operate(iced::advanced::widget::operation::focusable::focus(
        iced::advanced::widget::Id::unique(),
    ))
}

/// The field Tab moves to from `current`, wrapping at the ends, or `current`
/// when it is not one of `fields`.
pub(crate) fn field_after(fields: &[&'static str], current: Id, backwards: bool) -> Id {
    let at = fields.iter().copied().position(|field| Id::new(field) == current);
    match at {
        Some(at) => {
            let step = if backwards { fields.len() - 1 } else { 1 };
            Id::new(fields[(at + step) % fields.len()])
        }
        None => current,
    }
}

/// Moves focus to the next or previous of `fields`. With no named field in
/// focus it changes nothing. Iced's own `focus_next` walks every field in the
/// window, so it would go behind an open overlay (panels.md).
pub(crate) struct StepFocus {
    pub(crate) fields: &'static [&'static str],
    pub(crate) backwards: bool,
    pub(crate) current: Option<Id>,
}

impl Operation for StepFocus {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn focusable(&mut self, id: Option<&Id>, _bounds: iced::Rectangle, state: &mut dyn Focusable) {
        if state.is_focused() {
            self.current = id.cloned();
        }
    }

    fn finish(&self) -> Outcome<()> {
        match &self.current {
            Some(current) => Outcome::Chain(Box::new(operation::focusable::focus(field_after(
                self.fields,
                current.clone(),
                self.backwards,
            )))),
            None => Outcome::None,
        }
    }
}

impl App {
    /// The text fields Tab moves between in the view on top now. None when
    /// that view has fewer than two.
    pub(crate) fn tab_fields(&self) -> &'static [&'static str] {
        if self.overlays.patches().is_some() {
            &crate::patches::TEXT_FIELDS
        } else if self.no_overlay() && self.right_tab == RightTab::Inspector && self.session.selection.is_empty() {
            &crate::patch_info::TEXT_FIELDS
        } else {
            &[]
        }
    }

    /// True when the focused text field is one Tab moves on from, to the next
    /// field of its view. Any other field closes on Tab instead
    /// (panel-widgets.md, Typing a value).
    pub(crate) fn tab_moves_between_fields(&self) -> bool {
        self.text_focus.field.as_ref().is_some_and(|focused| {
            self.tab_fields()
                .iter()
                .copied()
                .any(|field| Id::new(field) == *focused)
        })
    }

    /// Starts recording an edit: keeps the patch as it is now. Does nothing
    /// if one is already open, so a gesture's first call is the one that
    /// counts.
    pub(crate) fn begin_edit(&mut self) {
        let (controller, positions, sizes) = (&self.session.controller, &self.session.positions, &self.session.sizes);
        self.session.edits.begin(controller.has_uncommitted_changes(), || {
            undo::Snapshot::take_with_sizes(controller, positions, sizes)
        });
    }

    /// Ends the recording and adds a step if the patch changed.
    pub(crate) fn record(&mut self) {
        // A command's name is for this step only, also when no step is made.
        let step_name = self.session.step_name.take();
        let (controller, positions, sizes) = (&self.session.controller, &self.session.positions, &self.session.sizes);
        self.session.edits.record(step_name, || {
            undo::Snapshot::take_with_sizes(controller, positions, sizes)
        });
    }

    /// Closes the open gesture, of either kind, and records it.
    pub(crate) fn end_gesture(&mut self) {
        let step_name = self.session.step_name.take();
        let (controller, positions, sizes) = (&self.session.controller, &self.session.positions, &self.session.sizes);
        self.session
            .edits
            .end_gesture(controller.has_uncommitted_changes(), step_name, || {
                undo::Snapshot::take_with_sizes(controller, positions, sizes)
            });
    }

    /// Runs one edit as one undo step, unless a gesture is open: then the
    /// gesture's end makes the step (undo.md, rule 1 and 2).
    pub(crate) fn edit(&mut self, change: impl FnOnce(&mut Self)) {
        self.begin_edit();
        change(self);
        if !self.session.edits.gesture_open() {
            self.record();
        }
    }

    /// Closes an inspector slider or a released arrow group before another action.
    pub(crate) fn close_slider_gesture(&mut self) {
        if self.session.edits.slider_gesture_open() {
            self.end_gesture();
        }
        self.close_pending_arrow_group();
    }

    /// Records a released arrow edit before an action outside the canvas.
    pub(crate) fn close_pending_arrow_group(&mut self) {
        self.arrow_event(crate::edits::ArrowEvent::Close);
    }

    /// Applies an arrow-group event: ends or opens the gesture as the group
    /// says, then keeps the group's new state (`docs/design/undo.md`).
    pub(crate) fn arrow_event(&mut self, event: crate::edits::ArrowEvent) {
        let (next, effect) = self.session.edits.arrow_group().on(event);
        if effect.ends() {
            self.end_gesture();
        }
        if effect.starts() {
            self.close_slider_gesture();
            self.session.edits.start_gesture();
            self.begin_edit();
        }
        self.session.edits.set_arrow_group(next);
    }

    pub(crate) fn undo(&mut self) {
        self.close_slider_gesture();
        if !self.session.edits.gesture_open() {
            self.step(false);
        }
    }

    pub(crate) fn redo(&mut self) {
        self.close_slider_gesture();
        if !self.session.edits.gesture_open() {
            self.step(true);
        }
    }

    /// Undoes (`forward` false) or redoes the next step.
    pub(crate) fn step(&mut self, forward: bool) {
        self.session.gestures.clear_label();
        let step = if forward {
            self.session.edits.history.redo_step()
        } else {
            self.session.edits.history.undo_step()
        };
        let Some(step) = step.cloned() else { return };
        let verb = if forward { "Redo" } else { "Undo" };
        if let Err(why) = undo::check(&step, forward, &self.session.controller) {
            self.alerts
                .warn(format!("Cannot {} {}: {why}", verb.to_lowercase(), step.name()));
            return;
        }
        // An armed parameter is a mode, not an edit: undo disarms it first.
        self.session.learn.disarm();
        let applied = undo::apply_with_sizes(
            &step,
            forward,
            &mut self.session.controller,
            &mut self.session.positions,
            &mut self.session.sizes,
        );
        match applied {
            Ok(()) => {
                if forward {
                    self.session.edits.history.redone();
                } else {
                    self.session.edits.history.undone();
                }
                // What the step touched, if it still exists, is selected.
                let touched: BTreeSet<u32> = step
                    .touched_modules()
                    .filter(|&id| self.session.controller.graph().node(ModuleId(id)).is_some())
                    .collect();
                if !touched.is_empty() {
                    self.session.selection = touched;
                }
                self.session
                    .selection
                    .retain(|&id| self.session.controller.graph().node(ModuleId(id)).is_some());
                if self.commit() {
                    self.alerts.say(format!("{verb}: {}", step.name()));
                } else {
                    // The graph has the change but the engine does not. The
                    // history is already cleared by `commit`.
                    self.alerts.error(
                        alerts::Cause::Other,
                        format!("{verb} could not reach the engine: {}", self.alerts.status()),
                    );
                }
            }
            Err(e) => {
                self.session.edits.history.clear();
                self.commit();
                self.alerts.error(
                    alerts::Cause::Other,
                    format!("{verb} failed ({e}); undo history cleared"),
                );
            }
        }
    }

    /// Sends the graph to the engine. Returns false if that failed; the
    /// status bar then holds the error.
    pub(crate) fn commit(&mut self) -> bool {
        self.keep_run_state();
        // Probes follow the graph: a removed module loses its probes.
        self.sync_live();
        let sent = match self.session.controller.commit() {
            Ok(_) => {
                self.session.retry_commit = false;
                self.report();
                self.session.edits.committed(true);
                true
            }
            Err(e) => {
                self.commit_failed(&e);
                // The controller stays dirty, so the next commit retries the
                // change; until then the history cannot be trusted.
                self.session.edits.committed(false);
                false
            }
        };
        self.sync_audio_input();
        self.refresh();
        sent
    }

    /// Says a commit failed, before the undo history is cleared. A full queue
    /// drains and the commit is retried, so with no history to lose that is a
    /// warning. Anything else stays true (the graph differs from what the
    /// engine plays), and lost history stays lost after a retry works, so
    /// those are kept.
    pub(crate) fn commit_failed(&mut self, error: &LoomError) {
        self.session.retry_commit = matches!(error, LoomError::QueueFull);
        let history = &self.session.edits.history;
        let lost_history = history.undo_step().is_some() || history.redo_step().is_some();
        let message = if lost_history {
            format!("{error}; undo history cleared")
        } else {
            error.to_string()
        };
        if self.session.retry_commit && !lost_history {
            self.alerts.warn(message);
        } else {
            self.alerts.error(alerts::Cause::Other, message);
        }
    }

    /// Removes modules and commits. Returns false if the commit failed; the
    /// status bar then holds the error.
    pub(crate) fn delete_modules(&mut self, ids: &[u32]) -> bool {
        for id in ids {
            if self.session.controller.remove_module(ModuleId(*id)).is_ok() {
                self.session.positions.remove(id);
                self.session.selection.remove(id);
            }
        }
        self.commit()
    }

    /// The clipboard command a message asks for: `'c'`, `'x'`, or `'d'`
    /// with Ctrl or Cmd. Paste comes from the canvas instead, which knows
    /// where the pointer is. They act on the patch, so they work only while the
    /// canvas has the input.
    pub(crate) fn clipboard_key(&self, message: &Message) -> Option<char> {
        let Message::Key(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Character(c),
            modifiers,
            ..
        }) = message
        else {
            return None;
        };
        if !modifiers.command() || self.input_owner() != crate::input_owner::InputOwner::Canvas {
            return None;
        }
        ['c', 'x', 'd']
            .into_iter()
            .find(|k| c.eq_ignore_ascii_case(&k.to_string()))
    }

    pub(crate) fn clipboard_command(&mut self, message: &Message) -> Task<Message> {
        let text = match self.clipboard_key(message) {
            Some('c') => self.copy(),
            Some('x') => self.cut(),
            Some('d') => {
                self.duplicate();
                None
            }
            _ => None,
        };
        text.map_or_else(Task::none, iced::clipboard::write)
    }

    /// The selected modules, or `None` and a status bar note when nothing
    /// is selected.
    pub(crate) fn selected_group(&mut self, verb: &str, incoming: bool) -> Option<clipboard::Group> {
        let group = clipboard::Group::from_selection(
            self.session.controller.graph(),
            &self.session.positions,
            &self.session.selection,
            incoming,
        )
        .with_sizes(&self.session.sizes);
        if group.is_empty() {
            self.alerts.warn(format!("Nothing selected to {verb}"));
            return None;
        }
        Some(group)
    }

    /// Copy: returns the patch text for the clipboard. Not an edit.
    pub(crate) fn copy(&mut self) -> Option<String> {
        let group = self.selected_group("copy", false)?;
        self.alerts.say(format!("Copied {}", group.describe()));
        Some(group.to_text())
    }

    /// Cut: copy, then delete, as one step named "cut ...".
    pub(crate) fn cut(&mut self) -> Option<String> {
        // Like undo, these commands wait for a drag to finish.
        self.close_slider_gesture();
        if self.session.edits.gesture_open() {
            return None;
        }
        let group = self.selected_group("cut", false)?;
        let ids: Vec<u32> = self.session.selection.iter().copied().collect();
        self.session.step_name = Some(format!("cut {}", group.describe()));
        self.edit(|app| {
            if app.delete_modules(&ids) {
                app.alerts.say(format!("Cut {}", group.describe()));
            }
        });
        Some(group.to_text())
    }

    /// Pastes clipboard text with its top-left corner at a world point.
    pub(crate) fn paste(&mut self, world: Point, text: Option<String>) {
        self.close_slider_gesture();
        if self.session.edits.gesture_open() {
            return;
        }
        let Some(text) = text else {
            self.alerts.warn(clipboard::NOTHING_TO_PASTE.to_string());
            return;
        };
        match clipboard::Group::from_text(&text, self.session.controller.registry()) {
            Ok(group) => {
                let at = browser::snap(world);
                self.add_group(&group, (at.x, at.y), "paste", "Pasted");
            }
            Err(why) => self.alerts.warn(why),
        }
    }

    /// Duplicates the selection next to it, keeping incoming cables and
    /// assignments.
    pub(crate) fn duplicate(&mut self) {
        self.close_slider_gesture();
        if self.session.edits.gesture_open() {
            return;
        }
        let Some(group) = self.selected_group("duplicate", true) else {
            return;
        };
        let at = group.top_left().unwrap_or((0.0, 0.0));
        let offset = clipboard::DUPLICATE_OFFSET;
        self.add_group(&group, (at.0 + offset, at.1 + offset), "duplicate", "Duplicated");
    }

    /// Adds a group as one step and selects it, or changes nothing and says
    /// why.
    pub(crate) fn add_group(&mut self, group: &clipboard::Group, origin: (f32, f32), verb: &str, done: &str) {
        if let Err(why) = group.check(&self.session.controller) {
            self.alerts.warn(format!("Cannot {verb}: {why}"));
            return;
        }
        self.session.step_name = Some(format!("{verb} {}", group.describe()));
        self.edit(
            |app| match group.add(&mut app.session.controller, &mut app.session.positions, origin) {
                Ok(ids) => {
                    group.apply_sizes(&ids, &mut app.session.sizes);
                    app.session.selection = ids.into_iter().collect();
                    if app.commit() {
                        app.alerts.say(format!("{done} {}", group.describe()));
                    }
                }
                Err(e) => {
                    // The rollback freed the new modules' slots. Only a commit
                    // hands them back, so the next paste has room.
                    app.commit();
                    app.alerts.warn(format!("Cannot {verb}: {e}"));
                }
            },
        );
        // `record` clears it, but an edit inside a gesture does not record.
        self.session.step_name = None;
    }
}
