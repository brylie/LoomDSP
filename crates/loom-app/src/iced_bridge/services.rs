//! The desktop's services for the app on the shell: the system's dialogs
//! and the window's clipboard (plan step 2.2b-ii).
//!
//! A dialog is modal and blocks the loop until it closes, as it did under
//! Iced, outside `update`; its answer comes on the next turn.

use super::window::SharedClipboard;
use crate::Message;
use iced::advanced::clipboard::Kind;
use loom_ui::Waker;
use loom_ui::effect::{Choice, FileAction, FileRequest, Level, Question, Reply};
use loom_ui::shell::Services;
use std::path::PathBuf;

/// The app's services: `rfd`'s dialogs and the clipboard the window's
/// layers share.
pub(crate) struct AppServices {
    /// The window's clipboard, once it has opened.
    clipboard: SharedClipboard,
}

impl AppServices {
    /// The services, with the clipboard the layers share.
    pub(crate) fn new(clipboard: SharedClipboard) -> Self {
        Self { clipboard }
    }
}

impl Services<Message> for AppServices {
    fn clipboard_write(&mut self, text: String) {
        if let Some(clipboard) = self.clipboard.borrow_mut().as_mut() {
            clipboard.write(Kind::Standard, text);
        }
    }

    fn clipboard_read(&mut self, reply: Reply<Option<String>, Message>, waker: &Waker<Message>) {
        let text = self
            .clipboard
            .borrow()
            .as_ref()
            .and_then(|clipboard| clipboard.read(Kind::Standard));
        waker.send(reply.give(text));
    }

    fn file_dialog(&mut self, request: FileRequest, reply: Reply<Option<PathBuf>, Message>, waker: &Waker<Message>) {
        waker.send(reply.give(show_file_dialog(&request)));
    }

    fn ask(&mut self, question: Question, reply: Reply<Choice, Message>, waker: &Waker<Message>) {
        waker.send(reply.give(show_question(&question)));
    }
}

/// Shows a native file dialog for `request` and waits for it.
fn show_file_dialog(request: &FileRequest) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(&request.title);
    for filter in &request.filters {
        dialog = dialog.add_filter(&filter.name, &filter.extensions);
    }
    if let Some(folder) = &request.folder {
        dialog = dialog.set_directory(folder);
    }
    if let Some(name) = &request.file_name {
        dialog = dialog.set_file_name(name);
    }
    match request.action {
        FileAction::Open => dialog.pick_file(),
        FileAction::Save => dialog.save_file(),
    }
}

/// Shows a native question and waits for it. The answer is the index of
/// the button pressed, or `None` when it closed with none.
fn show_question(question: &Question) -> Choice {
    use rfd::{MessageButtons, MessageDialog, MessageLevel};
    debug_assert!(
        (1..=3).contains(&question.buttons.len()),
        "a native question has one to three buttons: {:?}",
        question.buttons
    );
    let buttons = match question.buttons.as_slice() {
        [] => MessageButtons::Ok,
        [ok] => MessageButtons::OkCustom(ok.clone()),
        [ok, cancel] => MessageButtons::OkCancelCustom(ok.clone(), cancel.clone()),
        [yes, no, cancel, ..] => MessageButtons::YesNoCancelCustom(yes.clone(), no.clone(), cancel.clone()),
    };
    let result = MessageDialog::new()
        .set_level(match question.level {
            Level::Info => MessageLevel::Info,
            Level::Warning => MessageLevel::Warning,
            Level::Error => MessageLevel::Error,
        })
        .set_title(&question.title)
        .set_description(&question.text)
        .set_buttons(buttons)
        .show();
    choice(&question.buttons, result)
}

/// The button a dialog result names, by its label, or by its place where
/// the platform reports a standard button instead.
pub(super) fn choice(buttons: &[String], result: rfd::MessageDialogResult) -> Choice {
    use rfd::MessageDialogResult;
    match result {
        MessageDialogResult::Custom(label) => buttons.iter().position(|b| *b == label),
        MessageDialogResult::Ok | MessageDialogResult::Yes => (!buttons.is_empty()).then_some(0),
        MessageDialogResult::No => (buttons.len() > 2).then_some(1),
        MessageDialogResult::Cancel => None,
    }
}
