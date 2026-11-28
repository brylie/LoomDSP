//! Who has the input over the window.
//!
//! Every event reaches the runtime first, which routes it by section 6,
//! rule 6 of the UI layer spec: Panic, always heard (`App::always_key`,
//! `runtime_app.rs`), then the open question, the elements and fields, the
//! open overlay's keys, the canvas as the window's home element, and last
//! the app's shortcuts for a key nothing took (`App::unclaimed_key`). The
//! app's side of it is here: who owns the input, the overlay's keys, and
//! what the canvas may take.

use crate::{App, settings};
use loom_ui_core::{Key, KeyEvent};

/// Who the keys and clicks over the window belong to. When two are open, the
/// earlier one has the input. From `Question` down to `Preferences` this is the
/// order the view draws them, top layer first. The rest are small and open
/// over the canvas, and do not normally share the screen.
///
/// To add an overlay, add it here and to [`Overlays`](crate::overlays::Overlays):
/// a panel is one more [`Overlay`](crate::overlays::Overlay) variant, and the
/// compiler points to `Overlays::owner` and [`App::overlay_key`]. The canvas, the
/// clipboard keys, and key handling then follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputOwner {
    /// A question must be answered first.
    Question,
    /// The help overlay.
    Help,
    /// The licence chooser, over Preferences or the inspector.
    Chooser,
    /// The Your patches browser.
    Patches,
    /// The Preferences window.
    Preferences,
    /// The module browser.
    Browser,
    /// The Recent list.
    RecentList,
    /// The probe menu on an output.
    ProbeMenu,
    /// The tile's context menu.
    TileMenu,
    /// Nothing is over the canvas.
    Canvas,
}

impl App {
    /// Who has the input now.
    pub(crate) fn input_owner(&self) -> InputOwner {
        self.overlays.owner()
    }

    /// Whether the canvas owns input, including its editor and shortcuts.
    pub(crate) fn no_overlay(&self) -> bool {
        self.input_owner() == InputOwner::Canvas
    }

    /// True while a pointer press reaches the window's own parts: a tile
    /// and its controls, the toolbar, the right panel's tabs, and the stats
    /// bar. That is only while nothing is open. Every panel and menu the
    /// view draws puts a layer over the whole window (`view.rs`), either
    /// to close itself on a press outside or to hold the press, so the
    /// press never reaches what is below. The small menus on the canvas do
    /// this too.
    ///
    /// A screen reader follows the pointer: what a press cannot reach is
    /// hidden from it and refuses its actions (ui-layer.md, section 7,
    /// rule 24). A test presses under every panel and checks that the tree
    /// agrees (`a11y::tests::the_tree_follows_the_pointer_under_every_panel`).
    pub(crate) fn pointer_reaches_window(&self) -> bool {
        self.input_owner() == InputOwner::Canvas
    }

    /// Who else has the keyboard, for the canvas: the overlays, the matrix,
    /// and the app's text fields. One place decides it, so the canvas never
    /// infers it from loose flags.
    pub(crate) fn canvas_focus_policy(&self) -> loom_canvas::CanvasFocusPolicy {
        loom_canvas::CanvasFocusPolicy {
            interactive: self.input_owner() == InputOwner::Canvas,
            // The matrix keeps no flag: while the runtime's focus is on one
            // of its parts, the canvas reports a press on it
            // (`PointerDown`), which gives the keys back to the canvas.
            matrix_keyboard: self.focus_in_matrix()
                || self.session.outline.active
                || self.toolbar_has_focus()
                || self.session.outline.inspector_action.is_some(),
            jump_keyboard: self.session.outline.has_jump(),
            text_field_keys: self.focus.is_some(),
            text_field_keeps_tab: self.text_field_tabs(self.focus.as_ref()),
        }
    }

    /// A key no widget used while an overlay has the input. An overlay's keys
    /// stop here, so none reaches the canvas or a window behind it.
    pub(crate) fn overlay_key(&mut self, event: &KeyEvent) {
        let KeyEvent::Pressed { key, modifiers, .. } = event else {
            return;
        };
        let escape = *key == Key::Escape;
        match self.input_owner() {
            // These have listeners or widgets of their own, and the menus
            // their own keys (`menus.rs`), which `update` gives them first.
            InputOwner::Question
            | InputOwner::Help
            | InputOwner::Patches
            | InputOwner::Browser
            | InputOwner::RecentList
            | InputOwner::ProbeMenu
            | InputOwner::TileMenu
            | InputOwner::Canvas => {}
            InputOwner::Chooser if escape => self.overlays.chooser = None,
            InputOwner::Chooser => {}
            InputOwner::Preferences => match key {
                Key::Escape => self.preferences(settings::Message::Close),
                Key::Character(',') if modifiers.shortcut() && !modifiers.shift => self.toggle_preferences(),
                _ => {}
            },
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
