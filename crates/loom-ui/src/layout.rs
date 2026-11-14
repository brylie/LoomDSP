//! Layout: where each element of one panel goes (ui-layer.md, sections 4
//! and 5).
//!
//! A [`Layout`] belongs to one panel and lives as long as the panel does.
//! [`Layout::rebuild`] takes the panel's new [`Element`] tree when its model
//! changes; [`Layout::compute`] places it in the space the panel has, again
//! on every resize, with no rebuild. Then [`Layout::bounds`] gives each
//! keyed element's rectangle by UI ID, and [`Layout::hit`] finds the
//! element under a point.
//!
//! # Units and rounding
//!
//! Layout is in logical pixels, rounded to whole logical pixels. Each edge
//! is rounded from its place in the panel, never each width, so two
//! neighbours always meet with no gap and no overlap. At a scale factor of
//! 2 every edge then lands on an even physical pixel; placing at half a
//! logical pixel waits until that shows up as a problem (decision of
//! 2026-11-14).
//!
//! # Text
//!
//! Text is measured through a [`TextMeasure`], the same object that sets
//! the text for drawing, so the box laid out for a label is the size the
//! label is drawn. A text on one line is measured once until the panel
//! rebuilds, whatever width it is offered. A wrapping text keeps its last
//! three sizes by width, which covers the narrowest, widest, and final
//! widths the layout asks for.
//!
//! # Duplicate keys
//!
//! Two elements with the same UI ID are a mistake in the panel's code. In a
//! debug build, [`Layout::rebuild`] panics, naming the ID. In a release
//! build, the first in tree order keeps the ID, and every later one is laid
//! out and drawn but has no ID, and neither has anything inside it: it is
//! left out of [`Layout::bounds`] and [`Layout::ids`], and the pointer
//! passes through it to the nearest keyed element around it. Tree order is the order the panel built its elements, parent before
//! children, so the same model always gives the same result.
//! [`Layout::duplicate_keys`] counts them, so a measurement can show them.
//!
//! The layout engine is `taffy`, behind these types: no taffy type is in
//! this crate's API.

use crate::element::{Align, Element, Kind, Length, Padding};
use loom_ui_core::{Point, Rect, Size, TextMeasure, TextStyle, UiId};
use std::collections::HashMap;
use taffy::style_helpers::line;
use taffy::{
    AlignContent, AlignItems, AvailableSpace, Dimension, Display, FlexDirection, FlexWrap, LengthPercentage,
    LengthPercentageAuto, NodeId, Style, TaffyTree,
};

/// How many sizes a wrapping text keeps: the narrowest it can be, its one
/// line, and the width it was given.
const WRAPPED_SIZES: usize = 3;

/// The laid-out tree of one panel.
pub struct Layout {
    /// The panel's key, the first key of every UI ID in it.
    panel: UiId,
    tree: TaffyTree<Leaf>,
    root: Option<Root>,
    /// Every element, in tree order: parent before children, siblings in
    /// the order built.
    records: Vec<Record>,
    /// Where each UI ID's element is in `records`.
    by_id: HashMap<UiId, usize>,
    /// Elements whose UI ID an earlier element already had.
    duplicates: usize,
    /// Panic on a duplicate UI ID. On in a debug build; a test turns it off
    /// to check what a release build does.
    panic_on_duplicate: bool,
    /// The panel's size, after [`Layout::compute`].
    size: Size,
}

/// The root element's node and its own sizes, which depend on the space
/// the panel is given.
struct Root {
    node: NodeId,
    width: Length,
    height: Length,
}

/// One element, as the layout keeps it.
pub(crate) struct Record {
    node: NodeId,
    /// The element around it, in `records`.
    pub(crate) parent: Option<usize>,
    /// Its UI ID, if it has a key and no earlier element took the ID.
    pub(crate) id: Option<UiId>,
    /// It takes the pointer even where no keyed element is.
    pub(crate) opaque: bool,
    /// Where it is in the panel, after [`Layout::compute`].
    pub(crate) bounds: Rect,
}

/// The text of a text element, kept in its node, with the sizes measured
/// since the panel last rebuilt.
pub(crate) struct Leaf {
    text: String,
    style: TextStyle,
    wrap: bool,
    /// The size on one line, for text that does not wrap.
    line: Option<Size>,
    /// The last sizes of wrapping text, by the width it was given (`None`
    /// for no limit), oldest first.
    wrapped: [Option<(Option<u32>, Size)>; WRAPPED_SIZES],
}

impl Leaf {
    /// Its size for what taffy asks: a width it must have, or the space it
    /// may take. Measures only on a size not kept yet.
    fn size(
        &mut self,
        known: taffy::Size<Option<f32>>,
        available: taffy::Size<AvailableSpace>,
        measure: &mut dyn TextMeasure,
    ) -> Size {
        if !self.wrap {
            return *self
                .line
                .get_or_insert_with(|| measure.measure(&self.text, self.style, None));
        }
        // The narrowest it can be is its longest word, which is what
        // breaking at every space gives.
        let width = known.width.or(match available.width {
            AvailableSpace::Definite(width) => Some(width),
            AvailableSpace::MinContent => Some(0.0),
            AvailableSpace::MaxContent => None,
        });
        let key = width.map(f32::to_bits);
        if let Some((_, size)) = self.wrapped.iter().flatten().find(|(kept, _)| *kept == key) {
            return *size;
        }
        let size = measure.measure(&self.text, self.style, width);
        self.wrapped.rotate_left(1);
        self.wrapped[WRAPPED_SIZES - 1] = Some((key, size));
        size
    }
}

/// Which way the element around a child lays its children out, which says
/// which of the child's sizes grows and which stretches.
#[derive(Clone, Copy)]
enum Parent {
    /// A row: width is the main axis.
    Row,
    /// A column: height is the main axis.
    Column,
    /// A stack, or no parent: both sizes stretch.
    Stack,
}

impl Layout {
    /// An empty layout for the panel keyed `panel`.
    pub fn new(panel: UiId) -> Self {
        Self {
            panel,
            tree: TaffyTree::new(),
            root: None,
            records: Vec::new(),
            by_id: HashMap::new(),
            duplicates: 0,
            panic_on_duplicate: cfg!(debug_assertions),
            size: Size::ZERO,
        }
    }

    /// Takes the panel's new tree. The old tree's nodes are dropped, and
    /// their storage is kept for the new ones. Call [`Layout::compute`]
    /// before reading bounds.
    ///
    /// # Panics
    ///
    /// In a debug build, if two elements have the same UI ID.
    pub fn rebuild(&mut self, root: Element) {
        self.tree.clear();
        self.records.clear();
        self.by_id.clear();
        self.duplicates = 0;
        let (width, height) = (root.width, root.height);
        let scope = self.panel.clone();
        let node = self.add(root, None, &scope, Parent::Stack, false);
        self.root = Some(Root { node, width, height });
    }

    /// Lays the tree out in `available`, the space the panel has. The root
    /// takes all of it along an axis where its size is [`Length::Fill`].
    pub fn compute(&mut self, available: Size, measure: &mut dyn TextMeasure) {
        let Some(root) = &self.root else {
            self.size = Size::ZERO;
            return;
        };
        let mut style = self.tree.style(root.node).expect("the root is in the tree").clone();
        style.size = taffy::Size {
            width: root_size(root.width, available.width),
            height: root_size(root.height, available.height),
        };
        if *self.tree.style(root.node).expect("the root is in the tree") != style {
            self.tree.set_style(root.node, style).expect("the root is in the tree");
        }
        let space = taffy::Size {
            width: AvailableSpace::Definite(available.width),
            height: AvailableSpace::Definite(available.height),
        };
        self.tree
            .compute_layout_with_measure(root.node, space, |inputs, _, leaf, style| {
                taffy::compute_leaf_layout(
                    inputs,
                    style,
                    |_, _| 0.0,
                    |known, space| match leaf {
                        Some(leaf) => {
                            let size = leaf.size(known, space, measure);
                            taffy::Size {
                                width: size.width,
                                height: size.height,
                            }
                        }
                        None => taffy::Size::ZERO,
                    },
                )
            })
            .expect("every node is in the tree");
        // Parents come before their children, so each parent's place is
        // known when its children are placed.
        for index in 0..self.records.len() {
            let layout = self
                .tree
                .layout(self.records[index].node)
                .expect("every node is in the tree");
            let origin = match self.records[index].parent {
                Some(parent) => self.records[parent].bounds.position(),
                None => Point::ORIGIN,
            };
            self.records[index].bounds = Rect::new(
                Point::new(origin.x + layout.location.x, origin.y + layout.location.y),
                Size::new(layout.size.width, layout.size.height),
            );
        }
        self.size = self.records.first().map_or(Size::ZERO, |root| root.bounds.size());
    }

    /// The panel's key.
    pub fn panel(&self) -> &UiId {
        &self.panel
    }

    /// The panel's size, as last computed.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Where the element with UI ID `id` is in the panel, as last computed.
    /// `None` when no element has it.
    pub fn bounds(&self, id: &UiId) -> Option<Rect> {
        self.by_id.get(id).map(|index| self.records[*index].bounds)
    }

    /// Every UI ID in the panel, in tree order. A table that keeps state by
    /// UI ID drops what is not here.
    pub fn ids(&self) -> impl Iterator<Item = &UiId> {
        self.records.iter().filter_map(|record| record.id.as_ref())
    }

    /// How many keys were dropped: an element whose UI ID an earlier
    /// element already had, and each keyed element inside one.
    pub fn duplicate_keys(&self) -> usize {
        self.duplicates
    }

    /// Every element, in tree order, for hit-testing.
    pub(crate) fn records(&self) -> &[Record] {
        &self.records
    }

    /// Turns off the panic on a duplicate UI ID, as in a release build.
    #[cfg(test)]
    pub(crate) fn allow_duplicates(&mut self) {
        self.panic_on_duplicate = false;
    }

    /// Adds `element` and its children under the record `parent`, with
    /// `scope` the UI ID of the nearest keyed element around it, and
    /// returns its node. `lost` is true inside an element whose UI ID an
    /// earlier element had: nothing in there gets a UI ID, since each would
    /// name a place inside the earlier element.
    fn add(&mut self, element: Element, parent: Option<usize>, scope: &UiId, around: Parent, lost: bool) -> NodeId {
        let style = style(&element, around);
        let inside = match element.kind {
            Kind::Row => Parent::Row,
            Kind::Column => Parent::Column,
            _ => Parent::Stack,
        };
        let Element {
            kind,
            key,
            opaque,
            children,
            ..
        } = element;
        let node = match kind {
            Kind::Text {
                text,
                style: text_style,
                wrap,
            } => {
                let leaf = Leaf {
                    text,
                    style: text_style,
                    wrap,
                    line: None,
                    wrapped: [None; WRAPPED_SIZES],
                };
                self.tree.new_leaf_with_context(style, leaf)
            }
            _ => self.tree.new_leaf(style),
        }
        .expect("a new node always fits");

        let index = self.records.len();
        let id = key.map(|key| scope.clone().with(key));
        let mut kept = None;
        let mut lost = lost;
        if let Some(id) = &id {
            if lost {
                // A key inside a lost element is lost with it.
                self.duplicates += 1;
            } else if self.by_id.contains_key(id) {
                assert!(!self.panic_on_duplicate, "two elements have the UI ID {id}");
                self.duplicates += 1;
                lost = true;
            } else {
                self.by_id.insert(id.clone(), index);
                kept = Some(id.clone());
            }
        }
        self.records.push(Record {
            node,
            parent,
            id: kept,
            opaque,
            bounds: Rect::new(Point::ORIGIN, Size::ZERO),
        });

        if !children.is_empty() {
            let inner = id.as_ref().unwrap_or(scope).clone();
            let nodes: Vec<NodeId> = children
                .into_iter()
                .map(|child| self.add(child, Some(index), &inner, inside, lost))
                .collect();
            self.tree.set_children(node, &nodes).expect("the nodes were just added");
        }
        node
    }
}

/// The root's size along one axis, in `available` space.
fn root_size(length: Length, available: f32) -> Dimension {
    match length {
        Length::Fill(_) => Dimension::length(available),
        Length::Shrink => Dimension::auto(),
        Length::Fixed(size) => Dimension::length(size),
    }
}

/// The taffy style for `element`, inside a parent that lays out as
/// `around`. Only what `element` sets is set; the rest is flexbox's
/// default.
fn style(element: &Element, around: Parent) -> Style {
    let mut style = Style::default();
    match element.kind {
        Kind::Row => {
            style.display = Display::Flex;
            style.flex_direction = FlexDirection::Row;
            style.justify_content = element.align_x.map_or(AlignContent::NORMAL, content);
            style.align_items = element.align_y.map_or(AlignItems::NORMAL, items);
        }
        Kind::Column => {
            style.display = Display::Flex;
            style.flex_direction = FlexDirection::Column;
            style.justify_content = element.align_y.map_or(AlignContent::NORMAL, content);
            style.align_items = element.align_x.map_or(AlignItems::NORMAL, items);
        }
        Kind::Stack => {
            style.display = Display::Grid;
            style.justify_items = element.align_x.map_or(AlignItems::NORMAL, items);
            style.align_items = element.align_y.map_or(AlignItems::NORMAL, items);
        }
        Kind::Text { .. } | Kind::Space => {}
    }
    if element.wrap {
        style.flex_wrap = FlexWrap::Wrap;
    }
    style.gap = taffy::Size {
        width: LengthPercentage::length(element.spacing),
        height: LengthPercentage::length(element.spacing),
    };
    style.padding = padding(element.padding);
    style.min_size = taffy::Size {
        width: limit(element.min.0),
        height: limit(element.min.1),
    };
    style.max_size = taffy::Size {
        width: limit(element.max.0),
        height: limit(element.max.1),
    };
    style.size = taffy::Size {
        width: dimension(element.width),
        height: dimension(element.height),
    };

    // A fill size grows along its parent's main axis and stretches across
    // it. In a stack, both sizes stretch, each in its own direction.
    let fills = |length: Length| matches!(length, Length::Fill(_));
    match around {
        Parent::Row => {
            if let Length::Fill(share) = element.width {
                style.flex_grow = f32::from(share.get());
            }
            if fills(element.height) {
                style.align_self = Some(AlignItems::STRETCH);
            }
        }
        Parent::Column => {
            if let Length::Fill(share) = element.height {
                style.flex_grow = f32::from(share.get());
            }
            if fills(element.width) {
                style.align_self = Some(AlignItems::STRETCH);
            }
        }
        Parent::Stack => {
            if fills(element.width) {
                style.justify_self = Some(AlignItems::STRETCH);
            }
            if fills(element.height) {
                style.align_self = Some(AlignItems::STRETCH);
            }
            // Every child of a stack is in its one cell.
            style.grid_row = line(1);
            style.grid_column = line(1);
        }
    }
    style
}

/// A size as taffy takes it: a fill size is grown or stretched (see
/// [`style`]), so it starts from its content, as a shrink size does.
fn dimension(length: Length) -> Dimension {
    match length {
        Length::Fixed(size) => Dimension::length(size),
        Length::Fill(_) | Length::Shrink => Dimension::auto(),
    }
}

/// A minimum or a maximum, or none.
fn limit(size: Option<f32>) -> LengthPercentageAuto {
    size.map_or(LengthPercentageAuto::auto(), LengthPercentageAuto::length)
}

/// Padding as taffy takes it.
fn padding(padding: Padding) -> taffy::Rect<LengthPercentage> {
    taffy::Rect {
        left: LengthPercentage::length(padding.left),
        right: LengthPercentage::length(padding.right),
        top: LengthPercentage::length(padding.top),
        bottom: LengthPercentage::length(padding.bottom),
    }
}

/// Alignment of children along the main axis.
fn content(align: Align) -> AlignContent {
    match align {
        Align::Start => AlignContent::START,
        Align::Center => AlignContent::CENTER,
        Align::End => AlignContent::END,
    }
}

/// Alignment of children across the main axis, or in a stack's cell.
fn items(align: Align) -> AlignItems {
    match align {
        Align::Start => AlignItems::START,
        Align::Center => AlignItems::CENTER,
        Align::End => AlignItems::END,
    }
}

#[cfg(test)]
pub(crate) mod tests;
