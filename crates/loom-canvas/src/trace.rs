//! Scope traces and other live displays inside their tiles
//! (`docs/design/panels.md`, Drawing the scope trace).
//!
//! The app gives the canvas each display's points in [`ScopeTraces`], apart
//! from the [`Scene`] and the panel values. A new trace rebuilds only the
//! trace's own small buffer, never the tiles, ports, cables, or controls
//! (ADR-0010). The frame around the trace is drawn with the controls, so a
//! still scope costs nothing per frame.

use crate::gpu::{Shape, packed};
use crate::scene::{Scene, WidgetViewKind};
use crate::theme::Palette;
use bytemuck::{Pod, Zeroable};
use loom_ui_core::{Color, Point, Rect, Size};
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
/// Width of a lighter second line, in world units.
const PEAK_WIDTH: f32 = 0.8;
/// How bright an XY scope's oldest point is, from 0 to 1.
const XY_FADEST: f32 = 0.15;
/// How far a level history's column reaches over the next one, in world units.
const SEAM: f32 = 0.6;
/// Where a spectrum's faint grid lines sit: 100 Hz, 1 kHz, and 10 kHz, on its
/// logarithmic axis from 20 Hz to 20 kHz (panel-widgets.md, Spectrum).
const SPECTRUM_GRID: [f32; 3] = [0.232_99, 0.566_33, 0.899_66];

/// The points of one scope trace.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trace {
    /// Points from left to right. `x` runs from 0 at the frame's left edge
    /// to 1 at its right edge, and may start a little below 0: the trace is
    /// shifted left by the fraction of a sample between the trigger and the
    /// first point, and the frame clips it. `y` runs from -1 at the bottom
    /// to 1 at the top, and is clamped to that.
    pub points: Vec<[f32; 2]>,
    /// A second, lighter line over the same frame: a level history's peak or
    /// a spectrum's hold. Drawn only close up.
    pub second: Vec<[f32; 2]>,
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
    fn new(from: Point, to: Point, colour: Color, clip: Rect) -> Self {
        Self::with_width(from, to, colour, clip, TRACE_WIDTH)
    }

    /// A piece `width` world units wide, never thinner than one physical
    /// pixel. Result previews use it for their curves, so a preview is one
    /// anti-aliased line with round joins at every zoom (#455).
    pub(crate) fn with_width(from: Point, to: Point, colour: Color, clip: Rect, width: f32) -> Self {
        Self {
            ends: [from.x, from.y, to.x, to.y],
            colour: packed(colour),
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
pub(crate) fn trace_area(rect: Rect) -> Rect {
    rect.expand(-INSET)
}

/// The area inside an XY scope's rectangle that holds its trace: the largest
/// centred square. Both axes then have the same scale at any tile size, so a
/// circle stays a circle (panel-widgets.md, XY scope).
pub(crate) fn xy_scope_area(rect: Rect) -> Rect {
    let area = trace_area(rect);
    let side = area.width.min(area.height);
    Rect::new(
        Point::new(area.center_x() - side * 0.5, area.center_y() - side * 0.5),
        Size::new(side, side),
    )
}

/// GPU instances for every scope trace in a scene.
#[derive(Debug, Default)]
pub(crate) struct TraceGeometry {
    /// The scene and trace generations this was built from, and whether
    /// the full traces were drawn (close zoom) or flat lines (middle).
    pub key: (u64, u64, bool),
    /// Every trace's pieces.
    pub segments: Vec<Segment>,
    /// Every filled shape under a trace, such as a level history's columns.
    /// In its own buffer, so a moving history uploads nothing else.
    pub fills: Vec<Shape>,
}

impl TraceGeometry {
    /// Builds the traces. Close up, each is a line through its points.
    /// At the middle zoom, `full` is false: each is a flat line across the
    /// frame's centre, as bright as its level.
    pub fn build(scene: &Scene, traces: &ScopeTraces, full: bool, palette: &Palette) -> Self {
        crate::stats::count(crate::stats::Counter::TraceBuilds);
        let mut segments = Vec::new();
        let mut fills = Vec::new();
        for module in scene.modules() {
            for (index, widget) in module.panel.widgets.iter().enumerate() {
                if !widget.kind.draws_a_trace() {
                    continue;
                }
                let (Some(rect), Some(trace)) = (module.widget_bounds(index), traces.get(module.id, index)) else {
                    continue;
                };
                let area = if widget.kind == WidgetViewKind::XyScope {
                    xy_scope_area(rect)
                } else {
                    trace_area(rect)
                };
                let at = |[x, y]: [f32; 2]| {
                    Point::new(
                        area.x + x * area.width,
                        area.center_y() - y.clamp(-1.0, 1.0) * area.height * 0.5,
                    )
                };
                // The lighter line: a level history's peak, or a spectrum's hold.
                let second_line = || {
                    let colour = crate::controls::mix(palette.light_on, palette.text, 0.5);
                    trace
                        .second
                        .windows(2)
                        .map(move |pair| Segment::with_width(at(pair[0]), at(pair[1]), colour, area, PEAK_WIDTH))
                };
                if widget.kind == WidgetViewKind::LevelHistory {
                    // One flat column per point, because a capsule has a
                    // round top. The columns meet, so they read as one shape.
                    let step = match trace.points.as_slice() {
                        [first, next, ..] => next[0] - first[0],
                        _ => 0.0,
                    };
                    let column = area.width * step;
                    let colour = crate::controls::mix(palette.light_off, palette.light_on, 0.7);
                    for &point in &trace.points {
                        let top = at(point);
                        let height = area.y + area.height - top.y;
                        if height > 0.0 {
                            // A little wider, so the soft edges of two columns
                            // do not leave a seam between them.
                            let width = (column + SEAM).min(area.x + area.width - top.x);
                            let rect = Rect::new(top, Size::new(width, height));
                            fills.push(Shape::new(rect, 0.0, colour, Color::TRANSPARENT, 0.0));
                        }
                    }
                    if full {
                        segments.extend(second_line());
                    }
                } else if widget.kind == WidgetViewKind::XyScope {
                    // Older points fade, so the newest part is brightest. The
                    // app sends one point at the middle zoom, drawn as a dot.
                    if let [only] = trace.points.as_slice() {
                        segments.push(Segment::new(at(*only), at(*only), palette.light_on, area));
                    }
                    let last = trace.points.len().saturating_sub(2).max(1) as f32;
                    segments.extend(trace.points.windows(2).enumerate().map(|(i, pair)| {
                        let age = XY_FADEST + (1.0 - XY_FADEST) * i as f32 / last;
                        let colour = crate::controls::mix(palette.background, palette.light_on, age);
                        Segment::new(at(pair[0]), at(pair[1]), colour, area)
                    }));
                } else if widget.kind == WidgetViewKind::Spectrum {
                    if full {
                        for x in SPECTRUM_GRID {
                            let (top, bottom) = (at([x, 1.0]), at([x, -1.0]));
                            segments.push(Segment::new(top, bottom, palette.tile_border, area));
                        }
                        segments.extend(second_line());
                    }
                    segments.extend(
                        trace
                            .points
                            .windows(2)
                            .map(|pair| Segment::new(at(pair[0]), at(pair[1]), palette.light_on, area)),
                    );
                } else if full {
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
            fills,
        }
    }
}

#[cfg(test)]
mod tests;
