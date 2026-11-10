//! Tests for the accessibility tree: UI IDs and node IDs, the tree, and
//! actions. None needs a window or a screen reader, so they run on every OS.

use super::tree::{STEP, WidgetIds, canvas_id, status_id, tile_id, window_id};
use super::*;
use crate::{App, Message};
use accesskit::{Action, ActionData, Node, NodeId, Role, TreeId};
use loom_canvas::{Camera, CanvasEvent, ControlPart};
use loom_ui_core::{Point, Rect, Size, UiId};

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

/// The node ID the table gives `id`.
fn node_of(app: &mut App, id: &UiId) -> NodeId {
    NodeId(app.a11y.ids.number(id))
}

/// The node ID of a panel widget, after a tree was built.
fn widget_node(app: &mut App, module: u32, widget: usize) -> NodeId {
    let snapshot = app.ui_snapshot();
    let id = app
        .a11y_widget_ids(&snapshot)
        .get(module, widget)
        .expect("the widget has a UI ID")
        .clone();
    node_of(app, &id)
}

/// The node with `id` in an update.
fn node(update: &TreeUpdate, id: NodeId) -> &Node {
    &update
        .nodes
        .iter()
        .find(|(n, _)| *n == id)
        .expect("the node is in the tree")
        .1
}

/// The position of a panel widget.
fn position(app: &App, module: u32, widget: usize) -> f32 {
    app.ui_snapshot().tiles.iter().find(|t| t.id == module).unwrap().widgets[widget].position
}

/// The canvas events a request becomes, with the app as it is now.
fn events(app: &App, request: &ActionRequest) -> Vec<CanvasEvent> {
    let snapshot = app.ui_snapshot();
    let ids = app.a11y_widget_ids(&snapshot);
    actions::canvas_events(&snapshot, &ids, &app.a11y.ids, request)
}

/// A control's UI ID is the canvas, its module, its kind, and the name of
/// the parameter it is bound to, not its place in the panel.
#[test]
fn a_control_is_keyed_by_its_module_kind_and_name() {
    let (app, module, knob) = app_with_filter();
    let snapshot = app.ui_snapshot();
    let ids = app.a11y_widget_ids(&snapshot);
    let id = ids.get(module, knob).unwrap();
    assert_eq!(*id, tile_id(module).with("param").with("cutoff"));
    assert!(id.is_under(&canvas_id()));
    assert_eq!(ids.find(id), Some((module, knob)));
}

/// A widget that only shows a value has the kind `display`; a control on a
/// sub-patch tile has the kind `exposed`; a widget with no name has no ID.
#[test]
fn widget_kinds_and_missing_names() {
    let mut ids = WidgetIds::default();
    ids.add_tile(
        3,
        true,
        [
            ("knob", Some("gain".to_string())),
            ("meter", Some("out".to_string())),
            ("knob", None),
        ],
    );
    ids.add_tile(9, false, [("knob", Some("Cutoff".to_string()))]);

    assert_eq!(ids.get(3, 0), Some(&tile_id(3).with("param").with("gain")));
    assert_eq!(ids.get(3, 1), Some(&tile_id(3).with("display").with("out")));
    assert_eq!(ids.get(3, 2), None);
    assert_eq!(ids.get(9, 0), Some(&tile_id(9).with("exposed").with("Cutoff")));
    assert_eq!(ids.get(3, 7), None, "no such widget");
    assert_eq!(ids.get(4, 0), None, "no such tile");
    assert_eq!(ids.find(&tile_id(3)), None, "a tile is not a widget");
}

/// Two widgets that show the same value get different IDs: the later ones
/// are counted from 2. A parameter and a display of the same name differ by
/// kind.
#[test]
fn widgets_bound_to_the_same_name_get_different_ids() {
    let mut ids = WidgetIds::default();
    ids.add_tile(
        11,
        true,
        [
            ("meter", Some("out".to_string())),
            ("scope", Some("out".to_string())),
            ("readout", Some("out".to_string())),
            ("knob", Some("out".to_string())),
        ],
    );
    let display = tile_id(11).with("display").with("out");
    assert_eq!(ids.get(11, 0), Some(&display));
    assert_eq!(ids.get(11, 1), Some(&display.clone().with(2u32)));
    assert_eq!(ids.get(11, 2), Some(&display.clone().with(3u32)));
    assert_eq!(ids.get(11, 3), Some(&tile_id(11).with("param").with("out")));
    assert_eq!(ids.find(&display.with(2u32)), Some((11, 1)));
}

/// The tree names each tile, and gives a knob the slider role, a number for
/// its value, and the text the screen shows, with its unit, as its
/// description (ui-layer.md, section 7, rules 3 and 4).
#[test]
fn tree_shows_tiles_and_controls() {
    let (mut app, module, knob) = app_with_filter();
    let snapshot = app.ui_snapshot();
    let update = app.a11y_tree(None);
    let tile = snapshot.tiles.iter().find(|t| t.id == module).unwrap();
    let shown = &tile.widgets[knob];

    let root = node_of(&mut app, &window_id());
    let tile_node = node_of(&mut app, &tile_id(module));
    let status = node_of(&mut app, &status_id());
    assert_eq!(update.tree.as_ref().unwrap().root, root);
    assert_eq!(update.focus, root);
    assert_eq!(node(&update, root).role(), Role::Window);
    assert!(node(&update, root).children().contains(&tile_node));
    assert!(node(&update, root).children().contains(&status));
    assert_eq!(node(&update, status).value(), Some(snapshot.status.as_str()));
    assert_eq!(node(&update, tile_node).label(), Some(tile.title.as_str()));

    let cutoff_node = widget_node(&mut app, module, knob);
    assert!(node(&update, tile_node).children().contains(&cutoff_node));
    let cutoff = node(&update, cutoff_node);
    assert_eq!(cutoff.role(), Role::Slider);
    assert_eq!(cutoff.label(), Some(shown.label.as_str()));
    assert_eq!(cutoff.numeric_value(), Some(f64::from(shown.position)));
    assert_eq!(cutoff.value(), None, "the value is a number, so it can be stepped");
    assert!(!shown.text.is_empty());
    assert_eq!(cutoff.description(), Some(shown.text.as_str()));
    assert!(cutoff.supports_action(Action::Increment));
    assert!(cutoff.supports_action(Action::Decrement));
    assert!(cutoff.supports_action(Action::SetValue));
    // With no canvas located, nothing has bounds rather than wrong ones.
    assert!(cutoff.bounds().is_none());
}

/// Every node in the tree has a different node ID, and every child a node
/// names is in the tree.
#[test]
fn node_ids_are_unique_and_children_exist() {
    let (mut app, _, _) = app_with_filter();
    let update = app.a11y_tree(None);
    let mut ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count);
    for (_, parent) in &update.nodes {
        for child in parent.children() {
            assert!(ids.contains(&child.0));
        }
    }
}

/// Each widget kind gets the role the tree gives it so far: a toggle is a
/// switch that says whether it is on, a button a button, a meter a meter,
/// and the kinds the next slice covers a read-only label.
#[test]
fn each_widget_kind_has_its_role() {
    let mut app = crate::tests::app();
    let id = app.session.controller.add_module("loom.filter.svf").unwrap().0;
    app.commit();
    let mut snapshot = app.ui_snapshot();
    let ids = app.a11y_widget_ids(&snapshot);
    let at_tile = snapshot.tiles.iter().position(|t| t.id == id).unwrap();
    let index = (0..snapshot.tiles[at_tile].widgets.len())
        .find(|i| ids.get(id, *i).is_some())
        .expect("the tile has a named widget");
    let kinds = [
        ("toggle", 1.0, Role::Switch),
        ("toggle", 0.0, Role::Switch),
        ("button", 0.0, Role::Button),
        ("meter", 0.25, Role::Meter),
        ("radio", 0.0, Role::Label),
        ("fader", 0.5, Role::Slider),
    ];
    for (kind, at, role) in kinds {
        let widget = &mut snapshot.tiles[at_tile].widgets[index];
        widget.kind = kind;
        widget.position = at;
        widget.hidden = false;
        let update = tree::tree(&snapshot, &ids, None, 1.0, &mut app.a11y.ids);
        let found = node(&update, NodeId(app.a11y.ids.number(ids.get(id, index).unwrap())));
        assert_eq!(found.role(), role, "{kind}");
        match kind {
            "toggle" => {
                let on = if at >= 0.5 {
                    accesskit::Toggled::True
                } else {
                    accesskit::Toggled::False
                };
                assert_eq!(found.toggled(), Some(on));
                assert!(found.supports_action(Action::Click));
            }
            "button" => assert!(found.supports_action(Action::Click)),
            "meter" => assert_eq!(found.numeric_value(), Some(f64::from(at))),
            _ => {}
        }
    }
}

/// A hidden widget is not in the tree, and its tile does not name it.
#[test]
fn hidden_widgets_are_left_out() {
    let (mut app, module, knob) = app_with_filter();
    let mut snapshot = app.ui_snapshot();
    let ids = app.a11y_widget_ids(&snapshot);
    snapshot.tiles.iter_mut().find(|t| t.id == module).unwrap().widgets[knob].hidden = true;
    let update = tree::tree(&snapshot, &ids, None, 1.0, &mut app.a11y.ids);
    assert!(
        update
            .nodes
            .iter()
            .all(|(_, n)| n.label() != Some("Cutoff") || n.role() != Role::Slider)
    );
}

/// A widget with no name has no UI ID, so it gets no node, and the other
/// widgets of its tile keep theirs.
#[test]
fn a_widget_with_no_name_is_left_out() {
    let (mut app, module, knob) = app_with_filter();
    let snapshot = app.ui_snapshot();
    let tile = snapshot.tiles.iter().find(|t| t.id == module).unwrap();
    let named = app.a11y_widget_ids(&snapshot);
    // The same IDs, but with this tile's knob given no name.
    let mut ids = named.clone();
    ids.add_tile(
        module,
        true,
        tile.widgets.iter().enumerate().map(|(index, widget)| {
            let name = (index != knob).then(|| format!("w{index}"));
            (widget.kind, name)
        }),
    );
    let all = tree::tree(&snapshot, &named, None, 1.0, &mut app.a11y.ids);
    let without = tree::tree(&snapshot, &ids, None, 1.0, &mut app.a11y.ids);
    assert_eq!(without.nodes.len() + 1, all.nodes.len());
    assert!(
        without
            .nodes
            .iter()
            .all(|(_, n)| n.label() != Some(tile.widgets[knob].label.as_str()))
    );
}

/// Bounds are in the window and in physical pixels: at scale factor 2 they
/// are twice the logical layout (ui-layer.md, section 7, rule 8).
#[test]
fn bounds_are_in_physical_pixels() {
    let (mut app, module, knob) = app_with_filter();
    let view = CanvasView {
        bounds: Rect::new(Point::new(100.0, 40.0), Size::new(800.0, 600.0)),
        camera: Camera::default(),
    };
    let snapshot = app.ui_snapshot();
    let tile = snapshot.tiles.iter().find(|t| t.id == module).unwrap();
    let world = tile.widgets[knob].bounds.expect("the knob is drawn");
    let top_left = view.to_window(Point::new(world.x, world.y));
    let bottom_right = view.to_window(Point::new(world.x + world.width, world.y + world.height));

    app.a11y.scale = 1.0;
    let once = app.a11y_tree(Some(&view));
    let cutoff = widget_node(&mut app, module, knob);
    let logical = node(&once, cutoff).bounds().unwrap();
    assert!((logical.x0 - f64::from(top_left.x)).abs() < 1e-3);
    assert!((logical.y0 - f64::from(top_left.y)).abs() < 1e-3);
    assert!((logical.x1 - f64::from(bottom_right.x)).abs() < 1e-3);
    assert!((logical.y1 - f64::from(bottom_right.y)).abs() < 1e-3);
    assert!(logical.x1 > logical.x0 && logical.y1 > logical.y0);

    let _ = app.a11y_event(Event::ScaleFactor(2.0));
    let twice = app.a11y_tree(Some(&view));
    let physical = node(&twice, cutoff).bounds().unwrap();
    assert!((physical.x0 - logical.x0 * 2.0).abs() < 1e-3);
    assert!((physical.y0 - logical.y0 * 2.0).abs() < 1e-3);
    assert!((physical.x1 - logical.x1 * 2.0).abs() < 1e-3);
    assert!((physical.y1 - logical.y1 * 2.0).abs() < 1e-3);
    let tile_node = node_of(&mut app, &tile_id(module));
    assert!(node(&twice, tile_node).bounds().is_some(), "the tile has bounds too");
}

/// A node keeps its ID from one build to the next, and a control keeps it
/// while another module is added.
#[test]
fn node_ids_are_stable_across_builds() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let root = node_of(&mut app, &window_id());
    let cutoff = widget_node(&mut app, module, knob);

    app.session.controller.add_module("loom.filter.svf").unwrap();
    app.commit();
    let _ = app.a11y_tree(None);

    assert_eq!(node_of(&mut app, &window_id()), root);
    assert_eq!(widget_node(&mut app, module, knob), cutoff);
}

/// When a module is removed, the table drops its tile and controls, and an
/// action on one of their old node IDs does nothing.
#[test]
fn a_removed_module_leaves_the_table() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let cutoff = widget_node(&mut app, module, knob);
    let _ = app.a11y_tree(None);
    let before = app.a11y.ids.len();

    app.session
        .controller
        .remove_module(loom_core::ModuleId(module))
        .unwrap();
    app.commit();
    let update = app.a11y_tree(None);

    assert!(app.a11y.ids.len() < before);
    assert_eq!(app.a11y.ids.id(cutoff.0), None);
    assert!(update.nodes.iter().all(|(id, _)| *id != cutoff));
    assert!(events(&app, &request(Action::Increment, cutoff, None)).is_empty());
}

/// When the patch is replaced, its tiles get new node IDs even where a
/// module ID repeats, and the window keeps its own.
#[test]
fn a_replaced_patch_gets_new_node_ids() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let root = node_of(&mut app, &window_id());
    let tile = node_of(&mut app, &tile_id(module));
    let cutoff = widget_node(&mut app, module, knob);

    app.a11y.patch_replaced();
    let _ = app.a11y_tree(None);

    assert_eq!(node_of(&mut app, &window_id()), root);
    assert_ne!(node_of(&mut app, &tile_id(module)), tile);
    assert_ne!(widget_node(&mut app, module, knob), cutoff);
    assert_eq!(app.a11y.ids.id(cutoff.0), None, "the old ID names nothing now");
}

/// Opening another patch through the app drops the old tiles' node IDs,
/// and leaves the window's ID and the listening screen reader alone.
#[test]
fn replacing_the_patch_through_the_app_drops_tile_ids() {
    let (mut app, module, _) = app_with_filter();
    let _ = app.a11y_event(Event::Activated);
    let _ = app.a11y_tree(None);
    let root = node_of(&mut app, &window_id());
    let tile = node_of(&mut app, &tile_id(module));
    let _ = app.a11y_tree(None);

    assert!(app.replace(crate::Source::Empty));

    assert!(app.a11y.active, "the screen reader still listens");
    assert_eq!(app.a11y.ids.id(tile.0), None);
    assert_eq!(app.a11y.ids.id(root.0), Some(&window_id()));
}

/// Increment turns the knob one step, through the canvas, as one undo step.
#[test]
fn increment_turns_the_knob_as_one_undo_step() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let cutoff = widget_node(&mut app, module, knob);
    let before = position(&app, module, knob);

    let _ = app.update(Message::A11y(Event::Action(request(Action::Increment, cutoff, None))));

    let after = position(&app, module, knob);
    assert!((after - (before + STEP).min(1.0)).abs() < 1e-4, "{before} -> {after}");
    assert!(app.ui_snapshot().undo.is_some(), "the turn can be undone");

    let _ = app.update(Message::A11y(Event::Action(request(Action::Decrement, cutoff, None))));
    assert!((position(&app, module, knob) - before).abs() < 1e-4);
}

/// A request becomes the messages the pointer would send: the change, then
/// the release that ends its undo step.
#[test]
fn a_set_becomes_a_change_and_a_release() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let cutoff = widget_node(&mut app, module, knob);
    let snapshot = app.ui_snapshot();
    let ids = app.a11y_widget_ids(&snapshot);
    let messages = actions::messages(
        &snapshot,
        &ids,
        &app.a11y.ids,
        &request(Action::SetValue, cutoff, Some(ActionData::NumericValue(0.5))),
    );
    assert!(matches!(
        messages.as_slice(),
        [Message::Canvas(CanvasEvent::SetControl { .. }), Message::SliderReleased]
    ));
    let none = actions::messages(&snapshot, &ids, &app.a11y.ids, &request(Action::Click, cutoff, None));
    assert!(none.is_empty(), "a knob cannot be clicked");
}

/// SetValue moves the knob to the position asked for, kept within 0 to 1.
#[test]
fn set_value_is_clamped() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let cutoff = widget_node(&mut app, module, knob);
    let set = |app: &App, value| events(app, &request(Action::SetValue, cutoff, Some(value)));
    assert_eq!(
        set(&app, ActionData::NumericValue(3.0)),
        vec![CanvasEvent::SetControl {
            module,
            widget: knob,
            position: 1.0,
            part: ControlPart::Main,
        }]
    );
    assert!(
        set(&app, ActionData::Value("loud".into())).is_empty(),
        "a knob takes a number"
    );
}

/// A click turns a toggle over, and presses a button.
#[test]
fn click_works_a_toggle_and_a_button() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let target = widget_node(&mut app, module, knob);
    let mut snapshot = app.ui_snapshot();
    let ids = app.a11y_widget_ids(&snapshot);
    let click = |snapshot: &crate::snapshot::UiSnapshot, app: &App| {
        actions::canvas_events(snapshot, &ids, &app.a11y.ids, &request(Action::Click, target, None))
    };
    let set = |position| {
        vec![CanvasEvent::SetControl {
            module,
            widget: knob,
            position,
            part: ControlPart::Main,
        }]
    };
    let widget = |snapshot: &mut crate::snapshot::UiSnapshot, kind, position| {
        let tile = snapshot.tiles.iter_mut().find(|t| t.id == module).unwrap();
        tile.widgets[knob].kind = kind;
        tile.widgets[knob].position = position;
    };

    widget(&mut snapshot, "toggle", 0.0);
    assert_eq!(click(&snapshot, &app), set(1.0));
    widget(&mut snapshot, "toggle", 1.0);
    assert_eq!(click(&snapshot, &app), set(0.0));
    widget(&mut snapshot, "button", 0.0);
    assert_eq!(
        click(&snapshot, &app),
        vec![CanvasEvent::ButtonPressed { module, widget: knob }]
    );
}

/// Focus on a node never changes the patch: focusing a tile or a control
/// does not select anything (ui-layer.md, section 7, rule 9).
#[test]
fn focus_does_not_change_the_selection() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let tile = node_of(&mut app, &tile_id(module));
    let cutoff = widget_node(&mut app, module, knob);
    let before = app.ui_snapshot().selection;

    assert!(events(&app, &request(Action::Focus, tile, None)).is_empty());
    assert!(events(&app, &request(Action::Focus, cutoff, None)).is_empty());
    let _ = app.update(Message::A11y(Event::Action(request(Action::Focus, tile, None))));

    assert_eq!(app.ui_snapshot().selection, before);
    assert!(!app.unsaved(), "no edit was made");
}

/// Click on a tile selects its module, through the same message as a
/// pointer selection, so the inspector follows. It is not an edit.
#[test]
fn click_on_a_tile_selects_it() {
    let (mut app, module, knob) = app_with_filter();
    let other = app.session.controller.add_module("loom.filter.svf").unwrap().0;
    app.commit();
    let update = app.a11y_tree(None);
    let tile = node_of(&mut app, &tile_id(module));
    assert!(node(&update, tile).supports_action(Action::Click));
    assert!(node(&update, tile).supports_action(Action::Focus));
    assert_ne!(app.ui_snapshot().selection, vec![module]);

    assert_eq!(
        events(&app, &request(Action::Click, tile, None)),
        vec![CanvasEvent::Select(vec![module])]
    );
    let _ = app.update(Message::A11y(Event::Action(request(Action::Click, tile, None))));
    assert_eq!(app.ui_snapshot().selection, vec![module]);

    let second = node_of(&mut app, &tile_id(other));
    let _ = app.update(Message::A11y(Event::Action(request(Action::Click, second, None))));
    assert_eq!(
        app.ui_snapshot().selection,
        vec![other],
        "the selection moves, as a click moves it"
    );
    assert!(app.ui_snapshot().undo.is_none(), "selecting is not an undo step");

    // A tile has no value to step, and a control's ID is not a tile's.
    assert!(events(&app, &request(Action::Increment, tile, None)).is_empty());
    let cutoff = widget_node(&mut app, module, knob);
    assert_ne!(cutoff, tile);
    assert_eq!(tree::tile_of(&tile_id(module)), Some(module));
    assert_eq!(tree::tile_of(&tile_id(module).with("param").with("cutoff")), None);
    assert_eq!(tree::tile_of(&window_id()), None);
    assert_eq!(tree::tile_of(&UiId::panel("inspector").with(module)), None);
}

/// Click on a tile that has left the canvas selects nothing.
#[test]
fn click_on_a_removed_tile_does_nothing() {
    let (mut app, module, _) = app_with_filter();
    let _ = app.a11y_tree(None);
    let tile = node_of(&mut app, &tile_id(module));
    let mut snapshot = app.ui_snapshot();
    let ids = app.a11y_widget_ids(&snapshot);
    snapshot.tiles.retain(|t| t.id != module);
    let events = actions::canvas_events(&snapshot, &ids, &app.a11y.ids, &request(Action::Click, tile, None));
    assert!(events.is_empty());
}

/// Actions on nodes that are not controls, or that name nothing, do nothing.
#[test]
fn unknown_targets_do_nothing() {
    let (mut app, module, _) = app_with_filter();
    let _ = app.a11y_tree(None);
    let root = node_of(&mut app, &window_id());
    let tile = node_of(&mut app, &tile_id(module));
    assert!(events(&app, &request(Action::Increment, root, None)).is_empty());
    assert!(events(&app, &request(Action::Increment, tile, None)).is_empty());
    assert!(events(&app, &request(Action::Increment, NodeId(u64::MAX), None)).is_empty());
}

/// A hidden control cannot be changed, though its ID still exists.
#[test]
fn hidden_widgets_ignore_actions() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let cutoff = widget_node(&mut app, module, knob);
    let mut snapshot = app.ui_snapshot();
    let ids = app.a11y_widget_ids(&snapshot);
    let tile = snapshot.tiles.iter_mut().find(|t| t.id == module).unwrap();
    tile.widgets[knob].hidden = true;
    let events = actions::canvas_events(
        &snapshot,
        &ids,
        &app.a11y.ids,
        &request(Action::Increment, cutoff, None),
    );
    assert!(events.is_empty());
}

/// While a panel owns input, a screen reader action leaves the patch alone.
#[test]
fn actions_wait_while_a_panel_owns_input() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let cutoff = widget_node(&mut app, module, knob);
    app.overlays.open(crate::overlays::Overlay::RecentList(Vec::new()));
    assert!(!app.no_overlay(), "the recent list owns input");
    let before = position(&app, module, knob);
    let _ = app.a11y_event(Event::Action(request(Action::Increment, cutoff, None)));
    assert_eq!(before, position(&app, module, knob));
    assert!(!app.unsaved(), "no edit was made");
}

/// The refresh timer runs only once a screen reader listens, and the events
/// that only set up the layer change nothing in the patch.
#[test]
fn a_screen_reader_turns_the_layer_on() {
    let (mut app, _, _) = app_with_filter();
    assert!(!app.a11y.active);
    let _ = app.a11y_subscription();
    let _ = app.a11y_event(Event::Attached(Ok(())));
    let _ = app.a11y_event(Event::Attached(Err("no window".into())));
    assert!(!app.a11y.active, "attaching is not listening");

    let _ = app.a11y_event(Event::Activated);
    assert!(app.a11y.active);
    let _ = app.a11y_subscription();
    let _ = app.a11y_event(Event::Refresh);
    let _ = app.a11y_event(Event::Located(None));
    assert!(!app.a11y.ids.is_empty(), "locating the canvas built the tree");
    assert!(!app.unsaved(), "no edit was made");
}

/// The widget operation that finds the canvas reports the view the canvas
/// gave it, and nothing when no canvas answered.
#[test]
fn the_canvas_is_found_by_its_view() {
    use iced::advanced::widget::operation::Outcome;
    let mut find = FindCanvas(None);
    assert!(matches!(find.finish(), Outcome::Some(None)));

    let mut other = 7u32;
    find.custom(None, Rectangle::default(), &mut other);
    assert!(matches!(find.finish(), Outcome::Some(None)), "not a canvas view");

    let mut view = CanvasView {
        bounds: Rect::new(Point::new(1.0, 2.0), Size::new(3.0, 4.0)),
        camera: Camera::default(),
    };
    find.custom(None, Rectangle::default(), &mut view);
    assert!(matches!(find.finish(), Outcome::Some(Some(found)) if found == view));

    let mut visited = false;
    find.traverse(&mut |_| visited = true);
    assert!(visited);
}
