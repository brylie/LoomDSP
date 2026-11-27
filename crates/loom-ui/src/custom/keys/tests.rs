//! Tests for a custom element's keys and focus: step 3 while it has the
//! focus, step 5 as the window's home, Tab into and out of it, and focus
//! on the parts inside it (ui-layer.md, section 9, rules 2 and 6).

use crate::access::{Access, Action, Role};
use crate::app::{App, Context, InputState, WindowSpec};
use crate::custom::{Custom, CustomEvent, CustomKind, Moment, Out, Used};
use crate::effect::Effects;
use crate::element::{Element, Length};
use crate::event::{AccessEvent, DriverEvent, WindowState};
use crate::headless::Harness;
use crate::layout::tests::{Mono, STYLE};
use crate::routing::NoTarget;
use crate::view::{Layer, View};
use accesskit::{ActionRequest, NodeId};
use loom_ui_core::{Code, Event, IdKey, Key, KeyEvent, Modifiers, Size, UiId};
use std::time::Instant;

/// The roll's kind.
const ROLL: CustomKind = CustomKind::new("roll");

/// The roll's parts, in its own focus order.
const PARTS: [&str; 3] = ["a", "b", "c"];

/// What the roll sends, and what the test app hears.
#[derive(Debug, Clone, PartialEq)]
enum Message {
    /// The roll used a key: its name.
    Used(String),
    /// The focus moved to this UI ID, or off the runtime's elements.
    Focus(Option<UiId>),
    /// Focus the roll's part with this name, by a focus effect.
    FocusPart(&'static str),
    /// Focus the roll's part with this name as the latest input would.
    FocusPartAsInput(&'static str),
    /// Open or close the overlay.
    Overlay(bool),
}

/// The roll's state: which part has its focus, and what it heard.
#[derive(Default)]
struct RollState {
    part: Option<usize>,
    heard: Vec<CustomEvent>,
}

impl Custom<Rolls> for RollState {
    fn event(&mut self, _app: &Rolls, event: &CustomEvent, _moment: &Moment, out: &mut Out<'_, Message>) -> Used {
        self.heard.push(event.clone());
        match event {
            CustomEvent::Entered { back } => {
                self.part = Some(if *back { PARTS.len() - 1 } else { 0 });
                Used::Yes
            }
            // Tab moves through the parts, and lets go past either end.
            CustomEvent::Key(KeyEvent::Pressed {
                key: Key::Tab,
                modifiers,
                ..
            }) => {
                let at = self.part.unwrap_or(0);
                let next = if modifiers.shift {
                    at.checked_sub(1)
                } else {
                    Some(at + 1)
                };
                match next.filter(|next| *next < PARTS.len()) {
                    Some(next) => {
                        self.part = Some(next);
                        Used::Yes
                    }
                    None => Used::No,
                }
            }
            CustomEvent::Key(KeyEvent::Pressed {
                key: Key::Character(c), ..
            }) if *c == 'x' => {
                out.send(Message::Used("x".into()));
                Used::Yes
            }
            CustomEvent::Key(KeyEvent::Released { .. }) => Used::Yes,
            _ => Used::No,
        }
    }

    fn focus_inner(&mut self, _app: &Rolls, rest: &[IdKey]) -> bool {
        let found = PARTS.iter().position(|part| rest == [IdKey::from(*part)]);
        if found.is_some() {
            self.part = found;
        }
        found.is_some()
    }

    fn focused_inner(&self, _app: &Rolls) -> Option<Vec<IdKey>> {
        self.part.map(|part| vec![IdKey::from(PARTS[part])])
    }
}

/// An app with a stop, the roll, another stop, and a field, in one panel,
/// with the roll as the window's home, and an overlay when it is open.
struct Rolls {
    overlay: bool,
    log: Vec<Message>,
}

impl App for Rolls {
    type Message = Message;

    fn update(&mut self, message: Message, _cx: &Context<'_, Message>) -> Effects<Message> {
        let effects = match &message {
            Message::FocusPart(part) => Effects::focus(roll().with(*part)),
            Message::FocusPartAsInput(part) => Effects::focus_as_input(roll().with(*part)),
            Message::Overlay(open) => {
                self.overlay = *open;
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
                stop("before"),
                Element::custom(ROLL)
                    .key("roll")
                    .width(Length::Fixed(200.0))
                    .height(Length::Fixed(100.0))
                    .focusable()
                    .access(Access::new(Role::Group, "Roll")),
                stop("after"),
                Element::field("", STYLE)
                    .key("name")
                    .width(Length::Fixed(200.0))
                    .access(Access::new(Role::TextInput, "Name")),
            ])
        });
        if self.overlay {
            view.panel_with(UiId::panel("dialog"), Layer::Overlays, &(), |_| {
                Element::space(Length::Fixed(50.0), Length::Fixed(50.0)).key("box")
            });
        }
    }

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }

    fn input(&self) -> InputState {
        InputState {
            layer: if self.overlay { Layer::Overlays } else { Layer::Canvas },
            ..InputState::default()
        }
    }

    fn home(&self) -> Option<UiId> {
        Some(roll())
    }

    fn focus_moved(&self, id: Option<&UiId>) -> Option<Message> {
        Some(Message::Focus(id.cloned()))
    }

    fn accessibility_event(&self, event: &AccessEvent) -> Option<Message> {
        // The test's screen reader asks to focus part b, as a node for it
        // would.
        matches!(event, AccessEvent::Action(request) if request.action == accesskit::Action::Focus)
            .then_some(Message::FocusPart("b"))
    }

    fn make_custom(&self, _id: &UiId, kind: CustomKind) -> Option<Box<dyn Custom<Self>>> {
        (kind == ROLL).then(|| Box::new(RollState::default()) as Box<dyn Custom<Self>>)
    }
}

/// A focus stop, 16 high at the fake's measure.
fn stop(key: &str) -> Element {
    Element::text(key, STYLE)
        .key(key)
        .focusable()
        .access(Access::new(Role::ComboBox, key).action(Action::Click))
}

/// The roll's UI ID.
fn roll() -> UiId {
    UiId::panel("main").with("roll")
}

/// An element of the panel by key.
fn id(key: &str) -> UiId {
    UiId::panel("main").with(key)
}

/// A harness laid out in a 400 by 300 window, with nothing focused.
fn harness() -> Harness<Rolls, NoTarget> {
    let app = Rolls {
        overlay: false,
        log: Vec::new(),
    };
    let window = WindowState::new(Size::new(400.0, 300.0));
    let mut harness = Harness::new(app, NoTarget, window, Instant::now());
    harness.lay_out(&mut Mono::default());
    harness
}

/// Presses `key` with `modifiers`, then lays out.
fn press(harness: &mut Harness<Rolls, NoTarget>, key: Key, modifiers: Modifiers) {
    let text = match &key {
        Key::Character(c) => Some(c.to_string()),
        _ => None,
    };
    harness.step(DriverEvent::Input {
        event: Event::Key(KeyEvent::Pressed {
            key,
            modifiers,
            repeat: false,
            text,
            code: Code::Other,
        }),
        pointer: None,
    });
    harness.lay_out(&mut Mono::default());
}

/// Tab, or Shift+Tab when `back`.
fn tab(harness: &mut Harness<Rolls, NoTarget>, back: bool) {
    let modifiers = Modifiers {
        shift: back,
        ..Modifiers::NONE
    };
    press(harness, Key::Tab, modifiers);
}

/// The letter `c`.
fn letter(harness: &mut Harness<Rolls, NoTarget>, c: char) {
    press(harness, Key::Character(c), Modifiers::NONE);
}

/// Sends `message`, then lays out.
fn send(harness: &mut Harness<Rolls, NoTarget>, message: Message) {
    harness.send(message);
    harness.lay_out(&mut Mono::default());
}

/// What the roll heard.
fn heard(harness: &Harness<Rolls, NoTarget>) -> Vec<CustomEvent> {
    harness
        .runtime()
        .custom::<RollState>(&roll())
        .map(|roll| roll.heard.clone())
        .unwrap_or_default()
}

/// The last focus the app heard of.
fn told(harness: &Harness<Rolls, NoTarget>) -> Option<Option<UiId>> {
    harness.app().log.iter().rev().find_map(|message| match message {
        Message::Focus(id) => Some(id.clone()),
        _ => None,
    })
}

/// How many keys the roll used that sent a message.
fn used(harness: &Harness<Rolls, NoTarget>) -> usize {
    harness
        .app()
        .log
        .iter()
        .filter(|message| matches!(message, Message::Used(_)))
        .count()
}

/// Tab onto the roll enters it at its first part, and Tab moves through
/// its parts until it lets Tab go, then on to the next stop. The app hears
/// each part as the focus.
#[test]
fn tab_moves_inside_the_roll_until_it_lets_go() {
    let mut harness = harness();
    tab(&mut harness, false);
    assert_eq!(harness.runtime().focused(), Some(&id("before")));
    tab(&mut harness, false);
    assert!(heard(&harness).contains(&CustomEvent::Entered { back: false }));
    assert_eq!(told(&harness), Some(Some(roll().with("a"))));
    tab(&mut harness, false);
    assert_eq!(told(&harness), Some(Some(roll().with("b"))));
    tab(&mut harness, false);
    tab(&mut harness, false);
    assert_eq!(harness.runtime().focused(), Some(&id("after")));
    assert_eq!(told(&harness), Some(Some(id("after"))));
}

/// Shift+Tab onto the roll enters it at its last part.
#[test]
fn shift_tab_enters_the_roll_at_its_last_part() {
    let mut harness = harness();
    send(&mut harness, Message::FocusPart("b"));
    // From the stop after the roll.
    tab(&mut harness, false);
    tab(&mut harness, false);
    assert_eq!(harness.runtime().focused(), Some(&id("after")));
    tab(&mut harness, true);
    assert_eq!(heard(&harness).last(), Some(&CustomEvent::Entered { back: true }));
    assert_eq!(told(&harness), Some(Some(roll().with("c"))));
}

/// The focused roll hears a key before anything else of the runtime's,
/// and a key it used goes no further; its message runs in the key's turn.
#[test]
fn the_focused_roll_hears_its_keys_first() {
    let mut harness = harness();
    send(&mut harness, Message::FocusPart("a"));
    let turns = harness.runtime().counts().turns;
    letter(&mut harness, 'x');
    assert_eq!(harness.runtime().counts().turns, turns + 1);
    assert_eq!(used(&harness), 1);
}

/// A focus effect on a part inside the roll goes through the focus report
/// like any focus move, so the app's copy of the focus is never behind; a
/// screen reader's Focus takes the same path.
#[test]
fn a_focus_on_a_part_is_reported_like_any_other() {
    let mut harness = harness();
    send(&mut harness, Message::FocusPart("c"));
    assert_eq!(harness.runtime().focused(), Some(&roll()));
    assert_eq!(told(&harness), Some(Some(roll().with("c"))));
    harness.step(DriverEvent::Accessibility(AccessEvent::Action(ActionRequest {
        action: accesskit::Action::Focus,
        target_tree: accesskit::TreeId::ROOT,
        target_node: NodeId(1),
        data: None,
    })));
    assert_eq!(told(&harness), Some(Some(roll().with("b"))));
}

/// A focus effect that follows the latest input, onto a part inside the
/// roll, shows no ring after a pointer press, as on any element (section
/// 6, rule 3): the runtime's focus is the roll, which owns the part, and
/// the quiet check looks there.
#[test]
fn a_focus_as_input_on_a_part_after_a_press_shows_no_ring() {
    use loom_ui_core::{Point, PointerButton, PointerEvent};
    let mut harness = harness();
    harness.step(DriverEvent::Input {
        event: Event::Pointer(PointerEvent::Pressed(PointerButton::Left)),
        pointer: Some(Point::new(300.0, 290.0)),
    });
    send(&mut harness, Message::FocusPartAsInput("b"));
    assert_eq!(harness.runtime().focused(), Some(&roll()));
    assert_eq!(told(&harness), Some(Some(roll().with("b"))));
    assert_eq!(harness.runtime().focus_ring(), None, "no ring after a press");
}

/// A focus effect on a part the roll does not have is dropped: the focus
/// stays where it was.
#[test]
fn a_focus_on_a_missing_part_is_dropped() {
    let mut harness = harness();
    tab(&mut harness, false);
    let before = harness.app().log.len();
    harness.send(Message::FocusPart("zz"));
    assert_eq!(harness.runtime().focused(), Some(&id("before")));
    assert_eq!(harness.app().log.len(), before + 1, "no focus move was told");
}

/// With nothing focused, the roll is the window's home and hears the
/// keys; a focused field or another focused stop keeps them from it.
#[test]
fn the_home_element_hears_keys_while_nothing_has_focus() {
    let mut harness = harness();
    letter(&mut harness, 'x');
    assert_eq!(used(&harness), 1);
    tab(&mut harness, false);
    letter(&mut harness, 'x');
    assert_eq!(used(&harness), 1, "a focused stop keeps the key from home");
    let field = id("name");
    harness.focus_at(harness.runtime().bounds(&field).unwrap().center());
    assert_eq!(harness.runtime().focused(), Some(&field));
    letter(&mut harness, 'x');
    assert_eq!(used(&harness), 1, "a focused field keeps the key from home");
}

/// Under an open overlay, the home element hears no key: its layer takes
/// no input.
#[test]
fn the_home_element_hears_nothing_under_an_overlay() {
    let mut harness = harness();
    send(&mut harness, Message::Overlay(true));
    letter(&mut harness, 'x');
    assert_eq!(used(&harness), 0);
}

/// A roll with no state, as a release build leaves for a kind the app did
/// not know, is safe on the key, home, and focus paths: it uses nothing,
/// and a focus on its part is dropped.
#[test]
fn a_roll_with_no_state_is_safe_on_every_key_path() {
    let mut harness = harness();
    harness.runtime_mut().customs_mut().empty(&roll());
    letter(&mut harness, 'x');
    tab(&mut harness, false);
    tab(&mut harness, false);
    assert_eq!(harness.runtime().focused(), Some(&roll()));
    letter(&mut harness, 'x');
    send(&mut harness, Message::FocusPart("a"));
    assert_eq!(harness.runtime().focused(), Some(&roll()));
    assert_eq!(used(&harness), 0);
    tab(&mut harness, false);
    assert_eq!(harness.runtime().focused(), Some(&id("after")));
}
