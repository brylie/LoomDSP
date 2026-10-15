//! Panel controls on tiles: their live values and their GPU instances
//! (ADR-0013). Spec: `docs/design/panels.md`, Module panels on tiles.
//!
//! Values live apart from the [`Scene`]. Changing one rebuilds only the
//! controls' instances, never the tiles, ports, or cables (ADR-0010).

use crate::gpu::{Arc as ArcShape, Shape};
use crate::scene::{GRID, Scene, WidgetViewKind};
use crate::theme::Palette;
use iced::{Color, Point, Rectangle, Size};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// A knob's ring starts here, in radians clockwise from the top.
pub(crate) const KNOB_START: f32 = -0.75 * std::f32::consts::PI;
/// A knob's ring spans this angle.
pub(crate) const KNOB_SWEEP: f32 = 1.5 * std::f32::consts::PI;
/// Radius of a knob's ring, in world units.
const KNOB_RADIUS: f32 = 9.0;

/// The live value of one widget.
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetValue {
    /// Knob: position from 0 to 1. Toggle: 0 or 1. Radio: option index.
    /// Light: brightness from 0 to 1.
    pub position: f32,
    /// The value as text, with its unit, shown while a knob is dragged or
    /// focused.
    pub text: String,
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
}

impl ControlGeometry {
    pub fn build(scene: &Scene, values: &PanelValues, palette: &Palette) -> Self {
        let mut arcs = Vec::new();
        let mut shapes = Vec::new();
        for module in scene.modules() {
            for (index, widget) in module.panel.widgets.iter().enumerate() {
                let Some(rect) = module.widget_bounds(index) else {
                    continue;
                };
                let position = values.position(module.id, index);
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
                        if t > 0.0 {
                            arcs.push(ArcShape::new(
                                centre,
                                KNOB_RADIUS,
                                2.5,
                                KNOB_START,
                                KNOB_SWEEP * t,
                                palette.control_value,
                            ));
                        }
                        shapes.push(circle(centre, 6.5, palette.control_body, palette.tile_border, 1.0));
                        let angle = KNOB_START + KNOB_SWEEP * t;
                        let tip = Point::new(centre.x + 4.0 * angle.sin(), centre.y - 4.0 * angle.cos());
                        shapes.push(circle(tip, 1.5, palette.text, Color::TRANSPARENT, 0.0));
                    }
                    WidgetViewKind::Toggle => {
                        let on = position > 0.5;
                        let track = Rectangle::new(Point::new(rect.x + 7.0, rect.y + 1.0), Size::new(22.0, 11.0));
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
                    WidgetViewKind::Light => {
                        let b = position.clamp(0.0, 1.0);
                        let fill = mix(palette.light_off, palette.light_on, b);
                        shapes.push(circle(light_centre(rect), 5.0, fill, palette.tile_border, 1.0));
                    }
                }
            }
        }
        Self {
            key: (scene.generation(), values.generation()),
            arcs,
            shapes,
        }
    }
}

/// Centre of a knob's dial: in the top two grid rows of its 3 × 3 cell.
pub(crate) fn knob_centre(rect: Rectangle) -> Point {
    Point::new(rect.x + rect.width * 0.5, rect.y + GRID - 1.0)
}

/// Centre of a light: in the top row of its 3 × 2 cell.
pub(crate) fn light_centre(rect: Rectangle) -> Point {
    Point::new(rect.x + rect.width * 0.5, rect.y + GRID * 0.5 + 1.0)
}

/// Centre of radio option `i`: one row each, below the label row.
pub(crate) fn radio_centre(rect: Rectangle, i: usize) -> Point {
    Point::new(rect.x + 6.0, rect.y + GRID * (i as f32 + 1.5))
}

/// The radio option under a world point, if any.
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

fn mix(a: Color, b: Color, t: f32) -> Color {
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

    fn scene() -> Scene {
        let widget = |kind, x| WidgetView {
            kind,
            x,
            y: 0,
            label: String::new(),
        };
        Scene::new(
            vec![ModuleView {
                id: 1,
                title: String::new(),
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
            },
        );
        let after = ControlGeometry::build(&scene, &values, &palette);
        // The scene, and so the tile, port, and cable buffers, did not change.
        assert_eq!(scene.generation(), scene_generation);
        assert_ne!(after.key, before.key);
        // A value arc appeared on the knob.
        assert_eq!(after.arcs.len(), before.arcs.len() + 1);
    }

    #[test]
    fn setting_the_same_value_keeps_the_generation() {
        let mut values = PanelValues::new();
        let value = WidgetValue {
            position: 0.25,
            text: String::new(),
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
    fn radio_rows() {
        let rect = Rectangle::new(Point::ORIGIN, Size::new(36.0, 48.0));
        assert_eq!(radio_option(rect, 3, Point::new(5.0, 5.0)), None);
        assert_eq!(radio_option(rect, 3, Point::new(5.0, GRID + 1.0)), Some(0));
        assert_eq!(radio_option(rect, 3, Point::new(5.0, 3.0 * GRID + 1.0)), Some(2));
        assert_eq!(radio_option(rect, 3, Point::new(5.0, 4.0 * GRID + 1.0)), None);
    }
}
