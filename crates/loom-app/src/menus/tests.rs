use super::*;
use loom_ui_core::Modifiers;

/// Down and Up wrap at the ends, and from no highlight go to the first and
/// the last item. Home and End go to the ends. Other keys move nothing, and
/// an empty menu has nothing to move to.
#[test]
fn a_menu_step_wraps_and_goes_to_the_ends() {
    let down = Key::Down;
    let up = Key::Up;
    assert_eq!(step(None, 3, &down), Some(0));
    assert_eq!(step(None, 3, &up), Some(2));
    assert_eq!(step(Some(0), 3, &down), Some(1));
    assert_eq!(step(Some(2), 3, &down), Some(0), "Down wraps");
    assert_eq!(step(Some(0), 3, &up), Some(2), "Up wraps");
    assert_eq!(step(Some(1), 3, &Key::Home), Some(0));
    assert_eq!(step(Some(1), 3, &Key::End), Some(2));
    assert_eq!(step(Some(1), 3, &Key::Tab), None);
    assert_eq!(step(None, 0, &down), None, "an empty menu");
}

/// Enter that reaches the app's menu keys, with no item focused, stops at
/// the menu and does nothing: picking an item is the runtime's activation
/// of the focused item (section 3, rule 4), which reaches the app as
/// `App::clicked`, so the app never posts the item's message itself.
#[test]
fn enter_with_no_item_focused_stops_at_the_menu() {
    let mut app = crate::ui_test::open_app("subtractive.ron");
    let area = loom_ui_core::Rect::new(
        loom_ui_core::Point::new(0.0, 0.0),
        loom_ui_core::Size::new(800.0, 600.0),
    );
    let _ = app.send(crate::Message::Canvas(loom_canvas::CanvasEvent::TileMenu {
        module: 4,
        at: loom_ui_core::Point::new(100.0, 100.0),
        area,
        by_key: true,
    }));
    app.overlays.set_menu_highlight(None);
    let enter = KeyEvent::Pressed {
        key: Key::Enter,
        code: loom_ui_core::Code::Other,
        modifiers: Modifiers::NONE,
        text: None,
        repeat: false,
    };
    app.menu_key(&enter);
    assert!(app.take_posted().is_empty(), "nothing posted");
    assert!(app.overlays.tile_menu().is_some(), "the menu stays open");
}

/// The tile menu opened with tile 4 selected: from the keyboard when
/// `by_key`, from a press otherwise.
fn tile_menu_open(by_key: bool) -> crate::ui_test::Ui {
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    ui.send(crate::Message::Canvas(loom_canvas::CanvasEvent::Select(vec![4])));
    let area = loom_ui_core::Rect::new(
        loom_ui_core::Point::new(0.0, 0.0),
        loom_ui_core::Size::new(800.0, 600.0),
    );
    ui.send(crate::Message::Canvas(loom_canvas::CanvasEvent::TileMenu {
        module: 4,
        at: loom_ui_core::Point::new(100.0, 100.0),
        area,
        by_key,
    }));
    ui.redraw();
    ui
}

/// Row 3d: the tile menu is the runtime's. Opened from the keyboard it
/// starts on its first item; Down moves the runtime's focus, with a ring,
/// as in the probe menu and Recent; Escape closes it, and the selected
/// tile has the keys again (section 6, rules 4 and 8).
#[test]
fn the_keyed_tile_menu_moves_the_focus_and_closes_on_escape() {
    let mut ui = tile_menu_open(true);
    let items: Vec<_> = ui
        .app()
        .menu_model()
        .expect("open")
        .items
        .into_iter()
        .map(|item| item.id)
        .collect();
    assert!(items.len() > 1, "a menu with more than one item");
    assert_eq!(ui.runtime().focused(), Some(&items[0]), "on its first item");
    ui.tap(Key::Down, Modifiers::NONE);
    ui.redraw();
    assert_eq!(ui.runtime().focused(), Some(&items[1]), "Down moved the focus");
    assert!(ui.runtime().focus_ring().is_some(), "with a ring, from the keys");
    ui.tap(Key::Escape, Modifiers::NONE);
    ui.redraw();
    assert!(ui.app().overlays.tile_menu().is_none(), "Escape closed it");
    assert!(ui.app().canvas_has_keys(), "the selected tile has the keys again");
}

/// One Escape with a Space held on a menu item cancels that Space and
/// closes the menu, as Escape in a menu does at once (ui-layer.md,
/// section 5, rule 11), and the release then picks nothing: the patch is
/// as it was.
#[test]
fn escape_with_space_held_on_an_item_closes_the_menu() {
    use crate::ui_test::{key_press, key_released};
    let mut ui = tile_menu_open(true);
    let before = ui.app().current_patch();
    let none = loom_ui_core::Modifiers::NONE;
    let space = Key::Space;
    ui.step(&[key_press(space, none, Some(" "), false)]);
    ui.tap(Key::Escape, none);
    ui.redraw();
    assert!(ui.app().overlays.tile_menu().is_none(), "one Escape closed it");
    ui.step(&[key_released(space, none)]);
    ui.redraw();
    assert_eq!(ui.app().current_patch(), before, "nothing was picked");
}

/// Row 3d: a press outside the tile menu closes it, and goes no further, so
/// the canvas under it neither selects nor drags (section 5, rule 14).
#[test]
fn a_press_outside_the_tile_menu_closes_it() {
    let mut ui = tile_menu_open(false);
    let menu = ui.app().menu_model().expect("open").id;
    let bounds = ui.runtime().bounds(&menu).expect("laid out");
    let selected = ui.app().session.selection.clone();
    ui.move_to(loom_ui_core::Point::new(
        bounds.x + bounds.width + 200.0,
        bounds.y + bounds.height + 150.0,
    ));
    ui.press();
    ui.release();
    ui.redraw();
    assert!(ui.app().overlays.tile_menu().is_none(), "the press closed it");
    assert_eq!(
        ui.app().session.selection,
        selected,
        "the canvas did not hear the press"
    );
}
