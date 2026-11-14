//! Panel controls on tiles: their live values and their GPU instances
//! (ADR-0013). Spec: `docs/design/panels.md`, Module panels on tiles.
//!
//! Values live apart from the [`Scene`]. Changing one rebuilds only the
//! controls' instances, never the tiles, ports, or cables (ADR-0010).

use crate::gpu::{Arc as ArcShape, Shape};
use crate::input::ControlPart;
use crate::scene::{GRID, Scene, WidgetViewKind};
use crate::theme::Palette;
use crate::trace::Segment;
use loom_ui_core::{Color, Point, Rect, Size};
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

/// The fewest screen pixels for a number control's full range.
const MIN_THROW: f32 = 200.0;
/// The fewest screen pixels per whole value on a number.
const MIN_PIXELS_PER_STEP: f32 = 4.0;
/// How far an XY pad's modulation bars sit outside its square.
const XY_BAR_AWAY: f32 = 3.5;
/// The width of those bars.
const XY_BAR_WIDTH: f32 = 2.0;
/// Radius of an XY pad's point.
const XY_POINT_RADIUS: f32 = 4.0;
/// A horizontal range's track centre, below the top of its second row: room
/// for a bar above and below the handles.
const RANGE_ACROSS: f32 = 5.0;
/// How far a horizontal range's bars sit from its track.
const RANGE_BAR_AWAY: f32 = 5.0;
/// How far from a handle's centre, along the track, a press takes it.
const RANGE_REACH: f32 = 4.5;
/// A horizontal fader's track centre, below the top of its second row.
const FADER_ACROSS: f32 = 3.0;
/// How far a fader's modulation bar sits from the centre of its track: clear
/// of the handle.
const FADER_BAR_AWAY: f32 = 8.0;

/// The one number a widget's text says, with the range its drawing spans.
/// It is made where the text is made, so the two cannot disagree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    /// The number, in the unit the text names.
    pub value: f32,
    /// The value at the bottom of the drawing.
    pub min: f32,
    /// The value at the top of the drawing.
    pub max: f32,
}

/// The live value of one widget.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WidgetValue {
    /// Knob and fader: position from 0 to 1. Toggle and button: 0 or 1. Radio and
    /// select: option index. Light: brightness from 0 to 1. Meter: RMS level from 0 to 1.
    pub position: f32,
    /// Meter: peak level from 0 to 1. Unused by other widgets.
    pub peak: f32,
    /// Meter: right-channel RMS and peak, when the source is stereo.
    pub right: Option<(f32, f32)>,
    /// Meter on an output: true while its clip light is on.
    pub clip: bool,
    /// Meter on an output: the peak hold readout.
    pub hold: String,
    /// The value as text, with its unit, shown while a knob is dragged or
    /// focused.
    pub text: String,
    /// Meter: the number `text` says, when it says exactly one. `None` for
    /// other widgets. The canvas draws nothing from it: it is for what
    /// reports the value as a number, such as a screen reader.
    pub reading: Option<Reading>,
    /// What the control shows of its modulation assignments.
    pub modulation: ModulationMark,
    /// Knob: the live modulated value as a position from 0 to 1, drawn as
    /// a dot on the range arc (`docs/design/modulation.md`, Live value).
    /// `None` draws no dot.
    pub live: Option<f32>,
    /// Knob and fader: where the hardware of a waiting pickup stands, as a
    /// position from 0 to 1, drawn as a hollow mark (`docs/design/midi-learn.md`,
    /// Mapping modes). `None` draws none.
    pub ghost: Option<f32>,
    /// Read-only result shape, sampled left to right in the UI thread.
    pub shape: Vec<f32>,
    /// True for a control with nothing behind it, such as a surface
    /// control bound to an empty slot's role parameter: it is drawn under
    /// a veil of the tile's colour, and its hover text says why
    /// (sub-patches.md, Empty slots).
    pub dimmed: bool,
    /// The second value of an XY pad or a range. The fields above then hold
    /// the first value, and `text` holds both, as the widget says them.
    pub second: Option<PartValue>,
}

/// The second value of a widget that binds two: `y` or `high`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PartValue {
    /// Position from 0 to 1.
    pub position: f32,
    /// The value as text, with its unit.
    pub text: String,
    /// What this value shows of its own modulation assignments.
    pub modulation: ModulationMark,
    /// The live modulated value as a position from 0 to 1.
    pub live: Option<f32>,
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

    /// The position of one value of a widget, or 0.
    pub fn part_position(&self, module: u32, widget: usize, part: ControlPart) -> f32 {
        match part {
            ControlPart::Second => self
                .get(module, widget)
                .and_then(|v| v.second.as_ref())
                .map_or(0.0, |second| second.position),
            ControlPart::Main | ControlPart::Both => self.position(module, widget),
        }
    }

    /// The modulation mark of one value of a widget.
    pub fn part_mark(&self, module: u32, widget: usize, part: ControlPart) -> ModulationMark {
        let value = self.get(module, widget);
        match part {
            ControlPart::Second => value
                .and_then(|v| v.second.as_ref())
                .map_or(ModulationMark::None, |second| second.modulation),
            ControlPart::Main | ControlPart::Both => value.map_or(ModulationMark::None, |v| v.modulation),
        }
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
                if widget.exposed {
                    shapes.push(exposed_mark(rect, palette));
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
                        if let Some(ghost) = values.get(module.id, index).and_then(|v| v.ghost) {
                            shapes.push(ghost_mark(live_dot_centre(centre, ghost), palette));
                        }
                        shapes.push(circle(centre, 6.5, palette.control_body, palette.tile_border, 1.0));
                        let angle = KNOB_START + KNOB_SWEEP * t;
                        let tip = Point::new(centre.x + 4.0 * angle.sin(), centre.y - 4.0 * angle.cos());
                        shapes.push(circle(tip, 1.5, palette.text, Color::TRANSPARENT, 0.0));
                    }
                    WidgetViewKind::Fader { vertical, .. } => {
                        let t = position.clamp(0.0, 1.0);
                        let track = fader_track(rect, *vertical);
                        shapes.push(Shape::new(track, 1.5, palette.control_track, Color::TRANSPARENT, 0.0));
                        let filled = fader_span(track, *vertical, 0.0, t.max(KNOB_START_CAP));
                        shapes.push(Shape::new(filled, 1.5, palette.control_value, Color::TRANSPARENT, 0.0));
                        if let ModulationMark::Range { low, high } = modulation {
                            shapes.push(Shape::new(
                                fader_bar(rect, *vertical, low, high),
                                1.0,
                                palette.modulation,
                                Color::TRANSPARENT,
                                0.0,
                            ));
                        }
                        if let Some(live) = values.get(module.id, index).and_then(|v| v.live) {
                            let at = fader_bar_point(rect, *vertical, live);
                            shapes.push(circle(at, LIVE_RADIUS, palette.modulation, palette.text, 0.8));
                        }
                        if let Some(ghost) = values.get(module.id, index).and_then(|v| v.ghost) {
                            shapes.push(ghost_mark(fader_bar_point(rect, *vertical, ghost), palette));
                        }
                        shapes.push(Shape::new(
                            fader_handle(rect, *vertical, t),
                            2.0,
                            palette.control_body,
                            palette.text,
                            1.0,
                        ));
                    }
                    WidgetViewKind::XyPad { .. } => {
                        let area = xy_area(rect);
                        let value = values.get(module.id, index);
                        let second = value.and_then(|v| v.second.as_ref());
                        let (x, y) = (
                            position.clamp(0.0, 1.0),
                            second.map_or(0.0, |s| s.position.clamp(0.0, 1.0)),
                        );
                        shapes.push(Shape::new(area, 3.0, palette.control_body, palette.control_track, 1.0));
                        let marks = [
                            (ControlPart::Main, modulation),
                            (
                                ControlPart::Second,
                                second.map_or(ModulationMark::None, |s| s.modulation),
                            ),
                        ];
                        for (part, mark) in marks {
                            match mark {
                                ModulationMark::Range { low, high } => shapes.push(Shape::new(
                                    xy_bar(area, part, low, high),
                                    1.0,
                                    palette.modulation,
                                    Color::TRANSPARENT,
                                    0.0,
                                )),
                                ModulationMark::Dot => shapes.push(circle(
                                    xy_edge_point(area, part, 0.5),
                                    DOT_RADIUS,
                                    palette.modulation,
                                    Color::TRANSPARENT,
                                    0.0,
                                )),
                                ModulationMark::Ring => shapes.push(circle(
                                    xy_edge_point(area, part, 0.5),
                                    DOT_RADIUS,
                                    Color::TRANSPARENT,
                                    palette.modulation,
                                    RING_WIDTH,
                                )),
                                ModulationMark::None => {}
                            }
                        }
                        let live = (value.and_then(|v| v.live), second.and_then(|s| s.live));
                        if live.0.is_some() || live.1.is_some() {
                            let at = xy_point(area, live.0.unwrap_or(x), live.1.unwrap_or(y));
                            shapes.push(circle(at, XY_POINT_RADIUS, Color::TRANSPARENT, palette.modulation, 1.2));
                        }
                        shapes.push(circle(
                            xy_point(area, x, y),
                            XY_POINT_RADIUS,
                            palette.control_value,
                            palette.text,
                            1.0,
                        ));
                    }
                    WidgetViewKind::Range { vertical, .. } => {
                        let vertical = *vertical;
                        let value = values.get(module.id, index);
                        let second = value.and_then(|v| v.second.as_ref());
                        let low = position.clamp(0.0, 1.0);
                        let high = second.map_or(0.0, |s| s.position.clamp(0.0, 1.0));
                        let track = range_track(rect, vertical);
                        shapes.push(Shape::new(track, 1.5, palette.control_track, Color::TRANSPARENT, 0.0));
                        shapes.push(Shape::new(
                            fader_span(track, vertical, low.min(high), low.max(high)),
                            1.5,
                            palette.control_value,
                            Color::TRANSPARENT,
                            0.0,
                        ));
                        let parts = [
                            (ControlPart::Main, modulation, low, value.and_then(|v| v.live)),
                            (
                                ControlPart::Second,
                                second.map_or(ModulationMark::None, |s| s.modulation),
                                high,
                                second.and_then(|s| s.live),
                            ),
                        ];
                        for (part, mark, at, live) in parts {
                            let centre = range_bar_point(rect, vertical, part, at);
                            match mark {
                                ModulationMark::Range { low, high } => shapes.push(Shape::new(
                                    range_bar(rect, vertical, part, low, high),
                                    1.0,
                                    palette.modulation,
                                    Color::TRANSPARENT,
                                    0.0,
                                )),
                                ModulationMark::Dot => {
                                    shapes.push(circle(centre, DOT_RADIUS, palette.modulation, Color::TRANSPARENT, 0.0))
                                }
                                ModulationMark::Ring => shapes.push(circle(
                                    centre,
                                    DOT_RADIUS,
                                    Color::TRANSPARENT,
                                    palette.modulation,
                                    RING_WIDTH,
                                )),
                                ModulationMark::None => {}
                            }
                            if let Some(live) = live {
                                shapes.push(Shape::new(
                                    range_live_line(rect, vertical, live),
                                    0.0,
                                    palette.modulation,
                                    Color::TRANSPARENT,
                                    0.0,
                                ));
                            }
                        }
                        for at in [low, high] {
                            shapes.push(Shape::new(
                                range_handle(rect, vertical, at),
                                2.0,
                                palette.control_body,
                                palette.text,
                                1.0,
                            ));
                        }
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
                    WidgetViewKind::Number { .. } => {
                        shapes.push(Shape::new(
                            number_box(rect),
                            4.0,
                            palette.control_body,
                            palette.control_track,
                            1.0,
                        ));
                    }
                    // A surface that reads as a display, not an editable box.
                    WidgetViewKind::Readout { .. } => {
                        shapes.push(Shape::new(
                            number_box(rect),
                            3.0,
                            palette.tile,
                            palette.tile_border,
                            1.0,
                        ));
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
                                Rect::new(
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
                        let centre = Rect::new(Point::new(area.x, area.center_y() - 0.5), Size::new(area.width, 1.0));
                        shapes.push(Shape::new(centre, 0.0, palette.tile_border, Color::TRANSPARENT, 0.0));
                    }
                    WidgetViewKind::XyScope => {
                        // The frame and a faint cross at 0. The trace has its
                        // own buffer.
                        let area = crate::trace::xy_scope_area(rect);
                        shapes.push(Shape::new(area, 3.0, palette.background, palette.tile_border, 1.0));
                        let across = Rect::new(Point::new(area.x, area.center_y() - 0.5), Size::new(area.width, 1.0));
                        let up = Rect::new(Point::new(area.center_x() - 0.5, area.y), Size::new(1.0, area.height));
                        for line in [across, up] {
                            shapes.push(Shape::new(line, 0.0, palette.tile_border, Color::TRANSPARENT, 0.0));
                        }
                    }
                    WidgetViewKind::LevelHistory | WidgetViewKind::Spectrum => {
                        // The frame. What it shows has its own buffer.
                        let area = crate::trace::trace_area(rect);
                        shapes.push(Shape::new(area, 3.0, palette.background, palette.tile_border, 1.0));
                    }
                    WidgetViewKind::Meter { zero, .. } if *zero > 0.0 => {
                        // A value that runs down from 0 draws from there.
                        let track = meter_track(rect, false);
                        shapes.push(Shape::new(track, 2.0, palette.light_off, palette.tile_border, 1.0));
                        let (low, high) = (position.clamp(0.0, 1.0).min(*zero), position.clamp(0.0, 1.0).max(*zero));
                        if high > low {
                            let bar = Rect::new(
                                Point::new(track.x, track.y + track.height * (1.0 - high)),
                                Size::new(track.width, track.height * (high - low)),
                            );
                            let colour = mix(module.accent, palette.text, 0.55);
                            shapes.push(Shape::new(bar, 2.0, colour, Color::TRANSPARENT, 0.0));
                        }
                    }
                    WidgetViewKind::Meter { clips, .. } => {
                        let track = meter_track(rect, *clips);
                        if *clips {
                            // A ring when off and a filled square when on, so
                            // shape says it as well as colour (NFR-UX-03).
                            let centre = Point::new(rect.center_x(), rect.y + CLIP_ROW * 0.5);
                            if values.get(module.id, index).is_some_and(|value| value.clip) {
                                let lit = Rect::new(Point::new(centre.x - 4.0, centre.y - 4.0), Size::new(8.0, 8.0));
                                shapes.push(Shape::new(lit, 1.0, palette.warning, Color::TRANSPARENT, 0.0));
                            } else {
                                shapes.push(circle(centre, 3.5, Color::TRANSPARENT, palette.tile_border, 1.0));
                            }
                        }
                        let right = values.get(module.id, index).and_then(|value| value.right);
                        let left_track = if right.is_some() {
                            Rect::new(Point::new(track.x - 2.0, track.y), Size::new(5.0, track.height))
                        } else {
                            track
                        };
                        let channels = std::iter::once((left_track, position, values.peak(module.id, index))).chain(
                            right.map(|(rms, peak)| {
                                (
                                    Rect::new(Point::new(track.x + 5.0, track.y), Size::new(5.0, track.height)),
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
                                let bar = Rect::new(
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
                                let line = Rect::new(Point::new(track.x, y - 0.75), Size::new(track.width, 1.5));
                                shapes.push(Shape::new(line, 0.0, palette.text, Color::TRANSPARENT, 0.0));
                            }
                        }
                    }
                    WidgetViewKind::Shape { .. } => {
                        // The category tint marks this as a result display;
                        // orange remains the shared editable-value colour.
                        let line_colour = mix(module.accent, palette.text, 0.55);
                        let plot = Rect::new(
                            Point::new(rect.x + 4.0, rect.y + 3.0),
                            Size::new(rect.width - 8.0, rect.height - GRID - 7.0),
                        );
                        shapes.push(Shape::new(plot, 3.0, palette.tile, palette.tile_border, 1.0));
                        let samples = values.get(module.id, index).map_or(&[][..], |v| v.shape.as_slice());
                        lines.extend(preview_line(plot, samples, line_colour));
                    }
                    // A note is text only: its caption is the whole widget.
                    WidgetViewKind::Note { .. } => {}
                }
            }
        }
        // A control with nothing behind it goes under a veil of the tile's
        // colour, drawn after every control shape and arc, so it reads as
        // out of use (sub-patches.md, Empty slots).
        for module in scene.modules() {
            for index in 0..module.panel.widgets.len() {
                let Some(rect) = module.widget_bounds(index) else {
                    continue;
                };
                if values.get(module.id, index).is_some_and(|v| v.dimmed) {
                    let veil = Color { a: 0.6, ..palette.tile };
                    shapes.push(Shape::new(rect, 2.0, veil, Color::TRANSPARENT, 0.0));
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
fn preview_line(plot: Rect, samples: &[f32], colour: Color) -> impl Iterator<Item = Segment> + '_ {
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

/// Side of the square that marks a control exposed on a sub-patch's
/// surface, in world units.
const EXPOSED_SIDE: f32 = 4.0;

/// The mark of a control inside a sub-patch that is exposed on its
/// surface: a small filled square in the top left corner of its cell
/// (sub-patches.md, Exposing). It differs from the round modulation dot
/// and the hollow ghost ring by shape, so colour need not tell them apart
/// (NFR-UX-03).
pub(crate) fn exposed_mark(rect: Rect, palette: &Palette) -> Shape {
    Shape::new(
        Rect::new(
            Point::new(rect.x + 1.5, rect.y + 1.5),
            Size::new(EXPOSED_SIDE, EXPOSED_SIDE),
        ),
        0.8,
        palette.text_dim,
        Color::TRANSPARENT,
        0.0,
    )
}

/// A hollow ring at `centre`: where a waiting pickup's hardware stands. It
/// differs from the filled live value dot by shape, so colour need not tell
/// them apart.
fn ghost_mark(centre: Point, palette: &Palette) -> Shape {
    circle(centre, LIVE_RADIUS, Color::TRANSPARENT, palette.text, RING_WIDTH)
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
fn modulation_dot(kind: &WidgetViewKind, rect: Rect, mark: ModulationMark, palette: &Palette) -> Option<Shape> {
    let centre = modulation_dot_centre(kind, rect);
    match (kind, mark) {
        (_, ModulationMark::None)
        | (WidgetViewKind::Knob { .. } | WidgetViewKind::Fader { .. }, ModulationMark::Range { .. }) => None,
        // An XY pad draws a mark at each edge.
        (WidgetViewKind::XyPad { .. } | WidgetViewKind::Range { .. }, _) => None,
        // Displays have no parameter, so no assignments.
        (
            WidgetViewKind::Light
            | WidgetViewKind::Readout { .. }
            | WidgetViewKind::Meter { .. }
            | WidgetViewKind::Scope
            | WidgetViewKind::LevelHistory
            | WidgetViewKind::Spectrum
            | WidgetViewKind::XyScope
            | WidgetViewKind::Shape { .. },
            _,
        ) => None,
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
pub(crate) fn modulation_dot_centre(kind: &WidgetViewKind, rect: Rect) -> Point {
    match kind {
        // In the gap at the bottom of the ring, clear of the knob body.
        WidgetViewKind::Knob { .. } => {
            let c = knob_centre(rect);
            Point::new(c.x, c.y + KNOB_RADIUS + 0.5)
        }
        // In the middle of the bar's place beside the track.
        WidgetViewKind::Fader { vertical, .. } => fader_bar_point(rect, *vertical, 0.5),
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
pub(crate) fn knob_centre(rect: Rect) -> Point {
    Point::new(rect.x + rect.width * 0.5, rect.y + GRID - 1.0)
}

/// What a knob always draws: its ring, range arc, and live dot. Text beside
/// a knob may use the rest of the knob's cell (tile_text.rs). The focus
/// ring is left out: it shows only while the knob has keyboard focus, and
/// it stays above the caption anyway.
pub(crate) fn knob_drawn(rect: Rect) -> Rect {
    let centre = knob_centre(rect);
    let reach = RANGE_RADIUS + LIVE_RADIUS;
    Rect::new(
        Point::new(centre.x - reach, centre.y - reach),
        Size::new(2.0 * reach, 2.0 * reach),
    )
}

/// Centre of a knob's dial and the radius halfway across its range ring:
/// where a press on a modulated knob drags depth, not the value
/// (modulation.md, Arc drag). Both in world units. Tests use it to press
/// on the ring without counting pixels.
pub fn knob_range_ring(rect: Rect) -> (Point, f32) {
    (knob_centre(rect), (RANGE_RING_INNER + RANGE_RING_OUTER) * 0.5)
}

/// Centre of a light: in the top row of its 3 × 2 cell.
pub(crate) fn light_centre(rect: Rect) -> Point {
    Point::new(rect.x + rect.width * 0.5, rect.y + GRID * 0.5 + 1.0)
}

/// The bar of a meter: a narrow column in its 3 × 6 cell, above the label
/// row.
pub(crate) fn meter_track(rect: Rect, clips: bool) -> Rect {
    let top = if clips { CLIP_ROW } else { 0.0 };
    Rect::new(
        Point::new(rect.x + rect.width * 0.5 - 4.0, rect.y + top + 1.0),
        Size::new(8.0, rect.height - top - GRID - 2.0),
    )
}

/// Where a meter's peak hold readout is centred: the row below its bar.
pub(crate) fn meter_hold_centre(rect: Rect) -> Point {
    Point::new(rect.center_x(), rect.y + rect.height - GRID * 0.5)
}

/// The height of the row that holds a meter's clip light.
pub(crate) const CLIP_ROW: f32 = GRID;

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
pub(crate) fn focus_ring(kind: &WidgetViewKind, rect: Rect) -> (Rect, f32) {
    match kind {
        // Outside the modulation range arc, so focus never hides it. A
        // circle that wide would cut into the caption, so the ring is a
        // rounded rectangle: as far out as the arc needs at the top and
        // sides, and flatter at the bottom, where the arc has its gap.
        WidgetViewKind::Knob { .. } => {
            let c = knob_centre(rect);
            (
                Rect::new(
                    Point::new(c.x - KNOB_FOCUS_HALF_WIDTH, c.y - KNOB_FOCUS_TOP),
                    Size::new(2.0 * KNOB_FOCUS_HALF_WIDTH, KNOB_FOCUS_TOP + KNOB_FOCUS_BOTTOM),
                ),
                KNOB_FOCUS_CORNER,
            )
        }
        WidgetViewKind::Fader { vertical, .. } => (fader_drawn(rect, *vertical), 4.0),
        WidgetViewKind::XyPad { .. } => (fader_drawn(rect, true), 4.0),
        WidgetViewKind::Range { vertical, .. } => (fader_drawn(rect, *vertical), 4.0),
        WidgetViewKind::Toggle => (toggle_track(rect).expand(2.5), 8.0),
        WidgetViewKind::Number { .. } => (number_box(rect).expand(2.0), 5.0),
        WidgetViewKind::Select { .. } => (select_box(rect).expand(2.0), 5.0),
        // The segments, below the caption row.
        WidgetViewKind::Segmented { options, .. } => {
            let first = segment_rect(rect, 0, options.len());
            let last = segment_rect(rect, options.len().saturating_sub(1), options.len());
            (first.union(last).expand(2.0), 5.0)
        }
        WidgetViewKind::Button => (button_body(rect).expand(2.5), 8.0),
        // The options, below the caption row.
        WidgetViewKind::Radio { .. } => (
            Rect::new(
                Point::new(rect.x, rect.y + GRID),
                Size::new(rect.width, rect.height - GRID),
            )
            .expand(1.0),
            4.0,
        ),
        // Displays and notes never take focus.
        WidgetViewKind::Light
        | WidgetViewKind::Readout { .. }
        | WidgetViewKind::Meter { .. }
        | WidgetViewKind::Scope
        | WidgetViewKind::LevelHistory
        | WidgetViewKind::Spectrum
        | WidgetViewKind::XyScope
        | WidgetViewKind::Shape { .. }
        | WidgetViewKind::Note { .. } => (rect.expand(1.0), 4.0),
    }
}

/// A fader's track: down the cell above the caption row, or across the cell
/// below it.
pub(crate) fn fader_track(rect: Rect, vertical: bool) -> Rect {
    if vertical {
        Rect::new(
            Point::new(rect.center_x() - 1.5, rect.y + 4.0),
            Size::new(3.0, rect.height - GRID - 8.0),
        )
    } else {
        Rect::new(
            Point::new(rect.x + 4.0, rect.y + GRID + FADER_ACROSS - 1.5),
            Size::new(rect.width - 8.0, 3.0),
        )
    }
}

/// The part of a track from position `low` to position `high`.
fn fader_span(track: Rect, vertical: bool, low: f32, high: f32) -> Rect {
    if vertical {
        Rect::new(
            Point::new(track.x, track.y + track.height * (1.0 - high)),
            Size::new(track.width, track.height * (high - low)),
        )
    } else {
        Rect::new(
            Point::new(track.x + track.width * low, track.y),
            Size::new(track.width * (high - low), track.height),
        )
    }
}

/// A fader's handle at position `t`.
pub(crate) fn fader_handle(rect: Rect, vertical: bool, t: f32) -> Rect {
    let track = fader_track(rect, vertical);
    if vertical {
        Rect::new(
            Point::new(track.center_x() - 6.0, track.y + track.height * (1.0 - t) - 2.5),
            Size::new(12.0, 5.0),
        )
    } else {
        Rect::new(
            Point::new(track.x + track.width * t - 2.5, track.center_y() - 4.0),
            Size::new(5.0, 8.0),
        )
    }
}

/// The modulation bar beside a track, from position `low` to `high`.
fn fader_bar(rect: Rect, vertical: bool, low: f32, high: f32) -> Rect {
    let track = fader_track(rect, vertical);
    fader_span(
        if vertical {
            Rect::new(
                Point::new(track.center_x() + FADER_BAR_AWAY - 1.0, track.y),
                Size::new(2.0, track.height),
            )
        } else {
            Rect::new(
                Point::new(track.x, track.center_y() + FADER_BAR_AWAY - 1.0),
                Size::new(track.width, 2.0),
            )
        },
        vertical,
        low,
        high,
    )
}

/// The point at position `t` on the modulation bar: where the live dot goes.
/// With `t` at 0.5, tests and scripts press here to drag depth.
pub fn fader_bar_point(rect: Rect, vertical: bool, t: f32) -> Point {
    let track = fader_track(rect, vertical);
    let t = t.clamp(0.0, 1.0);
    if vertical {
        Point::new(track.center_x() + FADER_BAR_AWAY, track.y + track.height * (1.0 - t))
    } else {
        Point::new(track.x + track.width * t, track.center_y() + FADER_BAR_AWAY)
    }
}

/// True if a press at `world` is on the fader's modulation bar, or its place
/// beside the track: there a drag changes depth, not the value.
pub(crate) fn on_fader_bar(rect: Rect, vertical: bool, world: Point) -> bool {
    let track = fader_track(rect, vertical);
    let away = if vertical {
        world.x - track.center_x()
    } else {
        world.y - track.center_y()
    };
    let along = if vertical {
        track.y - 2.0 <= world.y && world.y <= track.y + track.height + 2.0
    } else {
        track.x - 2.0 <= world.x && world.x <= track.x + track.width + 2.0
    };
    along && (FADER_BAR_AWAY - 2.5..=FADER_BAR_AWAY + 2.5).contains(&away)
}

/// What a fader always draws: its cell without the caption row. Text beside
/// it may use the rest.
pub(crate) fn fader_drawn(rect: Rect, vertical: bool) -> Rect {
    if vertical {
        Rect::new(rect.position(), Size::new(rect.width, rect.height - GRID))
    } else {
        Rect::new(
            Point::new(rect.x, rect.y + GRID - 2.0),
            Size::new(rect.width, rect.height - GRID + 2.0),
        )
    }
}

/// A range's track: as a fader's, with more room around it on a horizontal
/// one for a bar on each side of the handles.
pub(crate) fn range_track(rect: Rect, vertical: bool) -> Rect {
    let track = fader_track(rect, vertical);
    if vertical {
        track
    } else {
        Rect::new(Point::new(track.x, rect.y + GRID + RANGE_ACROSS - 1.5), track.size())
    }
}

/// The point at position `t` on a range's track.
pub(crate) fn range_point(rect: Rect, vertical: bool, t: f32) -> Point {
    let track = range_track(rect, vertical);
    if vertical {
        Point::new(track.center_x(), track.y + track.height * (1.0 - t))
    } else {
        Point::new(track.x + track.width * t, track.center_y())
    }
}

/// A range's handle at position `t`.
pub(crate) fn range_handle(rect: Rect, vertical: bool, t: f32) -> Rect {
    let at = range_point(rect, vertical, t);
    if vertical {
        Rect::new(Point::new(at.x - 6.0, at.y - 2.5), Size::new(12.0, 5.0))
    } else {
        Rect::new(Point::new(at.x - 2.5, at.y - 3.5), Size::new(5.0, 7.0))
    }
}

/// How far a handle's modulation bar sits from the track: `low` before it and
/// `high` after it, so the two bars never overlap (panel-widgets.md, Range).
fn range_bar_offset(vertical: bool, part: ControlPart) -> f32 {
    let away = if vertical { FADER_BAR_AWAY } else { RANGE_BAR_AWAY };
    if part == ControlPart::Second { away } else { -away }
}

/// The point at position `t` on a handle's bar. With `t` at 0.5, tests and
/// scripts press here to drag depth.
pub fn range_bar_point(rect: Rect, vertical: bool, part: ControlPart, t: f32) -> Point {
    let at = range_point(rect, vertical, t.clamp(0.0, 1.0));
    let offset = range_bar_offset(vertical, part);
    if vertical {
        Point::new(at.x + offset, at.y)
    } else {
        Point::new(at.x, at.y + offset)
    }
}

/// A handle's modulation bar, from position `low` to `high`.
fn range_bar(rect: Rect, vertical: bool, part: ControlPart, low: f32, high: f32) -> Rect {
    let track = range_track(rect, vertical);
    let offset = range_bar_offset(vertical, part);
    let beside = if vertical {
        Rect::new(
            Point::new(track.center_x() + offset - 1.0, track.y),
            Size::new(2.0, track.height),
        )
    } else {
        Rect::new(
            Point::new(track.x, track.center_y() + offset - 1.0),
            Size::new(track.width, 2.0),
        )
    };
    fader_span(beside, vertical, low, high)
}

/// The thin line at a handle's live value, across the track.
fn range_live_line(rect: Rect, vertical: bool, t: f32) -> Rect {
    let at = range_point(rect, vertical, t.clamp(0.0, 1.0));
    if vertical {
        Rect::new(Point::new(at.x - 5.0, at.y - 0.5), Size::new(10.0, 1.0))
    } else {
        Rect::new(Point::new(at.x - 0.5, at.y - 4.0), Size::new(1.0, 8.0))
    }
}

/// The handle whose modulation bar, or its place, is under `world`. There a
/// drag changes depth, not the value.
pub(crate) fn range_bar_at(rect: Rect, vertical: bool, world: Point) -> Option<ControlPart> {
    let track = range_track(rect, vertical);
    let (away, along) = if vertical {
        (
            world.x - track.center_x(),
            track.y - 2.0 <= world.y && world.y <= track.y + track.height + 2.0,
        )
    } else {
        (
            world.y - track.center_y(),
            track.x - 2.0 <= world.x && world.x <= track.x + track.width + 2.0,
        )
    };
    let near = |part| (away - range_bar_offset(vertical, part)).abs() <= 1.5;
    if !along {
        None
    } else if near(ControlPart::Main) {
        Some(ControlPart::Main)
    } else if near(ControlPart::Second) {
        Some(ControlPart::Second)
    } else {
        None
    }
}

/// How far `world` lies from position `t`, along the track, in world units.
fn range_distance(rect: Rect, vertical: bool, t: f32, world: Point) -> f32 {
    let at = range_point(rect, vertical, t);
    if vertical {
        (world.y - at.y).abs()
    } else {
        (world.x - at.x).abs()
    }
}

/// The handle nearer `world` along the track. A dropped cable goes to it, and
/// that handle's hover text shows (panel-widgets.md, Range).
pub(crate) fn range_nearest(rect: Rect, vertical: bool, low: f32, high: f32, world: Point) -> ControlPart {
    if range_distance(rect, vertical, low, world) <= range_distance(rect, vertical, high, world) {
        ControlPart::Main
    } else {
        ControlPart::Second
    }
}

/// What a press on a range takes (panel-widgets.md, Range).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RangeGrab {
    /// One handle, on its side of the other. It stops at the other handle.
    Handle {
        /// `Main` for `low`, `Second` for `high`.
        part: ControlPart,
        /// True if it stays above the other handle. Set when it is taken, so a
        /// handle resting on the other cannot pass it on the next move.
        above: bool,
    },
    /// The band: both handles together, keeping the width.
    Band,
    /// Handles that overlap: the first movement decides.
    Either,
}

impl RangeGrab {
    /// A handle, on the side of the other that it is on now.
    pub(crate) fn handle(part: ControlPart, low: f32, high: f32) -> Self {
        let above = if part == ControlPart::Second {
            high >= low
        } else {
            low > high
        };
        Self::Handle { part, above }
    }

    /// What a press at `world` takes, or `None` on the track outside the band.
    pub(crate) fn at(rect: Rect, vertical: bool, low: f32, high: f32, world: Point) -> Option<Self> {
        let on_low = range_distance(rect, vertical, low, world) <= RANGE_REACH;
        let on_high = range_distance(rect, vertical, high, world) <= RANGE_REACH;
        let track = range_point(rect, vertical, 0.0);
        let end = range_point(rect, vertical, 1.0);
        let along = if vertical {
            (track.y - world.y) / (track.y - end.y)
        } else {
            (world.x - track.x) / (end.x - track.x)
        };
        match (on_low, on_high) {
            (true, true) => Some(Self::Either),
            (true, false) => Some(Self::handle(ControlPart::Main, low, high)),
            (false, true) => Some(Self::handle(ControlPart::Second, low, high)),
            (false, false) => (low.min(high) < along && along < low.max(high)).then_some(Self::Band),
        }
    }

    /// What a double-click here resets: the handle, or both.
    pub(crate) fn reset(self) -> ControlPart {
        match self {
            Self::Handle { part, .. } => part,
            Self::Band | Self::Either => ControlPart::Both,
        }
    }

    /// New positions after a move of `delta` along the track, and what the
    /// grab is from now on.
    pub(crate) fn moved(self, low: f32, high: f32, delta: f32) -> (Self, f32, f32) {
        match self {
            Self::Either if delta == 0.0 => (self, low, high),
            // The handle on that side of the other (panel-widgets.md, Range).
            Self::Either => {
                let part = match (delta > 0.0, high >= low) {
                    (true, true) | (false, false) => ControlPart::Second,
                    (true, false) | (false, true) => ControlPart::Main,
                };
                Self::Handle {
                    part,
                    above: delta > 0.0,
                }
                .moved(low, high, delta)
            }
            Self::Band => {
                let delta = delta.clamp(-low.min(high), 1.0 - low.max(high));
                (self, low + delta, high + delta)
            }
            Self::Handle { part, above } => {
                let (own, other) = if part == ControlPart::Second {
                    (high, low)
                } else {
                    (low, high)
                };
                let next = own + delta;
                let next = if above { next.max(other) } else { next.min(other) }.clamp(0.0, 1.0);
                if part == ControlPart::Second {
                    (self, low, next)
                } else {
                    (self, next, high)
                }
            }
        }
    }
}

/// Where a script aims to reach one value of a widget that binds two: the
/// handle of a range, or inside an XY pad at the middle of that value's edge.
/// `None` for any other widget.
pub fn part_bounds(kind: &WidgetViewKind, rect: Rect, position: f32, part: ControlPart) -> Option<Rect> {
    match kind {
        WidgetViewKind::Range { vertical, .. } => Some(range_handle(rect, *vertical, position)),
        WidgetViewKind::XyPad { .. } => {
            let area = xy_area(rect);
            let at = if part == ControlPart::Second {
                Point::new(area.x + 4.0, area.center_y())
            } else {
                Point::new(area.center_x(), area.y + area.height - 4.0)
            };
            Some(Rect::new(Point::new(at.x - 2.0, at.y - 2.0), Size::new(4.0, 4.0)))
        }
        _ => None,
    }
}

/// An XY pad's square: above the caption row, with room at the left and at
/// the bottom for the modulation bars.
pub fn xy_area(rect: Rect) -> Rect {
    let away = XY_BAR_AWAY + XY_BAR_WIDTH;
    let side = (rect.height - GRID - away - 1.0).min(rect.width - 2.0 * away);
    Rect::new(
        Point::new(rect.center_x() - side * 0.5, rect.y + 1.0),
        Size::new(side, side),
    )
}

/// The world point of the pad's point at these positions: `x` grows to the
/// right, and `y` grows upward.
pub(crate) fn xy_point(area: Rect, x: f32, y: f32) -> Point {
    Point::new(area.x + area.width * x, area.y + area.height * (1.0 - y))
}

/// The positions of a world point on the pad, inside its square.
pub(crate) fn xy_position(area: Rect, world: Point) -> (f32, f32) {
    (
        ((world.x - area.x) / area.width).clamp(0.0, 1.0),
        (1.0 - (world.y - area.y) / area.height).clamp(0.0, 1.0),
    )
}

/// The modulation bar of one axis, from position `low` to `high`: along the
/// bottom edge for `x`, and along the left edge for `y`.
fn xy_bar(area: Rect, part: ControlPart, low: f32, high: f32) -> Rect {
    if part == ControlPart::Second {
        Rect::new(
            Point::new(
                area.x - XY_BAR_AWAY - XY_BAR_WIDTH * 0.5,
                area.y + area.height * (1.0 - high),
            ),
            Size::new(XY_BAR_WIDTH, area.height * (high - low)),
        )
    } else {
        Rect::new(
            Point::new(
                area.x + area.width * low,
                area.y + area.height + XY_BAR_AWAY - XY_BAR_WIDTH * 0.5,
            ),
            Size::new(area.width * (high - low), XY_BAR_WIDTH),
        )
    }
}

/// The point at position `t` on one axis's bar. With `t` at 0.5, tests and
/// scripts press here to drag depth.
pub fn xy_edge_point(area: Rect, part: ControlPart, t: f32) -> Point {
    if part == ControlPart::Second {
        Point::new(area.x - XY_BAR_AWAY, area.y + area.height * (1.0 - t))
    } else {
        Point::new(area.x + area.width * t, area.y + area.height + XY_BAR_AWAY)
    }
}

/// The axis whose modulation bar, or its place, is under `world`. There a
/// drag changes depth, not the value.
pub(crate) fn xy_bar_at(area: Rect, world: Point) -> Option<ControlPart> {
    let beside = |distance: f32| (XY_BAR_AWAY - 2.5..=XY_BAR_AWAY + 2.5).contains(&distance);
    let along_x = area.x - 2.0 <= world.x && world.x <= area.x + area.width + 2.0;
    let along_y = area.y - 2.0 <= world.y && world.y <= area.y + area.height + 2.0;
    if along_x && beside(world.y - (area.y + area.height)) {
        Some(ControlPart::Main)
    } else if along_y && beside(area.x - world.x) {
        Some(ControlPart::Second)
    } else {
        None
    }
}

/// The axis whose edge is nearest `world`: the bottom edge for `x`, and the
/// left edge for `y`. A dropped cable goes to it (panel-widgets.md, XY pad).
pub(crate) fn xy_nearest_edge(area: Rect, world: Point) -> ControlPart {
    let bottom = (area.y + area.height - world.y).abs();
    let left = (world.x - area.x).abs();
    if bottom <= left {
        ControlPart::Main
    } else {
        ControlPart::Second
    }
}

/// The outline that shows which edge takes a dropped cable.
pub(crate) fn xy_edge_ring(area: Rect, part: ControlPart) -> Rect {
    xy_bar(area, part, 0.0, 1.0).expand(1.5)
}

/// How many screen pixels make an XY pad's full range: its square on screen,
/// and at least 200 pixels, as for a fader.
pub(crate) fn xy_throw(area: Rect, zoom: f32) -> f32 {
    (area.width * zoom).max(MIN_THROW)
}

/// A number's box, leaving a caption row above it.
pub(crate) fn number_box(rect: Rect) -> Rect {
    select_box(rect)
}

/// The closed select box, leaving a caption row above it.
pub(crate) fn select_box(rect: Rect) -> Rect {
    Rect::new(
        Point::new(rect.x + 2.0, rect.y + GRID + 1.0),
        Size::new(rect.width - 4.0, GRID - 3.0),
    )
}

/// The face of a button: its whole cell, with its caption on it
/// (panels.md, Panel layout, principle 10).
pub(crate) fn button_body(rect: Rect) -> Rect {
    Rect::new(
        Point::new(rect.x + 4.0, rect.y + 4.0),
        Size::new(rect.width - 8.0, rect.height - 8.0),
    )
}

/// The track of a toggle switch: in the top row of its 3 × 2 cell.
fn toggle_track(rect: Rect) -> Rect {
    Rect::new(Point::new(rect.x + 7.0, rect.y + 1.0), Size::new(22.0, 11.0))
}

/// Centre of radio option `i`: one row each, below the label row.
pub(crate) fn radio_centre(rect: Rect, i: usize) -> Point {
    Point::new(rect.x + 6.0, rect.y + GRID * (i as f32 + 1.5))
}

/// The radio option under a world point, if any.
/// Segment `index` of a segmented control with `count` segments: equal
/// shares of the bottom row, with a 1-unit gap between them.
pub(crate) fn segment_rect(rect: Rect, index: usize, count: usize) -> Rect {
    let count = count.max(1) as f32;
    let width = (rect.width - 4.0) / count;
    Rect::new(
        Point::new(rect.x + 2.0 + width * index as f32, rect.y + GRID),
        Size::new(width - 1.0, GRID - 1.0),
    )
}

/// Where option `index` of a radio or a segmented control is drawn: the
/// option's row, or its segment. `None` for any other widget, and for an
/// index past the last option. A screen reader's cursor goes here.
pub fn option_bounds(kind: &WidgetViewKind, rect: Rect, index: usize) -> Option<Rect> {
    match kind {
        WidgetViewKind::Radio { options } if index < options.len() => Some(Rect::new(
            Point::new(rect.x, rect.y + GRID * (index as f32 + 1.0)),
            Size::new(rect.width, GRID),
        )),
        WidgetViewKind::Segmented { options, .. } if index < options.len() => {
            Some(segment_rect(rect, index, options.len()))
        }
        _ => None,
    }
}

/// The centre of segment `index`, for tests and scripts that click it.
pub fn segment_centre(rect: Rect, index: usize, count: usize) -> Point {
    segment_rect(rect, index, count).center()
}

/// The segment of a segmented control under a world point, if any.
pub(crate) fn segment_at(rect: Rect, count: usize, point: Point) -> Option<usize> {
    (0..count).find(|&i| segment_rect(rect, i, count).expand(0.5).contains(point))
}

pub(crate) fn radio_option(rect: Rect, options: usize, point: Point) -> Option<usize> {
    let row = ((point.y - rect.y) / GRID).floor() as isize - 1;
    (row >= 0 && (row as usize) < options).then_some(row as usize)
}

fn circle(centre: Point, radius: f32, fill: Color, border: Color, border_width: f32) -> Shape {
    let rect = Rect::new(
        Point::new(centre.x - radius, centre.y - radius),
        Size::new(radius * 2.0, radius * 2.0),
    );
    Shape::new(rect, radius, fill, border, border_width)
}

pub(crate) fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

/// How a drag on a number control turns pointer movement into a new value:
/// which way it reads, and how many screen pixels make the full range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ValueDrag {
    /// True if movement along the screen's x axis counts. Otherwise y does.
    horizontal: bool,
    /// Screen pixels for the full range.
    throw: f32,
}

impl ValueDrag {
    /// A knob: up raises, and the full range is 200 pixels at any zoom.
    pub const KNOB: Self = Self {
        horizontal: false,
        throw: MIN_THROW,
    };

    /// A fader: along its track. The full range is the track on screen, and
    /// at least 200 pixels, so a short fader is never coarse (panel-widgets.md,
    /// Fader).
    pub fn fader(rect: Rect, vertical: bool, zoom: f32) -> Self {
        let track = fader_track(rect, vertical);
        let length = if vertical { track.height } else { track.width };
        Self {
            horizontal: !vertical,
            throw: (length * zoom).max(MIN_THROW),
        }
    }

    /// A range: along its track, like a fader.
    pub fn range(rect: Rect, vertical: bool, zoom: f32) -> Self {
        Self::fader(rect, vertical, zoom)
    }

    /// A bar on one axis of an XY pad: along that axis, over the pad's throw.
    pub fn xy_axis(area: Rect, part: ControlPart, zoom: f32) -> Self {
        Self {
            horizontal: part != ControlPart::Second,
            throw: xy_throw(area, zoom),
        }
    }

    /// A number: up raises. A whole number takes `200 / steps` pixels per
    /// value, and at least 4, so a small range feels like a knob and a large
    /// range never skips a value (panel-widgets.md, Number).
    pub fn number(steps: u32) -> Self {
        let steps = steps as f32;
        Self {
            horizontal: false,
            throw: (steps * (MIN_THROW / steps.max(1.0)).max(MIN_PIXELS_PER_STEP)).max(MIN_THROW),
        }
    }

    /// Where a screen point lies along the drag, growing as the value does.
    pub fn along(&self, screen: Point) -> f32 {
        if self.horizontal { screen.x } else { -screen.y }
    }

    /// A new position after the pointer moved `moved` pixels along the drag.
    /// Shift is 10 times finer.
    pub fn moved(&self, position: f32, moved: f32, fine: bool) -> f32 {
        (position + self.delta(moved, fine)).clamp(0.0, 1.0)
    }

    /// How far `moved` pixels go, as a share of the full range. Not clamped:
    /// a change that is not one value's position can be as large as the move.
    pub fn delta(&self, moved: f32, fine: bool) -> f32 {
        let scale = if fine { 0.1 } else { 1.0 };
        moved / self.throw * scale
    }

    /// Whether this drag goes left and right. Otherwise it goes up and down.
    pub fn is_horizontal(&self) -> bool {
        self.horizontal
    }
}

/// What one arrow key press adds to a continuous control, as a share of its
/// travel.
const ARROW_SHARE: f32 = 0.01;
/// The same with Shift held.
const ARROW_SHARE_FINE: f32 = 0.001;

/// How far one arrow key press, without Shift, moves a control that holds a
/// number, as a share of its travel: 1% for a continuous value, one step
/// for a whole-number parameter, or the step the parameter declares.
/// `None` for a control with no travel, such as a toggle or a choice.
///
/// A screen reader's slider declares this as its step, so what it says and
/// what [`arrow_step`] does cannot differ.
pub fn arrow_share(kind: &WidgetViewKind) -> Option<f32> {
    match kind {
        WidgetViewKind::Number { key_step, .. } if *key_step > 0.0 => Some(*key_step),
        WidgetViewKind::Knob { steps } | WidgetViewKind::Fader { steps, .. } | WidgetViewKind::Number { steps, .. }
            if *steps > 0 =>
        {
            Some(1.0 / *steps as f32)
        }
        WidgetViewKind::Knob { .. }
        | WidgetViewKind::Fader { .. }
        | WidgetViewKind::Number { .. }
        | WidgetViewKind::XyPad { .. }
        | WidgetViewKind::Range { .. } => Some(ARROW_SHARE),
        _ => None,
    }
}

/// Where one arrow key press moves a control: the new positions of its
/// first and second value, from the ones it has now. `part` is the value
/// the key moves: `Main` or `Second` for one value of an XY pad or one
/// handle of a range, `Both` for a range's band. `up` is Up or Right.
/// `fine` is Shift, 10 times finer on a continuous value. A control with
/// one value gets `second` back as it was.
///
/// `None` for a widget an arrow key does not move this way: a button, or
/// one that only shows a value.
///
/// This is the one place that says what an arrow key does to a control.
/// The keyboard uses it, and so does a screen reader's Increment and
/// Decrement (ui-layer.md, section 7, rule 5).
pub fn arrow_step(
    kind: &WidgetViewKind,
    part: ControlPart,
    main: f32,
    second: f32,
    up: bool,
    fine: bool,
) -> Option<(f32, f32)> {
    let sign = if up { 1.0 } else { -1.0 };
    match kind {
        // A parameter can declare its own step (panel-widgets.md, Number).
        WidgetViewKind::Number { key_step, .. } if *key_step > 0.0 => {
            let step = key_step * if fine { 0.1 } else { 1.0 };
            Some(((main + sign * step).clamp(0.0, 1.0), second))
        }
        WidgetViewKind::Knob { steps } | WidgetViewKind::Fader { steps, .. } | WidgetViewKind::Number { steps, .. } => {
            Some((knob_step(main, up, fine, *steps), second))
        }
        WidgetViewKind::Toggle => Some((if up { 1.0 } else { 0.0 }, second)),
        // A choice's position is the index of its option.
        WidgetViewKind::Radio { options }
        | WidgetViewKind::Select { options }
        | WidgetViewKind::Segmented { options, .. } => {
            let last = options.len().saturating_sub(1) as f32;
            Some(((main.round() + sign).clamp(0.0, last), second))
        }
        WidgetViewKind::XyPad { .. } => match part {
            ControlPart::Main => Some((knob_step(main, up, fine, 0), second)),
            ControlPart::Second => Some((main, knob_step(second, up, fine, 0))),
            ControlPart::Both => None,
        },
        WidgetViewKind::Range { .. } => {
            let grab = match part {
                ControlPart::Both => RangeGrab::Band,
                part => RangeGrab::handle(part, main, second),
            };
            Some(range_step(grab, main, second, up, fine))
        }
        _ => None,
    }
}

/// A range's positions after one arrow key press on what `grab` holds.
pub(crate) fn range_step(grab: RangeGrab, low: f32, high: f32, up: bool, fine: bool) -> (f32, f32) {
    let step = if fine { ARROW_SHARE_FINE } else { ARROW_SHARE };
    let (_, low, high) = grab.moved(low, high, if up { step } else { -step });
    (low, high)
}

/// A knob's new position after one arrow key press: 1% of the range, 0.1%
/// with Shift, or one step for a whole-number parameter.
pub(crate) fn knob_step(position: f32, up: bool, fine: bool, steps: u32) -> f32 {
    let step = if steps > 0 {
        1.0 / steps as f32
    } else if fine {
        ARROW_SHARE_FINE
    } else {
        ARROW_SHARE
    };
    let next = if up { position + step } else { position - step };
    next.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests;
