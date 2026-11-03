//! The user's Documents folder, as the system reports it (#556).
//!
//! `$HOME/Documents` is only a guess. On Windows, OneDrive or the user can
//! move the folder. On Linux, `user-dirs.dirs` names it, often in the user's
//! language. A wrong guess makes the default library folder in a place the
//! user does not look.
//!
//! The answer may not exist yet, because the library folder makes its
//! parents. A caller that needs a folder to exist checks for itself.

use std::path::{Path, PathBuf};

/// The user's Documents folder. Tests never look in the real one: they name
/// a library folder.
pub fn folder() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    system_folder()
}

/// Asks Windows, which knows where the folder went. If it cannot say, the
/// usual place is the best guess left.
#[cfg(windows)]
fn system_folder() -> Option<PathBuf> {
    known_folders::get_known_folder_path(known_folders::KnownFolder::Documents)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("Documents")))
}

/// macOS keeps the folder at its usual place and only shows a translated
/// name. Other Unix systems say in `user-dirs.dirs`.
#[cfg(not(windows))]
fn system_folder() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    if !cfg!(target_os = "macos") {
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        let file = user_dirs_file(config.as_deref(), &home);
        let named = std::fs::read_to_string(file)
            .ok()
            .and_then(|text| from_user_dirs(&text, &home));
        if named.is_some() {
            return named;
        }
    }
    Some(home.join("Documents"))
}

/// Where `user-dirs.dirs` is: in `XDG_CONFIG_HOME`, or else in `~/.config`.
/// An empty or relative `XDG_CONFIG_HOME` counts as unset, as the XDG base
/// directory rules say.
#[cfg_attr(windows, allow(dead_code))]
fn user_dirs_file(config_home: Option<&Path>, home: &Path) -> PathBuf {
    config_home
        .filter(|dir| dir.is_absolute())
        .map_or_else(|| home.join(".config"), Path::to_path_buf)
        .join("user-dirs.dirs")
}

/// The Documents folder named in the text of a `user-dirs.dirs` file. A line
/// looks like `XDG_DOCUMENTS_DIR="$HOME/Dokumente"`, and its path is either
/// absolute or starts with `$HOME/`. A folder set to `$HOME` itself means the
/// user turned it off, so it gives `None`, as does a line that is neither.
#[cfg_attr(windows, allow(dead_code))]
fn from_user_dirs(text: &str, home: &Path) -> Option<PathBuf> {
    let value = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| line.strip_prefix("XDG_DOCUMENTS_DIR="))?;
    let value = value.trim().strip_prefix('"')?.strip_suffix('"')?;
    if let Some(rest) = value.strip_prefix("$HOME/") {
        let rest = rest.trim_end_matches('/');
        return (!rest.is_empty()).then(|| home.join(rest));
    }
    let path = Path::new(value);
    (path.is_absolute() && path != home).then(|| path.to_path_buf())
}

#[cfg(test)]
mod tests;
