//! The licence chooser: two plain questions pick a Creative Commons
//! licence, a third answer gives up all rights, and **Other** takes any
//! SPDX expression. Spec: `docs/design/patch-metadata.md`, Licence chooser.
//!
//! It sets the open patch's licence, or the default licence in the
//! preferences. The app explains each answer; it does not give legal
//! advice. It never picks a licence by itself (rule 7): nothing is stored
//! until the user presses **Use this licence**.

use crate::style::{self, button};
use crate::{App, DIM};
use iced::widget::{column, container, opaque, radio, row, rule, scrollable, space, text, text_input};
use iced::{Element, Length};
use loom_patch::metadata::{self, Changes, Choice, Licence};

/// What the chosen licence is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The open patch's licence.
    Patch,
    /// The default licence for new patches, in the preferences.
    Default,
}

/// The first question's answers, plus **Other**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// One of the three answers about changes.
    Changes(Changes),
    /// "I give up all rights: anyone may do anything with it."
    GiveUp,
    /// Any SPDX expression, typed.
    Other,
}

/// A chooser message.
#[derive(Debug, Clone)]
pub enum Message {
    /// An answer to the first question, or **Other**.
    Answer(Answer),
    /// An answer to "Allow commercial use?"
    Commercial(bool),
    /// The **Other** field's text.
    OtherText(String),
    /// **Use this licence**.
    Use,
    /// **No licence** for a patch, **No default** for the preferences.
    Clear,
    /// **Cancel** or Escape: nothing changes.
    Cancel,
}

/// The open chooser.
#[derive(Debug, Clone, PartialEq)]
pub struct Chooser {
    target: Target,
    answer: Option<Answer>,
    commercial: Option<bool>,
    other: String,
}

/// What the chooser asks of the app after a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Keep it open.
    Stay,
    /// Close it and change nothing.
    Close,
    /// Close it and store this licence, or none.
    Set(Option<String>),
}

/// The first question's answers, with their explanations, in table order.
const CHANGES: [(Answer, &str, &str); 4] = [
    (
        Answer::Changes(Changes::Yes),
        "Yes",
        "Others may change it and share their version, as long as they credit you.",
    ),
    (
        Answer::Changes(Changes::ShareAlike),
        "Yes, if they share it under the same licence",
        "Others may change it, but must share their version under this same licence.",
    ),
    (
        Answer::Changes(Changes::No),
        "No, only unchanged copies",
        "Others may share copies, but not a changed version.",
    ),
    (
        Answer::GiveUp,
        "I give up all rights: anyone may do anything with it",
        "Anyone may use, change, and share it for any purpose, even without credit.",
    ),
];

/// The second question's answers, with their explanations.
const COMMERCIAL: [(bool, &str, &str); 2] = [
    (true, "Yes", "Others may use it in work they are paid for or sell."),
    (false, "No", "Others may use it only when no money is involved."),
];

impl Chooser {
    /// Opens the chooser on `current`: its answers when the chooser offers
    /// it, **Other** with its text otherwise, nothing picked when empty.
    pub fn open(target: Target, current: Option<&str>) -> Self {
        let mut chooser = Self {
            target,
            answer: None,
            commercial: None,
            other: String::new(),
        };
        match current.map(|text| (text, Choice::of(text))) {
            Some((_, Some(Choice::PublicDomain))) => chooser.answer = Some(Answer::GiveUp),
            Some((_, Some(Choice::Creative { changes, commercial }))) => {
                chooser.answer = Some(Answer::Changes(changes));
                chooser.commercial = Some(commercial);
            }
            Some((text, None)) => {
                chooser.answer = Some(Answer::Other);
                chooser.other = text.to_string();
            }
            None => {}
        }
        chooser
    }

    /// What the chooser is for.
    pub fn target(&self) -> Target {
        self.target
    }

    /// The licence the answers give: `None` while a question is open, an
    /// error for an **Other** text that is not SPDX.
    pub fn result(&self) -> Option<Result<String, String>> {
        match self.answer? {
            Answer::GiveUp => Some(Ok(Choice::PublicDomain.licence().to_string())),
            Answer::Changes(changes) => {
                let commercial = self.commercial?;
                Some(Ok(Choice::Creative { changes, commercial }.licence().to_string()))
            }
            Answer::Other if self.other.trim().is_empty() => None,
            Answer::Other if metadata::is_valid(&self.other) => Some(Ok(self.other.trim().to_string())),
            Answer::Other => Some(Err(
                "Not an SPDX licence expression. Use the names on spdx.org/licenses, such as MIT or CC-BY-4.0 OR MIT."
                    .to_string(),
            )),
        }
    }

    /// Handles a message.
    pub fn update(&mut self, message: Message) -> Outcome {
        match message {
            Message::Answer(answer) => self.answer = Some(answer),
            Message::Commercial(commercial) => self.commercial = Some(commercial),
            Message::OtherText(text) => {
                self.answer = Some(Answer::Other);
                self.other = text;
            }
            Message::Use => {
                return match self.result() {
                    Some(Ok(licence)) => Outcome::Set(Some(licence)),
                    _ => Outcome::Stay,
                };
            }
            Message::Clear => return Outcome::Set(None),
            Message::Cancel => return Outcome::Close,
        }
        Outcome::Stay
    }

    /// The chooser, as a panel over the window.
    pub fn view(&self) -> Element<'_, crate::Message> {
        let msg = crate::Message::Licence;
        let note = |line: &'static str| container(text(line).size(style::BODY).color(DIM)).padding([0, 24]);
        let mut changes =
            column![text("Allow others to change it and share the result?").size(style::HEADING)].spacing(4);
        for (answer, label, line) in CHANGES {
            changes = changes.push(
                radio(label, answer, self.answer, move |a| msg(Message::Answer(a)))
                    .size(style::SWITCH)
                    .text_size(style::CONTROL),
            );
            changes = changes.push(note(line));
        }
        let asks_commercial = matches!(self.answer, Some(Answer::Changes(_)));
        let mut commercial = column![text("Allow commercial use?").size(style::HEADING)].spacing(4);
        for (value, label, line) in COMMERCIAL {
            let choice = radio(label, value, self.commercial, move |c| msg(Message::Commercial(c)))
                .size(style::SWITCH)
                .text_size(style::CONTROL);
            commercial = commercial.push(choice).push(note(line));
        }
        let other = column![
            radio(
                "Other: any SPDX licence expression",
                Answer::Other,
                self.answer,
                move |a| { msg(Message::Answer(a)) }
            )
            .size(style::SWITCH)
            .text_size(style::CONTROL),
            container(
                text_input("For example MIT, or CC-BY-4.0 OR MIT", &self.other)
                    .on_input(move |t| msg(Message::OtherText(t)))
                    .on_submit(msg(Message::Use))
                    .size(style::CONTROL)
            )
            .padding([0, 24]),
        ]
        .spacing(4);

        let mut result = column![].spacing(2);
        match self.result() {
            Some(Ok(licence)) => {
                if let Licence::Known(info) = Licence::of(Some(&licence)) {
                    result = result.push(text(info.short).size(style::HEADING));
                    result = result.push(text(info.summary).size(style::BODY));
                    if let Some(url) = info.url {
                        result = result.push(text(url).size(style::SMALL).color(DIM));
                    }
                }
            }
            Some(Err(error)) => result = result.push(text(error).size(style::BODY)),
            None => {
                result = result.push(
                    text("Answer the questions to pick a licence.")
                        .size(style::BODY)
                        .color(DIM),
                );
            }
        }
        result = result.push(
            text("LoomDSP explains each licence in plain words. It does not give legal advice.")
                .size(style::SMALL)
                .color(DIM),
        );

        let ready = matches!(self.result(), Some(Ok(_)));
        let clear = match self.target {
            Target::Patch => "No licence",
            Target::Default => "No default",
        };
        let title = match self.target {
            Target::Patch => "Choose a licence",
            Target::Default => "Default licence for new patches",
        };
        let mut questions = column![changes].spacing(12);
        if asks_commercial {
            questions = questions.push(commercial);
        }
        let panel = column![
            text(title).size(style::TITLE),
            scrollable(
                column![questions, rule::horizontal(1), other, rule::horizontal(1), result]
                    .spacing(12)
                    .padding([0, 12])
            ),
            row![
                button(text(clear).size(style::CONTROL)).on_press(msg(Message::Clear)),
                space::horizontal(),
                button(text("Cancel").size(style::CONTROL)).on_press(msg(Message::Cancel)),
                button(text("Use this licence").size(style::CONTROL))
                    .style(style::primary)
                    .on_press_maybe(ready.then_some(msg(Message::Use))),
            ]
            .spacing(8),
        ]
        .spacing(12);
        let panel = container(panel)
            .padding(16)
            .width(560)
            .max_height(720)
            .style(container::rounded_box);
        opaque(
            container(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .center(Length::Fill)
                .style(|_| container::background(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.4))),
        )
    }
}

impl App {
    /// Handles a chooser message, storing the licence it gives.
    pub(crate) fn licence_message(&mut self, message: Message) {
        let Some(chooser) = self.overlays.chooser.as_mut() else {
            return;
        };
        let target = chooser.target();
        match chooser.update(message) {
            Outcome::Stay => {}
            Outcome::Close => self.overlays.chooser = None,
            Outcome::Set(licence) => {
                self.overlays.chooser = None;
                match target {
                    Target::Patch => {
                        if self.session.document.info().license != licence {
                            self.session.document.edit_info(|info| info.license = licence);
                        }
                    }
                    Target::Default => self.set_default_licence(licence),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
