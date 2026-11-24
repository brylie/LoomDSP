//! Tests for `hit.rs`: which element a point hits, in one panel and across
//! layers.

use super::Hit;
use crate::element::{Element, Length};
use crate::layout::Layout;
use crate::layout::tests::{Mono, id, lay_out};
use loom_ui_core::{Point, Size, UiId};

/// A keyed space of a fixed size.
fn space(key: &str, width: f32, height: f32) -> Element {
    Element::space(Length::Fixed(width), Length::Fixed(height)).key(key)
}

/// A hit as text: its UI ID, or `"blocked"`.
fn describe(hit: Hit<'_>) -> String {
    match hit {
        Hit::Element(id) => id.to_string(),
        Hit::Blocked => "blocked".to_string(),
    }
}

/// What the point (`x`, `y`) hits in `layout`, described, or `"nothing"`.
fn hit(layout: &Layout, x: f32, y: f32) -> String {
    layout
        .hit(Point::new(x, y))
        .map_or_else(|| "nothing".to_string(), describe)
}

/// The deepest keyed element under the point is hit; a point over an
/// unkeyed part hits the keyed element around it.
#[test]
fn the_deepest_keyed_element_is_hit() {
    let row = Element::row([Element::row([
        space("button", 20.0, 20.0),
        Element::space(Length::Fixed(20.0), Length::Fixed(20.0)),
    ])
    .key("bar")
    .padding(5.0)]);
    let layout = lay_out(row, 300.0, 300.0);
    assert_eq!(hit(&layout, 10.0, 10.0), "p/bar/button");
    assert_eq!(hit(&layout, 30.0, 10.0), "p/bar", "the unkeyed space hits the bar");
    assert_eq!(hit(&layout, 2.0, 2.0), "p/bar", "the padding is the bar's");
    assert_eq!(hit(&layout, 200.0, 200.0), "nothing");
}

/// In a stack, the later child is on top.
#[test]
fn a_later_sibling_is_on_top() {
    let layout = lay_out(
        Element::stack([space("under", 50.0, 50.0), space("over", 20.0, 20.0)]),
        300.0,
        300.0,
    );
    assert_eq!(hit(&layout, 10.0, 10.0), "p/over");
    assert_eq!(hit(&layout, 40.0, 40.0), "p/under");
}

/// A point on the edge two elements share hits the right one only.
#[test]
fn a_shared_edge_belongs_to_the_later_element() {
    let layout = lay_out(
        Element::row([space("left", 20.0, 20.0), space("right", 20.0, 20.0)]),
        300.0,
        300.0,
    );
    assert_eq!(hit(&layout, 20.0, 5.0), "p/right");
    assert_eq!(hit(&layout, 19.9, 5.0), "p/left");
    assert_eq!(hit(&layout, 40.0, 5.0), "nothing");
}

/// An opaque element takes the point: nothing under it is hit. With no
/// keyed element around it, the point is blocked; inside a keyed one, that
/// one is hit.
#[test]
fn an_opaque_element_takes_the_point() {
    let cover = Element::space(Length::FILL, Length::FILL).opaque();
    let layout = lay_out(
        Element::stack([space("under", 50.0, 50.0), cover.clone()]),
        300.0,
        300.0,
    );
    assert_eq!(hit(&layout, 10.0, 10.0), "blocked");

    let inside = Element::stack([space("under", 50.0, 50.0), cover]).key("panel");
    assert_eq!(hit(&lay_out(inside, 300.0, 300.0), 10.0, 10.0), "p/panel");
}

/// A later element that repeats a UI ID has none, so the point passes
/// through it to the element around it.
#[test]
fn a_duplicate_falls_through_to_its_parent() {
    let mut layout = Layout::new(UiId::panel("p"));
    layout.allow_duplicates();
    layout.rebuild(Element::row([space("a", 10.0, 10.0), space("a", 10.0, 10.0)]).key("row"));
    layout.compute(Size::new(300.0, 300.0), &mut Mono::default());
    assert_eq!(hit(&layout, 5.0, 5.0), "p/row/a");
    assert_eq!(hit(&layout, 15.0, 5.0), "p/row");
    assert_eq!(layout.bounds(&id("row/a")).map(|bounds| bounds.x), Some(0.0));
}

/// Nothing inside a lost duplicate is hit: a point over its keyed child
/// hits the nearest kept element around it, and so does a point over the
/// duplicate itself, opaque or not.
#[test]
fn nothing_inside_a_duplicate_is_hit() {
    for opaque in [false, true] {
        let mut duplicate = Element::row([space("name", 30.0, 10.0)]).key("row");
        if opaque {
            duplicate = duplicate.opaque();
        }
        let mut layout = Layout::new(UiId::panel("p"));
        layout.allow_duplicates();
        layout.rebuild(Element::column([Element::row([space("value", 10.0, 10.0)]).key("row"), duplicate]).key("list"));
        layout.compute(Size::new(300.0, 300.0), &mut Mono::default());
        assert_eq!(layout.bounds(&id("list/row/name")), None);
        assert_eq!(
            hit(&layout, 5.0, 15.0),
            "p/list",
            "over the duplicate's child, opaque {opaque}"
        );
        assert_eq!(hit(&layout, 5.0, 5.0), "p/list/row/value");
    }
}

/// A child that overflows a clipping parent is hit only inside the clip,
/// as it is drawn only there. Without the clip it is hit where it runs.
#[test]
fn a_clip_cuts_the_hits_of_what_overflows() {
    let row = |clip: bool| {
        let row = Element::row([space("wide", 100.0, 20.0).min_width(100.0)])
            .width(Length::Fixed(40.0))
            .min_width(0.0)
            .key("row");
        let row = if clip { row.clip() } else { row };
        Element::column([row]).align_x(crate::Align::Start)
    };
    let clipped = lay_out(row(true), 200.0, 100.0);
    assert_eq!(hit(&clipped, 30.0, 10.0), "p/row/wide");
    assert_eq!(hit(&clipped, 60.0, 10.0), "nothing", "past the clip");
    let open = lay_out(row(false), 200.0, 100.0);
    assert_eq!(hit(&open, 60.0, 10.0), "p/row/wide", "nothing clips it");
}

/// A clip is a rectangle, even on round corners: a point past a corner's
/// curve and inside the bounds still hits the child there, as the child is
/// still drawn there.
#[test]
fn a_round_clip_cuts_hits_square() {
    let root = Element::column([Element::stack([space("fill", 40.0, 40.0)])
        .key("round")
        .radius(10.0)
        .background(loom_ui_core::Color::WHITE)
        .clip()])
    .align_x(crate::Align::Start);
    let layout = lay_out(root, 200.0, 100.0);
    assert_eq!(hit(&layout, 0.5, 0.5), "p/round/fill", "the corner, past its curve");
}

/// A link to the page `key`, named by `words`.
fn link(words: &str, key: &str) -> crate::element::Inline {
    use crate::access::{Access, Action, Role};
    crate::element::Inline::new(words).link(key, Access::new(Role::Link, words).action(Action::Click))
}

/// A link inside a rich text is hit where its words are, and a point on
/// the paragraph beside it hits the paragraph. The fake measure sets 8
/// pixels a character, so "See " ends at 32 and "Saw" at 56.
#[test]
fn a_link_is_hit_where_its_words_are() {
    use crate::element::Inline;
    let notes = Element::rich(
        [Inline::new("See "), link("Saw", "saw"), Inline::new(" too")],
        crate::layout::tests::STYLE,
    )
    .key("notes");
    let layout = lay_out(Element::column([notes]), 300.0, 100.0);
    assert_eq!(hit(&layout, 40.0, 8.0), "p/notes/saw");
    assert_eq!(hit(&layout, 10.0, 8.0), "p/notes");
    assert_eq!(hit(&layout, 60.0, 8.0), "p/notes");
}

/// A measure that sets text as [`Mono`] does, but puts every range on two
/// lines, as a wrap would: the first half at the end of the first line,
/// the rest at the start of the second.
struct Wrapping(Mono);

impl loom_ui_core::TextMeasure for Wrapping {
    fn measure(&mut self, text: &str, style: loom_ui_core::TextStyle, wrap_width: Option<f32>) -> Size {
        self.0.measure(text, style, wrap_width)
    }

    fn span_rects(
        &mut self,
        _text: &str,
        _spans: &[loom_ui_core::Span],
        _style: loom_ui_core::TextStyle,
        _wrap_width: Option<f32>,
        _range: std::ops::Range<usize>,
    ) -> Vec<loom_ui_core::Rect> {
        use loom_ui_core::Rect;
        vec![
            Rect::new(Point::new(80.0, 0.0), Size::new(20.0, 16.0)),
            Rect::new(Point::new(0.0, 16.0), Size::new(20.0, 16.0)),
        ]
    }
}

/// A link that wraps is hit on both of its lines and not between them,
/// where the paragraph's other words are, and its bounds hold both.
#[test]
fn a_wrapped_link_is_hit_on_each_line_only() {
    use crate::element::Inline;
    let notes = Element::rich(
        [Inline::new("before "), link("Saw", "saw")],
        crate::layout::tests::STYLE,
    )
    .key("notes")
    .width(Length::Fixed(100.0));
    let mut layout = Layout::new(UiId::panel("p"));
    layout.rebuild(Element::column([notes]));
    layout.compute(Size::new(300.0, 100.0), &mut Wrapping(Mono::default()));
    assert_eq!(hit(&layout, 90.0, 8.0), "p/notes/saw", "its first line");
    assert_eq!(hit(&layout, 10.0, 24.0), "p/notes/saw", "its second line");
    assert_eq!(hit(&layout, 10.0, 8.0), "p/notes", "the words before it");
    let bounds = layout.bounds(&id("notes/saw")).expect("laid out");
    assert_eq!(
        bounds,
        loom_ui_core::Rect::new(Point::new(0.0, 0.0), Size::new(100.0, 32.0))
    );
    assert_eq!(layout.pieces(&id("notes/saw")).len(), 2);
}

/// A link in a text with no key is keyed under the element around it, and
/// a clip around the text cuts its hits as it cuts the text.
#[test]
fn a_link_takes_its_scope_and_its_clip() {
    use crate::element::Inline;
    let notes = Element::rich(
        [Inline::new("0123456789 "), link("Saw", "saw")],
        crate::layout::tests::STYLE,
    );
    let clipped = Element::column([notes]).key("box").width(Length::Fixed(100.0)).clip();
    let layout = lay_out(Element::column([clipped]), 300.0, 100.0);
    // "0123456789 " is 88 pixels, so the link runs from 88 to 112 on one
    // line (the fake's default rectangles do not wrap), past the clip.
    assert_eq!(hit(&layout, 92.0, 8.0), "p/box/saw");
    assert_eq!(hit(&layout, 104.0, 8.0), "nothing", "cut away by the clip");
}
