//! Focus and a screen reader's nodes, with no window.
//!
//! Two panels: a toolbar at the top and a status bar at the bottom. The
//! view lists the status bar first, so it is drawn first, but Tab follows
//! reading order, so the toolbar's stops come first (docs/design/ui-layer.md,
//! section 6, rule 2). Each element that a screen reader should meet says
//! what it is with an `Access`, and its node comes from that same element,
//! with the bounds the layout gave it (section 7, rule 1).
//!
//! Run it with `cargo run -p loom-ui --example focus`.

use loom_ui::access::{Access, Action, Role, Value};
use loom_ui::headless::Harness;
use loom_ui::{App, Context, DriverEvent, Effects, Element, Layer, Length, NoTarget, View, WindowSpec, WindowState};
use loom_ui_core::{Code, Event, IdTable, Key, KeyEvent, Modifiers, Point, Size, TextMeasure, TextStyle, UiId, Weight};
use std::time::Instant;

/// How text is set in this example.
const STYLE: TextStyle = TextStyle {
    size: 13.0,
    line_height: 1.25,
    weight: Weight::Regular,
};

/// Measures every character 7 pixels wide, so the example needs no fonts.
struct Monospace;

impl TextMeasure for Monospace {
    fn measure(&mut self, text: &str, _style: TextStyle, _wrap_width: Option<f32>) -> Size {
        Size::new(text.chars().count() as f32 * 7.0, 16.0)
    }
}

/// A button: a focus stop that a screen reader can press.
fn button(key: &str, label: &str) -> Element {
    Element::stack([Element::text(label, STYLE)])
        .key(key)
        .padding(4.0)
        .focusable()
        .access(Access::new(Role::Button, label).action(Action::Click))
}

/// The toolbar: two buttons and a volume slider, across the top.
pub fn toolbar(volume: &u8) -> Element {
    let slider = Element::space(Length::Fixed(120.0), Length::Fixed(20.0))
        .key("volume")
        .focusable()
        .access(
            Access::new(Role::Slider, "Volume")
                .value(Value::Number {
                    value: f64::from(*volume),
                    min: 0.0,
                    max: 100.0,
                    step: Some(1.0),
                })
                .description(format!("{volume} %"))
                .action(Action::Increment)
                .action(Action::Decrement)
                .action(Action::SetValue),
        );
    Element::row([button("open", "Open"), button("save", "Save"), slider])
        .width(Length::FILL)
        .spacing(6.0)
        .access(Access::new(Role::Group, "Toolbar"))
        .key("bar")
}

/// The status bar: a line of text and a mute switch, along the bottom.
pub fn status_bar(muted: &bool) -> Element {
    let switch = Element::space(Length::Fixed(30.0), Length::Fixed(16.0))
        .key("mute")
        .focusable()
        .access(
            Access::new(Role::Switch, "Mute")
                .value(Value::On(*muted))
                .action(Action::Click),
        );
    let bar = Element::row([
        Element::text("Ready", STYLE).key("text").access(Access::label("Ready")),
        switch,
    ])
    .width(Length::FILL)
    .spacing(6.0)
    .key("bar");
    Element::column([Element::space(Length::Shrink, Length::FILL), bar])
        .width(Length::FILL)
        .height(Length::FILL)
}

/// The example's messages.
#[derive(Debug)]
enum Message {
    /// Make the nodes from the last layout and keep them.
    Describe,
}

/// The example app: two panels' models, and the nodes last made.
#[derive(Default)]
struct Example {
    volume: u8,
    muted: bool,
    ids: IdTable,
    nodes: Vec<String>,
}

impl App for Example {
    type Message = Message;

    fn update(&mut self, message: Message, cx: &Context<'_, Message>) -> Effects<Message> {
        match message {
            Message::Describe => {
                let nodes = cx.access_nodes(&mut self.ids);
                self.nodes = nodes
                    .nodes
                    .iter()
                    .map(|(_, node)| {
                        let name = node.label().or(node.value()).unwrap_or("");
                        format!("{:?} \"{name}\"", node.role())
                    })
                    .collect();
                Effects::none()
            }
        }
    }

    fn view(&self, view: &mut View) {
        // Drawn first, read last.
        view.panel_with(UiId::panel("status"), Layer::Panels, &self.muted, status_bar);
        view.panel_with(UiId::panel("toolbar"), Layer::Panels, &self.volume, toolbar);
    }

    fn window(&self) -> WindowSpec {
        WindowSpec {
            title: "Focus".into(),
            min_size: None,
        }
    }
}

/// A Tab press, or Shift+Tab with `back`.
fn tab(back: bool) -> DriverEvent {
    DriverEvent::Input {
        event: Event::Key(KeyEvent::Pressed {
            key: Key::Tab,
            modifiers: Modifiers {
                shift: back,
                ..Modifiers::NONE
            },
            repeat: false,
            text: None,
            code: Code::Other,
        }),
        pointer: None,
    }
}

fn main() {
    let window = WindowState::new(Size::new(480.0, 120.0));
    let app = Example {
        volume: 70,
        ..Example::default()
    };
    let mut harness = Harness::new(app, NoTarget, window, Instant::now());
    harness.lay_out(&mut Monospace);

    println!("Focus order:");
    for stop in harness.runtime().focus_stops() {
        println!("  {stop}");
    }

    let pressed = harness
        .focus_at(Point::new(10.0, 10.0))
        .expect("Open is under the point");
    let ring = harness.runtime().focus_ring();
    println!(
        "A press at (10, 10) focuses {pressed}, and shows no ring: {}",
        ring.is_none()
    );
    for _ in 0..3 {
        harness.step(tab(false));
        let (id, ring) = harness.runtime().focus_ring().expect("Tab shows a ring");
        println!(
            "Tab: {id}, ring at {} x {} from ({}, {})",
            ring.width, ring.height, ring.x, ring.y
        );
    }
    harness.step(tab(true));
    let focused = harness.runtime().focused().expect("Shift+Tab stays inside");
    println!("Shift+Tab: {focused}");

    harness.send(Message::Describe);
    println!("Nodes:");
    for node in &harness.app().nodes {
        println!("  {node}");
    }
}
