//! Tests for the breadcrumb: its entries, IDs, and nodes, its clicks, and
//! where it sits in Tab's order.

use super::*;
use crate::ui_test::Ui;
use loom_ui_core::{Key, Modifiers};

/// The nested example, with its Voice sub-patch (top-level ID 2) open.
fn inside_voice() -> Ui {
    let mut ui = Ui::open("nested-subtractive.ron");
    ui.send(Message::OpenLevel(ModulePath::top(2)));
    ui
}

/// Inside a sub-patch the breadcrumb is "Patch", then the level, each
/// keyed as the key table says: `patch`, then the flat ID of the level's
/// instance. At the top level it has no entry and no panel.
#[test]
fn the_entries_are_keyed_by_flat_id() {
    let top = Ui::open("nested-subtractive.ron");
    assert!(top.app().crumbs().is_empty(), "no breadcrumb at the top level");
    let ui = inside_voice();
    let crumbs = ui.app().crumbs();
    let shown: Vec<(&str, bool)> = crumbs
        .iter()
        .map(|crumb| (crumb.text.as_str(), crumb.current))
        .collect();
    assert_eq!(shown, [("Patch", false), ("Voice", true)]);
    assert_eq!(
        ui.app().breadcrumb_stops(),
        [breadcrumb_id().with("patch"), breadcrumb_id().with(2u32)]
    );
}

/// The breadcrumb is a "Patch levels" group of buttons, after the
/// toolbar. Each offers Click and Focus; the open level's is marked
/// current, and says "Open now" in words for VoiceOver (ui-layer.md,
/// section 7).
#[test]
fn the_breadcrumb_is_a_group_with_the_open_level_marked() {
    let mut ui = inside_voice();
    let tree = ui.a11y_tree();
    let group = tree.node_by_id(&breadcrumb_id());
    assert_eq!(group.role(), accesskit::Role::Group);
    assert_eq!(group.name(), Some("Patch levels"));
    let children: Vec<Option<&UiId>> = tree.root().children().iter().map(|child| child.ui_id()).collect();
    assert_eq!(
        children.get(1).copied().flatten(),
        Some(&breadcrumb_id()),
        "the window's second child, after the toolbar"
    );
    let patch = tree.node_by_id(&breadcrumb_id().with("patch"));
    let voice = tree.node_by_id(&breadcrumb_id().with(2u32));
    for node in [&patch, &voice] {
        assert_eq!(node.role(), accesskit::Role::Button);
        assert!(node.offers(accesskit::Action::Click) && node.offers(accesskit::Action::Focus));
    }
    assert_eq!(patch.name(), Some("Patch"));
    assert!(!patch.is_current() && patch.description().is_none());
    assert_eq!(voice.name(), Some("Voice"));
    assert!(voice.is_current());
    assert_eq!(voice.description(), Some("Open now"));
}

/// A click on "Patch" opens the top level, and the breadcrumb goes.
#[test]
fn a_click_on_patch_opens_the_top_level() {
    let mut ui = inside_voice();
    ui.click_text("Patch");
    assert!(ui.app().session.level.is_root());
    assert!(ui.app().crumbs().is_empty());
}

/// A click on the open level's entry changes nothing the user can see
/// beyond a redraw, as on main: the level, the selection, the camera, and
/// the undo history all stay.
#[test]
fn a_click_on_the_open_level_changes_nothing() {
    let mut ui = inside_voice();
    let module = ui.app().ui_snapshot().tiles[0].id;
    let _ = ui.deliver(Message::Canvas(loom_canvas::CanvasEvent::Select(vec![module])));
    ui.redraw();
    let level = ui.app().session.level.clone();
    let selection = ui.app().session.selection.clone();
    let camera = ui.canvas().camera;
    let history = (ui.app().undo_hint(), ui.app().redo_hint());
    let messages = ui.click_text("Voice");
    assert!(
        messages.iter().any(|m| matches!(m, Message::OpenLevel(_))),
        "the click reached the app"
    );
    assert_eq!(ui.app().session.level, level);
    assert_eq!(ui.app().session.selection, selection, "the selection stays");
    assert_eq!(ui.canvas().camera, camera, "the camera stays");
    assert_eq!((ui.app().undo_hint(), ui.app().redo_hint()), history, "no undo step");
}

/// Tab meets the toolbar as one stop, then the breadcrumb's entries, one
/// stop each: Tab from any toolbar button goes to the first entry. Past
/// the last entry it goes into the canvas, which takes it as Tab from
/// nothing (ui-layer.md, section 6, rule 3, from row 2; the WAI-ARIA
/// toolbar pattern, Brylie's decision, 2026-11-19).
#[test]
fn tab_reaches_the_breadcrumb_after_the_toolbar_then_the_canvas() {
    let mut ui = inside_voice();
    let top = ui.app().top_stops();
    let crumbs = ui.app().breadcrumb_stops();
    assert_eq!(top[0], ui.app().toolbar_stops()[0], "the toolbar's one stop");
    assert_eq!(&top[1..], crumbs.as_slice());
    let button = ui.app().toolbar_stops().last().cloned().expect("a toolbar button");
    ui.a11y_action(&button, accesskit::Action::Focus);
    ui.tap(Key::Tab, Modifiers::NONE);
    assert_eq!(ui.runtime().focused(), Some(&crumbs[0]), "from any toolbar button");
    let last = top.last().cloned().expect("a top run");
    ui.a11y_action(&last, accesskit::Action::Focus);
    assert_eq!(ui.runtime().focused(), Some(&last));
    ui.tap(Key::Tab, Modifiers::NONE);
    assert_eq!(ui.runtime().focused(), None, "out of the top run");
    assert!(
        ui.app().a11y.focus().is_some() || !ui.app().session.selection.is_empty(),
        "the canvas took it"
    );
}
