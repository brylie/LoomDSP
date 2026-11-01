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
