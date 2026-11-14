//! A renderer that lays out and draws nothing, for the headless harness
//! (plan for leaving Iced, step 2.3, item 2).
//!
//! Event tests need layout, not pixels. Iced lays text out by building its
//! `Paragraph`: `iced_graphics`'s cosmic-text, over Iced's one font system,
//! on the CPU. [`Measure`] uses that same `Paragraph` and `Editor`, so a
//! view laid out on it is laid out exactly as the window's wgpu renderer
//! lays it out, and no GPU is needed. The canvas measures its own text with
//! Parley (`loom_canvas::CanvasText`), which never goes through Iced's
//! renderer, so it is the same on both too.
//!
//! Every draw call does nothing and never panics. The canvas's shader
//! primitive reaches Iced's fallback renderer as its second renderer, which
//! skips it and logs a warning that the primitive "is not supported with
//! this renderer". That warning is expected here and is not an error.

use iced::advanced::image;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::{self, Text};
use iced::{Background, Color, Font, Pixels, Point, Rectangle, Transformation};
use iced_wgpu::graphics::text::{Editor, Paragraph};

/// The wgpu renderer, whose text types and icons [`Measure`] shares.
type Wgpu = iced_wgpu::Renderer;

/// A renderer that measures text and draws nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Measure {
    /// The font text uses when it names none.
    font: Font,
    /// The text size used when a text names none.
    size: Pixels,
}

impl Measure {
    /// A renderer with `font` at `size` as its defaults, as the window's
    /// renderer is made.
    pub(crate) fn new(font: Font, size: Pixels) -> Self {
        Self { font, size }
    }
}

impl renderer::Renderer for Measure {
    fn start_layer(&mut self, _bounds: Rectangle) {}

    fn end_layer(&mut self) {}

    fn start_transformation(&mut self, _transformation: Transformation) {}

    fn end_transformation(&mut self) {}

    fn fill_quad(&mut self, _quad: Quad, _background: impl Into<Background>) {}

    fn reset(&mut self, _new_bounds: Rectangle) {}

    /// Allocates nothing: the app shows no images, and the callback is
    /// never called.
    fn allocate_image(
        &mut self,
        _handle: &image::Handle,
        _callback: impl FnOnce(Result<image::Allocation, image::Error>) + Send + 'static,
    ) {
    }
}

impl text::Renderer for Measure {
    type Font = Font;
    type Paragraph = Paragraph;
    type Editor = Editor;

    const ICON_FONT: Font = <Wgpu as text::Renderer>::ICON_FONT;
    const CHECKMARK_ICON: char = <Wgpu as text::Renderer>::CHECKMARK_ICON;
    const ARROW_DOWN_ICON: char = <Wgpu as text::Renderer>::ARROW_DOWN_ICON;
    const SCROLL_UP_ICON: char = <Wgpu as text::Renderer>::SCROLL_UP_ICON;
    const SCROLL_DOWN_ICON: char = <Wgpu as text::Renderer>::SCROLL_DOWN_ICON;
    const SCROLL_LEFT_ICON: char = <Wgpu as text::Renderer>::SCROLL_LEFT_ICON;
    const SCROLL_RIGHT_ICON: char = <Wgpu as text::Renderer>::SCROLL_RIGHT_ICON;
    const ICED_LOGO: char = <Wgpu as text::Renderer>::ICED_LOGO;

    fn default_font(&self) -> Font {
        self.font
    }

    fn default_size(&self) -> Pixels {
        self.size
    }

    fn fill_paragraph(&mut self, _text: &Paragraph, _position: Point, _color: Color, _clip_bounds: Rectangle) {}

    fn fill_editor(&mut self, _editor: &Editor, _position: Point, _color: Color, _clip_bounds: Rectangle) {}

    fn fill_text(&mut self, _text: Text<String, Font>, _position: Point, _color: Color, _clip_bounds: Rectangle) {}
}

#[cfg(test)]
mod tests;
