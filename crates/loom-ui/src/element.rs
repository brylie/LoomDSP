//! Elements: the tree a panel builds from its model (ui-layer.md, section 4).
//!
//! A panel describes what it shows as a tree of [`Element`]s: rows,
//! columns, stacks, text, and empty space. The tree is built again each
//! time the panel's model changes, and laid out by [`Layout`](crate::Layout).
//!
//! An element that must be found again (to hit it, focus it, or name it in
//! the accessibility tree) gets a key with [`Element::key`]. Its UI ID is
//! the panel's key, then the keys of the keyed elements around it, then its
//! own. An element with no key adds nothing to the path, so wrapping a part
//! in another row never changes its ID. Keys in a list that can change come
//! from what each row shows, never from its position (section 4, rule 3).
//!
//! # Sizes
//!
//! Sizes follow flexbox's defaults, not another toolkit's (decision of
//! 2026-11-14). [`Length`] sets only what it names:
//!
//! - [`Length::Fill`] grows by its share of the space left after every
//!   item's content, so two fill items with different content can end up
//!   of different sizes. Across a row (its height) it stretches to the
//!   row's height.
//! - [`Length::Shrink`] takes its content's size, and shrinks to its
//!   narrowest content when the row is too short.
//! - [`Length::Fixed`] takes that size when there is room, and shrinks too
//!   when there is not, down to its narrowest content.
//!
//! An item never shrinks below its narrowest content (its longest word, or
//! its widest fixed child). Across a row, an item with no alignment set
//! stretches to the row's height, as in flexbox.
//!
//! # Look
//!
//! An element may draw a box: a [`background`](Element::background), a
//! [`border`](Element::border), and a [`radius`](Element::radius). A border
//! takes room, as CSS's does with border-box sizing: the content sits
//! inside the border, then inside the padding. An element that
//! [`clip`](Element::clip)s cuts what is inside it to its bounds, a
//! rectangle even when its corners are round. Text draws in its
//! [`colour`](Element::colour), or in the panel style's text colour
//! ([`PanelStyle`](crate::paint::PanelStyle)).

use crate::access::Access;
use loom_ui_core::{Color, IdKey, TextStyle, UiId};
use std::num::NonZeroU16;

/// How large an element is along one axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    /// Grow by this many shares of the space left over. Along the cross
    /// axis of a row or column, and in a stack, it stretches to fill. A
    /// share is never 0, which would silently mean [`Length::Shrink`].
    Fill(NonZeroU16),
    /// The size of the content.
    Shrink,
    /// This many logical pixels when there is room.
    Fixed(f32),
}

impl Length {
    /// One share of the space left over, the usual fill.
    pub const FILL: Self = Self::Fill(NonZeroU16::MIN);

    /// `shares` shares of the space left over.
    ///
    /// # Panics
    ///
    /// If `shares` is 0, at compile time where it is a constant.
    pub const fn fill(shares: u16) -> Self {
        match NonZeroU16::new(shares) {
            Some(shares) => Self::Fill(shares),
            None => panic!("a fill length has at least one share"),
        }
    }
}

/// Where children sit along one axis of their container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// At the start: the left, or the top.
    Start,
    /// In the middle.
    Center,
    /// At the end: the right, or the bottom.
    End,
}

/// Space inside an element's edges, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Padding {
    /// Above the content.
    pub top: f32,
    /// Right of the content.
    pub right: f32,
    /// Below the content.
    pub bottom: f32,
    /// Left of the content.
    pub left: f32,
}

impl Padding {
    /// The same space on every side.
    pub const fn all(space: f32) -> Self {
        Self {
            top: space,
            right: space,
            bottom: space,
            left: space,
        }
    }

    /// `vertical` above and below, `horizontal` left and right.
    pub const fn symmetric(vertical: f32, horizontal: f32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }
}

impl From<f32> for Padding {
    fn from(space: f32) -> Self {
        Self::all(space)
    }
}

/// How an element is drawn, besides its text: the box it draws and
/// whether it clips. The default draws nothing and clips nothing.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Look {
    /// The colour inside the box.
    pub(crate) background: Option<Color>,
    /// The width and colour of the outline, inside the box's edge.
    pub(crate) border: Option<(f32, Color)>,
    /// The radius of the box's corners.
    pub(crate) radius: f32,
    /// What is inside is cut to the element's bounds.
    pub(crate) clip: bool,
}

impl Look {
    /// True when the element draws a box.
    pub(crate) fn draws_box(&self) -> bool {
        self.background.is_some() || self.border.is_some()
    }

    /// The border's width, or 0.
    pub(crate) fn border_width(&self) -> f32 {
        self.border.map_or(0.0, |(width, _)| width)
    }
}

/// What kind of element it is.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    /// Children side by side, left to right.
    Row,
    /// Children one under another, top to bottom.
    Column,
    /// Children on top of each other, the last on top.
    Stack,
    /// A piece of text.
    Text {
        /// The text.
        text: String,
        /// How it is set.
        style: TextStyle,
        /// Break lines between words to fit the width it gets.
        wrap: bool,
        /// The colour of its letters, or the panel style's.
        colour: Option<Color>,
    },
    /// A single-line text field: the app's text, edited in place
    /// ([`crate::field`]).
    Field {
        /// The text the app holds.
        text: String,
        /// How it is set.
        style: TextStyle,
    },
    /// Empty space.
    Space,
    /// The place of another panel, named by its key.
    Slot(UiId),
}

/// One node of a panel's tree. Build it with [`Element::row`],
/// [`Element::column`], [`Element::stack`], [`Element::text`], or
/// [`Element::space`], then set what differs from the defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub(crate) kind: Kind,
    pub(crate) key: Option<IdKey>,
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) min: (Option<f32>, Option<f32>),
    pub(crate) max: (Option<f32>, Option<f32>),
    pub(crate) padding: Padding,
    pub(crate) spacing: f32,
    pub(crate) align_x: Option<Align>,
    pub(crate) align_y: Option<Align>,
    pub(crate) wrap: bool,
    pub(crate) opaque: bool,
    pub(crate) look: Look,
    pub(crate) focusable: bool,
    pub(crate) access: Option<Access>,
    pub(crate) children: Vec<Element>,
}

impl Element {
    /// An element of `kind` with every default: shrink to content, no key.
    fn new(kind: Kind, children: Vec<Element>) -> Self {
        Self {
            kind,
            key: None,
            width: Length::Shrink,
            height: Length::Shrink,
            min: (None, None),
            max: (None, None),
            padding: Padding::default(),
            spacing: 0.0,
            align_x: None,
            align_y: None,
            wrap: false,
            opaque: false,
            look: Look::default(),
            focusable: false,
            access: None,
            children,
        }
    }

    /// `children` side by side, left to right.
    pub fn row(children: impl IntoIterator<Item = Element>) -> Self {
        Self::new(Kind::Row, children.into_iter().collect())
    }

    /// `children` one under another, top to bottom.
    pub fn column(children: impl IntoIterator<Item = Element>) -> Self {
        Self::new(Kind::Column, children.into_iter().collect())
    }

    /// `children` on top of each other, each laid out in the whole stack,
    /// the last on top. The stack is as large as its largest child.
    pub fn stack(children: impl IntoIterator<Item = Element>) -> Self {
        Self::new(Kind::Stack, children.into_iter().collect())
    }

    /// `text`, set in `style`, on one line.
    pub fn text(text: impl Into<String>, style: TextStyle) -> Self {
        Self::new(
            Kind::Text {
                text: text.into(),
                style,
                wrap: false,
                colour: None,
            },
            Vec::new(),
        )
    }

    /// A single-line text field showing `text`, the text the app holds,
    /// set in `style`. It is a focus stop, unless it is turned off, and a
    /// screen reader meets it as a text input: give it its name with
    /// [`Element::access`] and `Role::TextInput`. It needs a key.
    ///
    /// The field edits in place and tells the app each change through
    /// [`App::field_event`](crate::App::field_event). The text stays the
    /// app's: whenever a layout shows a text other than the field's, the
    /// field takes it, so an edit the app does not take goes back
    /// (ui-layer.md, section 8, rule 2).
    pub fn field(text: impl Into<String>, style: TextStyle) -> Self {
        let mut field = Self::new(
            Kind::Field {
                text: text.into(),
                style,
            },
            Vec::new(),
        );
        field.focusable = true;
        field.look.clip = true;
        field
    }

    /// Empty space, `width` by `height`.
    pub fn space(width: Length, height: Length) -> Self {
        Self::new(Kind::Space, Vec::new()).width(width).height(height)
    }

    /// The place of the panel keyed `panel`, which is laid out inside it
    /// (ui-layer.md, section 5, rule 8). Its own sizes size it. Filling or
    /// fixed, the parent alone sizes it; shrinking, it asks the panel how
    /// large it is. It has no UI ID of its own: what is in it is the
    /// other panel's.
    pub fn slot(panel: UiId) -> Self {
        Self::new(Kind::Slot(panel), Vec::new())
    }

    /// Names the element, so it can be found again across rebuilds.
    pub fn key(mut self, key: impl Into<IdKey>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Its width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Its height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// The narrowest it may get.
    pub fn min_width(mut self, width: f32) -> Self {
        self.min.0 = Some(width);
        self
    }

    /// The shortest it may get.
    pub fn min_height(mut self, height: f32) -> Self {
        self.min.1 = Some(height);
        self
    }

    /// The widest it may get.
    pub fn max_width(mut self, width: f32) -> Self {
        self.max.0 = Some(width);
        self
    }

    /// The tallest it may get.
    pub fn max_height(mut self, height: f32) -> Self {
        self.max.1 = Some(height);
        self
    }

    /// Space inside its edges.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Space between its children, and between the lines of a wrapping
    /// row.
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Where its children sit from left to right. Unset, a row packs them
    /// at the start, and a column and a stack stretch them.
    pub fn align_x(mut self, align: Align) -> Self {
        self.align_x = Some(align);
        self
    }

    /// Where its children sit from top to bottom. Unset, a column packs
    /// them at the start, and a row and a stack stretch them.
    pub fn align_y(mut self, align: Align) -> Self {
        self.align_y = Some(align);
        self
    }

    /// For a row or a column, starts a new line when the children do not
    /// fit. For text, breaks lines between words to fit the width it gets.
    pub fn wrap(mut self) -> Self {
        if let Kind::Text { wrap, .. } = &mut self.kind {
            *wrap = true;
        } else {
            self.wrap = true;
        }
        self
    }

    /// Draws a box of `colour` over its bounds, under its children.
    pub fn background(mut self, colour: Color) -> Self {
        self.look.background = Some(colour);
        self
    }

    /// Draws an outline `width` wide, inside its bounds. The outline takes
    /// room: what is inside sits within it, then within the padding.
    pub fn border(mut self, width: f32, colour: Color) -> Self {
        self.look.border = Some((width, colour));
        self
    }

    /// Rounds the corners of its box by `radius`.
    pub fn radius(mut self, radius: f32) -> Self {
        self.look.radius = radius;
        self
    }

    /// Cuts what is inside it to its bounds, when drawn and when hit. The
    /// cut is a rectangle, even when the corners are round: a GPU scissor
    /// cannot round.
    pub fn clip(mut self) -> Self {
        self.look.clip = true;
        self
    }

    /// For text, the colour of its letters. Unset, it is the panel style's
    /// text colour. Other elements have no letters, so a debug build panics
    /// on one.
    pub fn colour(mut self, colour: Color) -> Self {
        match &mut self.kind {
            Kind::Text { colour: set, .. } => *set = Some(colour),
            _ => debug_assert!(false, "only text has a colour"),
        }
        self
    }

    /// Makes it a focus stop: Tab and Shift+Tab reach it, a press on it
    /// focuses it, and its node offers the Focus action (ui-layer.md,
    /// section 6). Focus is by UI ID, so it needs a key.
    ///
    /// # Panics
    ///
    /// In a debug build, when the panel is laid out, if it has no key.
    pub fn focusable(mut self) -> Self {
        self.focusable = true;
        self
    }

    /// What it is to a screen reader: its node comes from this element, so
    /// the node and the box laid out cannot disagree (section 7, rule 1).
    /// A node needs a UI ID, so it needs a key.
    ///
    /// # Panics
    ///
    /// In a debug build, when the panel is laid out, if it has no key or
    /// `access` breaks section 7, rule 2 ([`Access::problem`]).
    pub fn access(mut self, access: Access) -> Self {
        self.access = Some(access);
        self
    }

    /// Takes the pointer where it covers, even where no keyed element is,
    /// so nothing under it is hit: a panel over the layer below.
    pub fn opaque(mut self) -> Self {
        self.opaque = true;
        self
    }
}

#[cfg(test)]
mod tests;
