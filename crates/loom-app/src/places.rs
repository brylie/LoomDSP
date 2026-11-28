//! Where the canvas is, for the accessibility tree and the control
//! channel.
//!
//! Every other part of the window is placed by the runtime's layout, by
//! its UI ID (`Context::bounds`); the canvas's place and camera come from
//! its own element (`canvas.rs`).

use loom_canvas::{CanvasTyped, CanvasView};

/// What the window holds of the canvas: its place and camera, and its
/// open typed field.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Places {
    /// The patch canvas and its camera.
    pub canvas: Option<CanvasView>,
    /// The value editor drawn over a tile, if open.
    pub typed: Option<CanvasTyped>,
    /// The part a search looked for by its UI ID is in the layout but
    /// scrolled out of view: a pointer sent there would land on something
    /// else, so `locate` refuses it.
    pub(crate) scrolled_out: bool,
}

impl Places {
    /// Places with only the canvas known, as tests of the canvas tree use.
    #[cfg(test)]
    pub(crate) fn of_canvas(canvas: Option<CanvasView>) -> Self {
        Self {
            canvas,
            ..Self::default()
        }
    }

    /// The canvas and its open typed field, read from its element.
    pub(crate) fn live(canvas: Option<CanvasView>, typed: Option<CanvasTyped>) -> Self {
        Self {
            canvas,
            typed,
            scrolled_out: false,
        }
    }
}
