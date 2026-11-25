//! The module browser: search the registry and add a module where the user
//! opened the browser. Spec: `docs/design/module-browser.md`. It also lists
//! an empty slot for each role a part fills (`docs/design/sub-patches.md`,
//! Empty slots), and the sub-patches in the library folder, inserted as a
//! copy, or linked from the item's menu (Library).

use loom_canvas::BrowserPlacement;
pub(crate) use loom_canvas::snap;
use loom_core::{Family, ModuleDescriptor, RoleDescriptor};
use loom_ui::Effects;
use loom_ui_core::{Point, Rect, UiId};
use serde::Serialize;
use std::cmp::Reverse;
use std::path::{Path, PathBuf};

mod panel;
mod placement;

/// Empty search results and how to recover.
const NO_MATCHES: &str = "No modules match. Clear the search to see every module.";

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

/// The mark a library item's row shows in place of a module's family.
pub(crate) const LIBRARY_MARK: &str = "Sub-patch";
/// The mark a built-in abstraction's row shows in place of a module's
/// family.
pub(crate) const BUILTIN_MARK: &str = "Built in";
/// The mark a slot row shows in place of a module's family.
pub(crate) const SLOT_MARK: &str = "Slot";
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
    /// A press outside a library row's open menu closes only the menu.
    CloseMenu,
    /// A key the browser handles itself: Up, Down, Page Up, Page Down, or
    /// Escape.
    Key(Nav),
    /// A click outside the browser.
    Close,
}

/// Keys that move the highlight, open a library row's menu, or close the
/// browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    PageUp,
    PageDown,
    Escape,
    /// The menu key, or Shift+F10: opens the highlighted library row's
    /// menu on its first item, as a menu over the canvas opens from the
    /// keyboard (ui-layer.md, section 6, rule 8). On any other row it does
    /// nothing.
    Menu,
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
    /// Close it and add this module with this placement.
    Add(&'static ModuleDescriptor, BrowserPlacement),
    /// Close it and add an empty slot for this role with this placement.
    AddSlot(&'static RoleDescriptor, BrowserPlacement),
    /// Close it and insert this built-in abstraction as a copy.
    InsertBuiltin(&'static crate::builtin::Builtin, BrowserPlacement),
    /// Close it and insert this library item as a sub-patch, as a copy or linked.
    Insert {
        item: LibraryItem,
        linked: bool,
        placement: BrowserPlacement,
    },
}

/// One row of the list.
#[derive(Debug, Clone, Copy)]
enum Entry {
    /// A built-in abstraction, by its index in the browser's built-ins.
    Builtin(usize),
    Module(&'static ModuleDescriptor),
    /// An empty slot for a role.
    Role(&'static RoleDescriptor),
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
    /// Its UI ID: `browser/list/module/<type id>`, `browser/list/library/<path>`,
    /// and so on by kind.
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

/// The UI ID of a row, inside the list and the group of its kind: a module
/// by its type ID, a slot by its role's ID, a library item by its path in
/// the library folder, each one key (ui-layer.md, section 4).
fn row_id(entry: Entry, builtins: &[crate::builtin::Builtin], library: &[LibraryItem]) -> UiId {
    let list = list_id();
    match entry {
        Entry::Builtin(i) => list.with("builtin").with(builtins[i].stem),
        Entry::Module(d) => list.with("module").with(d.id),
        Entry::Role(r) => list.with("role").with(r.id),
        Entry::Library(i) => list.with("library").with(library[i].library.as_str()),
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
    /// Every registered role, for the slot rows.
    roles: Vec<&'static RoleDescriptor>,
    /// The built-in abstractions, listed first, with their descriptions.
    builtins: &'static [crate::builtin::Builtin],
    /// The library folder's sub-patches, once read.
    library: Vec<LibraryItem>,
    /// What each row of the model is, by the same index.
    results: Vec<Entry>,
    /// What is shown.
    model: Model,
    /// Placement captured at open. A point chosen by the pointer is snapped to the grid.
    placement: BrowserPlacement,
    /// The panel's top-left corner, in window coordinates.
    at: Point,
}

impl Browser {
    /// Opens a browser over `modules` with the placement the canvas supplied. The panel opens at
    /// `at`, moved as needed to stay inside `area` (window coordinates).
    /// The library's items come with [`Message::Library`].
    pub fn open(
        modules: Vec<&'static ModuleDescriptor>,
        placement: impl Into<BrowserPlacement>,
        at: Point,
        area: Rect,
    ) -> Self {
        let mut browser = Self {
            modules,
            roles: Vec::new(),
            builtins: &[],
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
            placement: match placement.into() {
                BrowserPlacement::Pointer(point) => BrowserPlacement::Pointer(snap(point)),
                toolbar => toolbar,
            },
            at: inside(at, area),
        };
        browser.search();
        browser.set_highlight(0);
        browser
    }

    /// Lists `builtins` first (sub-patches.md, Built-in abstractions).
    pub fn with_builtins(mut self, builtins: &'static [crate::builtin::Builtin]) -> Self {
        self.builtins = builtins;
        self.search();
        self.set_highlight(0);
        self
    }

    /// Lists an empty slot for each of `roles` that a module or a library
    /// item fills (sub-patches.md, Empty slots).
    pub fn with_roles(mut self, roles: Vec<&'static RoleDescriptor>) -> Self {
        self.roles = roles;
        self.search();
        self.set_highlight(0);
        self
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
        self.model.about = self
            .model
            .highlight
            .map(|at| about(self.results[at], self.builtins, &self.library));
    }

    /// Opens the menu of `row`, or closes it with `None`.
    fn set_menu(&mut self, row: Option<usize>, highlight: Option<usize>) {
        self.model.menu = row;
        self.model.menu_highlight = row.and(highlight);
    }

    /// Handles a message, and returns what the app should do and the effect
    /// that keeps the highlighted row in view.
    pub fn update(&mut self, message: Message) -> (Outcome, Effects<crate::Message>) {
        // Typing and keys may move the highlight out of view. A pointer is
        // already on a visible row, and must not fight the scroll wheel. A
        // row's menu opened by a screen reader may be on a row out of view:
        // it is shown first, so the menu has a row to hang under, which
        // would otherwise close it at once (ui-layer.md, section 5,
        // rule 13).
        let follow = matches!(
            message,
            Message::Query(_) | Message::Key(_) | Message::Menu { by_key: true, .. }
        );
        let outcome = match message {
            Message::Query(query) => {
                self.model.query = query;
                self.search();
                self.set_highlight(0);
                self.set_menu(None, None);
                Outcome::Stay
            }
            Message::Library(items) => {
                // The highlight stays on the same row, by its UI ID: a slot
                // row a library item fills can appear above it.
                let kept = self
                    .model
                    .highlight
                    .and_then(|at| self.model.rows.get(at))
                    .map(|r| r.id.clone());
                self.library = items;
                self.search();
                let at = kept
                    .and_then(|id| self.model.rows.iter().position(|r| r.id == id))
                    .unwrap_or(self.highlight());
                self.set_highlight(at);
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
            Message::CloseMenu => {
                self.set_menu(None, None);
                Outcome::Stay
            }
            // The menu key opens the highlighted row's menu, as a screen
            // reader's ShowContextMenu does, so the keyboard reaches Insert
            // linked too.
            Message::Key(Nav::Menu) => {
                if let Some(row) = self.model.highlight
                    && matches!(self.results.get(row), Some(Entry::Library(_)))
                {
                    self.set_menu(Some(row), Some(0));
                }
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
        let effects = if follow { self.reveal() } else { Effects::none() };
        (outcome, effects)
    }

    /// The modules the list shows, best first, with the library items
    /// left out.
    #[cfg(test)]
    pub fn results(&self) -> Vec<&'static ModuleDescriptor> {
        self.results
            .iter()
            .filter_map(|entry| match entry {
                Entry::Module(descriptor) => Some(*descriptor),
                Entry::Builtin(_) | Entry::Role(_) | Entry::Library(_) => None,
            })
            .collect()
    }

    /// The slot rows the list shows, each with its row and role.
    #[cfg(test)]
    pub fn slot_rows(&self) -> Vec<(usize, &'static RoleDescriptor)> {
        self.results
            .iter()
            .enumerate()
            .filter_map(|(row, entry)| match entry {
                Entry::Role(role) => Some((row, *role)),
                Entry::Builtin(_) | Entry::Module(_) | Entry::Library(_) => None,
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
                Entry::Builtin(_) | Entry::Module(_) | Entry::Role(_) => None,
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
        self.results = results(
            &self.modules,
            &self.roles,
            self.builtins,
            &self.library,
            &self.model.query,
        );
        self.model.rows = rows(&self.results, self.builtins, &self.library);
        self.model.empty = self.results.is_empty().then_some(NO_MATCHES);
        // The menu belongs to a row that may be gone.
        self.set_menu(None, None);
    }

    /// The model worked out from scratch from the browser's own state: its
    /// modules, its library, and the query, highlight, and menu the user
    /// chose. A test compares it with the kept model after every message,
    /// so an update that forgets to change the model fails.
    #[cfg(test)]
    pub(crate) fn fresh_model(&self) -> Model {
        let results = results(
            &self.modules,
            &self.roles,
            self.builtins,
            &self.library,
            &self.model.query,
        );
        let highlight = self.model.highlight.filter(|&at| at < results.len());
        Model {
            query: self.model.query.clone(),
            rows: rows(&results, self.builtins, &self.library),
            highlight,
            about: highlight.map(|at| about(results[at], self.builtins, &self.library)),
            menu: self.model.menu,
            menu_highlight: self.model.menu_highlight,
            empty: results.is_empty().then_some(NO_MATCHES),
        }
    }

    fn add(&self, linked: bool) -> Outcome {
        if let Some(descriptor) = self.highlighted() {
            return Outcome::Add(descriptor, self.placement);
        }
        match self.model.highlight.and_then(|at| self.results.get(at)) {
            Some(Entry::Role(role)) => return Outcome::AddSlot(role, self.placement),
            Some(Entry::Builtin(i)) => return Outcome::InsertBuiltin(&self.builtins[*i], self.placement),
            _ => {}
        }
        match self.highlighted_item() {
            Some(item) => Outcome::Insert {
                item: item.clone(),
                linked,
                placement: self.placement,
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
            Nav::Escape | Nav::Menu => at,
        });
    }

    /// Scrolls the list just enough to show the highlighted row: the
    /// runtime's scroll container moves only as far as it must.
    fn reveal(&self) -> Effects<crate::Message> {
        match self.model.rows.get(self.highlight()) {
            Some(row) => Effects::scroll_into_view(row.id.clone()),
            None => Effects::none(),
        }
    }
}

/// The browser's message for an edit of its search field, which the
/// runtime holds (`runtime_fields.rs`): the new query. Enter never reaches
/// the field, since the browser takes it first (`panel.rs`), and the focus
/// leaving it means nothing to the browser.
pub(crate) fn field_message(event: &loom_ui::FieldEvent) -> Option<crate::Message> {
    match event {
        loom_ui::FieldEvent::Edited(query) => Some(crate::Message::Browser(Message::Query(query.clone()))),
        _ => None,
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

/// What the list shows for `query`: the modules, then the library items.
fn results(
    modules: &[&'static ModuleDescriptor],
    roles: &[&'static RoleDescriptor],
    builtins: &[crate::builtin::Builtin],
    library: &[LibraryItem],
    query: &str,
) -> Vec<Entry> {
    let slots = slot_roles(modules, roles, library);
    search_builtins(builtins, query)
        .into_iter()
        .map(Entry::Builtin)
        .chain(search(modules, query).into_iter().map(Entry::Module))
        .chain(search_roles(&slots, query).into_iter().map(Entry::Role))
        .chain(search_library(library, query).into_iter().map(Entry::Library))
        .collect()
}

/// The built-in abstractions that match a query, as indices into
/// [`crate::builtin::ALL`], best first: every word of the query is in the
/// name, `sub-patch`, or the mark `Built in`, ignoring case, ranked by name
/// as modules are (module-browser.md, Groups). Indices are into `all`.
fn search_builtins(all: &[crate::builtin::Builtin], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut found: Vec<usize> = (0..all.len())
        .filter(|&i| {
            let text = format!("{} sub-patch {BUILTIN_MARK}", all[i].name).to_lowercase();
            words.iter().all(|w| text.contains(w))
        })
        .collect();
    if !query.is_empty() {
        found.sort_by_key(|&i| Reverse(rank(all[i].name, &query)));
    }
    found
}

/// The roles an empty slot can be added for: each of `roles` that a module
/// in `modules` declares, or that a library item's file declares, once
/// each, by name. A library item's role kind resolves as a file's does: an
/// unknown role, or a version newer than the registry's, adds no row
/// (sub-patches.md, Empty slots).
fn slot_roles(
    modules: &[&'static ModuleDescriptor],
    roles: &[&'static RoleDescriptor],
    library: &[LibraryItem],
) -> Vec<&'static RoleDescriptor> {
    let filled_by_library = |role: &RoleDescriptor| {
        library.iter().flat_map(|item| &item.roles).any(|kind| {
            let Some((id, version)) = kind.split_once('@') else {
                return false;
            };
            id == role.id && version.parse::<u32>().is_ok_and(|v| v <= role.version)
        })
    };
    let mut found: Vec<&'static RoleDescriptor> = roles
        .iter()
        .copied()
        .filter(|role| modules.iter().any(|d| d.fills(role.id).is_some()) || filled_by_library(role))
        .collect();
    found.sort_by_key(|role| (role.name, role.id));
    found.dedup_by_key(|role| role.id);
    found
}

/// The slot rows that match a query, best first: every word of the query
/// is in the row's text (`Filter slot`), the mark `Slot`, or the role's
/// kind, ignoring case, ranked by the row's text as modules are.
fn search_roles(roles: &[&'static RoleDescriptor], query: &str) -> Vec<&'static RoleDescriptor> {
    let query = query.trim().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut found: Vec<&'static RoleDescriptor> = roles
        .iter()
        .copied()
        .filter(|role| {
            let text = format!("{} slot {} {SLOT_MARK}", role.name, role.kind()).to_lowercase();
            words.iter().all(|w| text.contains(w))
        })
        .collect();
    if !query.is_empty() {
        found.sort_by_key(|role| Reverse(rank(&format!("{} slot", role.name), &query)));
    }
    found
}

/// The rows the model shows for `results`, each with its UI ID, made
/// unique, its text worked out once, and its description.
fn rows(results: &[Entry], builtins: &[crate::builtin::Builtin], library: &[LibraryItem]) -> Vec<BrowserRow> {
    let mut ids = crate::a11y::items::UniqueIds::default();
    results
        .iter()
        .map(|&entry| {
            let id = ids.take(row_id(entry, builtins, library));
            let key = id.to_string();
            match entry {
                Entry::Builtin(i) => BrowserRow {
                    id,
                    key,
                    name: builtins[i].name.to_string(),
                    mark: BUILTIN_MARK,
                    family: Family::Interface,
                    library: false,
                    description: builtins[i].description().to_string(),
                },
                Entry::Module(d) => BrowserRow {
                    id,
                    key,
                    name: d.name.to_string(),
                    mark: d.family.label(),
                    family: d.family,
                    library: false,
                    description: d.description.to_string(),
                },
                Entry::Role(r) => BrowserRow {
                    id,
                    key,
                    name: format!("{} slot", r.name),
                    mark: SLOT_MARK,
                    family: Family::Interface,
                    library: false,
                    description: r.description.to_string(),
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
fn about(entry: Entry, builtins: &[crate::builtin::Builtin], library: &[LibraryItem]) -> About {
    match entry {
        Entry::Builtin(i) => About {
            text: builtins[i].description().to_string(),
            detail: "Built in, inserted as a copy".to_string(),
        },
        Entry::Module(d) => About {
            text: d.description.to_string(),
            detail: d.id.to_string(),
        },
        Entry::Role(r) => About {
            text: format!(
                "{} Add an empty slot, then swap a part in from its menu or the inspector.{}",
                r.description,
                if r.thru {
                    " While empty, it passes its input through."
                } else {
                    ""
                }
            ),
            detail: r.kind(),
        },
        Entry::Library(i) => About {
            text: "A sub-patch from your library, inserted as a copy. Right-click it to insert it linked.".to_string(),
            detail: library[i].library.clone(),
        },
    }
}

/// The modules that match a query, best first.
///
/// Match every query word against descriptor text. A result that needs
/// the description follows all matches in the shorter identifying fields.
pub fn search(modules: &[&'static ModuleDescriptor], query: &str) -> Vec<&'static ModuleDescriptor> {
    let query = query.trim().to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut found: Vec<_> = modules
        .iter()
        .copied()
        .filter_map(|d| {
            let score = if words.is_empty() {
                0
            } else {
                let fields = format!("{} {} {} {}", d.name, d.id, d.category, d.family.label()).to_lowercase();
                if words.iter().all(|word| fields.contains(word)) {
                    1 + rank(d.name, &query)
                } else {
                    let description = d.description.to_lowercase();
                    if !words
                        .iter()
                        .all(|word| fields.contains(word) || description.contains(word))
                    {
                        return None;
                    }
                    0
                }
            };
            Some((Reverse(score), d))
        })
        .collect();
    found.sort_by_key(|(rank, d)| (*rank, d.family, d.name, d.id));
    found.into_iter().map(|(_, d)| d).collect()
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

/// Moves the panel's corner so the whole panel fits inside `area`, where it
/// can. A small area keeps the corner at its top-left.
fn inside(at: Point, area: Rect) -> Point {
    let x = at.x.min(area.x + area.width - WIDTH).max(area.x);
    let y = at.y.min(area.y + area.height - HEIGHT).max(area.y);
    Point::new(x, y)
}

/// A family tint that stays distinct on the browser's normal and highlighted rows.
pub(crate) fn family_swatch(family: Family) -> loom_ui_core::Color {
    loom_canvas::Palette::default().family_tint(family.index())
}

pub(crate) mod access;

#[cfg(test)]
mod tests;
