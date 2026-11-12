//! The overlay that owns the input, as a screen reader is told it: its
//! [`Item`]s, built from the same models its view draws (ADR-0021), with
//! the same UI IDs and messages (`docs/design/ui-layer.md`, section 7,
//! rule 27).
//!
//! Only the overlay that owns the input is reported. What it covers, the
//! window and any overlay under it, stays hidden (rule 24). Slice 4a
//! reports the small overlays: the question, help, and the menus. The
//! browsers, Preferences, and the licence chooser follow.

use crate::a11y::items::{Item, Kind};
use crate::input_owner::InputOwner;
use crate::menus::MenuModel;
use crate::{App, Message, help, question};
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
            InputOwner::Question => match &self.overlays.question {
                Some(open) => {
                    let model = open.model();
                    let focus = answer_id(model.highlight);
                    (vec![question_item(open, &model)], Some(focus))
                }
                None => (Vec::new(), None),
            },
            InputOwner::Help => match &self.overlays.help {
                Some(open) => (vec![help_item(&open.model)], Some(help_id())),
                None => (Vec::new(), None),
            },
            InputOwner::TileMenu | InputOwner::ProbeMenu | InputOwner::RecentList => match self.menu_model() {
                Some(menu) => {
                    let focus = menu
                        .highlighted()
                        .map_or_else(|| menu.id.clone(), |item| item.id.clone());
                    (vec![menu_item(&menu)], Some(focus))
                }
                None => (Vec::new(), None),
            },
            InputOwner::Canvas
            | InputOwner::Browser
            | InputOwner::Patches
            | InputOwner::Preferences
            | InputOwner::Chooser => (Vec::new(), None),
        }
    }
}

/// The question: an alert dialog named by its title, with its sentence as
/// a text inside it, and one button per answer. The sentence is a text's
/// value, not the dialog's description, because it must be heard: an
/// error's title is only "LoomDSP" (section 7, rule 26).
fn question_item(open: &question::Question, model: &question::Model) -> Item {
    let mut parts = vec![Item::text(question_id().with("text"), model.text.clone())];
    parts.extend(open.answers().iter().map(|&answer| {
        Item::button(
            answer_id(answer.name()),
            answer.label(),
            Some(Message::Question(question::Message::Answer(answer))),
        )
    }));
    Item {
        label: Some(model.title.clone()),
        children: parts,
        ..Item::new(question_id(), Kind::AlertDialog)
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
        parts.push(Item::text(
            id.clone().with("summary"),
            format!("{} · {} · {}", model.family, model.category, model.id),
        ));
        parts.push(Item::text(id.clone().with("description"), model.description));
    }
    // The page does not change while it is open, so its blocks are keyed
    // by their place on it (section 4: a list that never changes length).
    let (lead, tail) = split_example(&model.notes);
    parts.extend(notes(&id.clone().with("notes"), lead));
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
    parts.extend(notes(&id.clone().with("example-notes"), tail));
    Item {
        label: Some(model.name.to_string()),
        children: parts,
        ..Item::new(id, Kind::Dialog)
    }
}

/// The notes before the example section, and the section itself, as the
/// view splits them.
fn split_example(notes: &str) -> (&str, &str) {
    match notes.find(help::EXAMPLE_HEADING) {
        Some(at) => notes.split_at(at),
        None => (notes, ""),
    }
}

/// Our own notes, one item per paragraph, heading, list entry, or code
/// block, under `under`. A link `[label](target)` in a block is read as its
/// label in the block's text, and is also an item of its own that follows
/// the link, as a click on it in the view does.
fn notes(under: &UiId, text: &str) -> Vec<Item> {
    let mut items = Vec::new();
    for (at, block) in blocks(text).into_iter().enumerate() {
        let key = under.clone().with(at as u32);
        let (words, links) = read_links(&block);
        if words.trim().is_empty() {
            continue;
        }
        items.push(Item::text(key.clone(), words));
        for (n, (label, target)) in links.into_iter().enumerate() {
            items.push(Item {
                label: Some(label),
                click: Some(Message::HelpLink(target)),
                ..Item::new(key.clone().with("link").with(n as u32), Kind::Link)
            });
        }
    }
    items
}

/// The blocks of our notes: paragraphs split at blank lines, each list
/// entry and heading on its own, and a fenced code block whole.
fn blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut paragraph = Vec::new();
    let mut code: Option<Vec<&str>> = None;
    let flush = |paragraph: &mut Vec<&str>, blocks: &mut Vec<String>| {
        if !paragraph.is_empty() {
            blocks.push(paragraph.join(" "));
            paragraph.clear();
        }
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(lines) = &mut code {
            if trimmed.starts_with("```") {
                blocks.push(lines.join("\n"));
                code = None;
            } else {
                lines.push(line);
            }
            continue;
        }
        if trimmed.starts_with("```") {
            flush(&mut paragraph, &mut blocks);
            code = Some(Vec::new());
        } else if trimmed.is_empty() {
            flush(&mut paragraph, &mut blocks);
        } else if let Some(entry) = list_entry(trimmed) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(entry.to_string());
        } else if let Some(heading) = trimmed.strip_prefix('#') {
            flush(&mut paragraph, &mut blocks);
            blocks.push(heading.trim_start_matches('#').trim().to_string());
        } else {
            paragraph.push(trimmed);
        }
    }
    if let Some(lines) = code {
        blocks.push(lines.join("\n"));
    }
    flush(&mut paragraph, &mut blocks);
    blocks
}

/// The text of a list entry, without its marker, or `None` for a line that
/// is not one.
fn list_entry(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        return Some(rest);
    }
    let digits = line.find(|c: char| !c.is_ascii_digit())?;
    (digits > 0).then(|| line[digits..].strip_prefix(". ")).flatten()
}

/// `block` with each `[label](target)` read as its label, and the links in
/// it, in order. Emphasis marks and code marks are dropped, since a screen
/// reader would say them.
fn read_links(block: &str) -> (String, Vec<(String, String)>) {
    let mut words = String::new();
    let mut links = Vec::new();
    let mut rest = block;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        // A label holds no `[`: one before the `](` starts the label, so
        // this bracket is plain text and the next one is tried.
        let found = after
            .find("](")
            .filter(|&close| !after[..close].contains('['))
            .and_then(|close| {
                let target = &after[close + 2..];
                target.find(')').map(|end| (close, &target[..end], close + 2 + end + 1))
            });
        match found {
            Some((close, target, used)) => {
                words.push_str(&rest[..open]);
                let label = &after[..close];
                words.push_str(label);
                links.push((plain(label), target.to_string()));
                rest = &after[used..];
            }
            None => {
                words.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    words.push_str(rest);
    (plain(&words), links)
}

/// Text without the marks of emphasis and code.
fn plain(text: &str) -> String {
    text.replace(['`', '*'], "")
}

#[cfg(test)]
mod tests;
