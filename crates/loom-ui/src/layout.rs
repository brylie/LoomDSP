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

use crate::access::{Access, Problem};
use crate::element::{Align, Element, Kind, Length, Look, Padding};
use loom_ui_core::{Color, Point, Rect, Size, TextMeasure, TextStyle, UiId, Vector};
use std::collections::HashMap;
use taffy::style_helpers::line;
use taffy::{
    AlignContent, AlignItems, AvailableSpace, Dimension, Display, FlexDirection, FlexWrap, LengthPercentage,
    LengthPercentageAuto, NodeId, Overflow, Style, TaffyTree,
};

mod scroll;

pub(crate) use scroll::{BarPart, ScrollState};

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
    /// Panic on a duplicate UI ID, and on a focus stop or an accessed
    /// element that breaks a rule. On in a debug build; a test turns it off
    /// to check what a release build does.
    panic_on_duplicate: bool,
    /// The panel's size, after [`Layout::compute`].
    size: Size,
    /// The slots, in tree order: the panel each names, and its record.
    slots: Vec<(UiId, usize)>,
    /// Focus stops and accessed elements that break a rule, in tree order:
    /// its UI ID where it has one, and what is wrong.
    problems: Vec<(Option<UiId>, Problem)>,
    /// How far each scroll container is scrolled, by its UI ID: widget
    /// state the runtime keeps across rebuilds (ui-layer.md, section 5,
    /// rule 12). It goes with the container.
    offsets: HashMap<UiId, f32>,
}

/// The space offered along one axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Space {
    /// At most this many logical pixels.
    Definite(f32),
    /// As narrow as the content can be.
    MinContent,
    /// As wide as the content wants.
    MaxContent,
}

/// The space offered to a panel along both axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Constraint {
    /// Across.
    pub(crate) width: Space,
    /// Down.
    pub(crate) height: Space,
}

impl Constraint {
    /// Exactly `size`, as a window or a slot gives it.
    pub(crate) fn within(size: Size) -> Self {
        Self {
            width: Space::Definite(size.width),
            height: Space::Definite(size.height),
        }
    }
}

/// What layout asks for sizes it cannot work out alone: the size of a
/// text, and the size of the panel in a shrink slot.
pub(crate) trait Measure {
    /// The size of `text` in `style`, wrapped at `wrap_width` or on one
    /// line, as [`TextMeasure::measure`] gives it.
    fn text(&mut self, text: &str, style: TextStyle, wrap_width: Option<f32>) -> Size;
    /// The size of the panel keyed `panel`, in slot number `index` of the
    /// panel keyed `owner`, in `constraint`.
    fn slot(&mut self, owner: &UiId, panel: &UiId, index: usize, constraint: Constraint) -> Size;
}

/// A measure for one panel on its own: text from a [`TextMeasure`], and a
/// slot has no size, since no other panel is there.
struct Alone<'a>(&'a mut dyn TextMeasure);

impl Measure for Alone<'_> {
    fn text(&mut self, text: &str, style: TextStyle, wrap_width: Option<f32>) -> Size {
        self.0.measure(text, style, wrap_width)
    }

    fn slot(&mut self, _owner: &UiId, _panel: &UiId, _index: usize, _constraint: Constraint) -> Size {
        Size::ZERO
    }
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
    /// It is a focus stop. Only an element with a UI ID is one.
    pub(crate) focusable: bool,
    /// What it is to a screen reader. Only an element with a UI ID has it.
    pub(crate) access: Option<Access>,
    /// Its hover text. Only an element with a UI ID has it.
    pub(crate) hover: Option<String>,
    /// The panel this record's slot holds, for a slot.
    pub(crate) slot: Option<UiId>,
    /// Where it is in the panel, after [`Layout::compute`].
    pub(crate) bounds: Rect,
    /// How it is drawn: its box, and whether it clips what is inside.
    pub(crate) look: Look,
    /// Its content box: inside its border and padding, where its text and
    /// a slot's panel go. Rounded as taffy places a child there, so text
    /// sits where a child at that edge would.
    pub(crate) content: Rect,
    /// Where its corner is before rounding, in the panel: the content box
    /// is rounded from it.
    unrounded: Point,
    /// Its content width before rounding: the width a wrapping text was
    /// measured at, so it wraps there when painted too. The rounded width
    /// could break a line the measure kept whole, and the extra line would
    /// spill out of its box.
    pub(crate) wrap_width: f32,
    /// What of it shows, cut by every clipping element around it, in the
    /// panel. `None` when nothing around it clips.
    pub(crate) clip: Option<Rect>,
    /// For a scroll container: its style, and how far it is scrolled.
    pub(crate) scroll: Option<ScrollState>,
    /// For a radio group, a radio, or a radio's mark: which.
    pub(crate) radio: Option<RadioPart>,
}

/// The part of a radio group a record is (ui-layer.md, section 6, rule 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RadioPart {
    /// The group.
    Group,
    /// One radio: the record of the group around it, if any, and whether
    /// it is the chosen one.
    Radio {
        /// The group's record.
        group: Option<usize>,
        /// It is chosen.
        selected: bool,
    },
    /// A radio's round mark.
    Mark {
        /// Its radio is chosen.
        selected: bool,
    },
}

impl Record {
    /// The clip for what is inside it: its own bounds when it clips, cut
    /// by the clip around it.
    pub(crate) fn inner_clip(&self) -> Option<Rect> {
        if !self.look.clip {
            return self.clip;
        }
        Some(match self.clip {
            Some(outer) => cut(outer, self.bounds),
            None => self.bounds,
        })
    }

    /// True when `point` is on the part of it that shows.
    pub(crate) fn shows(&self, point: Point) -> bool {
        self.bounds.contains(point) && self.clip.is_none_or(|clip| clip.contains(point))
    }
}

/// `value` rounded to a whole logical pixel as taffy rounds its layout:
/// halves go up. taffy 0.15 rounds a child's place as
/// `round(unrounded edge + border + padding)`, with `(v + 0.5).floor()`
/// (`compute/mod.rs`, `round_layout`, and `util/sys.rs`); the content box
/// copies that rule. A taffy upgrade that changes it fails
/// `draw::tests::text_sits_on_the_rounded_content_edge`.
fn round(value: f32) -> f32 {
    (value + 0.5).floor()
}

/// The part of `a` inside `b`: an empty rectangle at `b`'s corner when they
/// do not meet, so nothing inside shows or is hit.
pub(crate) fn cut(a: Rect, b: Rect) -> Rect {
    a.intersection(b).unwrap_or(Rect::new(b.position(), Size::ZERO))
}

/// A text element's text, for painting.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TextPaint<'a> {
    /// The text.
    pub(crate) text: &'a str,
    /// How it is set.
    pub(crate) style: TextStyle,
    /// Lines break to fit the width.
    pub(crate) wrap: bool,
    /// Its colour, or `None` for the panel style's.
    pub(crate) colour: Option<Color>,
    /// It is a text field's text, which the field paints from its editor.
    pub(crate) field: bool,
    /// It is a radio's note, in the quiet colour unless it sets its own.
    pub(crate) quiet: bool,
}

/// What a leaf's node holds: a text, or a slot.
pub(crate) enum Leaf {
    /// A text, with what it measured.
    Text(TextLeaf),
    /// The place of another panel.
    Slot {
        /// The panel's key.
        panel: UiId,
        /// Its number among the panel's slots, in tree order.
        index: usize,
        /// Its width shrinks to the panel's, so the panel is asked.
        shrink_width: bool,
        /// Its height shrinks to the panel's, so the panel is asked.
        shrink_height: bool,
    },
}

impl Leaf {
    /// Its size for what taffy asks: a size it must have along an axis, or
    /// the space it may take.
    fn size(
        &mut self,
        owner: &UiId,
        known: taffy::Size<Option<f32>>,
        available: taffy::Size<AvailableSpace>,
        measure: &mut dyn Measure,
    ) -> Size {
        match self {
            Self::Text(text) => text.size(known, available, measure),
            // A slot that fills or is fixed is sized by its parent alone:
            // it has no content, and the panel in it is never asked.
            Self::Slot {
                shrink_width: false,
                shrink_height: false,
                ..
            } => Size::ZERO,
            Self::Slot {
                panel,
                index,
                shrink_width,
                shrink_height,
            } => {
                let space = |known: Option<f32>, available| match (known, available) {
                    (Some(size), _) | (None, AvailableSpace::Definite(size)) => Space::Definite(size),
                    (None, AvailableSpace::MinContent) => Space::MinContent,
                    (None, AvailableSpace::MaxContent) => Space::MaxContent,
                };
                let constraint = Constraint {
                    width: space(known.width, available.width),
                    height: space(known.height, available.height),
                };
                let size = measure.slot(owner, panel, *index, constraint);
                Size::new(
                    if *shrink_width { size.width } else { 0.0 },
                    if *shrink_height { size.height } else { 0.0 },
                )
            }
        }
    }
}

/// The text of a text element, kept in its node, with the sizes measured
/// since the panel last rebuilt.
pub(crate) struct TextLeaf {
    text: String,
    style: TextStyle,
    wrap: bool,
    colour: Option<Color>,
    /// It is a radio's note.
    quiet: bool,
    /// It is a text field's: one line, at least one em wide and one line
    /// tall, so an empty field still has room for its caret and a press.
    field: bool,
    /// The size on one line, for text that does not wrap.
    line: Option<Size>,
    /// The last sizes of wrapping text, by the width it was given (`None`
    /// for no limit), oldest first.
    wrapped: [Option<(Option<u32>, Size)>; WRAPPED_SIZES],
}

impl TextLeaf {
    /// Its size for what taffy asks: a width it must have, or the space it
    /// may take. Measures only on a size not kept yet.
    fn size(
        &mut self,
        known: taffy::Size<Option<f32>>,
        available: taffy::Size<AvailableSpace>,
        measure: &mut dyn Measure,
    ) -> Size {
        if !self.wrap {
            let (text, style, field) = (&self.text, self.style, self.field);
            return *self.line.get_or_insert_with(|| {
                let size = measure.text(text, style, None);
                if field {
                    Size::new(
                        size.width.max(style.size),
                        size.height.max(style.size * style.line_height),
                    )
                } else {
                    size
                }
            });
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
        let size = measure.text(&self.text, self.style, width);
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
    /// A scroll container: a column whose child keeps its own height, so it
    /// never shrinks to fit.
    Scroll,
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
            slots: Vec::new(),
            problems: Vec::new(),
            offsets: HashMap::new(),
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
        self.slots.clear();
        self.problems.clear();
        self.duplicates = 0;
        let (width, height) = (root.width, root.height);
        let scope = self.panel.clone();
        let node = self.add(root, None, &scope, Parent::Stack, false);
        self.root = Some(Root { node, width, height });
        // A container's offset goes with it.
        let (by_id, records) = (&self.by_id, &self.records);
        self.offsets
            .retain(|id, _| by_id.get(id).is_some_and(|index| records[*index].scroll.is_some()));
    }

    /// Lays the tree out in `available`, the space the panel has. The root
    /// takes all of it along an axis where its size is [`Length::Fill`]. A
    /// slot has no size here: a panel laid out alone has no other panel to
    /// put in it.
    pub fn compute(&mut self, available: Size, measure: &mut dyn TextMeasure) {
        self.compute_in(Constraint::within(available), &mut Alone(measure));
    }

    /// Lays the tree out in `constraint`, with `measure` for text and
    /// slots.
    pub(crate) fn compute_in(&mut self, constraint: Constraint, measure: &mut dyn Measure) {
        let Some(root) = &self.root else {
            self.size = Size::ZERO;
            return;
        };
        let mut style = self.tree.style(root.node).expect("the root is in the tree").clone();
        style.size = taffy::Size {
            width: root_size(root.width, constraint.width),
            height: root_size(root.height, constraint.height),
        };
        if *self.tree.style(root.node).expect("the root is in the tree") != style {
            self.tree.set_style(root.node, style).expect("the root is in the tree");
        }
        let available = |space| match space {
            Space::Definite(size) => AvailableSpace::Definite(size),
            Space::MinContent => AvailableSpace::MinContent,
            Space::MaxContent => AvailableSpace::MaxContent,
        };
        let space = taffy::Size {
            width: available(constraint.width),
            height: available(constraint.height),
        };
        let owner = &self.panel;
        self.tree
            .compute_layout_with_measure(root.node, space, |inputs, _, leaf, style| {
                taffy::compute_leaf_layout(
                    inputs,
                    style,
                    |_, _| 0.0,
                    |known, space| match leaf {
                        Some(leaf) => {
                            let size = leaf.size(owner, known, space, measure);
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
        self.place();
        self.size = self.records.first().map_or(Size::ZERO, |root| root.bounds.size());
    }

    /// Places every element from the computed layout, moving what is in a
    /// scroll container up by the container's offset. A scroll needs only
    /// this, not a new layout.
    pub(crate) fn place(&mut self) {
        // Parents come before their children, so each parent's place is
        // known when its children are placed.
        for index in 0..self.records.len() {
            let layout = self
                .tree
                .layout(self.records[index].node)
                .expect("every node is in the tree");
            // What is in a scroll container moves up by its offset.
            let shift = self.records[index]
                .parent
                .and_then(|parent| self.records[parent].scroll)
                .map_or(0.0, |scroll| scroll.offset);
            let origin = match self.records[index].parent {
                Some(parent) => self.records[parent].bounds.position() - Vector::new(0.0, shift),
                None => Point::ORIGIN,
            };
            self.records[index].bounds = Rect::new(
                Point::new(origin.x + layout.location.x, origin.y + layout.location.y),
                Size::new(layout.size.width, layout.size.height),
            );
            // The content box, from the unrounded layout, rounded as taffy
            // rounds a child's place: from the edge inside the border and
            // the padding together, never each on its own, so the content
            // box is where a child at its edge sits.
            let exact = self.tree.unrounded_layout(self.records[index].node);
            let corner = match self.records[index].parent {
                Some(parent) => self.records[parent].unrounded - Vector::new(0.0, shift),
                None => Point::ORIGIN,
            } + Vector::new(exact.location.x, exact.location.y);
            let (border, padding, size) = (exact.border, exact.padding, exact.size);
            let left = round(corner.x + border.left + padding.left);
            let top = round(corner.y + border.top + padding.top);
            let right = round(corner.x + size.width - border.right - padding.right);
            let bottom = round(corner.y + size.height - border.bottom - padding.bottom);
            self.records[index].unrounded = corner;
            self.records[index].wrap_width =
                (size.width - border.left - border.right - padding.left - padding.right).max(0.0);
            self.records[index].content = Rect::new(
                Point::new(left, top),
                Size::new((right - left).max(0.0), (bottom - top).max(0.0)),
            );
            // The parent's clip for what is inside it is known, since the
            // parent came first.
            self.records[index].clip = self.records[index]
                .parent
                .and_then(|parent| self.records[parent].inner_clip());
            // A container's offset stays inside what its content allows,
            // which may have shrunk since it was set.
            if self.records[index].scroll.is_some() {
                let most = self
                    .tree
                    .layout(self.records[index].node)
                    .expect("every node is in the tree")
                    .scroll_height()
                    .round();
                let id = self.records[index].id.clone();
                let wanted = id.as_ref().and_then(|id| self.offsets.get(id)).copied().unwrap_or(0.0);
                let offset = wanted.clamp(0.0, most);
                if let Some(scroll) = &mut self.records[index].scroll {
                    scroll.offset = offset;
                    scroll.most = most;
                }
                if let Some(id) = id {
                    self.offsets.insert(id, offset);
                }
            }
        }
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

    /// Each slot's panel and its place in this panel, in tree order; a
    /// slot's number is its place in this order.
    ///
    /// A slot's place is its content box: inside its border and padding, as
    /// CSS puts a child, so a framed slot's panel never covers its frame.
    pub(crate) fn slots(&self) -> impl Iterator<Item = (&UiId, Rect)> {
        self.slots
            .iter()
            .map(|(panel, index)| (panel, self.records[*index].content))
    }

    /// The part of each slot that shows, in tree order: its place cut by
    /// every clipping element around it, in this panel.
    pub(crate) fn slot_clips(&self) -> impl Iterator<Item = Rect> {
        self.slots.iter().map(|(_, index)| {
            let record = &self.records[*index];
            record.clip.map_or(record.content, |clip| cut(clip, record.content))
        })
    }

    /// Forgets the layout around the slot for `panel`, whose size changed,
    /// so the next compute asks it again.
    pub(crate) fn slot_changed(&mut self, panel: &UiId) {
        if let Some((_, index)) = self.slots.iter().find(|(named, _)| named == panel) {
            self.tree
                .mark_dirty(self.records[*index].node)
                .expect("every node is in the tree");
        }
    }

    /// How many elements the panel has.
    pub(crate) fn element_count(&self) -> usize {
        self.records.len()
    }

    /// True when the element with UI ID `id` has a node: it has an
    /// [`Access`].
    pub(crate) fn has_node(&self, id: &UiId) -> bool {
        self.by_id
            .get(id)
            .is_some_and(|index| self.records[*index].access.is_some())
    }

    /// Every element, in tree order, for hit-testing and painting.
    pub(crate) fn records(&self) -> &[Record] {
        &self.records
    }

    /// The text of the record at `index`, when it is a text element.
    pub(crate) fn text(&self, index: usize) -> Option<TextPaint<'_>> {
        match self.tree.get_node_context(self.records[index].node)? {
            Leaf::Text(leaf) => Some(TextPaint {
                text: &leaf.text,
                style: leaf.style,
                wrap: leaf.wrap,
                colour: leaf.colour,
                field: leaf.field,
                quiet: leaf.quiet,
            }),
            Leaf::Slot { .. } => None,
        }
    }

    /// Focus stops and accessed elements that break a rule, in tree order:
    /// one with no key ([`Problem::NoKey`]), a stop with no [`Access`]
    /// ([`Problem::StopWithoutAccess`]), a slot with one
    /// ([`Problem::SlotWithAccess`]), and an [`Access`] that breaks section
    /// 7, rule 2. A debug build panics on the first instead, so this
    /// is for a release build and for a test that checks every panel.
    pub fn access_problems(&self) -> &[(Option<UiId>, Problem)] {
        &self.problems
    }

    /// Notes a problem, and panics on it in a debug build.
    fn problem(&mut self, id: Option<UiId>, problem: Problem) {
        let at = id
            .as_ref()
            .map_or_else(|| format!("in panel {}", self.panel), ToString::to_string);
        assert!(!self.panic_on_duplicate, "an element {at} breaks a rule: {problem}");
        self.problems.push((id, problem));
    }

    /// Turns off the panic on a duplicate UI ID, and on a focus stop or an
    /// accessed element that breaks a rule, as in a release build. For
    /// tests, and for a window whose own checks are off.
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
        let shrinks = (element.width == Length::Shrink, element.height == Length::Shrink);
        let inside = match element.kind {
            Kind::Row | Kind::Radio => Parent::Row,
            Kind::Column | Kind::RadioGroup => Parent::Column,
            Kind::Scroll(_) => Parent::Scroll,
            _ => Parent::Stack,
        };
        let scrollbar = match element.kind {
            Kind::Scroll(bar) => Some(bar),
            _ => None,
        };
        let radio = match element.kind {
            Kind::RadioGroup => Some(RadioPart::Group),
            Kind::Radio => Some(RadioPart::Radio {
                group: self.group_around(parent),
                selected: matches!(&element.access, Some(access) if access.is_on()),
            }),
            Kind::RadioMark { selected } => Some(RadioPart::Mark { selected }),
            _ => None,
        };
        let Element {
            kind,
            key,
            opaque,
            look,
            focusable,
            access,
            hover,
            children,
            ..
        } = element;
        let slot = match &kind {
            Kind::Slot(panel) => Some(panel.clone()),
            _ => None,
        };
        let node = match kind {
            Kind::Text {
                text,
                style: text_style,
                wrap,
                colour,
                quiet,
            } => {
                let leaf = Leaf::Text(TextLeaf {
                    text,
                    style: text_style,
                    wrap,
                    colour,
                    quiet,
                    field: false,
                    line: None,
                    wrapped: [None; WRAPPED_SIZES],
                });
                self.tree.new_leaf_with_context(style, leaf)
            }
            Kind::Field {
                text,
                style: text_style,
                many,
            } => {
                // A text area wraps to its width, as a wrapping text does, so
                // it grows with its lines.
                let leaf = Leaf::Text(TextLeaf {
                    text,
                    style: text_style,
                    wrap: many,
                    colour: None,
                    quiet: false,
                    field: true,
                    line: None,
                    wrapped: [None; WRAPPED_SIZES],
                });
                self.tree.new_leaf_with_context(style, leaf)
            }
            Kind::Slot(panel) => {
                let index = self.slots.len();
                self.slots.push((panel.clone(), self.records.len()));
                let leaf = Leaf::Slot {
                    panel,
                    index,
                    shrink_width: shrinks.0,
                    shrink_height: shrinks.1,
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
        // A scroll container keeps its offset by its UI ID, so it needs one.
        if (focusable || access.is_some() || scrollbar.is_some()) && id.is_none() {
            self.problem(None, Problem::NoKey);
        }
        let hover = hover.filter(|_| kept.is_some());
        // The hover text is the node's description unless it sets its own
        // (section 7, rule 23): one string for both. Text that only repeats
        // the node's name or value is not read a second time.
        let access = access.filter(|_| kept.is_some()).map(|access| match &hover {
            Some(text) if !access.has_description() && !access.repeats(text) => access.description(text.clone()),
            _ => access,
        });
        // A scroll container is a stop only when nothing inside is, which
        // is known once its children are added.
        let stop = kept.is_some() && is_stop(focusable, access.as_ref());
        if let Some(problem) = access.as_ref().and_then(|access| access.problem(stop)) {
            self.problem(kept.clone(), problem);
        }
        if stop && access.is_none() {
            self.problem(kept.clone(), Problem::StopWithoutAccess);
        }
        let access = if slot.is_some() && access.is_some() {
            self.problem(kept.clone(), Problem::SlotWithAccess);
            None
        } else {
            access
        };
        self.records.push(Record {
            node,
            parent,
            focusable: stop,
            access,
            hover,
            slot,
            id: kept,
            opaque,
            bounds: Rect::new(Point::ORIGIN, Size::ZERO),
            look,
            content: Rect::new(Point::ORIGIN, Size::ZERO),
            unrounded: Point::ORIGIN,
            wrap_width: 0.0,
            clip: None,
            scroll: scrollbar.map(|style| ScrollState {
                style,
                offset: 0.0,
                most: 0.0,
            }),
            radio,
        });

        if !children.is_empty() {
            let inner = id.as_ref().unwrap_or(scope).clone();
            let nodes: Vec<NodeId> = children
                .into_iter()
                .map(|child| self.add(child, Some(index), &inner, inside, lost))
                .collect();
            self.tree.set_children(node, &nodes).expect("the nodes were just added");
        }
        // A scroll container whose content has no focus stop is one itself,
        // so a keyboard user can scroll it (WCAG 2.2, SC 2.1.1).
        if scrollbar.is_some() && self.records[index].id.is_some() {
            let inside = self.records[index + 1..].iter().any(|record| record.focusable);
            let stop = !inside && is_stop(true, self.records[index].access.as_ref());
            self.records[index].focusable = stop;
            if stop && self.records[index].access.is_none() {
                let id = self.records[index].id.clone();
                self.problem(id, Problem::StopWithoutAccess);
            }
        }
        node
    }

    /// The record of the radio group around the record `parent`, or
    /// itself: the nearest one.
    fn group_around(&self, parent: Option<usize>) -> Option<usize> {
        let mut at = parent;
        while let Some(index) = at {
            if self.records[index].radio == Some(RadioPart::Group) {
                return Some(index);
            }
            at = self.records[index].parent;
        }
        None
    }
}

/// Whether an element its panel marked `focusable`, with `access`, is a
/// focus stop. This is the one place that says so (ui-layer.md, section
/// 6, rule 2). A turned-off element is not one, as on macOS and Windows:
/// Tab passes over it. Its node stays in the tree with its description
/// (section 7, rule 23), and offers no Focus.
fn is_stop(focusable: bool, access: Option<&Access>) -> bool {
    focusable && !access.is_some_and(Access::is_disabled)
}

/// The root's size along one axis, in `space`. A fill root takes a
/// definite space, and sizes to its content when asked how large it is.
fn root_size(length: Length, space: Space) -> Dimension {
    match length {
        Length::Fill(_) => match space {
            Space::Definite(size) => Dimension::length(size),
            Space::MinContent | Space::MaxContent => Dimension::auto(),
        },
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
        Kind::Scroll(bar) => {
            // A column whose child keeps its own height: taffy lets it run
            // past the container, keeps room at the right for the
            // scrollbar, and gives how far it can scroll.
            style.display = Display::Flex;
            style.flex_direction = FlexDirection::Column;
            style.overflow = taffy::Point {
                x: Overflow::Hidden,
                y: Overflow::Scroll,
            };
            style.scrollbar_width = bar.gutter();
        }
        Kind::RadioGroup => {
            style.display = Display::Flex;
            style.flex_direction = FlexDirection::Column;
        }
        Kind::Radio => {
            // The mark at the top, beside the label's first line.
            style.display = Display::Flex;
            style.flex_direction = FlexDirection::Row;
            style.align_items = AlignItems::START;
        }
        Kind::Text { .. } | Kind::Field { .. } | Kind::Space | Kind::Slot(_) | Kind::RadioMark { .. } => {}
    }
    if element.wrap {
        style.flex_wrap = FlexWrap::Wrap;
    }
    style.gap = taffy::Size {
        width: LengthPercentage::length(element.spacing),
        height: LengthPercentage::length(element.spacing),
    };
    style.padding = padding(element.padding);
    // A border takes room, as CSS's does: the content box is inside it.
    let border = LengthPercentage::length(element.look.border_width());
    style.border = taffy::Rect {
        left: border,
        right: border,
        top: border,
        bottom: border,
    };
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
        Parent::Column | Parent::Scroll => {
            if let Length::Fill(share) = element.height {
                style.flex_grow = f32::from(share.get());
            }
            if fills(element.width) {
                style.align_self = Some(AlignItems::STRETCH);
            }
            if matches!(around, Parent::Scroll) {
                style.flex_shrink = 0.0;
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
