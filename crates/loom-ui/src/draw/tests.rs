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
    caret: Color::rgb(0.9, 0.9, 0.9),
    selection: Color::rgb(0.2, 0.3, 0.6),
    ring: crate::focus::RingStyle {
        color: Color::WHITE,
        width: 2.0,
        offset: 2.0,
    },
    quiet: Color::rgb(0.6, 0.6, 0.6),
    mark: Color::rgb(0.9, 0.9, 0.9),
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
    let count = layout.paint(origin, rect(0.0, 0.0, 1000.0, 1000.0), &mut recorder, &PANEL, None);
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
    let painted = window.paint(Layer::Panels, &mut recorder, &PANEL, None, (None, None), 1.0);
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

/// A test panel: its key, its layer, and how its elements are built.
type TestPanel = (&'static str, Layer, fn(&u8) -> Element);

/// A window laid out at 300 by 200 from `panels`.
fn window_of(panels: &[TestPanel]) -> Window {
    let mut view = View::new();
    for (key, layer, build) in panels {
        view.panel_with(panel(key), *layer, &0, *build);
    }
    let mut window = Window::default();
    window.sync(view.built(None));
    window.lay_out(Size::new(300.0, 200.0), &mut crate::layout::tests::Mono::default());
    window
}

/// What a point hits, as text: its UI ID, `"blocked"`, or `"nothing"`.
fn hit_at(window: &Window, x: f32, y: f32) -> String {
    match window.hit(Point::new(x, y)) {
        Some((_, crate::Hit::Element(id))) => id.to_string(),
        Some((_, crate::Hit::Blocked)) => "blocked".to_string(),
        None => "nothing".to_string(),
    }
}

/// The clips the layers began, in order.
fn begins(recorder: &Recorder) -> Vec<Rect> {
    recorder
        .painted
        .iter()
        .filter_map(|piece| match piece {
            Painted::Begin(clip) => Some(*clip),
            _ => None,
        })
        .collect()
}

/// A slot with a border and padding holds its panel inside them, as CSS
/// puts a child in the content box: the panel is laid out at the content
/// size, painted inside it, and a point on the frame hits the parent.
#[test]
fn a_framed_slot_holds_its_panel_inside_the_frame() {
    fn parent(_: &u8) -> Element {
        Element::stack([Element::slot(panel("child"))
            .width(Length::Fixed(40.0))
            .height(Length::Fixed(30.0))
            .border(2.0, BLUE)
            .padding(3.0)])
        .key("frame")
        .align_x(crate::Align::Start)
        .align_y(crate::Align::Start)
    }
    fn child(_: &u8) -> Element {
        Element::space(Length::FILL, Length::FILL).key("inside")
    }
    let window = window_of(&[("parent", Layer::Panels, parent), ("child", Layer::Panels, child)]);
    assert_eq!(
        window.bounds(&panel("child").with("inside")),
        Some(rect(5.0, 5.0, 30.0, 20.0))
    );
    let mut recorder = Recorder::default();
    window.paint(Layer::Panels, &mut recorder, &PANEL, None, (None, None), 1.0);
    assert_eq!(
        begins(&recorder)[1],
        rect(5.0, 5.0, 30.0, 20.0),
        "painted inside the frame"
    );
    assert_eq!(hit_at(&window, 1.0, 10.0), "parent/frame", "the border is the parent's");
    assert_eq!(hit_at(&window, 4.0, 10.0), "parent/frame", "so is the padding");
    assert_eq!(hit_at(&window, 6.0, 10.0), "child/inside");
}

/// A shrink slot with a border is as large as its panel plus the border,
/// and the panel is laid out at its own size, not the slot's.
#[test]
fn a_shrink_slot_with_a_border_lays_its_panel_out_at_its_own_size() {
    fn parent(_: &u8) -> Element {
        Element::row([Element::slot(panel("child")).border(2.0, BLUE)]).align_y(crate::Align::Start)
    }
    fn child(_: &u8) -> Element {
        Element::space(Length::Fixed(20.0), Length::Fixed(10.0)).key("inside")
    }
    let window = window_of(&[("parent", Layer::Panels, parent), ("child", Layer::Panels, child)]);
    assert_eq!(
        window.bounds(&panel("child").with("inside")),
        Some(rect(2.0, 2.0, 20.0, 10.0))
    );
}

/// A slot inside a clipping element of its parent is cut by that clip too,
/// when painted and when hit, and the cut carries down to a panel in a slot
/// of that panel.
#[test]
fn a_slot_inside_a_clip_is_cut_by_it_two_levels_down() {
    fn parent(_: &u8) -> Element {
        Element::column([Element::row([Element::slot(panel("child"))
            .width(Length::Fixed(40.0))
            .height(Length::Fixed(20.0))
            .min_width(40.0)])
        .width(Length::Fixed(30.0))
        .min_width(0.0)
        .clip()])
        .align_x(crate::Align::Start)
    }
    fn child(_: &u8) -> Element {
        Element::row([Element::slot(panel("grandchild"))
            .width(Length::Fixed(40.0))
            .height(Length::Fixed(20.0))])
        .align_y(crate::Align::Start)
    }
    fn grandchild(_: &u8) -> Element {
        Element::space(Length::FILL, Length::FILL).key("deep")
    }
    let window = window_of(&[
        ("parent", Layer::Panels, parent),
        ("child", Layer::Panels, child),
        ("grandchild", Layer::Panels, grandchild),
    ]);
    let mut recorder = Recorder::default();
    window.paint(Layer::Panels, &mut recorder, &PANEL, None, (None, None), 1.0);
    let cut = rect(0.0, 0.0, 30.0, 20.0);
    let begins = begins(&recorder);
    // The parent, its row's clip, the child, and the grandchild.
    assert_eq!(begins.len(), 4, "{begins:?}");
    assert_eq!(begins[2], cut, "the child is cut by the row around its slot");
    assert_eq!(begins[3], cut, "and so is the grandchild, a level further down");
    assert_eq!(hit_at(&window, 20.0, 10.0), "grandchild/deep", "inside the clip");
    assert_eq!(
        hit_at(&window, 35.0, 10.0),
        "nothing",
        "inside the slots, past the clip"
    );
}

/// A wrapping text is wrapped at the width it was measured at, before
/// rounding, so painting cannot break a line the measure kept whole.
#[test]
fn a_wrapping_text_wraps_at_its_measured_width() {
    let leaf = Element::text("ab cd", STYLE)
        .wrap()
        .width(Length::Fixed(40.4))
        .min_width(0.0)
        .padding(0.3);
    let painted = painted(leaf, Point::ORIGIN);
    let [Painted::Text { run, .. }] = painted.as_slice() else {
        panic!("one text: {painted:?}");
    };
    assert!(run.wrap, "the run wraps");
    assert!(
        (run.bounds.width - 39.8).abs() < 1e-4,
        "40.4 less two paddings of 0.3, not the rounded 40: {}",
        run.bounds.width
    );
}

/// Text sits at its content edge as taffy rounds it: a fractional padding
/// or border is not added on its own, so the text is where a child at that
/// edge would be, on a whole pixel.
#[test]
fn text_sits_on_the_rounded_content_edge() {
    let padded = painted(Element::text("ab", STYLE).padding(2.5), Point::ORIGIN);
    let [Painted::Text { run, .. }] = padded.as_slice() else {
        panic!("one text: {padded:?}");
    };
    assert_eq!(run.anchor, Point::new(3.0, 3.0), "2.5 rounds to the content edge");
    let framed = painted(
        Element::column([Element::text("ab", STYLE)])
            .border(1.5, BLUE)
            .padding(1.0),
        Point::ORIGIN,
    );
    let text = framed
        .iter()
        .find_map(|piece| match piece {
            Painted::Text { run, .. } => Some(*run),
            _ => None,
        })
        .expect("the text is painted");
    assert_eq!(text.anchor, Point::new(3.0, 3.0), "1.5 and 1 make 2.5, rounded once");
}

/// A fill for each state, and a text colour, so a test can tell which
/// look painted.
fn looks() -> crate::element::Looks {
    use crate::element::StateLook;
    let state = |grey: f32| StateLook {
        background: Some(Color::rgb(grey, grey, grey)),
        text: Some(Color::rgb(grey, 0.0, 0.0)),
    };
    crate::element::Looks {
        normal: state(0.1),
        hovered: state(0.2),
        pressed: state(0.3),
        off: state(0.4),
    }
}

/// A button keyed `b` with `looks()`, its label a child text, turned off
/// when `off`.
fn button_with_looks(off: bool) -> Element {
    use crate::access::{Access, Action, Role};
    let access = if off {
        Access::new(Role::Button, "b").states(crate::access::States {
            disabled: true,
            ..Default::default()
        })
    } else {
        Access::new(Role::Button, "b").action(Action::Click)
    };
    Element::row([Element::text("b", STYLE)])
        .key("b")
        .looks(looks())
        .access(access)
}

/// The fill of the first box and the colour of the first text that
/// `root` paints, with `hovered` and `pressed` (keys under the panel `p`),
/// or with no state at all when `live` is false.
fn painted_state(root: Element, hovered: Option<&str>, pressed: Option<&str>, live: bool) -> (Color, Color) {
    let layout = lay_out(root, 200.0, 100.0);
    let fields = crate::field::Fields::default();
    let (hovered, pressed) = (
        hovered.map(crate::layout::tests::id),
        pressed.map(crate::layout::tests::id),
    );
    let state = crate::draw::PaintState {
        fields: &fields,
        focus: None,
        hovered: hovered.as_ref(),
        pressed: pressed.as_ref(),
        scale: 1.0,
    };
    let mut recorder = Recorder::default();
    layout.paint(
        Point::ORIGIN,
        rect(0.0, 0.0, 1000.0, 1000.0),
        &mut recorder,
        &PANEL,
        live.then_some(&state),
    );
    let fill = recorder
        .painted
        .iter()
        .find_map(|painted| match painted {
            Painted::Quad(quad) => Some(quad.fill),
            _ => None,
        })
        .expect("a box");
    let text = recorder
        .painted
        .iter()
        .find_map(|painted| match painted {
            Painted::Text { run, .. } => Some(run.colour),
            _ => None,
        })
        .expect("a text");
    (fill, text)
}

/// An element with looks paints the look of its state, chosen when it is
/// painted: normal, hovered under the pointer, pressed while a press is
/// held over it, and the text inside takes the state's colour
/// (visual-system.md, Role tokens).
#[test]
fn an_element_paints_the_look_of_its_state() {
    let grey = |g: f32| (Color::rgb(g, g, g), Color::rgb(g, 0.0, 0.0));
    assert_eq!(painted_state(button_with_looks(false), None, None, true), grey(0.1));
    assert_eq!(
        painted_state(button_with_looks(false), Some("b"), None, true),
        grey(0.2)
    );
    assert_eq!(
        painted_state(button_with_looks(false), Some("b"), Some("b"), true),
        grey(0.3)
    );
    assert_eq!(
        painted_state(button_with_looks(false), Some("other"), Some("other"), true),
        grey(0.1),
        "another element's hover and press leave it normal"
    );
}

/// Off wins over everything: a turned-off button hovered and pressed
/// still paints its off look.
#[test]
fn off_wins_over_hover_and_press() {
    let off = (Color::rgb(0.4, 0.4, 0.4), Color::rgb(0.4, 0.0, 0.0));
    assert_eq!(painted_state(button_with_looks(true), None, None, true), off);
    assert_eq!(painted_state(button_with_looks(true), Some("b"), Some("b"), true), off);
}

/// Text with a colour of its own keeps it inside an element with looks,
/// and a layout painted with no state at all, as hover text is, paints
/// the normal look.
#[test]
fn own_text_colour_wins_and_no_state_is_normal() {
    let root = Element::row([Element::text("b", STYLE).colour(RED)])
        .key("b")
        .looks(looks())
        .access(crate::access::Access::new(crate::access::Role::Button, "b").action(crate::access::Action::Click));
    assert_eq!(painted_state(root, Some("b"), None, true).1, RED);
    assert_eq!(
        painted_state(button_with_looks(false), Some("b"), Some("b"), false),
        (Color::rgb(0.1, 0.1, 0.1), Color::rgb(0.1, 0.0, 0.0))
    );
}

/// Section 5, rule 15: an element's shadow paints first, under its box,
/// moved by its offset, with the box's corners and the token's colour and
/// blur.
#[test]
fn a_shadow_paints_under_its_box_moved_by_its_offset() {
    let shadow = crate::element::ShadowStyle {
        colour: Color::rgba(0.0, 0.0, 0.0, 0.35),
        offset: loom_ui_core::Vector::new(0.0, 4.0),
        blur: 12.0,
    };
    let root = Element::column([Element::text("ab", STYLE)])
        .background(RED)
        .radius(6.0)
        .padding(4.0)
        .shadow(shadow);
    let painted = painted(root, Point::new(10.0, 20.0));
    let [Painted::Shadow(cast), Painted::Quad(quad), Painted::Text { .. }] = painted.as_slice() else {
        panic!("the shadow, the box, then the text: {painted:?}");
    };
    assert_eq!(quad.rect, rect(10.0, 20.0, 24.0, 24.0));
    assert_eq!(cast.rect, rect(10.0, 24.0, 24.0, 24.0), "moved by its offset");
    assert_eq!((cast.radius, cast.colour, cast.blur), (6.0, shadow.colour, 12.0));
}

/// A grey the placeholder tests set, unlike the panel's text colour.
const HINT: Color = Color::rgb(0.6, 0.6, 0.6);

/// What a window with one search field showing `text`, with a placeholder,
/// paints in the panels layer, after the input method composes `composing`
/// in it, when that is given.
fn search_field(text: &str) -> Vec<Painted> {
    search_field_composing(text, None)
}

/// [`search_field`], with `composing` given to the input method first.
fn search_field_composing(text: &str, composing: Option<&str>) -> Vec<Painted> {
    use crate::access::{Access, Role};
    let mut window = Window::default();
    let field = Element::field(text, STYLE)
        .key("search")
        .width(Length::Fixed(200.0))
        .placeholder("Search modules", HINT)
        .access(Access::new(Role::TextInput, "Search modules"));
    window.sync(vec![(
        UiId::panel("browser"),
        Layer::Panels,
        Some(Element::column([field])),
    )]);
    window.lay_out(Size::new(400.0, 100.0), &mut crate::layout::tests::Mono::default());
    if let Some(composing) = composing {
        let preedit = loom_ui_core::ImeEvent::Preedit {
            text: composing.to_owned(),
            cursor: Some((composing.len(), composing.len())),
        };
        let id = UiId::panel("browser").with("search");
        window.fields.ime(&id, &preedit, std::time::Instant::now());
    }
    let mut recorder = Recorder::default();
    window.paint(Layer::Panels, &mut recorder, &PANEL, None, (None, None), 1.0);
    recorder.painted
}

/// While the input method composes in an empty field, the field shows the
/// composition and no placeholder under it (ui-layer.md, section 8,
/// rule 4).
#[test]
fn a_composition_hides_the_placeholder() {
    let painted = search_field_composing("", Some("にほ"));
    assert!(
        painted.iter().all(|piece| piece.as_text() != Some("Search modules")),
        "{painted:?}"
    );
    assert!(
        painted.iter().any(|piece| piece.as_text() == Some("にほ")),
        "{painted:?}"
    );
}

/// An empty field paints its placeholder in its own colour, before the
/// field's own text, so the caret comes after both; a field with text
/// paints no placeholder.
#[test]
fn an_empty_field_shows_its_placeholder() {
    let painted = search_field("");
    let texts: Vec<(&str, Color)> = painted
        .iter()
        .filter_map(|piece| match piece {
            Painted::Text { text, run } => Some((text.as_str(), run.colour)),
            _ => None,
        })
        .collect();
    assert_eq!(texts, [("Search modules", HINT), ("", PANEL.text)]);
    let typed = search_field("saw");
    assert!(
        typed.iter().all(|piece| piece.as_text() != Some("Search modules")),
        "no placeholder over typed text: {typed:?}"
    );
}
