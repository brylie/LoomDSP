//! Tests for the multi-line field (ui-layer.md, section 8, rule 7): the
//! keys per platform, text that keeps its line breaks, wrapping, runs per
//! visual line, and its node.

use super::super::{Edge, FieldAction, FieldEvent, FieldState, Lines, multi_line};
use super::{Bench, Keys, PLATFORMS, SHIFT, STYLE, held, key_in, press, shortcut, word};
use crate::access::{Access, Role};
use crate::app::{App, Context, WindowSpec};
use crate::effect::Effects;
use crate::element::{Element, Length};
use crate::event::{DriverEvent, WindowState};
use crate::focus::AccessNodes;
use crate::headless::Harness;
use crate::paint::{Typesetter, editing_contexts};
use crate::view::{Layer, View};
use accesskit::Role as KitRole;
use loom_ui_core::{IdTable, Key, Modifiers, Platform, Size, UiId};
use std::time::Instant;

/// A multi-line field showing `text`, wrapping at `width`, with the caret at
/// its end, or `None` on a machine with no font.
fn area_in(text: &str, width: f32) -> Option<Bench> {
    let mut typesetter = Typesetter::new();
    if typesetter.measure("x", STYLE, None).width <= 0.0 {
        eprintln!("skipped: no system font");
        return None;
    }
    let (mut fonts, mut layouts) = editing_contexts();
    let mut field = FieldState::new(text, STYLE, &mut fonts, &mut layouts);
    field.take(text, STYLE, Lines::Many, width, &mut fonts, &mut layouts);
    Some(Bench {
        field,
        fonts,
        layouts,
        now: Instant::now(),
    })
}

impl Bench {
    /// Moves the caret to byte `at`, as a screen reader's selection does.
    fn caret_to(&mut self, at: usize) {
        let characters = self.field.characters(false);
        let spans = FieldState::run_spans(&characters);
        let index = characters
            .iter()
            .take_while(|character| character.bytes.start < at)
            .count();
        let run = spans
            .iter()
            .position(|span| index < span.end)
            .unwrap_or(spans.len() - 1);
        let place = (run, index - spans[run].start);
        self.field.act(
            FieldAction::Select {
                anchor: place,
                focus: place,
            },
            self.now,
            &mut self.fonts,
            &mut self.layouts,
        );
    }
}

/// A pasted, committed, or screen reader's text keeps its line breaks:
/// CRLF, CR, and the line and paragraph separators become LF, a tab becomes
/// a space, other control characters go, and a break at the end stays.
#[test]
fn multi_line_keeps_breaks_and_makes_them_lf() {
    assert_eq!(multi_line("a\r\nb\rc\u{2028}d\u{2029}e"), "a\nb\nc\nd\ne");
    assert_eq!(multi_line("a\tb\u{7}c"), "a bc");
    assert_eq!(multi_line("a\n"), "a\n");
    assert_eq!(multi_line(""), "");
}

/// Enter inserts a line break and commits nothing; with the shortcut key it
/// goes on to the app's shortcuts. Tab is never taken.
#[test]
fn enter_inserts_a_break_and_tab_leaves() {
    for platform in PLATFORMS {
        let Some(mut bench) = area_in("Alice", 200.0) else {
            return;
        };
        let used = bench.hit(platform, Key::Enter, Modifiers::NONE);
        assert!(used.taken && used.edited && !used.committed, "{platform:?}: {used:?}");
        bench.type_text(platform, "Bob");
        assert_eq!(bench.text(), "Alice\nBob");
        assert!(!bench.hit(platform, Key::Enter, shortcut(platform, false)).taken);
        assert!(!bench.hit(platform, Key::Tab, Modifiers::NONE).taken);
        assert!(!bench.hit(platform, Key::Tab, SHIFT).taken);
        assert_eq!(bench.text(), "Alice\nBob");
    }
}

/// Up and Down move one line and keep the caret's place across; on the
/// first line Up goes to the start, on the last Down goes to the end.
#[test]
fn up_and_down_move_by_line() {
    for platform in PLATFORMS {
        let Some(mut bench) = area_in("abc\nabc", 200.0) else {
            return;
        };
        bench.caret_to(6);
        bench.hit(platform, Key::Up, Modifiers::NONE);
        assert_eq!(bench.caret(), 2, "{platform:?}: up keeps its place");
        bench.hit(platform, Key::Down, Modifiers::NONE);
        assert_eq!(bench.caret(), 6, "{platform:?}: and down comes back");
        bench.hit(platform, Key::Down, Modifiers::NONE);
        assert_eq!(bench.caret(), 7, "{platform:?}: down on the last line goes to the end");
        bench.hit(platform, Key::Up, Modifiers::NONE);
        bench.hit(platform, Key::Up, Modifiers::NONE);
        assert_eq!(bench.caret(), 0, "{platform:?}: up on the first line goes to the start");
        bench.hit(platform, Key::Down, SHIFT);
        assert_eq!(bench.field.selection_bytes(), (0, 4), "{platform:?}: Shift extends");
    }
}

/// On a Mac, Cmd with Up and Down goes to the text's start and end, and
/// Home and End show its start and end without moving the caret.
#[test]
fn on_a_mac_cmd_arrows_go_to_the_ends_and_home_and_end_only_show_them() {
    let mac = Platform::Mac;
    let Some(mut bench) = area_in("ab\ncd\nef", 200.0) else {
        return;
    };
    bench.caret_to(4);
    bench.hit(mac, Key::Up, shortcut(mac, false));
    assert_eq!(bench.caret(), 0);
    bench.hit(mac, Key::Down, shortcut(mac, false));
    assert_eq!(bench.caret(), 8);
    bench.caret_to(4);
    let home = bench.hit(mac, Key::Home, Modifiers::NONE);
    assert_eq!(home.reveal, Some(Edge::Start));
    assert!(home.taken);
    assert_eq!(bench.caret(), 4, "Home leaves the caret");
    assert_eq!(bench.hit(mac, Key::End, Modifiers::NONE).reveal, Some(Edge::End));
    assert_eq!(bench.caret(), 4, "End leaves the caret");
}

/// On a Mac, Ctrl+A and Ctrl+E go to the start and the end of the line.
#[test]
fn on_a_mac_ctrl_a_and_ctrl_e_go_to_the_line_ends() {
    let mac = Platform::Mac;
    let control = held(false, true, false, false);
    let Some(mut bench) = area_in("ab\ncd", 200.0) else {
        return;
    };
    bench.caret_to(4);
    let start = bench.key(mac, &press(Key::Character('a'), control, None));
    assert!(start.taken);
    assert_eq!(bench.caret(), 3);
    bench.key(mac, &press(Key::Character('e'), control, None));
    assert_eq!(bench.caret(), 5);
}

/// Off a Mac, Home and End go to the line's ends, and with Ctrl to the
/// text's.
#[test]
fn off_a_mac_home_and_end_move_and_ctrl_goes_to_the_ends() {
    let other = Platform::Other;
    let Some(mut bench) = area_in("ab\ncd\nef", 200.0) else {
        return;
    };
    bench.caret_to(4);
    bench.hit(other, Key::Home, Modifiers::NONE);
    assert_eq!(bench.caret(), 3);
    bench.hit(other, Key::End, Modifiers::NONE);
    assert_eq!(bench.caret(), 5);
    assert_eq!(bench.hit(other, Key::Home, Modifiers::NONE).reveal, None);
    bench.hit(other, Key::Home, shortcut(other, false));
    assert_eq!(bench.caret(), 0);
    bench.hit(other, Key::End, shortcut(other, false));
    assert_eq!(bench.caret(), 8);
}

/// The word key with Up and Down (Option on a Mac, Ctrl elsewhere) goes to
/// the paragraph's start or end, and from there on to the next one.
#[test]
fn the_word_key_with_up_and_down_goes_by_paragraph() {
    for platform in PLATFORMS {
        let Some(mut bench) = area_in("ab\ncd\nef", 200.0) else {
            return;
        };
        bench.caret_to(4);
        bench.hit(platform, Key::Up, word(platform, false));
        assert_eq!(bench.caret(), 3, "{platform:?}: to this paragraph's start");
        bench.hit(platform, Key::Up, word(platform, false));
        assert_eq!(bench.caret(), 0, "{platform:?}: and on to the one before");
        bench.hit(platform, Key::Down, word(platform, false));
        assert_eq!(bench.caret(), 2, "{platform:?}: to its end");
        bench.hit(platform, Key::Down, word(platform, false));
        assert_eq!(bench.caret(), 5, "{platform:?}: and on to the next one's");
    }
}

/// Off a Mac, Page Up and Page Down go to the text's start and end, since
/// the box grows with its lines and one page is all of it. On a Mac the
/// field leaves them, for the panel to scroll.
#[test]
fn page_up_and_down_go_to_the_ends_off_a_mac() {
    let other = Platform::Other;
    let Some(mut bench) = area_in("ab\ncd\nef", 200.0) else {
        return;
    };
    bench.caret_to(4);
    bench.hit(other, Key::PageUp, Modifiers::NONE);
    assert_eq!(bench.caret(), 0);
    bench.hit(other, Key::PageDown, SHIFT);
    assert_eq!(bench.field.selection_bytes(), (0, 8), "Shift extends to the end");
    assert!(!bench.hit(Platform::Mac, Key::PageUp, Modifiers::NONE).taken);
    assert!(!bench.hit(Platform::Mac, Key::PageDown, Modifiers::NONE).taken);
}

/// The row shown for a field's start is its top pixel, and for its end
/// its bottom one, so Home and End scroll only as far as that edge needs.
#[test]
fn the_row_shown_for_an_edge_is_its_top_or_bottom_pixel() {
    use crate::window::scroll::edge_row;
    use loom_ui_core::{Point, Rect};
    let bounds = Rect::new(Point::new(10.0, 20.0), Size::new(100.0, 50.0));
    assert_eq!(
        edge_row(bounds, Edge::Start),
        Rect::new(Point::new(10.0, 20.0), Size::new(100.0, 1.0))
    );
    assert_eq!(
        edge_row(bounds, Edge::End),
        Rect::new(Point::new(10.0, 69.0), Size::new(100.0, 1.0))
    );
}

/// A paste and a screen reader's text keep their line breaks, made LF.
#[test]
fn a_paste_and_a_screen_readers_text_keep_their_breaks() {
    let Some(mut bench) = area_in("", 200.0) else { return };
    bench
        .field
        .paste("Alice\r\nBob", bench.now, &mut bench.fonts, &mut bench.layouts);
    assert_eq!(bench.text(), "Alice\nBob");
    bench.field.act(
        FieldAction::SetValue("Ann\rBo".into()),
        bench.now,
        &mut bench.fonts,
        &mut bench.layouts,
    );
    assert_eq!(bench.text(), "Ann\nBo");
}

/// A long text wraps to the field's width: its runs are on more than one
/// line, each line's run lower than the last, and none wider than the box
/// to its last letter. A trailing space may hang past the edge, and does
/// with Windows' fonts.
#[test]
fn a_long_text_wraps_into_one_run_per_line() {
    let Some(bench) = area_in("alpha beta gamma delta epsilon zeta eta theta", 80.0) else {
        return;
    };
    let runs = bench.field.runs();
    assert!(runs.len() > 1, "{runs:?}");
    for pair in runs.windows(2) {
        assert!(pair[1].line > pair[0].line, "one run per line here");
        assert!(pair[1].y > pair[0].y, "each line is lower");
    }
    // A line's trailing space may hang past the edge, as text layout lets
    // it, so a run is measured to its last letter that is not a space.
    let ink = |run: &super::super::ReaderRun| {
        run.text
            .chars()
            .zip(run.positions.iter().zip(&run.widths))
            .filter(|(letter, _)| !letter.is_whitespace())
            .map(|(_, (at, width))| at + width)
            .fold(0.0_f32, f32::max)
    };
    assert!(runs.iter().all(|run| ink(run) <= 80.0 + 0.5), "{runs:?}");
    // The hanging space keeps its own character, so the runs still spell
    // the whole text and a screen reader can put its caret after the space.
    let spelled: String = runs.iter().map(|run| run.text.as_str()).collect();
    assert_eq!(spelled, bench.text());
    for run in &runs {
        let bytes: usize = run.lengths.iter().map(|length| usize::from(*length)).sum();
        assert_eq!(bytes, run.text.len(), "{run:?}");
        assert_eq!(run.positions.len(), run.lengths.len());
        assert_eq!(run.widths.len(), run.lengths.len());
    }
    assert_eq!(bench.field.scroll(), 0.0, "a text area never scrolls sideways");
}

/// A run never crosses a hard break, and the selection's ends are named by
/// the run they are on: the caret at the end is on the last run.
#[test]
fn runs_stop_at_a_hard_break_and_the_selection_finds_its_run() {
    let Some(mut bench) = area_in("ab\ncd", 200.0) else {
        return;
    };
    let runs = bench.field.runs();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].text, "ab\n", "the break ends its line's run");
    assert_eq!(runs[1].text, "cd");
    assert_eq!(bench.field.reader_selection(), ((1, 2), (1, 2)));
    bench.field.act(
        FieldAction::Select {
            anchor: (0, 1),
            focus: (1, 1),
        },
        bench.now,
        &mut bench.fonts,
        &mut bench.layouts,
    );
    assert_eq!(bench.field.selection_bytes(), (1, 4), "across the break");
}

/// One app with one text area keyed `notes/area`, keeping its edits.
#[derive(Default)]
struct Notes {
    /// The area sits in a scroll container 40 px high.
    scrolled: bool,
    text: String,
    heard: Vec<FieldEvent>,
    ids: IdTable,
    nodes: Option<AccessNodes>,
}

/// What the notes app hears.
enum Note {
    Focus,
    Field(FieldEvent),
    Describe,
}

/// The text area's UI ID.
fn area() -> UiId {
    UiId::panel("notes").with("area")
}

impl Notes {
    /// Its area's UI ID, under the scroll container when there is one.
    fn area(&self) -> UiId {
        if self.scrolled {
            UiId::panel("notes").with("pane").with("area")
        } else {
            area()
        }
    }
}

impl App for Notes {
    type Message = Note;

    fn update(&mut self, message: Note, cx: &Context<'_, Note>) -> Effects<Note> {
        match message {
            Note::Focus => return Effects::focus(self.area()),
            Note::Field(event) => {
                if let FieldEvent::Edited(text) = &event {
                    self.text.clone_from(text);
                }
                self.heard.push(event);
            }
            Note::Describe => self.nodes = Some(cx.access_nodes(&mut self.ids)),
        }
        Effects::none()
    }

    fn view(&self, view: &mut View) {
        let model = (self.text.clone(), self.scrolled);
        view.panel_with(
            UiId::panel("notes"),
            Layer::Panels,
            &model,
            |(text, scrolled): &(String, bool)| {
                let area = Element::column([Element::text_area(text.clone(), STYLE)
                    .key("area")
                    .width(Length::Fixed(160.0))
                    .access(Access::new(Role::TextInput, "Authors, one per line"))]);
                if !*scrolled {
                    return area;
                }
                Element::scroll(area, crate::window::scroll::tests::SCROLL)
                    .key("pane")
                    .height(Length::Fixed(40.0))
                    .access(Access::new(Role::ScrollView, "Patch info"))
            },
        );
    }

    fn window(&self) -> WindowSpec {
        WindowSpec {
            title: "Notes".to_owned(),
            min_size: None,
        }
    }

    fn access_target(&self, node: accesskit::NodeId) -> Option<UiId> {
        self.ids.id(node.0).cloned()
    }

    fn field_event(&self, _id: &UiId, event: &FieldEvent) -> Option<Note> {
        Some(Note::Field(event.clone()))
    }
}

/// The notes app with `text`, its area focused and laid out, or `None`
/// with no font.
fn notes(text: &str) -> Option<(Harness<Notes, Keys>, Typesetter)> {
    notes_in(text, false)
}

/// The notes app with `text`, in a scroll container if `scrolled`.
fn notes_in(text: &str, scrolled: bool) -> Option<(Harness<Notes, Keys>, Typesetter)> {
    let mut typesetter = Typesetter::new();
    if typesetter.measure("x", STYLE, None).width <= 0.0 {
        eprintln!("skipped: no system font");
        return None;
    }
    let app = Notes {
        text: text.to_owned(),
        scrolled,
        ..Notes::default()
    };
    let window = WindowState::new(Size::new(400.0, 300.0));
    let mut harness = Harness::new(app, Keys::default(), window, Instant::now());
    harness.step(DriverEvent::Woken);
    harness.lay_out(&mut typesetter);
    harness.send(Note::Focus);
    harness.lay_out(&mut typesetter);
    let focused = harness.app().area();
    assert_eq!(harness.runtime().focused(), Some(&focused));
    Some((harness, typesetter))
}

/// In the runtime, Enter in a text area reaches the app as an edit with a
/// line break, never as a commit, and Tab moves focus on.
#[test]
fn a_text_area_edits_with_enter_and_never_commits() {
    let Some((mut harness, mut text)) = notes("Alice") else {
        return;
    };
    harness.step(key_in(Key::Enter, Modifiers::NONE, None));
    harness.step(key_in(Key::Character('B'), Modifiers::NONE, Some("B")));
    harness.lay_out(&mut text);
    assert_eq!(harness.app().text, "Alice\nB");
    assert!(
        !harness
            .app()
            .heard
            .iter()
            .any(|event| matches!(event, FieldEvent::Committed(_))),
        "{:?}",
        harness.app().heard
    );
}

/// A text area grows with its lines: a second line makes it taller.
#[test]
fn a_text_area_grows_with_its_lines() {
    let Some((mut harness, mut text)) = notes("Alice") else {
        return;
    };
    let one = harness.runtime().bounds(&area()).expect("laid out").height;
    harness.step(key_in(Key::Enter, Modifiers::NONE, None));
    harness.step(key_in(Key::Character('B'), Modifiers::NONE, Some("B")));
    harness.lay_out(&mut text);
    let two = harness.runtime().bounds(&area()).expect("laid out").height;
    assert!(two > one, "{one} then {two}");
}

/// Its node is a multi-line text input whose runs are one per line, and a
/// run links only to runs on its own line.
#[test]
fn its_node_is_multi_line_and_runs_link_only_along_a_line() {
    let Some((mut harness, mut text)) = notes("ab\ncd") else {
        return;
    };
    harness.lay_out(&mut text);
    harness.send(Note::Describe);
    let nodes = &harness.app().nodes.as_ref().expect("nodes").nodes;
    let (_, node) = nodes
        .iter()
        .find(|(_, node)| node.role() == KitRole::MultilineTextInput)
        .expect("a multi-line text input");
    assert_eq!(node.value(), Some("ab\ncd"));
    let runs = node.children().to_vec();
    assert_eq!(runs.len(), 2);
    let run = |id| {
        nodes
            .iter()
            .find(|(at, _)| *at == id)
            .map(|(_, node)| node.clone())
            .expect("run")
    };
    assert_eq!(run(runs[0]).next_on_line(), None, "no link across the break");
    assert_eq!(run(runs[1]).previous_on_line(), None);
    let (first, second) = (run(runs[0]), run(runs[1]));
    let top = |node: &accesskit::Node| node.bounds().expect("bounds").y0;
    assert!(top(&second) > top(&first), "the second line's run is lower");
}

/// How far the notes app's scroll container is scrolled, as its node says.
fn scrolled_by(harness: &mut Harness<Notes, Keys>, text: &mut Typesetter) -> f64 {
    harness.lay_out(text);
    harness.send(Note::Describe);
    let nodes = &harness.app().nodes.as_ref().expect("nodes").nodes;
    nodes
        .iter()
        .find(|(_, node)| node.role() == KitRole::ScrollView)
        .and_then(|(_, node)| node.scroll_y())
        .unwrap_or(0.0)
}

/// On a Mac, End in a text area taller than its scroll container scrolls
/// to show the box's end, and Home back to its start, and the caret never
/// moves. The keys run on this machine's platform, so the test runs on a
/// Mac only.
#[test]
fn on_a_mac_home_and_end_scroll_the_container_and_leave_the_caret() {
    if Platform::current() != Platform::Mac {
        eprintln!("skipped: Home and End move the caret off a Mac");
        return;
    }
    let Some((mut harness, mut text)) = notes_in("a\nb\nc\nd\ne\nf\ng", true) else {
        return;
    };
    assert_eq!(scrolled_by(&mut harness, &mut text), 0.0);
    harness.step(key_in(Key::End, Modifiers::NONE, None));
    let down = scrolled_by(&mut harness, &mut text);
    assert!(down > 0.0, "End shows the box's end");
    harness.step(key_in(Key::Home, Modifiers::NONE, None));
    assert_eq!(scrolled_by(&mut harness, &mut text), 0.0, "Home shows its start");
    assert!(
        !harness
            .app()
            .heard
            .iter()
            .any(|event| matches!(event, FieldEvent::Edited(_))),
        "neither key edits"
    );
}

/// The caret's place in the window, from the runtime's input method area,
/// and the bounds of the notes app's scroll container.
fn caret_and_pane(harness: &Harness<Notes, Keys>) -> (loom_ui_core::Rect, loom_ui_core::Rect) {
    let Some(crate::Ime::On { area }) = harness.runtime().ime() else {
        panic!("the area has focus, so its caret has a place");
    };
    let pane = harness
        .runtime()
        .bounds(&UiId::panel("notes").with("pane"))
        .expect("the container");
    (area, pane)
}

/// The caret lies within the container, top to bottom, after a layout.
fn assert_caret_shows(harness: &Harness<Notes, Keys>) {
    let (caret, pane) = caret_and_pane(harness);
    assert!(
        caret.y >= pane.y && caret.y + caret.height <= pane.y + pane.height,
        "the caret at {caret:?} is outside the container at {pane:?}"
    );
}

/// Typing new lines in a text area taller than its scroll container keeps
/// the caret in view after every layout: the container follows it down,
/// and Up back to the first line brings the container back to the top.
#[test]
fn the_container_follows_the_caret_as_lines_are_typed() {
    let Some((mut harness, mut text)) = notes_in("a", true) else {
        return;
    };
    assert_eq!(scrolled_by(&mut harness, &mut text), 0.0);
    for _ in 0..6 {
        harness.step(key_in(Key::Enter, Modifiers::NONE, None));
        harness.lay_out(&mut text);
        assert_caret_shows(&harness);
    }
    assert!(
        scrolled_by(&mut harness, &mut text) > 0.0,
        "the container followed the caret down"
    );
    for _ in 0..6 {
        harness.step(key_in(Key::Up, Modifiers::NONE, None));
        harness.lay_out(&mut text);
        assert_caret_shows(&harness);
    }
    assert_eq!(scrolled_by(&mut harness, &mut text), 0.0, "and back to the top");
}

/// A paste of many lines keeps the caret in view too: the container
/// follows it to the pasted text's end.
#[test]
fn the_container_follows_the_caret_after_a_paste() {
    let Some((mut harness, mut text)) = notes_in("a", true) else {
        return;
    };
    harness.driver_mut().set_clipboard(Some("1\n2\n3\n4\n5\n6".to_owned()));
    harness.step(key_in(Key::Character('v'), shortcut(Platform::current(), false), None));
    harness.step(DriverEvent::Woken);
    harness.lay_out(&mut text);
    assert_eq!(harness.app().text, "a1\n2\n3\n4\n5\n6", "the paste went in");
    assert_caret_shows(&harness);
    assert!(scrolled_by(&mut harness, &mut text) > 0.0);
}

/// On a Mac, Home right after a caret move, with no layout between, still
/// shows the box's start: the edge asked for wins over the caret request
/// that the move left waiting. The keys run on this machine's platform, so
/// the test runs on a Mac only.
#[test]
fn on_a_mac_home_wins_over_a_waiting_caret_request() {
    if Platform::current() != Platform::Mac {
        eprintln!("skipped: Home and End move the caret off a Mac");
        return;
    }
    let Some((mut harness, mut text)) = notes_in("a\nb\nc\nd\ne\nf\ng", true) else {
        return;
    };
    // The caret starts at the end; a letter typed there brings the end in.
    harness.step(key_in(Key::Character('h'), Modifiers::NONE, Some("h")));
    harness.lay_out(&mut text);
    assert!(scrolled_by(&mut harness, &mut text) > 0.0, "the caret's end shows");
    // A move, then Home, before any layout.
    harness.step(key_in(Key::Up, Modifiers::NONE, None));
    harness.step(key_in(Key::Home, Modifiers::NONE, None));
    harness.lay_out(&mut text);
    assert_eq!(scrolled_by(&mut harness, &mut text), 0.0, "Home's start, not the caret");
}

/// On a Mac, End right after an edit, with no layout between, shows the
/// box's end once the layout has the edit: the edge waits for a current
/// layout as the caret does. The keys run on this machine's platform, so
/// the test runs on a Mac only.
#[test]
fn on_a_mac_end_after_an_edit_shows_the_end_once_laid_out() {
    if Platform::current() != Platform::Mac {
        eprintln!("skipped: Home and End move the caret off a Mac");
        return;
    }
    let Some((mut harness, mut text)) = notes_in("a\nb\nc\nd\ne\nf\ng", true) else {
        return;
    };
    harness.step(key_in(Key::Up, shortcut(Platform::Mac, false), None));
    harness.lay_out(&mut text);
    assert_eq!(scrolled_by(&mut harness, &mut text), 0.0, "the caret is at the start");
    // An edit, then End, before any layout.
    harness.step(key_in(Key::Character('x'), Modifiers::NONE, Some("x")));
    harness.step(key_in(Key::End, Modifiers::NONE, None));
    harness.lay_out(&mut text);
    let most = harness
        .runtime()
        .scroll_offset(&UiId::panel("notes").with("pane"))
        .expect("the container")
        .1;
    assert!(most > 0.0);
    assert_eq!(f64::from(most), scrolled_by(&mut harness, &mut text), "End's end shows");
}
