//! The stats bar and the status line, drawn by `loom-ui` (plan for leaving
//! Iced, step 3.2, row 1; ui-layer.md, section 5, rule 10).
//!
//! The bar is the `stats` panel in the Panels layer, in a slot of
//! [`style::STATS_BAR_HEIGHT`] at the foot of the window's root panel
//! (`chrome.rs`), so the bar and each of its items keep their own place
//! and UI ID (`stats`, `stats/load`, `stats/status/dismiss`, ...). Iced
//! keeps a transparent space of the same height where the bar was
//! (`view.rs`), from the same token.
//!
//! Both draw from [`bars::Model`], which the snapshot reads too, and each
//! item says what it is to a screen reader from the same element (section
//! 7, rule 1). The app puts those nodes in its "Status bar" group
//! (`a11y/tree.rs`).

use crate::bars::{self, Field, Item, Kind, Model};
use crate::{App, Message, alerts, style};
use loom_ui::access::{Access, Action, Role, Value};
use loom_ui::{Align, Element, Layer, Length, Padding, View};
use loom_ui_core::{Color, UiId};

/// The bar's panel. Every item's UI ID is inside it.
pub(crate) fn stats_id() -> UiId {
    UiId::panel("stats")
}

/// What the bar is drawn from: the model, and the colours of its parts that
/// come from the palette tokens in `style.rs`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Bar {
    /// The items, the status line, and the developer switches.
    pub model: Model,
    /// A switch's track when it is on.
    pub on: Color,
    /// A switch's track when it is off.
    pub off: Color,
    /// A switch's knob.
    pub knob: Color,
}

impl App {
    /// Adds the bar's panel to the runtime's view, for its slot in the
    /// window's root panel.
    pub(crate) fn bar_panel(&self, view: &mut View) {
        let bar = Bar {
            model: self.bars_model(),
            on: crate::style::ACCENT,
            off: crate::style::EDGE,
            knob: crate::style::TEXT,
        };
        view.panel_with(stats_id(), Layer::Panels, &bar, stats);
    }

    /// The bar's focus stops in the order Tab meets them: the developer
    /// switches, then the kept error's ×, then the status line's. A test
    /// checks this against the runtime's own order.
    pub(crate) fn bar_stops(&self) -> Vec<UiId> {
        let model = self.bars_model();
        let mut stops = Vec::new();
        if model.dev.is_some() {
            stops.push(stats_id().with("measure"));
            stops.push(stats_id().with("move_camera"));
        }
        if model.kept.as_ref().is_some_and(|kept| kept.dismiss.is_some()) {
            stops.push(stats_id().with("kept").with("dismiss"));
        }
        if model.status.dismiss.is_some() {
            stops.push(stats_id().with("status").with("dismiss"));
        }
        stops
    }

    /// The message for a click, from the pointer, Enter or Space, or a
    /// screen reader, on one of the bar's elements: a developer switch
    /// turns over, and a × dismisses its message.
    pub(crate) fn bar_click(&self, id: &UiId) -> Option<Message> {
        let model = self.bars_model();
        let fixed = |text: &str| text.parse::<UiId>().ok();
        // The switches act only while they show.
        if Some(id) == fixed(bars::MEASURE_ID).as_ref() {
            return model.dev.map(|dev| Message::Measure(!dev.measuring));
        }
        if Some(id) == fixed(bars::MOVE_CAMERA_ID).as_ref() {
            return model.dev.map(|dev| Message::Animate(!dev.animating));
        }
        if Some(id) == fixed(bars::STATUS_DISMISS_ID).as_ref() {
            return model.status.dismiss.map(Message::DismissError);
        }
        if Some(id) == fixed(bars::KEPT_DISMISS_ID).as_ref() {
            return model.kept.and_then(|kept| kept.dismiss).map(Message::DismissError);
        }
        None
    }
}

/// The bar, left to right: the developer switches, the fixed items, then
/// the kept error and the status, which share what width is left and are
/// cut off rather than wrapped, with all of it in hover text.
fn stats(bar: &Bar) -> Element {
    let model = &bar.model;
    let mut parts = Vec::new();
    if let Some(dev) = &model.dev {
        parts.push(switch(bar, "measure", "Measure", dev.measuring));
        parts.push(switch(bar, "move_camera", "Move camera", dev.animating));
        parts.push(Element::text(dev.frames.clone(), style::BODY_TEXT));
    }
    // The audio items sit closer together than the rest of the bar.
    const AUDIO: [&str; 7] = ["audio", "load", "dropouts", "input", "silenced", "clipped", "telemetry"];
    let named = |names: &[&str]| -> Vec<Element> {
        model
            .items
            .iter()
            .filter(|item| names.contains(&item.name))
            .map(item)
            .collect()
    };
    parts.extend(named(&["patch"]));
    parts.push(Element::row(named(&AUDIO)).spacing(10.0).align_y(Align::Center));
    parts.extend(named(&["midi", "learning"]));
    if let Some(kept) = &model.kept {
        parts.push(field(kept, "kept"));
    }
    parts.push(field(&model.status, "status"));
    Element::row(parts)
        .spacing(16.0)
        .padding(Padding::symmetric(4.0, 10.0))
        .align_y(Align::Center)
        .width(Length::FILL)
        .height(Length::FILL)
}

/// One fixed item: its text in its tone, with its hover text. A warning is
/// a status, so a screen reader announces it; the rest are labels.
fn item(item: &Item) -> Element {
    let access = match item.kind {
        Kind::Alert => Access::new(Role::Status, "").value(Value::Text(item.text.clone())),
        Kind::Text | Kind::Live => Access::label(item.text.clone()),
    };
    let text = Element::text(item.text.clone(), style::BODY_TEXT);
    let text = match item.tone {
        Some(severity) => text.colour(tone(severity)),
        None => text,
    };
    let text = text.key(item.name).access(access);
    match &item.hover {
        Some(hover) => text.hover_text(hover.clone()),
        None => text,
    }
}

/// The status or the kept error, keyed `key`: its text cut to its share of
/// the width, aligned to the end, with all of it in hover text, and a × if
/// the player may dismiss it. Ordinary status text is a label; a warning
/// or an error is a status. Empty, it has no node and no hover text.
fn field(field: &Field, key: &str) -> Element {
    let plain = field.severity == alerts::Severity::Normal;
    let access = if plain {
        Access::label(field.shown.clone())
    } else {
        Access::new(Role::Status, "").value(Value::Text(field.shown.clone()))
    };
    // A width of 0 at least, so a long message shrinks and is cut, as
    // Iced's clip did, rather than pushing the fixed items out of the bar
    // (a flex item never shrinks below its content by default).
    let words = Element::row([Element::text(field.shown.clone(), style::BODY_TEXT).colour(tone(field.severity))])
        .width(Length::FILL)
        .min_width(0.0)
        .align_x(Align::End)
        .clip();
    let mut parts = vec![words];
    if field.dismiss.is_some() {
        let hover = field.dismiss_hover();
        parts.push(
            Element::text("×", style::CONTROL_TEXT)
                .padding(Padding::symmetric(0.0, 6.0))
                .key("dismiss")
                .focusable()
                .access(Access::new(Role::Button, hover).action(Action::Click))
                .hover_text(hover),
        );
    }
    let row = Element::row(parts)
        .spacing(4.0)
        .align_y(Align::Center)
        .width(Length::FILL)
        .min_width(0.0)
        .key(key.to_string());
    // An empty status line has nothing to read or to point at.
    if field.shown.is_empty() {
        row
    } else {
        row.access(access).hover_text(field.hover.clone())
    }
}

/// A developer switch keyed `key`: a track with its knob at the end it is
/// turned to, and its label.
fn switch(bar: &Bar, key: &str, label: &str, on: bool) -> Element {
    let size = style::SWITCH;
    let knob = size - 4.0;
    let track = Element::row([Element::space(Length::Fixed(knob), Length::Fixed(knob))
        .background(bar.knob)
        .radius(knob / 2.0)])
    .width(Length::Fixed(size * 2.0))
    .height(Length::Fixed(size))
    .padding(2.0)
    .align_x(if on { Align::End } else { Align::Start })
    .align_y(Align::Center)
    .background(if on { bar.on } else { bar.off })
    .radius(size / 2.0);
    Element::row([track, Element::text(label, style::BODY_TEXT)])
        .spacing(6.0)
        .align_y(Align::Center)
        .key(key.to_string())
        .focusable()
        .access(
            Access::new(Role::Switch, label)
                .value(Value::On(on))
                .action(Action::Click),
        )
}

/// The bar's colour for a warning level, as the window drew it.
fn tone(severity: alerts::Severity) -> Color {
    match severity {
        alerts::Severity::Normal => crate::style::QUIET,
        alerts::Severity::Warning => crate::style::WARNING_TEXT,
        alerts::Severity::Alarm => crate::style::ERROR_TEXT,
    }
}

#[cfg(test)]
mod tests;
