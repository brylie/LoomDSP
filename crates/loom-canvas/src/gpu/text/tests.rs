//! Tests for the plan of painted text, built on the CPU with no GPU.

use super::*;
use crate::painter::{Painter, Quad, TextRun, TextStyle, Vertical, Weight};
use crate::text::{CanvasText, ListPainter};
use crate::tile_text::Align;
use loom_ui_core::{Color, Size};
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
fn text() -> Option<RefCell<CanvasText>> {
    let mut text = CanvasText::new();
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
}

/// The plan draws layer by layer and batch by batch in the order painted,
/// with each layer's scissor, and boxes placed from the canvas's corner.
#[test]
fn the_plan_keeps_the_painted_order() {
    let Some(text) = text() else { return };
    let mut list = PaintList::new(BOUNDS);
    let popup = Rect::new(Point::new(150.0, 100.0), Size::new(100.0, 50.0));
    let mut painter = ListPainter::new(&text, &mut list);
    painter.text(&run("Cutoff", Point::new(110.0, 60.0)));
    painter.begin_layer(popup);
    painter.quad(quad(Point::new(150.0, 100.0)));
    painter.text(&run("50", Point::new(155.0, 105.0)));
    painter.quad(quad(Point::new(160.0, 100.0)));
    painter.end_layer();

    let mut atlas = Atlas::new(super::super::atlas::MAX_SIZE);
    let plan = TextPlan::build(&list, &mut atlas, target(1.0));
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
    assert_eq!(plan.glyphs.len(), 8, "six letters and two digits");
}

/// A layer wholly outside the target draws nothing, and an empty batch
/// is no step.
#[test]
fn a_layer_outside_the_target_draws_nothing() {
    let text = RefCell::new(CanvasText::new());
    let mut list = PaintList::new(BOUNDS);
    let away = Rect::new(Point::new(5000.0, 0.0), Size::new(10.0, 10.0));
    let mut painter = ListPainter::new(&text, &mut list);
    painter.begin_layer(away);
    painter.quad(quad(Point::new(5000.0, 0.0)));
    painter.end_layer();
    painter.text(&run(" ", Point::new(110.0, 60.0)));
    let mut atlas = Atlas::new(super::super::atlas::MAX_SIZE);
    let plan = TextPlan::build(&list, &mut atlas, target(1.0));
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

    let before = crate::stats::RenderCounts::this_thread().glyph_skips;
    let mut tiny = Atlas::sized(16, 16);
    let plan = TextPlan::build(&list, &mut tiny, target(2.0));
    assert!(tiny.skipped > 0);
    assert!(plan.glyphs.len() < list.glyphs.len());
    let after = crate::stats::RenderCounts::this_thread().glyph_skips;
    assert_eq!(after - before, tiny.skipped);

    let mut growing = Atlas::sized(16, 1024);
    let plan = TextPlan::build(&list, &mut growing, target(2.0));
    assert_eq!(growing.skipped, 0);
    assert!(growing.size() > 16, "it grew");
    let inked = list.glyphs.len() - line.matches(' ').count();
    assert_eq!(plan.glyphs.len(), inked, "every glyph with ink is placed");
}
