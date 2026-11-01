//! Panel controls on tiles: their live values and their GPU instances
//! (ADR-0013). Spec: `docs/design/panels.md`, Module panels on tiles.
//!
//! Values live apart from the [`Scene`]. Changing one rebuilds only the
//! controls' instances, never the tiles, ports, or cables (ADR-0010).

use crate::gpu::{Arc as ArcShape, Shape};
use crate::scene::{GRID, Scene, WidgetViewKind};
use crate::theme::Palette;
use crate::trace::Segment;
use iced::{Color, Point, Rectangle, Size};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Width of a result preview's curve, in world units.
const PREVIEW_WIDTH: f32 = 1.5;

/// A knob's ring starts here, in radians clockwise from the top.
pub(crate) const KNOB_START: f32 = -0.75 * std::f32::consts::PI;
/// A knob's ring spans this angle.
pub(crate) const KNOB_SWEEP: f32 = 1.5 * std::f32::consts::PI;
/// Radius of a knob's ring, in world units.
const KNOB_RADIUS: f32 = 9.0;
/// The least of a knob's value arc that is drawn, as a share of its sweep.
/// The white pointer still marks the exact value.
const KNOB_START_CAP: f32 = 0.04;
/// Radius of a knob's modulation range arc: just outside its ring.
const RANGE_RADIUS: f32 = 11.75;
/// A knob's focus ring reaches this far left, right, and up from its centre:
/// the ring's border lies just outside the range arc.
const KNOB_FOCUS_HALF_WIDTH: f32 = 15.0;
const KNOB_FOCUS_TOP: f32 = 15.0;
/// How far down the focus ring reaches: above the caption.
const KNOB_FOCUS_BOTTOM: f32 = 12.5;
/// Corner radius of a knob's focus ring.
const KNOB_FOCUS_CORNER: f32 = 10.0;
/// Where a press grabs a knob's range arc instead of the knob: from just
/// outside its ring to a little past the arc.
pub(crate) const RANGE_RING_INNER: f32 = KNOB_RADIUS + 1.5;
pub(crate) const RANGE_RING_OUTER: f32 = RANGE_RADIUS + 3.0;
/// Radius of the live value dot on a knob's range arc.
const LIVE_RADIUS: f32 = 2.25;
/// Radius of the modulation dot, and of the ring shown when every
/// assignment is off.
const DOT_RADIUS: f32 = 2.5;
/// Line width of that ring. It is drawn at least one pixel wide at any zoom.
const RING_WIDTH: f32 = 1.0;

/// The live value of one widget.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WidgetValue {
    /// Knob: position from 0 to 1. Toggle and button: 0 or 1. Radio and
    /// select: option index. Light: brightness from 0 to 1. Meter: RMS level from 0 to 1.
    pub position: f32,
    /// Meter: peak level from 0 to 1. Unused by other widgets.
    pub peak: f32,
    /// Meter: right-channel RMS and peak, when the source is stereo.
    pub right: Option<(f32, f32)>,
    /// The value as text, with its unit, shown while a knob is dragged or
    /// focused.
    pub text: String,
    /// What the control shows of its modulation assignments.
    pub modulation: ModulationMark,
    /// Knob: the live modulated value as a position from 0 to 1, drawn as
    /// a dot on the range arc (`docs/design/modulation.md`, Live value).
    /// `None` draws no dot.
    pub live: Option<f32>,
    /// Read-only result shape, sampled left to right in the UI thread.
    pub shape: Vec<f32>,
}

/// What a control shows of the modulation assignments on its parameter.
/// Spec: `docs/design/modulation.md`, Seeing assignments.
///
/// The application works out the mark, because it needs the parameter's
/// mapping. The canvas only draws it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ModulationMark {
    /// No assignments.
    #[default]
    None,
    /// The furthest the enabled assignments can reach, as knob positions
    /// from 0 to 1, with `low < high`. A knob draws it as an arc. Any other
    /// control draws the dot.
    Range {
        /// The lower end.
        low: f32,
        /// The upper end.
        high: f32,
    },
    /// At least one enabled assignment, but no range to draw: a toggle or
    /// radio button, or a knob whose arc has zero length. A filled dot.
    Dot,
    /// Every assignment is off. A hollow ring in the full modulation
    /// colour: it differs from the dot by shape, so it keeps the dot's
    /// contrast instead of fading.
    Ring,
}

/// Live values for every widget, by module ID and widget index.
#[derive(Debug, Clone, Default)]
pub struct PanelValues {
    values: HashMap<(u32, usize), WidgetValue>,
    generation: u64,
}

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

impl PanelValues {
    /// No values: every widget draws at 0.
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Sets a widget's value. The generation changes only if the value did,
    /// so setting the same value does not redraw.
    pub fn set(&mut self, module: u32, widget: usize, value: WidgetValue) {
        if self.values.get(&(module, widget)) != Some(&value) {
            self.values.insert((module, widget), value);
            self.generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// A widget's value, if set.
    pub fn get(&self, module: u32, widget: usize) -> Option<&WidgetValue> {
        self.values.get(&(module, widget))
    }

    /// The position of a widget's value, or 0.
    pub fn position(&self, module: u32, widget: usize) -> f32 {
        self.get(module, widget).map_or(0.0, |v| v.position)
    }

    /// The peak of a meter's value, or 0.
    pub fn peak(&self, module: u32, widget: usize) -> f32 {
        self.get(module, widget).map_or(0.0, |v| v.peak)
    }

    /// Changes whenever a value changes. Unique across all value sets.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// GPU instances for every widget in a scene.
#[derive(Debug, Default)]
pub(crate) struct ControlGeometry {
    /// The scene and value generations this was built from.
    pub key: (u64, u64),
    /// Knob rings, drawn first.
    pub arcs: Vec<ArcShape>,
    /// Knob bodies and pointers, toggles, radio buttons, and lights.
    pub shapes: Vec<Shape>,
    /// Result preview curves, drawn over the shapes with the trace
    /// pipeline.
    pub lines: Vec<Segment>,
}

impl ControlGeometry {
    pub fn build(scene: &Scene, values: &PanelValues, palette: &Palette) -> Self {
        crate::stats::count(crate::stats::Counter::ControlBuilds);
        let mut arcs = Vec::new();
        let mut shapes = Vec::new();
        let mut lines = Vec::new();
        for module in scene.modules() {
            for (index, widget) in module.panel.widgets.iter().enumerate() {
                let Some(rect) = module.widget_bounds(index) else {
                    continue;
                };
                let position = values.position(module.id, index);
                let modulation = values
                    .get(module.id, index)
                    .map_or(ModulationMark::None, |v| v.modulation);
                if let Some(dot) = modulation_dot(&widget.kind, rect, modulation, palette) {
                    shapes.push(dot);
                }
                match &widget.kind {
                    WidgetViewKind::Knob { .. } => {
                        let centre = knob_centre(rect);
                        let t = position.clamp(0.0, 1.0);
                        arcs.push(ArcShape::new(
                            centre,
                            KNOB_RADIUS,
                            2.5,
                            KNOB_START,
                            KNOB_SWEEP,
                            palette.control_track,
                        ));
                        // At least a short cap, so a knob at its minimum
                        // still reads as live (visual-system.md, Knobs).
                        arcs.push(ArcShape::new(
                            centre,
                            KNOB_RADIUS,
                            2.5,
                            KNOB_START,
                            KNOB_SWEEP * t.max(KNOB_START_CAP),
                            palette.control_value,
                        ));
                        if let ModulationMark::Range { low, high } = modulation {
                            arcs.push(ArcShape::new(
                                centre,
                                RANGE_RADIUS,
                                1.5,
                                KNOB_START + KNOB_SWEEP * low,
                                KNOB_SWEEP * (high - low),
                                palette.modulation,
                            ));
                        }
                        if let Some(live) = values.get(module.id, index).and_then(|v| v.live) {
                            shapes.push(live_dot(centre, live, palette));
                        }
                        shapes.push(circle(centre, 6.5, palette.control_body, palette.tile_border, 1.0));
                        let angle = KNOB_START + KNOB_SWEEP * t;
                        let tip = Point::new(centre.x + 4.0 * angle.sin(), centre.y - 4.0 * angle.cos());
                        shapes.push(circle(tip, 1.5, palette.text, Color::TRANSPARENT, 0.0));
                    }
                    WidgetViewKind::Toggle => {
                        let on = position > 0.5;
                        let track = toggle_track(rect);
                        let fill = if on {
                            palette.control_value
                        } else {
                            palette.control_track
                        };
                        shapes.push(Shape::new(track, 5.5, fill, Color::TRANSPARENT, 0.0));
                        let x = if on { track.x + track.width - 5.5 } else { track.x + 5.5 };
                        shapes.push(circle(
                            Point::new(x, track.y + 5.5),
                            4.0,
                            palette.text,
                            Color::TRANSPARENT,
                            0.0,
                        ));
                    }
                    WidgetViewKind::Radio { options } => {
                        let chosen = position.round() as usize;
                        for i in 0..options.len() {
                            let centre = radio_centre(rect, i);
                            let fill = if i == chosen {
                                palette.control_value
                            } else {
                                palette.tile
                            };
                            shapes.push(circle(centre, 3.5, fill, palette.control_track, 1.2));
                        }
                    }
                    WidgetViewKind::Segmented { options, .. } => {
                        let chosen = position.round() as usize;
                        for i in 0..options.len() {
                            let fill = if i == chosen {
                                palette.control_value
                            } else {
                                palette.control_body
                            };
                            shapes.push(Shape::new(
                                segment_rect(rect, i, options.len()),
                                3.0,
                                fill,
                                palette.control_track,
                                1.0,
                            ));
                        }
                    }
                    WidgetViewKind::Select { .. } => {
                        let box_rect = select_box(rect);
                        shapes.push(Shape::new(
                            box_rect,
                            4.0,
                            palette.control_body,
                            palette.control_track,
                            1.0,
                        ));
                        let arrow_x = box_rect.x + box_rect.width - 9.0;
                        for (row, width) in [(0.0, 7.0), (1.4, 5.0), (2.8, 3.0)] {
                            shapes.push(Shape::new(
                                Rectangle::new(
                                    Point::new(arrow_x + (7.0 - width) * 0.5, box_rect.center_y() - 2.0 + row),
                                    Size::new(width, 1.4),
                                ),
                                0.5,
                                palette.text_dim,
                                Color::TRANSPARENT,
                                0.0,
                            ));
                        }
                    }
                    WidgetViewKind::Light => {
                        let b = position.clamp(0.0, 1.0);
                        let fill = mix(palette.light_off, palette.light_on, b);
                        shapes.push(circle(light_centre(rect), 5.0, fill, palette.tile_border, 1.0));
                    }
                    WidgetViewKind::Button => {
                        let on = position > 0.5;
                        let fill = if on {
                            palette.control_value
                        } else {
                            palette.control_track
                        };
                        shapes.push(Shape::new(button_body(rect), 6.0, fill, Color::TRANSPARENT, 0.0));
                    }
                    WidgetViewKind::Scope => {
                        // The frame and its centre line. The trace has its
                        // own buffer (`trace.rs`), so a new trace never
                        // rebuilds these.
                        let area = crate::trace::trace_area(rect);
                        shapes.push(Shape::new(area, 3.0, palette.background, palette.tile_border, 1.0));
                        // The zero line: thick enough to see at 100% zoom, so
                        // a signal's offset can be judged.
                        let centre =
                            Rectangle::new(Point::new(area.x, area.center_y() - 0.5), Size::new(area.width, 1.0));
                        shapes.push(Shape::new(centre, 0.0, palette.tile_border, Color::TRANSPARENT, 0.0));
                    }
                    WidgetViewKind::Meter => {
                        let track = meter_track(rect);
                        let right = values.get(module.id, index).and_then(|value| value.right);
                        let left_track = if right.is_some() {
                            Rectangle::new(Point::new(track.x - 2.0, track.y), Size::new(5.0, track.height))
                        } else {
                            track
                        };
                        let channels = std::iter::once((left_track, position, values.peak(module.id, index))).chain(
                            right.map(|(rms, peak)| {
                                (
                                    Rectangle::new(Point::new(track.x + 5.0, track.y), Size::new(5.0, track.height)),
                                    rms,
                                    peak,
                                )
                            }),
                        );
                        for (track, rms, peak) in channels {
                            shapes.push(Shape::new(track, 2.0, palette.light_off, palette.tile_border, 1.0));
                            let rms = rms.clamp(0.0, 1.0);
                            if rms > 0.0 {
                                let height = track.height * rms;
                                let bar = Rectangle::new(
                                    Point::new(track.x, track.y + track.height - height),
                                    Size::new(track.width, height),
                                );
                                shapes.push(Shape::new(
                                    bar,
                                    2.0,
                                    meter_colour(rms, palette),
                                    Color::TRANSPARENT,
                                    0.0,
                                ));
                            }
                            // The peak never shows below the RMS bar.
                            let peak = peak.clamp(rms, 1.0);
                            if peak > 0.0 {
                                let y = track.y + track.height * (1.0 - peak);
                                let line = Rectangle::new(Point::new(track.x, y - 0.75), Size::new(track.width, 1.5));
                                shapes.push(Shape::new(line, 0.0, palette.text, Color::TRANSPARENT, 0.0));
                            }
                        }
                    }
                    WidgetViewKind::Shape { .. } => {
                        // The category tint marks this as a result display;
                        // orange remains the shared editable-value colour.
                        let line_colour = mix(module.accent, palette.text, 0.55);
                        let plot = Rectangle::new(
                            Point::new(rect.x + 4.0, rect.y + 3.0),
                            Size::new(rect.width - 8.0, rect.height - GRID - 7.0),
                        );
                        shapes.push(Shape::new(plot, 3.0, palette.tile, palette.tile_border, 1.0));
                        let samples = values.get(module.id, index).map_or(&[][..], |v| v.shape.as_slice());
                        lines.extend(preview_line(plot, samples, line_colour));
                    }
                }
            }
        }
        Self {
            key: (scene.generation(), values.generation()),
            arcs,
            shapes,
            lines,
        }
    }
}

/// A result preview's curve: one joined line through the samples, from 0
/// at the bottom of `plot` to 1 at the top, inside a 2-unit margin. Each
/// piece is an anti-aliased capsule, drawn like a scope trace, so the line
/// stays smooth at every zoom (#455).
fn preview_line(plot: Rectangle, samples: &[f32], colour: Color) -> impl Iterator<Item = Segment> + '_ {
    let last = samples.len().saturating_sub(1).max(1) as f32;
    let at = move |i: usize, y: f32| {
        Point::new(
            plot.x + 2.0 + (plot.width - 4.0) * i as f32 / last,
            plot.y + 2.0 + (plot.height - 4.0) * (1.0 - y.clamp(0.0, 1.0)),
        )
    };
    samples
        .windows(2)
        .enumerate()
        .map(move |(i, pair)| Segment::with_width(at(i, pair[0]), at(i + 1, pair[1]), colour, plot, PREVIEW_WIDTH))
}

/// The live value dot: on the range arc, at the knob angle of `position`.
/// A light outline keeps it apart from the arc under it.
fn live_dot(centre: Point, position: f32, palette: &Palette) -> Shape {
    circle(
        live_dot_centre(centre, position),
        LIVE_RADIUS,
        palette.modulation,
        palette.text,
        0.8,
    )
}

/// Where the live value dot of a knob centred at `centre` goes.
fn live_dot_centre(centre: Point, position: f32) -> Point {
    let angle = KNOB_START + KNOB_SWEEP * position.clamp(0.0, 1.0);
    Point::new(
        centre.x + RANGE_RADIUS * angle.sin(),
        centre.y - RANGE_RADIUS * angle.cos(),
    )
}

/// The modulation dot of a control, if it shows one. A knob shows it only
/// when it has no range arc to draw.
fn modulation_dot(kind: &WidgetViewKind, rect: Rectangle, mark: ModulationMark, palette: &Palette) -> Option<Shape> {
    let centre = modulation_dot_centre(kind, rect);
    match (kind, mark) {
        (_, ModulationMark::None) | (WidgetViewKind::Knob { .. }, ModulationMark::Range { .. }) => None,
        // Displays have no parameter, so no assignments.
        (WidgetViewKind::Light | WidgetViewKind::Meter | WidgetViewKind::Scope | WidgetViewKind::Shape { .. }, _) => {
            None
        }
        (_, ModulationMark::Ring) => Some(circle(
            centre,
            DOT_RADIUS,
            Color::TRANSPARENT,
            palette.modulation,
            RING_WIDTH,
        )),
        (_, ModulationMark::Range { .. } | ModulationMark::Dot) => {
            Some(circle(centre, DOT_RADIUS, palette.modulation, Color::TRANSPARENT, 0.0))
        }
    }
}

/// Where a control's modulation dot goes: next to the control, clear of its
/// value and caption.
pub(crate) fn modulation_dot_centre(kind: &WidgetViewKind, rect: Rectangle) -> Point {
    match kind {
        // In the gap at the bottom of the ring, clear of the knob body.
        WidgetViewKind::Knob { .. } => {
            let c = knob_centre(rect);
            Point::new(c.x, c.y + KNOB_RADIUS + 0.5)
        }
        // Right of the track.
        WidgetViewKind::Toggle => {
            let track = toggle_track(rect);
            Point::new(track.x + track.width + 3.5, track.y + track.height * 0.5)
        }
        // The top right corner of the cell, beside the caption or the face.
        _ => Point::new(rect.x + rect.width - 2.5, rect.y + 2.5),
    }
}

/// Centre of a knob's dial: in the top two grid rows of its 3 × 3 cell.
pub(crate) fn knob_centre(rect: Rectangle) -> Point {
    Point::new(rect.x + rect.width * 0.5, rect.y + GRID - 1.0)
}

/// What a knob always draws: its ring, range arc, and live dot. Text beside
/// a knob may use the rest of the knob's cell (tile_text.rs). The focus
/// ring is left out: it shows only while the knob has keyboard focus, and
/// it stays above the caption anyway.
pub(crate) fn knob_drawn(rect: Rectangle) -> Rectangle {
    let centre = knob_centre(rect);
    let reach = RANGE_RADIUS + LIVE_RADIUS;
    Rectangle::new(
        Point::new(centre.x - reach, centre.y - reach),
        Size::new(2.0 * reach, 2.0 * reach),
    )
}

/// Centre of a knob's dial and the radius halfway across its range ring:
/// where a press on a modulated knob drags depth, not the value
/// (modulation.md, Arc drag). Both in world units. Tests use it to press
/// on the ring without counting pixels.
pub fn knob_range_ring(rect: Rectangle) -> (Point, f32) {
    (knob_centre(rect), (RANGE_RING_INNER + RANGE_RING_OUTER) * 0.5)
}

/// Centre of a light: in the top row of its 3 × 2 cell.
pub(crate) fn light_centre(rect: Rectangle) -> Point {
    Point::new(rect.x + rect.width * 0.5, rect.y + GRID * 0.5 + 1.0)
}

/// The bar of a meter: a narrow column in its 3 × 6 cell, above the label
/// row.
pub(crate) fn meter_track(rect: Rectangle) -> Rectangle {
    Rectangle::new(
        Point::new(rect.x + rect.width * 0.5 - 4.0, rect.y + 1.0),
        Size::new(8.0, rect.height - GRID - 2.0),
    )
}

/// Meter level, from 0 to 1, for a decibel value (-60 dB to 0 dB).
fn level_of_db(db: f32) -> f32 {
    (db + 60.0) / 60.0
}

/// Green below -12 dB, amber up to -3 dB, red above (panels.md, Meter
/// widget).
pub(crate) fn meter_colour(level: f32, palette: &Palette) -> Color {
    if level < level_of_db(-12.0) {
        palette.light_on
    } else if level < level_of_db(-3.0) {
        palette.control_value
    } else {
        palette.warning
    }
}

/// The keyboard focus ring of a control, and its corner radius: around the
/// control itself, never its caption, so a value shown there stays clear
/// (panels.md, Interaction).
pub(crate) fn focus_ring(kind: &WidgetViewKind, rect: Rectangle) -> (Rectangle, f32) {
    match kind {
        // Outside the modulation range arc, so focus never hides it. A
        // circle that wide would cut into the caption, so the ring is a
        // rounded rectangle: as far out as the arc needs at the top and
        // sides, and flatter at the bottom, where the arc has its gap.
        WidgetViewKind::Knob { .. } => {
            let c = knob_centre(rect);
            (
                Rectangle::new(
                    Point::new(c.x - KNOB_FOCUS_HALF_WIDTH, c.y - KNOB_FOCUS_TOP),
                    Size::new(2.0 * KNOB_FOCUS_HALF_WIDTH, KNOB_FOCUS_TOP + KNOB_FOCUS_BOTTOM),
                ),
                KNOB_FOCUS_CORNER,
            )
        }
        WidgetViewKind::Toggle => (toggle_track(rect).expand(2.5), 8.0),
        WidgetViewKind::Select { .. } => (select_box(rect).expand(2.0), 5.0),
        // The segments, below the caption row.
        WidgetViewKind::Segmented { options, .. } => {
            let first = segment_rect(rect, 0, options.len());
            let last = segment_rect(rect, options.len().saturating_sub(1), options.len());
            (first.union(&last).expand(2.0), 5.0)
        }
        WidgetViewKind::Button => (button_body(rect).expand(2.5), 8.0),
        // The options, below the caption row.
        WidgetViewKind::Radio { .. } => (
            Rectangle::new(
                Point::new(rect.x, rect.y + GRID),
                Size::new(rect.width, rect.height - GRID),
            )
            .expand(1.0),
            4.0,
        ),
        // Displays never take focus.
        WidgetViewKind::Light | WidgetViewKind::Meter | WidgetViewKind::Scope | WidgetViewKind::Shape { .. } => {
            (rect.expand(1.0), 4.0)
        }
    }
}

/// The closed select box, leaving a caption row above it.
pub(crate) fn select_box(rect: Rectangle) -> Rectangle {
    Rectangle::new(
        Point::new(rect.x + 2.0, rect.y + GRID + 1.0),
        Size::new(rect.width - 4.0, GRID - 3.0),
    )
}

/// The face of a button: its whole cell, with its caption on it
/// (panels.md, Panel layout, principle 10).
pub(crate) fn button_body(rect: Rectangle) -> Rectangle {
    Rectangle::new(
        Point::new(rect.x + 4.0, rect.y + 4.0),
        Size::new(rect.width - 8.0, rect.height - 8.0),
    )
}

/// The track of a toggle switch: in the top row of its 3 × 2 cell.
fn toggle_track(rect: Rectangle) -> Rectangle {
    Rectangle::new(Point::new(rect.x + 7.0, rect.y + 1.0), Size::new(22.0, 11.0))
}

/// Centre of radio option `i`: one row each, below the label row.
pub(crate) fn radio_centre(rect: Rectangle, i: usize) -> Point {
    Point::new(rect.x + 6.0, rect.y + GRID * (i as f32 + 1.5))
}

/// The radio option under a world point, if any.
/// Segment `index` of a segmented control with `count` segments: equal
/// shares of the bottom row, with a 1-unit gap between them.
pub(crate) fn segment_rect(rect: Rectangle, index: usize, count: usize) -> Rectangle {
    let count = count.max(1) as f32;
    let width = (rect.width - 4.0) / count;
    Rectangle::new(
        Point::new(rect.x + 2.0 + width * index as f32, rect.y + GRID),
        Size::new(width - 1.0, GRID - 1.0),
    )
}

/// The centre of segment `index`, for tests and scripts that click it.
pub fn segment_centre(rect: Rectangle, index: usize, count: usize) -> Point {
    segment_rect(rect, index, count).center()
}

/// The segment of a segmented control under a world point, if any.
pub(crate) fn segment_at(rect: Rectangle, count: usize, point: Point) -> Option<usize> {
    (0..count).find(|&i| segment_rect(rect, i, count).expand(0.5).contains(point))
}

pub(crate) fn radio_option(rect: Rectangle, options: usize, point: Point) -> Option<usize> {
    let row = ((point.y - rect.y) / GRID).floor() as isize - 1;
    (row >= 0 && (row as usize) < options).then_some(row as usize)
}

fn circle(centre: Point, radius: f32, fill: Color, border: Color, border_width: f32) -> Shape {
    let rect = Rectangle::new(
        Point::new(centre.x - radius, centre.y - radius),
        Size::new(radius * 2.0, radius * 2.0),
    );
    Shape::new(rect, radius, fill, border, border_width)
}

pub(crate) fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

/// A knob's new position after the pointer moved `dy` screen pixels (up is
/// negative). The full range is 200 pixels at any zoom; Shift is 10 times
/// finer.
pub(crate) fn knob_drag(position: f32, dy: f32, fine: bool) -> f32 {
    let scale = if fine { 0.1 } else { 1.0 };
    (position - dy / 200.0 * scale).clamp(0.0, 1.0)
}

/// A knob's new position after one arrow key press: 1% of the range, 0.1%
/// with Shift, or one step for a whole-number parameter.
pub(crate) fn knob_step(position: f32, up: bool, fine: bool, steps: u32) -> f32 {
    let step = if steps > 0 {
        1.0 / steps as f32
    } else if fine {
        0.001
    } else {
        0.01
    };
    let next = if up { position + step } else { position - step };
    next.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{ModuleView, PanelView, WidgetView};
    use iced::advanced::graphics::color::pack;

    fn scene() -> Scene {
        let widget = |kind, x| WidgetView {
            kind,
            x,
            y: 0,
            label: String::new(),
            hover: crate::scene::HoverText::default(),
            hidden: false,
        };
        Scene::new(
            vec![ModuleView {
                size: None,
                unknown: false,
                id: 1,
                title: String::new(),
                hover: crate::scene::HoverText::default(),
                accent: Color::BLACK,
                position: Point::ORIGIN,
                inputs: Vec::new(),
                outputs: Vec::new(),
                panel: PanelView {
                    width: 9,
                    height: 4,
                    widgets: vec![
                        widget(WidgetViewKind::Knob { steps: 0 }, 0),
                        widget(WidgetViewKind::Toggle, 3),
                        widget(
                            WidgetViewKind::Radio {
                                options: vec!["a".into(), "b".into(), "c".into()],
                            },
                            6,
                        ),
                    ],
                },
            }],
            Vec::new(),
        )
    }

    #[test]
    fn values_change_only_the_control_geometry() {
        let scene = scene();
        let palette = Palette::default();
        let mut values = PanelValues::new();
        let before = ControlGeometry::build(&scene, &values, &palette);
        let scene_generation = scene.generation();

        values.set(
            1,
            0,
            WidgetValue {
                position: 0.5,
                text: "0.5".into(),
                ..WidgetValue::default()
            },
        );
        let after = ControlGeometry::build(&scene, &values, &palette);
        // The scene, and so the tile, port, and cable buffers, did not change.
        assert_eq!(scene.generation(), scene_generation);
        assert_ne!(after.key, before.key);
        // The knob's value arc grew from its start cap to half its sweep.
        assert_eq!(after.arcs.len(), before.arcs.len());
        assert!((before.arcs[1].sweep() - KNOB_SWEEP * KNOB_START_CAP).abs() < 1e-6);
        assert!((after.arcs[1].sweep() - KNOB_SWEEP * 0.5).abs() < 1e-6);
    }

    /// A knob at its minimum still draws an orange start cap, so it reads as
    /// live; at its maximum it draws the whole arc (visual-system.md,
    /// Knobs).
    #[test]
    fn a_knob_at_its_minimum_shows_a_start_cap() {
        let scene = scene();
        let palette = Palette::default();
        let value_arc = |position: f32| {
            let mut values = PanelValues::new();
            values.set(
                1,
                0,
                WidgetValue {
                    position,
                    ..WidgetValue::default()
                },
            );
            ControlGeometry::build(&scene, &values, &palette).arcs[1]
        };
        let low = value_arc(0.0);
        assert_eq!(
            low.colour(),
            pack(palette.control_value).components(),
            "in the value colour"
        );
        assert!(low.sweep() > 0.0, "a start cap at 0");
        assert!((value_arc(1.0).sweep() - KNOB_SWEEP).abs() < 1e-6, "the whole arc at 1");
    }

    fn modulated(position: f32, modulation: ModulationMark) -> WidgetValue {
        WidgetValue {
            position,
            modulation,
            ..WidgetValue::default()
        }
    }

    /// The modulation mark on widget `index`, if it is drawn.
    fn dot(geometry: &ControlGeometry, scene: &Scene, index: usize) -> Option<Shape> {
        let module = &scene.modules()[0];
        let kind = &module.panel.widgets[index].kind;
        let c = modulation_dot_centre(kind, module.widget_bounds(index).unwrap());
        let want = [c.x - DOT_RADIUS, c.y - DOT_RADIUS, 2.0 * DOT_RADIUS, 2.0 * DOT_RADIUS];
        geometry.shapes.iter().find(|s| s.rect() == want).copied()
    }

    #[test]
    fn a_knob_range_draws_one_arc_and_no_dot() {
        let scene = scene();
        let palette = Palette::default();
        let mut values = PanelValues::new();
        values.set(1, 0, modulated(0.5, ModulationMark::None));
        let plain = ControlGeometry::build(&scene, &values, &palette);
        values.set(1, 0, modulated(0.5, ModulationMark::Range { low: 0.25, high: 0.75 }));
        let ranged = ControlGeometry::build(&scene, &values, &palette);
        assert_eq!(ranged.arcs.len(), plain.arcs.len() + 1);
        let arc = ranged.arcs.last().unwrap();
        let expected = ArcShape::new(
            knob_centre(scene.modules()[0].widget_bounds(0).unwrap()),
            RANGE_RADIUS,
            1.5,
            KNOB_START + KNOB_SWEEP * 0.25,
            KNOB_SWEEP * 0.5,
            palette.modulation,
        );
        assert_eq!(bytemuck::bytes_of(arc), bytemuck::bytes_of(&expected));
        assert!(dot(&ranged, &scene, 0).is_none());
    }

    #[test]
    fn dots_show_on_knobs_without_a_range_and_on_toggles_and_radios() {
        let colour = pack(Palette::default().modulation).components();
        let clear = pack(Color::TRANSPARENT).components();
        let scene = scene();
        let palette = Palette::default();
        let mut values = PanelValues::new();
        values.set(1, 0, modulated(0.5, ModulationMark::Dot));
        values.set(1, 1, modulated(0.0, ModulationMark::Dot));
        values.set(1, 2, modulated(1.0, ModulationMark::Dot));
        let lit = ControlGeometry::build(&scene, &values, &palette);
        for index in 0..3 {
            let mark = dot(&lit, &scene, index).expect("a dot");
            assert_eq!((mark.fill(), mark.border_width()), (colour, 0.0), "widget {index}");
        }
        // No range arc on the knob: only its track and value arc.
        assert_eq!(lit.arcs.len(), 2);

        for index in 0..3 {
            values.set(1, index, modulated(0.0, ModulationMark::Ring));
        }
        // Every assignment off: a hollow ring in the full colour, the same
        // size, so it keeps its contrast and differs by shape.
        let off = ControlGeometry::build(&scene, &values, &palette);
        for index in 0..3 {
            let ring = dot(&off, &scene, index).expect("a ring");
            assert_eq!(
                (ring.fill(), ring.border(), ring.border_width()),
                (clear, colour, RING_WIDTH),
                "widget {index}"
            );
        }
        // Nothing modulated, no dots.
        let plain = ControlGeometry::build(&scene, &PanelValues::new(), &palette);
        assert!((0..3).all(|index| dot(&plain, &scene, index).is_none()));
    }

    #[test]
    fn a_knob_focus_ring_clears_the_range_arc_and_the_caption() {
        let rect = Rectangle::new(Point::ORIGIN, Size::new(3.0 * GRID, 3.0 * GRID));
        let c = knob_centre(rect);
        let (ring, corner) = focus_ring(&WidgetViewKind::Knob { steps: 0 }, rect);
        // Signed distance from a point to the ring's outline: negative inside.
        let outline = |p: Point| {
            let qx = (p.x - ring.center_x()).abs() - (ring.width * 0.5 - corner);
            let qy = (p.y - ring.center_y()).abs() - (ring.height * 0.5 - corner);
            qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - corner
        };
        // The keyboard focus border (1.5) and the MIDI learn border (2.5)
        // are drawn inside the outline. Neither may reach the arc.
        let arc_outer = RANGE_RADIUS + 0.75;
        for step in 0..=270 {
            let angle = KNOB_START + KNOB_SWEEP * step as f32 / 270.0;
            let p = Point::new(c.x + arc_outer * angle.sin(), c.y - arc_outer * angle.cos());
            let gap = -outline(p);
            assert!(gap >= 2.5 - 1e-4, "learn border reaches the arc at {angle}: gap {gap}");
        }
        // `a_control_focused_by_keyboard_has_a_ring_clear_of_its_caption`
        // checks the caption.
        // Inside the cell's width.
        assert!(ring.x >= rect.x && ring.x + ring.width <= rect.x + rect.width);
    }

    #[test]
    fn modulation_changes_only_the_control_geometry() {
        // Depth, polarity, and on/off reach the canvas as a new mark. Like a
        // value, it rebuilds only the controls (ADR-0010).
        let scene = scene();
        let palette = Palette::default();
        let scene_generation = scene.generation();
        let mut values = PanelValues::new();
        values.set(1, 0, modulated(0.5, ModulationMark::Range { low: 0.5, high: 0.75 }));
        let before = ControlGeometry::build(&scene, &values, &palette);
        values.set(1, 0, modulated(0.5, ModulationMark::Ring));
        let after = ControlGeometry::build(&scene, &values, &palette);
        assert_eq!(scene.generation(), scene_generation);
        assert_eq!(after.key.0, before.key.0);
        assert_ne!(after.key.1, before.key.1);
    }

    /// A knob with a live value gets one dot on its range arc, at the knob
    /// angle of that value. Only the controls rebuild when it moves.
    #[test]
    fn the_live_value_is_a_dot_on_the_range_arc() {
        let scene = scene();
        let palette = Palette::default();
        let mut values = PanelValues::new();
        let range = ModulationMark::Range { low: 0.25, high: 0.75 };
        values.set(1, 0, modulated(0.5, range));
        let without = ControlGeometry::build(&scene, &values, &palette);
        values.set(
            1,
            0,
            WidgetValue {
                live: Some(1.0),
                ..modulated(0.5, range)
            },
        );
        let with = ControlGeometry::build(&scene, &values, &palette);
        assert_eq!(with.shapes.len(), without.shapes.len() + 1);
        assert_eq!(with.key.0, without.key.0, "the scene is not rebuilt");
        // At 0 and 1 the dot sits at the ends of the sweep, down left and
        // down right, on the range radius; at 0.5 straight up.
        let c = Point::ORIGIN;
        let (low, mid, high) = (
            live_dot_centre(c, 0.0),
            live_dot_centre(c, 0.5),
            live_dot_centre(c, 1.0),
        );
        assert!(low.x < 0.0 && low.y > 0.0);
        assert!(high.x > 0.0 && high.y > 0.0);
        assert!(mid.x.abs() < 1e-4 && (mid.y + RANGE_RADIUS).abs() < 1e-4);
    }

    #[test]
    fn setting_the_same_value_keeps_the_generation() {
        let mut values = PanelValues::new();
        let value = WidgetValue {
            position: 0.25,
            ..WidgetValue::default()
        };
        values.set(1, 0, value.clone());
        let generation = values.generation();
        values.set(1, 0, value);
        assert_eq!(values.generation(), generation);
    }

    #[test]
    fn knob_drag_is_200_pixels_and_fine_with_shift() {
        // Up is negative screen y.
        assert_eq!(knob_drag(0.0, -100.0, false), 0.5);
        assert!((knob_drag(0.0, -100.0, true) - 0.05).abs() < 1e-6);
        assert_eq!(knob_drag(0.9, -100.0, false), 1.0);
        assert_eq!(knob_drag(0.1, 100.0, false), 0.0);
    }

    #[test]
    fn knob_steps() {
        assert!((knob_step(0.5, true, false, 0) - 0.51).abs() < 1e-6);
        assert!((knob_step(0.5, false, true, 0) - 0.499).abs() < 1e-6);
        // A whole-number parameter from 0 to 16 steps by 1/16.
        assert_eq!(knob_step(0.5, true, false, 16), 0.5625);
        assert_eq!(knob_step(1.0, true, false, 0), 1.0);
    }

    #[test]
    fn meter_colours_follow_the_decibel_zones() {
        let palette = Palette::default();
        assert_eq!(meter_colour(level_of_db(-30.0), &palette), palette.light_on);
        assert_eq!(meter_colour(level_of_db(-6.0), &palette), palette.control_value);
        assert_eq!(meter_colour(level_of_db(-1.0), &palette), palette.warning);
    }

    #[test]
    fn the_peak_line_is_never_below_the_bar() {
        let meter = Scene::new(
            vec![ModuleView {
                size: None,
                unknown: false,
                id: 1,
                title: String::new(),
                hover: crate::scene::HoverText::default(),
                accent: Color::BLACK,
                position: Point::ORIGIN,
                inputs: Vec::new(),
                outputs: Vec::new(),
                panel: PanelView {
                    width: 3,
                    height: 6,
                    widgets: vec![WidgetView {
                        kind: WidgetViewKind::Meter,
                        x: 0,
                        y: 0,
                        label: String::new(),
                        hover: crate::scene::HoverText::default(),
                        hidden: false,
                    }],
                },
            }],
            Vec::new(),
        );
        let mut values = PanelValues::new();
        // A stale peak below the RMS level.
        values.set(
            1,
            0,
            WidgetValue {
                position: 0.8,
                peak: 0.2,
                ..WidgetValue::default()
            },
        );
        let geometry = ControlGeometry::build(&meter, &values, &Palette::default());
        // Track, bar, peak line.
        assert_eq!(geometry.shapes.len(), 3);
        let bar = geometry.shapes[1].rect();
        let line = geometry.shapes[2].rect();
        assert!(
            line[1] + line[3] * 0.5 <= bar[1] + 0.01,
            "peak line {line:?} below bar {bar:?}"
        );
    }

    /// Stereo meters draw independent bars, including a silent right side.
    #[test]
    fn stereo_meter_draws_two_tracks() {
        let meter = Scene::new(
            vec![ModuleView {
                id: 1,
                title: String::new(),
                hover: crate::scene::HoverText::default(),
                accent: Color::BLACK,
                position: Point::ORIGIN,
                size: None,
                unknown: false,
                inputs: Vec::new(),
                outputs: Vec::new(),
                panel: PanelView {
                    width: 3,
                    height: 6,
                    widgets: vec![WidgetView {
                        kind: WidgetViewKind::Meter,
                        x: 0,
                        y: 0,
                        label: String::new(),
                        hover: crate::scene::HoverText::default(),
                        hidden: false,
                    }],
                },
            }],
            Vec::new(),
        );
        let mut values = PanelValues::new();
        values.set(
            1,
            0,
            WidgetValue {
                position: 0.8,
                peak: 0.9,
                right: Some((0.0, 0.0)),
                ..WidgetValue::default()
            },
        );
        let shapes = ControlGeometry::build(&meter, &values, &Palette::default()).shapes;
        assert_eq!(shapes.len(), 4, "two tracks and the left bar and peak");
        assert!(shapes[0].rect()[0] < shapes[3].rect()[0]);
    }

    #[test]
    fn radio_rows() {
        let rect = Rectangle::new(Point::ORIGIN, Size::new(36.0, 48.0));
        assert_eq!(radio_option(rect, 3, Point::new(5.0, 5.0)), None);
        assert_eq!(radio_option(rect, 3, Point::new(5.0, GRID + 1.0)), Some(0));
        assert_eq!(radio_option(rect, 3, Point::new(5.0, 3.0 * GRID + 1.0)), Some(2));
        assert_eq!(radio_option(rect, 3, Point::new(5.0, 4.0 * GRID + 1.0)), None);
    }

    /// A result preview is one joined line of equal-width pieces inside its
    /// frame, not a box per sample, so it stays smooth when zoomed and a
    /// steep step is a line, not a stair (#455).
    #[test]
    fn a_result_preview_is_one_joined_line() {
        let mut module = scene().modules()[0].clone();
        module.panel = PanelView {
            width: 9,
            height: 6,
            widgets: vec![WidgetView {
                kind: WidgetViewKind::Shape { width: 9 },
                x: 0,
                y: 0,
                label: "Shape".into(),
                hover: crate::scene::HoverText::default(),
                hidden: false,
            }],
        };
        let scene = Scene::new(vec![module], Vec::new());
        let mut values = PanelValues::new();
        let samples = vec![0.0, 0.0, 1.0, 1.0, 0.5];
        values.set(
            1,
            0,
            WidgetValue {
                shape: samples.clone(),
                ..WidgetValue::default()
            },
        );
        let geometry = ControlGeometry::build(&scene, &values, &Palette::default());

        // The frame is the only shape; the curve is all lines.
        assert_eq!(geometry.shapes.len(), 1);
        assert_eq!(geometry.lines.len(), samples.len() - 1);
        for pair in geometry.lines.windows(2) {
            assert_eq!(pair[0].ends().1, pair[1].ends().0, "the pieces join");
        }
        assert!(geometry.lines.iter().all(|line| line.width() == PREVIEW_WIDTH));
        // The step from 0 to 1 is one straight piece from the bottom of the
        // plot to its top.
        let (from, to) = geometry.lines[1].ends();
        assert!(from.y > to.y + 10.0, "the step rises");
        assert!((to.x - from.x) > 0.0, "the step moves right as it rises");
    }
}
