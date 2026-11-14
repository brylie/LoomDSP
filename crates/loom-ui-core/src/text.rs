//! How a piece of text is set, and the trait that says how large it is.
//!
//! Layout needs the size of a text before it can place it, and drawing
//! needs the same size, or a label would not fit the box laid out for it.
//! So one object both measures and sets text: in LoomDSP, the Parley text
//! of `loom-canvas`. This crate only names the style and the question, so
//! the layout in `loom-ui` and the canvas can share them without either
//! depending on the other.

use crate::Size;

/// How heavy the letters are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    /// The interface font as it is.
    Regular,
    /// Bold, for a label that must stand apart from the line under it.
    Bold,
}

/// How a piece of text is set, for drawing and for measuring. Every run is
/// shaped in full, with fallback fonts for a symbol or a script the
/// interface font lacks, so no style asks for a cheaper path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// The font size in logical pixels.
    pub size: f32,
    /// The height of a line, as a multiple of `size`.
    pub line_height: f32,
    /// How heavy the letters are.
    pub weight: Weight,
}

/// Says how large a piece of text is when set.
pub trait TextMeasure {
    /// The size of `text` in `style`. With `wrap_width`, lines break
    /// between words to fit that width, and the size is that of the lines
    /// together. Without it, the text is one line.
    ///
    /// It takes `&mut self` because measuring sets the text, and a measure
    /// keeps what it set for the painting that follows.
    fn measure(&mut self, text: &str, style: TextStyle, wrap_width: Option<f32>) -> Size;
}
