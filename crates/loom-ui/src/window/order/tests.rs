//! Tests for the radio group (ui-layer.md, section 6, rule 9, and section
//! 7, rule 29; the WAI-ARIA radio group pattern), through the headless
//! harness: one stop, arrows that move and choose, Space, a press, the
//! note, the field after the group, the nodes, and a tall group in a
//! scroll container.

use crate::access::{Access, Role};
use crate::element::Element;
use crate::event::{AccessEvent, DriverEvent};
use crate::headless::Harness;
use crate::layout::tests::STYLE;
use crate::view::Layer;
use crate::window::scroll::tests::{
    Message, Shown, Toolkit, click, container, id, key, list_panel, shifted, showing, step, stop,
};
use accesskit::{ActionRequest, Toggled};
use loom_ui_core::{Key, Point, Rect, Size, UiId};

mod checkbox;

/// The radios of the licence group, by key and label; the second has a
/// note.
const RADIOS: [(&str, &str); 3] = [("mit", "MIT"), ("apache", "Apache"), ("other", "Other")];

/// The note under the second radio.
const NOTE: &str = "Patent grant";

/// The UI ID of the radio keyed `key` in the licence group.
fn radio(key: &str) -> UiId {
    id("licence").with(key)
}

/// A stop, the licence group with `chosen` chosen, and a field after it.
fn licence(_: u32, chosen: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let radios = RADIOS.iter().map(|(key, label)| {
        let element = Element::radio(*label, STYLE, chosen == Some(&radio(key)));
        let element = if *key == "apache" {
            element.note(NOTE, STYLE)
        } else {
            element
        };
        element.key(*key)
    });
    let group = Element::radio_group("Licence", radios).key("licence").spacing(8.0);
    let field = Element::field("", STYLE)
        .key("name")
        .access(Access::new(Role::TextInput, "Name"));
    list_panel(Element::column([stop("before"), group, field]))
}

/// The same, with the second radio chosen from the start.
fn licence_apache(version: u32, chosen: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let apache = radio("apache");
    licence(version, Some(chosen.unwrap_or(&apache)))
}

/// Eight radios in a container 60 px tall.
fn tall(_: u32, chosen: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let radios = (0..8).map(|n| {
        let key = format!("r{n}");
        let chosen = chosen == Some(&id("rows").with("tall").with(key.as_str()));
        Element::radio(format!("Choice {n}"), STYLE, chosen).key(key)
    });
    let group = Element::radio_group("Choices", radios).key("tall").spacing(8.0);
    list_panel(container("rows", 60.0, vec![group]))
}

/// The centre of `rect`.
fn centre(rect: Rect) -> Point {
    Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

/// Tab twice: past the stop before, into the group.
fn into_group(harness: &mut Harness<Shown, Toolkit>) {
    step(harness, key(Key::Tab));
    step(harness, key(Key::Tab));
}

/// Tab lands on the chosen radio, or on the first when none is chosen.
#[test]
fn tab_lands_on_the_chosen_radio_or_the_first() {
    let mut harness = showing(licence);
    into_group(&mut harness);
    assert_eq!(harness.runtime().focused(), Some(&radio("mit")));

    let mut harness = showing(licence_apache);
    into_group(&mut harness);
    assert_eq!(harness.runtime().focused(), Some(&radio("apache")));
}

/// The arrows move to the next or the one before and choose it, wrapping at
/// both ends, and each choice reaches the app as a click.
#[test]
fn the_arrows_move_and_choose_and_wrap() {
    let mut harness = showing(licence);
    into_group(&mut harness);
    for (arrow, to) in [
        (Key::Down, "apache"),
        (Key::Right, "other"),
        (Key::Down, "mit"),
        (Key::Up, "other"),
        (Key::Left, "apache"),
    ] {
        step(&mut harness, key(arrow));
        assert_eq!(harness.runtime().focused(), Some(&radio(to)), "{arrow:?}");
        assert!(harness.runtime().focus_ring().is_some(), "from the keyboard, a ring");
    }
    let clicks: Vec<UiId> = ["apache", "other", "mit", "other", "apache"].map(radio).into();
    assert_eq!(harness.app().clicks, clicks);
    assert_eq!(harness.app().chosen, Some(radio("apache")));
}

/// Space chooses the focused radio when it is not chosen, and does nothing
/// more once it is. Enter does nothing in the group.
#[test]
fn space_chooses_and_enter_does_nothing() {
    let mut harness = showing(licence);
    into_group(&mut harness);
    step(&mut harness, key(Key::Enter));
    assert!(harness.app().clicks.is_empty(), "Enter chooses nothing");
    step(&mut harness, key(Key::Space));
    assert_eq!(harness.app().clicks, [radio("mit")]);
    step(&mut harness, key(Key::Space));
    assert_eq!(harness.app().clicks, [radio("mit")], "already chosen");
}

/// Section 6, rule 3: a press on a radio chooses it and focuses it with no
/// ring; the ring shows from the first key it takes, and the arrows then
/// choose among the group's radios.
#[test]
fn a_press_chooses_and_focuses_with_no_ring_until_a_key() {
    let mut harness = showing(licence);
    let label = harness.runtime().text_bounds("Other").expect("the label");
    click(&mut harness, centre(label));
    assert_eq!(harness.app().clicks, [radio("other")]);
    assert_eq!(harness.runtime().focused(), Some(&radio("other")));
    assert!(harness.runtime().focus_ring().is_none(), "no ring from a press");
    step(&mut harness, key(Key::Space));
    assert!(harness.runtime().focus_ring().is_some(), "a ring from the first key");
    assert_eq!(harness.app().clicks, [radio("other")], "Space on the chosen one");
    step(&mut harness, key(Key::Up));
    assert_eq!(harness.app().chosen, Some(radio("apache")));
}

/// A press on a radio's note chooses that radio.
#[test]
fn a_press_on_the_note_chooses_its_radio() {
    let mut harness = showing(licence);
    let note = harness.runtime().text_bounds(NOTE).expect("the note");
    click(&mut harness, centre(note));
    assert_eq!(harness.app().clicks, [radio("apache")]);
}

/// The note is inside its radio's bounds, so the ring goes around both
/// lines.
#[test]
fn the_ring_goes_around_the_note() {
    let mut harness = showing(licence);
    into_group(&mut harness);
    step(&mut harness, key(Key::Down));
    let (focus, ring) = harness.runtime().focus_ring().expect("a ring");
    assert_eq!(focus, radio("apache"));
    let note = harness.runtime().text_bounds(NOTE).expect("the note");
    let label = harness.runtime().text_bounds("Apache").expect("the label");
    for part in [note, label] {
        assert!(
            ring.contains(part.position())
                && ring.contains(Point::new(part.x + part.width - 1.0, part.y + part.height - 1.0)),
            "{part:?} is outside {ring:?}"
        );
    }
}

/// A field after the group is a stop of its own: Tab from the group's one
/// stop reaches it, and Shift+Tab from it comes back to the group.
#[test]
fn a_field_after_the_group_is_the_next_stop() {
    let mut harness = showing(licence_apache);
    into_group(&mut harness);
    step(&mut harness, key(Key::Tab));
    assert_eq!(harness.runtime().focused(), Some(&id("name")));
    step(&mut harness, shifted(Key::Tab));
    assert_eq!(harness.runtime().focused(), Some(&radio("apache")), "the chosen one");
    step(&mut harness, shifted(Key::Tab));
    assert_eq!(
        harness.runtime().focused(),
        Some(&id("before")),
        "the group is one stop"
    );
}

/// Section 7, rule 29: the group is a radio group named by its label, and
/// each radio a radio button with its checked state, its label, its note
/// as its description, Click, and Focus. A screen reader's Click chooses.
#[test]
fn the_nodes_are_a_radio_group_of_radio_buttons() {
    let mut harness = showing(licence_apache);
    harness.send(Message::Describe);
    let nodes = harness.app().nodes.clone().expect("the nodes");
    let by_label = |label: &str| {
        nodes
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label))
            .cloned()
            .unwrap_or_else(|| panic!("a node named {label}"))
    };
    let (_, group) = by_label("Licence");
    assert_eq!(group.role(), accesskit::Role::RadioGroup);
    assert_eq!(group.children().len(), 3);
    let (_, apache) = by_label("Apache");
    assert_eq!(apache.role(), accesskit::Role::RadioButton);
    assert_eq!(apache.toggled(), Some(Toggled::True));
    assert_eq!(apache.description(), Some(NOTE));
    assert!(apache.supports_action(accesskit::Action::Click));
    assert!(apache.supports_action(accesskit::Action::Focus));
    let (mit_node, mit) = by_label("MIT");
    assert_eq!(mit.toggled(), Some(Toggled::False));
    step(
        &mut harness,
        DriverEvent::Accessibility(AccessEvent::Action(ActionRequest {
            action: accesskit::Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: mit_node,
            data: None,
        })),
    );
    assert_eq!(harness.app().chosen, Some(radio("mit")));
}

/// A tall group in a scroll container: each arrow that moves the focus
/// below the fold, or back above it, brings the radio and its ring into
/// view.
#[test]
fn an_arrow_in_a_tall_group_brings_the_radio_into_view() {
    let mut harness = showing(tall);
    step(&mut harness, key(Key::Tab));
    let rows = id("rows");
    let view = |harness: &Harness<Shown, Toolkit>| {
        let bounds = harness.runtime().bounds(&rows).expect("the container");
        Rect::new(bounds.position(), Size::new(bounds.width - 10.0, bounds.height))
    };
    let mut deepest = 0.0_f32;
    for _ in 0..8 {
        step(&mut harness, key(Key::Down));
        deepest = deepest.max(harness.runtime().scroll_offset(&rows).map_or(0.0, |(at, _)| at));
        let (focus, ring) = harness.runtime().focus_ring().expect("a ring");
        let (view, reach) = (view(&harness), 4.0);
        assert!(
            ring.y - reach >= view.y && ring.y + ring.height + reach <= view.y + view.height,
            "{focus} at {ring:?} is cut by {view:?}"
        );
    }
    assert!(deepest > 0.0, "the container scrolled down on the way");
    // The last Down wrapped to the first radio, at the top again.
    assert_eq!(harness.runtime().focused(), Some(&id("rows").with("tall").with("r0")));
    assert_eq!(harness.runtime().scroll_offset(&rows).map(|(at, _)| at), Some(0.0));
}

/// A select is shown only when something of it is left after both clips
/// cut it, one after the other: two clips that each overlap it but not each
/// other leave nothing (section 5, rule 18). The panel's corner moves the
/// cut from the panel into the window.
#[test]
fn a_select_shows_only_through_both_clips() {
    use crate::window::order::shows_through;
    let rect = |x: f32, width: f32| Rect::new(Point::new(x, 0.0), Size::new(width, 20.0));
    let bounds = rect(0.0, 100.0);
    assert!(shows_through(bounds, None, Point::ORIGIN, rect(0.0, 400.0)));
    assert!(shows_through(
        bounds,
        Some(rect(0.0, 40.0)),
        Point::ORIGIN,
        rect(0.0, 40.0)
    ));
    assert!(
        !shows_through(bounds, Some(rect(0.0, 40.0)), Point::ORIGIN, rect(60.0, 40.0)),
        "each clip overlaps it, but not each other"
    );
    assert!(
        !shows_through(bounds, None, Point::new(500.0, 0.0), rect(0.0, 400.0)),
        "moved into the window, it is past the panel's clip"
    );
}
