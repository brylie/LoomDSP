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

use super::{CARET_MARGIN, FieldEvent, FieldState, TYPING_PAUSE};
use crate::access::{Access, Role};
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
use loom_ui_core::{Color, Event, IdTable, Key, KeyEvent, Modifiers, Platform, Point, Size, TextStyle, UiId, Weight};
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
    field.take(text, STYLE, width, &mut fonts, &mut layouts);
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
            (Key::Character('v'), shortcut(platform, false)),
            (Key::Character('s'), shortcut(platform, false)),
        ] {
            assert_eq!(bench.hit(platform, key, modifiers), super::KeyUse::default(), "{key:?}");
        }
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
    field.take("hi", STYLE, 300.0, fonts, layouts);
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
                self.heard.push(event);
            }
            Message::Describe => self.nodes = Some(cx.access_nodes(&mut self.ids)),
            Message::Cover(layer) => self.covered = Some(layer),
        }
        Effects::none()
    }

    fn view(&self, view: &mut View) {
        view.panel_with(UiId::panel("form"), Layer::Panels, &self.text, form_panel);
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

    fn field_event(&self, id: &UiId, event: &FieldEvent) -> Option<Message> {
        assert_eq!(*id, name());
        Some(Message::Field(event.clone()))
    }
}

/// The form's panel: the field showing `text`, then an OK button.
#[expect(
    clippy::ptr_arg,
    reason = "`View::panel_with` hands the builder its model, a `String`, by reference"
)]
fn form_panel(text: &String) -> Element {
    let field = Element::field(text.clone(), STYLE)
        .key("name")
        .width(Length::Fixed(120.0))
        .access(Access::new(Role::TextInput, "Name"));
    let button = Element::text("OK", STYLE)
        .key("ok")
        .focusable()
        .access(Access::new(Role::Button, "OK").action(crate::access::Action::Click));
    Element::row([field, button])
}

/// A routing target that counts the keys it hears, and the keys the
/// runtime kept from it.
#[derive(Default)]
struct Keys {
    heard: Vec<Key>,
    kept: Vec<Key>,
}

impl<M> Target<M> for Keys {
    fn input(&mut self, event: &Event, _pointer: Option<Point>, _now: Instant, _out: &mut Vec<M>) -> Routed {
        if let Event::Key(KeyEvent::Pressed { key, .. }) = event {
            self.heard.push(*key);
        }
        Routed::Passed
    }

    fn kept(&mut self, event: &Event) {
        if let Event::Key(KeyEvent::Pressed { key, .. }) = event {
            self.kept.push(*key);
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
/// Escape, Save, and paste do reach it, and are not kept.
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
    harness.step(key_in(Key::Character('v'), shortcut(platform, false), None));
    assert_eq!(
        harness.runtime().target().heard,
        [Key::Escape, Key::Character('s'), Key::Character('v')]
    );
    assert_eq!(harness.runtime().target().kept.len(), 8, "a key passed on is not kept");
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
