//! Tests for `select.rs`: what each key does to a closed select and an
//! open list, type-ahead, the highlight's moves, option IDs, and where the
//! list goes, without a runtime. The runtime's own tests are in
//! `tests/runtime.rs`.

use super::*;
use crate::layout::tests::STYLE;
use loom_ui_core::{Size, Vector};

mod runtime;

/// The style the tests draw selects in.
pub(crate) const SELECT: SelectStyle = SelectStyle {
    text: STYLE,
    colour: Color::rgb(0.9, 0.9, 0.9),
    background: Color::rgb(0.2, 0.2, 0.2),
    border: Color::rgb(0.5, 0.5, 0.5),
    border_width: 1.0,
    radius: 4.0,
    padding_x: 8.0,
    padding_y: 4.0,
    mark: Color::rgb(0.9, 0.9, 0.9),
    list_background: Color::rgb(0.15, 0.15, 0.15),
    list_border: Color::rgb(0.5, 0.5, 0.5),
    highlight: Color::rgb(0.3, 0.3, 0.4),
    shadow: Some(ShadowStyle {
        colour: Color::rgb(0.0, 0.0, 0.0),
        offset: Vector::new(0.0, 2.0),
        blur: 6.0,
    }),
    scroll: crate::window::scroll::tests::SCROLL,
};

/// No modifiers.
const NONE: Modifiers = Modifiers::NONE;

/// Alt alone.
const ALT: Modifiers = Modifiers {
    alt: true,
    ..Modifiers::NONE
};

/// Shift alone.
const SHIFT: Modifiers = Modifiers {
    shift: true,
    ..Modifiers::NONE
};

/// Ctrl alone.
const CTRL: Modifiers = Modifiers {
    control: true,
    ..Modifiers::NONE
};

/// Section 6, rule 12, in a grid: a closed select in a cell opens only on
/// Enter, F2, and Alt with Down, and leaves the arrows, Home, End, Page Up,
/// Page Down, Space, and type-ahead to the grid.
#[test]
fn the_keys_of_a_closed_select_in_a_cell() {
    assert_eq!(cell_key(Key::Enter, NONE), Some(Closed::OpenOnChosen));
    assert_eq!(cell_key(Key::F2, NONE), Some(Closed::OpenOnChosen));
    assert_eq!(cell_key(Key::Down, ALT), Some(Closed::OpenOnChosen));
    for key in [
        Key::Down,
        Key::Up,
        Key::Left,
        Key::Right,
        Key::Home,
        Key::End,
        Key::PageUp,
        Key::PageDown,
        Key::Space,
        Key::Escape,
        Key::Delete,
        Key::Character('s'),
    ] {
        assert_eq!(cell_key(key, NONE), None, "{key:?} is the grid's");
    }
    assert_eq!(cell_key(Key::Enter, CTRL), None, "a shortcut");
}

/// Section 6, rule 12, closed: Down, Alt with Down, Enter, and Space open
/// on the chosen option; Up and Home on the first; End on the last; a
/// printable character types; anything else goes on.
#[test]
fn the_keys_of_a_closed_select() {
    assert_eq!(closed_key(Key::Down, NONE, None), Some(Closed::OpenOnChosen));
    assert_eq!(closed_key(Key::Down, ALT, None), Some(Closed::OpenOnChosen));
    assert_eq!(closed_key(Key::Enter, NONE, None), Some(Closed::OpenOnChosen));
    assert_eq!(closed_key(Key::Space, NONE, Some(" ")), Some(Closed::OpenOnChosen));
    assert_eq!(closed_key(Key::Up, NONE, None), Some(Closed::OpenOnFirst));
    assert_eq!(closed_key(Key::Home, NONE, None), Some(Closed::OpenOnFirst));
    assert_eq!(closed_key(Key::End, NONE, None), Some(Closed::OpenOnLast));
    assert_eq!(closed_key(Key::Character('s'), NONE, Some("s")), Some(Closed::Type));
    assert_eq!(closed_key(Key::Character('s'), SHIFT, Some("S")), Some(Closed::Type));
    assert_eq!(closed_key(Key::Character('s'), CTRL, Some("s")), None, "a shortcut");
    assert_eq!(closed_key(Key::Escape, NONE, None), None);
    assert_eq!(closed_key(Key::Tab, NONE, None), None);
    assert_eq!(closed_key(Key::PageDown, NONE, None), None);
}

/// Section 6, rule 12, open: the arrows, Home, End, and the page keys move
/// the highlight; Enter, Space, and Alt with Up choose; Tab chooses and
/// moves on; Escape closes; a printable character types.
#[test]
fn the_keys_of_an_open_list() {
    assert_eq!(open_key(Key::Down, NONE, None), Some(OpenKey::Move(1)));
    assert_eq!(open_key(Key::Up, NONE, None), Some(OpenKey::Move(-1)));
    assert_eq!(open_key(Key::PageDown, NONE, None), Some(OpenKey::Move(10)));
    assert_eq!(open_key(Key::PageUp, NONE, None), Some(OpenKey::Move(-10)));
    assert_eq!(open_key(Key::Home, NONE, None), Some(OpenKey::First));
    assert_eq!(open_key(Key::End, NONE, None), Some(OpenKey::Last));
    assert_eq!(open_key(Key::Enter, NONE, None), Some(OpenKey::Choose));
    assert_eq!(open_key(Key::Space, NONE, Some(" ")), Some(OpenKey::Choose));
    assert_eq!(open_key(Key::Up, ALT, None), Some(OpenKey::Choose));
    assert_eq!(open_key(Key::Tab, NONE, None), Some(OpenKey::ChooseAndTab));
    assert_eq!(open_key(Key::Tab, SHIFT, None), Some(OpenKey::ChooseAndTab));
    assert_eq!(open_key(Key::Escape, NONE, None), Some(OpenKey::Close));
    assert_eq!(open_key(Key::Character('b'), NONE, Some("b")), Some(OpenKey::Type));
    assert_eq!(open_key(Key::Character('b'), CTRL, Some("b")), None);
}

/// The highlight stops at the ends, as the pattern says: it never wraps.
#[test]
fn the_highlight_stops_at_the_ends() {
    assert_eq!(moved(0, -1, 5), 0);
    assert_eq!(moved(4, 1, 5), 4);
    assert_eq!(moved(2, 10, 5), 4);
    assert_eq!(moved(8, -10, 12), 0);
    assert_eq!(moved(1, 10, 30), 11);
}

/// The options' labels the tests choose among.
fn rates() -> Vec<String> {
    ["44.1 kHz", "48 kHz", "88.2 kHz", "96 kHz", "Sine", "Saw", "Square"]
        .map(String::from)
        .into()
}

/// Type-ahead: one character finds the next option that starts with it,
/// ignoring case; the same character again steps on to the next one; a
/// string typed quickly is matched whole; a pause starts a new string.
#[test]
fn type_ahead_finds_steps_and_resets() {
    let id = UiId::panel("list").with("rate");
    let start = Instant::now();
    let soon = |ms| start + Duration::from_millis(ms);
    let options = rates();
    let mut typed = Typed::default();
    assert_eq!(typed.find(&id, "s", start, &options, 0), Some(4), "Sine");
    assert_eq!(typed.find(&id, "s", soon(100), &options, 4), Some(5), "Saw, after it");
    assert_eq!(typed.find(&id, "s", soon(200), &options, 5), Some(6), "Square");
    assert_eq!(typed.find(&id, "s", soon(300), &options, 6), Some(4), "wraps to Sine");

    let mut typed = Typed::default();
    assert_eq!(typed.find(&id, "s", start, &options, 0), Some(4));
    assert_eq!(
        typed.find(&id, "q", soon(100), &options, 4),
        Some(6),
        "\"sq\" is Square"
    );

    let mut typed = Typed::default();
    assert_eq!(typed.find(&id, "s", start, &options, 0), Some(4), "Sine");
    assert_eq!(
        typed.find(&id, "S", soon(100), &options, 4),
        Some(5),
        "\"sS\" steps as \"ss\" does"
    );

    let mut typed = Typed::default();
    assert_eq!(typed.find(&id, "9", start, &options, 0), Some(3), "96 kHz");
    assert_eq!(
        typed.find(&id, "4", soon(600), &options, 3),
        Some(0),
        "a pause: \"4\" again"
    );
    assert_eq!(
        typed.find(&id, "z", soon(700), &options, 0),
        None,
        "\"4z\" finds nothing"
    );
}

/// An option's UI ID names its index, and nothing else is an option.
#[test]
fn option_ids_name_their_index() {
    assert_eq!(option_index(&option_id(3)), Some(3));
    assert_eq!(option_index(&list_id()), None);
    assert_eq!(option_index(&UiId::panel("list").with("o3")), None);
    assert_eq!(option_index(&option_id(3).with("inside")), None);
}

/// Section 7, rule 32: a closed select is a combo box with its chosen
/// option as its value, offering Increment, Decrement, and Expand, and is
/// turned off with no options.
#[test]
fn its_node_is_a_combo_box_turned_off_with_no_options() {
    let select = Select::new("Rate", rates(), Some(1), SELECT);
    let access = select.access();
    assert_eq!(access.role(), Role::ComboBox);
    assert!(access.offers(Action::Expand) && access.offers(Action::Increment));
    assert!(!access.is_disabled());
    let node = access.node(true);
    assert_eq!(node.value(), Some("48 kHz"));
    assert_eq!(node.is_expanded(), Some(false));
    assert!(Select::new("Rate", Vec::new(), None, SELECT).access().is_disabled());
    assert_eq!(
        Select::new("Rate", rates(), Some(9), SELECT).chosen,
        None,
        "past the end"
    );
}

/// An open select's node is expanded, offers Collapse in place of Expand,
/// and names its highlight and its list.
#[test]
fn an_open_select_says_so() {
    let access = Select::new("Rate", rates(), Some(1), SELECT)
        .access()
        .opened(option_id(2), list_id());
    assert!(access.offers(Action::Collapse) && !access.offers(Action::Expand));
    assert_eq!(access.active(), Some(&option_id(2)));
    assert_eq!(access.controls(), Some(&list_id()));
    assert_eq!(access.node(true).is_expanded(), Some(true));
}

/// The open list for `count` options under a select at `bounds` in a
/// window 200 tall: where it is placed and how tall its scroll area is.
fn placed(count: usize, bounds: Rect) -> (Placement, f32) {
    let options = (0..count).map(|n| format!("Option {n}")).collect();
    let open = Open {
        id: UiId::panel("list").with("rate"),
        select: Select::new("Rate", options, Some(0), SELECT),
        highlight: 0,
        anchor: None,
    };
    let list = list_panel(&open, bounds, 200.0);
    let scroll = match list.children[0].height {
        Length::Fixed(height) => height,
        other => panic!("a fixed height, not {other:?}"),
    };
    (list.placement.expect("placed"), scroll)
}

/// Section 5, rule 18: the list goes under the select when it fits; when
/// it does not and there is more room above, above; otherwise under, with
/// what does not fit scrolled. Each row is 22 px (16 and 3 above and
/// below), and the list keeps 5 px of border and ring room at each end.
#[test]
fn the_list_goes_under_or_above_and_scrolls() {
    let near_top = Rect::new(Point::new(10.0, 16.0), Size::new(120.0, 24.0));
    let (at, scroll) = placed(5, near_top);
    assert_eq!(at, Placement::At(Point::new(10.0, 40.0)), "under");
    assert_eq!(scroll, 118.0, "five rows, and the ring room");

    let (at, scroll) = placed(12, near_top);
    assert_eq!(
        at,
        Placement::At(Point::new(10.0, 40.0)),
        "still under: more room there"
    );
    assert_eq!(scroll, 158.0, "the 160 px below, less its border");

    let near_bottom = Rect::new(Point::new(10.0, 150.0), Size::new(120.0, 24.0));
    let (at, _) = placed(5, near_bottom);
    assert_eq!(at, Placement::At(Point::new(10.0, 30.0)), "above, 120 tall");
}

/// The step's start is where steps begin, when it is an option; past the
/// end it is none, and the chosen option is the start again.
#[test]
fn the_steps_start_is_kept_only_inside_the_options() {
    let style = SELECT;
    let select = Select::new("Rate", rates(), Some(1), style).steps_from(Some(3));
    assert_eq!(select.start(), Some(3));
    assert_eq!(select.label(), "48 kHz", "it still shows the chosen one");
    let past = Select::new("Rate", rates(), Some(1), style).steps_from(Some(9));
    assert_eq!(past.start(), Some(1));
    assert_eq!(Select::new("Rate", rates(), None, style).start(), None);
}
