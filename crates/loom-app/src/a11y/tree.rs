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
use crate::{bars, toolbar};
use accesskit::{Action, Node, NodeId, Role, Toggled, TreeId, TreeInfo, TreeUpdate};
use loom_canvas::{CanvasView, ControlPart, Reading, WidgetViewKind, arrow_share, option_bounds, part_bounds};
use loom_core::{ParamMapping, ParamSpec};
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
    /// The widgets that only show a value, which can move on its own.
    live: std::collections::BTreeSet<UiId>,
    /// The control with keyboard focus on the canvas, if one has it.
    focus: Option<(u32, usize)>,
    /// True while a panel or a menu lies over the window, so a pointer
    /// press reaches nothing below it.
    covered: bool,
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
    let mut children = vec![frame.toolbar(&snapshot.toolbar), frame.tabs(snapshot.matrix.tab)];
    let bar = frame.bar(&frames.bars);
    // The open panel, drawn beside the canvas: each of its parts, and the
    // panel itself marked as cutting off what it scrolls out of view.
    let panel: Vec<NodeId> = frames.panel.iter().map(|item| frame.panel(item)).collect();
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
        .filter(|id| items::find(&frames.panel, id).is_some())
        .map(|id| node_id(table, id));
    // Focus is on the window unless a control on the canvas has it.
    let mut focus = None;
    for tile in &snapshot.tiles {
        let id = node_id(table, &tile_id(tile.id));
        let mut node = Node::new(Role::Group);
        node.set_label(tile.title.clone());
        if let Some(description) = &tile.description {
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
        node.set_children(kids);
        children.push(id);
        nodes.push((id, node));
    }

    children.extend(panel);
    children.push(bar);
    // Last, as it is drawn on top.
    children.extend(overlay);
    // A text field that has keyboard focus has it, whatever the canvas
    // last said (section 6, rule 5). Then a panel part the keys act on,
    // such as the matrix's chosen cell, since the panel hears the keys
    // before the canvas does (section 6, rule 6).
    let focus = field_focus.or(overlay_focus).or(item_focus).or(focus);
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

/// What the tree reports of the parts the toolkit draws around the canvas:
/// the bar at the bottom, the open panel's parts, and the text field that
/// has keyboard focus.
pub(crate) struct Frames {
    /// The stats bar and the status line.
    pub bars: bars::Model,
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
}

/// The UI ID a fixed text names. The texts are this app's own constants,
/// and a test reads each one back.
pub(crate) fn fixed_id(text: &str) -> UiId {
    text.parse().unwrap_or_else(|_| UiId::panel(text))
}

/// The UI ID of the toolbar. Every button's ID is inside it.
pub(crate) fn toolbar_id() -> UiId {
    UiId::panel("toolbar")
}

/// The UI ID of the stats bar. Every item's ID is inside it. The status
/// line and a kept error are drawn in the bar but keep IDs of their own,
/// under `status`.
pub(crate) fn stats_id() -> UiId {
    UiId::panel("stats")
}

/// The UI ID of the right panel's tabs. Each tab's ID is inside it.
pub(crate) fn tabs_id() -> UiId {
    UiId::panel("panel")
}

/// The UI ID of one of the right panel's tabs.
pub(crate) fn tab_id(tab: Tab) -> UiId {
    tabs_id().with(match tab {
        Tab::Inspector => "inspector",
        Tab::Matrix => "matrix",
    })
}

/// True for a node of the stats bar that shows a live value: the load and
/// the dropouts. It is sent at most every 250 ms, as a meter is.
pub(crate) fn is_live_bar_item(id: &UiId) -> bool {
    *id == fixed_id("stats/load") || *id == fixed_id("stats/dropouts")
}

/// What a press, a tab, or a switch outside the canvas does, by the node's
/// UI ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FramePart<'a> {
    /// A toolbar button, by its fixed name.
    Button(&'a str),
    /// One of the right panel's tabs.
    Tab(Tab),
    /// The × of the status line, or of the kept error.
    Dismiss {
        /// True for the kept error's ×.
        kept: bool,
    },
    /// The **Measure** switch.
    Measure,
    /// The **Move camera** switch.
    MoveCamera,
}

/// What `id` names outside the canvas, if it names something that acts.
pub(crate) fn frame_part(id: &UiId) -> Option<FramePart<'_>> {
    if let [IdKey::Name(panel), IdKey::Name(name)] = id.keys() {
        match (&**panel, &**name) {
            ("toolbar", name) => return Some(FramePart::Button(name)),
            ("panel", "inspector") => return Some(FramePart::Tab(Tab::Inspector)),
            ("panel", "matrix") => return Some(FramePart::Tab(Tab::Matrix)),
            ("status", "dismiss") => return Some(FramePart::Dismiss { kept: false }),
            ("stats", "measure") => return Some(FramePart::Measure),
            ("stats", "move_camera") => return Some(FramePart::MoveCamera),
            _ => {}
        }
    }
    (*id == fixed_id(bars::KEPT_DISMISS_ID)).then_some(FramePart::Dismiss { kept: true })
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
            Kind::Dialog => Role::Dialog,
            Kind::AlertDialog => Role::AlertDialog,
            Kind::Menu => Role::Menu,
            Kind::MenuItem => Role::MenuItem,
            Kind::Link => Role::Link,
            Kind::ListBox => Role::ListBox,
            Kind::ListBoxOption => Role::ListBoxOption,
        });
        // A dialog or a menu owns the input while it is open, as the pointer
        // finds: nothing outside it can be reached (section 7, rule 27).
        if matches!(item.kind, Kind::Dialog | Kind::AlertDialog | Kind::Menu) {
            node.set_modal();
        }
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
            if matches!(
                item.kind,
                Kind::Button | Kind::Check | Kind::Option | Kind::MenuItem | Kind::Link
            ) {
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
        // A list box scrolls, so it cuts off what it scrolls out of view and
        // is as tall as what shows, as a panel is.
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

    /// The toolbar: a named group with one button per toolbar button. A
    /// button's hover text is its description, and says why while the
    /// button is disabled. Outside the canvas only text fields take
    /// keyboard focus until phase 3, so a button offers Click and no
    /// Focus (ui-layer.md, section 7, rule 22).
    fn toolbar(&mut self, model: &toolbar::Model) -> NodeId {
        let mut buttons = Vec::new();
        for button in &model.buttons {
            let mut node = Node::new(Role::Button);
            node.set_label(button.label);
            if let Some(hover) = &button.hover {
                node.set_description(hover.clone());
            }
            if let Some(on) = button.on {
                node.set_toggled(if on { Toggled::True } else { Toggled::False });
            }
            if button.enabled {
                node.add_action(Action::Click);
            } else {
                node.set_disabled();
            }
            buttons.push(self.add(&toolbar_id().with(button.name), node));
        }
        let mut group = Node::new(Role::Group);
        group.set_label("Toolbar");
        group.set_children(buttons);
        self.add(&toolbar_id(), group)
    }

    /// The right panel's tabs: a tab list with the open tab selected.
    fn tabs(&mut self, open: Tab) -> NodeId {
        let mut tabs = Vec::new();
        for (tab, label) in [(Tab::Inspector, "Inspector"), (Tab::Matrix, "Matrix")] {
            let mut node = Node::new(Role::Tab);
            node.set_label(label);
            node.set_selected(tab == open);
            node.add_action(Action::Click);
            tabs.push(self.add(&tab_id(tab), node));
        }
        let mut list = Node::new(Role::TabList);
        list.set_label("Right panel");
        list.set_children(tabs);
        self.add(&tabs_id(), list)
    }

    /// The bar at the bottom: the developer switches, the fixed items, a
    /// kept error, and the status line. Plain text is a label whose value
    /// is the text shown. A warning is a status, so it is announced, and
    /// ordinary status text is not (ui-layer.md, section 7, the role table).
    fn bar(&mut self, model: &bars::Model) -> NodeId {
        let mut items = Vec::new();
        if let Some(dev) = &model.dev {
            for (id, label, on) in [
                (bars::MEASURE_ID, "Measure", dev.measuring),
                (bars::MOVE_CAMERA_ID, "Move camera", dev.animating),
            ] {
                let mut node = Node::new(Role::Switch);
                node.set_label(label);
                node.set_toggled(if on { Toggled::True } else { Toggled::False });
                node.add_action(Action::Click);
                items.push(self.add(&fixed_id(id), node));
            }
        }
        for item in &model.items {
            let mut node = Node::new(match item.kind {
                bars::Kind::Alert => Role::Status,
                bars::Kind::Text | bars::Kind::Live => Role::Label,
            });
            node.set_value(item.text.clone());
            if let Some(hover) = &item.hover {
                node.set_description(hover.clone());
            }
            items.push(self.add(&stats_id().with(item.name), node));
        }
        let fields = [
            (model.kept.as_ref(), bars::KEPT_ID, bars::KEPT_DISMISS_ID),
            (Some(&model.status), bars::STATUS_ID, bars::STATUS_DISMISS_ID),
        ];
        for (field, id, dismiss_id) in fields {
            let Some(field) = field else { continue };
            let plain = field.severity == crate::alerts::Severity::Normal;
            let mut node = Node::new(if plain { Role::Label } else { Role::Status });
            node.set_value(field.shown.clone());
            if field.hover != field.shown {
                node.set_description(field.hover.clone());
            }
            items.push(self.add(&fixed_id(id), node));
            if field.dismiss.is_some() {
                let mut dismiss = Node::new(Role::Button);
                dismiss.set_label(field.dismiss_hover());
                dismiss.add_action(Action::Click);
                items.push(self.add(&fixed_id(dismiss_id), dismiss));
            }
        }
        let mut group = Node::new(Role::Group);
        group.set_label("Status bar");
        group.set_children(items);
        self.add(&stats_id(), group)
    }
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
    match spec {
        Some(spec) => {
            node.set_numeric_value(f64::from(spec.denormalize(position)));
            node.set_min_numeric_value(f64::from(spec.min));
            node.set_max_numeric_value(f64::from(spec.max));
            if let (Some(share), ParamMapping::Linear) = (arrow_share(kind), spec.options.mapping) {
                node.set_numeric_value_step(f64::from(share * (spec.max - spec.min)));
            }
        }
        // No parameter to ask: the control's travel, from 0 to 1.
        None => {
            node.set_numeric_value(f64::from(position));
            node.set_min_numeric_value(0.0);
            node.set_max_numeric_value(1.0);
        }
    }
    node.add_action(Action::Increment);
    node.add_action(Action::Decrement);
    node.add_action(Action::SetValue);
    node
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
            let mut node = Node::new(Role::Switch);
            node.set_toggled(if widget.position >= 0.5 {
                Toggled::True
            } else {
                Toggled::False
            });
            node.add_action(Action::Click);
            node
        }
        WidgetViewKind::Button => {
            let mut node = Node::new(Role::Button);
            node.add_action(Action::Click);
            node
        }
        // One radio button per option, inside.
        WidgetViewKind::Radio { .. } | WidgetViewKind::Segmented { .. } => Node::new(Role::RadioGroup),
        // It says its chosen option and steps one option at a time. It
        // does not open: a list a screen reader can walk comes with our
        // own select, in phase 3.
        WidgetViewKind::Select { .. } => {
            let mut node = Node::new(Role::ComboBox);
            node.set_value(widget.text.clone());
            node.add_action(Action::Increment);
            node.add_action(Action::Decrement);
            description = None;
            node
        }
        // One slider per value, inside.
        WidgetViewKind::XyPad { .. } | WidgetViewKind::Range { .. } => Node::new(Role::Group),
        WidgetViewKind::Meter { .. } => {
            // The number its text says, never the stored level: the
            // tree does not work the mapping out a second time. Text with
            // no single number in it is said as words only.
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
        WidgetViewKind::Scope => {
            description = trace_words(widget);
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
                node.set_description(spec.value_text(spec.denormalize(widget.position)));
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
