//! Preferences as a panel of `loom-ui`'s Overlays layer (preferences.md,
//! Keys and focus; plan 3.2, row 8): a backdrop over the window that
//! takes every press and does nothing with it, and a box with the title
//! and Close above one scrolling body. The body holds every part, keyed
//! under it, since a scroll container keeps its offset by its UI ID. It is
//! drawn from the window's plain-data model, so the view, the tree, and
//! the UI snapshot read one source (ui-layer.md, section 7, rule 1).

use super::{Message, Model, Part, Picks};
use crate::App;
use crate::preferences::MapMode;
use crate::style::{self, Role as ButtonRole};
use loom_canvas::iced_convert::ToCore as _;
use loom_ui::access::{Access, Action, Role, States};
use loom_ui::{Align, Element, Layer, Length, Select, SelectStyle, View};
use loom_ui_core::{Color, IdKey, UiId};

/// The UI ID of the Preferences window.
pub(crate) fn preferences_id() -> UiId {
    UiId::panel("preferences")
}

/// The UI ID of its Close button, in the title row.
pub(crate) fn close_id() -> UiId {
    preferences_id().with("close")
}

/// The UI ID of its scrolling body. Every part below the title row is
/// keyed under it.
pub(crate) fn body_id() -> UiId {
    preferences_id().with("body")
}

/// The UI ID of the part `key` in the body.
pub(crate) fn part_id(key: &str) -> UiId {
    body_id().with(key)
}

/// The UI ID of the **Your name** field.
pub(crate) fn author_part() -> UiId {
    part_id("author")
}

/// The UI ID of the panic trigger's Learn button, where the focus goes
/// when Clear turns itself off.
pub(crate) fn learn_id() -> UiId {
    part_id("panic").with("learn")
}

/// The key of the backdrop's own panel, under the box's, so its press is
/// told apart from the box's.
fn backdrop_panel() -> UiId {
    preferences_id().with("backdrop")
}

/// The width of the label column beside a list or a field.
const LABEL_WIDTH: f32 = 100.0;

/// The width of the box.
const WIDTH: f32 = 560.0;

/// The most the box grows to, as before.
const MAX_HEIGHT: f32 = 720.0;

/// Everything the panel shows, so the runtime builds it again only when
/// this changes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Panel {
    /// The window's model.
    model: Model,
    /// The look of the backdrop, the box, and its text and lists.
    looks: Looks,
}

/// The colours Preferences is painted in, from the style tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Looks {
    scrim: Color,
    fill: Color,
    text: Color,
    quiet: Color,
    rule: Color,
    radius: f32,
    select: SelectStyle,
}

impl App {
    /// Adds Preferences' panels to the runtime's view, while it is open.
    pub(crate) fn preferences_panel(&self, view: &mut View) {
        let Some(model) = self.preferences_model() else {
            return;
        };
        let theme = self.theme();
        let dialog = style::dialog(&theme);
        let looks = Looks {
            scrim: style::SCRIM,
            fill: dialog.fill,
            text: dialog.text,
            quiet: style::QUIET.to_core(),
            rule: theme.extended_palette().background.strong.color.to_core(),
            radius: dialog.radius,
            select: style::select(&theme),
        };
        view.panel_with(backdrop_panel(), Layer::Overlays, &looks.scrim, backdrop);
        view.panel_with(preferences_id(), Layer::Overlays, &Panel { model, looks }, build);
    }

    /// Whether Preferences owns the input. Under help, a question, or the
    /// licence chooser it is drawn but inert: its parts take no click, no
    /// choice, and no focus, from the pointer or a screen reader
    /// (ui-layer.md, section 7, rule 24).
    fn preferences_owns(&self) -> bool {
        self.input_owner() == crate::input_owner::InputOwner::Preferences
    }

    /// The message for a click on a part of Preferences: a button, a
    /// radio, or a check box. From the pointer, a key, or a screen reader.
    pub(crate) fn preferences_click(&self, id: &UiId) -> Option<crate::Message> {
        if !id.is_under(&preferences_id()) || !self.preferences_owns() {
            return None;
        }
        let model = self.preferences_model()?;
        let message = if *id == close_id() {
            Message::Close
        } else if *id == part_id("refresh") {
            Message::Refresh
        } else if *id == learn_id() {
            Message::PanicLearn
        } else if *id == part_id("panic").with("clear") {
            if !model.panic.can_clear {
                return None;
            }
            Message::PanicClear
        } else if *id == part_id("licence").with("choose") {
            Message::ChooseLicence
        } else if *id == part_id("activity") {
            Message::Activity(!model.activity)
        } else if let Some(mode) = model.safety.iter().find(|mode| *id == part_id("safety").with(mode.key)) {
            Message::Safety(mode.mode)
        } else {
            model.midi.iter().find_map(|part| match part {
                Part::Midi { name, on, id: input } if input == id => Some(Message::Midi(name.clone(), !on)),
                Part::SendClock { name, on } if *id == send_clock_id(name) => {
                    Some(Message::SendClock(name.clone(), !on))
                }
                _ => None,
            })?
        };
        Some(crate::Message::Preferences(message))
    }

    /// The message for a choice in one of Preferences' lists.
    pub(crate) fn preferences_chose(&self, id: &UiId, at: usize) -> Option<crate::Message> {
        if !id.is_under(&preferences_id()) || !self.preferences_owns() {
            return None;
        }
        if *id == part_id("patch_map") {
            let mode = MapMode::ALL.get(at)?;
            return Some(crate::Message::Preferences(Message::Map(Some(*mode))));
        }
        let model = self.preferences_model()?;
        model
            .audio
            .iter()
            .chain(&model.midi)
            .find_map(|part| match part {
                Part::Picks(picks) if *id == part_id(picks.key) => picks.pick(at),
                _ => None,
            })
            .map(crate::Message::Preferences)
    }

    /// Preferences' focus stops, in reading order (preferences.md, Keys and
    /// focus): Close; the output safety group, as one stop on its chosen
    /// mode; then each control below it. A turned-off one is no stop.
    pub(crate) fn preferences_stops(&self) -> Vec<UiId> {
        let Some(model) = self.preferences_model().filter(|_| self.preferences_owns()) else {
            return Vec::new();
        };
        let mut stops = vec![close_id()];
        if let Some(mode) = model.safety.iter().find(|mode| mode.chosen).or(model.safety.first()) {
            stops.push(part_id("safety").with(mode.key));
        }
        stops.push(part_id("activity"));
        stops.push(part_id("patch_map"));
        for part in model.audio.iter().chain(&model.midi) {
            match part {
                Part::Picks(picks) if !picks.options.is_empty() => {
                    stops.push(part_id(picks.key));
                    if picks.key == "device" {
                        stops.push(part_id("refresh"));
                    }
                }
                Part::Midi { id, .. } => stops.push(id.clone()),
                Part::SendClock { name, .. } => stops.push(send_clock_id(name)),
                Part::Picks(_) | Part::Note(_) => {}
            }
        }
        stops.push(learn_id());
        if model.panic.can_clear {
            stops.push(part_id("panic").with("clear"));
        }
        stops.push(author_part());
        stops.push(part_id("licence").with("choose"));
        stops
    }

    /// The stop Tab goes to inside Preferences from `from`, or the one
    /// before when `back`, wrapping at both ends, so the focus stays in the
    /// dialog (the WAI-ARIA modal dialog pattern). Any radio of the output
    /// safety group stands for the group's one stop.
    pub(crate) fn preferences_next_stop(&self, from: &UiId, back: bool) -> Option<UiId> {
        let stops = self.preferences_stops();
        let last = stops.len().checked_sub(1)?;
        let safety = part_id("safety");
        let at = stops
            .iter()
            .position(|stop| stop == from || (from.is_under(&safety) && stop.is_under(&safety)))?;
        let to = match back {
            false if at == last => 0,
            false => at + 1,
            true => at.checked_sub(1).unwrap_or(last),
        };
        stops.get(to).cloned()
    }
}

/// The UI ID of a MIDI output's Send clock check box.
pub(crate) fn send_clock_id(name: &str) -> UiId {
    part_id("midi-output").with(name)
}

/// The backdrop over the whole window. It takes every press under it and
/// does nothing with it: a stray click beside the window must not close
/// it (preferences.md, Keys and focus).
fn backdrop(scrim: &Color) -> Element {
    Element::column([])
        .width(Length::FILL)
        .height(Length::FILL)
        .background(*scrim)
        .key("scrim")
        .opaque()
}

/// The box, centred over the backdrop.
fn build(panel: &Panel) -> Element {
    let looks = panel.looks;
    let model = &panel.model;
    let title = Element::row([
        Element::text("Preferences", style::TITLE_TEXT).colour(looks.text),
        Element::space(Length::FILL, Length::Shrink),
        button("close", "Close", true),
    ])
    .align_y(Align::Center)
    .width(Length::FILL);

    let heading = |label: &'static str| Element::text(label, style::HEADING_TEXT).colour(looks.text);
    let rule = || Element::space(Length::FILL, Length::Fixed(style::RULE)).background(looks.rule);
    let mut body = Vec::new();

    body.push(heading("Output safety"));
    let radios = model.safety.iter().map(|mode| {
        Element::radio(mode.label, style::CONTROL_TEXT, mode.chosen)
            .note(mode.note, style::BODY_TEXT)
            .key(mode.key)
    });
    body.push(Element::radio_group("Output safety", radios).spacing(6.0).key("safety"));
    body.extend(model.safety_notes.iter().map(|line| note(line, looks)));
    body.push(rule());

    body.push(
        Element::row([
            Element::checkbox("Show signal activity on ports", style::CONTROL_TEXT, model.activity).key("activity"),
            Element::space(Length::FILL, Length::Shrink),
            Element::text("Patch map", style::BODY_TEXT).colour(looks.text),
            Element::select(Select::new(
                "Patch map",
                MapMode::ALL.iter().map(ToString::to_string).collect(),
                MapMode::ALL.iter().position(|mode| *mode == model.patch_map),
                looks.select,
            ))
            .key("patch_map"),
        ])
        .spacing(8.0)
        .align_y(Align::Center)
        .width(Length::FILL),
    );
    body.push(rule());

    body.push(heading("Audio output"));
    body.extend(parts(&model.audio, looks));
    body.push(rule());

    body.push(heading("MIDI inputs"));
    body.extend(parts(&model.midi, looks));
    body.push(rule());

    body.push(heading("Panic"));
    body.push(
        Element::row([
            Element::text(model.panic.line.clone(), style::CONTROL_TEXT)
                .colour(looks.text)
                .width(Length::FILL),
            button("learn", if model.panic.learning { "Cancel" } else { "Learn" }, true),
            button("clear", "Clear", model.panic.can_clear),
        ])
        .spacing(8.0)
        .align_y(Align::Center)
        .width(Length::FILL)
        .key("panic")
        .access(Access::label(model.panic.line.clone())),
    );
    body.push(quiet("A control or pad that runs panic on every patch.", looks));
    body.push(rule());

    body.push(heading("New patches"));
    body.push(labelled(
        "Your name",
        Element::field(model.author.clone(), style::CONTROL_TEXT)
            .placeholder("Added as author of new patches", looks.quiet)
            .padding(loom_ui::Padding::symmetric(4.0, 6.0))
            .border(looks.select.border_width, looks.select.border)
            .radius(looks.radius)
            .background(looks.select.background)
            .width(Length::FILL)
            .key("author")
            .access(Access::new(Role::TextInput, "Your name")),
        looks,
    ));
    body.push(quiet(
        "Only the name is added to patches you share. Add more by hand if you want to.",
        looks,
    ));
    let licence = format!("Licence: {}", model.licence);
    body.push(
        Element::row([
            Element::text("Licence", style::CONTROL_TEXT)
                .colour(looks.text)
                .width(Length::Fixed(LABEL_WIDTH)),
            Element::text(model.licence.clone(), style::CONTROL_TEXT)
                .colour(looks.text)
                .width(Length::FILL),
            button("choose", "Choose", true),
        ])
        .spacing(8.0)
        .align_y(Align::Center)
        .width(Length::FILL)
        .key("licence")
        .access(Access::label(licence)),
    );
    body.push(quiet(
        "New patches start with this licence. You can change it for each patch.",
        looks,
    ));

    let content = Element::column(body)
        .spacing(12.0)
        .padding(loom_ui::Padding::symmetric(0.0, 12.0))
        .width(Length::FILL);
    // The scroll keys reach it from Close too, which is above it, as in
    // help (section 5, rule 12).
    let scroll = Element::scroll(content, style::scroll())
        .key("body")
        .scrolls_from(close_id())
        .height(Length::FILL)
        .min_height(0.0)
        .width(Length::FILL)
        .access(Access::new(Role::ScrollView, "Preferences"));
    let dialog = Element::column([title, scroll])
        .spacing(12.0)
        .padding(16.0)
        .width(Length::Fixed(WIDTH))
        .max_height(MAX_HEIGHT)
        .min_height(0.0)
        .background(looks.fill)
        .radius(looks.radius)
        .opaque();
    Element::column([dialog])
        .width(Length::FILL)
        .height(Length::FILL)
        .padding(30.0)
        .align_x(Align::Center)
        .align_y(Align::Center)
}

/// A section's parts in screen order: its lists, its check boxes, and its
/// notes. Each run of MIDI inputs is one column keyed `midi`, and each run
/// of MIDI outputs one keyed `midi-output`, so each box's UI ID is its
/// port's under them.
fn parts(parts: &[Part], looks: Looks) -> Vec<Element> {
    let mut out = Vec::new();
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    let flush = |out: &mut Vec<Element>, inputs: &mut Vec<Element>, outputs: &mut Vec<Element>| {
        if !inputs.is_empty() {
            out.push(Element::column(std::mem::take(inputs)).spacing(6.0).key("midi"));
        }
        if !outputs.is_empty() {
            out.push(Element::column(std::mem::take(outputs)).spacing(6.0).key("midi-output"));
        }
    };
    for part in parts {
        match part {
            Part::Midi { name, on, id } => {
                let key = id.keys().last().cloned().unwrap_or_else(|| IdKey::from(name.as_str()));
                inputs.push(Element::checkbox(name.clone(), style::CONTROL_TEXT, *on).key(key));
            }
            Part::SendClock { name, on } => {
                flush(&mut out, &mut inputs, &mut Vec::new());
                outputs.push(
                    Element::checkbox(format!("Send clock to {name}"), style::CONTROL_TEXT, *on).key(name.as_str()),
                );
            }
            Part::Picks(picks) => {
                flush(&mut out, &mut inputs, &mut outputs);
                out.push(list(picks, looks));
            }
            Part::Note(line) => {
                flush(&mut out, &mut inputs, &mut outputs);
                out.push(note(line, looks));
            }
        }
    }
    flush(&mut out, &mut inputs, &mut outputs);
    out
}

/// A list from its model, with its label on the left. A list that failed
/// starts its steps at the option that failed, so a device that cannot be
/// opened never blocks the ones after it (ui-layer.md, section 5, rule 18,
/// the step's start). The device list has Refresh beside it.
fn list(picks: &Picks, looks: Looks) -> Element {
    let select = Select::new(picks.label, picks.options.clone(), picks.chosen, looks.select).steps_from(picks.failed);
    let select = Element::select(select).key(picks.key).width(Length::FILL);
    let control = if picks.key == "device" {
        Element::row([select, button("refresh", "Refresh", true)])
            .spacing(8.0)
            .align_y(Align::Center)
            .width(Length::FILL)
    } else {
        select
    };
    labelled(picks.label, control, looks)
}

/// A control with its label on the left.
fn labelled(label: &'static str, control: Element, looks: Looks) -> Element {
    Element::row([
        Element::text(label, style::CONTROL_TEXT)
            .colour(looks.text)
            .width(Length::Fixed(LABEL_WIDTH)),
        control,
    ])
    .spacing(8.0)
    .align_y(Align::Center)
    .width(Length::FILL)
}

/// A line of the model's, keyed by what it is about, read by a screen
/// reader as text.
fn note(line: &super::model::Note, looks: Looks) -> Element {
    Element::text(line.text.clone(), style::BODY_TEXT)
        .colour(looks.quiet)
        .wrap()
        .width(Length::FILL)
        .key(line.key)
        .access(Access::label(line.text.clone()))
}

/// A fixed line of help under a section, in the quiet colour. It has no
/// node of its own: a screen reader reads it from the window's text.
fn quiet(text: &'static str, looks: Looks) -> Element {
    Element::text(text, style::BODY_TEXT)
        .colour(looks.quiet)
        .wrap()
        .width(Length::FILL)
}

/// A button in the secondary role, keyed `key`; turned off, it is no
/// stop and its node is disabled (ui-layer.md, section 6, rule 2).
fn button(key: &'static str, label: &'static str, on: bool) -> Element {
    let role = ButtonRole::Secondary;
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
