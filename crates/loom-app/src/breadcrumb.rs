//! The breadcrumb: "Patch", then one entry per level of the sub-patch
//! shown, with "›" between them, each a plain button that opens its level
//! ([sub-patches](../../../docs/design/sub-patches.md)). It shows only
//! inside a sub-patch. `loom-ui` draws it (plan for leaving Iced, step 3.2,
//! row 2), in a slot under the toolbar's in the window's root panel
//! (`chrome.rs`), and its elements give its nodes: a "Patch levels" group
//! with the open level marked current (ui-layer.md, section 7).

use crate::style::{self, Role as ButtonRole};
use crate::{App, Message};
use loom_canvas::iced_convert::ToCore as _;
use loom_patch::ModulePath;
use loom_ui::access::{Access, Action, Role, States};
use loom_ui::{Align, Element, Layer, Length, Padding, View};
use loom_ui_core::{Color, IdKey, UiId};

/// The breadcrumb's panel. Every entry's UI ID is inside it.
pub(crate) fn breadcrumb_id() -> UiId {
    UiId::panel("breadcrumb")
}

/// One entry.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Crumb {
    /// Its key under the panel: `patch` for the top patch, else the flat ID
    /// of its level's instance, which no other instance in the patch has.
    pub(crate) key: IdKey,
    /// The text it shows: "Patch", or the level's name.
    pub(crate) text: String,
    /// The level a click opens.
    pub(crate) level: ModulePath,
    /// It is the level shown now.
    pub(crate) current: bool,
}

/// What the breadcrumb's panel is drawn from.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Panel {
    /// The entries, from the top patch down.
    pub(crate) crumbs: Vec<Crumb>,
    /// The colour of the "›" between entries.
    pub(crate) separator: Color,
    /// The rule under it, the theme's strong background, as Iced's was.
    pub(crate) rule: Color,
}

impl App {
    /// The breadcrumb's entries, or none at the top level, where it does
    /// not show.
    pub(crate) fn crumbs(&self) -> Vec<Crumb> {
        let level = &self.session.level;
        if level.is_root() {
            return Vec::new();
        }
        let mut crumbs = vec![Crumb {
            key: IdKey::from("patch"),
            text: "Patch".to_string(),
            level: ModulePath::root(),
            current: false,
        }];
        let names = self.session.hierarchy.path_names(level);
        let depth = names.len();
        for (at, name) in names.into_iter().enumerate() {
            let target = level.prefix(at + 1);
            let parent = level.prefix(at);
            let Some(flat) = self.session.hierarchy.id_at(&parent, level.0[at]) else {
                continue;
            };
            crumbs.push(Crumb {
                key: IdKey::from(flat.0),
                text: name,
                level: target,
                current: at + 1 == depth,
            });
        }
        crumbs
    }

    /// Adds the breadcrumb's panel to the runtime's view, inside a
    /// sub-patch only.
    pub(crate) fn breadcrumb_panel(&self, view: &mut View) {
        let crumbs = self.crumbs();
        if crumbs.is_empty() {
            return;
        }
        let theme = self.theme();
        let palette = theme.extended_palette();
        let panel = Panel {
            crumbs,
            separator: style::QUIET.to_core(),
            rule: palette.background.strong.color.to_core(),
        };
        view.panel_with(breadcrumb_id(), Layer::Panels, &panel, breadcrumb);
    }

    /// The breadcrumb's focus stops in the order Tab meets them.
    pub(crate) fn breadcrumb_stops(&self) -> Vec<UiId> {
        self.crumbs()
            .into_iter()
            .map(|crumb| breadcrumb_id().with(crumb.key))
            .collect()
    }

    /// The message for a click on a breadcrumb entry: open its level. A
    /// click on the open level opens it again, which changes nothing
    /// (`show_level`).
    pub(crate) fn breadcrumb_click(&self, id: &UiId) -> Option<Message> {
        self.crumbs()
            .into_iter()
            .find(|crumb| &breadcrumb_id().with(crumb.key.clone()) == id)
            .map(|crumb| Message::OpenLevel(crumb.level))
    }
}

/// The breadcrumb: its entries with "›" between them, and the rule under
/// it.
fn breadcrumb(panel: &Panel) -> Element {
    let mut parts = Vec::new();
    for (at, crumb) in panel.crumbs.iter().enumerate() {
        if at > 0 {
            parts.push(Element::text("›", style::CONTROL_TEXT).colour(panel.separator));
        }
        parts.push(entry(crumb));
    }
    // As wide as the toolbar's, so a focused entry's ring clears the "›"
    // beside it (Brylie's decision, 2026-11-20).
    let row = Element::row(parts)
        .spacing(style::BUTTON_SPACING)
        .padding(Padding::symmetric(2.0, 8.0))
        .align_y(Align::Center)
        .width(Length::FILL)
        .height(Length::FILL);
    Element::column([
        row,
        Element::space(Length::FILL, Length::Fixed(style::RULE)).background(panel.rule),
    ])
    .width(Length::FILL)
    .height(Length::FILL)
}

/// One entry: a plain button. The open level's is marked current, and
/// says so in words too, since VoiceOver does not read the mark
/// (`accesskit_macos` 0.27.2).
fn entry(crumb: &Crumb) -> Element {
    let role = ButtonRole::Plain;
    let mut access = Access::new(Role::Button, crumb.text.clone()).action(Action::Click);
    if crumb.current {
        access = access
            .states(States {
                current: true,
                ..States::default()
            })
            .description("Open now");
    }
    Element::text(crumb.text.clone(), style::CONTROL_TEXT)
        .padding(style::BUTTON_PADDING)
        .radius(role.tokens().radius)
        .looks(role.looks())
        .key(crumb.key.clone())
        .focusable()
        .access(access)
}

#[cfg(test)]
mod tests;
