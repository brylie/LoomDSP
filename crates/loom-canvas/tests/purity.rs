//! The canvas's input handling names no Iced type and never reads the
//! clock (docs/design/ui-layer.md, section 9). `loom-canvas` depends on
//! Iced, so the compiler cannot hold this rule; this test does. It keeps
//! the input code able to move to our own UI layer unchanged, and keeps it
//! testable with a time the test chooses.

use std::fs;
use std::path::{Path, PathBuf};

/// What input code must not contain: a path into Iced or one of its crates,
/// and the ways to read the clock. The time arrives in a `Moment` instead.
const FORBIDDEN: [&str; 5] = ["iced::", "iced_", "Instant::now", "SystemTime", ".elapsed("];

/// True for a file that holds tests: `tests.rs`, a name ending in
/// `_tests.rs`, or any file under a `tests` folder. Test code may build
/// what it likes.
fn is_test_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
    name == "tests.rs"
        || name.ends_with("_tests.rs")
        || path
            .parent()
            .is_some_and(|parent| parent.iter().any(|part| part == "tests"))
}

/// The code on a line, without a trailing `//` comment, so prose may name
/// Iced or the clock.
fn code_of(line: &str) -> &str {
    line.split("//").next().unwrap_or(line)
}

/// Each forbidden name in `source`, with its 1-based line number.
fn violations(source: &str) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    for (index, line) in source.lines().enumerate() {
        for name in FORBIDDEN {
            if code_of(line).contains(name) {
                found.push((index + 1, name));
            }
        }
    }
    found
}

/// Adds every Rust file under `dir` that is not a test file to `files`.
fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("the input folder can be read") {
        let path = entry.expect("a folder entry can be read").path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") && !is_test_file(&path) {
            files.push(path);
        }
    }
}

/// The files that make up canvas input: `src/input.rs`, everything under
/// `src/input/`, and `src/reaction.rs`, without test files.
fn input_files() -> Vec<PathBuf> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![src.join("input.rs"), src.join("reaction.rs")];
    collect(&src.join("input"), &mut files);
    files.sort();
    files
}

/// No canvas input file names Iced or reads the clock.
#[test]
fn canvas_input_names_no_iced_type_and_never_reads_the_clock() {
    let files = input_files();
    assert!(files.len() > 2, "the scan found the files under src/input");
    let mut found = Vec::new();
    for path in files {
        let source = fs::read_to_string(&path).expect("an input file can be read");
        for (line, name) in violations(&source) {
            found.push(format!("{}:{line}: {name}", path.display()));
        }
    }
    assert_eq!(found, Vec::<String>::new());
}

/// The scan sees each forbidden name in code, and leaves comments and
/// harmless words alone.
#[test]
fn the_scan_finds_each_forbidden_name_in_code_only() {
    let source = "\
use iced::Point;
use iced_core::mouse;
let now = Instant::now();
let wall = std::time::SystemTime::UNIX_EPOCH;
let waited = at.elapsed();
// The Iced widget calls iced::Shell and Instant::now for us.
let noticed = sliced(now); // not iced::anything
let due = moment.now + delay;
";
    assert_eq!(
        violations(source),
        [
            (1, "iced::"),
            (2, "iced_"),
            (3, "Instant::now"),
            (4, "SystemTime"),
            (5, ".elapsed("),
        ]
    );
}

/// The scan skips test files and keeps the rest.
#[test]
fn the_scan_skips_test_files() {
    for (path, is_test) in [
        ("src/input/tests.rs", true),
        ("src/input/focus/tests.rs", true),
        ("src/input/panel_tests.rs", true),
        ("src/input/tests/helpers.rs", true),
        ("src/input/handle.rs", false),
        ("src/input.rs", false),
    ] {
        assert_eq!(is_test_file(Path::new(path)), is_test, "{path}");
    }
}
