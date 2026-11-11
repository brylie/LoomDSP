//! Tests for the tree the reproduction hands to AccessKit and for what an
//! action does. They check that the program gives AccessKit a name, a
//! number, and a unit; the gaps are in what the macOS adapter does next.

use super::*;

/// A slider named Cutoff at 800, from 20 to 20000 in steps of 10.
fn cutoff(spoken: Spoken) -> Slider {
    Slider {
        label: "Cutoff",
        value: 800.0,
        min: 20.0,
        max: 20_000.0,
        step: 10.0,
        spoken,
    }
}

/// An action on the slider at `index`.
fn request(index: usize, action: Action, data: Option<ActionData>) -> ActionRequest {
    ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: slider_id(index),
        data,
    }
}

/// The node with `id` in an update.
fn node(update: &TreeUpdate, id: NodeId) -> &Node {
    &update
        .nodes
        .iter()
        .find(|(node_id, _)| *node_id == id)
        .expect("the node is in the tree")
        .1
}

/// The tree has the window as its root, with each slider as a child that
/// has a slider role, its label, its range, and its step.
#[test]
fn the_tree_gives_each_slider_a_role_a_label_and_a_range() {
    let sliders = [cutoff(Spoken::Number), cutoff(Spoken::NumberAndText("Hz"))];
    let update = tree("Sliders", &sliders, 1, 2.0);
    assert_eq!(update.tree.as_ref().map(|tree| tree.root), Some(WINDOW));
    assert_eq!(update.focus, slider_id(1));
    let window = node(&update, WINDOW);
    assert_eq!(window.role(), Role::Window);
    assert_eq!(window.children(), [slider_id(0), slider_id(1)]);
    for index in 0..2 {
        let slider = node(&update, slider_id(index));
        assert_eq!(slider.role(), Role::Slider);
        assert_eq!(slider.label(), Some("Cutoff"));
        assert_eq!(slider.numeric_value(), Some(800.0));
        assert_eq!(slider.min_numeric_value(), Some(20.0));
        assert_eq!(slider.max_numeric_value(), Some(20_000.0));
        assert_eq!(slider.numeric_value_step(), Some(10.0));
        assert_eq!(slider.bounds(), Some(slider_bounds(index, 2.0)));
        for action in [Action::Focus, Action::Increment, Action::Decrement, Action::SetValue] {
            assert!(slider.supports_action(action), "{action:?}");
        }
    }
}

/// A number-only slider has no string value. A slider with a unit has both
/// the number and the text with the unit.
#[test]
fn a_unit_adds_a_string_value_beside_the_number() {
    let sliders = [cutoff(Spoken::Number), cutoff(Spoken::NumberAndText("Hz"))];
    let update = tree("Sliders", &sliders, 0, 1.0);
    assert_eq!(node(&update, slider_id(0)).value(), None);
    let with_unit = node(&update, slider_id(1));
    assert_eq!(with_unit.value(), Some("800 Hz"));
    assert_eq!(with_unit.numeric_value(), Some(800.0));
}

/// Increment and decrement move the value by one step and stop at the ends
/// of the range.
#[test]
fn increment_and_decrement_step_inside_the_range() {
    let mut sliders = [cutoff(Spoken::Number)];
    let mut focus = 0;
    assert!(apply(&mut sliders, &mut focus, &request(0, Action::Increment, None)));
    assert_eq!(sliders[0].value, 810.0);
    assert!(apply(&mut sliders, &mut focus, &request(0, Action::Decrement, None)));
    assert!(apply(&mut sliders, &mut focus, &request(0, Action::Decrement, None)));
    assert_eq!(sliders[0].value, 790.0);

    sliders[0].value = 20_000.0;
    assert!(
        !apply(&mut sliders, &mut focus, &request(0, Action::Increment, None)),
        "already at the top"
    );
    assert_eq!(sliders[0].value, 20_000.0);
}

/// A value can be set as a number or as text that starts with a number.
/// Text with no number changes nothing.
#[test]
fn a_value_is_set_from_a_number_or_from_text() {
    let mut sliders = [cutoff(Spoken::NumberAndText("Hz"))];
    let mut focus = 0;
    let number = request(0, Action::SetValue, Some(ActionData::NumericValue(1200.0)));
    assert!(apply(&mut sliders, &mut focus, &number));
    assert_eq!(sliders[0].value, 1200.0);
    assert_eq!(sliders[0].text().as_deref(), Some("1200 Hz"));

    let text = request(0, Action::SetValue, Some(ActionData::Value("810 Hz".into())));
    assert!(apply(&mut sliders, &mut focus, &text));
    assert_eq!(sliders[0].value, 810.0);

    let words = request(0, Action::SetValue, Some(ActionData::Value("loud".into())));
    assert!(!apply(&mut sliders, &mut focus, &words));
    let over = request(0, Action::SetValue, Some(ActionData::NumericValue(1e9)));
    assert!(apply(&mut sliders, &mut focus, &over));
    assert_eq!(sliders[0].value, 20_000.0, "kept inside the range");
}

/// A focus action moves the focus, and an action on a node that is not a
/// slider does nothing.
#[test]
fn focus_moves_and_other_nodes_are_ignored() {
    let mut sliders = [cutoff(Spoken::Number), cutoff(Spoken::Number)];
    let mut focus = 0;
    assert!(apply(&mut sliders, &mut focus, &request(1, Action::Focus, None)));
    assert_eq!(focus, 1);
    assert!(
        !apply(&mut sliders, &mut focus, &request(1, Action::Focus, None)),
        "already there"
    );

    let window = ActionRequest {
        action: Action::Increment,
        target_tree: TreeId::ROOT,
        target_node: WINDOW,
        data: None,
    };
    assert!(!apply(&mut sliders, &mut focus, &window));
    let missing = request(7, Action::Increment, None);
    assert!(!apply(&mut sliders, &mut focus, &missing));
    assert!(!apply(&mut sliders, &mut focus, &request(0, Action::Click, None)));
}

/// The arrow keys move the focused slider by whole steps, inside its range.
#[test]
fn a_nudge_moves_the_focused_slider() {
    let mut sliders = [cutoff(Spoken::Number)];
    nudge(&mut sliders, 0, 2.0);
    assert_eq!(sliders[0].value, 820.0);
    nudge(&mut sliders, 0, -1e9);
    assert_eq!(sliders[0].value, 20.0);
    nudge(&mut sliders, 5, 1.0);
}

/// Bounds are in physical pixels: the logical rows multiplied by the
/// window's scale factor.
#[test]
fn bounds_scale_with_the_window() {
    let one = slider_bounds(1, 1.0);
    assert_eq!((one.x0, one.y0, one.x1, one.y1), (20.0, 80.0, 380.0, 120.0));
    let two = slider_bounds(1, 2.0);
    assert_eq!((two.x0, two.y0, two.x1, two.y1), (40.0, 160.0, 760.0, 240.0));
}
