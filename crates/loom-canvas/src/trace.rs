//! Scope traces inside their tiles (`docs/design/panels.md`, Drawing the
//! scope trace).
//!
//! The app gives the canvas each scope's points in [`ScopeTraces`], apart
//! from the [`Scene`] and the panel values. A new trace rebuilds only the
//! trace's own small buffer, never the tiles, ports, cables, or controls
//! (ADR-0010). The frame around the trace is drawn with the controls, so a
//! still scope costs nothing per frame.

use crate::scene::{Scene, WidgetViewKind};
use crate::theme::Palette;
use bytemuck::{Pod, Zeroable};
use iced::advanced::graphics::color::pack;
use iced::{Color, Point, Rectangle};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// The most points one trace draws. More are reduced by the app, keeping
/// the lowest and highest sample of each group, so a short spike is never
/// skipped.
pub const MAX_POINTS: usize = 1024;

/// Line width of a trace, in world units.
const TRACE_WIDTH: f32 = 1.2;
/// Thinnest a trace is drawn, in physical pixels.
const TRACE_MIN_PX: f32 = 1.0;
/// How far the trace area sits inside the scope widget's rectangle.
const INSET: f32 = 2.0;

/// The points of one scope trace.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trace {
    /// Points from left to right. `x` runs from 0 at the frame's left edge
    /// to 1 at its right edge, and may start a little below 0: the trace is
    /// shifted left by the fraction of a sample between the trigger and the
    /// first point, and the frame clips it. `y` runs from -1 at the bottom
    /// to 1 at the top, and is clamped to that.
    pub points: Vec<[f32; 2]>,
    /// The signal's level from 0 to 1 (-60 dB to 0 dB). At the middle zoom
    /// the scope is a flat line with this brightness.
    pub level: f32,
}

/// The traces of every scope, by module ID and widget index.
#[derive(Debug, Clone, Default)]
pub struct ScopeTraces {
    traces: HashMap<(u32, usize), Trace>,
    generation: u64,
}

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

impl ScopeTraces {
    /// No traces: every scope shows an empty frame.
    pub fn new() -> Self {
        Self {
            traces: HashMap::new(),
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Sets a scope's trace. The generation changes only if the trace did,
    /// so setting the same trace does not redraw.
    pub fn set(&mut self, module: u32, widget: usize, trace: Trace) {
        if self.traces.get(&(module, widget)) != Some(&trace) {
            self.traces.insert((module, widget), trace);
            self.bump();
        }
    }

    /// Keeps only the traces `keep` returns true for.
    pub fn retain(&mut self, mut keep: impl FnMut(u32, usize) -> bool) {
        let before = self.traces.len();
        self.traces.retain(|&(module, widget), _| keep(module, widget));
        if self.traces.len() != before {
            self.bump();
        }
    }

    /// A scope's trace, if set.
    pub fn get(&self, module: u32, widget: usize) -> Option<&Trace> {
        self.traces.get(&(module, widget))
    }

    /// Changes whenever a trace changes. Unique across all trace sets.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn bump(&mut self) {
        self.generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
    }
}

/// One straight piece of a trace, as the GPU draws it: a capsule from one
/// point to the next, clipped to the scope's frame.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub struct Segment {
    /// Start x, start y, end x, end y, in world units.
    ends: [f32; 4],
    colour: [f32; 4],
    /// The frame: x, y, width, height in world units. Nothing outside it is
    /// drawn.
    clip: [f32; 4],
    /// Width (world), minimum width (physical px), unused, unused.
    params: [f32; 4],
}

impl Segment {
    fn new(from: Point, to: Point, colour: Color, clip: Rectangle) -> Self {
        Self::with_width(from, to, colour, clip, TRACE_WIDTH)
    }

    /// A piece `width` world units wide, never thinner than one physical
    /// pixel. Result previews use it for their curves, so a preview is one
    /// anti-aliased line with round joins at every zoom (#455).
    pub(crate) fn with_width(from: Point, to: Point, colour: Color, clip: Rectangle, width: f32) -> Self {
        Self {
            ends: [from.x, from.y, to.x, to.y],
            colour: pack(colour).components(),
            clip: [clip.x, clip.y, clip.width, clip.height],
            params: [width, TRACE_MIN_PX, 0.0, 0.0],
        }
    }

    /// The two ends in world units.
    #[cfg(test)]
    pub fn ends(&self) -> (Point, Point) {
        (
            Point::new(self.ends[0], self.ends[1]),
            Point::new(self.ends[2], self.ends[3]),
        )
    }

    /// The width in world units.
    #[cfg(test)]
    pub fn width(&self) -> f32 {
        self.params[0]
    }
}

/// The area inside a scope widget's rectangle that holds its trace.
pub(crate) fn trace_area(rect: Rectangle) -> Rectangle {
    rect.shrink(INSET)
}

/// GPU instances for every scope trace in a scene.
#[derive(Debug, Default)]
pub(crate) struct TraceGeometry {
    /// The scene and trace generations this was built from, and whether
    /// the full traces were drawn (close zoom) or flat lines (middle).
    pub key: (u64, u64, bool),
    /// Every trace's pieces.
    pub segments: Vec<Segment>,
}

impl TraceGeometry {
    /// Builds the traces. Close up, each is a line through its points.
    /// At the middle zoom, `full` is false: each is a flat line across the
    /// frame's centre, as bright as its level.
    pub fn build(scene: &Scene, traces: &ScopeTraces, full: bool, palette: &Palette) -> Self {
        crate::stats::count(crate::stats::Counter::TraceBuilds);
        let mut segments = Vec::new();
        for module in scene.modules() {
            for (index, widget) in module.panel.widgets.iter().enumerate() {
                if widget.kind != WidgetViewKind::Scope {
                    continue;
                }
                let (Some(rect), Some(trace)) = (module.widget_bounds(index), traces.get(module.id, index)) else {
                    continue;
                };
                let area = trace_area(rect);
                let at = |[x, y]: [f32; 2]| {
                    Point::new(
                        area.x + x * area.width,
                        area.center_y() - y.clamp(-1.0, 1.0) * area.height * 0.5,
                    )
                };
                if full {
                    segments.extend(
                        trace
                            .points
                            .windows(2)
                            .map(|pair| Segment::new(at(pair[0]), at(pair[1]), palette.light_on, area)),
                    );
                } else if trace.level > 0.0 {
                    let colour = crate::controls::mix(palette.light_off, palette.light_on, trace.level.clamp(0.0, 1.0));
                    segments.push(Segment::new(at([0.0, 0.0]), at([1.0, 0.0]), colour, area));
                }
            }
        }
        Self {
            key: (scene.generation(), traces.generation(), full),
            segments,
        }
    }
}

#[cfg(test)]
mod tests;
