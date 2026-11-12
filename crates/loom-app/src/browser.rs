//! The module browser: search the registry and add a module where the user
//! opened the browser. Spec: `docs/design/module-browser.md`. It also lists
//! the sub-patches in the library folder, inserted as a copy, or linked
//! from the item's menu (`docs/design/sub-patches.md`, Library).

use crate::style;
use iced::widget::{column, container, mouse_area, row, scrollable, space, text, text_input};
use iced::{Element, Length, Task, Theme};
use loom_canvas::GRID;
use loom_canvas::iced_convert::ToIced as _;
use loom_core::{Family, ModuleDescriptor};
use loom_ui_core::{Point, Rect, UiId};
use serde::Serialize;
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
    /// A row was right-clicked, or a screen reader asked for its menu
    /// (`by_key`): a library item opens its menu. Opened by a screen reader,
    /// the menu's first item is highlighted, as a menu opened by a key is.
    Menu { row: usize, by_key: bool },
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
    /// The roles the file declares, as role kinds, so a slot's Swap list
    /// can offer it (sub-patches.md, Slots).
    pub roles: Vec<String>,
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

/// What the browser shows, as plain data (ADR-0021). The browser owns it and
/// keeps it up to date in [`Browser::update`], so its view, the
/// accessibility tree, and the UI snapshot read the same thing, and it
/// changes only when what is shown does.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Model {
    /// The search text.
    pub query: String,
    /// Every result, best first: the modules, then the library items.
    pub rows: Vec<BrowserRow>,
    /// The highlighted row, or `None` while no row matches.
    pub highlight: Option<usize>,
    /// What the area under the list says about the highlighted row.
    pub about: Option<About>,
    /// The row whose menu is open, if one is.
    pub menu: Option<usize>,
    /// The item of that menu the keys are on, if a key or a screen reader
    /// has moved there.
    pub menu_highlight: Option<usize>,
    /// The line shown in place of the list while no row matches.
    pub empty: Option<&'static str>,
}

/// One row of the list, as it is shown.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BrowserRow {
    /// Its UI ID: `browser/module/<type id>` or `browser/library/<path>`.
    #[serde(serialize_with = "crate::snapshot::display")]
    pub id: UiId,
    /// The module's or the sub-patch's name.
    pub name: String,
    /// The words beside its swatch: the family's label, or "Sub-patch".
    pub mark: &'static str,
    /// The family whose colour the swatch shows.
    #[serde(skip)]
    pub family: Family,
    /// True for a library item, which has a menu.
    pub library: bool,
    /// What a screen reader says about the row, whichever row the keys are
    /// on: a module's one-sentence description, or a library item's path.
    pub description: String,
    /// The UI ID as text, which names the row's container in the window:
    /// worked out once with the row, not on every frame.
    #[serde(skip)]
    pub key: String,
}

/// What the area under the list says about the highlighted row: a module's
/// description and type ID, or a library item's sentence and path.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct About {
    /// The sentence.
    pub text: String,
    /// The line under it, quieter.
    pub detail: String,
}

/// The items of a library row's menu, in order. Only one for now.
pub(crate) const MENU_ITEMS: [&str; 1] = [INSERT_LINKED];

/// The UI ID of the browser.
pub(crate) fn browser_id() -> UiId {
    UiId::panel("browser")
}

/// The UI ID of a row: a module by its type ID, a library item by its path
/// in the library folder, each one key (ui-layer.md, section 4).
fn row_id(entry: Entry, library: &[LibraryItem]) -> UiId {
    match entry {
        Entry::Module(d) => browser_id().with("module").with(d.id),
        Entry::Library(i) => browser_id().with("library").with(library[i].library.as_str()),
    }
}

/// The UI ID of a library row's menu, under its row.
pub(crate) fn menu_id(row: &UiId) -> UiId {
    row.clone().with("menu")
}

/// The UI ID of the menu's Insert linked item.
pub(crate) fn insert_linked_id(row: &UiId) -> UiId {
    menu_id(row).with("insert-linked")
}

/// An open browser.
pub struct Browser {
    modules: Vec<&'static ModuleDescriptor>,
    /// The library folder's sub-patches, once read.
    library: Vec<LibraryItem>,
    /// What each row of the model is, by the same index.
    results: Vec<Entry>,
    /// What is shown.
    model: Model,
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
    pub fn open(modules: Vec<&'static ModuleDescriptor>, world: Point, at: Point, area: Rect) -> Self {
        let mut browser = Self {
            modules,
            library: Vec::new(),
            results: Vec::new(),
            model: Model {
                query: String::new(),
                rows: Vec::new(),
                highlight: None,
                about: None,
                menu: None,
                menu_highlight: None,
                empty: None,
            },
            world: snap(world),
            at: inside(at, area),
            scroll: 0.0,
        };
        browser.search();
        browser.set_highlight(0);
        browser
    }

    /// What the browser shows.
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// The highlighted row's index, 0 while no row matches.
    fn highlight(&self) -> usize {
        self.model.highlight.unwrap_or(0)
    }

    /// Highlights `row`, within the list, and says what it is under it.
    fn set_highlight(&mut self, row: usize) {
        self.model.highlight = (!self.results.is_empty()).then(|| row.min(self.results.len() - 1));
        self.model.about = self.model.highlight.map(|at| about(self.results[at], &self.library));
    }

    /// Opens the menu of `row`, or closes it with `None`.
    fn set_menu(&mut self, row: Option<usize>, highlight: Option<usize>) {
        self.model.menu = row;
        self.model.menu_highlight = row.and(highlight);
    }

    /// Handles a message, and returns what the app should do and a task
    /// that keeps the highlighted row in view.
    pub fn update(&mut self, message: Message) -> (Outcome, Task<Message>) {
        // Typing and keys may move the highlight out of view. A pointer is
        // already on a visible row, and must not fight the scroll wheel.
        let follow = matches!(message, Message::Query(_) | Message::Key(_));
        let outcome = match message {
            Message::Query(query) => {
                self.model.query = query;
                self.search();
                self.set_highlight(0);
                self.set_menu(None, None);
                Outcome::Stay
            }
            Message::Library(items) => {
                self.library = items;
                self.search();
                self.set_highlight(self.highlight());
                Outcome::Stay
            }
            Message::Hover(row) => {
                if row < self.results.len() && self.model.highlight != Some(row) {
                    self.set_highlight(row);
                }
                Outcome::Stay
            }
            Message::Pick(row) => {
                self.set_highlight(row);
                self.add(false)
            }
            Message::Menu { row, by_key } => {
                if matches!(self.results.get(row), Some(Entry::Library(_))) {
                    self.set_highlight(row);
                    self.set_menu(Some(row), by_key.then_some(0));
                }
                Outcome::Stay
            }
            Message::InsertLinked(row) => {
                self.set_highlight(row);
                self.set_menu(None, None);
                self.add(true)
            }
            // While a library item's menu is open, Enter picks the item the
            // keys are on, as in every menu (module-browser.md), and does
            // nothing before a key has moved there.
            Message::Submit => match (self.model.menu, self.model.menu_highlight) {
                (Some(row), Some(_)) => {
                    self.set_highlight(row);
                    self.set_menu(None, None);
                    self.add(true)
                }
                (Some(_), None) => Outcome::Stay,
                (None, _) => self.add(false),
            },
            Message::Scrolled(offset) => {
                self.scroll = offset;
                Outcome::Stay
            }
            // Escape closes a menu first, then the browser.
            Message::Key(Nav::Escape) if self.model.menu.is_some() => {
                self.set_menu(None, None);
                Outcome::Stay
            }
            Message::Key(Nav::Escape) | Message::Close => Outcome::Close,
            // Up and Down move in an open menu, as in every menu.
            Message::Key(nav @ (Nav::Up | Nav::Down)) if self.model.menu.is_some() => {
                let last = MENU_ITEMS.len() - 1;
                let at = match (self.model.menu_highlight, nav) {
                    (None, Nav::Down) => 0,
                    (None, _) => last,
                    (Some(at), Nav::Down) => {
                        if at >= last {
                            0
                        } else {
                            at + 1
                        }
                    }
                    (Some(at), _) => {
                        if at == 0 {
                            last
                        } else {
                            at - 1
                        }
                    }
                };
                self.model.menu_highlight = Some(at);
                Outcome::Stay
            }
            Message::Key(nav) => {
                self.step(nav);
                self.set_menu(None, None);
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

    /// The library items the list shows, each with its row.
    #[cfg(test)]
    pub fn library_rows(&self) -> Vec<(usize, &LibraryItem)> {
        self.results
            .iter()
            .enumerate()
            .filter_map(|(row, entry)| match entry {
                Entry::Library(i) => self.library.get(*i).map(|item| (row, item)),
                Entry::Module(_) => None,
            })
            .collect()
    }

    /// The highlighted module, if the highlight is on one.
    pub fn highlighted(&self) -> Option<&'static ModuleDescriptor> {
        match self.model.highlight.and_then(|at| self.results.get(at)) {
            Some(Entry::Module(descriptor)) => Some(descriptor),
            _ => None,
        }
    }

    /// The highlighted library item, if the highlight is on one.
    fn highlighted_item(&self) -> Option<&LibraryItem> {
        match self.model.highlight.and_then(|at| self.results.get(at)) {
            Some(Entry::Library(i)) => self.library.get(*i),
            _ => None,
        }
    }

    /// Fills the list from the query: the modules, then the library items.
    /// The model's rows are built here, once per search, never per frame.
    fn search(&mut self) {
        self.results = results(&self.modules, &self.library, &self.model.query);
        self.model.rows = rows(&self.results, &self.library);
        self.model.empty = self.results.is_empty().then_some("No modules match.");
        // The menu belongs to a row that may be gone.
        self.set_menu(None, None);
    }

    /// The model worked out from scratch from the browser's own state: its
    /// modules, its library, and the query, highlight, and menu the user
    /// chose. A test compares it with the kept model after every message,
    /// so an update that forgets to change the model fails.
    #[cfg(test)]
    pub(crate) fn fresh_model(&self) -> Model {
        let results = results(&self.modules, &self.library, &self.model.query);
        let highlight = self.model.highlight.filter(|&at| at < results.len());
        Model {
            query: self.model.query.clone(),
            rows: rows(&results, &self.library),
            highlight,
            about: highlight.map(|at| about(results[at], &self.library)),
            menu: self.model.menu,
            menu_highlight: self.model.menu_highlight,
            empty: results.is_empty().then_some("No modules match."),
        }
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
        let at = self.highlight();
        self.set_highlight(match nav {
            Nav::Up => at.saturating_sub(1),
            Nav::Down => (at + 1).min(last),
            Nav::PageUp => at.saturating_sub(ROWS),
            Nav::PageDown => (at + ROWS).min(last),
            Nav::Escape => at,
        });
    }

    /// Scrolls the list just enough to show the highlighted row.
    fn reveal(&mut self) -> Task<Message> {
        let top = self.highlight() as f32 * ROW_HEIGHT;
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
    /// when one is open. The app lays it over the window. Drawn from the
    /// model alone; each part is named by its UI ID, so the window says
    /// where it is (`places.rs`).
    pub fn view(&self) -> Element<'_, Message> {
        let model = &self.model;
        let input = container(
            text_input("Search modules", &model.query)
                .id(crate::places::field_widget_id(&search_id()))
                .on_input(Message::Query)
                .on_submit(Message::Submit)
                .size(style::CONTROL)
                .padding(6),
        )
        .id(crate::places::widget_id(&search_id()));

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
        let rows = model.rows.iter().enumerate().map(|(i, r)| {
            let highlighted = model.highlight == Some(i);
            // The family's colour and name, so colour is never the only cue
            // (ADR-0029). A sub-patch shows the colour of a sub-patch tile.
            let line = row![
                text(r.name.as_str()).size(style::CONTROL),
                swatch(crate::scene::family_colour(r.family).to_iced()),
                text(r.mark).size(style::SMALL).color(style::QUIET),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);
            let cell = container(line)
                .padding([0, 8])
                .height(ROW_HEIGHT)
                .width(Length::Fill)
                .align_y(iced::alignment::Vertical::Center)
                .style(move |theme: &Theme| row_style(theme, highlighted));
            container(
                mouse_area(cell)
                    .on_move(move |_| Message::Hover(i))
                    .on_press(Message::Pick(i))
                    .on_right_press(Message::Menu { row: i, by_key: false }),
            )
            .id(iced::widget::Id::from(r.key.clone()))
            .into()
        });
        let list: Element<'_, Message> = match model.empty {
            Some(line) => container(text(line).size(style::CONTROL).color(style::QUIET))
                .padding(8)
                .height(LIST_HEIGHT)
                .into(),
            // The scrollbar takes its own space, so it covers no row text.
            None => container(
                scrollable(column(rows))
                    .id(LIST)
                    .spacing(4)
                    .height(LIST_HEIGHT)
                    .on_scroll(|viewport| Message::Scrolled(viewport.absolute_offset().y)),
            )
            .id(crate::places::widget_id(&list_id()))
            .into(),
        };
        // The ID goes with the description, so no row is cut by a column
        // or covered by the scrollbar (module-browser.md).
        let about = match &model.about {
            Some(about) => column![
                text(about.text.as_str()).size(style::BODY),
                text(about.detail.as_str()).size(style::SMALL).color(style::QUIET),
            ]
            .spacing(2),
            None => column![],
        };
        let panel = column![input, list, container(about).height(DESCRIPTION_HEIGHT)]
            .spacing(SPACING)
            .padding(PADDING)
            .width(WIDTH);
        let panel = container(panel)
            .style(panel_style)
            .id(crate::places::widget_id(&browser_id()));
        let panel: Element<'_, Message> = iced::widget::pin(iced::widget::opaque(panel))
            .position(self.at.to_iced())
            .into();
        let Some(open) = model.menu else {
            return panel;
        };
        let row = &model.rows[open].id;
        // The menu hangs under its row's right end.
        let row_top = self.at.y + PADDING + INPUT_HEIGHT + SPACING + open as f32 * ROW_HEIGHT - self.scroll;
        let highlighted = model.menu_highlight == Some(0);
        let menu = container(column![
            container(
                style::button(text(INSERT_LINKED).size(style::CONTROL))
                    .style(style::outlined(style::secondary, highlighted))
                    .on_press(Message::InsertLinked(open))
                    .width(MENU_WIDTH)
            )
            .id(crate::places::widget_id(&insert_linked_id(row)))
        ])
        .padding(4)
        .style(container::rounded_box)
        .id(crate::places::widget_id(&menu_id(row)));
        let menu = iced::widget::pin(iced::widget::opaque(menu))
            .position(Point::new(self.at.x + WIDTH - MENU_WIDTH - PADDING, row_top + ROW_HEIGHT).to_iced());
        iced::widget::stack![panel, menu].into()
    }
}

/// The UI ID of the browser's search field.
pub(crate) fn search_id() -> UiId {
    browser_id().with("search")
}

/// The UI ID of the browser's list.
pub(crate) fn list_id() -> UiId {
    browser_id().with("list")
}

/// The UI ID of the text under the list.
pub(crate) fn about_id() -> UiId {
    browser_id().with("about")
}

/// What the list shows for `query`: the modules, then the library items.
fn results(modules: &[&'static ModuleDescriptor], library: &[LibraryItem], query: &str) -> Vec<Entry> {
    search(modules, query)
        .into_iter()
        .map(Entry::Module)
        .chain(search_library(library, query).into_iter().map(Entry::Library))
        .collect()
}

/// The rows the model shows for `results`, each with its UI ID, made
/// unique, its text worked out once, and its description.
fn rows(results: &[Entry], library: &[LibraryItem]) -> Vec<BrowserRow> {
    let mut ids = crate::a11y::items::UniqueIds::default();
    results
        .iter()
        .map(|&entry| {
            let id = ids.take(row_id(entry, library));
            let key = id.to_string();
            match entry {
                Entry::Module(d) => BrowserRow {
                    id,
                    key,
                    name: d.name.to_string(),
                    mark: d.family.label(),
                    family: d.family,
                    library: false,
                    description: d.description.to_string(),
                },
                Entry::Library(i) => BrowserRow {
                    id,
                    key,
                    name: library[i].name.clone(),
                    mark: LIBRARY_MARK,
                    family: Family::Interface,
                    library: true,
                    description: library[i].library.clone(),
                },
            }
        })
        .collect()
}

/// What the area under the list says about `entry`.
fn about(entry: Entry, library: &[LibraryItem]) -> About {
    match entry {
        Entry::Module(d) => About {
            text: d.description.to_string(),
            detail: d.id.to_string(),
        },
        Entry::Library(i) => About {
            text: "A sub-patch from your library, inserted as a copy. Right-click it to insert it linked.".to_string(),
            detail: library[i].library.clone(),
        },
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
            let roles = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| loom_patch::Patch::from_ron(&text).ok())
                .map(|patch| patch.roles)
                .unwrap_or_default();
            Some(LibraryItem {
                library: library.path_of(&path)?,
                name: row.name(),
                path,
                roles,
            })
        })
        .collect();
    items.sort_by(|a, b| a.library.cmp(&b.library));
    items
}

/// Reads the library folder for the list as background work, so the
/// window never waits for the disk; the items arrive as one
/// [`Message::Library`]. Under test the work runs inline (ui-layer.md,
/// section 2, rule 4).
pub fn read_library(folder: PathBuf) -> loom_ui::Effects<crate::Message> {
    loom_ui::Effects::work(move || crate::Message::Browser(Message::Library(library_items(&folder))))
}

/// Snaps a world point to the grid, for a new module's corner.
pub fn snap(world: Point) -> Point {
    Point::new((world.x / GRID).round() * GRID, (world.y / GRID).round() * GRID)
}

/// Moves the panel's corner so the whole panel fits inside `area`, where it
/// can. A small area keeps the corner at its top-left.
fn inside(at: Point, area: Rect) -> Point {
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

pub(crate) mod access;

#[cfg(test)]
mod tests;
