//! A view's panels laid out once, for a still picture of the window with no
//! runtime around it: `--screenshot-window`, and a test harness's capture.
//! Nothing is hovered or focused, so no hover text or focus ring shows.

use crate::paint::{PanelStyle, TextPainter};
use crate::view::{Layer, View};
use crate::window::Window;
use loom_ui_core::{Size, TextMeasure};

/// The panels of one view, laid out in a window of one size.
pub struct Still {
    window: Window,
}

impl Still {
    /// Lays out `view`'s panels in a window of `size`, with `text` for text.
    pub fn new(view: &View, size: Size, text: &mut dyn TextMeasure) -> Self {
        let mut window = Window::default();
        window.sync(view.built(None));
        window.lay_out(size, text);
        Self { window }
    }

    /// Paints the panels of `layer` into `painter`, as
    /// [`Runtime::paint`](crate::Runtime::paint) does, with nothing
    /// focused, so no field shows a caret.
    pub fn paint(&self, layer: Layer, painter: &mut dyn TextPainter, style: &PanelStyle) {
        let mut none = crate::custom::NoCustoms;
        let _ = self
            .window
            .paint(layer, painter, style, crate::window::Shown::STILL, &mut none);
    }

    /// Paints as [`Still::paint`] does, with each custom element painted
    /// from a state `app` makes for this picture
    /// ([`App::make_custom`](crate::App::make_custom)), so a capture shows
    /// a meter. With no runtime, no element has a state it kept: each
    /// paints as it is when new, from the app it reads.
    pub fn paint_with<A: crate::App>(&self, app: &A, layer: Layer, painter: &mut dyn TextPainter, style: &PanelStyle) {
        let mut fresh = crate::custom::Fresh { app };
        let _ = self
            .window
            .paint(layer, painter, style, crate::window::Shown::STILL, &mut fresh);
    }
}

#[cfg(test)]
mod tests;
