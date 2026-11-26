//! Text still being typed when the patch is saved (`docs/design/files.md`,
//! Asking before losing work, Text still being typed; #897).
//!
//! Three fields hold a draft, text typed but not yet in the patch: an
//! inspector value field, patch info's Tags field, and a MIDI mapping's Low
//! and High pair. A save commits each one first, as Enter would, so the file
//! holds what the user typed. A draft that cannot commit stops the save, and
//! its field gets the focus with its reason. An open draft also counts as
//! unsaved, so the title's mark and the unsaved-changes question agree.
//!
//! Commits happen only here, at a save. `App::finish_edits` never commits a
//! draft, because an audio restart calls it too, and that must not change
//! the patch under the user's typing. Autosave never commits one either.

use crate::App;
use loom_core::ModuleId;
use loom_ui_core::UiId;

/// A draft that could not commit, so the save stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refused {
    /// The field that holds the draft, when the window shows it.
    pub field: Option<UiId>,
    /// The field's name, as the status line says it.
    pub label: String,
    /// Why the text cannot commit.
    pub reason: String,
}

impl Refused {
    /// What the status line says: `Not saved: <field>: <reason>`.
    pub(crate) fn status(&self) -> String {
        format!("Not saved: {}: {}", self.label, self.reason)
    }
}

impl App {
    /// True when a field holds text typed but not yet committed. Blank text
    /// in the Tags field is not a draft, since it would add nothing.
    pub(crate) fn has_draft(&self) -> bool {
        self.session.value_draft.is_some()
            || self.session.learn.range_draft_param().is_some()
            || !self.session.patch_info.tag_draft.trim().is_empty()
    }

    /// Commits every open draft, as Enter would. A value and a range each
    /// become one undo step; a tag goes into the metadata, which undo does
    /// not cover (patch-metadata.md). A field that still has the focus
    /// shows the committed text. Stops at the first draft that cannot
    /// commit, which stays in its field with its reason.
    ///
    /// The focus is in one field at a time, and leaving a field commits or
    /// drops its draft, so at most one draft is open in practice.
    pub(crate) fn commit_drafts(&mut self) -> Result<(), Refused> {
        self.commit_value_draft()?;
        self.commit_range_draft()?;
        self.commit_tag_draft()
    }

    /// Commits the drafts before a save. If one cannot commit, nothing is
    /// to be written: the focus goes to its field, the status line says
    /// why, and this returns false.
    pub(crate) fn ready_to_save(&mut self) -> bool {
        self.finish_edits();
        match self.commit_drafts() {
            Ok(()) => true,
            Err(refused) => {
                self.refuse_save(&refused);
                false
            }
        }
    }

    /// Shows why a save stopped on a draft: the focus on its field, and the
    /// reason in the status line.
    fn refuse_save(&mut self, refused: &Refused) {
        if let Some(field) = &refused.field {
            self.effect(loom_ui::Effects::focus(field.clone()));
        }
        self.alerts.warn(refused.status());
    }

    /// Commits the value field's draft. After a commit, a field that still
    /// has the focus selects its new text, as after Enter.
    fn commit_value_draft(&mut self) -> Result<(), Refused> {
        let Some((module, index)) = self.session.value_draft.as_ref().map(|d| (d.module, d.index)) else {
            return Ok(());
        };
        let field = self.value_field_part(module, index);
        self.submit_value_draft(module, index);
        if let Some(left) = &self.session.value_draft {
            return Err(Refused {
                field,
                label: self.param_label(module, index),
                reason: left.draft.error().unwrap_or("not a value").to_string(),
            });
        }
        if let Some(part) = field.filter(|part| self.focus.as_ref() == Some(part)) {
            self.effect(loom_ui::Effects::focus_and_select_all(part));
        }
        Ok(())
    }

    /// Commits the MIDI range pair's draft.
    fn commit_range_draft(&mut self) -> Result<(), Refused> {
        let Some(param) = self.session.learn.range_draft_param() else {
            return Ok(());
        };
        let field = self.midi_range_fields().map(|[low, _]| low);
        self.submit_midi_range(param.0, param.1);
        match self.session.learn.range_draft(param) {
            None => Ok(()),
            Some(left) => Err(Refused {
                field,
                label: "MIDI range".to_string(),
                reason: left.error.clone().unwrap_or_else(|| "not a range".to_string()),
            }),
        }
    }

    /// Adds the Tags field's draft as a tag. A refused tag stays in the
    /// field, with its reason as the field's note.
    fn commit_tag_draft(&mut self) -> Result<(), Refused> {
        let draft = self.session.patch_info.tag_draft.clone();
        match self.try_add_tag(&draft) {
            Ok(()) => {
                self.session.patch_info.tag_draft.clear();
                self.session.patch_info.tag_note = None;
                Ok(())
            }
            Err(reason) => {
                self.session.patch_info.tag_note = Some(reason.clone());
                Err(Refused {
                    field: Some(crate::patch_info::tags_part()),
                    label: "Tags".to_string(),
                    reason,
                })
            }
        }
    }

    /// The label of `module`'s parameter `index`, or "Value" when it has
    /// none to show.
    fn param_label(&self, module: u32, index: usize) -> String {
        self.session
            .controller
            .graph()
            .node(ModuleId(module))
            .and_then(|node| node.descriptor.params.get(index))
            .map_or_else(|| "Value".to_string(), |spec| spec.label.to_string())
    }
}

#[cfg(test)]
mod tests;
