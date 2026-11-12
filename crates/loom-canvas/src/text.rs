//! Canvas text set with Parley (docs/design/ui-layer.md, section 8).
//!
//! [`CanvasText`] is the one place the canvas sets text. Measuring reads a
//! layout's size, and painting places the same layout's glyphs, so the box
//! the canvas lays out is the box the text is drawn in.
//!
//! Layouts are cached by their text and style. A layout no frame used is
//! dropped one frame later (see [`CanvasText::begin_frame`]), so a still
//! canvas sets no text again, and a zoom gesture leaves no old sizes behind.
//!
//! The system's fonts are found once for the whole process: each
//! `CanvasText` clones one font context, whose collection shares its scan,
//! so a canvas made again, as on New or Open, scans nothing. Its source
//! cache shares loaded font files between clones while any of them holds
//! the file, and each `CanvasText` holds every file it has used. So a font
//! keeps one identity while its canvas lives, and the glyph atlas reuses
//! its images (`gpu/atlas.rs`) even after a zoom too far out for text. A
//! file no canvas holds any more is loaded again when next used, as a font
//! of a new identity, and the atlas's old images age out.

use crate::painter::{TextStyle, Weight};
use loom_ui_core::Size;
use parley::fontique::{Collection, CollectionOptions, SourceCache};
use parley::{
    FontContext, FontData, FontFamily, GenericFamily, Layout, LayoutContext, LineHeight, PositionedLayoutItem,
    StyleProperty,
};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub(crate) mod list;
pub use list::{ListPainter, PaintList};

/// A Parley layout of canvas text. Its brush is empty: the colour of a run
/// is the painter's, not the layout's.
pub(crate) type TextLayout = Layout<()>;

/// The system's fonts, scanned and loaded once for the process. A clone
/// shares the scan and the loaded files.
fn shared_fonts() -> FontContext {
    static BASE: OnceLock<Mutex<FontContext>> = OnceLock::new();
    BASE.get_or_init(|| {
        Mutex::new(FontContext {
            collection: Collection::new(CollectionOptions {
                shared: true,
                ..CollectionOptions::default()
            }),
            source_cache: SourceCache::new_shared(),
        })
    })
    .lock()
    // A panic while cloning leaves the context as it was.
    .unwrap_or_else(std::sync::PoisonError::into_inner)
    .clone()
}

/// What decides a layout besides its text. Floats are kept as bits, so the
/// key can be hashed; text sizes are already quantised by the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct StyleKey {
    size: u32,
    line_height: u32,
    bold: bool,
    wrap: Option<u32>,
}

impl StyleKey {
    /// The key for `style`, wrapped at `wrap` or on one line.
    fn new(style: TextStyle, wrap: Option<f32>) -> Self {
        Self {
            size: style.size.to_bits(),
            line_height: style.line_height.to_bits(),
            bold: style.weight == Weight::Bold,
            wrap: wrap.map(f32::to_bits),
        }
    }
}

/// Layouts by style, then by text, so a lookup needs no new string.
type Layouts = HashMap<StyleKey, HashMap<String, TextLayout>>;

/// Sets canvas text with Parley and keeps what it set.
pub struct CanvasText {
    fonts: FontContext,
    layouts: LayoutContext<()>,
    /// Layouts used this frame.
    current: Layouts,
    /// Layouts used last frame and not yet this one.
    previous: Layouts,
    /// Layouts built since the counter was last read, for tests and stats.
    built: u64,
    /// Every font file a layout has used, held so it stays loaded.
    held: Vec<FontData>,
}

impl Default for CanvasText {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CanvasText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CanvasText")
            .field("current", &self.current.values().map(HashMap::len).sum::<usize>())
            .field("previous", &self.previous.values().map(HashMap::len).sum::<usize>())
            .finish_non_exhaustive()
    }
}

impl CanvasText {
    /// Text set in the system's interface font (`system-ui`), with the
    /// optical size at its default (spec section 8, rule 6).
    pub fn new() -> Self {
        Self {
            fonts: shared_fonts(),
            layouts: LayoutContext::new(),
            current: Layouts::new(),
            previous: Layouts::new(),
            built: 0,
            held: Vec::new(),
        }
    }

    /// Starts a frame. A layout used last frame and not since is dropped;
    /// a layout used this frame is kept for the next.
    pub fn begin_frame(&mut self) {
        self.previous = std::mem::take(&mut self.current);
    }

    /// The layout of `text` in `style`: wrapped between words to fit
    /// `wrap`, or on one line. Built once and reused while frames use it.
    pub(crate) fn layout(&mut self, text: &str, style: TextStyle, wrap: Option<f32>) -> &TextLayout {
        let key = StyleKey::new(style, wrap);
        let found = self.current.get(&key).is_some_and(|by_text| by_text.contains_key(text));
        if !found {
            let layout = match self.previous.get_mut(&key).and_then(|by_text| by_text.remove(text)) {
                Some(layout) => layout,
                None => {
                    self.built += 1;
                    self.build(text, style, wrap)
                }
            };
            self.current.entry(key).or_default().insert(text.to_owned(), layout);
        }
        &self.current[&key][text]
    }

    /// The size of `text` in `style`, as [`layout`](Self::layout) sets it.
    pub fn measure(&mut self, text: &str, style: TextStyle, wrap: Option<f32>) -> Size {
        let layout = self.layout(text, style, wrap);
        Size::new(layout.width(), layout.height())
    }

    /// How many layouts were built since the last call.
    pub fn take_built(&mut self) -> u64 {
        std::mem::take(&mut self.built)
    }

    /// Sets `text` from nothing.
    fn build(&mut self, text: &str, style: TextStyle, wrap: Option<f32>) -> TextLayout {
        let mut builder = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(FontFamily::from(GenericFamily::SystemUi)));
        builder.push_default(StyleProperty::FontSize(style.size));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            style.line_height,
        )));
        if style.weight == Weight::Bold {
            builder.push_default(StyleProperty::FontWeight(parley::FontWeight::BOLD));
        }
        let mut layout = builder.build(text);
        layout.break_all_lines(wrap);
        for line in layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(run) = item {
                    let font = run.run().font();
                    if !self.held.iter().any(|held| held.data.id() == font.data.id()) {
                        self.held.push(font.clone());
                    }
                }
            }
        }
        layout
    }
}

#[cfg(test)]
mod tests;
