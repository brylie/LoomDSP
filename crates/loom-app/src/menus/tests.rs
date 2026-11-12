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
