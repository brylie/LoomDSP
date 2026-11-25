//! The overlay that owns the input, as a screen reader is told it: its
//! [`Item`]s, built from the same models its view draws (ADR-0021), with
//! the same UI IDs and messages (`docs/design/ui-layer.md`, section 7,
//! rule 27).
//!
//! Only the overlay that owns the input is reported. What it covers, the
//! window and any overlay under it, stays hidden (rule 24). Every overlay
//! is reported: the question, help, the menus, the browsers, Preferences,
//! and the licence chooser.

use crate::App;
use crate::a11y::items::{Item, Kind};
use crate::input_owner::InputOwner;
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
            // Help is the runtime's (row 6): its nodes come from its panel.
            InputOwner::Help => (Vec::new(), None),
            // The menus are the runtime's (rows 3b and 3d), except the cable
            // menu, which Iced still draws (#856): its items come from here.
            InputOwner::TileMenu | InputOwner::ProbeMenu | InputOwner::RecentList => {
                match self
                    .menu_model()
                    .filter(|menu| menu.kind == crate::menus::MenuKind::Cable)
                {
                    Some(menu) => {
                        let focus = menu
                            .highlight
                            .and_then(|at| menu.items.get(at))
                            .map_or_else(|| menu.id.clone(), |item| item.id.clone());
                        (vec![menu_item(&menu)], Some(focus))
                    }
                    None => (Vec::new(), None),
                }
            }
            // The module browser is the runtime's (row 5): its nodes come
            // from its panels, and its focus is the runtime's search field.
            InputOwner::Browser => (Vec::new(), None),
            // Your patches is the runtime's (row 5b): its nodes come from
            // its panels.
            InputOwner::Patches => (Vec::new(), None),
            // No part of these has the keys until a field does, so the
            // dialog itself has the focus, as help does.
            InputOwner::Preferences => match self.preferences_model() {
                Some(model) => (
                    vec![crate::settings::access::items(&model)],
                    Some(crate::settings::access::preferences_id()),
                ),
                None => (Vec::new(), None),
            },
            // The licence chooser is the runtime's (row 3c).
            InputOwner::Chooser => (Vec::new(), None),
            InputOwner::Canvas => (Vec::new(), None),
        }
    }
}

/// The cable menu and its items, which Iced still draws (#856). The
/// highlighted item is reported as focus, not as a state of its own.
fn menu_item(menu: &crate::menus::MenuModel) -> Item {
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
