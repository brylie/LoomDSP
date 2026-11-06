//! Tests for the accessibility spike: node IDs, the tree, and actions.

use super::*;
use crate::{App, Message};

/// An app with one filter, its module ID, and the panel index of its Cutoff knob.
fn app_with_filter() -> (App, u32, usize) {
    let mut app = crate::tests::app();
    let id = app.session.controller.add_module("loom.filter.svf").unwrap();
    app.commit();
    let snapshot = app.ui_snapshot();
    let tile = snapshot.tiles.iter().find(|t| t.id == id.0).unwrap();
    let knob = tile
        .widgets
        .iter()
        .position(|w| w.label.eq_ignore_ascii_case("cutoff"))
        .expect("the filter tile has a Cutoff control");
    (app, id.0, knob)
}

/// A screen reader's request for `action` on `node`.
fn request(action: Action, node: NodeId, data: Option<ActionData>) -> ActionRequest {
    ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: node,
        data,
    }
}

/// A widget's node ID gives back its module and panel index; tile and root IDs give none.
#[test]
fn widget_nodes_round_trip() {
    assert_eq!(widget_of(widget_node(0, 0)), Some((0, 0)));
    assert_eq!(widget_of(widget_node(41, 7)), Some((41, 7)));
    assert_eq!(widget_of(tile_node(41)), None);
    assert_eq!(widget_of(ROOT), None);
    assert_ne!(tile_node(0), ROOT);
}

/// The tree names each tile, and gives a knob the slider role with its value as shown.
#[test]
fn tree_shows_tiles_and_controls() {
    let (app, module, knob) = app_with_filter();
    let snapshot = app.ui_snapshot();
    let update = tree(&snapshot, None, 2.0);
    let node = |id: NodeId| &update.nodes.iter().find(|(n, _)| *n == id).unwrap().1;
    let tile = snapshot.tiles.iter().find(|t| t.id == module).unwrap();

    assert!(node(ROOT).children().contains(&tile_node(module)));
    assert_eq!(node(tile_node(module)).label(), Some(tile.title.as_str()));
    let cutoff = node(widget_node(module, knob));
    assert_eq!(cutoff.role(), Role::Slider);
    assert_eq!(cutoff.value(), Some(tile.widgets[knob].text.as_str()));
    assert!(cutoff.supports_action(Action::Increment));
    // With no canvas located, nothing has bounds rather than wrong ones.
    assert!(cutoff.bounds().is_none());
}

/// Increment turns the knob one step, through the canvas, as one undo step.
#[test]
fn increment_turns_the_knob_as_one_undo_step() {
    let (mut app, module, knob) = app_with_filter();
    let before = app.ui_snapshot().tiles.iter().find(|t| t.id == module).unwrap().widgets[knob].position;

    let _ = app.update(Message::A11y(Event::Action(request(
        Action::Increment,
        widget_node(module, knob),
        None,
    ))));

    let after = app.ui_snapshot().tiles.iter().find(|t| t.id == module).unwrap().widgets[knob].position;
    assert!((after - (before + STEP).min(1.0)).abs() < 1e-4, "{before} -> {after}");
    assert!(app.ui_snapshot().undo.is_some(), "the turn can be undone");
}

/// SetValue moves the knob to the position asked for, kept within 0 to 1.
#[test]
fn set_value_is_clamped() {
    let (app, module, knob) = app_with_filter();
    let snapshot = app.ui_snapshot();
    let events = action(
        &snapshot,
        &request(
            Action::SetValue,
            widget_node(module, knob),
            Some(ActionData::NumericValue(3.0)),
        ),
    );
    assert_eq!(
        events,
        vec![CanvasEvent::SetControl {
            module,
            widget: knob,
            position: 1.0,
            part: ControlPart::Main,
        }]
    );
}

/// Focusing a tile selects it, so the inspector follows the screen reader.
#[test]
fn focus_on_a_tile_selects_it() {
    let (app, module, _) = app_with_filter();
    let events = action(&app.ui_snapshot(), &request(Action::Focus, tile_node(module), None));
    assert_eq!(events, vec![CanvasEvent::Select(vec![module])]);
}

/// Actions on nodes the spike does not handle do nothing.
#[test]
fn unknown_targets_do_nothing() {
    let (app, module, knob) = app_with_filter();
    let snapshot = app.ui_snapshot();
    assert!(action(&snapshot, &request(Action::Increment, ROOT, None)).is_empty());
    assert!(
        action(
            &snapshot,
            &request(Action::Increment, widget_node(module + 99, knob), None)
        )
        .is_empty()
    );
    assert!(action(&snapshot, &request(Action::Click, widget_node(module, knob), None)).is_empty());
}
