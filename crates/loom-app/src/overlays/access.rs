//! The overlay that owns the input, as a screen reader is told it: its
//! [`Item`]s, built from the same models its view draws (ADR-0021), with
//! the same UI IDs and messages (`docs/design/ui-layer.md`, section 7,
//! rule 27).
//!
//! Only the overlay that owns the input is reported. What it covers, the
//! window and any overlay under it, stays hidden (rule 24). Every overlay
//! is reported: the question, help, the menus, the browsers, Preferences,
//! and the licence chooser.

use crate::a11y::items::{Item, Kind};
use crate::input_owner::InputOwner;
use crate::menus::MenuModel;
use crate::{App, Message, help};
use loom_ui_core::UiId;

/// The UI ID of the open question.
pub(crate) fn question_id() -> UiId {
    UiId::panel("question")
}

/// The UI ID of one of the question's answers, by the name the control
/// channel uses for it.
pub(crate) fn answer_id(name: &str) -> UiId {
    question_id().with(name)
}

/// The UI ID of the open help page.
pub(crate) fn help_id() -> UiId {
    UiId::panel("help")
}

impl App {
    /// The overlay that owns the input, as items, and where its keys act
    /// while no text field has them. Empty while the canvas has the input,
    /// or while an overlay not yet reported has it.
    pub(crate) fn overlay_items(&self) -> (Vec<Item>, Option<UiId>) {
        match self.input_owner() {
            // The question is the runtime's (row 3b): its nodes come from
            // its panel, and its focus is the runtime's on an answer.
            InputOwner::Question => (Vec::new(), None),
            InputOwner::Help => match &self.overlays.help {
                Some(open) => (vec![help_item(&open.model)], Some(help_id())),
                None => (Vec::new(), None),
            },
            // The probe menu and Recent are the runtime's (row 3b).
            InputOwner::ProbeMenu | InputOwner::RecentList => (Vec::new(), None),
            InputOwner::TileMenu => match self.menu_model() {
                Some(menu) => {
                    let focus = menu
                        .highlighted()
                        .map_or_else(|| menu.id.clone(), |item| item.id.clone());
                    (vec![menu_item(&menu)], Some(focus))
                }
                None => (Vec::new(), None),
            },
            InputOwner::Browser => match self.overlays.browser() {
                Some(open) => {
                    let (item, focus) = crate::browser::access::items(open.model());
                    (vec![item], focus)
                }
                None => (Vec::new(), None),
            },
            InputOwner::Patches => match self.overlays.patches() {
                Some(open) => (vec![crate::patches::access::items(open.model())], None),
                None => (Vec::new(), None),
            },
            // No part of these has the keys until a field does, so the
            // dialog itself has the focus, as help does.
            InputOwner::Preferences => match self.preferences_model() {
                Some(model) => (
                    vec![crate::settings::access::items(&model)],
                    Some(crate::settings::access::preferences_id()),
                ),
                None => (Vec::new(), None),
            },
            InputOwner::Chooser => match &self.overlays.chooser {
                Some(open) => (
                    vec![crate::licence_chooser::access::items(&open.model())],
                    Some(crate::licence_chooser::access::licence_id()),
                ),
                None => (Vec::new(), None),
            },
            InputOwner::Canvas => (Vec::new(), None),
        }
    }
}

/// A menu and its items. The highlighted item is reported as focus, not
/// as a state of its own.
fn menu_item(menu: &MenuModel) -> Item {
    let items = menu
        .items
        .iter()
        .map(|item| Item {
            label: Some(item.label.clone()),
            description: item.description.clone(),
            click: Some(item.message.clone()),
            ..Item::new(item.id.clone(), Kind::MenuItem)
        })
        .collect();
    Item {
        label: Some(menu.label.clone()),
        children: items,
        ..Item::new(menu.id.clone(), Kind::Menu)
    }
}

/// The help page: a dialog with its Close button, the module's summary,
/// its notes one paragraph and one list entry at a time with links as
/// links, its parameters and ports, and Open example.
fn help_item(model: &help::Model) -> Item {
    let id = help_id();
    let mut parts = vec![Item::button(
        id.clone().with("close"),
        "Close",
        Some(Message::CloseHelp),
    )];
    if !model.id.is_empty() {
        parts.push(Item::text(id.clone().with("summary"), model.summary()));
        parts.push(Item::text(id.clone().with("description"), model.description));
    }
    // The page does not change while it is open, so its blocks are keyed
    // by their place on it (section 4: a list that never changes length).
    parts.extend(notes(&id.clone().with("notes"), &model.lead));
    if !model.id.is_empty() {
        for (key, title, entries) in [
            ("parameters", "Parameters", &model.params),
            ("inputs", "Inputs", &model.inputs),
            ("outputs", "Outputs", &model.outputs),
        ] {
            let section = id.clone().with(key);
            let rows = if entries.is_empty() {
                vec![Item::text(section.clone().with("none"), "None.")]
            } else {
                entries
                    .iter()
                    .map(|entry| {
                        Item::text(
                            section.clone().with(entry.name),
                            format!("{}: {} {}", entry.label, entry.details, entry.description),
                        )
                    })
                    .collect()
            };
            parts.push(Item::group(section, title, rows));
        }
    }
    if model.example.is_some() {
        parts.push(Item::button(
            id.clone().with("example"),
            "Open example",
            Some(Message::HelpExample),
        ));
    }
    parts.extend(notes(&id.clone().with("example-notes"), &model.tail));
    Item {
        label: Some(model.name.to_string()),
        children: parts,
        ..Item::new(id, Kind::Dialog)
    }
}

/// Our own notes, one item per paragraph, heading, list entry, or code
/// block, under `under`, from the blocks the view shows
/// (`help/markdown.rs`). A link is read as its words in its block's text,
/// and is also an item of its own that follows the link, as a click on it
/// in the view does.
fn notes(under: &UiId, blocks: &[help::markdown::Block]) -> Vec<Item> {
    let mut items = Vec::new();
    for (at, block) in blocks.iter().enumerate() {
        let key = under.clone().with(at as u32);
        let words = block.words();
        if words.trim().is_empty() {
            continue;
        }
        items.push(Item::text(key.clone(), words));
        for (n, link) in block.links().iter().enumerate() {
            items.push(Item {
                label: Some(link.label.clone()),
                click: Some(Message::HelpLink(link.target.clone())),
                ..Item::new(key.clone().with("link").with(n as u32), Kind::Link)
            });
        }
    }
    items
}

#[cfg(test)]
mod tests;
