//! Tests for the Outline's panels: the tree as one stop with its rows flat
//! under a Tree node, a click on a row, and the jump list as a list box
//! (plan 3.2, row 4e, commit 2).

use super::*;
use crate::matrix::Tab;
use crate::ui_test::Ui;
use iced::keyboard::{self, Key as IcedKey, key::Named};
use loom_canvas::CanvasEvent;

/// The subtractive example with the Outline open by `O`, its first row
/// focused.
fn outline_open() -> Ui {
    let mut ui = Ui::open("subtractive.ron");
    ui.tap(IcedKey::Character("o".into()), keyboard::Modifiers::empty());
    let _ = ui.redraw();
    ui
}

/// A tile's ID by its name.
fn tile(ui: &Ui, name: &str) -> u32 {
    ui.app()
        .session
        .patch_map
        .structure
        .tiles
        .iter()
        .find(|(_, tile)| tile.name == name)
        .map(|(id, _)| *id)
        .expect("the tile")
}

/// A screen reader meets one Tree named "Outline" with every row that
/// shows as a tree item directly under it, each saying its level, place in
/// its set, the set's size, and whether it is open; the current row is
/// selected and has the focus (section 7, rule 12; the WAI-ARIA Tree View
/// pattern).
#[test]
fn the_tree_reports_its_rows_flat_with_their_shape() {
    let mut ui = outline_open();
    let rows = ui.app().outline_rows();
    let tree = ui.a11y_tree();
    let node = tree.node_by_id(&tree_id());
    assert_eq!(node.role(), accesskit::Role::Tree);
    assert_eq!(node.name(), Some("Outline"));
    let children = node.children();
    assert_eq!(children.len(), rows.len(), "every row, flat");
    for (child, row) in children.iter().zip(rows.iter()) {
        assert_eq!(child.ui_id(), Some(&row.id));
        assert_eq!(child.role(), accesskit::Role::TreeItem);
        assert_eq!(child.name(), Some(row.name.as_str()));
        assert_eq!(child.level(), Some(row.level - 1), "{}", row.id);
        assert_eq!(child.position_in_set(), Some(row.position - 1), "{}", row.id);
        assert_eq!(child.size_of_set(), Some(row.size), "{}", row.id);
        assert_eq!(child.expanded(), row.expanded, "{}", row.id);
    }
    assert!(children[0].is_selected() && children[0].is_focused());
    assert!(!children[1].is_selected());
    // The tree sits in the open tab's panel.
    let panel = tree.node_by_id(&crate::tabs::tabs_id().with("content"));
    assert_eq!(panel.name(), Some("Outline"));
}

/// The tree is one stop, its current row, which follows the arrows.
#[test]
fn the_tree_is_one_stop_its_current_row() {
    let mut ui = outline_open();
    let summary = crate::outline::row_id("summary".into());
    let stops = ui.runtime().focus_stops();
    let rows: Vec<&UiId> = stops.iter().filter(|stop| stop.is_under(&rows_id())).collect();
    assert_eq!(rows, [&summary]);
    ui.tap(IcedKey::Named(Named::ArrowDown), keyboard::Modifiers::empty());
    let _ = ui.redraw();
    let now = ui.runtime().focused().cloned().expect("a row");
    assert_ne!(now, summary);
    let stops = ui.runtime().focus_stops();
    let rows: Vec<&UiId> = stops.iter().filter(|stop| stop.is_under(&rows_id())).collect();
    assert_eq!(rows, [&now], "the stop moved with the focus");
}

/// A click on a row makes it the current row and focuses it, and the
/// tree then has the keys.
#[test]
fn a_click_on_a_row_focuses_it() {
    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::Matrix(crate::matrix::Event::Tab(Tab::Outline)));
    let _ = ui.redraw();
    let saw = crate::outline::row_id(format!("module-{}", tile(&ui, "Saw")));
    ui.lay_out_panels();
    let at = ui.runtime().bounds(&saw).expect("the Saw's row");
    let _ = ui.move_to(loom_ui_core::Point::new(at.x + at.width / 2.0, at.y + at.height / 2.0));
    let _ = ui.press();
    let _ = ui.release();
    let _ = ui.redraw();
    assert_eq!(ui.runtime().focused(), Some(&saw));
    assert!(ui.app().session.outline.active);
}

/// A branch opened from the tree is a list box with the chosen option as
/// its active descendant, and the focus goes to it; Down chooses the next,
/// and Enter goes there in the tree.
#[test]
fn the_jump_list_is_a_list_box_the_focus_enters_from_the_tree() {
    let mut ui = outline_open();
    let filter = tile(&ui, "SVF");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![filter])));
    ui.send(Message::OutlineRow(crate::outline::row_id(format!("module-{filter}"))));
    let _ = ui.redraw();
    // Open the filter's connections and follow its output, which branches.
    ui.tap(IcedKey::Named(Named::ArrowRight), keyboard::Modifiers::empty());
    let _ = ui.redraw();
    let output = crate::outline::row_id(format!("module-{filter}-output-lowpass"));
    ui.send(Message::OutlineRow(output));
    let _ = ui.redraw();
    ui.tap(IcedKey::Named(Named::Enter), keyboard::Modifiers::empty());
    let _ = ui.redraw();
    assert!(ui.app().session.outline.has_jump(), "two places to go");
    let list = jump_id().with("list");
    assert_eq!(ui.runtime().focused(), Some(&list));
    let tree = ui.a11y_tree();
    let node = tree.node_by_id(&list);
    assert_eq!(node.role(), accesskit::Role::ListBox);
    assert_eq!(node.name(), Some("2 connections"));
    assert_eq!(node.active_descendant(), Some(&list.clone().with(0u64)));
    ui.tap(IcedKey::Named(Named::ArrowDown), keyboard::Modifiers::empty());
    let _ = ui.redraw();
    let tree = ui.a11y_tree();
    assert_eq!(
        tree.node_by_id(&list).active_descendant(),
        Some(&list.clone().with(1u64))
    );
    ui.tap(IcedKey::Named(Named::Enter), keyboard::Modifiers::empty());
    let _ = ui.redraw();
    assert!(!ui.app().session.outline.has_jump());
    let focused = ui.runtime().focused().cloned().expect("a row");
    assert!(focused.is_under(&rows_id()), "back in the tree: {focused}");
}

/// No two rows share a key, in any example patch with every module's
/// connections open: a row's key joins its parts with hyphens
/// (`module-4-output-lowpass`), so a port named with a hyphen must not
/// make it equal another's. The layout panics on a duplicate UI ID, so
/// this finds one before a patch does.
#[test]
fn no_two_outline_rows_share_a_key_in_any_example() {
    let mut checked = 0;
    for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/patches")).expect("examples") {
        let path = entry.expect("an entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("ron") {
            continue;
        }
        let name = path.file_name().and_then(|name| name.to_str()).expect("a name");
        let mut app = crate::ui_test::open_app(name);
        let modules: Vec<u32> = app.session.patch_map.structure.tiles.keys().copied().collect();
        for module in modules {
            app.session.outline.set_expanded(super::super::module_id(module), true);
        }
        let rows = app.build_outline_rows();
        let mut seen = std::collections::BTreeSet::new();
        for row in &rows {
            assert!(seen.insert(row.id.clone()), "{name}: {} twice", row.id);
        }
        assert!(
            rows.iter().any(|row| row.level > 1) || rows.len() < 3,
            "{name}: some connection rows"
        );
        checked += 1;
    }
    assert!(checked >= 10, "the examples were read: {checked}");
}

/// A long row wraps onto more lines inside the column rather than running
/// past the tree's right edge, as Iced's rows did. That its box has room
/// for every line it paints is `loom-ui`'s to check
/// (`wrapping_text_is_measured_inside_its_padding_and_border`).
#[test]
fn long_rows_wrap_to_the_column() {
    let mut ui = outline_open();
    ui.lay_out_panels();
    let list = ui.runtime().bounds(&rows_id()).expect("the tree is laid out");
    let rows = ui.app().outline_rows();
    let mut wrapped = false;
    for row in rows.iter() {
        let at = ui.runtime().bounds(&row.id).expect("the row is laid out");
        assert!(
            at.x + at.width <= list.x + list.width + 0.5,
            "{} runs past the column: {at:?} in {list:?}",
            row.id
        );
        wrapped |= at.height > 40.0;
    }
    assert!(wrapped, "some row of the subtractive example is long enough to wrap");
}
