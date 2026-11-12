//! File dialogs and the words around them (`docs/design/files.md`).
//!
//! The dialogs are native and blocking. They run on the UI thread, which is
//! what the operating systems expect; audio runs on its own thread and keeps
//! playing while one is open.

use loom_ui::effect::{Choice, FileAction, FileFilter, FileRequest, Level, Question};
use std::path::{Path, PathBuf};

/// The file extension of a patch.
pub const EXTENSION: &str = "ron";

/// A marker put in release patch folders so examples are still recognized
/// when macOS runs the app from a translocated copy.
const BUNDLED_MARKER: &str = ".loomdsp-bundled-examples";

/// What came of asking where to save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Saving {
    /// The dialog was cancelled.
    Cancelled,
    /// Save here. The dialog confirmed the name if it had to.
    Chosen(PathBuf),
    /// Save here only if the player agrees: adding the extension made the
    /// name of another file, which the dialog did not ask about.
    Confirm(PathBuf),
}

/// The name shown for a patch: its file name, or "Untitled" before it has one.
pub fn display_name(file: Option<&Path>) -> String {
    file.and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_string())
}

/// A file name made from a patch name: characters that file systems refuse
/// are dropped, and the ends are trimmed of spaces and dots. "Untitled"
/// when nothing is left. No extension.
pub fn file_stem(name: &str) -> String {
    let stem: String = name
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    let stem = stem.trim_matches(|c: char| c.is_whitespace() || c == '.');
    if stem.is_empty() {
        "Untitled".to_string()
    } else {
        stem.to_string()
    }
}

/// A patch file in `dir` named after `name` that does not exist yet:
/// `Name.ron`, else `Name 2.ron`, `Name 3.ron`, and so on. Save As
/// suggests it, so a second new patch does not meet the replace question.
pub fn free_path(dir: &Path, name: &str) -> PathBuf {
    let stem = file_stem(name);
    let mut path = dir.join(format!("{stem}.{EXTENSION}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} {n}.{EXTENSION}"));
        n += 1;
    }
    path
}

/// The window title for a patch shown under `name`: `* name - LoomDSP`
/// with unsaved changes.
pub fn titled(name: &str, unsaved: bool) -> String {
    format!("{}{name} - LoomDSP", if unsaved { "* " } else { "" })
}

/// `path` with the patch extension added when it has none.
pub fn with_extension(mut path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.set_extension(EXTENSION);
    }
    path
}

/// The bundled examples folder (`docs/design/files.md`, Defaults for the
/// dialogs): a checkout build uses its `examples/patches`; a release uses
/// `examples/patches` next to the program, or on macOS next to
/// `LoomDSP.app`. If that folder is missing, the checkout is the fallback.
/// `None` when neither exists. The app looks once, at start.
pub fn bundled_examples() -> Option<PathBuf> {
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches");
    examples_for_executable(std::env::current_exe().ok().as_deref(), &checkout)
}

/// Finds the examples that belong to this executable. A checkout build uses
/// its source tree; a bundled executable uses the folder beside the app.
fn examples_for_executable(executable: Option<&Path>, checkout: &Path) -> Option<PathBuf> {
    let mut bundled = Vec::new();
    if let Some(dir) = executable.and_then(Path::parent) {
        bundled.push(dir.join("examples/patches"));
        // LoomDSP.app/Contents/MacOS/loom-app: the folder holding the app.
        if dir.ends_with("Contents/MacOS")
            && let Some(beside_app) = dir.ancestors().nth(3)
        {
            bundled.push(beside_app.join("examples/patches"));
        }
    }
    // Cargo creates target/debug/examples for example binaries. A nearby
    // folder is a release bundle only when it contains patch files. This
    // also lets a bundle staged inside the checkout's dist/ take priority.
    let bundled = bundled.into_iter().find(|dir| {
        std::fs::read_dir(dir).is_ok_and(|entries| {
            entries
                .filter_map(Result::ok)
                .any(|entry| entry.path().extension().is_some_and(|ext| ext == EXTENSION))
        })
    });
    bundled.or_else(|| checkout.is_dir().then(|| checkout.to_path_buf()))
}

/// True when `path` is in the known examples folder or a marked release
/// patch folder. The marker works when macOS translocates the app away from
/// its download folder. A folder only named `examples` does not count.
pub fn is_example(path: &Path, examples: Option<&Path>) -> bool {
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    if examples
        .and_then(|dir| dir.canonicalize().ok())
        .is_some_and(|dir| path.starts_with(dir))
    {
        return true;
    }
    path.ancestors()
        .skip(1)
        .any(|dir| dir.ends_with("examples/patches") && dir.join(BUNDLED_MARKER).is_file())
}

/// Where a dialog starts: the folder of the current file, otherwise
/// `fallback`, the last folder used or the library folder, otherwise the
/// Documents folder. `None` lets the system choose.
fn start_dir(file: Option<&Path>, fallback: Option<&Path>) -> Option<PathBuf> {
    // A bare file name has an empty parent, which is not a folder.
    file.and_then(Path::parent)
        .filter(|dir| dir.is_dir())
        .or(fallback.filter(|dir| dir.is_dir()))
        .map(Path::to_path_buf)
        .or_else(|| crate::documents::folder().filter(|dir| dir.is_dir()))
}

/// The kinds of file the Open and Save dialogs offer.
fn patch_filters() -> Vec<FileFilter> {
    vec![FileFilter {
        name: "LoomDSP patch".to_string(),
        extensions: vec![EXTENSION.to_string()],
    }]
}

/// The Open dialog for a patch, starting in the current file's folder, or
/// `fallback`, or the Documents folder.
pub fn open_request(current: Option<&Path>, fallback: Option<&Path>) -> FileRequest {
    let folder = start_dir(current, fallback);
    #[cfg(test)]
    fake::asked_open(folder.clone());
    FileRequest {
        action: FileAction::Open,
        title: "Open patch".to_string(),
        filters: patch_filters(),
        folder,
        file_name: None,
    }
}

/// The Save dialog, starting at `current`: the current file, or a file
/// name to suggest. Its answer goes through [`saving`].
pub fn save_request(current: Option<&Path>) -> FileRequest {
    #[cfg(test)]
    fake::asked_save(current);
    FileRequest {
        action: FileAction::Save,
        title: "Save patch".to_string(),
        filters: patch_filters(),
        folder: start_dir(current, None),
        file_name: Some(display_name(current)),
    }
}

/// What the Save dialog's answer means. The result always has an
/// extension.
pub fn saving(picked: Option<PathBuf>) -> Saving {
    let Some(picked) = picked else {
        return Saving::Cancelled;
    };
    let path = with_extension(picked.clone());
    // The dialog checked the name it returned. If adding the extension made
    // a different file, that one needs its own confirmation.
    if path != picked && path.exists() {
        Saving::Confirm(path)
    } else {
        Saving::Chosen(path)
    }
}

/// The button that adds the user's name, in [`author_question`].
pub const ADD_MY_NAME: &str = "Add my name";

/// The question to add the user's name to the authors of a patch made by
/// others (patch-metadata.md, rule 3). `None` when no dialog can be shown:
/// then nothing is added, and the question is asked again next time. Its
/// answer goes through [`author_added`].
pub fn author_question(name: &str, authors: &[String]) -> Option<Question> {
    #[cfg(test)]
    if !fake::asked_add_author(name) {
        return None;
    }
    #[cfg(not(test))]
    if !dialogs_available() {
        return None;
    }
    let by = if authors.is_empty() {
        "This patch lists no authors.".to_string()
    } else {
        format!("This patch is by {}.", authors.join(", "))
    };
    Some(Question {
        level: Level::Info,
        title: format!("Add \"{name}\" as an author?"),
        text: format!("{by} Adding your name credits your changes. Every author stays in the list."),
        // Two answers only: the save goes ahead either way, so a Cancel
        // button would promise something the question does not do.
        buttons: vec![ADD_MY_NAME.to_string(), "Not for this patch".to_string()],
    })
}

/// True when the answer to [`author_question`] adds the name. A dialog
/// closed with no button declines, as "Not for this patch" does.
pub fn author_added(choice: Choice) -> bool {
    choice == Some(0)
}

/// False on Linux when no program that draws message boxes is installed.
/// Then the dialog would return "cancel" without showing anything. Only the
/// credit and crash recovery questions are still native.
pub fn dialogs_available() -> bool {
    // Tests say what the machine has, so they never depend on it.
    #[cfg(test)]
    if !fake::dialogs_available() {
        return false;
    }
    if !cfg!(target_os = "linux") || cfg!(test) {
        return true;
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| ["zenity", "kdialog"].iter().any(|p| dir.join(p).is_file()))
}

/// Stand-ins for the Open and Save dialogs and the credit question in
/// tests, which must not stop at a native dialog. Each test
/// runs on its own thread, so each has its own record.
#[cfg(test)]
pub mod fake;

#[cfg(test)]
mod tests;
