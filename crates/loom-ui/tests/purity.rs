//! What `loom-ui` may depend on and name (docs/design/ui-layer.md,
//! section 1, rule 1, and section 2, rule 9).
//!
//! 1. **Its required dependencies are `loom-ui-core` and `taffy`,** the
//!    layout engine. No LoomDSP crate. The window shell's `winit`, `wgpu`,
//!    and `pollster` are optional, and only the `shell` feature turns them
//!    on, so a build without it has no window toolkit (plan step 2.2).
//!    Only `layout.rs` names taffy, so no taffy type reaches the crate's
//!    API (decision of 2026-11-14).
//! 2. **No source file names a window toolkit** (`winit`, `wgpu`, or Iced),
//!    test files included, except the window shell: `shell.rs` and the
//!    files under `shell/`. Iced is named nowhere.
//! 3. **No source file reads the clock,** test files aside. Every call
//!    takes the moment from its driver, so a test chooses the time. Tests
//!    read it once to have a starting moment, and the shell and examples
//!    may read it, since they are drivers.

use std::fs;
use std::path::{Path, PathBuf};

/// The dependencies users of the crate get: the required ones, then the
/// shell's.
const ALLOWED: [&str; 5] = [
    "dependencies.loom-ui-core.workspace",
    "dependencies.taffy.workspace",
    "dependencies.winit",
    "dependencies.wgpu",
    "dependencies.pollster",
];

/// The shell's dependencies, each optional and turned on only by `shell`.
const SHELL_DEPENDENCIES: [&str; 3] = ["winit", "wgpu", "pollster"];

/// A path into a window toolkit or the GPU.
const TOOLKITS: [&str; 4] = ["winit", "wgpu", "iced::", "iced_"];

/// Iced, which no file names, the shell included.
const ICED: [&str; 2] = ["iced::", "iced_"];

/// The layout engine, which only `layout.rs` names.
const TAFFY: [&str; 1] = ["taffy"];

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

/// True for the window shell's files: `shell.rs` and those under `shell/`.
fn is_shell(path: &Path) -> bool {
    path == Path::new("shell.rs") || path.starts_with("shell")
}

/// The line in `manifest` that declares `name` as a dependency.
fn dependency_line<'a>(manifest: &'a str, name: &str) -> Option<&'a str> {
    manifest
        .lines()
        .map(str::trim)
        .find(|line| line.split('=').next().is_some_and(|key| key.trim() == name))
}

/// The crate depends on `loom-ui-core` and `taffy`, and on the shell's
/// crates only as optional ones that the `shell` feature turns on.
#[test]
fn the_required_dependencies_are_loom_ui_core_and_taffy() {
    let manifest = include_str!("../Cargo.toml");
    assert_eq!(user_dependencies(manifest), ALLOWED);
    for name in SHELL_DEPENDENCIES {
        let line = dependency_line(manifest, name).expect("the shell's dependency is declared");
        assert!(line.contains("optional = true"), "{name} is optional: {line}");
    }
    let shell = dependency_line(manifest, "shell").expect("the shell feature is declared");
    for name in SHELL_DEPENDENCIES {
        assert!(
            shell.contains(&format!("\"dep:{name}\"")),
            "`shell` turns on {name}: {shell}"
        );
    }
}

/// No source file names a window toolkit, except the shell, which names
/// winit and wgpu and never Iced.
#[test]
fn no_source_file_names_a_window_toolkit() {
    let (scanned, found) = scan(&TOOLKITS, |path| !is_shell(path));
    assert!(scanned >= 1, "the scan found the crate's files, not {scanned}");
    assert_eq!(found, Vec::<String>::new());
    let (shell, found) = scan(&ICED, is_shell);
    assert!(shell >= 1, "the scan found the shell's files, not {shell}");
    assert_eq!(found, Vec::<String>::new());
}

/// Only `layout.rs` names taffy, so the crate's API holds no taffy type.
#[test]
fn only_the_layout_names_taffy() {
    let (scanned, found) = scan(&TAFFY, |path| path != Path::new("layout.rs"));
    assert!(scanned >= 1, "the scan found the crate's files, not {scanned}");
    assert_eq!(found, Vec::<String>::new());
}

/// No source file reads the clock, test files and the shell aside.
#[test]
fn no_source_file_reads_the_clock() {
    let (scanned, found) = scan(&CLOCK, |path| !is_test_file(path) && !is_shell(path));
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

    for (path, shell) in [
        ("shell.rs", true),
        ("shell/gpu.rs", true),
        ("shell/events/tests.rs", true),
        ("shellfish.rs", false),
        ("runtime.rs", false),
    ] {
        assert_eq!(is_shell(Path::new(path)), shell, "{path}");
    }
    assert_eq!(
        dependency_line("winit = { workspace = true, optional = true }", "winit"),
        Some("winit = { workspace = true, optional = true }")
    );
}
