//! `loom-ui`'s layout measuring real text: the canvas's Parley text, which
//! sets the same text it draws (docs/design/ui-layer.md, section 8). Here,
//! because this crate depends on both.

use loom_canvas::CanvasText;
use loom_ui::{Element, Layout, Length};
use loom_ui_core::{Size, TextStyle, UiId, Weight};

/// The style of a small label.
const STYLE: TextStyle = TextStyle {
    size: 12.0,
    line_height: 1.25,
    weight: Weight::Regular,
};

/// A wrapping row of 50 labels, each keyed by its number.
fn labels() -> Element {
    Element::row((0..50_u64).map(|index| Element::text(format!("Label {index}"), STYLE).key(index)))
        .width(Length::FILL)
        .spacing(4.0)
        .wrap()
}

/// A label is laid out at the width Parley sets it, rounded to a whole
/// pixel.
#[test]
fn a_label_is_as_wide_as_parley_sets_it() {
    let mut text = CanvasText::new();
    let mut layout = Layout::new(UiId::panel("p"));
    layout.rebuild(Element::row([Element::text("Cutoff", STYLE).key("label")]));
    layout.compute(Size::new(400.0, 100.0), &mut text);
    let set = text.measure("Cutoff", STYLE, None);
    let bounds = layout
        .bounds(&UiId::panel("p").with("label"))
        .expect("the label is laid out");
    assert!(set.width > 0.0, "Parley found a font");
    assert_eq!(bounds.width, set.width.round());
}

/// 50 labels build 50 Parley layouts on the first pass. The next frame's
/// rebuild finds them all in the text's own cache, so it builds none.
#[test]
fn a_rebuild_finds_its_text_in_the_cache() {
    let mut text = CanvasText::new();
    let mut layout = Layout::new(UiId::panel("p"));
    layout.rebuild(labels());
    layout.compute(Size::new(400.0, 300.0), &mut text);
    assert_eq!(text.take_built(), 50);

    text.begin_frame();
    layout.rebuild(labels());
    layout.compute(Size::new(400.0, 300.0), &mut text);
    assert_eq!(text.take_built(), 0);
}
