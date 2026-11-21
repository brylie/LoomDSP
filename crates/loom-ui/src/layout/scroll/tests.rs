//! Tests for `layout/scroll.rs`: a container's viewport, gutter, and
//! thumb, and that its thumb is painted after its clip's layer ends.

use crate::access::{Access, Role};
use crate::element::{Element, Length};
use crate::layout::Layout;
use crate::layout::tests::{STYLE, id, lay_out};
use crate::paint::PanelStyle;
use crate::paint::testing::{Painted, Recorder};
use crate::window::scroll::tests::SCROLL;
use loom_ui_core::{Color, Point, Rect, Size};

/// Ten rows of text in a container 100 px tall, keyed `rows`, laid out in
/// 200 by 100.
fn list() -> Layout {
    let rows = (0..10).map(|n| Element::text(format!("row {n}"), STYLE));
    let scroll = Element::scroll(Element::column(rows), SCROLL)
        .key("rows")
        .width(Length::FILL)
        .height(Length::Fixed(100.0))
        .access(Access::new(Role::ScrollView, "Rows"));
    lay_out(scroll, 200.0, 100.0)
}

/// The viewport is the content box less the 10 px gutter, the thumb runs
/// 2 px in from the gutter's edges, as long as the view's share of the
/// content, and the content keeps 4 px of ring room above its first row.
#[test]
fn the_viewport_gutter_and_thumb_are_where_the_spec_puts_them() {
    let layout = list();
    let index = layout.scroll_index(&id("rows")).expect("the container");
    let record = &layout.records()[index];
    assert_eq!(record.scroll.map(|scroll| scroll.most), Some(68.0));
    assert_eq!(
        record.viewport(),
        Some(Rect::new(Point::ORIGIN, Size::new(190.0, 100.0)))
    );
    assert_eq!(
        record.gutter(),
        Some(Rect::new(Point::new(190.0, 0.0), Size::new(10.0, 100.0)))
    );
    let thumb = record.thumb().expect("a thumb, as the content overflows");
    assert_eq!((thumb.x, thumb.y, thumb.width), (192.0, 2.0, 6.0));
    assert!((thumb.height - 96.0 * 100.0 / 168.0).abs() < 0.01, "{}", thumb.height);
    let first = (0..layout.records().len())
        .find(|index| layout.text(*index).is_some_and(|text| text.text == "row 0"))
        .expect("the first row");
    assert_eq!(layout.records()[first].bounds.y, 4.0, "ring room above it");
}

/// Content that fits has no thumb, and nothing to scroll.
#[test]
fn content_that_fits_has_no_thumb() {
    let scroll = Element::scroll(Element::text("one row", STYLE), SCROLL)
        .key("rows")
        .height(Length::Fixed(100.0))
        .access(Access::new(Role::ScrollView, "Rows"));
    let layout = lay_out(scroll, 200.0, 100.0);
    let record = &layout.records()[0];
    assert_eq!(record.scroll.map(|scroll| scroll.most), Some(0.0));
    assert_eq!(record.thumb(), None);
}

/// The thumb is painted after the container's clip layer ends, so no row
/// covers it, in the token's colour and radius.
#[test]
fn the_thumb_is_painted_after_its_clip_layer_ends() {
    let layout = list();
    let mut recorder = Recorder::default();
    let style = PanelStyle {
        text: Color::WHITE,
        caret: Color::WHITE,
        selection: Color::BLACK,
        quiet: Color::WHITE,
        mark: Color::WHITE,
        ring: crate::focus::RingStyle {
            color: Color::WHITE,
            width: 2.0,
            offset: 2.0,
        },
    };
    let host = Rect::new(Point::ORIGIN, Size::new(1000.0, 1000.0));
    layout.paint(Point::ORIGIN, host, &mut recorder, &style, None);
    let last = recorder.painted.last().expect("something painted");
    let Painted::Quad(thumb) = last else {
        panic!("the thumb comes last, after the layer: {last:?}");
    };
    assert_eq!((thumb.fill, thumb.radius), (SCROLL.thumb, SCROLL.radius));
    let before = &recorder.painted[recorder.painted.len() - 2];
    assert!(matches!(before, Painted::End), "{before:?}");
}
