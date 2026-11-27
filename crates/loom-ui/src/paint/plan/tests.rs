//! Tests for the plan of what is painted, built on the CPU with no GPU.

use super::*;
use crate::paint::{Align, ListPainter, Painter, Quad, TextRun, TextStyle, Typesetter, Vertical, Weight};
use std::cell::RefCell;

/// The canvas's place: not at the window's corner, so a missing offset
/// shows.
const BOUNDS: Rect = Rect {
    x: 100.0,
    y: 50.0,
    width: 400.0,
    height: 300.0,
};

/// A target for the canvas at `BOUNDS` in a 1000 by 800 window at `scale`.
fn target(scale: f32) -> Target {
    Target {
        size: BOUNDS.size(),
        scale,
        origin: BOUNDS.position(),
        physical: ((1000.0 * scale) as u32, (800.0 * scale) as u32),
    }
}

/// One line of `text` at `at`.
fn run(text: &str, at: Point) -> TextRun<'_> {
    TextRun {
        text,
        anchor: at,
        bounds: Size::new(200.0, 20.0),
        align: Align::Left,
        vertical: Vertical::Top,
        style: TextStyle {
            size: 12.0,
            line_height: 1.2,
            weight: Weight::Regular,
        },
        wrap: false,
        colour: Color::WHITE,
        clip: BOUNDS,
    }
}

/// A box at `at`.
fn quad(at: Point) -> Quad {
    Quad::filled(Rect::new(at, Size::new(20.0, 10.0)), Color::BLACK, 2.0)
}

/// Canvas text, or `None` on a machine with no font.
fn text() -> Option<RefCell<Typesetter>> {
    let mut text = Typesetter::new();
    let probe = run("x", Point::ORIGIN);
    (text.measure("x", probe.style, None).width > 0.0).then(|| RefCell::new(text))
}

/// A clip becomes a scissor in physical pixels, cut to the target, and a
/// clip outside the target has none.
#[test]
fn a_clip_becomes_a_scissor_in_the_target() {
    assert_eq!(scissor(BOUNDS, target(1.0)), Some([100, 50, 400, 300]));
    assert_eq!(scissor(BOUNDS, target(2.0)), Some([200, 100, 800, 600]));
    let half = Rect::new(Point::new(100.25, 50.0), Size::new(10.5, 10.0));
    assert_eq!(
        scissor(half, target(1.0)),
        Some([100, 50, 11, 10]),
        "outward to whole pixels"
    );
    let past = Rect::new(Point::new(900.0, 700.0), Size::new(500.0, 500.0));
    assert_eq!(scissor(past, target(1.0)), Some([900, 700, 100, 100]));
    let away = Rect::new(Point::new(2000.0, 0.0), Size::new(5.0, 5.0));
    assert_eq!(scissor(away, target(1.0)), None);
    // Two clips that do not overlap cut to an empty one, at a place that
    // a fractional scale would round outward into a pixel.
    let empty = Rect::new(Point::new(1.0, 1.0), Size::ZERO);
    assert_eq!(scissor(empty, target(1.25)), None, "an empty clip shows nothing");
    let flat = Rect::new(Point::new(1.0, 1.0), Size::new(5.0, 0.0));
    assert_eq!(scissor(flat, target(1.25)), None);
}

/// The plan draws layer by layer and batch by batch in the order painted,
/// with each layer's scissor, and boxes placed from the canvas's corner.
#[test]
fn the_plan_keeps_the_painted_order() {
    let Some(text) = text() else { return };
    let mut list = PaintList::new(BOUNDS);
    let popup = Rect::new(Point::new(150.0, 100.0), Size::new(100.0, 50.0));
    let mut painter = ListPainter::new(&text, &mut list);
    // "Gain", not a word with "ff" or "fi": a font may join those into
    // one glyph, as Linux CI's does.
    painter.text(&run("Gain", Point::new(110.0, 60.0)));
    painter.begin_layer(popup);
    painter.quad(quad(Point::new(150.0, 100.0)));
    painter.text(&run("50", Point::new(155.0, 105.0)));
    painter.quad(quad(Point::new(160.0, 100.0)));
    painter.end_layer();

    let mut atlas = Atlas::new(super::super::atlas::MAX_SIZE);
    let plan = PaintPlan::build(&list, &mut atlas, target(1.0));
    let kinds: Vec<(&str, [u32; 4])> = plan
        .steps
        .iter()
        .map(|step| match step.draw {
            Draw::Boxes(_) => ("boxes", step.scissor),
            Draw::Glyphs(_) => ("glyphs", step.scissor),
        })
        .collect();
    let canvas = [100, 50, 400, 300];
    let layer = [150, 100, 100, 50];
    assert_eq!(
        kinds,
        [
            ("glyphs", canvas),
            ("boxes", layer),
            ("glyphs", layer),
            ("boxes", layer)
        ],
        "tile text, then the layer's box, text, and caret over it"
    );
    let first_box = plan.boxes[0];
    assert_eq!(first_box.rect(), [50.0, 50.0, 20.0, 10.0]);
    assert_eq!(plan.glyphs.len(), 6, "four letters and two digits");
}

/// A layer wholly outside the target draws nothing, and an empty batch
/// is no step.
#[test]
fn a_layer_outside_the_target_draws_nothing() {
    let text = RefCell::new(Typesetter::new());
    let mut list = PaintList::new(BOUNDS);
    let away = Rect::new(Point::new(5000.0, 0.0), Size::new(10.0, 10.0));
    let mut painter = ListPainter::new(&text, &mut list);
    painter.begin_layer(away);
    painter.quad(quad(Point::new(5000.0, 0.0)));
    painter.end_layer();
    painter.text(&run(" ", Point::new(110.0, 60.0)));
    let mut atlas = Atlas::new(super::super::atlas::MAX_SIZE);
    let plan = PaintPlan::build(&list, &mut atlas, target(1.0));
    assert!(plan.steps.is_empty(), "{:?}", plan.steps);
}

/// At its largest size, an atlas too small for the frame leaves glyphs
/// out and counts them, never a panic. A smaller atlas grows first.
#[test]
fn a_full_atlas_skips_glyphs_and_counts_them() {
    let Some(text) = text() else { return };
    let mut list = PaintList::new(BOUNDS);
    let line = "The quick brown fox jumps over the lazy dog";
    ListPainter::new(&text, &mut list).text(&run(line, Point::new(110.0, 60.0)));

    let mut tiny = Atlas::sized(16, 16);
    let plan = PaintPlan::build(&list, &mut tiny, target(2.0));
    assert!(tiny.skipped > 0);
    assert!(plan.glyphs.len() < list.glyphs.len());
    assert_eq!(plan.skipped, tiny.skipped, "the plan counts what it left out");

    let mut growing = Atlas::sized(16, 1024);
    let plan = PaintPlan::build(&list, &mut growing, target(2.0));
    assert_eq!(growing.skipped, 0);
    assert!(growing.size() > 16, "it grew");
    let inked = list.glyphs.len() - line.matches(' ').count();
    assert_eq!(plan.glyphs.len(), inked, "every glyph with ink is placed");
}

/// A packed colour keeps its four channels as they are, with no change of
/// colour space.
#[test]
fn a_packed_colour_keeps_its_channels() {
    assert_eq!(packed(Color::rgba(0.25, 0.5, 0.75, 0.4)), [0.25, 0.5, 0.75, 0.4]);
    assert_eq!(packed(Color::TRANSPARENT), [0.0, 0.0, 0.0, 0.0]);
}

/// At scale 2, a clip edge on a half pixel rounds outward to whole physical
/// pixels: the scissor covers every pixel the clip touches, so nothing a
/// hit can reach inside the clip is cut away.
#[test]
fn a_half_pixel_clip_rounds_out_at_scale_two() {
    let clip = Rect::new(Point::new(100.25, 50.75), Size::new(10.5, 10.0));
    // 200.5 to 221.5 across, 101.5 to 121.5 down.
    assert_eq!(scissor(clip, target(2.0)), Some([200, 101, 22, 21]));
}

/// Each of the list's layers maps to its first step, a layer clipped away
/// included, so a cut after one still draws the right steps: here the
/// boxes before the cut are the first two steps, and the one after it the
/// last.
#[test]
fn layers_map_to_their_steps_past_a_layer_clipped_away() {
    let text = RefCell::new(Typesetter::new());
    let mut list = PaintList::new(BOUNDS);
    {
        let mut painter = ListPainter::new(&text, &mut list);
        painter.quad(quad(Point::new(120.0, 60.0)));
        // Outside the target: no steps.
        painter.begin_layer(Rect::new(Point::new(-500.0, -500.0), Size::new(10.0, 10.0)));
        painter.quad(quad(Point::new(-495.0, -495.0)));
        painter.end_layer();
        painter.quad(quad(Point::new(140.0, 60.0)));
        painter.mark(crate::paint::Mark::LayerEnd(crate::view::Layer::Panels));
        painter.quad(quad(Point::new(160.0, 60.0)));
    }
    let mut atlas = Atlas::new(1024);
    let plan = PaintPlan::build(&list, &mut atlas, target(1.0));
    let cut = list.marks()[0].0;
    assert_eq!(plan.layer_steps.len(), list.layer_count() + 1);
    assert_eq!(plan.layer_steps[cut], 2, "two boxes drew before the cut");
    assert_eq!(plan.layer_steps[list.layer_count()], 3);
    assert_eq!(plan.steps.len(), 3);
}
