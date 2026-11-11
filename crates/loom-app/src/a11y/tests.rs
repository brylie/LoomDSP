//! Tests for the accessibility tree: UI IDs and node IDs, the tree, and
//! actions. None needs a window or a screen reader, so they run on every OS.

use super::tree::{WidgetIds, WidgetKey, canvas_id, status_id, tile_id, window_id};
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
        &tile_id(3),
        [
            (WidgetKey::Param, Some("gain".to_string())),
            (WidgetKey::Display, Some("out".to_string())),
            (WidgetKey::Param, None),
        ],
    );
    ids.add_tile(9, &tile_id(9), [(WidgetKey::Exposed, Some("Cutoff".to_string()))]);

    assert_eq!(ids.get(3, 0), Some(&tile_id(3).with("param").with("gain")));
    assert_eq!(ids.get(3, 1), Some(&tile_id(3).with("display").with("out")));
    assert_eq!(ids.get(3, 2), None);
    assert_eq!(ids.get(9, 0), Some(&tile_id(9).with("exposed").with("Cutoff")));
    assert_eq!(ids.get(3, 7), None, "no such widget");
    assert_eq!(ids.get(4, 0), None, "no such tile");
    assert_eq!(ids.find(&tile_id(3)), None, "a tile is not a widget");
}

/// On every tile of the patch that shows every panel widget, a widget's kind
/// word is `display` exactly when the panel's description says it is not a
/// control, whatever it is drawn as, and its ID agrees with the name the
/// control channel's `locate` takes for it (ui-layer.md, section 4, rule 1).
#[test]
fn kinds_come_from_the_panel_and_agree_with_locate() {
    use crate::targets::{self, Part};
    use loom_ui_core::IdKey;
    let patch = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/dev/test-widgets.ron");
    let app = App::new(crate::Args {
        patch: Some(patch),
        test_modules: true,
        ..crate::tests::app_args()
    });
    let snapshot = app.ui_snapshot();
    let ids = app.a11y_widget_ids(&snapshot);
    let graph = app.session.controller.graph();
    let hierarchy = &app.session.hierarchy;
    let name = |key: &IdKey| match key {
        IdKey::Name(name) => name.to_string(),
        IdKey::Number(number) => number.to_string(),
    };
    let (mut displays, mut counted, mut drawn) = (0, 0, std::collections::BTreeSet::new());
    for tile in &snapshot.tiles {
        for (index, widget) in tile.widgets.iter().enumerate() {
            let Some(id) = ids.get(tile.id, index) else { continue };
            drawn.insert(widget.kind);
            let control = crate::surface::is_control(graph, hierarchy, tile.id, index);
            // The kind and what follows it: a slot's part comes before them.
            let keys = &id.keys()[2..];
            assert_eq!(
                keys[0] == IdKey::from("display"),
                !control,
                "{} on tile {}: {id}",
                widget.kind,
                tile.id
            );
            if control {
                continue;
            }
            displays += 1;
            // `11:display:out` or `11:display:out:2` for a display, however
            // `locate` would write it: the name and the count must match.
            let located = targets::name(graph, hierarchy, Part::Display(tile.id, index)).unwrap();
            let bound = name(&keys[1]);
            match keys.get(2) {
                Some(count) => {
                    counted += 1;
                    assert_eq!(located, format!("{}:display:{bound}:{}", tile.id, name(count)));
                }
                None => assert!(
                    located == format!("{}:{bound}", tile.id) || located == format!("{}:display:{bound}", tile.id),
                    "{located} for {id}"
                ),
            }
        }
    }
    assert!(displays > 0, "the patch has displays");
    // The kinds a list of drawn kinds once missed: a display that is not a
    // light, a readout, a meter, or a scope.
    for kind in ["level_history", "meter", "light"] {
        assert!(drawn.contains(kind), "the patch shows a {kind}: {drawn:?}");
    }
    assert!(
        counted > 0,
        "a tile shows the same value twice, as a meter and its history do"
    );
}

/// Two widgets that show the same value get different IDs: the later ones
/// are counted from 2. A parameter and a display of the same name differ by
/// kind.
#[test]
fn widgets_bound_to_the_same_name_get_different_ids() {
    let mut ids = WidgetIds::default();
    ids.add_tile(
        11,
        &tile_id(11),
        [
            (WidgetKey::Display, Some("out".to_string())),
            (WidgetKey::Display, Some("out".to_string())),
            (WidgetKey::Display, Some("out".to_string())),
            (WidgetKey::Param, Some("out".to_string())),
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
    // The value is the parameter's own number, in its own range.
    let spec = app.control_target(module, knob, ControlPart::Main).unwrap().spec;
    assert_eq!(
        cutoff.numeric_value(),
        Some(f64::from(spec.denormalize(shown.position)))
    );
    assert_eq!(cutoff.min_numeric_value(), Some(f64::from(spec.min)));
    assert_eq!(cutoff.max_numeric_value(), Some(f64::from(spec.max)));
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

/// The app with the patch that shows every panel widget kind.
fn app_with_every_widget() -> App {
    let patch = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/dev/test-widgets.ron");
    App::new(crate::Args {
        patch: Some(patch),
        test_modules: true,
        ..crate::tests::app_args()
    })
}

/// The first visible widget drawn as `kind` that has a node: its tile and
/// its panel index.
fn first_of(app: &App, kind: &str) -> (u32, usize) {
    let snapshot = app.ui_snapshot();
    let ids = app.a11y_widget_ids(&snapshot);
    snapshot
        .tiles
        .iter()
        .find_map(|tile| {
            let index = tile
                .widgets
                .iter()
                .enumerate()
                .position(|(i, w)| w.kind == kind && !w.hidden && ids.get(tile.id, i).is_some())?;
            Some((tile.id, index))
        })
        .unwrap_or_else(|| panic!("the patch shows a {kind}"))
}

/// The node ID of one piece of a widget.
fn piece_node(app: &mut App, module: u32, widget: usize, piece: tree::Piece) -> NodeId {
    let snapshot = app.ui_snapshot();
    let ids = app.a11y_widget_ids(&snapshot);
    let id = tree::piece_id(
        ids.get(module, widget).unwrap(),
        &ids.detail(module, widget).unwrap().kind,
        piece,
    );
    node_of(app, &id)
}

/// Every kind of widget a tile draws has the role the specification's
/// table gives it, a name, and a value or state. A control can be operated,
/// and a control that holds a number says its unit in the description
/// (ui-layer.md, section 7, rule 2).
#[test]
fn every_widget_kind_has_its_role_a_name_and_a_value() {
    let mut app = app_with_every_widget();
    let update = app.a11y_tree(None);
    let roles = [
        ("knob", Role::Slider),
        ("fader", Role::Slider),
        ("number", Role::Slider),
        ("toggle", Role::Switch),
        ("button", Role::Button),
        ("radio", Role::RadioGroup),
        ("select", Role::ComboBox),
        ("xy_pad", Role::Group),
        ("range", Role::Group),
        ("meter", Role::Meter),
        ("light", Role::Image),
        ("readout", Role::Label),
        ("scope", Role::Image),
    ];
    for (kind, role) in roles {
        let (module, index) = first_of(&app, kind);
        let found = node(&update, widget_node(&mut app, module, index));
        assert_eq!(found.role(), role, "{kind}");
        // A label's text is its value, name and all, and it has no label.
        if kind != "readout" {
            assert!(found.label().is_some_and(|name| !name.is_empty()), "{kind} has a name");
        }
        match kind {
            "knob" | "fader" | "number" => {
                assert!(found.numeric_value().is_some(), "{kind}");
                assert!(found.min_numeric_value() < found.max_numeric_value(), "{kind}");
                assert!(found.description().is_some_and(|text| !text.is_empty()), "{kind}");
                for action in [Action::Increment, Action::Decrement, Action::SetValue] {
                    assert!(found.supports_action(action), "{kind}");
                }
            }
            "toggle" => {
                assert!(found.toggled().is_some());
                assert!(found.supports_action(Action::Click));
            }
            "button" => assert!(found.supports_action(Action::Click)),
            "select" => {
                assert!(found.value().is_some_and(|option| !option.is_empty()));
                assert!(found.supports_action(Action::Increment));
                assert!(found.supports_action(Action::Decrement));
                assert!(!found.supports_action(Action::Expand), "it does not open until phase 3");
            }
            "radio" => {
                let options = found.children();
                assert!(options.len() > 1);
                let chosen = options
                    .iter()
                    .filter(|option| node(&update, **option).toggled() == Some(accesskit::Toggled::True))
                    .count();
                assert_eq!(chosen, 1, "one option is chosen");
                for option in options {
                    let option = node(&update, *option);
                    assert_eq!(option.role(), Role::RadioButton);
                    assert!(option.label().is_some_and(|name| !name.is_empty()));
                    assert!(option.supports_action(Action::Click));
                }
            }
            "xy_pad" | "range" => {
                let values = found.children();
                assert_eq!(values.len(), 2, "{kind}: one slider per value");
                for value in values {
                    let value = node(&update, *value);
                    assert_eq!(value.role(), Role::Slider, "{kind}");
                    assert!(value.label().is_some_and(|name| !name.is_empty()), "{kind}");
                    assert!(value.numeric_value().is_some(), "{kind}");
                    assert!(value.description().is_some_and(|text| !text.is_empty()), "{kind}");
                    assert!(value.supports_action(Action::Increment), "{kind}");
                }
            }
            // At rest its text is "below -60 dB": words, so no number.
            "meter" => {
                assert_eq!(found.numeric_value(), None);
                assert!(found.description().is_some_and(|text| text.contains("below")));
            }
            "readout" => {
                let snapshot = app.ui_snapshot();
                let shown = &snapshot.tiles.iter().find(|t| t.id == module).unwrap().widgets[index];
                let said = found.value().expect("a readout says its reading");
                assert!(said.ends_with(shown.text.as_str()), "{said}");
                assert!(said.len() > shown.text.len() + 2, "{said} names what it reads");
                assert_eq!(found.label(), None);
            }
            "light" => assert!(found.description().is_some_and(|text| !text.is_empty())),
            _ => {}
        }
    }
}

/// A widget the app says nothing of is a plain label with its text. One
/// with a kind but no parameter is a slider over the control's travel,
/// from 0 to 1, and still takes a value and a step.
#[test]
fn a_widget_without_a_detail_or_a_parameter() {
    let (mut app, module, knob) = app_with_filter();
    let snapshot = app.ui_snapshot();
    let count = snapshot.tiles.iter().find(|t| t.id == module).unwrap().widgets.len();
    let named = |ids: &mut WidgetIds| {
        ids.add_tile(
            module,
            &tile_id(module),
            (0..count).map(|i| (WidgetKey::Param, (i == knob).then(|| "cutoff".to_string()))),
        );
    };
    let mut bare = WidgetIds::default();
    named(&mut bare);
    let id = bare.get(module, knob).unwrap().clone();
    let update = tree::tree(&snapshot, &bare, None, 1.0, &mut app.a11y.ids);
    let target = node_of(&mut app, &id);
    let found = node(&update, target);
    assert_eq!(found.role(), Role::Label);
    assert!(found.description().is_some_and(|text| !text.is_empty()));
    let up = request(Action::Increment, target, None);
    assert!(actions::canvas_events(&snapshot, &bare, &app.a11y.ids, &up).is_empty());

    let mut travel = WidgetIds::default();
    named(&mut travel);
    let kind = loom_canvas::WidgetViewKind::Knob { steps: 0 };
    travel.set_detail(
        module,
        knob,
        tree::Detail {
            kind: kind.clone(),
            name: "Cutoff".into(),
            main: None,
            second: None,
            reading: None,
        },
    );
    assert_eq!(tree::piece_id(&id, &kind, tree::Piece::Whole), id);
    let update = tree::tree(&snapshot, &travel, None, 1.0, &mut app.a11y.ids);
    let found = node(&update, target);
    assert_eq!(found.role(), Role::Slider);
    assert_eq!(found.min_numeric_value(), Some(0.0));
    assert_eq!(found.max_numeric_value(), Some(1.0));
    let set = request(Action::SetValue, target, Some(ActionData::NumericValue(3.0)));
    assert_eq!(
        actions::canvas_events(&snapshot, &travel, &app.a11y.ids, &set),
        vec![CanvasEvent::SetControl {
            module,
            widget: knob,
            position: 1.0,
            part: ControlPart::Main,
        }]
    );
}

/// A scope is an image that says its trace's lowest and highest points in
/// words, once it has a trace.
#[test]
fn a_scope_says_its_trace_in_words() {
    let mut app = app_with_every_widget();
    let (module, index) = first_of(&app, "scope");
    let mut snapshot = app.ui_snapshot();
    let ids = app.a11y_widget_ids(&snapshot);
    let scope = &mut snapshot.tiles.iter_mut().find(|t| t.id == module).unwrap().widgets[index];
    scope.trace = Some(crate::snapshot::TraceShown {
        points: 100,
        low: -0.5,
        high: 0.25,
        level: 0.5,
    });
    let update = tree::tree(&snapshot, &ids, None, 1.0, &mut app.a11y.ids);
    let found = node(&update, widget_node(&mut app, module, index));
    assert_eq!(found.role(), Role::Image);
    assert_eq!(found.description(), Some("Lowest -0.50, highest 0.25"));
}

/// A slider on a linear parameter declares the step an arrow key takes, in
/// the parameter's own units. One on a logarithmic parameter declares none:
/// an arrow key moves it by a different amount at each place.
#[test]
fn a_step_is_declared_only_where_it_is_one_number() {
    let (mut app, module, knob) = app_with_filter();
    let update = app.a11y_tree(None);
    let spec = |app: &App, widget| app.control_target(module, widget, ControlPart::Main).unwrap().spec;
    assert_eq!(spec(&app, knob).options.mapping, loom_core::ParamMapping::Logarithmic);
    assert_eq!(
        node(&update, widget_node(&mut app, module, knob)).numeric_value_step(),
        None
    );
    let snapshot = app.ui_snapshot();
    let tile = snapshot.tiles.iter().find(|t| t.id == module).unwrap();
    let linear = (0..tile.widgets.len())
        .find(|i| {
            tile.widgets[*i].kind == "knob"
                && app.control_target(module, *i, ControlPart::Main).is_some_and(|t| {
                    t.spec.options.mapping == loom_core::ParamMapping::Linear && !t.spec.options.integer
                })
        })
        .expect("the filter has a linear knob");
    let s = spec(&app, linear);
    let step = node(&update, widget_node(&mut app, module, linear))
        .numeric_value_step()
        .unwrap();
    assert!((step - f64::from(0.01 * (s.max - s.min))).abs() < 1e-6);
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
        &tile_id(module),
        (0..tile.widgets.len()).map(|index| (WidgetKey::Param, (index != knob).then(|| format!("w{index}")))),
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

/// Increment turns the knob one step, through the canvas, and Undo can
/// take it back at once.
#[test]
fn increment_turns_the_knob_and_can_be_undone() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let cutoff = widget_node(&mut app, module, knob);
    let before = position(&app, module, knob);

    let _ = app.update(Message::A11y(Event::Action(request(Action::Increment, cutoff, None))));

    let after = position(&app, module, knob);
    assert!((after - (before + 0.01).min(1.0)).abs() < 1e-4, "{before} -> {after}");
    // The turn is in an open group, and Undo closes the group and takes it.
    assert_eq!(app.undo_hint().unwrap(), "Undo: the screen reader edit");

    let _ = app.update(Message::A11y(Event::Action(request(Action::Decrement, cutoff, None))));
    assert!((position(&app, module, knob) - before).abs() < 1e-4);
}

/// A SetValue gives only its change, since the app groups it with the
/// ones that follow. A Click is one step of its own: its change, then the
/// release that ends the step.
#[test]
fn a_set_is_a_change_and_a_click_is_a_change_and_a_release() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let cutoff = widget_node(&mut app, module, knob);
    let snapshot = app.ui_snapshot();
    let ids = app.a11y_widget_ids(&snapshot);
    let messages = actions::messages(
        &snapshot,
        &ids,
        &app.a11y.ids,
        &request(Action::SetValue, cutoff, Some(ActionData::NumericValue(500.0))),
    );
    assert!(matches!(
        messages.as_slice(),
        [Message::Canvas(CanvasEvent::SetControl { .. })]
    ));
    let none = actions::messages(&snapshot, &ids, &app.a11y.ids, &request(Action::Click, cutoff, None));
    assert!(none.is_empty(), "a knob cannot be clicked");

    let mut app = app_with_every_widget();
    let _ = app.a11y_tree(None);
    let (module, toggle) = first_of(&app, "toggle");
    let target = widget_node(&mut app, module, toggle);
    let snapshot = app.ui_snapshot();
    let ids = app.a11y_widget_ids(&snapshot);
    let messages = actions::messages(&snapshot, &ids, &app.a11y.ids, &request(Action::Click, target, None));
    assert!(matches!(
        messages.as_slice(),
        [Message::Canvas(CanvasEvent::SetControl { .. }), Message::SliderReleased]
    ));
}

/// Screen reader SetValues on one control, each within 500 ms of the last,
/// make one undo step, and Undo is enabled while the group is open
/// (ui-layer.md, section 7, rule 6).
#[test]
fn quick_sets_on_one_control_are_one_undo_step() {
    use std::time::{Duration, Instant};
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let cutoff = widget_node(&mut app, module, knob);
    let before = position(&app, module, knob);
    let steps = app.session.edits.history.len();
    let t = Instant::now();
    for (i, hz) in [200.0, 400.0, 800.0].into_iter().enumerate() {
        let set = request(Action::SetValue, cutoff, Some(ActionData::NumericValue(hz)));
        let _ = app.a11y_action(&set, t + Duration::from_millis(300 * i as u64));
        assert!(app.undo_hint().is_ok(), "Undo is enabled while the group is open");
    }
    assert_eq!(app.session.edits.history.len(), steps, "the group is still open");
    assert_eq!(app.undo_hint().unwrap(), "Undo: the screen reader edit");

    // Undo closes the group first, then takes it back as one step.
    let _ = app.update(Message::Undo);
    assert!((position(&app, module, knob) - before).abs() < 1e-6);
    assert_eq!(app.session.edits.reader_group(), crate::edits::ReaderGroup::Closed);
}

/// The group ends at its deadline, on an action on another control, on a
/// Click, and on a key press, and each end is one undo step.
#[test]
fn a_screen_reader_group_ends_as_the_rule_says() {
    use crate::edits::ReaderGroup;
    use std::time::{Duration, Instant};
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let cutoff = widget_node(&mut app, module, knob);
    let other_knob = {
        let snapshot = app.ui_snapshot();
        let tile = snapshot.tiles.iter().find(|t| t.id == module).unwrap();
        (0..tile.widgets.len())
            .find(|i| *i != knob && tile.widgets[*i].kind == "knob")
            .expect("the filter has a second knob")
    };
    let other = widget_node(&mut app, module, other_knob);
    let t = Instant::now();
    let ms = Duration::from_millis;
    let up = |node| request(Action::Increment, node, None);
    let steps = |app: &App| app.session.edits.history.len();
    let start = steps(&app);

    // The deadline: the canvas wakes once and reports it.
    let _ = app.a11y_action(&up(cutoff), t);
    let _ = app.update(Message::Canvas(CanvasEvent::EditGroupDue { at: t + ms(499) }));
    assert_ne!(app.session.edits.reader_group(), ReaderGroup::Closed, "not yet due");
    let _ = app.update(Message::Canvas(CanvasEvent::EditGroupDue { at: t + ms(500) }));
    assert_eq!(app.session.edits.reader_group(), ReaderGroup::Closed);
    assert_eq!(steps(&app), start + 1);

    // Another control: the first group ends, and a new one opens.
    let _ = app.a11y_action(&up(cutoff), t + ms(1000));
    let _ = app.a11y_action(&up(other), t + ms(1100));
    assert_eq!(steps(&app), start + 2);
    assert_ne!(app.session.edits.reader_group(), ReaderGroup::Closed);

    // A late action on the same control is a new group too.
    let _ = app.a11y_action(&up(other), t + ms(1700));
    assert_eq!(steps(&app), start + 3);

    // A key press or a pointer press on the canvas ends it.
    let _ = app.update(Message::Canvas(CanvasEvent::EditGroupEnd));
    assert_eq!(app.session.edits.reader_group(), ReaderGroup::Closed);
    assert_eq!(steps(&app), start + 4);

    // An arrow key press does too, before its own group opens.
    let _ = app.a11y_action(&up(cutoff), t + ms(3000));
    let _ = app.update(Message::Canvas(CanvasEvent::ArrowPressed { at: t + ms(3100) }));
    assert_eq!(app.session.edits.reader_group(), ReaderGroup::Closed);
    assert_eq!(steps(&app), start + 5);
}

/// While a screen reader's group is open, the canvas is shown a released
/// arrow group with the same deadline, so it wakes once then. With no
/// group open it is shown none, so nothing wakes at rest.
#[test]
fn the_canvas_wakes_once_for_an_open_group() {
    use std::time::Instant;
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    assert_eq!(app.session.edits.reader_group().due(), None);
    let cutoff = widget_node(&mut app, module, knob);
    let t = Instant::now();
    let _ = app.a11y_action(&request(Action::Increment, cutoff, None), t);
    assert_eq!(
        app.session.edits.reader_group().due(),
        Some(t + crate::edits::ARROW_GROUP)
    );
}

/// SetValue takes a number in the parameter's own range and moves the
/// knob to it, kept within the range.
#[test]
fn set_value_is_in_the_parameters_range_and_clamped() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let cutoff = widget_node(&mut app, module, knob);
    let spec = app.control_target(module, knob, ControlPart::Main).unwrap().spec;
    let set = |app: &App, value| events(app, &request(Action::SetValue, cutoff, Some(value)));
    let at = |events: Vec<CanvasEvent>| match events.as_slice() {
        [
            CanvasEvent::SetControl {
                module: m,
                widget,
                position,
                part: ControlPart::Main,
            },
        ] if *m == module && *widget == knob => *position,
        other => panic!("expected one set, got {other:?}"),
    };
    let middle = spec.denormalize(0.5);
    assert!((at(set(&app, ActionData::NumericValue(f64::from(middle)))) - 0.5).abs() < 1e-4);
    assert_eq!(at(set(&app, ActionData::NumericValue(f64::from(spec.max) * 4.0))), 1.0);
    assert_eq!(at(set(&app, ActionData::NumericValue(f64::INFINITY))), 1.0);
    assert_eq!(at(set(&app, ActionData::NumericValue(f64::NEG_INFINITY))), 0.0);
    assert_eq!(at(set(&app, ActionData::NumericValue(-2.0))), 0.0);
    assert!(
        set(&app, ActionData::Value("loud".into())).is_empty(),
        "a knob takes a number"
    );
    assert!(
        set(&app, ActionData::NumericValue(f64::NAN)).is_empty(),
        "NaN is not a value"
    );
}

/// A click turns a toggle over, and presses a button.
#[test]
fn click_works_a_toggle_and_a_button() {
    let mut app = app_with_every_widget();
    let _ = app.a11y_tree(None);
    let (module, toggle) = first_of(&app, "toggle");
    let target = widget_node(&mut app, module, toggle);
    let before = position(&app, module, toggle);
    assert_eq!(
        events(&app, &request(Action::Click, target, None)),
        vec![CanvasEvent::SetControl {
            module,
            widget: toggle,
            position: if before >= 0.5 { 0.0 } else { 1.0 },
            part: ControlPart::Main,
        }]
    );
    let (module, button) = first_of(&app, "button");
    let target = widget_node(&mut app, module, button);
    assert_eq!(
        events(&app, &request(Action::Click, target, None)),
        vec![CanvasEvent::ButtonPressed { module, widget: button }]
    );
    assert!(
        events(&app, &request(Action::Increment, target, None)).is_empty(),
        "a button has no value to step"
    );
}

/// Increment and Decrement move a control by what an arrow key moves it,
/// from the one function the keyboard uses (ui-layer.md, section 7, rule
/// 5): a knob by its step, a select by one option.
#[test]
fn increment_and_decrement_take_the_arrow_keys_step() {
    let mut app = app_with_every_widget();
    let _ = app.a11y_tree(None);
    for kind in ["knob", "fader", "number", "select"] {
        let (module, index) = first_of(&app, kind);
        let target = widget_node(&mut app, module, index);
        let snapshot = app.ui_snapshot();
        let ids = app.a11y_widget_ids(&snapshot);
        let drawn = &ids.detail(module, index).unwrap().kind;
        let before = position(&app, module, index);
        for (action, up) in [(Action::Increment, true), (Action::Decrement, false)] {
            let (next, _) = loom_canvas::arrow_step(drawn, ControlPart::Main, before, 0.0, up, false).unwrap();
            let expected = if next == before {
                Vec::new()
            } else {
                vec![CanvasEvent::SetControl {
                    module,
                    widget: index,
                    position: next,
                    part: ControlPart::Main,
                }]
            };
            assert_eq!(events(&app, &request(action, target, None)), expected, "{kind}");
        }
    }
}

/// A click on a radio option chooses it, and a click on the chosen one
/// does nothing.
#[test]
fn click_on_an_option_chooses_it() {
    let mut app = app_with_every_widget();
    let _ = app.a11y_tree(None);
    let (module, radio) = first_of(&app, "radio");
    let chosen = position(&app, module, radio).round() as usize;
    let other = if chosen == 0 { 1 } else { 0 };
    let option = piece_node(&mut app, module, radio, tree::Piece::Option(other));
    assert_eq!(
        events(&app, &request(Action::Click, option, None)),
        vec![CanvasEvent::SetControl {
            module,
            widget: radio,
            position: other as f32,
            part: ControlPart::Main,
        }]
    );
    let same = piece_node(&mut app, module, radio, tree::Piece::Option(chosen));
    assert!(events(&app, &request(Action::Click, same, None)).is_empty());
    let whole = widget_node(&mut app, module, radio);
    assert!(events(&app, &request(Action::Click, whole, None)).is_empty());
}

/// Each value of an XY pad and of a range is its own slider. Increment
/// moves that value alone, and a range's handle set past the other handle
/// stops at it.
#[test]
fn each_value_of_a_two_value_control_is_its_own_slider() {
    let mut app = app_with_every_widget();
    let _ = app.a11y_tree(None);
    let second_at = |app: &App, module: u32, index: usize| {
        let snapshot = app.ui_snapshot();
        let tile = snapshot.tiles.iter().find(|t| t.id == module).unwrap();
        tile.widgets[index].second.as_ref().unwrap().position
    };
    let part_of = |events: Vec<CanvasEvent>| match events.as_slice() {
        [CanvasEvent::SetControl { part, position, .. }] => (*part, *position),
        other => panic!("expected one set, got {other:?}"),
    };

    let (module, pad) = first_of(&app, "xy_pad");
    let y = piece_node(&mut app, module, pad, tree::Piece::Second);
    let before = second_at(&app, module, pad);
    let (part, at) = part_of(events(&app, &request(Action::Decrement, y, None)));
    assert_eq!(part, ControlPart::Second);
    assert!((at - (before - 0.01)).abs() < 1e-6);
    let x = piece_node(&mut app, module, pad, tree::Piece::Main);
    let x_before = position(&app, module, pad);
    let (part, at) = part_of(events(&app, &request(Action::Increment, x, None)));
    assert_eq!(part, ControlPart::Main);
    assert!((at - (x_before + 0.01)).abs() < 1e-6);

    let (module, range) = first_of(&app, "range");
    let low = piece_node(&mut app, module, range, tree::Piece::Main);
    let high_at = second_at(&app, module, range);
    let low_at = position(&app, module, range);
    assert!(low_at < high_at, "the test patch's range is open");
    let spec = app.control_target(module, range, ControlPart::Main).unwrap().spec;
    let top = Some(ActionData::NumericValue(f64::from(spec.max) * 2.0));
    let (part, at) = part_of(events(&app, &request(Action::SetValue, low, top)));
    assert_eq!(part, ControlPart::Main);
    assert_eq!(at, high_at, "low stops at high");
    let high = piece_node(&mut app, module, range, tree::Piece::Second);
    let bottom = Some(ActionData::NumericValue(f64::NEG_INFINITY));
    let (part, at) = part_of(events(&app, &request(Action::SetValue, high, bottom)));
    assert_eq!(part, ControlPart::Second);
    assert_eq!(at, low_at, "high stops at low");
}

/// Focus on a tile changes nothing, and Focus on a control changes
/// nothing by itself: the canvas moves the focus and selects the tile
/// when it next draws (ui-layer.md, section 7, rules 9 and 20).
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

/// Nothing is built until a screen reader listens, and the events that
/// only set up the layer change nothing in the patch.
#[test]
fn a_screen_reader_turns_the_layer_on() {
    let (mut app, _, _) = app_with_filter();
    assert!(!app.a11y.active);
    let _ = app.a11y_subscription();
    let _ = app.a11y_event(Event::Attached(Ok(())));
    let _ = app.a11y_event(Event::Attached(Err("no window".into())));
    assert!(!app.a11y.active, "attaching is not listening");
    let _ = app.a11y_event(Event::Located(None));
    assert!(app.a11y.ids.is_empty(), "no tree without a screen reader");
    let _ = app.a11y_after(Touch::Changed);
    assert!(!app.a11y.locating, "and no rebuild is asked for");

    let _ = app.a11y_event(Event::Activated);
    assert!(app.a11y.active);
    assert!(app.a11y.locating, "the first tree is on its way");
    let _ = app.a11y_subscription();
    let _ = app.a11y_event(Event::Located(None));
    assert!(!app.a11y.locating);
    assert!(!app.a11y.ids.is_empty(), "locating the canvas built the tree");
    assert!(!app.unsaved(), "no edit was made");
}

/// Makes a screen reader listen, and takes the first whole tree.
fn listen(app: &mut App) {
    let _ = app.a11y_event(Event::Activated);
    let _ = app.a11y_event(Event::Located(None));
}

/// The node IDs in an update, sorted.
fn sent_ids(update: &TreeUpdate) -> Vec<NodeId> {
    let mut ids: Vec<NodeId> = update.nodes.iter().map(|(id, _)| *id).collect();
    ids.sort_unstable_by_key(|id| id.0);
    ids
}

/// The first update is the whole tree with its root. After that, a build
/// with nothing changed sends nothing, and a turned knob sends that knob's
/// node alone (ui-layer.md, section 7, rule 7).
#[test]
fn only_changed_nodes_are_sent() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_event(Event::Activated);
    let (whole, first) = app.a11y_changes(None);
    let first = first.expect("the first update is sent");
    assert!(first.tree.is_some(), "it names the root");
    assert_eq!(sent_ids(&first), sent_ids(&whole));

    let (_, again) = app.a11y_changes(None);
    assert!(again.is_none(), "nothing changed, so nothing is sent");

    let cutoff = widget_node(&mut app, module, knob);
    let _ = app.update(Message::Canvas(CanvasEvent::SetControl {
        module,
        widget: knob,
        position: 0.25,
        part: ControlPart::Main,
    }));
    let _ = app.update(Message::SliderReleased);
    let (_, changes) = app.a11y_changes(None);
    let changes = changes.expect("the knob changed");
    assert!(changes.tree.is_none(), "the root is named once");
    let changed = sent_ids(&changes);
    assert!(changed.contains(&cutoff));
    // The knob, and at most what shows the same value beside it.
    assert!(changed.len() <= 2, "{changed:?}");
    assert!(changed.len() < whole.nodes.len());
}

/// A message that changes nothing the tree reports sends nothing, and a
/// change of selection, which the tree does not report, sends nothing.
#[test]
fn a_message_that_changes_nothing_sends_nothing() {
    let (mut app, module, _) = app_with_filter();
    listen(&mut app);
    let _ = app.update(Message::Canvas(CanvasEvent::Select(vec![module])));
    assert!(app.a11y.locating, "the message asked for a rebuild");
    app.a11y.locating = false;
    let (_, changes) = app.a11y_changes(None);
    assert!(changes.is_none());
}

/// Messages that arrive while a rebuild waits for the canvas ask for no
/// second one: the one on its way holds them.
#[test]
fn one_rebuild_at_a_time() {
    let (mut app, _, _) = app_with_filter();
    listen(&mut app);
    let _ = app.a11y_after(Touch::Changed);
    assert!(app.a11y.locating);
    // Asking again while it waits changes nothing.
    let _ = app.a11y_after(Touch::Changed);
    let _ = app.a11y_after(Touch::Own);
    assert!(app.a11y.locating);
    let _ = app.a11y_event(Event::Located(None));
    assert!(!app.a11y.locating);
}

/// Runs the engine and hands the app the lights and meters that moved, as
/// the watcher thread does.
fn move_live_values(app: &mut App) {
    let tiles: Vec<u32> = app.ui_snapshot().tiles.iter().map(|t| t.id).collect();
    let _ = app.update(Message::Canvas(CanvasEvent::VisibleDisplays(tiles)));
    let watch = app.watched_lights().expect("the patch shows lights");
    app.process(4_800);
    let _ = app.update(Message::Lights(watch.read()));
}

/// The nodes of widgets that only show a value, among `ids`.
fn live_among(app: &App, ids: &[NodeId]) -> Vec<NodeId> {
    let snapshot = app.ui_snapshot();
    let widgets = app.a11y_widget_ids(&snapshot);
    ids.iter()
        .copied()
        .filter(|node| app.a11y.ids.id(node.0).is_some_and(|id| widgets.is_live(id)))
        .collect()
}

/// A node that shows a live value is held until the live timer fires, so
/// it is sent at most every 250 ms. The timer runs only while a live value
/// waits, and the last value is sent after the values stop moving
/// (ui-layer.md, section 7, rule 7).
#[test]
fn live_values_wait_for_the_timer() {
    let mut app = app_with_every_widget();
    listen(&mut app);
    assert!(!app.a11y.live_waits(), "at rest no timer runs");

    // The engine runs: meters and lights move.
    move_live_values(&mut app);
    assert!(app.a11y.live_waits(), "live values wait for the timer");

    // A rebuild for another reason holds the live nodes back.
    app.a11y.locating = false;
    let (_, changes) = app.a11y_changes(None);
    let sent = changes.map(|update| sent_ids(&update)).unwrap_or_default();
    assert_eq!(live_among(&app, &sent), Vec::new(), "live nodes wait");
    assert!(app.a11y.live_waits());

    // The timer fires: the live values are sent, and the timer stops.
    let _ = app.a11y_event(Event::LiveTick);
    assert!(app.a11y.locating, "the timer asks for a rebuild");
    app.a11y.locating = false;
    let (_, changes) = app.a11y_changes(None);
    let sent = sent_ids(&changes.expect("the lights moved"));
    assert!(!live_among(&app, &sent).is_empty(), "the moved values are sent");
    assert!(!app.a11y.live_waits(), "nothing waits, so the timer stops");

    // The values have stopped, and the last ones were sent.
    let (_, none) = app.a11y_changes(None);
    assert!(none.is_none());
}

/// A new scale factor sends every node that has bounds again, and the
/// same one asks for nothing (ui-layer.md, section 7, rule 8).
#[test]
fn a_new_scale_factor_resends_bounds() {
    let (mut app, module, knob) = app_with_filter();
    let view = CanvasView {
        bounds: Rect::new(Point::new(0.0, 0.0), Size::new(800.0, 600.0)),
        camera: Camera::default(),
    };
    let _ = app.a11y_event(Event::Activated);
    let _ = app.a11y_changes(Some(&view));
    app.a11y.locating = false;

    let _ = app.a11y_event(Event::ScaleFactor(1.0));
    assert!(!app.a11y.locating, "the same scale factor asks for nothing");
    let _ = app.a11y_event(Event::ScaleFactor(2.0));
    assert!(app.a11y.locating, "a new one asks for a rebuild");
    let (_, changes) = app.a11y_changes(Some(&view));
    let changed = sent_ids(&changes.expect("bounds changed"));
    assert!(changed.contains(&node_of(&mut app, &tile_id(module))));
    assert!(changed.contains(&widget_node(&mut app, module, knob)));
}

/// When the screen reader leaves, nothing is built, and one that comes
/// back gets the whole tree again.
#[test]
fn a_screen_reader_that_leaves_stops_the_tree() {
    let (mut app, _, _) = app_with_filter();
    listen(&mut app);
    let _ = app.a11y_event(Event::Deactivated);
    assert!(!app.a11y.is_active());
    let _ = app.a11y_after(Touch::Changed);
    let _ = app.a11y_after(Touch::Live);
    assert!(!app.a11y.locating && !app.a11y.live_waits());

    let _ = app.a11y_event(Event::Activated);
    let (whole, first) = app.a11y_changes(None);
    assert_eq!(sent_ids(&first.expect("the whole tree again")), sent_ids(&whole));
}

/// The window event that carries a new scale factor becomes the layer's
/// event, and other window events do not.
#[test]
fn a_rescaled_window_reports_its_scale_factor() {
    let window = iced::window::Id::unique();
    let status = iced::event::Status::Ignored;
    let rescaled = iced::Event::Window(iced::window::Event::Rescaled(2.0));
    assert!(matches!(super::rescaled(rescaled, status, window), Some(Event::ScaleFactor(s)) if s == 2.0));
    let focused = iced::Event::Window(iced::window::Event::Focused);
    assert!(super::rescaled(focused, status, window).is_none());
}

/// The tree reports focus on the control the canvas says has it, and on
/// the window when no control has it (ui-layer.md, section 7, rule 7).
#[test]
fn focus_is_reported_on_the_focused_control() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_event(Event::Activated);
    let (whole, _) = app.a11y_changes(None);
    let root = node_of(&mut app, &window_id());
    assert_eq!(whole.focus, root);

    let _ = app.update(Message::Canvas(CanvasEvent::FocusChanged(Some((module, knob)))));
    let cutoff = widget_node(&mut app, module, knob);
    let (whole, changes) = app.a11y_changes(None);
    assert_eq!(whole.focus, cutoff);
    assert_eq!(changes.expect("the focus moved, so an update goes").focus, cutoff);

    let (_, again) = app.a11y_changes(None);
    assert!(again.is_none(), "the same focus sends nothing");

    let _ = app.update(Message::Canvas(CanvasEvent::FocusChanged(None)));
    let (whole, changes) = app.a11y_changes(None);
    assert_eq!(whole.focus, root);
    assert!(changes.is_some());
}

/// A control and its pieces offer Focus, and a widget that only shows a
/// value does not.
#[test]
fn controls_offer_focus_and_displays_do_not() {
    let mut app = app_with_every_widget();
    let update = app.a11y_tree(None);
    for kind in ["knob", "toggle", "button", "select", "radio", "xy_pad"] {
        let (module, index) = first_of(&app, kind);
        let found = node(&update, widget_node(&mut app, module, index));
        assert!(found.supports_action(Action::Focus), "{kind}");
        for piece in found.children() {
            assert!(node(&update, *piece).supports_action(Action::Focus), "{kind} piece");
        }
    }
    for kind in ["meter", "light", "readout", "scope"] {
        let (module, index) = first_of(&app, kind);
        let found = node(&update, widget_node(&mut app, module, index));
        assert!(!found.supports_action(Action::Focus), "{kind}");
    }
}

/// The Focus action on a control asks the canvas to move keyboard focus
/// there, with a new key each time. It changes nothing in the patch, and
/// the canvas selects the tile. Focus on a piece asks for its control. Focus
/// on a display, or on a hidden control, asks for nothing.
#[test]
fn the_focus_action_asks_the_canvas_for_focus() {
    let mut app = app_with_every_widget();
    let _ = app.a11y_tree(None);
    let t = std::time::Instant::now();
    let (module, knob) = first_of(&app, "knob");
    let selection = app.session.selection.clone();
    let (key, _) = app.a11y.focus_request();

    let target = widget_node(&mut app, module, knob);
    let _ = app.a11y_action(&request(Action::Focus, target, None), t);
    assert_eq!(app.a11y.focus_request(), (key + 1, Some((module, knob))));
    assert_eq!(app.session.selection, selection, "the canvas selects, when it draws");
    assert!(!app.unsaved(), "focus is not an edit");

    let (pad_module, pad) = first_of(&app, "xy_pad");
    let y = piece_node(&mut app, pad_module, pad, tree::Piece::Second);
    let _ = app.a11y_action(&request(Action::Focus, y, None), t);
    assert_eq!(app.a11y.focus_request(), (key + 2, Some((pad_module, pad))));

    let (light_module, light) = first_of(&app, "light");
    let display = widget_node(&mut app, light_module, light);
    let _ = app.a11y_action(&request(Action::Focus, display, None), t);
    assert_eq!(app.a11y.focus_request().0, key + 2, "a display takes no focus");
    let tile = node_of(&mut app, &tile_id(module));
    let _ = app.a11y_action(&request(Action::Focus, tile, None), t);
    assert_eq!(app.a11y.focus_request().0, key + 2, "a tile takes no focus");
}

/// Focus moving to another control ends a screen reader's undo group, and
/// a replaced patch forgets the focus, since its module IDs mean nothing
/// in the new one.
#[test]
fn focus_ends_the_group_and_a_new_patch_forgets_it() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let cutoff = widget_node(&mut app, module, knob);
    let steps = app.session.edits.history.len();
    let _ = app.a11y_action(&request(Action::Increment, cutoff, None), std::time::Instant::now());
    let _ = app.update(Message::Canvas(CanvasEvent::FocusChanged(Some((module, knob)))));
    assert_eq!(app.session.edits.reader_group(), crate::edits::ReaderGroup::Closed);
    assert_eq!(app.session.edits.history.len(), steps + 1);

    let _ = app.a11y_action(&request(Action::Focus, cutoff, None), std::time::Instant::now());
    assert!(app.a11y.focus_request().1.is_some());
    app.a11y.patch_replaced();
    assert_eq!(app.a11y.focus_request().1, None);
    let update = app.a11y_tree(None);
    assert_eq!(update.focus, node_of(&mut app, &window_id()));
}

/// While a panel owns the input, the tiles are hidden from a screen
/// reader, focus is not reported inside them, and what is inside a tile
/// keeps the value last sent. When the panel closes, the tiles show again
/// with their current values (ui-layer.md, section 7, rule 7).
#[test]
fn tiles_below_a_panel_are_hidden_and_keep_their_values() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_event(Event::Activated);
    let (whole, _) = app.a11y_changes(None);
    let tile = node_of(&mut app, &tile_id(module));
    let cutoff = widget_node(&mut app, module, knob);
    assert!(!node(&whole, tile).is_hidden());
    let _ = app.update(Message::Canvas(CanvasEvent::FocusChanged(Some((module, knob)))));
    let _ = app.a11y_changes(None);

    app.overlays.open(crate::overlays::Overlay::RecentList(Vec::new()));
    // The patch changes behind the panel, as MIDI or a script can do.
    app.set_param(module, 0, 0.5);
    let _ = app.update(Message::Canvas(CanvasEvent::SetControl {
        module,
        widget: knob,
        position: 0.1,
        part: ControlPart::Main,
    }));
    let _ = app.update(Message::SliderReleased);
    let (whole, changes) = app.a11y_changes(None);
    assert!(node(&whole, tile).is_hidden());
    assert_eq!(
        whole.focus,
        node_of(&mut app, &window_id()),
        "no focus inside a hidden tile"
    );
    let sent = sent_ids(&changes.expect("the tile is sent, hidden"));
    assert!(sent.contains(&tile));
    assert!(!sent.contains(&cutoff), "the knob keeps the value last sent");

    app.overlays = crate::overlays::Overlays::default();
    let (whole, changes) = app.a11y_changes(None);
    assert!(!node(&whole, tile).is_hidden());
    let sent = sent_ids(&changes.expect("the tiles show again"));
    assert!(sent.contains(&tile) && sent.contains(&cutoff));
}

/// A live value off screen keeps the value last sent, and nothing waits
/// for it, so the live timer does not run for a meter nobody can see. When
/// it comes on screen it is sent again.
#[test]
fn live_values_off_screen_are_not_sent() {
    let mut app = app_with_every_widget();
    let on_screen = CanvasView {
        bounds: Rect::new(Point::new(0.0, 0.0), Size::new(4000.0, 4000.0)),
        camera: Camera::default(),
    };
    // The canvas is a small corner far from every tile.
    let away = CanvasView {
        bounds: Rect::new(Point::new(-9000.0, -9000.0), Size::new(1.0, 1.0)),
        camera: Camera::default(),
    };
    let _ = app.a11y_event(Event::Activated);
    let _ = app.a11y_changes(Some(&away));
    move_live_values(&mut app);
    assert!(app.a11y.live_waits());

    app.a11y.flush_live = true;
    let (_, changes) = app.a11y_changes(Some(&away));
    let sent = changes.map(|update| sent_ids(&update)).unwrap_or_default();
    assert_eq!(live_among(&app, &sent), Vec::new(), "off screen, nothing is sent");
    assert!(!app.a11y.live_waits(), "and the timer stops");

    // On screen now: the bounds changed, so the live nodes wait, then go.
    let _ = app.a11y_changes(Some(&on_screen));
    assert!(app.a11y.live_waits());
    app.a11y.flush_live = true;
    let (_, changes) = app.a11y_changes(Some(&on_screen));
    let sent = sent_ids(&changes.expect("on screen, the values are sent"));
    assert!(!live_among(&app, &sent).is_empty());
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

/// A meter reports the number its text shows, in dB, with the range the
/// meter draws, never the stored level from 0 to 1. The tree reads the
/// number the text was written from (ui-layer.md, section 7, rule 6).
#[test]
fn a_meter_reports_the_number_its_text_shows() {
    let mut app = app_with_every_widget();
    move_live_values(&mut app);
    let snapshot = app.ui_snapshot();
    let ids = app.a11y_widget_ids(&snapshot);
    let update = app.a11y_tree(None);
    let mut numbered = 0;
    for tile in &snapshot.tiles {
        for (index, widget) in tile.widgets.iter().enumerate() {
            if widget.kind != "meter" {
                continue;
            }
            let reading = ids.detail(tile.id, index).unwrap().reading;
            let found = node(&update, widget_node(&mut app, tile.id, index));
            assert_eq!(found.numeric_value(), reading.map(|reading| f64::from(reading.value)));
            assert_eq!(found.description(), Some(widget.text.as_str()));
            let Some(reading) = reading else { continue };
            numbered += 1;
            assert_eq!(found.min_numeric_value(), Some(f64::from(reading.min)));
            assert_eq!(found.max_numeric_value(), Some(f64::from(reading.max)));
            assert!(
                widget.text.contains(&format!("{:.1}", reading.value)),
                "{} says {}",
                widget.text,
                reading.value
            );
            assert!(
                (reading.value - widget.position).abs() > f32::EPSILON,
                "the stored level is not the number"
            );
        }
    }
    assert!(numbered > 0, "a meter moved and shows a number");
}

/// A Focus action that arrives while nothing else happens still moves the
/// focus: the runtime draws the window after every message, and the canvas
/// applies the request on that redraw. The control's tile is selected, as
/// a press on the control selects it, so an arrow key then changes that
/// control and the focus stays (ui-layer.md, section 7, rule 20).
#[test]
fn focus_on_a_control_selects_its_tile_and_arrow_keys_reach_it() {
    use iced::keyboard::{Key, key::Named};
    let (mut app, module, knob) = app_with_filter();
    let other = app.session.controller.add_module("loom.filter.svf").unwrap().0;
    app.commit();
    let _ = app.update(Message::Canvas(CanvasEvent::Select(vec![other])));
    let mut ui = crate::ui_test::Ui::with_app(app);
    // The first frame only notes the request's key.
    let _ = ui.redraw();
    let _ = ui.app.a11y_tree(None);
    let cutoff = widget_node(&mut ui.app, module, knob);
    let steps = ui.app.session.edits.history.len();

    // The action and the one redraw that follows any message.
    ui.send(Message::A11y(Event::Action(request(Action::Focus, cutoff, None))));
    let drawn = ui.redraw();
    assert!(drawn.iter().any(
        |message| matches!(message, Message::Canvas(CanvasEvent::FocusChanged(Some(at))) if *at == (module, knob))
    ));
    assert_eq!(ui.app.ui_snapshot().selection, vec![module]);
    assert_eq!(
        ui.app.session.edits.history.len(),
        steps,
        "selecting is not an undo step"
    );
    assert!(!ui.app.unsaved(), "nor an edit");
    assert_eq!(ui.app.a11y_tree(None).focus, cutoff);

    let before = ui
        .app
        .ui_snapshot()
        .tiles
        .iter()
        .find(|t| t.id == module)
        .unwrap()
        .widgets[knob]
        .position;
    let _ = ui.key_down(Key::Named(Named::ArrowUp), false);
    let _ = ui.key_up(Key::Named(Named::ArrowUp));
    let after = ui
        .app
        .ui_snapshot()
        .tiles
        .iter()
        .find(|t| t.id == module)
        .unwrap()
        .widgets[knob]
        .position;
    assert!(after > before, "the arrow key moved the focused control");
    assert_eq!(ui.app.a11y_tree(None).focus, cutoff, "and the focus stayed");
}

/// `--measure-tree` runs the layer as if a screen reader listened from the
/// start, and times each build. A normal run does neither.
#[test]
fn a_measured_layer_builds_and_times_the_tree() {
    assert!(!Access::default().is_active());
    let (mut app, _, _) = app_with_filter();
    app.a11y = Access::measured();
    assert!(app.a11y.is_active());

    let before = BuildTimes::process();
    let _ = app.a11y_event(Event::Located(None));
    let after = BuildTimes::process();
    // Other tests build trees too, but only a measured layer counts them.
    assert!(after.count > before.count);
    assert!(after.total >= before.total && after.worst >= before.worst);
    assert!(after.average() <= after.worst);
    assert_eq!(BuildTimes::default().average(), Duration::ZERO);
}

/// A timer's poll that changes nothing builds no tree, however many come.
/// One that changes the status line asks for one build. With no screen
/// reader, a poll takes no key at all (ui-layer.md, section 7, rule 7).
#[test]
fn a_poll_that_changes_nothing_builds_no_tree() {
    let (mut app, _, _) = app_with_filter();
    app.a11y = Access::measured();
    app.a11y.locating = false;

    let before = app.a11y_poll_key();
    for _ in 0..20 {
        for poll in [
            Message::AudioPoll,
            Message::MidiPoll,
            Message::Autosave,
            Message::MidiGhosts,
        ] {
            let _ = app.update(poll);
            assert!(!app.a11y.locating, "a poll that changed nothing asks for no build");
        }
    }
    assert_eq!(app.a11y_poll_key(), before);

    // The same poll, after something it reports changed underneath it.
    let _ = app.a11y_after(Touch::Poll(Some(before)));
    assert!(!app.a11y.locating);
    app.alerts.say("The audio device is gone");
    assert_ne!(app.a11y_poll_key(), before);
    let _ = app.a11y_after(Touch::Poll(Some(before)));
    assert!(app.a11y.locating, "a changed status asks for one build");
    let _ = app.a11y_after(Touch::Poll(Some(before)));
    assert!(app.a11y.locating, "and one build is on its way, not two");

    // Each part of the key is something the tree reports.
    let key = app.a11y_poll_key();
    app.a11y.set_focus(Some((1, 0)));
    assert_ne!(app.a11y_poll_key(), key);
}

/// A label node never has both a label and a value. An adapter gives a
/// label node's label in place of its value (`accesskit_macos` 0.27.2,
/// `label_is_exposed_in_value`), so the value would not be read.
#[test]
fn a_label_node_never_hides_its_value() {
    let mut app = app_with_every_widget();
    let update = app.a11y_tree(None);
    let mut labels = 0;
    for (_, found) in &update.nodes {
        if found.role() == Role::Label {
            labels += 1;
            assert!(
                found.label().is_none() || found.value().is_none(),
                "{:?} would hide {:?}",
                found.label(),
                found.value()
            );
        }
    }
    assert!(labels > 1, "the status line and a readout");
}

/// A range handle set by a screen reader stops at the other handle from
/// the side it is on. A typed value can leave low above high, and then low
/// moves freely above and stops when it comes down to high.
#[test]
fn a_range_handle_stops_on_its_own_side() {
    use actions::stop_at;
    // Low below high, the usual way round.
    assert_eq!(stop_at(0.9, 0.2, 0.8), 0.8);
    assert_eq!(stop_at(0.5, 0.2, 0.8), 0.5);
    assert_eq!(stop_at(0.1, 0.8, 0.2), 0.2, "high stops at low");
    // Low above high.
    assert_eq!(stop_at(0.9, 0.8, 0.2), 0.9, "low moves away from high");
    assert_eq!(stop_at(0.1, 0.8, 0.2), 0.2);
    assert_eq!(stop_at(0.1, 0.2, 0.8), 0.1, "high moves away from low");
    // Together: the handle may go either way.
    assert_eq!(stop_at(0.3, 0.5, 0.5), 0.3);
}

/// A Click that changes nothing, such as one on the option already
/// chosen, still ends an open undo group, as a pointer press does. A
/// request for a node that is gone ends nothing.
#[test]
fn a_click_that_changes_nothing_ends_the_group() {
    use crate::edits::ReaderGroup;
    let mut app = app_with_every_widget();
    let _ = app.a11y_tree(None);
    let (module, knob) = first_of(&app, "knob");
    let (radio_module, radio) = first_of(&app, "radio");
    let now = std::time::Instant::now();
    let knob_node = widget_node(&mut app, module, knob);
    let steps = app.session.edits.history.len();

    // Whichever way the knob can still move.
    let step = [Action::Increment, Action::Decrement]
        .into_iter()
        .find(|action| !events(&app, &request(*action, knob_node, None)).is_empty())
        .expect("the knob can move one way");
    let _ = app.a11y_action(&request(step, knob_node, None), now);
    assert_ne!(app.session.edits.reader_group(), ReaderGroup::Closed);
    // A node the window does not show leaves the group open.
    let gone = request(Action::Click, NodeId(u64::MAX), None);
    assert!(app.a11y_action(&gone, now).is_empty());
    assert_ne!(app.session.edits.reader_group(), ReaderGroup::Closed);

    let chosen = position(&app, radio_module, radio).round() as usize;
    let option = piece_node(&mut app, radio_module, radio, tree::Piece::Option(chosen));
    assert!(app.a11y_action(&request(Action::Click, option, None), now).is_empty());
    assert_eq!(app.session.edits.reader_group(), ReaderGroup::Closed);
    assert_eq!(
        app.session.edits.history.len(),
        steps + 1,
        "the knob's step is recorded"
    );

    // The next edit on the knob is a step of its own, though it is in time.
    let _ = app.a11y_action(&request(step, knob_node, None), now);
    app.reader_event(crate::edits::ReaderEvent::Close);
    assert_eq!(app.session.edits.history.len(), steps + 2);
}

/// While an arrow key is held, its group owns the edit in progress, so a
/// screen reader's action does nothing, as Undo does nothing then.
#[test]
fn an_action_waits_while_an_arrow_key_is_held() {
    use crate::edits::{ArrowEvent, ArrowGroup, ReaderGroup};
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(None);
    let cutoff = widget_node(&mut app, module, knob);
    let now = std::time::Instant::now();
    app.arrow_event(ArrowEvent::Step(now));
    assert_eq!(app.session.edits.arrow_group(), ArrowGroup::Held);
    let before = position(&app, module, knob);

    assert!(
        app.a11y_action(&request(Action::Increment, cutoff, None), now)
            .is_empty()
    );
    assert_eq!(position(&app, module, knob), before);
    assert_eq!(app.session.edits.reader_group(), ReaderGroup::Closed);
    assert_eq!(
        app.session.edits.arrow_group(),
        ArrowGroup::Held,
        "the key still owns the edit"
    );

    // The key comes up: the action works again.
    app.arrow_event(ArrowEvent::Released(now));
    assert!(
        !app.a11y_action(&request(Action::Increment, cutoff, None), now)
            .is_empty()
    );
    assert!(position(&app, module, knob) > before);
}

/// With a text field in focus and a screen reader listening, one message
/// asks which field has focus once. The layer's own messages do not ask
/// again, so the tree is not rebuilt in a loop.
#[test]
fn a_focused_field_does_not_rebuild_the_tree_in_a_loop() {
    let (app, _, _) = app_with_filter();
    let mut ui = crate::ui_test::Ui::with_app(app);
    let _ = ui.redraw();
    // A real field with real focus, so each time the app asks, the
    // answer is that a field has it.
    ui.focus(crate::patch_info::TAG_INPUT);
    assert!(ui.text_field_focused(), "the tag field took focus");
    let _ = ui.app.a11y_event(Event::Activated);
    ui.app.a11y.locating = false;
    ui.app.text_focus.any = true;

    let task = ui.app.update(Message::Canvas(CanvasEvent::Select(Vec::new())));
    let log = ui.run_task(task);
    let rebuilds = log
        .iter()
        .filter(|message| matches!(message, Message::A11y(Event::Located(_))))
        .count();
    let asked = log
        .iter()
        .filter(|message| matches!(message, Message::TextFocus(_)))
        .count();
    assert!(rebuilds <= 2, "{rebuilds} rebuilds for one message");
    assert!(asked <= 1, "focus was asked {asked} times");
}
