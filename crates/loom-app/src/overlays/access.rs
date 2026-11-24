//! The overlay that owns the input, as a screen reader is told it: its
//! [`Item`]s, built from the same models its view draws (ADR-0021), with
//! the same UI IDs and messages (`docs/design/ui-layer.md`, section 7,
//! rule 27).
//!
//! Only the overlay that owns the input is reported. What it covers, the
//! window and any overlay under it, stays hidden (rule 24). Every overlay
//! is reported: the question, help, the menus, the browsers, Preferences,
//! and the licence chooser.

use crate::App;
use crate::a11y::items::Item;
use crate::input_owner::InputOwner;
use loom_ui_core::UiId;

/// The UI ID of the open question.
pub(crate) fn question_id() -> UiId {
    UiId::panel("question")
}

/// The UI ID of one of the question's answers, by the name the control
/// channel uses for it.
pub(crate) fn answer_id(name: &str) -> UiId {
    question_id().with(name)
}

/// The UI ID of the open help page.
pub(crate) fn help_id() -> UiId {
    UiId::panel("help")
}

impl App {
    /// The overlay that owns the input, as items, and where its keys act
    /// while no text field has them. Empty while the canvas has the input,
    /// or while an overlay not yet reported has it.
    pub(crate) fn overlay_items(&self) -> (Vec<Item>, Option<UiId>) {
        match self.input_owner() {
            // The question is the runtime's (row 3b): its nodes come from
            // its panel, and its focus is the runtime's on an answer.
            InputOwner::Question => (Vec::new(), None),
            // Help is the runtime's (row 6): its nodes come from its panel.
            InputOwner::Help => (Vec::new(), None),
            // The menus are the runtime's (rows 3b and 3d).
            InputOwner::TileMenu | InputOwner::ProbeMenu | InputOwner::RecentList => (Vec::new(), None),
            InputOwner::Browser => match self.overlays.browser() {
                Some(open) => {
                    let (item, focus) = crate::browser::access::items(open.model());
                    (vec![item], focus)
                }
                None => (Vec::new(), None),
            },
            InputOwner::Patches => match self.overlays.patches() {
                Some(open) => (vec![crate::patches::access::items(open.model())], None),
                None => (Vec::new(), None),
            },
            // No part of these has the keys until a field does, so the
            // dialog itself has the focus, as help does.
            InputOwner::Preferences => match self.preferences_model() {
                Some(model) => (
                    vec![crate::settings::access::items(&model)],
                    Some(crate::settings::access::preferences_id()),
                ),
                None => (Vec::new(), None),
            },
            // The licence chooser is the runtime's (row 3c).
            InputOwner::Chooser => (Vec::new(), None),
            InputOwner::Canvas => (Vec::new(), None),
        }
    }
}
