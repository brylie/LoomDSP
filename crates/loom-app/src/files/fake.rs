use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// A dialog a test caused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// The Save dialog, with the path it was given to start from.
    Save(Option<PathBuf>),
    /// The Open dialog, with the path it was given to start from.
    Open(Option<PathBuf>),
    /// An error message, with the text it showed.
    Error(String),
    /// The question to add the user's name to the authors, with the name.
    AddAuthor(String),
}

thread_local! {
    static ASKED: RefCell<Vec<Asked>> = const { RefCell::new(Vec::new()) };
    static SAVE_ANSWER: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    static OPEN_ANSWER: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    static EXAMPLES: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    static ADD_AUTHOR_ANSWER: RefCell<Option<bool>> = const { RefCell::new(Some(false)) };
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

/// The Open dialog picks `path` from now on. Without a call it is
/// cancelled.
pub fn answer_open(path: Option<PathBuf>) {
    OPEN_ANSWER.with(|answer| *answer.borrow_mut() = path);
}

/// Answers the questions to add the user's name from now on: `Some(true)`
/// adds it, `Some(false)` declines, `None` acts as if no dialog could be
/// shown. Without a call they are declined.
pub fn answer_add_author(answer: Option<bool>) {
    ADD_AUTHOR_ANSWER.with(|a| *a.borrow_mut() = answer);
}

/// The dialogs asked on this thread so far, which are then forgotten.
pub fn asked() -> Vec<Asked> {
    ASKED.with(|asked| std::mem::take(&mut *asked.borrow_mut()))
}

pub(super) fn pick_save(current: Option<&Path>) -> Option<Option<PathBuf>> {
    ASKED.with(|asked| asked.borrow_mut().push(Asked::Save(current.map(Path::to_path_buf))));
    Some(SAVE_ANSWER.with(|answer| answer.borrow().clone()))
}

pub(super) fn pick_open(current: Option<&Path>) -> Option<Option<PathBuf>> {
    ASKED.with(|asked| asked.borrow_mut().push(Asked::Open(current.map(Path::to_path_buf))));
    Some(OPEN_ANSWER.with(|answer| answer.borrow().clone()))
}

/// Records the error message in place of the dialog.
pub(super) fn show_error(message: &str) {
    ASKED.with(|asked| asked.borrow_mut().push(Asked::Error(message.to_string())));
}

/// Records the question and gives the configured answer.
pub(super) fn confirm_add_author(name: &str) -> Option<Option<bool>> {
    ASKED.with(|asked| asked.borrow_mut().push(Asked::AddAuthor(name.to_string())));
    Some(ADD_AUTHOR_ANSWER.with(|answer| *answer.borrow()))
}
