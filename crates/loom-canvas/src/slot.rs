//! One frame of the canvas and where it goes: what the canvas element's
//! prepare draws, and what a capture draws offscreen
//! (`CanvasInput::drawing`, ui-layer.md, section 9, rule 4).

use crate::gpu::CanvasFrame;
use loom_ui_core::Rect;

/// One frame of the canvas, and where it goes.
#[derive(Debug)]
pub struct CanvasDrawing {
    /// The canvas's place in the host's logical pixels.
    pub bounds: Rect,
    /// What to draw there.
    pub frame: CanvasFrame,
}
