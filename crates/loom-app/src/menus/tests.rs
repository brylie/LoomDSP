use super::*;

/// Down and Up wrap at the ends, and from no highlight go to the first and
/// the last item. Home and End go to the ends. Other keys move nothing, and
/// an empty menu has nothing to move to.
#[test]
fn a_menu_step_wraps_and_goes_to_the_ends() {
    let down = Key::Named(Named::ArrowDown);
    let up = Key::Named(Named::ArrowUp);
    assert_eq!(step(None, 3, &down), Some(0));
    assert_eq!(step(None, 3, &up), Some(2));
    assert_eq!(step(Some(0), 3, &down), Some(1));
    assert_eq!(step(Some(2), 3, &down), Some(0), "Down wraps");
    assert_eq!(step(Some(0), 3, &up), Some(2), "Up wraps");
    assert_eq!(step(Some(1), 3, &Key::Named(Named::Home)), Some(0));
    assert_eq!(step(Some(1), 3, &Key::Named(Named::End)), Some(2));
    assert_eq!(step(Some(1), 3, &Key::Named(Named::Tab)), None);
    assert_eq!(step(None, 0, &down), None, "an empty menu");
}

/// Enter on a highlighted item sends the item's message as its own update,
/// on the next turn, as a click on the item would (ui-layer.md, section 2,
/// rule 5): it is not applied inside the key's update.
#[test]
fn enter_on_an_item_sends_its_message_as_its_own_update() {
    let mut app = crate::ui_test::Ui::open("subtractive.ron").app;
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
    app.overlays.set_menu_highlight(Some(0));
    let item = app.menu_model().expect("the tile menu is open").items[0]
        .message
        .clone();
    let enter = keyboard::Event::KeyPressed {
        key: Key::Named(Named::Enter),
        modified_key: Key::Named(Named::Enter),
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::empty(),
        text: None,
        repeat: false,
    };
    app.menu_key(&enter);
    let posted = app.take_posted();
    assert_eq!(format!("{posted:?}"), format!("{:?}", [item]), "posted, not applied");
}
