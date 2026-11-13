use super::*;
use loom_ui_core::UiId;

/// Every button has a name of its own, a UI ID that is `toolbar` and that
/// name, and a message for its press. A name the toolbar does not have
/// sends nothing.
#[test]
fn every_button_has_an_id_and_a_message() {
    let app = crate::tests::app();
    let model = app.toolbar_model();
    let mut names = std::collections::BTreeSet::new();
    for button in &model.buttons {
        assert!(names.insert(button.name), "{} is there twice", button.name);
        let id: UiId = button.id.parse().unwrap();
        assert_eq!(id, UiId::panel("toolbar").with(button.name));
        assert_eq!(id.to_string(), button.id, "the text reads back");
        assert!(message(button.name).is_some(), "{}", button.name);
        assert!(!button.label.is_empty());
    }
    for name in ["new", "open", "save", "save_as", "browse", "recent", "add_module"] {
        assert!(names.contains(name), "{name}");
    }
    for name in ["hold", "panic", "undo", "redo", "midi_learn", "preferences"] {
        assert!(names.contains(name), "{name}");
    }
    // At the top level there is no sub-patch to save to the library.
    assert!(!names.contains("save_to_library"));
    assert!(message("save_to_library").is_some());
    assert!(message("nonsense").is_none());
}

/// A button that cannot be pressed says why in its hover text, and a mode
/// button says whether its mode is on.
#[test]
fn a_disabled_button_says_why() {
    let mut app = crate::tests::app();
    let button = |app: &crate::App, name: &str| {
        app.toolbar_model()
            .buttons
            .into_iter()
            .find(|button| button.name == name)
            .unwrap()
    };
    let undo = button(&app, "undo");
    assert!(!undo.enabled);
    assert_eq!(undo.hover.as_deref(), Some("Nothing to undo"));
    assert_eq!(button(&app, "redo").hover.as_deref(), Some("Nothing to redo"));
    let hold = button(&app, "hold");
    assert_eq!(hold.enabled, hold.hover.is_none(), "disabled exactly when it says why");
    let recent = button(&app, "recent");
    assert_eq!(recent.enabled, !app.rig.recent.files().is_empty());
    assert!(recent.hover.is_some());

    assert_eq!(button(&app, "midi_learn").on, Some(false));
    let _ = app.send(Message::Learn(learn::Message::Toggle));
    assert_eq!(button(&app, "midi_learn").on, Some(true));
    assert_eq!(button(&app, "new").on, None);

    // One edit: a knob turned and let go.
    let module = app.session.controller.add_module("loom.filter.svf").unwrap().0;
    app.commit();
    let _ = app.send(Message::Canvas(loom_canvas::CanvasEvent::SetControl {
        module,
        widget: 0,
        position: 0.25,
        part: loom_canvas::ControlPart::Main,
    }));
    let _ = app.send(Message::SliderReleased);
    let undo = button(&app, "undo");
    assert!(undo.enabled);
    assert!(undo.hover.is_some_and(|hover| hover.starts_with("Undo: ")));
}

/// The snapshot carries the toolbar, with the fields a script reads and
/// without the ones only the window needs.
#[test]
fn the_snapshot_has_the_toolbar() {
    let app = crate::tests::app();
    let json = serde_json::to_value(app.ui_snapshot()).unwrap();
    let buttons = json["toolbar"]["buttons"].as_array().unwrap();
    assert_eq!(buttons.len(), app.toolbar_model().buttons.len());
    assert_eq!(buttons[0]["name"], "new");
    assert_eq!(buttons[0]["label"], "New");
    assert_eq!(buttons[0]["enabled"], true);
    assert!(buttons[0].get("id").is_none());
    assert!(buttons[0].get("group").is_none());
    assert!(buttons[0].get("hover").is_none(), "left out when there is none");
}
