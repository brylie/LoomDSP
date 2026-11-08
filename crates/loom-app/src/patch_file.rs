//! Reading and writing patch files on disk.
//!
//! `loom-patch` only turns a patch into text and back, so it builds for
//! WebAssembly, which has no files (AGENTS.md, hard rule 6). The app is the
//! only part of LoomDSP that saves a patch, so the file code lives here.

use loom_patch::{Library, LibraryError, Patch, PatchError};
use std::fmt;
use std::path::{Path, PathBuf};

/// A patch file that could not be read or written.
#[derive(Debug)]
pub enum PatchFileError {
    /// The file could not be read or written.
    Io {
        /// The path as the caller gave it.
        path: String,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The text is not a patch this build can use.
    Patch(PatchError),
}

impl fmt::Display for PatchFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{path}: {source}"),
            Self::Patch(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for PatchFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Patch(error) => Some(error),
        }
    }
}

impl From<PatchError> for PatchFileError {
    fn from(error: PatchError) -> Self {
        Self::Patch(error)
    }
}

/// Reads a patch file.
pub fn load(path: impl AsRef<Path>) -> Result<Patch, PatchFileError> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).map_err(|source| PatchFileError::Io {
        path: path.display().to_string(),
        source,
    })?;
    Ok(Patch::from_ron(&text)?)
}

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

/// The library folder as the loader reads linked sub-patches from it
/// (`docs/design/sub-patches.md`, Linked library items). A library path is
/// relative to the folder, with `/` between its parts; one that leaves the
/// folder is treated as missing.
#[derive(Debug, Clone)]
pub struct FolderLibrary {
    /// The library folder.
    pub root: PathBuf,
}

impl FolderLibrary {
    /// The file a library path names, or `None` for a path that is not a
    /// plain relative one.
    pub fn file(&self, path: &str) -> Option<PathBuf> {
        let mut file = self.root.clone();
        for part in path.split('/') {
            if part.is_empty() || part == "." || part == ".." || part.contains(['\\', ':']) {
                return None;
            }
            file.push(part);
        }
        Some(file)
    }

    /// The library path of `file`, when it is inside the folder: its parts
    /// below the folder joined by `/`, as a definition's `library` writes it.
    pub fn path_of(&self, file: &Path) -> Option<String> {
        let relative = file.strip_prefix(&self.root).ok()?;
        let parts: Vec<String> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        (!parts.is_empty()).then(|| parts.join("/"))
    }
}

impl Library for FolderLibrary {
    fn read(&self, path: &str) -> Result<Patch, LibraryError> {
        let Some(file) = self.file(path) else {
            return Err(LibraryError::Missing);
        };
        match load(&file) {
            Ok(patch) => Ok(patch),
            Err(PatchFileError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                Err(LibraryError::Missing)
            }
            Err(error) => Err(LibraryError::Unreadable(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests;
