//! Where the canvas widget leaves the frame it built, for the host to draw
//! (plan for leaving Iced, step 2.2c).
//!
//! The canvas no longer draws inside Iced. Its widget still lives in Iced's
//! tree, for input and for overlays above it, and its draw call builds the
//! frame from the state it keeps there. It writes that frame into a slot
//! the host lends it, and the host draws it with [`CanvasRenderer`] in a
//! pass of its own, before Iced draws the rest of the window.
//!
//! [`CanvasRenderer`]: crate::CanvasRenderer

use crate::gpu::CanvasFrame;
use loom_ui_core::Rect;
use std::cell::RefCell;

/// One frame of the canvas, and where it goes.
#[derive(Debug)]
pub struct CanvasDrawing {
    /// The canvas's place in the host's logical pixels.
    pub bounds: Rect,
    /// What to draw there.
    pub frame: CanvasFrame,
}

/// The slot the canvas widget writes its frame into. The host takes the
/// frame after Iced's draw call and before it draws the canvas.
#[derive(Debug, Default)]
pub struct CanvasSlot(RefCell<Option<CanvasDrawing>>);

impl CanvasSlot {
    /// Leaves `drawing` in the slot. The host empties the slot before each
    /// draw, so a frame still there means two canvases drew into one slot,
    /// or one drew twice in a frame: a bug, since one of them would never
    /// be shown. Debug builds panic on it; release builds keep the newer
    /// frame.
    pub fn put(&self, drawing: CanvasDrawing) {
        debug_assert!(self.0.borrow().is_none(), "two canvases drew into one slot in a frame");
        *self.0.borrow_mut() = Some(drawing);
    }

    /// Takes the frame last left, leaving the slot empty.
    pub fn take(&self) -> Option<CanvasDrawing> {
        self.0.borrow_mut().take()
    }

    /// Empties the slot, so a frame left for a patch that is gone is never
    /// drawn.
    pub fn clear(&self) {
        self.0.borrow_mut().take();
    }
}

#[cfg(test)]
mod tests;
