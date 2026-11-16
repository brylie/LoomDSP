use super::*;

/// The default shape is the plain arrow.
#[test]
fn the_default_cursor_is_the_plain_arrow() {
    assert_eq!(Cursor::default(), Cursor::Default);
}

/// `ALL` holds every shape once. The match has no wildcard, so a new shape
/// fails to build until it is given a place here, and a place past the end
/// of `ALL` fails the test.
#[test]
fn all_holds_every_shape_once() {
    let place = |cursor: Cursor| match cursor {
        Cursor::Default => 0,
        Cursor::Grab => 1,
        Cursor::Grabbing => 2,
        Cursor::Pointer => 3,
        Cursor::Crosshair => 4,
        Cursor::Text => 5,
        Cursor::NotAllowed => 6,
        Cursor::ResizeHorizontal => 7,
        Cursor::ResizeVertical => 8,
        Cursor::ResizeDiagonal => 9,
    };
    for (i, cursor) in Cursor::ALL.iter().enumerate() {
        assert_eq!(place(*cursor), i, "{cursor:?}");
    }
}
