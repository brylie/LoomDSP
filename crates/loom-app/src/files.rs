//! File dialogs and the words around them (`docs/design/files.md`).
//!
//! The dialogs are native and blocking. They run on the UI thread, which is
//! what the operating systems expect; audio runs on its own thread and keeps
//! playing while one is open.

use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::path::{Path, PathBuf};

/// The file extension of a patch.
pub const EXTENSION: &str = "ron";

/// A marker put in release patch folders so examples are still recognized
/// when macOS runs the app from a translocated copy.
const BUNDLED_MARKER: &str = ".loomdsp-bundled-examples";

/// What the user chose when asked about unsaved changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Save,
    DontSave,
    Cancel,
    /// No dialog could be shown (Linux without `zenity` or `kdialog`).
    Unavailable,
}

/// The name shown for a patch: its file name, or "Untitled" before it has one.
pub fn display_name(file: Option<&Path>) -> String {
    file.and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_string())
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

/// Where a dialog starts: the folder of the current file, otherwise the
/// Documents folder. `None` lets the system choose.
fn start_dir(file: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = file.and_then(Path::parent).filter(|d| d.is_dir()) {
        return Some(dir.to_path_buf());
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join("Documents")).filter(|d| d.is_dir())
}

fn dialog(file: Option<&Path>) -> FileDialog {
    let dialog = FileDialog::new().add_filter("LoomDSP patch", &[EXTENSION]);
    match start_dir(file) {
        Some(dir) => dialog.set_directory(dir),
        None => dialog,
    }
}

/// Asks for a patch to open.
pub fn pick_open(current: Option<&Path>) -> Option<PathBuf> {
    dialog(current).set_title("Open patch").pick_file()
}

/// Asks where to save, starting at `current`: the current file, or a file
/// name to suggest. The result always has an extension.
pub fn pick_save(current: Option<&Path>) -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(answer) = fake::pick_save(current) {
        return answer;
    }
    let picked = dialog(current)
        .set_title("Save patch")
        .set_file_name(display_name(current))
        .save_file()?;
    let path = with_extension(picked.clone());
    // The dialog checked the name it returned. If adding the extension made
    // a different file, that one needs its own confirmation.
    if path != picked && path.exists() {
        // With no way to ask, keep the name the dialog confirmed.
        if !dialogs_available() {
            return Some(picked);
        }
        if !confirm_replace(&path) {
            return None;
        }
    }
    Some(path)
}

fn confirm_replace(path: &Path) -> bool {
    let result = MessageDialog::new()
        .set_level(MessageLevel::Warning)
        .set_title(format!("Replace \"{}\"?", display_name(Some(path))))
        .set_description("A file with this name already exists. Saving replaces it.")
        .set_buttons(MessageButtons::YesNo)
        .show();
    result == MessageDialogResult::Yes
}

/// Asks what to do with unsaved changes.
pub fn confirm_discard(name: &str) -> Choice {
    #[cfg(test)]
    if let Some(choice) = fake::confirm_discard(name) {
        return choice;
    }
    if !dialogs_available() {
        return Choice::Unavailable;
    }
    let result = MessageDialog::new()
        .set_level(MessageLevel::Warning)
        .set_title(format!("Save changes to \"{name}\"?"))
        .set_description("Your changes will be lost if you do not save them.")
        .set_buttons(MessageButtons::YesNoCancelCustom(
            "Save".to_string(),
            "Don't Save".to_string(),
            "Cancel".to_string(),
        ))
        .show();
    match result {
        MessageDialogResult::Custom(label) if label == "Save" => Choice::Save,
        MessageDialogResult::Custom(label) if label == "Don't Save" => Choice::DontSave,
        MessageDialogResult::Yes => Choice::Save,
        MessageDialogResult::No => Choice::DontSave,
        _ => Choice::Cancel,
    }
}

/// False on Linux when no program that draws message boxes is installed.
/// Then the dialog would return "cancel" without showing anything.
pub fn dialogs_available() -> bool {
    if !cfg!(target_os = "linux") {
        return true;
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| ["zenity", "kdialog"].iter().any(|p| dir.join(p).is_file()))
}

/// Tells the user that something failed. One button, so it cannot be a
/// question.
pub fn show_error(message: &str) {
    MessageDialog::new()
        .set_level(MessageLevel::Error)
        .set_title("LoomDSP")
        .set_description(message)
        .set_buttons(MessageButtons::Ok)
        .show();
}

/// Stand-ins for the Save dialog and the unsaved changes question in
/// tests, which must not stop at a native dialog. Each test runs on its own
/// thread, so each has its own record.
#[cfg(test)]
pub mod fake;

#[cfg(test)]
mod tests;
