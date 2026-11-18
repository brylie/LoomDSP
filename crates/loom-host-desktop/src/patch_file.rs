//! Reading patches and linked sub-patches from disk.

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
