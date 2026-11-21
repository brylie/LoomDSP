//! Text set with Parley (docs/design/ui-layer.md, section 8).
//!
//! A [`Typesetter`] is the one place its owner sets text. Measuring reads
//! a layout's size, and painting places the same layout's glyphs, so the
//! box an element lays out is the box the text is drawn in.
//!
//! Layouts are cached by their text and style. A layout no frame used is
//! dropped one frame later (see [`Typesetter::begin_frame`]), so a still
//! view sets no text again, and a zoom gesture leaves no old sizes behind.
//!
//! The system's fonts are found once for the whole process: each
//! `Typesetter` clones one font context, whose collection shares its scan,
//! so one made again, as LoomDSP's canvas is on New or Open, scans
//! nothing. Its source cache shares loaded font files between clones while
//! any of them holds the file, and each `Typesetter` holds every file it
//! has used. So a font keeps one identity while its typesetter lives, and
//! the glyph atlas reuses its images (`atlas.rs`) even after a zoom too far
//! out for text. A file no typesetter holds any more is loaded again when
//! next used, as a font of a new identity, and the atlas's old images age
//! out.
//!
//! A text with [`Span`]s is set in one layout, so its lines break as one
//! paragraph. Each span's index is its glyphs' brush, so a painter finds
//! each glyph run's span, and with it the run's colour and underline.

use super::{Face, Span, TextMeasure, TextStyle, Weight};
use fontique::{Collection, CollectionOptions, SourceCache};
use loom_ui_core::{Point, Rect, Size};
use parley::{
    Affinity, Brush, Cursor, FontContext, FontData, FontFamily, GenericFamily, Layout, LayoutContext, LineHeight,
    PositionedLayoutItem, Selection, StyleProperty,
};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Mutex, OnceLock};

/// A Parley layout of a piece of text. Its brush is the index of the span a
/// glyph is in (0 for plain text), not a colour: the colour of a run is the
/// painter's, looked up from the span, so a colour of its own keys no
/// layout.
pub(crate) type TextLayout = Layout<u16>;

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

/// How text in `style` is set: the family, size, line height, and weight.
/// The one place that says so, for the typesetter's layouts and a text
/// field's editor alike, so a field's caret sits where its drawn text puts
/// the letters (ui-layer.md, section 8, rule 9).
pub(crate) fn properties<B: Brush>(style: TextStyle) -> impl Iterator<Item = StyleProperty<'static, B>> {
    [
        Some(StyleProperty::FontFamily(FontFamily::from(GenericFamily::SystemUi))),
        Some(StyleProperty::FontSize(style.size)),
        Some(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            style.line_height,
        ))),
        (style.weight == Weight::Bold).then_some(StyleProperty::FontWeight(parley::FontWeight::BOLD)),
    ]
    .into_iter()
    .flatten()
}

/// The brush of the span at `index`: its index, which a `u16` holds for
/// any paragraph a view builds.
///
/// # Panics
///
/// In a debug build, past `u16::MAX` spans, so the mistake shows in a
/// test. A release build gives the spans past it the last brush, and they
/// paint as that span does, rather than stop a performance.
fn brush(index: usize) -> u16 {
    debug_assert!(
        index <= usize::from(u16::MAX),
        "a paragraph has more than {} spans",
        u16::MAX
    );
    u16::try_from(index).unwrap_or(u16::MAX)
}

/// The system's fonts and a layout context of their own, for a text field's
/// editor ([`crate::field`]). A key reaches a field in a turn, where no
/// typesetter is at hand, and moving by word walks the shaped layout, so
/// the runtime keeps these. The fonts are the process's one scan.
pub(crate) fn editing_contexts() -> (FontContext, LayoutContext<()>) {
    (shared_fonts(), LayoutContext::new())
}

/// What decides a layout besides its text. Floats are kept as bits, so the
/// key can be hashed; a host quantises its text sizes, as the canvas does by
/// zoom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct StyleKey {
    size: u32,
    line_height: u32,
    bold: bool,
    wrap: Option<u32>,
}

/// What a span adds to a layout's key: where it ends and what changes how
/// its letters are shaped. Its colour and underline do not, since they are
/// painted, not set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SpanKey {
    len: usize,
    bold: bool,
    mono: bool,
}

/// A layout's key: its style, and its spans, empty for plain text, so a
/// plain text's key holds nothing on the heap.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LayoutKey {
    style: StyleKey,
    spans: Box<[SpanKey]>,
}

impl LayoutKey {
    /// The key for `spans` over a text in `style`, wrapped at `wrap`.
    fn new(style: TextStyle, spans: &[Span], wrap: Option<f32>) -> Self {
        Self {
            style: StyleKey::new(style, wrap),
            spans: spans
                .iter()
                .map(|span| SpanKey {
                    len: span.len,
                    bold: span.weight == Weight::Bold,
                    mono: span.face == Face::Mono,
                })
                .collect(),
        }
    }
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

/// Layouts by style and spans, then by text, so a lookup needs no new
/// string.
type Layouts = HashMap<LayoutKey, HashMap<String, TextLayout>>;

/// Sets text with Parley and keeps what it set.
pub struct Typesetter {
    fonts: FontContext,
    layouts: LayoutContext<u16>,
    /// Layouts used this frame.
    current: Layouts,
    /// Layouts used last frame and not yet this one.
    previous: Layouts,
    /// Layouts built since the counter was last read, for tests and stats.
    built: u64,
    /// Every font file a layout has used, held so it stays loaded.
    held: Vec<FontData>,
}

impl Default for Typesetter {
    fn default() -> Self {
        Self::new()
    }
}

/// The same measure as [`Typesetter::measure`], for a layout that asks
/// through the trait, such as the panels' layout: what a panel lays out is
/// what the typesetter draws.
impl TextMeasure for Typesetter {
    fn measure(&mut self, text: &str, style: TextStyle, wrap_width: Option<f32>) -> Size {
        Typesetter::measure(self, text, style, wrap_width)
    }

    fn measure_spans(&mut self, text: &str, spans: &[Span], style: TextStyle, wrap_width: Option<f32>) -> Size {
        let layout = self.layout_spans(text, spans, style, wrap_width);
        Size::new(layout.width(), layout.height())
    }

    fn span_rects(
        &mut self,
        text: &str,
        spans: &[Span],
        style: TextStyle,
        wrap_width: Option<f32>,
        range: Range<usize>,
    ) -> Vec<Rect> {
        Typesetter::span_rects(self, text, spans, style, wrap_width, range)
    }
}

impl std::fmt::Debug for Typesetter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Typesetter")
            .field("current", &self.current.values().map(HashMap::len).sum::<usize>())
            .field("previous", &self.previous.values().map(HashMap::len).sum::<usize>())
            .finish_non_exhaustive()
    }
}

impl Typesetter {
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
        self.layout_spans(text, &[], style, wrap)
    }

    /// The layout of `text` in `style` with `spans` over it, cached as
    /// [`layout`](Self::layout) caches plain text.
    pub(crate) fn layout_spans(
        &mut self,
        text: &str,
        spans: &[Span],
        style: TextStyle,
        wrap: Option<f32>,
    ) -> &TextLayout {
        let key = LayoutKey::new(style, spans, wrap);
        let found = self.current.get(&key).is_some_and(|by_text| by_text.contains_key(text));
        if !found {
            let layout = match self.previous.get_mut(&key).and_then(|by_text| by_text.remove(text)) {
                Some(layout) => layout,
                None => {
                    self.built += 1;
                    self.build(text, spans, style, wrap)
                }
            };
            self.current
                .entry(key.clone())
                .or_default()
                .insert(text.to_owned(), layout);
        }
        &self.current[&key][text]
    }

    /// The size of `text` in `style`, read from the same cached layout that
    /// is drawn.
    ///
    /// Spaces at the end of the text count, so a caret after a typed space
    /// moves past it. A space where wrapping breaks a line does not, as it
    /// takes no room in the box.
    pub fn measure(&mut self, text: &str, style: TextStyle, wrap: Option<f32>) -> Size {
        let layout = self.layout(text, style, wrap);
        Size::new(layout.width(), layout.height())
    }

    /// Where the bytes `range` of `text` lie, set with `spans` in `style`
    /// and wrapped at `wrap`: one rectangle per line, top to bottom, from
    /// the text's top-left corner, each as tall as its line. Empty for an
    /// empty range, or one that does not fall on character boundaries.
    pub fn span_rects(
        &mut self,
        text: &str,
        spans: &[Span],
        style: TextStyle,
        wrap: Option<f32>,
        range: Range<usize>,
    ) -> Vec<Rect> {
        if range.is_empty()
            || range.end > text.len()
            || !text.is_char_boundary(range.start)
            || !text.is_char_boundary(range.end)
        {
            return Vec::new();
        }
        let layout = self.layout_spans(text, spans, style, wrap);
        let selection = Selection::new(
            Cursor::from_byte_index(layout, range.start, Affinity::Downstream),
            Cursor::from_byte_index(layout, range.end, Affinity::Upstream),
        );
        // Each rectangle takes its line's own box from top to bottom, not
        // the selection's, which reaches out to the tallest glyph: a link's
        // hit area and ring then stay on its line.
        selection
            .geometry(layout)
            .into_iter()
            .map(|(rect, line)| {
                let (top, bottom) = layout.get(line).map_or((rect.y0 as f32, rect.y1 as f32), |line| {
                    let metrics = line.metrics();
                    (metrics.block_min_coord, metrics.block_max_coord)
                });
                Rect::new(
                    Point::new(rect.x0 as f32, top),
                    Size::new((rect.x1 - rect.x0) as f32, bottom - top),
                )
            })
            .collect()
    }

    /// How many layouts were built since the last call.
    pub fn take_built(&mut self) -> u64 {
        std::mem::take(&mut self.built)
    }

    /// Sets `text` from nothing.
    fn build(&mut self, text: &str, spans: &[Span], style: TextStyle, wrap: Option<f32>) -> TextLayout {
        let mut builder = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        for property in properties(style) {
            builder.push_default(property);
        }
        for (index, (span, range)) in spans.iter().zip(Span::ranges(spans, text.len())).enumerate() {
            if range.is_empty() {
                continue;
            }
            builder.push(StyleProperty::Brush(brush(index)), range.clone());
            if span.weight == Weight::Bold {
                builder.push(StyleProperty::FontWeight(parley::FontWeight::BOLD), range.clone());
            }
            if span.face == Face::Mono {
                builder.push(
                    StyleProperty::FontFamily(FontFamily::from(GenericFamily::Monospace)),
                    range,
                );
            }
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
