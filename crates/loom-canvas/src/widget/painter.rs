//! The canvas's painting done by Iced: text through Iced's text renderer,
//! boxes as Iced quads, and layers as Iced layers. This is the only place
//! that turns a [`TextRun`] or a [`Quad`] into an Iced value.

use crate::iced_convert::{ToCore as _, ToIced as _};
use crate::painter::{Painter, Quad, TextMeasure, TextRun, TextStyle, Vertical, Weight};
use crate::tile_text::Align;
use iced::advanced::renderer;
use iced::advanced::text::{self, Paragraph as _, Text};
use iced::{Font, Pixels, alignment};
use loom_ui_core::{Rect, Size};

/// `font` in the weight a style asks for.
fn font_for(font: Font, weight: Weight) -> Font {
    match weight {
        Weight::Regular => font,
        Weight::Bold => Font {
            weight: iced::font::Weight::Bold,
            ..font
        },
    }
}

/// Every run is shaped in full with fallback fonts (see [`TextStyle`]),
/// which is Iced's advanced mode.
const SHAPING: text::Shaping = text::Shaping::Advanced;

/// Iced's wrapping for a run or a measurement.
fn wrapping_for(wrap: bool) -> text::Wrapping {
    if wrap {
        text::Wrapping::Word
    } else {
        text::Wrapping::None
    }
}

/// The Iced text for a run. Iced's text owns its string, so the run's
/// borrowed text is copied here.
fn iced_text(run: &TextRun<'_>, font: Font) -> Text<String, Font> {
    Text {
        content: run.text.to_string(),
        bounds: run.bounds.to_iced(),
        size: Pixels(run.style.size),
        line_height: text::LineHeight::Relative(run.style.line_height),
        font: font_for(font, run.style.weight),
        align_x: match run.align {
            Align::Left => text::Alignment::Left,
            Align::Center => text::Alignment::Center,
            Align::Right => text::Alignment::Right,
        },
        align_y: match run.vertical {
            Vertical::Center => alignment::Vertical::Center,
            Vertical::Top => alignment::Vertical::Top,
        },
        shaping: SHAPING,
        wrapping: wrapping_for(run.wrap),
    }
}

/// The Iced quad for a box, without its fill.
fn iced_quad(quad: Quad) -> renderer::Quad {
    renderer::Quad {
        bounds: quad.rect.to_iced(),
        border: iced::Border {
            color: quad.border.to_iced(),
            width: quad.border_width,
            radius: quad.radius.into(),
        },
        ..renderer::Quad::default()
    }
}

/// Paints into an Iced renderer, and measures as it sets text.
pub(super) struct IcedPainter<'r, Renderer> {
    renderer: &'r mut Renderer,
    font: Font,
}

impl<'r, Renderer> IcedPainter<'r, Renderer>
where
    Renderer: text::Renderer<Font = Font>,
{
    /// A painter that draws with the renderer's default font.
    pub(super) fn new(renderer: &'r mut Renderer) -> Self {
        let font = renderer.default_font();
        Self { renderer, font }
    }
}

impl<Renderer> Painter for IcedPainter<'_, Renderer>
where
    Renderer: text::Renderer<Font = Font>,
{
    fn text(&mut self, run: &TextRun<'_>) {
        self.renderer.fill_text(
            iced_text(run, self.font),
            run.anchor.to_iced(),
            run.colour.to_iced(),
            run.clip.to_iced(),
        );
    }

    fn quad(&mut self, quad: Quad) {
        self.renderer.fill_quad(iced_quad(quad), quad.fill.to_iced());
    }

    fn begin_layer(&mut self, clip: Rect) {
        self.renderer.start_layer(clip.to_iced());
    }

    fn end_layer(&mut self) {
        self.renderer.end_layer();
    }
}

/// Measures text as the Iced renderer sets it.
impl<Renderer> TextMeasure for IcedPainter<'_, Renderer>
where
    Renderer: text::Renderer<Font = Font>,
{
    fn measure(&mut self, text: &str, style: TextStyle, wrap_width: Option<f32>) -> Size {
        Renderer::Paragraph::with_text(Text {
            content: text,
            bounds: iced::Size::new(wrap_width.unwrap_or(f32::INFINITY), f32::INFINITY),
            size: Pixels(style.size),
            line_height: text::LineHeight::Relative(style.line_height),
            font: font_for(self.font, style.weight),
            align_x: text::Alignment::Left,
            align_y: alignment::Vertical::Top,
            shaping: SHAPING,
            wrapping: wrapping_for(wrap_width.is_some()),
        })
        .min_bounds()
        .to_core()
    }
}

#[cfg(test)]
mod tests;
