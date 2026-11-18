//! The window's focus order and its accessibility nodes (ui-layer.md,
//! section 6, rule 2, and section 7).
//!
//! **The focus order** is the reading order:
//!
//! 1. by layer, from the top: the question, then the overlays, the panels,
//!    and the canvas;
//! 2. inside a layer, root panel by root panel in the order a reader meets
//!    them, by where each one's first focus stop is: the higher first, and
//!    where two are level (their first stops overlap from top to bottom),
//!    the one further left, then the view's order. Draw order is not used,
//!    because what is drawn on top is not what is read first;
//! 3. inside a panel, its elements in the order it builds them, with the
//!    panel in a slot at the slot's place in that order.
//!
//! **The nodes** nest as the elements do: an element's node holds the
//! nodes of the accessed elements inside it, and a slot's panel's nodes go
//! in the node around the slot.

use super::Window;
use crate::field::run_id;
use crate::focus::AccessNodes;
use crate::view::Layer;
use accesskit::{Node, NodeId};
use loom_ui_core::{IdTable, Point, Rect, UiId};

impl Window {
    /// Works the focus order out again, after a layout.
    pub(super) fn order_stops(&mut self) {
        let mut roots: Vec<(Layer, Option<Rect>, usize, &UiId)> = self
            .order
            .iter()
            .enumerate()
            .filter(|(_, key)| self.records[*key].parent.is_none())
            .map(|(place, key)| {
                let mut stops = Vec::new();
                self.panel_stops(key, &mut stops);
                let first = stops.first().and_then(|stop| self.bounds(stop));
                (self.records[key].layer, first, place, key)
            })
            .collect();
        // The top layer first, then by the top of each first stop, then the
        // view's order. A panel with no stops adds nothing, wherever it sorts.
        roots.sort_by(|a, b| {
            let top = |first: Option<Rect>| first.map_or(f32::MAX, |first| first.y);
            b.0.cmp(&a.0).then(top(a.1).total_cmp(&top(b.1))).then(a.2.cmp(&b.2))
        });
        // Then each line of level panels, left to right: a panel is level
        // with the first of the line when their first stops overlap from
        // top to bottom, so a stop one pixel lower does not read first.
        let mut start = 0;
        while start < roots.len() {
            let (layer, first) = (roots[start].0, roots[start].1);
            let end = start
                + roots[start..]
                    .iter()
                    .take_while(|root| root.0 == layer && level(first, root.1))
                    .count();
            roots[start..end].sort_by(|a, b| {
                let left = |first: Option<Rect>| first.map_or(f32::MAX, |first| first.x);
                left(a.1).total_cmp(&left(b.1)).then(a.2.cmp(&b.2))
            });
            start = end.max(start + 1);
        }
        let keys: Vec<UiId> = roots.into_iter().map(|(_, _, _, key)| key.clone()).collect();
        let mut stops = Vec::new();
        for key in &keys {
            self.panel_stops(key, &mut stops);
        }
        self.stops = stops;
    }

    /// Adds the focus stops of the panel keyed `key` to `stops`, in build
    /// order, with each slot's panel at the slot's place.
    fn panel_stops(&self, key: &UiId, stops: &mut Vec<UiId>) {
        let Some(record) = self.records.get(key) else {
            return;
        };
        let mut slot = 0;
        for element in record.layout.records() {
            if element.focusable
                && let Some(id) = &element.id
            {
                stops.push(id.clone());
            }
            if let Some(child) = &element.slot {
                if self.records.get(child).is_some_and(|held| held.held_by(key, slot)) {
                    self.panel_stops(child, stops);
                }
                slot += 1;
            }
        }
    }

    /// The focus stops in focus order, as last laid out.
    pub(crate) fn stops(&self) -> &[UiId] {
        &self.stops
    }

    /// The focus stop a press at `point` focuses, among the panels of the
    /// layers `keep` allows, tried as [`Window::hit_in`] tries them. `None`
    /// when no panel takes the point; `Some(None)` when one takes it and no
    /// stop is there.
    pub(crate) fn focus_at_in(&self, point: Point, keep: impl Fn(Layer) -> bool) -> Option<Option<&UiId>> {
        self.first_taker(point, |record, inside| {
            keep(record.layer).then(|| record.layout.focus_at(inside)).flatten()
        })
    }

    /// The panel that holds the element with UI ID `id`: the one whose key
    /// starts the ID, the longest such key if several do.
    pub(crate) fn panel_of(&self, id: &UiId) -> Option<&UiId> {
        self.records
            .keys()
            .filter(|key| id == *key || id.is_under(key))
            .max_by_key(|key| key.keys().len())
    }

    /// What the element with UI ID `id` is to a screen reader, as last
    /// laid out.
    pub(crate) fn access_of(&self, id: &UiId) -> Option<&crate::access::Access> {
        let panel = self.panel_of(id)?;
        self.records[panel]
            .layout
            .records()
            .iter()
            .find(|element| element.id.as_ref() == Some(id))?
            .access
            .as_ref()
    }

    /// Whether the panel that holds the element with UI ID `id` has it in
    /// its current layout, with a node: false for an element a rebuild
    /// removed before the next layout drops what the window keeps for it.
    pub(crate) fn has_node(&self, id: &UiId) -> bool {
        self.panel_of(id)
            .is_some_and(|panel| self.records[panel].layout.has_node(id))
    }

    /// The layer of the panel that holds the element with UI ID `id`.
    pub(crate) fn layer_of(&self, id: &UiId) -> Option<Layer> {
        self.panel_of(id).map(|panel| self.records[panel].layer)
    }

    /// The first focus stop of the panel keyed `panel`, or of the panels in
    /// its slots, in focus order.
    pub(crate) fn first_stop_in(&self, panel: &UiId) -> Option<UiId> {
        let mut stops = Vec::new();
        self.panel_stops(panel, &mut stops);
        stops.into_iter().next()
    }

    /// The nodes of every laid-out panel, with node IDs from `ids`, bounds
    /// in physical pixels at `scale` (section 7, rule 8), and `focused` as
    /// the focus.
    pub(crate) fn access_nodes(&self, ids: &mut IdTable, scale: f32, focused: Option<&UiId>) -> AccessNodes {
        let mut out = AccessNodes::default();
        for key in &self.drawn {
            let record = &self.records[key];
            if record.parent.is_some() {
                continue;
            }
            let tops = self.panel_nodes(key, ids, scale, &mut out.nodes);
            out.roots.push((key.clone(), record.layer, tops));
        }
        // Only a node that was made: a tree whose focus names no node is
        // refused by the screen reader's side.
        out.focus = focused
            .filter(|id| self.stops.contains(id))
            .filter(|id| {
                self.panel_of(id)
                    .is_some_and(|panel| self.records[panel].layout.has_node(id))
            })
            .map(|id| NodeId(ids.number(id)));
        out
    }

    /// Adds the nodes of the panel keyed `key`, and of the panels in its
    /// slots, to `nodes`, and gives the ones no other of its nodes holds.
    fn panel_nodes(&self, key: &UiId, ids: &mut IdTable, scale: f32, nodes: &mut Vec<(NodeId, Node)>) -> Vec<NodeId> {
        let record = &self.records[key];
        let elements = record.layout.records();
        // Each element's node, as its place in `made`.
        let mut node_of: Vec<Option<usize>> = vec![None; elements.len()];
        let mut made: Vec<(NodeId, Node, Vec<NodeId>)> = Vec::new();
        let mut tops = Vec::new();
        // The nodes of the panels in its slots. They go after this panel's,
        // so a parent always comes before its children.
        let mut later = Vec::new();
        let mut slot = 0;
        for (index, element) in elements.iter().enumerate() {
            // The nearest node around it: parents come before children, so
            // theirs are made already.
            let mut holder = element.parent;
            while let Some(at) = holder {
                if node_of[at].is_some() {
                    break;
                }
                holder = elements[at].parent;
            }
            let holder = holder.and_then(|at| node_of[at]);
            let mut add = |child: NodeId, made: &mut Vec<(NodeId, Node, Vec<NodeId>)>| match holder {
                Some(at) => made[at].2.push(child),
                None => tops.push(child),
            };
            if let (Some(access), Some(id)) = (&element.access, &element.id) {
                let mut node = access.node(element.focusable);
                let bounds = element.bounds;
                let origin = record.origin;
                let scale = f64::from(scale);
                node.set_bounds(accesskit::Rect {
                    x0: f64::from(origin.x + bounds.x) * scale,
                    y0: f64::from(origin.y + bounds.y) * scale,
                    x1: f64::from(origin.x + bounds.x + bounds.width) * scale,
                    y1: f64::from(origin.y + bounds.y + bounds.height) * scale,
                });
                let node_id = NodeId(ids.number(id));
                add(node_id, &mut made);
                node_of[index] = Some(made.len());
                // A text field: its text as it is now, which may be ahead
                // of the app's by this turn's edits, its runs, and its
                // selection in them (section 7, rule 10).
                let runs = self.fields.get(id).map(|field| {
                    node.set_value(field.text().to_owned());
                    if !access.is_disabled() {
                        node.add_action(accesskit::Action::SetValue);
                        node.add_action(accesskit::Action::SetTextSelection);
                        node.add_action(accesskit::Action::ReplaceSelectedText);
                    }
                    let content = element.content;
                    let left = origin.x + content.x - field.scroll();
                    let top = origin.y + content.y;
                    let mut runs: Vec<(NodeId, Node)> = field
                        .runs()
                        .into_iter()
                        .enumerate()
                        .map(|(at, run)| {
                            let mut node = Node::new(accesskit::Role::TextRun);
                            node.set_bounds(accesskit::Rect {
                                x0: f64::from(left + run.x) * scale,
                                y0: f64::from(top) * scale,
                                x1: f64::from(left + run.x + run.width) * scale,
                                y1: f64::from(top + content.height) * scale,
                            });
                            let physical = |values: &[f32]| -> Vec<f32> {
                                values.iter().map(|value| value * scale as f32).collect()
                            };
                            node.set_character_positions(physical(&run.positions));
                            node.set_character_widths(physical(&run.widths));
                            node.set_character_lengths(run.lengths);
                            node.set_word_starts(run.word_starts);
                            node.set_text_direction(accesskit::TextDirection::LeftToRight);
                            node.set_value(run.text);
                            (NodeId(ids.number(&run_id(id, at))), node)
                        })
                        .collect();
                    // A field is one line, so its runs are one line too:
                    // each links to its neighbours, or a screen reader
                    // would read 255 characters as a whole line.
                    let ids: Vec<NodeId> = runs.iter().map(|(id, _)| *id).collect();
                    for (at, (_, run)) in runs.iter_mut().enumerate() {
                        if at > 0 {
                            run.set_previous_on_line(ids[at - 1]);
                        }
                        if let Some(next) = ids.get(at + 1) {
                            run.set_next_on_line(*next);
                        }
                    }
                    let ((anchor_run, anchor), (focus_run, focus)) = field.reader_selection();
                    node.set_text_selection(accesskit::TextSelection {
                        anchor: accesskit::TextPosition {
                            node: runs[anchor_run].0,
                            character_index: anchor,
                        },
                        focus: accesskit::TextPosition {
                            node: runs[focus_run].0,
                            character_index: focus,
                        },
                    });
                    runs
                });
                let children = runs
                    .as_ref()
                    .map_or_else(Vec::new, |runs| runs.iter().map(|(id, _)| *id).collect());
                made.push((node_id, node, children));
                for (run, node) in runs.into_iter().flatten() {
                    made.push((run, node, Vec::new()));
                }
            }
            if let Some(child) = &element.slot {
                if self.records.get(child).is_some_and(|held| held.held_by(key, slot)) {
                    for id in self.panel_nodes(child, ids, scale, &mut later) {
                        add(id, &mut made);
                    }
                }
                slot += 1;
            }
        }
        for (id, mut node, children) in made {
            if !children.is_empty() {
                node.set_children(children);
            }
            nodes.push((id, node));
        }
        nodes.append(&mut later);
        tops
    }
}

/// True when two first stops are level: their spans from top to bottom
/// overlap. Two panels with no stop are level with each other.
fn level(a: Option<Rect>, b: Option<Rect>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.y < b.y + b.height && b.y < a.y + a.height,
        (None, None) => true,
        _ => false,
    }
}
