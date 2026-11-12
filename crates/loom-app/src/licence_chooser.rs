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
use serde::Serialize;

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
#[derive(Debug, Clone, PartialEq)]
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

/// The first question's answers, with their keys in UI IDs and their
/// explanations, in table order.
const CHANGES: [(Answer, &str, &str, &str); 4] = [
    (
        Answer::Changes(Changes::Yes),
        "yes",
        "Yes",
        "Others may change it and share their version, as long as they credit you.",
    ),
    (
        Answer::Changes(Changes::ShareAlike),
        "share-alike",
        "Yes, if they share it under the same licence",
        "Others may change it, but must share their version under this same licence.",
    ),
    (
        Answer::Changes(Changes::No),
        "no",
        "No, only unchanged copies",
        "Others may share copies, but not a changed version.",
    ),
    (
        Answer::GiveUp,
        "give-up",
        "I give up all rights: anyone may do anything with it",
        "Anyone may use, change, and share it for any purpose, even without credit.",
    ),
];

/// The second question's answers, with their keys in UI IDs and their
/// explanations.
const COMMERCIAL: [(bool, &str, &str, &str); 2] = [
    (
        true,
        "yes",
        "Yes",
        "Others may use it in work they are paid for or sell.",
    ),
    (false, "no", "No", "Others may use it only when no money is involved."),
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

    /// What the chooser shows, as plain data (ADR-0021). The view draws
    /// it, and the accessibility tree reports it.
    pub(crate) fn model(&self) -> Model {
        let changes = CHANGES
            .iter()
            .map(|&(answer, key, label, note)| Pick {
                key,
                label,
                note,
                chosen: self.answer == Some(answer),
                message: Message::Answer(answer),
            })
            .collect();
        let commercial = matches!(self.answer, Some(Answer::Changes(_))).then(|| {
            COMMERCIAL
                .iter()
                .map(|&(value, key, label, note)| Pick {
                    key,
                    label,
                    note,
                    chosen: self.commercial == Some(value),
                    message: Message::Commercial(value),
                })
                .collect()
        });
        let given = match self.result() {
            Some(Ok(licence)) => match Licence::of(Some(&licence)) {
                Licence::Known(info) => Given::Licence {
                    short: info.short.to_string(),
                    summary: info.summary.to_string(),
                    url: info.url.map(|url| url.to_string()),
                },
                _ => Given::Unlisted,
            },
            Some(Err(error)) => Given::Error(error),
            None => Given::Open,
        };
        Model {
            title: match self.target {
                Target::Patch => "Choose a licence",
                Target::Default => "Default licence for new patches",
            },
            changes,
            commercial,
            other: self.answer == Some(Answer::Other),
            other_text: self.other.clone(),
            ready: matches!(self.result(), Some(Ok(_))),
            given,
            clear: match self.target {
                Target::Patch => "No licence",
                Target::Default => "No default",
            },
        }
    }
}

/// The chooser, as it shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Model {
    /// Its title, which says what the licence is for.
    pub title: &'static str,
    /// The first question's answers, without **Other**.
    pub changes: Vec<Pick>,
    /// The second question's answers, while it is asked.
    pub commercial: Option<Vec<Pick>>,
    /// True while **Other** is chosen.
    pub other: bool,
    /// The **Other** field's text.
    pub other_text: String,
    /// What the answers give so far.
    pub given: Given,
    /// True when **Use this licence** works.
    pub ready: bool,
    /// What the clearing button says.
    pub clear: &'static str,
}

/// One answer to a question.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Pick {
    /// Its key in UI IDs.
    pub key: &'static str,
    /// What it says.
    pub label: &'static str,
    /// The line that explains it.
    pub note: &'static str,
    /// True while it is chosen.
    pub chosen: bool,
    /// The message that chooses it.
    #[serde(skip)]
    pub message: Message,
}

/// What the answers give so far.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) enum Given {
    /// A question is still open.
    Open,
    /// A licence, with what it means.
    Licence {
        /// Its short name.
        short: String,
        /// What it allows, in plain words.
        summary: String,
        /// Where its full text is.
        url: Option<String>,
    },
    /// A valid licence the app has no words for: nothing more is shown.
    Unlisted,
    /// The **Other** text is not a licence expression.
    Error(String),
}

/// The line under every result.
pub(crate) const DISCLAIMER: &str = "LoomDSP explains each licence in plain words. It does not give legal advice.";

/// The line shown while a question is still open.
pub(crate) const UNANSWERED: &str = "Answer the questions to pick a licence.";

/// The id of the **Other** field, so a screen reader's Focus reaches it.
pub(crate) const OTHER_INPUT: &str = "licence-other";

/// The chooser, as a panel over the window, drawn from its model. Each part
/// that acts is named by its UI ID, so the window says where it drew it.
pub(crate) fn view(model: Model) -> Element<'static, crate::Message> {
    use access::licence_id;
    let named =
        |element: Element<'static, crate::Message>, id: loom_ui_core::UiId| -> Element<'static, crate::Message> {
            container(element).id(crate::places::widget_id(&id)).into()
        };
    let msg = crate::Message::Licence;
    let note = |line: &'static str| container(text(line).size(style::BODY).color(DIM)).padding([0, 24]);
    let mut answers = column![].spacing(4);
    let other_answer = model.other.then_some(Answer::Other);
    for pick in &model.changes {
        let Message::Answer(answer) = pick.message else {
            continue;
        };
        let chosen = pick.chosen.then_some(answer);
        answers = answers.push(named(
            radio(pick.label, answer, chosen, move |a| msg(Message::Answer(a)))
                .size(style::SWITCH)
                .text_size(style::CONTROL)
                .into(),
            licence_id().with("changes").with(pick.key),
        ));
        answers = answers.push(note(pick.note));
    }
    let changes = column![
        text("Allow others to change it and share the result?").size(style::HEADING),
        named(answers.into(), licence_id().with("changes")),
    ]
    .spacing(4);
    let mut questions = column![changes].spacing(12);
    if let Some(picks) = &model.commercial {
        let mut answers = column![].spacing(4);
        for pick in picks {
            let Message::Commercial(value) = pick.message else {
                continue;
            };
            let chosen = pick.chosen.then_some(value);
            let choice = radio(pick.label, value, chosen, move |c| msg(Message::Commercial(c)))
                .size(style::SWITCH)
                .text_size(style::CONTROL);
            answers = answers
                .push(named(choice.into(), licence_id().with("commercial").with(pick.key)))
                .push(note(pick.note));
        }
        questions = questions.push(
            column![
                text("Allow commercial use?").size(style::HEADING),
                named(answers.into(), licence_id().with("commercial")),
            ]
            .spacing(4),
        );
    }
    let other = column![
        named(
            radio(
                "Other: any SPDX licence expression",
                Answer::Other,
                other_answer,
                move |a| { msg(Message::Answer(a)) }
            )
            .size(style::SWITCH)
            .text_size(style::CONTROL)
            .into(),
            licence_id().with("other"),
        ),
        container(named(
            text_input("For example MIT, or CC-BY-4.0 OR MIT", &model.other_text)
                .id(iced::widget::Id::new(OTHER_INPUT))
                .on_input(move |t| msg(Message::OtherText(t)))
                .on_submit(msg(Message::Use))
                .size(style::CONTROL)
                .into(),
            licence_id().with("other-text"),
        ))
        .padding([0, 24]),
    ]
    .spacing(4);

    let mut result = column![].spacing(2);
    match model.given {
        Given::Licence { short, summary, url } => {
            result = result.push(text(short).size(style::HEADING));
            result = result.push(text(summary).size(style::BODY));
            if let Some(url) = url {
                result = result.push(text(url).size(style::SMALL).color(DIM));
            }
        }
        Given::Error(error) => result = result.push(text(error).size(style::BODY)),
        Given::Unlisted => {}
        Given::Open => result = result.push(text(UNANSWERED).size(style::BODY).color(DIM)),
    }
    result = result.push(text(DISCLAIMER).size(style::SMALL).color(DIM));

    let panel = column![
        text(model.title).size(style::TITLE),
        scrollable(
            column![questions, rule::horizontal(1), other, rule::horizontal(1), result]
                .spacing(12)
                .padding([0, 12])
        ),
        row![
            named(
                button(text(model.clear).size(style::CONTROL))
                    .on_press(msg(Message::Clear))
                    .into(),
                licence_id().with("clear"),
            ),
            space::horizontal(),
            named(
                button(text("Cancel").size(style::CONTROL))
                    .on_press(msg(Message::Cancel))
                    .into(),
                licence_id().with("cancel"),
            ),
            named(
                button(text("Use this licence").size(style::CONTROL))
                    .style(style::primary)
                    .on_press_maybe(model.ready.then_some(msg(Message::Use)))
                    .into(),
                licence_id().with("use"),
            ),
        ]
        .spacing(8),
    ]
    .spacing(12);
    let panel = container(panel)
        .padding(16)
        .width(560)
        .max_height(720)
        .style(container::rounded_box)
        .id(crate::places::widget_id(&licence_id()));
    opaque(
        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .style(|_| container::background(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.4))),
    )
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

pub(crate) mod access;

#[cfg(test)]
mod tests;
