//! The module browser: search the registry and add a module where the user
//! opened the browser. Spec: `docs/design/module-browser.md`.

use crate::style;
use iced::widget::{column, container, mouse_area, row, scrollable, space, text, text_input};
use iced::{Element, Length, Point, Rectangle, Task, Theme};
use loom_canvas::GRID;
use loom_core::ModuleDescriptor;
use std::cmp::Reverse;

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

const INPUT: &str = "module-browser-search";
const LIST: &str = "module-browser-list";

/// What the browser asks of the app.
#[derive(Debug, Clone)]
pub enum Message {
    /// The query changed.
    Query(String),
    /// The pointer moved over a row.
    Hover(usize),
    /// A row was clicked.
    Pick(usize),
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

/// What the app should do after a browser message.
#[derive(Debug)]
pub enum Outcome {
    /// Keep the browser open.
    Stay,
    /// Close it and add nothing.
    Close,
    /// Close it and add this module at this world point.
    Add(&'static ModuleDescriptor, Point),
}

/// An open browser.
pub struct Browser {
    modules: Vec<&'static ModuleDescriptor>,
    query: String,
    results: Vec<&'static ModuleDescriptor>,
    highlight: usize,
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
    pub fn open(modules: Vec<&'static ModuleDescriptor>, world: Point, at: Point, area: Rectangle) -> Self {
        let results = search(&modules, "");
        Self {
            modules,
            query: String::new(),
            results,
            highlight: 0,
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
                self.results = search(&self.modules, &query);
                self.query = query;
                self.highlight = 0;
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
                self.add()
            }
            Message::Submit => self.add(),
            Message::Scrolled(offset) => {
                self.scroll = offset;
                Outcome::Stay
            }
            Message::Key(Nav::Escape) | Message::Close => Outcome::Close,
            Message::Key(nav) => {
                self.step(nav);
                Outcome::Stay
            }
        };
        let task = if follow { self.reveal() } else { Task::none() };
        (outcome, task)
    }

    /// Gives the search field the keyboard focus.
    pub fn focus<T>() -> Task<T> {
        iced::widget::operation::focus(INPUT)
    }

    /// The modules the list shows, best first.
    #[cfg(test)]
    pub fn results(&self) -> &[&'static ModuleDescriptor] {
        &self.results
    }

    /// The highlighted module, if there are results.
    pub fn highlighted(&self) -> Option<&'static ModuleDescriptor> {
        self.results.get(self.highlight).copied()
    }

    fn add(&self) -> Outcome {
        match self.highlighted() {
            Some(descriptor) => Outcome::Add(descriptor, self.world),
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

    /// The panel, placed at its corner. The app lays it over the window.
    pub fn view(&self) -> Element<'_, Message> {
        let input = text_input("Search modules", &self.query)
            .id(INPUT)
            .on_input(Message::Query)
            .on_submit(Message::Submit)
            .size(style::CONTROL)
            .padding(6);

        let rows = self.results.iter().enumerate().map(|(i, d)| {
            let highlighted = i == self.highlight;
            let line = row![
                text(d.name).size(style::CONTROL),
                // The family's colour and name, so colour is never the only
                // cue (ADR-0029).
                container(space().width(8).height(8)).style(move |_: &Theme| container::Style {
                    background: Some(crate::scene::family_colour(d.family).into()),
                    border: iced::Border {
                        radius: 2.0.into(),
                        ..iced::Border::default()
                    },
                    ..container::Style::default()
                }),
                text(d.family.label()).size(style::SMALL).color(style::QUIET),
            ]
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
        let about = self.highlighted().map_or_else(
            || column![],
            |d| {
                column![
                    text(d.description).size(style::BODY),
                    text(d.id).size(style::SMALL).color(style::QUIET),
                ]
                .spacing(2)
            },
        );
        let panel = column![input, list, container(about).height(DESCRIPTION_HEIGHT)]
            .spacing(SPACING)
            .padding(PADDING)
            .width(WIDTH);
        let panel = container(panel).style(panel_style);
        iced::widget::pin(iced::widget::opaque(panel)).position(self.at).into()
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
mod tests {
    use super::*;

    fn modules() -> Vec<&'static ModuleDescriptor> {
        loom_modules::registry().iter().collect()
    }

    fn ids(found: &[&'static ModuleDescriptor]) -> Vec<&'static str> {
        found.iter().map(|d| d.id).collect()
    }

    #[test]
    fn empty_query_lists_every_module_by_family_then_name() {
        let all = modules();
        let found = search(&all, "  ");
        assert_eq!(found.len(), all.len());
        let keys: Vec<_> = found.iter().map(|d| (d.family, d.name)).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }

    /// A family's name finds its modules, and a category still finds
    /// its own (ADR-0029).
    #[test]
    fn family_and_category_names_both_match() {
        let all = modules();
        let modulation = search(&all, "modulation");
        let family = all.iter().filter(|d| d.family == loom_core::Family::Modulation).count();
        assert!(
            modulation.len() >= family && family >= 3,
            "{} of {family}",
            modulation.len()
        );
        assert!(
            search(&all, "filters and effects")
                .iter()
                .any(|d| d.id == "loom.time.delay")
        );
        assert!(search(&all, "oscillator").iter().any(|d| d.id == "loom.osc.sine"));
    }

    #[test]
    fn a_name_that_starts_with_the_query_comes_first() {
        assert_eq!(search(&modules(), "saw")[0].id, "loom.osc.saw");
        // "Phase" starts with "phase"; so do "Phase Scale" and "Phase
        // Trigger", and they sort after it by name.
        assert_eq!(
            ids(&search(&modules(), "phase"))[..3],
            ["loom.phase.generator", "loom.phase.scale", "loom.phase.trigger"]
        );
    }

    #[test]
    fn a_word_in_the_name_ranks_above_a_match_elsewhere() {
        // "trig" starts a word of "Phase Trigger". Nothing else has it.
        assert_eq!(search(&modules(), "trig")[0].id, "loom.phase.trigger");
        // "meter" starts a word of "Level Meter" and is in its ID.
        assert_eq!(search(&modules(), "meter")[0].id, "loom.display.meter");
    }

    #[test]
    fn every_word_must_match_name_id_or_category() {
        let found = search(&modules(), "osc saw");
        assert_eq!(ids(&found), ["loom.osc.saw"]);
        for d in search(&modules(), "oscillator noise") {
            assert_eq!(d.id, "loom.osc.noise");
        }
    }

    #[test]
    fn case_does_not_matter() {
        let found = search(&modules(), "FILTER");
        assert!(!found.is_empty());
        let filters = modules().into_iter().filter(|d| d.category == "Filter").count();
        assert_eq!(found.iter().filter(|d| d.category == "Filter").count(), filters);
    }

    #[test]
    fn no_match_lists_nothing_and_enter_adds_nothing() {
        let mut browser = Browser::open(modules(), Point::ORIGIN, Point::ORIGIN, Rectangle::default());
        let _ = browser.update(Message::Query("zzzz".into()));
        assert!(browser.results.is_empty());
        assert!(matches!(browser.update(Message::Submit).0, Outcome::Stay));
        assert!(matches!(browser.update(Message::Key(Nav::Down)).0, Outcome::Stay));
    }

    #[test]
    fn enter_adds_the_highlighted_module_at_the_snapped_point() {
        let area = Rectangle::new(Point::ORIGIN, iced::Size::new(1000.0, 800.0));
        let mut browser = Browser::open(modules(), Point::new(100.4, 29.0), Point::ORIGIN, area);
        let _ = browser.update(Message::Query("saw".into()));
        match browser.update(Message::Submit).0 {
            Outcome::Add(d, at) => {
                assert_eq!(d.id, "loom.osc.saw");
                assert_eq!(at, Point::new(96.0, 24.0));
            }
            other => panic!("expected an add, got {other:?}"),
        }
    }

    #[test]
    fn arrows_move_the_highlight_and_stop_at_the_ends() {
        let mut browser = Browser::open(modules(), Point::ORIGIN, Point::ORIGIN, Rectangle::default());
        let last = browser.results.len() - 1;
        let _ = browser.update(Message::Key(Nav::Up));
        assert_eq!(browser.highlight, 0);
        let _ = browser.update(Message::Key(Nav::Down));
        assert_eq!(browser.highlight, 1);
        for _ in 0..5 {
            let _ = browser.update(Message::Key(Nav::PageDown));
        }
        assert_eq!(browser.highlight, last);
        let _ = browser.update(Message::Key(Nav::Down));
        assert_eq!(browser.highlight, last);
        let _ = browser.update(Message::Key(Nav::PageUp));
        assert_eq!(browser.highlight, last.saturating_sub(ROWS));
        // A new query highlights the first result again.
        let _ = browser.update(Message::Query("a".into()));
        assert_eq!(browser.highlight, 0);
    }

    #[test]
    fn escape_and_a_click_outside_close_without_adding() {
        let mut browser = Browser::open(modules(), Point::ORIGIN, Point::ORIGIN, Rectangle::default());
        assert!(matches!(browser.update(Message::Key(Nav::Escape)).0, Outcome::Close));
        assert!(matches!(browser.update(Message::Close).0, Outcome::Close));
    }

    #[test]
    fn the_panel_stays_inside_the_canvas() {
        let area = Rectangle::new(Point::new(0.0, 40.0), iced::Size::new(1000.0, 700.0));
        assert_eq!(inside(Point::new(100.0, 100.0), area), Point::new(100.0, 100.0));
        let corner = inside(Point::new(990.0, 730.0), area);
        assert_eq!(corner, Point::new(1000.0 - WIDTH, 740.0 - HEIGHT));
    }
}
