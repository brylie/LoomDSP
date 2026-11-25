//! The licence chooser as a panel of `loom-ui`'s Overlays layer
//! (patch-metadata.md, Licence chooser, Keys; plan 3.2, row 3c): a scrim
//! over the window holding a box with the title, one scrolling body with
//! the two questions, **Other** and its field, and what the answers give,
//! then the three buttons, which stay in view. It is drawn from the
//! chooser's model, which the snapshot reads too, and its nodes are the
//! runtime's: the tree puts them in a dialog named by the title, with no
//! description (`a11y.rs`, `ours_overlay`).
//!
//! Each question is a radio group whose radios carry their explanations as
//! notes. **Other** is the one radio of a group of its own, which gives no
//! node (ui-layer.md, section 7, rule 29), and its field follows it.

use super::{Answer, DISCLAIMER, Given, Message, Model, Pick, UNANSWERED};
use crate::App;
use crate::input_owner::InputOwner;
use crate::style::{self, Role as ButtonRole};
use loom_canvas::iced_convert::ToCore as _;
use loom_ui::access::{Access, Action, Role, States};
use loom_ui::{Align, Element, Layer, Length, View};
use loom_ui_core::{Color, UiId};

/// The UI ID of the licence chooser's panel.
pub(crate) fn licence_id() -> UiId {
    UiId::panel("licence")
}

/// The UI ID of the scrolling body. Every part between the title and the
/// buttons is keyed under it, since a scroll container keeps its offset
/// by its UI ID: `licence/body/changes/yes`, `licence/body/other-text`.
pub(crate) fn body_id() -> UiId {
    licence_id().with("body")
}

/// The UI ID of a question's group: `changes` or `commercial`. An
/// answer's is under it, by the answer's key.
pub(crate) fn question_id(key: &str) -> UiId {
    body_id().with(key)
}

/// The UI ID of **Other**'s radio, the one radio of its group.
pub(crate) fn other_id() -> UiId {
    body_id().with("other").with("expression")
}

/// The UI ID of the **Other** field.
pub(crate) fn other_text_id() -> UiId {
    body_id().with("other-text")
}

/// The UI ID of one of the buttons: `clear`, `cancel`, or `use`.
pub(crate) fn button_id(key: &str) -> UiId {
    licence_id().with(key)
}

/// Everything the chooser's panel shows, so the runtime builds it again
/// only when this changes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Panel {
    /// What the chooser shows.
    model: Model,
    /// The colours it is painted in.
    looks: Looks,
}

/// The colours the chooser is painted in, from the style tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Looks {
    scrim: Color,
    fill: Color,
    text: Color,
    quiet: Color,
    radius: f32,
    field: style::FieldLook,
}

impl App {
    /// Adds the chooser's panel to the runtime's view, while it is open.
    /// Help and a question can open over it, and cover it then: it is
    /// still drawn, under them, but only the top overlay is reported and
    /// acts (`a11y/tree.rs`, `Frame::ours`).
    pub(crate) fn chooser_panel(&self, view: &mut View) {
        let Some(chooser) = &self.overlays.chooser else {
            return;
        };
        let theme = self.theme();
        let dialog = style::dialog(&theme);
        let looks = Looks {
            scrim: style::SCRIM,
            fill: dialog.fill,
            text: dialog.text,
            quiet: style::QUIET.to_core(),
            radius: dialog.radius,
            field: style::field(&theme),
        };
        let panel = Panel {
            model: chooser.model(),
            looks,
        };
        view.panel_with(licence_id(), Layer::Overlays, &panel, build);
    }

    /// True while the chooser owns the input: open, with no help or
    /// question over it.
    fn chooser_owns(&self) -> bool {
        self.input_owner() == InputOwner::Chooser
    }

    /// The message for a click on a part of the chooser, from the pointer,
    /// a key that chooses a radio, Enter or Space on a button, or a screen
    /// reader. `None` for any other part, and while it is covered.
    pub(crate) fn chooser_click(&self, id: &UiId) -> Option<crate::Message> {
        if !self.chooser_owns() {
            return None;
        }
        let model = self.overlays.chooser.as_ref()?.model();
        let pick = |key: &str, picks: &[Pick]| {
            picks
                .iter()
                .find(|pick| question_id(key).with(pick.key) == *id)
                .map(|pick| pick.message.clone())
        };
        let message = if *id == other_id() {
            Some(Message::Answer(Answer::Other))
        } else if *id == button_id("clear") {
            Some(Message::Clear)
        } else if *id == button_id("cancel") {
            Some(Message::Cancel)
        } else if *id == button_id("use") {
            model.ready.then_some(Message::Use)
        } else {
            pick("changes", &model.changes).or_else(|| pick("commercial", model.commercial.as_deref()?))
        };
        message.map(crate::Message::Licence)
    }

    /// Every focus stop of the chooser, while it owns the input: each
    /// radio, the field, and the buttons, **Use this licence** only while
    /// it is turned on. A screen reader's Focus can reach any of them; Tab
    /// meets each question once ([`Self::chooser_tab`]).
    pub(crate) fn chooser_stops(&self) -> Vec<UiId> {
        let Some(model) = self.overlays.chooser.as_ref().map(super::Chooser::model) else {
            return Vec::new();
        };
        if !self.chooser_owns() {
            return Vec::new();
        }
        let mut stops: Vec<UiId> = model
            .changes
            .iter()
            .map(|pick| question_id("changes").with(pick.key))
            .collect();
        for pick in model.commercial.iter().flatten() {
            stops.push(question_id("commercial").with(pick.key));
        }
        stops.extend([other_id(), other_text_id(), button_id("clear"), button_id("cancel")]);
        if model.ready {
            stops.push(button_id("use"));
        }
        stops
    }

    /// Where Tab goes from the chooser's stop `from`, or `None` when `from`
    /// is not one of them. `next` is the runtime's next stop in reading
    /// order, which already meets each question once: Tab stays inside and
    /// wraps at both ends, from the last stop to the first question and
    /// back (the WAI-ARIA modal dialog pattern; patch-metadata.md, Keys).
    pub(crate) fn chooser_tab(&self, from: &UiId, next: Option<&UiId>, back: bool) -> Option<UiId> {
        if !from.is_under(&licence_id()) || !self.chooser_owns() {
            return None;
        }
        if let Some(next) = next.filter(|next| next.is_under(&licence_id())) {
            return Some(next.clone());
        }
        let chooser = self.overlays.chooser.as_ref()?;
        Some(if back {
            last_stop(&chooser.model())
        } else {
            chooser.first_stop()
        })
    }
}

impl super::Chooser {
    /// Where the focus goes when the chooser opens: the first question's
    /// chosen answer, or its first when none is, the first focusable
    /// element, as the WAI-ARIA modal dialog pattern says
    /// (patch-metadata.md, Keys).
    pub(crate) fn first_stop(&self) -> UiId {
        let model = self.model();
        let pick = model
            .changes
            .iter()
            .find(|pick| pick.chosen)
            .or_else(|| model.changes.first())
            .map_or("yes", |pick| pick.key);
        question_id("changes").with(pick)
    }
}

/// The last stop: **Use this licence** while it is on, Cancel while not.
fn last_stop(model: &Model) -> UiId {
    button_id(if model.ready { "use" } else { "cancel" })
}

/// The scrim, opaque, with the box centred in it. The box shrinks to what
/// it holds, up to [`style::CHOOSER_HEIGHT`] and the window's height, and
/// the body scrolls inside it.
fn build(panel: &Panel) -> Element {
    let model = &panel.model;
    let looks = panel.looks;
    let mut body = vec![question(
        "changes",
        "Allow others to change it and share the result?",
        &model.changes,
        looks,
    )];
    if let Some(picks) = &model.commercial {
        body.push(question("commercial", "Allow commercial use?", picks, looks));
    }
    body.push(rule(looks));
    body.push(other(model, looks));
    body.push(rule(looks));
    body.push(result(&model.given, looks));
    let content = Element::column(body).spacing(12.0).width(Length::FILL);
    let scroll = Element::scroll(content, style::scroll())
        .key("body")
        .height(Length::FILL)
        .min_height(0.0)
        .width(Length::FILL)
        .access(Access::new(Role::ScrollView, model.title));
    let buttons = Element::row([
        button("clear", model.clear, ButtonRole::Secondary, true),
        Element::space(Length::FILL, Length::Shrink),
        button("cancel", "Cancel", ButtonRole::Secondary, true),
        button("use", "Use this licence", ButtonRole::Primary, model.ready),
    ])
    .spacing(style::BUTTON_SPACING)
    .align_y(Align::Center)
    .width(Length::FILL);
    let dialog = Element::column([
        Element::text(model.title, style::TITLE_TEXT).colour(looks.text),
        scroll,
        buttons,
    ])
    .spacing(12.0)
    .padding(16.0)
    .width(Length::Fixed(style::CHOOSER_WIDTH))
    .max_height(style::CHOOSER_HEIGHT)
    .min_height(0.0)
    .background(looks.fill)
    .radius(looks.radius);
    Element::column([dialog])
        .width(Length::FILL)
        .height(Length::FILL)
        .padding(30.0)
        .align_x(Align::Center)
        .align_y(Align::Center)
        .background(looks.scrim)
        .opaque()
}

/// A question: its heading, then its answers as one radio group named by
/// it, each with its explanation as its note.
fn question(key: &'static str, heading: &'static str, picks: &[Pick], looks: Looks) -> Element {
    let radios = picks.iter().map(|pick| {
        Element::radio(pick.label, style::CONTROL_TEXT, pick.chosen)
            .note(pick.note, style::BODY_TEXT)
            .key(pick.key)
    });
    Element::column([
        Element::text(heading, style::HEADING_TEXT).colour(looks.text).wrap(),
        Element::radio_group(heading, radios)
            .spacing(style::RADIO_SPACING)
            .key(key),
    ])
    .spacing(4.0)
    .width(Length::FILL)
}

/// **Other**: its radio, alone in its group, and its field under it,
/// indented to the radio's label.
fn other(model: &Model, looks: Looks) -> Element {
    let radio =
        Element::radio("Other: any SPDX licence expression", style::CONTROL_TEXT, model.other).key("expression");
    let field = Element::field(model.other_text.clone(), style::CONTROL_TEXT)
        .placeholder("For example MIT, or CC-BY-4.0 OR MIT", looks.quiet)
        .padding(style::FIELD_PADDING)
        .background(looks.field.fill)
        .border(style::RULE, looks.field.border)
        .radius(looks.field.radius)
        .width(Length::FILL)
        .key("other-text")
        .access(Access::new(Role::TextInput, "Other licence expression"));
    Element::column([
        Element::radio_group("Other", [radio]).key("other"),
        Element::row([field])
            .padding(loom_ui::Padding {
                left: style::RADIO_INDENT,
                ..loom_ui::Padding::default()
            })
            .width(Length::FILL),
    ])
    .spacing(4.0)
    .width(Length::FILL)
}

/// What the answers give: the licence and what it means, the error in the
/// Other text, or the line that asks for answers, then the disclaimer.
/// Each line is read as text.
fn result(given: &Given, looks: Looks) -> Element {
    let line = |key: &'static str, text: String, style, colour| {
        Element::text(text.clone(), style)
            .colour(colour)
            .wrap()
            .key(key)
            .access(Access::label(text))
    };
    let mut lines = Vec::new();
    match given {
        Given::Licence { short, summary, url } => {
            lines.push(line("short", short.clone(), style::HEADING_TEXT, looks.text));
            lines.push(line("summary", summary.clone(), style::BODY_TEXT, looks.text));
            if let Some(url) = url {
                lines.push(line("url", url.clone(), style::SMALL_TEXT, looks.quiet));
            }
        }
        Given::Error(error) => lines.push(line("error", error.clone(), style::BODY_TEXT, looks.text)),
        Given::Unlisted => {}
        Given::Open => lines.push(line("open", UNANSWERED.to_string(), style::BODY_TEXT, looks.quiet)),
    }
    lines.push(line(
        "disclaimer",
        DISCLAIMER.to_string(),
        style::SMALL_TEXT,
        looks.quiet,
    ));
    Element::column(lines).spacing(2.0).key("result").width(Length::FILL)
}

/// A thin rule across the body, between its parts.
fn rule(looks: Looks) -> Element {
    Element::space(Length::FILL, Length::Fixed(style::RULE)).background(looks.quiet)
}

/// A button keyed `key` in `role`, a focus stop while `on`; turned off,
/// it says so and offers nothing (ui-layer.md, section 6, rule 2).
fn button(key: &'static str, label: &'static str, role: ButtonRole, on: bool) -> Element {
    let access = Access::new(Role::Button, label);
    let access = if on {
        access.action(Action::Click)
    } else {
        access.states(States {
            disabled: true,
            ..States::default()
        })
    };
    Element::text(label, style::CONTROL_TEXT)
        .padding(style::BUTTON_PADDING)
        .radius(role.tokens().radius)
        .looks(role.looks())
        .key(key)
        .focusable()
        .access(access)
}

#[cfg(test)]
mod tests;
