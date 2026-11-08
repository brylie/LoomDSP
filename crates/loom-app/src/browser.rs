//! The module browser: search the registry and add a module where the user
//! opened the browser. Spec: `docs/design/module-browser.md`. It also lists
//! the sub-patches in the library folder, inserted as a copy, or linked
//! from the item's menu (`docs/design/sub-patches.md`, Library).

use crate::style;
use iced::widget::{column, container, mouse_area, row, scrollable, space, text, text_input};
use iced::{Element, Length, Point, Rectangle, Task, Theme};
use loom_canvas::GRID;
use loom_core::ModuleDescriptor;
use std::cmp::Reverse;
use std::path::{Path, PathBuf};

/// Width of the browser panel.
pub const WIDTH: f32 = 340.0;
/// Height of one result row. Fixed, so the list can scroll to a row.
const ROW_HEIGHT: f32 = 26.0;
/// Rows shown before the list scrolls.
const ROWS: usize = 10;
/// Height of the area for the highlighted module's description.
const DESCRIPTION_HEIGHT: f32 = 64.0;
const PADDING: f32 = 8.0;
const SPACING: f32 = 6.0;
const INPUT_HEIGHT: f32 = 30.0;
/// Height of the whole panel, for keeping it inside the canvas.
pub const HEIGHT: f32 = 2.0 * PADDING + INPUT_HEIGHT + LIST_HEIGHT + DESCRIPTION_HEIGHT + 2.0 * SPACING;
const LIST_HEIGHT: f32 = ROWS as f32 * ROW_HEIGHT;
/// Width of a library item's menu.
const MENU_WIDTH: f32 = 140.0;

pub(crate) const INPUT: &str = "module-browser-search";
const LIST: &str = "module-browser-list";

/// The mark a library item's row shows in place of a module's family.
pub(crate) const LIBRARY_MARK: &str = "Sub-patch";
/// The one item on a library item's menu.
pub(crate) const INSERT_LINKED: &str = "Insert linked";

/// What the browser asks of the app.
#[derive(Debug, Clone)]
pub enum Message {
    /// The query changed.
    Query(String),
    /// The pointer moved over a row.
    Hover(usize),
    /// A row was clicked.
    Pick(usize),
    /// A row was right-clicked: a library item opens its menu.
    Menu(usize),
    /// Insert linked, from a library item's menu.
    InsertLinked(usize),
    /// The library folder was read.
    Library(Vec<LibraryItem>),
    /// Enter in the search field.
    Submit,
    /// The list scrolled.
    Scrolled(f32),
    /// A key the browser handles itself: Up, Down, Page Up, Page Down, or
    /// Escape.
    Key(Nav),
    /// A click outside the browser.
    Close,
}

/// Keys that move the highlight or close the browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    PageUp,
    PageDown,
    Escape,
}

/// A patch file in the library folder, as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryItem {
    /// The file.
    pub path: PathBuf,
    /// Its path relative to the library folder, with `/` between its
    /// parts: what a linked definition's `library` field writes.
    pub library: String,
    /// The name shown: the patch's name, or its file name when it has none
    /// or does not read.
    pub name: String,
}

/// What the app should do after a browser message.
#[derive(Debug)]
pub enum Outcome {
    /// Keep the browser open.
    Stay,
    /// Close it and add nothing.
    Close,
    /// Close it and add this module at this world point.
    Add(&'static ModuleDescriptor, Point),
    /// Close it and insert this library item as a sub-patch at this world
    /// point, as a copy or linked.
    Insert {
        item: LibraryItem,
        linked: bool,
        world: Point,
    },
}

/// One row of the list.
#[derive(Debug, Clone, Copy)]
enum Entry {
    Module(&'static ModuleDescriptor),
    /// A library item, by its index in the browser's list.
    Library(usize),
}

/// An open browser.
pub struct Browser {
    modules: Vec<&'static ModuleDescriptor>,
    /// The library folder's sub-patches, once read.
    library: Vec<LibraryItem>,
    query: String,
    results: Vec<Entry>,
    highlight: usize,
    /// The row whose menu is open, if one is.
    menu: Option<usize>,
    /// Where the new module goes: its top-left corner, snapped.
    world: Point,
    /// The panel's top-left corner, in window coordinates.
    at: Point,
    /// The list's scroll offset, so a keyboard move can keep the highlight
    /// in view.
    scroll: f32,
}

impl Browser {
    /// Opens a browser over `modules`, adding at `world`. The panel opens at
    /// `at`, moved as needed to stay inside `area` (window coordinates).
    /// The library's items come with [`Message::Library`].
    pub fn open(modules: Vec<&'static ModuleDescriptor>, world: Point, at: Point, area: Rectangle) -> Self {
        let results = search(&modules, "").into_iter().map(Entry::Module).collect();
        Self {
            modules,
            library: Vec::new(),
            query: String::new(),
            results,
            highlight: 0,
            menu: None,
            world: snap(world),
            at: inside(at, area),
            scroll: 0.0,
        }
    }

    /// Handles a message, and returns what the app should do and a task
    /// that keeps the highlighted row in view.
    pub fn update(&mut self, message: Message) -> (Outcome, Task<Message>) {
        // Typing and keys may move the highlight out of view. A pointer is
        // already on a visible row, and must not fight the scroll wheel.
        let follow = matches!(message, Message::Query(_) | Message::Key(_));
        let outcome = match message {
            Message::Query(query) => {
                self.query = query;
                self.search();
                self.highlight = 0;
                self.menu = None;
                Outcome::Stay
            }
            Message::Library(items) => {
                self.library = items;
                self.search();
                self.highlight = self.highlight.min(self.results.len().saturating_sub(1));
                Outcome::Stay
            }
            Message::Hover(row) => {
                if row < self.results.len() {
                    self.highlight = row;
                }
                Outcome::Stay
            }
            Message::Pick(row) => {
                self.highlight = row;
                self.add(false)
            }
            Message::Menu(row) => {
                if matches!(self.results.get(row), Some(Entry::Library(_))) {
                    self.highlight = row;
                    self.menu = Some(row);
                }
                Outcome::Stay
            }
            Message::InsertLinked(row) => {
                self.highlight = row;
                self.menu = None;
                self.add(true)
            }
            Message::Submit => self.add(false),
            Message::Scrolled(offset) => {
                self.scroll = offset;
                Outcome::Stay
            }
            Message::Key(Nav::Escape) | Message::Close => Outcome::Close,
            Message::Key(nav) => {
                self.step(nav);
                self.menu = None;
                Outcome::Stay
            }
        };
        let task = if follow { self.reveal() } else { Task::none() };
        (outcome, task)
    }

    /// The modules the list shows, best first, with the library items
    /// left out.
    #[cfg(test)]
    pub fn results(&self) -> Vec<&'static ModuleDescriptor> {
        self.results
            .iter()
            .filter_map(|entry| match entry {
                Entry::Module(descriptor) => Some(*descriptor),
                Entry::Library(_) => None,
            })
            .collect()
    }

    /// The library items the list shows, in list order.
    #[cfg(test)]
    pub fn library_results(&self) -> Vec<&LibraryItem> {
        self.results
            .iter()
            .filter_map(|entry| match entry {
                Entry::Library(i) => self.library.get(*i),
                Entry::Module(_) => None,
            })
            .collect()
    }

    /// The highlighted module, if the highlight is on one.
    pub fn highlighted(&self) -> Option<&'static ModuleDescriptor> {
        match self.results.get(self.highlight) {
            Some(Entry::Module(descriptor)) => Some(descriptor),
            _ => None,
        }
    }

    /// The highlighted library item, if the highlight is on one.
    fn highlighted_item(&self) -> Option<&LibraryItem> {
        match self.results.get(self.highlight) {
            Some(Entry::Library(i)) => self.library.get(*i),
            _ => None,
        }
    }

    /// Fills the list from the query: the modules, then the library items.
    fn search(&mut self) {
        self.results = search(&self.modules, &self.query)
            .into_iter()
            .map(Entry::Module)
            .chain(
                search_library(&self.library, &self.query)
                    .into_iter()
                    .map(Entry::Library),
            )
            .collect();
    }

    fn add(&self, linked: bool) -> Outcome {
        if let Some(descriptor) = self.highlighted() {
            return Outcome::Add(descriptor, self.world);
        }
        match self.highlighted_item() {
            Some(item) => Outcome::Insert {
                item: item.clone(),
                linked,
                world: self.world,
            },
            None => Outcome::Stay,
        }
    }

    /// Moves the highlight. It stops at the ends.
    fn step(&mut self, nav: Nav) {
        let Some(last) = self.results.len().checked_sub(1) else {
            return;
        };
        self.highlight = match nav {
            Nav::Up => self.highlight.saturating_sub(1),
            Nav::Down => (self.highlight + 1).min(last),
            Nav::PageUp => self.highlight.saturating_sub(ROWS),
            Nav::PageDown => (self.highlight + ROWS).min(last),
            Nav::Escape => self.highlight,
        };
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
        iced::widget::operation::scroll_to(LIST, scrollable::AbsoluteOffset { x: 0.0, y: offset })
    }

    /// The panel, placed at its corner, with a library item's menu over it
    /// when one is open. The app lays it over the window.
    pub fn view(&self) -> Element<'_, Message> {
        let input = text_input("Search modules", &self.query)
            .id(INPUT)
            .on_input(Message::Query)
            .on_submit(Message::Submit)
            .size(style::CONTROL)
            .padding(6);

        let swatch = |colour: iced::Color| {
            container(space().width(8).height(8)).style(move |_: &Theme| container::Style {
                background: Some(colour.into()),
                border: iced::Border {
                    radius: 2.0.into(),
                    ..iced::Border::default()
                },
                ..container::Style::default()
            })
        };
        let rows = self.results.iter().enumerate().map(|(i, entry)| {
            let highlighted = i == self.highlight;
            let line = match entry {
                // The family's colour and name, so colour is never the only
                // cue (ADR-0029).
                Entry::Module(d) => row![
                    text(d.name).size(style::CONTROL),
                    swatch(crate::scene::family_colour(d.family)),
                    text(d.family.label()).size(style::SMALL).color(style::QUIET),
                ],
                // A sub-patch, in the colour of a sub-patch tile.
                Entry::Library(item) => row![
                    text(self.library[*item].name.clone()).size(style::CONTROL),
                    swatch(crate::scene::family_colour(loom_core::Family::Interface)),
                    text(LIBRARY_MARK).size(style::SMALL).color(style::QUIET),
                ],
            }
            .spacing(8)
            .align_y(iced::Alignment::Center);
            let cell = container(line)
                .padding([0, 8])
                .height(ROW_HEIGHT)
                .width(Length::Fill)
                .align_y(iced::alignment::Vertical::Center)
                .style(move |theme: &Theme| row_style(theme, highlighted));
            mouse_area(cell)
                .on_move(move |_| Message::Hover(i))
                .on_press(Message::Pick(i))
                .on_right_press(Message::Menu(i))
                .into()
        });
        let list: Element<'_, Message> = if self.results.is_empty() {
            container(text("No modules match.").size(style::CONTROL).color(style::QUIET))
                .padding(8)
                .height(LIST_HEIGHT)
                .into()
        } else {
            // The scrollbar takes its own space, so it covers no row text.
            scrollable(column(rows))
                .id(LIST)
                .spacing(4)
                .height(LIST_HEIGHT)
                .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y))
                .into()
        };
        // The ID goes with the description, so no row is cut by a column
        // or covered by the scrollbar (module-browser.md).
        let about = match self.results.get(self.highlight) {
            Some(Entry::Module(d)) => column![
                text(d.description).size(style::BODY),
                text(d.id).size(style::SMALL).color(style::QUIET),
            ]
            .spacing(2),
            Some(Entry::Library(item)) => column![
                text("A sub-patch from your library, inserted as a copy. Right-click it to insert it linked.")
                    .size(style::BODY),
                text(self.library[*item].library.clone())
                    .size(style::SMALL)
                    .color(style::QUIET),
            ]
            .spacing(2),
            None => column![],
        };
        let panel = column![input, list, container(about).height(DESCRIPTION_HEIGHT)]
            .spacing(SPACING)
            .padding(PADDING)
            .width(WIDTH);
        let panel = container(panel).style(panel_style);
        let panel: Element<'_, Message> = iced::widget::pin(iced::widget::opaque(panel)).position(self.at).into();
        let Some(open) = self.menu else {
            return panel;
        };
        // The menu hangs under its row's right end.
        let row_top = self.at.y + PADDING + INPUT_HEIGHT + SPACING + open as f32 * ROW_HEIGHT - self.scroll;
        let menu = container(column![
            style::button(text(INSERT_LINKED).size(style::CONTROL))
                .on_press(Message::InsertLinked(open))
                .width(MENU_WIDTH)
        ])
        .padding(4)
        .style(container::rounded_box);
        let menu = iced::widget::pin(iced::widget::opaque(menu)).position(Point::new(
            self.at.x + WIDTH - MENU_WIDTH - PADDING,
            row_top + ROW_HEIGHT,
        ));
        iced::widget::stack![panel, menu].into()
    }
}

/// The modules that match a query, best first.
///
/// A module matches when every word of the query is in its name, ID, or
/// category, or family, ignoring case. Names that start with the query come
/// first, then names with a word that starts with it, then other matches.
/// Ties keep the order of an empty search: by family, then by name.
pub fn search(modules: &[&'static ModuleDescriptor], query: &str) -> Vec<&'static ModuleDescriptor> {
    let query = query.trim().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut found: Vec<&'static ModuleDescriptor> = modules
        .iter()
        .copied()
        .filter(|d| {
            let text = format!("{} {} {} {}", d.name, d.id, d.category, d.family.label()).to_lowercase();
            words.iter().all(|w| text.contains(w))
        })
        .collect();
    found.sort_by_key(|d| (d.family, d.name, d.id));
    if !query.is_empty() {
        // A stable sort, so ties keep the order above.
        found.sort_by_key(|d| Reverse(rank(d.name, &query)));
    }
    found
}

/// The library items that match a query, as indices into `items`, best
/// first: every word of the query is in the name, the library path, or
/// the mark `Sub-patch`, ignoring case, ranked as modules are. Ties keep
/// the folder's order, by path.
pub fn search_library(items: &[LibraryItem], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut found: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            let text = format!("{} {} {LIBRARY_MARK}", item.name, item.library).to_lowercase();
            words.iter().all(|w| text.contains(w))
        })
        .map(|(i, _)| i)
        .collect();
    found.sort_by_key(|&i| (items[i].library.clone(), i));
    if !query.is_empty() {
        found.sort_by_key(|&i| Reverse(rank(&items[i].name, &query)));
    }
    found
}

/// How well a name fits the query: 2 if it starts with it, 1 if one of its
/// words does, 0 otherwise.
fn rank(name: &str, query: &str) -> u8 {
    let name = name.to_lowercase();
    if name.starts_with(query) {
        2
    } else if name.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(query)) {
        1
    } else {
        0
    }
}

/// The library folder's patch files as items, by path. A file that does
/// not read is listed by its file name: inserting it says why.
pub fn library_items(folder: &Path) -> Vec<LibraryItem> {
    let library = crate::patch_file::FolderLibrary {
        root: folder.to_path_buf(),
    };
    let stop = std::sync::atomic::AtomicBool::new(false);
    let mut items: Vec<LibraryItem> = crate::patches::patch_files(folder, &stop)
        .into_iter()
        .filter_map(|path| {
            let row = crate::patches::Row::read(&path);
            Some(LibraryItem {
                library: library.path_of(&path)?,
                name: row.name(),
                path,
            })
        })
        .collect();
    items.sort_by(|a, b| a.library.cmp(&b.library));
    items
}

/// Reads the library folder for the list, on a thread, so the window never
/// waits for the disk; the items arrive as one [`Message::Library`].
#[cfg(not(test))]
pub fn read_library(folder: PathBuf) -> Task<Message> {
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = sender.send(library_items(&folder));
    });
    Task::perform(async move { receiver.await.unwrap_or_default() }, Message::Library)
}

/// Reads the library folder at once: the headless test harness runs a task
/// to its end and cannot wait for a thread.
#[cfg(test)]
pub fn read_library(folder: PathBuf) -> Task<Message> {
    Task::done(Message::Library(library_items(&folder)))
}

/// Snaps a world point to the grid, for a new module's corner.
pub fn snap(world: Point) -> Point {
    Point::new((world.x / GRID).round() * GRID, (world.y / GRID).round() * GRID)
}

/// Moves the panel's corner so the whole panel fits inside `area`, where it
/// can. A small area keeps the corner at its top-left.
fn inside(at: Point, area: Rectangle) -> Point {
    let x = at.x.min(area.x + area.width - WIDTH).max(area.x);
    let y = at.y.min(area.y + area.height - HEIGHT).max(area.y);
    Point::new(x, y)
}

fn panel_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(palette.background.base.color.into()),
        border: iced::Border {
            color: palette.background.strong.color,
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: iced::Shadow {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..container::Style::default()
    }
}

/// The highlighted row: a lifted grey with an orange outline, like a
/// selected card, so its quieter text stays readable on it.
pub(crate) fn row_style(theme: &Theme, highlighted: bool) -> container::Style {
    if !highlighted {
        return container::Style::default();
    }
    let palette = theme.extended_palette();
    container::Style {
        background: Some(palette.background.weak.color.into()),
        border: iced::Border {
            color: palette.primary.base.color,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    }
}

#[cfg(test)]
mod tests;
