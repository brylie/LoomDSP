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
use crate::menus::MenuModel;
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
