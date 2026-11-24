//! Tests for the select through the runtime and the headless harness
//! (ui-layer.md, section 5, rule 18; section 6, rule 12; section 7, rule
//! 32): opening, the keys, type-ahead, the pointer, closing on its own, and
//! the nodes.
//!
//! The geometry: a 16 px stop, then the select, 24 px tall at y 16 and as
//! wide as the panel, 400 px. Each option's row in the list is 22 px, and
//! the list starts 5 px below the select's bottom, at y 45.
//!
//! The test app keeps the chosen option as its version's last two digits
//! (99 is none), and which options to show as the hundreds.

use super::SELECT;
use crate::access::{Access, Role};
use crate::element::{Element, Length};
use crate::event::{AccessEvent, DriverEvent};
use crate::headless::Harness;
use crate::layout::tests::STYLE;
use crate::select::{Select, list_id};
use crate::view::Layer;
use crate::window::scroll::tests::{
    Message, Shown, Toolkit, click, id, key, key_with, list_panel, pointer, shifted, showing, step, stop,
};
use accesskit::ActionRequest;
use loom_ui_core::{Event, Key, KeyEvent, Modifiers, Point, PointerButton, PointerEvent, UiId};
use std::time::Duration;

/// The select's UI ID.
fn rate() -> UiId {
    id("rate")
}

/// The options the app shows for `variant`: five rates, the same five
/// renamed, one, none, or fifteen.
fn options(variant: u32) -> Vec<String> {
    let rates = ["44.1 kHz", "48 kHz", "88.2 kHz", "96 kHz", "192 kHz"];
    match variant {
        0 => rates.map(String::from).into(),
        1 => rates.map(|rate| format!("{rate} (new)")).into(),
        2 => vec!["48 kHz".into()],
        3 => Vec::new(),
        _ => (0..15).map(|n| format!("Choice {n}")).collect(),
    }
}

/// A stop, then the select with the app's options and choice; version 5xx
/// adds nothing and drops the select, so it leaves the layout, and 6xx
/// shows the first five options with 30 px more above the select.
fn panel(version: u32, _: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let chosen = Some(version % 100).filter(|at| *at != 99).map(|at| at as usize);
    let variant = version / 100;
    if variant == 5 {
        return list_panel(Element::column([stop("before")]));
    }
    let options = options(if variant == 6 { 0 } else { variant });
    let select = Element::select(Select::new("Sample rate", options, chosen, SELECT)).key("rate");
    let after = Element::field("", STYLE)
        .key("after")
        .access(Access::new(Role::TextInput, "After"));
    let room = Element::space(Length::Shrink, Length::Fixed(if variant == 6 { 30.0 } else { 0.0 }));
    list_panel(Element::column([stop("before"), room, select, after]))
}

/// The panel of [`panel`], and an overlay of the app's with a container
/// 30 px tall at (300, 165), clear of the open list, whose ten rows scroll:
/// its bar is the 10 px at its right, from x 390.
fn with_overlay(version: u32, chosen: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let mut panels = panel(version, chosen);
    let rows = (0..10).map(|n| Element::text(format!("row {n}"), STYLE)).collect();
    let container = crate::window::scroll::tests::container("rows", 30.0, rows).width(Length::Fixed(100.0));
    let overlay = Element::column([container]).placed(crate::element::Placement::At(Point::new(300.0, 165.0)));
    panels.push((UiId::panel("overlay"), Layer::Overlays, overlay));
    panels
}

/// The select alone, in a panel of the Question layer, as in a question.
fn in_question(version: u32, _: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let select = Element::select(Select::new(
        "Sample rate",
        options(0),
        Some((version % 100) as usize),
        SELECT,
    ))
    .key("rate");
    vec![(
        UiId::panel("list"),
        Layer::Question,
        Element::column([stop("before"), select]).width(Length::FILL),
    )]
}

/// The same, inside a container 60 px tall with 200 px of space after the
/// select, so it can be scrolled out of view; version 1xx puts 200 px of
/// space before the select too, so a rebuild moves it out of view.
fn scrolled(version: u32, _: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let select = Element::select(Select::new(
        "Sample rate",
        options(0),
        Some((version % 100) as usize),
        SELECT,
    ))
    .key("rate");
    let before = Element::space(
        Length::Shrink,
        Length::Fixed(if version / 100 == 1 { 200.0 } else { 0.0 }),
    );
    list_panel(crate::window::scroll::tests::container(
        "rows",
        60.0,
        vec![before, select, Element::space(Length::Shrink, Length::Fixed(200.0))],
    ))
}

/// A harness showing `build` at `version`.
fn at(build: crate::window::scroll::tests::Build, version: u32) -> Harness<Shown, Toolkit> {
    let mut harness = showing(build);
    harness.send(Message::Show(version));
    step(&mut harness, DriverEvent::Woken);
    harness
}

/// Tab past the stop to the select.
fn tab_in(harness: &mut Harness<Shown, Toolkit>) {
    step(harness, key(Key::Tab));
    step(harness, key(Key::Tab));
    assert_eq!(harness.runtime().focused(), Some(&rate()));
}

/// The open list's highlighted option, or `None` when it is closed.
fn highlight(harness: &Harness<Shown, Toolkit>) -> Option<usize> {
    let open = harness.runtime().select_highlight();
    // An open list is laid out, under or above its select.
    assert_eq!(
        open.is_some(),
        harness.runtime().bounds(&list_id()).is_some(),
        "the list's panel"
    );
    open
}

/// The options chosen so far, by index.
fn chosen(harness: &Harness<Shown, Toolkit>) -> Vec<usize> {
    harness.app().chosen_options.iter().map(|(_, at)| *at).collect()
}

/// A key typing `c`.
fn typing(c: char) -> DriverEvent {
    DriverEvent::Input {
        event: Event::Key(KeyEvent::Pressed {
            key: Key::Character(c),
            modifiers: Modifiers::NONE,
            repeat: false,
            text: Some(c.to_string()),
        }),
        pointer: None,
    }
}

/// The middle of the option at `index` in a list under the select.
fn option_point(index: usize) -> Point {
    Point::new(100.0, 45.0 + 22.0 * index as f32 + 11.0)
}

/// Section 6, rule 12: Down opens the list on the chosen option, the
/// arrows move the highlight and stop at the ends, and Enter chooses and
/// closes. Focus stays on the select throughout, with its ring.
#[test]
fn down_opens_on_the_chosen_option_and_enter_chooses() {
    let mut harness = at(panel, 1);
    tab_in(&mut harness);
    step(&mut harness, key(Key::Down));
    assert_eq!(highlight(&harness), Some(1), "on the chosen option");
    assert_eq!(harness.runtime().focused(), Some(&rate()));
    assert!(harness.runtime().focus_ring().is_some());
    for _ in 0..6 {
        step(&mut harness, key(Key::Down));
    }
    assert_eq!(highlight(&harness), Some(4), "it stops at the end");
    assert!(chosen(&harness).is_empty(), "moving chooses nothing");
    step(&mut harness, key(Key::Enter));
    assert_eq!(highlight(&harness), None, "closed");
    assert_eq!(chosen(&harness), [4]);
    assert_eq!(harness.runtime().focused(), Some(&rate()));
}

/// Up and Home open it on the first option, End on the last, and Enter,
/// Space, and Alt with Down open it on the chosen one.
#[test]
fn each_key_opens_where_the_pattern_says() {
    let alt = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    for (event, at) in [
        (key(Key::Up), 0),
        (key(Key::Home), 0),
        (key(Key::End), 4),
        (key(Key::Enter), 2),
        (key(Key::Space), 2),
        (key_with(Key::Down, alt), 2),
    ] {
        let mut harness = at_chosen(2);
        step(&mut harness, event);
        assert_eq!(highlight(&harness), Some(at));
    }
}

/// A harness with the select focused and option `chosen` chosen.
fn at_chosen(chosen: u32) -> Harness<Shown, Toolkit> {
    let mut harness = at(panel, chosen);
    tab_in(&mut harness);
    harness
}

/// Escape closes it with no choice; Alt with Up and Space choose.
#[test]
fn escape_closes_and_alt_up_and_space_choose() {
    let alt = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    let mut harness = at_chosen(0);
    step(&mut harness, key(Key::Down));
    step(&mut harness, key(Key::Down));
    step(&mut harness, key(Key::Escape));
    assert_eq!(highlight(&harness), None);
    assert!(chosen(&harness).is_empty());
    step(&mut harness, key(Key::Down));
    step(&mut harness, key(Key::Down));
    step(&mut harness, key_with(Key::Up, alt));
    assert_eq!(chosen(&harness), [1]);
    step(&mut harness, key(Key::Space));
    step(&mut harness, key(Key::Down));
    step(&mut harness, key(Key::Space));
    assert_eq!(chosen(&harness), [1, 2]);
}

/// Tab chooses the highlighted option, closes the list, and moves the
/// focus on, as the pattern says; Shift+Tab does the same backward.
#[test]
fn tab_chooses_and_moves_on() {
    let mut harness = at_chosen(0);
    step(&mut harness, key(Key::Down));
    step(&mut harness, key(Key::Down));
    step(&mut harness, key(Key::Tab));
    assert_eq!(chosen(&harness), [1]);
    assert_eq!(highlight(&harness), None);
    assert_eq!(harness.runtime().focused(), Some(&id("after")));

    let mut harness = at_chosen(0);
    step(&mut harness, key(Key::End));
    step(&mut harness, shifted(Key::Tab));
    assert_eq!(chosen(&harness), [4]);
    assert_eq!(harness.runtime().focused(), Some(&id("before")));
}

/// Page Down and Page Up move ten options, stopping at the ends.
#[test]
fn the_page_keys_move_ten() {
    let mut harness = at_chosen(400);
    step(&mut harness, key(Key::Home));
    step(&mut harness, key(Key::PageDown));
    assert_eq!(highlight(&harness), Some(10));
    step(&mut harness, key(Key::PageDown));
    assert_eq!(highlight(&harness), Some(14), "stops at the last");
    step(&mut harness, key(Key::PageUp));
    assert_eq!(highlight(&harness), Some(4));
    step(&mut harness, key(Key::PageUp));
    assert_eq!(highlight(&harness), Some(0));
}

/// Type-ahead opens a closed select on what it finds, and moves an open
/// list's highlight; characters 500 ms apart start again.
#[test]
fn type_ahead_opens_and_moves() {
    let mut harness = at_chosen(0);
    step(&mut harness, typing('9'));
    assert_eq!(highlight(&harness), Some(3), "96 kHz");
    harness.advance(Duration::from_millis(600));
    step(&mut harness, typing('4'));
    assert_eq!(highlight(&harness), Some(0), "44.1 kHz");
    harness.advance(Duration::from_millis(100));
    step(&mut harness, typing('8'));
    assert_eq!(highlight(&harness), Some(1), "\"48\" is 48 kHz, not 88.2");
}

/// A press on the select opens it and focuses it with no ring; the pointer
/// over an option highlights it; a click on one chooses it and closes the
/// list.
#[test]
fn a_press_opens_and_a_click_on_an_option_chooses() {
    let mut harness = at(panel, 0);
    click(&mut harness, Point::new(100.0, 28.0));
    assert_eq!(highlight(&harness), Some(0));
    assert_eq!(harness.runtime().focused(), Some(&rate()));
    assert!(harness.runtime().focus_ring().is_none(), "no ring from a press");
    step(&mut harness, pointer(PointerEvent::Moved, option_point(3)));
    assert_eq!(highlight(&harness), Some(3));
    click(&mut harness, option_point(2));
    assert_eq!(chosen(&harness), [2]);
    assert_eq!(highlight(&harness), None);
    assert!(harness.app().clicks.is_empty(), "no click reaches the app");
}

/// A press outside the list closes it with no choice, and is taken; so is
/// a press on the select itself while it is open.
#[test]
fn a_press_outside_or_on_the_select_closes_it() {
    let mut harness = at(panel, 0);
    click(&mut harness, Point::new(100.0, 28.0));
    let press = pointer(PointerEvent::Pressed(PointerButton::Left), Point::new(100.0, 8.0));
    step(&mut harness, press);
    assert_eq!(highlight(&harness), None);
    assert_eq!(harness.runtime().focused(), Some(&rate()), "the press was taken");
    step(
        &mut harness,
        pointer(PointerEvent::Released(PointerButton::Left), Point::new(100.0, 8.0)),
    );
    click(&mut harness, Point::new(100.0, 28.0));
    click(&mut harness, Point::new(100.0, 28.0));
    assert_eq!(highlight(&harness), None, "a second press closed it");
    assert!(chosen(&harness).is_empty());
}

/// The list closes with no choice when the select leaves the layout, when
/// its options change, and when focus leaves by a path other than Tab.
#[test]
fn it_closes_on_its_own() {
    let mut harness = at_chosen(0);
    step(&mut harness, key(Key::Down));
    harness.send(Message::Show(500));
    step(&mut harness, DriverEvent::Woken);
    assert_eq!(highlight(&harness), None, "the select is gone");

    let mut harness = at_chosen(0);
    step(&mut harness, key(Key::Down));
    harness.send(Message::Show(0));
    step(&mut harness, DriverEvent::Woken);
    assert_eq!(highlight(&harness), Some(0), "the same options keep it open");
    harness.send(Message::Show(100));
    step(&mut harness, DriverEvent::Woken);
    assert_eq!(highlight(&harness), None, "new options close it");

    let mut harness = at_chosen(0);
    step(&mut harness, key(Key::Down));
    harness.send(Message::Focus(id("after")));
    step(&mut harness, DriverEvent::Woken);
    assert_eq!(highlight(&harness), None, "focus left");
    assert!(chosen(&harness).is_empty());
}

/// An overlay of the app's opening over an open list, as help on F1 does,
/// closes the list with no choice first, and the focus is still on the
/// select, closed (section 6, rule 4).
#[test]
fn an_overlay_over_it_closes_it() {
    let mut harness = at_chosen(0);
    step(&mut harness, key(Key::Down));
    harness.send(Message::Raise(Layer::Overlays));
    step(&mut harness, DriverEvent::Woken);
    assert_eq!(highlight(&harness), None);
    assert!(chosen(&harness).is_empty());
    assert_eq!(harness.runtime().focused(), Some(&rate()));
}

/// Scrolled out of view in its container, the list closes.
#[test]
fn scrolled_out_of_view_it_closes() {
    let mut harness = at(scrolled, 0);
    step(&mut harness, key(Key::Tab));
    step(&mut harness, key(Key::Down));
    assert_eq!(highlight(&harness), Some(0));
    // Over the select itself, above the list: the container scrolls.
    let wheel = pointer(
        PointerEvent::Scrolled(loom_ui_core::ScrollDelta::Pixels { x: 0.0, y: -100.0 }),
        Point::new(100.0, 10.0),
    );
    step(&mut harness, wheel);
    assert_eq!(highlight(&harness), None);
}

/// Section 5, rule 18: one option opens as any other; none is turned off,
/// no stop, and opens nothing.
#[test]
fn one_option_opens_and_none_is_turned_off() {
    let mut harness = at_chosen(200);
    step(&mut harness, key(Key::Down));
    assert_eq!(highlight(&harness), Some(0));

    let mut harness = at(panel, 399);
    step(&mut harness, key(Key::Tab));
    step(&mut harness, key(Key::Tab));
    assert_eq!(harness.runtime().focused(), Some(&id("after")), "no stop");
    click(&mut harness, Point::new(100.0, 28.0));
    assert_eq!(highlight(&harness), None);
}

/// Section 7, rule 32: the select is a combo box, expanded while open, with
/// the highlighted option as its active descendant and the list as what it
/// controls; the list is a list box of options, the highlighted one
/// selected. Expand opens, Collapse closes, Increment steps the choice,
/// and Click on an option chooses it.
#[test]
fn the_nodes_and_a_screen_readers_actions() {
    let mut harness = at_chosen(1);
    let action = |node, action| {
        DriverEvent::Accessibility(AccessEvent::Action(ActionRequest {
            action,
            target_tree: accesskit::TreeId::ROOT,
            target_node: node,
            data: None,
        }))
    };
    let find = |harness: &mut Harness<Shown, Toolkit>, label: &str| {
        harness.send(Message::Describe);
        let nodes = harness.app().nodes.clone().expect("the nodes");
        nodes
            .nodes
            .iter()
            .find(|(_, node)| {
                node.label() == Some(label)
                    && node.role() != accesskit::Role::ScrollView
                    && node.role() != accesskit::Role::ListBox
            })
            .cloned()
            .unwrap_or_else(|| panic!("a node named {label}"))
    };
    let (select, node) = find(&mut harness, "Sample rate");
    assert_eq!(node.role(), accesskit::Role::ComboBox);
    assert_eq!(node.value(), Some("48 kHz"));
    assert_eq!(node.is_expanded(), Some(false));
    assert!(node.supports_action(accesskit::Action::Expand));
    step(&mut harness, action(select, accesskit::Action::Expand));
    assert_eq!(highlight(&harness), Some(1));
    let (_, node) = find(&mut harness, "Sample rate");
    assert_eq!(node.is_expanded(), Some(true));
    assert!(node.supports_action(accesskit::Action::Collapse));
    assert!(node.active_descendant().is_some(), "the highlighted option");
    assert_eq!(node.controls().len(), 1, "the list");
    let (option, row) = find(&mut harness, "88.2 kHz");
    assert_eq!(row.role(), accesskit::Role::ListBoxOption);
    assert_ne!(row.is_selected(), Some(true), "not highlighted");
    let (_, highlighted) = find(&mut harness, "48 kHz");
    assert_eq!(highlighted.is_selected(), Some(true));
    step(&mut harness, action(option, accesskit::Action::Click));
    assert_eq!(chosen(&harness), [2]);
    assert_eq!(highlight(&harness), None);

    let (select, _) = find(&mut harness, "Sample rate");
    step(&mut harness, action(select, accesskit::Action::Increment));
    assert_eq!(chosen(&harness), [2, 3], "one option on");
    step(&mut harness, action(select, accesskit::Action::Expand));
    step(&mut harness, action(select, accesskit::Action::Collapse));
    assert_eq!(highlight(&harness), None);
    assert_eq!(chosen(&harness), [2, 3]);
}

/// A screen reader's Expand with the keyboard focus elsewhere, as with
/// VoiceOver's cursor on the select and keyboard focus not following it:
/// the select takes the focus and its list opens, and stays open on the
/// next turn.
#[test]
fn expand_with_the_focus_elsewhere_focuses_the_select() {
    let mut harness = at(panel, 0);
    step(&mut harness, key(Key::Tab));
    assert_eq!(harness.runtime().focused(), Some(&id("before")));
    harness.send(Message::Describe);
    let nodes = harness.app().nodes.clone().expect("the nodes");
    let (select, _) = nodes
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Sample rate") && node.role() == accesskit::Role::ComboBox)
        .cloned()
        .expect("the select's node");
    step(
        &mut harness,
        DriverEvent::Accessibility(AccessEvent::Action(ActionRequest {
            action: accesskit::Action::Expand,
            target_tree: accesskit::TreeId::ROOT,
            target_node: select,
            data: None,
        })),
    );
    assert_eq!(harness.runtime().focused(), Some(&rate()));
    assert_eq!(highlight(&harness), Some(0));
    step(&mut harness, DriverEvent::Woken);
    assert_eq!(highlight(&harness), Some(0), "still open on the next turn");
}

/// While the list is open, the app may change its choice: the list's tick
/// follows, and choosing the option chosen before is reported as a choice.
#[test]
fn the_list_follows_the_apps_choice() {
    let mut harness = at_chosen(0);
    step(&mut harness, key(Key::Down));
    harness.send(Message::Show(2));
    step(&mut harness, DriverEvent::Woken);
    assert_eq!(highlight(&harness), Some(0), "still open");
    step(&mut harness, key(Key::Enter));
    assert_eq!(chosen(&harness), [0], "option 0 differs from the app's 2");
}

/// When its select moves while the list is open, the list moves with it.
#[test]
fn the_list_follows_its_select_when_it_moves() {
    let mut harness = at_chosen(0);
    step(&mut harness, key(Key::Down));
    let before = harness.runtime().bounds(&list_id()).expect("the list");
    harness.send(Message::Show(600));
    step(&mut harness, DriverEvent::Woken);
    assert_eq!(highlight(&harness), Some(0), "the same options keep it open");
    let after = harness.runtime().bounds(&list_id()).expect("the list");
    assert_eq!(after.y, before.y + 30.0, "under the select where it is now");
}

/// A select in a question has its list in the Question layer, where the
/// question's input goes, so a click on an option chooses it.
#[test]
fn a_select_in_a_question_has_its_list_there() {
    let mut harness = at(in_question, 0);
    harness.send(Message::Raise(Layer::Question));
    step(&mut harness, DriverEvent::Woken);
    // A question's focus comes from the app's focus effect, not from Tab.
    harness.send(Message::Focus(rate()));
    step(&mut harness, DriverEvent::Woken);
    assert_eq!(harness.runtime().focused(), Some(&rate()));
    step(&mut harness, key(Key::Down));
    assert_eq!(highlight(&harness), Some(0));
    click(&mut harness, option_point(2));
    assert_eq!(chosen(&harness), [2]);
}

/// A press on another overlay's scroll bar while the list is open closes
/// the list with no choice, and is taken: no drag starts, so moving the
/// pointer scrolls nothing.
#[test]
fn a_press_on_another_bar_closes_the_list_and_drags_nothing() {
    let mut harness = at(with_overlay, 0);
    // The overlay's container is a stop of its own, read first, so the
    // select takes the focus from the app, not from Tab.
    harness.send(Message::Focus(rate()));
    step(&mut harness, key(Key::Down));
    assert_eq!(highlight(&harness), Some(0));
    let rows = UiId::panel("overlay").with("rows");
    let on_bar = Point::new(395.0, 170.0);
    step(
        &mut harness,
        pointer(PointerEvent::Pressed(PointerButton::Left), on_bar),
    );
    assert_eq!(highlight(&harness), None, "closed");
    step(&mut harness, pointer(PointerEvent::Moved, Point::new(395.0, 190.0)));
    step(
        &mut harness,
        pointer(PointerEvent::Released(PointerButton::Left), Point::new(395.0, 190.0)),
    );
    assert_eq!(
        harness.runtime().scroll_offset(&rows).expect("the container").0,
        0.0,
        "no drag"
    );
    assert!(chosen(&harness).is_empty());
}

/// Expand while the target holds the keys, as an Iced field would, tells
/// the target to let go before the select takes the focus, so the two
/// never both hold the keys.
#[test]
fn expand_releases_the_targets_focus() {
    let mut harness = at(panel, 0);
    harness.parts().1.holds = Some(UiId::panel("iced").with("field"));
    harness.send(Message::Describe);
    let nodes = harness.app().nodes.clone().expect("the nodes");
    let (select, _) = nodes
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Sample rate") && node.role() == accesskit::Role::ComboBox)
        .cloned()
        .expect("the select's node");
    step(
        &mut harness,
        DriverEvent::Accessibility(AccessEvent::Action(ActionRequest {
            action: accesskit::Action::Expand,
            target_tree: accesskit::TreeId::ROOT,
            target_node: select,
            data: None,
        })),
    );
    assert_eq!(harness.runtime().target().released, 1);
    assert_eq!(harness.runtime().target().holds, None);
    assert_eq!(harness.runtime().focused(), Some(&rate()));
    assert_eq!(highlight(&harness), Some(0));
}

/// Opening the list and moving its highlight each tell the app that what
/// the tree reports changed, in that turn, so its expanded state and
/// active descendant reach a screen reader.
#[test]
fn opening_and_moving_raise_the_tree_notice() {
    let mut harness = at_chosen(0);
    harness.send(Message::CountNotices);
    let before = harness.app().notices;
    step(&mut harness, key(Key::Down));
    assert_eq!(harness.app().notices, before + 1, "opening");
    step(&mut harness, key(Key::Down));
    assert_eq!(harness.app().notices, before + 2, "moving the highlight");
}

/// A list that a layout closes, here because a rebuild moved its select out
/// of view, is still told to the app as a change of what the tree reports,
/// in the turn after that layout.
#[test]
fn a_list_closed_by_a_layout_raises_the_tree_notice() {
    let mut harness = at(scrolled, 0);
    step(&mut harness, key(Key::Tab));
    step(&mut harness, key(Key::Down));
    assert_eq!(highlight(&harness), Some(0));
    harness.send(Message::CountNotices);
    // The rebuild moves the select; only the layout after it sees that it
    // is out of view, and closes the list.
    harness.send(Message::Show(100));
    assert_eq!(harness.runtime().select_highlight(), Some(0), "open until the layout");
    let before = harness.app().notices;
    harness.lay_out(&mut crate::layout::tests::Mono::default());
    assert_eq!(harness.runtime().select_highlight(), None, "the layout closed it");
    step(&mut harness, DriverEvent::Woken);
    assert_eq!(harness.app().notices, before + 1, "told in the next turn");
}
