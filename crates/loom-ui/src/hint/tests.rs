//! Tests for hover text (ui-layer.md, section 5, rule 11): which shows,
//! where, Escape, and the pointer keeping it open.

use super::*;
use crate::access::{Access, Action, Role};
use crate::app::{App, Context, InputState, WindowSpec};
use crate::effect::Effects;
use crate::event::{DriverEvent, WindowState};
use crate::headless::Harness;
use crate::layout::tests::{Mono, STYLE};
use crate::routing::NoTarget;
use crate::view::{Layer, View};
use loom_ui_core::{Event, Key, KeyEvent, Modifiers, PointerButton, PointerEvent};
use std::time::Instant;

/// A Chooser shows the hovered element's text after a pointer move and the
/// focused one's after a focus move, whichever came last; with one of them
/// missing, the other's.
#[test]
fn the_latest_input_chooses_the_text() {
    let one = (UiId::panel("p").with("one"), "One".to_string());
    let two = (UiId::panel("p").with("two"), "Two".to_string());
    let mut chooser = Chooser::default();
    assert_eq!(chooser.choose(Some(one.clone()), None, None), Some(one.clone()));
    // The focus moves to two while the pointer rests on one.
    assert_eq!(
        chooser.choose(Some(one.clone()), Some(two.clone()), Some(&two.0)),
        Some(two.clone())
    );
    // Then the pointer moves.
    chooser.latest = Latest::Pointer;
    assert_eq!(
        chooser.choose(Some(one.clone()), Some(two.clone()), Some(&two.0)),
        Some(one.clone())
    );
    assert_eq!(chooser.choose(None, Some(two.clone()), Some(&two.0)), Some(two));
}

/// Escape hides what shows until another element's text would show; then
/// the hidden one shows again when it comes back.
#[test]
fn escape_hides_until_what_shows_changes() {
    let one = (UiId::panel("p").with("one"), "One".to_string());
    let two = (UiId::panel("p").with("two"), "Two".to_string());
    let mut chooser = Chooser::default();
    assert!(chooser.hide(Some(&one.0)));
    assert!(!chooser.hide(Some(&one.0)), "already hidden");
    assert!(!chooser.hide(None), "nothing showing");
    assert_eq!(chooser.choose(Some(one.clone()), None, None), None);
    assert_eq!(chooser.choose(Some(two.clone()), None, None), Some(two));
    assert_eq!(chooser.choose(Some(one.clone()), None, None), Some(one));
}

/// The box goes above its element, left edges aligned; moved in at the
/// window's right edge; and below the element when there is no room above.
#[test]
fn the_box_sits_above_its_element_or_below_it() {
    let window = Size::new(300.0, 200.0);
    let style = HintStyle {
        text: STYLE,
        ..HintStyle::default()
    };
    let id = UiId::panel("p").with("one");
    let low = Rect::new(Point::new(10.0, 150.0), Size::new(40.0, 20.0));
    let shown = place(id.clone(), "Hello".into(), low, window, &style, &mut Mono::default());
    assert_eq!(shown.rect.x, 10.0);
    assert_eq!(shown.rect.y + shown.rect.height, 150.0 - style.gap, "above, by the gap");
    assert!(
        shown.keeps(Point::new(20.0, 150.0 - style.gap / 2.0)),
        "the gap keeps it"
    );

    let right = Rect::new(Point::new(290.0, 150.0), Size::new(10.0, 20.0));
    let moved = place(id.clone(), "Hello".into(), right, window, &style, &mut Mono::default());
    assert_eq!(moved.rect.x + moved.rect.width, 300.0, "moved in at the right edge");

    let high = Rect::new(Point::new(10.0, 5.0), Size::new(40.0, 20.0));
    let below = place(id, "Hello".into(), high, window, &style, &mut Mono::default());
    assert_eq!(below.rect.y, 25.0 + style.gap, "below, with no room above");
}

/// What the app hears.
#[derive(Debug, Clone, PartialEq)]
enum Message {
    /// A click on the element with this UI ID.
    Clicked(UiId),
}

/// An app with a bar of two buttons at the bottom of a 300 by 200 window,
/// each with hover text, and a plain button with none.
struct Hints {
    heard: Vec<Message>,
}

/// A button keyed `key`, 40 wide, with `hover` text if any.
fn button(key: &str, hover: Option<&str>) -> Element {
    let element = Element::text(key, STYLE)
        .key(key)
        .width(Length::Fixed(40.0))
        .min_width(40.0)
        .focusable()
        .access(Access::new(Role::Button, key).action(Action::Click));
    match hover {
        Some(hover) => element.hover_text(hover),
        None => element,
    }
}

impl App for Hints {
    type Message = Message;

    fn update(&mut self, message: Message, _cx: &Context<'_, Message>) -> Effects<Message> {
        self.heard.push(message);
        Effects::none()
    }

    fn view(&self, view: &mut View) {
        view.panel_with(UiId::panel("bar"), Layer::Panels, &(), |_| {
            Element::column([
                Element::space(Length::Shrink, Length::FILL),
                Element::row([
                    button("a", Some("First")),
                    button("b", Some("Second")),
                    button("c", None),
                ]),
            ])
            .width(Length::FILL)
            .height(Length::FILL)
        });
    }

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }

    fn input(&self) -> InputState {
        InputState::default()
    }

    fn clicked(&self, id: &UiId) -> Option<Message> {
        Some(Message::Clicked(id.clone()))
    }
}

/// The app in a 300 by 200 window, laid out.
fn harness() -> Harness<Hints, NoTarget> {
    let window = WindowState::new(Size::new(300.0, 200.0));
    let mut harness = Harness::new(Hints { heard: Vec::new() }, NoTarget, window, Instant::now());
    harness.lay_out(&mut Mono::default());
    harness
}

/// Runs `event`, then lays out, as a driver does before it draws.
fn step(harness: &mut Harness<Hints, NoTarget>, event: DriverEvent) {
    harness.step(event);
    harness.lay_out(&mut Mono::default());
}

/// A pointer event at `(x, y)`.
fn at(x: f32, y: f32, event: PointerEvent) -> DriverEvent {
    DriverEvent::Input {
        event: Event::Pointer(event),
        pointer: Some(Point::new(x, y)),
    }
}

/// `key` going down, or coming up when `up`.
fn key(key: Key, up: bool) -> DriverEvent {
    let event = if up {
        KeyEvent::Released { key }
    } else {
        KeyEvent::Pressed {
            key,
            modifiers: Modifiers::NONE,
            repeat: false,
            text: None,
        }
    };
    DriverEvent::Input {
        event: Event::Key(event),
        pointer: None,
    }
}

/// The element whose hover text shows.
fn showing(harness: &Harness<Hints, NoTarget>) -> Option<String> {
    harness.runtime().hover_text().map(|(id, _)| id.to_string())
}

/// The bar's row is the last 16 pixels of the window.
const ROW: f32 = 192.0;

/// Hover text shows at once on hover and on keyboard focus; the latest
/// input wins; an element with none shows none; and the node's description
/// is the hover text.
#[test]
fn hover_text_follows_the_pointer_and_the_focus() {
    let mut harness = harness();
    assert_eq!(showing(&harness), None);
    step(&mut harness, at(20.0, ROW, PointerEvent::Moved));
    assert_eq!(showing(&harness).as_deref(), Some("bar/a"));
    // Tab focuses a while the pointer is on it; another Tab focuses b,
    // whose text then shows although the pointer is still on a.
    step(&mut harness, key(Key::Tab, false));
    step(&mut harness, key(Key::Tab, false));
    assert_eq!(showing(&harness).as_deref(), Some("bar/b"), "the focus moved last");
    step(&mut harness, at(25.0, ROW, PointerEvent::Moved));
    assert_eq!(
        showing(&harness).as_deref(),
        Some("bar/b"),
        "a move on the same element moves nothing"
    );
    step(&mut harness, at(100.0, ROW, PointerEvent::Moved));
    assert_eq!(
        showing(&harness).as_deref(),
        Some("bar/b"),
        "c has none, so the focus's shows"
    );
    step(&mut harness, at(20.0, ROW, PointerEvent::Moved));
    assert_eq!(showing(&harness).as_deref(), Some("bar/a"), "the pointer moved last");

    let (_, _, panels) = harness.parts();
    let access = panels.window.access_of(&UiId::panel("bar").with("a")).expect("a node");
    assert!(access.has_description(), "its hover text is its description");
}

/// Escape hides the hover text without moving the pointer or the focus,
/// until the pointer moves to another element; with nothing showing,
/// Escape is not taken.
#[test]
fn escape_hides_hover_text() {
    let mut harness = harness();
    step(&mut harness, at(20.0, ROW, PointerEvent::Moved));
    step(&mut harness, key(Key::Escape, false));
    assert_eq!(showing(&harness), None);
    step(&mut harness, at(60.0, ROW, PointerEvent::Moved));
    assert_eq!(showing(&harness).as_deref(), Some("bar/b"));
    step(&mut harness, at(20.0, ROW, PointerEvent::Moved));
    assert_eq!(showing(&harness).as_deref(), Some("bar/a"), "a shows again");
}

/// The pointer moving from the element onto its hover text keeps it
/// showing; a press there goes through, as if the text were not there.
#[test]
fn the_pointer_keeps_hover_text_open() {
    let mut harness = harness();
    step(&mut harness, at(20.0, ROW, PointerEvent::Moved));
    let (_, rect) = harness.runtime().hover_text().expect("a's text");
    let on_it = Point::new(rect.x + 2.0, rect.y + rect.height / 2.0);
    step(&mut harness, at(on_it.x, on_it.y, PointerEvent::Moved));
    assert_eq!(showing(&harness).as_deref(), Some("bar/a"), "on its text, it stays");
    assert_eq!(harness.runtime().hovered(), Some(&UiId::panel("bar").with("a")));
    let (_, _, panels) = harness.parts();
    let press = Event::Pointer(PointerEvent::Pressed(PointerButton::Left));
    assert!(
        !panels.over_target(&press, on_it),
        "a press there is not the hover text's"
    );
    step(&mut harness, at(150.0, 50.0, PointerEvent::Moved));
    assert_eq!(showing(&harness), None, "off both, it goes");
}

/// On a focused button with its hover text showing and Space held, Escape
/// does both: it cancels the click and hides the text, and goes no
/// further (section 3, rule 4; section 5, rule 11).
#[test]
fn escape_with_space_held_cancels_and_hides() {
    let mut harness = harness();
    step(&mut harness, key(Key::Tab, false));
    assert_eq!(showing(&harness).as_deref(), Some("bar/a"));
    step(&mut harness, key(Key::Space, false));
    step(&mut harness, key(Key::Escape, false));
    step(&mut harness, key(Key::Space, true));
    assert_eq!(showing(&harness), None, "hidden");
    assert!(harness.app().heard.is_empty(), "no click: {:?}", harness.app().heard);
    assert_eq!(
        harness.runtime().focused(),
        Some(&UiId::panel("bar").with("a")),
        "focus stays"
    );
}

/// Beside a narrow element, under its wider hover text, the pointer is on
/// the next button, not on the text or its element: that button takes the
/// hover, and its own text shows. The rectangle around the box and the
/// element would have kept the first one.
#[test]
fn a_neighbour_under_a_wide_hover_text_takes_the_hover() {
    let mut harness = harness();
    step(&mut harness, at(20.0, ROW, PointerEvent::Moved));
    let (_, rect) = harness.runtime().hover_text().expect("a's text");
    assert!(rect.x + rect.width > 42.0, "a's text reaches over b: {rect:?}");
    step(&mut harness, at(42.0, ROW, PointerEvent::Moved));
    assert_eq!(harness.runtime().hovered(), Some(&UiId::panel("bar").with("b")));
    assert_eq!(showing(&harness).as_deref(), Some("bar/b"));
}
