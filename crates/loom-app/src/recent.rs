//! The recent patches, the last folder used in a dialog, and files dropped on
//! the window (`docs/design/files.md`, Recent files, the last folder, and
//! dropping a file).
//!
//! The list belongs to the machine, not to one patch, so it lives in the
//! [`Rig`](crate::settings::Rig) and outlives each engine. A failed read or
//! write never stops an open or a save: it is only a convenience.

use crate::files;
use crate::style::{self, button};
use iced::widget::{column, container, row, space, text, tooltip};
use iced::{Element, Length, Point};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file beside `preferences.ron`.
pub const FILE_NAME: &str = "recent.ron";
/// How many patches the list holds.
const MAX_FILES: usize = 10;
/// Where the list opens, under the file buttons in the toolbar.
pub const LIST_AT: Point = Point::new(8.0, 44.0);

/// What `recent.ron` holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Saved {
    /// Newest first.
    files: Vec<PathBuf>,
    /// The last folder used in a dialog.
    last_folder: Option<PathBuf>,
}

/// The recent patches and the last folder, and the file that keeps them.
#[derive(Debug, Clone, Default)]
pub struct RecentList {
    /// `None` when no settings folder was found, and in tests that do not
    /// name a file: nothing is read or written.
    path: Option<PathBuf>,
    saved: Saved,
}

impl RecentList {
    /// A list that keeps nothing between runs.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Reads the list beside the preferences file at `preferences`. A file
    /// that does not read gives an empty list and a note for the status bar.
    pub fn open(preferences: Option<&Path>) -> (Self, Option<String>) {
        let path = preferences.map(|file| file.with_file_name(FILE_NAME));
        let text = path.as_deref().and_then(|file| std::fs::read_to_string(file).ok());
        let (saved, note) = match text.map(|text| ron::from_str::<Saved>(&text)) {
            None => (Saved::default(), None),
            Some(Ok(saved)) => (saved, None),
            Some(Err(e)) => (
                Saved::default(),
                Some(format!(
                    "Could not read the recent patches: {e}. The list starts empty."
                )),
            ),
        };
        (Self { path, saved }, note)
    }

    /// The patches, newest first.
    pub fn files(&self) -> &[PathBuf] {
        &self.saved.files
    }

    /// The last folder used in a dialog, if it is still there.
    pub fn last_folder(&self) -> Option<&Path> {
        self.saved.last_folder.as_deref().filter(|dir| dir.is_dir())
    }

    /// Puts `path` at the top of the list, once, and keeps the newest ones.
    pub fn remember(&mut self, path: &Path) -> Result<(), String> {
        self.saved.files.retain(|file| file != path);
        self.saved.files.insert(0, path.to_path_buf());
        self.saved.files.truncate(MAX_FILES);
        self.write()
    }

    /// Takes `path` off the list.
    pub fn forget(&mut self, path: &Path) -> Result<(), String> {
        self.saved.files.retain(|file| file != path);
        self.write()
    }

    /// Notes the folder of `path`, picked in a dialog.
    pub fn used_folder_of(&mut self, path: &Path) -> Result<(), String> {
        self.saved.last_folder = path.parent().map(Path::to_path_buf);
        self.write()
    }

    /// Writes the file through a temporary file and a rename.
    fn write(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let text = ron::ser::to_string_pretty(&self.saved, ron::ser::PrettyConfig::new()).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        crate::recovery::write_atomic(path, &text).map_err(|e| e.to_string())
    }
}

/// One row of the open list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Row {
    /// Where the file is.
    pub path: PathBuf,
    /// Its file name, which the row shows.
    pub name: String,
    /// Its folder, the row's hover text.
    pub folder: String,
    /// The file was not there when the list opened: the row is dimmed.
    pub missing: bool,
}

/// The rows for `files`. This looks at the disk, so it runs when the list
/// opens, not on every frame.
pub fn rows(files: &[PathBuf]) -> Vec<Row> {
    files
        .iter()
        .map(|path| Row {
            path: path.clone(),
            name: files::display_name(Some(path)),
            folder: path.parent().map(|dir| dir.display().to_string()).unwrap_or_default(),
            missing: !path.is_file(),
        })
        .collect()
}

/// A message for the open list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// A row was chosen.
    Pick(usize),
    /// Escape, or a click outside.
    Close,
}

/// The open list, with the row its keys are on outlined. Each row, and the
/// list, sit in a container named by its UI ID (`menus.rs`).
pub fn view(rows: &[Row], highlight: Option<usize>) -> Element<'_, Message> {
    let mut list = column![].spacing(2);
    for (i, row_data) in rows.iter().enumerate() {
        let name = text(&row_data.name).size(style::CONTROL);
        // Dimmed, and hover text says where it was: a shape alone cannot
        // show that a file has gone.
        let name = if row_data.missing {
            name.color(style::QUIET)
        } else {
            name
        };
        let line = row![name, space::horizontal()];
        let entry = button(line)
            .style(style::outlined(style::plain, highlight == Some(i)))
            .width(Length::Fill)
            .on_press(Message::Pick(i));
        let entry = crate::inspector::ids::named(
            &crate::menus::recent_id().with(row_data.path.display().to_string().as_str()),
            entry,
        );
        let hover = container(text(&row_data.folder).size(style::BODY))
            .padding(6)
            .style(container::rounded_box);
        list = list.push(tooltip(entry, hover, tooltip::Position::Right));
    }
    crate::inspector::ids::named(
        &crate::menus::recent_id(),
        container(list).padding(6).width(280).style(container::rounded_box),
    )
}

/// What a drop of `dropped` files comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dropped {
    /// Open this patch. `several` says other files came with it.
    Open { path: PathBuf, several: bool },
    /// None of them is a patch.
    NoPatch,
}

/// The first patch among `dropped`, whatever comes before it.
pub fn dropped(files: &[PathBuf]) -> Dropped {
    let patch = files.iter().find(|path| {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case(files::EXTENSION))
    });
    match patch {
        Some(path) => Dropped::Open {
            path: path.clone(),
            several: files.len() > 1,
        },
        None => Dropped::NoPatch,
    }
}

/// A file dropped on the window, as a message. The window reports each file
/// of a drop on its own, so the app collects them for a moment.
pub fn drop_event(
    event: iced::Event,
    _status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<crate::Message> {
    match event {
        iced::Event::Window(iced::window::Event::FileDropped(path)) => Some(crate::Message::FileDropped(path)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
