//! Tests for focus among the runtime's own elements and the nodes it gives
//! a screen reader (ui-layer.md, sections 6 and 7), through the headless
//! harness: the order, Tab and Shift+Tab, a press, a focus effect, an
//! element that goes, and the nodes by name and role.

use super::FocusStep;
use crate::access::{Access, Action, Role, States, Value};
use crate::app::{App, Context, WindowSpec};
use crate::effect::{Effects, FocusTarget, Selection};
use crate::element::{Element, Length};
use crate::event::{AccessEvent, DriverEvent, WindowState};
use crate::focus::AccessNodes;
use crate::headless::Harness;
use crate::layout::tests::{Mono, STYLE};
use crate::routing::{NoTarget, Routed, Target};
use crate::view::{Layer, View};
use accesskit::{ActionRequest, NodeId, Role as KitRole};
use loom_ui_core::{Event, IdTable, Key, KeyEvent, Modifiers, Point, Rect, Size, UiId};
use std::time::Instant;

/// A panel: its key, its layer, and its elements.
type Panel = (UiId, Layer, Element);

/// What the test app does.
#[derive(Debug)]
enum Message {
    /// Show these panels.
    Show(Vec<Panel>),
    /// Focus the element with this UI ID.
    Focus(UiId),
    /// Show these panels and focus this element in the same turn.
    ShowAndFocus(Vec<Panel>, UiId),
    /// Make the nodes from the last layout and keep them.
    Describe,
    /// Show these panels, and make the nodes after the draw, as an app
    /// whose panels changed does (`Context::access_nodes`).
    ShowThenDescribe(Vec<Panel>),
    /// The runtime's focus moved here (`App::focus_moved`).
    Moved(Option<UiId>),
}

/// An app that shows whatever panels a test gives it, and turns a screen
/// reader's Focus action into a focus effect by its own node table, as
/// section 7, rule 6 says an app does.
#[derive(Default)]
struct Panels {
    panels: Vec<Panel>,
    ids: IdTable,
    nodes: Option<AccessNodes>,
    focused_in_update: Option<UiId>,
    /// The app asks to hear of focus moves (`App::focus_moved`).
    reports: bool,
    /// Every focus the runtime reported, in order.
    moves: Vec<Option<UiId>>,
    /// When the focus is reported on the first, the app moves it to the
    /// second.
    redirect: Option<(UiId, UiId)>,
}

impl App for Panels {
    type Message = Message;

    fn update(&mut self, message: Message, cx: &Context<'_, Message>) -> Effects<Message> {
        self.focused_in_update = cx.focused().cloned();
        match message {
            Message::Show(panels) => {
                self.panels = panels;
                Effects::none()
            }
            Message::Focus(id) => Effects::focus(id),
            Message::ShowAndFocus(panels, id) => {
                self.panels = panels;
                Effects::focus(id)
            }
            Message::Describe => {
                self.nodes = Some(cx.access_nodes(&mut self.ids));
                Effects::none()
            }
            Message::ShowThenDescribe(panels) => {
                self.panels = panels;
                Effects::after_draw(Message::Describe)
            }
            Message::Moved(focus) => {
                let redirect = self.redirect.clone().filter(|(from, _)| focus.as_ref() == Some(from));
                self.moves.push(focus);
                redirect.map_or_else(Effects::none, |(_, to)| Effects::focus(to))
            }
        }
    }

    fn view(&self, view: &mut View) {
        for (key, layer, element) in &self.panels {
            view.panel_with(key.clone(), *layer, element, Element::clone);
        }
    }

    fn window(&self) -> WindowSpec {
        WindowSpec {
            title: "Focus".into(),
            min_size: None,
        }
    }

    fn focus_moved(&self, focus: Option<&UiId>) -> Option<Message> {
        self.reports.then(|| Message::Moved(focus.cloned()))
    }

    fn accessibility_event(&self, event: &AccessEvent) -> Option<Message> {
        let AccessEvent::Action(request) = event else {
            return None;
        };
        let id = self.ids.id(request.target_node.0)?;
        (request.action == accesskit::Action::Focus).then(|| Message::Focus(id.clone()))
    }
}

/// A focus stop the fake measures at 8 pixels a character, 16 high. A
/// combo box, so a press focuses it: a button or a switch takes no focus
/// from a press (ui-layer.md, section 6, rule 3), and these tests are
/// about the focus a press, Tab, or an effect gives.
fn stop(key: &str) -> Element {
    Element::text(key, STYLE)
        .key(key)
        .focusable()
        .access(Access::new(Role::ComboBox, key).action(Action::Click))
}

/// A stop that is turned off.
fn off(key: &str) -> Element {
    Element::text(key, STYLE).key(key).focusable().access(
        Access::new(Role::ComboBox, key)
            .action(Action::Click)
            .description("Nothing to save")
            .states(States {
                disabled: true,
                ..States::default()
            }),
    )
}

/// A row of stops across the top of a panel.
fn row(keys: &[&str]) -> Element {
    Element::row(keys.iter().map(|key| stop(key))).width(Length::FILL)
}

/// `inner` at the bottom of a panel the window's height.
fn at_bottom(inner: Element) -> Element {
    Element::column([Element::space(Length::Shrink, Length::FILL), inner])
        .width(Length::FILL)
        .height(Length::FILL)
}

/// A panel in the panels layer.
fn panel(key: &str, element: Element) -> Panel {
    (UiId::panel(key), Layer::Panels, element)
}

/// The UI ID `<panel>/<key>`.
fn id(panel: &str, key: &str) -> UiId {
    UiId::panel(panel).with(key)
}

/// A harness showing `panels` in a 400 by 200 window, laid out.
fn showing(panels: Vec<Panel>) -> Harness<Panels, NoTarget> {
    let window = WindowState::new(Size::new(400.0, 200.0));
    let mut harness = Harness::new(Panels::default(), NoTarget, window, Instant::now());
    harness.send(Message::Show(panels));
    harness.lay_out(&mut Mono::default());
    harness
}

/// A Tab press with `modifiers`.
fn tab_with(modifiers: Modifiers) -> DriverEvent {
    DriverEvent::Input {
        event: Event::Key(KeyEvent::Pressed {
            key: Key::Tab,
            modifiers,
            repeat: false,
            text: None,
        }),
        pointer: None,
    }
}

/// Tab, or Shift+Tab with `back`.
fn tab(back: bool) -> DriverEvent {
    tab_with(Modifiers {
        shift: back,
        ..Modifiers::NONE
    })
}

/// Rule 2, as decided 2026-11-16: the order is what a reader sees, not what
/// is drawn. The status bar is listed and drawn first, at the bottom, and
/// the toolbar second, at the top, so the toolbar's stops come first; the
/// overlay's come before both, since its layer is on top.
#[test]
fn the_order_is_reading_order_not_draw_order() {
    let harness = showing(vec![
        panel("status", at_bottom(row(&["mute"]))),
        panel("toolbar", row(&["open", "save"])),
        (UiId::panel("menu"), Layer::Overlays, row(&["item"])),
    ]);
    assert_eq!(
        harness.runtime().focus_stops(),
        [
            id("menu", "item"),
            id("toolbar", "open"),
            id("toolbar", "save"),
            id("status", "mute"),
        ]
    );
}

/// Two root panels at the same height read left to right, and two that
/// are level in both read in the view's order.
#[test]
fn level_panels_read_left_to_right_then_in_view_order() {
    let right = Element::row([Element::space(Length::Fixed(200.0), Length::Shrink), stop("b")]);
    let harness = showing(vec![
        panel("right", right),
        panel("left", row(&["a"])),
        panel("same", row(&["c"])),
    ]);
    assert_eq!(
        harness.runtime().focus_stops(),
        [id("left", "a"), id("same", "c"), id("right", "b")]
    );
}

/// Brylie's decision, 2026-11-17: two first stops are level when they
/// overlap from top to bottom. A stop one pixel lower but further left
/// still reads first; a stop wholly below the others reads after them,
/// however far left it is.
#[test]
fn overlapping_first_stops_are_level() {
    let lower =
        |pixels: f32, key: &str| Element::column([Element::space(Length::Shrink, Length::Fixed(pixels)), stop(key)]);
    let right = Element::row([Element::space(Length::Fixed(200.0), Length::Shrink), stop("b")]);
    let harness = showing(vec![
        panel("right", right),
        panel("below", lower(100.0, "c")),
        panel("left", lower(1.0, "a")),
    ]);
    assert_eq!(
        harness.runtime().focus_stops(),
        [id("left", "a"), id("right", "b"), id("below", "c")]
    );
}

/// A panel in a slot is read at the slot's place in its parent's build
/// order: between the stops before and after the slot.
#[test]
fn a_slots_panel_is_read_at_the_slot() {
    let parent = Element::row([stop("before"), Element::slot(UiId::panel("child")), stop("after")]);
    let harness = showing(vec![panel("parent", parent), panel("child", row(&["inner"]))]);
    assert_eq!(
        harness.runtime().focus_stops(),
        [id("parent", "before"), id("child", "inner"), id("parent", "after")]
    );
}

/// From outside, a step forward enters at the first stop and a step back at
/// the last. Past either end is `PastEnd`, and a step needs no focus.
#[test]
fn a_step_enters_at_either_end_and_leaves_past_it() {
    let harness = showing(vec![panel("bar", row(&["a", "b"]))]);
    let runtime = harness.runtime();
    let (a, b) = (id("bar", "a"), id("bar", "b"));
    assert_eq!(runtime.focus_step(None, false), FocusStep::To(a.clone()));
    assert_eq!(runtime.focus_step(None, true), FocusStep::To(b.clone()));
    assert_eq!(runtime.focus_step(Some(&a), false), FocusStep::To(b.clone()));
    assert_eq!(runtime.focus_step(Some(&b), true), FocusStep::To(a.clone()));
    assert_eq!(runtime.focus_step(Some(&b), false), FocusStep::PastEnd);
    assert_eq!(runtime.focus_step(Some(&a), true), FocusStep::PastEnd);
    assert_eq!(
        runtime.focus_step(Some(&id("bar", "gone")), false),
        FocusStep::To(a),
        "a UI ID that is no stop comes in from outside"
    );
    assert_eq!(runtime.focused(), None, "a step moves nothing");
    let empty = showing(vec![panel("bar", Element::text("no stops", STYLE))]);
    assert_eq!(empty.runtime().focus_step(None, false), FocusStep::PastEnd);
}

/// Rule 3, through the harness: a press focuses the stop under it, or the
/// stop around what is under it, and shows no ring.
#[test]
fn a_press_focuses_with_no_ring() {
    let holder = Element::stack([Element::text("label", STYLE).key("label")])
        .key("holder")
        .focusable()
        .access(Access::new(Role::Group, "Holder"));
    let mut harness = showing(vec![panel("bar", Element::row([stop("a"), holder]))]);
    assert_eq!(harness.focus_at(Point::new(2.0, 2.0)), Some(id("bar", "a")));
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "a")));
    assert_eq!(harness.runtime().focus_ring(), None, "a press shows no ring");
    assert_eq!(
        harness.focus_at(Point::new(10.0, 2.0)),
        Some(id("bar", "holder")),
        "a press on a part with no stop focuses the stop around it"
    );
    assert_eq!(harness.focus_at(Point::new(300.0, 100.0)), None);
    assert_eq!(harness.runtime().focused(), None, "a press on no stop clears the focus");
}

/// Rule 3, through the harness: Tab moves the focus one stop and shows a
/// ring on its bounds; Shift+Tab moves back.
#[test]
fn tab_moves_the_focus_with_a_ring() {
    let mut harness = showing(vec![panel("bar", row(&["a", "b"]))]);
    harness.focus_at(Point::new(2.0, 2.0));
    let next = harness.step(tab(false));
    assert!(next.draw, "the ring moved, so the window draws");
    assert_eq!(
        harness.runtime().focus_ring(),
        Some((id("bar", "b"), Rect::new(Point::new(8.0, 0.0), Size::new(8.0, 16.0))))
    );
    harness.step(tab(true));
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "a")));
    assert!(harness.runtime().focus_ring().is_some(), "Shift+Tab shows a ring too");
}

/// Tab from the last stop moves the focus out, and asks for a redraw so
/// the ring does not stay on screen, though the key goes on to the target.
#[test]
fn tab_past_the_last_stop_clears_the_ring_and_redraws() {
    let mut harness = showing(vec![panel("bar", row(&["a"]))]);
    harness.send(Message::Focus(id("bar", "a")));
    assert!(harness.runtime().focus_ring().is_some());
    let next = harness.step(tab(false));
    assert!(next.draw, "the ring went, so the window draws");
    assert_eq!(harness.runtime().focus_ring(), None);
}

/// A press on one of the runtime's stops takes the focus from the target,
/// so only one element has it (rule 1).
#[test]
fn a_press_takes_the_focus_from_the_target() {
    let window = WindowState::new(Size::new(400.0, 200.0));
    let mut harness = Harness::new(Panels::default(), Toolkit::default(), window, Instant::now());
    harness.send(Message::Show(vec![panel("bar", row(&["a"]))]));
    harness.lay_out(&mut Mono::default());
    harness.send(Message::Focus(UiId::panel("inspector").with("name")));
    assert!(harness.runtime().target().focused.is_some());
    assert_eq!(harness.focus_at(Point::new(2.0, 2.0)), Some(id("bar", "a")));
    assert_eq!(harness.runtime().target().focused, None, "the target let go");
}

/// A focus effect on an element of the runtime's that is no stop, such as
/// a label, leaves the focus where it was after the layout.
#[test]
fn a_focus_effect_on_a_label_keeps_the_focus() {
    let bar = Element::row([stop("a"), Element::text("label", STYLE).key("label")]);
    let mut harness = showing(vec![panel("bar", bar)]);
    harness.send(Message::Focus(id("bar", "a")));
    harness.send(Message::Focus(id("bar", "label")));
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "a")));
    harness.lay_out(&mut Mono::default());
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "a")));
}

/// A focus effect on a turned-off element of the runtime's, which is no
/// stop, leaves the focus where it was after the layout.
#[test]
fn a_focus_effect_on_a_turned_off_element_keeps_the_focus() {
    let mut harness = showing(vec![panel("bar", Element::row([stop("a"), off("save")]))]);
    harness.send(Message::Focus(id("bar", "a")));
    harness.send(Message::Focus(id("bar", "save")));
    harness.lay_out(&mut Mono::default());
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "a")));
}

/// In a release build, a focus stop with no Access is listed as a problem
/// and has no node, so the tree's focus is left empty rather than naming a
/// node that does not exist.
#[test]
fn the_focus_never_names_a_node_that_was_not_made() {
    let mut window = crate::window::Window::default();
    window.allow_mistakes();
    let bare = Element::text("bare", STYLE).key("bare").focusable();
    window.sync(vec![(UiId::panel("bar"), Layer::Panels, Some(Element::row([bare])))]);
    window.lay_out(Size::new(400.0, 200.0), &mut Mono::default());
    let focused = id("bar", "bare");
    assert_eq!(window.stops(), std::slice::from_ref(&focused));
    let nodes = window.access_nodes(&mut IdTable::new(), 1.0, Some(&focused));
    assert_eq!(nodes.focus, None);
    assert!(nodes.nodes.is_empty());
}

/// Tab past the last stop leaves the runtime's elements, for the router to
/// hand on, and the next Tab, with nothing focused in either toolkit,
/// enters the first stop again (rule 2, between the two toolkits). Ctrl+Tab
/// is not a step (rule 7).
#[test]
fn tab_past_the_end_and_ctrl_tab_leave_the_focus_alone() {
    let mut harness = showing(vec![panel("bar", row(&["a", "b"]))]);
    harness.step(tab(false));
    assert_eq!(
        harness.runtime().focused(),
        Some(&id("bar", "a")),
        "with nothing focused, Tab enters the first stop"
    );
    harness.focus_at(Point::new(10.0, 2.0));
    harness.step(tab_with(Modifiers {
        control: true,
        ..Modifiers::NONE
    }));
    assert_eq!(
        harness.runtime().focused(),
        Some(&id("bar", "b")),
        "Ctrl+Tab is no step"
    );
    harness.step(tab(false));
    assert_eq!(harness.runtime().focused(), None, "past the end, focus leaves");
    harness.step(tab(false));
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "a")), "and comes back");
}

/// A focus effect on one of the runtime's stops focuses it with a ring,
/// and the app reads the new focus in its next update.
#[test]
fn a_focus_effect_on_a_stop_focuses_it() {
    let mut harness = showing(vec![panel("bar", row(&["a", "b"]))]);
    harness.send(Message::Focus(id("bar", "b")));
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "b")));
    assert!(harness.runtime().focus_ring().is_some());
    harness.send(Message::Describe);
    assert_eq!(harness.app().focused_in_update, Some(id("bar", "b")));
}

/// A target that keeps the focus a toolkit's element has, and records each
/// focus effect it is given.
#[derive(Default)]
struct Toolkit {
    focused: Option<UiId>,
    told: Vec<FocusTarget>,
    /// The focus it had when each input event reached it.
    heard_with: Vec<Option<UiId>>,
}

impl Target<Message> for Toolkit {
    fn input(&mut self, _event: &Event, _pointer: Option<Point>, _now: Instant, _out: &mut Vec<Message>) -> Routed {
        self.heard_with.push(self.focused.clone());
        Routed::Passed
    }

    fn focus(&mut self, target: &FocusTarget, _select: Selection) {
        self.focused = match target {
            FocusTarget::Element(id) => Some(id.clone()),
            _ => None,
        };
        self.told.push(target.clone());
    }

    fn scroll_into_view(&mut self, _id: &UiId) {}

    fn focused(&self) -> Option<UiId> {
        self.focused.clone()
    }

    fn needs_redraw(&mut self, _updates: usize) -> bool {
        false
    }
}

/// A focus effect on an element of a panel opened in the same turn cannot
/// be told from a toolkit's yet, so the target hears it, as before. The
/// next layout finds it is a stop here, and takes the focus from the
/// target (rule 1). A focus effect that no stop has is the target's alone.
#[test]
fn a_focus_effect_waits_for_its_panel_to_be_laid_out() {
    let window = WindowState::new(Size::new(400.0, 200.0));
    let mut harness = Harness::new(Panels::default(), Toolkit::default(), window, Instant::now());
    harness.send(Message::ShowAndFocus(vec![panel("bar", row(&["a"]))], id("bar", "a")));
    assert_eq!(harness.runtime().focused(), None, "not laid out yet");
    harness.lay_out(&mut Mono::default());
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "a")));
    assert!(harness.runtime().focus_ring().is_some());
    assert_eq!(harness.runtime().target().focused, None, "the target let it go");

    let field = UiId::panel("inspector").with("name");
    harness.send(Message::Focus(field.clone()));
    harness.lay_out(&mut Mono::default());
    assert_eq!(harness.runtime().focused(), None);
    assert_eq!(harness.runtime().target().focused, Some(field));
}

/// A focus effect on a toolkit's element that no panel here ever lays out
/// stays the toolkit's: after the next layout the target still has it, and
/// nothing told it to let go.
#[test]
fn a_toolkit_focus_is_left_alone_after_the_layout() {
    let window = WindowState::new(Size::new(400.0, 200.0));
    let mut harness = Harness::new(Panels::default(), Toolkit::default(), window, Instant::now());
    harness.send(Message::Show(vec![panel("bar", row(&["a"]))]));
    harness.lay_out(&mut Mono::default());
    let field = UiId::panel("inspector").with("name");
    harness.send(Message::Focus(field.clone()));
    harness.lay_out(&mut Mono::default());
    harness.lay_out(&mut Mono::default());
    let target = harness.runtime().target();
    assert_eq!(target.focused, Some(field.clone()));
    assert_eq!(
        target.told,
        [FocusTarget::Element(field)],
        "told once, and never Nothing"
    );
    assert_eq!(harness.runtime().focused(), None);
}

/// Rule 2, as Brylie decided 2026-11-16: Tab passes over a turned-off
/// element, as macOS and Windows controls do. Its node stays in the tree
/// with its description, offering nothing, not even Focus.
#[test]
fn tab_passes_over_a_turned_off_element_and_its_node_stays() {
    let mut harness = showing(vec![panel("bar", Element::row([stop("a"), off("save"), stop("b")]))]);
    assert_eq!(harness.runtime().focus_stops(), [id("bar", "a"), id("bar", "b")]);
    harness.focus_at(Point::new(2.0, 2.0));
    harness.step(tab(false));
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "b")));
    assert_eq!(
        harness.focus_at(Point::new(10.0, 2.0)),
        None,
        "a press on it focuses nothing"
    );
    harness.send(Message::Describe);
    let nodes = harness.app().nodes.as_ref().expect("described");
    let (_, save) = nodes
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("save"))
        .expect("its node stays");
    assert!(save.is_disabled());
    assert_eq!(save.description(), Some("Nothing to save"));
    assert!(!save.supports_action(accesskit::Action::Focus));
    assert!(!save.supports_action(accesskit::Action::Click));
}

/// An element that is turned off while it has focus is no stop any more,
/// so focus falls back as for an element that went: to the first stop of
/// its panel.
#[test]
fn turning_off_the_focused_element_moves_the_focus_as_if_it_went() {
    let mut harness = showing(vec![panel("bar", row(&["a", "b"]))]);
    harness.focus_at(Point::new(10.0, 2.0));
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "b")));
    harness.send(Message::Show(vec![panel("bar", Element::row([stop("a"), off("b")]))]));
    harness.lay_out(&mut Mono::default());
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "a")));
}

/// Rule 4's last part: when the focused element goes, focus goes to the
/// first stop of the panel that held it, or to none when the panel goes.
#[test]
fn focus_falls_back_when_its_element_goes() {
    let mut harness = showing(vec![
        panel("bar", row(&["a", "b"])),
        panel("other", at_bottom(row(&["c"]))),
    ]);
    harness.focus_at(Point::new(10.0, 2.0));
    harness.send(Message::Show(vec![
        panel("bar", row(&["a"])),
        panel("other", at_bottom(row(&["c"]))),
    ]));
    harness.lay_out(&mut Mono::default());
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "a")));
    harness.send(Message::Show(vec![panel("other", at_bottom(row(&["c"])))]));
    harness.lay_out(&mut Mono::default());
    assert_eq!(harness.runtime().focused(), None, "its panel is gone");
}

/// A menu placed under an element that goes is not drawn, so its items
/// are no stops: the focus on one falls back as for an element that went,
/// and Tab never reaches them (section 5, rule 13; section 6, rule 4).
#[test]
fn a_placed_root_whose_anchor_goes_has_no_stops() {
    let menu = || {
        (
            UiId::panel("menu"),
            Layer::Overlays,
            row(&["m1", "m2"]).placed(crate::element::Placement::Under(id("bar", "b"))),
        )
    };
    let mut harness = showing(vec![panel("bar", row(&["a", "b"])), menu()]);
    harness.step(tab(false));
    assert_eq!(
        harness.runtime().focused(),
        Some(&id("menu", "m1")),
        "the menu reads first"
    );
    harness.send(Message::Show(vec![panel("bar", row(&["a"])), menu()]));
    harness.lay_out(&mut Mono::default());
    assert_eq!(harness.runtime().focused(), None, "the hidden item lost the focus");
    let mut reached = Vec::new();
    for _ in 0..4 {
        harness.step(tab(false));
        reached.extend(harness.runtime().focused().cloned());
    }
    assert!(reached.contains(&id("bar", "a")), "Tab still moves: {reached:?}");
    assert!(
        reached.iter().all(|stop| !stop.is_under(&UiId::panel("menu"))),
        "Tab never reaches the hidden menu: {reached:?}"
    );
}

/// The nodes, by name and role, nested as the elements are, with a slot's
/// panel inside the node around the slot; bounds in physical pixels; and
/// focus reported on the focused stop (section 7, rules 1, 5, and 8).
#[test]
fn the_nodes_come_from_the_elements() {
    let toolbar = Element::row([stop("open"), Element::slot(UiId::panel("tools"))])
        .key("bar")
        .access(Access::new(Role::Group, "Toolbar"));
    let volume = Element::space(Length::Fixed(40.0), Length::Fixed(10.0))
        .key("volume")
        .focusable()
        .access(Access::new(Role::Slider, "Volume").value(Value::Number {
            value: 7.0,
            min: 0.0,
            max: 10.0,
            step: Some(1.0),
        }));
    let window = WindowState {
        scale: 2.0,
        ..WindowState::new(Size::new(400.0, 200.0))
    };
    let mut harness = Harness::new(Panels::default(), NoTarget, window, Instant::now());
    harness.send(Message::Show(vec![
        panel("toolbar", toolbar),
        panel("tools", Element::row([volume])),
    ]));
    harness.lay_out(&mut Mono::default());
    harness.focus_at(Point::new(40.0, 2.0));
    harness.send(Message::Describe);

    let app = harness.app();
    let nodes = app.nodes.as_ref().expect("described");
    let find = |name: &str| {
        nodes
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(name))
            .unwrap_or_else(|| panic!("no node named {name}"))
    };
    let (group_id, group) = find("Toolbar");
    let (open_id, open) = find("open");
    let (volume_id, volume) = find("Volume");
    assert_eq!(group.role(), KitRole::Group);
    assert_eq!(open.role(), KitRole::ComboBox);
    assert_eq!(volume.role(), KitRole::Slider);
    assert_eq!(
        group.children(),
        [*open_id, *volume_id],
        "the slot's panel is inside the group"
    );
    assert_eq!(nodes.roots.len(), 1, "the slot's panel is no root");
    assert_eq!(nodes.roots[0].2, [*group_id]);
    let bounds = volume.bounds().expect("bounds");
    assert_eq!((bounds.x0, bounds.y0, bounds.x1, bounds.y1), (64.0, 0.0, 144.0, 20.0));
    assert_eq!(nodes.focus, Some(*volume_id));
    let position = |id: &NodeId| nodes.nodes.iter().position(|(kept, _)| kept == id);
    assert!(
        position(group_id) < position(volume_id),
        "a parent comes before its children"
    );
    assert_eq!(
        app.ids.id(volume_id.0),
        Some(&id("tools", "volume")),
        "IDs from the app's table"
    );
}

/// A screen reader's Focus action on one of the nodes reaches the app as
/// any action does; the app turns it into a focus effect by the UI ID its
/// table gives, and the focus moves (section 7, rules 6 and 20).
#[test]
fn a_focus_action_moves_the_focus_through_the_app() {
    let mut harness = showing(vec![panel("bar", row(&["a", "b"]))]);
    harness.send(Message::Describe);
    let target = {
        let nodes = harness.app().nodes.as_ref().expect("described");
        nodes
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("b"))
            .map(|(id, _)| *id)
            .expect("b has a node")
    };
    harness.step(DriverEvent::Accessibility(AccessEvent::Action(ActionRequest {
        action: accesskit::Action::Focus,
        target_tree: accesskit::TreeId::ROOT,
        target_node: target,
        data: None,
    })));
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "b")));
}

/// Outside a runtime, a context has no nodes.
#[test]
fn a_context_outside_a_runtime_has_no_nodes() {
    let (sender, _queue) = std::sync::mpsc::channel::<()>();
    let waker = crate::waker::Waker::new(sender, std::sync::Arc::new(|| {}));
    let cx = Context::new(Instant::now(), &waker, None, WindowState::new(Size::new(10.0, 10.0)));
    let nodes = cx.access_nodes(&mut IdTable::new());
    assert!(nodes.nodes.is_empty() && nodes.roots.is_empty() && nodes.focus.is_none());
}

/// A press on a part of a stop that a clip cuts away focuses nothing, as it
/// hits nothing there: focus and hits walk the same way (section 5, rule 7).
/// The same holds for a slot's panel past its slot.
#[test]
fn a_press_outside_a_clip_focuses_nothing() {
    let wide = stop("aaaaaa").width(Length::Fixed(48.0)).min_width(48.0);
    let clipped = Element::row([wide]).width(Length::Fixed(20.0)).min_width(0.0).clip();
    let mut harness = showing(vec![panel("bar", clipped)]);
    assert_eq!(harness.focus_at(Point::new(2.0, 2.0)), Some(id("bar", "aaaaaa")));
    assert_eq!(harness.focus_at(Point::new(30.0, 2.0)), None, "cut away by the clip");

    let host = Element::row([Element::slot(UiId::panel("child"))
        .width(Length::Fixed(20.0))
        .height(Length::Fixed(16.0))
        .min_width(0.0)]);
    let child = Element::row([stop("bbbbbb").width(Length::Fixed(48.0)).min_width(48.0)]);
    let mut harness = showing(vec![panel("host", host), panel("child", child)]);
    assert_eq!(harness.focus_at(Point::new(2.0, 2.0)), Some(id("child", "bbbbbb")));
    assert_eq!(harness.focus_at(Point::new(30.0, 2.0)), None, "past the slot");
}

/// A layout through the driver's parts settles the focus as the runtime's
/// own does: a focus whose element went falls back, and a focus that waited
/// for its layout is taken. The target cannot be reached through the parts,
/// so it lets go of its focus at the start of the next turn, before that
/// turn's key reaches it (section 6, rule 1).
#[test]
fn a_layout_through_the_parts_settles_the_focus_before_the_next_key() {
    let window = WindowState::new(Size::new(400.0, 200.0));
    let mut harness = Harness::new(Panels::default(), Toolkit::default(), window, Instant::now());
    harness.send(Message::Show(vec![panel("bar", row(&["a", "b"]))]));
    harness.parts().2.lay_out(&mut Mono::default());
    harness.focus_at(Point::new(10.0, 2.0));
    harness.send(Message::Show(vec![panel("bar", row(&["a"]))]));
    harness.parts().2.lay_out(&mut Mono::default());
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "a")), "it fell back");

    harness.send(Message::Focus(UiId::panel("inspector").with("name")));
    assert!(harness.runtime().target().focused.is_some(), "the toolkit's field");
    harness.send(Message::ShowAndFocus(
        vec![panel("bar", row(&["a", "c"]))],
        id("bar", "c"),
    ));
    harness.parts().2.lay_out(&mut Mono::default());
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "c")), "taken");
    harness.step(DriverEvent::Input {
        event: Event::Key(KeyEvent::Pressed {
            key: Key::Character('x'),
            modifiers: Modifiers::NONE,
            repeat: false,
            text: Some("x".into()),
        }),
        pointer: None,
    });
    let target = harness.runtime().target();
    assert_eq!(target.heard_with, [None], "let go before the key reached it");
    assert_eq!(target.told.last(), Some(&FocusTarget::Nothing));
}

/// Section 7, as decided for PR 2: a focus the layout settles reaches the
/// screen reader in the tree an app builds after the draw. The focused
/// element goes in an update; the layout after it moves the focus to the
/// first stop of its panel; the tree built once the frame is shown reports
/// focus on that stop, with no turn in between.
#[test]
fn a_tree_built_after_the_draw_shows_the_settled_focus() {
    let mut harness = showing(vec![panel("bar", row(&["a", "b"]))]);
    harness.focus_at(Point::new(10.0, 2.0));
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "b")));
    harness.send(Message::ShowThenDescribe(vec![panel("bar", row(&["a"]))]));
    harness.lay_out(&mut Mono::default());
    harness.step(DriverEvent::Presented);
    let app = harness.app();
    let nodes = app.nodes.as_ref().expect("described after the draw");
    let focus = nodes.focus.expect("a node has the focus");
    assert_eq!(app.ids.id(focus.0), Some(&id("bar", "a")), "the settled focus");
}

/// Section 7, rule 7: every change of the runtime's focus reaches the app
/// once, on the next turn, through `App::focus_moved`, whatever moved it:
/// a press, Tab, a focus effect, and a layout that settled it. A ring
/// shown on the element that already had the focus is no change.
#[test]
fn every_focus_move_is_reported_once() {
    let window = WindowState::new(Size::new(400.0, 200.0));
    let app = Panels {
        reports: true,
        ..Panels::default()
    };
    let mut harness = Harness::new(app, NoTarget, window, Instant::now());
    harness.send(Message::Show(vec![panel("bar", row(&["a", "b"]))]));
    harness.lay_out(&mut Mono::default());
    harness.focus_at(Point::new(2.0, 2.0));
    harness.step(DriverEvent::Woken);
    assert_eq!(harness.app().moves, [Some(id("bar", "a"))], "a press");
    harness.send(Message::Focus(id("bar", "a")));
    assert!(harness.runtime().focus_ring().is_some());
    assert_eq!(harness.app().moves.len(), 1, "the ring alone is no change");
    harness.step(tab(false));
    assert_eq!(harness.app().moves.last(), Some(&Some(id("bar", "b"))), "Tab");
    harness.send(Message::Focus(id("bar", "a")));
    assert_eq!(harness.app().moves.last(), Some(&Some(id("bar", "a"))), "an effect");
    harness.send(Message::Show(vec![panel("bar", row(&["b"]))]));
    harness.parts().2.lay_out(&mut Mono::default());
    harness.step(DriverEvent::Woken);
    assert_eq!(harness.app().moves.last(), Some(&Some(id("bar", "b"))), "a settle");
    assert_eq!(harness.app().moves.len(), 4, "each once");
    harness.step(DriverEvent::Woken);
    assert_eq!(harness.app().moves.len(), 4, "nothing moved, nothing told");
}

/// An app that answers a report by moving the focus again hears of that
/// move once more, and then nothing: the reports end.
#[test]
fn a_report_answered_with_a_focus_effect_settles() {
    let window = WindowState::new(Size::new(400.0, 200.0));
    let app = Panels {
        reports: true,
        redirect: Some((id("bar", "a"), id("bar", "b"))),
        ..Panels::default()
    };
    let mut harness = Harness::new(app, NoTarget, window, Instant::now());
    harness.send(Message::Show(vec![panel("bar", row(&["a", "b"]))]));
    harness.lay_out(&mut Mono::default());
    harness.focus_at(Point::new(2.0, 2.0));
    harness.step(DriverEvent::Woken);
    assert_eq!(harness.app().moves, [Some(id("bar", "a")), Some(id("bar", "b"))]);
    assert_eq!(harness.runtime().focused(), Some(&id("bar", "b")));
}

/// What `Copy` hears.
#[derive(Debug, Clone, PartialEq)]
enum Heard {
    /// Focus this element.
    Focus(UiId),
    /// The runtime reported its focus here.
    Moved(Option<UiId>),
    /// A key reached the target.
    Key,
}

/// An app that keeps a copy of the focus from the runtime's reports, and
/// checks it in every other update, as `loom-app` does.
struct Copy {
    copy: Option<UiId>,
    heard: Vec<Heard>,
}

impl App for Copy {
    type Message = Heard;

    fn update(&mut self, message: Heard, cx: &Context<'_, Heard>) -> Effects<Heard> {
        self.heard.push(message.clone());
        match message {
            Heard::Moved(focus) => {
                self.copy = focus;
                Effects::none()
            }
            other => {
                assert_eq!(cx.focused(), self.copy.as_ref(), "the copy drifted before {other:?}");
                match other {
                    Heard::Focus(id) => Effects::focus(id),
                    _ => Effects::none(),
                }
            }
        }
    }

    fn view(&self, view: &mut View) {
        view.panel_with(UiId::panel("bar"), Layer::Panels, &(), |_| row(&["a", "b"]));
    }

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }

    fn focus_moved(&self, focus: Option<&UiId>) -> Option<Heard> {
        Some(Heard::Moved(focus.cloned()))
    }
}

/// A target that turns every key into `Heard::Key`.
struct Keys;

impl Target<Heard> for Keys {
    fn input(&mut self, event: &Event, _pointer: Option<Point>, _now: Instant, out: &mut Vec<Heard>) -> Routed {
        if matches!(event, Event::Key(_)) {
            out.push(Heard::Key);
        }
        Routed::Passed
    }

    fn focus(&mut self, _target: &FocusTarget, _select: Selection) {}

    fn scroll_into_view(&mut self, _id: &UiId) {}

    fn focused(&self) -> Option<UiId> {
        None
    }

    fn needs_redraw(&mut self, _updates: usize) -> bool {
        false
    }
}

/// A focus effect moves the runtime's focus inside an update, and its
/// report runs right after that update, in the same turn, so it comes
/// before the next key's update too: the app's copy never drifts (section
/// 7, rule 7).
#[test]
fn a_report_waiting_in_the_queue_runs_before_the_next_key() {
    use crate::headless::Headless;
    let mut runtime = crate::Runtime::new(
        Copy {
            copy: None,
            heard: Vec::new(),
        },
        Keys,
        WindowState::new(Size::new(400.0, 200.0)),
        std::sync::Arc::new(|| {}),
    );
    let mut driver = Headless::new();
    runtime.handle(DriverEvent::Woken, Instant::now(), &mut driver);
    runtime.lay_out(&mut Mono::default());
    runtime.waker().send(Heard::Focus(id("bar", "b")));
    runtime.handle(DriverEvent::Woken, Instant::now(), &mut driver);
    assert_eq!(
        runtime.app().heard,
        [Heard::Focus(id("bar", "b")), Heard::Moved(Some(id("bar", "b")))],
        "the report ran in the update's own turn"
    );
    let key = DriverEvent::Input {
        event: Event::Key(KeyEvent::Pressed {
            key: Key::Character('k'),
            modifiers: Modifiers::NONE,
            repeat: false,
            text: Some("k".into()),
        }),
        pointer: None,
    };
    runtime.handle(key, Instant::now(), &mut driver);
    assert_eq!(
        runtime.app().heard,
        [
            Heard::Focus(id("bar", "b")),
            Heard::Moved(Some(id("bar", "b"))),
            Heard::Key
        ]
    );
}

/// A link inside a rich text has a node of its own, a link with Click and
/// Focus and the bounds of its words, held by the paragraph's node when
/// the paragraph has one, and the tree reports focus on it.
#[test]
fn a_link_has_a_node_of_its_own() {
    use crate::access::{Access, Action, Role};
    use crate::element::Inline;
    let mut window = crate::window::Window::default();
    let link = Inline::new("Saw").link("saw", Access::new(Role::Link, "Saw").action(Action::Click));
    let notes = Element::rich([Inline::new("See "), link], STYLE)
        .key("notes")
        .access(Access::label("See Saw"));
    window.sync(vec![(
        UiId::panel("help"),
        Layer::Panels,
        Some(Element::column([notes])),
    )]);
    window.lay_out(Size::new(400.0, 200.0), &mut Mono::default());
    let saw = UiId::panel("help").with("notes").with("saw");
    assert_eq!(window.stops(), std::slice::from_ref(&saw));
    let mut ids = IdTable::new();
    let nodes = window.access_nodes(&mut ids, 1.0, Some(&saw));
    let link_id = accesskit::NodeId(ids.number(&saw));
    let notes_id = accesskit::NodeId(ids.number(&UiId::panel("help").with("notes")));
    assert_eq!(nodes.focus, Some(link_id));
    let node = |id| &nodes.nodes.iter().find(|(node, _)| *node == id).expect("a node").1;
    let link = node(link_id);
    assert_eq!(link.role(), accesskit::Role::Link);
    assert!(link.supports_action(accesskit::Action::Click) && link.supports_action(accesskit::Action::Focus));
    let bounds = link.bounds().expect("its words' place");
    // STYLE is 16 pixels at the fake's 8 a character: "See " is 32 wide.
    assert_eq!((bounds.x0, bounds.y0, bounds.x1, bounds.y1), (32.0, 0.0, 56.0, 16.0));
    assert!(node(notes_id).children().contains(&link_id), "inside its paragraph");
}

/// A search field with a list box of `rows`, the field naming `active` as
/// its active descendant, laid out in a window.
fn search(rows: &[&str], active: &str) -> crate::window::Window {
    let mut window = crate::window::Window::default();
    let field = Element::field("", STYLE)
        .key("search")
        .access(Access::new(Role::TextInput, "Search modules").active_descendant(id("browser", "list").with(active)));
    let options = rows.iter().enumerate().map(|(at, row)| {
        Element::text(*row, STYLE).key(*row).access(
            Access::new(Role::ListBoxOption, *row)
                .action(Action::Click)
                .position_in_set(at)
                .states(States {
                    selected: *row == active,
                    ..States::default()
                }),
        )
    });
    let list = Element::column(options)
        .key("list")
        .access(Access::new(Role::ListBox, "Modules").size_of_set(rows.len()));
    window.sync(vec![(
        UiId::panel("browser"),
        Layer::Overlays,
        Some(Element::column([field, list])),
    )]);
    window.lay_out(Size::new(400.0, 300.0), &mut Mono::default());
    window
}

/// The field's active descendant is the highlighted row's node, the rows
/// say where each is in the list, and the list how many it holds
/// (section 7, rule 27, and the browsers' rows in the role table).
#[test]
fn a_field_names_its_highlighted_row_as_its_active_descendant() {
    let window = search(&["saw", "sine", "square"], "sine");
    let mut ids = IdTable::new();
    let nodes = window.access_nodes(&mut ids, 1.0, None);
    let mut node = |id: &UiId| {
        let number = NodeId(ids.number(id));
        let found = nodes.nodes.iter().find(|(node, _)| *node == number).expect("a node");
        (number, found.1.clone())
    };
    let list = id("browser", "list");
    let (sine, sine_node) = node(&list.clone().with("sine"));
    assert_eq!(node(&id("browser", "search")).1.active_descendant(), Some(sine));
    assert_eq!(node(&list).1.size_of_set(), Some(3));
    assert_eq!(node(&list.clone().with("square")).1.position_in_set(), Some(2));
    assert_eq!(sine_node.is_selected(), Some(true));
}

/// An active descendant that names no laid-out element is left out, so the
/// tree never names a node it does not hold.
#[test]
fn an_active_descendant_that_is_gone_is_left_out() {
    let window = search(&["saw"], "sine");
    let mut ids = IdTable::new();
    let nodes = window.access_nodes(&mut ids, 1.0, None);
    let field = NodeId(ids.number(&id("browser", "search")));
    let field = &nodes.nodes.iter().find(|(node, _)| *node == field).expect("a node").1;
    assert_eq!(field.active_descendant(), None);
}

/// AccessKit's convention, which the adapters undo for each platform: a
/// position counts from 0, and the size is on the container, not on each
/// item (`accesskit` 0.25.2, `lib.rs`, the doc comments of `size_of_set`
/// and `position_in_set`). `accesskit_windows` 0.35.2 adds 1 for UIA's
/// PositionInSet and reads SizeOfSet from the container (`node.rs`,
/// `position_in_set` and `size_of_set`, through
/// `size_of_set_from_container`), and `accesskit_atspi_common` 0.21.1 does
/// the same for `posinset` and `setsize`. So the first row reports 0 and
/// the third 2, heard as "1 of 3" and "3 of 3", and no row has a size.
#[test]
fn positions_count_from_zero_with_the_size_on_the_list() {
    let window = search(&["saw", "sine", "square"], "saw");
    let mut ids = IdTable::new();
    let nodes = window.access_nodes(&mut ids, 1.0, None);
    let list = id("browser", "list");
    let mut node = |id: &UiId| {
        let number = NodeId(ids.number(id));
        nodes
            .nodes
            .iter()
            .find(|(node, _)| *node == number)
            .expect("a node")
            .1
            .clone()
    };
    let (first, third) = (node(&list.clone().with("saw")), node(&list.clone().with("square")));
    assert_eq!(first.position_in_set(), Some(0), "the first item is 0");
    assert_eq!(third.position_in_set(), Some(2), "the third item is 2");
    assert_eq!(first.size_of_set(), None, "an item holds no size");
    assert_eq!(node(&list).size_of_set(), Some(3), "the list holds it");
}

/// A field's placeholder reaches its node as a placeholder, never as its
/// value, so a screen reader reads the name, then the hint, and an empty
/// value.
#[test]
fn a_placeholder_is_the_fields_hint_not_its_value() {
    let mut window = crate::window::Window::default();
    let field = Element::field("", STYLE)
        .key("search")
        .placeholder("Search modules", loom_ui_core::Color::WHITE)
        .access(Access::new(Role::TextInput, "Search"));
    window.sync(vec![(
        UiId::panel("browser"),
        Layer::Panels,
        Some(Element::column([field])),
    )]);
    window.lay_out(Size::new(400.0, 100.0), &mut Mono::default());
    let mut ids = IdTable::new();
    let nodes = window.access_nodes(&mut ids, 1.0, None);
    let number = NodeId(ids.number(&id("browser", "search")));
    let node = &nodes.nodes.iter().find(|(node, _)| *node == number).expect("a node").1;
    assert_eq!(node.placeholder(), Some("Search modules"));
    assert_eq!(node.value(), Some(""));
    assert_eq!(node.label(), Some("Search"));
}

/// Two messages in one turn, the first of which moves the focus by an
/// effect: the report runs between them, so the second sees the app's
/// copy current (section 7, rule 7). Help closing over a browser, which
/// gives the focus back to its field while another message waits, is the
/// case that found it.
#[test]
fn a_report_runs_between_two_messages_of_one_turn() {
    use crate::headless::Headless;
    let mut runtime = crate::Runtime::new(
        Copy {
            copy: None,
            heard: Vec::new(),
        },
        Keys,
        WindowState::new(Size::new(400.0, 200.0)),
        std::sync::Arc::new(|| {}),
    );
    let mut driver = Headless::new();
    runtime.handle(DriverEvent::Woken, Instant::now(), &mut driver);
    runtime.lay_out(&mut Mono::default());
    runtime.waker().send(Heard::Focus(id("bar", "b")));
    runtime.waker().send(Heard::Key);
    runtime.handle(DriverEvent::Woken, Instant::now(), &mut driver);
    assert_eq!(
        runtime.app().heard,
        [
            Heard::Focus(id("bar", "b")),
            Heard::Moved(Some(id("bar", "b"))),
            Heard::Key
        ]
    );
}
