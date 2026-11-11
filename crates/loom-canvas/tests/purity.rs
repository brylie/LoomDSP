//! Where `loom-canvas` may name Iced, and where it may read the clock
//! (docs/design/ui-layer.md, section 9; docs/dev/leaving-iced-plan.md,
//! Phase 1 done). The crate depends on Iced, so the compiler cannot hold
//! these rules; this test does.
//!
//! 1. **Iced is named only on the widget side:** `src/widget.rs`, files
//!    under `src/widget/`, `src/iced_convert.rs`, and files under
//!    `src/iced_convert/`. Every other file under `src/`, test files
//!    included, has no path into Iced or one of its crates. The one line
//!    `pub mod iced_convert;` in `src/lib.rs` is allowed, because a module
//!    must be declared somewhere. The canvas can then move to our own UI
//!    layer by replacing those files alone.
//! 2. **Canvas input never reads the clock:** `src/input.rs`, files under
//!    `src/input/`, and `src/reaction.rs`, without their test files. The
//!    time arrives in a `Moment`, so a test chooses it.

use std::fs;
use std::path::{Path, PathBuf};

/// A path into Iced or one of its crates: `iced::`, `iced_wgpu`,
/// `iced_core`, `iced_convert::`, and the like.
const ICED: [&str; 2] = ["iced::", "iced_"];

/// The ways to read the clock.
const CLOCK: [&str; 3] = ["Instant::now", "SystemTime", ".elapsed("];

/// The one line outside the widget side that may name the conversion
/// module: its declaration in `src/lib.rs`.
const DECLARATION: &str = "pub mod iced_convert;";

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

/// True for a file on the widget side, which may name Iced. `path` is
/// relative to `src/`.
fn is_widget_side(path: &Path) -> bool {
    let first = path.iter().next().and_then(|part| part.to_str()).unwrap_or("");
    matches!(first, "widget.rs" | "widget" | "iced_convert.rs" | "iced_convert")
}

/// True for a canvas input file, which may not read the clock. `path` is
/// relative to `src/`.
fn is_input(path: &Path) -> bool {
    let first = path.iter().next().and_then(|part| part.to_str()).unwrap_or("");
    matches!(first, "input.rs" | "input" | "reaction.rs") && !is_test_file(path)
}

/// The code on a line, without a trailing `//` comment, so prose may name
/// Iced or the clock.
fn code_of(line: &str) -> &str {
    line.split("//").next().unwrap_or(line)
}

/// Each of `names` found in the code of `source`, with its 1-based line
/// number. A line that is exactly `allowed` is skipped.
fn violations(source: &str, names: &[&'static str], allowed: Option<&str>) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    for (index, line) in source.lines().enumerate() {
        if allowed.is_some_and(|allowed| line.trim() == allowed) {
            continue;
        }
        for name in names {
            if code_of(line).contains(name) {
                found.push((index + 1, *name));
            }
        }
    }
    found
}

/// Adds every Rust file under `dir` to `files`, as a path relative to
/// `root`.
fn collect(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("a source folder can be read") {
        let path = entry.expect("a folder entry can be read").path();
        if path.is_dir() {
            collect(root, &path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path.strip_prefix(root).expect("the file is under src").to_path_buf());
        }
    }
}

/// Every Rust file under `src/`, relative to it, in a fixed order.
fn source_files() -> (PathBuf, Vec<PathBuf>) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect(&src, &src, &mut files);
    files.sort();
    (src, files)
}

/// The violations of `names` in the files that `applies` picks, as
/// `path:line: name`.
fn scan(names: &[&'static str], applies: impl Fn(&Path) -> bool) -> (usize, Vec<String>) {
    let (src, files) = source_files();
    let mut scanned = 0;
    let mut found = Vec::new();
    for path in files.iter().filter(|path| applies(path)) {
        scanned += 1;
        let source = fs::read_to_string(src.join(path)).expect("a source file can be read");
        let allowed = (path == Path::new("lib.rs")).then_some(DECLARATION);
        for (line, name) in violations(&source, names, allowed) {
            found.push(format!("{}:{line}: {name}", path.display()));
        }
    }
    (scanned, found)
}

/// Outside the widget side, no file names Iced, test files included.
#[test]
fn iced_is_named_only_on_the_widget_side() {
    let (scanned, found) = scan(&ICED, |path| !is_widget_side(path));
    assert!(scanned > 20, "the scan found the crate's files, not {scanned}");
    assert_eq!(found, Vec::<String>::new());
}

/// No canvas input file reads the clock.
#[test]
fn canvas_input_never_reads_the_clock() {
    let (scanned, found) = scan(&CLOCK, is_input);
    assert!(scanned > 2, "the scan found the files under src/input, not {scanned}");
    assert_eq!(found, Vec::<String>::new());
}

/// The scan sees each forbidden name in code, and leaves comments, harmless
/// words, and the allowed line alone.
#[test]
fn the_scan_finds_each_forbidden_name_in_code_only() {
    let source = "\
use iced::Point;
use iced_core::mouse;
use crate::iced_convert::ToIced as _;
pub mod iced_convert;
// The Iced widget calls iced::Shell and Instant::now for us.
let noticed = sliced(now); // not iced::anything
let due = moment.now + delay;
";
    assert_eq!(
        violations(source, &ICED, Some(DECLARATION)),
        [(1, "iced::"), (2, "iced_"), (3, "iced_")]
    );
    assert_eq!(
        violations(source, &ICED, None),
        [(1, "iced::"), (2, "iced_"), (3, "iced_"), (4, "iced_")],
        "the declaration counts where it is not allowed"
    );

    let clock = "\
let now = Instant::now();
let wall = std::time::SystemTime::UNIX_EPOCH;
let waited = at.elapsed();
// Instant::now is read by the host.
";
    assert_eq!(
        violations(clock, &CLOCK, None),
        [(1, "Instant::now"), (2, "SystemTime"), (3, ".elapsed(")]
    );
}

/// The scan knows which files hold tests, which are on the widget side,
/// and which are canvas input.
#[test]
fn the_scan_sorts_files_into_the_right_rule() {
    for (path, test, widget, input) in [
        ("lib.rs", false, false, false),
        ("gpu.rs", false, false, false),
        ("gpu/tests.rs", true, false, false),
        ("input.rs", false, false, true),
        ("input/handle.rs", false, false, true),
        ("input/tests.rs", true, false, false),
        ("input/focus/tests.rs", true, false, false),
        ("input/panel_tests.rs", true, false, false),
        ("input/tests/helpers.rs", true, false, false),
        ("reaction.rs", false, false, true),
        ("widget.rs", false, true, false),
        ("widget/painter.rs", false, true, false),
        ("widget/tests.rs", true, true, false),
        ("iced_convert.rs", false, true, false),
        ("iced_convert/tests.rs", true, true, false),
        ("widgets.rs", false, false, false),
    ] {
        let path = Path::new(path);
        assert_eq!(is_test_file(path), test, "{path:?} test");
        assert_eq!(is_widget_side(path), widget, "{path:?} widget side");
        assert_eq!(is_input(path), input, "{path:?} input");
    }
}
