//! Tests for the check box (ui-layer.md, section 5, rule 17; section 6,
//! rule 11; section 7, rule 31; the WAI-ARIA checkbox pattern), through the
//! headless harness: a press on the box or the label, Space, Enter, and
//! its node.

use crate::access::{Access, Role};
use crate::element::Element;
use crate::event::{AccessEvent, DriverEvent};
use crate::headless::Harness;
use crate::layout::tests::STYLE;
use crate::view::Layer;
use crate::window::scroll::tests::{Message, Shown, Toolkit, click, id, key, list_panel, showing, step};
use accesskit::{ActionRequest, Toggled};
use loom_ui_core::{Event, Key, KeyEvent, Point, UiId};

/// The check box's UI ID.
fn follow() -> UiId {
    id("follow")
}

/// A field, then a check box named "Follow tempo", on once it was clicked.
fn panel(_: u32, chosen: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let field = Element::field("", STYLE)
        .key("name")
        .access(Access::new(Role::TextInput, "Name"));
    let checkbox = Element::checkbox("Follow tempo", STYLE, chosen == Some(&follow())).key("follow");
    list_panel(Element::column([field, checkbox]))
}

/// Space coming up.
fn space_up() -> DriverEvent {
    DriverEvent::Input {
        event: Event::Key(KeyEvent::Released { key: Key::Space }),
        pointer: None,
    }
}

/// Tab past the field to the check box.
fn tab_in(harness: &mut Harness<Shown, Toolkit>) {
    step(harness, key(Key::Tab));
    step(harness, key(Key::Tab));
    assert_eq!(harness.runtime().focused(), Some(&follow()));
}

/// Section 5, rule 17: a press on the box or on its label toggles it, and
/// takes no focus: the field keeps it.
#[test]
fn a_press_on_the_box_or_the_label_toggles_it_and_keeps_the_focus() {
    let mut harness = showing(panel);
    step(&mut harness, key(Key::Tab));
    assert_eq!(harness.runtime().focused(), Some(&id("name")));
    let bounds = harness.runtime().bounds(&follow()).expect("the check box");
    click(&mut harness, Point::new(bounds.x + 7.0, bounds.y + bounds.height / 2.0));
    let label = harness.runtime().text_bounds("Follow tempo").expect("the label");
    click(
        &mut harness,
        Point::new(label.x + label.width / 2.0, label.y + label.height / 2.0),
    );
    assert_eq!(harness.app().clicks, [follow(), follow()]);
    assert_eq!(harness.runtime().focused(), Some(&id("name")), "no focus from a press");
}

/// Section 6, rule 11: Space toggles it when it comes up, and Enter does
/// nothing.
#[test]
fn space_toggles_it_and_enter_does_nothing() {
    let mut harness = showing(panel);
    tab_in(&mut harness);
    step(&mut harness, key(Key::Enter));
    assert!(harness.app().clicks.is_empty(), "Enter toggles nothing");
    step(&mut harness, key(Key::Space));
    assert!(harness.app().clicks.is_empty(), "nothing on the way down");
    step(&mut harness, space_up());
    assert_eq!(harness.app().clicks, [follow()]);
}

/// Section 7, rule 31: a check box named by its label, with its checked
/// state, offering Click and Focus. A screen reader's Click toggles it.
#[test]
fn its_node_is_a_check_box() {
    let mut harness = showing(panel);
    harness.send(Message::Describe);
    let nodes = harness.app().nodes.clone().expect("the nodes");
    let find = |nodes: &crate::AccessNodes| {
        nodes
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Follow tempo"))
            .cloned()
            .expect("the check box's node")
    };
    let (node, checkbox) = find(&nodes);
    assert_eq!(checkbox.role(), accesskit::Role::CheckBox);
    assert_eq!(checkbox.toggled(), Some(Toggled::False));
    assert!(checkbox.supports_action(accesskit::Action::Click));
    assert!(checkbox.supports_action(accesskit::Action::Focus));
    step(
        &mut harness,
        DriverEvent::Accessibility(AccessEvent::Action(ActionRequest {
            action: accesskit::Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: node,
            data: None,
        })),
    );
    assert_eq!(harness.app().clicks, [follow()]);
    harness.send(Message::Describe);
    let (_, checkbox) = find(harness.app().nodes.as_ref().expect("the nodes"));
    assert_eq!(checkbox.toggled(), Some(Toggled::True), "on, as the app keeps it");
}
