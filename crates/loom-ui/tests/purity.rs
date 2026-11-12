//! What `loom-ui` may depend on and name (docs/design/ui-layer.md,
//! section 1, rule 1, and section 2, rule 9).
//!
//! 1. **Its only dependency is `loom-ui-core`.** No LoomDSP crate and no
//!    window toolkit. The winit shell arrives in plan step 2.2; this test
//!    then names the one module that may use it.
//! 2. **No source file names a window toolkit,** `winit` or Iced, test
//!    files included.
//! 3. **No source file reads the clock,** test files aside. Every call
//!    takes the moment from its driver, so a test chooses the time. Tests
//!    read it once to have a starting moment, and examples may read it,
//!    since they are drivers.

use std::fs;
use std::path::{Path, PathBuf};

/// The dependencies users of the crate get.
const ALLOWED: [&str; 1] = ["dependencies.loom-ui-core.workspace"];

/// A path into a window toolkit.
const TOOLKITS: [&str; 3] = ["winit", "iced::", "iced_"];

/// The ways to read the clock.
const CLOCK: [&str; 3] = ["Instant::now", "SystemTime", ".elapsed("];

/// True for a TOML key path, such as `target.'cfg(unix)'.dependencies.foo`,
/// that gives users of the crate a dependency.
fn is_user_dependency(path: &str) -> bool {
    path.split('.')
        .any(|part| part == "dependencies" || part == "build-dependencies")
}

/// The key paths in `manifest` that would give users a dependency.
fn user_dependencies(manifest: &str) -> Vec<String> {
    let mut section = String::new();
    let mut found = Vec::new();
    for line in manifest.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            section = rest.split(']').next().unwrap_or(rest).trim().to_string();
        } else {
            let key = line.split('=').next().unwrap_or(line).trim();
            let path = format!("{section}.{key}");
            if is_user_dependency(&path) {
                found.push(path);
            }
        }
    }
    found
}

/// The code on a line, without a trailing `//` comment, so prose may name
/// a toolkit or the clock.
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

/// Adds every Rust file under `dir` to `files`, relative to `root`.
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

/// The violations of `names` in the files under `src/` that `applies`
/// picks, as `path:line: name`, and how many files were read.
fn scan(names: &[&'static str], applies: impl Fn(&Path) -> bool) -> (usize, Vec<String>) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect(&src, &src, &mut files);
    files.sort();
    files.retain(|path| applies(path));
    let mut found = Vec::new();
    for path in &files {
        let source = fs::read_to_string(src.join(path)).expect("a source file can be read");
        for (line, name) in violations(&source, names) {
            found.push(format!("{}:{line}: {name}", path.display()));
        }
    }
    (files.len(), found)
}

/// The crate depends on `loom-ui-core` and nothing else.
#[test]
fn the_only_dependency_is_loom_ui_core() {
    assert_eq!(user_dependencies(include_str!("../Cargo.toml")), ALLOWED);
}

/// No source file names a window toolkit.
#[test]
fn no_source_file_names_a_window_toolkit() {
    let (scanned, found) = scan(&TOOLKITS, |_| true);
    assert!(scanned >= 1, "the scan found the crate's files, not {scanned}");
    assert_eq!(found, Vec::<String>::new());
}

/// No source file reads the clock, test files aside.
#[test]
fn no_source_file_reads_the_clock() {
    let (scanned, found) = scan(&CLOCK, |path| !is_test_file(path));
    assert!(scanned >= 1, "the scan found the crate's files, not {scanned}");
    assert_eq!(found, Vec::<String>::new());
}

/// The checks see what they look for, in code only, and leave comments and
/// dev dependencies alone.
#[test]
fn the_checks_find_what_they_look_for() {
    let source = "\
use winit::window::Window;
use iced::Task;
let at = Instant::now();
// A winit shell reads Instant::now for us.
let due = now + delay;
";
    assert_eq!(violations(source, &TOOLKITS), [(1, "winit"), (2, "iced::")]);
    assert_eq!(violations(source, &CLOCK), [(3, "Instant::now")]);

    let manifest = "
        [dependencies]
        loom-ui-core.workspace = true
        winit = \"0.30\"
        [dev-dependencies]
        pretty = \"1\"
    ";
    assert_eq!(
        user_dependencies(manifest),
        ["dependencies.loom-ui-core.workspace", "dependencies.winit"]
    );

    for (path, test) in [
        ("runtime.rs", false),
        ("runtime/tests.rs", true),
        ("effect/more_tests.rs", true),
        ("headless/tests/apps.rs", true),
        ("headless.rs", false),
    ] {
        assert_eq!(is_test_file(Path::new(path)), test, "{path}");
    }
}
