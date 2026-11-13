//! Tests for the accessibility tree: UI IDs and node IDs, the tree, and
//! actions. None needs a window or a screen reader, so they run on every OS.

use super::tree::{WidgetIds, WidgetKey, canvas_id, tile_id, window_id};
use super::*;

use crate::places::Places;
use crate::{App, Message};
use accesskit::{Action, ActionData, Node, NodeId, Role, TreeId};
use loom_canvas::{Camera, CanvasEvent, CanvasView, ControlPart};
use loom_core::ModuleNames as _;
use loom_ui_core::{Point, Rect, Size, UiId};

/// A screen reader's action at `at`, and the turn after it, where its
/// messages run. True when it sent any.
fn act(app: &mut App, request: &ActionRequest, at: std::time::Instant) -> bool {
    let sent = app.a11y_act(request, at);
    app.run_posted();
    sent
}

/// [`act`] in the `Ui` harness.
fn ui_act(ui: &mut crate::ui_test::Ui, request: &ActionRequest, at: std::time::Instant) -> bool {
    let sent = ui.app.a11y_act(request, at);
    for message in ui.app.take_posted() {
        let _ = ui.deliver(message);
    }
    sent
}

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
    let update = app.a11y_tree(&Places::default());
    let tile = snapshot.tiles.iter().find(|t| t.id == module).unwrap();
    let shown = &tile.widgets[knob];

    let root = node_of(&mut app, &window_id());
    let tile_node = node_of(&mut app, &tile_id(module));
    let status = node_of(&mut app, &tree::fixed_id(crate::bars::STATUS_ID));
    assert_eq!(update.tree.as_ref().unwrap().root, root);
    assert_eq!(update.focus, root);
    assert_eq!(node(&update, root).role(), Role::Window);
    assert!(node(&update, root).children().contains(&tile_node));
    // The status line is the last item of the bar at the bottom.
    let bar = node_of(&mut app, &tree::stats_id());
    assert!(node(&update, root).children().contains(&bar));
    assert_eq!(node(&update, bar).children().last(), Some(&status));
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
    let update = app.a11y_tree(&Places::default());
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
    let update = app.a11y_tree(&Places::default());
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
    let update = tree::tree(
        &snapshot,
        &app.a11y_frames(),
        &bare,
        &Places::of_canvas(None),
        1.0,
        &mut app.a11y.ids,
    );
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
    let update = tree::tree(
        &snapshot,
        &app.a11y_frames(),
        &travel,
        &Places::of_canvas(None),
        1.0,
        &mut app.a11y.ids,
    );
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
    let update = tree::tree(
        &snapshot,
        &app.a11y_frames(),
        &ids,
        &Places::of_canvas(None),
        1.0,
        &mut app.a11y.ids,
    );
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
    let update = app.a11y_tree(&Places::default());
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
    let update = tree::tree(
        &snapshot,
        &app.a11y_frames(),
        &ids,
        &Places::of_canvas(None),
        1.0,
        &mut app.a11y.ids,
    );
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
    let all = tree::tree(
        &snapshot,
        &app.a11y_frames(),
        &named,
        &Places::of_canvas(None),
        1.0,
        &mut app.a11y.ids,
    );
    let without = tree::tree(
        &snapshot,
        &app.a11y_frames(),
        &ids,
        &Places::of_canvas(None),
        1.0,
        &mut app.a11y.ids,
    );
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
    let once = app.a11y_tree(&Places::of_canvas(Some(view)));
    let cutoff = widget_node(&mut app, module, knob);
    let logical = node(&once, cutoff).bounds().unwrap();
    assert!((logical.x0 - f64::from(top_left.x)).abs() < 1e-3);
    assert!((logical.y0 - f64::from(top_left.y)).abs() < 1e-3);
    assert!((logical.x1 - f64::from(bottom_right.x)).abs() < 1e-3);
    assert!((logical.y1 - f64::from(bottom_right.y)).abs() < 1e-3);
    assert!(logical.x1 > logical.x0 && logical.y1 > logical.y0);

    app.a11y_event(Event::ScaleFactor(2.0));
    let twice = app.a11y_tree(&Places::of_canvas(Some(view)));
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
    let _ = app.a11y_tree(&Places::default());
    let root = node_of(&mut app, &window_id());
    let cutoff = widget_node(&mut app, module, knob);

    app.session.controller.add_module("loom.filter.svf").unwrap();
    app.commit();
    let _ = app.a11y_tree(&Places::default());

    assert_eq!(node_of(&mut app, &window_id()), root);
    assert_eq!(widget_node(&mut app, module, knob), cutoff);
}

/// When a module is removed, the table drops its tile and controls, and an
/// action on one of their old node IDs does nothing.
#[test]
fn a_removed_module_leaves_the_table() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(&Places::default());
    let cutoff = widget_node(&mut app, module, knob);
    let _ = app.a11y_tree(&Places::default());
    let before = app.a11y.ids.len();

    app.session
        .controller
        .remove_module(loom_core::ModuleId(module))
        .unwrap();
    app.commit();
    let update = app.a11y_tree(&Places::default());

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
    let _ = app.a11y_tree(&Places::default());
    let root = node_of(&mut app, &window_id());
    let tile = node_of(&mut app, &tile_id(module));
    let cutoff = widget_node(&mut app, module, knob);

    app.a11y.patch_replaced();
    let _ = app.a11y_tree(&Places::default());

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
    app.a11y_event(Event::Activated);
    let _ = app.a11y_tree(&Places::default());
    let root = node_of(&mut app, &window_id());
    let tile = node_of(&mut app, &tile_id(module));
    let _ = app.a11y_tree(&Places::default());

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
    let _ = app.a11y_tree(&Places::default());
    let cutoff = widget_node(&mut app, module, knob);
    let before = position(&app, module, knob);

    let _ = app.send(Message::A11y(Event::Action(request(Action::Increment, cutoff, None))));

    let after = position(&app, module, knob);
    assert!((after - (before + 0.01).min(1.0)).abs() < 1e-4, "{before} -> {after}");
    // The turn is in an open group, and Undo closes the group and takes it.
    assert_eq!(app.undo_hint().unwrap(), "Undo: the screen reader edit");

    let _ = app.send(Message::A11y(Event::Action(request(Action::Decrement, cutoff, None))));
    assert!((position(&app, module, knob) - before).abs() < 1e-4);
}

/// A SetValue gives only its change, since the app groups it with the
/// ones that follow. A Click is one step of its own: its change, then the
/// release that ends the step.
#[test]
fn a_set_is_a_change_and_a_click_is_a_change_and_a_release() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(&Places::default());
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
    let _ = app.a11y_tree(&Places::default());
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
    let _ = app.a11y_tree(&Places::default());
    let cutoff = widget_node(&mut app, module, knob);
    let before = position(&app, module, knob);
    let steps = app.session.edits.history.len();
    let t = Instant::now();
    for (i, hz) in [200.0, 400.0, 800.0].into_iter().enumerate() {
        let set = request(Action::SetValue, cutoff, Some(ActionData::NumericValue(hz)));
        act(&mut app, &set, t + Duration::from_millis(300 * i as u64));
        assert!(app.undo_hint().is_ok(), "Undo is enabled while the group is open");
    }
    assert_eq!(app.session.edits.history.len(), steps, "the group is still open");
    assert_eq!(app.undo_hint().unwrap(), "Undo: the screen reader edit");

    // Undo closes the group first, then takes it back as one step.
    let _ = app.send(Message::Undo);
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
    let _ = app.a11y_tree(&Places::default());
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
    act(&mut app, &up(cutoff), t);
    let _ = app.send(Message::Canvas(CanvasEvent::EditGroupDue { at: t + ms(499) }));
    assert_ne!(app.session.edits.reader_group(), ReaderGroup::Closed, "not yet due");
    let _ = app.send(Message::Canvas(CanvasEvent::EditGroupDue { at: t + ms(500) }));
    assert_eq!(app.session.edits.reader_group(), ReaderGroup::Closed);
    assert_eq!(steps(&app), start + 1);

    // Another control: the first group ends, and a new one opens.
    act(&mut app, &up(cutoff), t + ms(1000));
    act(&mut app, &up(other), t + ms(1100));
    assert_eq!(steps(&app), start + 2);
    assert_ne!(app.session.edits.reader_group(), ReaderGroup::Closed);

    // A late action on the same control is a new group too.
    act(&mut app, &up(other), t + ms(1700));
    assert_eq!(steps(&app), start + 3);

    // A key press or a pointer press on the canvas ends it.
    let _ = app.send(Message::Canvas(CanvasEvent::EditGroupEnd));
    assert_eq!(app.session.edits.reader_group(), ReaderGroup::Closed);
    assert_eq!(steps(&app), start + 4);

    // An arrow key press does too, before its own group opens.
    act(&mut app, &up(cutoff), t + ms(3000));
    let _ = app.send(Message::Canvas(CanvasEvent::ArrowPressed { at: t + ms(3100) }));
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
    let _ = app.a11y_tree(&Places::default());
    assert_eq!(app.session.edits.reader_group().due(), None);
    let cutoff = widget_node(&mut app, module, knob);
    let t = Instant::now();
    act(&mut app, &request(Action::Increment, cutoff, None), t);
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
    let _ = app.a11y_tree(&Places::default());
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
    let _ = app.a11y_tree(&Places::default());
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
    let _ = app.a11y_tree(&Places::default());
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
    let _ = app.a11y_tree(&Places::default());
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
    let _ = app.a11y_tree(&Places::default());
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
    let _ = app.a11y_tree(&Places::default());
    let tile = node_of(&mut app, &tile_id(module));
    let cutoff = widget_node(&mut app, module, knob);
    let before = app.ui_snapshot().selection;

    assert!(events(&app, &request(Action::Focus, tile, None)).is_empty());
    assert!(events(&app, &request(Action::Focus, cutoff, None)).is_empty());
    let _ = app.send(Message::A11y(Event::Action(request(Action::Focus, tile, None))));

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
    let update = app.a11y_tree(&Places::default());
    let tile = node_of(&mut app, &tile_id(module));
    assert!(node(&update, tile).supports_action(Action::Click));
    assert!(node(&update, tile).supports_action(Action::Focus));
    assert_ne!(app.ui_snapshot().selection, vec![module]);

    assert_eq!(
        events(&app, &request(Action::Click, tile, None)),
        vec![CanvasEvent::Select(vec![module])]
    );
    let _ = app.send(Message::A11y(Event::Action(request(Action::Click, tile, None))));
    assert_eq!(app.ui_snapshot().selection, vec![module]);

    let second = node_of(&mut app, &tile_id(other));
    let _ = app.send(Message::A11y(Event::Action(request(Action::Click, second, None))));
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
    let _ = app.a11y_tree(&Places::default());
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
    let _ = app.a11y_tree(&Places::default());
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
    let _ = app.a11y_tree(&Places::default());
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
    let _ = app.a11y_tree(&Places::default());
    let cutoff = widget_node(&mut app, module, knob);
    app.overlays.open(crate::overlays::Overlay::RecentList(Vec::new()));
    assert!(!app.no_overlay(), "the recent list owns input");
    let before = position(&app, module, knob);
    let _ = app.send(Message::A11y(Event::Action(request(Action::Increment, cutoff, None))));
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
    app.a11y_event(Event::Attached(Ok(())));
    app.a11y_event(Event::Attached(Err("no window".into())));
    assert!(!app.a11y.active, "attaching is not listening");
    app.a11y_event(Event::Located(Places::default()));
    assert!(app.a11y.ids.is_empty(), "no tree without a screen reader");
    app.a11y_after(Touch::Changed);
    assert!(!app.a11y.locating, "and no rebuild is asked for");

    app.a11y_event(Event::Activated);
    assert!(app.a11y.active);
    assert!(app.a11y.locating, "the first tree is on its way");
    let _ = app.a11y_subscription();
    app.a11y_event(Event::Located(Places::default()));
    assert!(!app.a11y.locating);
    assert!(!app.a11y.ids.is_empty(), "locating the canvas built the tree");
    assert!(!app.unsaved(), "no edit was made");
}

/// Makes a screen reader listen, and takes the first whole tree.
fn listen(app: &mut App) {
    app.a11y_event(Event::Activated);
    app.a11y_event(Event::Located(Places::default()));
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
    app.a11y_event(Event::Activated);
    let (whole, first) = app.a11y_changes(&Places::default());
    let first = first.expect("the first update is sent");
    assert!(first.tree.is_some(), "it names the root");
    assert_eq!(sent_ids(&first), sent_ids(&whole));

    let (_, again) = app.a11y_changes(&Places::default());
    assert!(again.is_none(), "nothing changed, so nothing is sent");

    let cutoff = widget_node(&mut app, module, knob);
    let _ = app.send(Message::Canvas(CanvasEvent::SetControl {
        module,
        widget: knob,
        position: 0.25,
        part: ControlPart::Main,
    }));
    let _ = app.send(Message::SliderReleased);
    let (_, changes) = app.a11y_changes(&Places::default());
    let changes = changes.expect("the knob changed");
    assert!(changes.tree.is_none(), "the root is named once");
    let changed = sent_ids(&changes);
    assert!(changed.contains(&cutoff));
    // The knob, at most what shows the same value beside it, and the
    // toolbar's Undo and Redo, which now have a step to name.
    let undo = node_of(&mut app, &tree::toolbar_id().with("undo"));
    assert!(changed.contains(&undo));
    assert!(changed.len() <= 4, "{changed:?}");
    assert!(changed.len() < whole.nodes.len());
}

/// A message that changes nothing the tree reports sends nothing. A new
/// selection changes the inspector, which the tree reports; the same
/// selection again changes nothing.
#[test]
fn a_message_that_changes_nothing_sends_nothing() {
    let (mut app, module, _) = app_with_filter();
    listen(&mut app);
    let _ = app.send(Message::Canvas(CanvasEvent::Select(vec![module])));
    app.a11y.locating = false;
    let (_, changes) = app.a11y_changes(&Places::default());
    assert!(changes.is_some(), "the inspector now shows the module");

    let _ = app.send(Message::Canvas(CanvasEvent::Select(vec![module])));
    assert!(app.a11y.locating, "the message asked for a rebuild");
    app.a11y.locating = false;
    let (_, changes) = app.a11y_changes(&Places::default());
    assert!(changes.is_none());
}

/// Messages that arrive while a rebuild waits for the canvas ask for no
/// second one: the one on its way holds them.
#[test]
fn one_rebuild_at_a_time() {
    let (mut app, _, _) = app_with_filter();
    listen(&mut app);
    app.a11y_after(Touch::Changed);
    assert!(app.a11y.locating);
    // Asking again while it waits changes nothing.
    app.a11y_after(Touch::Changed);
    app.a11y_after(Touch::Own);
    assert!(app.a11y.locating);
    app.a11y_event(Event::Located(Places::default()));
    assert!(!app.a11y.locating);
}

/// Runs the engine and hands the app the lights and meters that moved, as
/// the watcher thread does.
fn move_live_values(app: &mut App) {
    let tiles: Vec<u32> = app.ui_snapshot().tiles.iter().map(|t| t.id).collect();
    let _ = app.send(Message::Canvas(CanvasEvent::VisibleDisplays(tiles)));
    let watch = app.watched_lights().expect("the patch shows lights");
    app.process(4_800);
    let _ = app.send(Message::Lights(watch.read()));
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
    let (_, changes) = app.a11y_changes(&Places::default());
    let sent = changes.map(|update| sent_ids(&update)).unwrap_or_default();
    assert_eq!(live_among(&app, &sent), Vec::new(), "live nodes wait");
    assert!(app.a11y.live_waits());

    // The timer fires: the live values are sent, and the timer stops.
    app.a11y_event(Event::LiveTick);
    assert!(app.a11y.locating, "the timer asks for a rebuild");
    app.a11y.locating = false;
    let (_, changes) = app.a11y_changes(&Places::default());
    let sent = sent_ids(&changes.expect("the lights moved"));
    assert!(!live_among(&app, &sent).is_empty(), "the moved values are sent");
    assert!(!app.a11y.live_waits(), "nothing waits, so the timer stops");

    // The values have stopped, and the last ones were sent.
    let (_, none) = app.a11y_changes(&Places::default());
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
    app.a11y_event(Event::Activated);
    let _ = app.a11y_changes(&Places::of_canvas(Some(view)));
    app.a11y.locating = false;

    app.a11y_event(Event::ScaleFactor(1.0));
    assert!(!app.a11y.locating, "the same scale factor asks for nothing");
    app.a11y_event(Event::ScaleFactor(2.0));
    assert!(app.a11y.locating, "a new one asks for a rebuild");
    let (_, changes) = app.a11y_changes(&Places::of_canvas(Some(view)));
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
    app.a11y_event(Event::Deactivated);
    assert!(!app.a11y.is_active());
    app.a11y_after(Touch::Changed);
    app.a11y_after(Touch::Live);
    assert!(!app.a11y.locating && !app.a11y.live_waits());

    app.a11y_event(Event::Activated);
    let (whole, first) = app.a11y_changes(&Places::default());
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
    app.a11y_event(Event::Activated);
    let (whole, _) = app.a11y_changes(&Places::default());
    let root = node_of(&mut app, &window_id());
    assert_eq!(whole.focus, root);

    let _ = app.send(Message::Canvas(CanvasEvent::FocusChanged(Some((module, knob)))));
    let cutoff = widget_node(&mut app, module, knob);
    let (whole, changes) = app.a11y_changes(&Places::default());
    assert_eq!(whole.focus, cutoff);
    assert_eq!(changes.expect("the focus moved, so an update goes").focus, cutoff);

    let (_, again) = app.a11y_changes(&Places::default());
    assert!(again.is_none(), "the same focus sends nothing");

    let _ = app.send(Message::Canvas(CanvasEvent::FocusChanged(None)));
    let (whole, changes) = app.a11y_changes(&Places::default());
    assert_eq!(whole.focus, root);
    assert!(changes.is_some());
}

/// A control and its pieces offer Focus, and a widget that only shows a
/// value does not.
#[test]
fn controls_offer_focus_and_displays_do_not() {
    let mut app = app_with_every_widget();
    let update = app.a11y_tree(&Places::default());
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
    let _ = app.a11y_tree(&Places::default());
    let t = std::time::Instant::now();
    let (module, knob) = first_of(&app, "knob");
    let selection = app.session.selection.clone();
    let (key, _) = app.a11y.focus_request();

    let target = widget_node(&mut app, module, knob);
    act(&mut app, &request(Action::Focus, target, None), t);
    assert_eq!(app.a11y.focus_request(), (key + 1, Some((module, knob))));
    assert_eq!(app.session.selection, selection, "the canvas selects, when it draws");
    assert!(!app.unsaved(), "focus is not an edit");

    let (pad_module, pad) = first_of(&app, "xy_pad");
    let y = piece_node(&mut app, pad_module, pad, tree::Piece::Second);
    act(&mut app, &request(Action::Focus, y, None), t);
    assert_eq!(app.a11y.focus_request(), (key + 2, Some((pad_module, pad))));

    let (light_module, light) = first_of(&app, "light");
    let display = widget_node(&mut app, light_module, light);
    act(&mut app, &request(Action::Focus, display, None), t);
    assert_eq!(app.a11y.focus_request().0, key + 2, "a display takes no focus");
    let tile = node_of(&mut app, &tile_id(module));
    act(&mut app, &request(Action::Focus, tile, None), t);
    assert_eq!(app.a11y.focus_request().0, key + 2, "a tile takes no focus");
}

/// Focus moving to another control ends a screen reader's undo group, and
/// a replaced patch forgets the focus, since its module IDs mean nothing
/// in the new one.
#[test]
fn focus_ends_the_group_and_a_new_patch_forgets_it() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(&Places::default());
    let cutoff = widget_node(&mut app, module, knob);
    let steps = app.session.edits.history.len();
    act(
        &mut app,
        &request(Action::Increment, cutoff, None),
        std::time::Instant::now(),
    );
    let _ = app.send(Message::Canvas(CanvasEvent::FocusChanged(Some((module, knob)))));
    assert_eq!(app.session.edits.reader_group(), crate::edits::ReaderGroup::Closed);
    assert_eq!(app.session.edits.history.len(), steps + 1);

    act(
        &mut app,
        &request(Action::Focus, cutoff, None),
        std::time::Instant::now(),
    );
    assert!(app.a11y.focus_request().1.is_some());
    app.a11y.patch_replaced();
    assert_eq!(app.a11y.focus_request().1, None);
    let update = app.a11y_tree(&Places::default());
    assert_eq!(update.focus, node_of(&mut app, &window_id()));
}

/// While a panel owns the input, the tiles are hidden from a screen
/// reader, focus is not reported inside them, and what is inside a tile
/// keeps the value last sent. When the panel closes, the tiles show again
/// with their current values (ui-layer.md, section 7, rule 7).
#[test]
fn tiles_below_a_panel_are_hidden_and_keep_their_values() {
    let (mut app, module, knob) = app_with_filter();
    app.a11y_event(Event::Activated);
    let (whole, _) = app.a11y_changes(&Places::default());
    let tile = node_of(&mut app, &tile_id(module));
    let cutoff = widget_node(&mut app, module, knob);
    assert!(!node(&whole, tile).is_hidden());
    let _ = app.send(Message::Canvas(CanvasEvent::FocusChanged(Some((module, knob)))));
    let _ = app.a11y_changes(&Places::default());

    app.overlays.open(crate::overlays::Overlay::RecentList(Vec::new()));
    // The patch changes behind the panel, as MIDI or a script can do.
    app.set_param(module, 0, 0.5);
    let _ = app.send(Message::Canvas(CanvasEvent::SetControl {
        module,
        widget: knob,
        position: 0.1,
        part: ControlPart::Main,
    }));
    let _ = app.send(Message::SliderReleased);
    let (whole, changes) = app.a11y_changes(&Places::default());
    assert!(node(&whole, tile).is_hidden());
    assert_eq!(
        whole.focus,
        node_of(&mut app, &crate::menus::recent_id()),
        "no focus inside a hidden tile: it is on the open list"
    );
    let sent = sent_ids(&changes.expect("the tile is sent, hidden"));
    assert!(sent.contains(&tile));
    assert!(!sent.contains(&cutoff), "the knob keeps the value last sent");

    app.overlays = crate::overlays::Overlays::default();
    let (whole, changes) = app.a11y_changes(&Places::default());
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
    app.a11y_event(Event::Activated);
    let _ = app.a11y_changes(&Places::of_canvas(Some(away)));
    move_live_values(&mut app);
    assert!(app.a11y.live_waits());

    app.a11y.flush_live = true;
    let (_, changes) = app.a11y_changes(&Places::of_canvas(Some(away)));
    let sent = changes.map(|update| sent_ids(&update)).unwrap_or_default();
    assert_eq!(live_among(&app, &sent), Vec::new(), "off screen, nothing is sent");
    assert!(!app.a11y.live_waits(), "and the timer stops");

    // On screen now: the bounds changed, so the live nodes wait, then go.
    let _ = app.a11y_changes(&Places::of_canvas(Some(on_screen)));
    assert!(app.a11y.live_waits());
    app.a11y.flush_live = true;
    let (_, changes) = app.a11y_changes(&Places::of_canvas(Some(on_screen)));
    let sent = sent_ids(&changes.expect("on screen, the values are sent"));
    assert!(!live_among(&app, &sent).is_empty());
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
    let update = app.a11y_tree(&Places::default());
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
    let _ = app.send(Message::Canvas(CanvasEvent::Select(vec![other])));
    let mut ui = crate::ui_test::Ui::with_app(app);
    // The first frame only notes the request's key.
    let _ = ui.redraw();
    let _ = ui.app.a11y_tree(&Places::default());
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
    assert_eq!(ui.app.a11y_tree(&Places::default()).focus, cutoff);

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
    assert_eq!(
        ui.app.a11y_tree(&Places::default()).focus,
        cutoff,
        "and the focus stayed"
    );
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
    app.a11y_event(Event::Located(Places::default()));
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
            let _ = app.send(poll);
            assert!(!app.a11y.locating, "a poll that changed nothing asks for no build");
        }
    }
    assert_eq!(app.a11y_poll_key(), before);

    // The same poll, after something it reports changed underneath it.
    app.a11y_after(Touch::Poll(Some(before)));
    assert!(!app.a11y.locating);
    app.alerts.say("The audio device is gone");
    assert_ne!(app.a11y_poll_key(), before);
    app.a11y_after(Touch::Poll(Some(before)));
    assert!(app.a11y.locating, "a changed status asks for one build");
    app.a11y_after(Touch::Poll(Some(before)));
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
    let update = app.a11y_tree(&Places::default());
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
    // Low below high, the usual way round. The last argument says which
    // handle moves: false for low, true for high.
    assert_eq!(stop_at(0.9, 0.2, 0.8, false), 0.8, "low stops at high");
    assert_eq!(stop_at(0.5, 0.2, 0.8, false), 0.5);
    assert_eq!(stop_at(0.1, 0.8, 0.2, true), 0.2, "high stops at low");
    // Low above high.
    assert_eq!(stop_at(0.9, 0.8, 0.2, false), 0.9, "low moves away from high");
    assert_eq!(stop_at(0.1, 0.8, 0.2, false), 0.2);
    assert_eq!(stop_at(0.1, 0.2, 0.8, true), 0.1, "high moves away from low");
    // Together: low is the one below and high the one above, so low can
    // go down and high can go up, and neither crosses.
    assert_eq!(stop_at(0.3, 0.5, 0.5, false), 0.3);
    assert_eq!(stop_at(0.8, 0.5, 0.5, false), 0.5);
    assert_eq!(stop_at(0.8, 0.5, 0.5, true), 0.8);
    assert_eq!(stop_at(0.3, 0.5, 0.5, true), 0.5);
}

/// A Click that changes nothing, such as one on the option already
/// chosen, still ends an open undo group, as a pointer press does. A
/// request for a node that is gone ends nothing.
#[test]
fn a_click_that_changes_nothing_ends_the_group() {
    use crate::edits::ReaderGroup;
    let mut app = app_with_every_widget();
    let _ = app.a11y_tree(&Places::default());
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
    act(&mut app, &request(step, knob_node, None), now);
    assert_ne!(app.session.edits.reader_group(), ReaderGroup::Closed);
    // A node the window does not show leaves the group open.
    let gone = request(Action::Click, NodeId(u64::MAX), None);
    assert!(!act(&mut app, &gone, now));
    assert_ne!(app.session.edits.reader_group(), ReaderGroup::Closed);

    let chosen = position(&app, radio_module, radio).round() as usize;
    let option = piece_node(&mut app, radio_module, radio, tree::Piece::Option(chosen));
    assert!(!act(&mut app, &request(Action::Click, option, None), now));
    assert_eq!(app.session.edits.reader_group(), ReaderGroup::Closed);
    assert_eq!(
        app.session.edits.history.len(),
        steps + 1,
        "the knob's step is recorded"
    );

    // The next edit on the knob is a step of its own, though it is in time.
    act(&mut app, &request(step, knob_node, None), now);
    app.reader_event(crate::edits::ReaderEvent::Close);
    assert_eq!(app.session.edits.history.len(), steps + 2);
}

/// While an arrow key is held, its group owns the edit in progress, so a
/// screen reader's action does nothing, as Undo does nothing then.
#[test]
fn an_action_waits_while_an_arrow_key_is_held() {
    use crate::edits::{ArrowEvent, ArrowGroup, ReaderGroup};
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(&Places::default());
    let cutoff = widget_node(&mut app, module, knob);
    let now = std::time::Instant::now();
    app.arrow_event(ArrowEvent::Step(now));
    assert_eq!(app.session.edits.arrow_group(), ArrowGroup::Held);
    let before = position(&app, module, knob);

    assert!(!act(&mut app, &request(Action::Increment, cutoff, None), now));
    assert_eq!(position(&app, module, knob), before);
    assert_eq!(app.session.edits.reader_group(), ReaderGroup::Closed);
    assert_eq!(
        app.session.edits.arrow_group(),
        ArrowGroup::Held,
        "the key still owns the edit"
    );

    // The key comes up: the action works again.
    app.arrow_event(ArrowEvent::Released(now));
    assert!(act(&mut app, &request(Action::Increment, cutoff, None), now));
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
    ui.focus(&crate::patch_info::tags_part());
    assert!(ui.text_field_focused(), "the tag field took focus");
    let _ = ui.deliver(Message::TextFocusCheck);
    assert!(ui.app.focus.is_some(), "the app heard of it");
    ui.app.a11y_event(Event::Activated);
    // Its rebuild is not asked for here: this test counts the ones that
    // one message makes.
    drop(ui.app.take_window_questions());
    ui.app.a11y.locating = false;

    let log = ui.deliver(Message::Canvas(CanvasEvent::Select(Vec::new())));
    let rebuilds = log
        .iter()
        .filter(|message| matches!(message, Message::A11y(Event::Located(_))))
        .count();
    let asked = log
        .iter()
        .filter(|message| matches!(message, Message::FocusMoved(_)))
        .count();
    assert!(rebuilds <= 2, "{rebuilds} rebuilds for one message");
    assert!(asked <= 1, "focus was asked {asked} times");
}

/// A screen reader focuses a control on one tile and then clicks another
/// tile. The focus does not stay on the first tile: the tree reports it on
/// the window again, and an arrow key then moves the selected tile, not a
/// control the tree still claimed was focused.
#[test]
fn a_click_on_another_tile_drops_focus_from_the_first() {
    let (mut app, module, knob) = app_with_filter();
    let other = app.session.controller.add_module("loom.filter.svf").unwrap().0;
    app.commit();
    let mut ui = crate::ui_test::Ui::with_app(app);
    let _ = ui.redraw();
    let _ = ui.app.a11y_tree(&Places::default());
    let cutoff = widget_node(&mut ui.app, module, knob);
    let other_tile = node_of(&mut ui.app, &tile_id(other));
    let window = node_of(&mut ui.app, &window_id());

    ui.send(Message::A11y(Event::Action(request(Action::Focus, cutoff, None))));
    let _ = ui.redraw();
    assert_eq!(ui.app.a11y_tree(&Places::default()).focus, cutoff);

    ui.send(Message::A11y(Event::Action(request(Action::Click, other_tile, None))));
    let drawn = ui.redraw();
    assert!(
        drawn
            .iter()
            .any(|message| matches!(message, Message::Canvas(CanvasEvent::FocusChanged(None))))
    );
    assert_eq!(ui.app.ui_snapshot().selection, vec![other]);
    assert_eq!(ui.app.a11y_tree(&Places::default()).focus, window);

    let before = position(&ui.app, module, knob);
    let _ = ui.key_down(iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowUp), false);
    let _ = ui.key_up(iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowUp));
    assert_eq!(
        position(&ui.app, module, knob),
        before,
        "the first tile's knob is left alone"
    );
}

/// The toolbar is a named group of buttons. Each button has its label, its
/// hover text as the description, and Click only while it works. A mode
/// button says whether its mode is on. No button offers Focus: outside
/// the canvas only text fields take it until phase 3 (ui-layer.md, section
/// 7, rule 22).
#[test]
fn the_toolbar_is_a_group_of_buttons() {
    let (mut app, _, _) = app_with_filter();
    let update = app.a11y_tree(&Places::default());
    let root = node_of(&mut app, &window_id());
    let toolbar = node_of(&mut app, &tree::toolbar_id());
    assert_eq!(node(&update, root).children().first(), Some(&toolbar));
    assert_eq!(node(&update, toolbar).role(), Role::Group);
    assert_eq!(node(&update, toolbar).label(), Some("Toolbar"));

    let model = app.toolbar_model();
    assert_eq!(node(&update, toolbar).children().len(), model.buttons.len());
    for button in &model.buttons {
        let found = node(&update, node_of(&mut app, &tree::toolbar_id().with(button.name)));
        assert_eq!(found.role(), Role::Button, "{}", button.name);
        assert_eq!(found.label(), Some(button.label));
        assert_eq!(found.description(), button.hover.as_deref());
        assert_eq!(found.supports_action(Action::Click), button.enabled, "{}", button.name);
        assert_eq!(found.is_disabled(), !button.enabled, "{}", button.name);
        assert!(!found.supports_action(Action::Focus), "{}", button.name);
        assert_eq!(found.toggled().is_some(), button.on.is_some(), "{}", button.name);
    }
}

/// A Click on a toolbar button sends the message its press sends. A
/// disabled button, another action, and a node that is no button send
/// nothing.
#[test]
fn a_click_on_a_toolbar_button_presses_it() {
    let (mut app, module, knob) = app_with_filter();
    let _ = app.a11y_tree(&Places::default());
    let undo = node_of(&mut app, &tree::toolbar_id().with("undo"));
    assert!(
        app.a11y_frame_message(&request(Action::Click, undo, None)).is_none(),
        "nothing to undo yet"
    );

    let before = app.ui_snapshot().tiles.iter().find(|t| t.id == module).unwrap().widgets[knob].position;
    let cutoff = widget_node(&mut app, module, knob);
    let now = std::time::Instant::now();
    act(&mut app, &request(Action::Increment, cutoff, None), now);
    let moved = app.ui_snapshot().tiles.iter().find(|t| t.id == module).unwrap().widgets[knob].position;
    assert!(moved > before);

    assert!(matches!(
        app.a11y_frame_message(&request(Action::Click, undo, None)),
        Some(Message::Undo)
    ));
    assert!(app.a11y_frame_message(&request(Action::Focus, undo, None)).is_none());
    assert!(app.a11y_frame_message(&request(Action::Click, cutoff, None)).is_none());
    act(&mut app, &request(Action::Click, undo, None), now);
    let back = app.ui_snapshot().tiles.iter().find(|t| t.id == module).unwrap().widgets[knob].position;
    assert!((back - before).abs() < 1e-6, "Undo took the screen reader's edit back");

    let learn = node_of(&mut app, &tree::toolbar_id().with("midi_learn"));
    act(&mut app, &request(Action::Click, learn, None), now);
    assert!(app.ui_snapshot().learning);
    let update = app.a11y_tree(&Places::default());
    assert_eq!(node(&update, learn).toggled(), Some(accesskit::Toggled::True));
}

/// The right panel's tabs are a tab list with the open tab selected, and a
/// Click on a tab opens it.
#[test]
fn the_right_panel_has_tabs() {
    let (mut app, _, _) = app_with_filter();
    let update = app.a11y_tree(&Places::default());
    let list = node_of(&mut app, &tree::tabs_id());
    let inspector = node_of(&mut app, &tree::tab_id(crate::matrix::Tab::Inspector));
    let matrix = node_of(&mut app, &tree::tab_id(crate::matrix::Tab::Matrix));
    assert_eq!(node(&update, list).role(), Role::TabList);
    assert_eq!(node(&update, list).children(), [inspector, matrix]);
    assert_eq!(node(&update, inspector).role(), Role::Tab);
    assert_eq!(node(&update, inspector).label(), Some("Inspector"));
    assert_eq!(node(&update, inspector).is_selected(), Some(true));
    assert_eq!(node(&update, matrix).is_selected(), Some(false));

    act(
        &mut app,
        &request(Action::Click, matrix, None),
        std::time::Instant::now(),
    );
    let update = app.a11y_tree(&Places::default());
    assert_eq!(node(&update, matrix).is_selected(), Some(true));
    assert_eq!(node(&update, inspector).is_selected(), Some(false));
}

/// The bar at the bottom is a named group. Its plain items are labels with
/// the text shown, hover text is the description, and ordinary status text
/// is a label. A warning in the status line is a status, so it is
/// announced (ui-layer.md, section 7, the role table).
#[test]
fn the_bar_reports_its_items_and_the_status() {
    let (mut app, _, _) = app_with_filter();
    app.alerts.say("Saved");
    let update = app.a11y_tree(&Places::default());
    let bar = node_of(&mut app, &tree::stats_id());
    assert_eq!(node(&update, bar).role(), Role::Group);
    assert_eq!(node(&update, bar).label(), Some("Status bar"));
    let model = app.bars_model();
    for item in &model.items {
        let found = node(&update, node_of(&mut app, &tree::stats_id().with(item.name)));
        assert_eq!(found.value(), Some(item.text.as_str()), "{}", item.name);
        assert_eq!(found.description(), item.hover.as_deref(), "{}", item.name);
        let alert = item.kind == crate::bars::Kind::Alert;
        assert_eq!(
            found.role(),
            if alert { Role::Status } else { Role::Label },
            "{}",
            item.name
        );
    }
    let status = node_of(&mut app, &tree::fixed_id(crate::bars::STATUS_ID));
    assert_eq!(node(&update, status).role(), Role::Label);
    assert_eq!(node(&update, status).value(), Some(model.status.shown.as_str()));

    app.alerts.warn("The audio device is gone");
    let update = app.a11y_tree(&Places::default());
    assert_eq!(node(&update, status).role(), Role::Status);
    assert!(node(&update, status).value().is_some_and(|text| text.contains("gone")));

    assert!(tree::is_live_bar_item(&tree::stats_id().with("load")));
    assert!(tree::is_live_bar_item(&tree::stats_id().with("dropouts")));
    assert!(!tree::is_live_bar_item(&tree::stats_id().with("midi")));
}

/// The developer switches are switches, and a Click turns one.
#[test]
fn the_developer_switches_are_switches() {
    let (mut app, _, _) = app_with_filter();
    app.dev_tools = true;
    let was = app.measuring;
    let update = app.a11y_tree(&Places::default());
    let measure = node_of(&mut app, &tree::fixed_id(crate::bars::MEASURE_ID));
    let camera = node_of(&mut app, &tree::fixed_id(crate::bars::MOVE_CAMERA_ID));
    assert_eq!(node(&update, measure).role(), Role::Switch);
    assert_eq!(node(&update, measure).label(), Some("Measure"));
    assert_eq!(node(&update, camera).label(), Some("Move camera"));
    act(
        &mut app,
        &request(Action::Click, measure, None),
        std::time::Instant::now(),
    );
    assert_eq!(app.measuring, !was);
    assert!(matches!(
        app.a11y_frame_message(&request(Action::Click, camera, None)),
        Some(Message::Animate(on)) if on != app.animating
    ));
    // Without the tools the switches are not drawn, and do nothing.
    app.dev_tools = false;
    assert!(app.a11y_frame_message(&request(Action::Click, measure, None)).is_none());
}

/// What a UI ID names outside the canvas: a button, a tab, a ×, a switch,
/// or nothing that acts.
#[test]
fn frame_parts_are_told_by_their_ids() {
    use tree::{FramePart, frame_part};
    let id = |text: &str| tree::fixed_id(text);
    assert_eq!(frame_part(&id("toolbar/undo")), Some(FramePart::Button("undo")));
    assert_eq!(
        frame_part(&id("panel/matrix")),
        Some(FramePart::Tab(crate::matrix::Tab::Matrix))
    );
    assert_eq!(
        frame_part(&id("panel/inspector")),
        Some(FramePart::Tab(crate::matrix::Tab::Inspector))
    );
    assert_eq!(
        frame_part(&id("status/dismiss")),
        Some(FramePart::Dismiss { kept: false })
    );
    assert_eq!(
        frame_part(&id("status/kept/dismiss")),
        Some(FramePart::Dismiss { kept: true })
    );
    assert_eq!(frame_part(&id("stats/measure")), Some(FramePart::Measure));
    assert_eq!(frame_part(&id("stats/move_camera")), Some(FramePart::MoveCamera));
    for nothing in [
        "stats/load",
        "status",
        "status/kept",
        "toolbar",
        "panel",
        "canvas/4",
        "panel/other",
    ] {
        assert_eq!(frame_part(&id(nothing)), None, "{nothing}");
    }
}

/// The window says where it drew each named part, and the tree gives that
/// place to the part's node, in physical pixels. Every button, tab, and bar
/// item the tree reports was found in the window, inside it, and not on top
/// of another.
#[test]
fn frame_nodes_have_the_bounds_the_window_drew() {
    let (app, _, _) = app_with_filter();
    let mut ui = crate::ui_test::Ui::with_app(app);
    let _ = ui.redraw();
    let places = ui.places();
    assert!(places.canvas.is_some());
    ui.app.a11y.scale = 2.0;
    let update = ui.app.a11y_tree(&places);

    let mut ids: Vec<UiId> = ui
        .app
        .toolbar_model()
        .buttons
        .iter()
        .map(|button| tree::toolbar_id().with(button.name))
        .collect();
    ids.extend(
        ui.app
            .bars_model()
            .items
            .iter()
            .map(|item| tree::stats_id().with(item.name)),
    );
    ids.push(tree::fixed_id(crate::bars::STATUS_ID));
    ids.push(tree::tab_id(crate::matrix::Tab::Inspector));
    ids.push(tree::tab_id(crate::matrix::Tab::Matrix));
    // The groups hold their parts, so only the parts are compared for
    // overlap below.
    for group in [tree::toolbar_id(), tree::tabs_id(), tree::stats_id()] {
        let place = places
            .of(&group)
            .unwrap_or_else(|| panic!("the window drew no `{group}`"));
        assert!(place.rect.width > 0.0 && place.rect.height > 0.0, "{group}");
        assert!(
            node(&update, node_of(&mut ui.app, &group)).bounds().is_some(),
            "{group}"
        );
    }
    let mut seen: Vec<accesskit::Rect> = Vec::new();
    for id in &ids {
        let place = places.of(id).unwrap_or_else(|| panic!("the window drew no `{id}`"));
        assert!(place.is_shown());
        let bounds = node(&update, node_of(&mut ui.app, id))
            .bounds()
            .unwrap_or_else(|| panic!("`{id}` has no bounds"));
        assert!((bounds.x0 - f64::from(place.rect.x) * 2.0).abs() < 1e-3, "{id}");
        assert!((bounds.y0 - f64::from(place.rect.y) * 2.0).abs() < 1e-3, "{id}");
        assert!(bounds.x1 > bounds.x0 && bounds.y1 > bounds.y0, "{id} has a size");
        assert!(bounds.x0 >= 0.0 && bounds.y0 >= 0.0, "{id} is inside the window");
        for other in &seen {
            let apart =
                bounds.x1 <= other.x0 || other.x1 <= bounds.x0 || bounds.y1 <= other.y0 || other.y1 <= bounds.y0;
            assert!(apart, "{id} lies on another part");
        }
        seen.push(bounds);
    }
}

/// While a panel owns the input, the toolbar, the tabs, and the bar are
/// hidden with the tiles, and a Click on a button behind the panel does
/// nothing.
#[test]
fn frame_nodes_are_hidden_below_a_panel() {
    let (mut app, _, _) = app_with_filter();
    let _ = app.a11y_tree(&Places::default());
    let add = node_of(&mut app, &tree::toolbar_id().with("add_module"));
    app.overlays.open(crate::overlays::Overlay::RecentList(Vec::new()));
    assert!(!app.no_overlay());
    let update = app.a11y_tree(&Places::default());
    for id in [
        tree::toolbar_id(),
        tree::tabs_id(),
        tree::stats_id(),
        tree::toolbar_id().with("undo"),
    ] {
        assert!(node(&update, node_of(&mut app, &id)).is_hidden(), "{id}");
    }
    assert!(!act(
        &mut app,
        &request(Action::Click, add, None),
        std::time::Instant::now()
    ));
}

/// The audio watcher's reading is a live value: it asks for no build, and
/// waits for the live timer. A change of the toolbar changes the key a
/// poll is compared by.
#[test]
fn bar_readings_are_live_and_the_frame_is_in_the_poll_key() {
    let (mut app, _, _) = app_with_filter();
    app.a11y = Access::measured();
    app.a11y.locating = false;
    let _ = app.send(Message::Audio(crate::audio_watch::Event::Stats {
        dropouts: 0,
        at: std::time::Instant::now(),
        peak: 0.1,
        max: 0.2,
        silenced: false,
        clipped: false,
    }));
    assert!(!app.a11y.locating, "a reading asks for no build");
    assert!(app.a11y.live_waits(), "it waits for the live timer");

    let key = app.a11y_poll_key();
    let _ = app.send(Message::Learn(crate::learn::Message::Toggle));
    assert_ne!(app.a11y_poll_key(), key, "the toolbar and the bar changed");
    let key = app.a11y_poll_key();
    let _ = app.send(Message::Matrix(crate::matrix::Event::Tab(crate::matrix::Tab::Matrix)));
    assert_ne!(app.a11y_poll_key(), key, "the open tab changed");
}

/// Each state of the window that can have a panel open, by who has the
/// input then.
fn with_each_panel() -> Vec<(crate::input_owner::InputOwner, crate::ui_test::Ui)> {
    use crate::input_owner::InputOwner;
    use crate::overlays::{ContextMenu, Overlay};
    let open = |change: &dyn Fn(&mut App, u32)| {
        let (mut app, module, _) = app_with_filter();
        change(&mut app, module);
        let mut ui = crate::ui_test::Ui::with_app(app);
        let _ = ui.redraw();
        ui
    };
    let at = Point::new(300.0, 300.0);
    let mut all = vec![
        open(&|_, _| {}),
        open(&|app, module| app.overlays.open(Overlay::Menu(ContextMenu::Tile { module, at }))),
        open(&|app, module| {
            let output = loom_canvas::PortRef {
                module,
                output: true,
                index: 0,
            };
            let area = Rect::new(Point::new(0.0, 0.0), Size::new(800.0, 600.0));
            let menu = crate::probe_menu::ProbeMenu::open(output, at, at, area);
            app.overlays.open(Overlay::Menu(ContextMenu::Probe(menu)));
        }),
        open(&|app, _| app.overlays.open(Overlay::RecentList(Vec::new()))),
        open(&|app, _| {
            let _ = app.send(Message::Preferences(crate::settings::Message::Open));
        }),
        open(&|app, _| {
            let browser = crate::patches::PatchBrowser::new(1, std::path::PathBuf::from("unread"));
            app.overlays.open(Overlay::Patches(browser));
        }),
        open(&|app, _| app.overlays.help = Some(crate::help::Help::shortcuts())),
        open(&|app, _| {
            let target = crate::licence_chooser::Target::Patch;
            app.overlays.chooser = Some(crate::licence_chooser::Chooser::open(target, None));
        }),
        open(&|app, _| app.overlays.question = Some(crate::question::Question::error("No".to_string()))),
    ];
    // The module browser opens from the canvas, on the redraw after the
    // request.
    let (app, _, _) = app_with_filter();
    let mut ui = crate::ui_test::Ui::with_app(app);
    let _ = ui.redraw();
    ui.send(Message::OpenBrowser);
    let _ = ui.redraw();
    all.push(ui);
    let all: Vec<_> = all.into_iter().map(|ui| (ui.app.input_owner(), ui)).collect();
    for owner in [
        InputOwner::Canvas,
        InputOwner::TileMenu,
        InputOwner::ProbeMenu,
        InputOwner::RecentList,
        InputOwner::Preferences,
        InputOwner::Patches,
        InputOwner::Help,
        InputOwner::Chooser,
        InputOwner::Question,
        InputOwner::Browser,
    ] {
        assert!(all.iter().any(|(found, _)| *found == owner), "{owner:?} did not open");
    }
    all
}

/// The tree follows the pointer. With nothing open, a pointer press on
/// Panic presses it and a drag on a tile's knob turns it, and a screen
/// reader's Click and Increment do the same. With any panel or menu open,
/// the press reaches neither, so the nodes are hidden and the actions do
/// nothing: the panic shortcut is what works then (ui-layer.md, section 6,
/// rule 6, and section 7, rule 24). The pointer and the tree are checked
/// against each other for every panel, so they cannot drift.
#[test]
fn the_tree_follows_the_pointer_under_every_panel() {
    use crate::input_owner::InputOwner;
    for (owner, mut ui) in with_each_panel() {
        let open = owner == InputOwner::Canvas;
        assert_eq!(ui.app.pointer_reaches_window(), open, "{owner:?}");
        let snapshot = ui.app.ui_snapshot();
        let (module, knob) = snapshot
            .tiles
            .iter()
            .find_map(|tile| {
                let knob = tile
                    .widgets
                    .iter()
                    .position(|w| w.label.eq_ignore_ascii_case("cutoff"))?;
                Some((tile.id, knob))
            })
            .expect("the patch has a filter");
        let cutoff = |ui: &crate::ui_test::Ui| {
            let tile = ui.app.ui_snapshot().tiles.into_iter().find(|t| t.id == module).unwrap();
            tile.widgets[knob].position
        };

        // What a screen reader is told, and what its actions would do.
        let places = ui.places();
        let panic_id = tree::toolbar_id().with("panic");
        assert!(places.of(&panic_id).is_some(), "Panic is drawn");
        let update = ui.app.a11y_tree(&places);
        let panic_node = node_of(&mut ui.app, &panic_id);
        let click = ui.app.a11y_frame_message(&request(Action::Click, panic_node, None));
        assert_eq!(
            matches!(click, Some(Message::Panic)),
            open,
            "{owner:?}: a Click on Panic"
        );
        assert_eq!(node(&update, panic_node).is_hidden(), !open, "{owner:?}: Panic's node");
        for group in [tree::toolbar_id(), tree::tabs_id(), tree::stats_id(), tile_id(module)] {
            let group_node = node_of(&mut ui.app, &group);
            assert_eq!(node(&update, group_node).is_hidden(), !open, "{owner:?}: {group}");
        }
        let knob_node = widget_node(&mut ui.app, module, knob);
        let before = cutoff(&ui);
        let sent = ui_act(
            &mut ui,
            &request(Action::Increment, knob_node, None),
            std::time::Instant::now(),
        );
        assert_eq!(sent, open, "{owner:?}: Increment on a tile's knob");
        assert_eq!(
            (cutoff(&ui) - before).abs() > 1e-6,
            open,
            "{owner:?}: the knob after Increment"
        );

        // The inspector follows the same rule (rule 24): its Keyboard
        // shortcuts button, shown while nothing is selected.
        let shortcuts = crate::inspector::ids::part(&crate::inspector::ids::patch(), "shortcuts");
        let shortcuts_node = node_of(&mut ui.app, &shortcuts);
        assert_eq!(
            node(&update, shortcuts_node).is_hidden(),
            !open,
            "{owner:?}: the inspector"
        );
        let clicked = ui_act(
            &mut ui,
            &request(Action::Click, shortcuts_node, None),
            std::time::Instant::now(),
        );
        assert_eq!(clicked, open, "{owner:?}: a Click in the inspector");
        if open {
            // The overlay it opened is closed again, so the knob below is
            // pressed with nothing open.
            ui.app.overlays.help = None;
        }

        // What the pointer reaches, in the same state. The knob first: a
        // press outside a menu closes it, which would change the state.
        let before = cutoff(&ui);
        let at = ui.control(module, "cutoff");
        let _ = ui.drag(at, Point::new(at.x, at.y - 40.0), 5);
        assert_eq!(
            (cutoff(&ui) - before).abs() > 1e-6,
            open,
            "{owner:?}: a drag on the knob"
        );
    }
    for (owner, mut ui) in with_each_panel() {
        let open = owner == InputOwner::Canvas;
        let panic = ui
            .places()
            .of(&tree::toolbar_id().with("panic"))
            .expect("Panic is drawn");
        let mut messages = ui.move_to(panic.rect.center());
        messages.extend(ui.press());
        messages.extend(ui.release());
        let reached = messages.iter().any(|message| matches!(message, Message::Panic));
        assert_eq!(reached, open, "{owner:?}: a press on Panic");
    }
    for (owner, mut ui) in with_each_panel() {
        let open = owner == InputOwner::Canvas;
        let shortcuts = crate::inspector::ids::part(&crate::inspector::ids::patch(), "shortcuts");
        let place = ui.places().of(&shortcuts).expect("the inspector's button is drawn");
        let mut messages = ui.move_to(place.rect.center());
        messages.extend(ui.press());
        messages.extend(ui.release());
        let reached = messages.iter().any(|message| matches!(message, Message::ShowShortcuts));
        assert_eq!(reached, open, "{owner:?}: a press in the inspector");
    }
}

/// The inspector's parts, after the window has drawn the app with
/// `select` selected: the places the window gives, the tree, and the items.
fn inspector_of(ui: &mut crate::ui_test::Ui, select: &[u32]) -> (Places, TreeUpdate, Vec<super::items::Item>) {
    ui.send(Message::Canvas(CanvasEvent::Select(select.to_vec())));
    let _ = ui.redraw();
    let places = ui.places();
    let update = ui.app.a11y_tree(&places);
    let items = ui.app.a11y_frames().panel;
    (places, update, items)
}

/// Every part the inspector reports has a role, every part but text has a
/// name, and every part that acts is where the window drew it, inside the
/// panel. Checked for a module, a sub-patch tile, and the patch itself, so
/// the items and the view cannot drift apart.
#[test]
fn every_inspector_part_is_named_and_placed() {
    use super::items::Kind;
    let cases = [
        ("subtractive.ron", vec![4]),
        ("subtractive.ron", vec![]),
        ("nested-subtractive.ron", vec![2]),
    ];
    for (patch, select) in cases {
        let mut ui = crate::ui_test::Ui::open(patch);
        let (places, update, items) = inspector_of(&mut ui, &select);
        assert_eq!(items.len(), 1, "{patch} {select:?}: one panel");
        let panel = &items[0];
        let panel_place = places
            .of(&panel.id)
            .unwrap_or_else(|| panic!("{}: the panel is drawn", panel.id));
        let panel_node = node(&update, node_of(&mut ui.app, &panel.id));
        assert!(panel_node.clips_children(), "the panel cuts off what it scrolls away");
        let mut acting = 0;
        for item in panel.walk() {
            let found = node(&update, node_of(&mut ui.app, &item.id));
            // The role the table in section 7 gives each kind, and the
            // state each kind must say.
            let role = match item.kind {
                Kind::Group => Role::Group,
                Kind::Text => Role::Label,
                Kind::Button => Role::Button,
                Kind::Check => Role::CheckBox,
                Kind::Slider => Role::Slider,
                Kind::Options => Role::RadioGroup,
                Kind::Option => Role::RadioButton,
                Kind::Choice => Role::ComboBox,
                Kind::Field => Role::TextInput,
                Kind::List => Role::List,
                Kind::ListItem => Role::ListItem,
                Kind::Dialog => Role::Dialog,
                Kind::AlertDialog => Role::AlertDialog,
                Kind::Menu => Role::Menu,
                Kind::MenuItem => Role::MenuItem,
                Kind::Link => Role::Link,
                Kind::ListBox => Role::ListBox,
                Kind::ListBoxOption => Role::ListBoxOption,
            };
            assert_eq!(found.role(), role, "{}", item.id);
            match item.kind {
                Kind::Check | Kind::Option => assert!(found.toggled().is_some(), "{} says on or off", item.id),
                Kind::Choice => assert!(
                    found.value().is_some_and(|value| !value.is_empty()),
                    "{} says what is chosen",
                    item.id
                ),
                Kind::Slider => assert!(found.numeric_value().is_some(), "{} says its value", item.id),
                Kind::Field => assert!(found.value().is_some(), "{} says its text", item.id),
                _ => {}
            }
            if item.enabled && matches!(item.kind, Kind::Button | Kind::Check | Kind::Option) {
                assert!(found.supports_action(Action::Click), "{} can be pressed", item.id);
            }
            if item.kind == Kind::Text {
                assert_eq!(found.label(), None, "{}: text says its words as its value", item.id);
                assert!(found.value().is_some_and(|text| !text.is_empty()), "{}", item.id);
                continue;
            }
            assert!(
                item.label.as_deref().is_some_and(|name| !name.is_empty()),
                "{} has a name",
                item.id
            );
            let acts = item.click.is_some()
                || item.step.0.is_some()
                || item.step.1.is_some()
                || item.set_number.is_some()
                || item.set_text.is_some()
                || item.focus.is_some();
            if !acts {
                continue;
            }
            acting += 1;
            let place = places
                .of(&item.id)
                .unwrap_or_else(|| panic!("the window drew no `{}`", item.id));
            assert!(place.rect.width > 0.0 && place.rect.height > 0.0, "{}", item.id);
            assert!(found.bounds().is_some(), "{} has bounds", item.id);
            assert!(
                place.rect.x >= panel_place.rect.x - 0.5
                    && place.rect.x + place.rect.width <= panel_place.rect.x + panel_place.rect.width + 0.5,
                "{} lies in the panel",
                item.id
            );
        }
        assert!(acting > 0, "{patch} {select:?}: something acts");
    }
}

/// A slider in the inspector reports the parameter's own value and range,
/// with the value and unit the row shows as its description. Increment
/// moves one of its own arrow-key steps, and screen reader steps on it in
/// quick turn are one undo step.
#[test]
fn an_inspector_slider_steps_as_its_own_arrow_key_and_groups() {
    use super::items::Kind;
    let (app, module, _) = app_with_filter();
    let mut ui = crate::ui_test::Ui::with_app(app);
    let (_, update, items) = inspector_of(&mut ui, &[module]);
    let row = crate::inspector::ids::row(module, crate::inspector::ids::Kind::Param, "cutoff");
    let slider_id = crate::inspector::ids::part(&row, "value");
    let slider = super::items::find(&items, &slider_id).expect("the cutoff row has a slider");
    assert_eq!(slider.kind, Kind::Slider);
    let found = node(&update, node_of(&mut ui.app, &slider_id));
    assert_eq!(found.role(), Role::Slider);
    let spec = ui.app.control_target(module, 0, ControlPart::Main).unwrap().spec;
    assert_eq!(found.min_numeric_value(), Some(f64::from(spec.min)));
    assert_eq!(found.max_numeric_value(), Some(f64::from(spec.max)));
    assert!(
        found.description().is_some_and(|text| text.contains("Hz")),
        "{:?}",
        found.description()
    );
    assert!(found.supports_action(Action::Increment) && found.supports_action(Action::SetValue));
    assert!(
        !found.supports_action(Action::Focus),
        "only text fields take focus (rule 22)"
    );

    let index = ui
        .app
        .session
        .controller
        .graph()
        .node(ModuleId(module))
        .unwrap()
        .parameter_index("cutoff")
        .unwrap();
    let position = |app: &App| {
        let node = app.session.controller.graph().node(ModuleId(module)).unwrap();
        spec.normalize(node.params[index])
    };
    // From a place on the slider's grid, so each step is a whole step.
    ui.send(Message::Inspector(crate::inspector::Message::Param {
        module,
        index,
        position: 0.5,
    }));
    ui.send(Message::SliderReleased);
    let steps = ui.app.session.edits.history.len();
    let before = position(&ui.app);
    let node_id = node_of(&mut ui.app, &slider_id);
    let at = std::time::Instant::now();
    ui_act(&mut ui, &request(Action::Increment, node_id, None), at);
    ui_act(&mut ui, &request(Action::Increment, node_id, None), at);
    let step = crate::param::slider_step(&spec);
    assert!(
        (position(&ui.app) - (before + 2.0 * step)).abs() < 1e-5,
        "two of the slider's own steps"
    );
    ui.app.reader_event(crate::edits::ReaderEvent::Close);
    assert_eq!(ui.app.session.edits.history.len(), steps + 1, "one undo step");

    // SetValue in the parameter's own units.
    ui_act(
        &mut ui,
        &request(Action::SetValue, node_id, Some(ActionData::NumericValue(1000.0))),
        at,
    );
    let value = ui.app.session.controller.graph().node(ModuleId(module)).unwrap().params[index];
    assert!((value - 1000.0).abs() < 1.0, "{value}");
}

/// A row's value field takes text as typing and Enter would, offers Focus,
/// and is reported as focused while it has keyboard focus.
#[test]
fn an_inspector_field_takes_text_and_focus() {
    let (app, module, _) = app_with_filter();
    let mut ui = crate::ui_test::Ui::with_app(app);
    let (_, update, _) = inspector_of(&mut ui, &[module]);
    let row = crate::inspector::ids::row(module, crate::inspector::ids::Kind::Param, "cutoff");
    let field_id = crate::inspector::ids::part(&row, "field");
    let field = node_of(&mut ui.app, &field_id);
    let found = node(&update, field);
    assert_eq!(found.role(), Role::TextInput);
    assert!(found.label().is_some_and(|name| name.contains("value")));
    assert!(found.supports_action(Action::Focus) && found.supports_action(Action::SetValue));

    let _ = ui.deliver(Message::A11y(Event::Action(request(
        Action::SetValue,
        field,
        Some(ActionData::Value("640".into())),
    ))));
    let index = ui
        .app
        .session
        .controller
        .graph()
        .node(ModuleId(module))
        .unwrap()
        .parameter_index("cutoff")
        .unwrap();
    let value = ui.app.session.controller.graph().node(ModuleId(module)).unwrap().params[index];
    assert!((value - 640.0).abs() < 0.5, "{value}");

    // Focus moves keyboard focus to the field, and the tree says so.
    let _ = ui.deliver(Message::A11y(Event::Action(request(Action::Focus, field, None))));
    assert_eq!(ui.focused_field(), ui.app.value_field_part(module, index));
    assert_eq!(
        ui.app.focus,
        ui.app.value_field_part(module, index),
        "the app heard of it"
    );
    let places = ui.places();
    assert_eq!(ui.app.a11y_tree(&places).focus, field);
}

/// A row whose choices are shown at once is a set of options, one chosen,
/// and a Click on another option chooses it.
#[test]
fn an_inspector_option_click_chooses_it() {
    use super::items::Kind;
    let app = app_with_every_widget();
    let modules: Vec<u32> = app.ui_snapshot().tiles.iter().map(|tile| tile.id).collect();
    let mut ui = crate::ui_test::Ui::with_app(app);
    // The first module whose inspector shows a choice as options.
    let (module, options) = modules
        .into_iter()
        .find_map(|module| {
            let (_, _, items) = inspector_of(&mut ui, &[module]);
            let options = items
                .first()?
                .walk()
                .into_iter()
                .find(|item| item.kind == Kind::Options)?
                .clone();
            Some((module, options))
        })
        .expect("a module shows a choice as options");
    let (_, update, _) = inspector_of(&mut ui, &[module]);
    assert_eq!(
        node(&update, node_of(&mut ui.app, &options.id)).role(),
        Role::RadioGroup
    );
    let chosen = options.children.iter().filter(|o| o.on == Some(true)).count();
    assert_eq!(chosen, 1);
    let other = options
        .children
        .iter()
        .find(|o| o.on == Some(false))
        .expect("another option")
        .clone();
    let target = node_of(&mut ui.app, &other.id);
    assert_eq!(node(&update, target).role(), Role::RadioButton);
    ui_act(
        &mut ui,
        &request(Action::Click, target, None),
        std::time::Instant::now(),
    );
    let (_, update, _) = inspector_of(&mut ui, &[module]);
    assert_eq!(node(&update, target).toggled(), Some(accesskit::Toggled::True));
}

/// An inspector row keeps its node ID while other modules come and go, and
/// the table forgets the rows of a module no longer shown (ui-layer.md,
/// section 4, rule 4).
#[test]
fn inspector_node_ids_survive_other_modules_and_are_dropped_when_gone() {
    let (app, module, _) = app_with_filter();
    let mut ui = crate::ui_test::Ui::with_app(app);
    let row = crate::inspector::ids::row(module, crate::inspector::ids::Kind::Param, "cutoff");
    let _ = inspector_of(&mut ui, &[module]);
    let number = ui.app.a11y.ids.number(&row);

    let other = ui.app.session.controller.add_module("loom.filter.svf").unwrap().0;
    ui.app.commit();
    let _ = inspector_of(&mut ui, &[module]);
    assert_eq!(ui.app.a11y.ids.number(&row), number, "another module came");
    let _ = ui.app.session.controller.remove_module(ModuleId(other));
    ui.app.commit();
    let _ = inspector_of(&mut ui, &[module]);
    assert_eq!(ui.app.a11y.ids.number(&row), number, "and went");

    // Nothing selected: the rows are gone from the table. Asking for the
    // row's number above marked it as seen once more, so the first build
    // keeps it and the next drops it.
    let _ = inspector_of(&mut ui, &[]);
    let _ = inspector_of(&mut ui, &[]);
    assert_eq!(ui.app.a11y.ids.id(number), None);
}

/// An exposed parameter on a sub-patch tile and the inner module's own row
/// for it have different UI IDs, and `locate` finds each where the window
/// drew it.
#[test]
fn an_exposed_row_and_the_inner_row_differ_and_are_both_found() {
    let mut ui = crate::ui_test::Ui::open("nested-subtractive.ron");
    let (places, _, items) = inspector_of(&mut ui, &[2]);
    let exposed = crate::inspector::ids::row(2, crate::inspector::ids::Kind::Exposed, "cutoff");
    assert!(
        super::items::find(&items, &exposed).is_some(),
        "the tile shows its exposed Cutoff"
    );
    let found = crate::control::Found {
        canvas: places.canvas,
        text: None,
        places: places.clone(),
    };
    let at = crate::control::locate(&ui.app, &exposed.to_string(), &found).expect("found");
    assert!(at.width > 0.0);

    // Inside the sub-patch, the inner filter's own row.
    let inner = items[0]
        .walk()
        .into_iter()
        .find_map(|item| match &item.set_number {
            Some(super::items::SetNumber::Param { module, .. }) if item.id.is_under(&exposed) => Some(*module),
            _ => None,
        })
        .expect("the exposed row edits an inner module");
    ui.send(Message::OpenTile(2));
    let (places, _, items) = inspector_of(&mut ui, &[inner]);
    let own = crate::inspector::ids::row(inner, crate::inspector::ids::Kind::Param, "cutoff");
    assert_ne!(own, exposed);
    assert!(super::items::find(&items, &own).is_some(), "the inner module's own row");
    let found = crate::control::Found {
        canvas: places.canvas,
        text: None,
        places,
    };
    let at = crate::control::locate(&ui.app, &own.to_string(), &found).expect("found");
    assert!(at.width > 0.0);
}

/// A module whose inspector is taller than its panel, so it scrolls, and
/// the harness with it selected and drawn.
fn scrolling_inspector() -> (crate::ui_test::Ui, u32) {
    let app = app_with_every_widget();
    let modules: Vec<u32> = app.ui_snapshot().tiles.iter().map(|tile| tile.id).collect();
    let mut ui = crate::ui_test::Ui::with_app(app);
    for module in modules {
        ui.send(Message::Canvas(CanvasEvent::Select(vec![module])));
        let _ = ui.redraw();
        let places = ui.places();
        let panel = places.of(&crate::inspector::ids::panel(module));
        if panel.is_some_and(|place| place.clip.is_some_and(|clip| place.rect.height > clip.height + 40.0)) {
            return (ui, module);
        }
    }
    panic!("no module's inspector is taller than its panel");
}

/// Scrolls the inspector down by a few lines, with the pointer over it.
fn scroll_inspector(ui: &mut crate::ui_test::Ui, module: u32) -> Vec<Message> {
    let panel = ui.places().of(&crate::inspector::ids::panel(module)).unwrap();
    let over = panel.clip.unwrap().center();
    let _ = ui.move_to(over);
    ui.step(&[iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
        delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -6.0 },
    })])
}

/// With no screen reader, a scroll of the inspector sends no message, as
/// before. With one, it sends one that changes nothing but asks for a
/// rebuild, and the rows' nodes move with the rows (section 7, rule 7).
#[test]
fn a_scroll_moves_the_rows_and_is_a_message_only_while_listening() {
    let (mut ui, module) = scrolling_inspector();
    let before = ui.scroll_translation(crate::inspector::SCROLL_ID).unwrap();
    let messages = scroll_inspector(&mut ui, module);
    assert_ne!(
        ui.scroll_translation(crate::inspector::SCROLL_ID).unwrap(),
        before,
        "it scrolled"
    );
    assert!(
        !messages
            .iter()
            .any(|message| matches!(message, Message::Inspector(crate::inspector::Message::Scrolled))),
        "no message with no screen reader"
    );

    ui.app.a11y_event(Event::Activated);
    let _ = ui.redraw();
    let places = ui.places();
    let row = super::items::find(&ui.app.a11y_frames().panel, &crate::inspector::ids::panel(module))
        .and_then(|panel| {
            panel
                .children
                .iter()
                .find(|item| item.kind == super::items::Kind::Group)
        })
        .map(|item| item.id.clone())
        .expect("a row");
    let row_node = node_of(&mut ui.app, &row);
    let y = |update: &TreeUpdate| node(update, row_node).bounds().unwrap().y0;
    let first = ui.app.a11y_tree(&places);
    ui.app.a11y.locating = false;
    let messages = scroll_inspector(&mut ui, module);
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::Inspector(crate::inspector::Message::Scrolled)))
    );
    // The harness runs the rebuild it asked for at once.
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::A11y(Event::Located(_)))),
        "the scroll asked for a rebuild"
    );
    let places = ui.places();
    let moved = ui.app.a11y_tree(&places);
    assert!(y(&moved) < y(&first), "the row's node moved up with the row");
}

/// A panel taller than the area that scrolls it says it is as tall as what
/// shows, so a screen reader does not take rows out of view as visible.
/// The rows keep their real bounds.
#[test]
fn a_scrolling_panel_is_as_tall_as_what_it_shows() {
    let (mut ui, module) = scrolling_inspector();
    ui.app.a11y_event(Event::Activated);
    let _ = ui.redraw();
    let places = ui.places();
    let panel = crate::inspector::ids::panel(module);
    let place = places.of(&panel).unwrap();
    let clip = place.clip.unwrap();
    let update = ui.app.a11y_tree(&places);
    let bounds = node(&update, node_of(&mut ui.app, &panel)).bounds().unwrap();
    let scale = f64::from(ui.app.a11y.scale);
    assert!(
        (bounds.height() - f64::from(clip.height) * scale).abs() < 1.0,
        "{bounds:?} {clip:?}"
    );
    assert!(bounds.height() < f64::from(place.rect.height) * scale - 1.0);
}

/// A field that has the keys is not reported as focused while a panel lies
/// over the window, even before the window says the keys have moved: it is
/// hidden, and a hidden node cannot have focus.
#[test]
fn a_covered_field_is_not_focused() {
    let (app, module, _) = app_with_filter();
    let mut ui = crate::ui_test::Ui::with_app(app);
    let _ = inspector_of(&mut ui, &[module]);
    let row = crate::inspector::ids::row(module, crate::inspector::ids::Kind::Param, "cutoff");
    let field = node_of(&mut ui.app, &crate::inspector::ids::part(&row, "field"));
    let _ = ui.deliver(Message::A11y(Event::Action(request(Action::Focus, field, None))));
    let places = ui.places();
    assert_eq!(ui.app.a11y_tree(&places).focus, field, "focused while open");

    let _ = ui.app.send(Message::ShowShortcuts);
    let update = ui.app.a11y_tree(&places);
    assert!(node(&update, field).is_hidden());
    assert_ne!(update.focus, field, "not focused while covered");
}

/// A line under Drives keeps its node while a line before it is removed:
/// it is keyed by its assignment, not its place (section 4, rule 4).
#[test]
fn a_drives_line_keeps_its_node_when_one_before_it_goes() {
    let (mut app, filter, _) = app_with_filter();
    let lfo = app.session.controller.add_module("loom.mod.lfo").unwrap();
    app.commit();
    let graph = app.session.controller.graph();
    let cutoff = graph.node(ModuleId(filter)).unwrap().parameter_index("cutoff").unwrap();
    let resonance = graph
        .node(ModuleId(filter))
        .unwrap()
        .parameter_index("resonance")
        .unwrap();
    let out = loom_core::OutputRef { module: lfo, port: 0 };
    let first = app
        .session
        .controller
        .add_assignment(out, ModuleId(filter), cutoff, 0.5)
        .unwrap();
    let second = app
        .session
        .controller
        .add_assignment(out, ModuleId(filter), resonance, 0.5)
        .unwrap();
    app.commit();
    let mut ui = crate::ui_test::Ui::with_app(app);
    let lines = |ui: &mut crate::ui_test::Ui| -> Vec<UiId> {
        let (_, _, items) = inspector_of(ui, &[lfo.0]);
        let drives = crate::inspector::ids::part(&crate::inspector::ids::panel(lfo.0), "drives");
        super::items::find(&items, &drives)
            .expect("the Drives list")
            .children
            .iter()
            .map(|line| line.id.clone())
            .collect()
    };
    let before = lines(&mut ui);
    assert_eq!(before.len(), 2);
    let kept = before
        .iter()
        .find(|id| id.to_string().ends_with(&format!("/{}", second.0)))
        .expect("the second line is keyed by its assignment")
        .clone();
    let number = ui.app.a11y.ids.number(&kept);
    let _ = ui.app.session.controller.remove_assignment(first).unwrap();
    ui.app.commit();
    assert_eq!(lines(&mut ui), vec![kept.clone()]);
    assert_eq!(ui.app.a11y.ids.number(&kept), number);
}

/// One Increment on the inspector's Cutoff slider moves a tenth of one on
/// the tile's Cutoff knob: 0.1% of travel against 1%. Each steps as its
/// own arrow key does (section 7, rule 5). Brylie chose 1% for both, but
/// the toolkit's slider shares one step between its arrow keys and a drag,
/// so the slider keeps 0.1% until our own slider separates them in phase 3
/// (plan, step 3.1).
#[test]
fn the_inspector_slider_steps_a_tenth_of_the_tile_knob_until_phase_3() {
    let (app, module, knob) = app_with_filter();
    let mut ui = crate::ui_test::Ui::with_app(app);
    let _ = inspector_of(&mut ui, &[module]);
    let index = ui
        .app
        .session
        .controller
        .graph()
        .node(ModuleId(module))
        .unwrap()
        .parameter_index("cutoff")
        .unwrap();
    let spec = ui.app.control_target(module, knob, ControlPart::Main).unwrap().spec;
    let position =
        |app: &App| spec.normalize(app.session.controller.graph().node(ModuleId(module)).unwrap().params[index]);
    let at = std::time::Instant::now();
    let mut moved = Vec::new();
    for target in ["inspector", "tile"] {
        ui.send(Message::Inspector(crate::inspector::Message::Param {
            module,
            index,
            position: 0.5,
        }));
        ui.send(Message::SliderReleased);
        let _ = inspector_of(&mut ui, &[module]);
        let node_id = if target == "inspector" {
            let row = crate::inspector::ids::row(module, crate::inspector::ids::Kind::Param, "cutoff");
            node_of(&mut ui.app, &crate::inspector::ids::part(&row, "value"))
        } else {
            widget_node(&mut ui.app, module, knob)
        };
        let before = position(&ui.app);
        ui_act(&mut ui, &request(Action::Increment, node_id, None), at);
        ui.app.reader_event(crate::edits::ReaderEvent::Close);
        moved.push(position(&ui.app) - before);
    }
    assert!((moved[0] - 0.001).abs() < 1e-4, "the inspector slider: {moved:?}");
    assert!((moved[1] - 0.01).abs() < 1e-4, "the tile knob: {moved:?}");
}

/// `modulation.ron` with the Matrix tab open, drawn, and its tree built.
/// Two assignments act on the filter's cutoff: from the LFO and from an
/// envelope.
fn matrix_ui() -> crate::ui_test::Ui {
    let mut ui = crate::ui_test::Ui::open("modulation.ron");
    ui.send(Message::Matrix(crate::matrix::Event::Tab(crate::matrix::Tab::Matrix)));
    let _ = ui.redraw();
    let places = ui.places();
    let _ = ui.app.a11y_tree(&places);
    ui
}

/// The tree as it is now in `ui`, drawn first.
fn matrix_tree(ui: &mut crate::ui_test::Ui) -> (Places, TreeUpdate) {
    let _ = ui.redraw();
    let places = ui.places();
    let update = ui.app.a11y_tree(&places);
    (places, update)
}

/// The first assignment in the graph's order.
fn first_assignment(app: &App) -> loom_core::AssignmentId {
    app.session
        .controller
        .graph()
        .assignments()
        .next()
        .expect("an assignment")
        .0
}

/// Every part the matrix reports has a role, every part but text has a
/// name, and every part that acts is where the window drew it, inside the
/// panel. With a sort set and the Add form open, so the reverse button and
/// the form's parts are checked too.
#[test]
fn every_matrix_part_is_named_and_placed() {
    use super::items::Kind;
    let mut ui = matrix_ui();
    ui.send(Message::Matrix(crate::matrix::Event::SortBy(Some(
        crate::matrix::Column::Depth,
    ))));
    ui.send(Message::Matrix(crate::matrix::Event::AddOpen));
    let (places, update) = matrix_tree(&mut ui);
    let items = ui.app.a11y_frames().panel;
    assert_eq!(items.len(), 1, "one panel");
    let panel = &items[0];
    assert_eq!(panel.id, crate::matrix::access::panel());
    let panel_place = places.of(&panel.id).expect("the panel is drawn");
    assert!(node(&update, node_of(&mut ui.app, &panel.id)).clips_children());
    let mut acting = 0;
    for item in panel.walk() {
        let found = node(&update, node_of(&mut ui.app, &item.id));
        if item.kind == Kind::Text {
            assert_eq!(found.label(), None, "{}", item.id);
            continue;
        }
        assert!(
            item.label.as_deref().is_some_and(|name| !name.is_empty()),
            "{} has a name",
            item.id
        );
        let acts = item.click.is_some()
            || item.step.0.is_some()
            || item.step.1.is_some()
            || item.set_number.is_some()
            || item.set_text.is_some()
            || item.focus.is_some();
        if !acts {
            continue;
        }
        acting += 1;
        let place = places
            .of(&item.id)
            .unwrap_or_else(|| panic!("the window drew no `{}`", item.id));
        assert!(place.rect.width > 0.0 && place.rect.height > 0.0, "{}", item.id);
        assert!(found.bounds().is_some(), "{} has bounds", item.id);
        assert!(
            place.rect.x >= panel_place.rect.x - 0.5
                && place.rect.x + place.rect.width <= panel_place.rect.x + panel_place.rect.width + 0.5,
            "{} lies in the panel",
            item.id
        );
    }
    // The filter, the switch, Add, the sort, its reverse, the form's two
    // picks and two buttons, and six parts on each of the two rows.
    assert_eq!(acting, 9 + 2 * 6);
}

/// The assignments are a list, not a table: the list says how many rows it
/// has, each row is a list item with its place in it, and the rows follow
/// the sort the view shows.
#[test]
fn the_matrix_rows_are_a_list_in_the_order_shown() {
    let mut ui = matrix_ui();
    for reverse in [false, true] {
        ui.send(Message::Matrix(crate::matrix::Event::SortBy(Some(
            crate::matrix::Column::Source,
        ))));
        if reverse {
            ui.send(Message::Matrix(crate::matrix::Event::ReverseSort));
        }
        let (_, update) = matrix_tree(&mut ui);
        let list_id = crate::matrix::access::part("assignments");
        let list = node(&update, node_of(&mut ui.app, &list_id));
        assert_eq!(list.role(), Role::List);
        assert_eq!(list.size_of_set(), Some(2));
        let model = ui.app.session.matrix.model(
            ui.app.session.controller.graph(),
            &ui.app.session.selection,
            ui.app.right_tab,
        );
        let shown: Vec<NodeId> = model
            .rows
            .iter()
            .map(|r| node_of(&mut ui.app, &crate::matrix::access::row(loom_core::AssignmentId(r.id))))
            .collect();
        assert_eq!(
            list.children(),
            shown.as_slice(),
            "reverse {reverse}: in the order shown"
        );
        for (at, row) in shown.iter().enumerate() {
            let row = node(&update, *row);
            assert_eq!(row.role(), Role::ListItem);
            assert_eq!(row.position_in_set(), Some(at));
            assert!(row.label().is_some_and(|name| name.starts_with("Modulation from ")));
        }
        ui.send(Message::Matrix(crate::matrix::Event::SortBy(None)));
    }
}

/// The matrix's parts are reported only while its tab is open, and the
/// inspector's only while its own is.
#[test]
fn the_matrix_is_reported_only_while_its_tab_is_open() {
    let mut ui = matrix_ui();
    let matrix = crate::matrix::access::panel();
    assert!(super::items::find(&ui.app.a11y_frames().panel, &matrix).is_some());
    ui.send(Message::Matrix(crate::matrix::Event::Tab(
        crate::matrix::Tab::Inspector,
    )));
    let (_, update) = matrix_tree(&mut ui);
    let frames = ui.app.a11y_frames();
    assert!(super::items::find(&frames.panel, &matrix).is_none());
    let row = crate::matrix::access::row(first_assignment(&ui.app));
    assert!(
        !update
            .nodes
            .iter()
            .any(|(id, _)| ui.app.a11y.ids.id(id.0) == Some(&row)),
        "no row of the matrix is in the tree"
    );
}

/// A source step goes to the next source the assignment may take. It skips
/// the envelope, whose pair with the cutoff another row already has, so it
/// edits something and warns of nothing. Each step is its own undo step,
/// since it may give the row a new ID, and the reported focus follows the
/// row.
#[test]
fn a_matrix_source_step_skips_a_taken_pair_and_is_its_own_undo_step() {
    let mut ui = matrix_ui();
    let graph = ui.app.session.controller.graph();
    let (id, a) = graph.assignments().next().map(|(id, a)| (id, *a)).unwrap();
    let other = graph.assignments().nth(1).map(|(_, a)| a.source).unwrap();
    let options = crate::matrix::sources(graph);
    // The step lands on a source no other row joins to this target.
    let Some(Message::Matrix(crate::matrix::Event::Source(_, pick))) = ui.app.matrix_source_step(id, true, &options)
    else {
        panic!("a source step");
    };
    assert_ne!(pick.output, other, "the taken pair is skipped");
    assert_ne!(pick.output, a.source);
    let from = options.iter().position(|s| s.output == a.source).unwrap();
    let to = options.iter().position(|s| s.output == pick.output).unwrap();
    assert!(to > from, "forward");
    for skipped in &options[from + 1..to] {
        assert!(
            skipped.output == other || !ui.app.allows_assignment(skipped.output, a.target, a.param),
            "only a source the assignment may not take is skipped: {}",
            skipped.label
        );
    }

    let steps = ui.app.session.edits.history.len();
    let source = crate::matrix::access::cell(id, crate::matrix::Column::Source);
    let target = node_of(&mut ui.app, &source);
    let at = std::time::Instant::now();
    ui_act(&mut ui, &request(Action::Increment, target, None), at);
    let moved = ui
        .app
        .session
        .controller
        .graph()
        .assignments()
        .find(|(_, b)| b.source == pick.output && b.target == a.target && b.param == a.param)
        .map(|(id, _)| id)
        .expect("retargeted onto the pick");
    assert_eq!(ui.app.session.edits.history.len(), steps + 1, "one undo step");
    assert_ne!(
        ui.app.alerts.severity(),
        crate::alerts::Severity::Warning,
        "nothing refused, nothing warned: {}",
        ui.app.alerts.status()
    );
    assert_eq!(ui.app.session.matrix.selected, Some(moved));
    let (_, update) = matrix_tree(&mut ui);
    let cell = crate::matrix::access::cell(moved, crate::matrix::Column::Source);
    assert_eq!(
        update.focus,
        node_of(&mut ui.app, &cell),
        "focus is on the row's source"
    );
    let cell_node = node_of(&mut ui.app, &cell);
    ui_act(&mut ui, &request(Action::Decrement, cell_node, None), at);
    assert_eq!(
        ui.app.session.edits.history.len(),
        steps + 2,
        "a second step of its own"
    );
}

/// A step with nothing allowed that way does nothing: the node offers the
/// action, but no message is sent and nothing is said.
#[test]
fn a_matrix_step_with_nothing_allowed_does_nothing() {
    let mut ui = matrix_ui();
    let id = first_assignment(&ui.app);
    // Step back until no source is left that way.
    for _ in 0..64 {
        let Some(message) =
            ui.app
                .matrix_source_step(id, false, &crate::matrix::sources(ui.app.session.controller.graph()))
        else {
            break;
        };
        ui.send(message);
    }
    let id = ui.app.session.matrix.selected.unwrap_or(id);
    assert!(
        ui.app
            .matrix_source_step(id, false, &crate::matrix::sources(ui.app.session.controller.graph()))
            .is_none()
    );
    let steps = ui.app.session.edits.history.len();
    let status = ui.app.alerts.status().to_string();
    let source = crate::matrix::access::cell(id, crate::matrix::Column::Source);
    let _ = matrix_tree(&mut ui);
    let node_id = node_of(&mut ui.app, &source);
    ui_act(
        &mut ui,
        &request(Action::Decrement, node_id, None),
        std::time::Instant::now(),
    );
    assert_eq!(ui.app.session.edits.history.len(), steps);
    assert_eq!(ui.app.alerts.status(), status, "nothing is said");
}

/// A row's depth is a slider in percent. SetValue sets what typing the
/// depth would, and quick steps on it are one undo step, as on a tile.
#[test]
fn a_matrix_depth_is_a_slider_in_percent_that_groups() {
    let mut ui = matrix_ui();
    let id = first_assignment(&ui.app);
    let depth_id = crate::matrix::access::cell(id, crate::matrix::Column::Depth);
    let (_, update) = matrix_tree(&mut ui);
    let slider = node_of(&mut ui.app, &depth_id);
    let found = node(&update, slider);
    assert_eq!(found.role(), Role::Slider);
    assert_eq!(
        (found.min_numeric_value(), found.max_numeric_value()),
        (Some(-100.0), Some(100.0))
    );
    assert_eq!(found.numeric_value_step(), Some(1.0));
    let depth = |app: &App| app.session.controller.graph().assignment(id).unwrap().depth;
    let steps = ui.app.session.edits.history.len();
    let at = std::time::Instant::now();
    ui_act(
        &mut ui,
        &request(Action::SetValue, slider, Some(ActionData::NumericValue(40.0))),
        at,
    );
    assert!((depth(&ui.app) - 0.4).abs() < 1e-6);
    ui_act(&mut ui, &request(Action::Increment, slider, None), at);
    ui_act(&mut ui, &request(Action::Increment, slider, None), at);
    assert!((depth(&ui.app) - 0.42).abs() < 1e-5);
    ui.app.reader_event(crate::edits::ReaderEvent::Close);
    assert_eq!(
        ui.app.session.edits.history.len(),
        steps + 1,
        "the set and the steps are one undo step"
    );
    let (_, update) = matrix_tree(&mut ui);
    assert_eq!(update.focus, slider, "focus is on the cell acted on");
}

/// The filter is a field: SetValue filters as typing does, and Focus gives
/// it the keys.
#[test]
fn the_matrix_filter_takes_text_and_focus() {
    let mut ui = matrix_ui();
    let filter = crate::matrix::access::part("filter");
    let (_, update) = matrix_tree(&mut ui);
    let node_id = node_of(&mut ui.app, &filter);
    let found = node(&update, node_id);
    assert_eq!(found.role(), Role::TextInput);
    assert!(found.supports_action(Action::Focus) && found.supports_action(Action::SetValue));
    ui_act(
        &mut ui,
        &request(Action::SetValue, node_id, Some(ActionData::Value("lfo".into()))),
        std::time::Instant::now(),
    );
    assert_eq!(ui.app.session.matrix.filter, "lfo");
    let (_, update) = matrix_tree(&mut ui);
    assert_eq!(
        node(
            &update,
            node_of(&mut ui.app, &crate::matrix::access::part("assignments"))
        )
        .size_of_set(),
        Some(1),
        "one row is left"
    );
}

/// A row keeps its node ID while another row comes and goes, and the table
/// forgets the parts of a row that was removed (ui-layer.md, section 4,
/// rule 4).
#[test]
fn matrix_node_ids_survive_other_rows_and_are_dropped_when_gone() {
    let mut ui = matrix_ui();
    let ids: Vec<_> = ui
        .app
        .session
        .controller
        .graph()
        .assignments()
        .map(|(id, _)| id)
        .collect();
    let (kept, gone) = (ids[0], ids[1]);
    let kept_cell = crate::matrix::access::cell(kept, crate::matrix::Column::Depth);
    let gone_cell = crate::matrix::access::cell(gone, crate::matrix::Column::Depth);
    let kept_node = node_of(&mut ui.app, &kept_cell);
    let gone_node = node_of(&mut ui.app, &gone_cell);
    ui.send(Message::Matrix(crate::matrix::Event::Remove(gone)));
    let _ = matrix_tree(&mut ui);
    let _ = matrix_tree(&mut ui);
    assert_eq!(
        ui.app.a11y.ids.id(kept_node.0),
        Some(&kept_cell),
        "the kept row's node stays"
    );
    assert_eq!(
        ui.app.a11y.ids.id(gone_node.0),
        None,
        "the removed row's entries are gone"
    );
}

/// With no screen reader, a scroll of the rows sends no message. With one,
/// it sends one that changes nothing and asks for a rebuild.
#[test]
fn a_matrix_scroll_is_a_message_only_while_listening() {
    let mut ui = matrix_ui();
    let a = *ui
        .app
        .session
        .controller
        .graph()
        .assignment(first_assignment(&ui.app))
        .unwrap();
    for n in 0..30 {
        ui.app
            .session
            .controller
            .add_assignment(a.source, a.target, a.param, -0.9 + n as f32 * 0.03)
            .unwrap();
    }
    let scroll = |ui: &mut crate::ui_test::Ui| {
        let _ = ui.redraw();
        let list = ui.places().of(&crate::matrix::access::part("assignments")).unwrap();
        let _ = ui.move_to(list.clip.unwrap().center());
        ui.step(&[iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -6.0 },
        })])
    };
    let scrolled = |messages: &[Message]| {
        messages
            .iter()
            .any(|message| matches!(message, Message::Matrix(crate::matrix::Event::Scrolled)))
    };
    let before = ui.scroll_translation("matrix-table").unwrap();
    let messages = scroll(&mut ui);
    assert_ne!(ui.scroll_translation("matrix-table").unwrap(), before, "it scrolled");
    assert!(!scrolled(&messages), "no message with no screen reader");
    ui.app.a11y_event(Event::Activated);
    // Its rebuild is not asked for here: this test counts the ones that
    // one message makes.
    drop(ui.app.take_window_questions());
    ui.app.a11y.locating = false;
    let messages = scroll(&mut ui);
    assert!(scrolled(&messages));
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::A11y(Event::Located(_)))),
        "the scroll asked for a rebuild"
    );
}

/// A screen reader steps an assignment's target in the matrix until it
/// lands on a parameter inside a sub-patch, where the retarget gives the
/// row a new ID (sub-patches.md, Editor model). Every step goes to a target
/// the assignment may take, so each one edits and none warns; each is an
/// undo step of its own; and the reported focus follows the row to its new
/// ID (ui-layer.md, decisions, 2026-11-11).
#[test]
fn a_screen_reader_target_step_follows_a_renumbered_row_inside_a_sub_patch() {
    use crate::matrix::{Column, Event, Tab, access};
    use loom_core::ModuleNames as _;
    use loom_patch::ModulePath;
    let mut ui = crate::ui_test::Ui::open("nested-subtractive.ron");
    let (svf, cutoff) = {
        let graph = ui.app.session.controller.graph();
        let (svf, node) = graph
            .nodes()
            .find(|(_, n)| n.descriptor.id == "loom.filter.svf")
            .expect("the filter two levels down");
        (svf, node.parameter_index("cutoff").unwrap())
    };
    let note_pitch = loom_core::OutputRef {
        module: ModuleId(1),
        port: 0,
    };
    let scope = ModuleId(11);
    let window = {
        let graph = ui.app.session.controller.graph();
        graph.node(scope).unwrap().parameter_index("window").unwrap()
    };
    ui.app.assign(note_pitch, scope, window);
    let outer = ui.app.session.controller.graph().assignments().next().unwrap().0;
    // An LFO inside the Voice onto the same cutoff, so the inner
    // assignment comes first in the file's order.
    ui.app.show_level(&ModulePath::top(2));
    let lfo = ui.app.modules.iter().copied().find(|d| d.id == "loom.mod.lfo").unwrap();
    ui.app.add_module(lfo, Point::new(100.0, 300.0));
    let inner = ModuleId(
        *ui.app
            .session
            .selection
            .iter()
            .next()
            .expect("the new module is selected"),
    );
    ui.app
        .assign(loom_core::OutputRef { module: inner, port: 0 }, svf, cutoff);
    ui.app.show_level(&ModulePath::root());
    ui.send(Message::Matrix(Event::Tab(Tab::Matrix)));

    let targets = crate::matrix::targets(ui.app.session.controller.graph());
    let goal = targets
        .iter()
        .position(|t| (t.module, t.param) == (svf, cutoff))
        .expect("the cutoff is a target");
    let mut id = outer;
    let mut steps = 0;
    let history = ui.app.session.edits.history.len();
    loop {
        let a = *ui.app.session.controller.graph().assignment(id).unwrap();
        let at = targets
            .iter()
            .position(|t| (t.module, t.param) == (a.target, a.param))
            .unwrap();
        if at == goal {
            break;
        }
        let forward = goal > at;
        let cell = access::cell(id, Column::Target);
        let node = node_of(&mut ui.app, &cell);
        let action = if forward { Action::Increment } else { Action::Decrement };
        ui_act(&mut ui, &request(action, node, None), std::time::Instant::now());
        steps += 1;
        assert_ne!(
            ui.app.alerts.severity(),
            crate::alerts::Severity::Warning,
            "step {steps} warned: {}",
            ui.app.alerts.status()
        );
        id = ui.app.session.matrix.selected.expect("a row is chosen");
        let now = ui.app.session.controller.graph().assignment(id).unwrap();
        assert_eq!(now.source, note_pitch, "the same assignment, followed");
        assert!(ui.app.allows_assignment(note_pitch, now.target, now.param));
        assert!(steps < targets.len(), "the steps reach the cutoff");
    }
    assert_eq!(
        ui.app.session.edits.history.len(),
        history + steps,
        "one undo step each"
    );
    let graph = ui.app.session.controller.graph();
    assert!(graph.assignment(outer).is_none(), "the moved assignment got a new id");
    assert!(id > outer);

    let _ = ui.redraw();
    let places = ui.places();
    let update = ui.app.a11y_tree(&places);
    let cell = access::cell(id, Column::Target);
    assert_eq!(
        update.focus,
        node_of(&mut ui.app, &cell),
        "focus is on the renumbered row's target"
    );
    assert!(
        ui.app.a11y.ids.id(update.focus.0) == Some(&cell),
        "the focused node is the new row's"
    );
}

/// A depth a drag left between two percents steps onto the percent grid:
/// the first step is shorter, and every step after lands on a whole
/// percent, so what is heard and what is set never drift apart.
#[test]
fn a_matrix_depth_steps_onto_the_percent_grid() {
    let mut ui = matrix_ui();
    let id = first_assignment(&ui.app);
    ui.send(Message::Modulation(crate::modulation::Message::Depth {
        id,
        depth: 0.304,
    }));
    ui.send(Message::SliderReleased);
    let depth_id = crate::matrix::access::cell(id, crate::matrix::Column::Depth);
    let _ = matrix_tree(&mut ui);
    let slider = node_of(&mut ui.app, &depth_id);
    let depth = |app: &App| app.session.controller.graph().assignment(id).unwrap().depth;
    let at = std::time::Instant::now();
    ui_act(&mut ui, &request(Action::Increment, slider, None), at);
    assert!((depth(&ui.app) - 0.31).abs() < 1e-6, "{}", depth(&ui.app));
    for _ in 0..19 {
        ui_act(&mut ui, &request(Action::Increment, slider, None), at);
    }
    assert!((depth(&ui.app) - 0.50).abs() < 1e-6, "no drift: {}", depth(&ui.app));
    let (_, update) = matrix_tree(&mut ui);
    assert_eq!(node(&update, slider).numeric_value(), Some(50.0));
    ui_act(&mut ui, &request(Action::Decrement, slider, None), at);
    assert!((depth(&ui.app) - 0.49).abs() < 1e-6);
}

/// A modulation card's depth a drag left between two percents steps onto
/// the percent grid: the first step is shorter, and every step after lands
/// on a whole percent, so what is heard and what is set never drift apart.
#[test]
fn an_inspector_depth_steps_onto_the_percent_grid() {
    let mut ui = crate::ui_test::Ui::open("modulation.ron");
    let filter = 4;
    let (id, _) = ui
        .app
        .session
        .controller
        .graph()
        .assignments()
        .find(|(_, a)| a.target.0 == filter)
        .expect("an assignment on the filter");
    ui.send(Message::Modulation(crate::modulation::Message::Depth {
        id,
        depth: 0.304,
    }));
    ui.send(Message::SliderReleased);
    let row = crate::inspector::ids::row(filter, crate::inspector::ids::Kind::Param, "cutoff");
    let depth_id = crate::inspector::ids::part(&crate::inspector::ids::assignment(&row, id.0), "depth");
    let (_, _, items) = inspector_of(&mut ui, &[filter]);
    assert!(
        super::items::find(&items, &depth_id).is_some(),
        "the card's depth is reported"
    );
    let slider = node_of(&mut ui.app, &depth_id);
    let depth = |app: &App| app.session.controller.graph().assignment(id).unwrap().depth;
    let at = std::time::Instant::now();
    ui_act(&mut ui, &request(Action::Increment, slider, None), at);
    assert!((depth(&ui.app) - 0.31).abs() < 1e-6, "{}", depth(&ui.app));
    for _ in 0..19 {
        ui_act(&mut ui, &request(Action::Increment, slider, None), at);
    }
    assert!((depth(&ui.app) - 0.50).abs() < 1e-6, "no drift: {}", depth(&ui.app));
    let (_, update, _) = inspector_of(&mut ui, &[filter]);
    assert_eq!(node(&update, slider).numeric_value(), Some(50.0));
}

/// `subtractive.ron` with one of slice 4a's overlays open, drawn, and its
/// tree built: the tile menu, the probe menu, the Recent list with two
/// files, module help, the shortcuts page, and a question.
fn with_each_small_overlay() -> Vec<(&'static str, crate::ui_test::Ui)> {
    use crate::overlays::{ContextMenu, Overlay};
    let at = Point::new(300.0, 300.0);
    let open = |change: &dyn Fn(&mut App)| {
        let mut ui = crate::ui_test::Ui::open("subtractive.ron");
        change(&mut ui.app);
        let _ = ui.redraw();
        ui
    };
    let rows = |app: &App| {
        let _ = app;
        ["one.ron", "two.ron"]
            .into_iter()
            .map(|name| crate::recent::Row {
                path: std::path::PathBuf::from("/patches").join(name),
                name: name.to_string(),
                folder: "/patches".to_string(),
                missing: false,
            })
            .collect::<Vec<_>>()
    };
    vec![
        (
            "tile menu",
            open(&|app| app.overlays.open(Overlay::Menu(ContextMenu::Tile { module: 4, at }))),
        ),
        (
            "probe menu",
            open(&|app| {
                let output = loom_canvas::PortRef {
                    module: 4,
                    output: true,
                    index: 0,
                };
                let area = Rect::new(Point::new(0.0, 0.0), Size::new(800.0, 600.0));
                let menu = crate::probe_menu::ProbeMenu::open(output, at, at, area);
                app.overlays.open(Overlay::Menu(ContextMenu::Probe(menu)));
            }),
        ),
        (
            "recent",
            open(&|app| {
                let rows = rows(app);
                app.overlays.open(Overlay::RecentList(rows));
            }),
        ),
        (
            "help",
            open(&|app| {
                let _ = app.send(Message::ShowModuleHelp(4));
            }),
        ),
        (
            "shortcuts",
            open(&|app| app.overlays.help = Some(crate::help::Help::shortcuts())),
        ),
        (
            "question",
            open(&|app| app.overlays.question = Some(crate::question::Question::error("No room".to_string()))),
        ),
    ]
}

/// Each small overlay is reported while it owns the input: a modal root
/// with its role and name, and inside it, every part but text has a name,
/// and every part that acts is where the window drew it, except a link in
/// help's text, which the toolkit draws inside one widget (a phase 1 gap).
/// What the overlay covers stays hidden.
#[test]
fn every_small_overlay_part_is_named_and_placed() {
    use super::items::Kind;
    for (name, mut ui) in with_each_small_overlay() {
        let places = ui.places();
        let update = ui.app.a11y_tree(&places);
        let frames = ui.app.a11y_frames();
        assert_eq!(frames.overlay.len(), 1, "{name}: one overlay is reported");
        let root = &frames.overlay[0];
        let root_node = node(&update, node_of(&mut ui.app, &root.id));
        assert!(!root_node.is_hidden(), "{name}: the overlay is not hidden");
        assert!(root_node.is_modal(), "{name}: it owns the input");
        assert!(root.label.as_deref().is_some_and(|label| !label.is_empty()), "{name}");
        let role = root_node.role();
        assert!(
            matches!(role, Role::Menu | Role::Dialog | Role::AlertDialog),
            "{name}: {role:?}"
        );
        assert!(places.of(&root.id).is_some(), "{name}: the window drew {}", root.id);
        let mut acting = 0;
        for item in root.walk() {
            let found = node(&update, node_of(&mut ui.app, &item.id));
            assert!(!found.is_hidden(), "{name}: {} is not hidden", item.id);
            if item.kind == Kind::Text {
                assert!(found.value().is_some_and(|text| !text.is_empty()), "{}", item.id);
                continue;
            }
            assert!(
                item.label.as_deref().is_some_and(|label| !label.is_empty()),
                "{name}: {} has a name",
                item.id
            );
            if item.click.is_none() || item.kind == Kind::Link {
                continue;
            }
            acting += 1;
            let place = places
                .of(&item.id)
                .unwrap_or_else(|| panic!("{name}: the window drew no `{}`", item.id));
            assert!(place.rect.width > 0.0 && place.rect.height > 0.0, "{}", item.id);
            assert!(found.bounds().is_some(), "{} has bounds", item.id);
        }
        assert!(acting > 0, "{name}: something acts");
        let panic = node(&update, node_of(&mut ui.app, &tree::toolbar_id().with("panic")));
        assert!(panic.is_hidden(), "{name}: what it covers stays hidden");
    }
}

/// A menu's keys move its highlight, and the tree reports the highlighted
/// item as focus. With no highlight, focus is on the menu. Enter picks the
/// highlighted item and Escape closes the menu.
#[test]
fn a_menu_highlight_is_moved_by_keys_and_reported_as_focus() {
    use iced::keyboard::{Key, Modifiers, key::Named};
    let (_, mut ui) = with_each_small_overlay()
        .into_iter()
        .find(|(name, _)| *name == "probe menu")
        .unwrap();
    let focus = |ui: &mut crate::ui_test::Ui| {
        let _ = ui.redraw();
        let places = ui.places();
        let update = ui.app.a11y_tree(&places);
        ui.app.a11y.ids.id(update.focus.0).cloned()
    };
    assert_eq!(focus(&mut ui), Some(crate::menus::probe_id()), "on the menu");
    let _ = ui.tap(Key::Named(Named::ArrowDown), Modifiers::empty());
    assert_eq!(focus(&mut ui), Some(crate::menus::probe_id().with("scope")));
    let _ = ui.tap(Key::Named(Named::End), Modifiers::empty());
    assert_eq!(focus(&mut ui), Some(crate::menus::probe_id().with("meter")));
    let _ = ui.tap(Key::Named(Named::ArrowDown), Modifiers::empty());
    assert_eq!(focus(&mut ui), Some(crate::menus::probe_id().with("scope")), "it wraps");
    let modules = ui.app.session.controller.graph().nodes().count();
    let _ = ui.tap(Key::Named(Named::Enter), Modifiers::empty());
    assert!(
        ui.app.overlays.probe_menu().is_none(),
        "Enter picked, and the menu closed"
    );
    assert_eq!(
        ui.app.session.controller.graph().nodes().count(),
        modules + 1,
        "a scope was added"
    );

    let (_, mut ui) = with_each_small_overlay()
        .into_iter()
        .find(|(name, _)| *name == "tile menu")
        .unwrap();
    let _ = ui.tap(Key::Named(Named::Escape), Modifiers::empty());
    assert!(ui.app.overlays.tile_menu().is_none(), "Escape closed it");
}

/// A screen reader's Click on a menu item picks it, as a press does, while
/// the menu owns the input.
#[test]
fn a_menu_item_is_picked_by_a_screen_reader() {
    let (_, mut ui) = with_each_small_overlay()
        .into_iter()
        .find(|(name, _)| *name == "probe menu")
        .unwrap();
    let places = ui.places();
    let _ = ui.app.a11y_tree(&places);
    let meter = node_of(&mut ui.app, &crate::menus::probe_id().with("meter"));
    let modules = ui.app.session.controller.graph().nodes().count();
    ui.send(Message::A11y(Event::Action(request(Action::Click, meter, None))));
    assert!(ui.app.overlays.probe_menu().is_none());
    assert_eq!(ui.app.session.controller.graph().nodes().count(), modules + 1);
}

/// The question reports its highlighted answer as focus, and a screen
/// reader's Click on an answer gives it, as a press does, though most
/// messages wait while a question is open.
#[test]
fn the_question_reports_its_highlight_and_takes_a_screen_reader_answer() {
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    ui.app.overlays.question = Some(crate::question::Question::discard(
        "Patch".to_string(),
        crate::question::AfterDiscard::New,
    ));
    let _ = ui.redraw();
    let places = ui.places();
    let update = ui.app.a11y_tree(&places);
    let model = ui.app.overlays.question.as_ref().unwrap().model();
    assert_eq!(
        ui.app.a11y.ids.id(update.focus.0),
        Some(&crate::overlays::access::answer_id(model.highlight)),
        "focus is on the highlighted answer"
    );
    let root = node(&update, node_of(&mut ui.app, &crate::overlays::access::question_id()));
    assert_eq!(root.role(), Role::AlertDialog);
    // The sentence is heard as a text's value, not as a description a
    // screen reader may never say.
    assert_eq!(root.description(), None);
    let text = node_of(&mut ui.app, &crate::overlays::access::question_id().with("text"));
    assert_eq!(node(&update, text).value(), Some(model.text.as_str()));
    let cancel = node_of(&mut ui.app, &crate::overlays::access::answer_id("cancel"));
    ui.send(Message::A11y(Event::Action(request(Action::Click, cancel, None))));
    assert!(ui.app.overlays.question.is_none(), "Cancel answered it");
}

/// While help is open over a menu's level, only help is reported: an
/// overlay under the one that owns the input is not.
#[test]
fn only_the_overlay_that_owns_the_input_is_reported() {
    use crate::overlays::{ContextMenu, Overlay};
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    ui.app.overlays.open(Overlay::Menu(ContextMenu::Tile {
        module: 4,
        at: Point::new(300.0, 300.0),
    }));
    ui.app.overlays.help = Some(crate::help::Help::shortcuts());
    let _ = ui.redraw();
    let frames = ui.app.a11y_frames();
    assert_eq!(frames.overlay.len(), 1);
    assert_eq!(frames.overlay[0].id, crate::overlays::access::help_id());
}

/// The menu key opens the selected tile's menu with the pointer anywhere,
/// but never under an overlay that owns the input, such as help.
#[test]
fn the_menu_key_opens_no_tile_menu_under_help() {
    use iced::keyboard::{Key, Modifiers, key::Named};
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    let _ = ui.app.send(Message::Canvas(CanvasEvent::Select(vec![4])));
    let _ = ui.redraw();
    let _ = ui.tap(Key::Named(Named::ContextMenu), Modifiers::empty());
    assert_eq!(ui.app.overlays.tile_menu().map(|(module, _)| module), Some(4));
    ui.app.overlays.close_tile_menu();
    ui.app.overlays.help = Some(crate::help::Help::shortcuts());
    let _ = ui.redraw();
    let _ = ui.tap(Key::Named(Named::ContextMenu), Modifiers::empty());
    assert!(ui.app.overlays.tile_menu().is_none(), "help keeps the key");
    assert!(ui.app.overlays.help.is_some());
}

/// A tile's node offers ShowContextMenu, which opens the tile's menu at the
/// tile's top-left corner, as the menu key does. Under an open panel it
/// does nothing, as a right-click there does nothing.
#[test]
fn show_context_menu_on_a_tile_opens_its_menu() {
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    let _ = ui.redraw();
    let places = ui.places();
    let (whole, _) = ui.app.a11y_changes(&places);
    let tile = node_of(&mut ui.app, &tile_id(4));
    assert!(node(&whole, tile).supports_action(Action::ShowContextMenu));
    ui.send(Message::A11y(Event::Action(request(
        Action::ShowContextMenu,
        tile,
        None,
    ))));
    let (module, at) = ui.app.overlays.tile_menu().expect("the tile's menu opened");
    assert_eq!(module, 4);
    let view = places.canvas.unwrap();
    let corner = ui
        .app
        .ui_snapshot()
        .tiles
        .iter()
        .find(|t| t.id == 4)
        .map(|t| (t.bounds.x, t.bounds.y))
        .unwrap();
    // At the corner, kept inside the canvas as every tile menu is.
    let screen = view.camera.to_screen(Point::new(corner.0, corner.1));
    let b = view.bounds;
    let x = (b.x + screen.x).min(b.x + b.width - 140.0).max(b.x);
    let y = (b.y + screen.y).min(b.y + b.height - 36.0).max(b.y);
    assert!(
        (at.x - x).abs() < 0.01 && (at.y - y).abs() < 0.01,
        "{at:?} against ({x}, {y})"
    );

    ui.app.overlays.close_tile_menu();
    ui.app.overlays.help = Some(crate::help::Help::shortcuts());
    ui.send(Message::A11y(Event::Action(request(
        Action::ShowContextMenu,
        tile,
        None,
    ))));
    assert!(ui.app.overlays.tile_menu().is_none(), "nothing opens under a panel");
}

/// A tile menu opened from the keyboard, or by ShowContextMenu, starts on
/// its first item, so Enter picks at once and focus is on that item. One
/// opened by the pointer starts on none, with focus on the menu.
#[test]
fn a_menu_opened_by_key_starts_on_its_first_item() {
    let opened = |by_key: bool| {
        let mut ui = crate::ui_test::Ui::open("subtractive.ron");
        ui.send(Message::Canvas(CanvasEvent::TileMenu {
            module: 4,
            at: Point::new(100.0, 100.0),
            area: Rect::new(Point::new(0.0, 0.0), Size::new(800.0, 600.0)),
            by_key,
        }));
        let _ = ui.redraw();
        let places = ui.places();
        let update = ui.app.a11y_tree(&places);
        let focus = ui.app.a11y.ids.id(update.focus.0).cloned();
        (ui.app.overlays.menu_highlight(), focus, ui.app.menu_model().unwrap())
    };
    let (highlight, focus, menu) = opened(true);
    assert_eq!(highlight, Some(0));
    assert_eq!(focus.as_ref(), Some(&menu.items[0].id), "focus on the first item");
    let (highlight, focus, menu) = opened(false);
    assert_eq!(highlight, None);
    assert_eq!(focus, Some(menu.id), "focus on the menu");

    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    let _ = ui.redraw();
    let places = ui.places();
    let _ = ui.app.a11y_changes(&places);
    let tile = node_of(&mut ui.app, &tile_id(4));
    ui.send(Message::A11y(Event::Action(request(
        Action::ShowContextMenu,
        tile,
        None,
    ))));
    assert_eq!(
        ui.app.overlays.menu_highlight(),
        Some(0),
        "ShowContextMenu opens it by key"
    );
}

/// With the matrix holding the keys, a screen reader's Focus on a knob
/// gives the keys to the canvas, as a press on the canvas does: the next
/// arrow key steps the knob, and the matrix's chosen row stays where it was.
#[test]
fn focus_on_a_knob_takes_the_keys_from_the_matrix() {
    let mut ui = crate::ui_test::Ui::open("modulation.ron");
    ui.send(Message::Matrix(crate::matrix::Event::Tab(crate::matrix::Tab::Matrix)));
    let first = ui.app.session.controller.graph().assignments().next().unwrap().0;
    ui.send(Message::Matrix(crate::matrix::Event::Select(
        first,
        crate::matrix::Column::On,
    )));
    assert!(ui.app.matrix_has_keys());
    let _ = ui.app.a11y_tree(&Places::default());
    let filter = 4;
    let knob = ui
        .app
        .ui_snapshot()
        .tiles
        .iter()
        .find(|tile| tile.id == filter)
        .and_then(|tile| tile.widgets.iter().position(|w| w.label.eq_ignore_ascii_case("cutoff")))
        .expect("the filter's Cutoff knob");
    let target = widget_node(&mut ui.app, filter, knob);
    let index = {
        let node = ui.app.session.controller.graph().node(ModuleId(filter)).unwrap();
        loom_core::ModuleNames::parameter_index(node, "cutoff").unwrap()
    };
    let cutoff = |app: &App| app.session.controller.graph().node(ModuleId(filter)).unwrap().params[index];
    let before = cutoff(&ui.app);

    ui.send(Message::A11y(Event::Action(request(Action::Focus, target, None))));
    let _ = ui.redraw();
    assert!(!ui.app.matrix_has_keys(), "the canvas has the keys now");
    let _ = ui.tap(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowUp),
        iced::keyboard::Modifiers::empty(),
    );
    assert_ne!(cutoff(&ui.app), before, "the arrow stepped the knob");
    assert_eq!(
        ui.app.session.matrix.selected,
        Some(first),
        "the matrix's row is left as it was"
    );
}

/// With an inspector value field open, a screen reader's Focus on a knob
/// closes the field as a press on the canvas does: its draft is dropped,
/// not submitted, and the next arrow key steps the knob.
#[test]
fn focus_on_a_knob_closes_a_text_field_as_a_press_does() {
    let (app, module, knob) = app_with_filter();
    let mut ui = crate::ui_test::Ui::with_app(app);
    ui.send(Message::Canvas(CanvasEvent::Select(vec![module])));
    let _ = ui.redraw();
    let index = {
        let node = ui.app.session.controller.graph().node(ModuleId(module)).unwrap();
        loom_core::ModuleNames::parameter_index(node, "cutoff").unwrap()
    };
    let field = ui.value_field(module, index);
    let _ = ui.move_to(field);
    let _ = ui.press();
    let _ = ui.release();
    let _ = ui.typewrite("1234");
    assert!(ui.app.session.value_draft.is_some(), "a draft is typed");
    assert!(ui.app.focus.is_some());
    let cutoff = |app: &App| app.session.controller.graph().node(ModuleId(module)).unwrap().params[index];
    let before = cutoff(&ui.app);

    let _ = ui.app.a11y_tree(&Places::default());
    let target = widget_node(&mut ui.app, module, knob);
    ui.send(Message::A11y(Event::Action(request(Action::Focus, target, None))));
    let _ = ui.redraw();
    assert!(!ui.app.focus.is_some(), "the field is closed");
    assert!(ui.app.session.value_draft.is_none(), "its draft is dropped");
    assert_eq!(cutoff(&ui.app), before, "and not submitted");
    let _ = ui.tap(
        iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowUp),
        iced::keyboard::Modifiers::empty(),
    );
    assert_ne!(cutoff(&ui.app), before, "the arrow stepped the knob");
}

/// The inspector is reported in every state it can show, not only for a
/// plain module: an unknown module, several modules, a mapped parameter,
/// the sources offered while adding a modulation, a module inside a
/// sub-patch (with its expose buttons), a Patch port tile, a slot, and a
/// stack with copies and spreads. In each, the panel's items exist, every
/// part but text has a name, and every part that acts sends something.
#[test]
fn every_inspector_state_is_reported_with_named_parts() {
    use super::items::{Kind, messages};
    use crate::inspector::Inspector;
    // Every item of the panel shown now, after a redraw.
    let check = |ui: &mut crate::ui_test::Ui, name: &str, want: fn(&Inspector) -> bool| {
        let _ = ui.redraw();
        assert!(want(&ui.app.inspector_model()), "{name}: the inspector shows the state");
        let items = ui.app.a11y_frames().panel;
        assert!(!items.is_empty(), "{name}: the panel is reported");
        let mut acting = 0;
        for item in items.iter().flat_map(super::items::Item::walk) {
            if item.kind != Kind::Text {
                assert!(
                    item.label.as_deref().is_some_and(|label| !label.is_empty()),
                    "{name}: {} has a name",
                    item.id
                );
            }
            let sends = [Action::Click, Action::Increment, Action::Decrement]
                .into_iter()
                .any(|action| !messages(item, action, None).is_empty())
                || !messages(item, Action::SetValue, Some(&ActionData::NumericValue(0.5))).is_empty()
                || !messages(item, Action::SetValue, Some(&ActionData::Value("1".into()))).is_empty();
            if item.click.is_some() || item.set_number.is_some() || item.set_text.is_some() {
                assert!(sends || !item.enabled, "{name}: {} sends something", item.id);
                acting += 1;
            }
        }
        assert!(
            acting > 0 || matches!(name, "several" | "port"),
            "{name}: something acts"
        );
    };

    // An unknown module.
    let (mut app, filter, _) = app_with_filter();
    let missing = ModuleId(100);
    app.session
        .controller
        .add_unknown_with_id(
            missing,
            loom_core::UnknownModule {
                kind: "future.effect@1".into(),
                params: std::collections::BTreeMap::from([("amount".into(), 0.5)]),
                ranges: std::collections::BTreeMap::new(),
                inputs: vec!["in".into()],
                outputs: vec!["out".into()],
                parameters: vec!["amount".into()],
            },
        )
        .unwrap();
    app.commit();
    let mut ui = crate::ui_test::Ui::with_app(app);
    ui.send(Message::Canvas(CanvasEvent::Select(vec![missing.0])));
    check(&mut ui, "unknown", |m| matches!(m, Inspector::Unknown(_)));
    ui.send(Message::Canvas(CanvasEvent::Select(vec![missing.0, filter])));
    check(&mut ui, "several", |m| matches!(m, Inspector::Several(_)));

    // A mapped parameter, and the sources offered while adding a modulation.
    let mut ui = crate::ui_test::Ui::open("modulation.ron");
    let cutoff = {
        let node = ui.app.session.controller.graph().node(ModuleId(4)).unwrap();
        node.parameter_index("cutoff").unwrap()
    };
    ui.app
        .session
        .controller
        .map_midi_named(
            loom_core::MidiControl {
                channel: 0,
                controller: 74,
            },
            ModuleId(4),
            "cutoff",
        )
        .unwrap();
    ui.send(Message::Canvas(CanvasEvent::Select(vec![4])));
    check(&mut ui, "mapped", |m| matches!(m, Inspector::Module(_)));
    // Its options rows, then the Relative mode's, reached by stepping the
    // mode choice as a screen reader would.
    ui.send(Message::Learn(crate::learn::Message::MidiOptionsRows {
        module: 4,
        index: cutoff,
    }));
    check(&mut ui, "options", |m| matches!(m, Inspector::Module(_)));
    for _ in 0..2 {
        let mode = ui
            .app
            .a11y_frames()
            .panel
            .iter()
            .flat_map(super::items::Item::walk)
            .find(|item| item.id.to_string().ends_with("midi/mode"))
            .cloned()
            .expect("the mapping's mode");
        for message in messages(&mode, Action::Increment, None) {
            ui.send(message);
        }
    }
    check(&mut ui, "relative", |m| matches!(m, Inspector::Module(_)));
    assert!(
        ui.app
            .a11y_frames()
            .panel
            .iter()
            .flat_map(super::items::Item::walk)
            .any(|item| item.id.to_string().ends_with("midi/encoding")),
        "the Relative mode shows its encoding"
    );
    ui.send(Message::Modulation(crate::modulation::Message::AddModulation {
        module: 4,
        index: cutoff,
    }));
    check(&mut ui, "adding", |m| matches!(m, Inspector::Module(_)));

    // Inside a sub-patch: a module with its expose buttons, a port tile,
    // and a slot.
    let mut ui = crate::ui_test::Ui::open("nested-subtractive.ron");
    let svf = ui
        .app
        .session
        .controller
        .graph()
        .nodes()
        .find(|(_, node)| node.descriptor.id == "loom.filter.svf")
        .map(|(id, _)| id)
        .expect("a filter inside the sub-patches");
    let level = ui.app.session.hierarchy.level_of(svf);
    let svf = svf.0;
    ui.app.show_level(&level);
    let _ = ui.redraw();
    let tiles: Vec<u32> = ui.app.ui_snapshot().tiles.iter().map(|tile| tile.id).collect();
    assert!(tiles.contains(&svf), "the filter's level is shown");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![svf])));
    check(&mut ui, "inside", |m| matches!(m, Inspector::Module(_)));
    let port = tiles
        .iter()
        .copied()
        .find(|&tile| {
            ui.app.session.selection = std::collections::BTreeSet::from([tile]);
            matches!(ui.app.inspector_model(), Inspector::Port(_))
        })
        .expect("a Patch port tile inside the Voice");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![port])));
    check(&mut ui, "port", |m| matches!(m, Inspector::Port(_)));
    let _ = ui.app.send(Message::MakeSlot {
        module: svf,
        role: "loom.role.filter".to_string(),
    });
    let slot = ui
        .app
        .session
        .hierarchy
        .slot_of_part(ModuleId(svf))
        .expect("the filter is a part now")
        .0;
    ui.send(Message::Canvas(CanvasEvent::Select(vec![slot])));
    check(&mut ui, "slot", |m| matches!(m, Inspector::Slot(_)));

    // A stack with copies, members, and spreads.
    let mut ui = crate::ui_test::Ui::open("nested-subtractive.ron");
    ui.send(Message::Mode(crate::modes::Message::SetMode {
        tile: 2,
        mode: loom_patch::modes::Mode::Stack,
    }));
    ui.send(Message::Mode(crate::modes::Message::SetCopies { tile: 2, copies: 3 }));
    ui.send(Message::Canvas(CanvasEvent::Select(vec![2])));
    check(&mut ui, "stack", |m| matches!(m, Inspector::SubPatch(_)));
    // A layer, which lists its members.
    ui.send(Message::Mode(crate::modes::Message::SetMode {
        tile: 2,
        mode: loom_patch::modes::Mode::Layer,
    }));
    check(&mut ui, "layer", |m| matches!(m, Inspector::SubPatch(_)));
    assert!(
        ui.app
            .a11y_frames()
            .panel
            .iter()
            .flat_map(super::items::Item::walk)
            .any(|item| item.id.to_string().contains("/member/")),
        "the layer's members are reported"
    );
}

/// The harness with the library preference at `dir`, which holds a
/// sub-patch at `Voices/Voice.ron`, and the module browser open over it.
fn browser_ui(dir: &std::path::Path) -> crate::ui_test::Ui {
    std::fs::create_dir_all(dir.join("Voices")).unwrap();
    std::fs::write(
        dir.join("Voices/Voice.ron"),
        r#"Patch(version: 1, metadata: Metadata(name: "Voice"))"#,
    )
    .unwrap();
    let mut app = crate::tests::app();
    let mut preferences = app.rig.store.saved().clone();
    preferences.library = Some(dir.to_path_buf());
    let _ = app.rig.store.save(preferences);
    let mut ui = crate::ui_test::Ui::with_app(app);
    ui.send(Message::OpenBrowser);
    let _ = ui.redraw();
    assert!(ui.app.overlays.browser().is_some(), "the browser is open");
    ui
}

/// The node of the option for the row at `row` in the open browser.
fn browser_option(ui: &mut crate::ui_test::Ui, row: usize) -> NodeId {
    let id = ui.app.overlays.browser().unwrap().model().rows[row].id.clone();
    node_of(&mut ui.app, &id)
}

/// The module browser in the tree: a modal dialog whose search field has
/// the focus, with the highlighted row as its active descendant and as the
/// list box's selected option. Up and Down move the selection and leave
/// the focus in the field. Every part that acts is where the window drew
/// it.
#[test]
fn the_module_browser_is_reported_with_the_focus_in_its_field() {
    let dir = crate::recovery::tests::TempDir::new("a11y-browser");
    let mut ui = browser_ui(&dir.0);
    let places = ui.places();
    let update = ui.app.a11y_tree(&places);
    let dialog = node(&update, node_of(&mut ui.app, &crate::browser::browser_id()));
    assert_eq!(dialog.role(), Role::Dialog);
    assert!(dialog.is_modal());
    let field = node_of(&mut ui.app, &crate::browser::search_id());
    assert_eq!(update.focus, field, "the search field has the focus");
    let first = browser_option(&mut ui, 0);
    assert_eq!(node(&update, field).active_descendant(), Some(first));
    assert_eq!(node(&update, first).is_selected(), Some(true));
    assert_eq!(node(&update, first).role(), Role::ListBoxOption);
    let list = node(&update, node_of(&mut ui.app, &crate::browser::list_id()));
    assert_eq!(list.role(), Role::ListBox);
    assert_eq!(
        list.size_of_set(),
        Some(ui.app.overlays.browser().unwrap().model().rows.len())
    );
    assert!(list.clips_children());

    let (overlay, _) = ui.app.overlay_items();
    for item in overlay[0].walk() {
        let acts = item.click.is_some() || item.focus.is_some() || item.set_text.is_some();
        if acts && !matches!(item.kind, super::items::Kind::Dialog) {
            assert!(places.of(&item.id).is_some(), "the window drew `{}`", item.id);
        }
    }

    ui.send(Message::Browser(crate::browser::Message::Key(
        crate::browser::Nav::Down,
    )));
    let _ = ui.redraw();
    let places = ui.places();
    let update = ui.app.a11y_tree(&places);
    let second = browser_option(&mut ui, 1);
    assert_eq!(update.focus, field, "the focus stays in the field");
    assert_eq!(node(&update, field).active_descendant(), Some(second));
    assert_eq!(node(&update, second).is_selected(), Some(true));
    assert_eq!(node(&update, first).is_selected(), Some(false));
}

/// A screen reader's Click on a module's option adds it, as a press does.
#[test]
fn a_click_on_a_browser_option_adds_its_module() {
    let dir = crate::recovery::tests::TempDir::new("a11y-browser-click");
    let mut ui = browser_ui(&dir.0);
    let before = ui.app.ui_snapshot().tiles.len();
    let _ = ui.places();
    let option = browser_option(&mut ui, 0);
    ui_act(
        &mut ui,
        &request(Action::Click, option, None),
        std::time::Instant::now(),
    );
    assert!(ui.app.overlays.browser().is_none(), "the browser closed");
    assert_eq!(ui.app.ui_snapshot().tiles.len(), before + 1);
}

/// ShowContextMenu on a library row opens its menu as a menu, with the
/// focus on Insert linked; a Click there inserts the sub-patch linked. A
/// module's option offers no menu.
#[test]
fn show_context_menu_on_a_library_row_opens_its_menu() {
    let dir = crate::recovery::tests::TempDir::new("a11y-browser-menu");
    let mut ui = browser_ui(&dir.0);
    let row = ui
        .app
        .overlays
        .browser()
        .unwrap()
        .model()
        .rows
        .iter()
        .position(|row| row.library)
        .expect("the library row");
    let places = ui.places();
    let update = ui.app.a11y_tree(&places);
    let module = browser_option(&mut ui, 0);
    assert!(!node(&update, module).supports_action(Action::ShowContextMenu));
    let option = browser_option(&mut ui, row);
    assert!(node(&update, option).supports_action(Action::ShowContextMenu));
    ui_act(
        &mut ui,
        &request(Action::ShowContextMenu, option, None),
        std::time::Instant::now(),
    );
    let _ = ui.redraw();
    let places = ui.places();
    let update = ui.app.a11y_tree(&places);
    let id = ui.app.overlays.browser().unwrap().model().rows[row].id.clone();
    let item = node_of(&mut ui.app, &crate::browser::insert_linked_id(&id));
    assert_eq!(update.focus, item, "the focus is on the menu's item");
    assert_eq!(node(&update, item).role(), Role::MenuItem);
    assert!(places.of(&crate::browser::insert_linked_id(&id)).is_some());

    let before = ui.app.ui_snapshot().tiles.len();
    ui_act(&mut ui, &request(Action::Click, item, None), std::time::Instant::now());
    assert_eq!(ui.app.ui_snapshot().tiles.len(), before + 1, "inserted");
    assert!(ui.app.overlays.browser().is_none());
}

/// Help opened over the browser owns the input, so the browser is not
/// reported, and its options do not act.
#[test]
fn help_over_the_browser_hides_it() {
    let dir = crate::recovery::tests::TempDir::new("a11y-browser-help");
    let mut ui = browser_ui(&dir.0);
    let _ = ui.places();
    let option = browser_option(&mut ui, 0);
    ui.send(Message::ShowHelp);
    let _ = ui.redraw();
    assert!(ui.app.overlays.help.is_some());
    let places = ui.places();
    let update = ui.app.a11y_tree(&places);
    assert!(update.nodes.iter().all(|(id, _)| *id != option), "not reported");
    let tiles = ui.app.ui_snapshot().tiles.len();
    ui_act(
        &mut ui,
        &request(Action::Click, option, None),
        std::time::Instant::now(),
    );
    assert_eq!(ui.app.ui_snapshot().tiles.len(), tiles, "inert");
}

/// Your patches in the tree, as its rows stream in: after each batch the
/// focus is still the search field, with no new focus to announce, the
/// selected row keeps its node, and the list's size is the rows shown.
#[test]
fn your_patches_follows_each_batch_without_moving_the_focus() {
    let library = crate::recovery::tests::TempDir::new("a11y-patches-library");
    let source = crate::recovery::tests::TempDir::new("a11y-patches-rows");
    for i in 0..9 {
        std::fs::write(
            source.0.join(format!("{i}.ron")),
            format!(
                r#"Patch(version: 1, metadata: Metadata(name: "Patch {i}", tags: ["t{}"]))"#,
                i % 3
            ),
        )
        .unwrap();
    }
    let mut app = crate::tests::app();
    let mut preferences = app.rig.store.saved().clone();
    preferences.library = Some(library.0.clone());
    let _ = app.rig.store.save(preferences);
    let mut ui = crate::ui_test::Ui::with_app(app);
    // Run so the focus the opening asks for is reported back, as the
    // window would report it.
    let _ = ui.deliver(Message::File(crate::FileAction::Browse));
    let _ = ui.redraw();
    let open = ui.app.patches_open;
    let field = node_of(&mut ui.app, &crate::patches::part_id("search"));
    let places = ui.places();
    assert_eq!(ui.app.a11y_tree(&places).focus, field);

    let paths = crate::patches::patch_files(&source.0, &std::sync::atomic::AtomicBool::new(false));
    let mut selected: Option<(UiId, NodeId)> = None;
    for batch in paths.chunks(3) {
        let rows = batch.iter().map(|path| crate::patches::Row::read(path)).collect();
        ui.send(Message::Patches(crate::patches::Message::Found { open, rows }));
        let _ = ui.redraw();
        let places = ui.places();
        let update = ui.app.a11y_tree(&places);
        assert_eq!(update.focus, field, "the focus stays in the field");
        let model = ui.app.overlays.patches().unwrap().model().clone();
        let list = node(&update, node_of(&mut ui.app, &crate::patches::part_id("list")));
        assert_eq!(list.size_of_set(), Some(model.rows.len()));
        let highlighted = model.rows[model.highlight.unwrap()].id.clone();
        let number = node_of(&mut ui.app, &highlighted);
        assert_eq!(node(&update, number).is_selected(), Some(true));
        if let Some((id, before)) = &selected {
            assert_eq!(id, &highlighted, "the highlight stays on its patch");
            assert_eq!(*before, number, "and keeps its node");
        }
        selected = Some((highlighted, number));
    }
    // Every part that acts is where the window drew it.
    let places = ui.places();
    let (overlay, _) = ui.app.overlay_items();
    for item in overlay[0].walk() {
        let acts = item.click.is_some() || item.focus.is_some() || item.set_text.is_some();
        if acts && item.kind != super::items::Kind::Dialog {
            assert!(places.of(&item.id).is_some(), "the window drew `{}`", item.id);
        }
    }
}

/// Each view and overlay for task 6's sweep: every state of
/// [`with_each_panel`], the chooser over Preferences, and the right
/// panel's two tabs with a tile selected.
fn with_every_view() -> Vec<(String, crate::ui_test::Ui)> {
    let mut all: Vec<(String, crate::ui_test::Ui)> = with_each_panel()
        .into_iter()
        .map(|(owner, ui)| (format!("{owner:?}"), ui))
        .collect();
    let (mut app, module, _) = app_with_filter();
    let _ = app.send(Message::Preferences(crate::settings::Message::Open));
    let _ = app.send(Message::Preferences(crate::settings::Message::ChooseLicence));
    let mut ui = crate::ui_test::Ui::with_app(app);
    let _ = ui.redraw();
    all.push(("chooser over Preferences".to_string(), ui));
    for tab in [crate::matrix::Tab::Inspector, crate::matrix::Tab::Matrix] {
        let (mut app, _, _) = app_with_filter();
        let _ = app.send(Message::Canvas(CanvasEvent::Select(vec![module])));
        app.right_tab = tab;
        let mut ui = crate::ui_test::Ui::with_app(app);
        let _ = ui.redraw();
        all.push((format!("{tab:?} tab, a tile selected"), ui));
    }
    all
}

/// Task 6, in every view and overlay: every node that offers an action has
/// a role, a name, and a place, and every node with bounds lies inside the
/// window.
/// A part scrolled out of view keeps its real bounds, under a node that
/// cuts it off (section 7, rule 7), so what is checked is the part that
/// shows: its bounds cut by each such node above it.
#[test]
fn every_view_names_its_acting_parts_and_keeps_them_in_the_window() {
    let window = accesskit::Rect {
        x0: 0.0,
        y0: 0.0,
        x1: f64::from(crate::ui_test::WINDOW.width),
        y1: f64::from(crate::ui_test::WINDOW.height),
    };
    let cut = |a: accesskit::Rect, b: accesskit::Rect| accesskit::Rect {
        x0: a.x0.max(b.x0),
        y0: a.y0.max(b.y0),
        x1: a.x1.min(b.x1),
        y1: a.y1.min(b.y1),
    };
    for (name, mut ui) in with_every_view() {
        let places = ui.places();
        let update = ui.app.a11y_tree(&places);
        let parent: std::collections::HashMap<NodeId, NodeId> = update
            .nodes
            .iter()
            .flat_map(|(id, node)| node.children().iter().map(move |child| (*child, *id)))
            .collect();
        let of = |id: NodeId| update.nodes.iter().find(|(n, _)| *n == id).map(|(_, node)| node);
        for (id, node) in &update.nodes {
            let acts = [
                Action::Click,
                Action::Increment,
                Action::Decrement,
                Action::SetValue,
                Action::ShowContextMenu,
            ]
            .into_iter()
            .any(|action| node.supports_action(action));
            if acts && !node.is_hidden() {
                assert_ne!(node.role(), Role::Unknown, "{name}: node {id:?}");
                // A link in help's text is drawn inside one widget, so it
                // has no place of its own (a phase 1 gap).
                assert!(
                    node.bounds().is_some() || node.role() == Role::Link,
                    "{name}: {:?} {:?} has no place in the window",
                    ui.app.a11y.ids.id(id.0),
                    node.role()
                );
                assert!(
                    node.label().is_some_and(|label| !label.is_empty()),
                    "{name}: {:?} {:?} has a name",
                    ui.app.a11y.ids.id(id.0),
                    node.role()
                );
            }
            if let Some(mut bounds) = node.bounds()
                && !node.is_hidden()
            {
                let mut above = parent.get(id).copied();
                while let Some(up) = above {
                    if let Some(clip) = of(up).filter(|n| n.clips_children()).and_then(|n| n.bounds()) {
                        bounds = cut(bounds, clip);
                    }
                    above = parent.get(&up).copied();
                }
                if bounds.x0 >= bounds.x1 || bounds.y0 >= bounds.y1 {
                    continue; // scrolled out of view
                }
                let inside = bounds.x0 >= window.x0 - 0.5
                    && bounds.y0 >= window.y0 - 0.5
                    && bounds.x1 <= window.x1 + 0.5
                    && bounds.y1 <= window.y1 + 0.5;
                assert!(
                    inside,
                    "{name}: {:?} at {bounds:?} is outside the window",
                    ui.app.a11y.ids.id(id.0)
                );
            }
        }
    }
}

/// Task 6, in every view and overlay: a build with nothing changed sends
/// nothing, after the first one sent the whole tree.
#[test]
fn every_view_sends_nothing_when_nothing_changed() {
    for (name, mut ui) in with_every_view() {
        ui.app.a11y_event(Event::Activated);
        let places = ui.places();
        let (_, first) = ui.app.a11y_changes(&places);
        assert!(first.is_some(), "{name}: the first build is sent");
        let (_, again) = ui.app.a11y_changes(&places);
        assert!(again.is_none(), "{name}: nothing changed, so nothing is sent");
    }
}

/// The licence chooser over Preferences is reported alone: Preferences is
/// hidden, and its parts do nothing. After Use, Preferences is reported
/// again with the new default, and has the focus.
#[test]
fn the_chooser_over_preferences_hides_it_until_it_closes() {
    use crate::licence_chooser::access::licence_id;
    use crate::settings::access::preferences_id;
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    let _ = ui.app.send(Message::Preferences(crate::settings::Message::Open));
    let _ = ui.redraw();
    let frames = ui.app.a11y_frames();
    assert_eq!(frames.overlay[0].id, preferences_id());
    let places = ui.places();
    let _ = ui.app.a11y_tree(&places);
    let choose = node_of(&mut ui.app, &preferences_id().with("licence").with("choose"));
    ui.send(Message::A11y(Event::Action(request(Action::Click, choose, None))));
    let _ = ui.redraw();

    let frames = ui.app.a11y_frames();
    assert_eq!(frames.overlay.len(), 1);
    assert_eq!(frames.overlay[0].id, licence_id(), "only the chooser");
    let places = ui.places();
    let update = ui.app.a11y_tree(&places);
    assert_eq!(ui.app.a11y.ids.id(update.focus.0), Some(&licence_id()));
    let activity = node_of(&mut ui.app, &preferences_id().with("activity"));
    let before = ui.app.rig.store.saved().show_activity;
    ui.send(Message::A11y(Event::Action(request(Action::Click, activity, None))));
    assert_eq!(ui.app.rig.store.saved().show_activity, before, "Preferences is inert");

    for part in [licence_id().with("changes").with("give-up"), licence_id().with("use")] {
        let places = ui.places();
        let _ = ui.app.a11y_tree(&places);
        let target = node_of(&mut ui.app, &part);
        ui.send(Message::A11y(Event::Action(request(Action::Click, target, None))));
        let _ = ui.redraw();
    }
    assert!(ui.app.overlays.chooser.is_none(), "Use closed it");
    assert!(ui.app.rig.store.saved().licence.is_some(), "the default is set");
    let frames = ui.app.a11y_frames();
    assert_eq!(frames.overlay[0].id, preferences_id(), "Preferences is back");
    let places = ui.places();
    let update = ui.app.a11y_tree(&places);
    assert_eq!(
        ui.app.a11y.ids.id(update.focus.0),
        Some(&preferences_id()),
        "focus is on the window, since Choose is a button and takes none"
    );
    let licence = node(&update, node_of(&mut ui.app, &preferences_id().with("licence")));
    assert_ne!(licence.value(), Some("No default"));
}

/// The chooser opened from the patch's details sets the patch's licence
/// and marks the patch unsaved. Patch details are not undo steps
/// (patch-metadata.md), and a default licence is a preference, so neither
/// adds one.
#[test]
fn a_licence_from_the_chooser_adds_no_undo_step() {
    use crate::licence_chooser::access::licence_id;
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    let before = ui.app.session.document.info().license.clone();
    let steps = ui.app.session.edits.history.len();
    let _ = ui
        .app
        .send(Message::PatchInfo(crate::patch_info::Message::ChooseLicence));
    let _ = ui.redraw();
    for part in [
        licence_id().with("changes").with("no"),
        licence_id().with("commercial").with("yes"),
        licence_id().with("use"),
    ] {
        let places = ui.places();
        let _ = ui.app.a11y_tree(&places);
        let target = node_of(&mut ui.app, &part);
        ui.send(Message::A11y(Event::Action(request(Action::Click, target, None))));
    }
    assert_ne!(ui.app.session.document.info().license, before, "the licence is set");
    assert!(ui.app.session.document.info_unsaved());
    assert_eq!(ui.app.session.edits.history.len(), steps, "no undo step");

    let _ = ui.app.send(Message::Preferences(crate::settings::Message::Open));
    let _ = ui
        .app
        .send(Message::Preferences(crate::settings::Message::ChooseLicence));
    let _ = ui.app.send(Message::Licence(crate::licence_chooser::Message::Answer(
        crate::licence_chooser::Answer::GiveUp,
    )));
    let _ = ui.app.send(Message::Licence(crate::licence_chooser::Message::Use));
    assert!(ui.app.rig.store.saved().licence.is_some());
    assert_eq!(
        ui.app.session.edits.history.len(),
        steps,
        "a preference is not an undo step"
    );
}

/// Task 6's round trip: a screen reader's action on an overlay that edits
/// the patch goes through `App::update` as one undo step, which one Undo
/// takes back. Here, a Click on the module browser's first option.
#[test]
fn an_overlay_edit_is_one_undo_step() {
    let dir = crate::recovery::tests::TempDir::new("a11y-browser-undo");
    let mut ui = browser_ui(&dir.0);
    let before = ui.app.ui_snapshot().tiles.len();
    let steps = ui.app.session.edits.history.len();
    let _ = ui.places();
    let option = browser_option(&mut ui, 0);
    ui_act(
        &mut ui,
        &request(Action::Click, option, None),
        std::time::Instant::now(),
    );
    assert_eq!(ui.app.ui_snapshot().tiles.len(), before + 1);
    assert_eq!(ui.app.session.edits.history.len(), steps + 1, "one step");
    let _ = ui.app.send(Message::Undo);
    assert_eq!(ui.app.ui_snapshot().tiles.len(), before, "one Undo takes it back");
}

/// Task 6 in Preferences: one change sends the nodes it changed and no
/// others. Turning activity on changes its check box, and the status line
/// says so.
#[test]
fn one_change_in_preferences_sends_only_what_changed() {
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    let _ = ui.app.send(Message::Preferences(crate::settings::Message::Open));
    let _ = ui.redraw();
    ui.app.a11y_event(Event::Activated);
    let places = ui.places();
    let _ = ui.app.a11y_changes(&places);
    let on = ui.app.rig.store.saved().show_activity;
    let _ = ui
        .app
        .send(Message::Preferences(crate::settings::Message::Activity(!on)));
    let _ = ui.redraw();
    let places = ui.places();
    let (_, changes) = ui.app.a11y_changes(&places);
    let changes = changes.expect("the check box changed");
    let activity = node_of(&mut ui.app, &crate::settings::access::preferences_id().with("activity"));
    let status = node_of(&mut ui.app, &UiId::panel("status"));
    let mut sent = sent_ids(&changes);
    sent.sort_by_key(|id| id.0);
    let mut expected = vec![activity, status];
    expected.sort_by_key(|id| id.0);
    assert_eq!(
        sent, expected,
        "the check box, and the status line that says what changed"
    );
}

/// The harness answers a window question after what the same update
/// posted, as the bridge does: Enter on a menu item, with a screen reader
/// listening, posts the item's message and asks where the parts are; the
/// item's message runs first, then the tree reads the window.
#[test]
fn a_posted_message_runs_before_the_window_answers() {
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    let area = Rect::new(Point::new(0.0, 0.0), Size::new(800.0, 600.0));
    ui.send(Message::Canvas(CanvasEvent::TileMenu {
        module: 4,
        at: Point::new(100.0, 100.0),
        area,
        by_key: true,
    }));
    ui.send(Message::A11y(Event::Activated));
    ui.app.overlays.set_menu_highlight(Some(0));
    let item = ui.app.menu_model().expect("the tile menu is open").items[0]
        .message
        .clone();
    let log = ui.deliver(Message::Key(iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
        modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
        physical_key: iced::keyboard::key::Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::empty(),
        text: None,
        repeat: false,
    }));
    let item = format!("{item:?}");
    let posted = log.iter().position(|message| format!("{message:?}") == item);
    let located = log
        .iter()
        .position(|message| matches!(message, Message::A11y(Event::Located(_))));
    assert!(
        matches!((posted, located), (Some(posted), Some(located)) if posted < located),
        "the item's message, then the answer: {posted:?}, {located:?}"
    );
}
