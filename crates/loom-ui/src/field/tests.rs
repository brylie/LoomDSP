//! Tests for text fields (ui-layer.md, section 8, rules 2 and 3, and
//! section 6, rule 6): editing, the keys on each platform, undo, scrolling,
//! the app's text, what reaches the app, the node, and drawing.
//!
//! The first group drives a field's state straight, so a test picks the
//! platform: the Mac's keys and the others' are covered there, on any
//! machine. The second goes through the harness as key events, and the
//! runtime reads `Platform::current()`, so it covers only the platform the
//! tests were built for. Both need a system font, since moving
//! by word walks the shaped layout, and skip with none, as the
//! typesetter's tests do.

use super::{CARET_MARGIN, FieldEvent, FieldState, Lines, TYPING_PAUSE};
use crate::access::{Access, Role, States};
use crate::app::{App, Context, WindowSpec};
use crate::effect::Effects;
use crate::effect::{FocusTarget, Selection as FocusSelection};
use crate::element::{Element, Length};
use crate::event::{DriverEvent, WindowEvent, WindowState};
use crate::focus::AccessNodes;
use crate::headless::Harness;
use crate::paint::testing::{Painted, Recorder};
use crate::paint::{PanelStyle, Typesetter, editing_contexts};
use crate::routing::{Routed, Target};
use crate::view::{Layer, View};
use accesskit::Role as KitRole;
use loom_ui_core::{
    Color, Event, IdTable, ImeEvent, Key, KeyEvent, Modifiers, Platform, Point, Size, TextStyle, UiId, Weight,
};
use parley::{FontContext, LayoutContext};
use std::time::{Duration, Instant};

/// The style every field here is set in.
const STYLE: TextStyle = TextStyle {
    size: 14.0,
    weight: Weight::Regular,
    line_height: 1.25,
};

/// The colours fields are painted in.
const PANEL: PanelStyle = PanelStyle {
    text: Color::rgb(0.9, 0.9, 0.9),
    caret: Color::rgb(1.0, 1.0, 1.0),
    selection: Color::rgb(0.2, 0.3, 0.6),
    ring: crate::focus::RingStyle {
        color: Color::WHITE,
        width: 2.0,
        offset: 2.0,
    },
};

/// A field and the contexts it edits with.
struct Bench {
    field: FieldState,
    fonts: FontContext,
    layouts: LayoutContext<()>,
    now: Instant,
}

/// A field showing `text` in a box `width` wide, with the caret at its end,
/// or `None` on a machine with no font.
fn field_in(text: &str, width: f32) -> Option<Bench> {
    let mut typesetter = Typesetter::new();
    if typesetter.measure("x", STYLE, None).width <= 0.0 {
        eprintln!("skipped: no system font");
        return None;
    }
    let (mut fonts, mut layouts) = editing_contexts();
    let mut field = FieldState::new(text, STYLE, &mut fonts, &mut layouts);
    field.take(text, STYLE, Lines::One, width, &mut fonts, &mut layouts);
    Some(Bench {
        field,
        fonts,
        layouts,
        now: Instant::now(),
    })
}

/// A key press with `modifiers`, typing `text`.
fn press(key: Key, modifiers: Modifiers, text: Option<&str>) -> KeyEvent {
    KeyEvent::Pressed {
        key,
        modifiers,
        repeat: false,
        text: text.map(str::to_owned),
    }
}

/// Modifiers: Shift, Control, Alt, and the system key.
fn held(shift: bool, control: bool, alt: bool, meta: bool) -> Modifiers {
    Modifiers {
        shift,
        control,
        alt,
        meta,
    }
}

/// The shortcut key on `platform`, with Shift if `shift`.
fn shortcut(platform: Platform, shift: bool) -> Modifiers {
    match platform {
        Platform::Mac => held(shift, false, false, true),
        Platform::Other => held(shift, true, false, false),
    }
}

/// The word key on `platform`, with Shift if `shift`.
fn word(platform: Platform, shift: bool) -> Modifiers {
    match platform {
        Platform::Mac => held(shift, false, true, false),
        Platform::Other => held(shift, true, false, false),
    }
}

/// Shift alone.
const SHIFT: Modifiers = Modifiers {
    shift: true,
    control: false,
    alt: false,
    meta: false,
};

impl Bench {
    /// Gives `event` to the field on `platform`, 10 ms after the last key.
    fn key(&mut self, platform: Platform, event: &KeyEvent) -> super::KeyUse {
        self.now += Duration::from_millis(10);
        self.field
            .key(event, self.now, platform, &mut self.fonts, &mut self.layouts)
    }

    /// Presses `key` with `modifiers` on `platform`.
    fn hit(&mut self, platform: Platform, key: Key, modifiers: Modifiers) -> super::KeyUse {
        self.key(platform, &press(key, modifiers, None))
    }

    /// Types each character of `text`, one key each, on `platform`.
    fn type_text(&mut self, platform: Platform, text: &str) {
        for letter in text.chars() {
            let key = if letter == ' ' {
                Key::Space
            } else {
                Key::Character(letter)
            };
            let typed = letter.to_string();
            self.key(platform, &press(key, Modifiers::NONE, Some(&typed)));
        }
    }

    /// The text.
    fn text(&self) -> &str {
        self.field.text()
    }

    /// Where the caret is, in bytes.
    fn caret(&self) -> usize {
        self.field.selection_bytes().1
    }
}

const PLATFORMS: [Platform; 2] = [Platform::Mac, Platform::Other];

/// Typing goes in at the caret, and Left moves it back one letter.
#[test]
fn typing_goes_in_at_the_caret() {
    for platform in PLATFORMS {
        let Some(mut bench) = field_in("", 300.0) else { return };
        bench.type_text(platform, "ab");
        bench.hit(platform, Key::Left, Modifiers::NONE);
        bench.type_text(platform, "x");
        assert_eq!(bench.text(), "axb");
        assert_eq!(bench.caret(), 2);
    }
}

/// Shift extends the selection, and typing replaces what is selected.
#[test]
fn shift_extends_and_typing_replaces_the_selection() {
    for platform in PLATFORMS {
        let Some(mut bench) = field_in("hello", 300.0) else {
            return;
        };
        bench.hit(platform, Key::Left, SHIFT);
        bench.hit(platform, Key::Left, SHIFT);
        assert_eq!(bench.field.selection_bytes(), (5, 3));
        bench.type_text(platform, "p");
        assert_eq!(bench.text(), "help");
    }
}

/// The word key is Alt on a Mac and Control elsewhere, and Shift with it
/// selects by word. On a Mac, Control with Left is a plain move. Where
/// Chinese or Thai words end depends on the `complex-scripts` feature, so
/// these words are Latin; the typesetter's tests cover the feature.
#[test]
fn word_movement_follows_the_platform() {
    for platform in PLATFORMS {
        let Some(mut bench) = field_in("one two abc", 300.0) else {
            return;
        };
        bench.hit(platform, Key::Left, word(platform, false));
        assert_eq!(bench.caret(), "one two ".len(), "{platform:?}");
        bench.hit(platform, Key::Left, word(platform, false));
        assert_eq!(bench.caret(), "one ".len(), "{platform:?}");
        bench.hit(platform, Key::Home, Modifiers::NONE);
        bench.hit(platform, Key::Right, word(platform, true));
        assert_eq!(bench.field.selection_bytes().0, 0);
        assert!(bench.caret() >= "one".len(), "{platform:?}");
    }
    let Some(mut bench) = field_in("one two", 300.0) else {
        return;
    };
    bench.hit(Platform::Mac, Key::Left, held(false, true, false, false));
    assert_eq!(bench.caret(), "one tw".len());
}

/// Home and End go to the start and the end, and Shift extends to there.
/// On a Mac, Command with Left and Right does the same.
#[test]
fn home_and_end_go_to_the_ends() {
    for platform in PLATFORMS {
        let Some(mut bench) = field_in("hello", 300.0) else {
            return;
        };
        bench.hit(platform, Key::Home, Modifiers::NONE);
        assert_eq!(bench.caret(), 0);
        bench.hit(platform, Key::End, SHIFT);
        assert_eq!(bench.field.selection_bytes(), (0, 5));
    }
    let Some(mut bench) = field_in("hello", 300.0) else {
        return;
    };
    bench.hit(Platform::Mac, Key::Left, shortcut(Platform::Mac, false));
    assert_eq!(bench.caret(), 0);
    bench.hit(Platform::Mac, Key::Right, shortcut(Platform::Mac, true));
    assert_eq!(bench.field.selection_bytes(), (0, 5));
}

/// Up and Down go to the start and the end on a Mac, as its one-line
/// fields do. Elsewhere the field takes them and does not move.
#[test]
fn up_and_down_move_on_a_mac_only() {
    let Some(mut bench) = field_in("hello", 300.0) else {
        return;
    };
    let up = bench.hit(Platform::Mac, Key::Up, Modifiers::NONE);
    assert!(up.taken);
    assert_eq!(bench.caret(), 0);
    bench.hit(Platform::Mac, Key::Down, Modifiers::NONE);
    assert_eq!(bench.caret(), 5);

    let Some(mut bench) = field_in("hello", 300.0) else {
        return;
    };
    let up = bench.hit(Platform::Other, Key::Up, Modifiers::NONE);
    assert!(up.taken);
    assert!(!up.redraw);
    assert_eq!(bench.caret(), 5);
}

/// Backspace and Delete take one letter, and with the word key one word.
#[test]
fn delete_by_letter_and_by_word() {
    for platform in PLATFORMS {
        let Some(mut bench) = field_in("one two", 300.0) else {
            return;
        };
        bench.hit(platform, Key::Backspace, Modifiers::NONE);
        assert_eq!(bench.text(), "one tw");
        bench.hit(platform, Key::Backspace, word(platform, false));
        assert_eq!(bench.text(), "one ");
        bench.hit(platform, Key::Home, Modifiers::NONE);
        bench.hit(platform, Key::Delete, Modifiers::NONE);
        assert_eq!(bench.text(), "ne ");
    }
}

/// Select all, copy, and cut, by the platform's shortcut key. Copy and cut
/// with nothing selected give the clipboard nothing, and cut changes
/// nothing.
#[test]
fn select_all_copy_and_cut() {
    for platform in PLATFORMS {
        let Some(mut bench) = field_in("abc", 300.0) else {
            return;
        };
        let copy = bench.hit(platform, Key::Character('c'), shortcut(platform, false));
        assert!(copy.taken);
        assert_eq!(copy.copy, None);
        let cut = bench.hit(platform, Key::Character('x'), shortcut(platform, false));
        assert_eq!((cut.copy, bench.text()), (None, "abc"));

        bench.hit(platform, Key::Character('a'), shortcut(platform, false));
        assert_eq!(bench.field.selection_bytes(), (0, 3));
        let copy = bench.hit(platform, Key::Character('c'), shortcut(platform, false));
        assert_eq!(copy.copy.as_deref(), Some("abc"));
        assert_eq!(bench.text(), "abc");
        let cut = bench.hit(platform, Key::Character('x'), shortcut(platform, false));
        assert_eq!(cut.copy.as_deref(), Some("abc"));
        assert!(cut.edited);
        assert_eq!(bench.text(), "");
    }
}

/// A typing run is one undo step; a pause of [`TYPING_PAUSE`] or a space
/// after a letter starts a new one. Redo puts it back, and a new edit
/// drops what could be redone.
#[test]
fn a_typing_run_is_one_undo_step() {
    for platform in PLATFORMS {
        let undo = shortcut(platform, false);
        let Some(mut bench) = field_in("", 300.0) else { return };
        bench.type_text(platform, "abc");
        assert_eq!(bench.field.steps(), (1, 0));
        bench.now += TYPING_PAUSE;
        bench.type_text(platform, "de");
        bench.type_text(platform, " fg");
        assert_eq!(bench.field.steps(), (3, 0));

        bench.hit(platform, Key::Character('z'), undo);
        assert_eq!(bench.text(), "abcde");
        bench.hit(platform, Key::Character('z'), undo);
        assert_eq!(bench.text(), "abc");
        bench.hit(platform, Key::Character('z'), shortcut(platform, true));
        assert_eq!(bench.text(), "abcde");
        bench.type_text(platform, "!");
        assert_eq!(bench.field.steps().1, 0);
    }
    let Some(mut bench) = field_in("", 300.0) else { return };
    bench.type_text(Platform::Other, "ab");
    bench.hit(Platform::Other, Key::Character('z'), shortcut(Platform::Other, false));
    bench.hit(Platform::Other, Key::Character('y'), shortcut(Platform::Other, false));
    assert_eq!(bench.text(), "ab");
}

/// A caret move ends the typing run, and a delete by word and typing over
/// a selection are each a step of their own.
#[test]
fn moves_and_word_deletes_end_a_step() {
    let platform = Platform::Mac;
    let Some(mut bench) = field_in("", 300.0) else { return };
    bench.type_text(platform, "ab");
    bench.hit(platform, Key::Left, Modifiers::NONE);
    bench.type_text(platform, "x");
    assert_eq!(bench.field.steps(), (2, 0));
    bench.hit(platform, Key::Backspace, word(platform, false));
    assert_eq!(bench.field.steps(), (3, 0));
    bench.type_text(platform, "cd");
    bench.hit(platform, Key::Left, SHIFT);
    bench.type_text(platform, "e");
    assert_eq!(
        bench.field.steps(),
        (5, 0),
        "typing over a selection is a step of its own"
    );
}

/// Enter commits and clears the undo steps. Tab, Escape, paste, and other
/// shortcuts are not the field's, and nor is Control typing on a Mac.
#[test]
fn enter_commits_and_other_keys_pass() {
    for platform in PLATFORMS {
        let Some(mut bench) = field_in("", 300.0) else { return };
        bench.type_text(platform, "ab");
        let enter = bench.hit(platform, Key::Enter, Modifiers::NONE);
        assert!(enter.committed);
        assert_eq!(bench.field.steps(), (0, 0));
        for (key, modifiers) in [
            (Key::Tab, Modifiers::NONE),
            (Key::Tab, SHIFT),
            (Key::Escape, Modifiers::NONE),
            (Key::Character('s'), shortcut(platform, false)),
        ] {
            assert_eq!(bench.hit(platform, key, modifiers), super::KeyUse::default(), "{key:?}");
        }
        // Paste is the field's own: it takes the key and asks for the
        // clipboard (section 6, rule 6).
        let paste = bench.hit(platform, Key::Character('v'), shortcut(platform, false));
        assert!(paste.taken && paste.paste, "{platform:?}");
    }
    let Some(mut bench) = field_in("", 300.0) else { return };
    let control = bench.key(
        Platform::Mac,
        &press(Key::Character('a'), held(false, true, false, false), Some("a")),
    );
    assert!(!control.taken);
}

/// Off a Mac, AltGr (Control and Alt together) types its letter.
#[test]
fn altgr_types_off_a_mac() {
    let Some(mut bench) = field_in("", 300.0) else { return };
    bench.key(
        Platform::Other,
        &press(Key::Character('q'), held(false, true, true, false), Some("@")),
    );
    assert_eq!(bench.text(), "@");
}

/// A text the app changes is taken: the selection is clamped to it and
/// the undo steps go.
#[test]
fn the_apps_text_is_taken() {
    let Some(mut bench) = field_in("", 300.0) else { return };
    bench.type_text(Platform::Mac, "hello");
    let Bench {
        field, fonts, layouts, ..
    } = &mut bench;
    field.take("hi", STYLE, Lines::One, 300.0, fonts, layouts);
    assert_eq!(field.text(), "hi");
    assert_eq!(field.selection_bytes(), (2, 2));
    assert_eq!(field.steps(), (0, 0));
}

/// A text wider than its box slides left to keep the caret inside, with
/// the margin to spare, and Home shows the start again.
#[test]
fn a_long_text_scrolls_to_keep_the_caret_in_view() {
    let Some(mut bench) = field_in("", 40.0) else { return };
    bench.type_text(Platform::Mac, "a long line of text");
    let caret = bench.field.caret().expect("a caret");
    assert!(bench.field.scroll() > 0.0);
    let shown = caret.x - bench.field.scroll();
    assert!((CARET_MARGIN..=40.0 - CARET_MARGIN + 0.01).contains(&shown), "{shown}");
    bench.hit(Platform::Mac, Key::Home, Modifiers::NONE);
    assert_eq!(bench.field.scroll(), 0.0);

    let Some(bench) = field_in("short", 300.0) else { return };
    assert_eq!(bench.field.scroll(), 0.0);
}

/// The caret the editor gives sits where the typesetter's layout of the
/// same text puts that place, at every cluster boundary, so the caret and
/// the selection line up with the drawn letters (section 8, rule 9). Thai
/// is in the list, set with whatever font the system falls back to.
#[test]
fn the_caret_lines_up_with_the_drawn_text_at_every_boundary() {
    for text in ["日本 abc", "Wavy fA", "สวัสดีครับ"] {
        let Some(mut bench) = field_in(text, 1000.0) else {
            return;
        };
        let mut typesetter = Typesetter::new();
        bench.hit(Platform::Mac, Key::Home, Modifiers::NONE);
        let mut seen = 0;
        loop {
            let index = bench.caret();
            let drawn = typesetter.layout(text, STYLE, None);
            let expected = parley::editing::Cursor::from_byte_index(drawn, index, parley::layout::Affinity::Downstream)
                .geometry(drawn, 1.0)
                .x0 as f32;
            let caret = bench.field.caret().expect("a caret").x;
            assert!(
                (caret - expected).abs() < 0.01,
                "{text:?} at {index}: {caret} against {expected}"
            );
            seen += 1;
            if index == text.len() {
                break;
            }
            bench.hit(Platform::Mac, Key::Right, Modifiers::NONE);
            assert!(bench.caret() > index, "{text:?}: stuck at {index}");
        }
        assert!(seen > 2);
    }
}

// --- Through the harness ---

/// What the test app does.
#[derive(Debug)]
enum Message {
    /// Focus the element with this UI ID.
    Focus(UiId),
    /// What the field said.
    Field(FieldEvent),
    /// Make the nodes from the last layout and keep them.
    Describe,
    /// Take input only from this layer up, as an open question does.
    Cover(Layer),
    /// Turn the field off, or back on.
    TurnOff(bool),
    /// Take the field out of the form, keeping the panel.
    Remove,
    /// Focus the first UI ID, then the second, in one update.
    FocusTwice(UiId, UiId),
    /// What a field reports to a screen reader changed, with the text the
    /// app held when it was asked.
    Changed(String),
    /// The app changes the text it holds.
    SetText(String),
    /// A screen reader's action the runtime left to the app.
    Reader(accesskit::Action),
    /// Logs whether a field has focus, as the context says.
    AskField,
}

/// An app with one field, keyed `form/name`, and a button after it. It
/// takes the field's edits when `keeps` is set.
#[derive(Default)]
struct Form {
    text: String,
    keeps: bool,
    heard: Vec<FieldEvent>,
    ids: IdTable,
    nodes: Option<AccessNodes>,
    /// The lowest layer that takes input, when not every layer does.
    covered: Option<Layer>,
    /// The field is turned off.
    off: bool,
    /// The field is out of the form.
    removed: bool,
    /// The text each notice carried, in order.
    noticed: Vec<String>,
    /// Every field message, notice, and screen reader action, in order.
    log: Vec<String>,
    /// How many times the runtime asked where Tab goes (`App::tab_from`).
    tabs: std::cell::Cell<u32>,
}

impl App for Form {
    type Message = Message;

    fn update(&mut self, message: Message, cx: &Context<'_, Message>) -> Effects<Message> {
        match message {
            Message::Focus(id) => return Effects::focus(id),
            Message::Field(event) => {
                if let (FieldEvent::Edited(text), true) = (&event, self.keeps) {
                    self.text.clone_from(text);
                }
                self.log.push(format!("{event:?}"));
                self.heard.push(event);
            }
            Message::Describe => self.nodes = Some(cx.access_nodes(&mut self.ids)),
            Message::Cover(layer) => self.covered = Some(layer),
            Message::FocusTwice(first, second) => return Effects::focus(first).and(Effects::focus(second)),
            Message::Changed(text) => {
                self.log.push("changed".to_owned());
                self.noticed.push(text);
            }
            Message::TurnOff(off) => self.off = off,
            Message::Remove => self.removed = true,
            Message::SetText(text) => self.text = text,
            Message::Reader(action) => self.log.push(format!("reader {action:?}")),
            Message::AskField => self.log.push(format!("field focus {}", cx.field_has_focus())),
        }
        Effects::none()
    }

    fn view(&self, view: &mut View) {
        view.panel_with(
            UiId::panel("form"),
            Layer::Panels,
            &(self.text.clone(), self.off, self.removed),
            form_panel,
        );
    }

    fn window(&self) -> WindowSpec {
        WindowSpec {
            title: "Form".to_owned(),
            min_size: None,
        }
    }

    fn input(&self) -> crate::app::InputState {
        crate::app::InputState {
            layer: self.covered.unwrap_or(Layer::Canvas),
            ..crate::app::InputState::default()
        }
    }

    /// Counts the question, and answers as the default does.
    fn tab_from(&self, _from: &UiId, next: Option<&UiId>, _back: bool) -> crate::app::TabStep<Message> {
        self.tabs.set(self.tabs.get() + 1);
        next.map_or(crate::app::TabStep::Leave, |next| crate::app::TabStep::To(next.clone()))
    }

    fn access_target(&self, node: accesskit::NodeId) -> Option<UiId> {
        self.ids.id(node.0).cloned()
    }

    fn access_changed(&self) -> Option<Message> {
        Some(Message::Changed(self.text.clone()))
    }

    fn accessibility_event(&self, event: &crate::event::AccessEvent) -> Option<Message> {
        match event {
            crate::event::AccessEvent::Action(request) => Some(Message::Reader(request.action)),
            _ => None,
        }
    }

    fn field_event(&self, id: &UiId, event: &FieldEvent) -> Option<Message> {
        assert_eq!(*id, name());
        Some(Message::Field(event.clone()))
    }
}

/// The form's panel: the field showing `text`, turned off if `off`, then
/// an OK button; with `removed`, the button alone.
fn form_panel((text, off, removed): &(String, bool, bool)) -> Element {
    let states = States {
        disabled: *off,
        ..States::default()
    };
    let field = Element::field(text.clone(), STYLE)
        .key("name")
        .width(Length::Fixed(120.0))
        .access(Access::new(Role::TextInput, "Name").states(states));
    let button = Element::text("OK", STYLE)
        .key("ok")
        .focusable()
        .access(Access::new(Role::Button, "OK").action(crate::access::Action::Click));
    if *removed {
        return Element::row([button]);
    }
    Element::row([field, button])
}

/// A routing target that counts the keys it hears, and the keys the
/// runtime kept from it.
#[derive(Default)]
struct Keys {
    heard: Vec<Key>,
    kept: Vec<Key>,
    /// Input method events it heard, and that were kept from it.
    ime_heard: usize,
    ime_kept: usize,
}

impl<M> Target<M> for Keys {
    fn input(&mut self, event: &Event, _pointer: Option<Point>, _now: Instant, _out: &mut Vec<M>) -> Routed {
        if let Event::Key(KeyEvent::Pressed { key, .. }) = event {
            self.heard.push(*key);
        }
        if let Event::Ime(_) = event {
            self.ime_heard += 1;
        }
        Routed::Passed
    }

    fn kept(&mut self, event: &Event) {
        if let Event::Key(KeyEvent::Pressed { key, .. }) = event {
            self.kept.push(*key);
        }
        if let Event::Ime(_) = event {
            self.ime_kept += 1;
        }
    }

    fn focus(&mut self, _target: &FocusTarget, _select: FocusSelection) {}

    fn scroll_into_view(&mut self, _id: &UiId) {}

    fn focused(&self) -> Option<UiId> {
        None
    }

    fn needs_redraw(&mut self, _updates: usize) -> bool {
        false
    }
}

/// The field's UI ID.
fn name() -> UiId {
    UiId::panel("form").with("name")
}

/// A harness showing a form with `text`, taking its edits if `keeps`, with
/// the field focused and laid out by the system's fonts. `None` with no
/// font.
fn form(text: &str, keeps: bool) -> Option<(Harness<Form, Keys>, Typesetter)> {
    let mut typesetter = Typesetter::new();
    if typesetter.measure("x", STYLE, None).width <= 0.0 {
        eprintln!("skipped: no system font");
        return None;
    }
    let app = Form {
        text: text.to_owned(),
        keeps,
        ..Form::default()
    };
    let window = WindowState::new(Size::new(400.0, 100.0));
    let mut harness = Harness::new(app, Keys::default(), window, Instant::now());
    harness.step(DriverEvent::Woken);
    harness.lay_out(&mut typesetter);
    harness.send(Message::Focus(name()));
    harness.lay_out(&mut typesetter);
    assert_eq!(harness.runtime().focused(), Some(&name()));
    Some((harness, typesetter))
}

/// A key press, as the driver sends it.
fn key_in(key: Key, modifiers: Modifiers, text: Option<&str>) -> DriverEvent {
    DriverEvent::Input {
        event: Event::Key(press(key, modifiers, text)),
        pointer: None,
    }
}

/// Typing `text` into the focused element, one key each.
fn type_in(harness: &mut Harness<Form, Keys>, text: &str) {
    for letter in text.chars() {
        let typed = letter.to_string();
        harness.step(key_in(Key::Character(letter), Modifiers::NONE, Some(&typed)));
    }
}

/// Each edit reaches the app as `Edited`, Enter as `Committed`, and
/// focus moving on as `Left`. The window losing the keys is not leaving.
#[test]
fn the_app_hears_edits_commits_and_leaving() {
    let Some((mut harness, mut text)) = form("", true) else {
        return;
    };
    type_in(&mut harness, "ab");
    harness.step(key_in(Key::Enter, Modifiers::NONE, None));
    harness.step(DriverEvent::Window(WindowEvent::Focused(false)));
    harness.step(DriverEvent::Window(WindowEvent::Focused(true)));
    assert_eq!(
        harness.app().heard,
        [
            FieldEvent::Edited("a".to_owned()),
            FieldEvent::Edited("ab".to_owned()),
            FieldEvent::Committed("ab".to_owned()),
        ]
    );
    harness.lay_out(&mut text);
    harness.step(key_in(Key::Tab, Modifiers::NONE, None));
    assert_eq!(harness.runtime().focused(), Some(&UiId::panel("form").with("ok")));
    assert_eq!(harness.app().heard.last(), Some(&FieldEvent::Left));
}

/// The text is the app's: an app that does not take an edit sees the
/// field go back to its own text at the next layout.
#[test]
fn an_edit_the_app_does_not_take_goes_back() {
    let Some((mut harness, mut text)) = form("keep", false) else {
        return;
    };
    type_in(&mut harness, "x");
    let field = |harness: &Harness<Form, Keys>| harness.runtime().field_text(&name()).unwrap_or_default().to_owned();
    assert_eq!(field(&harness), "keepx");
    harness.lay_out(&mut text);
    assert_eq!(field(&harness), "keep");
}

/// A key the field takes never reaches the routing target, so undo in a
/// field never undoes the patch, and Backspace or Delete with the shortcut
/// key never deletes a module. The target hears that the runtime kept it
/// (`Target::kept`), so a toolkit that saw the key first drops it too.
/// Escape and Save do reach it, and are not kept; paste is the field's.
#[test]
fn keys_the_field_takes_stop_there() {
    let Some((mut harness, _text)) = form("", true) else {
        return;
    };
    let platform = Platform::current();
    type_in(&mut harness, "ab");
    harness.step(key_in(Key::Character('z'), shortcut(platform, false), None));
    harness.step(key_in(Key::Left, Modifiers::NONE, None));
    for key in [Key::Backspace, Key::Delete] {
        harness.step(key_in(key, shortcut(platform, false), None));
        harness.step(key_in(key, shortcut(platform, true), None));
    }
    assert!(harness.runtime().target().heard.is_empty());
    assert_eq!(
        harness.runtime().target().kept,
        [
            Key::Character('a'),
            Key::Character('b'),
            Key::Character('z'),
            Key::Left,
            Key::Backspace,
            Key::Backspace,
            Key::Delete,
            Key::Delete,
        ]
    );
    harness.step(key_in(Key::Escape, Modifiers::NONE, None));
    harness.step(key_in(Key::Character('s'), shortcut(platform, false), None));
    assert_eq!(harness.runtime().target().heard, [Key::Escape, Key::Character('s')]);
    assert_eq!(harness.runtime().target().kept.len(), 8, "a key passed on is not kept");
    harness.step(key_in(Key::Character('v'), shortcut(platform, false), None));
    assert_eq!(harness.runtime().target().heard.len(), 2, "paste stays in the field");
    assert_eq!(harness.runtime().target().kept.last(), Some(&Key::Character('v')));
}

/// Copy writes the clipboard through the driver. With nothing selected it
/// leaves the clipboard alone.
#[test]
fn copy_writes_the_clipboard() {
    let Some((mut harness, _text)) = form("abc", true) else {
        return;
    };
    let platform = Platform::current();
    harness.driver_mut().set_clipboard(Some("before".to_owned()));
    harness.step(key_in(Key::Character('c'), shortcut(platform, false), None));
    assert_eq!(harness.driver().clipboard(), Some("before"));
    harness.step(key_in(Key::Character('a'), shortcut(platform, false), None));
    harness.step(key_in(Key::Character('c'), shortcut(platform, false), None));
    assert_eq!(harness.driver().clipboard(), Some("abc"));
}

/// A field's node is a text input named by its panel, with its text as it
/// is now as its value, and focus on it.
#[test]
fn the_node_says_the_text() {
    let Some((mut harness, mut text)) = form("hi", true) else {
        return;
    };
    type_in(&mut harness, "!");
    harness.lay_out(&mut text);
    harness.send(Message::Describe);
    let app = harness.app();
    let nodes = app.nodes.as_ref().expect("nodes");
    let (id, node) = nodes
        .nodes
        .iter()
        .find(|(_, node)| node.role() == KitRole::TextInput)
        .expect("a text input");
    assert_eq!(node.label(), Some("Name"));
    assert_eq!(node.value(), Some("hi!"));
    assert_eq!(nodes.focus, Some(*id));
}

/// A focused field paints its box, then its text, then its caret, in a
/// layer cut to its content box; with something selected, the selection
/// goes under the text and there is no caret. An unfocused field shows no
/// caret.
#[test]
fn a_field_paints_selection_text_and_caret_in_order() {
    let Some((mut harness, mut text)) = form("abc", true) else {
        return;
    };
    let paint = |harness: &mut Harness<Form, Keys>| {
        let mut recorder = Recorder::default();
        let (_, _, mut panels) = harness.parts();
        panels.paint(Layer::Panels, &mut recorder, &PANEL);
        recorder.painted
    };
    let caret = |painted: &[Painted]| {
        painted
            .iter()
            .any(|piece| matches!(piece, Painted::Quad(quad) if quad.fill == PANEL.caret))
    };
    let painted = paint(&mut harness);
    let at = painted
        .iter()
        .position(|piece| piece.as_text() == Some("abc"))
        .expect("the text");
    assert!(matches!(painted[at - 1], Painted::Begin(_)));
    assert!(matches!(&painted[at + 1], Painted::Quad(quad) if quad.fill == PANEL.caret));

    harness.step(key_in(Key::Character('a'), shortcut(Platform::current(), false), None));
    let painted = paint(&mut harness);
    let at = painted
        .iter()
        .position(|piece| piece.as_text() == Some("abc"))
        .expect("the text");
    assert!(matches!(&painted[at - 1], Painted::Quad(quad) if quad.fill == PANEL.selection));
    assert!(!caret(&painted));

    harness.step(key_in(Key::Tab, Modifiers::NONE, None));
    harness.lay_out(&mut text);
    assert!(!caret(&paint(&mut harness)));
}

/// The caret is snapped to whole physical pixels: at a scale of 1.5 its
/// left edge and width are whole numbers of physical pixels.
#[test]
fn the_caret_is_snapped_to_physical_pixels() {
    let Some((mut harness, mut text)) = form("abc", true) else {
        return;
    };
    harness.step(DriverEvent::Window(WindowEvent::ScaleChanged(1.5)));
    harness.lay_out(&mut text);
    let mut recorder = Recorder::default();
    let (_, _, mut panels) = harness.parts();
    panels.paint(Layer::Panels, &mut recorder, &PANEL);
    let caret = recorder
        .painted
        .iter()
        .find_map(|piece| match piece {
            Painted::Quad(quad) if quad.fill == PANEL.caret => Some(quad.rect),
            _ => None,
        })
        .expect("a caret");
    let whole = |value: f32| ((value * 1.5) - (value * 1.5).round()).abs() < 1e-3;
    assert!(whole(caret.x), "{caret:?}");
    assert!(whole(caret.width), "{caret:?}");
}

/// A field is a focus stop that needs a key, as any stop does, and a
/// turned-off field is no stop (section 6, rule 2).
#[test]
fn a_field_is_a_stop_with_a_key_unless_turned_off() {
    use crate::access::{Problem, States};
    use crate::layout::Layout;
    let mut layout = Layout::new(UiId::panel("p"));
    layout.allow_duplicates();
    let off = States {
        disabled: true,
        ..States::default()
    };
    layout.rebuild(Element::row([
        Element::field("", STYLE).access(Access::new(Role::TextInput, "Lost")),
        Element::field("", STYLE)
            .key("on")
            .access(Access::new(Role::TextInput, "On")),
        Element::field("", STYLE)
            .key("off")
            .access(Access::new(Role::TextInput, "Off").states(off)),
    ]));
    assert_eq!(layout.access_problems(), [(None, Problem::NoKey)]);
    let stops: Vec<bool> = layout.records().iter().map(|record| record.focusable).collect();
    assert_eq!(stops, [false, false, true, false]);
}

/// On a Mac, Command with Backspace deletes to the start and with Delete
/// to the end, each a step of its own, and Command with Up and Down goes
/// to the ends. Backspace and Delete with any other modifiers are taken
/// and do nothing, on both platforms, so the app never hears them.
#[test]
fn backspace_and_delete_with_any_modifier_stay_in_the_field() {
    let Some(mut bench) = field_in("one two", 300.0) else {
        return;
    };
    bench.hit(Platform::Mac, Key::Left, Modifiers::NONE);
    let gone = bench.hit(Platform::Mac, Key::Backspace, shortcut(Platform::Mac, false));
    assert!(gone.taken && gone.edited);
    assert_eq!(bench.text(), "o");
    assert_eq!(bench.field.steps(), (1, 0));
    bench.hit(Platform::Mac, Key::Home, Modifiers::NONE);
    bench.hit(Platform::Mac, Key::Delete, shortcut(Platform::Mac, false));
    assert_eq!(bench.text(), "");
    assert_eq!(bench.field.steps(), (2, 0));

    let Some(mut bench) = field_in("abc", 300.0) else {
        return;
    };
    let up = bench.hit(Platform::Mac, Key::Up, shortcut(Platform::Mac, false));
    assert!(up.taken);
    assert_eq!(bench.caret(), 0);
    bench.hit(Platform::Mac, Key::Down, shortcut(Platform::Mac, false));
    assert_eq!(bench.caret(), 3);

    for (platform, modifiers) in [
        (Platform::Other, held(false, false, true, false)),
        (Platform::Other, held(false, false, false, true)),
        (Platform::Other, held(true, false, true, false)),
        (Platform::Mac, held(false, true, false, false)),
        (Platform::Mac, held(true, true, false, false)),
    ] {
        let Some(mut bench) = field_in("abc", 300.0) else {
            return;
        };
        bench.hit(platform, Key::Left, Modifiers::NONE);
        for key in [Key::Backspace, Key::Delete] {
            let used = bench.hit(platform, key, modifiers);
            assert!(used.taken, "{platform:?} {key:?} {modifiers:?}");
            assert!(!used.edited, "{platform:?} {key:?} {modifiers:?}");
            assert_eq!(bench.text(), "abc", "{platform:?} {key:?} {modifiers:?}");
        }
    }
}

/// Backspace and Delete with Shift alone delete one character, as plain,
/// on both platforms, so a slip while typing capitals still deletes.
#[test]
fn shift_backspace_and_delete_delete_one_character() {
    for platform in PLATFORMS {
        let Some(mut bench) = field_in("loom", 300.0) else {
            return;
        };
        bench.hit(platform, Key::Left, Modifiers::NONE);
        bench.hit(platform, Key::Backspace, SHIFT);
        assert_eq!(bench.text(), "lom", "{platform:?}");
        bench.hit(platform, Key::Delete, SHIFT);
        assert_eq!(bench.text(), "lo", "{platform:?}");
    }
}

/// Space types the text the platform gives with it, such as the no-break
/// space of Option+Space on a Mac, and a plain space when it gives none.
#[test]
fn space_types_the_text_it_comes_with() {
    let Some(mut bench) = field_in("", 300.0) else {
        return;
    };
    let option = held(false, false, true, false);
    bench.key(Platform::Mac, &press(Key::Space, option, Some("\u{a0}")));
    bench.key(Platform::Mac, &press(Key::Space, Modifiers::NONE, None));
    assert_eq!(bench.text(), "\u{a0} ");
}

/// Typing over a selection is an undo step of its own: the letters typed
/// right after it are the next step, so undo takes them away first.
#[test]
fn typing_over_a_selection_is_its_own_step() {
    for platform in PLATFORMS {
        let Some(mut bench) = field_in("abc", 300.0) else {
            return;
        };
        bench.hit(platform, Key::Character('a'), shortcut(platform, false));
        bench.type_text(platform, "xy");
        assert_eq!(bench.text(), "xy");
        bench.hit(platform, Key::Character('z'), shortcut(platform, false));
        assert_eq!(bench.text(), "x", "{platform:?}");
        bench.hit(platform, Key::Character('z'), shortcut(platform, false));
        assert_eq!(bench.text(), "abc", "{platform:?}");
    }
}

/// On a Mac, Control+Space is the system's input-source switch, not a
/// space.
#[test]
fn control_space_types_nothing_on_a_mac() {
    let Some(mut bench) = field_in("", 300.0) else { return };
    let used = bench.key(
        Platform::Mac,
        &press(Key::Space, held(false, true, false, false), Some(" ")),
    );
    assert!(!used.taken);
    assert_eq!(bench.text(), "");
}

/// A delete with nothing to delete leaves no undo step, and keeps what
/// could be redone.
#[test]
fn a_delete_that_deletes_nothing_is_no_step() {
    for (platform, modifiers) in [
        (Platform::Mac, shortcut(Platform::Mac, false)),
        (Platform::Mac, word(Platform::Mac, false)),
        (Platform::Other, Modifiers::NONE),
    ] {
        let Some(mut bench) = field_in("", 300.0) else { return };
        bench.type_text(platform, "ab");
        bench.hit(platform, Key::Character('z'), shortcut(platform, false));
        bench.hit(platform, Key::Backspace, modifiers);
        assert_eq!(bench.field.steps(), (0, 1), "{platform:?} {modifiers:?}");
    }
}

/// A Backspace that deletes nothing opens no run: a Delete straight after
/// it is a step of its own, and undo gives back exactly its letter.
#[test]
fn a_delete_that_deletes_nothing_opens_no_run() {
    let platform = Platform::Mac;
    let Some(mut bench) = field_in("ab", 300.0) else { return };
    bench.hit(platform, Key::Home, Modifiers::NONE);
    bench.hit(platform, Key::Backspace, Modifiers::NONE);
    bench.hit(platform, Key::Delete, Modifiers::NONE);
    assert_eq!(bench.text(), "b");
    bench.hit(platform, Key::Character('z'), shortcut(platform, false));
    assert_eq!(bench.text(), "ab");
    assert_eq!(bench.field.steps(), (0, 1));
}

/// A field in a layer below the one taking input, such as a panel under
/// an open question, edits nothing: its keys go on to the target.
#[test]
fn a_covered_field_takes_no_keys() {
    let Some((mut harness, mut text)) = form("ab", true) else {
        return;
    };
    harness.send(Message::Cover(Layer::Question));
    harness.lay_out(&mut text);
    harness.step(key_in(Key::Character('x'), Modifiers::NONE, Some("x")));
    harness.step(key_in(Key::Backspace, Modifiers::NONE, None));
    harness.lay_out(&mut text);
    assert_eq!(harness.app().text, "ab");
    assert_eq!(harness.runtime().target().heard, [Key::Character('x'), Key::Backspace]);
    assert!(harness.runtime().target().kept.is_empty());
}

/// Focus that leaves a field and comes back within one turn still leaves
/// it: the app hears `Left` on the next turn, and the field's undo steps
/// are gone (section 8, rule 3).
#[test]
fn focus_away_and_back_in_one_turn_leaves_the_field() {
    let Some((mut harness, mut text)) = form("", true) else {
        return;
    };
    type_in(&mut harness, "ab");
    harness.lay_out(&mut text);
    let ok = UiId::panel("form").with("ok");
    harness
        .runtime()
        .waker()
        .send_all([Message::Focus(ok), Message::Focus(name())]);
    harness.step(DriverEvent::Woken);
    harness.step(DriverEvent::Woken);
    assert_eq!(harness.runtime().focused(), Some(&name()));
    assert_eq!(harness.app().heard.last(), Some(&FieldEvent::Left));
    harness.step(key_in(Key::Character('z'), shortcut(Platform::current(), false), None));
    harness.lay_out(&mut text);
    assert_eq!(harness.app().text, "ab", "no undo step was left");
}

/// Two focus effects of one update, away from the field and back, still
/// leave it: the runtime looks after each effect, not once after the
/// update.
#[test]
fn focus_away_and_back_in_one_update_leaves_the_field() {
    let Some((mut harness, mut text)) = form("", true) else {
        return;
    };
    type_in(&mut harness, "ab");
    harness.lay_out(&mut text);
    harness.send(Message::FocusTwice(UiId::panel("form").with("ok"), name()));
    harness.step(DriverEvent::Woken);
    assert_eq!(harness.runtime().focused(), Some(&name()));
    assert_eq!(harness.app().heard.last(), Some(&FieldEvent::Left));
    harness.step(key_in(Key::Character('z'), shortcut(Platform::current(), false), None));
    harness.lay_out(&mut text);
    assert_eq!(harness.app().text, "ab", "no undo step was left");
}

// --- For a screen reader (section 7, rules 7 and 10) ---

/// A run's characters are the editor's clusters: their lengths add up to
/// the text, a three-byte ideograph is one character, and an empty field
/// has one empty run.
#[test]
fn a_run_has_one_character_per_cluster() {
    let Some(bench) = field_in("日本a", 300.0) else {
        return;
    };
    let runs = bench.field.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text, "日本a");
    assert_eq!(runs[0].lengths, [3, 3, 1]);
    let Some(bench) = field_in("", 300.0) else { return };
    let empty = bench.field.runs();
    assert_eq!(empty.len(), 1, "an empty field has one run");
    assert!(empty[0].text.is_empty() && empty[0].lengths.is_empty());
}

/// Positions go up along the line, and every width is positive.
#[test]
fn a_runs_positions_go_up_and_its_widths_are_positive() {
    let Some(bench) = field_in("Wavy fA 日本", 300.0) else {
        return;
    };
    let run = &bench.field.runs()[0];
    assert!(
        run.positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{:?}",
        run.positions
    );
    assert!(run.widths.iter().all(|width| *width > 0.0), "{:?}", run.widths);
    assert_eq!(run.positions[0], 0.0);
}

/// A run's word starts are where moving left by word stops, so a screen
/// reader and the keyboard agree on where words begin. (Moving right stops
/// at word ends.) The space after a word belongs to it, as AccessKit says.
#[test]
fn word_starts_are_where_word_movement_stops() {
    let mut texts = vec!["one  two", "a-b c", "one two three"];
    if cfg!(feature = "complex-scripts") {
        texts.push("日本 abc");
    }
    for text in texts {
        let Some(mut bench) = field_in(text, 300.0) else { return };
        let mut stops = Vec::new();
        loop {
            bench.hit(Platform::Mac, Key::Left, word(Platform::Mac, false));
            let at = bench.caret();
            if stops.last() == Some(&at) {
                break;
            }
            stops.push(at);
            if at == 0 {
                break;
            }
        }
        stops.reverse();
        let run = &bench.field.runs()[0];
        let starts: Vec<usize> = run
            .word_starts
            .iter()
            .map(|&start| {
                run.lengths[..usize::from(start)]
                    .iter()
                    .map(|&length| usize::from(length))
                    .sum()
            })
            .collect();
        assert_eq!(starts, stops, "{text:?}");
    }
}

/// Right-to-left text, alone or mixed with Latin, builds runs whose
/// lengths still add up to the text.
#[test]
fn right_to_left_text_builds_runs() {
    for text in ["שלום", "abc שלום def"] {
        let Some(bench) = field_in(text, 300.0) else { return };
        let total: usize = bench
            .field
            .runs()
            .iter()
            .flat_map(|run| run.lengths.iter().map(|&length| usize::from(length)))
            .sum();
        assert_eq!(total, text.len(), "{text:?}");
    }
}

/// A text longer than one run splits into runs of at most 255 characters,
/// and the caret at the end is at the last run's length.
#[test]
fn a_long_text_splits_into_runs() {
    let text = "a".repeat(300);
    let Some(bench) = field_in(&text, 3000.0) else { return };
    let runs = bench.field.runs();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].lengths.len(), super::RUN_CHARACTERS);
    assert_eq!(runs[0].text.len() + runs[1].text.len(), 300);
    assert_eq!(bench.field.reader_selection(), ((1, 45), (1, 45)));
}

/// The selection is given as characters in a run: after Shift+Left over
/// an ideograph, it runs from the end back one character.
#[test]
fn the_selection_is_given_in_characters() {
    let Some(mut bench) = field_in("日本a", 300.0) else {
        return;
    };
    assert_eq!(bench.field.reader_selection(), ((0, 3), (0, 3)));
    bench.hit(Platform::Mac, Key::Left, SHIFT);
    bench.hit(Platform::Mac, Key::Left, SHIFT);
    assert_eq!(bench.field.reader_selection(), ((0, 3), (0, 1)));
}

/// SetValue takes the text as one step with the caret at the end;
/// ReplaceSelection never joins the typing before it; Select picks by
/// character, and a place past the text does nothing.
#[test]
fn a_screen_readers_actions_edit_and_select() {
    use super::FieldAction;
    let Some(mut bench) = field_in("", 300.0) else { return };
    let Bench {
        field,
        fonts,
        layouts,
        now,
    } = &mut bench;
    let set = field.act(FieldAction::SetValue("日本".to_owned()), *now, fonts, layouts);
    assert!(set.edited && set.changed);
    assert_eq!(
        (field.text(), field.selection_bytes(), field.steps()),
        ("日本", (6, 6), (1, 0))
    );

    let select = FieldAction::Select {
        anchor: (0, 0),
        focus: (0, 1),
    };
    assert!(field.act(select, *now, fonts, layouts).changed);
    assert_eq!(field.selection_bytes(), (0, 3));
    let stale = FieldAction::Select {
        anchor: (0, 0),
        focus: (0, 9),
    };
    assert!(!field.act(stale, *now, fonts, layouts).changed);

    let Some(mut bench) = field_in("", 300.0) else { return };
    bench.type_text(Platform::Mac, "ab");
    let Bench {
        field,
        fonts,
        layouts,
        now,
    } = &mut bench;
    field.act(FieldAction::ReplaceSelection("c".to_owned()), *now, fonts, layouts);
    assert_eq!(field.text(), "abc");
    bench.hit(Platform::Mac, Key::Character('z'), shortcut(Platform::Mac, false));
    assert_eq!(bench.text(), "ab");
}

/// The form's nodes, made after a layout: the field's node ID and node,
/// and its runs' IDs.
fn describe(harness: &mut Harness<Form, Keys>, text: &mut Typesetter) -> (accesskit::NodeId, accesskit::Node) {
    harness.lay_out(text);
    harness.send(Message::Describe);
    let nodes = harness.app().nodes.as_ref().expect("nodes");
    let (id, node) = nodes
        .nodes
        .iter()
        .find(|(_, node)| node.role() == KitRole::TextInput)
        .expect("a text input");
    (*id, node.clone())
}

/// A screen reader's action on `target`, as the driver sends it.
fn reader(action: accesskit::Action, target: accesskit::NodeId, data: Option<accesskit::ActionData>) -> DriverEvent {
    DriverEvent::Accessibility(crate::event::AccessEvent::Action(accesskit::ActionRequest {
        action,
        target_tree: accesskit::TreeId::ROOT,
        target_node: target,
        data,
    }))
}

/// The field's node holds its runs as children, names them in its text
/// selection, and offers the three text actions with Focus.
#[test]
fn the_field_node_holds_its_runs_and_selection() {
    let Some((mut harness, mut text)) = form("hi", true) else {
        return;
    };
    let (_, node) = describe(&mut harness, &mut text);
    let runs = node.children();
    assert_eq!(runs.len(), 1);
    let selection = node.text_selection().expect("a selection");
    assert_eq!(selection.focus.node, runs[0]);
    assert_eq!(selection.focus.character_index, 2);
    for action in [
        accesskit::Action::Focus,
        accesskit::Action::SetValue,
        accesskit::Action::SetTextSelection,
        accesskit::Action::ReplaceSelectedText,
    ] {
        assert!(node.supports_action(action), "{action:?}");
    }
    let nodes = harness.app().nodes.as_ref().expect("nodes");
    let run = &nodes.nodes.iter().find(|(id, _)| *id == runs[0]).expect("the run").1;
    assert_eq!(run.role(), KitRole::TextRun);
    assert_eq!(run.value(), Some("hi"));
    assert_eq!(run.character_lengths(), [1, 1]);
}

/// A key that moves the caret gives one notice, after the field's own
/// message for an edit; a key that changes nothing, and turns with nothing
/// to do, give none.
#[test]
fn what_the_tree_reports_changing_gives_one_notice() {
    let Some((mut harness, _text)) = form("abc", true) else {
        return;
    };
    let start = harness.app().log.len();
    harness.step(key_in(Key::Left, Modifiers::NONE, None));
    assert_eq!(harness.app().log[start..], ["changed"]);
    harness.step(key_in(Key::Character('x'), Modifiers::NONE, Some("x")));
    assert_eq!(harness.app().log[start + 1..], ["Edited(\"abxc\")", "changed"]);
    let quiet = harness.app().log.len();
    harness.step(key_in(Key::End, Modifiers::NONE, None));
    harness.step(key_in(Key::End, Modifiers::NONE, None));
    harness.step(DriverEvent::Woken);
    harness.step(DriverEvent::Woken);
    assert_eq!(harness.app().log[quiet..], ["changed"], "only the first End moved");
}

/// A text the app changes reaches the field at the next layout, and the
/// clamped caret gives one notice on the turn after.
#[test]
fn a_change_found_in_layout_gives_a_notice_next_turn() {
    let Some((mut harness, mut text)) = form("abcdef", true) else {
        return;
    };
    harness.send(Message::SetText("ab".to_owned()));
    harness.lay_out(&mut text);
    let start = harness.app().log.len();
    harness.step(DriverEvent::Woken);
    assert_eq!(harness.app().log[start..], ["changed"]);
    harness.step(DriverEvent::Woken);
    assert_eq!(harness.app().log.len(), start + 1);
}

/// A screen reader's SetTextSelection on the field selects by character,
/// without moving focus to it, and gives one notice; SetValue edits and
/// reaches the app as `Edited`. Focus, and actions on other nodes, still
/// go to the app.
#[test]
fn a_screen_readers_text_actions_are_the_fields() {
    use accesskit::{Action, ActionData, TextPosition, TextSelection};
    let Some((mut harness, mut text)) = form("abc", true) else {
        return;
    };
    let (field, node) = describe(&mut harness, &mut text);
    let run = node.children()[0];
    harness.step(key_in(Key::Tab, Modifiers::NONE, None));
    harness.lay_out(&mut text);
    let ok = UiId::panel("form").with("ok");
    assert_eq!(harness.runtime().focused(), Some(&ok));

    let start = harness.app().log.len();
    let position = |character_index| TextPosition {
        node: run,
        character_index,
    };
    let selection = TextSelection {
        anchor: position(0),
        focus: position(2),
    };
    harness.step(reader(
        Action::SetTextSelection,
        field,
        Some(ActionData::SetTextSelection(selection)),
    ));
    assert_eq!(harness.runtime().focused(), Some(&ok), "only Focus moves focus");
    assert_eq!(harness.app().log[start..], ["changed"]);

    harness.step(reader(Action::SetValue, field, Some(ActionData::Value("new".into()))));
    assert_eq!(harness.app().log[start + 1..], ["Edited(\"new\")", "changed"]);
    harness.step(reader(Action::SetValue, field, None));
    assert_eq!(harness.app().log.len(), start + 3, "no data, no change");

    harness.step(reader(Action::Focus, field, None));
    assert_eq!(harness.app().log.last().map(String::as_str), Some("reader Focus"));
}

/// A cluster longer than one byte can count, a letter with hundreds of
/// combining marks, splits into pieces: the lengths still add up to the
/// text, none is zero, and a caret after the last letter is at the end.
#[test]
fn a_cluster_longer_than_255_bytes_splits_into_pieces() {
    let text = format!("a{}b", "\u{301}".repeat(150));
    let Some(mut bench) = field_in(&text, 3000.0) else {
        return;
    };
    bench.hit(Platform::Mac, Key::End, Modifiers::NONE);
    let runs = bench.field.runs();
    let lengths: Vec<u8> = runs.iter().flat_map(|run| run.lengths.iter().copied()).collect();
    assert_eq!(
        lengths.iter().map(|&length| usize::from(length)).sum::<usize>(),
        text.len()
    );
    assert!(lengths.iter().all(|&length| length > 0));
    assert!(lengths.len() >= 3, "{lengths:?}");
    let positions = &runs[0].positions;
    assert!(positions.windows(2).all(|pair| pair[0] <= pair[1]), "{positions:?}");
    // The pieces after the first stand at the cluster's trailing edge,
    // with no width: no caret stands inside a cluster, and the one after
    // the last piece reports the cluster's end.
    assert_eq!(positions[1], positions[0] + runs[0].widths[0], "{positions:?}");
    assert_eq!(runs[0].widths[1], 0.0);
    let ((_, _), (run, index)) = bench.field.reader_selection();
    assert_eq!((run, index), (runs.len() - 1, runs[runs.len() - 1].lengths.len()));
}

/// At a scale of 2, a run's bounds and its character positions and widths
/// are in the same physical pixels as the field node's bounds.
#[test]
fn a_runs_units_match_the_field_nodes_at_scale_two() {
    let Some((mut harness, mut text)) = form("ab", true) else {
        return;
    };
    harness.step(DriverEvent::Window(WindowEvent::ScaleChanged(2.0)));
    let (_, field) = describe(&mut harness, &mut text);
    let nodes = harness.app().nodes.as_ref().expect("nodes");
    let run = &nodes
        .nodes
        .iter()
        .find(|(id, _)| *id == field.children()[0])
        .expect("the run")
        .1;
    let field_bounds = field.bounds().expect("field bounds");
    let run_bounds = run.bounds().expect("run bounds");
    let widths = run.character_widths().expect("widths");
    let positions = run.character_positions().expect("positions");
    let last = positions.len() - 1;
    let ink = f64::from(positions[last] + widths[last]);
    assert!(
        (ink - (run_bounds.x1 - run_bounds.x0)).abs() < 0.01,
        "{ink} against {run_bounds:?}"
    );
    let logical = bench_width("ab");
    assert!(
        (ink - 2.0 * f64::from(logical)).abs() < 0.01,
        "{ink} against 2 x {logical}"
    );
    assert!(run_bounds.x0 >= field_bounds.x0 && run_bounds.x1 <= field_bounds.x1 + 0.01);
}

/// The width of `text` on one line, in logical pixels.
fn bench_width(text: &str) -> f32 {
    field_in(text, 3000.0).map_or(0.0, |bench| bench.field.text_width())
}

/// While a layer above the field takes input, such as an open question,
/// the field takes no screen reader text action (section 5, rule 3):
/// SetTextSelection, SetValue, and ReplaceSelectedText leave its text and
/// selection as they were, with no message and no notice, and nothing goes
/// on to the app. Once the cover is gone, the same three work.
#[test]
fn a_covered_field_takes_no_text_actions() {
    use accesskit::{Action, ActionData, TextPosition, TextSelection};
    let Some((mut harness, mut text)) = form("abc", true) else {
        return;
    };
    let (field, node) = describe(&mut harness, &mut text);
    let run = node.children()[0];
    let selection = TextSelection {
        anchor: TextPosition {
            node: run,
            character_index: 0,
        },
        focus: TextPosition {
            node: run,
            character_index: 2,
        },
    };
    let actions = [
        (Action::SetTextSelection, ActionData::SetTextSelection(selection)),
        (Action::SetValue, ActionData::Value("new".into())),
        (Action::ReplaceSelectedText, ActionData::Value("x".into())),
    ];

    harness.send(Message::Cover(Layer::Question));
    harness.lay_out(&mut text);
    let start = harness.app().log.len();
    for (action, data) in actions.clone() {
        harness.step(reader(action, field, Some(data)));
        harness.lay_out(&mut text);
    }
    let (_, after) = describe(&mut harness, &mut text);
    assert_eq!(after.text_selection(), node.text_selection(), "the selection stayed");
    assert_eq!(harness.app().text, "abc");
    assert_eq!(harness.app().log.len(), start, "{:?}", &harness.app().log[start..]);

    harness.send(Message::Cover(Layer::Canvas));
    harness.lay_out(&mut text);
    for (action, data) in actions {
        harness.step(reader(action, field, Some(data)));
        harness.lay_out(&mut text);
    }
    assert_eq!(harness.app().text, "newx", "SetValue, then the replacement at its end");
    assert_eq!(
        harness.app().log[start..],
        ["changed", "Edited(\"new\")", "changed", "Edited(\"newx\")", "changed"]
    );
}

/// A screen reader's text is made one line, as the field shows one: each
/// line break becomes a space and a break at the end goes, for SetValue
/// and ReplaceSelectedText alike. An empty text still clears.
#[test]
fn a_screen_readers_text_is_one_line() {
    use super::{FieldAction, one_line};
    assert_eq!(one_line("one\r\ntwo\n"), "one two");
    assert_eq!(one_line("a\rb"), "a b");
    assert_eq!(one_line("cell\tcell"), "cell cell", "a tab separates words");
    assert_eq!(one_line("a\u{7}b"), "ab", "other control characters go");
    assert_eq!(
        one_line("a\u{2028}b\u{2029}c\u{2029}"),
        "a b c",
        "line and paragraph separators"
    );
    assert_eq!(one_line(""), "");
    let Some(mut bench) = field_in("", 300.0) else { return };
    let Bench {
        field,
        fonts,
        layouts,
        now,
    } = &mut bench;
    field.act(FieldAction::SetValue("A\nB".to_owned()), *now, fonts, layouts);
    assert_eq!(field.text(), "A B");
    field.act(
        FieldAction::ReplaceSelection("\nC\r\n".to_owned()),
        *now,
        fonts,
        layouts,
    );
    assert_eq!(field.text(), "A B C");
    field.act(FieldAction::SetValue(String::new()), *now, fonts, layouts);
    assert_eq!(field.text(), "");
}

/// A selection's ends are checked against the run they name: past a run's
/// length, or in a run the field does not have, the request is stale and
/// does nothing. A run's length is its end, the next run's start.
#[test]
fn a_selection_end_must_be_inside_its_run() {
    use super::FieldAction;
    let text = "x".repeat(300);
    let Some(mut bench) = field_in(&text, 3000.0) else {
        return;
    };
    let Bench {
        field,
        fonts,
        layouts,
        now,
    } = &mut bench;
    let select = |anchor, focus| FieldAction::Select { anchor, focus };
    assert!(!field.act(select((0, 0), (0, 256)), *now, fonts, layouts).changed);
    assert!(!field.act(select((0, 0), (2, 0)), *now, fonts, layouts).changed);
    assert!(field.act(select((0, 0), (0, 255)), *now, fonts, layouts).changed);
    assert_eq!(field.selection_bytes(), (0, 255));
    field.act(select((1, 45), (1, 45)), *now, fonts, layouts);
    assert_eq!(field.selection_bytes(), (300, 300), "the last run's end");
}

/// A stale selection leaves the typing run as it was, so the letter typed
/// after it joins the same undo step.
#[test]
fn a_stale_selection_keeps_the_typing_run() {
    use super::FieldAction;
    let Some(mut bench) = field_in("", 300.0) else { return };
    bench.type_text(Platform::Mac, "ab");
    let stale = FieldAction::Select {
        anchor: (0, 0),
        focus: (0, 9),
    };
    let use_ = bench.field.act(stale, bench.now, &mut bench.fonts, &mut bench.layouts);
    assert_eq!(use_, super::ActUse::default());
    bench.type_text(Platform::Mac, "c");
    bench.hit(Platform::Mac, Key::Character('z'), shortcut(Platform::Mac, false));
    assert_eq!(bench.text(), "", "one step for abc");
}

/// Selecting a piece of a cluster split for AccessKit selects the whole
/// cluster, since the editor selects whole clusters only, so a replacement
/// after it replaces the cluster rather than inserting beside it.
#[test]
fn selecting_a_piece_of_a_split_cluster_selects_the_cluster() {
    use super::FieldAction;
    let text = format!("a{}b", "\u{301}".repeat(150));
    let Some(mut bench) = field_in(&text, 3000.0) else {
        return;
    };
    let Bench {
        field,
        fonts,
        layouts,
        now,
    } = &mut bench;
    let cluster = text.len() - 1;
    let select = FieldAction::Select {
        anchor: (0, 0),
        focus: (0, 1),
    };
    field.act(select, *now, fonts, layouts);
    assert_eq!(field.selection_bytes(), (0, cluster), "the whole cluster");
    field.act(FieldAction::ReplaceSelection("x".to_owned()), *now, fonts, layouts);
    assert_eq!(field.text(), "xb");
}

/// A turned-off field takes no screen reader text action (rule 22): its
/// node offers none, and a request that comes anyway changes nothing and
/// gives no message. Turned back on, it takes them again.
#[test]
fn a_turned_off_field_takes_no_text_actions() {
    use accesskit::{Action, ActionData};
    let Some((mut harness, mut text)) = form("abc", true) else {
        return;
    };
    let (field, _) = describe(&mut harness, &mut text);
    harness.send(Message::TurnOff(true));
    harness.lay_out(&mut text);
    let (_, off_node) = describe(&mut harness, &mut text);
    for action in [Action::SetValue, Action::SetTextSelection, Action::ReplaceSelectedText] {
        assert!(!off_node.supports_action(action), "{action:?} offered while off");
    }
    // Focus leaves the field as it turns off, and the app hears Left.
    harness.step(DriverEvent::Woken);
    assert_eq!(harness.app().log.last().map(String::as_str), Some("Left"));
    let start = harness.app().log.len();
    harness.step(reader(Action::SetValue, field, Some(ActionData::Value("new".into()))));
    harness.lay_out(&mut text);
    assert_eq!(harness.app().text, "abc");
    assert_eq!(harness.app().log.len(), start, "{:?}", &harness.app().log[start..]);
    harness.send(Message::TurnOff(false));
    harness.lay_out(&mut text);
    harness.step(reader(Action::SetValue, field, Some(ActionData::Value("new".into()))));
    assert_eq!(harness.app().text, "new");
}

/// The notice is asked for after the turn's updates, so an app that puts
/// its text in the notice puts the new text there, not the old.
#[test]
fn the_notice_is_built_after_the_turns_updates() {
    use accesskit::{Action, ActionData};
    let Some((mut harness, mut text)) = form("abc", true) else {
        return;
    };
    let (field, _) = describe(&mut harness, &mut text);
    harness.step(reader(Action::SetValue, field, Some(ActionData::Value("new".into()))));
    assert_eq!(harness.app().noticed.last().map(String::as_str), Some("new"));
}

/// A run is built in one pass over the line, not one per cluster: for a
/// field of 1,000 characters, building its runs visits each cluster a fixed
/// number of times (once for its place, once in text order), so the cost
/// grows with the text, not with its square. The count covers this
/// crate's own loops only and cannot see inside Parley; that no call per
/// cluster walks the line (as `Cluster::visual_offset` does) is shown by
/// the code, which no longer makes one.
#[test]
fn runs_are_built_in_linear_time() {
    let text = "x".repeat(1000);
    let Some(bench) = field_in(&text, 30000.0) else {
        return;
    };
    super::VISITS.with(|visits| visits.set(0));
    let runs = bench.field.runs();
    let visits = super::VISITS.with(std::cell::Cell::get);
    assert_eq!(runs.iter().map(|run| run.lengths.len()).sum::<usize>(), 1000);
    assert!(visits <= 2 * 1000 + 2, "{visits} visits for 1000 clusters");
}

/// A request naming a run far past the field's, as a made-up or stale
/// node could, is stale and does nothing: the run is checked before it is
/// multiplied, so a huge index cannot overflow.
#[test]
fn a_huge_run_index_is_stale() {
    use super::FieldAction;
    let Some(mut bench) = field_in("abc", 300.0) else {
        return;
    };
    let Bench {
        field,
        fonts,
        layouts,
        now,
    } = &mut bench;
    let select = FieldAction::Select {
        anchor: (usize::MAX, 0),
        focus: (usize::MAX, usize::MAX),
    };
    assert!(!field.act(select, *now, fonts, layouts).changed);
}

/// A caret placed inside a split cluster stays a caret, at the cluster's
/// start, so a replacement after it goes in before the cluster rather than
/// replacing it.
#[test]
fn a_caret_inside_a_split_cluster_stays_a_caret() {
    use super::FieldAction;
    let text = format!("a{}b", "\u{301}".repeat(150));
    let Some(mut bench) = field_in(&text, 3000.0) else {
        return;
    };
    let Bench {
        field,
        fonts,
        layouts,
        now,
    } = &mut bench;
    let caret = FieldAction::Select {
        anchor: (0, 1),
        focus: (0, 1),
    };
    field.act(caret, *now, fonts, layouts);
    assert_eq!(field.selection_bytes(), (0, 0));
    field.act(FieldAction::ReplaceSelection("x".to_owned()), *now, fonts, layouts);
    assert_eq!(field.text(), format!("x{text}"));
}

/// A field that ends in a split cluster reports a caret after it at the
/// cluster's end: the last piece's place plus its width is the cluster's
/// trailing edge, where AccessKit puts that caret.
#[test]
fn the_end_caret_after_a_split_cluster_is_at_its_end() {
    let text = format!("a{}", "\u{301}".repeat(150));
    let Some(bench) = field_in(&text, 3000.0) else {
        return;
    };
    let runs = bench.field.runs();
    let run = &runs[0];
    let last = run.positions.len() - 1;
    assert_eq!(run.positions[last] + run.widths[last], run.positions[0] + run.widths[0]);
}

/// Text that changes script often, so the line has many shaped runs,
/// still gives its characters in text order with lengths that add up,
/// from one pass: each cluster is visited once.
#[test]
fn many_shaped_runs_are_read_in_one_pass() {
    let text = "a\u{3b1}".repeat(300);
    let Some(bench) = field_in(&text, 30000.0) else {
        return;
    };
    super::VISITS.with(|visits| visits.set(0));
    let runs = bench.field.runs();
    let visits = super::VISITS.with(std::cell::Cell::get);
    let joined: String = runs.iter().map(|run| run.text.as_str()).collect();
    assert_eq!(joined, text, "text order");
    let lengths: usize = runs
        .iter()
        .flat_map(|run| run.lengths.iter())
        .map(|&length| usize::from(length))
        .sum();
    assert_eq!(lengths, text.len());
    assert!(visits <= 600 + 2, "{visits} visits for 600 clusters");
}

/// A field a rebuild took out of its panel takes no screen reader action
/// in the gap before the layout that drops its state: no edit, no message.
#[test]
fn a_removed_field_takes_no_text_actions() {
    use accesskit::{Action, ActionData};
    let Some((mut harness, mut text)) = form("abc", true) else {
        return;
    };
    let (field, _) = describe(&mut harness, &mut text);
    harness.send(Message::Remove);
    let start = harness.app().log.len();
    harness.step(reader(Action::SetValue, field, Some(ActionData::Value("new".into()))));
    assert_eq!(harness.app().text, "abc");
    assert!(
        !harness.app().log[start..].iter().any(|line| line.starts_with("Edited")),
        "{:?}",
        &harness.app().log[start..]
    );
}

/// A field's runs are one line: each links to its neighbours both ways, so
/// a screen reader reads the whole field as one line. One run links to
/// nothing.
#[test]
fn a_fields_runs_link_as_one_line() {
    let Some((mut harness, mut text)) = form(&"x".repeat(300), true) else {
        return;
    };
    let (_, node) = describe(&mut harness, &mut text);
    let runs = node.children().to_vec();
    assert_eq!(runs.len(), 2);
    let nodes = &harness.app().nodes.as_ref().expect("nodes").nodes;
    let run = |id| {
        nodes
            .iter()
            .find(|(at, _)| *at == id)
            .map(|(_, node)| node.clone())
            .expect("run")
    };
    assert_eq!(run(runs[0]).next_on_line(), Some(runs[1]));
    assert_eq!(run(runs[0]).previous_on_line(), None);
    assert_eq!(run(runs[1]).previous_on_line(), Some(runs[0]));
    assert_eq!(run(runs[1]).next_on_line(), None);
}

// --- The input method and the field's own paste (section 8, rules 2 and 4) ---

/// The input method's `event`, as the driver sends it.
fn ime_in(event: ImeEvent) -> DriverEvent {
    DriverEvent::Input {
        event: Event::Ime(event),
        pointer: None,
    }
}

/// A preedit of `text` with the caret at its end.
fn preedit(text: &str) -> ImeEvent {
    ImeEvent::Preedit {
        text: text.to_owned(),
        cursor: Some((text.len(), text.len())),
    }
}

/// The plan's sequence: a preedit, an empty preedit, then the commit,
/// leaves the committed text once, with the caret after it, as one undo
/// step. While composing, the field shows the preedit, but its committed
/// text does not hold it.
#[test]
fn a_composition_commits_its_text_once() {
    let Some(mut bench) = field_in("", 300.0) else { return };
    let Bench {
        field,
        fonts,
        layouts,
        now,
    } = &mut bench;
    let used = field.ime(&preedit("にほ"), *now, fonts, layouts);
    assert!(used.taken && !used.edited && used.redraw);
    assert_eq!((field.text(), field.committed().as_str()), ("にほ", ""));
    assert!(field.composing());
    field.ime(&preedit(""), *now, fonts, layouts);
    assert!(!field.composing());
    let used = field.ime(&ImeEvent::Commit("日本".to_owned()), *now, fonts, layouts);
    assert!(used.edited);
    assert_eq!(field.committed(), "日本");
    assert_eq!(field.text(), "日本");
    assert_eq!(field.selection_bytes(), (6, 6));
    assert_eq!(field.steps(), (1, 0));
}

/// While the input method composes, the field takes every key and does
/// nothing with it: neither the committed text nor the preedit changes.
#[test]
fn keys_while_composing_are_taken_and_do_nothing() {
    for platform in PLATFORMS {
        let Some(mut bench) = field_in("ab", 300.0) else { return };
        bench
            .field
            .ime(&preedit("に"), bench.now, &mut bench.fonts, &mut bench.layouts);
        let control_tab = held(false, true, false, false);
        for (key, modifiers) in [
            (Key::Backspace, Modifiers::NONE),
            (Key::Enter, Modifiers::NONE),
            (Key::Left, Modifiers::NONE),
            (Key::Tab, Modifiers::NONE),
            (Key::Tab, control_tab),
            (Key::Escape, Modifiers::NONE),
            (Key::Character('z'), shortcut(platform, false)),
            (Key::Character('v'), shortcut(platform, false)),
        ] {
            let used = bench.hit(platform, key, modifiers);
            assert!(used.taken && !used.edited && !used.paste, "{platform:?} {key:?}");
        }
        assert_eq!(bench.text(), "abに", "{platform:?}");
        assert_eq!(bench.field.committed(), "ab");
    }
}

/// The input method turned off in mid-composition keeps what it showed,
/// as one undo step.
#[test]
fn an_input_method_turned_off_keeps_its_text() {
    let Some(mut bench) = field_in("a", 300.0) else { return };
    let Bench {
        field,
        fonts,
        layouts,
        now,
    } = &mut bench;
    field.ime(&preedit("に"), *now, fonts, layouts);
    let used = field.ime(&ImeEvent::Disabled, *now, fonts, layouts);
    assert!(used.edited);
    assert_eq!(field.committed(), "aに");
    assert!(!field.composing());
    assert_eq!(field.steps(), (1, 0));
}

/// The app's text taken at a layout is compared with the committed text,
/// so an app text that did not change never ends a composition.
#[test]
fn an_unchanged_app_text_keeps_the_composition() {
    let Some(mut bench) = field_in("ab", 300.0) else { return };
    bench
        .field
        .ime(&preedit("に"), bench.now, &mut bench.fonts, &mut bench.layouts);
    bench
        .field
        .take("ab", STYLE, Lines::One, 300.0, &mut bench.fonts, &mut bench.layouts);
    assert!(bench.field.composing());
    assert_eq!(bench.text(), "abに");
}

/// While composing, a screen reader hears the preedit in the field's value
/// (Brylie's answer to C2's question 1), and the app's `Edited` never
/// carries it.
#[test]
fn the_node_carries_the_preedit_and_edited_does_not() {
    let Some((mut harness, mut text)) = form("ab", true) else {
        return;
    };
    let start = harness.app().log.len();
    harness.step(ime_in(preedit("に")));
    let (_, node) = describe(&mut harness, &mut text);
    assert_eq!(node.value(), Some("abに"));
    assert!(
        !harness.app().log[start..].iter().any(|line| line.starts_with("Edited")),
        "{:?}",
        &harness.app().log[start..]
    );
    assert_eq!(harness.app().text, "ab");
    assert_eq!(harness.runtime().target().ime_kept, 1, "the field took it");
}

/// Focus leaving in mid-composition keeps the preedit as text: the app
/// hears it as `Edited`, then `Left`.
#[test]
fn focus_leaving_keeps_the_composition_as_text() {
    let Some((mut harness, mut text)) = form("a", true) else {
        return;
    };
    harness.step(ime_in(preedit("に")));
    let start = harness.app().log.len();
    harness.send(Message::Focus(UiId::panel("form").with("ok")));
    harness.lay_out(&mut text);
    assert_eq!(
        harness.app().log[start..],
        ["Edited(\"aに\")".to_owned(), "Left".to_owned()],
        "{:?}",
        &harness.app().log[start..]
    );
    assert_eq!(harness.app().text, "aに");
}

/// Tab, Ctrl+Tab, Escape, and undo in mid-composition reach neither the
/// target nor the app, and move no focus: the keys are the input method's.
#[test]
fn keys_in_mid_composition_reach_nothing() {
    let Some((mut harness, _text)) = form("ab", true) else {
        return;
    };
    let platform = Platform::current();
    harness.step(ime_in(preedit("に")));
    for (key, modifiers) in [
        (Key::Tab, Modifiers::NONE),
        (Key::Tab, held(false, true, false, false)),
        (Key::Escape, Modifiers::NONE),
        (Key::Enter, Modifiers::NONE),
        (Key::Backspace, Modifiers::NONE),
        (Key::Character('z'), shortcut(platform, false)),
    ] {
        harness.step(key_in(key, modifiers, None));
    }
    assert!(
        harness.runtime().target().heard.is_empty(),
        "{:?}",
        harness.runtime().target().heard
    );
    assert_eq!(harness.runtime().focused(), Some(&name()));
    assert_eq!(harness.app().text, "ab");
}

/// An input method event with no field focused goes to the target, and so
/// does one for a field under a raised layer: that field is not changed.
#[test]
fn an_input_method_event_with_no_field_goes_to_the_target() {
    let Some((mut harness, mut text)) = form("ab", true) else {
        return;
    };
    harness.send(Message::Cover(Layer::Question));
    harness.lay_out(&mut text);
    harness.step(ime_in(ImeEvent::Commit("x".to_owned())));
    assert_eq!(harness.runtime().target().ime_heard, 1);
    assert_eq!(harness.app().text, "ab");
    harness.send(Message::Cover(Layer::Canvas));
    harness.send(Message::Focus(UiId::panel("form").with("ok")));
    harness.lay_out(&mut text);
    harness.step(ime_in(ImeEvent::Commit("y".to_owned())));
    assert_eq!(harness.runtime().target().ime_heard, 2);
    assert_eq!(harness.app().text, "ab");
}

/// Paste reads the clipboard through the driver, and the text goes in on
/// the next turn as one undo step, made one line: "one\r\ntwo\n" is "one
/// two". The app hears it as `Edited`.
#[test]
fn paste_comes_in_as_one_line_and_one_step() {
    let Some((mut harness, mut text)) = form("", true) else {
        return;
    };
    let platform = Platform::current();
    harness.driver_mut().set_clipboard(Some("one\r\ntwo\n".to_owned()));
    harness.step(key_in(Key::Character('v'), shortcut(platform, false), None));
    harness.lay_out(&mut text);
    assert_eq!(harness.app().text, "one two");
    assert!(harness.app().log.iter().any(|line| line == "Edited(\"one two\")"));
    harness.step(key_in(Key::Character('z'), shortcut(platform, false), None));
    harness.lay_out(&mut text);
    assert_eq!(harness.app().text, "", "one undo step");
}

/// A paste whose answer comes after focus moved, or for a field that no
/// longer takes input, is dropped, so a slow clipboard never types into
/// another place.
#[test]
fn a_paste_after_focus_moved_is_dropped() {
    use crate::Paste;
    let Some((mut harness, mut text)) = form("ab", true) else {
        return;
    };
    // The answer comes later, from outside: built here as a driver would.
    let paste = |harness: &Harness<Form, Keys>| harness.runtime().test_paste(name());
    let later = paste(&harness);
    harness.send(Message::Focus(UiId::panel("form").with("ok")));
    harness.lay_out(&mut text);
    later.give(crate::ClipboardText::Text("x".to_owned()));
    harness.step(DriverEvent::Woken);
    harness.lay_out(&mut text);
    assert_eq!(harness.app().text, "ab", "focus moved");

    harness.send(Message::Focus(name()));
    harness.send(Message::Cover(Layer::Question));
    harness.lay_out(&mut text);
    let covered: Paste = paste(&harness);
    covered.give(crate::ClipboardText::Text("y".to_owned()));
    harness.step(DriverEvent::Woken);
    harness.lay_out(&mut text);
    assert_eq!(harness.app().text, "ab", "covered");
}

/// The runtime's input method area: on, at the caret in the window, while
/// one of its fields has focus; `None` when none has.
#[test]
fn the_input_method_area_follows_the_focused_field() {
    let Some((mut harness, mut text)) = form("ab", true) else {
        return;
    };
    let Some(crate::Ime::On { area }) = harness.runtime().ime() else {
        panic!("on while the field has focus");
    };
    let start = harness.runtime().ime();
    harness.step(key_in(Key::Home, Modifiers::NONE, None));
    harness.lay_out(&mut text);
    let Some(crate::Ime::On { area: home }) = harness.runtime().ime() else {
        panic!("on");
    };
    // Parley widens the area by about three characters each way, so the
    // caret's move shows at its right edge.
    assert!(
        home.x + home.width < area.x + area.width,
        "the area follows the caret: {home:?} {area:?} {start:?}"
    );
    assert!(area.height > 0.0 && area.width > 0.0);
    harness.send(Message::Focus(UiId::panel("form").with("ok")));
    harness.lay_out(&mut text);
    assert_eq!(harness.runtime().ime(), None);
}

/// Text being composed is underlined: a quad in the text colour at the
/// foot of the preedit's box, after the text.
#[test]
fn a_preedit_is_underlined() {
    let Some((mut harness, mut text)) = form("ab", true) else {
        return;
    };
    harness.step(ime_in(preedit("に")));
    harness.lay_out(&mut text);
    let mut recorder = Recorder::default();
    let (_, _, mut panels) = harness.parts();
    panels.paint(Layer::Panels, &mut recorder, &PANEL);
    let painted = recorder.painted;
    let at = painted
        .iter()
        .position(|piece| piece.as_text() == Some("abに"))
        .expect("the text with the preedit");
    let underline = painted[at + 1..]
        .iter()
        .find_map(|piece| match piece {
            Painted::Quad(quad) if quad.fill == PANEL.text => Some(quad.rect),
            _ => None,
        })
        .expect("an underline");
    assert!(underline.height <= 1.0 && underline.width > 0.0, "{underline:?}");
}

/// A screen reader acting in mid-composition keeps the preedit as text
/// first, then acts: a replacement lands after the kept text.
#[test]
fn a_screen_reader_action_while_composing_keeps_the_preedit() {
    use super::FieldAction;
    let Some(mut bench) = field_in("a", 300.0) else { return };
    let Bench {
        field,
        fonts,
        layouts,
        now,
    } = &mut bench;
    field.ime(&preedit("に"), *now, fonts, layouts);
    field.act(FieldAction::ReplaceSelection("b".to_owned()), *now, fonts, layouts);
    assert!(!field.composing());
    assert_eq!(field.committed(), "aにb");
}

/// A paste whose focus moved away and came back before the answer came is
/// dropped too: the focus moved, so the text would appear after the user
/// had moved on, as a held Space's click does not happen then.
#[test]
fn a_paste_after_focus_went_away_and_back_is_dropped() {
    let Some((mut harness, mut text)) = form("ab", true) else {
        return;
    };
    let asked = harness.runtime().test_paste(name());
    harness.send(Message::Focus(UiId::panel("form").with("ok")));
    harness.lay_out(&mut text);
    harness.send(Message::Focus(name()));
    harness.lay_out(&mut text);
    assert_eq!(harness.runtime().focused(), Some(&name()));
    asked.give(crate::ClipboardText::Text("x".to_owned()));
    harness.step(DriverEvent::Woken);
    harness.lay_out(&mut text);
    assert_eq!(harness.app().text, "ab");
}

/// A screen reader's action in mid-composition keeps the preedit as text,
/// and that is an edit the app hears, even when the action itself changes
/// nothing more: SetValue to the text already shown, or a stale
/// selection, still reports the kept text.
#[test]
fn keeping_a_preedit_for_an_action_is_an_edit() {
    use super::FieldAction;
    for action in [
        FieldAction::SetValue("aに".to_owned()),
        FieldAction::Select {
            anchor: (0, 0),
            focus: (0, 9),
        },
    ] {
        let Some(mut bench) = field_in("a", 300.0) else { return };
        let Bench {
            field,
            fonts,
            layouts,
            now,
        } = &mut bench;
        field.ime(&preedit("に"), *now, fonts, layouts);
        let used = field.act(action.clone(), *now, fonts, layouts);
        assert!(used.edited && used.changed, "{action:?}");
        assert_eq!(field.committed(), "aに");
        assert_eq!(
            field.steps(),
            (1, 0),
            "keeping the preedit is one undo step: {action:?}"
        );
    }
}

/// The same through the runtime: the app hears `Edited` with the kept
/// text, so the next layout does not take the preedit away.
#[test]
fn the_app_hears_a_preedit_a_screen_reader_kept() {
    use accesskit::{Action, ActionData};
    let Some((mut harness, mut text)) = form("a", true) else {
        return;
    };
    let (field, _) = describe(&mut harness, &mut text);
    harness.step(ime_in(preedit("に")));
    let start = harness.app().log.len();
    harness.step(reader(Action::SetValue, field, Some(ActionData::Value("aに".into()))));
    harness.lay_out(&mut text);
    assert!(
        harness.app().log[start..].contains(&"Edited(\"aに\")".to_owned()),
        "Edited carries the committed text with the kept preedit: {:?}",
        &harness.app().log[start..]
    );
    assert_eq!(harness.app().text, "aに");
}

/// Every key is the input method's while it composes, Cmd+V too, so a
/// paste that meets a composition was asked for before it began. The
/// sequence: the paste is asked for, then the composition starts in the
/// same field, with focus staying there, then the answer arrives. The
/// preedit stays as text and the paste goes in after it, as macOS commits
/// marked text before it pastes: neither is lost.
#[test]
fn a_paste_during_a_composition_goes_in_after_it() {
    let Some((mut harness, mut text)) = form("a", true) else {
        return;
    };
    let asked = harness.runtime().test_paste(name());
    harness.step(ime_in(preedit("に")));
    assert_eq!(harness.runtime().focused(), Some(&name()), "focus stays on the field");
    asked.give(crate::ClipboardText::Text("x".to_owned()));
    harness.step(DriverEvent::Woken);
    harness.lay_out(&mut text);
    assert_eq!(harness.runtime().focused(), Some(&name()));
    assert_eq!(harness.app().text, "aにx");
}

/// A composition that starts over a selection deletes the selection first,
/// as an edit of its own: the committed text and the app agree, a layout
/// that brings the app's (new) text back keeps the composition, and undo
/// after a cancelled composition brings the selected text back.
#[test]
fn a_composition_over_a_selection_deletes_it_as_its_own_step() {
    let Some(mut bench) = field_in("abc", 300.0) else {
        return;
    };
    bench.hit(Platform::Mac, Key::Character('a'), shortcut(Platform::Mac, false));
    let used = bench
        .field
        .ime(&preedit("に"), bench.now, &mut bench.fonts, &mut bench.layouts);
    assert!(used.edited, "the app hears the deletion");
    assert_eq!(bench.field.committed(), "");
    assert_eq!(bench.text(), "に");
    bench
        .field
        .take("", STYLE, Lines::One, 300.0, &mut bench.fonts, &mut bench.layouts);
    assert!(
        bench.field.composing(),
        "the app's text matches, so the composition stays"
    );
    bench
        .field
        .ime(&preedit(""), bench.now, &mut bench.fonts, &mut bench.layouts);
    assert_eq!(bench.text(), "");
    assert_eq!(bench.field.steps(), (1, 0));
    bench.hit(Platform::Mac, Key::Character('z'), shortcut(Platform::Mac, false));
    assert_eq!(bench.text(), "abc", "undo brings the selected text back");
}

/// Through the runtime, the app hears the deletion as `Edited`, so its text
/// and the field's agree while the composition goes on, and the commit
/// then lands once.
#[test]
fn the_app_hears_a_selection_a_composition_replaced() {
    let Some((mut harness, mut text)) = form("abc", true) else {
        return;
    };
    harness.step(key_in(Key::Character('a'), shortcut(Platform::current(), false), None));
    harness.step(ime_in(preedit("に")));
    harness.lay_out(&mut text);
    assert_eq!(harness.app().text, "");
    harness.step(ime_in(preedit("")));
    harness.step(ime_in(ImeEvent::Commit("日".to_owned())));
    harness.lay_out(&mut text);
    assert_eq!(harness.app().text, "日");
}

/// The context tells the app when one of the runtime's fields has focus, so
/// a shortcut the field leaves can do nothing (section 6, rule 6, step 6):
/// true on the field, false on the button after it.
#[test]
fn the_context_says_when_a_field_has_focus() {
    let Some((mut harness, mut text)) = form("", false) else {
        return;
    };
    harness.send(Message::AskField);
    harness.send(Message::Focus(UiId::panel("form").with("ok")));
    harness.lay_out(&mut text);
    harness.send(Message::AskField);
    let asked: Vec<_> = harness
        .app()
        .log
        .iter()
        .filter(|entry| entry.starts_with("field focus"))
        .cloned()
        .collect();
    assert_eq!(asked, ["field focus true", "field focus false"]);
}

/// A paste whose read is refused reaches the app as `PasteRefused`, so it
/// can show a status message a screen reader hears, and changes nothing.
/// Through the headless driver, which can be made to refuse.
#[test]
fn a_refused_paste_reaches_the_app() {
    let Some((mut harness, mut text)) = form("ab", true) else {
        return;
    };
    harness.driver_mut().set_clipboard(Some("x".to_owned()));
    harness.driver_mut().refuse_reads(true);
    let start = harness.app().log.len();
    harness.step(key_in(Key::Character('v'), shortcut(Platform::current(), false), None));
    harness.lay_out(&mut text);
    assert_eq!(harness.app().text, "ab");
    assert_eq!(harness.app().log[start..], ["PasteRefused"]);
}

/// A refusal is heard even when focus has moved on since the paste was
/// asked for: the user asked, so the user is told it did not work. An
/// empty clipboard says nothing.
#[test]
fn a_refusal_is_heard_after_focus_moved_and_empty_says_nothing() {
    let Some((mut harness, mut text)) = form("ab", true) else {
        return;
    };
    let asked = harness.runtime().test_paste(name());
    harness.send(Message::Focus(UiId::panel("form").with("ok")));
    harness.lay_out(&mut text);
    let start = harness.app().log.len();
    asked.give(crate::ClipboardText::Refused);
    harness.step(DriverEvent::Woken);
    assert_eq!(harness.app().log[start..], ["PasteRefused"]);
    let empty = harness.runtime().test_paste(name());
    empty.give(crate::ClipboardText::Empty);
    harness.step(DriverEvent::Woken);
    assert_eq!(harness.app().log.len(), start + 1, "an empty clipboard says nothing");
}

/// While the input method composes in a focused field, Tab is the input
/// method's: the runtime never asks the app where Tab goes (`App::tab_from`,
/// plan 3.2, row 2), so the focus stays in the field. Once the text is
/// committed, Tab steps on as usual, and the app is asked.
#[test]
fn tab_while_composing_never_asks_where_tab_goes() {
    let Some((mut harness, _typesetter)) = form("ab", true) else {
        return;
    };
    harness.step(ime_in(preedit("に")));
    harness.step(key_in(Key::Tab, Modifiers::NONE, None));
    assert_eq!(
        harness.runtime().focused(),
        Some(&name()),
        "the focus stayed in the field"
    );
    assert_eq!(harness.app().tabs.get(), 0, "tab_from never ran");
    harness.step(ime_in(ImeEvent::Commit("日".to_owned())));
    harness.step(key_in(Key::Tab, Modifiers::NONE, None));
    assert_eq!(harness.app().tabs.get(), 1, "after the commit, Tab asks");
    assert_eq!(harness.runtime().focused(), Some(&UiId::panel("form").with("ok")));
}

/// A single-line field's runs are cut every 255 characters, as before runs
/// could end at a line: 600 characters make runs of 255, 255, and 90, and
/// the caret at the end is at the end of the last run.
#[test]
fn a_single_line_fields_runs_are_cut_every_255_characters() {
    let Some(bench) = field_in(&"x".repeat(600), 300.0) else {
        return;
    };
    let lengths: Vec<usize> = bench.field.runs().iter().map(|run| run.lengths.len()).collect();
    assert_eq!(lengths, [255, 255, 90]);
    assert!(
        bench.field.runs().iter().all(|run| run.line == 0),
        "all on its one line"
    );
    assert_eq!(bench.field.reader_selection(), ((2, 90), (2, 90)));
}

mod many;
