//! Tests for the module browser as the runtime's panels (plan 3.2, row 5;
//! module-browser.md, Focus and Tab).

use super::super::{HEIGHT, LibraryItem, Message, WIDTH, browser_id, insert_linked_id, list_id, menu_id, search_id};
use crate::ui_test::Ui;
use loom_ui_core::{Key, Modifiers, Point, Rect, UiId};

/// The app with the browser open, laid out.
fn open() -> Ui {
    let mut ui = Ui::open("subtractive.ron");
    ui.send(crate::Message::OpenBrowser);
    let _ = ui.redraw();
    ui
}

/// The app with the browser open over a library of two sub-patches, and
/// the query "pad", so the library row `Pad.ron` is the first.
fn with_library() -> Ui {
    let mut ui = open();
    let item = |library: &str, name: &str| LibraryItem {
        path: std::path::PathBuf::from(library),
        library: library.to_string(),
        name: name.to_string(),
        roles: Vec::new(),
    };
    ui.send(crate::Message::Browser(Message::Library(vec![
        item("Pad.ron", "Pad"),
        item("Voices/Bass voice.ron", "Bass voice"),
    ])));
    ui.typewrite("pad");
    let _ = ui.redraw();
    ui
}

/// The UI ID of the library row for `path`.
fn library_row(path: &str) -> UiId {
    list_id().with("library").with(path)
}

/// The browser's model.
fn model(ui: &Ui) -> &super::super::Model {
    ui.app().overlays.browser().expect("the browser is open").model()
}

/// The browser's highlighted row.
fn highlight(ui: &Ui) -> Option<usize> {
    model(ui).highlight
}

/// Where the runtime laid out `id`, in the window.
fn bounds(ui: &mut Ui, id: &UiId) -> Rect {
    ui.lay_out_panels();
    ui.runtime().bounds(id).unwrap_or_else(|| panic!("{id} is laid out"))
}

/// The centre of `id`.
fn centre(ui: &mut Ui, id: &UiId) -> Point {
    let at = bounds(ui, id);
    Point::new(at.x + at.width / 2.0, at.y + at.height / 2.0)
}

/// A press and a release of `button` where the pointer is.
fn click(ui: &mut Ui, button: loom_ui_core::PointerButton) {
    ui.step(&[crate::ui_test::Input::pointer(loom_ui_core::PointerEvent::Pressed(
        button,
    ))]);
    ui.step(&[crate::ui_test::Input::pointer(loom_ui_core::PointerEvent::Released(
        button,
    ))]);
}

/// Presses `key` with no modifier.
fn tap(ui: &mut Ui, key: Key) {
    let _ = ui.tap(key, Modifiers::NONE);
}

/// Opened, the search field has the runtime's focus, and what is typed is
/// the query.
#[test]
fn the_search_field_has_the_focus_and_takes_typing() {
    let mut ui = open();
    assert_eq!(ui.runtime().focused(), Some(&search_id()));
    ui.typewrite("svf");
    assert_eq!(model(&ui).query, "svf");
}

/// Up and Down move the highlight and never the caret: the field leaves
/// them to the browser before it edits (the WAI-ARIA combobox). The
/// highlight stops at the ends.
#[test]
fn up_and_down_move_the_highlight_not_the_caret() {
    let mut ui = open();
    tap(&mut ui, Key::Down);
    tap(&mut ui, Key::Down);
    assert_eq!(highlight(&ui), Some(2));
    tap(&mut ui, Key::Up);
    tap(&mut ui, Key::Up);
    tap(&mut ui, Key::Up);
    assert_eq!(highlight(&ui), Some(0), "it stops at the top");
    assert_eq!(ui.runtime().focused(), Some(&search_id()));
}

/// Tab and Shift+Tab stay on the search field, the dialog's only stop.
#[test]
fn tab_stays_on_the_search_field() {
    let mut ui = open();
    tap(&mut ui, Key::Tab);
    assert_eq!(ui.runtime().focused(), Some(&search_id()));
    let _ = ui.tap(Key::Tab, Modifiers::SHIFT);
    assert_eq!(ui.runtime().focused(), Some(&search_id()));
}

/// Enter adds the highlighted module and closes the browser.
#[test]
fn enter_adds_the_highlighted_module() {
    let mut ui = open();
    let before = ui.app().session.controller.graph().nodes().count();
    ui.typewrite("svf");
    tap(&mut ui, Key::Enter);
    assert!(ui.app().overlays.browser().is_none(), "the browser closed");
    assert_eq!(ui.app().session.controller.graph().nodes().count(), before + 1);
}

/// The panel is `WIDTH` by `HEIGHT`, the size the canvas keeps it inside
/// by, so it is never cut at the canvas's edge.
#[test]
fn the_panel_is_as_big_as_the_canvas_expects() {
    let mut ui = open();
    let panel = bounds(&mut ui, &browser_id());
    assert_eq!((panel.width, panel.height), (WIDTH, HEIGHT));
}

/// The pointer moving onto a row highlights it, and only highlights it:
/// nothing is added, and the next key moves on from that row.
#[test]
fn hover_highlights_a_row_and_keys_go_on_from_it() {
    let mut ui = open();
    let third = model(&ui).rows[3].id.clone();
    let at = centre(&mut ui, &third);
    let before = ui.app().session.controller.graph().nodes().count();
    ui.move_to(at);
    assert_eq!(highlight(&ui), Some(3));
    assert!(ui.app().overlays.browser().is_some(), "nothing was chosen");
    assert_eq!(ui.app().session.controller.graph().nodes().count(), before);
    tap(&mut ui, Key::Down);
    assert_eq!(highlight(&ui), Some(4), "the key goes on from the hovered row");
}

/// A click on a row adds it.
#[test]
fn a_click_on_a_row_adds_it() {
    let mut ui = open();
    let before = ui.app().session.controller.graph().nodes().count();
    let second = model(&ui).rows[1].id.clone();
    let at = centre(&mut ui, &second);
    ui.move_to(at);
    click(&mut ui, loom_ui_core::PointerButton::Left);
    assert!(ui.app().overlays.browser().is_none());
    // The row may be a built-in sub-patch, which adds its modules too.
    assert!(ui.app().session.controller.graph().nodes().count() > before);
}

/// Down past the last row in view scrolls the list just enough to show the
/// highlighted row, even when the same update made the rows.
#[test]
fn the_highlighted_row_scrolls_into_view() {
    let mut ui = open();
    ui.typewrite("o");
    for _ in 0..14 {
        tap(&mut ui, Key::Down);
    }
    let _ = ui.redraw();
    let row = model(&ui).rows[highlight(&ui).unwrap()].id.clone();
    let list = bounds(&mut ui, &list_id());
    let at = bounds(&mut ui, &row);
    assert!(
        at.y >= list.y - 0.5 && at.y + at.height <= list.y + list.height + 0.5,
        "{at:?} shows in {list:?}"
    );
}

/// A right press on a library row opens its menu with no item highlighted;
/// Down highlights Insert linked, which is then the field's active
/// descendant, and Enter inserts the row linked. The focus stays in the
/// field throughout.
#[test]
fn a_right_press_opens_a_library_rows_menu() {
    let mut ui = with_library();
    let row = library_row("Pad.ron");
    let at = centre(&mut ui, &row);
    ui.move_to(at);
    click(&mut ui, loom_ui_core::PointerButton::Right);
    assert_eq!(ui.runtime().focused(), Some(&search_id()), "the press kept the focus");
    assert_eq!(model(&ui).menu, Some(0));
    assert_eq!(model(&ui).menu_highlight, None, "opened by the pointer");
    let _ = bounds(&mut ui, &insert_linked_id(&row));
    tap(&mut ui, Key::Down);
    assert_eq!(model(&ui).menu_highlight, Some(0));
    assert_eq!(ui.runtime().focused(), Some(&search_id()));
    let tree = ui.a11y_tree();
    let field = tree.node_by_id(&search_id());
    assert_eq!(field.active_descendant(), Some(&insert_linked_id(&row)));
    tap(&mut ui, Key::Enter);
    assert!(ui.app().overlays.browser().is_none(), "inserted, and closed");
}

/// The menu key opens the highlighted library row's menu on its item, so
/// Enter inserts it linked at once; Escape closes the menu first, then the
/// browser.
#[test]
fn the_menu_key_opens_the_menu_and_escape_closes_it_first() {
    let mut ui = with_library();
    tap(&mut ui, Key::ContextMenu);
    assert_eq!((model(&ui).menu, model(&ui).menu_highlight), (Some(0), Some(0)));
    tap(&mut ui, Key::Escape);
    assert_eq!(model(&ui).menu, None, "the menu closed");
    assert!(ui.app().overlays.browser().is_some(), "the browser stays");
    let _ = ui.tap(Key::F10, Modifiers::SHIFT);
    assert_eq!(model(&ui).menu, Some(0), "Shift+F10 opens it too");
    tap(&mut ui, Key::Escape);
    tap(&mut ui, Key::Escape);
    assert!(ui.app().overlays.browser().is_none());
}

/// A press on the browser outside an open menu closes only the menu; a
/// press outside both closes both.
#[test]
fn a_press_outside_the_menu_closes_it_and_outside_both_closes_both() {
    let mut ui = with_library();
    tap(&mut ui, Key::ContextMenu);
    let field = centre(&mut ui, &search_id());
    ui.move_to(field);
    click(&mut ui, loom_ui_core::PointerButton::Left);
    assert_eq!(model(&ui).menu, None, "the menu closed");
    assert!(ui.app().overlays.browser().is_some(), "the browser stays");
    tap(&mut ui, Key::ContextMenu);
    let panel = bounds(&mut ui, &browser_id());
    ui.move_to(Point::new(panel.x - 20.0, panel.y + 10.0));
    click(&mut ui, loom_ui_core::PointerButton::Left);
    assert!(ui.app().overlays.browser().is_none(), "both closed");
}

/// Typing while the menu is open closes it and goes on to the field.
#[test]
fn typing_closes_the_menu_and_reaches_the_field() {
    let mut ui = with_library();
    tap(&mut ui, Key::ContextMenu);
    ui.typewrite(" x");
    assert_eq!(model(&ui).menu, None);
    assert_eq!(model(&ui).query, "pad x");
}

/// The menu hangs under its row, and the pointer over its item leaves the
/// focus in the field: the item is no focus stop.
#[test]
fn the_menu_is_under_its_row_and_hover_keeps_the_focus() {
    let mut ui = with_library();
    tap(&mut ui, Key::ContextMenu);
    let row = library_row("Pad.ron");
    let under = bounds(&mut ui, &row);
    let menu = bounds(&mut ui, &menu_id(&row));
    assert_eq!((menu.x, menu.y), (under.x, under.y + under.height));
    let item = centre(&mut ui, &insert_linked_id(&row));
    ui.move_to(item);
    assert_eq!(ui.runtime().focused(), Some(&search_id()));
}

/// A screen reader meets a dialog named "Add module" holding the search
/// field, with the highlighted row as its active descendant, and the list
/// box, whose highlighted option is selected and whose library option
/// offers its menu.
#[test]
fn a_screen_reader_finds_the_dialog_the_field_and_the_list_box() {
    let mut ui = with_library();
    let tree = ui.a11y_tree();
    let dialog = tree.node_by_id(&browser_id());
    assert_eq!(dialog.role(), accesskit::Role::Dialog);
    assert_eq!(dialog.name(), Some("Add module"));
    let field = tree.node_by_id(&search_id());
    assert_eq!(field.role(), accesskit::Role::TextInput);
    assert!(field.is_focused());
    let row = library_row("Pad.ron");
    assert_eq!(field.active_descendant(), Some(&row));
    let list = tree.node_by_id(&list_id());
    assert_eq!(list.role(), accesskit::Role::ListBox);
    let option = tree.node_by_id(&row);
    assert_eq!(option.role(), accesskit::Role::ListBoxOption);
    assert_eq!(option.name(), Some("Pad, Sub-patch"));
    assert_eq!(option.description(), Some("Pad.ron"));
    assert!(option.is_selected());
    assert!(option.offers(accesskit::Action::ShowContextMenu));
}

/// A screen reader's ShowContextMenu on a library row opens its menu on its
/// item, and the tree shows the menu named by the row.
#[test]
fn show_context_menu_opens_the_menu_on_its_item() {
    let mut ui = with_library();
    let row = library_row("Pad.ron");
    ui.a11y_action(&row, accesskit::Action::ShowContextMenu);
    assert_eq!((model(&ui).menu, model(&ui).menu_highlight), (Some(0), Some(0)));
    let tree = ui.a11y_tree();
    let menu = tree.node_by_id(&menu_id(&row));
    assert_eq!(menu.role(), accesskit::Role::Menu);
    assert_eq!(menu.name(), Some("Pad"));
}

/// A screen reader's SetValue on the search field is the query.
#[test]
fn set_value_on_the_field_is_the_query() {
    let mut ui = open();
    ui.a11y_set_value(&search_id(), "reverb");
    assert_eq!(model(&ui).query, "reverb");
}

/// The app with the browser open over a library of `count` sub-patches,
/// all matching the query "zz", so the list scrolls.
fn with_long_library(count: usize) -> Ui {
    let mut ui = open();
    let items = (0..count)
        .map(|n| LibraryItem {
            path: std::path::PathBuf::from(format!("zz {n:02}.ron")),
            library: format!("zz {n:02}.ron"),
            name: format!("zz {n:02}"),
            roles: Vec::new(),
        })
        .collect();
    ui.send(crate::Message::Browser(Message::Library(items)));
    ui.typewrite("zz");
    let _ = ui.redraw();
    ui
}

/// Turns the wheel `lines` over the list: down when negative.
fn wheel(ui: &mut Ui, lines: f32) {
    let list = centre(ui, &list_id());
    ui.move_to(list);
    ui.step(&[crate::ui_test::Input::pointer(loom_ui_core::PointerEvent::Scrolled(
        loom_ui_core::ScrollDelta::Lines { x: 0.0, y: lines },
    ))]);
    let _ = ui.redraw();
}

/// With a row's menu open, the wheel moves the menu with its row while the
/// row shows; once the row is scrolled out of view, the menu closes with no
/// choice, and the search field still has the keys. A list key closes the
/// menu first, as before.
#[test]
fn the_menu_follows_its_row_and_closes_when_the_row_scrolls_away() {
    let mut ui = with_long_library(20);
    tap(&mut ui, Key::ContextMenu);
    let row = library_row("zz 00.ron");
    let menu = menu_id(&row);
    let before = bounds(&mut ui, &menu);
    wheel(&mut ui, -0.3);
    let row_now = bounds(&mut ui, &row);
    let after = bounds(&mut ui, &menu);
    assert!(after.y < before.y, "the menu moved up with its row");
    assert_eq!(after.y, row_now.y + row_now.height, "still under its row");
    assert_eq!(model(&ui).menu, Some(0), "still open while the row shows");
    wheel(&mut ui, -3.0);
    let _ = ui.redraw();
    assert_eq!(model(&ui).menu, None, "the row scrolled away: the menu closed");
    assert!(ui.app().overlays.browser().is_some(), "the browser stays");
    assert_eq!(ui.runtime().focused(), Some(&search_id()));
    // Scrolled back, the menu opens again, and the keys reach it first.
    // The pointer over the list hovers a row, which is highlighted, so the
    // menu key opens that row's menu.
    wheel(&mut ui, 6.0);
    tap(&mut ui, Key::ContextMenu);
    assert_eq!(model(&ui).menu, highlight(&ui));
    tap(&mut ui, Key::Escape);
    assert_eq!(model(&ui).menu, None, "Escape closes the menu first");
    assert!(ui.app().overlays.browser().is_some());
}

/// Enter while an input method composes in the search field confirms the
/// composition and picks no row: the browser takes Enter only when the
/// field is not composing (ui-layer.md, section 6, rule 6, step 2b).
#[test]
fn enter_while_composing_confirms_the_text_and_picks_nothing() {
    let mut ui = open();
    ui.step(&[loom_ui_core::Event::Ime(loom_ui_core::ImeEvent::Enabled)]);
    ui.step(&[loom_ui_core::Event::Ime(loom_ui_core::ImeEvent::Preedit {
        text: "か".to_string(),
        cursor: None,
    })]);
    tap(&mut ui, Key::Enter);
    assert!(ui.app().overlays.browser().is_some(), "Enter picked nothing");
    ui.step(&[loom_ui_core::Event::Ime(loom_ui_core::ImeEvent::Commit(
        "か".to_string(),
    ))]);
    assert_eq!(model(&ui).query, "か");
    assert!(ui.app().overlays.browser().is_some());
}
