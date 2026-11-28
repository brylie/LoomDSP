//! What `loom-canvas` may not name, and where it may not read the clock
//! (docs/design/ui-layer.md, section 9; docs/dev/leaving-iced-plan.md,
//! P4). The workspace still holds Iced until P5, so the compiler cannot
//! hold the first rule; this test does.
//!
//! 1. **Iced is named nowhere:** no file under `src/`, test files
//!    included, has a path into Iced or one of its crates. The Iced widget
//!    and its conversions went in phase 4, P4: the app hosts the canvas as
//!    a `loom-ui` custom element.
//! 2. **Canvas input never reads the clock:** `src/input.rs`, files under
//!    `src/input/`, and `src/reaction.rs`, without their test files. The
//!    time arrives in a `Moment`, so a test chooses it.
//! 3. **The canvas takes `loom-ui` without its window shell:** only the
//!    `gpu` feature, for the paint renderer. So no window toolkit reaches
//!    the canvas through `loom-ui`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A path into Iced or one of its crates: `iced::`, `iced_wgpu`,
/// `iced_core`, `iced_convert::`, and the like.
const ICED: [&str; 2] = ["iced::", "iced_"];

/// The ways to read the clock.
const CLOCK: [&str; 3] = ["Instant::now", "SystemTime", ".elapsed("];

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
/// number.
fn violations(source: &str, names: &[&'static str]) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    for (index, line) in source.lines().enumerate() {
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
        for (line, name) in violations(&source, names) {
            found.push(format!("{}:{line}: {name}", path.display()));
        }
    }
    (scanned, found)
}

/// No file names Iced, test files included.
#[test]
fn iced_is_named_nowhere() {
    let (scanned, found) = scan(&ICED, |_| true);
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

/// The scan sees each forbidden name in code, and leaves comments and
/// harmless words alone.
#[test]
fn the_scan_finds_each_forbidden_name_in_code_only() {
    let source = "\
use iced::Point;
use iced_core::mouse;
pub mod iced_convert;
// The Iced widget called iced::Shell and Instant::now for us.
let noticed = sliced(now); // not iced::anything
let due = moment.now + delay;
";
    assert_eq!(violations(source, &ICED), [(1, "iced::"), (2, "iced_"), (3, "iced_")]);

    let clock = "\
let now = Instant::now();
let wall = std::time::SystemTime::UNIX_EPOCH;
let waited = at.elapsed();
// Instant::now is read by the host.
";
    assert_eq!(
        violations(clock, &CLOCK),
        [(1, "Instant::now"), (2, "SystemTime"), (3, ".elapsed(")]
    );
}

/// The scan knows which files hold tests and which are canvas input.
#[test]
fn the_scan_sorts_files_into_the_right_rule() {
    for (path, test, input) in [
        ("lib.rs", false, false),
        ("gpu.rs", false, false),
        ("gpu/tests.rs", true, false),
        ("input.rs", false, true),
        ("input/handle.rs", false, true),
        ("input/tests.rs", true, false),
        ("input/focus/tests.rs", true, false),
        ("input/panel_tests.rs", true, false),
        ("input/tests/helpers.rs", true, false),
        ("reaction.rs", false, true),
        ("inputs.rs", false, false),
    ] {
        let path = Path::new(path);
        assert_eq!(is_test_file(path), test, "{path:?} test");
        assert_eq!(is_input(path), input, "{path:?} input");
    }
}

/// What `cargo tree` prints for this crate with `args`, one package or
/// feature per line, from the lock file alone. An inverted tree for a
/// package that does not reach the crate at all is empty.
fn tree(args: &[&str]) -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["tree", "--frozen", "--prefix", "none", "--manifest-path"])
        .arg(&manifest)
        .args(["-p", "loom-canvas"])
        .args(args)
        .output()
        .expect("cargo runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() && stderr.contains("did not match any packages") {
        return String::new();
    }
    assert!(output.status.success(), "cargo tree failed: {stderr}");
    String::from_utf8(output.stdout).expect("cargo tree prints UTF-8")
}

/// The canvas turns on `loom-ui`'s `gpu` feature and not its `shell`, and
/// no path from the canvas to `winit` goes through `loom-ui`. Since the
/// Iced widget went, `winit` does not reach the canvas at all.
#[test]
fn loom_ui_comes_without_its_shell() {
    let features = tree(&["-e", "normal,features", "-i", "loom-ui"]);
    assert!(features.contains("loom-ui feature \"gpu\""), "{features}");
    assert!(!features.contains("loom-ui feature \"shell\""), "{features}");
    assert!(!features.contains("loom-ui feature \"default\""), "{features}");
    let winit = tree(&["-e", "normal", "-i", "winit"]);
    assert!(
        !winit.lines().any(|line| line.starts_with("loom-ui ")),
        "winit reaches the canvas through loom-ui:\n{winit}"
    );
}
