//! The text an input method is composing, drawn over the window until
//! phase 3 draws it in the field (ui-layer.md, section 8, rule 4).
//!
//! Iced's text field reports what is being composed in the input method's
//! state: the caret's area, the text, its selected range, and its size.
//! Iced's own loop drew it; on the shell nothing would, so someone typing
//! Chinese, Japanese, or Korean would not see what they compose. The
//! window's layers draw it here, after the views, in a layer above them: a
//! box below the caret, kept inside the window, with the text underlined
//! and the selected range marked.

use iced::advanced::input_method::{InputMethod, Preedit};
use iced::advanced::text::{self, Paragraph as _, Renderer as _};
use iced::advanced::{Renderer as _, renderer};
use iced::{Color, Pixels, Point, Rectangle, Size, Theme};

/// Room between the text and the box's edge, in logical pixels.
const PADDING: f32 = 4.0;

/// The text being composed, if any.
pub(super) fn of(method: &InputMethod) -> Option<&Preedit> {
    match method {
        InputMethod::Enabled {
            preedit: Some(preedit), ..
        } if !preedit.content.is_empty() => Some(preedit),
        _ => None,
    }
}

/// Where a box of `size` goes for a caret at `caret`: just below it, at its
/// left edge, or above it when there is no room below, then moved to stay
/// inside a window of `window`.
pub(super) fn place(caret: Rectangle, size: Size, window: Size) -> Rectangle {
    let below = caret.y + caret.height;
    let y = if below + size.height <= window.height {
        below
    } else {
        caret.y - size.height
    };
    let x = caret.x.min(window.width - size.width).max(0.0);
    Rectangle::new(Point::new(x, y.min(window.height - size.height).max(0.0)), size)
}

/// The text before the selection and the text through its end, when the
/// selection is a non-empty range on character boundaries of `content`.
pub(super) fn selected<'a>(content: &'a str, selection: Option<&std::ops::Range<usize>>) -> Option<(&'a str, &'a str)> {
    let range = selection.filter(|range| range.start < range.end)?;
    Some((content.get(..range.start)?, content.get(..range.end)?))
}

/// Where the selected range of `content` is drawn at `size`, from the
/// text's top left: one rectangle a line, from one shaped layout of the
/// whole text in three spans (before, selected, after). Measuring the two
/// sides apart would mark the wrong place in right-to-left text, where the
/// first characters are drawn at the right.
pub(super) fn marked(content: &str, selection: Option<&std::ops::Range<usize>>, size: f32) -> Vec<Rectangle> {
    let Some((before, through)) = selected(content, selection) else {
        return Vec::new();
    };
    let spans: [text::Span<'_>; 3] = [
        text::Span::new(before),
        text::Span::new(&content[before.len()..through.len()]),
        text::Span::new(&content[through.len()..]),
    ];
    let paragraph = <crate::Renderer as text::Renderer>::Paragraph::with_spans(text(&spans[..], size, Size::INFINITE));
    paragraph.span_bounds(1)
}

/// How wide `content` is at `size`.
fn width(content: &str, size: f32) -> f32 {
    let paragraph = <crate::Renderer as text::Renderer>::Paragraph::with_text(text(content, size, Size::INFINITE));
    paragraph.min_bounds().width
}

/// One line of `content` at `size`, within `bounds`.
fn text<T>(content: T, size: f32, bounds: Size) -> text::Text<T> {
    text::Text {
        content,
        bounds,
        size: Pixels(size),
        line_height: text::LineHeight::default(),
        font: iced::Font::DEFAULT,
        align_x: text::Alignment::Left,
        align_y: iced::alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::None,
    }
}

/// Draws `preedit` for a caret at `caret`, in a window of `window`, in a
/// layer above what is drawn. `fallback` is the text size when the field
/// reports none.
pub(super) fn draw(
    renderer: &mut crate::Renderer,
    preedit: &Preedit,
    caret: Rectangle,
    window: Size,
    theme: &Theme,
    fallback: f32,
) {
    let size = preedit.text_size.map_or(fallback, |pixels| pixels.0);
    let line = text::LineHeight::default().to_absolute(Pixels(size)).0;
    let content = Size::new(width(&preedit.content, size), line);
    let area = place(
        caret,
        Size::new(content.width + 2.0 * PADDING, content.height + 2.0 * PADDING),
        window,
    );
    let palette = theme.extended_palette();
    let origin = Point::new(area.x + PADDING, area.y + PADDING);
    let window_bounds = Rectangle::new(Point::ORIGIN, window);
    renderer.with_layer(window_bounds, |renderer| {
        let quad = |bounds| renderer::Quad {
            bounds,
            ..renderer::Quad::default()
        };
        renderer.fill_quad(
            renderer::Quad {
                border: iced::Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..quad(area)
            },
            palette.background.weak.color,
        );
        for marked in marked(&preedit.content, preedit.selection.as_ref(), size) {
            renderer.fill_quad(
                quad(Rectangle::new(
                    Point::new(origin.x + marked.x, origin.y + marked.y),
                    marked.size(),
                )),
                Color {
                    a: 0.4,
                    ..palette.primary.base.color
                },
            );
        }
        let text_color = palette.background.weak.text;
        renderer.fill_text(text(preedit.content.clone(), size, content), origin, text_color, area);
        let underline = Rectangle::new(
            Point::new(origin.x, origin.y + line - 1.0),
            Size::new(content.width, 1.0),
        );
        renderer.fill_quad(quad(underline), text_color);
    });
}

#[cfg(test)]
mod tests;
