//! What a screen reader finds in the licence chooser: a dialog named by
//! its title, with its two questions as radio groups, **Other** and its
//! field, what the answers give, and its three buttons. Built from the
//! chooser's model, which its view draws too (ui-layer.md, section 7,
//! rule 27).

use super::{DISCLAIMER, Given, Message, Model, Pick, UNANSWERED, other_part};
use crate::Message as AppMessage;
use crate::a11y::items::{Item, Kind, SetText};
use loom_ui_core::UiId;

/// The UI ID of the licence chooser.
pub(crate) fn licence_id() -> UiId {
    UiId::panel("licence")
}

/// The chooser's items, as one dialog.
pub(crate) fn items(model: &Model) -> Item {
    let id = licence_id();
    let send = |message: Message| AppMessage::Licence(message);
    let question = |key: &str, label: &str, picks: &[Pick]| Item {
        label: Some(label.to_string()),
        children: picks
            .iter()
            .map(|pick| Item {
                label: Some(pick.label.to_string()),
                description: Some(pick.note.to_string()),
                on: Some(pick.chosen),
                click: (!pick.chosen).then(|| send(pick.message.clone())),
                ..Item::new(id.clone().with(key).with(pick.key), Kind::Option)
            })
            .collect(),
        ..Item::new(id.clone().with(key), Kind::Options)
    };
    let mut parts = vec![question(
        "changes",
        "Allow others to change it and share the result?",
        &model.changes,
    )];
    if let Some(picks) = &model.commercial {
        parts.push(question("commercial", "Allow commercial use?", picks));
    }
    parts.push(Item {
        label: Some("Other: any SPDX licence expression".to_string()),
        on: Some(model.other),
        click: (!model.other).then(|| send(Message::Answer(super::Answer::Other))),
        ..Item::new(id.clone().with("other"), Kind::Option)
    });
    parts.push(Item {
        label: Some("Other licence expression".to_string()),
        value: Some(model.other_text.clone()),
        set_text: Some(SetText::LicenceOther),
        focus: Some(crate::places::field_widget_id(&other_part())),
        ..Item::new(other_part(), Kind::Field)
    });
    let result = id.clone().with("result");
    match &model.given {
        Given::Licence { short, summary, url } => {
            parts.push(Item::text(result.clone().with("short"), short.clone()));
            parts.push(Item::text(result.clone().with("summary"), summary.clone()));
            if let Some(url) = url {
                parts.push(Item::text(result.clone().with("url"), url.clone()));
            }
        }
        Given::Error(error) => parts.push(Item::text(result.clone().with("error"), error.clone())),
        Given::Unlisted => {}
        Given::Open => parts.push(Item::text(result.clone().with("open"), UNANSWERED)),
    }
    parts.push(Item::text(result.with("disclaimer"), DISCLAIMER));
    parts.push(Item::button(
        id.clone().with("clear"),
        model.clear,
        Some(send(Message::Clear)),
    ));
    parts.push(Item::button(
        id.clone().with("cancel"),
        "Cancel",
        Some(send(Message::Cancel)),
    ));
    parts.push(Item::button(
        id.clone().with("use"),
        "Use this licence",
        model.ready.then(|| send(Message::Use)),
    ));
    Item {
        label: Some(model.title.to_string()),
        children: parts,
        ..Item::new(id, Kind::Dialog)
    }
}

#[cfg(test)]
mod tests;
