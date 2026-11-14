//! Lays out a small toolbar with no window and no GPU, and prints where
//! each part went and what a click at a few points would hit.
//!
//! ```sh
//! cargo run -p loom-ui --example layout
//! ```
//!
//! Text is measured by a stand-in that gives every character the same
//! width. An app measures with the object that draws its text, so the
//! boxes fit the drawn text.

use loom_ui::{Align, Element, Hit, Layout, Length, Padding};
use loom_ui_core::{Point, Size, TextMeasure, TextStyle, UiId, Weight};

/// Every character is 7 pixels wide, and a line is 16 pixels high.
struct Monospace;

impl TextMeasure for Monospace {
    fn measure(&mut self, text: &str, _style: TextStyle, _wrap_width: Option<f32>) -> Size {
        Size::new(text.chars().count() as f32 * 7.0, 16.0)
    }
}

fn main() {
    let style = TextStyle {
        size: 13.0,
        line_height: 1.25,
        weight: Weight::Regular,
    };
    let button = |key: &str, label: &str| {
        Element::stack([Element::text(label, style)])
            .key(key)
            .padding(Padding::symmetric(4.0, 10.0))
    };
    let toolbar = Element::row([
        button("open", "Open"),
        button("save", "Save"),
        // Pushes what follows to the right end.
        Element::space(Length::FILL, Length::Shrink),
        button("panic", "Panic"),
    ])
    .width(Length::FILL)
    .spacing(6.0)
    .padding(6.0)
    .align_y(Align::Center);

    let mut layout = Layout::new(UiId::panel("toolbar"));
    layout.rebuild(toolbar);
    layout.compute(Size::new(480.0, 40.0), &mut Monospace);

    println!("The toolbar is {:?}.", layout.size());
    for id in layout.ids() {
        let bounds = layout.bounds(id).expect("every listed ID has bounds");
        println!(
            "{id}: {} x {} at ({}, {})",
            bounds.width, bounds.height, bounds.x, bounds.y
        );
    }
    for point in [Point::new(20.0, 18.0), Point::new(240.0, 18.0), Point::new(450.0, 18.0)] {
        let hit = match layout.hit(point) {
            Some(Hit::Element(id)) => id.to_string(),
            Some(Hit::Blocked) => "blocked".to_string(),
            None => "nothing".to_string(),
        };
        println!("A click at ({}, {}) hits {hit}.", point.x, point.y);
    }
}
