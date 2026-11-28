//! `loom-ui` panels a test adds to the window, to check routing and drawing
//! between the app's own panels and a test's (plan for leaving Iced, step
//! 3.1). Empty outside tests, so the app shows no extra panel.

use crate::Message;
use loom_ui_core::UiId;

/// A test's panels: what they show and what a click on one means.
pub(crate) struct TestPanels {
    /// Adds the panels to the view.
    pub(crate) view: fn(&mut loom_ui::View),
    /// The message for a click on one of their elements.
    pub(crate) clicked: fn(&UiId) -> Option<Message>,
    /// The element a message moves the focus to, by a focus effect after
    /// the app's own, as one of the app's panels would ask.
    pub(crate) focuses: fn(&Message) -> Option<UiId>,
    /// The message for an event on one of their text fields, as a view's
    /// own mapping gives it (`runtime_fields.rs`).
    pub(crate) fields: fn(&UiId, &loom_ui::FieldEvent) -> Option<Message>,
}
