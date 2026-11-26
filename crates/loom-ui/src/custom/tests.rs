//! Tests for custom elements: their state lasts by UI ID, follows each
//! layout's bounds, and is read back in the same turn (ui-layer.md,
//! section 9, rule 1).

use crate::app::{App, Context, WindowSpec};
use crate::custom::{Custom, CustomKind};
use crate::effect::Effects;
use crate::element::{Element, Length};
use crate::event::WindowState;
use crate::headless::Harness;
use crate::layout::Layout;
use crate::layout::tests::Mono;
use crate::routing::NoTarget;
use crate::view::{Layer, View};
use loom_ui_core::{Point, Rect, Size, UiId};
use std::cell::Cell;
use std::time::Instant;

/// The kind the test app shows.
const PAD: CustomKind = CustomKind::new("pad");

/// A second kind, to check a kind change makes the state again.
const DIAL: CustomKind = CustomKind::new("dial");

/// A pad's state: which one the app made, counting from 1.
struct PadState {
    made: u32,
}

impl Custom<Pads> for PadState {}

/// A dial's state.
struct DialState;

impl Custom<Pads> for DialState {}

/// What the test app hears.
#[derive(Debug, Clone, PartialEq)]
enum Message {
    /// Show the pad under this key, or none.
    Key(Option<&'static str>),
    /// Show the pad with this width.
    Width(f32),
    /// Show the pad as this kind.
    Kind(CustomKind),
    /// Change nothing the view shows, so only the label is rebuilt.
    Label(u32),
    /// Read the pad's state, and note what it was.
    Read,
}

/// An app with a label and one custom element, in one panel.
struct Pads {
    key: Option<&'static str>,
    width: f32,
    kind: CustomKind,
    label: u32,
    /// How many states the app has made.
    made: Cell<u32>,
    /// What [`Message::Read`] found: the pad's state number, and whether
    /// reading it as the wrong type gave nothing.
    read: Vec<(Option<u32>, bool)>,
}

impl App for Pads {
    type Message = Message;

    fn update(&mut self, message: Message, cx: &Context<'_, Message>) -> Effects<Message> {
        match message {
            Message::Key(key) => self.key = key,
            Message::Width(width) => self.width = width,
            Message::Kind(kind) => self.kind = kind,
            Message::Label(label) => self.label = label,
            Message::Read => {
                let id = UiId::panel("main").with(self.key.unwrap_or("pad"));
                let made = cx.custom::<PadState>(&id).map(|state| state.made);
                let wrong = cx.custom::<DialState>(&id).is_none();
                self.read.push((made, wrong));
            }
        }
        Effects::none()
    }

    fn view(&self, view: &mut View) {
        let model = (self.key, self.width, self.kind, self.label);
        view.panel_with(
            UiId::panel("main"),
            Layer::Panels,
            &model,
            |&(key, width, kind, label)| {
                let label = Element::space(Length::Fixed(10.0 + label as f32), Length::Fixed(20.0));
                let mut children = vec![label];
                if let Some(key) = key {
                    children.push(
                        Element::custom(kind)
                            .key(key)
                            .width(Length::Fixed(width))
                            .height(Length::Fixed(50.0)),
                    );
                }
                Element::column(children)
            },
        );
    }

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }

    fn make_custom(&self, _id: &UiId, kind: CustomKind) -> Option<Box<dyn Custom<Self>>> {
        self.made.set(self.made.get() + 1);
        match kind {
            PAD => Some(Box::new(PadState { made: self.made.get() })),
            DIAL => Some(Box::new(DialState)),
            _ => None,
        }
    }
}

/// A harness with the pad keyed `pad`, 100 wide, laid out in a 400 by 200
/// window.
fn harness() -> Harness<Pads, NoTarget> {
    let app = Pads {
        key: Some("pad"),
        width: 100.0,
        kind: PAD,
        label: 0,
        made: Cell::new(0),
        read: Vec::new(),
    };
    let window = WindowState::new(Size::new(400.0, 200.0));
    let mut harness = Harness::new(app, NoTarget, window, Instant::now());
    harness.lay_out(&mut Mono::default());
    harness
}

/// Sends `message`, then lays the window out, as a driver does before it
/// draws.
fn send(harness: &mut Harness<Pads, NoTarget>, message: Message) {
    harness.send(message);
    harness.lay_out(&mut Mono::default());
}

/// The pad's UI ID under `key`.
fn pad(key: &str) -> UiId {
    UiId::panel("main").with(key)
}

/// The pad's state number under `key`, as the runtime keeps it.
fn made(harness: &Harness<Pads, NoTarget>, key: &str) -> Option<u32> {
    harness.runtime().custom::<PadState>(&pad(key)).map(|state| state.made)
}

/// The first layout that finds the element makes its state, once.
#[test]
fn the_first_layout_makes_the_state() {
    let harness = harness();
    assert_eq!(harness.app().made.get(), 1);
    assert_eq!(made(&harness, "pad"), Some(1));
}

/// Its state lasts across rebuilds of its panel and new layouts.
#[test]
fn the_state_lasts_across_rebuilds() {
    let mut harness = harness();
    send(&mut harness, Message::Label(5));
    send(&mut harness, Message::Label(9));
    assert_eq!(harness.app().made.get(), 1);
    assert_eq!(made(&harness, "pad"), Some(1));
}

/// Its bounds follow each layout, in the window, while its state lasts.
#[test]
fn the_bounds_follow_each_layout() {
    let mut harness = harness();
    let bounds = harness.runtime().custom_bounds(&pad("pad"));
    assert_eq!(bounds, Some(Rect::new(Point::new(0.0, 20.0), Size::new(100.0, 50.0))));
    send(&mut harness, Message::Width(140.0));
    send(&mut harness, Message::Label(4));
    let bounds = harness.runtime().custom_bounds(&pad("pad"));
    assert_eq!(bounds, Some(Rect::new(Point::new(0.0, 20.0), Size::new(140.0, 50.0))));
    assert_eq!(made(&harness, "pad"), Some(1));
}

/// The first layout that does not find its UI ID drops the state, and the
/// next that finds it makes a new one.
#[test]
fn leaving_the_layout_drops_the_state() {
    let mut harness = harness();
    send(&mut harness, Message::Key(None));
    assert_eq!(made(&harness, "pad"), None);
    assert_eq!(harness.runtime().custom_bounds(&pad("pad")), None);
    send(&mut harness, Message::Key(Some("pad")));
    assert_eq!(made(&harness, "pad"), Some(2));
}

/// A new key is a new element: the app resets the state by its choice of
/// key, as a key that holds a session's number would on New or Open.
#[test]
fn a_new_key_resets_the_state() {
    let mut harness = harness();
    send(&mut harness, Message::Key(Some("pad-2")));
    assert_eq!(made(&harness, "pad"), None);
    assert_eq!(made(&harness, "pad-2"), Some(2));
}

/// A new kind under the same UI ID makes the state again, since the old
/// state is of the wrong type.
#[test]
fn a_new_kind_makes_the_state_again() {
    let mut harness = harness();
    send(&mut harness, Message::Kind(DIAL));
    assert_eq!(made(&harness, "pad"), None);
    assert!(harness.runtime().custom::<DialState>(&pad("pad")).is_some());
    assert_eq!(harness.app().made.get(), 2);
}

/// `update` reads the state by its type through its context, and a read
/// as another type gives nothing.
#[test]
fn update_reads_the_state() {
    let mut harness = harness();
    send(&mut harness, Message::Read);
    assert_eq!(harness.app().read, [(Some(1), true)]);
}

/// A custom element with no key has no UI ID to keep its state by, which
/// a debug build refuses at layout.
#[test]
#[should_panic(expected = "an element in panel p breaks a rule: it has no key")]
fn a_custom_element_needs_a_key() {
    let mut layout = Layout::new(UiId::panel("p"));
    layout.rebuild(Element::custom(PAD));
}

/// A kind the app does not know gets no state, which a debug build
/// refuses.
#[test]
#[should_panic(expected = "the app made no state for the custom element")]
fn an_unknown_kind_panics_in_a_debug_build() {
    let mut harness = harness();
    send(&mut harness, Message::Kind(CustomKind::new("unknown")));
}
