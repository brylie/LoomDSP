//! Your patches browser: the patches in the library folder, with a search
//! and filters by author, tag, and licence. Spec:
//! `docs/design/patch-metadata.md`, Your patches browser (FR-SHARE-06).
//!
//! The folder is read off the UI thread when the browser opens, and rows
//! appear as files are read. Only the metadata of each file is kept, so a
//! folder of hundreds of patches never freezes the window.

use crate::style::{self, button};
use crate::{DIM, files, recovery};
use iced::widget::{checkbox, column, container, mouse_area, opaque, row, scrollable, space, text, text_input};
use iced::{Element, Length, Task, Theme};
use loom_patch::Metadata;
use loom_patch::metadata::{Filter, Licence, LicenceFilter, tidy_tag};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The search field's widget ID: typing goes there when the browser opens.
const SEARCH: &str = "patch-browser-search";
/// The Author filter's field.
const AUTHOR: &str = "patch-browser-author";
/// The browser's text fields, in the order Tab moves between them.
pub(crate) const TEXT_FIELDS: [&str; 2] = [SEARCH, AUTHOR];
/// The list's widget ID, for keeping the highlight in view.
const LIST: &str = "patch-browser-list";
/// Height of one row. Fixed, so the list can scroll to a row.
const ROW_HEIGHT: f32 = 46.0;
/// Height of the list.
const LIST_HEIGHT: f32 = 10.0 * ROW_HEIGHT;

/// One file in the library folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Where the file is.
    pub path: PathBuf,
    /// When it last changed, for the order and the date shown.
    pub changed: Option<SystemTime>,
    /// Its metadata, or `None` when it does not read as a patch: the row
    /// shows "Could not read" and cannot be opened.
    pub info: Option<Metadata>,
}

impl Row {
    /// Reads the metadata of the patch at `path`.
    pub fn read(path: &Path) -> Self {
        let info = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| Metadata::from_ron(&text).ok());
        Self {
            path: path.to_path_buf(),
            changed: std::fs::metadata(path).and_then(|m| m.modified()).ok(),
            info,
        }
    }

    /// The name shown: the patch's name, or its file name when it has none
    /// or does not read.
    pub fn name(&self) -> String {
        self.info
            .as_ref()
            .map(|info| info.name.trim())
            .filter(|name| !name.is_empty())
            .map_or_else(|| files::display_name(Some(&self.path)), str::to_string)
    }
}

/// Every patch file in `folder` and its subfolders, in no set order.
/// Hidden files and folders, such as a save's temporary file, are skipped.
pub fn patch_files(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(dir) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => folders.push(path),
                Ok(_) if path.extension().is_some_and(|e| e == files::EXTENSION) => found.push(path),
                _ => {}
            }
        }
    }
    found
}

/// Reads the patch files in `folder` on a thread, one message per file,
/// so the window never waits for the disk.
#[cfg(not(test))]
fn read(open: u64, folder: PathBuf) -> Task<Message> {
    let (sender, receiver) = iced::futures::channel::mpsc::unbounded();
    std::thread::spawn(move || {
        for path in patch_files(&folder) {
            let row = Box::new(Row::read(&path));
            if sender.unbounded_send(Message::Found { open, row }).is_err() {
                return;
            }
        }
    });
    Task::run(receiver, |message| message)
}

/// Reads the patch files at once: the headless test harness runs a task
/// to its end and cannot wait for a thread.
#[cfg(test)]
fn read(open: u64, folder: PathBuf) -> Task<Message> {
    let rows = patch_files(&folder).into_iter().map(|path| {
        Task::done(Message::Found {
            open,
            row: Box::new(Row::read(&path)),
        })
    });
    Task::batch(rows)
}

/// A browser message.
#[derive(Debug, Clone)]
pub enum Message {
    /// A file was read, for the browser opened as `open`.
    Found { open: u64, row: Box<Row> },
    /// The search field changed.
    Search(String),
    /// The author filter changed.
    Author(String),
    /// A tag was picked or unpicked.
    Tag(String),
    /// A licence filter was turned on or off.
    Licence(LicenceFilter, bool),
    /// The pointer moved over a row of the list.
    Hover(usize),
    /// A row was clicked.
    Pick(usize),
    /// Enter: open the highlighted patch.
    Submit,
    /// The list scrolled.
    Scrolled(f32),
    /// Up or Down.
    Step(i32),
    /// Escape, a click outside, or **Close**.
    Close,
}

/// What the app should do after a browser message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Keep it open.
    Stay,
    /// Close it.
    Close,
    /// Close it and open this patch, asking about unsaved changes first.
    Open(PathBuf),
}

/// The open browser.
pub struct PatchBrowser {
    /// Which opening this is, so rows read for an earlier one are dropped.
    open: u64,
    /// The library folder.
    folder: PathBuf,
    /// Every file read so far.
    rows: Vec<Row>,
    /// The search and the filters.
    filter: Filter,
    /// The rows that match, by index into `rows`, newest first.
    shown: Vec<usize>,
    /// The highlighted entry of `shown`.
    highlight: usize,
    /// The list's scroll offset.
    scroll: f32,
}

impl PatchBrowser {
    /// An empty browser over `folder`. Rows come in with [`Message::Found`].
    pub fn new(open: u64, folder: PathBuf) -> Self {
        Self {
            open,
            folder,
            rows: Vec::new(),
            filter: Filter::default(),
            shown: Vec::new(),
            highlight: 0,
            scroll: 0.0,
        }
    }

    /// Opens a browser over `folder` and the task that reads it on a
    /// thread, one message per file.
    pub fn open(open: u64, folder: PathBuf) -> (Self, Task<Message>) {
        let task = Task::batch([read(open, folder.clone()), iced::widget::operation::focus(SEARCH)]);
        (Self::new(open, folder), task)
    }

    /// The rows the list shows, in order.
    pub fn shown(&self) -> impl Iterator<Item = &Row> {
        self.shown.iter().map(|&i| &self.rows[i])
    }

    /// The highlighted row, if any.
    pub fn highlighted(&self) -> Option<&Row> {
        self.shown.get(self.highlight).map(|&i| &self.rows[i])
    }

    /// Every tag in the folder, tidied, sorted, each once, for the tag
    /// filter.
    pub fn tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self
            .rows
            .iter()
            .filter_map(|row| row.info.as_ref())
            .flat_map(|info| info.tags.iter().filter_map(|t| tidy_tag(t)))
            .collect();
        tags.sort();
        tags.dedup();
        tags
    }

    /// Handles a message, and returns what the app should do and a task
    /// that keeps the highlighted row in view.
    pub fn update(&mut self, message: Message) -> (Outcome, Task<Message>) {
        let follow = matches!(message, Message::Step(_));
        let outcome = match message {
            Message::Found { open, row } => {
                if open == self.open {
                    self.rows.push(*row);
                    self.refilter();
                }
                Outcome::Stay
            }
            Message::Search(text) => {
                self.filter.search = text;
                self.refilter();
                self.highlight = 0;
                Outcome::Stay
            }
            Message::Author(text) => {
                self.filter.author = text;
                self.refilter();
                self.highlight = 0;
                Outcome::Stay
            }
            Message::Tag(tag) => {
                match self.filter.tags.iter().position(|t| *t == tag) {
                    Some(at) => {
                        self.filter.tags.remove(at);
                    }
                    None => self.filter.tags.push(tag),
                }
                self.refilter();
                self.highlight = 0;
                Outcome::Stay
            }
            Message::Licence(filter, on) => {
                self.filter.licence.retain(|f| *f != filter);
                if on {
                    self.filter.licence.push(filter);
                }
                self.refilter();
                self.highlight = 0;
                Outcome::Stay
            }
            Message::Hover(row) => {
                if row < self.shown.len() {
                    self.highlight = row;
                }
                Outcome::Stay
            }
            Message::Pick(row) => {
                self.highlight = row;
                self.open_highlighted()
            }
            Message::Submit => self.open_highlighted(),
            Message::Scrolled(offset) => {
                self.scroll = offset;
                Outcome::Stay
            }
            Message::Step(by) => {
                let last = self.shown.len().saturating_sub(1);
                self.highlight = self.highlight.saturating_add_signed(by as isize).min(last);
                Outcome::Stay
            }
            Message::Close => Outcome::Close,
        };
        let task = if follow { self.reveal() } else { Task::none() };
        (outcome, task)
    }

    /// The highlighted patch to open, if it read.
    fn open_highlighted(&self) -> Outcome {
        match self.highlighted() {
            Some(row) if row.info.is_some() => Outcome::Open(row.path.clone()),
            _ => Outcome::Stay,
        }
    }

    /// Works out which rows match, newest first. A file that does not read
    /// matches only an empty search and no filters, so it is still listed.
    fn refilter(&mut self) {
        let empty = self.filter == Filter::default();
        let highlighted = self.highlighted().map(|row| row.path.clone());
        let mut shown: Vec<usize> = (0..self.rows.len())
            .filter(|&i| match &self.rows[i].info {
                Some(info) => self.filter.matches(info),
                None => empty,
            })
            .collect();
        shown.sort_by(|&a, &b| {
            let (a, b) = (&self.rows[a], &self.rows[b]);
            b.changed.cmp(&a.changed).then_with(|| a.path.cmp(&b.path))
        });
        self.shown = shown;
        // Rows that arrive while the user looks keep the highlight on
        // the same patch.
        if let Some(path) = highlighted {
            let at = self.shown().position(|row| row.path == path);
            self.highlight = at.unwrap_or(0);
        }
    }

    /// Scrolls the list just enough to show the highlighted row.
    fn reveal(&mut self) -> Task<Message> {
        let top = self.highlight as f32 * ROW_HEIGHT;
        let offset = if top < self.scroll {
            top
        } else if top + ROW_HEIGHT > self.scroll + LIST_HEIGHT {
            top + ROW_HEIGHT - LIST_HEIGHT
        } else {
            return Task::none();
        };
        self.scroll = offset;
        iced::widget::operation::scroll_to(LIST, iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: offset })
    }

    /// The browser, as a panel over the canvas.
    pub fn view(&self) -> Element<'_, Message> {
        let search = text_input("Search names, descriptions, and tags", &self.filter.search)
            .id(SEARCH)
            .on_input(Message::Search)
            .on_submit(Message::Submit)
            .size(style::CONTROL)
            .padding(6);
        let author = text_input("Author", &self.filter.author)
            .id(AUTHOR)
            .on_input(Message::Author)
            .on_submit(Message::Submit)
            .size(style::CONTROL)
            .width(200);
        let mut licences = row![].spacing(12);
        for filter in LicenceFilter::ALL {
            licences = licences.push(
                checkbox(self.filter.licence.contains(&filter))
                    .label(filter.label())
                    .size(style::SWITCH)
                    .text_size(style::CONTROL)
                    .on_toggle(move |on| Message::Licence(filter, on)),
            );
        }
        let chips = self.tags().into_iter().map(|tag| {
            let picked = self.filter.tags.contains(&tag);
            button(text(tag.clone()).size(style::SMALL))
                .padding([2, 8])
                // A picked tag looks like an open tab: filled and outlined.
                // The primary role is kept for one main action per view.
                .style(style::tab(picked))
                .on_press(Message::Tag(tag))
                .into()
        });
        let tags = iced::widget::Row::with_children(chips).spacing(4).wrap();

        let rows = self.shown().enumerate().map(|(i, r)| {
            let highlighted = i == self.highlight;
            let details = match &r.info {
                Some(info) => {
                    let when = r.changed.map(recovery::local_time).unwrap_or_default();
                    let licence = match Licence::of(info.license.as_deref()) {
                        Licence::None => String::new(),
                        other => other.short(),
                    };
                    let parts: Vec<String> = [info.authors.join(", "), info.tags.join(" · "), licence, when]
                        .into_iter()
                        .filter(|part| !part.is_empty())
                        .collect();
                    parts.join("   ")
                }
                None => "Could not read".to_string(),
            };
            let cell = container(
                column![
                    text(r.name()).size(style::CONTROL),
                    text(details).size(style::SMALL).color(style::QUIET),
                ]
                .spacing(2),
            )
            .padding([4, 8])
            .height(ROW_HEIGHT)
            .width(Length::Fill)
            .style(move |theme: &Theme| crate::browser::row_style(theme, highlighted));
            mouse_area(cell)
                .on_move(move |_| Message::Hover(i))
                .on_press(Message::Pick(i))
                .into()
        });
        let list: Element<'_, Message> = if self.shown.is_empty() {
            let line = if self.rows.is_empty() {
                format!("No patches in {} yet.", self.folder.display())
            } else {
                "No patches match.".to_string()
            };
            container(text(line).size(style::CONTROL).color(style::QUIET))
                .padding(8)
                .height(LIST_HEIGHT)
                .into()
        } else {
            scrollable(column(rows))
                .id(LIST)
                .spacing(4)
                .height(LIST_HEIGHT)
                .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y))
                .into()
        };
        let panel = column![
            row![
                text("Your patches").size(style::TITLE),
                space::horizontal(),
                button(text("Close").size(style::CONTROL)).on_press(Message::Close),
            ]
            .align_y(iced::Alignment::Center),
            search,
            row![author, licences].spacing(12).align_y(iced::Alignment::Center),
            tags,
            list,
            text(self.folder.display().to_string()).size(style::SMALL).color(DIM),
        ]
        .spacing(8);
        let panel = container(panel).padding(16).width(720).style(container::rounded_box);
        opaque(
            container(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .center(Length::Fill)
                .style(|_| container::background(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.4))),
        )
    }
}

/// The keys the open browser handles, whether or not a widget used them:
/// Up, Down, and Escape (Your patches browser, Keys).
pub(crate) fn key(
    event: iced::Event,
    _status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<crate::Message> {
    use iced::keyboard::{Event, Key, key::Named};
    let iced::Event::Keyboard(Event::KeyPressed { key, .. }) = event else {
        return None;
    };
    let message = match key {
        Key::Named(Named::ArrowUp) => Message::Step(-1),
        Key::Named(Named::ArrowDown) => Message::Step(1),
        Key::Named(Named::Escape) => Message::Close,
        _ => return None,
    };
    Some(crate::Message::Patches(message))
}

#[cfg(test)]
mod tests;
