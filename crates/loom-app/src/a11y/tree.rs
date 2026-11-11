//! Builds the accessibility tree from the view models.
//!
//! No platform code: this builds and is tested on every OS. The tree comes
//! from the same plain data the window draws (`UiSnapshot`, ADR-0021), so it
//! cannot say something the screen does not show (ui-layer.md, section 7,
//! rule 1).
//!
//! What it covers so far is what the spike covered: the status line, each
//! tile, and each tile's panel widgets. The toolbar, the inspector, and the
//! overlays come with the later slices of plan step 1.4.

use crate::snapshot::{Rect, UiSnapshot, Widget};
use accesskit::{Action, Node, NodeId, Role, Toggled, TreeId, TreeInfo, TreeUpdate};
use loom_canvas::CanvasView;
use loom_ui_core::{IdKey, IdTable, UiId};
use std::collections::BTreeMap;

/// How far one Increment or Decrement moves a continuous control, as a
/// share of its travel. Kept from the spike; the step of an arrow key
/// replaces it in the next slice (ui-layer.md, section 7, rule 5).
pub(super) const STEP: f32 = 0.05;

/// The UI ID of the window, the root of the tree.
pub(crate) fn window_id() -> UiId {
    UiId::panel("window")
}

/// The UI ID of the status line.
pub(crate) fn status_id() -> UiId {
    UiId::panel("status")
}

/// The UI ID of the canvas. Every tile's ID is inside it.
pub(crate) fn canvas_id() -> UiId {
    UiId::panel("canvas")
}

/// The UI ID of a tile: the canvas, then the tile's flat module ID
/// (ui-layer.md, section 4).
pub(crate) fn tile_id(module: u32) -> UiId {
    canvas_id().with(module)
}

/// The module of the tile `id` names, if it names a tile and nothing
/// inside one.
pub(crate) fn tile_of(id: &UiId) -> Option<u32> {
    match id.keys() {
        [_, IdKey::Number(module)] if id.is_under(&canvas_id()) => u32::try_from(*module).ok(),
        _ => None,
    }
}

/// What kind of item a panel widget is, which is the kind word in its UI
/// ID (ui-layer.md, section 4, rule 2). The app decides it from the panel's
/// description, as `targets.rs` does, never from how the widget is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WidgetKey {
    /// A module's own control, bound to a parameter.
    Param,
    /// A control on a sub-patch's surface.
    Exposed,
    /// A widget that only shows a value: a light, a meter, a scope.
    Display,
}

impl WidgetKey {
    /// The kind word, as the control channel's prefixes name it.
    fn word(self) -> &'static str {
        match self {
            Self::Param => "param",
            Self::Exposed => "exposed",
            Self::Display => "display",
        }
    }
}

/// The UI IDs of the panel widgets on each tile, in panel order.
///
/// A widget's key is its kind and the name it is bound to, not its place in
/// the panel. A widget with no name has no ID and no node.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WidgetIds(BTreeMap<u32, Vec<Option<UiId>>>);

impl WidgetIds {
    /// Adds a tile's widgets. `widgets` gives, for each widget in panel
    /// order, its kind and the name that finds it again.
    pub(crate) fn add_tile(&mut self, module: u32, widgets: impl IntoIterator<Item = (WidgetKey, Option<String>)>) {
        let mut ids: Vec<Option<UiId>> = Vec::new();
        for (kind, name) in widgets {
            let id = name.map(|name| {
                let id = tile_id(module).with(kind.word()).with(name);
                // Two widgets can show the same value, as a meter and its
                // level history do. The later ones are counted, from 2.
                let earlier = ids.iter().flatten().filter(|other| other.is_under(&id)).count();
                if earlier == 0 { id } else { id.with(earlier as u64 + 1) }
            });
            ids.push(id);
        }
        self.0.insert(module, ids);
    }

    /// The ID of one widget, if it has one.
    pub(crate) fn get(&self, module: u32, widget: usize) -> Option<&UiId> {
        self.0.get(&module)?.get(widget)?.as_ref()
    }

    /// The module and panel index of the widget with this ID.
    pub(crate) fn find(&self, id: &UiId) -> Option<(u32, usize)> {
        self.0.iter().find_map(|(module, ids)| {
            let widget = ids.iter().position(|other| other.as_ref() == Some(id))?;
            Some((*module, widget))
        })
    }
}

/// The node ID of a UI ID, from the table.
pub(super) fn node_id(table: &mut IdTable, id: &UiId) -> NodeId {
    NodeId(table.number(id))
}

/// A world rectangle as AccessKit wants it: in the window, in physical
/// pixels (ui-layer.md, section 7, rule 8).
fn physical(view: &CanvasView, world: Rect, scale: f32) -> accesskit::Rect {
    let a = view.to_window(loom_ui_core::Point::new(world.x, world.y));
    let b = view.to_window(loom_ui_core::Point::new(world.x + world.width, world.y + world.height));
    let s = f64::from(scale);
    accesskit::Rect {
        x0: f64::from(a.x) * s,
        y0: f64::from(a.y) * s,
        x1: f64::from(b.x) * s,
        y1: f64::from(b.y) * s,
    }
}

/// Builds the whole tree from the snapshot.
///
/// `ids` names the panel widgets. `view` places the canvas in the window;
/// without it nothing has bounds, rather than wrong ones. `scale` is the
/// window's scale factor. `table` gives each element its node ID, and drops
/// the entries of elements this build did not show.
pub(crate) fn tree(
    snapshot: &UiSnapshot,
    ids: &WidgetIds,
    view: Option<&CanvasView>,
    scale: f32,
    table: &mut IdTable,
) -> TreeUpdate {
    let mut nodes = Vec::new();
    let place = |node: &mut Node, world: Rect| {
        if let Some(view) = view {
            node.set_bounds(physical(view, world, scale));
        }
    };

    let status_node = node_id(table, &status_id());
    let mut status = Node::new(Role::Label);
    status.set_value(snapshot.status.clone());
    nodes.push((status_node, status));

    let mut children = vec![status_node];
    for tile in &snapshot.tiles {
        let id = node_id(table, &tile_id(tile.id));
        let mut node = Node::new(Role::Group);
        node.set_label(tile.title.clone());
        place(&mut node, tile.bounds);
        node.add_action(Action::Focus);
        // Click selects the tile's module, so the inspector follows.
        node.add_action(Action::Click);
        let mut kids = Vec::new();
        for (index, widget) in tile.widgets.iter().enumerate() {
            if widget.hidden {
                continue;
            }
            let Some(widget_id) = ids.get(tile.id, index) else {
                continue;
            };
            let child = node_id(table, widget_id);
            let mut w = widget_node(widget);
            if let Some(bounds) = widget.bounds {
                place(&mut w, bounds);
            }
            kids.push(child);
            nodes.push((child, w));
        }
        node.set_children(kids);
        children.push(id);
        nodes.push((id, node));
    }

    let root_node = node_id(table, &window_id());
    let mut root = Node::new(Role::Window);
    root.set_label(snapshot.title.clone());
    root.set_children(children);
    nodes.push((root_node, root));
    table.retain_seen();

    let mut info = TreeInfo::new(root_node);
    info.toolkit_name = Some("LoomDSP".into());
    TreeUpdate {
        nodes,
        tree: Some(info),
        tree_id: TreeId::ROOT,
        focus: root_node,
    }
}

/// The node for one panel widget: its role, name, value, and actions.
///
/// The name goes in the label. A control's value is a number, and the text
/// the screen shows for it, with its unit, goes in the description
/// (ui-layer.md, section 7, rules 3 and 4).
fn widget_node(widget: &Widget) -> Node {
    let continuous = |role| {
        let mut node = Node::new(role);
        node.set_numeric_value(f64::from(widget.position));
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(1.0);
        node.set_numeric_value_step(f64::from(STEP));
        node.add_action(Action::Increment);
        node.add_action(Action::Decrement);
        node.add_action(Action::SetValue);
        node
    };
    let mut node = match widget.kind {
        "knob" | "fader" | "number" => continuous(Role::Slider),
        "toggle" => {
            let mut node = Node::new(Role::Switch);
            node.set_toggled(if widget.position >= 0.5 {
                Toggled::True
            } else {
                Toggled::False
            });
            node.add_action(Action::Click);
            node
        }
        "button" => {
            let mut node = Node::new(Role::Button);
            node.add_action(Action::Click);
            node
        }
        "meter" => {
            let mut node = Node::new(Role::Meter);
            node.set_numeric_value(f64::from(widget.position));
            node
        }
        // Radio, segmented, select, light, readout, scope: read only until
        // the next slice gives each its role from the specification's table.
        _ => Node::new(Role::Label),
    };
    node.set_label(widget.label.clone());
    if !widget.text.is_empty() {
        node.set_description(widget.text.clone());
    }
    node
}
