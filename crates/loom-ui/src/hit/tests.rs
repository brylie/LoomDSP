//! Tests for `hit.rs`: which element a point hits, in one panel and across
//! layers.

use super::{Hit, hit_layers};
use crate::element::{Element, Length};
use crate::layout::Layout;
use crate::layout::tests::{Mono, id, lay_out};
use crate::view::Layer;
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

/// The topmost layer is tried first, then each one below; within a layer,
/// the later layout is on top.
#[test]
fn layers_are_tried_from_the_top() {
    let panel = |key: &str| Layout::new(UiId::panel(key));
    let place = |mut layout: Layout, element: Element| {
        layout.rebuild(element);
        layout.compute(Size::new(300.0, 300.0), &mut Mono::default());
        layout
    };
    let toolbar = place(panel("toolbar"), space("undo", 100.0, 20.0));
    let menu = place(panel("menu"), space("open", 50.0, 50.0));
    let help = place(panel("help"), space("close", 10.0, 10.0));
    // Given in any order: the overlays are tried before the panels, and
    // `help`, later in the same layer, before `menu`.
    let layers = [
        (Layer::Overlays, &menu),
        (Layer::Panels, &toolbar),
        (Layer::Overlays, &help),
    ];
    let at = |x, y| hit_layers(&layers, Point::new(x, y)).map(|(layer, hit)| (layer, describe(hit)));

    assert_eq!(at(5.0, 5.0), Some((Layer::Overlays, "help/close".to_string())));
    assert_eq!(at(20.0, 5.0), Some((Layer::Overlays, "menu/open".to_string())));
    assert_eq!(at(80.0, 5.0), Some((Layer::Panels, "toolbar/undo".to_string())));
    assert_eq!(at(200.0, 200.0), None);
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
