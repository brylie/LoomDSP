//! Tests for `window.rs`: panels in slots, what each kind of change lays
//! out again, bounds and hits across panels, and slot mistakes.

use super::Window;
use crate::element::{Element, Length};
use crate::hit::Hit;
use crate::layout::tests::{Mono, STYLE};
use crate::view::{Layer, View};
use loom_ui_core::{Point, Rect, Size, UiId};

/// The window panel's model: where its slots are, and a number that
/// changes the model and nothing it shows.
#[derive(Debug, Clone, PartialEq)]
struct Frame {
    /// Space above the toolbar's slot, which moves it without resizing it.
    gap: f32,
    /// The side panel's slot width.
    side: f32,
    /// Changes the model only.
    revision: u32,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            gap: 0.0,
            side: 80.0,
            revision: 0,
        }
    }
}

/// A key's panel ID.
fn panel(key: &str) -> UiId {
    UiId::panel(key)
}

/// The window: a toolbar slot that fills across, 30 high; below it, a side
/// slot of a fixed width that fills down, then a status slot that shrinks
/// to its panel.
fn frame(model: &Frame) -> Element {
    Element::column([
        Element::space(Length::FILL, Length::Fixed(model.gap)),
        Element::slot(panel("toolbar"))
            .width(Length::FILL)
            .height(Length::Fixed(30.0)),
        Element::row([
            Element::slot(panel("side"))
                .width(Length::Fixed(model.side))
                .height(Length::FILL),
            Element::slot(panel("status")),
        ])
        .height(Length::FILL),
    ])
    .width(Length::FILL)
    .height(Length::FILL)
}

/// A panel showing one label, keyed `label`, filling its place.
#[expect(clippy::ptr_arg, reason = "a builder takes a reference to its model, here a String")]
fn label(text: &String) -> Element {
    Element::row([Element::text(text.clone(), STYLE).key("label")])
        .width(Length::FILL)
        .height(Length::FILL)
}

/// A panel that is its label's size.
#[expect(clippy::ptr_arg, reason = "a builder takes a reference to its model, here a String")]
fn shrink_label(text: &String) -> Element {
    Element::row([Element::text(text.clone(), STYLE).key("label")])
}

/// What the window shows: its frame, and the text of each panel in it.
#[derive(Clone)]
struct Shown {
    frame: Frame,
    toolbar: String,
    side: String,
    status: String,
}

impl Default for Shown {
    fn default() -> Self {
        Self {
            frame: Frame::default(),
            toolbar: "Open".to_string(),
            side: "Side".to_string(),
            status: "Ready".to_string(),
        }
    }
}

/// A window, the view it was last given, and a text measure.
struct Screen {
    window: Window,
    last: Option<View>,
    mono: Mono,
}

impl Screen {
    /// An empty window.
    fn new() -> Self {
        Self {
            window: Window::default(),
            last: None,
            mono: Mono::default(),
        }
    }

    /// Gives the window `view`, as a turn does.
    fn show_view(&mut self, view: View) {
        self.window.sync(view.built(self.last.as_ref()));
        self.last = Some(view);
    }

    /// Gives the window the panels of `shown`, and lays it out at 300 by
    /// 200.
    fn show(&mut self, shown: &Shown) {
        let mut view = View::new();
        view.panel_with(panel("window"), Layer::Panels, &shown.frame, frame);
        view.panel_with(panel("toolbar"), Layer::Panels, &shown.toolbar, label);
        view.panel_with(panel("side"), Layer::Panels, &shown.side, label);
        view.panel_with(panel("status"), Layer::Panels, &shown.status, shrink_label);
        self.show_view(view);
        self.lay_out(Size::new(300.0, 200.0));
    }

    /// Lays the window out at `size`.
    fn lay_out(&mut self, size: Size) {
        self.window.lay_out(size, &mut self.mono);
    }

    /// How many times each panel was laid out: window, toolbar, side,
    /// status.
    fn computes(&self) -> [u64; 4] {
        ["window", "toolbar", "side", "status"].map(|key| self.window.computes(&panel(key)))
    }

    /// The window bounds of the UI ID written `id`.
    fn at(&self, id: &str) -> Option<Rect> {
        self.window.bounds(&id.parse().expect("a UI ID"))
    }
}

/// A rectangle from its four numbers.
fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect::new(Point::new(x, y), Size::new(width, height))
}

/// Each panel is laid out in its slot, and its parts' bounds are in window
/// coordinates. A panel's own key gives its whole place.
#[test]
fn panels_are_laid_out_in_their_slots() {
    let mut screen = Screen::new();
    screen.show(&Shown::default());
    assert_eq!(screen.at("window"), Some(rect(0.0, 0.0, 300.0, 200.0)));
    assert_eq!(screen.at("toolbar"), Some(rect(0.0, 0.0, 300.0, 30.0)));
    assert_eq!(screen.at("side"), Some(rect(0.0, 30.0, 80.0, 170.0)));
    // "Ready" is 40 pixels at 8 a character; the status slot shrinks to it
    // across and stretches down the row. In it, the panel's row shrinks to
    // the label's line.
    assert_eq!(screen.at("status"), Some(rect(80.0, 30.0, 40.0, 170.0)));
    assert_eq!(screen.at("status/label"), Some(rect(80.0, 30.0, 40.0, 16.0)));
    assert_eq!(
        screen.at("side/label").map(|at| at.position()),
        Some(Point::new(0.0, 30.0))
    );
    assert_eq!(screen.at("nowhere"), None);
    assert_eq!(screen.at("toolbar/nothing"), None);
    assert!(screen.window.is_settled(Size::new(300.0, 200.0)));
}

/// When only the parent rebuilds and a slot keeps its size, the panel in it
/// is not laid out again, and its bounds move with the slot.
#[test]
fn a_parent_rebuild_moves_a_child_without_laying_it_out() {
    let mut screen = Screen::new();
    let mut shown = Shown::default();
    screen.show(&shown);
    let before = screen.computes();

    shown.frame.revision = 1;
    screen.show(&shown);
    let after = screen.computes();
    assert_eq!(after[0], before[0] + 1, "the window is laid out again");
    assert_eq!(after[1..], before[1..], "nothing in a slot is");

    shown.frame.gap = 10.0;
    screen.show(&shown);
    let moved = screen.computes();
    assert_eq!(moved[1], before[1], "the toolbar's slot moved but kept its size");
    assert_eq!(screen.at("toolbar/label").map(|at| at.y), Some(10.0));
}

/// When a slot's size changes, the panel in it is laid out again, with no
/// rebuild; on a resize, every panel is.
#[test]
fn a_resized_slot_lays_its_child_out_again() {
    let mut screen = Screen::new();
    let mut shown = Shown::default();
    screen.show(&shown);
    let before = screen.computes();

    shown.frame.side = 120.0;
    screen.show(&shown);
    let after = screen.computes();
    assert_eq!(after[2], before[2] + 1);
    assert_eq!(after[1], before[1], "the toolbar kept its size");
    assert_eq!(screen.at("side").map(|at| at.width), Some(120.0));

    // A resize, with no new view: each panel is laid out again.
    screen.lay_out(Size::new(400.0, 300.0));
    let resized = screen.computes();
    assert_eq!(resized[..3], [after[0] + 1, after[1] + 1, after[2] + 1]);
    assert_eq!(screen.at("toolbar").map(|at| at.width), Some(400.0));
    assert!(!screen.window.is_settled(Size::new(300.0, 200.0)));
}

/// When only a child in a filling or fixed slot rebuilds, it is laid out
/// again and its parent is not.
#[test]
fn a_child_rebuild_in_a_fill_slot_leaves_the_parent() {
    let mut screen = Screen::new();
    let mut shown = Shown::default();
    screen.show(&shown);
    let before = screen.computes();
    shown.toolbar = "Open and save".to_string();
    screen.show(&shown);
    let after = screen.computes();
    assert_eq!(after[0], before[0]);
    assert_eq!(after[1], before[1] + 1);
}

/// A filling slot is sized by its parent alone: a wide panel in it does not
/// widen it.
#[test]
fn a_fill_slot_ignores_its_child() {
    let mut screen = Screen::new();
    let shown = Shown {
        toolbar: "x".repeat(100),
        ..Shown::default()
    };
    screen.show(&shown);
    assert_eq!(screen.at("toolbar").map(|at| at.width), Some(300.0));
    assert_eq!(screen.at("toolbar/label").map(|at| at.width), Some(800.0));
}

/// When only a child in a shrink slot rebuilds at the same size, its
/// parent is not laid out again; at a new size, the parent is, with no
/// rebuild, and the slot follows.
#[test]
fn a_child_rebuild_in_a_shrink_slot_lays_out_the_parent_only_on_a_new_size() {
    let mut screen = Screen::new();
    let mut shown = Shown::default();
    screen.show(&shown);
    let before = screen.computes();

    shown.status = "Saved".to_string();
    screen.show(&shown);
    let same = screen.computes();
    assert_eq!(same[0], before[0], "the same size leaves the window alone");
    assert!(same[3] > before[3]);

    shown.status = "Saving".to_string();
    screen.show(&shown);
    let grown = screen.computes();
    assert_eq!(grown[0], same[0] + 1, "a new size lays the window out again");
    assert_eq!(screen.at("status").map(|at| at.width), Some(48.0));
}

/// Panels are hit in the reverse of draw order: a panel in a slot is over
/// all of its parent's elements, even those after the slot; a later root of
/// a layer is over an earlier one and what is in its slots; and a higher
/// layer is over a lower one.
#[test]
fn hits_follow_draw_order() {
    /// A stack: the slot, then a keyed element over it.
    fn covered(_: &u8) -> Element {
        Element::stack([
            Element::slot(panel("child"))
                .width(Length::Fixed(50.0))
                .height(Length::Fixed(50.0)),
            Element::space(Length::Fixed(50.0), Length::Fixed(50.0)).key("cover"),
        ])
    }
    /// A keyed square of `size`.
    fn square(size: &u8) -> Element {
        let size = f32::from(*size);
        Element::space(Length::Fixed(size), Length::Fixed(size)).key("square")
    }
    let mut screen = Screen::new();
    let mut view = View::new();
    view.panel_with(panel("parent"), Layer::Panels, &0, covered);
    view.panel_with(panel("child"), Layer::Panels, &40, square);
    view.panel_with(panel("later"), Layer::Panels, &20, square);
    view.panel_with(panel("menu"), Layer::Overlays, &10, square);
    screen.show_view(view);
    screen.lay_out(Size::new(300.0, 200.0));

    let hit = |x, y| match screen.window.hit(Point::new(x, y)) {
        Some((layer, Hit::Element(id))) => Some((layer, id.to_string())),
        Some((layer, Hit::Blocked)) => Some((layer, "blocked".to_string())),
        None => None,
    };
    assert_eq!(hit(5.0, 5.0), Some((Layer::Overlays, "menu/square".to_string())));
    assert_eq!(hit(15.0, 15.0), Some((Layer::Panels, "later/square".to_string())));
    assert_eq!(hit(30.0, 30.0), Some((Layer::Panels, "child/square".to_string())));
    assert_eq!(hit(45.0, 45.0), Some((Layer::Panels, "parent/cover".to_string())));
    assert_eq!(hit(200.0, 150.0), None);
}

/// A panel the view no longer has is gone, and an unchanged view lays out
/// nothing.
#[test]
fn a_removed_panel_is_gone() {
    let mut screen = Screen::new();
    screen.show(&Shown::default());
    let mut view = View::new();
    view.panel_with(panel("window"), Layer::Panels, &Frame::default(), frame);
    screen.show_view(view);
    screen.lay_out(Size::new(300.0, 200.0));
    assert_eq!(screen.at("toolbar"), None);
    assert_eq!(screen.window.computes(&panel("toolbar")), 0);
    assert!(screen.at("window").is_some());
}

/// A view with the slot mistakes `mistake` names: a slot for a panel in
/// another layer, a panel in two slots, or two panels in each other's
/// slots.
fn mistaken(mistake: &str) -> View {
    /// A slot for the panel `b`.
    fn holds_b(_: &u8) -> Element {
        Element::row([Element::slot(panel("b"))
            .width(Length::Fixed(10.0))
            .height(Length::Fixed(10.0))])
    }
    /// A slot for the panel `a`.
    fn holds_a(_: &u8) -> Element {
        Element::row([Element::slot(panel("a"))
            .width(Length::Fixed(10.0))
            .height(Length::Fixed(10.0))])
    }
    /// A keyed square.
    fn square(_: &u8) -> Element {
        Element::space(Length::Fixed(5.0), Length::Fixed(5.0)).key("square")
    }
    let mut view = View::new();
    match mistake {
        "layer" => {
            view.panel_with(panel("a"), Layer::Panels, &0, holds_b);
            view.panel_with(panel("b"), Layer::Overlays, &0, square);
        }
        "twice" => {
            view.panel_with(panel("a"), Layer::Panels, &0, holds_b);
            view.panel_with(panel("c"), Layer::Panels, &0, holds_b);
            view.panel_with(panel("b"), Layer::Panels, &0, square);
        }
        _ => {
            view.panel_with(panel("a"), Layer::Panels, &0, holds_b);
            view.panel_with(panel("b"), Layer::Panels, &0, holds_a);
        }
    }
    view
}

/// A slot for a panel in another layer is a mistake.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "panel a has a slot for b, which is in another layer")]
fn a_slot_for_another_layer_panics_in_a_debug_build() {
    Screen::new().show_view(mistaken("layer"));
}

/// One panel in two slots is a mistake.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "panel b is in two slots")]
fn a_panel_in_two_slots_panics_in_a_debug_build() {
    Screen::new().show_view(mistaken("twice"));
}

/// A slot that holds its own ancestor is a mistake.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "the slots of panel a hold its own ancestor")]
fn a_slot_holding_an_ancestor_panics_in_a_debug_build() {
    Screen::new().show_view(mistaken("loop"));
}

/// In a release build, the first slot in view order wins: a panel in
/// another layer stays a root there, a panel in two slots is in the first,
/// and the first panel of a loop becomes a root.
#[test]
fn slot_mistakes_in_a_release_build_keep_the_first() {
    let place = |mistake: &str, id: &str| {
        let mut screen = Screen::new();
        screen.window.allow_mistakes();
        screen.show_view(mistaken(mistake));
        screen.lay_out(Size::new(100.0, 100.0));
        screen.at(id).map(|at| at.position())
    };
    assert_eq!(place("layer", "b/square"), Some(Point::ORIGIN));
    // `a`'s slot comes first, at its top left; `c`'s stays empty.
    assert_eq!(place("twice", "b/square"), Some(Point::ORIGIN));
    // `a` becomes a root; `b` is in its slot, and `a` in `b`'s is not.
    assert_eq!(place("loop", "b"), Some(Point::ORIGIN));
    assert_eq!(place("loop", "a"), Some(Point::ORIGIN));
}

/// A row: a shrink slot for the panel `child`, then a keyed 10-pixel
/// square, `after`, whose place shows the slot's width.
fn holder(_: &u8) -> Element {
    Element::row([
        Element::slot(panel("child")),
        Element::space(Length::Fixed(10.0), Length::Fixed(10.0)).key("after"),
    ])
}

/// A row: a shrink slot for the panel `grandchild`.
fn middle(_: &u8) -> Element {
    Element::row([Element::slot(panel("grandchild"))])
}

/// A view of `holder`, and, with `child` text, the child panel in its slot.
fn holding(child: Option<&String>) -> View {
    let mut view = View::new();
    view.panel_with(panel("holder"), Layer::Panels, &0, holder);
    if let Some(text) = child {
        view.panel_with(panel("child"), Layer::Panels, text, shrink_label);
    }
    view
}

/// A panel that leaves or comes into a shrink slot changes its parent's
/// layout, though the parent did not rebuild.
#[test]
fn a_child_coming_or_going_lays_out_its_parent_again() {
    let mut screen = Screen::new();
    let text = "Ready".to_string();
    screen.show_view(holding(Some(&text)));
    screen.lay_out(Size::new(300.0, 100.0));
    assert_eq!(screen.at("holder/after").map(|at| at.x), Some(40.0));

    screen.show_view(holding(None));
    assert!(!screen.window.is_settled(Size::new(300.0, 100.0)));
    screen.lay_out(Size::new(300.0, 100.0));
    assert_eq!(screen.at("holder/after").map(|at| at.x), Some(0.0));

    screen.show_view(holding(Some(&text)));
    screen.lay_out(Size::new(300.0, 100.0));
    assert_eq!(screen.at("holder/after").map(|at| at.x), Some(40.0));
}

/// A change in size moves up through every enclosing shrink slot: in
/// holder, child, grandchild, all in shrink slots, a wider grandchild
/// widens the child's slot in the holder.
#[test]
fn a_new_size_moves_up_through_shrink_slots() {
    let view = |text: &String| {
        let mut view = View::new();
        view.panel_with(panel("holder"), Layer::Panels, &0, holder);
        view.panel_with(panel("child"), Layer::Panels, &0, middle);
        view.panel_with(panel("grandchild"), Layer::Panels, text, shrink_label);
        view
    };
    let mut screen = Screen::new();
    screen.show_view(view(&"Ready".to_string()));
    screen.lay_out(Size::new(300.0, 100.0));
    assert_eq!(screen.at("holder/after").map(|at| at.x), Some(40.0));

    screen.show_view(view(&"Ready now".to_string()));
    screen.lay_out(Size::new(300.0, 100.0));
    assert_eq!(screen.at("child").map(|at| at.width), Some(72.0));
    assert_eq!(screen.at("holder/after").map(|at| at.x), Some(72.0));
}

/// A new order of roots changes what is on top, so reads wait for the next
/// layout, and then the later root is hit.
#[test]
fn a_new_order_of_roots_waits_for_layout() {
    /// A keyed square of 20 pixels.
    fn square(_: &u8) -> Element {
        Element::space(Length::Fixed(20.0), Length::Fixed(20.0)).key("square")
    }
    let view = |first: &str, second: &str| {
        let mut view = View::new();
        view.panel_with(panel(first), Layer::Panels, &0, square);
        view.panel_with(panel(second), Layer::Panels, &0, square);
        view
    };
    let top = |screen: &Screen| match screen.window.hit(Point::new(5.0, 5.0)) {
        Some((_, Hit::Element(id))) => id.to_string(),
        other => format!("{other:?}"),
    };
    let mut screen = Screen::new();
    screen.show_view(view("a", "b"));
    screen.lay_out(Size::new(100.0, 100.0));
    assert_eq!(top(&screen), "b/square");

    screen.show_view(view("b", "a"));
    assert!(!screen.window.is_settled(Size::new(100.0, 100.0)));
    screen.lay_out(Size::new(100.0, 100.0));
    assert_eq!(top(&screen), "a/square");
}

/// In a release build, a panel one parent names in two slots is in the
/// first only: placed there, and not hit in the second.
#[test]
fn a_panel_named_twice_by_one_parent_is_in_the_first_slot() {
    /// Two fixed slots for the panel `child`, side by side.
    fn twice(_: &u8) -> Element {
        Element::row([
            Element::slot(panel("child"))
                .width(Length::Fixed(20.0))
                .height(Length::Fixed(10.0)),
            Element::slot(panel("child"))
                .width(Length::Fixed(20.0))
                .height(Length::Fixed(10.0)),
        ])
    }
    let mut screen = Screen::new();
    screen.window.allow_mistakes();
    let mut view = View::new();
    view.panel_with(panel("parent"), Layer::Panels, &0, twice);
    view.panel_with(panel("child"), Layer::Panels, &"Hi".to_string(), label);
    screen.show_view(view);
    screen.lay_out(Size::new(100.0, 100.0));
    assert_eq!(screen.at("child"), Some(rect(0.0, 0.0, 20.0, 10.0)));
    assert!(matches!(
        screen.window.hit(Point::new(25.0, 5.0)),
        None | Some((_, Hit::Blocked))
    ));
}
