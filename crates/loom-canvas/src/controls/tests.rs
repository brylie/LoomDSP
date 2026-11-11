use super::*;
use crate::gpu::packed;
use crate::scene::{ModuleView, PanelView, WidgetView};

fn scene() -> Scene {
    let widget = |kind, x| WidgetView {
        kind,
        x,
        y: 0,
        label: String::new(),
        hover: crate::scene::HoverText::default(),
        hidden: false,
        exposed: false,
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
    assert_eq!(low.colour(), packed(palette.control_value), "in the value colour");
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
    let colour = packed(Palette::default().modulation);
    let clear = packed(Color::TRANSPARENT);
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
    let rect = Rect::new(Point::ORIGIN, Size::new(3.0 * GRID, 3.0 * GRID));
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
    let knob_drag = |position, dy: f32, fine| ValueDrag::KNOB.moved(position, -dy, fine);
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
                    kind: WidgetViewKind::Meter {
                        zero: 0.0,
                        clips: false,
                    },
                    x: 0,
                    y: 0,
                    label: String::new(),
                    hover: crate::scene::HoverText::default(),
                    hidden: false,
                    exposed: false,
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
                    kind: WidgetViewKind::Meter {
                        zero: 0.0,
                        clips: false,
                    },
                    x: 0,
                    y: 0,
                    label: String::new(),
                    hover: crate::scene::HoverText::default(),
                    hidden: false,
                    exposed: false,
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
    let rect = Rect::new(Point::ORIGIN, Size::new(36.0, 48.0));
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
            exposed: false,
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

/// A fader's drag is along its track. Up raises a vertical one and right raises
/// a horizontal one. The full range is at least 200 screen pixels, so a short
/// fader at middle zoom is never coarse (panel-widgets.md, Fader).
#[test]
fn a_fader_drags_along_its_track_over_at_least_200_pixels() {
    let tall = Rect::new(Point::ORIGIN, Size::new(2.0 * GRID, 6.0 * GRID));
    let wide = Rect::new(Point::ORIGIN, Size::new(6.0 * GRID, 2.0 * GRID));
    let up = ValueDrag::fader(tall, true, 0.7);
    let right = ValueDrag::fader(wide, false, 0.7);
    assert_eq!(up.along(Point::new(9.0, 40.0)), -40.0);
    assert_eq!(right.along(Point::new(9.0, 40.0)), 9.0);
    assert_eq!(up.moved(0.0, 100.0, false), 0.5, "200 pixels at middle zoom");
    assert_eq!(right.moved(0.0, 100.0, false), 0.5);
    assert!((up.moved(0.0, 100.0, true) - 0.05).abs() < 1e-6, "Shift is finer");

    // Zoomed far in, the track on screen is longer, and it is the full range.
    let close = ValueDrag::fader(wide, false, 5.0);
    let length = fader_track(wide, false).width * 5.0;
    assert!(length > 200.0);
    assert!((close.moved(0.0, length, false) - 1.0).abs() < 1e-6);
}

/// The handle follows the value, and stays within the cell. The modulation bar
/// lies clear of the handle, in the place a press on it is taken as a depth
/// drag.
#[test]
fn a_fader_handle_and_bar_are_where_they_should_be() {
    for (rect, vertical) in [
        (Rect::new(Point::ORIGIN, Size::new(2.0 * GRID, 6.0 * GRID)), true),
        (Rect::new(Point::ORIGIN, Size::new(6.0 * GRID, 2.0 * GRID)), false),
    ] {
        for t in [0.0, 0.5, 1.0] {
            let handle = fader_handle(rect, vertical, t);
            assert!(rect.expand(0.01).contains(handle.position()), "{handle:?}");
            let bar = fader_bar_point(rect, vertical, t);
            assert!(on_fader_bar(rect, vertical, bar), "{bar:?}");
            assert!(!handle.contains(bar), "the bar is clear of the handle");
            assert!(rect.contains(bar), "{bar:?} in {rect:?}");
        }
        assert!(!on_fader_bar(rect, vertical, fader_track(rect, vertical).center()));
        let top = fader_handle(rect, vertical, 1.0).center();
        let bottom = fader_handle(rect, vertical, 0.0).center();
        if vertical {
            assert!(top.y < bottom.y, "up is higher");
        } else {
            assert!(top.x > bottom.x, "right is higher");
        }
    }
}

/// A fader shows its reach as a bar and its live value as a dot, and only the
/// dot when the reach has no length, as a knob does (rule 3).
#[test]
fn a_fader_shows_modulation_as_a_bar_with_a_live_dot() {
    let rect = Rect::new(Point::ORIGIN, Size::new(2.0 * GRID, 6.0 * GRID));
    let palette = Palette::default();
    let kind = WidgetViewKind::Fader {
        vertical: true,
        steps: 0,
    };
    let range = ModulationMark::Range { low: 0.2, high: 0.6 };
    assert!(
        modulation_dot(&kind, rect, range, &palette).is_none(),
        "the bar shows it"
    );
    assert!(modulation_dot(&kind, rect, ModulationMark::Dot, &palette).is_some());
    assert!(modulation_dot(&kind, rect, ModulationMark::Ring, &palette).is_some());
    let bar = fader_bar(rect, true, 0.2, 0.6);
    let track = fader_track(rect, true);
    assert!((bar.height - track.height * 0.4).abs() < 1e-3);
    let live = fader_bar_point(rect, true, 0.5);
    assert!(bar.expand(0.01).contains(live), "the live dot sits on the bar");
}

/// A whole-number drag takes 200 pixels over a small range, like a knob, and
/// never skips a value over a large one. Shift is 10 times finer
/// (panel-widgets.md, Number).
#[test]
fn a_number_drag_steps_by_one() {
    let small = ValueDrag::number(10);
    assert_eq!(small.moved(0.0, 20.0, false), 0.1, "20 pixels per value");
    assert_eq!(small, ValueDrag::KNOB, "a small range feels like a knob");

    let large = ValueDrag::number(1000);
    assert!(
        (large.moved(0.0, 4.0, false) - 0.001).abs() < 1e-6,
        "4 pixels per value"
    );
    assert!((large.moved(0.0, 40.0, true) - 0.001).abs() < 1e-6, "40 with Shift");
    assert_eq!(ValueDrag::number(0), ValueDrag::KNOB, "a smooth value is like a knob");
}

/// A handle stops at the other and cannot pass it, whichever way it comes
/// from, and after resting on it (panel-widgets.md, Range).
#[test]
fn a_range_handle_stops_at_the_other() {
    let low = RangeGrab::handle(ControlPart::Main, 0.2, 0.7);
    let (grab, a, b) = low.moved(0.2, 0.7, 0.9);
    assert_eq!((a, b), (0.7, 0.7), "low stops at high");
    let (_, a, b) = grab.moved(a, b, 0.1);
    assert_eq!((a, b), (0.7, 0.7), "resting on it, low still cannot pass");
    let (_, a, _) = grab.moved(0.7, 0.7, -0.2);
    assert!((a - 0.5).abs() < 1e-6, "and it can move away");

    let high = RangeGrab::handle(ControlPart::Second, 0.2, 0.7);
    let (_, a, b) = high.moved(0.2, 0.7, -0.9);
    assert_eq!((a, b), (0.2, 0.2), "high stops at low");
    let (_, _, b) = high.moved(0.2, 0.7, 0.9);
    assert_eq!(b, 1.0, "and at the end of the track");
}

/// A crossed handle moves freely away from the other, stops on reaching it,
/// and never jumps (panel-widgets.md, Range).
#[test]
fn a_crossed_range_handle_keeps_its_side() {
    let low = RangeGrab::handle(ControlPart::Main, 0.7, 0.2);
    let (_, a, _) = low.moved(0.7, 0.2, 0.1);
    assert!((a - 0.8).abs() < 1e-6, "away from the other");
    let (_, a, b) = low.moved(0.7, 0.2, -0.9);
    assert_eq!((a, b), (0.2, 0.2), "stops on reaching it");
}

/// The band keeps its width, and stops when either handle reaches an end,
/// crossed or not (panel-widgets.md, Range).
#[test]
fn a_range_band_keeps_its_width() {
    let (_, a, b) = RangeGrab::Band.moved(0.2, 0.7, 0.1);
    assert!((a - 0.3).abs() < 1e-6 && (b - 0.8).abs() < 1e-6);
    let (_, a, b) = RangeGrab::Band.moved(0.2, 0.7, 0.9);
    assert!((a - 0.5).abs() < 1e-6 && b == 1.0, "stops at the top");
    let (_, a, b) = RangeGrab::Band.moved(0.2, 0.7, -0.9);
    assert!(a == 0.0 && (b - 0.5).abs() < 1e-6, "stops at the bottom");
    let (_, a, b) = RangeGrab::Band.moved(0.7, 0.2, -0.9);
    assert!((a - 0.5).abs() < 1e-6 && b == 0.0, "crossed too");
}

/// Where handles overlap, the first movement takes the handle on that side,
/// at both ends of the track and when crossed. Before it moves, nothing is
/// taken (panel-widgets.md, Range).
#[test]
fn overlapping_range_handles_follow_the_first_movement() {
    let either = RangeGrab::Either;
    assert_eq!(either.moved(1.0, 1.0, 0.0), (either, 1.0, 1.0));

    let (_, a, b) = either.moved(1.0, 1.0, -0.1);
    assert!((a - 0.9).abs() < 1e-6 && b == 1.0, "at the top, down takes low");
    let (_, a, b) = either.moved(0.0, 0.0, 0.1);
    assert!(a == 0.0 && (b - 0.1).abs() < 1e-6, "at the bottom, up takes high");

    let (grab, a, b) = either.moved(0.5, 0.5, 0.1);
    assert!(a == 0.5 && (b - 0.6).abs() < 1e-6, "in the middle, up takes high");
    assert_eq!(
        grab,
        RangeGrab::Handle {
            part: ControlPart::Second,
            above: true
        },
        "and keeps it"
    );

    let (_, a, b) = either.moved(0.52, 0.5, 0.1);
    assert!(
        (a - 0.62).abs() < 1e-6 && b == 0.5,
        "crossed, up takes the one drawn higher"
    );
    let (_, a, b) = either.moved(0.52, 0.5, -0.1);
    assert!(
        a == 0.52 && (b - 0.4).abs() < 1e-6,
        "crossed, down takes the one drawn lower"
    );
}

/// What a press on a range takes: a handle, the band, both where they overlap,
/// and nothing on the track outside the band. A double-click resets the same
/// thing (panel-widgets.md, Range).
#[test]
fn a_press_on_a_range_takes_what_is_under_it() {
    let rect = Rect::new(Point::new(0.0, 0.0), Size::new(72.0, 24.0));
    let at = |t: f32| range_point(rect, false, t);
    let take = |low, high, t| RangeGrab::at(rect, false, low, high, at(t));
    assert!(matches!(
        take(0.2, 0.8, 0.2),
        Some(RangeGrab::Handle {
            part: ControlPart::Main,
            ..
        })
    ));
    assert!(matches!(
        take(0.2, 0.8, 0.8),
        Some(RangeGrab::Handle {
            part: ControlPart::Second,
            ..
        })
    ));
    assert_eq!(take(0.2, 0.8, 0.5), Some(RangeGrab::Band));
    assert_eq!(take(0.2, 0.8, 0.05), None, "outside the band");
    assert_eq!(take(0.5, 0.52, 0.5), Some(RangeGrab::Either));
    assert_eq!(RangeGrab::Band.reset(), ControlPart::Both);
    assert_eq!(RangeGrab::Either.reset(), ControlPart::Both);
    assert_eq!(
        RangeGrab::handle(ControlPart::Second, 0.2, 0.8).reset(),
        ControlPart::Second
    );
}

/// A range's bars sit on opposite sides of the track, clear of the handles, on
/// both directions (panel-widgets.md, Range).
#[test]
fn a_range_has_a_bar_on_each_side() {
    for (vertical, rect) in [
        (false, Rect::new(Point::new(0.0, 0.0), Size::new(72.0, 24.0))),
        (true, Rect::new(Point::new(0.0, 0.0), Size::new(24.0, 72.0))),
    ] {
        let low = range_bar_point(rect, vertical, ControlPart::Main, 0.5);
        let high = range_bar_point(rect, vertical, ControlPart::Second, 0.5);
        assert_ne!(low, high);
        assert_eq!(range_bar_at(rect, vertical, low), Some(ControlPart::Main));
        assert_eq!(range_bar_at(rect, vertical, high), Some(ControlPart::Second));
        let handle = range_handle(rect, vertical, 0.5);
        assert!(!handle.contains(low) && !handle.contains(high), "clear of the handle");
        assert!(rect.contains(low) && rect.contains(high), "inside the cell");
        assert_eq!(range_bar_at(rect, vertical, range_point(rect, vertical, 0.5)), None);
    }
}

/// A drag's change is the whole move, not capped by a position's 0 to 1, so a
/// range's band and a depth keep up with a fast pointer.
#[test]
fn a_drag_delta_is_not_capped() {
    assert!((ValueDrag::KNOB.delta(180.0, false) - 0.9).abs() < 1e-6);
    assert!((ValueDrag::KNOB.delta(-180.0, false) + 0.9).abs() < 1e-6);
    assert!((ValueDrag::KNOB.delta(180.0, true) - 0.09).abs() < 1e-6);
}

/// A waiting pickup's hardware shows as one more shape on a knob, and only
/// the control values change, so the scene is not rebuilt (midi-learn.md,
/// Mapping modes).
#[test]
fn a_ghost_adds_a_hollow_mark() {
    let scene = scene();
    let palette = Palette::default();
    let mut values = PanelValues::new();
    values.set(1, 0, modulated(0.5, ModulationMark::None));
    let without = ControlGeometry::build(&scene, &values, &palette);
    values.set(
        1,
        0,
        WidgetValue {
            ghost: Some(0.75),
            ..modulated(0.5, ModulationMark::None)
        },
    );
    let with = ControlGeometry::build(&scene, &values, &palette);
    assert_eq!(with.shapes.len(), without.shapes.len() + 1);
    assert_eq!(with.key.0, without.key.0, "the scene is not rebuilt");
    assert_ne!(with.key, without.key, "the control values changed");
}

/// A clip light leaves the bar shorter than a meter without one, and a meter's
/// bar stays clear of the readout's row.
#[test]
fn a_clip_light_shortens_the_bar() {
    let rect = Rect::new(Point::ORIGIN, Size::new(36.0, 72.0));
    let plain = meter_track(rect, false);
    let with_light = meter_track(rect, true);
    assert!(with_light.height < plain.height);
    assert!(with_light.y > plain.y);
    assert!(with_light.y + with_light.height < meter_hold_centre(rect).y);
}

/// The text of an XY pad's or a range's two values, for a test.
fn two_parts() -> Box<[crate::scene::PartText; 2]> {
    Box::default()
}

/// One arrow key press moves a continuous knob by 1%, a stepped one by one
/// step, and a number with its own step by that step.
#[test]
fn an_arrow_step_follows_the_kind_of_value() {
    let main = |kind: &WidgetViewKind, at: f32, up: bool, fine: bool| {
        arrow_step(kind, ControlPart::Main, at, 0.0, up, fine).unwrap().0
    };
    let knob = WidgetViewKind::Knob { steps: 0 };
    assert!((main(&knob, 0.5, true, false) - 0.51).abs() < 1e-6);
    assert!((main(&knob, 0.5, false, true) - 0.499).abs() < 1e-6);
    assert_eq!(main(&knob, 1.0, true, false), 1.0);
    let stepped = WidgetViewKind::Fader {
        vertical: true,
        steps: 4,
    };
    assert_eq!(main(&stepped, 0.5, true, false), 0.75);
    let number = WidgetViewKind::Number {
        steps: 0,
        key_step: 0.125,
    };
    assert_eq!(main(&number, 0.5, false, false), 0.375);
    assert_eq!(main(&number, 0.5, true, true), 0.5125);
}

/// The step a slider declares is the step an arrow key takes.
#[test]
fn the_declared_share_is_the_step_taken() {
    let kinds = [
        WidgetViewKind::Knob { steps: 0 },
        WidgetViewKind::Knob { steps: 16 },
        WidgetViewKind::Fader {
            vertical: false,
            steps: 0,
        },
        WidgetViewKind::Number {
            steps: 0,
            key_step: 0.25,
        },
        WidgetViewKind::XyPad { parts: two_parts() },
    ];
    for kind in kinds {
        let share = arrow_share(&kind).unwrap();
        let (next, _) = arrow_step(&kind, ControlPart::Main, 0.25, 0.5, true, false).unwrap();
        assert!((next - 0.25 - share).abs() < 1e-6, "{kind:?}");
    }
    assert_eq!(arrow_share(&WidgetViewKind::Toggle), None);
    assert_eq!(arrow_share(&WidgetViewKind::Button), None);
}

/// A toggle goes on with Up and off with Down, and a choice moves one
/// option and stops at its ends.
#[test]
fn an_arrow_step_on_a_toggle_and_a_choice() {
    let step =
        |kind: &WidgetViewKind, at: f32, up: bool| arrow_step(kind, ControlPart::Main, at, 0.0, up, false).unwrap().0;
    assert_eq!(step(&WidgetViewKind::Toggle, 0.0, true), 1.0);
    assert_eq!(step(&WidgetViewKind::Toggle, 1.0, false), 0.0);
    let select = WidgetViewKind::Select {
        options: vec!["a".into(), "b".into(), "c".into()],
    };
    assert_eq!(step(&select, 1.0, true), 2.0);
    assert_eq!(step(&select, 2.0, true), 2.0);
    assert_eq!(step(&select, 0.0, false), 0.0);
}

/// An XY pad moves one value at a time, and a range's handle stops at the
/// other handle. Its band moves both and keeps the width.
#[test]
fn an_arrow_step_on_a_control_with_two_values() {
    let pad = WidgetViewKind::XyPad { parts: two_parts() };
    let (x, y) = arrow_step(&pad, ControlPart::Second, 0.2, 0.4, true, false).unwrap();
    assert_eq!(x, 0.2);
    assert!((y - 0.41).abs() < 1e-6);
    assert_eq!(arrow_step(&pad, ControlPart::Both, 0.2, 0.4, true, false), None);

    let range = WidgetViewKind::Range {
        vertical: false,
        parts: two_parts(),
    };
    let (low, high) = arrow_step(&range, ControlPart::Main, 0.395, 0.4, true, false).unwrap();
    assert_eq!((low, high), (0.4, 0.4));
    let (low, high) = arrow_step(&range, ControlPart::Both, 0.2, 0.4, false, false).unwrap();
    assert!((low - 0.19).abs() < 1e-6 && (high - 0.39).abs() < 1e-6);
}

/// A button and a widget that only shows a value have no arrow step.
#[test]
fn no_arrow_step_on_a_button_or_a_display() {
    for kind in [WidgetViewKind::Button, WidgetViewKind::Light, WidgetViewKind::Scope] {
        assert_eq!(arrow_step(&kind, ControlPart::Main, 0.5, 0.0, true, false), None);
    }
}
