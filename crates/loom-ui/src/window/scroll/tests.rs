//! Tests for the runtime's scroll containers (ui-layer.md, section 5, rule
//! 12, and section 7, rule 28), through the headless harness: the wheel,
//! the thumb, focus moving in, the scroll effect, the keys of a container
//! with no stops, the offset kept across rebuilds, and the node.
//!
//! The geometry, with the fake measure's 16 px lines: a list of ten rows in
//! a container 100 px tall keeps 4 px of ring room above and below, so its
//! content is 168 px and it scrolls at most 68 px. Its gutter is the 10 px
//! at the right: a 6 px thumb, 2 px in from each edge.

use crate::access::{Access, Action, Role, Value};
use crate::app::{App, Context, WindowSpec};
use crate::effect::{Effects, FocusTarget, Selection};
use crate::element::{Element, Length, ScrollStyle};
use crate::event::{AccessEvent, DriverEvent, WindowState};
use crate::focus::AccessNodes;
use crate::headless::Harness;
use crate::layout::tests::{Mono, STYLE};
use crate::routing::{Routed, Target};
use crate::view::{Layer, View};
use accesskit::{ActionData, ActionRequest, NodeId};
use loom_ui_core::{
    Color, Event, IdTable, Key, KeyEvent, Modifiers, Point, PointerButton, PointerEvent, Rect, ScrollDelta, Size, UiId,
};
use std::time::Instant;

/// The scroll style the tests use: the app's tokens, with a 4 px ring and
/// the fake measure's 16 px line.
pub(crate) const SCROLL: ScrollStyle = ScrollStyle {
    width: 6.0,
    inset: 2.0,
    radius: 3.0,
    thumb: Color::rgb(0.66, 0.69, 0.75),
    ring: 4.0,
    line: 16.0,
};

/// What the test app does.
#[derive(Debug)]
pub(crate) enum Message {
    /// Show what the builder makes, with this version.
    Show(u32),
    /// A click on the runtime's element with this UI ID: a radio becomes
    /// the chosen one.
    Clicked(UiId),
    /// Scroll this element into view.
    Reveal(UiId),
    /// Focus this element.
    Focus(UiId),
    /// Make the nodes from the last layout and keep them.
    Describe,
    /// A slider with this UI ID moved, in a phase of its gesture.
    Slid(UiId, f32, crate::SlidePhase),
    /// A select with this UI ID had this option chosen.
    Chose(UiId, usize),
    /// An overlay or a question of the app's opened, so this layer takes
    /// input.
    Raise(Layer),
    /// From now on, count each notice that what the tree reports changed.
    CountNotices,
    /// What the tree reports changed (`App::access_changed`).
    Notice,
}

/// Builds the panels from the version shown and the radio chosen.
pub(crate) type Build = fn(u32, Option<&UiId>) -> Vec<(UiId, Layer, Element)>;

/// An app that shows what its builder makes, keeps the radio a click
/// chooses, and maps nodes to UI IDs by its own table, as an app does
/// (section 7, rule 6).
pub(crate) struct Shown {
    build: Build,
    version: u32,
    /// The radio chosen, as the app's model keeps it.
    pub(crate) chosen: Option<UiId>,
    /// Every click, in order.
    pub(crate) clicks: Vec<UiId>,
    ids: IdTable,
    /// The nodes from the last `Describe`.
    pub(crate) nodes: Option<AccessNodes>,
    /// Every report from a slider, in order.
    pub(crate) slid: Vec<(UiId, f32, crate::SlidePhase)>,
    /// Every choice in a select, in order.
    pub(crate) chosen_options: Vec<(UiId, usize)>,
    /// The lowest layer that takes input.
    layer: Layer,
    /// Notices are counted, once a test asks.
    count_notices: bool,
    /// How many notices came that what the tree reports changed.
    pub(crate) notices: usize,
}

impl App for Shown {
    type Message = Message;

    fn update(&mut self, message: Message, cx: &Context<'_, Message>) -> Effects<Message> {
        match message {
            Message::Show(version) => self.version = version,
            Message::Clicked(id) => {
                self.chosen = Some(id.clone());
                self.clicks.push(id);
            }
            Message::Reveal(id) => return Effects::scroll_into_view(id),
            Message::Focus(id) => return Effects::focus(id),
            Message::Describe => self.nodes = Some(cx.access_nodes(&mut self.ids)),
            Message::Slid(id, position, phase) => self.slid.push((id, position, phase)),
            Message::Raise(layer) => self.layer = layer,
            Message::CountNotices => self.count_notices = true,
            Message::Notice => self.notices += 1,
            Message::Chose(id, index) => {
                // A select's builder reads its chosen option as the version's
                // last two digits, so the app keeps the choice it heard.
                self.version = self.version / 100 * 100 + index as u32;
                self.chosen_options.push((id, index));
            }
        }
        Effects::none()
    }

    fn view(&self, view: &mut View) {
        for (key, layer, element) in (self.build)(self.version, self.chosen.as_ref()) {
            view.panel_with(key, layer, &element, Element::clone);
        }
    }

    fn window(&self) -> WindowSpec {
        WindowSpec {
            title: "Scroll".into(),
            min_size: None,
        }
    }

    fn clicked(&self, id: &UiId) -> Option<Message> {
        Some(Message::Clicked(id.clone()))
    }

    fn slid(&self, id: &UiId, position: f32, phase: crate::SlidePhase) -> Option<Message> {
        Some(Message::Slid(id.clone(), position, phase))
    }

    fn chose(&self, id: &UiId, index: usize) -> Option<Message> {
        Some(Message::Chose(id.clone(), index))
    }

    fn access_changed(&self) -> Option<Message> {
        self.count_notices.then_some(Message::Notice)
    }

    fn input(&self) -> crate::app::InputState {
        crate::app::InputState {
            layer: self.layer,
            ..crate::app::InputState::default()
        }
    }

    fn access_target(&self, node: NodeId) -> Option<UiId> {
        self.ids.id(node.0).cloned()
    }

    fn accessibility_event(&self, event: &AccessEvent) -> Option<Message> {
        let AccessEvent::Action(request) = event else {
            return None;
        };
        let id = self.ids.id(request.target_node.0)?.clone();
        match request.action {
            accesskit::Action::Click => Some(Message::Clicked(id)),
            accesskit::Action::Focus => Some(Message::Focus(id)),
            _ => None,
        }
    }
}

/// A target that stands for the toolkit: it passes every event on, and
/// records what it was asked to scroll.
#[derive(Default)]
pub(crate) struct Toolkit {
    /// Every element it was asked to bring into view.
    pub(crate) scrolled: Vec<UiId>,
    /// Every key pressed that reached it.
    pub(crate) keys: Vec<Key>,
    /// An element of its own with the keys, as an Iced field would hold
    /// them; none unless a test gives it one.
    pub(crate) holds: Option<UiId>,
    /// How many times the runtime told it to let go of its focus.
    pub(crate) released: usize,
}

impl Target<Message> for Toolkit {
    fn input(&mut self, event: &Event, _pointer: Option<Point>, _now: Instant, _out: &mut Vec<Message>) -> Routed {
        if let Event::Key(KeyEvent::Pressed { key, .. }) = event {
            self.keys.push(*key);
        }
        Routed::Passed
    }

    fn focus(&mut self, _target: &FocusTarget, _select: Selection) {}

    fn scroll_into_view(&mut self, id: &UiId) {
        self.scrolled.push(id.clone());
    }

    fn focused(&self) -> Option<UiId> {
        self.holds.clone()
    }

    fn release_focus(&mut self) {
        self.released += 1;
        self.holds = None;
    }

    fn needs_redraw(&mut self, _updates: usize) -> bool {
        false
    }
}

/// A harness showing what `build` makes in a 400 by 200 window, laid out.
pub(crate) fn showing(build: Build) -> Harness<Shown, Toolkit> {
    let app = Shown {
        build,
        version: 0,
        chosen: None,
        clicks: Vec::new(),
        ids: IdTable::default(),
        nodes: None,
        slid: Vec::new(),
        chosen_options: Vec::new(),
        layer: Layer::Canvas,
        count_notices: false,
        notices: 0,
    };
    let window = WindowState::new(Size::new(400.0, 200.0));
    let mut harness = Harness::new(app, Toolkit::default(), window, Instant::now());
    harness.lay_out(&mut Mono::default());
    harness
}

/// Runs `event` and lays the window out, as a driver does before it draws.
pub(crate) fn step(harness: &mut Harness<Shown, Toolkit>, event: DriverEvent) {
    harness.step(event);
    harness.lay_out(&mut Mono::default());
}

/// A key press with `modifiers`.
pub(crate) fn key_with(key: Key, modifiers: Modifiers) -> DriverEvent {
    DriverEvent::Input {
        event: Event::Key(KeyEvent::Pressed {
            key,
            modifiers,
            repeat: false,
            text: None,
        }),
        pointer: None,
    }
}

/// A key press with no modifiers.
pub(crate) fn key(key: Key) -> DriverEvent {
    key_with(key, Modifiers::NONE)
}

/// Shift with `key`.
pub(crate) fn shifted(key: Key) -> DriverEvent {
    key_with(
        key,
        Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
    )
}

/// A pointer event at `point`.
pub(crate) fn pointer(event: PointerEvent, point: Point) -> DriverEvent {
    DriverEvent::Input {
        event: Event::Pointer(event),
        pointer: Some(point),
    }
}

/// A left press and release at `point`: a click.
pub(crate) fn click(harness: &mut Harness<Shown, Toolkit>, point: Point) {
    step(harness, pointer(PointerEvent::Pressed(PointerButton::Left), point));
    step(harness, pointer(PointerEvent::Released(PointerButton::Left), point));
}

/// A wheel turned `y` lines at `point`: down when negative.
fn wheel(y: f32, point: Point) -> DriverEvent {
    pointer(PointerEvent::Scrolled(ScrollDelta::Lines { x: 0.0, y }), point)
}

/// The UI ID `list/<key>`.
pub(crate) fn id(key: &str) -> UiId {
    UiId::panel("list").with(key)
}

/// The list container's UI ID.
fn list() -> UiId {
    id("rows")
}

/// A focus stop named `key`, 16 px tall.
pub(crate) fn stop(key: &str) -> Element {
    Element::text(key, STYLE)
        .key(key)
        .focusable()
        .access(Access::new(Role::ComboBox, key).action(Action::Click))
}

/// A scroll container keyed `key`, `height` tall, filling the width,
/// showing `rows` one under another.
pub(crate) fn container(key: &str, height: f32, rows: Vec<Element>) -> Element {
    Element::scroll(Element::column(rows), SCROLL)
        .key(key)
        .width(Length::FILL)
        .height(Length::Fixed(height))
        .access(Access::new(Role::ScrollView, key))
}

/// The panel `list`, holding `element`.
pub(crate) fn list_panel(element: Element) -> Vec<(UiId, Layer, Element)> {
    vec![(UiId::panel("list"), Layer::Panels, element.width(Length::FILL))]
}

/// Ten focus stops, `s0` to `s9`, in a container 100 px tall.
fn ten_stops(_: u32, _: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    list_panel(container(
        "rows",
        100.0,
        (0..10).map(|n| stop(&format!("s{n}"))).collect(),
    ))
}

/// Ten rows of text with no stop, in a container 100 px tall; version 1
/// changes a row's words, and version 2 has no container.
fn ten_rows(version: u32, _: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let rows = (0..10)
        .map(|n| Element::text(format!("row {n} v{}", version.min(1)), STYLE))
        .collect();
    match version {
        2 => list_panel(Element::text("gone", STYLE)),
        _ => list_panel(container("rows", 100.0, rows)),
    }
}

/// An inner container 60 px tall of ten rows, between two spaces, in an
/// outer one 100 px tall.
fn nested(_: u32, _: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let rows = (0..10).map(|n| Element::text(format!("row {n}"), STYLE)).collect();
    list_panel(container(
        "outer",
        100.0,
        vec![
            Element::space(Length::Shrink, Length::Fixed(20.0)),
            container("inner", 60.0, rows),
            Element::space(Length::Shrink, Length::Fixed(100.0)),
        ],
    ))
}

/// A slider and nine text rows in a container.
fn with_slider(_: u32, _: Option<&UiId>) -> Vec<(UiId, Layer, Element)> {
    let slider = Element::slider(crate::Slider::new(0.5)).key("level").access(
        Access::new(Role::Slider, "Level")
            .value(Value::Number {
                value: 0.5,
                min: 0.0,
                max: 1.0,
                step: None,
            })
            .action(Action::Increment),
    );
    let mut rows = vec![slider];
    rows.extend((1..10).map(|n| Element::text(format!("row {n}"), STYLE)));
    list_panel(container("rows", 100.0, rows))
}

/// How far the container with UI ID `id` is scrolled.
fn offset(harness: &Harness<Shown, Toolkit>, id: &UiId) -> f32 {
    harness.runtime().scroll_offset(id).expect("a scroll container").0
}

/// The viewport of the list container: its bounds less the 10 px gutter.
fn viewport(harness: &Harness<Shown, Toolkit>) -> Rect {
    let bounds = harness.runtime().bounds(&list()).expect("the list");
    Rect::new(bounds.position(), Size::new(bounds.width - 10.0, bounds.height))
}

/// The focused element's ring, as far as it reaches (4 px), is wholly
/// inside the list's viewport, so no part of it is cut.
fn assert_ring_shows(harness: &Harness<Shown, Toolkit>) {
    let (focus, bounds) = harness.runtime().focus_ring().expect("a ring");
    let ring = Rect::new(
        Point::new(bounds.x - 4.0, bounds.y - 4.0),
        Size::new(bounds.width + 8.0, bounds.height + 8.0),
    );
    let view = viewport(harness);
    assert!(
        ring.y >= view.y && ring.y + ring.height <= view.y + view.height,
        "the ring of {focus} at {ring:?} is cut by {view:?}"
    );
}

/// Section 5, rule 12: a wheel's line is three lines of body text, 48 px
/// here, and a touch pad's pixels are taken as given. The offset stops at
/// either end.
#[test]
fn the_wheel_scrolls_by_lines_and_pixels_and_stops_at_the_ends() {
    let mut harness = showing(ten_rows);
    let over = Point::new(50.0, 50.0);
    step(&mut harness, wheel(-1.0, over));
    assert_eq!(offset(&harness, &list()), 48.0, "one line of the wheel, down");
    step(&mut harness, wheel(-1.0, over));
    assert_eq!(offset(&harness, &list()), 68.0, "held at the bottom");
    let pixels = pointer(PointerEvent::Scrolled(ScrollDelta::Pixels { x: 0.0, y: 10.0 }), over);
    step(&mut harness, pixels);
    assert_eq!(offset(&harness, &list()), 58.0, "ten pixels up");
    step(&mut harness, wheel(5.0, over));
    assert_eq!(offset(&harness, &list()), 0.0, "held at the top");
}

/// What an inner container cannot take passes to the one around it.
#[test]
fn a_wheel_at_an_inner_end_passes_to_the_container_around_it() {
    let mut harness = showing(nested);
    let (inner, outer) = (id("outer").with("inner"), id("outer"));
    let over = Point::new(50.0, 40.0);
    step(&mut harness, wheel(-1.0, over));
    assert_eq!(offset(&harness, &inner), 48.0, "the inner one first");
    assert_eq!(offset(&harness, &outer), 0.0);
    step(&mut harness, wheel(-1.0, over));
    assert_eq!(offset(&harness, &inner), 96.0);
    assert_eq!(offset(&harness, &outer), 0.0, "the inner one took it all");
    // The inner one's 168 px in 60 scroll at most 108: 12 more, and the
    // other 36 go out.
    step(&mut harness, wheel(-1.0, over));
    assert_eq!(offset(&harness, &inner), 108.0, "held at its bottom");
    assert_eq!(offset(&harness, &outer), 36.0, "the rest went to the outer one");
}

/// Section 5, rule 16: the wheel over a slider scrolls the container, with
/// Ctrl held as without it, since the wheel never moves a slider.
#[test]
fn the_wheel_over_a_slider_scrolls_with_or_without_ctrl() {
    let mut harness = showing(with_slider);
    let on_slider = Point::new(10.0, 10.0);
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::NONE
    };
    let held = DriverEvent::Input {
        event: Event::Key(KeyEvent::ModifiersChanged(ctrl)),
        pointer: None,
    };
    step(&mut harness, held);
    step(&mut harness, wheel(-1.0, on_slider));
    assert_eq!(offset(&harness, &list()), 48.0, "with Ctrl, the container's");
    let let_go = DriverEvent::Input {
        event: Event::Key(KeyEvent::ModifiersChanged(Modifiers::NONE)),
        pointer: None,
    };
    step(&mut harness, let_go);
    step(&mut harness, wheel(1.0, on_slider));
    assert_eq!(offset(&harness, &list()), 0.0, "without it, the container's");
}

/// Dragging the thumb scrolls in proportion, and a press on the gutter
/// below or above it moves one page that way.
#[test]
fn the_thumb_drags_and_the_track_pages() {
    let mut harness = showing(ten_rows);
    // The track runs from 2 to 98, and the thumb is 96 × 100 / 168 long.
    let length = 96.0 * 100.0 / 168.0;
    let on_thumb = Point::new(395.0, 10.0);
    step(
        &mut harness,
        pointer(PointerEvent::Pressed(PointerButton::Left), on_thumb),
    );
    // Off the bar, the drag still holds.
    step(&mut harness, pointer(PointerEvent::Moved, Point::new(200.0, 40.0)));
    let expected = (40.0_f32 - 8.0 - 2.0) / (96.0 - length) * 68.0;
    assert!(
        (offset(&harness, &list()) - expected).abs() <= 1.0,
        "{} against {expected}",
        offset(&harness, &list())
    );
    step(
        &mut harness,
        pointer(PointerEvent::Released(PointerButton::Left), on_thumb),
    );
    let after = offset(&harness, &list());
    step(&mut harness, pointer(PointerEvent::Moved, Point::new(395.0, 90.0)));
    assert_eq!(
        offset(&harness, &list()),
        after,
        "a move after the release drags nothing"
    );

    let mut harness = showing(ten_rows);
    let below = Point::new(395.0, 90.0);
    click(&mut harness, below);
    assert_eq!(offset(&harness, &list()), 68.0, "a page down, held at the bottom");
    click(&mut harness, Point::new(395.0, 5.0));
    assert_eq!(offset(&harness, &list()), 0.0, "a page up");
}

/// Section 5, rule 12: Tab to a stop below the fold scrolls the least
/// distance that shows it and its ring, with 2 px to spare, and Shift+Tab
/// does the same going up. No ring of a focused element is ever cut.
#[test]
fn tab_and_shift_tab_bring_each_stop_and_its_ring_into_view() {
    let mut harness = showing(ten_stops);
    for _ in 0..10 {
        step(&mut harness, key(Key::Tab));
        assert_ring_shows(&harness);
    }
    assert_eq!(harness.runtime().focused(), Some(&id("rows").with("s9")));
    // The last stop runs from 148 to 164 in the content: 6 px past it
    // shows at 100.
    assert_eq!(offset(&harness, &list()), 68.0);
    let bounds = harness.runtime().bounds(&id("rows").with("s9")).expect("s9");
    assert_eq!(bounds.y + bounds.height + 4.0 + 2.0, 102.0, "held at the most");
    for _ in 0..9 {
        step(&mut harness, shifted(Key::Tab));
        assert_ring_shows(&harness);
    }
    assert_eq!(harness.runtime().focused(), Some(&id("rows").with("s0")));
    assert_eq!(offset(&harness, &list()), 0.0);
}

/// A move inside the view scrolls nothing: the least distance is none.
#[test]
fn focus_already_in_view_scrolls_nothing() {
    let mut harness = showing(ten_stops);
    step(&mut harness, key(Key::Tab));
    step(&mut harness, key(Key::Tab));
    assert_eq!(offset(&harness, &list()), 0.0);
}

/// `Effects::scroll_into_view` on the runtime's element scrolls its
/// container, as a focus move does; on a toolkit's part, it reaches the
/// target as before. A focus effect brings its element into view too.
#[test]
fn scroll_into_view_is_ours_for_our_elements_and_the_targets_otherwise() {
    let mut harness = showing(ten_stops);
    step(&mut harness, DriverEvent::Woken);
    harness.send(Message::Reveal(id("rows").with("s9")));
    harness.lay_out(&mut Mono::default());
    assert_eq!(offset(&harness, &list()), 68.0);
    assert!(
        harness.runtime().target().scrolled.is_empty(),
        "the target heard nothing"
    );

    let theirs = UiId::panel("inspector").with("cutoff");
    harness.send(Message::Reveal(theirs.clone()));
    assert_eq!(harness.runtime().target().scrolled, [theirs]);

    harness.send(Message::Focus(id("rows").with("s0")));
    harness.lay_out(&mut Mono::default());
    assert_eq!(offset(&harness, &list()), 0.0, "a focus effect shows its element");
}

/// Section 5, rule 12 and SC 2.1.1: a container whose content has no stop
/// is a stop itself, and its keys scroll it: a line, a page less a line, a
/// page, and the ends. One whose content has stops is none.
#[test]
fn a_container_with_no_stops_inside_is_a_stop_and_its_keys_scroll() {
    let mut harness = showing(ten_rows);
    assert_eq!(harness.runtime().focus_stops(), [list()]);
    step(&mut harness, key(Key::Tab));
    assert_eq!(harness.runtime().focused(), Some(&list()));
    step(&mut harness, key(Key::Down));
    assert_eq!(offset(&harness, &list()), 16.0, "a line");
    step(&mut harness, key(Key::Up));
    assert_eq!(offset(&harness, &list()), 0.0);
    step(&mut harness, key(Key::PageDown));
    assert_eq!(offset(&harness, &list()), 68.0, "a page less a line, 84, held at 68");
    step(&mut harness, key(Key::Home));
    assert_eq!(offset(&harness, &list()), 0.0);
    step(&mut harness, key(Key::End));
    assert_eq!(offset(&harness, &list()), 68.0);
    step(&mut harness, key(Key::PageUp));
    assert_eq!(offset(&harness, &list()), 0.0);
    step(&mut harness, key(Key::Space));
    assert_eq!(offset(&harness, &list()), 68.0, "a page");
    step(&mut harness, shifted(Key::Space));
    assert_eq!(offset(&harness, &list()), 0.0, "a page up");

    let harness = showing(ten_stops);
    assert!(
        !harness.runtime().focus_stops().contains(&list()),
        "its stops scroll it"
    );
}

/// The offset is widget state kept by UI ID: a rebuilt view keeps it, and
/// it goes with the container.
#[test]
fn the_offset_survives_a_rebuild_and_goes_with_the_container() {
    let mut harness = showing(ten_rows);
    step(&mut harness, wheel(-1.0, Point::new(50.0, 50.0)));
    harness.send(Message::Show(1));
    harness.lay_out(&mut Mono::default());
    assert_eq!(offset(&harness, &list()), 48.0, "kept across a rebuild");
    harness.send(Message::Show(2));
    harness.lay_out(&mut Mono::default());
    assert_eq!(harness.runtime().scroll_offset(&list()), None);
    harness.send(Message::Show(0));
    harness.lay_out(&mut Mono::default());
    assert_eq!(offset(&harness, &list()), 0.0, "a new container starts at the top");
}

/// Section 7, rule 28: the container is a scroll view with its offset, its
/// range, and its actions; a screen reader's ScrollDown moves it a page,
/// ScrollUp by an item moves a line, and SetScrollOffset sets it. What it
/// cuts away is cut from its parts' bounds too.
#[test]
fn the_node_reports_the_offset_and_takes_the_scroll_actions() {
    let mut harness = showing(ten_rows);
    step(&mut harness, wheel(-1.0, Point::new(50.0, 50.0)));
    harness.send(Message::Describe);
    let nodes = harness.app().nodes.clone().expect("the app described its nodes");
    let (target, node) = nodes
        .nodes
        .iter()
        .find(|(_, node)| node.role() == accesskit::Role::ScrollView)
        .cloned()
        .expect("the container's node");
    assert_eq!(node.label(), Some("rows"));
    assert_eq!(node.scroll_y(), Some(48.0));
    assert_eq!((node.scroll_y_min(), node.scroll_y_max()), (Some(0.0), Some(68.0)));
    for action in [
        accesskit::Action::ScrollUp,
        accesskit::Action::ScrollDown,
        accesskit::Action::SetScrollOffset,
    ] {
        assert!(node.supports_action(action), "{action:?}");
    }
    let act = |action, data| {
        DriverEvent::Accessibility(AccessEvent::Action(ActionRequest {
            action,
            target_tree: accesskit::TreeId::ROOT,
            target_node: target,
            data,
        }))
    };
    step(
        &mut harness,
        act(
            accesskit::Action::ScrollUp,
            Some(ActionData::ScrollUnit(accesskit::ScrollUnit::Item)),
        ),
    );
    assert_eq!(offset(&harness, &list()), 32.0, "an item is a line");
    step(&mut harness, act(accesskit::Action::ScrollDown, None));
    assert_eq!(offset(&harness, &list()), 68.0, "a page, held at the most");
    let to = ActionData::SetScrollOffset(accesskit::Point::new(0.0, 20.0));
    step(&mut harness, act(accesskit::Action::SetScrollOffset, Some(to)));
    assert_eq!(offset(&harness, &list()), 20.0);
}

/// A part inside a container that is scrolled out of view has its node's
/// bounds cut to the container, as it is drawn.
#[test]
fn a_parts_bounds_are_cut_by_the_container() {
    let mut harness = showing(ten_stops);
    step(&mut harness, wheel(-1.0, Point::new(50.0, 50.0)));
    let mut ids = IdTable::default();
    let nodes = harness.runtime().access_nodes(&mut ids);
    let mut bounds = |key: &str| {
        let number = NodeId(ids.number(&id("rows").with(key)));
        nodes
            .nodes
            .iter()
            .find(|(id, _)| *id == number)
            .and_then(|(_, node)| node.bounds())
            .expect("bounds")
    };
    // s0 ran from 4 to 20 and is now 48 px higher, wholly cut away.
    let gone = bounds("s0");
    assert_eq!(gone.y1 - gone.y0, 0.0, "{gone:?}");
    let shown = bounds("s4");
    assert_eq!(shown.y0, 68.0 - 48.0);
}

/// While the thumb is held, a toolkit that routes before the turn is kept
/// from every pointer event, wherever the pointer goes, so it hovers
/// nothing under the drag; after the release it hears them again.
#[test]
fn a_held_thumb_keeps_the_pointer_from_the_toolkit() {
    let mut harness = showing(ten_rows);
    let far = Point::new(200.0, 150.0);
    let moved = Event::Pointer(PointerEvent::Moved);
    step(
        &mut harness,
        pointer(PointerEvent::Pressed(PointerButton::Left), Point::new(395.0, 10.0)),
    );
    assert!(harness.parts().2.over_target(&moved, far), "held");
    step(&mut harness, pointer(PointerEvent::Released(PointerButton::Left), far));
    assert!(!harness.parts().2.over_target(&moved, far), "let go");
}

/// The ring colour the painted-ring test looks for, unlike anything else.
const RING: Color = Color::rgb(0.0, 1.0, 0.0);

/// The focus ring as painted in the panels layer, 2 px wide and 2 px out,
/// and the layer it was painted in, cut to that rectangle.
fn painted_ring(harness: &mut Harness<Shown, Toolkit>) -> (Rect, Rect) {
    let style = crate::paint::PanelStyle {
        text: Color::WHITE,
        caret: Color::WHITE,
        selection: Color::BLACK,
        quiet: Color::WHITE,
        mark: Color::WHITE,
        primary: Color::rgb(0.3, 0.6, 1.0),
        track: Color::rgb(0.4, 0.4, 0.4),
        ring: crate::focus::RingStyle {
            color: RING,
            width: 2.0,
            offset: 2.0,
        },
    };
    harness.lay_out(&mut Mono::default());
    let mut recorder = crate::paint::testing::Recorder::default();
    let (_, _, mut panels) = harness.parts();
    panels.paint(Layer::Panels, &mut recorder, &style);
    let painted = recorder.painted;
    let at = painted
        .iter()
        .position(|painted| matches!(painted, crate::paint::testing::Painted::Quad(quad) if quad.border == RING))
        .expect("a painted ring");
    let crate::paint::testing::Painted::Quad(ring) = painted[at] else {
        unreachable!("found as a quad")
    };
    let crate::paint::testing::Painted::Begin(layer) = painted[at - 1] else {
        panic!("the ring is painted in a layer of its own: {:?}", painted[at - 1]);
    };
    (ring.rect, layer)
}

/// True when `inner` lies wholly inside `outer`.
fn inside(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}

/// Section 5, rule 12 with the ring the runtime paints (section 6, rule
/// 3): as Tab moves the focus down the list and back up, the container
/// scrolls far enough that the painted ring is never cut by the clip it is
/// painted in, which is the container's. A wheel that then scrolls the
/// focused stop out of view leaves no ring outside the container.
#[test]
fn focus_moving_in_scrolls_so_the_painted_ring_is_never_cut() {
    let mut harness = showing(ten_stops);
    let container = harness.runtime().bounds(&list()).expect("the list");
    let mut steps: Vec<DriverEvent> = (0..10).map(|_| key(Key::Tab)).collect();
    steps.extend((0..9).map(|_| shifted(Key::Tab)));
    for event in steps {
        step(&mut harness, event);
        let (ring, layer) = painted_ring(&mut harness);
        assert_eq!(layer, container, "the ring is cut to its container");
        assert!(inside(ring, layer), "the ring at {ring:?} is cut by {layer:?}");
    }
    // s0 is focused at the top; the wheel moves it out of view.
    step(&mut harness, wheel(-1.0, Point::new(50.0, 50.0)));
    let (ring, layer) = painted_ring(&mut harness);
    assert_eq!(layer, container);
    assert!(
        ring.y + ring.height <= layer.y,
        "s0's ring is above the container, cut away"
    );
}

/// A link inside a rich text in a scroll container moves with the scroll,
/// with no new measure: its bounds, its words, and where it is hit
/// (ui-layer.md, section 8, rule 10).
#[test]
fn a_link_moves_with_its_scroll_container() {
    use crate::element::Inline;
    let saw = UiId::panel("help").with("rows").with("notes").with("saw");
    let lines = (0..6).map(|n| Element::text(format!("line {n}"), STYLE));
    let link = Inline::new("Saw").link("saw", Access::new(Role::Link, "Saw").action(Action::Click));
    let notes = Element::rich([Inline::new("See "), link], STYLE).key("notes");
    let content = Element::column(lines.chain([notes]));
    let rows = Element::scroll(content, SCROLL).key("rows").height(Length::Fixed(48.0));
    let mut window = crate::window::Window::default();
    window.sync(vec![(
        UiId::panel("help"),
        Layer::Panels,
        Some(Element::column([rows])),
    )]);
    window.lay_out(Size::new(300.0, 200.0), &mut Mono::default());
    let before = window.bounds(&saw).expect("laid out");
    assert!(window.scroll_to_show(&saw), "the container moved");
    let after = window.bounds(&saw).expect("still laid out");
    assert!(after.y < before.y, "{before:?} {after:?}");
    assert_eq!(after.x, before.x);
    assert_eq!(window.pieces(&saw), [after], "its words moved with it");
    let middle = Point::new(after.x + after.width / 2.0, after.y + after.height / 2.0);
    let hit = window.hit(middle).map(|(_, hit)| format!("{hit:?}"));
    assert!(hit.is_some_and(|hit| hit.contains("saw")), "hit where it shows now");
}

mod keys;
