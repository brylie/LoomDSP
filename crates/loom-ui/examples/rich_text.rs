//! Sets a paragraph with links, code, and bold words, with no window and
//! no GPU, and prints where each link's words went and what a click at a
//! few points would hit.
//!
//! ```sh
//! cargo run -p loom-ui --example rich_text
//! ```
//!
//! The text is set with the system's fonts, as a window sets it, so a
//! link that the width breaks has a place on each of its lines.

use loom_ui::access::{Access, Action, Role};
use loom_ui::paint::Typesetter;
use loom_ui::{Element, Hit, Inline, Layout, Length};
use loom_ui_core::{Color, Point, Size, TextStyle, UiId, Weight};

fn main() {
    let style = TextStyle {
        size: 13.0,
        line_height: 1.4,
        weight: Weight::Regular,
    };
    let blue = Color::rgb(0.45, 0.7, 1.0);
    let link = |words: &str, key: &str| {
        Inline::new(words)
            .colour(blue)
            .underline()
            .link(key, Access::new(Role::Link, words).action(Action::Click))
    };
    let paragraph = Element::rich(
        [
            Inline::new("A loop needs a "),
            Inline::new("delay1").mono(),
            Inline::new(" somewhere in it. "),
            Inline::new("Read more").bold(),
            Inline::new(" in "),
            link("feedback loops and their unit delays", "loops"),
            Inline::new(", or open the "),
            link("Saw oscillator", "saw"),
            Inline::new("."),
        ],
        style,
    )
    .key("notes")
    .width(Length::Fixed(240.0));

    let mut layout = Layout::new(UiId::panel("help"));
    layout.rebuild(Element::column([paragraph]));
    layout.compute(Size::new(400.0, 300.0), &mut Typesetter::new());

    for key in ["loops", "saw"] {
        let id = UiId::panel("help").with("notes").with(key);
        println!("{id}: {:?}", layout.bounds(&id));
    }
    for point in [Point::new(5.0, 5.0), Point::new(60.0, 24.0), Point::new(10.0, 42.0)] {
        let hit = match layout.hit(point) {
            Some(Hit::Element(id)) => id.to_string(),
            Some(Hit::Blocked) => "blocked".to_string(),
            None => "nothing".to_string(),
        };
        println!("({}, {}) hits {hit}", point.x, point.y);
    }
}
