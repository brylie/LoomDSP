//! Text typed for a value and not committed yet: the rules both value
//! fields share, the one over a canvas control and the one in the inspector
//! (`docs/design/panel-widgets.md`, Typing a value).
//!
//! Each field keeps the rest itself: the canvas its caret and selection, and
//! the inspector the Iced text input that holds them. What they share is what
//! decides a commit:
//!
//! - **Unchanged text commits nothing.** The shown value text is rounded, so
//!   committing it as typed would move the value.
//! - **Bad text stays, with its reason**, and Enter does nothing, so a typo
//!   never becomes a value.
//! - Escape, or focus moving away, drops the draft. That is the field's own
//!   business: it simply forgets the draft.

/// Text typed for a value, with the reason it is not one yet, if it is not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypedDraft {
    /// The text as typed.
    pub text: String,
    /// Why the text is not a value, kept from the last check.
    error: Option<String>,
}

/// What Enter does with a draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftSubmit {
    /// The text is the value as shown: close the field with no change.
    Unchanged,
    /// The text is not a value: keep the field open, to be corrected.
    Blocked,
    /// Commit the text.
    Commit,
}

impl TypedDraft {
    /// A draft holding `text`, checked with `check`, which says why a text is
    /// not a value.
    pub fn new(text: String, check: impl FnOnce(&str) -> Result<(), String>) -> Self {
        let mut draft = Self { text, error: None };
        draft.recheck(check);
        draft
    }

    /// Checks the text again after it changed.
    pub fn recheck(&mut self, check: impl FnOnce(&str) -> Result<(), String>) {
        self.error = check(&self.text).err();
    }

    /// Why the text is not a value, if it is not.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// What Enter does. `baseline` is the value text that counts as no
    /// change: the canvas passes the text its field opened with, and the
    /// inspector the value as it shows now.
    pub fn submit(&self, baseline: Option<&str>) -> DraftSubmit {
        if baseline == Some(self.text.as_str()) {
            DraftSubmit::Unchanged
        } else if self.error.is_some() {
            DraftSubmit::Blocked
        } else {
            DraftSubmit::Commit
        }
    }
}

#[cfg(test)]
mod tests;
