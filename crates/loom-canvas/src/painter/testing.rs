//! A test double for painting: a painter that records what it is given and
//! measures with a fixed width for each character.

use super::*;

/// One thing a [`Recorder`] was given, in order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Painted {
    /// A piece of text, with its text copied.
    Text {
        /// The text.
        text: String,
        /// The rest of the run, with an empty text.
        run: TextRun<'static>,
    },
    /// A box.
    Quad(Quad),
    /// A layer began, cut to this rectangle.
    Begin(Rect),
    /// The layer ended.
    End,
}

impl Painted {
    /// The text, when this is a piece of text.
    pub(crate) fn as_text(&self) -> Option<&str> {
        match self {
            Painted::Text { text, .. } => Some(text),
            _ => None,
        }
    }
}

/// A painter that keeps everything it is given, in order, and measures
/// every character as half the font size wide.
#[derive(Debug, Default)]
pub(crate) struct Recorder {
    /// What was painted, first to last.
    pub(crate) painted: Vec<Painted>,
}

impl Recorder {
    /// Every piece of text painted, in order.
    pub(crate) fn texts(&self) -> Vec<&str> {
        self.painted.iter().filter_map(Painted::as_text).collect()
    }
}

impl Painter for Recorder {
    fn text(&mut self, run: &TextRun<'_>) {
        self.painted.push(Painted::Text {
            text: run.text.to_string(),
            run: TextRun { text: "", ..*run },
        });
    }

    fn quad(&mut self, quad: Quad) {
        self.painted.push(Painted::Quad(quad));
    }

    fn begin_layer(&mut self, clip: Rect) {
        self.painted.push(Painted::Begin(clip));
    }

    fn end_layer(&mut self) {
        self.painted.push(Painted::End);
    }
}

/// Every character is half the font size wide, and a line is
/// `line_height` times the size high. Wrapping breaks after as many
/// characters as fit.
impl TextMeasure for Recorder {
    fn measure(&mut self, text: &str, style: TextStyle, wrap_width: Option<f32>) -> Size {
        let advance = style.size * 0.5;
        let characters = text.chars().count() as f32;
        let line = style.size * style.line_height;
        match wrap_width {
            Some(width) if characters * advance > width => {
                let per_line = (width / advance).floor().max(1.0);
                Size::new(per_line * advance, (characters / per_line).ceil() * line)
            }
            _ => Size::new(characters * advance, line),
        }
    }
}
