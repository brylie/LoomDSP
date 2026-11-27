//! A custom element's accessibility nodes, and a screen reader's actions
//! on them (ui-layer.md, section 9, rule 3).
//!
//! The element gives a node for each part inside it a user can reach, by a
//! path under its UI ID. A part whose path is under another part's goes
//! inside that one in the tree, as UI IDs nest (section 4). The runtime
//! builds them outside `update`, which holds the app mutably, and keeps
//! them until the next build: after a layout that made or moved the
//! element, after an event or tick that said they changed, or after a turn
//! in which the element's access version moved. A frame builds none, so an
//! element that animates costs the tree nothing. Node IDs are given when
//! the tree is sent, from the app's one table, so none is kept here and
//! none is reused.

use super::{Customs, Out};
use crate::access::{Access, Action};
use crate::app::App;
use crate::draw::Panels;
use loom_ui_core::{IdKey, Rect, UiId};

/// The nodes a custom element gives for its parts ([`Custom::access`](super::Custom::access)).
#[derive(Debug, Default)]
pub struct CustomNodes {
    nodes: Vec<CustomNode>,
}

impl CustomNodes {
    /// Adds a node for the part at `path` under the element, with `access`
    /// and `bounds` in window logical pixels. Add a part after the part it
    /// is inside, in the element's own focus order.
    pub fn add(
        &mut self,
        path: impl IntoIterator<Item = impl Into<IdKey>>,
        access: Access,
        bounds: Rect,
    ) -> &mut CustomNode {
        self.nodes.push(CustomNode {
            path: path.into_iter().map(Into::into).collect(),
            access,
            bounds,
            focusable: false,
        });
        self.nodes.last_mut().expect("a node was just added")
    }
}

/// One part's node, as an element gave it.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomNode {
    path: Vec<IdKey>,
    access: Access,
    bounds: Rect,
    focusable: bool,
}

impl CustomNode {
    /// Says a user can put the focus on the part, so its node offers
    /// Focus: the element takes it in [`Custom::focus_inner`](super::Custom::focus_inner).
    pub fn focusable(&mut self) -> &mut Self {
        self.focusable = true;
        self
    }

    /// Its path under the element's UI ID.
    pub(crate) fn path(&self) -> &[IdKey] {
        &self.path
    }

    /// What it is to a screen reader.
    pub(crate) fn access(&self) -> &Access {
        &self.access
    }

    /// Where it is, in window logical pixels.
    pub(crate) fn bounds(&self) -> Rect {
        self.bounds
    }

    /// True when the focus can go on it.
    pub(crate) fn is_focusable(&self) -> bool {
        self.focusable
    }
}

/// A screen reader's request on a custom element's node: what to do, and
/// the value to set, for [`Action::SetValue`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AccessRequest {
    /// What to do.
    pub action: Action,
    /// The value to set, when the request carries one.
    pub value: Option<AccessValue>,
}

/// The value a screen reader asks to set.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AccessValue {
    /// A number, as on a slider.
    Number(f64),
    /// Text.
    Text(String),
}

impl AccessRequest {
    /// The request for AccessKit's `request`, or `None` when it is not an
    /// element's own to carry out.
    pub(crate) fn from_kit(request: &accesskit::ActionRequest) -> Option<Self> {
        let action = Action::from_kit(request.action)?;
        let value = match &request.data {
            Some(accesskit::ActionData::NumericValue(number)) => Some(AccessValue::Number(*number)),
            Some(accesskit::ActionData::Value(text)) => Some(AccessValue::Text(text.to_string())),
            _ => None,
        };
        Some(Self { action, value })
    }
}

impl<A: App> Customs<A> {
    /// The element whose node `id` is, and the path under it: its own node
    /// with an empty path, or a part's.
    fn node_owner<'a>(&self, id: &'a UiId) -> Option<(UiId, &'a [IdKey])> {
        if self.is_custom(id) {
            return Some((id.clone(), &[]));
        }
        let owner = self.owner_of(id)?.clone();
        let rest = &id.keys()[owner.keys().len()..];
        Some((owner, rest))
    }

    /// Asks the element `id` to carry out `request` on its node at `rest`.
    fn access_action(&mut self, app: &A, id: &UiId, rest: &[IdKey], request: &AccessRequest) -> bool {
        let Some(state) = self.elements.get_mut(id).and_then(|hosted| hosted.state.as_deref_mut()) else {
            return false;
        };
        let mut out = Out {
            sent: &mut self.sent,
            redraw: &mut self.redraw,
            nodes_changed: &mut self.nodes_changed,
        };
        state.access_action(app, rest, request, &mut out)
    }

    /// True when a build outside a turn changed the nodes since the last
    /// call, which forgets it.
    pub(crate) fn take_nodes_reported(&mut self) -> bool {
        std::mem::take(&mut self.nodes_reported)
    }

    /// True when a build outside a turn changed the nodes and no turn has
    /// told the app yet.
    pub(crate) fn nodes_reported(&self) -> bool {
        self.nodes_reported
    }
}

impl<A: App> Panels<'_, A> {
    /// Builds the nodes of each custom element that needs it, and drops
    /// those of elements that are gone. Returns true when what the tree
    /// holds changed, so the app hears of it ([`App::access_changed`]).
    pub(crate) fn build_custom_nodes(&mut self) -> bool {
        let force = std::mem::take(&mut self.customs.nodes_changed);
        let before = self.window.custom_nodes.len();
        let customs = &*self.customs;
        self.window.custom_nodes.retain(|id, _| customs.is_custom(id));
        let mut changed = self.window.custom_nodes.len() != before;
        let panics = self.window.panics_on_mistakes();
        for (id, hosted) in &mut self.customs.elements {
            let Some(state) = hosted.state.as_deref() else {
                continue;
            };
            let version = state.access_version(self.app);
            if !force && hosted.built == Some((version, hosted.bounds)) {
                continue;
            }
            hosted.built = Some((version, hosted.bounds));
            let mut nodes = CustomNodes::default();
            state.access(self.app, hosted.bounds, &mut nodes);
            self.window.custom_problems.retain(|(part, _)| !part.is_under(id));
            for node in &nodes.nodes {
                let part = node.path.iter().cloned().fold(id.clone(), UiId::with);
                let problem = if node.path.is_empty() {
                    Some(crate::access::Problem::NoKey)
                } else {
                    node.access.problem(node.focusable)
                };
                if let Some(problem) = problem {
                    assert!(!panics, "a node {part} of a custom element breaks a rule: {problem}");
                    self.window.custom_problems.push((part, problem));
                }
            }
            if self.window.custom_nodes.get(id) != Some(&nodes.nodes) {
                self.window.custom_nodes.insert(id.clone(), nodes.nodes);
                changed = true;
            }
        }
        changed
    }

    /// Builds the custom elements' nodes after a layout, outside a turn: a
    /// change is told to the app on the next turn, which the waker wakes.
    pub(crate) fn build_custom_nodes_later(&mut self) {
        if self.build_custom_nodes() {
            self.customs.nodes_reported = true;
        }
    }

    /// Gives a screen reader's `request` on the node with UI ID `id` to the
    /// custom element it belongs to. `None` when `id` is no custom
    /// element's node, or the element does not carry it out, so the
    /// request goes on to the app. `Some(false)` when the element does not
    /// take input now, or the node does not offer the action or is off:
    /// the request is used up and changes nothing (section 5, rule 3;
    /// section 7, rule 22).
    pub(crate) fn custom_action(&mut self, id: &UiId, request: &AccessRequest) -> Option<bool> {
        let (owner, rest) = self.customs.node_owner(id)?;
        let access = if rest.is_empty() {
            self.window.access_of(&owner)
        } else {
            self.window
                .custom_nodes
                .get(&owner)
                .and_then(|nodes| nodes.iter().find(|node| node.path == rest))
                .map(CustomNode::access)
        };
        let offered = access.is_some_and(|access| access.offers(request.action) && !access.is_disabled());
        if !offered || !crate::runtime::in_input_layer(self.window, self.app, &owner) {
            return Some(false);
        }
        self.customs
            .access_action(self.app, &owner, rest, request)
            .then_some(false)
    }
}

#[cfg(test)]
mod tests;
