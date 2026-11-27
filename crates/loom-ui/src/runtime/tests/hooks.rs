//! Tests for the two key hooks on the `App` trait: a key that is always
//! heard, asked first (section 6, rule 6, step 1), and a key that nothing
//! took, last (step 6), with the home element's key kept from the target
//! (step 5).

use crate::access::{Access, Action, Role};
use crate::app::{App, Context, InputState, WindowSpec};
use crate::custom::{Custom, CustomEvent, CustomKind, Moment, Out, Used};
use crate::effect::{Effects, FocusTarget, Selection};
use crate::element::{Element, Length};
use crate::event::{DriverEvent, WindowState};
use crate::headless::Harness;
use crate::layout::tests::{Mono, STYLE};
use crate::routing::{Routed, Target};
use crate::view::{Layer, View};
use loom_ui_core::{Code, Event, Key, KeyEvent, Modifiers, Point, Size, UiId};
use std::time::Instant;

/// The home element's kind.
const HOME: CustomKind = CustomKind::new("home");

/// What the test app hears.
#[derive(Debug, Clone, PartialEq)]
enum Msg {
    /// The always-heard key.
    Panic,
    /// A key the question took.
    Question,
    /// A key nothing took, by its name, and whether it was a press.
    Unclaimed(String, bool),
    /// The home element took a key.
    Home,
    /// Focus the field.
    FocusName,
    /// Open the question.
    Ask,
}

/// The home element: it takes Left, and nothing else.
struct HomeState;

impl Custom<Keys> for HomeState {
    fn event(&mut self, _app: &Keys, event: &CustomEvent, _moment: &Moment, out: &mut Out<'_, Msg>) -> Used {
        match event {
            CustomEvent::Key(KeyEvent::Pressed { key: Key::Left, .. }) => {
                out.send(Msg::Home);
                Used::Yes
            }
            _ => Used::No,
        }
    }
}

/// An app with the home element and a field, and a question when asked.
#[derive(Default)]
struct Keys {
    asking: bool,
    log: Vec<Msg>,
}

impl App for Keys {
    type Message = Msg;

    fn update(&mut self, message: Msg, _cx: &Context<'_, Msg>) -> Effects<Msg> {
        let effects = match message {
            Msg::FocusName => Effects::focus(name()),
            Msg::Ask => {
                self.asking = true;
                Effects::none()
            }
            _ => Effects::none(),
        };
        self.log.push(message);
        effects
    }

    fn view(&self, view: &mut View) {
        view.panel_with(UiId::panel("main"), Layer::Panels, &(), |_| {
            Element::column([
                Element::custom(HOME)
                    .key("home")
                    .width(Length::Fixed(200.0))
                    .height(Length::Fixed(100.0)),
                Element::field("", STYLE)
                    .key("name")
                    .width(Length::Fixed(200.0))
                    .access(Access::new(Role::TextInput, "Name")),
            ])
        });
        if self.asking {
            view.panel_with(UiId::panel("ask"), Layer::Question, &(), |_| {
                Element::text("OK", STYLE)
                    .key("ok")
                    .focusable()
                    .access(Access::new(Role::Button, "OK").action(Action::Click))
            });
        }
    }

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }

    fn input(&self) -> InputState {
        InputState {
            layer: if self.asking { Layer::Question } else { Layer::Canvas },
            ..InputState::default()
        }
    }

    fn home(&self) -> Option<UiId> {
        Some(UiId::panel("main").with("home"))
    }

    fn make_custom(&self, _id: &UiId, kind: CustomKind) -> Option<Box<dyn Custom<Self>>> {
        (kind == HOME).then(|| Box::new(HomeState) as Box<dyn Custom<Self>>)
    }

    fn question_key(&self, _key: &KeyEvent) -> Option<Msg> {
        Some(Msg::Question)
    }

    fn always_key(&self, key: &KeyEvent) -> Option<Msg> {
        matches!(
            key,
            KeyEvent::Pressed { key: Key::Character('.'), modifiers, .. } if modifiers.shortcut()
        )
        .then_some(Msg::Panic)
    }

    fn unclaimed_key(&self, key: &KeyEvent) -> Option<Msg> {
        match key {
            KeyEvent::Pressed { key, .. } => Some(Msg::Unclaimed(format!("{key:?}"), true)),
            KeyEvent::Released { key, .. } => Some(Msg::Unclaimed(format!("{key:?}"), false)),
            _ => None,
        }
    }
}

/// A target that passes every event on and records what it heard and
/// what the runtime kept from it.
#[derive(Default)]
struct Recorder {
    heard: Vec<Event>,
    kept: Vec<Event>,
}

impl Target<Msg> for Recorder {
    fn input(&mut self, event: &Event, _pointer: Option<Point>, _now: Instant, _out: &mut Vec<Msg>) -> Routed {
        self.heard.push(event.clone());
        Routed::Passed
    }

    fn kept(&mut self, event: &Event) {
        self.kept.push(event.clone());
    }

    fn focus(&mut self, _target: &FocusTarget, _select: Selection) {}

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
    UiId::panel("main").with("name")
}

/// A harness laid out in a 400 by 300 window, with nothing focused.
fn harness() -> Harness<Keys, Recorder> {
    let window = WindowState::new(Size::new(400.0, 300.0));
    let mut harness = Harness::new(Keys::default(), Recorder::default(), window, Instant::now());
    harness.lay_out(&mut Mono::default());
    harness
}

/// A press of `key` with `modifiers`.
fn pressed(key: Key, modifiers: Modifiers) -> Event {
    let text = match &key {
        Key::Character(c) if !modifiers.shortcut() => Some(c.to_string()),
        _ => None,
    };
    Event::Key(KeyEvent::Pressed {
        key,
        modifiers,
        repeat: false,
        text,
        code: Code::Other,
    })
}

/// Sends `event` as one turn, then lays out.
fn input(harness: &mut Harness<Keys, Recorder>, event: Event) {
    harness.step(DriverEvent::Input { event, pointer: None });
    harness.lay_out(&mut Mono::default());
}

/// Sends `message`, then lays out.
fn send(harness: &mut Harness<Keys, Recorder>, message: Msg) {
    harness.send(message);
    harness.lay_out(&mut Mono::default());
}

/// Every message the app heard that a key made.
fn keys_heard(harness: &Harness<Keys, Recorder>) -> Vec<Msg> {
    harness
        .app()
        .log
        .iter()
        .filter(|m| !matches!(m, Msg::FocusName | Msg::Ask))
        .cloned()
        .collect()
}

/// Cmd+. (Ctrl+. off a Mac).
fn panic_key() -> Event {
    pressed(Key::Character('.'), Modifiers::platform_shortcut())
}

/// Step 1 comes before the focused field, which would take the key, and
/// the routing target never hears it.
#[test]
fn an_always_heard_key_comes_before_a_focused_field() {
    let mut harness = harness();
    send(&mut harness, Msg::FocusName);
    input(&mut harness, panic_key());
    assert_eq!(keys_heard(&harness), [Msg::Panic]);
    let target = harness.runtime().target();
    assert_eq!(target.kept, [panic_key()]);
    assert!(target.heard.is_empty(), "{:?}", target.heard);
}

/// Step 1 comes before the question, which has every other key (step 2).
#[test]
fn an_always_heard_key_comes_before_the_question() {
    let mut harness = harness();
    send(&mut harness, Msg::Ask);
    input(&mut harness, panic_key());
    input(&mut harness, pressed(Key::Character('y'), Modifiers::NONE));
    assert_eq!(keys_heard(&harness), [Msg::Panic, Msg::Question]);
}

/// Step 6: a key nothing took reaches the app in its own turn, and so does
/// its release.
#[test]
fn a_key_nothing_took_reaches_the_app_in_its_turn() {
    let mut harness = harness();
    input(&mut harness, pressed(Key::Character('s'), Modifiers::NONE));
    assert_eq!(keys_heard(&harness), [Msg::Unclaimed("Character('s')".into(), true)]);
    input(
        &mut harness,
        Event::Key(KeyEvent::Released {
            key: Key::Character('s'),
            code: Code::Other,
        }),
    );
    assert_eq!(
        keys_heard(&harness)[1..],
        [Msg::Unclaimed("Character('s')".into(), false)]
    );
}

/// A key the home element took is kept from the target and is no app
/// shortcut; one it left goes on to step 6.
#[test]
fn a_key_the_home_element_takes_is_kept_and_goes_no_further() {
    let mut harness = harness();
    let left = pressed(Key::Left, Modifiers::NONE);
    input(&mut harness, left.clone());
    assert_eq!(keys_heard(&harness), [Msg::Home]);
    assert_eq!(harness.runtime().target().kept, [left]);
    input(&mut harness, pressed(Key::Right, Modifiers::NONE));
    assert_eq!(keys_heard(&harness)[1..], [Msg::Unclaimed("Right".into(), true)]);
}

/// A key a focused field takes is the field's, and reaches no shortcut.
#[test]
fn a_key_a_field_takes_is_no_shortcut() {
    let mut harness = harness();
    send(&mut harness, Msg::FocusName);
    input(&mut harness, pressed(Key::Character('a'), Modifiers::NONE));
    assert_eq!(keys_heard(&harness), []);
}
