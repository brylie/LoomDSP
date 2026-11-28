//! Tests for a custom element's nodes: where they go in the tree, their
//! IDs, when they are built, their focus and relations, and a screen
//! reader's actions on them (ui-layer.md, section 9, rule 3).

use crate::access::{Access, Action, Problem, Role, Value};
use crate::app::{App, Context, InputState, WindowSpec};
use crate::custom::{AccessRequest, Custom, CustomEvent, CustomKind, CustomNodes, Moment, Out, Used};
use crate::effect::Effects;
use crate::element::{Element, Length};
use crate::event::{AccessEvent, DriverEvent, WindowState};
use crate::focus::AccessNodes;
use crate::headless::Harness;
use crate::layout::tests::{Mono, STYLE};
use crate::routing::NoTarget;
use crate::view::{Layer, View};
use accesskit::{ActionRequest, NodeId};
use loom_ui_core::{Event, IdKey, IdTable, Point, PointerButton, PointerEvent, Rect, Size, UiId};
use std::cell::RefCell;
use std::time::Instant;

/// The grid's kind.
const GRID: CustomKind = CustomKind::new("grid");

/// What the test app hears.
#[derive(Debug, Clone, PartialEq)]
enum Message {
    /// The grid clicked the part with this name, or itself with "".
    Clicked(String),
    /// The screen reader's request went on to the app.
    Reader(accesskit::Action),
    /// Show these cells.
    Cells(Vec<&'static str>),
    /// Change nothing the grid shows.
    Nothing,
    /// Focus the grid's part with this name.
    FocusPart(&'static str),
    /// Open or close the overlay.
    Overlay(bool),
    /// What the tree reports changed.
    Changed,
}

/// The grid's state: which part has its focus.
#[derive(Default)]
struct GridState {
    part: Option<IdKey>,
    /// The app's due moment its last deadline tick answered.
    answered: Option<Instant>,
    /// How many deadline ticks it heard.
    ticks: usize,
}

impl Custom<Grids> for GridState {
    fn event(&mut self, app: &Grids, event: &CustomEvent, _moment: &Moment, out: &mut Out<'_, Message>) -> Used {
        match event {
            // A press changes what its nodes show; a frame only redraws.
            CustomEvent::Pointer(PointerEvent::Pressed(_)) => out.nodes_changed(),
            CustomEvent::Frame => out.redraw(),
            CustomEvent::Deadline => {
                self.answered = app.due;
                self.ticks += 1;
            }
            _ => {}
        }
        Used::Yes
    }

    fn next_deadline(&self, app: &Grids) -> Option<Instant> {
        app.due.filter(|due| self.answered != Some(*due))
    }

    fn animating(&self, app: &Grids) -> bool {
        app.animate
    }

    fn focus_inner(&mut self, _app: &Grids, rest: &[IdKey]) -> bool {
        self.part = rest.first().cloned();
        true
    }

    fn focused_inner(&self, _app: &Grids) -> Option<Vec<IdKey>> {
        self.part.clone().map(|part| vec![part])
    }

    fn access(&self, app: &Grids, bounds: Rect, nodes: &mut CustomNodes) {
        for (at, cell) in app.cells.iter().enumerate() {
            let place = Rect::new(Point::new(bounds.x + 50.0 * at as f32, bounds.y), Size::new(50.0, 50.0));
            let mut access = Access::new(Role::Button, format!("Cell {cell}")).action(Action::Click);
            // A cell named "off" is turned off, and still asks for focus.
            if *cell == "off" {
                access = access.states(crate::access::States {
                    disabled: true,
                    ..crate::access::States::default()
                });
            }
            if *cell == "a" {
                access = access.controlling(UiId::panel("main").with("inspector"));
            }
            nodes.add([*cell], access, place).focusable();
            // Cell a holds a knob, inside it on screen.
            if *cell == "a" {
                let knob = Access::new(Role::Slider, "Knob").value(Value::Number {
                    value: 0.5,
                    min: 0.0,
                    max: 1.0,
                    step: None,
                });
                nodes.add([*cell, "knob"], knob, place).focusable();
            }
        }
        if app.broken {
            nodes.add(["broken"], Access::new(Role::Button, "Broken"), bounds);
        }
    }

    fn access_version(&self, app: &Grids) -> u64 {
        app.version
    }

    fn access_action(
        &mut self,
        _app: &Grids,
        rest: &[IdKey],
        request: &AccessRequest,
        out: &mut Out<'_, Message>,
    ) -> bool {
        if request.action != Action::Click {
            return false;
        }
        let name = match rest.first() {
            Some(IdKey::Name(name)) => name.to_string(),
            _ => String::new(),
        };
        out.send(Message::Clicked(name));
        true
    }
}

/// An app with the grid and an inspector it controls, in one panel, and
/// an overlay over them when open.
struct Grids {
    cells: Vec<&'static str>,
    /// A moment the grid's deadline follows, set when the app hears its
    /// tree changed.
    due: Option<Instant>,
    /// True when the tree's change notice makes the grid's deadline due.
    due_when_changed: bool,
    /// Moves when the cells change, as the grid's access version.
    version: u64,
    animate: bool,
    overlay: bool,
    /// True when the grid adds a node that breaks section 7, rule 2.
    broken: bool,
    /// True when the cells change with no new access version, as an
    /// element that forgot to say so would.
    stale: bool,
    /// True when the grid has no Access of its own and is no focus stop.
    plain: bool,
    /// The app's one node table, which maps a screen reader's node back.
    ids: RefCell<IdTable>,
    log: Vec<Message>,
}

impl App for Grids {
    type Message = Message;

    fn update(&mut self, message: Message, cx: &Context<'_, Message>) -> Effects<Message> {
        let effects = match &message {
            Message::Changed if self.due_when_changed => {
                self.due = Some(cx.now());
                Effects::none()
            }
            Message::Cells(cells) => {
                self.cells = cells.clone();
                if !self.stale {
                    self.version += 1;
                }
                Effects::none()
            }
            Message::FocusPart(part) => Effects::focus(grid().with(*part)),
            Message::Overlay(open) => {
                self.overlay = *open;
                Effects::none()
            }
            _ => Effects::none(),
        };
        self.log.push(message);
        effects
    }

    fn view(&self, view: &mut View) {
        view.panel_with(UiId::panel("main"), Layer::Panels, &self.plain, |&plain| {
            let element = Element::custom(GRID)
                .key("grid")
                .width(Length::Fixed(200.0))
                .height(Length::Fixed(100.0));
            let element = if plain {
                element
            } else {
                element
                    .focusable()
                    .access(Access::new(Role::Group, "Grid").action(Action::Click))
            };
            Element::column([
                element,
                Element::text("inspector", STYLE)
                    .key("inspector")
                    .focusable()
                    .access(Access::new(Role::Button, "Inspector").controlling(grid().with("a"))),
            ])
        });
        if self.overlay {
            view.panel_with(UiId::panel("dialog"), Layer::Overlays, &(), |_| {
                Element::space(Length::Fixed(50.0), Length::Fixed(50.0)).key("box")
            });
        }
    }

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }

    fn input(&self) -> InputState {
        InputState {
            layer: if self.overlay { Layer::Overlays } else { Layer::Canvas },
            ..InputState::default()
        }
    }

    fn access_changed(&self) -> Option<Message> {
        Some(Message::Changed)
    }

    fn access_target(&self, node: NodeId) -> Option<UiId> {
        self.ids.borrow().id(node.0).cloned()
    }

    fn accessibility_event(&self, event: &AccessEvent) -> Option<Message> {
        match event {
            AccessEvent::Action(request) => Some(Message::Reader(request.action)),
            _ => None,
        }
    }

    fn make_custom(&self, _id: &UiId, kind: CustomKind) -> Option<Box<dyn Custom<Self>>> {
        (kind == GRID).then(|| Box::new(GridState::default()) as Box<dyn Custom<Self>>)
    }
}

/// The grid's UI ID.
fn grid() -> UiId {
    UiId::panel("main").with("grid")
}

/// A new app with cells a and b.
fn grids() -> Grids {
    Grids {
        cells: vec!["a", "b"],
        due: None,
        due_when_changed: false,
        version: 0,
        animate: false,
        overlay: false,
        broken: false,
        stale: false,
        plain: false,
        ids: RefCell::new(IdTable::default()),
        log: Vec::new(),
    }
}

/// A harness with the grid at (0, 0), 200 by 100, in a 400 by 300 window
/// at scale 2, laid out.
fn harness(app: Grids) -> Harness<Grids, NoTarget> {
    let mut window = WindowState::new(Size::new(400.0, 300.0));
    window.scale = 2.0;
    let mut harness = Harness::new(app, NoTarget, window, Instant::now());
    harness.lay_out(&mut Mono::default());
    harness.step(DriverEvent::Woken);
    harness
}

/// Sends `message`, then lays out.
fn send(harness: &mut Harness<Grids, NoTarget>, message: Message) {
    harness.send(message);
    harness.lay_out(&mut Mono::default());
}

/// The tree's nodes, numbered from the app's table.
fn tree(harness: &Harness<Grids, NoTarget>) -> AccessNodes {
    let ids = &harness.app().ids;
    harness.runtime().access_nodes(&mut ids.borrow_mut())
}

/// The node number of `id` in the app's table.
fn number(harness: &Harness<Grids, NoTarget>, id: &UiId) -> NodeId {
    NodeId(harness.app().ids.borrow_mut().number(id))
}

/// The node `node` in `nodes`.
fn node(nodes: &AccessNodes, node: NodeId) -> &accesskit::Node {
    &nodes
        .nodes
        .iter()
        .find(|(id, _)| *id == node)
        .expect("the node is in the tree")
        .1
}

/// How many times the runtime built the grid's nodes.
fn builds(harness: &Harness<Grids, NoTarget>) -> u64 {
    harness.runtime().counts().custom_node_builds
}

/// A screen reader's `action` on the node of `id`.
fn act(harness: &mut Harness<Grids, NoTarget>, id: &UiId, action: accesskit::Action) {
    let target_node = number(harness, id);
    harness.step(DriverEvent::Accessibility(AccessEvent::Action(ActionRequest {
        action,
        target_tree: accesskit::TreeId::ROOT,
        target_node,
        data: None,
    })));
    harness.lay_out(&mut Mono::default());
}

/// Each part's node goes under the element's own node, and a part whose
/// path is under another's goes inside that one, in the element's order,
/// with bounds in physical pixels.
#[test]
fn the_parts_go_under_the_element() {
    let harness = harness(grids());
    let nodes = tree(&harness);
    let (a, b, knob) = (grid().with("a"), grid().with("b"), grid().with("a").with("knob"));
    let grid_node = node(&nodes, number(&harness, &grid()));
    assert_eq!(grid_node.children(), [number(&harness, &a), number(&harness, &b)]);
    let a_node = node(&nodes, number(&harness, &a));
    assert_eq!(a_node.children(), [number(&harness, &knob)]);
    let b_node = node(&nodes, number(&harness, &b));
    let bounds = b_node.bounds().expect("b has bounds");
    assert_eq!((bounds.x0, bounds.y0, bounds.x1, bounds.y1), (100.0, 0.0, 200.0, 100.0));
    assert!(b_node.supports_action(accesskit::Action::Focus));
    assert!(harness.runtime().custom_access_problems().is_empty());
}

/// A turned-off part that asks to be focusable is no focus stop, as any
/// turned-off element (section 6, rule 2): its node offers no Focus, and
/// no action at all, and that breaks no rule.
#[test]
fn a_turned_off_part_offers_no_focus() {
    let mut app = grids();
    app.cells = vec!["a", "off"];
    let harness = harness(app);
    let nodes = tree(&harness);
    let off = node(&nodes, number(&harness, &grid().with("off")));
    assert!(off.is_disabled());
    assert!(
        !off.supports_action(accesskit::Action::Focus),
        "no Focus on a turned-off part"
    );
    assert!(!off.supports_action(accesskit::Action::Click));
    assert!(harness.runtime().custom_access_problems().is_empty());
}

/// A focus effect on a turned-off part is dropped: its element does not
/// take the focus for it, and reports no such part.
#[test]
fn a_focus_effect_on_a_turned_off_part_is_dropped() {
    let mut app = grids();
    app.cells = vec!["a", "off"];
    let mut harness = harness(app);
    harness.send(Message::FocusPart("off"));
    harness.lay_out(&mut Mono::default());
    assert_eq!(harness.runtime().focused(), None, "the grid took no focus for it");
    harness.send(Message::FocusPart("a"));
    harness.lay_out(&mut Mono::default());
    assert_eq!(harness.runtime().focused(), Some(&grid()), "an enabled part still does");
}

/// A part that comes and goes keeps no node ID another part can take: the
/// app's table never gives a number twice.
#[test]
fn node_ids_are_never_reused() {
    let mut harness = harness(grids());
    let _ = tree(&harness);
    let b = number(&harness, &grid().with("b"));
    send(&mut harness, Message::Cells(vec!["a"]));
    let nodes = tree(&harness);
    assert!(nodes.nodes.iter().all(|(id, _)| *id != b), "b is gone from the tree");
    harness.app().ids.borrow_mut().retain_seen();
    send(&mut harness, Message::Cells(vec!["a", "c"]));
    let _ = tree(&harness);
    let c = number(&harness, &grid().with("c"));
    assert_ne!(c, b);
}

/// The nodes are built only when the element says they changed or its
/// access version moves, or a layout makes or moves it: never at a frame,
/// a redraw, a layout that changes nothing, or an update that changes
/// nothing it shows.
#[test]
fn nodes_are_built_only_when_they_change() {
    let mut app = grids();
    app.animate = true;
    let mut harness = harness(app);
    assert_eq!(builds(&harness), 1);
    for _ in 0..3 {
        harness.step(DriverEvent::Frame);
        harness.lay_out(&mut Mono::default());
    }
    send(&mut harness, Message::Nothing);
    assert_eq!(builds(&harness), 1);
    send(&mut harness, Message::Cells(vec!["a", "b", "c"]));
    assert_eq!(builds(&harness), 2);
    harness.step(DriverEvent::Woken);
    assert_eq!(builds(&harness), 2);
    harness.step(DriverEvent::Input {
        event: Event::Pointer(PointerEvent::Pressed(PointerButton::Left)),
        pointer: Some(Point::new(20.0, 20.0)),
    });
    assert_eq!(builds(&harness), 3);
}

/// A change of the nodes is told to the app, after the build, so the tree
/// it builds then holds them.
#[test]
fn a_change_is_told_to_the_app() {
    let mut harness = harness(grids());
    harness.step(DriverEvent::Woken);
    let changed =
        |harness: &Harness<Grids, NoTarget>| harness.app().log.iter().filter(|m| **m == Message::Changed).count();
    let before = changed(&harness);
    send(&mut harness, Message::Nothing);
    assert_eq!(
        changed(&harness),
        before,
        "an update that changes nothing it shows is not told"
    );
    send(&mut harness, Message::Cells(vec!["a", "b", "c"]));
    assert_eq!(changed(&harness), before + 1);
    assert!(
        tree(&harness)
            .nodes
            .iter()
            .any(|(id, _)| *id == number(&harness, &grid().with("c")))
    );
}

/// A deadline the tree's change notice makes due ticks in the notice's own
/// turn, as one any other update makes due does (section 9, rule 4), so the
/// loop does not wake again for it.
#[test]
fn a_deadline_the_change_notice_makes_due_ticks_in_its_turn() {
    let mut app = grids();
    app.due_when_changed = true;
    let mut harness = harness(app);
    let ticks = |harness: &Harness<Grids, NoTarget>| {
        harness
            .runtime()
            .custom::<GridState>(&grid())
            .map(|grid| grid.ticks)
            .expect("the grid's state")
    };
    let notices =
        |harness: &Harness<Grids, NoTarget>| harness.app().log.iter().filter(|m| **m == Message::Changed).count();
    let (ticked, told) = (ticks(&harness), notices(&harness));
    // One turn, by hand, at a later moment: the cells change, the nodes are
    // built again, the notice runs, and its deadline ticks, all in it.
    let now = harness.now() + std::time::Duration::from_millis(5);
    let mut driver = crate::headless::Headless::new();
    let runtime = harness.runtime_mut();
    runtime.waker().send(Message::Cells(vec!["a", "b", "c"]));
    let next = runtime.handle(DriverEvent::Woken, now, &mut driver);
    assert_eq!(notices(&harness), told + 1, "the notice ran in the turn");
    assert_eq!(ticks(&harness), ticked + 1, "the grid ticked in the notice's turn");
    assert_eq!(next.deadline, None, "nothing left to wake for");
}

/// The focus is on the part inside the element that has it, as the app
/// hears it.
#[test]
fn the_focus_is_on_the_inner_node() {
    let mut harness = harness(grids());
    send(&mut harness, Message::FocusPart("b"));
    assert_eq!(tree(&harness).focus, Some(number(&harness, &grid().with("b"))));
}

/// A focus on a part the element's last build has no node for is on the
/// element's own node, so the tree never names a node it does not hold.
#[test]
fn a_focus_on_a_part_with_no_node_is_on_the_element() {
    let mut harness = harness(grids());
    send(&mut harness, Message::FocusPart("ghost"));
    assert_eq!(tree(&harness).focus, Some(number(&harness, &grid())));
}

/// A relation from a panel element to a part, and from a part to a panel
/// element, both resolve.
#[test]
fn relations_reach_parts_and_back() {
    let harness = harness(grids());
    let nodes = tree(&harness);
    let inspector = UiId::panel("main").with("inspector");
    let a = grid().with("a");
    assert_eq!(
        node(&nodes, number(&harness, &inspector)).controls(),
        [number(&harness, &a)]
    );
    assert_eq!(
        node(&nodes, number(&harness, &a)).controls(),
        [number(&harness, &inspector)]
    );
}

/// A screen reader's Click on a part reaches the element, and its message
/// runs in that turn; on the element's own node it comes with no path.
#[test]
fn an_action_on_a_part_reaches_the_element() {
    let mut harness = harness(grids());
    let _ = tree(&harness);
    act(&mut harness, &grid().with("b"), accesskit::Action::Click);
    act(&mut harness, &grid(), accesskit::Action::Click);
    let clicked: Vec<_> = harness
        .app()
        .log
        .iter()
        .filter(|m| matches!(m, Message::Clicked(_)))
        .cloned()
        .collect();
    assert_eq!(clicked, [Message::Clicked("b".into()), Message::Clicked(String::new())]);
}

/// A request on a path under the element that it gave no node for is not
/// the element's: while the element takes input, it goes on to the app, which may build nodes of its own
/// there, as the patch canvas builds its tiles' nodes in the app until the
/// canvas gives its own (ui-layer.md, section 9).
#[test]
fn an_action_on_a_part_with_no_node_goes_on_to_the_app() {
    let mut harness = harness(grids());
    let _ = tree(&harness);
    act(&mut harness, &grid().with("not-a-cell"), accesskit::Action::Click);
    assert!(
        !harness.app().log.iter().any(|m| matches!(m, Message::Clicked(_))),
        "the element hears nothing"
    );
    assert!(harness.app().log.contains(&Message::Reader(accesskit::Action::Click)));

    // Under a layer that has the input, the same request is used up: the
    // app hears nothing of it either, as for the element's own nodes.
    send(&mut harness, Message::Overlay(true));
    let heard = harness.app().log.len();
    act(&mut harness, &grid().with("not-a-cell"), accesskit::Action::Click);
    assert!(
        !harness.app().log[heard..].contains(&Message::Reader(accesskit::Action::Click)),
        "used up under the overlay: {:?}",
        &harness.app().log[heard..]
    );
}

/// Focus on a part goes on to the app, as any Focus does, and so does an
/// action the element leaves.
#[test]
fn focus_on_a_part_goes_on_to_the_app() {
    let mut harness = harness(grids());
    let _ = tree(&harness);
    act(&mut harness, &grid().with("b"), accesskit::Action::Focus);
    assert!(harness.app().log.contains(&Message::Reader(accesskit::Action::Focus)));
}

/// An action the node does not offer, or under a layer that takes the
/// input, is used up: the element hears nothing, nor does the app.
#[test]
fn an_action_not_offered_or_under_an_overlay_is_used_up() {
    let mut harness = harness(grids());
    let _ = tree(&harness);
    act(&mut harness, &grid().with("b"), accesskit::Action::Increment);
    send(&mut harness, Message::Overlay(true));
    act(&mut harness, &grid().with("b"), accesskit::Action::Click);
    assert!(
        harness
            .app()
            .log
            .iter()
            .all(|m| !matches!(m, Message::Clicked(_) | Message::Reader(_)))
    );
}

/// A part's node that breaks section 7, rule 2 panics in a debug build.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a node main/grid/broken of a custom element breaks a rule")]
fn a_broken_node_panics() {
    let mut app = grids();
    app.broken = true;
    let _ = harness(app);
}

/// A release build reports a broken part's node as an access problem.
#[test]
fn a_broken_node_is_reported() {
    let mut app = grids();
    app.broken = true;
    let window = WindowState::new(Size::new(400.0, 300.0));
    let mut harness = Harness::new(app, NoTarget, window, Instant::now());
    harness.runtime_mut().allow_mistakes();
    harness.lay_out(&mut Mono::default());
    assert_eq!(
        harness.runtime().custom_access_problems(),
        [(grid().with("broken"), Problem::NotOperable)]
    );
}

/// An element whose nodes follow the app's model but whose access version
/// does not move would give a screen reader stale nodes: a debug build
/// checks each turn, and panics naming the fix.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a custom element's nodes changed but access_version did not")]
fn nodes_that_change_with_no_new_version_panic() {
    let mut app = grids();
    app.stale = true;
    let mut harness = harness(app);
    send(&mut harness, Message::Cells(vec!["a", "b", "c"]));
}

/// A custom element with parts and no Access of its own breaks section 9,
/// rule 2: a release build reports it. One with neither, purely for show,
/// is allowed.
#[test]
fn parts_need_an_access_of_their_own() {
    let mut app = grids();
    app.plain = true;
    let window = WindowState::new(Size::new(400.0, 300.0));
    let mut harness = Harness::new(app, NoTarget, window, Instant::now());
    harness.runtime_mut().allow_mistakes();
    harness.lay_out(&mut Mono::default());
    assert_eq!(
        harness.runtime().custom_access_problems(),
        [(grid(), Problem::PartsWithoutAccess)]
    );
    let mut app = grids();
    app.plain = true;
    app.cells = Vec::new();
    let window = WindowState::new(Size::new(400.0, 300.0));
    let mut harness = Harness::new(app, NoTarget, window, Instant::now());
    harness.lay_out(&mut Mono::default());
    assert!(harness.runtime().custom_access_problems().is_empty());
}

/// In a debug build, the same element panics.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a custom element main/grid breaks a rule: it is a custom element with parts and no Access")]
fn parts_without_an_access_panic() {
    let mut app = grids();
    app.plain = true;
    let _ = harness(app);
}
