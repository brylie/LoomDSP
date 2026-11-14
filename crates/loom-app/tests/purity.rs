//! Where `loom-app` may name Iced's task and effect calls, and `rfd`, while
//! the app moves onto our own runtime (docs/dev/leaving-iced-plan.md, step
//! 2.1, item 2; docs/design/ui-layer.md, section 2). The app still depends
//! on Iced for its views, so the compiler cannot hold this line; this test
//! does.
//!
//! **Iced's `Task`, its widget operations, its clipboard, exit, and
//! screenshot, and `rfd` are named only in the bridge:** `src/iced_bridge.rs`
//! and files under `src/iced_bridge/`. A file not yet ported is listed in
//! [`NOT_YET_PORTED`]. The list only shrinks: a listed file that no longer
//! names any of them fails the test, so it is taken off when it is ported.
//! Test files are not checked, since the `Ui` harness and its tests run the
//! same calls until step 2.3.

use std::fs;
use std::path::{Path, PathBuf};

/// What only the bridge may name. `Task` is a whole word, so `iced::Task`,
/// `Task::done`, and a renamed import `use iced::Task as T` all match.
const BRIDGE_ONLY: [&str; 7] = [
    "Task",
    "iced::widget::operation",
    "iced::advanced::widget::operate",
    "iced::clipboard",
    "iced::exit",
    "iced::window::screenshot",
    "rfd",
];

/// Files under `src/` that still name them, until each is ported. The list
/// only shrinks. It is empty since the app runs on the `winit` shell (step
/// 2.2b-ii), and stays as the place for one that must name them for a while.
const NOT_YET_PORTED: [&str; 0] = [];

/// True for a file that holds tests: `tests.rs`, a name ending in
/// `_tests.rs`, or any file under a `tests` folder.
fn is_test_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
    name == "tests.rs"
        || name.ends_with("_tests.rs")
        || path
            .parent()
            .is_some_and(|parent| parent.iter().any(|part| part == "tests"))
}

/// Every `.rs` file under `dir`.
fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("a source folder") {
        let path = entry.expect("a folder entry").path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// True when `word` appears in `text` with no letter, digit, or `_` on
/// either side.
fn names(text: &str, word: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

/// What `text` names of [`BRIDGE_ONLY`].
fn bridge_names(text: &str) -> Vec<&'static str> {
    BRIDGE_ONLY.into_iter().filter(|word| names(text, word)).collect()
}

/// The rule in the module's doc.
#[test]
fn iced_tasks_and_dialogs_are_named_only_in_the_bridge() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    let mut wrong = Vec::new();
    let mut ported = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(&src)
            .expect("under src")
            .to_string_lossy()
            .replace('\\', "/");
        if relative == "iced_bridge.rs" || relative.starts_with("iced_bridge/") || is_test_file(&path) {
            continue;
        }
        let text = fs::read_to_string(&path).expect("a source file");
        let named = bridge_names(&text);
        let listed = NOT_YET_PORTED.contains(&relative.as_str());
        if !listed && !named.is_empty() {
            wrong.push(format!("{relative}: {named:?}"));
        }
        if listed && named.is_empty() {
            ported.push(relative);
        }
    }
    assert!(wrong.is_empty(), "only the bridge may name these: {wrong:#?}");
    assert!(ported.is_empty(), "ported, so take them off NOT_YET_PORTED: {ported:?}");
}

/// The check itself: whole words only, so a renamed import is caught and
/// a longer name is not.
#[test]
fn the_check_matches_whole_words() {
    assert_eq!(bridge_names("use iced::Task as T;"), ["Task"]);
    assert_eq!(bridge_names("Task::done(m)"), ["Task"]);
    assert_eq!(bridge_names("rfd::FileDialog::new()"), ["rfd"]);
    assert!(bridge_names("TaskList, my_task, rfdx").is_empty());
}
