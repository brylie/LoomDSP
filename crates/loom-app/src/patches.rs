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
use loom_patch::metadata::{Filter, Licence, LicenceFilter, Prepared};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime};

/// The search field's widget ID: typing goes there when the browser opens.
pub(crate) const SEARCH: &str = "patch-browser-search";
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
/// Most rows sent to the UI at once. Each batch sorts the whole list again,
/// so sending rows one by one costs more with every file in the folder.
const BATCH_ROWS: usize = 32;
/// Longest a read row waits for its batch, so a slow drive still shows rows
/// as they are read (Your patches browser, Reading files).
const BATCH_WAIT: Duration = Duration::from_millis(50);

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
    /// What the filters compare, worked out once. `None` when it does not
    /// read.
    prepared: Option<Prepared>,
    /// The line under the name, worked out once, because the list builds
    /// every row on every frame.
    details: String,
}

impl Row {
    /// Reads the metadata of the patch at `path`.
    pub fn read(path: &Path) -> Self {
        let info = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| Metadata::from_ron(&text).ok());
        let changed = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        Self {
            path: path.to_path_buf(),
            changed,
            prepared: info.as_ref().map(Prepared::of),
            details: details(info.as_ref(), changed),
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

/// The line under a row's name: authors, tags, licence, and date.
fn details(info: Option<&Metadata>, changed: Option<SystemTime>) -> String {
    let Some(info) = info else {
        return "Could not read".to_string();
    };
    let when = changed.map(recovery::local_time).unwrap_or_default();
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

/// Every patch file in `folder` and its subfolders, in no set order.
/// Hidden files and folders, such as a save's temporary file, are skipped.
/// Stops at the next folder once `stop` is set, so a closed browser does not
/// go on walking a large library on a slow drive.
pub fn patch_files(folder: &Path, stop: &AtomicBool) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(dir) = folders.pop() {
        if stop.load(Ordering::Relaxed) {
            return Vec::new();
        }
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

/// Rows waiting to be sent. Pure, with the time passed in, so a test can say
/// exactly when a batch goes.
#[derive(Default)]
struct Batch {
    rows: Vec<Row>,
    /// When the first waiting row arrived.
    since: Option<Instant>,
}

impl Batch {
    /// Adds a row read at `now`, and returns the rows to send, if it is time.
    fn push(&mut self, row: Row, now: Instant) -> Option<Vec<Row>> {
        let since = *self.since.get_or_insert(now);
        self.rows.push(row);
        (self.rows.len() >= BATCH_ROWS || now.duration_since(since) >= BATCH_WAIT).then(|| self.take())
    }

    /// When the oldest waiting row has waited long enough, if one is waiting.
    fn deadline(&self) -> Option<Instant> {
        self.since.map(|since| since + BATCH_WAIT)
    }

    /// The waiting rows, which are then forgotten.
    fn take(&mut self) -> Vec<Row> {
        self.since = None;
        std::mem::take(&mut self.rows)
    }
}

/// Reads each patch file in `folder` with `read_row`, and gives each row to
/// `send`. Stops within one file of `stop` being set, or when `send` says
/// nobody is listening.
fn read_rows(
    folder: &Path,
    stop: &AtomicBool,
    mut read_row: impl FnMut(&Path) -> Row,
    mut send: impl FnMut(Row) -> bool,
) {
    for path in patch_files(folder, stop) {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if !send(read_row(&path)) {
            return;
        }
    }
}

/// Gathers the rows that come from `rows` into batches for `send`, until the
/// reader has finished or nobody is listening. It waits for a row only as
/// long as the oldest waiting row may wait. *Why not in the reader:* a read
/// from a slow drive would hold back the rows read before it.
fn send_batches(rows: &mpsc::Receiver<Row>, mut now: impl FnMut() -> Instant, mut send: impl FnMut(Vec<Row>) -> bool) {
    let mut batch = Batch::default();
    loop {
        let received = match batch.deadline() {
            Some(deadline) => rows.recv_timeout(deadline.saturating_duration_since(now())),
            None => rows.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let ready = match received {
            Ok(row) => batch.push(row, now()),
            Err(RecvTimeoutError::Timeout) => Some(batch.take()),
            Err(RecvTimeoutError::Disconnected) => {
                let rows = batch.take();
                if !rows.is_empty() {
                    send(rows);
                }
                return;
            }
        };
        if let Some(rows) = ready
            && !send(rows)
        {
            return;
        }
    }
}

/// Reads the patch files in `folder` on one thread and sends the rows in
/// batches from another, so the window never waits for the disk. The reader
/// ends when `stop` is set, so a closed browser does not go on reading a large
/// or slow library. The batcher then ends too, which ends the task. If the
/// task goes first, the batcher ends, and then the reader.
#[cfg(not(test))]
fn read(open: u64, folder: PathBuf, stop: Arc<AtomicBool>) -> Task<Message> {
    let (sender, receiver) = iced::futures::channel::mpsc::unbounded();
    let (rows_in, rows_out) = mpsc::channel();
    std::thread::spawn(move || {
        read_rows(&folder, &stop, Row::read, |row| rows_in.send(row).is_ok());
    });
    std::thread::spawn(move || {
        send_batches(&rows_out, Instant::now, |rows| {
            sender.unbounded_send(Message::Found { open, rows }).is_ok()
        });
    });
    Task::run(receiver, |message| message)
}

/// Reads the patch files at once: the headless test harness runs a task
/// to its end and cannot wait for a thread.
#[cfg(test)]
fn read(open: u64, folder: PathBuf, stop: Arc<AtomicBool>) -> Task<Message> {
    let (rows_in, rows_out) = mpsc::channel();
    read_rows(&folder, &stop, Row::read, |row| rows_in.send(row).is_ok());
    drop(rows_in);
    let mut tasks = Vec::new();
    send_batches(&rows_out, Instant::now, |rows| {
        tasks.push(Task::done(Message::Found { open, rows }));
        true
    });
    Task::batch(tasks)
}

/// A browser message.
#[derive(Debug, Clone)]
pub enum Message {
    /// Files were read, for the browser opened as `open`.
    Found { open: u64, rows: Vec<Row> },
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
    /// Every tag in those files, tidied. A set that grows as rows arrive,
    /// because the tag filter draws it on every frame.
    tags: BTreeSet<String>,
    /// How many times the list was worked out, so a test can count.
    #[cfg(test)]
    refilters: usize,
    /// The search and the filters.
    filter: Filter,
    /// The rows that match, by index into `rows`, newest first.
    shown: Vec<usize>,
    /// The highlighted entry of `shown`.
    highlight: usize,
    /// The list's scroll offset.
    scroll: f32,
    /// Set when this browser goes, to stop the thread reading its folder.
    stop: Arc<AtomicBool>,
}

/// However a browser goes (Close, opening a patch, opening it again, or the
/// app replacing its patch), its reader stops. *Why here:* no way of closing
/// it can forget to.
impl Drop for PatchBrowser {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl PatchBrowser {
    /// An empty browser over `folder`. Rows come in with [`Message::Found`].
    pub fn new(open: u64, folder: PathBuf) -> Self {
        Self {
            open,
            folder,
            rows: Vec::new(),
            tags: BTreeSet::new(),
            #[cfg(test)]
            refilters: 0,
            filter: Filter::default(),
            shown: Vec::new(),
            highlight: 0,
            scroll: 0.0,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Opens a browser over `folder` and the task that reads it on a
    /// thread, one message per file.
    pub fn open(open: u64, folder: PathBuf) -> (Self, Task<Message>) {
        let browser = Self::new(open, folder.clone());
        let reader = read(open, folder, browser.stop.clone());
        (browser, Task::batch([reader, iced::widget::operation::focus(SEARCH)]))
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
    pub fn tags(&self) -> impl Iterator<Item = &str> {
        self.tags.iter().map(String::as_str)
    }

    /// Handles a message, and returns what the app should do and a task
    /// that keeps the highlighted row in view.
    pub fn update(&mut self, message: Message) -> (Outcome, Task<Message>) {
        let follow = matches!(message, Message::Step(_));
        let outcome = match message {
            Message::Found { open, rows } => {
                if open == self.open {
                    self.tags.extend(
                        rows.iter()
                            .flat_map(|row| row.prepared.iter())
                            .flat_map(|p| p.tags())
                            .cloned(),
                    );
                    self.rows.extend(rows);
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
        #[cfg(test)]
        {
            self.refilters += 1;
        }
        let empty = self.filter == Filter::default();
        let matcher = self.filter.matcher();
        let highlighted = self.highlighted().map(|row| row.path.clone());
        let mut shown: Vec<usize> = (0..self.rows.len())
            .filter(|&i| match &self.rows[i].prepared {
                Some(prepared) => matcher.matches(prepared),
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
        let chips = self.tags().map(|tag| {
            let tag = tag.to_string();
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
            let cell = container(
                column![
                    text(r.name()).size(style::CONTROL),
                    text(&r.details).size(style::SMALL).color(style::QUIET),
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
