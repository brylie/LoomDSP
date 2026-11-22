//! Tests for the scroll keys that go to a container (ui-layer.md, section 5,
//! rule 12, `keys_from`, and section 6, rule 6, step 3): a key the focused
//! element does not use moves the nearest container around it, or the one
//! that takes its keys from it.

use super::{Message, SCROLL, Shown, Toolkit, container, id, key, key_with, list_panel, offset, showing, step, stop};
use crate::access::{Access, Action, Problem, Role};
use crate::element::{Element, Length};
use crate::event::DriverEvent;
use crate::focus::FocusStep;
use crate::headless::Harness;
use crate::layout::Layout;
use crate::layout::tests::{Mono, STYLE};
use crate::view::Layer;
use loom_ui_core::{Event, Key, KeyEvent, Modifiers, UiId};

/// Ten rows of text, with no stop.
fn rows() -> Vec<Element> {
    (0..10).map(|n| Element::text(format!("row {n}"), STYLE)).collect()
}

/// A search stop over a container of ten rows that takes its keys from it,
/// as a browser's results do from its search field.
fn searched(_: u32, _: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    list_panel(Element::column([
        stop("search"),
        container("rows", 100.0, rows()).keys_from(id("search")),
    ]))
}

/// Ten stops, `s0` to `s9`, in a container 100 px tall.
fn ten_stops(_: u32, _: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    list_panel(container(
        "rows",
        100.0,
        (0..10).map(|n| stop(&format!("s{n}"))).collect(),
    ))
}

/// A stop and nine rows in an inner container 60 px tall, inside an outer
/// one 100 px tall with room above and below.
fn nested_stop(_: u32, _: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let mut inner = vec![stop("deep")];
    inner.extend(rows());
    list_panel(container(
        "outer",
        100.0,
        vec![
            Element::space(Length::Shrink, Length::Fixed(20.0)),
            container("inner", 60.0, inner),
            Element::space(Length::Shrink, Length::Fixed(100.0)),
        ],
    ))
}

/// A text field over nine rows, in a container.
fn with_field(_: u32, _: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let field = Element::field("ab", STYLE)
        .key("name")
        .width(Length::Fixed(120.0))
        .access(Access::new(Role::TextInput, "Name"));
    let mut all = vec![field];
    all.extend(rows());
    list_panel(container("rows", 100.0, all))
}

/// A button over nine rows, in a container.
fn with_button(_: u32, _: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let button = Element::text("Go", STYLE)
        .key("go")
        .focusable()
        .access(Access::new(Role::Button, "Go").action(Action::Click));
    let mut all = vec![button];
    all.extend(rows());
    list_panel(container("rows", 100.0, all))
}

/// A key let go.
fn released(key: Key) -> DriverEvent {
    DriverEvent::Input {
        event: Event::Key(KeyEvent::Released { key }),
        pointer: None,
    }
}

/// The harness showing `build`, with `stop` focused.
fn focused(build: super::Build, stop: &UiId) -> Harness<Shown, Toolkit> {
    let mut harness = showing(build);
    harness.send(Message::Focus(stop.clone()));
    harness.lay_out(&mut Mono::default());
    assert_eq!(harness.runtime().focused(), Some(stop));
    harness
}

/// A container that takes its keys from a stop is no stop itself, though
/// nothing inside it is one: Tab from the search field leaves the panel,
/// so Tab stops once for the list.
#[test]
fn a_container_that_takes_its_keys_from_a_stop_is_no_stop() {
    let harness = focused(searched, &id("search"));
    assert!(matches!(
        harness.runtime().focus_step(Some(&id("search")), false),
        FocusStep::PastEnd
    ));
}

/// The named stop's keys never scroll the container that takes its keys
/// from it: Down, End, and Home from the search field move nothing, and go
/// on to the target unused, so the app's keys for the field hear them
/// first and move its highlight (the APG combobox). The case with a
/// panel's key hook that takes them comes with row 3a's test of that hook.
#[test]
fn the_named_stops_keys_do_not_scroll_its_container() {
    let mut harness = focused(searched, &id("search"));
    for pressed in [Key::Down, Key::End, Key::Home] {
        step(&mut harness, key(pressed));
    }
    assert_eq!(offset(&harness, &id("rows")), 0.0);
    assert_eq!(
        harness.runtime().target().keys,
        [Key::Down, Key::End, Key::Home],
        "the keys went on"
    );
    assert_eq!(
        harness.runtime().focused(),
        Some(&id("search")),
        "the field keeps the keys"
    );
}

/// Space on a focused button inside a container clicks the button and
/// scrolls nothing: the button uses Space.
#[test]
fn space_on_a_button_in_a_container_clicks_it_and_scrolls_nothing() {
    let button = UiId::panel("list").with("rows").with("go");
    let mut harness = focused(with_button, &button);
    step(&mut harness, key(Key::Space));
    step(&mut harness, released(Key::Space));
    assert_eq!(
        harness.app().clicks,
        std::slice::from_ref(&button),
        "the button was clicked"
    );
    assert_eq!(offset(&harness, &id("rows")), 0.0, "and nothing scrolled");
}

/// A container that names no stop of its panel is a problem of the panel,
/// as a release build reports it.
#[test]
fn a_container_naming_no_stop_is_a_problem() {
    let mut layout = Layout::new(UiId::panel("list"));
    layout.allow_duplicates();
    layout.rebuild(Element::column([
        Element::text("Results", STYLE),
        container("rows", 100.0, rows()).keys_from(id("nothing")),
    ]));
    assert!(
        layout
            .access_problems()
            .iter()
            .any(|(at, problem)| *at == Some(id("rows")) && *problem == Problem::KeysFromNoStop),
        "{:?}",
        layout.access_problems()
    );
}

/// A scroll key the focused stop does not use moves the container around
/// it: Down a line, End to the bottom, Home back to the top.
#[test]
fn a_scroll_key_a_stop_leaves_moves_the_container_around_it() {
    let mut harness = focused(ten_stops, &UiId::panel("list").with("rows").with("s0"));
    step(&mut harness, key(Key::Down));
    assert_eq!(offset(&harness, &id("rows")), SCROLL.line);
    step(&mut harness, key(Key::End));
    assert!(offset(&harness, &id("rows")) > SCROLL.line);
    step(&mut harness, key(Key::Home));
    assert_eq!(offset(&harness, &id("rows")), 0.0);
}

/// Nested containers take a key the innermost first: once the inner one is
/// at its end, the next End moves the outer one.
#[test]
fn a_container_at_its_end_passes_the_key_out() {
    let deep = UiId::panel("list").with("outer").with("inner").with("deep");
    let mut harness = focused(nested_stop, &deep);
    let (inner, outer) = (
        UiId::panel("list").with("outer").with("inner"),
        UiId::panel("list").with("outer"),
    );
    let outer_before = offset(&harness, &outer);
    step(&mut harness, key(Key::End));
    assert!(offset(&harness, &inner) > 0.0, "the inner one first");
    assert_eq!(offset(&harness, &outer), outer_before, "the outer one waits");
    step(&mut harness, key(Key::End));
    assert!(offset(&harness, &outer) > outer_before, "then the outer one");
}

/// A key the focused element uses never moves a container: Space typed in
/// a field is the field's. With another modifier, a key moves none either.
#[test]
fn a_key_the_element_uses_or_a_modified_key_moves_no_container() {
    let name = UiId::panel("list").with("rows").with("name");
    let mut harness = focused(with_field, &name);
    step(&mut harness, key(Key::Space));
    assert_eq!(offset(&harness, &id("rows")), 0.0, "Space typed in the field");
    let mut harness = focused(ten_stops, &UiId::panel("list").with("rows").with("s0"));
    let alt = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    step(&mut harness, key_with(Key::Down, alt));
    assert_eq!(offset(&harness, &id("rows")), 0.0, "Alt with Down");
}

/// A Close button over a container of ten rows that scrolls from it, as
/// help's body does from its Close button.
fn closed_over(_: u32, _: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let close = Element::text("Close", STYLE)
        .key("close")
        .focusable()
        .access(Access::new(Role::Button, "Close").action(Action::Click));
    list_panel(Element::column([
        close,
        container("rows", 100.0, rows()).scrolls_from(id("close")),
    ]))
}

/// A container that scrolls from a stop outside it takes the scroll keys
/// that stop leaves unused, Down and End, as a container around the stop
/// would, and still is no stop itself; Space stays the button's.
#[test]
fn a_container_scrolls_from_the_stop_it_names() {
    let mut harness = focused(closed_over, &id("close"));
    step(&mut harness, key(Key::Down));
    let down = offset(&harness, &id("rows"));
    assert!(down > 0.0, "Down scrolled it: {down}");
    step(&mut harness, key(Key::End));
    assert!(offset(&harness, &id("rows")) > down, "End went to the end");
    assert!(
        harness.runtime().target().keys.is_empty(),
        "the keys were the container's"
    );
    assert!(
        matches!(
            harness.runtime().focus_step(Some(&id("close")), false),
            FocusStep::PastEnd
        ),
        "the container is no stop"
    );
}
