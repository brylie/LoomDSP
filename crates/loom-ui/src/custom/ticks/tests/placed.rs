//! Tests for [`CustomEvent::Placed`]: once after a layout that placed,
//! moved, or resized an element, in the next turn, before any deadline,
//! with its messages run in that turn (ui-layer.md, section 9, rule 4;
//! Brylie's decision, 2026-11-27).

use crate::app::{App, Context, WindowSpec};
use crate::custom::{Custom, CustomEvent, CustomKind, Moment, Out, Used};
use crate::effect::Effects;
use crate::element::{Element, Length};
use crate::event::{DriverEvent, WindowState};
use crate::headless::Harness;
use crate::layout::tests::Mono;
use crate::routing::NoTarget;
use crate::view::{Layer, View};
use loom_ui_core::{Event, Point, PointerButton, PointerEvent, Size, UiId};
use std::time::{Duration, Instant};

/// The wave's kind.
const WAVE: CustomKind = CustomKind::new("wave");

/// What the test app hears.
#[derive(Debug, Clone, PartialEq)]
enum Msg {
    /// The wave was placed at this width.
    Placed(f32),
    /// The wave's width changes to this.
    Width(f32),
    /// The space above the wave grows to this height.
    Gap(f32),
    /// The wave was placed this far from the window's top.
    Top(f32),
    /// The wave's deadline came.
    Due,
}

/// A view whose drawing depends on its size, as a waveform's does: it
/// hears what it was told, and asks for a deadline on a press.
#[derive(Default)]
struct WaveState {
    heard: Vec<CustomEvent>,
    deadline: Option<Instant>,
}

impl Custom<Waves> for WaveState {
    fn event(&mut self, _app: &Waves, event: &CustomEvent, moment: &Moment, out: &mut Out<'_, Msg>) -> Used {
        self.heard.push(event.clone());
        match event {
            CustomEvent::Placed => {
                out.send(Msg::Placed(moment.bounds.width));
                out.send(Msg::Top(moment.bounds.y));
            }
            CustomEvent::Pointer(PointerEvent::Pressed(_)) => self.deadline = Some(moment.now + DUE),
            CustomEvent::Deadline => {
                self.deadline = None;
                out.send(Msg::Due);
            }
            _ => {}
        }
        Used::Yes
    }

    fn next_deadline(&self, _app: &Waves) -> Option<Instant> {
        self.deadline
    }
}

/// How long after a press the wave's deadline comes.
const DUE: Duration = Duration::from_millis(10);

/// An app with one wave as wide as it says, under a space as tall as it
/// says.
struct Waves {
    width: f32,
    gap: f32,
    log: Vec<Msg>,
}

impl App for Waves {
    type Message = Msg;

    fn update(&mut self, message: Msg, _cx: &Context<'_, Msg>) -> Effects<Msg> {
        match message {
            Msg::Width(width) => self.width = width,
            Msg::Gap(gap) => self.gap = gap,
            _ => {}
        }
        self.log.push(message);
        Effects::none()
    }

    fn view(&self, view: &mut View) {
        let shape = (self.width.to_bits(), self.gap.to_bits());
        view.panel_with(UiId::panel("main"), Layer::Panels, &shape, |(width, gap)| {
            Element::column([
                Element::space(Length::Fixed(10.0), Length::Fixed(f32::from_bits(*gap))),
                Element::custom(WAVE)
                    .key("wave")
                    .width(Length::Fixed(f32::from_bits(*width)))
                    .height(Length::Fixed(100.0)),
            ])
        });
    }

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }

    fn make_custom(&self, _id: &UiId, kind: CustomKind) -> Option<Box<dyn Custom<Self>>> {
        (kind == WAVE).then(|| Box::new(WaveState::default()) as Box<dyn Custom<Self>>)
    }
}

/// The wave's UI ID.
fn wave() -> UiId {
    UiId::panel("main").with("wave")
}

/// A harness with a wave 200 wide, laid out once, and when it started.
fn harness() -> (Harness<Waves, NoTarget>, Instant) {
    let start = Instant::now();
    let app = Waves {
        width: 200.0,
        gap: 0.0,
        log: Vec::new(),
    };
    let window = WindowState::new(Size::new(400.0, 300.0));
    let mut harness = Harness::new(app, NoTarget, window, start);
    harness.lay_out(&mut Mono::default());
    (harness, start)
}

/// What the wave heard.
fn heard(harness: &Harness<Waves, NoTarget>) -> Vec<CustomEvent> {
    harness
        .runtime()
        .custom::<WaveState>(&wave())
        .map(|wave| wave.heard.clone())
        .unwrap_or_default()
}

/// The first layout places the wave: it hears that once, on the next
/// turn, and what it sends runs in that turn.
#[test]
fn the_first_layout_places_it_once() {
    let (mut harness, _) = harness();
    harness.step(DriverEvent::Woken);
    assert_eq!(heard(&harness), [CustomEvent::Placed]);
    assert_eq!(harness.app().log, [Msg::Placed(200.0), Msg::Top(0.0)]);
}

/// A layout that leaves it where it was tells it nothing.
#[test]
fn an_unchanged_layout_tells_it_nothing() {
    let (mut harness, _) = harness();
    harness.step(DriverEvent::Woken);
    harness.lay_out(&mut Mono::default());
    harness.step(DriverEvent::Woken);
    assert_eq!(heard(&harness), [CustomEvent::Placed]);
}

/// A resize tells it once, with its new bounds.
#[test]
fn a_resize_tells_it_once() {
    let (mut harness, _) = harness();
    harness.step(DriverEvent::Woken);
    harness.send(Msg::Width(300.0));
    harness.lay_out(&mut Mono::default());
    harness.step(DriverEvent::Woken);
    harness.lay_out(&mut Mono::default());
    harness.step(DriverEvent::Woken);
    assert_eq!(heard(&harness), [CustomEvent::Placed, CustomEvent::Placed]);
    assert_eq!(
        harness.app().log,
        [
            Msg::Placed(200.0),
            Msg::Top(0.0),
            Msg::Width(300.0),
            Msg::Placed(300.0),
            Msg::Top(0.0)
        ]
    );
}

/// Placed comes before a deadline due in the same turn.
#[test]
fn placed_comes_before_a_deadline_in_the_same_turn() {
    let (mut harness, _) = harness();
    harness.step(DriverEvent::Input {
        event: Event::Pointer(PointerEvent::Pressed(PointerButton::Left)),
        pointer: Some(Point::new(50.0, 50.0)),
    });
    harness.send(Msg::Width(250.0));
    harness.lay_out(&mut Mono::default());
    harness.advance(DUE * 2);
    let heard = heard(&harness);
    let tail = &heard[heard.len() - 2..];
    assert_eq!(tail, [CustomEvent::Placed, CustomEvent::Deadline]);
}

/// A move with the same size tells it once too, as a sibling growing in
/// front of it moves it down.
#[test]
fn a_move_tells_it_once() {
    let (mut harness, _) = harness();
    harness.step(DriverEvent::Woken);
    harness.send(Msg::Gap(40.0));
    harness.lay_out(&mut Mono::default());
    harness.step(DriverEvent::Woken);
    assert_eq!(heard(&harness), [CustomEvent::Placed, CustomEvent::Placed]);
    assert_eq!(
        harness.app().log,
        [
            Msg::Placed(200.0),
            Msg::Top(0.0),
            Msg::Gap(40.0),
            Msg::Placed(200.0),
            Msg::Top(40.0)
        ]
    );
}
