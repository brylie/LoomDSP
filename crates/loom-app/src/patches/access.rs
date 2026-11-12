//! What a screen reader finds in Your patches: a dialog with its Close
//! button, its two fields, the licence check boxes, the tags as toggle
//! buttons, and the list box of patches (ui-layer.md, section 7, rule 27).
//! Built from the browser's model, which its view draws too.

use super::{AUTHOR, Message, Model, SEARCH, licence_id, part_id, patches_id, tag_id};
use crate::Message as AppMessage;
use crate::a11y::items::{Item, Kind, SetText};

/// The dialog's items. The keys act on the field that has them, so no other
/// part is reported as focused.
pub(crate) fn items(model: &Model) -> Item {
    let patches = |message: Message| AppMessage::Patches(message);
    let highlighted = model.highlight.and_then(|at| model.rows.get(at));
    let field = |name: &'static str, label: &str, text: &str, set: SetText, widget: &'static str| Item {
        label: Some(label.to_string()),
        value: Some(text.to_string()),
        set_text: Some(set),
        focus: Some(iced::advanced::widget::Id::new(widget)),
        active: highlighted.map(|row| row.id.clone()),
        ..Item::new(part_id(name), Kind::Field)
    };
    let mut parts = vec![
        Item::button(part_id("close"), "Close", Some(patches(Message::Close))),
        field(
            "search",
            "Search names, descriptions, and tags",
            &model.search,
            SetText::PatchesSearch,
            SEARCH,
        ),
        field("author", "Author", &model.author, SetText::PatchesAuthor, AUTHOR),
    ];
    parts.extend(model.licences.iter().map(|licence| Item {
        label: Some(licence.label.to_string()),
        on: Some(licence.on),
        click: Some(patches(Message::Licence(licence.filter, !licence.on))),
        ..Item::new(licence_id(licence.key), Kind::Check)
    }));
    // A tag is a toggle button: on while it filters, as the Learn button is.
    parts.extend(model.tags.iter().map(|tag| Item {
        on: Some(tag.picked),
        ..Item::button(
            tag_id(&tag.text),
            tag.text.clone(),
            Some(patches(Message::Tag(tag.text.clone()))),
        )
    }));
    match &model.empty {
        Some(line) => parts.push(Item::text(part_id("empty"), line.clone())),
        None => parts.push(Item {
            label: Some("Patches".to_string()),
            children: model
                .rows
                .iter()
                .enumerate()
                .map(|(at, row)| Item {
                    // A file that does not read says so in its name, so it
                    // is heard, and opens nothing; it stays enabled, so a
                    // reader does not skip it.
                    label: Some(if row.readable {
                        row.name.clone()
                    } else {
                        format!("{}, could not read", row.name)
                    }),
                    description: row
                        .readable
                        .then(|| row.details.clone())
                        .filter(|text| !text.is_empty()),
                    selected: Some(model.highlight == Some(at)),
                    click: row.readable.then(|| patches(Message::Pick(at))),
                    ..Item::new(row.id.clone(), Kind::ListBoxOption)
                })
                .collect(),
            ..Item::new(part_id("list"), Kind::ListBox)
        }),
    }
    parts.push(Item::text(part_id("folder"), model.folder.clone()));
    Item {
        label: Some("Your patches".to_string()),
        children: parts,
        ..Item::new(patches_id(), Kind::Dialog)
    }
}

#[cfg(test)]
mod tests;
