//! The question as a panel of `loom-ui`'s Question layer (ui-layer.md,
//! section 6, rule 6, step 2; plan 3.2, row 3b): a scrim over the whole
//! window that takes every press, holding a box with the title, the
//! sentence, and one button per answer. Its highlight is the runtime's
//! focus on an answer. The tree puts its nodes in an alert dialog named by
//! the title (`a11y/tree.rs`), as it puts the toolbar's in a toolbar.

use super::{Message, Question};
use crate::App;
use crate::overlays::access::{answer_id, question_id};
use crate::style::{self, Role as ButtonRole};
use loom_canvas::iced_convert::ToCore as _;
use loom_ui::access::{Access, Action, Role};
use loom_ui::{Align, Element, Layer, Length, View};
use loom_ui_core::{Color, UiId};

/// Everything the question's panel shows, so the runtime builds it again
/// only when this changes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Panel {
    /// The title: what is asked.
    title: String,
    /// The sentence under it.
    text: String,
    /// Each answer: its UI ID's key, its label, and whether it is the
    /// primary one, which is the safe answer.
    answers: Vec<(&'static str, &'static str, bool)>,
    /// The wash over the window behind the box.
    scrim: Color,
    /// The box's fill.
    fill: Color,
    /// The colour of text that sets none of its own.
    text_colour: Color,
}

impl App {
    /// Adds the question's panel to the runtime's view, while one is open.
    pub(crate) fn question_panel(&self, view: &mut View) {
        let Some(question) = &self.overlays.question else {
            return;
        };
        let model = question.model();
        let theme = self.theme();
        let palette = theme.extended_palette();
        let panel = Panel {
            title: model.title,
            text: model.text,
            answers: question
                .answers()
                .iter()
                .map(|answer| (answer.name(), answer.label(), answer.name() == model.primary))
                .collect(),
            scrim: style::SCRIM,
            fill: palette.background.weak.color.to_core(),
            text_colour: palette.background.weak.text.to_core(),
        };
        view.panel_with(question_id(), Layer::Question, &panel, build);
    }

    /// The message for a click on one of the question's answers, from the
    /// pointer, Enter or Space, or a screen reader.
    pub(crate) fn question_click(&self, id: &UiId) -> Option<crate::Message> {
        let question = self.overlays.question.as_ref()?;
        question
            .answers()
            .iter()
            .find(|answer| answer_id(answer.name()) == *id)
            .map(|answer| crate::Message::Question(Message::Answer(*answer)))
    }

    /// The question's answers as focus stops, in order.
    pub(crate) fn question_stops(&self) -> Vec<UiId> {
        self.overlays
            .question
            .as_ref()
            .map(|question| {
                question
                    .answers()
                    .iter()
                    .map(|answer| answer_id(answer.name()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Question {
    /// Takes the highlight to the answer with UI ID `id`, when it is one
    /// of this question's: the runtime's focus moved there.
    pub(crate) fn follow_focus(&mut self, id: &UiId) {
        if let Some(at) = self.answers().iter().position(|answer| answer_id(answer.name()) == *id) {
            self.highlight = at;
        }
    }

    /// The UI ID of the highlighted answer, where the focus goes.
    pub(crate) fn highlighted_id(&self) -> UiId {
        answer_id(self.highlighted().name())
    }
}

/// The scrim, opaque, with the box centred in it.
fn build(panel: &Panel) -> Element {
    let mut row = vec![Element::space(Length::FILL, Length::Shrink)];
    row.extend(
        panel
            .answers
            .iter()
            .map(|(name, label, primary)| answer(name, label, *primary)),
    );
    let body = Element::column([
        Element::text(panel.title.clone(), style::TITLE_TEXT).colour(panel.text_colour),
        Element::text(panel.text.clone(), style::BODY_TEXT)
            .colour(panel.text_colour)
            .wrap()
            .key("text")
            .access(Access::label(panel.text.clone())),
        Element::row(row)
            .spacing(style::BUTTON_SPACING)
            .align_y(Align::Center)
            .width(Length::FILL),
    ])
    .spacing(12.0)
    .padding(16.0)
    .width(Length::Fixed(style::QUESTION_WIDTH))
    .background(panel.fill)
    .radius(style::OVERLAY_RADIUS);
    Element::column([body])
        .width(Length::FILL)
        .height(Length::FILL)
        .align_x(Align::Center)
        .align_y(Align::Center)
        .background(panel.scrim)
        .opaque()
}

/// One answer: a button in the primary role for the safe answer, the
/// secondary role for the others.
fn answer(name: &'static str, label: &'static str, primary: bool) -> Element {
    let role = if primary {
        ButtonRole::Primary
    } else {
        ButtonRole::Secondary
    };
    Element::text(label, style::CONTROL_TEXT)
        .padding(style::BUTTON_PADDING)
        .radius(role.tokens().radius)
        .looks(role.looks())
        .key(name)
        .focusable()
        .access(Access::new(Role::Button, label).action(Action::Click))
}
