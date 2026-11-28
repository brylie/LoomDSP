//! Tests for the app as the runtime runs it (`runtime_app.rs`).

use crate::tests::app;

/// The app's window spec carries the title the window shows.
#[test]
fn the_app_describes_its_window() {
    let app = app();
    let spec = loom_ui::App::window(&app);
    assert_eq!(spec.title, app.title());
}

/// A screen reader's start, request, and departure reach the app as its
/// own messages, each as the adapter gave it (ui-layer.md, section 7,
/// rule 6).
#[test]
fn a_screen_readers_events_become_the_apps() {
    use crate::Message;
    use crate::a11y::Event;
    let app = app();
    let request = accesskit::ActionRequest {
        action: accesskit::Action::Click,
        target_tree: accesskit::TreeId::ROOT,
        target_node: accesskit::NodeId(7),
        data: None,
    };
    let heard = |event: loom_ui::AccessEvent| loom_ui::App::accessibility_event(&app, &event);
    assert!(matches!(
        heard(loom_ui::AccessEvent::Activated),
        Some(Message::A11y(Event::Activated))
    ));
    assert!(matches!(
        heard(loom_ui::AccessEvent::Action(request.clone())),
        Some(Message::A11y(Event::Action(got))) if got == request
    ));
    assert!(matches!(
        heard(loom_ui::AccessEvent::Deactivated),
        Some(Message::A11y(Event::Deactivated))
    ));
}
