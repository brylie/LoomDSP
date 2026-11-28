//! What the toolbar's disabled buttons say. The window itself is the
//! runtime's: its root panel (`chrome.rs`), with the canvas in a slot of it
//! (`canvas.rs`).

use crate::App;

impl App {
    /// Hold note's hover text: why it is disabled, if it is.
    pub(crate) fn hold_hover(&self) -> Option<&'static str> {
        self.session.engine.midi.is_none().then_some(HOLD_DISABLED)
    }

    /// Recent's hover text, which says why while it is disabled.
    pub(crate) fn recent_hover(&self) -> &'static str {
        if self.rig.recent.files().is_empty() {
            RECENT_DISABLED
        } else {
            "The last patches you opened or saved"
        }
    }
}

/// Hold note's hover text while it is disabled. The control channel refuses
/// the press with the same words.
pub(crate) const HOLD_DISABLED: &str = "Hold note is disabled: no audio is playing the engine";

/// Recent's hover text while it is disabled.
const RECENT_DISABLED: &str = "Recent is disabled: no patches opened or saved yet";
