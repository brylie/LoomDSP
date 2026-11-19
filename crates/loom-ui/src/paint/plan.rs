//! What a paint list draws, as plain data: its boxes as shapes in the
//! target's logical pixels, its glyphs as quads from the atlas, and the
//! draw steps, layer by layer and batch by batch in the order painted.
//!
//! The plan is built on the CPU, so its order is tested without a GPU.
//! [`PaintRenderer`](super::PaintRenderer) uploads and draws it.

use super::atlas::{Atlas, Full, GlyphQuad};
use super::list::{Batch, PaintBox};
use super::{PaintList, Segment};
use bytemuck::{Pod, Zeroable};
use loom_ui_core::{Color, Point, Rect, Size};
use std::ops::Range;

/// A colour as the shaders read it: red, green, blue, and alpha, each
/// unchanged. The channels stay sRGB-encoded, and the shaders blend in that
/// space, which is how the rest of the window blends its colours.
pub fn packed(colour: Color) -> [f32; 4] {
    [colour.r, colour.g, colour.b, colour.a]
}

/// One rounded rectangle, as the shape stage of `paint.wgsl` reads it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Shape {
    rect: [f32; 4],
    fill: [f32; 4],
    border: [f32; 4],
    params: [f32; 4],
}

impl Shape {
    /// A filled rounded rectangle with an optional border. Its place, size,
    /// radius, and border width are in the units of the globals it is drawn
    /// with: logical pixels for painted boxes, or a host's own units scaled
    /// by its zoom.
    pub fn new(rect: Rect, radius: f32, fill: Color, border: Color, border_width: f32) -> Self {
        Self {
            rect: [rect.x, rect.y, rect.width, rect.height],
            fill: packed(fill),
            border: packed(border),
            params: [radius, border_width, 1.0, 0.0],
        }
    }

    /// A filled rectangle rotated about the centre of the mark.
    fn segment(segment: Segment, origin: Point) -> Self {
        let delta = segment.to - segment.from;
        let length = delta.x.hypot(delta.y);
        let centre = segment.from + delta * 0.5;
        let rect = Rect::new(
            Point::new(
                centre.x - origin.x - length * 0.5,
                centre.y - origin.y - segment.width * 0.5,
            ),
            Size::new(length, segment.width),
        );
        let mut shape = Self::new(rect, 0.0, segment.colour, Color::TRANSPARENT, 0.0);
        shape.params[3] = delta.y.atan2(delta.x);
        shape
    }

    /// The rectangle: x, y, width, height.
    pub fn rect(&self) -> [f32; 4] {
        self.rect
    }

    /// The fill colour, as uploaded.
    pub fn fill(&self) -> [f32; 4] {
        self.fill
    }

    /// The border colour, as uploaded.
    pub fn border(&self) -> [f32; 4] {
        self.border
    }

    /// The border width.
    pub fn border_width(&self) -> f32 {
        self.params[1]
    }
}

/// Where a host draws: the size of its viewport and how dense the pixels
/// are. The host that owns the window gives it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    /// The viewport's size in logical pixels.
    pub size: Size,
    /// Physical pixels per logical pixel.
    pub scale: f32,
    /// The viewport's top-left corner in the host's logical pixels: where
    /// painted text and boxes are placed from, and snapped to pixels.
    pub origin: Point,
    /// The whole target's size in physical pixels, which scissors are cut
    /// to.
    pub physical: (u32, u32),
}

/// One draw call of the plan.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Step {
    /// The scissor, in the target's physical pixels: x, y, width, height.
    pub(crate) scissor: [u32; 4],
    /// What it draws.
    pub(crate) draw: Draw,
}

/// What one step draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Draw {
    /// A range of the screen-space box shapes.
    Boxes(Range<u32>),
    /// A range of the glyph quads.
    Glyphs(Range<u32>),
}

/// What the painted part of a frame draws, in order.
#[derive(Debug, Clone, Default)]
pub(crate) struct PaintPlan {
    /// The box shapes, in screen space: logical pixels from the viewport's
    /// corner.
    pub(crate) boxes: Vec<Shape>,
    /// The glyph quads.
    pub(crate) glyphs: Vec<GlyphQuad>,
    /// The draw calls, back to front.
    pub(crate) steps: Vec<Step>,
    /// Glyphs left out because the atlas was full at its largest size.
    pub(crate) skipped: u64,
}

/// A clip in the host's logical pixels as a scissor in the target's
/// physical pixels, cut to the target. `None` when nothing of it is in it,
/// or when it is empty: rounding outward would grow an empty clip at a
/// fractional place into a pixel that shows.
pub(crate) fn scissor(clip: Rect, target: Target) -> Option<[u32; 4]> {
    if clip.width <= 0.0 || clip.height <= 0.0 {
        return None;
    }
    let (width, height) = (target.physical.0 as f32, target.physical.1 as f32);
    let x0 = (clip.x * target.scale).floor().clamp(0.0, width);
    let y0 = (clip.y * target.scale).floor().clamp(0.0, height);
    let x1 = ((clip.x + clip.width) * target.scale).ceil().clamp(0.0, width);
    let y1 = ((clip.y + clip.height) * target.scale).ceil().clamp(0.0, height);
    (x1 > x0 && y1 > y0).then_some([x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32])
}

impl PaintPlan {
    /// The plan for `list`, placing its glyphs in `atlas`. When the atlas
    /// fills, it grows and the frame is placed again; at its largest size,
    /// a glyph that does not fit is left out and counted.
    pub(crate) fn build(list: &PaintList, atlas: &mut Atlas, target: Target) -> Self {
        atlas.begin_frame();
        let origin = target.origin;
        'frame: loop {
            let mut plan = PaintPlan::default();
            for layer in &list.layers {
                let Some(scissor) = scissor(layer.clip, target) else {
                    continue;
                };
                for batch in &layer.batches {
                    let draw = match batch.clone() {
                        Batch::Boxes(range) => {
                            let start = plan.boxes.len() as u32;
                            plan.boxes
                                .extend(
                                    list.boxes[range.start as usize..range.end as usize].iter().map(
                                        |shape| match *shape {
                                            PaintBox::Quad(quad) => {
                                                let mut rect = quad.rect;
                                                rect.x -= target.origin.x;
                                                rect.y -= target.origin.y;
                                                Shape::new(rect, quad.radius, quad.fill, quad.border, quad.border_width)
                                            }
                                            PaintBox::Segment(segment) => Shape::segment(segment, target.origin),
                                        },
                                    ),
                                );
                            Draw::Boxes(start..plan.boxes.len() as u32)
                        }
                        Batch::Glyphs(range) => {
                            let start = plan.glyphs.len() as u32;
                            for glyph in &list.glyphs[range.start as usize..range.end as usize] {
                                let face = &list.faces[glyph.face as usize];
                                let clip = glyph.clip.intersection(layer.clip);
                                let Some(clip) = clip else { continue };
                                match atlas.place(glyph, face, clip, origin, target.scale) {
                                    Ok(Some(quad)) => plan.glyphs.push(quad),
                                    Ok(None) => {}
                                    Err(Full) if atlas.grow() => continue 'frame,
                                    Err(Full) => {
                                        atlas.skipped += 1;
                                        plan.skipped += 1;
                                    }
                                }
                            }
                            Draw::Glyphs(start..plan.glyphs.len() as u32)
                        }
                    };
                    let empty = match &draw {
                        Draw::Boxes(range) | Draw::Glyphs(range) => range.is_empty(),
                    };
                    if !empty {
                        plan.steps.push(Step { scissor, draw });
                    }
                }
            }
            return plan;
        }
    }
}

#[cfg(test)]
mod tests;
