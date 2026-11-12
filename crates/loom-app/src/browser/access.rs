//! What a screen reader finds in the module browser: a dialog named "Add
//! module" with its search field, the list box of results, the text under
//! it, and a library row's menu while it is open (ui-layer.md, section 7,
//! rule 27). Built from the browser's model, which its view draws too.

use super::{INSERT_LINKED, Message, Model, about_id, browser_id, insert_linked_id, list_id, menu_id, search_id};
use crate::Message as AppMessage;
use crate::a11y::items::{Item, Kind, SetText};
use loom_ui_core::UiId;

/// The browser's items, and where its keys act while no text field has
/// them: the menu's highlighted item, or the menu, while a row's menu is
/// open.
pub(crate) fn items(model: &Model) -> (Item, Option<UiId>) {
    let browser = |message: Message| AppMessage::Browser(message);
    let highlighted = model.highlight.and_then(|at| model.rows.get(at));
    let mut parts = vec![Item {
        label: Some("Search modules".to_string()),
        value: Some(model.query.clone()),
        set_text: Some(SetText::BrowserQuery),
        // The field keeps the keys while a menu is open, but the keys act on
        // the menu, so the menu has the reported focus then.
        focus: model
            .menu
            .is_none()
            .then(|| crate::places::field_widget_id(&search_id())),
        active: highlighted.map(|row| row.id.clone()),
        ..Item::new(search_id(), Kind::Field)
    }];
    match model.empty {
        Some(line) => parts.push(Item::text(list_id().with("empty"), line)),
        None => {
            let options = model
                .rows
                .iter()
                .enumerate()
                .map(|(at, row)| {
                    let selected = model.highlight == Some(at);
                    Item {
                        // The mark is in the name, as the row shows it beside
                        // the name: a sub-patch can share a module's name.
                        label: Some(format!("{}, {}", row.name, row.mark)),
                        // Every option has its own, so a reader browsing the
                        // list with its own cursor hears each one, and it
                        // does not change as the highlight moves.
                        description: Some(row.description.clone()),
                        selected: Some(selected),
                        click: Some(browser(Message::Pick(at))),
                        menu: row.library.then(|| browser(Message::Menu { row: at, by_key: true })),
                        ..Item::new(row.id.clone(), Kind::ListBoxOption)
                    }
                })
                .collect();
            parts.push(Item {
                label: Some("Modules".to_string()),
                children: options,
                ..Item::new(list_id(), Kind::ListBox)
            });
        }
    }
    if let Some(about) = &model.about {
        parts.push(Item::text(about_id(), format!("{} {}", about.text, about.detail)));
    }
    let mut focus = None;
    if let Some(open) = model.menu
        && let Some(row) = model.rows.get(open)
    {
        let item = insert_linked_id(&row.id);
        focus = Some(match model.menu_highlight {
            Some(_) => item.clone(),
            None => menu_id(&row.id),
        });
        parts.push(Item {
            label: Some(row.name.clone()),
            children: vec![Item {
                label: Some(INSERT_LINKED.to_string()),
                click: Some(browser(Message::InsertLinked(open))),
                ..Item::new(item, Kind::MenuItem)
            }],
            ..Item::new(menu_id(&row.id), Kind::Menu)
        });
    }
    let dialog = Item {
        label: Some("Add module".to_string()),
        children: parts,
        ..Item::new(browser_id(), Kind::Dialog)
    };
    (dialog, focus)
}

#[cfg(test)]
mod tests;
