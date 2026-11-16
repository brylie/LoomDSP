//! Keyboard focus among the runtime's own elements, and the nodes it gives
//! a screen reader (ui-layer.md, sections 6 and 7).
//!
//! The runtime keeps which of its elements has focus, and whether the
//! keyboard put it there, which is when a ring shows (section 6, rule 3).
//! Elements a toolkit still draws keep their own focus in the routing
//! target; crossing from one toolkit's order to the other's is routing.

use crate::view::Layer;
use accesskit::{Node, NodeId};
use loom_ui_core::{Color, UiId};

/// Where a step through the focus order lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusStep {
    /// On this focus stop.
    To(UiId),
    /// Past the first or the last stop, or there are none: focus leaves
    /// the runtime's elements, for a router to hand it on.
    PastEnd,
}

/// How a focus ring looks. The app fills it from its own style tokens: this
/// crate keeps no palette.
///
/// The ring must stand out against what it sits on: at least 3:1 against
/// the backgrounds around the focused element (WCAG 2.2, success criteria
/// 1.4.11 and 2.4.13), and at least 2 logical pixels wide. The app checks
/// its values against its own backgrounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingStyle {
    /// Its colour.
    pub color: Color,
    /// How wide its line is, in logical pixels.
    pub width: f32,
    /// How far outside the element's bounds it is drawn, in logical pixels.
    pub offset: f32,
}

/// The runtime's elements as a screen reader meets them, as last laid out:
/// one node per element with an [`Access`](crate::access::Access), nested
/// as the elements are, with a slot's panel inside the node around the
/// slot. Node IDs come from the app's table, so every node in the window
/// has an ID from one table (section 4, rule 4).
#[derive(Debug, Clone, Default)]
pub struct AccessNodes {
    /// Every node, parents before their children.
    pub nodes: Vec<(NodeId, Node)>,
    /// Each root panel, in draw order: its key, its layer, and its nodes
    /// that no other of its nodes holds. The app puts them in its tree
    /// where that panel is.
    pub roots: Vec<(UiId, Layer, Vec<NodeId>)>,
    /// The node of the element with focus, when one of these has it.
    pub focus: Option<NodeId>,
}

#[cfg(test)]
mod tests;
