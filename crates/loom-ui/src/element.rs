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
//!
//! # Rich text
//!
//! [`Element::rich`] is one wrapping paragraph made of [`Inline`] pieces,
//! each in its own weight, face, and colour. A piece that is a link is a
//! part of its own: keyed, a focus stop, hit where its words are, with a
//! ring around them, and its own node for a screen reader. Its place comes
//! from where the paragraph sets its words, one rectangle per line it is
//! on, so a link that wraps is hit on both lines and nowhere between.

use crate::access::Access;
use loom_ui_core::{Color, Face, IdKey, Key, Point, Span, TextStyle, UiId, Weight};
use std::num::NonZeroU16;
use std::ops::Range;

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
    /// Its look in each state, for an element that can be pressed: chosen
    /// when it is painted, so a hover or a press redraws it without
    /// building its panel again.
    pub(crate) states: Option<Looks>,
    /// A soft shadow under its box (section 5, rule 15).
    pub(crate) shadow: Option<ShadowStyle>,
}

/// A soft shadow under an element's box, from an app's token (section 5,
/// rule 15): its colour, how far it is moved from the box, and how wide
/// its edge fades.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowStyle {
    /// Its colour, alpha included.
    pub colour: Color,
    /// How far it is moved from the box, in logical pixels.
    pub offset: loom_ui_core::Vector,
    /// How wide its edge fades, in logical pixels.
    pub blur: f32,
}

impl Look {
    /// True when the element draws a box.
    pub(crate) fn draws_box(&self) -> bool {
        self.background.is_some() || self.border.is_some() || self.states.is_some()
    }

    /// The border's width, or 0.
    pub(crate) fn border_width(&self) -> f32 {
        self.border.map_or(0.0, |(width, _)| width)
    }
}

/// How a scroll container looks and moves (ui-layer.md, section 5, rule
/// 12). The app fills it from its style tokens, so `loom-ui` chooses no
/// size or colour of its own. It is given with the container, not at
/// paint, because layout needs it first: the gutter the bar takes and the
/// room the focus ring keeps are part of where the content goes.
///
/// The bar is a thumb on the right, drawn whenever the content is taller
/// than the container. No track is drawn: the panel behind it is the
/// track, so the thumb's colour needs a contrast of 3:1 or more against it
/// (WCAG 2.2, SC 1.4.11).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollStyle {
    /// The thumb's width, in logical pixels.
    pub width: f32,
    /// The room between the thumb and the container's edges.
    pub inset: f32,
    /// The radius of the thumb's corners.
    pub radius: f32,
    /// The thumb's colour.
    pub thumb: Color,
    /// How far the focus ring reaches outside an element: its offset and
    /// its width. The content keeps this much room on every side inside
    /// the clip, so a focused element at the edge keeps its whole ring.
    pub ring: f32,
    /// One line of the panel's body text, in logical pixels: the up and
    /// down arrows move this far, and a wheel's line three times this.
    pub line: f32,
}

impl ScrollStyle {
    /// The width the container keeps for the bar at its right: the thumb
    /// and the room on both sides of it. The content is laid out beside
    /// it, never under it.
    pub(crate) fn gutter(&self) -> f32 {
        self.width + 2.0 * self.inset
    }
}

/// The fill and the text colour of an element in one state, part of its
/// [`Looks`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StateLook {
    /// The fill inside its box, or none.
    pub background: Option<Color>,
    /// The colour of the text inside it that sets none of its own. `None`
    /// leaves it to the panel style's text colour.
    pub text: Option<Color>,
}

/// How an element that can be pressed looks in each state (visual-system.md,
/// Role tokens): the app fills it from its style tokens, so `loom-ui`
/// chooses no colour of its own.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Looks {
    /// Neither hovered nor pressed, and turned on.
    pub normal: StateLook,
    /// The pointer is over it.
    pub hovered: StateLook,
    /// A press on it is held with the pointer over it, or Space is held
    /// on it.
    pub pressed: StateLook,
    /// It is turned off.
    pub off: StateLook,
}

/// Which of its [`Looks`] an element shows when it is painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    /// Neither hovered nor pressed.
    Normal,
    /// The pointer is over it.
    Hovered,
    /// A press on it is held.
    Pressed,
    /// Turned off.
    Off,
}

impl Looks {
    /// Its look in `state`.
    pub(crate) fn of(&self, state: State) -> StateLook {
        match state {
            State::Normal => self.normal,
            State::Hovered => self.hovered,
            State::Pressed => self.pressed,
            State::Off => self.off,
        }
    }
}

/// The size of a radio's round mark, in logical pixels.
const RADIO_MARK: f32 = 14.0;

/// The room between a radio's mark and its label.
const RADIO_GAP: f32 = 6.0;

/// The size of a check box's box, in logical pixels (ui-layer.md, section
/// 5, rule 17).
const CHECK_BOX: f32 = 14.0;

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
        /// How its pieces differ from `style`, for rich text; empty for
        /// plain text.
        spans: Vec<Span>,
        /// The links in it, in the order of the text.
        links: Vec<Link>,
        /// How it is set.
        style: TextStyle,
        /// Break lines between words to fit the width it gets.
        wrap: bool,
        /// The colour of its letters, or the panel style's.
        colour: Option<Color>,
        /// It is a radio's note, in the panel style's quiet colour when it
        /// sets none of its own.
        quiet: bool,
    },
    /// A text field: the app's text, edited in place ([`crate::field`]),
    /// on one line, or on many for a text area.
    Field {
        /// The text the app holds.
        text: String,
        /// How it is set.
        style: TextStyle,
        /// It shows many lines: it wraps to its width and grows with them.
        many: bool,
        /// What it shows while it is empty, and in which colour.
        placeholder: Option<(String, Color)>,
    },
    /// A window onto its one child, which scrolls up and down inside it
    /// ([`Element::scroll`]).
    Scroll(ScrollStyle),
    /// A group of radios, one focus stop ([`Element::radio_group`]).
    RadioGroup,
    /// One radio of a group ([`Element::radio`]): its mark, then its label
    /// and its note.
    Radio,
    /// A radio's round mark, filled when the radio is chosen.
    RadioMark {
        /// The radio is the chosen one.
        selected: bool,
    },
    /// A slider's track and thumb ([`Element::slider`]).
    Slider(crate::slider::Slider),
    /// A check box ([`Element::checkbox`]): its box, then its label.
    CheckBox,
    /// A check box's box, with its check mark when on.
    CheckMark {
        /// The check box is on.
        on: bool,
    },
    /// A select ([`Element::select`]): its chosen label, then its mark.
    Select(Box<crate::select::Select>),
    /// A drawn mark: a select's, or a chosen option's tick.
    Glyph(crate::select::Glyph),
    /// Empty space.
    Space,
    /// The place of another panel, named by its key.
    Slot(UiId),
}

/// Where an overlay's root panel goes (ui-layer.md, section 5, rule 13),
/// in place of filling the window from its top-left corner.
#[derive(Debug, Clone, PartialEq)]
pub enum Placement {
    /// Its top-left corner at this point in the window.
    At(Point),
    /// Under the element with this UI ID, in another panel, its left edge
    /// at that element's, read from the layout already made.
    Under(UiId),
}

/// A link inside a rich text: the bytes of the text it covers, and what
/// it is as a part of its own.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Link {
    /// Its bytes in the paragraph's text.
    pub(crate) range: Range<usize>,
    /// Its key, under the paragraph's UI ID, or the one around it.
    pub(crate) key: IdKey,
    /// What it is to a screen reader.
    pub(crate) access: Access,
}

/// One piece of an [`Element::rich`] paragraph: some words, how they
/// differ from the paragraph's style, and, for a link, what the link is.
#[derive(Debug, Clone, PartialEq)]
pub struct Inline {
    text: String,
    span: Span,
    link: Option<(IdKey, Access)>,
}

impl Inline {
    /// `text` in the paragraph's own style.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            span: Span::default(),
            link: None,
        }
    }

    /// Bold letters.
    #[must_use]
    pub fn bold(mut self) -> Self {
        self.span.weight = Weight::Bold;
        self
    }

    /// The monospace face, for code.
    #[must_use]
    pub fn mono(mut self) -> Self {
        self.span.face = Face::Mono;
        self
    }

    /// Letters in `colour`, not the paragraph's.
    #[must_use]
    pub fn colour(mut self, colour: Color) -> Self {
        self.span.colour = Some(colour);
        self
    }

    /// A line under the letters, in their colour.
    #[must_use]
    pub fn underline(mut self) -> Self {
        self.span.underline = true;
        self
    }

    /// Makes the piece a link keyed `key`: a focus stop of its own, with
    /// `access` as its node (a `Role::Link` that offers Click). Its look
    /// is the app's to set, with [`colour`](Self::colour) and
    /// [`underline`](Self::underline), so the words show they are a link
    /// by more than colour (WCAG 2.2, SC 1.4.1).
    #[must_use]
    pub fn link(mut self, key: impl Into<IdKey>, access: Access) -> Self {
        self.link = Some((key.into(), access));
        self
    }
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
    pub(crate) hover: Option<String>,
    /// For a scroll container, the focus stop whose unused scroll keys move
    /// it ([`Element::keys_from`]).
    pub(crate) keys_from: Option<UiId>,
    /// The stop's unused scroll keys move it ([`Element::scrolls_from`]).
    pub(crate) scrolls_from_keys: bool,
    pub(crate) keys_first: Vec<Key>,
    /// For a scroll container, it is never a focus stop, even with no stop
    /// inside: a select's open list, whose select keeps the focus.
    pub(crate) passive: bool,
    /// The app hears the pointer moving onto it ([`Element::reports_hover`]).
    pub(crate) reports_hover: bool,
    /// The app hears a right press on it, or a screen reader's
    /// ShowContextMenu ([`Element::context_menu`]).
    pub(crate) context_menu: bool,
    pub(crate) children: Vec<Element>,
    /// Where a root panel goes; `None` fills the window. Only a panel's
    /// root reads it.
    pub(crate) placement: Option<Placement>,
    /// A root panel hears a press outside it (section 5, rule 14). Only a
    /// panel's root reads it.
    pub(crate) closes_outside: bool,
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
            hover: None,
            keys_from: None,
            scrolls_from_keys: false,
            keys_first: Vec::new(),
            passive: false,
            reports_hover: false,
            context_menu: false,
            children,
            placement: None,
            closes_outside: false,
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
                spans: Vec::new(),
                links: Vec::new(),
                style,
                wrap: false,
                colour: None,
                quiet: false,
            },
            Vec::new(),
        )
    }

    /// One paragraph made of `pieces`, set in `style` where a piece sets
    /// nothing else, breaking its lines between words to fit the width it
    /// gets. Each link among the pieces is a part of its own, keyed under
    /// this element's UI ID, or under the one around it when this element
    /// has no key.
    pub fn rich(pieces: impl IntoIterator<Item = Inline>, style: TextStyle) -> Self {
        let mut text = String::new();
        let mut spans = Vec::new();
        let mut links = Vec::new();
        for piece in pieces {
            let start = text.len();
            text.push_str(&piece.text);
            spans.push(Span {
                len: piece.text.len(),
                ..piece.span
            });
            if let Some((key, access)) = piece.link {
                links.push(Link {
                    range: start..text.len(),
                    key,
                    access,
                });
            }
        }
        Self::new(
            Kind::Text {
                text,
                spans,
                links,
                style,
                wrap: true,
                colour: None,
                quiet: false,
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
                many: false,
                placeholder: None,
            },
            Vec::new(),
        );
        field.focusable = true;
        field.look.clip = true;
        field
    }

    /// A multi-line text field showing `text`, the text the app holds, set
    /// in `style` (ui-layer.md, section 8, rule 7). It is a field in every
    /// other way: a focus stop, edited in place, telling the app each
    /// change through [`App::field_event`](crate::App::field_event), and
    /// taking the app's text at each layout. Give it its name with
    /// [`Element::access`] and `Role::TextInput`; a screen reader meets it
    /// as a multi-line text input. It needs a key.
    ///
    /// It wraps to its width and grows with its lines, so give it a width
    /// and leave its height to shrink. Enter inserts a line break, Tab
    /// leaves it, and the arrow keys move by line. A pasted text keeps its
    /// line breaks. It never commits: only focus leaving it ends editing.
    pub fn text_area(text: impl Into<String>, style: TextStyle) -> Self {
        let mut area = Self::new(
            Kind::Field {
                text: text.into(),
                style,
                many: true,
                placeholder: None,
            },
            Vec::new(),
        );
        area.focusable = true;
        area.look.clip = true;
        area
    }

    /// A scroll container showing `child`, laid out at its own height,
    /// through a window as tall as the container, cut to it (ui-layer.md,
    /// section 5, rule 12). The wheel and the thumb scroll it, and so does
    /// focus moving to an element inside that is out of view. The content
    /// keeps `style.ring` of room on every side, so no ring inside is cut.
    ///
    /// The offset is the runtime's, kept by the container's UI ID across
    /// rebuilds, so it needs a key. It is a scroll view to a screen reader:
    /// give it a name with [`Element::access`] and `Role::ScrollView`. A
    /// container whose content has no focus stop is a stop itself, so a
    /// keyboard user can scroll it. Its own sizes size it: give it a fixed
    /// or filling height, or it grows to its child and never scrolls.
    pub fn scroll(child: Element, style: ScrollStyle) -> Self {
        let room = Self::new(Kind::Column, vec![child]).padding(style.ring);
        let mut scroll = Self::new(Kind::Scroll(style), vec![room]);
        scroll.look.clip = true;
        scroll
    }

    /// For a scroll container, takes its keys from the focus stop `stop` in
    /// the same panel, such as a search field above a list of results
    /// (ui-layer.md, section 5, rule 12). The container is then never a
    /// stop itself, even with no stop inside, so Tab does not stop twice
    /// for one list. It takes no scroll key itself: the app drives it from
    /// that stop's keys, by moving its highlight and asking for the row
    /// with `Effects::scroll_into_view`, as the APG combobox pattern does.
    /// [`Element::scrolls_from`] is the other kind. `stop` must be a focus stop of
    /// the panel when it is laid out; a debug build panics when it is not.
    /// Other elements take no keys this way, so a debug build panics on one.
    pub fn keys_from(mut self, stop: UiId) -> Self {
        if !matches!(self.kind, Kind::Scroll(_)) {
            debug_assert!(false, "only a scroll container takes its keys from a stop");
            return self;
        }
        self.keys_from = Some(stop);
        self
    }

    /// For a scroll container, as [`Element::keys_from`], and a scroll key
    /// the stop `stop` leaves unused also moves it, as it would move a
    /// container around the stop (ui-layer.md, section 6, rule 6, step 3):
    /// help's body scrolls from its Close button, which sits above it, so a
    /// keyboard user can read on without first tabbing to a link (WCAG 2.2,
    /// SC 2.1.1).
    #[must_use]
    pub fn scrolls_from(mut self, stop: UiId) -> Self {
        self = self.keys_from(stop);
        self.scrolls_from_keys = self.keys_from.is_some();
        self
    }

    /// A group of `radios`, built with [`Element::radio`], one under
    /// another, named `label` for a screen reader (ui-layer.md, section 6,
    /// rule 9; the WAI-ARIA radio group pattern). The group is one focus
    /// stop: Tab lands on the chosen radio, or the first when none is, and
    /// the arrows move among them and choose. Give it a key, and space the
    /// radios with [`Element::spacing`] so no ring touches a neighbour.
    pub fn radio_group(label: impl Into<String>, radios: impl IntoIterator<Item = Element>) -> Self {
        let mut group = Self::new(Kind::RadioGroup, radios.into_iter().collect());
        group.access = Some(Access::new(crate::access::Role::RadioGroup, label));
        group
    }

    /// A radio named `label`, set in `style`, chosen when `selected`, for
    /// [`Element::radio_group`]. A press on it, or a key that chooses it,
    /// reaches the app as [`App::clicked`](crate::App::clicked) on the next
    /// turn; the app keeps which one is chosen. It needs a key.
    pub fn radio(label: impl Into<String>, style: TextStyle, selected: bool) -> Self {
        let label = label.into();
        // The mark sits on the middle of the label's first line.
        let lift = ((style.size * style.line_height - RADIO_MARK) / 2.0).max(0.0);
        let mark = Self::new(Kind::RadioMark { selected }, Vec::new())
            .width(Length::Fixed(RADIO_MARK))
            .height(Length::Fixed(RADIO_MARK));
        let mark = Self::column([mark]).padding(Padding {
            top: lift,
            ..Padding::default()
        });
        let words = Self::column([Self::text(label.clone(), style)]);
        let mut radio = Self::new(Kind::Radio, vec![mark, words]);
        radio.spacing = RADIO_GAP;
        radio.focusable = true;
        radio.access = Some(
            Access::new(crate::access::Role::RadioButton, label)
                .value(crate::access::Value::On(selected))
                .action(crate::access::Action::Click),
        );
        radio
    }

    /// A slider showing `slider`'s position, from 0 to 1 along its travel
    /// (ui-layer.md, section 5, rule 16): a 4 px track and a 14 px round
    /// thumb, filling the width it gets. It is a focus stop, unless it is
    /// turned off, and takes the WAI-ARIA slider pattern's keys (section 6,
    /// rule 10). A drag, a double-click, a key, and a screen reader's
    /// Increment and Decrement reach the app through
    /// [`App::slid`](crate::App::slid); the app keeps the position, and the
    /// one it shows at the next layout wins.
    ///
    /// It needs a key, and a node the app gives with [`Element::access`]:
    /// `Role::Slider`, named by its parameter, with the parameter's own
    /// value and range, offering Increment, Decrement, and SetValue
    /// (section 7, rule 30). The runtime carries out Increment and
    /// Decrement; SetValue, in the parameter's units, goes to the app.
    pub fn slider(slider: crate::slider::Slider) -> Self {
        let mut element = Self::new(Kind::Slider(slider), Vec::new())
            .width(Length::FILL)
            .height(Length::Fixed(crate::slider::THUMB));
        element.focusable = true;
        element
    }

    /// A check box named `label`, set in `style`, checked when `on`
    /// (ui-layer.md, section 5, rule 17): a 14 px box with a 1.5 px border,
    /// and a check mark in the panel style's primary colour when on, beside
    /// its label. A press on the box or the label toggles it and takes no
    /// focus; Space toggles it when it has focus (section 6, rule 11). Each
    /// toggle reaches the app as [`App::clicked`](crate::App::clicked) on
    /// the next turn, and a screen reader's Click through
    /// [`App::accessibility_event`](crate::App::accessibility_event), as a
    /// button's does. The app keeps whether it is on. It needs a key.
    pub fn checkbox(label: impl Into<String>, style: TextStyle, on: bool) -> Self {
        let label = label.into();
        // The box sits on the middle of the label's first line.
        let lift = ((style.size * style.line_height - CHECK_BOX) / 2.0).max(0.0);
        let mark = Self::new(Kind::CheckMark { on }, Vec::new())
            .width(Length::Fixed(CHECK_BOX))
            .height(Length::Fixed(CHECK_BOX));
        let mark = Self::column([mark]).padding(Padding {
            top: lift,
            ..Padding::default()
        });
        let words = Self::column([Self::text(label.clone(), style)]);
        let mut checkbox = Self::new(Kind::CheckBox, vec![mark, words]);
        checkbox.spacing = RADIO_GAP;
        checkbox.focusable = true;
        checkbox.access = Some(
            Access::new(crate::access::Role::CheckBox, label)
                .value(crate::access::Value::On(on))
                .action(crate::access::Action::Click),
        );
        checkbox
    }

    /// A select showing `select`'s chosen option (ui-layer.md, section 5,
    /// rule 18): a box with the chosen label and a mark that says it opens,
    /// filling the width it gets. It is a focus stop, unless it has no
    /// options, and takes the WAI-ARIA select-only combobox pattern's keys
    /// (section 6, rule 12). Its open list is the runtime's. A choice
    /// reaches the app through [`App::chose`](crate::App::chose); the app
    /// keeps it, and the one it shows at the next layout wins. Its node is
    /// a combo box named by `select`'s name (section 7, rule 32). It needs
    /// a key.
    pub fn select(select: crate::select::Select) -> Self {
        let style = select.style;
        let access = select.access();
        let children = crate::select::select_children(&select);
        let mut element = Self::new(Kind::Select(Box::new(select)), children)
            .width(Length::FILL)
            .spacing(6.0)
            .padding(Padding {
                top: style.padding_y,
                bottom: style.padding_y,
                left: style.padding_x,
                right: style.padding_x,
            })
            .background(style.background)
            .border(style.border_width, style.border)
            .radius(style.radius);
        element.focusable = true;
        element.access = Some(access);
        element
    }

    /// A drawn mark, [`crate::select::MARK`] square.
    pub(crate) fn glyph(glyph: crate::select::Glyph) -> Self {
        Self::new(Kind::Glyph(glyph), Vec::new())
            .width(Length::Fixed(crate::select::MARK))
            .height(Length::Fixed(crate::select::MARK))
    }

    /// For a scroll container: never a focus stop, even with no stop
    /// inside, for a list whose keys come from an element in another
    /// panel, as a select's open list does.
    pub(crate) fn passive(mut self) -> Self {
        self.passive = true;
        self
    }

    /// For a radio, a second line under its label, `text` set in `style`,
    /// in the panel style's quiet colour. It is part of the radio: a press
    /// on it chooses the radio, the ring goes around both lines, and it is
    /// the radio's description. Other elements have no note, so a debug
    /// build panics on one.
    pub fn note(mut self, text: impl Into<String>, style: TextStyle) -> Self {
        if self.kind != Kind::Radio {
            debug_assert!(false, "only a radio has a note");
            return self;
        }
        let text = text.into();
        if let Some(words) = self.children.last_mut() {
            let mut note = Self::text(text.clone(), style);
            if let Kind::Text { quiet, .. } = &mut note.kind {
                *quiet = true;
            }
            words.children.push(note);
        }
        self.access = self.access.map(|access| access.description(text));
        self
    }

    /// For a text field: `text` shown in `colour` while the field is empty
    /// and nothing is being composed in it, as a hint of what to type. It
    /// is not the field's value, and a screen reader hears it as the
    /// field's placeholder, after its name (`set_placeholder`). Its colour
    /// is the app's, which must reach 4.5:1 against the field (WCAG 2.2,
    /// SC 1.4.3), since it is text to read. On any other element it does
    /// nothing.
    #[must_use]
    pub fn placeholder(mut self, text: impl Into<String>, colour: Color) -> Self {
        if let Kind::Field { placeholder, .. } = &mut self.kind {
            *placeholder = Some((text.into(), colour));
        }
        self
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

    /// Its look in each state: a button's, chosen when it is painted
    /// (visual-system.md, Role tokens). Off when its access says it is
    /// turned off, whatever else holds; otherwise pressed while a press
    /// on it is held with the pointer over it, or while Space is held on
    /// it; otherwise hovered while the pointer is over it; otherwise
    /// normal. Its box takes the state's fill, over any
    /// [`background`](Self::background), and the text inside that sets
    /// no colour takes the state's. The focus ring is drawn over whichever
    /// shows. Only an element with a key is ever hovered or pressed.
    pub fn looks(mut self, looks: Looks) -> Self {
        self.look.states = Some(looks);
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

    /// Its hover text (ui-layer.md, section 5, rule 11): shown by the
    /// runtime while the pointer is over it or it has the focus, and its
    /// node's description when its access sets none (section 7, rule 23).
    /// Only an element with a key has it.
    pub fn hover_text(mut self, text: impl Into<String>) -> Self {
        self.hover = Some(text.into());
        self
    }

    /// Takes `keys` before the focused element inside it (ui-layer.md,
    /// section 6, rule 6, step 2b): while the focus is on it or inside it,
    /// a press of one of these keys goes to
    /// [`App::panel_key`](crate::App::panel_key) with this element's UI ID
    /// first, and a key the app answers `None` for goes on to the focused
    /// element as before. A browser's list keys over its search field are
    /// the case it is for: in WAI-ARIA's combobox, Up and Down move the
    /// list's highlight and never the caret. The nearest such element
    /// around the focus that names the key is asked, in the focus's own
    /// panel. Tab is never taken this way. It needs a key.
    #[must_use]
    pub fn takes_first(mut self, keys: impl IntoIterator<Item = Key>) -> Self {
        self.keys_first = keys.into_iter().filter(|key| *key != Key::Tab).collect();
        self
    }

    /// Tells the app when the pointer moves onto it, through
    /// [`App::hovered`](crate::App::hovered) on the next turn (ui-layer.md,
    /// section 3, rule 4): for an element whose hover means something to
    /// the app, such as a list's row, where it moves the highlight. Hover
    /// stays widget state for every other element. It needs a key.
    #[must_use]
    pub fn reports_hover(mut self) -> Self {
        self.reports_hover = true;
        self
    }

    /// Gives it a context menu that the app opens (ui-layer.md, section 5,
    /// rule 19): a right press on it, or a screen reader's ShowContextMenu
    /// on its node, reaches [`App::context_menu`](crate::App::context_menu)
    /// on the next turn. Its node offers ShowContextMenu. It needs a key.
    #[must_use]
    pub fn context_menu(mut self) -> Self {
        self.context_menu = true;
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

    /// Casts a soft shadow under the element's box, drawn first and cut
    /// only by its panel's clip (section 5, rule 15).
    pub fn shadow(mut self, shadow: ShadowStyle) -> Self {
        self.look.shadow = Some(shadow);
        self
    }

    /// On a panel's root: places the panel at `placement`, at its content
    /// size, moved in to stay inside the window, instead of filling the
    /// window (section 5, rule 13). A root under an element that is gone
    /// shows nothing until the element is back.
    pub fn placed(mut self, placement: Placement) -> Self {
        self.placement = Some(placement);
        self
    }

    /// On a panel's root: a press of any mouse button outside the panel,
    /// while its layer takes input, is taken and told to the app through
    /// [`App::pressed_outside`](crate::App::pressed_outside) (section 5,
    /// rule 14).
    pub fn closes_on_press_outside(mut self) -> Self {
        self.closes_outside = true;
        self
    }
}

#[cfg(test)]
mod tests;
