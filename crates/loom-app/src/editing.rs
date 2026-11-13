//! Edits and undo: grouping changes into undo steps, committing them to
//! the engine, and the clipboard commands (`docs/design/undo.md`).

use crate::{App, Message, RightTab, alerts, browser, clipboard, undo};
use loom_core::{LoomError, ModuleId};
use loom_ui_core::{Point, UiId};
use std::collections::BTreeSet;

/// Why toolbar Undo or Redo is disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryBlock {
    /// The history has no step that way.
    Nothing,
    /// An edit is still in progress, such as a held arrow key.
    EditOpen,
}

impl HistoryBlock {
    /// The button's hover text while it is disabled.
    pub(crate) fn hover(self, redo: bool) -> &'static str {
        match (self, redo) {
            (Self::Nothing, false) => "Nothing to undo",
            (Self::Nothing, true) => "Nothing to redo",
            (Self::EditOpen, _) => "Waits until the edit in progress ends",
        }
    }

    /// Why `press` cannot press the button, for the control channel.
    pub(crate) fn press_error(self, redo: bool) -> &'static str {
        match (self, redo) {
            (Self::Nothing, false) => "Undo is disabled: nothing to undo",
            (Self::Nothing, true) => "Redo is disabled: nothing to redo",
            (Self::EditOpen, false) => "Undo is disabled: an edit is in progress",
            (Self::EditOpen, true) => "Redo is disabled: an edit is in progress",
        }
    }
}

/// The field Tab moves to from `current`, wrapping at the ends, or `None`
/// when `current` is not one of `fields`. Not the next element anywhere in
/// the window, as Iced's `focus_next` would give: Tab stays in its view, so
/// it never goes behind an open overlay (panels.md).
pub(crate) fn field_after(fields: &[UiId], current: &UiId, backwards: bool) -> Option<UiId> {
    let at = fields.iter().position(|field| field == current)?;
    let step = if backwards { fields.len() - 1 } else { 1 };
    Some(fields[(at + step) % fields.len()].clone())
}

impl App {
    /// The text fields Tab moves between in the view on top now. None when
    /// that view has fewer than two.
    pub(crate) fn tab_fields(&self) -> Vec<UiId> {
        if self.overlays.patches().is_some() {
            crate::patches::text_fields().to_vec()
        } else if self.no_overlay() && self.right_tab == RightTab::Inspector && self.session.selection.is_empty() {
            crate::patch_info::text_fields().to_vec()
        } else {
            Vec::new()
        }
    }

    /// True when the focused text field is one Tab moves on from, to the next
    /// field of its view. Any other field closes on Tab instead
    /// (panel-widgets.md, Typing a value).
    pub(crate) fn tab_moves_between_fields(&self, focused: Option<&UiId>) -> bool {
        focused.is_some_and(|focused| self.tab_fields().contains(focused))
    }

    /// Tab or Shift+Tab between the fields of the view on top: the next one
    /// takes focus. With no field of the view in focus it changes nothing.
    pub(crate) fn tab_between_fields(&mut self, focused: Option<&UiId>, backwards: bool) {
        let next = focused.and_then(|focused| field_after(&self.tab_fields(), focused, backwards));
        if let Some(next) = next {
            self.effect(loom_ui::Effects::focus(next));
        }
    }

    /// Starts recording an edit: keeps the patch as it is now. Does nothing
    /// if one is already open, so a gesture's first call is the one that
    /// counts.
    pub(crate) fn begin_edit(&mut self) {
        if self.session.linked_before.is_none() {
            self.session.linked_before = Some(self.linked_fingerprints());
        }
        let (controller, hierarchy, level, positions, sizes) = (
            &self.session.controller,
            &self.session.hierarchy,
            &self.session.level,
            &self.session.positions,
            &self.session.sizes,
        );
        self.session.edits.begin(controller.has_uncommitted_changes(), || {
            undo::Snapshot::take_full(controller, hierarchy, level, positions, sizes)
        });
    }

    /// What each linked sub-patch would save as now, by definition key.
    pub(crate) fn linked_fingerprints(&self) -> std::collections::BTreeMap<String, String> {
        self.session
            .hierarchy
            .linked_fingerprints(self.session.controller.graph(), self.layout_fn())
    }

    /// Makes every linked sub-patch the edit in progress changed inside a
    /// plain copy, so the next load does not replace the edit, and says so
    /// (sub-patches.md, Copies and editing). Called as the edit's step is
    /// recorded, so the unlink is part of the same step. An exposed value
    /// belongs to the instance, not the definition, so changing one keeps
    /// the link.
    fn unlink_edited(&mut self) {
        if let Some(before) = self.session.linked_before.take() {
            self.unlink_changed(&before);
        }
    }

    /// Unlinks every linked sub-patch whose saved text differs from
    /// `before`, and says so. Also for a MIDI control that moved a value
    /// inside a linked tile, which is an edit inside that goes through no
    /// undo step (Notes from building).
    pub(crate) fn unlink_changed(&mut self, before: &std::collections::BTreeMap<String, String>) {
        if before.is_empty() {
            return;
        }
        let after = self.linked_fingerprints();
        let changed: Vec<String> = before
            .iter()
            .filter(|(key, text)| after.get(*key).is_some_and(|now| now != *text))
            .map(|(key, _)| key.clone())
            .collect();
        for key in changed {
            if let Some(name) = self.session.hierarchy.unlink_key(&key) {
                self.alerts.say(crate::levels::now_a_copy(&name));
            }
        }
    }

    /// Ends the recording and adds a step if the patch changed.
    pub(crate) fn record(&mut self) {
        // A command's name is for this step only, also when no step is made.
        let step_name = self.session.step_name.take();
        self.unlink_edited();
        let (controller, hierarchy, level, positions, sizes) = (
            &self.session.controller,
            &self.session.hierarchy,
            &self.session.level,
            &self.session.positions,
            &self.session.sizes,
        );
        self.session.edits.record(step_name, || {
            undo::Snapshot::take_full(controller, hierarchy, level, positions, sizes)
        });
    }

    /// Closes the open gesture, of either kind, and records it.
    pub(crate) fn end_gesture(&mut self) {
        let step_name = self.session.step_name.take();
        self.unlink_edited();
        let (controller, hierarchy, level, positions, sizes) = (
            &self.session.controller,
            &self.session.hierarchy,
            &self.session.level,
            &self.session.positions,
            &self.session.sizes,
        );
        self.session
            .edits
            .end_gesture(controller.has_uncommitted_changes(), step_name, || {
                undo::Snapshot::take_full(controller, hierarchy, level, positions, sizes)
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
        self.reader_event(crate::edits::ReaderEvent::Close);
    }

    /// Applies an event to the screen reader's undo group: ends or opens
    /// the gesture as the group says, then keeps the group's new state
    /// (ui-layer.md, section 7, rule 6).
    pub(crate) fn reader_event(&mut self, event: crate::edits::ReaderEvent) {
        let (next, effect) = self.session.edits.reader_group().on(event);
        if effect.ends() {
            self.end_gesture();
        }
        if effect.starts() {
            // Closed first, so closing what else is open does not end this
            // group a second time.
            self.session.edits.set_reader_group(crate::edits::ReaderGroup::Closed);
            self.close_slider_gesture();
            self.session.edits.start_gesture();
            self.begin_edit();
        }
        self.session.edits.set_reader_group(next);
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

    /// What toolbar Undo would do, or why it is disabled. The button and
    /// the control channel both ask this, so a script presses what a click
    /// could. An edit that Undo closes first, a released arrow group or an
    /// inspector slider gesture, is not a step until it closes, so Undo
    /// works then too and takes that edit. While an arrow key is held, Undo
    /// would do nothing, so it is disabled (undo.md).
    pub(crate) fn undo_hint(&self) -> Result<String, HistoryBlock> {
        if self.history_waits() {
            return Err(HistoryBlock::EditOpen);
        }
        if let Some(edit) = self.edit_undo_closes() {
            return Ok(format!("Undo: {edit}"));
        }
        let step = self.session.edits.history.undo_step().ok_or(HistoryBlock::Nothing)?;
        Ok(format!("Undo: {}", step.name()))
    }

    /// What toolbar Redo would do, or why it is disabled. An edit that Redo
    /// closes first is a new edit, and a new edit drops the redo steps, so
    /// there is nothing to redo then (undo.md).
    pub(crate) fn redo_hint(&self) -> Result<String, HistoryBlock> {
        if self.history_waits() {
            return Err(HistoryBlock::EditOpen);
        }
        if self.edit_undo_closes().is_some() {
            return Err(HistoryBlock::Nothing);
        }
        let step = self.session.edits.history.redo_step().ok_or(HistoryBlock::Nothing)?;
        Ok(format!("Redo: {}", step.name()))
    }

    /// The edit in progress that Undo and Redo close before they act, as
    /// hover text names it: an inspector slider gesture or a released arrow
    /// group (`close_slider_gesture`).
    fn edit_undo_closes(&self) -> Option<&'static str> {
        let edits = &self.session.edits;
        if edits.slider_gesture_open() {
            Some("the slider edit")
        } else if matches!(edits.arrow_group(), crate::edits::ArrowGroup::Released { .. }) {
            Some("the arrow key edit")
        } else if edits.reader_group() != crate::edits::ReaderGroup::Closed {
            // No key is held, so Undo never waits for this group.
            Some("the screen reader edit")
        } else {
            None
        }
    }

    /// True while a gesture is open that Undo and Redo do not close first,
    /// such as a held arrow key or a drag, so they would do nothing.
    fn history_waits(&self) -> bool {
        self.session.edits.gesture_open() && self.edit_undo_closes().is_none()
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
        // A fade in progress is finished first; the step's own swap, if
        // any, fades again (sub-patches.md, Slots).
        self.finish_all_fades();
        if let Err(why) = undo::check(&step, forward, &self.session.controller) {
            self.alerts
                .warn(format!("Cannot {} {}: {why}", verb.to_lowercase(), step.name()));
            return;
        }
        // An armed parameter is a mode, not an edit: undo disarms it first.
        self.session.learn.disarm();
        let layout = self.layout_fn_owned();
        let applied = undo::apply_full(
            &step,
            forward,
            &mut self.session.controller,
            &mut self.session.hierarchy,
            &mut self.session.positions,
            &mut self.session.sizes,
            &layout,
        );
        match applied {
            Ok(()) => {
                if forward {
                    self.session.edits.history.redone();
                } else {
                    self.session.edits.history.undone();
                }
                // The level the edit was made on is shown first, so the
                // player sees what was undone (sub-patches.md, Undo across
                // levels). The graph's cables follow the restored hierarchy.
                self.show_level(&self.session.hierarchy.nearest_level(step.level()));
                match self.session.hierarchy.sync(&mut self.session.controller) {
                    Ok(renamed) => self.assignments_renumbered(&renamed),
                    Err(e) => {
                        // Both sides of the step were recorded together, so
                        // they cannot disagree: a failure here is a bug in a
                        // record.
                        debug_assert!(false, "a restored hierarchy must sync: {e}");
                        self.alerts
                            .warn(format!("Cannot {} {}: {e}", verb.to_lowercase(), step.name()));
                    }
                }
                // What the step touched, if it is still on this level, is
                // selected.
                let touched: BTreeSet<u32> = step
                    .touched_modules()
                    .filter(|&id| on_shown_level(&self.session, id))
                    .collect();
                if !touched.is_empty() {
                    self.session.selection = touched;
                }
                let kept: BTreeSet<u32> = self
                    .session
                    .selection
                    .iter()
                    .copied()
                    .filter(|&id| on_shown_level(&self.session, id))
                    .collect();
                self.session.selection = kept;
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
        // Stack copies and the modules a mode adds follow an edit made
        // straight on the graph (ADR-0040).
        self.sync_modes();
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
        // Through the hierarchy: a sub-patch tile takes its inside with it,
        // and a port its cables.
        self.delete_tiles(ids)
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

    pub(crate) fn clipboard_command(&mut self, message: &Message) {
        let text = match self.clipboard_key(message) {
            Some('c') => self.copy(),
            Some('x') => self.cut(),
            Some('d') => {
                self.duplicate();
                None
            }
            _ => None,
        };
        if let Some(text) = text {
            self.effect(loom_ui::Effects::clipboard_write(text));
        }
    }

    /// The selected tiles, or `None` and a status bar note when nothing is
    /// selected. A port tile and the Transport are left out, so a selection
    /// of only those is nothing.
    pub(crate) fn selected_group(&mut self, verb: &str, incoming: bool) -> Option<clipboard::Group> {
        // A slot tile is not carried yet (sub-patches.md, Slots).
        if self
            .session
            .selection
            .iter()
            .any(|&id| self.session.hierarchy.slot(ModuleId(id)).is_some())
        {
            self.alerts.warn(crate::slots::CANNOT_COPY_SLOT.to_string());
            return None;
        }
        let group = clipboard::Group::from_selection(
            self.session.controller.graph(),
            &self.session.hierarchy,
            &self.session.level,
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
        // Only what the clipboard holds goes: a patch-wide module or a port
        // tile in the selection stays, since it was not copied.
        let ids: Vec<u32> = group.ids().collect();
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
    /// assignments, also those that come in through a port or from a
    /// sub-patch tile (sub-patches.md, Editor model).
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
    /// why. A sub-patch tile in it gets a definition of its own. Patch-wide
    /// modules the text held are left out, and the status bar says so
    /// (sub-patches.md, rule 8).
    pub(crate) fn add_group(&mut self, group: &clipboard::Group, origin: (f32, f32), verb: &str, done: &str) {
        if let Err(why) = group.check(&self.session.controller) {
            self.alerts.warn(format!("Cannot {verb}: {why}"));
            return;
        }
        // A layer or select level holds only its members.
        if group.has_modules()
            && let Some(why) = self.session.hierarchy.level_refusal(&self.session.level)
        {
            self.alerts.warn(why);
            return;
        }
        self.session.step_name = Some(format!("{verb} {}", group.describe()));
        self.edit(|app| {
            // On a candidate, so a refusal leaves the hierarchy as it was
            // (ADR-0039, invariant 3).
            let mut candidate = app.session.hierarchy.clone();
            let added = group.add(
                &mut app.session.controller,
                &mut candidate,
                &app.session.level,
                &mut app.session.positions,
                &mut app.session.sizes,
                origin,
            );
            match added {
                Ok(added) => {
                    app.session.hierarchy = candidate;
                    app.assignments_renumbered(&added.renamed);
                    app.session.selection = added.ids.into_iter().collect();
                    if app.commit() {
                        let status = format!("{done} {}", group.describe());
                        match group.left_out_note() {
                            Some(note) => app.alerts.warn(format!("{status}. {note}")),
                            None => app.alerts.say(status),
                        }
                    }
                }
                Err(why) => {
                    // The rollback freed the new modules' slots. Only a commit
                    // hands them back, so the next paste has room.
                    app.commit();
                    app.alerts.warn(format!("Cannot {verb}: {why}"));
                }
            }
        });
        // `record` clears it, but an edit inside a gesture does not record.
        self.session.step_name = None;
    }
}

/// True when `id` is a tile on the level the canvas shows.
fn on_shown_level(session: &crate::session::PatchSession, id: u32) -> bool {
    let id = ModuleId(id);
    session.hierarchy.tile(session.controller.graph(), id).is_some() && session.hierarchy.level_of(id) == session.level
}
