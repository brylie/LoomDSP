//! Tests for `measure.rs`: it keeps its defaults, and drawing on it does
//! nothing and never panics.

use super::Measure;
use iced::advanced::renderer::{Quad, Renderer as _};
use iced::advanced::text::{Paragraph as _, Renderer as _, Text};
use iced::{Color, Font, Pixels, Point, Rectangle, Size, Transformation};

/// It reports the default font and size it was made with.
#[test]
fn it_keeps_its_defaults() {
    let measure = Measure::new(Font::MONOSPACE, Pixels(13.0));
    assert_eq!(measure.default_font(), Font::MONOSPACE);
    assert_eq!(measure.default_size(), Pixels(13.0));
}

/// Every draw call does nothing and returns; text laid out for it has a
/// size, since measuring is what it is for.
#[test]
fn drawing_does_nothing() {
    let mut measure = Measure::new(Font::DEFAULT, Pixels(16.0));
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(100.0, 20.0));
    measure.with_layer(bounds, |measure| {
        measure.with_transformation(Transformation::scale(2.0), |measure| {
            measure.fill_quad(
                Quad {
                    bounds,
                    ..Quad::default()
                },
                Color::BLACK,
            );
        });
    });
    let text = Text {
        content: "Cutoff",
        bounds: Size::new(f32::INFINITY, f32::INFINITY),
        size: Pixels(16.0),
        line_height: iced::advanced::text::LineHeight::default(),
        font: Font::DEFAULT,
        align_x: iced::advanced::text::Alignment::Default,
        align_y: iced::alignment::Vertical::Top,
        shaping: iced::advanced::text::Shaping::Advanced,
        wrapping: iced::advanced::text::Wrapping::None,
    };
    let paragraph = <Measure as iced::advanced::text::Renderer>::Paragraph::with_text(text);
    assert!(paragraph.min_width() > 0.0, "Iced's paragraph measures the text");
    measure.fill_paragraph(&paragraph, Point::ORIGIN, Color::WHITE, bounds);
    measure.fill_text(
        Text {
            content: "Cutoff".to_string(),
            bounds: text.bounds,
            size: text.size,
            line_height: text.line_height,
            font: text.font,
            align_x: text.align_x,
            align_y: text.align_y,
            shaping: text.shaping,
            wrapping: text.wrapping,
        },
        Point::ORIGIN,
        Color::WHITE,
        bounds,
    );
    measure.reset(bounds);
    assert_eq!(measure, Measure::new(Font::DEFAULT, Pixels(16.0)), "nothing changed");
}
