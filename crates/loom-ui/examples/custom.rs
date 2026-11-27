//! A custom element with no GPU: a level meter, run with no window.
//!
//! The meter is a custom element (docs/design/ui-layer.md, section 9). Its
//! state is kept by its UI ID, and it paints its bar through the runtime's
//! paint walk, at its place in its panel. When the level drops, the bar
//! falls at a steady rate: the meter asks for animation frames until it
//! reaches the level, and each frame paints and runs no update. Its node
//! comes from the view, with the level the app holds, rounded so a screen
//! reader is not told every small change (section 7, rule 7).
//!
//! The example sends a peak of -6 dB, then a drop to -30 dB, and prints
//! the bar's width at each frame and the node a screen reader reads.
//!
//! Run it with `cargo run -p loom-ui --example custom`.

use loom_ui::access::{Access, Role, Value};
use loom_ui::headless::Harness;
use loom_ui::paint::{CheckLook, Painter, PanelStyle, Quad, Segment, Shadow, TextRun};
use loom_ui::{
    App, Context, Custom, CustomEvent, CustomKind, DriverEvent, Effects, Element, Layer, Length, Moment, NoTarget, Out,
    RingStyle, Used, View, WindowSpec, WindowState,
};
use loom_ui_core::{Color, IdTable, Rect, Size, TextMeasure, TextStyle, UiId};
use std::time::{Duration, Instant};

/// The meter's kind.
const METER: CustomKind = CustomKind::new("meter");

/// The lowest level the meter shows, in dB.
const FLOOR: f32 = -60.0;

/// How fast the bar falls, in dB per second, chosen for this example so
/// the fall is easy to watch. It is not taken from a standard: a peak
/// programme meter falls 20 dB in about 1.7 s (IEC 60268-10, Type I),
/// about 12 dB a second, and a real meter module cites the standard it
/// follows.
const FALL: f32 = 20.0;

/// The bar's colour.
const BAR: Color = Color::rgb(0.3, 0.8, 0.4);

/// The level the app holds, rounded to half a dB for the view, so the
/// meter's node changes no faster than a screen reader should hear.
fn rounded(db: f32) -> i32 {
    (db * 2.0).round() as i32
}

/// The meter's state: the level the bar shows now, and when it last moved.
struct MeterState {
    shown: f32,
    last: Option<Instant>,
}

impl MeterState {
    /// Where the bar is going: the app's level, never below the floor.
    fn target(app: &Meters) -> f32 {
        app.level.max(FLOOR)
    }
}

impl Custom<Meters> for MeterState {
    fn event(&mut self, app: &Meters, event: &CustomEvent, moment: &Moment, _out: &mut Out<'_, Message>) -> Used {
        if *event == CustomEvent::Frame {
            // A rise shows at once; a drop falls at the steady rate.
            let since = self.last.map_or(0.0, |last| (moment.now - last).as_secs_f32());
            let target = Self::target(app);
            self.shown = target.max(self.shown - FALL * since);
            // Once it settles it forgets the time, so a drop after a quiet
            // spell starts falling from the frame it began, not with a jump.
            self.last = (self.shown != target).then_some(moment.now);
        }
        Used::No
    }

    fn animating(&self, app: &Meters) -> bool {
        self.shown != Self::target(app)
    }

    fn paint(&self, _app: &Meters, bounds: Rect, painter: &mut dyn loom_ui::paint::TextPainter) {
        let full = (self.shown - FLOOR) / -FLOOR;
        painter.quad(Quad {
            rect: Rect::new(bounds.position(), Size::new(bounds.width * full, bounds.height)),
            fill: BAR,
            border: Color::TRANSPARENT,
            border_width: 0.0,
            radius: 0.0,
        });
    }
}

/// The example's messages.
#[derive(Debug, Clone)]
enum Message {
    /// The level, in dB.
    Level(f32),
}

/// The example app: the level its audio last reported.
struct Meters {
    level: f32,
}

impl App for Meters {
    type Message = Message;

    fn update(&mut self, message: Message, _cx: &Context<'_, Message>) -> Effects<Message> {
        let Message::Level(db) = message;
        self.level = db;
        Effects::none()
    }

    fn view(&self, view: &mut View) {
        view.panel_with(UiId::panel("mixer"), Layer::Panels, &rounded(self.level), |&half| {
            let db = half as f64 / 2.0;
            Element::custom(METER)
                .key("meter")
                .width(Length::Fixed(200.0))
                .height(Length::Fixed(12.0))
                .access(
                    Access::new(Role::Meter, "Level")
                        .value(Value::Number {
                            value: db,
                            min: f64::from(FLOOR),
                            max: 0.0,
                            step: None,
                        })
                        .description(format!("{db:.1} dB")),
                )
        });
    }

    fn window(&self) -> WindowSpec {
        WindowSpec {
            title: "Meter".into(),
            min_size: None,
        }
    }

    fn make_custom(&self, _id: &UiId, kind: CustomKind) -> Option<Box<dyn Custom<Self>>> {
        (kind == METER).then(|| {
            Box::new(MeterState {
                shown: FLOOR,
                last: None,
            }) as Box<dyn Custom<Self>>
        })
    }
}

/// Measures every character 7 pixels wide, and keeps the width of the
/// last bar painted.
#[derive(Default)]
struct Printer {
    bar: f32,
}

impl TextMeasure for Printer {
    fn measure(&mut self, text: &str, _style: TextStyle, _wrap_width: Option<f32>) -> Size {
        Size::new(text.chars().count() as f32 * 7.0, 16.0)
    }
}

impl Painter for Printer {
    fn text(&mut self, _run: &TextRun<'_>) {}
    fn quad(&mut self, quad: Quad) {
        if quad.fill == BAR {
            self.bar = quad.rect.width;
        }
    }
    fn segment(&mut self, _segment: Segment) {}
    fn shadow(&mut self, _shadow: Shadow) {}
    fn begin_layer(&mut self, _clip: Rect) {}
    fn end_layer(&mut self) {}
}

/// A panel style; the meter paints its own colours.
fn style() -> PanelStyle {
    let grey = Color::rgb(0.5, 0.5, 0.5);
    PanelStyle {
        text: Color::WHITE,
        caret: Color::WHITE,
        selection: grey,
        ring: RingStyle {
            color: Color::WHITE,
            width: 2.0,
            offset: 2.0,
        },
        quiet: grey,
        mark: grey,
        primary: BAR,
        track: grey,
        check: CheckLook {
            edge: grey,
            fill: BAR,
            mark: Color::BLACK,
        },
    }
}

/// Runs frames 16 ms apart while the meter animates, printing the bar's
/// width at each.
fn run_frames(harness: &mut Harness<Meters, NoTarget>) {
    let mut frames = 0;
    while harness.next().frames {
        harness.advance(Duration::from_millis(16));
        harness.step(DriverEvent::Frame);
        harness.lay_out(&mut Printer::default());
        let mut printer = Printer::default();
        let (_, _, mut panels) = harness.parts();
        panels.paint(Layer::Panels, &mut printer, &style());
        frames += 1;
        if frames % 10 == 1 || !harness.next().frames {
            println!("  frame {frames:3}: bar {:5.1} px", printer.bar);
        }
    }
    println!("  stopped after {frames} frames");
}

fn main() {
    let window = WindowState::new(Size::new(240.0, 40.0));
    let mut harness = Harness::new(Meters { level: FLOOR }, NoTarget, window, Instant::now());
    harness.lay_out(&mut Printer::default());

    println!("A peak of -6 dB:");
    harness.send(Message::Level(-6.0));
    run_frames(&mut harness);
    println!("A drop to -30 dB:");
    harness.send(Message::Level(-30.0));
    harness.lay_out(&mut Printer::default());
    run_frames(&mut harness);

    let mut ids = IdTable::default();
    let nodes = harness.runtime().access_nodes(&mut ids);
    for (_, node) in &nodes.nodes {
        println!(
            "Node: {:?} \"{}\", {}",
            node.role(),
            node.label().unwrap_or(""),
            node.description().unwrap_or("")
        );
    }
}
