//! The runtime's select and its open list (ui-layer.md, section 5, rule
//! 18; section 6, rule 12; section 7, rule 32; the WAI-ARIA select-only
//! combobox pattern).
//!
//! A select shows its chosen option. The app gives the options' labels and
//! which is chosen, and keeps the choice; a choice reaches it as
//! [`App::chose`](crate::App::chose). Everything else is the runtime's:
//!
//! - **The open list** is a panel of the runtime's, in the Overlays layer,
//!   added to the view while the select is open ([`list_panel`]), placed
//!   under the select, or above it when there is more room there, and
//!   scrolling when its options do not fit.
//! - **Focus stays on the select.** The highlight is the list's own
//!   ([`Open`]), and the select's node names it as its active descendant.
//! - **The keys** are the pattern's ([`closed_key`], [`open_key`]), with
//!   type-ahead ([`Typed`]).
//! - **It closes on its own, with no choice,** when the select leaves the
//!   layout, scrolls out of view, loses focus, has an overlay opened over
//!   it, or its options change.

use crate::access::{Access, Action, Role, States, Value};
use crate::element::{Element, Length, Padding, Placement, ScrollStyle, ShadowStyle};
use loom_ui_core::{Color, Key, Modifiers, Point, Rect, TextStyle, UiId};
use std::time::{Duration, Instant};

/// How a select and its list look: the app fills it from its tokens, as it
/// fills [`ScrollStyle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectStyle {
    /// The labels' size and weight, in the select and in the list.
    pub text: TextStyle,
    /// The labels' colour.
    pub colour: Color,
    /// The select's fill.
    pub background: Color,
    /// The select's border colour.
    pub border: Color,
    /// The width of the select's and the list's borders.
    pub border_width: f32,
    /// The radius of the select's and the list's corners.
    pub radius: f32,
    /// The room between the select's border and its label, across.
    pub padding_x: f32,
    /// The room between the select's border and its label, up and down.
    pub padding_y: f32,
    /// The colour of the mark that says it opens, and of the chosen
    /// option's check mark. It needs 3:1 against its fill (SC 1.4.11).
    pub mark: Color,
    /// The list's fill: the menu's.
    pub list_background: Color,
    /// The list's border colour.
    pub list_border: Color,
    /// The highlighted option's fill: the hovered row's.
    pub highlight: Color,
    /// The list's shadow: the menu's.
    pub shadow: Option<ShadowStyle>,
    /// The list's scroll bar, when its options do not fit.
    pub scroll: ScrollStyle,
}

/// What a select shows, for [`Element::select`](crate::Element::select):
/// its name, its options' labels, and which one is chosen.
#[derive(Debug, Clone, PartialEq)]
pub struct Select {
    pub(crate) name: String,
    pub(crate) options: Vec<String>,
    pub(crate) chosen: Option<usize>,
    pub(crate) style: SelectStyle,
}

impl Select {
    /// A select named `name` for a screen reader, offering `options`, with
    /// `chosen` chosen, or none, drawn in `style`. A chosen index past the
    /// end is none.
    pub fn new(name: impl Into<String>, options: Vec<String>, chosen: Option<usize>, style: SelectStyle) -> Self {
        let chosen = chosen.filter(|index| *index < options.len());
        Self {
            name: name.into(),
            options,
            chosen,
            style,
        }
    }

    /// The chosen option's label, or nothing.
    pub(crate) fn label(&self) -> &str {
        self.chosen.map_or("", |index| self.options[index].as_str())
    }

    /// The select's node (section 7, rule 32): a combo box with the chosen
    /// option as its value, offering Increment, Decrement, and Expand, and
    /// turned off when it has no options (section 5, rule 18).
    pub(crate) fn access(&self) -> Access {
        let states = States {
            disabled: self.options.is_empty(),
            expanded: Some(false),
            ..States::default()
        };
        Access::new(Role::ComboBox, self.name.clone())
            .value(Value::Text(self.label().to_owned()))
            .action(Action::Increment)
            .action(Action::Decrement)
            .action(Action::Expand)
            .states(states)
    }

    /// The height of one option's row in the list.
    fn row_height(&self) -> f32 {
        self.style.text.size * self.style.text.line_height + 2.0 * ROW_PADDING_Y
    }
}

/// The size of the mark that says a select opens, and of the chosen
/// option's check mark, in logical pixels.
pub(crate) const MARK: f32 = 10.0;

/// The room between a label and the mark beside it.
const MARK_GAP: f32 = 6.0;

/// The room above and below an option's label in the list.
const ROW_PADDING_Y: f32 = 3.0;

/// The room before and after an option's row content in the list.
const ROW_PADDING_X: f32 = 8.0;

/// How far Page Up and Page Down move the highlight (the pattern's ten).
const PAGE: usize = 10;

/// How soon a character must follow the last to join its type-ahead
/// string.
const TYPE_AHEAD: Duration = Duration::from_millis(500);

/// The key of the panel that holds a select's open list.
pub(crate) const LIST_PANEL: &str = "loom-ui.select-list";

/// A drawn mark: the select's, or a chosen option's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Glyph {
    /// The mark that says a select opens: a small V.
    Opens(Color),
    /// A tick before the chosen option in the list.
    Tick(Color),
}

/// The UI ID of the open list's root, the list box.
pub(crate) fn list_id() -> UiId {
    UiId::panel(LIST_PANEL).with("list")
}

/// The UI ID of the option at `index` in the open list. The list closes
/// when its options change, so an option keeps its index for as long as
/// the list is open, and the index names it (section 4, rule 3's reason
/// does not reach a list that never changes while it shows).
pub(crate) fn option_id(index: usize) -> UiId {
    list_id().with("options").with(format!("o{index}"))
}

/// The index of the option `id` names in the open list, if it is one.
pub(crate) fn option_index(id: &UiId) -> Option<usize> {
    let parent = list_id().with("options");
    if !id.is_under(&parent) || id.keys().len() != parent.keys().len() + 1 {
        return None;
    }
    match id.keys().last()? {
        loom_ui_core::IdKey::Name(name) => name.strip_prefix('o')?.parse().ok(),
        _ => None,
    }
}

/// A select's open list: which select, what it offered when it opened, and
/// the highlight.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Open {
    /// The select.
    pub(crate) id: UiId,
    /// What it showed when it opened. The list closes when the options
    /// change.
    pub(crate) select: Select,
    /// The highlighted option.
    pub(crate) highlight: usize,
}

/// What a key does to a closed select (section 6, rule 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Closed {
    /// Open the list with the chosen option highlighted.
    OpenOnChosen,
    /// Open the list on the first option.
    OpenOnFirst,
    /// Open the list on the last option.
    OpenOnLast,
    /// Open the list on the option the typed text finds.
    Type,
}

/// What a key does to an open list (section 6, rule 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenKey {
    /// Move the highlight by this many options, stopping at the ends.
    Move(isize),
    /// Highlight the first option.
    First,
    /// Highlight the last option.
    Last,
    /// Choose the highlighted option and close the list.
    Choose,
    /// Choose the highlighted option, close the list, and let Tab move the
    /// focus on.
    ChooseAndTab,
    /// Close the list with no choice.
    Close,
    /// Move the highlight to the option the typed text finds.
    Type,
}

/// True when `text` is something a key types: one or more characters, none
/// of them a control character.
fn typed(text: Option<&str>) -> bool {
    text.is_some_and(|text| !text.is_empty() && !text.chars().any(char::is_control))
}

/// What `key` with `modifiers`, typing `text`, does to a closed select, by
/// the pattern: Down, Alt with Down, Enter, and Space open it on the chosen
/// option; Up and Home on the first; End on the last; a printable character
/// opens it on what it types. `None` for any other key.
pub(crate) fn closed_key(key: Key, modifiers: Modifiers, text: Option<&str>) -> Option<Closed> {
    let alt_only = modifiers.alt && !(modifiers.control || modifiers.meta || modifiers.shift);
    Some(match key {
        Key::Down | Key::Enter | Key::Space if modifiers.none() => Closed::OpenOnChosen,
        Key::Down if alt_only => Closed::OpenOnChosen,
        Key::Up | Key::Home if modifiers.none() => Closed::OpenOnFirst,
        Key::End if modifiers.none() => Closed::OpenOnLast,
        _ if !(modifiers.control || modifiers.alt || modifiers.meta) && typed(text) => Closed::Type,
        _ => return None,
    })
}

/// What `key` with `modifiers`, typing `text`, does to an open list, by the
/// pattern. `None` for any other key, which goes on.
pub(crate) fn open_key(key: Key, modifiers: Modifiers, text: Option<&str>) -> Option<OpenKey> {
    let alt_only = modifiers.alt && !(modifiers.control || modifiers.meta || modifiers.shift);
    let shift_only = modifiers.shift && !(modifiers.control || modifiers.alt || modifiers.meta);
    Some(match key {
        Key::Down if modifiers.none() => OpenKey::Move(1),
        Key::Up if modifiers.none() => OpenKey::Move(-1),
        Key::PageDown if modifiers.none() => OpenKey::Move(PAGE as isize),
        Key::PageUp if modifiers.none() => OpenKey::Move(-(PAGE as isize)),
        Key::Home if modifiers.none() => OpenKey::First,
        Key::End if modifiers.none() => OpenKey::Last,
        Key::Enter | Key::Space if modifiers.none() => OpenKey::Choose,
        Key::Up if alt_only => OpenKey::Choose,
        Key::Tab if modifiers.none() || shift_only => OpenKey::ChooseAndTab,
        Key::Escape if modifiers.none() => OpenKey::Close,
        _ if !(modifiers.control || modifiers.alt || modifiers.meta) && typed(text) => OpenKey::Type,
        _ => return None,
    })
}

/// `from` moved by `by` among `count` options, stopping at the ends.
pub(crate) fn moved(from: usize, by: isize, count: usize) -> usize {
    let last = count.saturating_sub(1);
    from.saturating_add_signed(by).min(last)
}

/// The text typed into a select's type-ahead, and when its last character
/// came.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Typed {
    at: Option<(UiId, String, Instant)>,
}

impl Typed {
    /// Adds `text`, typed on the select `id` at `now`, and returns the
    /// option it finds among `options` from the one at `from`, if any.
    /// Characters within 500 ms of each other build one string, which is
    /// matched from `from` on; a new string, and one character typed again
    /// and again, look from the option after `from`, so a repeated
    /// character steps through the options that start with it. The match
    /// ignores case and wraps past the end.
    pub(crate) fn find(
        &mut self,
        id: &UiId,
        text: &str,
        now: Instant,
        options: &[String],
        from: usize,
    ) -> Option<usize> {
        let joins = self
            .at
            .as_ref()
            .is_some_and(|(on, _, last)| on == id && now.saturating_duration_since(*last) < TYPE_AHEAD);
        let string = match self.at.take() {
            Some((_, mut string, _)) if joins => {
                string.push_str(text);
                string
            }
            _ => text.to_owned(),
        };
        let mut chars = string.chars();
        let first = chars.next();
        let repeated = string.chars().count() > 1 && chars.all(|c| Some(c) == first);
        let (needle, start) = if string.chars().count() == 1 || repeated {
            (first.map(String::from).unwrap_or_default(), from + 1)
        } else {
            (string.clone(), from)
        };
        self.at = Some((id.clone(), string, now));
        let needle = needle.to_lowercase();
        let count = options.len();
        (0..count)
            .map(|step| (start + step) % count.max(1))
            .find(|index| options[*index].to_lowercase().starts_with(&needle))
    }
}

/// The open list's panel: the list box, placed under the select at
/// `bounds`, or above it when the room below is short and there is more
/// above, in a window of height `window`. It is as wide as the select or
/// its widest option, as tall as its options up to the room it has, and
/// scrolls beyond that.
pub(crate) fn list_panel(open: &Open, bounds: Rect, window: f32) -> Element {
    let style = &open.select.style;
    let rows: Vec<Element> = open
        .select
        .options
        .iter()
        .enumerate()
        .map(|(index, label)| row(open, index, label))
        .collect();
    let count = rows.len();
    let frame = 2.0 * (style.border_width + style.scroll.ring);
    let content = count as f32 * open.select.row_height() + frame;
    let below = (window - (bounds.y + bounds.height)).max(0.0);
    let above = bounds.y.max(0.0);
    let (height, at) = if content <= below || below >= above {
        let height = content.min(below.max(open.select.row_height() + frame));
        (height, Point::new(bounds.x, bounds.y + bounds.height))
    } else {
        let height = content.min(above);
        (height, Point::new(bounds.x, bounds.y - height))
    };
    let scroll = Element::scroll(Element::column(rows).width(Length::FILL), style.scroll)
        .key("options")
        .width(Length::FILL)
        .height(Length::Fixed((height - 2.0 * style.border_width).max(0.0)))
        .access(Access::new(Role::ScrollView, open.select.name.clone()))
        .passive();
    let mut list = Element::column([scroll])
        .key("list")
        .min_width(bounds.width)
        .background(style.list_background)
        .border(style.border_width, style.list_border)
        .radius(style.radius)
        .clip()
        .access(Access::new(Role::ListBox, open.select.name.clone()).size_of_set(count))
        .placed(Placement::At(at));
    if let Some(shadow) = style.shadow {
        list = list.shadow(shadow);
    }
    list
}

/// The row of the option at `index`, labelled `label`: its tick when it is
/// the chosen one, its label, and the highlight's fill when highlighted.
fn row(open: &Open, index: usize, label: &str) -> Element {
    let style = &open.select.style;
    let tick = if open.select.chosen == Some(index) {
        Element::glyph(Glyph::Tick(style.mark))
    } else {
        Element::space(Length::Fixed(MARK), Length::Fixed(MARK))
    };
    let lift = ((style.text.size * style.text.line_height - MARK) / 2.0).max(0.0);
    let tick = Element::column([tick]).padding(Padding {
        top: lift,
        ..Padding::default()
    });
    let highlighted = index == open.highlight;
    let mut row = Element::row([tick, Element::text(label, style.text).colour(style.colour)])
        .key(format!("o{index}"))
        .width(Length::FILL)
        .spacing(MARK_GAP)
        .padding(Padding {
            top: ROW_PADDING_Y,
            bottom: ROW_PADDING_Y,
            left: ROW_PADDING_X,
            right: ROW_PADDING_X,
        })
        .access(
            Access::new(Role::ListBoxOption, label)
                .states(States {
                    selected: highlighted,
                    ..States::default()
                })
                .position_in_set(index)
                .action(Action::Click),
        );
    if highlighted {
        row = row.background(style.highlight);
    }
    row
}

/// The select element itself: its chosen label filling the box, and the
/// mark that says it opens at its right, in the select's border and fill.
pub(crate) fn select_children(select: &Select) -> Vec<Element> {
    let style = &select.style;
    let label = Element::text(select.label(), style.text)
        .colour(style.colour)
        .width(Length::FILL);
    let lift = ((style.text.size * style.text.line_height - MARK) / 2.0).max(0.0);
    let mark = Element::column([Element::glyph(Glyph::Opens(style.mark))]).padding(Padding {
        top: lift,
        ..Padding::default()
    });
    vec![label, mark]
}

/// What the open list's panel is built from: the list, the select's place,
/// and the window's height, so the panel is built again when any changes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ListModel {
    /// The open list.
    pub(crate) open: Open,
    /// Where the select is in the window.
    pub(crate) bounds: Rect,
    /// The window's height.
    pub(crate) window: f32,
}

/// Builds the open list's panel from its model.
pub(crate) fn build_list(model: &ListModel) -> Element {
    list_panel(&model.open, model.bounds, model.window)
}

impl<A: crate::App> crate::Panels<'_, A> {
    /// Opens the list of the select `id`, showing `select`, with the option
    /// at `highlight` highlighted and brought into view.
    pub(crate) fn open_select(&mut self, id: &UiId, select: Select, highlight: usize) {
        let highlight = highlight.min(select.options.len().saturating_sub(1));
        self.window.select = Some(Open {
            id: id.clone(),
            select,
            highlight,
        });
        self.window.reveal = Some(option_id(highlight));
        self.window.select_changed = true;
    }

    /// Closes the open list, if any, with no choice.
    pub(crate) fn close_select(&mut self) {
        if self.window.select.take().is_some() {
            self.window.select_changed = true;
        }
    }

    /// Moves the open list's highlight to `index`, brought into view.
    pub(crate) fn highlight_option(&mut self, index: usize) {
        if let Some(open) = self.window.select.as_mut() {
            let index = index.min(open.select.options.len().saturating_sub(1));
            if open.highlight != index {
                open.highlight = index;
                self.window.select_changed = true;
            }
            self.window.reveal = Some(option_id(index));
        }
    }

    /// Chooses the option at `index` of the select `id`, telling the app
    /// on the next turn when it differs from the chosen one.
    pub(crate) fn choose_option(&self, id: &UiId, chosen: Option<usize>, index: usize) {
        if chosen != Some(index)
            && let Some(message) = self.app.chose(id, index)
        {
            self.waker.send(message);
        }
    }

    /// Chooses the open list's highlighted option and closes the list.
    pub(crate) fn choose_highlighted(&mut self) {
        if let Some(open) = self.window.select.clone() {
            self.choose_option(&open.id, open.select.chosen, open.highlight);
            self.close_select();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
