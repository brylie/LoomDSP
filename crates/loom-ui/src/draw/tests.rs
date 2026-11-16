//! Tests for `draw.rs`: what a laid-out panel paints, in what order, and
//! where, with the recording painter.

use crate::element::{Element, Length};
use crate::layout::tests::{STYLE, lay_out};
use crate::paint::PanelStyle;
use crate::paint::testing::{Painted, Recorder};
use crate::view::{Layer, View};
use crate::window::Window;
use loom_ui_core::{Color, Point, Rect, Size, UiId};

/// The style the tests paint with: its text colour is one no element sets.
const PANEL: PanelStyle = PanelStyle {
    text: Color::rgb(0.5, 0.5, 0.5),
};

/// A red the tests set on boxes and text.
const RED: Color = Color::rgb(1.0, 0.0, 0.0);

/// A blue the tests set on borders.
const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);

/// A rectangle from its four numbers.
fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect::new(Point::new(x, y), Size::new(width, height))
}

/// What `root`, laid out in 200 by 100, paints at `origin`, inside the
/// whole of a 1000 by 1000 host.
fn painted(root: Element, origin: Point) -> Vec<Painted> {
    let layout = lay_out(root, 200.0, 100.0);
    let mut recorder = Recorder::default();
    let count = layout.paint(origin, rect(0.0, 0.0, 1000.0, 1000.0), &mut recorder, &PANEL);
    assert_eq!(count, layout.element_count() as u64, "every element is counted");
    recorder.painted
}

/// A box paints first, under its text, and the text sits inside the border
/// and the padding: a border of 2 and padding of 4 put it at 6. Text with
/// no colour of its own takes the panel style's.
#[test]
fn a_box_paints_under_its_text_inside_border_and_padding() {
    let root = Element::column([Element::text("ab", STYLE)])
        .background(RED)
        .border(2.0, BLUE)
        .radius(3.0)
        .padding(4.0);
    let painted = painted(root, Point::ORIGIN);
    let [Painted::Quad(quad), Painted::Text { text, run }] = painted.as_slice() else {
        panic!("a box, then the text: {painted:?}");
    };
    // The text is 16 by 16; the box adds 6 on each side.
    assert_eq!(quad.rect, rect(0.0, 0.0, 28.0, 28.0));
    assert_eq!(
        (quad.fill, quad.border, quad.border_width, quad.radius),
        (RED, BLUE, 2.0, 3.0)
    );
    assert_eq!(text, "ab");
    assert_eq!(run.anchor, Point::new(6.0, 6.0));
    assert_eq!(run.bounds, Size::new(16.0, 16.0));
    assert_eq!(run.colour, PANEL.text);
}

/// A text's own colour wins over the panel style's, and the origin moves
/// everything painted.
#[test]
fn a_text_colour_and_the_origin_are_kept() {
    let painted = painted(Element::text("ab", STYLE).colour(RED), Point::new(30.0, 40.0));
    let [Painted::Text { run, .. }] = painted.as_slice() else {
        panic!("one text: {painted:?}");
    };
    assert_eq!(run.colour, RED);
    assert_eq!(run.anchor, Point::new(30.0, 40.0));
}

/// An element with no background and no border paints no box.
#[test]
fn an_element_with_no_look_paints_no_box() {
    let painted = painted(
        Element::row([Element::space(Length::Fixed(10.0), Length::Fixed(10.0))]).padding(3.0),
        Point::ORIGIN,
    );
    assert!(painted.is_empty(), "{painted:?}");
}

/// A clipping element opens a layer cut to its bounds before its children
/// and closes it after the last one, so a later sibling is outside it.
/// Text inside carries the clip too.
#[test]
fn a_clip_opens_a_layer_around_its_children_only() {
    let root = Element::column([
        Element::row([Element::text("wide text", STYLE)])
            .width(Length::Fixed(40.0))
            .min_width(0.0)
            .clip(),
        Element::text("after", STYLE),
    ])
    .align_x(crate::Align::Start);
    let painted = painted(root, Point::ORIGIN);
    let [
        Painted::Begin(clip),
        Painted::Text { text: inside, run: cut },
        Painted::End,
        Painted::Text { text: after, run: free },
    ] = painted.as_slice()
    else {
        panic!("a layer around the clipped row only: {painted:?}");
    };
    assert_eq!(*clip, rect(0.0, 0.0, 40.0, 16.0));
    assert_eq!((inside.as_str(), after.as_str()), ("wide text", "after"));
    assert_eq!(cut.clip, rect(0.0, 0.0, 40.0, 16.0), "the text inside is cut");
    assert_eq!(free.clip, rect(0.0, 0.0, 1000.0, 1000.0), "the text after is not");
}

/// A text element that clips cuts its own letters to its bounds, not only
/// what is inside it.
#[test]
fn a_clipping_text_cuts_its_own_letters() {
    let root = Element::row([Element::text("wide text", STYLE)
        .width(Length::Fixed(30.0))
        .min_width(0.0)
        .clip()])
    .align_y(crate::Align::Start);
    let painted = painted(root, Point::ORIGIN);
    let run = painted
        .iter()
        .find_map(|piece| match piece {
            Painted::Text { run, .. } => Some(*run),
            _ => None,
        })
        .expect("the text is painted");
    assert_eq!(run.clip, rect(0.0, 0.0, 30.0, 16.0), "cut to its own 30-wide box");
}

/// Clips nest: a clip inside a clip is cut to both.
#[test]
fn a_clip_inside_a_clip_is_cut_to_both() {
    let inner = Element::row([Element::text("abcdefgh", STYLE)])
        .width(Length::Fixed(30.0))
        .min_width(0.0)
        .clip();
    // The spacer keeps its 20: a fixed size shrinks too when space is short.
    let spacer = Element::space(Length::Fixed(20.0), Length::Fixed(1.0)).min_width(20.0);
    let root = Element::row([spacer, inner])
        .width(Length::Fixed(35.0))
        .min_width(0.0)
        .clip();
    let painted = painted(root, Point::ORIGIN);
    let run = painted
        .iter()
        .find_map(|piece| match piece {
            Painted::Text { run, .. } => Some(*run),
            _ => None,
        })
        .expect("the text is painted");
    // The inner row is at 20, 30 wide; the outer clip ends at 35.
    assert_eq!(run.clip, rect(20.0, 0.0, 15.0, 16.0));
}

/// The key of a test panel.
fn panel(key: &str) -> UiId {
    UiId::panel(key)
}

/// A window of a parent with a slot 40 wide, a child too wide for it, and
/// a panel in another layer, laid out at 300 by 200.
fn window() -> Window {
    fn parent(_: &u8) -> Element {
        Element::row([
            Element::text("p", STYLE).background(RED),
            Element::slot(panel("child"))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(20.0)),
        ])
        .align_y(crate::Align::Start)
    }
    fn child(_: &u8) -> Element {
        Element::text("child text", STYLE)
    }
    fn menu(_: &u8) -> Element {
        Element::text("menu", STYLE)
    }
    let mut view = View::new();
    view.panel_with(panel("parent"), Layer::Panels, &0, parent);
    view.panel_with(panel("child"), Layer::Panels, &0, child);
    view.panel_with(panel("menu"), Layer::Overlays, &0, menu);
    let mut window = Window::default();
    window.sync(view.built(None));
    window.lay_out(Size::new(300.0, 200.0), &mut crate::layout::tests::Mono::default());
    window
}

/// A layer's panels paint in draw order: the parent's elements, then the
/// panel in its slot, inside a layer cut to the slot. Other layers' panels
/// are not painted.
#[test]
fn a_layer_paints_its_panels_in_draw_order_cut_to_their_slots() {
    let window = window();
    let mut recorder = Recorder::default();
    let painted = window.paint(Layer::Panels, &mut recorder, &PANEL);
    assert_eq!(
        (painted.panels, painted.elements),
        (2, 4),
        "a row, a text, and a slot, then a text"
    );
    let texts = recorder.texts();
    assert_eq!(texts, ["p", "child text"], "the menu is in another layer");
    let begins: Vec<Rect> = recorder
        .painted
        .iter()
        .filter_map(|piece| match piece {
            Painted::Begin(clip) => Some(*clip),
            _ => None,
        })
        .collect();
    // The parent is cut to the window; the child to its slot, after the
    // 8-pixel "p".
    assert_eq!(begins, [rect(0.0, 0.0, 300.0, 200.0), rect(8.0, 0.0, 40.0, 20.0)]);
    let Some(Painted::Text { run, .. }) = recorder.painted.iter().rev().find(|piece| piece.as_text().is_some()) else {
        panic!("the child's text is painted");
    };
    assert_eq!(
        run.clip,
        rect(8.0, 0.0, 40.0, 20.0),
        "the child's text is cut to its slot"
    );
}

/// A panel too wide for its slot is not hit outside the slot, as it is not
/// drawn there.
#[test]
fn a_panel_is_hit_only_inside_its_slot() {
    fn parent(_: &u8) -> Element {
        Element::row([Element::slot(panel("child"))
            .width(Length::Fixed(40.0))
            .height(Length::Fixed(20.0))])
        .align_y(crate::Align::Start)
    }
    fn child(_: &u8) -> Element {
        Element::space(Length::Fixed(100.0), Length::Fixed(20.0)).key("wide")
    }
    let mut view = View::new();
    view.panel_with(panel("parent"), Layer::Panels, &0, parent);
    view.panel_with(panel("child"), Layer::Panels, &0, child);
    let mut window = Window::default();
    window.sync(view.built(None));
    window.lay_out(Size::new(300.0, 200.0), &mut crate::layout::tests::Mono::default());
    assert!(window.hit(Point::new(30.0, 10.0)).is_some(), "inside the slot");
    assert_eq!(
        window.hit(Point::new(60.0, 10.0)),
        None,
        "past the slot, though the child reaches there"
    );
}
