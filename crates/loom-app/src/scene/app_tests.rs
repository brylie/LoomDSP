//! The far-edge growth rule as a keyboard and screen reader user meets it,
//! on a panel built here, since no built-in panel has a control past a trace
//! display's right edge (`docs/design/panels.md`, Resizable tiles).

use crate::Message;
use crate::ui_test::Ui;
use iced::keyboard;
use keyboard::key::Named;
use loom_core::{Channels, ModuleDescriptor, Panel, ParamSpec, PortDefault, PortRange, PortSpec, SignalType, Widget};

/// A knob's parameter, from 0 to 1.
const fn param(name: &'static str, label: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        label,
        description: "A knob for the layout test.",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        unit: "",
        options: loom_core::ParamOptions::DEFAULT,
    }
}

/// A scope at the top left, with a knob beside it, one that shares only
/// some of its rows, and one below it on the left.
static WIDE_SCOPE: ModuleDescriptor = ModuleDescriptor {
    id: "test.wide-scope",
    version: 1,
    name: "Wide Scope",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "A scope with controls past its right edge.",
    inputs: &[],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "Silence.",
        default: PortDefault::Value(0.0),
        range: PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        param("beside", "Beside"),
        param("lower", "Lower"),
        param("below", "Below"),
    ],
    role: loom_core::ModuleRole::Dsp(|_| Box::new(Silent)),
    panel: Panel::new(
        12,
        8,
        &[
            Widget::scope(0, 0, "out"),
            Widget::knob(9, 0, "beside"),
            Widget::knob(9, 4, "lower"),
            Widget::knob(0, 5, "below"),
        ],
    ),
    displays: &[],
    roles: &[],
};

/// Makes nothing: only its panel matters here.
struct Silent;
impl loom_core::Module for Silent {
    fn process(&mut self, _: &loom_core::ProcessContext<'_>, _: &mut loom_core::PortIo<'_>) {}
}

/// An app with one Wide Scope tile and nothing else.
fn with_wide_scope() -> (Ui, u32) {
    let mut registry = loom_core::Registry::new();
    registry.register(&WIDE_SCOPE).unwrap();
    let mut parts = loom_core::create_engine(std::sync::Arc::new(registry), loom_core::EngineConfig::default());
    let module = parts.controller.add_module(WIDE_SCOPE.id).unwrap().0;
    parts.controller.commit().unwrap();
    let mut app = crate::tests::app_with_text("Patch(version: 1, modules: [])");
    app.session.controller = parts.controller;
    app.session.positions.insert(module, (0.0, 0.0));
    app.commit();
    let mut ui = Ui::with_app(app);
    ui.redraw();
    (ui, module)
}

/// The window bounds of a control, as `ui.control_bounds` places it. The
/// window's scale is 1, so these are the tree's physical pixels too.
fn window_bounds(ui: &mut Ui, module: u32, param: &str) -> [f64; 4] {
    let world = ui.control_bounds(module, param);
    let canvas = ui.canvas();
    let top_left = canvas.to_window(world.position());
    let bottom_right = canvas.to_window(loom_ui_core::Point::new(world.x + world.width, world.y + world.height));
    [top_left.x, top_left.y, bottom_right.x, bottom_right.y].map(f64::from)
}

/// The accessible name of the Wide Scope's scope node.
const SCOPE_NODE: &str = "out";

/// The window bounds of the Wide Scope's scope, its first widget, as the
/// scene places it.
fn scope_window_bounds(ui: &mut Ui, module: u32) -> [f64; 4] {
    let world = ui
        .app()
        .session
        .scene
        .module(module)
        .and_then(|m| m.widget_bounds(0))
        .expect("the scene has the scope");
    let canvas = ui.canvas();
    let top_left = canvas.to_window(world.position());
    let bottom_right = canvas.to_window(loom_ui_core::Point::new(world.x + world.width, world.y + world.height));
    [top_left.x, top_left.y, bottom_right.x, bottom_right.y].map(f64::from)
}

/// The names Tab visits in the tile's panel: Enter on the selected tile
/// focuses the first control, then Tab steps to each of the others.
fn tab_sequence(ui: &mut Ui, module: u32) -> Vec<String> {
    ui.app_mut().session.selection = [module].into();
    ui.redraw();
    let _ = ui.tap(keyboard::Key::Named(Named::Enter), keyboard::Modifiers::default());
    let mut names = Vec::new();
    for _ in 0..3 {
        let tree = ui.a11y_tree();
        names.push(tree.focused().name().unwrap_or_default().to_string());
        let _ = ui.tap(keyboard::Key::Named(Named::Tab), keyboard::Modifiers::default());
    }
    let _ = ui.tap(keyboard::Key::Named(Named::Escape), keyboard::Modifiers::default());
    names
}

/// Widened, the knobs past the scope's right edge move right. Every
/// widget's accessibility node, the scope's and each knob's, has the
/// bounds the scene gives it, and Tab visits the same controls in the same
/// order as before the resize.
#[test]
fn a_control_moved_right_keeps_its_node_and_its_place_in_tab_order() {
    let (mut ui, module) = with_wide_scope();
    ui.app_mut().a11y.set_scale(1.0);
    let before = tab_sequence(&mut ui, module);
    assert_eq!(before, ["Beside", "Lower", "Below"]);
    let x_before = ui.control_bounds(module, "beside").x;

    ui.app_mut().session.sizes.insert(module, (40, 20));
    ui.app_mut().commit();
    let _ = ui
        .app_mut()
        .send(Message::Canvas(loom_canvas::CanvasEvent::VisibleDisplays(vec![module])));
    ui.redraw();
    assert!(ui.control_bounds(module, "beside").x > x_before, "moved right");

    let tree = ui.a11y_tree();
    let tile = tree.node_by_id(&crate::a11y::tree::tile_id(module));
    let found: Vec<_> = [SCOPE_NODE, "Beside", "Lower", "Below"]
        .iter()
        .map(|name| (name.to_string(), tile.node(name).bounds()))
        .collect();
    drop(tree);
    for (name, bounds) in found {
        let bounds = bounds.expect("a shown widget has bounds");
        let expected = if name == SCOPE_NODE {
            scope_window_bounds(&mut ui, module)
        } else {
            window_bounds(&mut ui, module, &name.to_lowercase())
        };
        let got = [bounds.x0, bounds.y0, bounds.x1, bounds.y1];
        for (g, e) in got.iter().zip(expected) {
            assert!((g - e).abs() < 1e-3, "{name}: {got:?} against {expected:?}");
        }
    }
    assert_eq!(tab_sequence(&mut ui, module), before);
}
