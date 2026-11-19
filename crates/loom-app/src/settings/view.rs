//! The Preferences window, drawn from its plain-data model.

use super::{Message, Model, Part, Picks, access, model};
use crate::style::{self, button};
use crate::{App, Element};
use iced::Length;
use iced::widget::{
    checkbox, column, container, opaque, pick_list, radio, row, rule, scrollable, space, text, text_input,
};
use std::fmt;

impl App {
    /// The window, over the canvas, while it is open.
    pub(crate) fn preferences_view(&self) -> Option<Element<'_, crate::Message>> {
        self.preferences_model().map(preferences_panel)
    }
}

/// The part of the **Your name** field, `preferences/author`. Its text
/// field's widget ID comes from it (`places::field_widget_id`), so a
/// screen reader's Focus reaches it and the window's focus reads back.
pub(crate) fn author_part() -> loom_ui_core::UiId {
    access::preferences_id().with("author")
}

/// The Preferences window, drawn from its model. Each part is named by its
/// UI ID, so the window says where it drew each one (`places.rs`).
fn preferences_panel(model: Model) -> Element<'static, crate::Message> {
    use access::{part_id, preferences_id};
    let msg = crate::Message::Preferences;
    let heading = |label: &'static str| text(label).size(style::HEADING);
    let note = |line: String| text(line).size(style::BODY).color(crate::DIM);

    let note_of = |line: model::Note| named(note(line.text), part_id("note").with(line.key));
    let mut modes = column![].spacing(6);
    for mode in &model.safety {
        let chosen = mode.chosen.then_some(mode.mode);
        modes = modes.push(
            column![
                named(
                    radio(mode.label, mode.mode, chosen, move |m| msg(Message::Safety(m)))
                        .size(style::SWITCH)
                        .text_size(style::CONTROL),
                    part_id("safety").with(mode.key),
                ),
                container(note(mode.note.to_string())).padding([0, 24]),
            ]
            .spacing(2),
        );
    }
    let mut safety = column![heading("Output safety"), named(modes, part_id("safety"))].spacing(6);
    for line in model.safety_notes {
        safety = safety.push(note_of(line));
    }

    let mut audio = column![].spacing(8);
    for part in model.audio {
        audio = match part {
            Part::Picks(picks) if picks.key == "device" => audio.push(labelled(
                picks.label,
                row![
                    pick_list_of(picks),
                    named(
                        button(text("Refresh").size(style::CONTROL)).on_press(msg(Message::Refresh)),
                        part_id("refresh"),
                    ),
                ]
                .spacing(8)
                .into(),
            )),
            Part::Picks(picks) => audio.push(labelled(picks.label, pick_list_of(picks))),
            Part::Note(line) => audio.push(note_of(line)),
            Part::Midi { .. } | Part::SendClock { .. } => audio,
        };
    }

    let mut midi = column![].spacing(6);
    for part in model.midi {
        midi = match part {
            Part::Midi { name, on, id } => midi.push(named(
                checkbox(on)
                    .style(style::check_box)
                    .label(name.clone())
                    .on_toggle(move |on| msg(Message::Midi(name.clone(), on)))
                    .size(style::SWITCH)
                    .text_size(style::CONTROL),
                id,
            )),
            Part::Note(line) => midi.push(note_of(line)),
            Part::Picks(picks) => midi.push(labelled(picks.label, pick_list_of(picks))),
            Part::SendClock { name, on } => {
                let id = part_id("midi-output").with(name.as_str());
                midi.push(named(
                    checkbox(on)
                        .style(style::check_box)
                        .label(format!("Send clock to {name}"))
                        .on_toggle(move |on| msg(Message::SendClock(name.clone(), on)))
                        .size(style::SWITCH)
                        .text_size(style::CONTROL),
                    id,
                ))
            }
        };
    }

    let panic = column![
        row![
            container(named(text(model.panic.line).size(style::CONTROL), part_id("panic"))).width(Length::Fill),
            named(
                button(text(if model.panic.learning { "Cancel" } else { "Learn" }).size(style::CONTROL))
                    .on_press(msg(Message::PanicLearn)),
                part_id("panic").with("learn"),
            ),
            named(
                button(text("Clear").size(style::CONTROL))
                    .on_press_maybe(model.panic.can_clear.then_some(msg(Message::PanicClear))),
                part_id("panic").with("clear"),
            ),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
        note("A control or pad that runs panic on every patch.".into()),
    ]
    .spacing(6);

    let patches = column![
        labelled(
            "Your name",
            named(
                text_input("Added as author of new patches", &model.author)
                    .id(crate::places::field_widget_id(&author_part()))
                    .on_input(move |t| msg(Message::Author(t)))
                    .on_submit(msg(Message::SaveAuthor))
                    .size(style::CONTROL),
                part_id("author"),
            ),
        ),
        note("Only the name is added to patches you share. Add more by hand if you want to.".into()),
        labelled(
            "Licence",
            row![
                container(named(text(model.licence).size(style::CONTROL), part_id("licence"))).width(Length::Fill),
                named(
                    button(text("Choose").size(style::CONTROL)).on_press(msg(Message::ChooseLicence)),
                    part_id("licence").with("choose"),
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into(),
        ),
        note("New patches start with this licence. You can change it for each patch.".into()),
    ]
    .spacing(6);

    let panel = column![
        row![
            text("Preferences").size(style::TITLE),
            space::horizontal(),
            named(
                button(text("Close").size(style::CONTROL)).on_press(msg(Message::Close)),
                part_id("close"),
            ),
        ]
        .align_y(iced::Alignment::Center),
        scrollable(
            column![
                safety,
                rule::horizontal(1),
                row![
                    named(
                        checkbox(model.activity)
                            .style(style::check_box)
                            .label("Show signal activity on ports")
                            .on_toggle(move |on| msg(Message::Activity(on))),
                        part_id("activity"),
                    ),
                    space::horizontal(),
                    text("Patch map").size(style::BODY),
                    named(
                        pick_list(
                            crate::preferences::MapMode::ALL,
                            Some(model.patch_map),
                            move |mode| msg(Message::Map(Some(mode)))
                        ),
                        part_id("patch_map"),
                    ),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
                rule::horizontal(1),
                heading("Audio output"),
                audio,
                rule::horizontal(1),
                heading("MIDI inputs"),
                midi,
                rule::horizontal(1),
                heading("Panic"),
                panic,
                rule::horizontal(1),
                heading("New patches"),
                patches
            ]
            .spacing(12)
            .padding([0, 12])
        )
    ]
    .spacing(12);
    let panel = container(panel)
        .padding(16)
        .width(560)
        .max_height(720)
        .style(container::rounded_box)
        .id(crate::places::widget_id(&preferences_id()));
    opaque(
        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .style(|_| container::background(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.4))),
    )
}

/// `element` in a container named by `id`, so the window says where it
/// drew it.
fn named<'a>(element: impl Into<Element<'a, crate::Message>>, id: loom_ui_core::UiId) -> Element<'a, crate::Message> {
    container(element).id(crate::places::widget_id(&id)).into()
}

/// A list from its model: picking an option sends that option's message.
fn pick_list_of(picks: Picks) -> Element<'static, crate::Message> {
    let id = access::part_id(picks.key);
    let shown: Vec<Shown> = picks
        .options
        .iter()
        .enumerate()
        .map(|(at, text)| Shown { at, text: text.clone() })
        .collect();
    let selected = picks.chosen.and_then(|at| shown.get(at).cloned());
    let messages = picks.messages;
    let list = pick_list(shown, selected, move |item: Shown| {
        crate::Message::Preferences(messages[item.at].clone())
    })
    .text_size(style::CONTROL)
    .width(Length::Fill);
    container(list)
        .width(Length::Fill)
        .id(crate::places::widget_id(&id))
        .into()
}

/// A control with its label on the left.
fn labelled(label: &'static str, control: Element<'static, crate::Message>) -> Element<'static, crate::Message> {
    row![text(label).size(style::CONTROL).width(100), control]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
}

/// One option of a list as the list shows it: its place and its text.
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    at: usize,
    text: String,
}

impl fmt::Display for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
