use super::super::{Browser, LibraryItem, Outcome};
use super::*;
use loom_ui_core::{Point, Rect};

/// A browser over every registered module, with two library items, one in
/// a subfolder whose path holds a `/`.
fn browser() -> Browser {
    let modules = loom_modules::registry().iter().collect();
    let area = Rect::new(Point::new(0.0, 0.0), loom_ui_core::Size::new(2000.0, 2000.0));
    let mut browser = Browser::open(modules, Point::new(0.0, 0.0), Point::new(10.0, 10.0), area);
    let item = |library: &str, name: &str| LibraryItem {
        path: std::path::PathBuf::from(library),
        library: library.to_string(),
        name: name.to_string(),
        roles: Vec::new(),
    };
    let _ = browser.update(Message::Library(vec![
        item("Voices/Bass voice.ron", "Bass voice"),
        item("Pad.ron", "Pad"),
    ]));
    browser
}

/// The row whose item is the library item called `name`.
fn library_row(browser: &Browser, name: &str) -> usize {
    browser
        .model()
        .rows
        .iter()
        .position(|row| row.library && row.name == name)
        .expect("the library row")
}

/// The browser is a dialog named "Add module": its search field has the
/// focus and the highlighted row as its active descendant, and its list box
/// has one option per row, the highlighted one selected, each named by
/// what the row shows.
#[test]
fn the_browser_is_a_dialog_with_a_field_and_a_list_box() {
    let browser = browser();
    let (dialog, focus) = items(browser.model());
    assert_eq!(dialog.kind, Kind::Dialog);
    assert_eq!(dialog.label.as_deref(), Some("Add module"));
    assert_eq!(focus, None, "the field has the focus");
    let field = &dialog.children[0];
    assert_eq!(field.kind, Kind::Field);
    assert!(field.focus.is_some());
    let first = &browser.model().rows[0];
    assert_eq!(field.active.as_ref(), Some(&first.id));

    let list = dialog.children.iter().find(|item| item.kind == Kind::ListBox).unwrap();
    assert_eq!(list.children.len(), browser.model().rows.len());
    let selected: Vec<usize> = list
        .children
        .iter()
        .enumerate()
        .filter(|(_, option)| option.selected == Some(true))
        .map(|(at, _)| at)
        .collect();
    assert_eq!(selected, vec![0]);
    assert_eq!(
        list.children[0].label.as_deref(),
        Some(format!("{}, {}", first.name, first.mark).as_str())
    );
    let pad = &list.children[library_row(&browser, "Pad")];
    assert_eq!(pad.label.as_deref(), Some("Pad, Sub-patch"));
    assert!(list.children.iter().all(|option| option.click.is_some()));
}

/// A module's row is keyed by its type ID and a library item's by its
/// path in the library folder, one key, `/` and all.
#[test]
fn rows_are_keyed_by_type_id_and_library_path() {
    let browser = browser();
    let model = browser.model();
    let svf = model
        .rows
        .iter()
        .find(|row| row.id.to_string() == "browser/module/loom.filter.svf")
        .expect("the filter's row");
    assert!(!svf.library);
    let bass = &model.rows[library_row(&browser, "Bass voice")];
    assert_eq!(bass.id.to_string(), "browser/library/Voices%2FBass voice.ron");
    assert_eq!(bass.id.keys().len(), 3);
    assert_eq!(bass.id.to_string().parse::<UiId>().unwrap(), bass.id);
}

/// A library row offers its menu to a screen reader; a module row has no
/// menu, so it offers none.
#[test]
fn a_library_row_has_a_menu_and_a_module_row_does_not() {
    let browser = browser();
    let (dialog, _) = items(browser.model());
    let list = dialog.children.iter().find(|item| item.kind == Kind::ListBox).unwrap();
    let row = library_row(&browser, "Pad");
    assert!(matches!(
        list.children[row].menu,
        Some(AppMessage::Browser(Message::Menu { row: r, by_key: true })) if r == row
    ));
    assert!(list.children[0].menu.is_none());
}

/// A menu opened by a screen reader starts on its item, which has the
/// reported focus, and the field gives it up; Enter then inserts linked.
#[test]
fn a_menu_opened_by_a_reader_is_focused_and_enter_inserts_linked() {
    let mut browser = browser();
    let row = library_row(&browser, "Bass voice");
    let _ = browser.update(Message::Menu { row, by_key: true });
    let (dialog, focus) = items(browser.model());
    let id = &browser.model().rows[row].id;
    assert_eq!(focus.as_ref(), Some(&insert_linked_id(id)));
    assert!(dialog.children[0].focus.is_none(), "the keys act on the menu");
    let menu = dialog.children.iter().find(|item| item.kind == Kind::Menu).unwrap();
    assert_eq!(menu.children[0].kind, Kind::MenuItem);
    assert_eq!(menu.children[0].label.as_deref(), Some(INSERT_LINKED));

    let (outcome, _) = browser.update(Message::Submit);
    assert!(matches!(outcome, Outcome::Insert { linked: true, ref item, .. } if item.name == "Bass voice"));
}

/// A menu opened by the pointer waits for a key: Enter does nothing until
/// Down moves to the item, then picks it. Escape closes the menu first,
/// and the browser only on a second press (module-browser.md).
#[test]
fn the_library_menu_follows_the_menu_keys() {
    let mut browser = browser();
    let row = library_row(&browser, "Pad");
    let _ = browser.update(Message::Menu { row, by_key: false });
    assert_eq!(browser.model().menu_highlight, None);
    let (_, focus) = items(browser.model());
    assert_eq!(focus, Some(menu_id(&browser.model().rows[row].id)));
    assert!(matches!(browser.update(Message::Submit).0, Outcome::Stay));
    assert_eq!(browser.model().menu, Some(row), "Enter with no item leaves it open");

    let _ = browser.update(Message::Key(super::super::Nav::Down));
    assert_eq!(browser.model().menu_highlight, Some(0));
    let _ = browser.update(Message::Key(super::super::Nav::Up));
    assert_eq!(browser.model().menu_highlight, Some(0), "one item: it stays");
    assert_eq!(browser.model().highlight, Some(row), "the list's highlight stays");

    assert!(matches!(
        browser.update(Message::Key(super::super::Nav::Escape)).0,
        Outcome::Stay
    ));
    assert_eq!(browser.model().menu, None);
    assert!(matches!(
        browser.update(Message::Key(super::super::Nav::Escape)).0,
        Outcome::Close
    ));
}

/// The model changes only when what is shown does: a scroll, or the
/// pointer moving over the row already highlighted, leaves it alone.
#[test]
fn the_model_changes_only_with_what_is_shown() {
    let mut browser = browser();
    let before = browser.model().clone();
    let _ = browser.update(Message::Scrolled(40.0));
    let _ = browser.update(Message::Hover(0));
    assert_eq!(browser.model(), &before);
    let _ = browser.update(Message::Hover(1));
    assert_eq!(browser.model().highlight, Some(1));
    assert_ne!(browser.model().about, before.about);
}

/// With nothing matching, the list box gives way to its line, and the
/// field has no active descendant.
#[test]
fn no_match_shows_the_line_and_no_option() {
    let mut browser = browser();
    let _ = browser.update(Message::Query("zzzz nothing".into()));
    let (dialog, _) = items(browser.model());
    assert!(dialog.children[0].active.is_none());
    assert!(!dialog.children.iter().any(|item| item.kind == Kind::ListBox));
    assert!(
        dialog
            .children
            .iter()
            .any(|item| item.value.as_deref() == Some("No modules match."))
    );
}
