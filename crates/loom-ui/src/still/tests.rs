//! Tests for a still picture of a view's panels.

use super::*;
use crate::element::{Element, Length};
use crate::layout::tests::{Mono, STYLE};
use crate::paint::{ListPainter, PaintList, Typesetter};
use loom_ui_core::{Color, Point, Rect, UiId};

/// A still paints what the view's panels hold, in their own layers only.
#[test]
fn a_still_paints_each_layer_of_the_view() {
    let mut view = View::new();
    view.panel_with(UiId::panel("bar"), Layer::Panels, &(), |_| {
        Element::text("Ready", STYLE)
            .width(Length::Fixed(80.0))
            .background(Color::rgb(1.0, 0.0, 0.0))
    });
    let size = Size::new(200.0, 100.0);
    let still = Still::new(&view, size, &mut Mono::default());
    let text = std::cell::RefCell::new(Typesetter::new());
    let whole = Rect::new(Point::ORIGIN, size);
    let style = PanelStyle {
        text: Color::WHITE,
        caret: Color::WHITE,
        selection: Color::BLACK,
        ring: crate::focus::RingStyle {
            color: Color::WHITE,
            width: 2.0,
            offset: 2.0,
        },
        quiet: Color::rgb(0.6, 0.6, 0.6),
        mark: Color::rgb(0.9, 0.9, 0.9),
        primary: Color::rgb(0.3, 0.6, 1.0),
        track: Color::rgb(0.4, 0.4, 0.4),
    };
    let mut panels = PaintList::new(whole);
    still.paint(Layer::Panels, &mut ListPainter::new(&text, &mut panels), &style);
    let mut overlays = PaintList::new(whole);
    still.paint(Layer::Overlays, &mut ListPainter::new(&text, &mut overlays), &style);
    assert!(!panels.is_empty(), "the bar is painted");
    assert!(overlays.is_empty(), "nothing is in the overlays");
}
