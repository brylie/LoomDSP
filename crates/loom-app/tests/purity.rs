//! Where `loom-app` may name Iced and `rfd` (docs/dev/leaving-iced-plan.md,
//! P4 and P5; docs/design/ui-layer.md, section 1, rule 7). The workspace
//! still holds Iced until P5 removes it, so the compiler cannot hold these
//! lines; this test does.
//!
//! - **Iced is named nowhere in `src/`,** test files included: the app
//!   runs on `loom-ui`'s runtime and shell since phase 4, P4. P5 drops the
//!   dependencies, and `deny.toml` then bans the crates.
//! - **`rfd` only in the window's services:** `src/services.rs` and files
//!   under `src/services/`, which carry out the runtime's dialogs for the
//!   window. Test files are not checked for it.
//!
//! Comments are not checked: they may name Iced, as the plan's file name
//! does.

use std::fs;
use std::path::{Path, PathBuf};

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

/// The code of `text`: each line without a `//` comment.
fn code(text: &str) -> String {
    text.lines()
        .map(|line| line.split("//").next().unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// True when `code` names Iced or one of its crates: the word `iced`, as
/// in `iced::` or `use iced`, or a crate name such as `iced_wgpu`.
fn names_iced(code: &str) -> bool {
    names(code, "iced")
        || code
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|w| w.starts_with("iced_"))
}

/// The files under `src/`, each with its path relative to it and its
/// code.
fn sources() -> Vec<(String, PathBuf, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(&src)
                .expect("under src")
                .to_string_lossy()
                .replace('\\', "/");
            let text = fs::read_to_string(&path).expect("a source file");
            (relative, path, code(&text))
        })
        .collect()
}

/// No file under `src/`, test files included, names Iced in its code.
#[test]
fn iced_is_named_nowhere() {
    let files = sources();
    assert!(files.len() > 50, "the scan found the app's files, not {}", files.len());
    let wrong: Vec<_> = files
        .iter()
        .filter(|(_, _, code)| names_iced(code))
        .map(|(relative, ..)| relative.clone())
        .collect();
    assert!(wrong.is_empty(), "these name Iced: {wrong:#?}");
}

/// `rfd` is named only in the window's services, outside the tests.
#[test]
fn dialogs_are_named_only_in_the_services() {
    let wrong: Vec<_> = sources()
        .into_iter()
        .filter(|(relative, path, code)| {
            let services = relative == "services.rs" || relative.starts_with("services/");
            !services && !is_test_file(path) && names(code, "rfd")
        })
        .map(|(relative, ..)| relative)
        .collect();
    assert!(wrong.is_empty(), "only services.rs may name rfd: {wrong:#?}");
}

/// The checks themselves: whole words and crate names only, in code only,
/// so a longer name and a comment pass.
#[test]
fn the_checks_match_what_they_mean() {
    assert!(names_iced("use iced::Event;"));
    assert!(names_iced("let r = iced_wgpu::Renderer::new();"));
    assert!(names_iced("use iced;"));
    assert!(!names_iced(&code("let x = 1; // the leaving-iced plan")));
    assert!(!names_iced("let sliced = 1;"));
    assert!(names(&code("rfd::FileDialog::new()"), "rfd"));
    assert!(!names("rfdx", "rfd"));
}
