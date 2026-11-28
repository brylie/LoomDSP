//! Tests for what the runtime hands the app about the window
//! (`subscriptions.rs`).

use super::*;

/// The close request becomes the app's close, so the window stays open
/// until the app has asked about unsaved work (files.md). It comes as the
/// runtime's window event, as the shell hands it over.
#[test]
fn the_close_request_is_the_apps_close() {
    assert!(matches!(
        window_message(&loom_ui::WindowEvent::CloseRequested),
        Some(Message::File(FileAction::Close))
    ));
}

/// A close request through the runtime, with unsaved changes, asks first
/// and keeps the window open.
#[test]
fn a_close_request_with_changes_asks_first() {
    let mut app = crate::tests::app();
    app.edit(|app| {
        app.session.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
    });
    let _ = crate::files::fake::asked();
    let mut ui = crate::ui_test::Ui::with_app(app);
    let messages = ui.window_event(loom_ui::WindowEvent::CloseRequested);
    assert!(
        messages.iter().any(|m| matches!(m, Message::File(FileAction::Close))),
        "{messages:?}"
    );
    assert!(ui.app().overlays.question.is_some(), "the unsaved-changes question");
    assert!(!ui.app().quit);
}

/// A close request through the runtime, with nothing unsaved, quits.
#[test]
fn a_close_request_with_no_changes_quits() {
    let mut ui = crate::ui_test::Ui::with_app(crate::tests::app());
    assert!(!ui.app().unsaved());
    let _ = ui.window_event(loom_ui::WindowEvent::CloseRequested);
    assert!(ui.app().overlays.question.is_none());
    assert!(ui.app().quit, "the app quits");
}

/// The other window events: a dropped file becomes its message, a new
/// scale a redraw, and the rest nothing.
#[test]
fn window_events_become_their_messages() {
    let path = std::path::PathBuf::from("/tmp/a.ron");
    assert!(matches!(
        window_message(&loom_ui::WindowEvent::FileDropped(path.clone())),
        Some(Message::FileDropped(found)) if found == path
    ));
    assert!(matches!(
        window_message(&loom_ui::WindowEvent::ScaleChanged(2.0)),
        Some(Message::WindowRescaled)
    ));
    assert!(window_message(&loom_ui::WindowEvent::Focused(true)).is_none());
    assert!(window_message(&loom_ui::WindowEvent::Hidden(true)).is_none());
}
