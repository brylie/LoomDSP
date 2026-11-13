use super::*;
use loom_ui_core::UiId;

/// Every item's UI ID is `stats` and its name, and the fixed IDs of the
/// status line read back as UI IDs.
#[test]
fn every_item_has_an_id() {
    let mut app = crate::tests::app();
    let _ = app.send(crate::Message::Learn(crate::learn::Message::Toggle));
    let model = app.bars_model();
    let names: Vec<&str> = model.items.iter().map(|item| item.name).collect();
    for name in ["patch", "audio", "midi", "learning"] {
        assert!(names.contains(&name), "{name} in {names:?}");
    }
    for item in &model.items {
        let id: UiId = item.id.parse().unwrap();
        assert_eq!(id, UiId::panel("stats").with(item.name));
        assert!(!item.text.is_empty());
    }
    for text in [
        STATUS_ID,
        STATUS_DISMISS_ID,
        KEPT_ID,
        KEPT_DISMISS_ID,
        MEASURE_ID,
        MOVE_CAMERA_ID,
    ] {
        let id: UiId = text.parse().unwrap();
        assert_eq!(id.to_string(), text);
    }
}

/// The bar says what the patch holds and the status as the window shows
/// it. Learn mode adds its word, and a warning turns the status amber.
#[test]
fn the_bar_follows_the_app() {
    let mut app = crate::tests::app();
    let patch = |app: &crate::App| app.bars_model().items[0].text.clone();
    let before = app.session.controller.graph().len();
    assert_eq!(patch(&app), format!("{before} modules, 0 cables"));
    let _ = app.session.controller.add_module("loom.filter.svf").unwrap();
    assert_eq!(patch(&app), format!("{} modules, 0 cables", before + 1));
    assert!(!app.bars_model().items.iter().any(|item| item.name == "learning"));

    app.alerts.say("Saved");
    let status = app.bars_model().status;
    assert_eq!(status.severity, alerts::Severity::Normal);
    assert!(status.shown.contains("Saved"));
    assert_eq!(status.hover, status.shown);
    assert_eq!(status.dismiss, None);
    app.alerts.warn("Careful");
    let status = app.bars_model().status;
    assert_eq!(status.severity, alerts::Severity::Warning);
    assert_eq!(status.dismiss_hover(), "Dismiss this warning");
    assert_eq!(
        Field {
            severity: alerts::Severity::Alarm,
            ..status
        }
        .dismiss_hover(),
        "Dismiss this error"
    );
}

/// The developer switches show only with the developer tools, and their
/// frame text is steady while nothing is measured.
#[test]
fn the_developer_switches_show_with_the_tools() {
    let mut app = crate::tests::app();
    app.dev_tools = false;
    assert_eq!(app.bars_model().dev, None);
    app.dev_tools = true;
    let dev = app.bars_model().dev.unwrap();
    assert_eq!((dev.measuring, dev.animating), (app.measuring, app.animating));
    if !app.measuring {
        assert_eq!(dev.frames, "redraws on change");
    }
}
