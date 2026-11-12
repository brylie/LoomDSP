//! The glyph atlas: glyph images rasterised with `swash` and packed with
//! `etagere` into two textures, one of coverage and one of colour.
//!
//! This half is plain code with no GPU: it decides where each glyph's
//! image goes, makes the images to upload, and turns placed glyphs into
//! quads. `text.rs` beside it owns the textures and draws.
//!
//! - A glyph is rasterised at its size in physical pixels, and placed on
//!   the baseline at a whole pixel down and a quarter pixel across, so
//!   text is sharp and its spacing even.
//! - Each glyph keeps the frame it was last used in. When a page is full,
//!   glyphs no frame used since the last began are freed. A zoom gesture
//!   leaves old sizes unused, so they are what goes.
//! - When that is not enough, the atlas doubles, up to its largest size,
//!   and every glyph of the frame is placed again. At the largest size,
//!   glyphs that still do not fit are left out and counted, never a panic.

use crate::text::list::{Face, PlacedGlyph};
use bytemuck::{Pod, Zeroable};
use loom_ui_core::{Point, Rect};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use swash::scale::image::{Content, Image};
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Angle, Format, Transform, Vector};

/// The atlas's first size, in texels on a side.
pub(crate) const START_SIZE: u32 = 512;
/// The atlas's largest size, unless the device allows less.
pub(crate) const MAX_SIZE: u32 = 4096;
/// Horizontal positions a glyph image is made for, per pixel.
const BINS: f32 = 4.0;
/// Texels left empty around each image, so sampling never reads a
/// neighbour's edge.
const PAD: u32 = 1;

/// Which texture an image is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Page {
    /// One byte per texel: how much of the pixel the glyph covers.
    Mask = 0,
    /// Four bytes per texel, premultiplied: an emoji or a colour font.
    Colour = 1,
}

impl Page {
    /// Bytes per texel.
    pub(crate) fn bytes(self) -> u32 {
        match self {
            Page::Mask => 1,
            Page::Colour => 4,
        }
    }
}

/// What makes two glyph images the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    font: u64,
    index: u32,
    glyph: u32,
    /// Size in physical pixels, as bits.
    size: u32,
    /// A hash of the variation coordinates.
    coords: u64,
    embolden: bool,
    skew: u32,
    /// The quarter pixel across the image is made for.
    bin: u8,
}

impl GlyphKey {
    /// The key for glyph `glyph` of `face` at `scale`, in `bin`.
    fn new(face: &Face, glyph: u32, scale: f32, bin: u8) -> Self {
        let mut coords = std::collections::hash_map::DefaultHasher::new();
        face.coords.hash(&mut coords);
        Self {
            font: face.font.data.id(),
            index: face.font.index,
            glyph,
            size: (face.size * scale).to_bits(),
            coords: coords.finish(),
            embolden: face.embolden,
            skew: face.skew.unwrap_or(0.0).to_bits(),
            bin,
        }
    }
}

/// Where a glyph's image is, and how it sits on the baseline.
#[derive(Debug, Clone, Copy)]
struct Entry {
    page: Page,
    /// Its place in the page, or `None` for a glyph with no image (a
    /// space).
    alloc: Option<etagere::AllocId>,
    /// The image's corner in the page, in texels.
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    /// From the pen position to the image's left edge, in physical pixels.
    left: i32,
    /// From the baseline up to the image's top edge, in physical pixels.
    top: i32,
    /// The frame that last used it.
    used: u64,
}

/// New texels to write to a page.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Upload {
    pub(crate) page: Page,
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// `width * height * page.bytes()` bytes, row by row.
    pub(crate) data: Vec<u8>,
}

/// One glyph quad for the shader, in the layout of a shape instance: four
/// `vec4`s.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct GlyphQuad {
    /// x, y, width, height in logical pixels from the canvas's corner.
    pub(crate) rect: [f32; 4],
    /// The image's corners in the page, in texels: left, top, right,
    /// bottom.
    pub(crate) uv: [f32; 4],
    /// The colour of a coverage glyph's letters.
    pub(crate) colour: [f32; 4],
    /// 1 in the first for a colour glyph; the rest unused.
    pub(crate) params: [f32; 4],
}

/// The atlas is full at its largest size: the frame must be placed again
/// after [`Atlas::grow`], or the glyph left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Full;

/// Glyph images and where they are.
pub(crate) struct Atlas {
    size: u32,
    max: u32,
    packers: [etagere::BucketedAtlasAllocator; 2],
    entries: HashMap<GlyphKey, Entry>,
    frame: u64,
    context: ScaleContext,
    /// Texels to write before drawing, oldest first.
    pub(crate) uploads: Vec<Upload>,
    /// True when the pages must be made again at [`size`](Self::size),
    /// empty, before the uploads are written.
    pub(crate) resized: bool,
    /// Glyphs left out because the atlas was full at its largest size,
    /// since the counter was last read.
    pub(crate) skipped: u64,
}

impl std::fmt::Debug for Atlas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Atlas")
            .field("size", &self.size)
            .field("glyphs", &self.entries.len())
            .field("frame", &self.frame)
            .finish_non_exhaustive()
    }
}

/// An empty packer for a page `size` texels on a side.
fn packer(size: u32) -> etagere::BucketedAtlasAllocator {
    etagere::BucketedAtlasAllocator::new(etagere::size2(size as i32, size as i32))
}

impl Atlas {
    /// An empty atlas that may grow to `max` texels on a side.
    pub(crate) fn new(max: u32) -> Self {
        Self::sized(START_SIZE, max)
    }

    /// An empty atlas `start` texels on a side that may grow to `max`.
    pub(crate) fn sized(start: u32, max: u32) -> Self {
        let max = max.clamp(1, MAX_SIZE);
        let size = start.clamp(1, max);
        Self {
            size,
            max,
            packers: [packer(size), packer(size)],
            entries: HashMap::new(),
            frame: 0,
            context: ScaleContext::new(),
            uploads: Vec::new(),
            resized: true,
            skipped: 0,
        }
    }

    /// The pages' size, in texels on a side.
    pub(crate) fn size(&self) -> u32 {
        self.size
    }

    /// How many glyph images the atlas holds.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Starts a frame: glyphs used before it may now be freed.
    pub(crate) fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// Doubles the pages and empties them, so the frame's glyphs are placed
    /// again. False at the largest size, where nothing changes.
    pub(crate) fn grow(&mut self) -> bool {
        if self.size >= self.max {
            return false;
        }
        self.size = (self.size * 2).min(self.max);
        self.packers = [packer(self.size), packer(self.size)];
        self.entries.clear();
        self.uploads.clear();
        self.resized = true;
        true
    }

    /// The quad for one glyph, cut to `clip`, placed for a canvas whose
    /// corner is at `origin` in the host's logical pixels, at `scale`
    /// physical pixels per logical one. `None` when nothing of it shows.
    pub(crate) fn place(
        &mut self,
        glyph: &PlacedGlyph,
        face: &Face,
        clip: Rect,
        origin: Point,
        scale: f32,
    ) -> Result<Option<GlyphQuad>, Full> {
        // Whole physical pixels down; quarter pixels across.
        let x = glyph.origin.x * scale;
        let whole = x.floor();
        let bin = (((x - whole) * BINS).floor() as u8).min(BINS as u8 - 1);
        let baseline = (glyph.origin.y * scale).round();
        let key = GlyphKey::new(face, glyph.id, scale, bin);
        let entry = match self.entries.get_mut(&key) {
            Some(entry) => {
                entry.used = self.frame;
                *entry
            }
            None => {
                let entry = self.add(face, glyph.id, scale, bin)?;
                self.entries.insert(key, entry);
                entry
            }
        };
        if entry.alloc.is_none() {
            return Ok(None);
        }
        // The image's place, in logical pixels from the canvas's corner.
        let left = (whole + entry.left as f32) / scale - origin.x;
        let top = (baseline - entry.top as f32) / scale - origin.y;
        let image = Rect::new(
            Point::new(left, top),
            loom_ui_core::Size::new(entry.width as f32 / scale, entry.height as f32 / scale),
        );
        let clip = Rect::new(Point::new(clip.x - origin.x, clip.y - origin.y), clip.size());
        let Some(shown) = image.intersection(clip) else {
            return Ok(None);
        };
        // The same cut in texels: one texel is one physical pixel.
        let u0 = entry.x as f32 + (shown.x - image.x) * scale;
        let v0 = entry.y as f32 + (shown.y - image.y) * scale;
        Ok(Some(GlyphQuad {
            rect: [shown.x, shown.y, shown.width, shown.height],
            uv: [u0, v0, u0 + shown.width * scale, v0 + shown.height * scale],
            colour: crate::gpu::packed(glyph.colour),
            params: [if entry.page == Page::Colour { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
        }))
    }

    /// Rasterises a glyph and finds it a place, freeing unused glyphs if
    /// its page is full.
    fn add(&mut self, face: &Face, glyph: u32, scale: f32, bin: u8) -> Result<Entry, Full> {
        let used = self.frame;
        let Some(image) = self.rasterise(face, glyph, scale, bin) else {
            return Ok(Entry {
                page: Page::Mask,
                alloc: None,
                x: 0,
                y: 0,
                width: 0,
                height: 0,
                left: 0,
                top: 0,
                used,
            });
        };
        let page = match image.content {
            Content::Color => Page::Colour,
            Content::Mask | Content::SubpixelMask => Page::Mask,
        };
        let (width, height) = (image.placement.width, image.placement.height);
        let empty = Entry {
            page,
            alloc: None,
            x: 0,
            y: 0,
            width,
            height,
            left: image.placement.left,
            top: image.placement.top,
            used,
        };
        if width == 0 || height == 0 {
            return Ok(empty);
        }
        let want = etagere::size2((width + 2 * PAD) as i32, (height + 2 * PAD) as i32);
        let allocation = match self.packers[page as usize].allocate(want) {
            Some(allocation) => allocation,
            None => {
                self.free_unused(page);
                self.packers[page as usize].allocate(want).ok_or(Full)?
            }
        };
        let corner = allocation.rectangle.min;
        let (x, y) = (corner.x as u32 + PAD, corner.y as u32 + PAD);
        let mut data = image.data;
        // swash 0.2.10 gives colour outlines (COLR, as in Segoe UI Emoji)
        // premultiplied: it draws each layer over a transparent image, as
        // `(1 - a) * dst + a * colour`. Colour bitmaps (sbix and CBDT, as
        // in Apple Color Emoji) are PNG data, which is straight alpha, and
        // are scaled from their nearest strike to the size asked for.
        if matches!(image.source, Source::ColorBitmap(_)) {
            premultiply(&mut data);
        }
        self.uploads.push(Upload {
            page,
            x,
            y,
            width,
            height,
            data,
        });
        Ok(Entry {
            alloc: Some(allocation.id),
            x,
            y,
            ..empty
        })
    }

    /// Frees every glyph on `page` that the current frame has not used.
    fn free_unused(&mut self, page: Page) {
        let frame = self.frame;
        let packer = &mut self.packers[page as usize];
        self.entries.retain(|_, entry| {
            if entry.page != page || entry.used == frame {
                return true;
            }
            if let Some(alloc) = entry.alloc {
                packer.deallocate(alloc);
            }
            false
        });
    }

    /// The glyph's image at its size in physical pixels, offset by `bin`
    /// quarter pixels. A colour image first, so an emoji keeps its colours.
    fn rasterise(&mut self, face: &Face, glyph: u32, scale: f32, bin: u8) -> Option<Image> {
        let font = swash::FontRef::from_index(face.font.data.data(), face.font.index as usize)?;
        let size = face.size * scale;
        let mut scaler = self
            .context
            .builder(font)
            .size(size)
            .hint(false)
            .normalized_coords(face.coords.iter())
            .build();
        let sources = [
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ];
        let mut render = Render::new(&sources);
        render.format(Format::Alpha).offset(Vector::new(bin as f32 / BINS, 0.0));
        if face.embolden {
            // About what a bold face adds to a regular one's stems.
            render.embolden(size / 32.0);
        }
        if let Some(degrees) = face.skew {
            render.transform(Some(Transform::skew(Angle::from_degrees(degrees), Angle::ZERO)));
        }
        render.render(&mut scaler, u16::try_from(glyph).ok()?)
    }
}

/// Turns straight RGBA into premultiplied RGBA, as the colour page holds.
fn premultiply(data: &mut [u8]) {
    for texel in data.as_chunks_mut::<4>().0 {
        let alpha = texel[3] as u32;
        for channel in &mut texel[..3] {
            *channel = ((*channel as u32 * alpha + 127) / 255) as u8;
        }
    }
}

#[cfg(test)]
mod tests;
