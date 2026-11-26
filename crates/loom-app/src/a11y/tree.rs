//! Builds the accessibility tree from the view models.
//!
//! No platform code: this builds and is tested on every OS. The tree comes
//! from the same plain data the window draws (`UiSnapshot`, ADR-0021), so it
//! cannot say something the screen does not show (ui-layer.md, section 7,
//! rule 1).
//!
//! What it covers so far: the status line, each tile, and each tile's panel
//! widgets, each with the role the specification's table gives its kind.
//! The toolbar, the inspector, and the overlays come with the later slices
//! of plan step 1.4.

use super::items;
use crate::matrix::Tab;
use crate::places::Places;
use crate::snapshot::{Rect, UiSnapshot, Widget};
use accesskit::{Action, Node, NodeId, Role, Toggled, TreeId, TreeInfo, TreeUpdate};
use loom_canvas::{CanvasView, ControlPart, PortRef, Reading, WidgetViewKind, arrow_share, option_bounds, part_bounds};
use loom_core::ParamSpec;
use loom_ui_core::{IdKey, IdTable, UiId};
use std::collections::BTreeMap;

/// The UI ID of the window, the root of the tree.
pub(crate) fn window_id() -> UiId {
    UiId::panel("window")
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

/// What fills a slot: the key of a slot's part in a UI ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotPart<'a> {
    /// A module, by its module type ID.
    Module(&'a str),
    /// A sub-patch, by its definition's key in the patch.
    SubPatch(&'a str),
}

/// The ID the controls of a slot tile sit in: the slot's flat ID, then its
/// part (ui-layer.md, section 4). When the slot swaps its part, the new
/// part's controls are new elements, even where a name repeats, so a screen
/// reader does not read the new filter's cutoff as the old one's.
pub(crate) fn part_id(slot: u32, part: SlotPart<'_>) -> UiId {
    match part {
        SlotPart::Module(kind) => tile_id(slot).with(kind),
        // Two keys, so a sub-patch named like a module type is not it.
        SlotPart::SubPatch(key) => tile_id(slot).with("subpatch").with(key),
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
#[derive(Debug, Clone, Default)]
pub(crate) struct WidgetIds {
    /// Each tile's widget IDs, in panel order.
    ids: BTreeMap<u32, Vec<Option<UiId>>>,
    /// What each widget is, by tile and panel index.
    details: BTreeMap<(u32, usize), Detail>,
    /// The ports, by their stable identity.
    pub(crate) ports: BTreeMap<(u32, bool, usize), PortNode>,
    /// The neighbours of each tile in the level.
    pub(crate) neighbours: BTreeMap<u32, String>,
    /// Bounds of the map image, when it is visible.
    pub(crate) map: Option<loom_ui_core::Rect>,
    /// The canvas port receiving follow commands.
    pub(crate) focused_port: Option<PortRef>,
    /// The widgets that only show a value, which can move on its own.
    live: std::collections::BTreeSet<UiId>,
    /// The control with keyboard focus on the canvas, if one has it.
    focus: Option<(u32, usize)>,
    /// True while a panel or a menu lies over the window, so a pointer
    /// press reaches nothing below it.
    covered: bool,
}

/// The graph description and bounds of an input or output port.
#[derive(Debug, Clone)]
pub(crate) struct PortNode {
    pub(crate) id: UiId,
    pub(crate) label: String,
    pub(crate) description: String,
    pub(crate) centre: loom_ui_core::Point,
    pub(crate) controls: Vec<PortRef>,
}

/// What a panel widget is, beyond what the snapshot says of it: enough to
/// give it a role, a name, and a value in its parameter's own range. It
/// comes from the scene the canvas draws and from the parameter the control
/// moves, so the tree and the screen have one source (ui-layer.md, section
/// 7, rule 1).
#[derive(Debug, Clone)]
pub(crate) struct Detail {
    /// The widget as the canvas draws it, with its options and steps.
    pub kind: WidgetViewKind,
    /// Its name: the caption, or the name it is bound to when the panel
    /// gives it no caption, as for most meters and scopes.
    pub name: String,
    /// The parameter it moves, as the control maps it. `None` for a widget
    /// that only shows a value.
    pub main: Option<ParamSpec>,
    /// The second parameter of an XY pad or a range.
    pub second: Option<ParamSpec>,
    /// A meter's number, from the code that wrote its text. `None` when
    /// the text names no single number, and for other widgets.
    pub reading: Option<Reading>,
}

/// Which node of a widget an ID names. Most widgets are one node. An XY
/// pad and a range hold one slider per value, and a radio or segmented
/// control holds one radio button per option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Piece {
    /// The widget's own node.
    Whole,
    /// The slider of the first value: `x`, or `low`.
    Main,
    /// The slider of the second value: `y`, or `high`.
    Second,
    /// The radio button of the option with this index.
    Option(usize),
}

/// The keys a piece adds to its widget's ID. They are names, so they never
/// collide with the number that counts a second widget on the same value.
fn piece_keys(kind: &WidgetViewKind, piece: Piece) -> Option<(&'static str, Option<u64>)> {
    let range = matches!(kind, WidgetViewKind::Range { .. });
    match piece {
        Piece::Whole => None,
        Piece::Main => Some((if range { "low" } else { "x" }, None)),
        Piece::Second => Some((if range { "high" } else { "y" }, None)),
        Piece::Option(index) => Some(("option", Some(index as u64))),
    }
}

/// The ID of one piece of a widget.
pub(crate) fn piece_id(widget: &UiId, kind: &WidgetViewKind, piece: Piece) -> UiId {
    match piece_keys(kind, piece) {
        None => widget.clone(),
        Some((word, None)) => widget.clone().with(word),
        Some((word, Some(index))) => widget.clone().with(word).with(index),
    }
}

/// The pieces a widget of this kind has inside its own node, in order.
fn pieces(kind: &WidgetViewKind) -> Vec<Piece> {
    match kind {
        WidgetViewKind::XyPad { .. } | WidgetViewKind::Range { .. } => vec![Piece::Main, Piece::Second],
        WidgetViewKind::Radio { options } | WidgetViewKind::Segmented { options, .. } => {
            (0..options.len()).map(Piece::Option).collect()
        }
        _ => Vec::new(),
    }
}

impl WidgetIds {
    /// Adds a tile's widgets. `under` is the ID they sit in: the tile's, or
    /// for a slot the tile's and then its part ([`part_id`]). `widgets`
    /// gives, for each widget in panel order, its kind and the name that
    /// finds it again.
    pub(crate) fn add_tile(
        &mut self,
        module: u32,
        under: &UiId,
        widgets: impl IntoIterator<Item = (WidgetKey, Option<String>)>,
    ) {
        let mut ids: Vec<Option<UiId>> = Vec::new();
        for (kind, name) in widgets {
            let id = name.map(|name| {
                let id = under.clone().with(kind.word()).with(name);
                // Two widgets can show the same value, as a meter and its
                // level history do. The later ones are counted, from 2.
                let earlier = ids.iter().flatten().filter(|other| other.is_under(&id)).count();
                let id = if earlier == 0 { id } else { id.with(earlier as u64 + 1) };
                if kind == WidgetKey::Display {
                    self.live.insert(id.clone());
                }
                id
            });
            ids.push(id);
        }
        self.ids.insert(module, ids);
    }

    /// The ID of one widget, if it has one.
    pub(crate) fn get(&self, module: u32, widget: usize) -> Option<&UiId> {
        self.ids.get(&module)?.get(widget)?.as_ref()
    }

    /// True for a widget that shows a live value: a meter, a light, a
    /// readout, a scope. Its node is sent at most every 250 ms
    /// (ui-layer.md, section 7, rule 7).
    pub(crate) fn is_live(&self, id: &UiId) -> bool {
        self.live.contains(id)
    }

    /// Says that a panel over the canvas owns the input. The tiles are
    /// then hidden from a screen reader, as they are from the pointer and
    /// the keys (ui-layer.md, section 5, rule 3).
    pub(crate) fn set_covered(&mut self) {
        self.covered = true;
    }

    /// True while a panel over the canvas owns the input.
    pub(crate) fn is_covered(&self) -> bool {
        self.covered
    }

    /// Says which control has keyboard focus on the canvas.
    pub(crate) fn set_focus(&mut self, module: u32, widget: usize) {
        self.focus = Some((module, widget));
    }

    /// Says what one widget is. Without this the widget is a plain label.
    pub(crate) fn set_detail(&mut self, module: u32, widget: usize, detail: Detail) {
        self.details.insert((module, widget), detail);
    }

    /// What one widget is, if the app said.
    pub(crate) fn detail(&self, module: u32, widget: usize) -> Option<&Detail> {
        self.details.get(&(module, widget))
    }

    /// The module and panel index of the widget with this ID.
    pub(crate) fn find(&self, id: &UiId) -> Option<(u32, usize)> {
        self.ids.iter().find_map(|(module, ids)| {
            let widget = ids.iter().position(|other| other.as_ref() == Some(id))?;
            Some((*module, widget))
        })
    }

    /// The widget an ID names, and which of its nodes: the widget itself,
    /// or a piece inside it.
    pub(crate) fn find_piece(&self, id: &UiId) -> Option<(u32, usize, Piece)> {
        if let Some((module, widget)) = self.find(id) {
            return Some((module, widget, Piece::Whole));
        }
        self.details.iter().find_map(|(&(module, widget), detail)| {
            let own = self.get(module, widget)?;
            let piece = pieces(&detail.kind)
                .into_iter()
                .find(|piece| &piece_id(own, &detail.kind, *piece) == id)?;
            Some((module, widget, piece))
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
    let logical = view.rect_to_window(loom_ui_core::Rect::new(
        loom_ui_core::Point::new(world.x, world.y),
        loom_ui_core::Size::new(world.width, world.height),
    ));
    let s = f64::from(scale);
    accesskit::Rect {
        x0: f64::from(logical.x) * s,
        y0: f64::from(logical.y) * s,
        x1: f64::from(logical.x + logical.width) * s,
        y1: f64::from(logical.y + logical.height) * s,
    }
}

/// What to do with a node that differs from what was last sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hold {
    /// Send it now.
    Send,
    /// A live value: it waits for the live timer, which must keep running.
    Wait,
    /// Off screen, or below a panel that owns the input: it keeps the
    /// value last sent, and nothing waits for it.
    Skip,
}

/// What the screen reader was last sent, so the next update holds only the
/// nodes that changed (ui-layer.md, section 7, rule 7).
#[derive(Debug, Default)]
pub(crate) struct Sent {
    /// Every node as last sent, by node ID.
    nodes: BTreeMap<u64, Node>,
    /// The node that had focus.
    focus: Option<NodeId>,
}

impl Sent {
    /// Forgets what was sent, so the next update is the whole tree: when a
    /// screen reader starts to listen, or stops.
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    /// The part of a whole tree that differs from what was last sent, and
    /// true if a change was held back. `None` when nothing is to be sent.
    ///
    /// `hold` says what to do with a node that changed. A live value
    /// waits, so a meter is sent at most as often as the caller lets live
    /// values through. A node off screen or below a panel is skipped. A
    /// node the screen reader has never had is always sent. The first
    /// update holds every node and says which is the root.
    pub(crate) fn changes(
        &mut self,
        whole: &TreeUpdate,
        hold: impl Fn(NodeId, &Node) -> Hold,
    ) -> (Option<TreeUpdate>, bool) {
        let first = self.nodes.is_empty();
        let mut held = false;
        let mut nodes = Vec::new();
        for (id, node) in &whole.nodes {
            let known = self.nodes.get(&id.0);
            if known == Some(node) {
                continue;
            }
            match known.map_or(Hold::Send, |_| hold(*id, node)) {
                Hold::Wait => held = true,
                Hold::Skip => {}
                Hold::Send => {
                    self.nodes.insert(id.0, node.clone());
                    nodes.push((*id, node.clone()));
                }
            }
        }
        // A node that is gone leaves with its parent's new list of children.
        let shown: std::collections::BTreeSet<u64> = whole.nodes.iter().map(|(id, _)| id.0).collect();
        self.nodes.retain(|id, _| shown.contains(id));
        let moved = self.focus != Some(whole.focus);
        self.focus = Some(whole.focus);
        if nodes.is_empty() && !moved {
            return (None, held);
        }
        let update = TreeUpdate {
            nodes,
            tree: if first { whole.tree.clone() } else { None },
            tree_id: whole.tree_id,
            focus: whole.focus,
        };
        (Some(update), held)
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
    frames: &Frames,
    ids: &WidgetIds,
    places: &Places,
    ours: &loom_ui::AccessNodes,
    scale: f32,
    table: &mut IdTable,
) -> TreeUpdate {
    let view = places.canvas.as_ref();
    let mut nodes = Vec::new();
    let place = |node: &mut Node, world: Rect| {
        if let Some(view) = view {
            node.set_bounds(physical(view, world, scale));
        }
    };

    // The window around the canvas, in the order it is drawn: the
    // toolbar and the panel's tabs first, the bar at the bottom last.
    let mut frame = Frame {
        places,
        scale,
        covered: ids.covered,
        field: frames.focused_field.as_ref(),
        focused: None,
        table,
        nodes: &mut nodes,
    };
    // The runtime's own panels: the toolbar, the breadcrumb inside a
    // sub-patch, and the bar, each in its named group.
    let groups = frame.ours(ours, frames);
    let mut children = groups.toolbar;
    children.extend(groups.breadcrumb);
    children.extend(groups.tabs);
    let bar = groups.bar;
    // The runtime's overlay that owns the input (row 3b): an alert dialog
    // for the question, named by its title, or a menu, modal, around the
    // panel's nodes (section 7, rule 27).
    let overlay_root = frames.ours_overlay.as_ref().map(|(_, _, id)| id.clone());
    let groups_overlay = match &frames.ours_overlay {
        Some((role, name, id)) => {
            let covered = std::mem::replace(&mut frame.covered, false);
            let dialog = frame.group(groups.overlay, *role, name, id);
            frame.covered = covered;
            if let Some((_, node)) = frame.nodes.last_mut().filter(|_| !dialog.is_empty()) {
                node.set_modal();
                if let Some(size) = frames.ours_overlay_items {
                    node.set_size_of_set(size);
                }
            }
            dialog
        }
        None => groups.overlay,
    };
    // The open panel, drawn beside the canvas: each of its parts, and the
    // panel itself marked as cutting off what it scrolls out of view.
    let mut panel: Vec<NodeId> = frames.panel.iter().map(|item| frame.panel(item)).collect();
    // The runtime's inspector, when it draws the column (plan 3.2, row 4d):
    // its scroll container is the panel's node, as Iced's content was.
    panel.extend(groups.inspector);
    // The runtime's Outline tree and jump list (plan 3.2, row 4e), each in
    // the group its pattern names, the rows flat under the tree (section
    // 7, rule 12).
    panel.extend(groups.outline);
    panel.extend(groups.jump);
    // The open tab's content, in its tab panel (the WAI-ARIA Tabs pattern).
    let panel = frame.tab_panel(panel, snapshot.matrix.tab);
    // The overlay that owns the input is not below anything: it is reported
    // and acts, while what it covers is hidden.
    let covered = std::mem::replace(&mut frame.covered, false);
    let overlay: Vec<NodeId> = frames.overlay.iter().map(|item| frame.item(item)).collect();
    frame.covered = covered;
    let field_focus = frame.focused;
    let overlay_focus = frames
        .overlay_focus
        .as_ref()
        .filter(|id| items::find(&frames.overlay, id).is_some())
        .map(|id| node_id(table, id));
    // Only a part the panel reports now can have focus.
    let item_focus = frames
        .focused_item
        .as_ref()
        .filter(|id| {
            items::find(&frames.panel, id).is_some()
                || id.is_under(&crate::outline::tree_id())
                || id.is_under(&crate::outline::jump_id())
        })
        .map(|id| node_id(table, id));
    // Focus is on the window unless a control on the canvas has it.
    let mut focus = None;
    for tile in &snapshot.tiles {
        let id = node_id(table, &tile_id(tile.id));
        let mut node = Node::new(Role::Group);
        node.set_label(tile.title.clone());
        let description = ids.neighbours.get(&tile.id).filter(|text| !text.is_empty());
        if let Some(description) = description.or(tile.description.as_ref()) {
            node.set_description(description.clone());
        }
        place(&mut node, tile.bounds);
        if ids.covered {
            // Below a panel that owns the input: not there for a screen
            // reader, with everything inside it.
            node.set_hidden();
        }
        node.add_action(Action::Focus);
        // Click selects the tile's module, so the inspector follows.
        node.add_action(Action::Click);
        // ShowContextMenu opens the tile's menu, as a right-click does.
        // AccessKit's macOS adapter (0.27.2) does not pass it on.
        node.add_action(Action::ShowContextMenu);
        let mut kids = Vec::new();
        for (index, widget) in tile.widgets.iter().enumerate() {
            // A note's text is the tile's description, read once there.
            if widget.hidden || widget.kind == "note" {
                continue;
            }
            let Some(widget_id) = ids.get(tile.id, index) else {
                continue;
            };
            let child = node_id(table, widget_id);
            if let Some(typed) = places
                .typed
                .as_ref()
                .filter(|typed| !ids.covered && (typed.module, typed.widget) == (tile.id, index))
            {
                let field_id = widget_id.clone().with("field");
                let name = ids
                    .detail(tile.id, index)
                    .map_or(widget.label.as_str(), |detail| &detail.name);
                focus = Some(typed_field(typed, &field_id, name, scale, table, &mut nodes));
                kids.extend(focus);
                continue;
            }
            // A control with nothing behind it is shown disabled, says why,
            // and offers no actions (sub-patches.md, Empty slots).
            if let Some(reason) = &widget.inert {
                // The role its kind gives a live control, so the role does
                // not change under the focus when the slot fills.
                let detail = ids.detail(tile.id, index);
                let role = detail.map_or(Role::Slider, |d| control_role(&d.kind));
                let mut w = Node::new(role);
                w.set_label(widget.label.clone());
                w.set_description(reason.clone());
                if role == Role::Slider {
                    if let Some(detail) = detail
                        && let Some(spec) = &detail.main
                    {
                        slider_value(&mut w, &detail.kind, Some(spec), widget.position);
                    }
                    if !widget.text.is_empty() {
                        w.set_description(format!("{reason}. {}", widget.text));
                    }
                } else if !widget.text.is_empty() {
                    w.set_value(widget.text.clone());
                }
                w.set_disabled();
                if let Some(bounds) = widget.bounds {
                    place(&mut w, bounds);
                }
                // Focus the canvas left on it, as when its slot was just
                // emptied, goes to the tile: a disabled node cannot hold it.
                if ids.focus == Some((tile.id, index)) && !ids.covered {
                    focus = Some(id);
                }
                kids.push(child);
                nodes.push((child, w));
                continue;
            }
            let mut w = widget_node(widget, ids.detail(tile.id, index));
            // A control can take keyboard focus, and so can each of its
            // pieces, which focus the control.
            let control = !ids.is_live(widget_id);
            if control {
                w.add_action(Action::Focus);
            }
            if ids.focus == Some((tile.id, index)) && !ids.covered {
                focus = Some(child);
            }
            if let Some(bounds) = widget.bounds {
                place(&mut w, bounds);
            }
            if let Some(detail) = ids.detail(tile.id, index) {
                let mut inside = Vec::new();
                for piece in pieces(&detail.kind) {
                    let piece_node = node_id(table, &piece_id(widget_id, &detail.kind, piece));
                    let mut p = piece_node_of(widget, detail, piece);
                    if control {
                        p.add_action(Action::Focus);
                    }
                    if let Some(bounds) = widget.bounds.and_then(|rect| piece_bounds(widget, detail, piece, rect)) {
                        place(&mut p, bounds);
                    }
                    inside.push(piece_node);
                    nodes.push((piece_node, p));
                }
                if !inside.is_empty() {
                    w.set_children(inside);
                }
            }
            kids.push(child);
            nodes.push((child, w));
        }
        for (&(_, output, index), port) in ids.ports.range((tile.id, false, 0)..=(tile.id, true, usize::MAX)) {
            let port_id = node_id(table, &port.id);
            let mut part = Node::new(Role::Button);
            part.set_label(port.label.clone());
            part.set_description(port.description.clone());
            part.add_action(Action::Focus);
            let radius = loom_canvas::PORT_HIT_RADIUS;
            place(
                &mut part,
                Rect {
                    x: port.centre.x - radius,
                    y: port.centre.y - radius,
                    width: 2.0 * radius,
                    height: 2.0 * radius,
                },
            );
            if output {
                part.set_controls(
                    port.controls
                        .iter()
                        .filter_map(|to| {
                            ids.ports
                                .get(&(to.module, to.output, to.index))
                                .map(|peer| node_id(table, &peer.id))
                        })
                        .collect::<Vec<_>>(),
                );
            }
            if !ids.covered
                && ids.focused_port
                    == Some(PortRef {
                        module: tile.id,
                        output,
                        index,
                    })
            {
                focus = Some(port_id);
            }
            kids.push(port_id);
            nodes.push((port_id, part));
        }
        node.set_children(kids);
        children.push(id);
        nodes.push((id, node));
    }

    if let Some(map) = ids.map {
        let mut image = Node::new(Role::Image);
        image.set_label("Patch map");
        image.set_description("Overview of this level. Open Outline for a text description.");
        image.set_bounds(logical(map, scale));
        if ids.covered {
            image.set_hidden();
        }
        let id = node_id(table, &canvas_id().with("patch-map"));
        nodes.push((id, image));
        children.push(id);
    }
    children.extend(panel);
    children.extend(bar);
    // Last, as it is drawn on top.
    children.extend(overlay);
    children.extend(groups_overlay);
    // A text field that has keyboard focus has it, whatever the canvas
    // last said (section 6, rule 5). Then a panel part the keys act on,
    // such as the matrix's chosen cell, since the panel hears the keys
    // before the canvas does (section 6, rule 6).
    // Within the runtime's overlay that owns the input, focus is on its
    // part that has the runtime's focus, or else on the overlay itself
    // (section 7, rule 27).
    let ours_focus = match &overlay_root {
        Some(root) => ours
            .focus
            .filter(|node| table.id(node.0).is_some_and(|id| id.is_under(root)))
            .or_else(|| Some(node_id(table, root))),
        None => ours.focus,
    };
    // While the runtime's overlay owns the input, nothing it covers has
    // focus: not a field, a panel part, or an Iced overlay below it.
    let focus = if overlay_root.is_some() {
        ours_focus
    } else {
        field_focus.or(overlay_focus).or(item_focus).or(ours_focus).or(focus)
    };
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
        focus: focus.unwrap_or(root_node),
    }
}

/// Reports the open value editor and its single-line text selection.
fn typed_field(
    typed: &loom_canvas::CanvasTyped,
    id: &UiId,
    name: &str,
    scale: f32,
    table: &mut IdTable,
    nodes: &mut Vec<(NodeId, Node)>,
) -> NodeId {
    let field_id = node_id(table, id);
    let run_id = node_id(table, &id.clone().with("text"));
    let mut field = Node::new(Role::TextInput);
    field.set_label(format!("{name} value"));
    field.set_value(typed.text.clone());
    field.add_action(Action::SetTextSelection);
    field.add_action(Action::ReplaceSelectedText);
    field.set_children([run_id]);
    let rect = typed.bounds;
    let bounds = logical(rect, scale);
    field.set_bounds(bounds);
    let caret = typed.text[..typed.caret].chars().count();
    let position = |character_index| accesskit::TextPosition {
        node: run_id,
        character_index,
    };
    field.set_text_selection(accesskit::TextSelection {
        anchor: position(typed.text[..typed.anchor].chars().count()),
        focus: position(caret),
    });
    let mut run = Node::new(Role::TextRun);
    run.set_bounds(bounds);
    run.set_value(typed.text.clone());
    run.set_character_lengths(typed.text.chars().map(|c| c.len_utf8() as u8).collect::<Vec<_>>());
    run.set_text_direction(accesskit::TextDirection::LeftToRight);
    nodes.extend([(field_id, field), (run_id, run)]);
    field_id
}

/// Scales a window rectangle to physical pixels.
fn logical(rect: loom_ui_core::Rect, scale: f32) -> accesskit::Rect {
    let scale = f64::from(scale);
    accesskit::Rect {
        x0: f64::from(rect.x) * scale,
        y0: f64::from(rect.y) * scale,
        x1: f64::from(rect.x + rect.width) * scale,
        y1: f64::from(rect.y + rect.height) * scale,
    }
}

/// What the tree reports of the parts the toolkit draws around the canvas:
/// the bar at the bottom, the open panel's parts, and the text field that
/// has keyboard focus.
pub(crate) struct Frames {
    /// The open panel's parts: the inspector's or the matrix's, by the tab
    /// that is open.
    pub panel: Vec<items::Item>,
    /// The text field with keyboard focus, as the window last said.
    pub focused_field: Option<iced::advanced::widget::Id>,
    /// The panel part the keys act on while no text field has them: the
    /// matrix's chosen cell while the matrix has the keys.
    pub focused_item: Option<UiId>,
    /// The overlay that owns the input, if one is reported: its root item,
    /// with its parts inside. Everything else is hidden below it
    /// (section 7, rules 24 and 27).
    pub overlay: Vec<items::Item>,
    /// Where the overlay's keys act while no text field has them: a menu's
    /// highlighted item, the question's highlighted answer, or else the
    /// overlay itself.
    pub overlay_focus: Option<UiId>,
    /// The runtime's overlay that owns the input, if one is open (row 3b):
    /// the role, name, and UI ID of the node around its panel's nodes, an
    /// alert dialog for the question and a menu for the probe menu or
    /// Recent.
    pub ours_overlay: Option<(Role, String, UiId)>,
    /// How many items the runtime menu in `ours_overlay` holds, for its
    /// node's set size, so an item in a column still reads as one of all.
    pub ours_overlay_items: Option<usize>,
    /// A menu the runtime draws inside that overlay, as a panel of its
    /// own: its name and UI ID, around its items. A library row's menu in
    /// the module browser (row 5).
    pub ours_menu: Option<(String, UiId)>,
}

/// The UI ID a fixed text names. The texts are this app's own constants,
/// and a test reads each one back.
pub(crate) fn fixed_id(text: &str) -> UiId {
    text.parse().unwrap_or_else(|_| UiId::panel(text))
}

/// The UI ID of the toolbar. Every button's ID is inside it.
pub(crate) use crate::toolbar::toolbar_id;

/// The UI IDs of the right panel's tabs and of each tab, which the
/// runtime draws (`tabs.rs`).
pub(crate) use crate::tabs::{tab_id, tabs_id};

/// True for a node of the stats bar that shows a live value: the load and
/// the dropouts. It is sent at most every 250 ms, as a meter is.
pub(crate) fn is_live_bar_item(id: &UiId) -> bool {
    *id == fixed_id("stats/load") || *id == fixed_id("stats/dropouts")
}

/// Builds the nodes of the window around the canvas: the toolbar, the
/// right panel's tabs, and the bar at the bottom.
struct Frame<'a> {
    /// Where the window drew each named part.
    places: &'a Places,
    /// The window's scale factor.
    scale: f32,
    /// True while a pointer press cannot reach these parts, because a
    /// panel or a menu lies over the window: the nodes are then hidden
    /// (ui-layer.md, section 5, rule 3).
    covered: bool,
    /// The text field that has keyboard focus, as the window last said.
    field: Option<&'a iced::advanced::widget::Id>,
    /// The node of that field, once it is built.
    focused: Option<NodeId>,
    table: &'a mut IdTable,
    nodes: &'a mut Vec<(NodeId, Node)>,
}

impl Frame<'_> {
    /// Adds `node` under `id`, with its place in the window if it was
    /// drawn, and gives its node ID.
    fn add(&mut self, id: &UiId, mut node: Node) -> NodeId {
        if let Some(place) = self.places.of(id) {
            let s = f64::from(self.scale);
            let r = place.rect;
            node.set_bounds(accesskit::Rect {
                x0: f64::from(r.x) * s,
                y0: f64::from(r.y) * s,
                x1: f64::from(r.x + r.width) * s,
                y1: f64::from(r.y + r.height) * s,
            });
        }
        if self.covered {
            node.set_hidden();
        }
        let node_id = node_id(self.table, id);
        self.nodes.push((node_id, node));
        node_id
    }

    /// A panel's root item and everything in it. The panel cuts off what
    /// it scrolls out of view, and says so; those parts stay in the tree
    /// with their real bounds (ui-layer.md, section 7, rule 7). The panel's
    /// own bounds are the part of it that shows: its content can be taller
    /// than the area that scrolls it, and a clipping node larger than what
    /// it shows would claim rows out of view are visible.
    fn panel(&mut self, item: &items::Item) -> NodeId {
        let id = self.item(item);
        self.clip_to_shown(id, &item.id);
        id
    }

    /// Marks node `id`, drawn as `ui`, as cutting off its children, and
    /// gives it the bounds of the part of it that shows: its content can be
    /// taller than the area that scrolls it, and a clipping node larger than
    /// what it shows would claim rows out of view are visible.
    fn clip_to_shown(&mut self, id: NodeId, ui: &UiId) {
        let shown = self.places.of(ui).and_then(|place| {
            let clip = place.clip?;
            Some(place.rect.intersection(clip).unwrap_or(loom_ui_core::Rect::new(
                loom_ui_core::Point::new(place.rect.x, place.rect.y),
                loom_ui_core::Size::new(0.0, 0.0),
            )))
        });
        let s = f64::from(self.scale);
        if let Some((_, node)) = self.nodes.iter_mut().rev().find(|(node, _)| *node == id) {
            node.set_clips_children();
            if let Some(r) = shown {
                node.set_bounds(accesskit::Rect {
                    x0: f64::from(r.x) * s,
                    y0: f64::from(r.y) * s,
                    x1: f64::from(r.x + r.width) * s,
                    y1: f64::from(r.y + r.height) * s,
                });
            }
        }
    }

    /// The node of one item, after the nodes of what it holds. Its role
    /// comes from its kind, and its actions from what it does now: a part
    /// that is disabled offers none (rule 22 for Focus).
    fn item(&mut self, item: &items::Item) -> NodeId {
        use items::Kind;
        // Each child's node, and where it was pushed: a child's own node is
        // the last one its build pushes.
        let built: Vec<(NodeId, usize)> = item
            .children
            .iter()
            .map(|child| (self.item(child), self.nodes.len() - 1))
            .collect();
        let children: Vec<NodeId> = built.iter().map(|(id, _)| *id).collect();
        let mut node = Node::new(match item.kind {
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
            Kind::ListBox => Role::ListBox,
            Kind::ListBoxOption => Role::ListBoxOption,
        });
        // A list says how many rows it has, and each row its place, so a
        // screen reader can say "3 of 7". AccessKit puts the size on the
        // list and counts places from zero.
        if matches!(item.kind, Kind::List | Kind::ListBox) {
            let rows: Vec<usize> = item
                .children
                .iter()
                .zip(&built)
                .filter(|(child, _)| matches!(child.kind, Kind::ListItem | Kind::ListBoxOption))
                .map(|(_, (_, pushed))| *pushed)
                .collect();
            node.set_size_of_set(rows.len());
            for (at, pushed) in rows.into_iter().enumerate() {
                self.nodes[pushed].1.set_position_in_set(at);
            }
        }
        if let Some(label) = &item.label {
            node.set_label(label.clone());
        }
        if let Some(value) = &item.value {
            node.set_value(value.clone());
        }
        if let Some(description) = &item.description {
            node.set_description(description.clone());
        }
        if let Some(number) = item.number {
            node.set_numeric_value(number.value);
            node.set_min_numeric_value(number.min);
            node.set_max_numeric_value(number.max);
            if let Some(step) = number.step {
                node.set_numeric_value_step(step);
            }
        }
        if let Some(on) = item.on {
            node.set_toggled(if on { Toggled::True } else { Toggled::False });
        }
        if let Some(selected) = item.selected {
            node.set_selected(selected);
        }
        if let Some(active) = &item.active {
            node.set_active_descendant(node_id(self.table, active));
        }
        if item.enabled {
            if matches!(item.kind, Kind::Button | Kind::Check | Kind::Option) {
                node.add_action(Action::Click);
            }
            // An option offers Click only when it can be picked: a file that
            // does not read is listed, and says why, but opens nothing.
            if item.kind == Kind::ListBoxOption && item.click.is_some() {
                node.add_action(Action::Click);
            }
            if item.menu.is_some() {
                node.add_action(Action::ShowContextMenu);
            }
            if matches!(item.kind, Kind::Slider | Kind::Choice) {
                node.add_action(Action::Increment);
                node.add_action(Action::Decrement);
            }
            if item.set_number.is_some() || item.set_text.is_some() {
                node.add_action(Action::SetValue);
            }
            if item.focus.is_some() {
                node.add_action(Action::Focus);
            }
        } else {
            node.set_disabled();
        }
        if !children.is_empty() {
            node.set_children(children);
        }
        let id = self.add(&item.id, node);
        // List boxes scroll, so their bounds clip descendants.
        if item.kind == Kind::ListBox {
            self.clip_to_shown(id, &item.id);
        }
        // A covered field is hidden, so it cannot be focused, even while
        // the window has not yet said its keys have moved.
        if !self.covered && item.focus.is_some() && item.focus.as_ref() == self.field {
            self.focused = Some(id);
        }
        id
    }

    /// The open tab's content, the nodes `content`, in a tab panel named by
    /// its tab, which the open tab says it controls (the WAI-ARIA Tabs
    /// pattern). Nothing when there is no content.
    fn tab_panel(&mut self, content: Vec<NodeId>, open: Tab) -> Vec<NodeId> {
        let id = crate::tabs::tabs_id().with("content");
        let panel = self.group(content, Role::TabPanel, crate::tabs::label(open), &id);
        let tab = node_id(self.table, &tab_id(open));
        if let Some((_, node)) = self.nodes.iter_mut().find(|(node, _)| *node == tab) {
            node.set_controls(panel.clone());
        }
        panel
    }

    /// The runtime's own nodes, from `loom-ui`'s layout, as the window's
    /// children: the toolbar's buttons in a group named "Toolbar", the
    /// breadcrumb's entries in one named "Patch levels", and the stats
    /// bar's items in one named "Status bar". Below a panel that owns the
    /// input they are hidden, as the window's other parts are.
    fn ours(&mut self, ours: &loom_ui::AccessNodes, frames: &Frames) -> Ours {
        // Only the overlay that owns the input and what is inside it: help
        // over the module browser leaves the browser covered (rule 24).
        let owner = frames.ours_overlay.as_ref().map(|(_, _, id)| id);
        let owns = |root: &UiId| owner.is_none_or(|owner| root == owner || root.is_under(owner));
        let tops: Vec<NodeId> = ours
            .roots
            .iter()
            .flat_map(|(_, _, tops)| tops.iter().copied())
            .collect();
        // The overlay that owns the input (the question, help, the licence
        // chooser, a menu, a browser) is reported and acts, while what it
        // covers is hidden (section 7, rule 27). An overlay under it, such
        // as the chooser under help, is drawn but not reported at all, as a
        // covered menu is not (rule 24): its nodes would otherwise sit
        // inside the top one's dialog.
        let within = |id: &UiId, root: &UiId| id == root || id.is_under(root);
        let overlay_roots: Vec<&UiId> = ours
            .roots
            .iter()
            .filter(|(root, layer, _)| *layer >= loom_ui::Layer::Overlays && owns(root))
            .map(|(root, _, _)| root)
            .collect();
        let covered_roots: Vec<&UiId> = ours
            .roots
            .iter()
            .filter(|(root, layer, _)| *layer >= loom_ui::Layer::Overlays && !overlay_roots.contains(&root))
            .map(|(root, _, _)| root)
            .collect();
        let in_roots = |table: &IdTable, node: NodeId, roots: &[&UiId]| {
            table
                .id(node.0)
                .is_some_and(|id| roots.iter().any(|root| within(id, root)))
        };
        for (id, node) in &ours.nodes {
            if in_roots(self.table, *id, &covered_roots) {
                continue;
            }
            let mut node = node.clone();
            if self.covered && !in_roots(self.table, *id, &overlay_roots) {
                node.set_hidden();
            }
            self.nodes.push((*id, node));
        }
        let mut overlay: Vec<NodeId> = ours
            .roots
            .iter()
            .filter(|(root, _, _)| overlay_roots.contains(&root))
            .flat_map(|(_, _, tops)| tops.iter().copied())
            .collect();
        // A menu drawn as a panel of its own is named around its items.
        if let Some((label, menu)) = &frames.ours_menu {
            let (items, rest): (Vec<NodeId>, Vec<NodeId>) = overlay
                .iter()
                .partition(|top| self.table.id(top.0).is_some_and(|id| id.is_under(menu)));
            overlay = rest;
            overlay.extend(self.group(items, Role::Menu, label, menu));
        }
        let under = |panel: &UiId| -> Vec<NodeId> {
            tops.iter()
                .copied()
                .filter(|top| self.table.id(top.0).is_some_and(|id| id.is_under(panel)))
                .collect()
        };
        let toolbar = under(&toolbar_id());
        let breadcrumb = under(&crate::breadcrumb::breadcrumb_id());
        let bar = under(&crate::stats_bar::stats_id());
        let inspector = under(&crate::inspector::panel::inspector_id());
        let tabs = under(&tabs_id());
        let outline = under(&crate::outline::tree_id());
        // The jump list's list box is the runtime's node.
        let jump = under(&crate::outline::jump_id());
        Ours {
            inspector,
            // The right panel's tabs, one stop with arrows inside (the
            // WAI-ARIA Tabs pattern).
            tabs: self.group(tabs, Role::TabList, "Right panel", &tabs_id()),
            outline: self.group(outline, Role::Tree, "Outline", &crate::outline::tree_id()),
            jump,
            // The toolbar is one stop with arrows inside, the WAI-ARIA
            // toolbar pattern (Brylie's decision, 2026-11-19), so its node
            // says so; the breadcrumb is a list of links, a group.
            toolbar: self.group(toolbar, Role::Toolbar, "Toolbar", &toolbar_id()),
            breadcrumb: self.group(
                breadcrumb,
                Role::Group,
                "Patch levels",
                &crate::breadcrumb::breadcrumb_id(),
            ),
            bar: self.group(bar, Role::Group, "Status bar", &crate::stats_bar::stats_id()),
            overlay,
        }
    }

    /// A group of `role` named `label`, with UI ID `id`, around
    /// `children`, which were pushed already, and their bounds; none when
    /// there are no children. A toolbar lies across the window, so it says
    /// it is horizontal, and a screen reader names Left and Right as its
    /// keys.
    fn group(&mut self, children: Vec<NodeId>, role: Role, label: &str, id: &UiId) -> Vec<NodeId> {
        if children.is_empty() {
            return Vec::new();
        }
        let bounds = self
            .nodes
            .iter()
            .filter(|(node, _)| children.contains(node))
            .filter_map(|(_, node)| node.bounds())
            .reduce(|a, b| a.union(b));
        let mut group = Node::new(role);
        group.set_label(label);
        group.set_children(children);
        if role == Role::Toolbar {
            group.set_orientation(accesskit::Orientation::Horizontal);
        }
        if let Some(bounds) = bounds {
            group.set_bounds(bounds);
        }
        if self.covered {
            group.set_hidden();
        }
        let id = node_id(self.table, id);
        self.nodes.push((id, group));
        vec![id]
    }
}

/// The runtime's own nodes, in their groups: each list is empty or holds
/// its group.
struct Ours {
    /// The toolbar's group.
    toolbar: Vec<NodeId>,
    /// The breadcrumb's group, inside a sub-patch.
    breadcrumb: Vec<NodeId>,
    /// The stats bar's group.
    bar: Vec<NodeId>,
    /// The runtime's inspector's top nodes, while it draws the column.
    inspector: Vec<NodeId>,
    /// The right panel's tab list.
    tabs: Vec<NodeId>,
    /// The Outline's tree, its rows flat under it.
    outline: Vec<NodeId>,
    /// The cable jump list's list box, while it is open.
    jump: Vec<NodeId>,
    /// The top nodes of the runtime's overlay that owns the input.
    overlay: Vec<NodeId>,
}

/// Sets a slider's value, range, and step from the parameter it moves.
///
/// The value is the parameter's own number, in its own range (ui-layer.md,
/// section 7, rule 4). A step is declared only where equal steps of the
/// control are equal steps of the value. On a logarithmic or decibel
/// parameter one arrow key press is a different amount at each place, so
/// no one number would be true, and none is given. Increment and Decrement
/// still move what an arrow key moves.
fn slider(kind: &WidgetViewKind, spec: Option<&ParamSpec>, position: f32) -> Node {
    let mut node = Node::new(Role::Slider);
    slider_value(&mut node, kind, spec, position);
    node.add_action(Action::Increment);
    node.add_action(Action::Decrement);
    node.add_action(Action::SetValue);
    node
}

/// Sets a slider's number and range without making it interactive.
fn slider_value(node: &mut Node, kind: &WidgetViewKind, spec: Option<&ParamSpec>, position: f32) {
    match spec {
        Some(spec) => {
            let number = items::Number::parameter(spec, position, arrow_share(kind));
            node.set_numeric_value(number.value);
            node.set_min_numeric_value(number.min);
            node.set_max_numeric_value(number.max);
            if let Some(step) = number.step {
                node.set_numeric_value_step(step);
            }
        }
        // No parameter to ask: the control's travel, from 0 to 1.
        None => {
            node.set_numeric_value(crate::param::decimal(position));
            node.set_min_numeric_value(0.0);
            node.set_max_numeric_value(1.0);
        }
    }
}

/// What a scope's trace shows, in words: its lowest and highest points,
/// from -1 at the bottom of the frame to 1 at the top.
fn trace_words(widget: &Widget) -> Option<String> {
    let trace = widget.trace.filter(|trace| trace.points > 0)?;
    Some(format!("Lowest {:.2}, highest {:.2}", trace.low, trace.high))
}

/// The node for one panel widget: its role, name, value, and actions, by
/// the kind of widget drawn (ui-layer.md, section 7, the role table).
///
/// The role a control of `kind` has: what [`widget_node`] gives it, and
/// what an inert control of that kind keeps while it is disabled. A kind
/// that is not a control is an image.
pub(super) fn control_role(kind: &WidgetViewKind) -> Role {
    match kind {
        WidgetViewKind::Knob { .. } | WidgetViewKind::Fader { .. } | WidgetViewKind::Number { .. } => Role::Slider,
        WidgetViewKind::Toggle => Role::Switch,
        WidgetViewKind::Button => Role::Button,
        WidgetViewKind::Radio { .. } | WidgetViewKind::Segmented { .. } => Role::RadioGroup,
        WidgetViewKind::Select { .. } => Role::ComboBox,
        WidgetViewKind::XyPad { .. } | WidgetViewKind::Range { .. } => Role::Group,
        _ => Role::Image,
    }
}

/// The name goes in the label. A control's value is a number, and the text
/// the screen shows for it, with its unit, goes in the description
/// (ui-layer.md, section 7, rules 3 and 4). Without a `detail` the widget
/// is a plain label that says its text.
fn widget_node(widget: &Widget, detail: Option<&Detail>) -> Node {
    let Some(detail) = detail else {
        let mut node = Node::new(Role::Label);
        node.set_label(widget.label.clone());
        if !widget.text.is_empty() {
            node.set_description(widget.text.clone());
        }
        return node;
    };
    let mut description = (!widget.text.is_empty()).then(|| widget.text.clone());
    let mut node = match &detail.kind {
        WidgetViewKind::Knob { .. } | WidgetViewKind::Fader { .. } | WidgetViewKind::Number { .. } => {
            slider(&detail.kind, detail.main.as_ref(), widget.position)
        }
        WidgetViewKind::Toggle => {
            let mut node = Node::new(control_role(&detail.kind));
            node.set_toggled(if widget.position >= 0.5 {
                Toggled::True
            } else {
                Toggled::False
            });
            node.add_action(Action::Click);
            node
        }
        WidgetViewKind::Button => {
            let mut node = Node::new(control_role(&detail.kind));
            node.add_action(Action::Click);
            node
        }
        // One radio button per option, inside.
        WidgetViewKind::Radio { .. } | WidgetViewKind::Segmented { .. } => Node::new(control_role(&detail.kind)),
        // It says its chosen option and steps one option at a time. It
        // does not open: a list a screen reader can walk comes with our
        // own select, in phase 3.
        WidgetViewKind::Select { .. } => {
            let mut node = Node::new(control_role(&detail.kind));
            node.set_value(widget.text.clone());
            node.add_action(Action::Increment);
            node.add_action(Action::Decrement);
            description = None;
            node
        }
        // One slider per value, inside.
        WidgetViewKind::XyPad { .. } | WidgetViewKind::Range { .. } => Node::new(control_role(&detail.kind)),
        WidgetViewKind::Meter { .. } => {
            // The number its text says, never the stored level: the
            // tree does not work the mapping out a second time. Text with
            // no single number in it is said as words only.
            if widget.clip {
                description =
                    Some(description.map_or_else(|| "clipped".to_string(), |level| format!("{level}, clipped")));
            }
            let mut node = Node::new(Role::Meter);
            if let Some(reading) = detail.reading.filter(|reading| reading.value.is_finite()) {
                node.set_numeric_value(f64::from(reading.value));
                node.set_min_numeric_value(f64::from(reading.min));
                node.set_max_numeric_value(f64::from(reading.max));
            }
            node
        }
        // A label's text is its value, and it has no label of its own:
        // an adapter gives a label node's label in place of its value
        // (`accesskit_macos` 0.27.2, `label_is_exposed_in_value`), so a
        // name set as the label would hide the reading. The value says
        // both, as a meter's text does: "Cents: 12.5 cents".
        WidgetViewKind::Readout { .. } => {
            let mut node = Node::new(Role::Label);
            node.set_value(format!("{}: {}", detail.name, widget.text));
            return node;
        }
        WidgetViewKind::Scope | WidgetViewKind::XyScope => {
            if widget.trace.is_some_and(|trace| trace.invalid) {
                let mark = loom_canvas::Trace::INVALID_LABEL;
                description = Some(description.map_or_else(|| mark.to_string(), |axes| format!("{axes}. {mark}")));
            } else if detail.kind == WidgetViewKind::Scope {
                description = trace_words(widget);
            }
            Node::new(Role::Image)
        }
        // A light, and the other drawings: named, with what the screen
        // says of them in words.
        _ => Node::new(Role::Image),
    };
    node.set_label(detail.name.clone());
    if let Some(description) = description {
        node.set_description(description);
    }
    node
}

/// The node of one piece inside a widget: a slider for one value of an XY
/// pad or a range, or a radio button for one option.
fn piece_node_of(widget: &Widget, detail: &Detail, piece: Piece) -> Node {
    match piece {
        Piece::Option(index) => {
            let mut node = Node::new(Role::RadioButton);
            let options = match &detail.kind {
                WidgetViewKind::Radio { options } | WidgetViewKind::Segmented { options, .. } => options.as_slice(),
                _ => &[],
            };
            node.set_label(options.get(index).cloned().unwrap_or_default());
            let chosen = widget.position.round() as usize == index;
            node.set_toggled(if chosen { Toggled::True } else { Toggled::False });
            node.add_action(Action::Click);
            node
        }
        Piece::Main | Piece::Whole => {
            let mut node = slider(&detail.kind, detail.main.as_ref(), widget.position);
            if let Some(spec) = &detail.main {
                node.set_label(spec.label.to_string());
                node.set_description(crate::param::format(spec, spec.denormalize(widget.position)));
            }
            node
        }
        Piece::Second => {
            let second = widget.second.as_ref();
            let position = second.map_or(0.0, |second| second.position);
            let mut node = slider(&detail.kind, detail.second.as_ref(), position);
            if let Some(spec) = &detail.second {
                node.set_label(spec.label.to_string());
            }
            if let Some(second) = second {
                node.set_description(second.text.clone());
            }
            node
        }
    }
}

/// Where one piece of a widget is drawn, in the world.
fn piece_bounds(widget: &Widget, detail: &Detail, piece: Piece, rect: Rect) -> Option<Rect> {
    let core = loom_ui_core::Rect::new(
        loom_ui_core::Point::new(rect.x, rect.y),
        loom_ui_core::Size::new(rect.width, rect.height),
    );
    let found = match piece {
        Piece::Whole => None,
        Piece::Option(index) => option_bounds(&detail.kind, core, index),
        Piece::Main => part_bounds(&detail.kind, core, widget.position, ControlPart::Main),
        Piece::Second => {
            let position = widget.second.as_ref().map_or(0.0, |second| second.position);
            part_bounds(&detail.kind, core, position, ControlPart::Second)
        }
    }?;
    Some(Rect {
        x: found.x,
        y: found.y,
        width: found.width,
        height: found.height,
    })
}
