use super::Choice;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// A dialog a test caused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// The Save dialog, with the path it was given to start from.
    Save(Option<PathBuf>),
    /// The unsaved changes question, with the patch name it showed.
    Discard(String),
    /// An error message, with the text it showed.
    Error(String),
}

thread_local! {
    static ASKED: RefCell<Vec<Asked>> = const { RefCell::new(Vec::new()) };
    static SAVE_ANSWER: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    static DISCARD_ANSWER: RefCell<Choice> = const { RefCell::new(Choice::Cancel) };
    static EXAMPLES: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Apps built on this thread from now on find `dir` as their bundled
/// examples folder. Without a call they have none.
pub fn use_examples(dir: Option<PathBuf>) {
    EXAMPLES.with(|examples| *examples.borrow_mut() = dir);
}

/// The bundled examples folder a new app on this thread finds.
pub fn examples() -> Option<PathBuf> {
    EXAMPLES.with(|examples| examples.borrow().clone())
}

/// The Save dialog picks `path` from now on. Without a call it is
/// cancelled.
pub fn answer_save(path: Option<PathBuf>) {
    SAVE_ANSWER.with(|answer| *answer.borrow_mut() = path);
}

/// Answers the next unsaved-changes question, then returns to Cancel.
pub fn answer_discard(choice: Choice) {
    DISCARD_ANSWER.with(|answer| *answer.borrow_mut() = choice);
}

/// The dialogs asked on this thread so far, which are then forgotten.
pub fn asked() -> Vec<Asked> {
    ASKED.with(|asked| std::mem::take(&mut *asked.borrow_mut()))
}

pub(super) fn pick_save(current: Option<&Path>) -> Option<Option<PathBuf>> {
    ASKED.with(|asked| asked.borrow_mut().push(Asked::Save(current.map(Path::to_path_buf))));
    Some(SAVE_ANSWER.with(|answer| answer.borrow().clone()))
}

/// Answers the configured choice, defaulting to Cancel.
pub(super) fn confirm_discard(name: &str) -> Option<Choice> {
    ASKED.with(|asked| asked.borrow_mut().push(Asked::Discard(name.to_string())));
    Some(DISCARD_ANSWER.with(|answer| answer.replace(Choice::Cancel)))
}

/// Records the error message in place of the dialog.
pub(super) fn show_error(message: &str) {
    ASKED.with(|asked| asked.borrow_mut().push(Asked::Error(message.to_string())));
}
