//! Tests for the paint list: order, layers, placement, and one layout for
//! measuring and painting.

use super::*;
use crate::painter::Weight;

/// The canvas's place in the tests.
const BOUNDS: Rect = Rect {
    x: 100.0,
    y: 50.0,
    width: 800.0,
    height: 600.0,
};

/// A plain style at 12 px.
fn style() -> TextStyle {
    TextStyle {
        size: 12.0,
        line_height: 1.2,
        weight: Weight::Regular,
    }
}

/// One line of `text` placed by `anchor` as `align` and `vertical` say.
fn run(text: &str, anchor: Point, align: Align, vertical: Vertical) -> TextRun<'_> {
    TextRun {
        text,
        anchor,
        bounds: Size::new(200.0, 20.0),
        align,
        vertical,
        style: style(),
        wrap: false,
        colour: Color::WHITE,
        clip: BOUNDS,
    }
}

/// A box at `x`.
fn quad(x: f32) -> Quad {
    Quad::filled(Rect::new(Point::new(x, 60.0), Size::new(10.0, 10.0)), Color::BLACK, 0.0)
}

/// Canvas text, or `None` with no system font, where glyph tests skip.
fn text() -> Option<RefCell<CanvasText>> {
    let mut text = CanvasText::new();
    (text.measure("x", style(), None).width > 0.0).then(|| RefCell::new(text))
}

/// Pieces in a row of one kind make one batch, and a change of kind starts
/// another, so a caret painted after its text is drawn over it.
#[test]
fn batches_keep_the_call_order() {
    let Some(text) = text() else { return };
    let mut list = PaintList::new(BOUNDS);
    let mut painter = ListPainter::new(&text, &mut list);
    painter.begin_layer(BOUNDS);
    painter.quad(quad(110.0));
    painter.quad(quad(120.0));
    painter.text(&run("50", Point::new(130.0, 80.0), Align::Left, Vertical::Center));
    painter.quad(quad(140.0));
    painter.end_layer();

    assert_eq!(list.layers.len(), 3, "the canvas's, the field's, and the one after");
    let glyphs = list.glyphs.len() as u32;
    assert_eq!(glyphs, 2, "one glyph a digit");
    assert_eq!(
        list.layers[1].batches,
        [Batch::Boxes(0..2), Batch::Glyphs(0..glyphs), Batch::Boxes(2..3)]
    );
    assert!(list.layers[0].batches.is_empty());
    assert!(list.layers[2].batches.is_empty());
}

/// A layer keeps its clip, and the layer after one ends takes the clip of
/// the layer around it, however deep.
#[test]
fn layers_nest_and_restore_their_clip() {
    let text = RefCell::new(CanvasText::new());
    let mut list = PaintList::new(BOUNDS);
    let inner = Rect::new(Point::new(200.0, 200.0), Size::new(50.0, 50.0));
    let innermost = Rect::new(Point::new(210.0, 210.0), Size::new(5.0, 5.0));
    let mut painter = ListPainter::new(&text, &mut list);
    painter.begin_layer(inner);
    painter.begin_layer(innermost);
    painter.end_layer();
    painter.quad(quad(220.0));
    painter.end_layer();
    painter.quad(quad(230.0));
    let clips: Vec<Rect> = list.layers.iter().map(|layer| layer.clip).collect();
    assert_eq!(clips, [BOUNDS, inner, innermost, inner, BOUNDS]);
    assert_eq!(list.layers[3].batches, [Batch::Boxes(0..1)]);
    assert_eq!(list.layers[4].batches, [Batch::Boxes(1..2)]);
    assert!(!list.is_empty());
}

/// A default list paints into a layer with no clip of its own.
#[test]
fn a_default_list_starts_a_layer() {
    let text = RefCell::new(CanvasText::new());
    let mut list = PaintList::default();
    assert!(list.is_empty());
    ListPainter::new(&text, &mut list).quad(quad(0.0));
    assert_eq!(list.layers.len(), 1);
    assert_eq!(list.layers[0].batches, [Batch::Boxes(0..1)]);
}

/// The anchor is the left, middle, or right of the text's own width, and
/// its top or middle, as the run says.
#[test]
fn the_anchor_places_the_text_box() {
    let Some(text) = text() else { return };
    let size = text.borrow_mut().measure("Cutoff", style(), None);
    let anchor = Point::new(400.0, 300.0);
    let first_x = |align| {
        let mut list = PaintList::new(BOUNDS);
        ListPainter::new(&text, &mut list).text(&run("Cutoff", anchor, align, Vertical::Top));
        list.glyphs[0].origin.x
    };
    assert_eq!(first_x(Align::Left), anchor.x);
    assert!((first_x(Align::Center) - (anchor.x - size.width / 2.0)).abs() < 1e-3);
    assert!((first_x(Align::Right) - (anchor.x - size.width)).abs() < 1e-3);

    let baseline = |vertical| {
        let mut list = PaintList::new(BOUNDS);
        ListPainter::new(&text, &mut list).text(&run("Cutoff", anchor, Align::Left, vertical));
        list.glyphs[0].origin.y
    };
    let top = baseline(Vertical::Top);
    assert!(
        top > anchor.y && top < anchor.y + size.height,
        "the baseline is inside the line"
    );
    assert!((baseline(Vertical::Center) - (top - size.height / 2.0)).abs() < 1e-3);
}

/// Glyphs carry the run's colour and clip, and one face is named once.
#[test]
fn glyphs_carry_the_run_and_share_a_face() {
    let Some(text) = text() else { return };
    let mut list = PaintList::new(BOUNDS);
    let clip = Rect::new(Point::new(10.0, 10.0), Size::new(30.0, 30.0));
    let mut painter = ListPainter::new(&text, &mut list);
    painter.text(&TextRun {
        colour: Color::BLACK,
        clip,
        ..run("Mix", Point::new(20.0, 20.0), Align::Left, Vertical::Top)
    });
    painter.text(&run("Max", Point::new(20.0, 40.0), Align::Left, Vertical::Top));
    assert_eq!(list.faces.len(), 1);
    assert_eq!(list.faces[0].size, 12.0);
    assert!(
        list.glyphs[..3]
            .iter()
            .all(|g| g.colour == Color::BLACK && g.clip == clip)
    );
    assert!(
        list.glyphs[3..]
            .iter()
            .all(|g| g.colour == Color::WHITE && g.clip == BOUNDS)
    );
}

/// Measuring a run and then painting it sets the text once: the box laid
/// out and the glyphs drawn come from one layout.
#[test]
fn measure_and_paint_share_one_layout() {
    let text = RefCell::new(CanvasText::new());
    let mut list = PaintList::new(BOUNDS);
    let mut painter = ListPainter::new(&text, &mut list);
    let measured = painter.measure("Resonance", style(), None);
    painter.text(&run("Resonance", Point::new(0.0, 0.0), Align::Left, Vertical::Top));
    assert_eq!(text.borrow_mut().take_built(), 1);
    assert_eq!(text.borrow_mut().measure("Resonance", style(), None), measured);
}

/// A wrapped run is set at its room's width.
#[test]
fn a_wrapped_run_breaks_at_its_width() {
    let Some(text) = text() else { return };
    let sentence = "The cutoff frequency of the filter, in hertz.";
    let mut list = PaintList::new(BOUNDS);
    ListPainter::new(&text, &mut list).text(&TextRun {
        wrap: true,
        bounds: Size::new(60.0, 100.0),
        ..run(sentence, Point::new(0.0, 0.0), Align::Left, Vertical::Top)
    });
    let lines: std::collections::BTreeSet<u32> = list.glyphs.iter().map(|g| g.origin.y.to_bits()).collect();
    assert!(lines.len() > 1, "{lines:?}");
}

/// A layer inside another is cut to both clips, and one wholly outside its
/// parent is cut to nothing.
#[test]
fn a_nested_layer_is_cut_to_its_parent() {
    let text = RefCell::new(CanvasText::new());
    let mut list = PaintList::new(BOUNDS);
    let parent = Rect::new(Point::new(200.0, 200.0), Size::new(50.0, 50.0));
    let across = Rect::new(Point::new(240.0, 190.0), Size::new(30.0, 30.0));
    let away = Rect::new(Point::new(400.0, 400.0), Size::new(10.0, 10.0));
    let mut painter = ListPainter::new(&text, &mut list);
    painter.begin_layer(parent);
    painter.begin_layer(across);
    painter.end_layer();
    painter.begin_layer(away);
    painter.end_layer();
    painter.end_layer();
    let clips: Vec<Rect> = list.layers.iter().map(|layer| layer.clip).collect();
    assert_eq!(clips[2], Rect::new(Point::new(240.0, 200.0), Size::new(10.0, 20.0)));
    assert_eq!(clips[4].size(), Size::new(0.0, 0.0), "outside its parent");
    assert_eq!(clips[3], parent, "back to the parent's clip");
    assert_eq!(clips[5], parent);
    assert_eq!(clips[6], BOUNDS, "back to the canvas's clip");
    assert_eq!(clips.len(), 7);
}
