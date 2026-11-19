//! Writing patch files on disk. Reading is shared with the CLI.

pub use loom_host_desktop::patch_file::{FolderLibrary, PatchFileError, load};
use loom_patch::Patch;
use std::path::Path;

/// Writes a patch file.
///
/// The text goes to a new temporary file in the same folder first, then
/// replaces the target by a rename. A full disk or a failed write cannot
/// leave a half-written patch where a good one was. It does not promise
/// the data survives a power loss. The temporary name is
/// unique to this process and is never an existing file, so another
/// save or a backup is never overwritten. A read-only target is refused,
/// and the target keeps its permissions.
pub fn save(patch: &Patch, path: impl AsRef<Path>) -> Result<(), PatchFileError> {
    use std::io::Write;
    let path = path.as_ref();
    let text = patch.to_ron()?;
    // Write through a symlink to its target, as a plain write would. A
    // rename over the link would replace the link itself.
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let target = resolved.as_path();
    #[cfg(any(test, feature = "test-ui"))]
    refuse_examples(target);
    let io = |source| PatchFileError::Io {
        path: path.display().to_string(),
        source,
    };
    let existing = std::fs::metadata(target).ok();
    if existing.as_ref().is_some_and(|m| m.permissions().readonly()) {
        return Err(io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "the file is read-only",
        )));
    }
    let name = target.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    let temp = target.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    // An open that fails returns at once: the temporary name may belong to
    // a file this save did not create, and cleanup must not remove it.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(io)?;
    let written = file.write_all(text.as_bytes());
    drop(file);
    let written = written
        .and_then(|()| match &existing {
            Some(meta) => std::fs::set_permissions(&temp, meta.permissions()),
            None => Ok(()),
        })
        .and_then(|()| std::fs::rename(&temp, target));
    written.map_err(|source| {
        let _ = std::fs::remove_file(&temp);
        io(source)
    })
}

/// In a test build, panics when `target` is inside the repository's
/// `examples` folder, naming the test: a test must never write the
/// example patches the app ships. The `Ui` harness opens copies of them
/// (`ui_test::example_copy`), so this trips only when a test or a helper
/// reaches a real example another way.
#[cfg(any(test, feature = "test-ui"))]
fn refuse_examples(target: &Path) {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let Ok(examples) = std::fs::canonicalize(examples) else {
        return;
    };
    let parent = target.parent().and_then(|parent| std::fs::canonicalize(parent).ok());
    if parent.is_some_and(|parent| parent.starts_with(&examples)) {
        let test = std::thread::current().name().unwrap_or("a test").to_owned();
        panic!(
            "{test} tried to save over an example in the repository: {}",
            target.display()
        );
    }
}

#[cfg(test)]
mod tests;
