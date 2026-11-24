//! The app's shortcuts that act on the patch: undo and redo, and those on
//! the canvas selection (ui-layer.md, section 6, rule 6).
//!
//! Each waits for one check: whether a text field has focus, ours or
//! Iced's, single-line or multi-line. While one has, the shortcut does
//! nothing, and the field keeps passing the key on: the user is typing, and
//! the shortcut would change the patch behind the field (Brylie's decision,
//! 2026-11-19). The check asks the window, after the key's turn, so a key
//! right after a click into a field sees the field (`window_question.rs`).
//!
//! Shortcuts for the whole window stay global and are not here: New, Open,
//! Save, Save As, and Preferences.

/// A shortcut that acts on the patch, waiting for the text-focus check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shortcut {
    /// Cmd+Z or Ctrl+Z.
    Undo,
    /// Shift with Cmd+Z or Ctrl+Z, and Ctrl+Y off a Mac.
    Redo,
    /// Cmd+Enter or Ctrl+Enter: open the selected sub-patch.
    OpenSelected,
    /// Cmd+G or Ctrl+G: make a sub-patch from the selection.
    MakeSubpatch,
    /// Shift with Cmd+G or Ctrl+G: dissolve the selected sub-patch.
    DissolveSubpatch,
    /// Cmd+D or Ctrl+D: duplicate the selection.
    Duplicate,
    /// Cmd+C or Ctrl+C: copy the selection.
    Copy,
    /// Cmd+X or Ctrl+X: cut the selection.
    Cut,
}

impl Shortcut {
    /// The copy, cut, or duplicate shortcut for `key`, the letter a
    /// clipboard key names.
    pub(crate) fn clipboard(key: char) -> Option<Self> {
        match key {
            'c' => Some(Self::Copy),
            'x' => Some(Self::Cut),
            'd' => Some(Self::Duplicate),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
