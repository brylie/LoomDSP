use super::*;

/// A window of 400 by 300 logical pixels.
const WINDOW: Size = Size::new(400.0, 300.0);

/// A caret 2 wide and 16 tall at (`x`, `y`).
fn caret(x: f32, y: f32) -> Rectangle {
    Rectangle::new(Point::new(x, y), Size::new(2.0, 16.0))
}

/// The box goes just below the caret, at its left edge, when it fits.
#[test]
fn the_box_goes_below_the_caret() {
    let area = place(caret(50.0, 40.0), Size::new(100.0, 24.0), WINDOW);
    assert_eq!(area, Rectangle::new(Point::new(50.0, 56.0), Size::new(100.0, 24.0)));
}

/// At the bottom edge it goes above the caret instead.
#[test]
fn at_the_bottom_it_goes_above() {
    let area = place(caret(50.0, 280.0), Size::new(100.0, 24.0), WINDOW);
    assert_eq!(area.y, 256.0);
}

/// At the right edge it moves left to stay inside the window.
#[test]
fn at_the_right_it_moves_left() {
    let area = place(caret(380.0, 40.0), Size::new(100.0, 24.0), WINDOW);
    assert_eq!(area.x, 300.0);
}

/// At the left and top edges it stays inside, and a box wider or taller
/// than the window starts at its corner.
#[test]
fn at_the_left_and_top_it_stays_inside() {
    let area = place(caret(-5.0, -30.0), Size::new(100.0, 24.0), WINDOW);
    assert_eq!((area.x, area.y), (0.0, 0.0));
    let huge = place(caret(50.0, 40.0), Size::new(500.0, 400.0), WINDOW);
    assert_eq!((huge.x, huge.y), (0.0, 0.0));
}

/// The selected range splits the text at character boundaries; an empty
/// range, a range off a boundary, or none marks nothing.
#[test]
fn the_selection_splits_at_characters() {
    let content = "にほんご";
    assert_eq!(selected(content, Some(&(3..9))), Some(("に", "にほん")));
    assert_eq!(selected(content, Some(&(3..3))), None);
    assert_eq!(selected(content, Some(&(1..4))), None);
    assert_eq!(selected(content, Some(&(3..99))), None);
    assert_eq!(selected(content, None), None);
}

/// Only an enabled input method with text being composed has a preedit.
#[test]
fn only_composed_text_is_drawn() {
    let enabled = |preedit| InputMethod::Enabled {
        cursor: caret(0.0, 0.0),
        purpose: iced::advanced::input_method::Purpose::Normal,
        preedit,
    };
    let composing = Preedit {
        content: "ni".to_string(),
        selection: None,
        text_size: None,
    };
    assert_eq!(of(&enabled(Some(composing.clone()))), Some(&composing));
    assert_eq!(of(&enabled(Some(Preedit::new()))), None);
    assert_eq!(of(&enabled(None)), None);
    assert_eq!(of(&InputMethod::Disabled), None);
}
