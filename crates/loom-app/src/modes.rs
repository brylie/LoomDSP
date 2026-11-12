//! Sub-patch modes in the app (`docs/design/sub-patches.md`, The editor for
//! modes; ADR-0040): the Mode, members, Copies, spread, and stereo spread
//! gestures from the sub-patch inspector, each one undo step, and the fan
//! out that makes a parameter write or an assignment edit on copy 0 of a
//! stack reach every copy at once.
//!
//! The overlay does the work (`loom_patch::Hierarchy`'s mode edits); this
//! file turns a gesture into a candidate change, a sync, and a commit, and
//! keeps the stack's copies in step between syncs.

use crate::App;
use loom_core::{AssignmentId, ModuleId};
use loom_patch::Mode;
use loom_patch::flatten::{SEED_PARAM, seeded};

/// How far apart members sit in their row, in canvas units.
const MEMBER_SPACING: f32 = 240.0;

/// A gesture on a sub-patch tile's mode rows.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Change the tile's mode.
    SetMode {
        /// The sub-patch tile.
        tile: u32,
        /// The new mode.
        mode: Mode,
    },
    /// Add a copy of the last member, named as a duplicate is.
    AddMember(u32),
    /// Remove this member tile from its layer or select.
    RemoveMember(u32),
    /// Move this member tile one place earlier (`up`) or later.
    MoveMember {
        /// The member tile.
        member: u32,
        /// True for earlier.
        up: bool,
    },
    /// Set a stack's copy count.
    SetCopies {
        /// The stack tile.
        tile: u32,
        /// 1 to 16.
        copies: u32,
    },
    /// Turn a stack's stereo spread on or off.
    SetStereoSpread {
        /// The stack tile.
        tile: u32,
        /// On or off.
        on: bool,
    },
    /// Set the spread of an exposed item, 0 to 1, as one undo step per
    /// slider drag.
    SetSpread {
        /// The stack tile.
        tile: u32,
        /// The exposed name.
        name: String,
        /// 0 to 1.
        amount: f32,
    },
}

impl App {
    /// Handles a mode gesture.
    pub(crate) fn mode_message(&mut self, message: Message) {
        match message {
            Message::SetMode { tile, mode } => self.set_mode(tile, mode),
            Message::AddMember(tile) => self.add_member(tile),
            Message::RemoveMember(member) => self.remove_member(member),
            Message::MoveMember { member, up } => self.move_member(member, up),
            Message::SetCopies { tile, copies } => {
                self.mode_edit("set Copies", |h, _| h.set_copies(ModuleId(tile), copies))
            }
            Message::SetStereoSpread { tile, on } => self.mode_edit("set stereo spread", |h, _| {
                h.set_stereo_spread(ModuleId(tile), on);
                Ok(())
            }),
            Message::SetSpread { tile, name, amount } => self.set_spread(tile, &name, amount),
        }
    }

    /// Runs an overlay edit on the modes as one undo step: a candidate,
    /// a sync, and a commit; a refusal goes to the status bar with its
    /// named text and changes nothing.
    fn mode_edit(
        &mut self,
        verb: &str,
        change: impl FnOnce(&mut loom_patch::Hierarchy, &loom_core::Graph) -> Result<(), String>,
    ) {
        self.close_slider_gesture();
        if self.session.edits.gesture_open() {
            return;
        }
        self.session.step_name = Some(verb.to_string());
        self.edit(|app| {
            let playing = app.session.hierarchy.playing_members(app.session.controller.graph());
            let mut candidate = app.session.hierarchy.clone();
            if let Err(why) = change(&mut candidate, app.session.controller.graph()) {
                app.alerts.warn(why);
                return;
            }
            match candidate.sync(&mut app.session.controller) {
                Ok(renamed) => {
                    app.session.hierarchy = candidate;
                    app.assignments_renumbered(&renamed);
                    app.keep_members_playing(&playing);
                    app.commit();
                }
                Err(e) => app.alerts.warn(format!("Cannot {verb}: {e}")),
            }
        });
    }

    /// Changes the mode of sub-patch tile `tile` (sub-patches.md, The editor
    /// for modes): a change alone never changes the sound.
    pub(crate) fn set_mode(&mut self, tile: u32, mode: Mode) {
        let registry = self.session.controller.registry().clone();
        let role_name = move |kind: &str| {
            registry
                .resolve_role(kind)
                .map_or_else(|_| kind.to_string(), |role| role.name.to_string())
        };
        self.mode_edit(&format!("make {}", mode.word()), |h, graph| {
            h.set_mode(graph, ModuleId(tile), mode, &role_name)
        });
    }

    /// Adds a copy of the last member of the layer or select tile `tile`,
    /// named as a duplicate is ("Saw 2"), as one undo step. A seventeenth
    /// is refused.
    pub(crate) fn add_member(&mut self, tile: u32) {
        let Some(&last) = self.session.hierarchy.members_of(ModuleId(tile)).last() else {
            return;
        };
        let layout = self.layout_fn();
        let Some(file) = self
            .session
            .hierarchy
            .whole_definition(self.session.controller.graph(), layout, last)
        else {
            return;
        };
        let level = self.session.hierarchy.path_of(ModuleId(tile));
        self.close_slider_gesture();
        self.session.step_name = Some("add member".to_string());
        self.edit(|app| {
            let mut candidate = app.session.hierarchy.clone();
            let (definition, nested) = file;
            let name = definition.metadata.name.clone();
            let inserted = match candidate.insert_definition(
                &mut app.session.controller,
                &level,
                definition,
                nested,
                None,
                None,
                Vec::new(),
            ) {
                Ok(inserted) => inserted,
                Err(e) => {
                    app.alerts.warn(e.to_string());
                    return;
                }
            };
            let taken: Vec<String> = candidate
                .members_of(ModuleId(tile))
                .into_iter()
                .filter(|m| *m != inserted.instance)
                .filter_map(|m| candidate.tile_name(m))
                .collect();
            let unique = loom_patch::unique_name(&name, |n| taken.iter().any(|t| t == n));
            let _ = candidate.rename(inserted.instance, &unique);
            match candidate.sync(&mut app.session.controller) {
                Ok(renamed) => {
                    app.session.hierarchy = candidate;
                    app.assignments_renumbered(&renamed);
                    app.place_beside(tile, inserted.instance.0);
                    app.commit();
                }
                Err(e) => {
                    for id in &inserted.modules {
                        let _ = app.session.controller.remove_module(*id);
                    }
                    app.alerts.warn(e.to_string());
                }
            }
        });
    }

    /// Places the new member tile `new` of tile `tile` to the right of the
    /// others, so members sit in a row inside (sub-patches.md, The editor
    /// for modes).
    fn place_beside(&mut self, tile: u32, new: u32) {
        let right = self
            .session
            .hierarchy
            .members_of(ModuleId(tile))
            .into_iter()
            .filter(|m| m.0 != new)
            .filter_map(|m| self.session.positions.get(&m.0).copied())
            .max_by(|a, b| a.0.total_cmp(&b.0));
        let at = right.map_or((0.0, 0.0), |(x, y)| (x + MEMBER_SPACING, y));
        self.session.positions.insert(new, at);
    }

    /// Removes member tile `member` from its layer or select, as one undo
    /// step; the last member stays.
    pub(crate) fn remove_member(&mut self, member: u32) {
        if let Some(why) = self.session.hierarchy.remove_refusal(ModuleId(member)) {
            self.alerts.warn(why);
            return;
        }
        self.close_slider_gesture();
        self.session.step_name = Some("remove member".to_string());
        self.edit(|app| {
            app.delete_tiles(&[member]);
        });
    }

    /// Moves member tile `member` one place, as one undo step.
    pub(crate) fn move_member(&mut self, member: u32, up: bool) {
        self.mode_edit("reorder members", |h, _| {
            if h.move_member(ModuleId(member), up) {
                Ok(())
            } else {
                Err(String::new())
            }
        });
    }

    /// Sets the spread of exposed item `name` of stack tile `tile`, one
    /// undo step per slider drag.
    fn set_spread(&mut self, tile: u32, name: &str, amount: f32) {
        self.close_pending_arrow_group();
        self.session.edits.start_slider_gesture();
        self.begin_edit();
        let mut candidate = self.session.hierarchy.clone();
        candidate.set_spread(ModuleId(tile), name, amount);
        match candidate.sync(&mut self.session.controller) {
            Ok(renamed) => {
                self.session.hierarchy = candidate;
                self.assignments_renumbered(&renamed);
                self.commit();
            }
            Err(e) => self.alerts.warn(format!("Cannot set the spread: {e}")),
        }
    }

    /// After an edit to a select's members: Member follows the member that
    /// played, so reordering or removing members never switches the sound
    /// (sub-patches.md, The editor for modes). Part of the edit's undo step.
    pub(crate) fn keep_members_playing(&mut self, before: &[loom_patch::hierarchy::PlayingMember]) {
        for (module, value) in self.session.hierarchy.members_kept_playing(before) {
            let index = self
                .session
                .controller
                .graph()
                .node(module)
                .and_then(|n| loom_core::ModuleNames::parameter_index(n, loom_patch::modes::MEMBER_PARAM));
            if let Some(index) = index {
                // A select inside a stack: every copy's Member follows.
                if let Err(error) = self.fan_out_param(module.0, index, value) {
                    self.alerts.warn(error.to_string());
                }
            }
        }
    }

    /// Writes copy 0 and every stack copy together (ADR-0040, invariant 9).
    /// Structural edits compile once and fail for the whole group. Ordinary
    /// edits use next-buffer commands. Seeds take each copy's offset.
    pub(crate) fn fan_out_param(&mut self, module: u32, index: usize, value: f32) -> Result<(), loom_core::LoomError> {
        let spec = self
            .session
            .controller
            .graph()
            .node(ModuleId(module))
            .and_then(|node| node.descriptor.params.get(index));
        let mut changes = vec![(ModuleId(module), index, value)];
        for &(copy, offset) in self.session.hierarchy.copies_of(ModuleId(module)) {
            let value = if let Some(spec) = spec.filter(|spec| spec.name == SEED_PARAM) {
                seeded(spec.min, spec.max, value, offset)
            } else {
                value
            };
            changes.push((copy, index, value));
        }
        self.session.controller.set_params_across(&changes)
    }

    /// Applies `edit` to the copies of assignment `id` onto the other copies
    /// of a stack: a depth, polarity, or on change reaches them at once.
    pub(crate) fn fan_out_assignment(
        &mut self,
        id: AssignmentId,
        edit: impl Fn(&mut loom_core::Controller, AssignmentId) -> Result<(), loom_core::LoomError>,
    ) {
        let copies = self
            .session
            .hierarchy
            .assignment_copies(self.session.controller.graph(), id);
        for copy in copies {
            let _ = edit(&mut self.session.controller, copy);
        }
    }

    /// Before a commit in a patch with modes: makes the copies and the added
    /// modules follow an edit made straight on the graph, such as a cable
    /// inside a stack, so the commit sends them together (ADR-0040,
    /// invariant 7). A patch with no mode pays nothing.
    pub(crate) fn sync_modes(&mut self) {
        // Nothing to do with no mode, or when the last sync saw this very
        // graph: a gesture that synced already pays only once.
        if !self.session.hierarchy.has_modes() || self.session.hierarchy.synced_with(&self.session.controller) {
            return;
        }
        let mut candidate = self.session.hierarchy.clone();
        match candidate.sync(&mut self.session.controller) {
            Ok(renamed) => {
                self.session.hierarchy = candidate;
                self.assignments_renumbered(&renamed);
            }
            Err(e) => self.alerts.warn(format!("The copies could not follow: {e}")),
        }
    }
}

#[cfg(test)]
mod tests;
