//! Tests for how the canvas's painting becomes Iced values.

use super::*;
use loom_ui_core::{Color, Point};

/// A run for the tests.
fn run(text: &str) -> TextRun<'_> {
    TextRun {
        text,
        anchor: Point::new(10.0, 20.0),
        bounds: Size::new(80.0, 14.0),
        align: Align::Center,
        vertical: Vertical::Center,
        style: TextStyle {
            size: 10.0,
            line_height: 1.2,
            weight: Weight::Regular,
            shaping: Shaping::Simple,
        },
        wrap: false,
        colour: Color::WHITE,
        clip: Rect::new(Point::ORIGIN, Size::new(100.0, 100.0)),
    }
}

/// A run becomes Iced text with the same text, room, size, line height,
/// and alignment, on one line with basic shaping.
#[test]
fn a_run_becomes_iced_text() {
    let text = iced_text(&run("gain"), Font::DEFAULT);
    assert_eq!(text.content, "gain");
    assert_eq!(text.bounds, iced::Size::new(80.0, 14.0));
    assert_eq!(text.size, Pixels(10.0));
    assert_eq!(text.line_height, text::LineHeight::Relative(1.2));
    assert_eq!(text.font, Font::DEFAULT);
    assert_eq!(text.align_x, text::Alignment::Center);
    assert_eq!(text.align_y, alignment::Vertical::Center);
    assert_eq!(text.shaping, text::Shaping::Basic);
    assert_eq!(text.wrapping, text::Wrapping::None);
}

/// Each alignment, weight, shaping, and wrap choice has its Iced value.
#[test]
fn each_choice_on_a_run_has_its_iced_value() {
    for (align, expected) in [
        (Align::Left, text::Alignment::Left),
        (Align::Center, text::Alignment::Center),
        (Align::Right, text::Alignment::Right),
    ] {
        let text = iced_text(&TextRun { align, ..run("a") }, Font::DEFAULT);
        assert_eq!(text.align_x, expected);
    }
    let top = iced_text(
        &TextRun {
            vertical: Vertical::Top,
            wrap: true,
            ..run("a")
        },
        Font::DEFAULT,
    );
    assert_eq!(top.align_y, alignment::Vertical::Top);
    assert_eq!(top.wrapping, text::Wrapping::Word);

    assert_eq!(font_for(Font::DEFAULT, Weight::Regular), Font::DEFAULT);
    assert_eq!(font_for(Font::DEFAULT, Weight::Bold).weight, iced::font::Weight::Bold);
    assert_eq!(shaping_for(Shaping::Simple), text::Shaping::Basic);
    assert_eq!(shaping_for(Shaping::Full), text::Shaping::Advanced);
}

/// A box becomes an Iced quad with the same place, outline, and corners.
#[test]
fn a_quad_becomes_an_iced_quad() {
    let rect = Rect::new(Point::new(1.0, 2.0), Size::new(30.0, 40.0));
    let quad = iced_quad(Quad {
        rect,
        fill: Color::BLACK,
        border: Color::WHITE,
        border_width: 2.0,
        radius: 4.0,
    });
    assert_eq!(
        quad.bounds,
        iced::Rectangle::new(iced::Point::new(1.0, 2.0), iced::Size::new(30.0, 40.0))
    );
    assert_eq!(quad.border.color, iced::Color::WHITE);
    assert_eq!(quad.border.width, 2.0);
    assert_eq!(quad.border.radius, 4.0.into());
}

/// The painter and the measure run against Iced's null renderer, which
/// draws nothing and measures everything as empty.
#[test]
fn the_painter_and_measure_drive_an_iced_renderer() {
    let mut renderer = ();
    let measure = IcedMeasure::new(&renderer);
    let style = run("a").style;
    assert_eq!(measure.measure("abc", style, None), Size::new(0.0, 0.0));
    assert_eq!(measure.measure("abc", style, Some(50.0)), Size::new(0.0, 0.0));

    let mut painter = IcedPainter::new(&mut renderer);
    let clip = Rect::new(Point::ORIGIN, Size::new(10.0, 10.0));
    painter.begin_layer(clip);
    painter.quad(Quad::filled(clip, Color::BLACK, 0.0));
    painter.text(&run("gain"));
    painter.end_layer();
}
