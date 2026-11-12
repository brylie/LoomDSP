//! What a screen reader finds in the Preferences window: a dialog named
//! "Preferences" with its parts in screen order, built from the window's
//! model, which its view draws too (ui-layer.md, section 7, rule 27).
//!
//! A list is a choice: it says its chosen option and steps one option at a
//! time (rule 15). Its open menu is Iced's and is not reported.

use super::model::{Model, Note, Part, Picks};
use super::{AUTHOR_INPUT, Message};
use crate::Message as AppMessage;
use crate::a11y::items::{Item, Kind, SetText};
use loom_ui_core::UiId;

/// The UI ID of the Preferences window.
pub(crate) fn preferences_id() -> UiId {
    UiId::panel("preferences")
}

/// The UI ID of one of its parts, by its key.
pub(crate) fn part_id(key: &str) -> UiId {
    preferences_id().with(key)
}

/// The window's items, as one dialog.
pub(crate) fn items(model: &Model) -> Item {
    let id = preferences_id();
    let send = |message: Message| AppMessage::Preferences(message);
    let mut parts = vec![Item::button(
        id.clone().with("close"),
        "Close",
        Some(send(Message::Close)),
    )];

    let safety = model
        .safety
        .iter()
        .map(|mode| Item {
            label: Some(mode.label.to_string()),
            description: Some(mode.note.to_string()),
            on: Some(mode.chosen),
            click: (!mode.chosen).then(|| send(Message::Safety(mode.mode))),
            ..Item::new(id.clone().with("safety").with(mode.key), Kind::Option)
        })
        .collect();
    parts.push(Item {
        label: Some("Output safety".to_string()),
        children: safety,
        ..Item::new(id.clone().with("safety"), Kind::Options)
    });
    parts.extend(model.safety_notes.iter().map(|line| note(&id, line)));

    parts.push(Item {
        label: Some("Show signal activity on ports".to_string()),
        on: Some(model.activity),
        click: Some(send(Message::Activity(!model.activity))),
        ..Item::new(id.clone().with("activity"), Kind::Check)
    });

    for part in &model.audio {
        match part {
            Part::Picks(picks) => {
                parts.push(choice(&id, picks));
                if picks.key == "device" {
                    parts.push(Item::button(
                        id.clone().with("refresh"),
                        "Refresh",
                        Some(send(Message::Refresh)),
                    ));
                }
            }
            Part::Note(line) => parts.push(note(&id, line)),
            Part::Midi { .. } => {}
        }
    }

    for part in &model.midi {
        match part {
            Part::Midi { name, on, id } => parts.push(Item {
                label: Some(name.clone()),
                on: Some(*on),
                click: Some(send(Message::Midi(name.clone(), !on))),
                ..Item::new(id.clone(), Kind::Check)
            }),
            Part::Note(line) => parts.push(note(&id, line)),
            Part::Picks(_) => {}
        }
    }

    let panic = id.clone().with("panic");
    parts.push(Item::text(panic.clone(), model.panic.line.clone()));
    parts.push(Item::button(
        panic.clone().with("learn"),
        if model.panic.learning { "Cancel" } else { "Learn" },
        Some(send(Message::PanicLearn)),
    ));
    parts.push(Item::button(
        panic.with("clear"),
        "Clear",
        model.panic.can_clear.then(|| send(Message::PanicClear)),
    ));

    parts.push(Item {
        label: Some("Your name".to_string()),
        value: Some(model.author.clone()),
        set_text: Some(SetText::PreferencesAuthor),
        focus: Some(iced::advanced::widget::Id::new(AUTHOR_INPUT)),
        ..Item::new(id.clone().with("author"), Kind::Field)
    });
    let licence = id.clone().with("licence");
    parts.push(Item {
        label: Some("Licence".to_string()),
        value: Some(model.licence.clone()),
        ..Item::new(licence.clone(), Kind::Text)
    });
    parts.push(Item::button(
        licence.with("choose"),
        "Choose",
        Some(send(Message::ChooseLicence)),
    ));

    Item {
        label: Some("Preferences".to_string()),
        children: parts,
        ..Item::new(id, Kind::Dialog)
    }
}

/// A line of text under `window`, keyed by what it is about.
fn note(window: &UiId, line: &Note) -> Item {
    Item::text(window.clone().with("note").with(line.key), line.text.clone())
}

/// A list as a choice: its chosen option as the value, and a step to the
/// next and previous options. A step goes on from an option that could
/// not be used, so a broken device never blocks the ones after it. With
/// nothing chosen, a step up picks the first, as a pointer can pick any.
fn choice(window: &UiId, picks: &Picks) -> Item {
    let send = |at: Option<usize>| at.and_then(|at| picks.pick(at)).map(AppMessage::Preferences);
    Item {
        label: Some(picks.label.to_string()),
        value: Some(picks.chosen_text().unwrap_or("None").to_string()),
        step: match picks.step_from() {
            Some(at) => (send(Some(at + 1)), send(at.checked_sub(1))),
            None => (send(Some(0)), None),
        },
        ..Item::new(window.clone().with(picks.key), Kind::Choice)
    }
}

#[cfg(test)]
mod tests;
