//! Tests for the right column's tabs: one stop, arrows that choose, the
//! panel that follows, their clicks, nodes, and place in Tab's order
//! (plan 3.2, row 4e; the WAI-ARIA Tabs pattern).

use super::*;
use crate::ui_test::Ui;
use iced::keyboard::{self, Key as IcedKey, key::Named};

/// The subtractive example with the runtime's focus on the open tab, as a
/// keyboard user's Tab gives it.
fn on_the_inspector_tab() -> Ui {
    let mut ui = Ui::open("subtractive.ron");
    ui.focus(&tab_id(Tab::Inspector));
    let _ = ui.redraw();
    assert_eq!(ui.runtime().focused(), Some(&tab_id(Tab::Inspector)));
    ui
}

/// The subtractive example's filter tile.
fn the_filter(ui: &Ui) -> u32 {
    ui.app()
        .session
        .patch_map
        .structure
        .tiles
        .iter()
        .find(|(_, tile)| tile.name == "SVF")
        .map(|(id, _)| *id)
        .expect("the SVF tile")
}

/// Presses `named` with no modifier.
fn press(ui: &mut Ui, named: Named) {
    ui.tap(IcedKey::Named(named), keyboard::Modifiers::empty());
    let _ = ui.redraw();
}

/// The tab list is one stop, the open tab, whichever tab is open.
#[test]
fn the_tab_list_is_one_stop_the_open_tab() {
    let mut ui = Ui::open("subtractive.ron");
    assert_eq!(ui.app().tabs_stops(), [tab_id(Tab::Inspector)]);
    ui.send(Message::Matrix(Event::Tab(Tab::Matrix)));
    assert_eq!(ui.app().tabs_stops(), [tab_id(Tab::Matrix)]);
    let _ = ui.redraw();
    let stops = ui.runtime().focus_stops();
    assert!(stops.contains(&tab_id(Tab::Matrix)));
    assert!(!stops.contains(&tab_id(Tab::Inspector)) && !stops.contains(&tab_id(Tab::Outline)));
}

/// Left and Right choose the previous and next tab and wrap at the ends,
/// Home and End the first and the last; the panel follows at once and the
/// focus goes with the tab now open. The content keeps no keys while the
/// focus is on the tabs, so no row moves and no tile moves (the WAI-ARIA
/// Tabs pattern, with automatic activation).
#[test]
fn arrows_choose_a_tab_and_the_panel_follows() {
    let mut ui = on_the_inspector_tab();
    let positions = ui.app().session.positions.clone();
    for (named, open) in [
        (Named::ArrowRight, Tab::Matrix),
        (Named::ArrowRight, Tab::Outline),
        (Named::ArrowRight, Tab::Inspector),
        (Named::ArrowLeft, Tab::Outline),
        (Named::Home, Tab::Inspector),
        (Named::End, Tab::Outline),
    ] {
        press(&mut ui, named);
        assert_eq!(ui.app().right_tab, open, "{named:?}");
        assert_eq!(ui.runtime().focused(), Some(&tab_id(open)), "the focus goes with it");
        assert!(!ui.app().session.matrix.focused && !ui.app().session.outline.active);
    }
    // Up and Down are not the tab list's, and reach no content.
    press(&mut ui, Named::ArrowDown);
    assert_eq!(ui.app().right_tab, Tab::Outline);
    assert_eq!(ui.app().session.positions, positions, "no tile moved");
}

/// A click on a tab opens it, and the open content takes the keys, as
/// before: the pointer focuses no tab.
#[test]
fn a_click_opens_a_tab_and_its_content_takes_the_keys() {
    let mut ui = Ui::open("subtractive.ron");
    ui.click_text("Matrix");
    assert_eq!(ui.app().right_tab, Tab::Matrix);
    assert!(ui.app().session.matrix.focused);
    assert_ne!(ui.runtime().focused(), Some(&tab_id(Tab::Matrix)));
}

/// A screen reader meets a tab list named "Right panel" holding the three
/// tabs, the open one selected, each offering Focus and Click, and then
/// the open tab's panel, named by its tab, which the open tab controls.
#[test]
fn a_screen_reader_meets_a_tab_list_and_the_open_panel() {
    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::Matrix(Event::Tab(Tab::Matrix)));
    let tree = ui.a11y_tree();
    let list = tree.node_by_id(&tabs_id());
    assert_eq!(list.role(), accesskit::Role::TabList);
    assert_eq!(list.name(), Some("Right panel"));
    let tabs: Vec<(Option<&str>, bool)> = list
        .children()
        .iter()
        .map(|tab| (tab.name(), tab.is_selected()))
        .collect();
    assert_eq!(
        tabs,
        [
            (Some("Inspector"), false),
            (Some("Matrix"), true),
            (Some("Outline"), false)
        ]
    );
    for tab in list.children() {
        assert_eq!(tab.role(), accesskit::Role::Tab);
        assert!(tab.offers(accesskit::Action::Click));
    }
    let content = tabs_id().with("content");
    let panel = tree.node_by_id(&content);
    assert_eq!(panel.role(), accesskit::Role::TabPanel);
    assert_eq!(panel.name(), Some("Matrix"));
    assert_eq!(tree.node_by_id(&tab_id(Tab::Matrix)).controls(), [&content]);
    assert!(tree.node_by_id(&tab_id(Tab::Inspector)).controls().is_empty());
}

/// A screen reader's Click on a tab opens it, as a click does.
#[test]
fn a_screen_reader_click_opens_a_tab() {
    let mut ui = Ui::open("subtractive.ron");
    ui.a11y_action(&tab_id(Tab::Outline), accesskit::Action::Click);
    assert_eq!(ui.app().right_tab, Tab::Outline);
}

/// Tab from the open Inspector tab goes into its panel, patch info's
/// first stop; Shift+Tab from there comes back to the tab, and Shift+Tab
/// from the tab goes to the top run's last stop, the toolbar, where the
/// column's first stop went before (the WAI-ARIA Tabs pattern).
#[test]
fn tab_goes_from_the_tab_list_into_its_panel_and_back() {
    let mut ui = on_the_inspector_tab();
    press(&mut ui, Named::Tab);
    assert_eq!(ui.runtime().focused(), Some(&crate::patch_info::authors_part()));
    ui.tap(IcedKey::Named(Named::Tab), keyboard::Modifiers::SHIFT);
    let _ = ui.redraw();
    assert_eq!(ui.runtime().focused(), Some(&tab_id(Tab::Inspector)));
    ui.tap(IcedKey::Named(Named::Tab), keyboard::Modifiers::SHIFT);
    let _ = ui.redraw();
    let toolbar = ui.app().toolbar_entry();
    assert_eq!(ui.runtime().focused(), toolbar.as_ref());
}

/// Tab from the open Matrix tab gives the keys to the matrix Iced still
/// draws, as Ctrl+Tab into the column does, and the focus leaves the tab.
#[test]
fn tab_from_the_matrix_tab_enters_the_matrix() {
    let mut ui = on_the_inspector_tab();
    press(&mut ui, Named::ArrowRight);
    assert_eq!(ui.app().right_tab, Tab::Matrix);
    press(&mut ui, Named::Tab);
    assert!(ui.app().session.matrix.focused, "the matrix has the keys");
    assert_eq!(ui.runtime().focused(), None);
}

/// Tab from the open Outline tab gives the keys to the Outline Iced still
/// draws, as Ctrl+Tab into the column does.
#[test]
fn tab_from_the_outline_tab_enters_the_outline() {
    let mut ui = on_the_inspector_tab();
    press(&mut ui, Named::End);
    assert_eq!(ui.app().right_tab, Tab::Outline);
    press(&mut ui, Named::Tab);
    assert!(ui.app().session.outline.active, "the Outline has the keys");
    assert_eq!(ui.runtime().focused(), None);
}

/// A module's rows: Tab from the open tab goes to the column's first stop,
/// and Shift+Tab from there comes back to the tab list.
#[test]
fn shift_tab_from_a_modules_first_stop_reaches_the_tab() {
    let mut ui = Ui::open("subtractive.ron");
    let filter = the_filter(&ui);
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::Select(vec![filter])));
    let _ = ui.redraw();
    let column = crate::inspector::panel::inspector_id();
    let first = ui
        .runtime()
        .focus_stops()
        .iter()
        .find(|stop| stop.is_under(&column))
        .cloned()
        .expect("the module's rows have a stop");
    ui.focus(&tab_id(Tab::Inspector));
    let _ = ui.redraw();
    press(&mut ui, Named::Tab);
    assert_eq!(ui.runtime().focused(), Some(&first), "into the panel");
    ui.tap(IcedKey::Named(Named::Tab), keyboard::Modifiers::SHIFT);
    let _ = ui.redraw();
    assert_eq!(ui.runtime().focused(), Some(&tab_id(Tab::Inspector)), "back to the tab");
}

/// Entering the column from the canvas is as before: Ctrl+Tab from a
/// selected tile lands on the inspector's stop, not on the tab list.
#[test]
fn ctrl_tab_from_the_canvas_still_lands_in_the_inspector() {
    let mut ui = Ui::open("subtractive.ron");
    let filter = the_filter(&ui);
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::Select(vec![filter])));
    let _ = ui.redraw();
    ui.tap(IcedKey::Named(Named::Tab), keyboard::Modifiers::CTRL);
    let _ = ui.redraw();
    let focused = ui.runtime().focused().cloned().expect("a stop has the focus");
    assert!(focused.is_under(&crate::inspector::panel::inspector_id()), "{focused}");
}

/// Ctrl+Tab back into the column gives the focus back to the selected
/// radio it left, which offers no click and so was in no list of stops
/// (CodeRabbit on #900, #905).
#[test]
fn ctrl_tab_back_gives_the_focus_to_the_selected_radio() {
    let mut ui = Ui::open("subtractive.ron");
    let tiles: Vec<u32> = ui.app().session.patch_map.structure.tiles.keys().copied().collect();
    let mut radio = None;
    for tile in tiles {
        ui.send(Message::Canvas(loom_canvas::CanvasEvent::Select(vec![tile])));
        let items = crate::inspector::access::items(&ui.app().inspector_model());
        radio = items
            .iter()
            .flat_map(crate::a11y::items::Item::walk)
            .find(|item| item.kind == crate::a11y::items::Kind::Option && item.on == Some(true))
            .map(|item| item.id.clone());
        if radio.is_some() {
            break;
        }
    }
    let radio = radio.expect("a module of the example shows a selected radio");
    let _ = ui.redraw();
    ui.focus(&radio);
    let _ = ui.redraw();
    assert_eq!(ui.runtime().focused(), Some(&radio));
    ui.tap(IcedKey::Named(Named::Tab), keyboard::Modifiers::CTRL);
    let _ = ui.redraw();
    ui.tap(
        IcedKey::Named(Named::Tab),
        keyboard::Modifiers::CTRL | keyboard::Modifiers::SHIFT,
    );
    let _ = ui.redraw();
    assert_eq!(ui.runtime().focused(), Some(&radio), "back on the radio it left");
}

/// Each tab's node lies where the runtime laid the tab out, in physical
/// pixels, inside the right column at the window's right edge, and no tab
/// lies on another.
#[test]
fn each_tab_node_lies_where_the_tab_was_laid_out() {
    let mut ui = Ui::open("subtractive.ron");
    ui.lay_out_panels();
    let window = ui.runtime().window();
    let column = window.size.width - style::RIGHT_COLUMN_WIDTH;
    let scale = f64::from(window.scale);
    let tree = ui.a11y_tree();
    let mut seen: Vec<accesskit::Rect> = Vec::new();
    for (tab, _) in TABS {
        let laid = ui.runtime().bounds(&tab_id(tab)).expect("laid out");
        assert!(
            laid.x >= column && laid.x + laid.width <= window.size.width,
            "{tab:?} in the column"
        );
        assert!(laid.y >= 0.0 && laid.height > 0.0, "{tab:?}");
        let bounds = tree.node_by_id(&tab_id(tab)).bounds().expect("bounds");
        assert!((bounds.x0 - f64::from(laid.x) * scale).abs() < 1e-3, "{tab:?}");
        assert!((bounds.y0 - f64::from(laid.y) * scale).abs() < 1e-3, "{tab:?}");
        assert!(
            (bounds.x1 - f64::from(laid.x + laid.width) * scale).abs() < 1e-3,
            "{tab:?}"
        );
        for other in &seen {
            let apart = bounds.x1 <= other.x0 || other.x1 <= bounds.x0;
            assert!(apart, "{tab:?} lies on another tab");
        }
        seen.push(bounds);
    }
}
