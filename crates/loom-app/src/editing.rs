//! Edits and undo: grouping changes into undo steps, committing them to
//! the engine, and the clipboard commands (`docs/design/undo.md`).

use crate::{App, Message, browser, clipboard, undo};
use iced::{Point, Task};
use loom_core::{LoomError, ModuleId};
use std::collections::BTreeSet;

impl App {
    /// Starts recording an edit: keeps the patch as it is now. Does nothing
    /// if one is already open, so a gesture's first call is the one that
    /// counts.
    pub(crate) fn begin_edit(&mut self) {
        let (controller, positions, sizes) = (&self.controller, &self.positions, &self.sizes);
        self.edits.begin(controller.has_uncommitted_changes(), || {
            undo::Snapshot::take_with_sizes(controller, positions, sizes)
        });
    }

    /// Ends the recording and adds a step if the patch changed.
    pub(crate) fn record(&mut self) {
        // A command's name is for this step only, also when no step is made.
        let step_name = self.step_name.take();
        let (controller, positions, sizes) = (&self.controller, &self.positions, &self.sizes);
        self.edits.record(step_name, || {
            undo::Snapshot::take_with_sizes(controller, positions, sizes)
        });
    }

    /// Closes the open gesture, of either kind, and records it.
    pub(crate) fn end_gesture(&mut self) {
        let step_name = self.step_name.take();
        let (controller, positions, sizes) = (&self.controller, &self.positions, &self.sizes);
        self.edits
            .end_gesture(controller.has_uncommitted_changes(), step_name, || {
                undo::Snapshot::take_with_sizes(controller, positions, sizes)
            });
    }

    /// Runs one edit as one undo step, unless a gesture is open: then the
    /// gesture's end makes the step (undo.md, rule 1 and 2).
    pub(crate) fn edit(&mut self, change: impl FnOnce(&mut Self)) {
        self.begin_edit();
        change(self);
        if !self.edits.gesture_open() {
            self.record();
        }
    }

    /// Closes an inspector slider or a released arrow group before another action.
    pub(crate) fn close_slider_gesture(&mut self) {
        if self.edits.slider_gesture_open() {
            self.end_gesture();
        }
        self.close_pending_arrow_group();
    }

    /// Records a released arrow edit before an action outside the canvas.
    pub(crate) fn close_pending_arrow_group(&mut self) {
        if self.arrow_group_pending {
            self.arrow_group_pending = false;
            self.arrow_group_closed = true;
            self.end_gesture();
        }
    }

    pub(crate) fn undo(&mut self) {
        self.close_slider_gesture();
        if !self.edits.gesture_open() {
            self.step(false);
        }
    }

    pub(crate) fn redo(&mut self) {
        self.close_slider_gesture();
        if !self.edits.gesture_open() {
            self.step(true);
        }
    }

    /// Undoes (`forward` false) or redoes the next step.
    pub(crate) fn step(&mut self, forward: bool) {
        self.gestures.clear_label();
        let step = if forward {
            self.edits.history.redo_step()
        } else {
            self.edits.history.undo_step()
        };
        let Some(step) = step.cloned() else { return };
        let verb = if forward { "Redo" } else { "Undo" };
        if let Err(why) = undo::check(&step, forward, &self.controller) {
            self.alerts
                .say(format!("Cannot {} {}: {why}", verb.to_lowercase(), step.name()));
            return;
        }
        // An armed parameter is a mode, not an edit: undo disarms it first.
        self.learn.disarm();
        let applied = undo::apply_with_sizes(
            &step,
            forward,
            &mut self.controller,
            &mut self.positions,
            &mut self.sizes,
        );
        match applied {
            Ok(()) => {
                if forward {
                    self.edits.history.redone();
                } else {
                    self.edits.history.undone();
                }
                // What the step touched, if it still exists, is selected.
                let touched: BTreeSet<u32> = step
                    .touched_modules()
                    .filter(|&id| self.controller.graph().node(ModuleId(id)).is_some())
                    .collect();
                if !touched.is_empty() {
                    self.selection = touched;
                }
                self.selection
                    .retain(|&id| self.controller.graph().node(ModuleId(id)).is_some());
                if self.commit() {
                    self.alerts.say(format!("{verb}: {}", step.name()));
                } else {
                    // The graph has the change but the engine does not. The
                    // history is already cleared by `commit`.
                    self.alerts.say(format!(
                        "{verb} could not reach the engine ({}); undo history cleared",
                        self.alerts.status()
                    ));
                }
            }
            Err(e) => {
                self.edits.history.clear();
                self.commit();
                self.alerts.say(format!("{verb} failed ({e}); undo history cleared"));
            }
        }
    }

    /// Sends the graph to the engine. Returns false if that failed; the
    /// status bar then holds the error.
    pub(crate) fn commit(&mut self) -> bool {
        self.keep_run_state();
        // Probes follow the graph: a removed module loses its probes.
        self.sync_live();
        let sent = match self.controller.commit() {
            Ok(_) => {
                self.retry_commit = false;
                self.report();
                self.edits.committed(true);
                true
            }
            Err(e) => {
                self.retry_commit = matches!(&e, LoomError::QueueFull);
                self.alerts.say(format!("error: {e}"));
                // The controller stays dirty, so the next commit retries the
                // change; until then the history cannot be trusted.
                self.edits.committed(false);
                false
            }
        };
        self.sync_audio_input();
        self.refresh();
        sent
    }

    /// Removes modules and commits. Returns false if the commit failed; the
    /// status bar then holds the error.
    pub(crate) fn delete_modules(&mut self, ids: &[u32]) -> bool {
        for id in ids {
            if self.controller.remove_module(ModuleId(*id)).is_ok() {
                self.positions.remove(id);
                self.selection.remove(id);
            }
        }
        self.commit()
    }

    /// The clipboard command a message asks for: `'c'`, `'x'`, or `'d'`
    /// with Ctrl or Cmd. Paste comes from the canvas instead, which knows
    /// where the pointer is. Keys go to the browser while it is open.
    pub(crate) fn clipboard_key(&self, message: &Message) -> Option<char> {
        let Message::Key(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Character(c),
            modifiers,
            ..
        }) = message
        else {
            return None;
        };
        if !modifiers.command() || self.browser.is_some() || self.probe_menu.is_some() || self.rig.window.is_some() {
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
        let group =
            clipboard::Group::from_selection(self.controller.graph(), &self.positions, &self.selection, incoming)
                .with_sizes(&self.sizes);
        if group.is_empty() {
            self.alerts.say(format!("Nothing selected to {verb}"));
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
        if self.edits.gesture_open() {
            return None;
        }
        let group = self.selected_group("cut", false)?;
        let ids: Vec<u32> = self.selection.iter().copied().collect();
        self.step_name = Some(format!("cut {}", group.describe()));
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
        if self.edits.gesture_open() {
            return;
        }
        let Some(text) = text else {
            self.alerts.say(clipboard::NOTHING_TO_PASTE.to_string());
            return;
        };
        match clipboard::Group::from_text(&text, self.controller.registry()) {
            Ok(group) => self.add_group(&group, browser::snap(world), "paste", "Pasted"),
            Err(why) => self.alerts.say(why),
        }
    }

    /// Duplicates the selection next to it, keeping incoming cables and
    /// assignments.
    pub(crate) fn duplicate(&mut self) {
        self.close_slider_gesture();
        if self.edits.gesture_open() {
            return;
        }
        let Some(group) = self.selected_group("duplicate", true) else {
            return;
        };
        let at = group.top_left().unwrap_or(Point::ORIGIN);
        let offset = clipboard::DUPLICATE_OFFSET;
        self.add_group(
            &group,
            Point::new(at.x + offset, at.y + offset),
            "duplicate",
            "Duplicated",
        );
    }

    /// Adds a group as one step and selects it, or changes nothing and says
    /// why.
    pub(crate) fn add_group(&mut self, group: &clipboard::Group, origin: Point, verb: &str, done: &str) {
        if let Err(why) = group.check(&self.controller) {
            self.alerts.say(format!("Cannot {verb}: {why}"));
            return;
        }
        self.step_name = Some(format!("{verb} {}", group.describe()));
        self.edit(|app| match group.add(&mut app.controller, &mut app.positions, origin) {
            Ok(ids) => {
                group.apply_sizes(&ids, &mut app.sizes);
                app.selection = ids.into_iter().collect();
                if app.commit() {
                    app.alerts.say(format!("{done} {}", group.describe()));
                }
            }
            Err(e) => {
                // The rollback freed the new modules' slots. Only a commit
                // hands them back, so the next paste has room.
                app.commit();
                app.alerts.say(format!("Cannot {verb}: {e}"));
            }
        });
        // `record` clears it, but an edit inside a gesture does not record.
        self.step_name = None;
    }
}
