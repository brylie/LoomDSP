//! Questions drawn inside the window: unsaved changes, Replace, and errors
//! (`docs/design/files.md`, Asking before losing work).
//!
//! A native message box stopped the app while it was open, and on Linux it
//! needed another program. This one needs nothing, but nothing stops either:
//! `update` goes on running, so what a question blocks is listed on purpose
//! (`Message::allowed_during_question` in `lib.rs`), and each question
//! carries what to do after its answer.

use crate::files;
use crate::style::{self, button};
use iced::widget::{column, container, opaque, row, space, text};
use iced::{Element, Length};
use loom_patch::Patch;
use serde::Serialize;
use std::path::PathBuf;

/// What to do once unsaved changes are dealt with.
#[derive(Debug, Clone)]
pub enum AfterDiscard {
    /// Start an empty patch.
    New,
    /// Ask for a patch file and open it.
    OpenDialog,
    /// Open this patch file.
    OpenPath(PathBuf),
    /// Open this patch file, dropped on the window. `several` says other
    /// files came with it.
    OpenDropped { path: PathBuf, several: bool },
    /// Quit.
    Close,
    /// Open this patch from module help, as an untitled copy.
    Example(Box<Patch>),
}

/// One button of a question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Save,
    DontSave,
    Cancel,
    Replace,
    Ok,
}

impl Answer {
    /// The words on the button.
    pub fn label(self) -> &'static str {
        match self {
            Self::Save => "Save",
            Self::DontSave => "Don't Save",
            Self::Cancel => "Cancel",
            Self::Replace => "Replace",
            Self::Ok => "OK",
        }
    }

    /// The name the control channel's `toolbar` command uses for the button.
    pub fn name(self) -> &'static str {
        match self {
            Self::Save => "save",
            Self::DontSave => "dont-save",
            Self::Cancel => "cancel",
            Self::Replace => "replace",
            Self::Ok => "ok",
        }
    }
}

/// What is being asked, and what its answer goes on to do.
#[derive(Debug, Clone)]
pub enum Kind {
    /// The patch has unsaved changes, and `next` would drop them.
    Discard { name: String, next: AfterDiscard },
    /// Saving to `path` would replace another file. `next` runs once it is
    /// saved. Without one, the save was all that was asked for.
    Replace { path: PathBuf, next: Option<AfterDiscard> },
    /// Save to library would replace the file at `path` with the sub-patch
    /// `name`, written as `file` (sub-patches.md, Library).
    ReplaceLibrary {
        path: PathBuf,
        file: Box<Patch>,
        name: String,
    },
    /// Something failed. One button, so it cannot be a question.
    Error { message: String },
}

/// The open question.
#[derive(Debug, Clone)]
pub struct Question {
    kind: Kind,
    /// Which answer Enter would give, by index into [`Self::answers`].
    highlight: usize,
    /// The answer it starts on, which also has the primary style. *Why:* the
    /// strongest colour must not point at overwriting a file.
    primary: Answer,
}

/// A question message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// A button was pressed.
    Answer(Answer),
    /// Tab or an arrow key moved the highlight, by this many buttons.
    Move(isize),
    /// Enter: give the highlighted answer.
    Activate,
    /// Escape: stop, changing nothing.
    Escape,
}

/// What the question shows, for tests and the control channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Model {
    pub title: String,
    pub text: String,
    /// The buttons, by the names the control channel uses for them.
    pub buttons: Vec<&'static str>,
    /// The button Enter would press.
    pub highlight: &'static str,
    /// The button with the primary style: the safe answer.
    pub primary: &'static str,
}

impl Question {
    /// Asks about the unsaved changes in the patch called `name`.
    pub fn discard(name: String, next: AfterDiscard) -> Self {
        Self::starting_on(Kind::Discard { name, next }, Answer::Save)
    }

    /// Asks whether saving may replace the file at `path`.
    pub fn replace(path: PathBuf, next: Option<AfterDiscard>) -> Self {
        Self::starting_on(Kind::Replace { path, next }, Answer::Cancel)
    }

    /// Asks whether Save to library may replace the file at `path`.
    pub fn replace_library(path: PathBuf, file: Patch, name: String) -> Self {
        Self::starting_on(
            Kind::ReplaceLibrary {
                path,
                file: Box::new(file),
                name,
            },
            Answer::Cancel,
        )
    }

    /// Says that something failed.
    pub fn error(message: String) -> Self {
        Self::starting_on(Kind::Error { message }, Answer::Ok)
    }

    /// A question whose highlight starts on the safe answer. *Why:* a stray
    /// Enter must never lose work or overwrite a file.
    fn starting_on(kind: Kind, safe: Answer) -> Self {
        let mut question = Self {
            kind,
            highlight: 0,
            primary: safe,
        };
        question.highlight = question.answers().iter().position(|&a| a == safe).unwrap_or(0);
        question
    }

    /// The buttons, in the order they are drawn.
    pub fn answers(&self) -> &'static [Answer] {
        match self.kind {
            Kind::Discard { .. } => &[Answer::Save, Answer::DontSave, Answer::Cancel],
            Kind::Replace { .. } | Kind::ReplaceLibrary { .. } => &[Answer::Replace, Answer::Cancel],
            Kind::Error { .. } => &[Answer::Ok],
        }
    }

    /// The answer Enter would give.
    pub fn highlighted(&self) -> Answer {
        self.answers()[self.highlight]
    }

    /// The answer Escape gives: the one that changes nothing.
    pub fn safe_answer(&self) -> Answer {
        match self.kind {
            Kind::Error { .. } => Answer::Ok,
            _ => Answer::Cancel,
        }
    }

    /// Moves the highlight, wrapping around.
    pub fn step(&mut self, by: isize) {
        let count = self.answers().len() as isize;
        self.highlight = (self.highlight as isize + by).rem_euclid(count) as usize;
    }

    /// What is being asked, which is then the caller's to act on.
    pub fn into_kind(self) -> Kind {
        self.kind
    }

    /// The title and the sentence under it.
    fn words(&self) -> (String, String) {
        match &self.kind {
            Kind::Discard { name, .. } => (
                format!("Save changes to \"{name}\"?"),
                "Your changes will be lost if you do not save them.".to_string(),
            ),
            Kind::Replace { path, .. } | Kind::ReplaceLibrary { path, .. } => (
                format!("Replace \"{}\"?", files::display_name(Some(path))),
                "A file with this name already exists. Saving replaces it.".to_string(),
            ),
            Kind::Error { message } => ("LoomDSP".to_string(), message.clone()),
        }
    }

    /// What the question shows.
    pub fn model(&self) -> Model {
        let (title, text) = self.words();
        Model {
            title,
            text,
            buttons: self.answers().iter().map(|a| a.name()).collect(),
            highlight: self.highlighted().name(),
            primary: self.primary.name(),
        }
    }

    /// The question, as a panel over everything else. The highlight has an
    /// outline as well as a colour, so it does not rest on colour alone.
    pub fn view(&self) -> Element<'_, Message> {
        let (title, words) = self.words();
        let mut buttons = row![space::horizontal()].spacing(8);
        for (i, &answer) in self.answers().iter().enumerate() {
            let press = button(text(answer.label()).size(style::CONTROL)).on_press(Message::Answer(answer));
            let press = if answer == self.primary {
                press.style(style::primary)
            } else {
                press
            };
            buttons = buttons.push(container(press).padding(2).style(style::card(i == self.highlight)));
        }
        let panel = column![
            text(title).size(style::TITLE),
            text(words).size(style::BODY),
            buttons.align_y(iced::Alignment::Center),
        ]
        .spacing(12);
        opaque(
            container(container(panel).padding(16).width(460).style(container::rounded_box))
                .width(Length::Fill)
                .height(Length::Fill)
                .center(Length::Fill)
                .style(|_| container::background(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.4))),
        )
    }
}

/// The keys an open question owns, whether or not a widget used them: a
/// focused text field keeps its focus behind the question, and would take
/// Enter for itself. A held Enter does not repeat, so one press cannot both
/// raise a question and answer it.
pub fn key(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<crate::Message> {
    use iced::keyboard::{Event, Key, key::Named};
    let iced::Event::Keyboard(Event::KeyPressed {
        key, modifiers, repeat, ..
    }) = event
    else {
        return None;
    };
    let message = match key {
        Key::Named(Named::Tab) if modifiers.shift() => Message::Move(-1),
        Key::Named(Named::Tab | Named::ArrowRight | Named::ArrowDown) => Message::Move(1),
        Key::Named(Named::ArrowLeft | Named::ArrowUp) => Message::Move(-1),
        Key::Named(Named::Enter) if !repeat => Message::Activate,
        Key::Named(Named::Escape) if !repeat => Message::Escape,
        _ => return None,
    };
    Some(crate::Message::Question(message))
}

#[cfg(test)]
mod tests;
