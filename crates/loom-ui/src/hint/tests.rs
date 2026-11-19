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
/// focused one's after a focus move, by the focus's move count: whichever
/// came last; with one of them missing, the other's.
#[test]
fn the_latest_input_chooses_the_text() {
    let one = (UiId::panel("p").with("one"), "One".to_string());
    let two = (UiId::panel("p").with("two"), "Two".to_string());
    let mut chooser = Chooser::default();
    chooser.pointer_moved(0);
    assert_eq!(chooser.choose(Some(one.clone()), None, 1, 0), Some(one.clone()));
    // The focus moves to two (move 1) while the pointer rests on one.
    assert_eq!(
        chooser.choose(Some(one.clone()), Some(two.clone()), 1, 1),
        Some(two.clone())
    );
    // Then the pointer moves.
    chooser.pointer_moved(1);
    assert_eq!(
        chooser.choose(Some(one.clone()), Some(two.clone()), 1, 1),
        Some(one.clone())
    );
    assert_eq!(chooser.choose(None, Some(two.clone()), 2, 1), Some(two));
}

/// Escape hides what shows until the focus moves or the hover changes;
/// then it shows again, even when the same element would show.
#[test]
fn escape_hides_until_the_focus_or_the_hover_moves() {
    let one = (UiId::panel("p").with("one"), "One".to_string());
    let mut chooser = Chooser::default();
    chooser.pointer_moved(0);
    assert!(chooser.hide(Some(&one.0), 1, 0));
    assert!(!chooser.hide(Some(&one.0), 1, 0), "already hidden");
    assert!(!chooser.hide(None, 0, 0), "nothing showing");
    assert_eq!(chooser.choose(Some(one.clone()), None, 1, 0), None);
    // The pointer leaves one; the focus, on one, still offers its text.
    assert_eq!(chooser.choose(None, Some(one.clone()), 2, 0), Some(one.clone()));
    assert!(chooser.hide(Some(&one.0), 2, 0));
    // The focus moves away and back: the count moved, so it shows again.
    assert_eq!(chooser.choose(None, Some(one.clone()), 2, 2), Some(one));
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
    let shown = place(
        id.clone(),
        "Hello".into(),
        low,
        window,
        &style,
        &mut Mono::default(),
        Layer::Overlays,
    );
    assert_eq!(shown.rect.x, 10.0);
    assert_eq!(shown.rect.y + shown.rect.height, 150.0 - style.gap, "above, by the gap");
    assert!(
        shown.keeps(Point::new(20.0, 150.0 - style.gap / 2.0), low),
        "the gap keeps it"
    );

    let right = Rect::new(Point::new(290.0, 150.0), Size::new(10.0, 20.0));
    let moved = place(
        id.clone(),
        "Hello".into(),
        right,
        window,
        &style,
        &mut Mono::default(),
        Layer::Overlays,
    );
    assert_eq!(moved.rect.x + moved.rect.width, 300.0, "moved in at the right edge");

    let high = Rect::new(Point::new(10.0, 5.0), Size::new(40.0, 20.0));
    let below = place(
        id,
        "Hello".into(),
        high,
        window,
        &style,
        &mut Mono::default(),
        Layer::Overlays,
    );
    assert_eq!(below.rect.y, 25.0 + style.gap, "below, with no room above");
}

/// What the app hears.
#[derive(Debug, Clone, PartialEq)]
enum Message {
    /// A click on the element with this UI ID.
    Clicked(UiId),
    /// Set the input layer.
    Input(Layer),
    /// Move the bar's buttons right by 100, or back.
    Shift(bool),
    /// Removes the hovered source.
    Remove,
    /// Show a question with a button that has hover text, or not.
    Question(bool),
}

/// An app with a bar of two buttons at the bottom of a 300 by 200 window,
/// each with hover text, and a plain button with none.
struct Hints {
    heard: Vec<Message>,
    /// The input layer.
    input: Layer,
    /// The bar's buttons are 100 to the right.
    shifted: bool,
    /// Whether button a is present.
    removed: bool,
    /// A question shows.
    question: bool,
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
        match &message {
            Message::Input(layer) => self.input = *layer,
            Message::Shift(on) => self.shifted = *on,
            Message::Remove => self.removed = true,
            Message::Question(on) => self.question = *on,
            Message::Clicked(_) => {}
        }
        self.heard.push(message);
        Effects::none()
    }

    fn view(&self, view: &mut View) {
        view.panel_with(
            UiId::panel("bar"),
            Layer::Panels,
            &(self.shifted, self.removed),
            |(shifted, removed)| {
                let gap = if *shifted { 100.0 } else { 0.0 };
                Element::column([
                    Element::space(Length::Shrink, Length::FILL),
                    Element::row([
                        Element::space(Length::Fixed(gap), Length::Fixed(1.0)).min_width(gap),
                        if *removed {
                            Element::space(Length::Fixed(40.0), Length::Shrink)
                        } else {
                            button("a", Some("First"))
                        },
                        button("b", Some("Second")),
                        button("c", None),
                    ]),
                ])
                .width(Length::FILL)
                .height(Length::FILL)
            },
        );
        if self.question {
            view.panel_with(UiId::panel("ask"), Layer::Question, &(), |_| {
                Element::column([
                    Element::space(Length::Shrink, Length::Fixed(80.0)),
                    Element::row([button("ok", Some("Answer"))]),
                ])
                .width(Length::FILL)
                .height(Length::FILL)
                .background(loom_ui_core::Color::rgb(0.2, 0.2, 0.2))
                .opaque()
            });
        }
    }

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }

    fn input(&self) -> InputState {
        InputState {
            layer: self.input,
            ..InputState::default()
        }
    }

    fn clicked(&self, id: &UiId) -> Option<Message> {
        Some(Message::Clicked(id.clone()))
    }
}

/// The app in a 300 by 200 window, laid out.
fn harness() -> Harness<Hints, NoTarget> {
    let window = WindowState::new(Size::new(300.0, 200.0));
    let app = Hints {
        heard: Vec::new(),
        input: Layer::Canvas,
        shifted: false,
        removed: false,
        question: false,
    };
    let mut harness = Harness::new(app, NoTarget, window, Instant::now());
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
        Some("bar/a"),
        "a move on the hovered element makes the pointer the latest input again"
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

/// A hover round trip between layouts ends Escape suppression.
#[test]
fn leaving_and_returning_before_layout_restores_the_hint() {
    let mut harness = harness();
    step(&mut harness, at(20.0, ROW, PointerEvent::Moved));
    step(&mut harness, key(Key::Escape, false));
    harness.step(at(250.0, 20.0, PointerEvent::Moved));
    harness.step(at(20.0, ROW, PointerEvent::Moved));
    harness.lay_out(&mut Mono::default());
    assert_eq!(showing(&harness).as_deref(), Some("bar/a"));
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

/// The order of inputs is kept as they happen, not when the hover text is
/// laid out: Tab and then a pointer move before one layout show the
/// pointer's element's text, and the other way round the focused one's.
#[test]
fn the_order_of_two_inputs_in_one_frame_counts() {
    let mut pointer_last = harness();
    pointer_last.step(key(Key::Tab, false));
    pointer_last.step(at(60.0, ROW, PointerEvent::Moved));
    pointer_last.lay_out(&mut Mono::default());
    assert_eq!(
        showing(&pointer_last).as_deref(),
        Some("bar/b"),
        "the pointer moved last"
    );

    let mut focus_last = harness();
    focus_last.step(at(60.0, ROW, PointerEvent::Moved));
    focus_last.step(key(Key::Tab, false));
    focus_last.lay_out(&mut Mono::default());
    assert_eq!(showing(&focus_last).as_deref(), Some("bar/a"), "the focus moved last");
}

/// A move within the element already hovered is still the latest input:
/// hover b, Tab to a (a's text), then move inside b: b's text shows.
#[test]
fn a_move_within_the_hovered_element_is_the_latest_input() {
    let mut harness = harness();
    step(&mut harness, at(60.0, ROW, PointerEvent::Moved));
    step(&mut harness, key(Key::Tab, false));
    assert_eq!(showing(&harness).as_deref(), Some("bar/a"));
    assert!(harness.step(at(65.0, ROW, PointerEvent::Moved)).draw);
    harness.lay_out(&mut Mono::default());
    assert_eq!(showing(&harness).as_deref(), Some("bar/b"));
}

/// With the pointer and the focus both on a, Escape hides a's text; the
/// pointer leaving a ends that, so the focus's text, a's, shows again.
#[test]
fn escape_ends_when_the_pointer_leaves() {
    let mut harness = harness();
    step(&mut harness, key(Key::Tab, false));
    step(&mut harness, at(20.0, ROW, PointerEvent::Moved));
    step(&mut harness, key(Key::Escape, false));
    assert_eq!(showing(&harness), None);
    step(&mut harness, at(250.0, 20.0, PointerEvent::Moved));
    assert_eq!(showing(&harness).as_deref(), Some("bar/a"), "the focus's text, back");
}

/// A hover text whose element is below the input layer now (a question
/// opened before the next draw) keeps no move, so the move reaches what
/// takes input.
#[test]
fn a_hint_below_the_input_layer_keeps_no_move() {
    let mut harness = harness();
    step(&mut harness, at(20.0, ROW, PointerEvent::Moved));
    let (_, rect) = harness.runtime().hover_text().expect("a's text");
    let on_it = Point::new(rect.x + 2.0, rect.y + rect.height / 2.0);
    harness.send(Message::Input(Layer::Question));
    let (_, _, panels) = harness.parts();
    assert!(!panels.over_target(&Event::Pointer(PointerEvent::Moved), on_it));
}

/// A layout that moves the hovered element away from a pointer that did
/// not move ends the hover, so its hover text does not follow it.
#[test]
fn a_layout_that_moves_the_hovered_element_away_ends_the_hover() {
    let mut harness = harness();
    step(&mut harness, at(20.0, ROW, PointerEvent::Moved));
    assert_eq!(showing(&harness).as_deref(), Some("bar/a"));
    harness.send(Message::Shift(true));
    harness.lay_out(&mut Mono::default());
    assert_eq!(harness.runtime().hovered(), None);
    assert_eq!(showing(&harness), None);
}

/// A hover text whose element is in a question is painted in the question
/// layer, so the question's own panel never covers it.
#[test]
fn a_questions_hover_text_is_painted_above_the_question() {
    let mut harness = harness();
    harness.send(Message::Question(true));
    harness.send(Message::Input(Layer::Question));
    harness.lay_out(&mut Mono::default());
    step(&mut harness, at(20.0, 88.0, PointerEvent::Moved));
    assert_eq!(showing(&harness).as_deref(), Some("ask/ok"));
    let (_, _, panels) = harness.parts();
    assert_eq!(panels.window.hint().map(|hint| hint.layer), Some(Layer::Question));
}

/// A press supplies the current pointer point even without a preceding move.
#[test]
fn a_press_replaces_an_older_pointer_position() {
    let mut harness = harness();
    step(&mut harness, at(20.0, ROW, PointerEvent::Moved));
    step(&mut harness, at(60.0, ROW, PointerEvent::Pressed(PointerButton::Left)));
    assert_eq!(harness.runtime().hovered(), Some(&UiId::panel("bar").with("b")));
    assert_eq!(showing(&harness).as_deref(), Some("bar/b"));
}

/// The old strip cannot preserve hover when its anchor moves sideways.
#[test]
fn a_relayout_moves_the_hint_keep_strip() {
    let mut harness = harness();
    step(&mut harness, at(20.0, ROW, PointerEvent::Moved));
    let (_, hint) = harness.runtime().hover_text().expect("hover text");
    let y = hint.y + hint.height + 1.0;
    step(&mut harness, at(20.0, y, PointerEvent::Moved));
    assert_eq!(showing(&harness).as_deref(), Some("bar/a"));
    harness.send(Message::Shift(true));
    harness.lay_out(&mut Mono::default());
    assert!(harness.runtime().hovered().is_none());
    assert_eq!(showing(&harness), None);
}

/// A removed source cannot stay hovered even while the pointer is on its old box.
#[test]
fn removing_a_hint_source_ends_its_hover() {
    let mut harness = harness();
    step(&mut harness, at(20.0, ROW, PointerEvent::Moved));
    let (_, hint) = harness.runtime().hover_text().expect("hover text");
    step(&mut harness, at(hint.center_x(), hint.center_y(), PointerEvent::Moved));
    harness.send(Message::Remove);
    harness.lay_out(&mut Mono::default());
    assert!(harness.runtime().hovered().is_none());
    assert_eq!(showing(&harness), None);
}

/// Movement on the currently showing hint's source outranks focus before a layout.
#[test]
fn a_move_on_the_kept_source_is_the_latest_input() {
    let mut harness = harness();
    step(&mut harness, at(60.0, ROW, PointerEvent::Moved));
    assert_eq!(showing(&harness).as_deref(), Some("bar/b"));
    harness.step(key(Key::Tab, false));
    harness.step(at(65.0, ROW, PointerEvent::Moved));
    harness.lay_out(&mut Mono::default());
    assert_eq!(showing(&harness).as_deref(), Some("bar/b"));
}
