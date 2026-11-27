//! The Outline tab's contents as panels of `loom-ui`'s Panels layer (plan
//! 3.2, row 4e, commit 2), in the right column's content slot
//! (`chrome.rs`): the tree of the level, or, while a cable branches, the
//! list of places to jump to.
//!
//! The tree follows the WAI-ARIA Tree View pattern (canvas-accessibility.md,
//! section 5): one focus stop, the current row; its scroll container takes
//! the tree keys first (ui-layer.md, section 6, rule 6, step 2b) and hands
//! them to the app's tree logic (`outline_key`), and a key the tree does not
//! take (a letter for type-ahead, `[` and `]`) goes on to the app's
//! listeners as before. Each row has one key under the list, so its UI ID is
//! `outline/list/<row>` (`row_id`): a scroll container keeps its offset by
//! its UI ID, so what it holds is keyed under it (section 5, rule 12).
//!
//! The jump list is a list box (the WAI-ARIA Listbox pattern): its list is
//! one stop whose active descendant is the chosen option. Its keys reach
//! the app's jump logic wherever the focus is, as before (`jump_key`), and
//! the list takes them first when it has the focus.

use super::{Row, jump_id, tree_id};
use crate::style;
use crate::{App, Message};
use loom_ui::access::{Access, Action, Role, States};
use loom_ui::{Element, Layer, Length, Looks, Padding, StateLook, View};
use loom_ui_core::{Color, Key, KeyEvent, UiId};

/// The keys the tree takes before its scroll container would: they move
/// the current row, open and close branches, and act on a row (the
/// WAI-ARIA Tree View pattern).
const TREE_KEYS: [Key; 9] = [
    Key::Up,
    Key::Down,
    Key::Left,
    Key::Right,
    Key::Home,
    Key::End,
    Key::Enter,
    Key::Space,
    Key::Escape,
];

/// The UI ID of the tree's scrolling list. Every row is keyed under it.
pub(crate) fn rows_id() -> UiId {
    tree_id().with("list")
}

/// What the tree's panel is drawn from.
#[derive(Debug, Clone, PartialEq)]
struct Tree {
    /// The rows that show, in tree order.
    rows: Vec<Row>,
    /// The current row, the tree's one stop.
    current: Option<UiId>,
    /// The colours of the rows.
    looks: RowLooks,
}

/// What the jump list's panel is drawn from.
#[derive(Debug, Clone, PartialEq)]
struct Jumps {
    /// The heading: how many places there are.
    heading: String,
    /// Each place's name.
    names: Vec<String>,
    /// The chosen one.
    selected: usize,
    /// The colours of the rows.
    looks: RowLooks,
}

/// The colours of a row, as Iced's tab style drew it: the current row
/// filled and bordered, the others quiet text that fills under the
/// pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RowLooks {
    fill: Color,
    border: Color,
    text: Color,
    quiet: Color,
}

impl App {
    /// The colours the tree and the jump list are painted in.
    fn row_looks(&self) -> RowLooks {
        RowLooks {
            fill: crate::style::RAISED,
            // The chosen row's edge marks it at 3:1 or more against the
            // panel (WCAG 2.2, SC 1.4.11; #463), as the open tab's does:
            // its fill alone is 1.43:1.
            border: crate::style::FIELD_EDGE,
            text: crate::style::TEXT,
            quiet: crate::style::QUIET,
        }
    }

    /// The panel the right column's content slot shows for the Outline
    /// tab: the jump list while a cable branches, else the tree. `None`
    /// while the Outline tab is shut and no jump list is open.
    pub(crate) fn outline_slot(&self) -> Option<UiId> {
        if self.session.outline.has_jump() {
            Some(jump_id())
        } else if self.right_tab == crate::matrix::Tab::Outline {
            Some(tree_id())
        } else {
            None
        }
    }

    /// Adds the tree's or the jump list's panel to the runtime's view,
    /// while the Outline tab or the jump list shows.
    pub(crate) fn outline_panels(&self, view: &mut View) {
        match self.outline_slot() {
            Some(id) if id == jump_id() => {
                let Some(jump) = self.session.outline.jump.as_ref() else {
                    return;
                };
                let panel = Jumps {
                    heading: super::count(jump.choices.len(), "connection"),
                    names: jump.choices.iter().map(|choice| choice.name.clone()).collect(),
                    selected: jump.selected,
                    looks: self.row_looks(),
                };
                view.panel_with(jump_id(), Layer::Panels, &panel, jumps);
            }
            Some(_) => {
                let rows = self.outline_rows();
                let panel = Tree {
                    current: self.outline_current(&rows),
                    rows: rows.to_vec(),
                    looks: self.row_looks(),
                };
                view.panel_with(tree_id(), Layer::Panels, &panel, tree);
            }
            None => {}
        }
    }

    /// The current row of the tree: the tree's one stop, which the focus
    /// lands on when the tree takes it.
    pub(crate) fn outline_current(&self, rows: &[Row]) -> Option<UiId> {
        rows.get(self.session.outline.focused_index(rows))
            .map(|row| row.id.clone())
    }

    /// The tree's one focus stop while it shows and nothing is open over
    /// the canvas.
    pub(crate) fn outline_stops(&self) -> Vec<UiId> {
        if self.input_owner() != crate::input_owner::InputOwner::Canvas {
            return Vec::new();
        }
        match self.outline_slot() {
            Some(id) if id == jump_id() => vec![jump_id().with("list")],
            Some(_) => self.outline_current(&self.outline_rows()).into_iter().collect(),
            None => Vec::new(),
        }
    }

    /// True while the runtime's focus is on one of the tree's rows.
    pub(crate) fn focus_on_outline(&self) -> bool {
        self.focus.as_ref().is_some_and(|id| id.is_under(&rows_id()))
    }

    /// The message for a click on a row or a place to jump to, by the
    /// pointer, Enter, or a screen reader.
    pub(crate) fn outline_click(&self, id: &UiId) -> Option<Message> {
        if id.is_under(&rows_id()) {
            return Some(Message::OutlineRow(id.clone()));
        }
        self.jump_action(id)
    }

    /// The message for one of the tree keys on the tree's list: the app's
    /// tree logic moves the current row, opens or closes a branch, or acts
    /// on the row. `None` for any other element or key.
    pub(crate) fn outline_panel_key(&self, id: &UiId, key: &KeyEvent) -> Option<Message> {
        let KeyEvent::Pressed { key, modifiers, .. } = key else {
            return None;
        };
        let list = *id == rows_id() || *id == jump_id().with("list");
        if !list || !TREE_KEYS.contains(key) {
            return None;
        }
        let command = modifiers.shortcut();
        let plain = !modifiers.shift && !modifiers.alt && (!modifiers.shortcut() || *key == Key::Enter);
        plain.then(|| Message::OutlineKey { key: *key, command })
    }
}

/// The tree: its rows, indented by level, in a scroll keyed `list`.
fn tree(panel: &Tree) -> Element {
    let rows = panel.rows.iter().map(|row| tree_row(panel, row));
    Element::scroll(Element::column(rows).spacing(2.0).width(Length::FILL), style::scroll())
        .key("list")
        .takes_first(TREE_KEYS)
        .width(Length::FILL)
        .height(Length::FILL)
        .min_height(0.0)
}

/// One row: a tree item, its mark, and its name, wrapped to the column's
/// width as Iced's rows were. The current row is the tree's one stop.
fn tree_row(panel: &Tree, row: &Row) -> Element {
    let current = panel.current.as_ref() == Some(&row.id);
    let mark = match row.expanded {
        Some(true) => "▾ ",
        Some(false) => "▸ ",
        None => "",
    };
    let mut access = Access::new(Role::TreeItem, row.name.clone())
        .action(Action::Click)
        .level(row.level - 1)
        .position_in_set(row.position - 1)
        .states(States {
            selected: current,
            expanded: row.expanded,
            ..States::default()
        });
    if !row.description.is_empty() {
        access = access.description(row.description.clone());
    }
    let key = row.id.keys().last().cloned().expect("a row has a key");
    let indent = 8.0 + (row.level.saturating_sub(1) as f32) * 12.0;
    let element = Element::text(format!("{mark}{}", row.name), style::BODY_TEXT)
        .wrap()
        .padding(Padding {
            top: 5.0,
            bottom: 5.0,
            left: indent,
            right: indent,
        })
        .width(Length::FILL)
        .radius(style::TAB_RADIUS)
        .border(
            style::TAB_BORDER,
            if current {
                panel.looks.border
            } else {
                Color::TRANSPARENT
            },
        )
        .looks(looks(panel.looks, current))
        .key(key)
        .access(access.size_of_set(row.size));
    if current { element.focusable() } else { element }
}

/// The jump list: its heading, then one option per place, the chosen one
/// marked. No option takes the focus.
fn jumps(panel: &Jumps) -> Element {
    let options = panel.names.iter().enumerate().map(|(index, name)| {
        let chosen = index == panel.selected;
        Element::text(name.clone(), style::BODY_TEXT)
            .wrap()
            .padding(Padding::symmetric(5.0, 8.0))
            .width(Length::FILL)
            .radius(style::TAB_RADIUS)
            .border(
                style::TAB_BORDER,
                if chosen { panel.looks.border } else { Color::TRANSPARENT },
            )
            .looks(looks(panel.looks, chosen))
            .key(index as u64)
            .access(
                Access::new(Role::ListBoxOption, name.clone())
                    .action(Action::Click)
                    .position_in_set(index)
                    .states(States {
                        selected: chosen,
                        ..States::default()
                    }),
            )
    });
    Element::column([
        Element::text(panel.heading.clone(), style::HEADING_TEXT).colour(panel.looks.text),
        Element::scroll(
            Element::column(options).spacing(2.0).width(Length::FILL),
            style::scroll(),
        )
        .key("list")
        .takes_first(TREE_KEYS)
        .focusable()
        .access(
            Access::new(Role::ListBox, panel.heading.clone())
                .size_of_set(panel.names.len())
                .active_descendant(jump_id().with("list").with(panel.selected as u64)),
        )
        .width(Length::FILL)
        .height(Length::FILL)
        .min_height(0.0),
    ])
    .spacing(6.0)
    .width(Length::FILL)
    .height(Length::FILL)
}

/// A row's looks: `on` filled, the others quiet until the pointer is over
/// them.
fn looks(looks: RowLooks, on: bool) -> Looks {
    let text = if on { looks.text } else { looks.quiet };
    let resting = StateLook {
        background: on.then_some(looks.fill),
        text: Some(text),
    };
    let lit = StateLook {
        background: Some(looks.fill),
        text: Some(text),
    };
    Looks {
        normal: resting,
        hovered: lit,
        pressed: lit,
        off: resting,
    }
}

#[cfg(test)]
mod tests;
