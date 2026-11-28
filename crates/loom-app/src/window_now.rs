//! What the window shows now, read from the runtime at the start of each
//! update: the canvas's live view, its typed field, and the field that has
//! the keys.
//!
//! On Iced these lived in its widgets, so the app asked the window and the
//! answer came on a later turn (window questions, gone in phase 4, P4).
//! The runtime holds them, and an update's context reads them, so the app
//! keeps them for the update that needs them: Tab between fields and a
//! shortcut read the focus in the key's own turn (ui-layer.md, section 6,
//! rule 6, step 6), and `locate` and the accessibility tree read where the
//! canvas is.

use crate::control::Found;
use crate::places::Places;
use crate::{App, Message};
use loom_canvas::{CanvasTyped, CanvasView};
use loom_ui_core::UiId;

/// What the window showed when the current update began.
#[derive(Debug, Clone, Default)]
pub(crate) struct WindowNow {
    /// The canvas and its camera, once it is laid out.
    pub(crate) canvas: Option<CanvasView>,
    /// The canvas's typed value field, while one is open.
    pub(crate) typed: Option<CanvasTyped>,
    /// The text field that has the keys, if one does.
    pub(crate) field: Option<UiId>,
    /// There is a window: the runtime holds the window's root panel. An
    /// update through `App::send`, in tests, has none, so nothing reads
    /// where a part is.
    pub(crate) window: bool,
}

impl App {
    /// Reads what the window shows from the update's context.
    pub(crate) fn read_window(&mut self, cx: &loom_ui::Context<'_, Message>) {
        self.window_now = WindowNow {
            canvas: crate::canvas::live_view(cx),
            typed: crate::canvas::live_typed(self, cx),
            field: cx.focused().filter(|_| cx.field_has_focus()).cloned(),
            window: cx.holds(&crate::chrome::window_id()),
        };
    }

    /// The text field that has the keys now.
    pub(crate) fn focused_field_now(&self) -> Option<&UiId> {
        self.window_now.field.as_ref()
    }

    /// Where the canvas and its typed field are now, for the tree.
    pub(crate) fn places_now(&self) -> Places {
        Places::live(self.window_now.canvas, self.window_now.typed.clone())
    }

    /// What `locate` starts from: the canvas and its places now. A part of
    /// the runtime's own is placed when the answer runs (`Found::find_ours`),
    /// since that waits for a current layout.
    pub(crate) fn found_now(&self) -> Found {
        Found {
            canvas: self.window_now.canvas,
            places: self.places_now(),
            ..Found::default()
        }
    }
}

#[cfg(test)]
mod tests;
