//! The accessibility tree as the harness sees it (ui-layer.md, section 10,
//! rule 4): [`Ui::a11y_tree`] reads the tree a screen reader is sent, and
//! [`Ui::a11y_action`] and [`Ui::a11y_set_value`] act on a node the way a
//! screen reader's request does.
//!
//! Nodes are found by their accessible name, which is what a screen reader
//! says. Names repeat (every filter tile has a Cutoff knob), so a lookup can
//! be scoped by the names above it, such as `["Filter", "Cutoff"]`, or name
//! one exact part by its UI ID. A name that matches no node, or more than
//! one, panics and lists what it found, so a test never acts on the wrong
//! node and the panic shows how to scope it. Hidden nodes, and everything
//! inside them, are left out, as a screen reader leaves them out.

use super::Ui;
use crate::Message;
use crate::a11y::Event;
use accesskit::{Action, ActionData, ActionRequest, Node, NodeId, Role, TreeId, TreeUpdate};
use loom_ui_core::UiId;
use std::collections::HashMap;

/// The whole accessibility tree at one moment, with the UI ID of each node.
#[derive(Debug)]
pub struct A11yTree {
    /// Every node, by its ID.
    nodes: HashMap<NodeId, Node>,
    /// The window's node.
    root: NodeId,
    /// The node with keyboard focus.
    focus: NodeId,
    /// The UI ID each node was given.
    ids: HashMap<NodeId, UiId>,
}

/// One node of an [`A11yTree`].
#[derive(Debug, Clone, Copy)]
pub struct A11yNode<'a> {
    tree: &'a A11yTree,
    id: NodeId,
}

/// A value a screen reader sets: text, for a field, or a number, for a
/// slider or a knob.
#[derive(Debug, Clone, PartialEq)]
pub enum A11yValue {
    /// Text, sent as [`ActionData::Value`].
    Text(String),
    /// A number, sent as [`ActionData::NumericValue`].
    Number(f64),
}

impl From<&str> for A11yValue {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

impl From<f64> for A11yValue {
    fn from(number: f64) -> Self {
        Self::Number(number)
    }
}

/// How a test names the node to act on: one accessible name, a path of
/// names from the outside in, or a UI ID.
pub trait A11yTarget {
    /// The node it names in `tree`. Panics on none or several.
    fn find<'a>(&self, tree: &'a A11yTree) -> A11yNode<'a>;
}

impl A11yTarget for &str {
    fn find<'a>(&self, tree: &'a A11yTree) -> A11yNode<'a> {
        tree.node(self)
    }
}

impl A11yTarget for &[&str] {
    fn find<'a>(&self, tree: &'a A11yTree) -> A11yNode<'a> {
        tree.path(self)
    }
}

impl<const N: usize> A11yTarget for [&str; N] {
    fn find<'a>(&self, tree: &'a A11yTree) -> A11yNode<'a> {
        tree.path(self)
    }
}

impl A11yTarget for &UiId {
    fn find<'a>(&self, tree: &'a A11yTree) -> A11yNode<'a> {
        tree.node_by_id(self)
    }
}

impl A11yTree {
    /// The tree in `update`, a whole tree, with node IDs read back to UI
    /// IDs through `id_of`.
    fn new(update: TreeUpdate, id_of: impl Fn(NodeId) -> Option<UiId>) -> Self {
        let root = update.tree.as_ref().expect("a whole tree names its root").root;
        let ids = update
            .nodes
            .iter()
            .filter_map(|(node, _)| Some((*node, id_of(*node)?)))
            .collect();
        Self {
            nodes: update.nodes.into_iter().collect(),
            root,
            focus: update.focus,
            ids,
        }
    }

    /// The window's node, which holds every other.
    pub fn root(&self) -> A11yNode<'_> {
        A11yNode {
            tree: self,
            id: self.root,
        }
    }

    /// The node with keyboard focus.
    pub fn focused(&self) -> A11yNode<'_> {
        A11yNode {
            tree: self,
            id: self.focus,
        }
    }

    /// The one shown node named `name`, anywhere in the window.
    pub fn node(&self, name: &str) -> A11yNode<'_> {
        self.root().node(name)
    }

    /// The node at the end of `path`: each name is searched for inside the
    /// node the name before it found.
    pub fn path(&self, path: &[&str]) -> A11yNode<'_> {
        path.iter().fold(self.root(), |node, name| node.node(name))
    }

    /// The node for the part `id` names, shown or not. Panics if there is
    /// none.
    pub fn node_by_id(&self, id: &UiId) -> A11yNode<'_> {
        let node = self
            .ids
            .iter()
            .find_map(|(node, of)| (of == id).then_some(*node))
            .unwrap_or_else(|| panic!("no node in the accessibility tree has the UI ID {id}"));
        A11yNode { tree: self, id: node }
    }

    /// The accessible name of every shown node, in tree order, for a test
    /// that checks what a screen reader can reach.
    pub fn names(&self) -> Vec<String> {
        let mut names = Vec::new();
        self.root().walk(&mut |node| {
            if let Some(label) = node.name() {
                names.push(label.to_string());
            }
        });
        names
    }
}

impl<'a> A11yNode<'a> {
    /// AccessKit's node.
    fn raw(&self) -> &'a Node {
        &self.tree.nodes[&self.id]
    }

    /// Its accessible name, what a screen reader says for it.
    pub fn name(&self) -> Option<&'a str> {
        self.raw().label()
    }

    /// Its role: a button, a slider, a group.
    pub fn role(&self) -> Role {
        self.raw().role()
    }

    /// Its text value, such as a field's text or a choice.
    pub fn value(&self) -> Option<&'a str> {
        self.raw().value()
    }

    /// Its number, for a slider or a knob.
    pub fn number(&self) -> Option<f64> {
        self.raw().numeric_value()
    }

    /// Its description, which holds a value's unit and a hover text.
    pub fn description(&self) -> Option<&'a str> {
        self.raw().description()
    }

    /// True when it offers `action`.
    pub fn offers(&self, action: Action) -> bool {
        self.raw().supports_action(action)
    }

    /// True when it has keyboard focus.
    pub fn is_focused(&self) -> bool {
        self.id == self.tree.focus
    }

    /// Its UI ID.
    pub fn ui_id(&self) -> Option<&'a UiId> {
        self.tree.ids.get(&self.id)
    }

    /// Its shown children, in order. A hidden node has none: what is
    /// inside it is hidden too, even when only the node itself is marked,
    /// as a covered tile is.
    pub fn children(&self) -> Vec<A11yNode<'a>> {
        if self.raw().is_hidden() {
            return Vec::new();
        }
        self.raw()
            .children()
            .iter()
            .map(|id| A11yNode {
                tree: self.tree,
                id: *id,
            })
            .filter(|child| !child.raw().is_hidden())
            .collect()
    }

    /// The one shown node named `name` inside this one. Panics on none or
    /// several, listing what it found.
    pub fn node(&self, name: &str) -> A11yNode<'a> {
        let mut found = Vec::new();
        for child in self.children() {
            child.walk(&mut |node| {
                if node.name() == Some(name) {
                    found.push(node);
                }
            });
        }
        match found.as_slice() {
            [node] => *node,
            [] => panic!(
                "no shown node named {name:?} inside {:?}; the names there: {:?}",
                self.name().unwrap_or("the window"),
                self.names_inside()
            ),
            several => panic!(
                "{} shown nodes are named {name:?} inside {:?}: scope it by the names above it, or use its UI ID. \
                 The matches, each with the names above it: {:?}",
                several.len(),
                self.name().unwrap_or("the window"),
                several.iter().map(|node| self.path_to(node.id)).collect::<Vec<_>>()
            ),
        }
    }

    /// The names of the shown nodes inside this one.
    fn names_inside(&self) -> Vec<&'a str> {
        let mut names = Vec::new();
        for child in self.children() {
            child.walk(&mut |node| names.extend(node.name()));
        }
        names
    }

    /// The names from this node down to `target`, for a panic message.
    fn path_to(&self, target: NodeId) -> Vec<&'a str> {
        fn search<'a>(node: A11yNode<'a>, target: NodeId, path: &mut Vec<&'a str>) -> bool {
            path.extend(node.name());
            if node.id == target || node.children().into_iter().any(|child| search(child, target, path)) {
                return true;
            }
            if node.name().is_some() {
                path.pop();
            }
            false
        }
        let mut path = Vec::new();
        for child in self.children() {
            if search(child, target, &mut path) {
                break;
            }
        }
        path
    }

    /// Calls `f` on this node and every shown node inside it, in tree
    /// order.
    fn walk(&self, f: &mut impl FnMut(A11yNode<'a>)) {
        f(*self);
        for child in self.children() {
            child.walk(f);
        }
    }
}

impl Ui {
    /// The accessibility tree as a screen reader is sent it now, with the
    /// canvas and the window's parts where this view placed them.
    pub fn a11y_tree(&mut self) -> A11yTree {
        let places = self.places();
        let update = self.app_mut().a11y_tree(&places);
        let app = self.app();
        A11yTree::new(update, |node| app.a11y_ui_id(node))
    }

    /// Performs `action` on the node `target` names, as a screen reader's
    /// request does: it reaches the app as the adapter's request, an
    /// [`Event::Action`], runs until no messages are left, and draws the
    /// next frame. A test reads what changed from the tree or the app.
    pub fn a11y_action(&mut self, target: impl A11yTarget, action: Action) {
        self.a11y_request(target, action, None);
    }

    /// Sets the value of the node `target` names, as a screen reader's
    /// SetValue does: text for a field, a number for a slider or a knob.
    pub fn a11y_set_value(&mut self, target: impl A11yTarget, value: impl Into<A11yValue>) {
        let data = match value.into() {
            A11yValue::Text(text) => ActionData::Value(text.into()),
            A11yValue::Number(number) => ActionData::NumericValue(number),
        };
        self.a11y_request(target, Action::SetValue, Some(data));
    }

    /// Sends a request for `action` on `target`, with `data`.
    fn a11y_request(&mut self, target: impl A11yTarget, action: Action, data: Option<ActionData>) {
        let tree = self.a11y_tree();
        let node = target.find(&tree);
        let request = ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: node.id,
            data,
        };
        let _ = self.deliver(Message::A11y(Event::Action(request)));
        // The window draws its next frame after the turn, as after any
        // other; the canvas acts on some requests there, such as opening
        // the module browser.
        let _ = self.redraw();
    }
}

#[cfg(test)]
mod tests;
