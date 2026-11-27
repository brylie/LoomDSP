//! Tests for pointer routing in the runtime's layers (ui-layer.md,
//! section 5, rule 10), with a fake target that answers as a test says.

use crate::access::{Access, Action, Role};
use crate::app::{App, Context, InputState, WindowSpec};
use crate::effect::{Effects, FocusTarget, Selection};
use crate::element::{Element, Length};
use crate::event::{DriverEvent, WindowState};
use crate::headless::Harness;
use crate::layout::tests::{Mono, STYLE};
use crate::routing::{Routed, Target};
use crate::view::{Layer, View};
use loom_ui_core::{Code, Event, Point, PointerButton, PointerEvent, Size, UiId};
use std::time::Instant;

/// What the test app hears.
#[derive(Debug, Clone, PartialEq)]
enum Message {
    /// A click on the element with this UI ID.
    Clicked(UiId),
    /// A key reached the target.
    Key,
    /// Set the input layer.
    Input(Layer),
    /// Focus each of these in turn, in one update.
    Refocus(Vec<UiId>),
    /// The canvas takes the keyboard, or lets it go.
    Canvas(bool),
    /// Tab left the first run of stops for the target's stops after it.
    IntoTarget,
    /// A press outside this panel, which closes on one.
    Outside(UiId),
    /// A key press under the question.
    QuestionKey,
    /// The pointer came onto this element, which reports its hover.
    Hovered(UiId),
    /// The hover this element reported ended.
    Unhovered(UiId),
    /// The bar's second button reports its hover, or stops.
    Reports(bool),
    /// This element's context menu, opened by a key or a screen reader
    /// (`true`) or by a right press (`false`).
    Menu(UiId, bool),
    /// Turn the menu's item off, keeping it a stop.
    OffItem,
    /// Close the overlay, back to the canvas's input layer, and focus this
    /// element, in one update.
    CloseAndFocus(UiId),
}

/// An app that shows a bar of two buttons in the panels layer, across the
/// top, and, when `overlay` is set, a button in the overlays layer over
/// the first.
struct Layers {
    overlay: bool,
    input: Layer,
    /// The canvas has the keyboard.
    canvas: bool,
    /// The bar's two buttons are two runs, `a` and then `b`, with the
    /// target's own stops between them, and the order wraps.
    runs: bool,
    /// The bar's second button reports its hover and has a context menu.
    reports: bool,
    /// A second menu over the first, placed at (100, 50).
    stacked: bool,
    /// The overlay is a dialog, not the menu: an opaque panel with 30 px
    /// of room around its one button, room that is no stop.
    padded: bool,
    /// The menu's item is turned off, and stays a focus stop as the APG
    /// keeps a disabled menu item (`Element::focusable_while_off`).
    off_item: bool,
    heard: Vec<Message>,
}

impl App for Layers {
    type Message = Message;

    fn update(&mut self, message: Message, _cx: &Context<'_, Message>) -> Effects<Message> {
        let mut effects = Effects::none();
        match &message {
            Message::Input(layer) => self.input = *layer,
            Message::Reports(on) => self.reports = *on,
            Message::Canvas(on) => self.canvas = *on,
            Message::OffItem => self.off_item = true,
            Message::CloseAndFocus(id) => {
                self.input = Layer::Canvas;
                effects = Effects::focus(id.clone());
            }
            Message::Refocus(ids) => {
                for id in ids {
                    effects = effects.and(Effects::focus(id.clone()));
                }
            }
            _ => {}
        }
        self.heard.push(message);
        effects
    }

    fn view(&self, view: &mut View) {
        view.panel_with(UiId::panel("bar"), Layer::Panels, &self.reports, |reports| {
            let b = button("b", 40.0);
            let b = if *reports { b.reports_hover().context_menu() } else { b };
            Element::row([button("a", 40.0), b]).width(Length::FILL)
        });
        if self.overlay {
            if self.padded {
                view.panel_with(UiId::panel("dialog"), Layer::Overlays, &(), |_| {
                    Element::column([button("ok", 20.0)]).padding(30.0).opaque()
                });
            } else {
                view.panel_with(UiId::panel("menu"), Layer::Overlays, &self.off_item, |off| {
                    let item = if *off {
                        let states = crate::access::States {
                            disabled: true,
                            ..crate::access::States::default()
                        };
                        Element::text("item", STYLE)
                            .key("item")
                            .width(Length::Fixed(20.0))
                            .min_width(20.0)
                            .focusable_while_off()
                            .access(Access::new(Role::MenuItem, "item").action(Action::Click).states(states))
                    } else {
                        menu_item("item", 20.0)
                    };
                    Element::row([item]).closes_on_press_outside()
                });
            }
        }
        if self.stacked {
            view.panel_with(UiId::panel("sub"), Layer::Overlays, &(), |_| {
                Element::row([menu_item("subitem", 20.0)])
                    .placed(crate::element::Placement::At(Point::new(100.0, 50.0)))
                    .closes_on_press_outside()
            });
        }
    }

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }

    fn input(&self) -> InputState {
        InputState {
            layer: self.input,
            canvas_focused: self.canvas,
            target_first: false,
        }
    }

    fn clicked(&self, id: &UiId) -> Option<Message> {
        Some(Message::Clicked(id.clone()))
    }

    fn pressed_outside(&self, root: &UiId) -> Option<Message> {
        Some(Message::Outside(root.clone()))
    }

    fn hovered(&self, id: &UiId) -> Option<Message> {
        Some(Message::Hovered(id.clone()))
    }

    fn unhovered(&self, id: &UiId) -> Option<Message> {
        Some(Message::Unhovered(id.clone()))
    }

    fn context_menu(&self, id: &UiId, by_key: bool) -> Option<Message> {
        Some(Message::Menu(id.clone(), by_key))
    }

    fn access_target(&self, node: accesskit::NodeId) -> Option<UiId> {
        // Node 2 stands for the bar's second button, node 3 for its first.
        match node.0 {
            2 => Some(UiId::panel("bar").with("b")),
            3 => Some(UiId::panel("bar").with("a")),
            _ => None,
        }
    }

    fn question_key(&self, _key: &loom_ui_core::KeyEvent) -> Option<Message> {
        Some(Message::QuestionKey)
    }

    fn tab_from(&self, from: &UiId, next: Option<&UiId>, back: bool) -> crate::app::TabStep<Message> {
        use crate::app::TabStep;
        let (a, b) = (UiId::panel("bar").with("a"), UiId::panel("bar").with("b"));
        if !self.runs {
            return next.map_or(TabStep::Leave, |next| TabStep::To(next.clone()));
        }
        match (from == &a, back, next) {
            // Forward from the first run: the target's stops come next.
            (true, false, _) => TabStep::Send(Message::IntoTarget),
            // Back from the first: the order wraps to the last.
            (true, true, None) => TabStep::To(b),
            // Forward from the last: it wraps to the first.
            (false, false, None) => TabStep::To(a),
            (_, _, next) => next.map_or(TabStep::Leave, |next| TabStep::To(next.clone())),
        }
    }
}

/// A menu item `width` wide and 16 high, keyed `key`: a button in a
/// list, to a screen reader and to the keyboard.
fn menu_item(key: &str, width: f32) -> Element {
    Element::text(key, STYLE)
        .key(key)
        .width(Length::Fixed(width))
        .min_width(width)
        .focusable()
        .access(Access::new(Role::MenuItem, key).action(Action::Click))
}

/// A button `width` wide and 16 high, keyed `key`.
fn button(key: &str, width: f32) -> Element {
    Element::text(key, STYLE)
        .key(key)
        .width(Length::Fixed(width))
        .min_width(width)
        .focusable()
        .access(Access::new(Role::Button, key).action(Action::Click))
}

/// A target that answers every pointer event with `answer`, except those
/// at `x` of `taken_from` or more, which it takes, as a toolkit's widget at
/// the right of the window would. It records what reached it.
struct Fake {
    answer: Routed,
    taken_from: f32,
    heard: Vec<Event>,
    /// The target's own element with the keyboard, as a test sets it.
    focus: Option<UiId>,
}

impl Target<Message> for Fake {
    fn input(&mut self, event: &Event, pointer: Option<Point>, _now: Instant, out: &mut Vec<Message>) -> Routed {
        self.heard.push(event.clone());
        match event {
            Event::Pointer(_) if pointer.is_some_and(|at| at.x >= self.taken_from) => Routed::Taken,
            Event::Pointer(_) => self.answer,
            _ => {
                out.push(Message::Key);
                Routed::Taken
            }
        }
    }

    fn focus(&mut self, _target: &FocusTarget, _select: Selection) {}

    fn scroll_into_view(&mut self, _id: &UiId) {}

    fn focused(&self) -> Option<UiId> {
        self.focus.clone()
    }

    fn needs_redraw(&mut self, _updates: usize) -> bool {
        false
    }
}

/// A harness with the bar, the overlay when `overlay` is set, and a target
/// that answers `answer`, laid out in a 400 by 200 window.
fn harness(overlay: bool, answer: Routed) -> Harness<Layers, Fake> {
    let app = Layers {
        overlay,
        input: Layer::Canvas,
        canvas: false,
        runs: false,
        reports: false,
        stacked: false,
        padded: false,
        off_item: false,
        heard: Vec::new(),
    };
    let target = Fake {
        answer,
        taken_from: 350.0,
        heard: Vec::new(),
        focus: None,
    };
    let window = WindowState::new(Size::new(400.0, 200.0));
    let mut harness = Harness::new(app, target, window, Instant::now());
    harness.lay_out(&mut Mono::default());
    harness
}

/// A pointer event at `(x, y)`.
fn at(x: f32, y: f32, event: PointerEvent) -> DriverEvent {
    DriverEvent::Input {
        event: Event::Pointer(event),
        pointer: Some(Point::new(x, y)),
    }
}

/// A left click at `(x, y)`: press, then release.
fn click(harness: &mut Harness<Layers, Fake>, x: f32, y: f32) {
    harness.step(at(x, y, PointerEvent::Pressed(PointerButton::Left)));
    harness.step(at(x, y, PointerEvent::Released(PointerButton::Left)));
}

/// A key press, which the fake target takes and turns into `Key`.
fn key() -> DriverEvent {
    DriverEvent::Input {
        event: Event::Key(loom_ui_core::KeyEvent::Pressed {
            key: loom_ui_core::Key::Character('k'),
            modifiers: loom_ui_core::Modifiers::NONE,
            repeat: false,
            text: Some("k".into()),
            code: Code::Other,
        }),
        pointer: None,
    }
}

/// The clicks the app heard.
fn clicks(harness: &Harness<Layers, Fake>) -> Vec<UiId> {
    harness
        .app()
        .heard
        .iter()
        .filter_map(|message| match message {
            Message::Clicked(id) => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// The order: a layer above the target takes a point before the target,
/// and the target before a layer below it. The overlay's button is over
/// the bar's first, so a click there is the overlay's and the target never
/// hears it.
#[test]
fn layers_above_the_target_come_first() {
    let mut harness = harness(true, Routed::Passed);
    click(&mut harness, 5.0, 5.0);
    assert_eq!(clicks(&harness), [UiId::panel("menu").with("item")]);
    assert!(harness.runtime().target().heard.is_empty(), "the target heard nothing");
    assert_eq!(
        harness.runtime().focused(),
        None,
        "a press on a button takes no focus (section 6, rule 3)"
    );
}

/// What the target answers decides the layers below it: Taken stops the
/// event, Passed hands it on, and Later leaves it for the driver.
#[test]
fn the_target_answer_decides_the_layers_below() {
    let mut taken = harness(false, Routed::Taken);
    click(&mut taken, 5.0, 5.0);
    assert!(clicks(&taken).is_empty(), "taken by the target");
    assert_eq!(taken.runtime().target().heard.len(), 2);

    let mut passed = harness(false, Routed::Passed);
    click(&mut passed, 5.0, 5.0);
    assert_eq!(clicks(&passed), [UiId::panel("bar").with("a")]);
    assert_eq!(passed.runtime().focused(), None, "a button takes no focus from a press");

    let mut later = harness(false, Routed::Later);
    click(&mut later, 5.0, 5.0);
    assert!(clicks(&later).is_empty(), "nothing until the driver hands it on");
    assert_eq!(later.runtime().focused(), None);
}

/// A late answer goes through `pointer_below`, the same step the runtime
/// runs for Passed: a press and a release there click, and the message
/// runs on the next turn, once, before a key that comes after it.
#[test]
fn pointer_below_runs_the_same_step_for_a_late_answer() {
    let mut harness = harness(false, Routed::Later);
    for event in [
        PointerEvent::Pressed(PointerButton::Left),
        PointerEvent::Released(PointerButton::Left),
    ] {
        let (_, _, mut panels) = harness.parts();
        let heard = panels.pointer_below(&Event::Pointer(event), Point::new(45.0, 5.0));
        assert!(heard.taken);
    }
    assert!(harness.app().heard.is_empty(), "not in the turn that found it");
    assert_eq!(
        harness.runtime().focused(),
        None,
        "a button takes no focus from a press"
    );
    harness.step(key());
    assert_eq!(
        harness.app().heard,
        [Message::Clicked(UiId::panel("bar").with("b")), Message::Key],
        "the click once, then the key"
    );
}

/// `over_target` agrees with the first step: true over the overlay's
/// button, false over the bar below the target and over nothing.
#[test]
fn over_target_agrees_with_the_layers_above() {
    let mut harness = harness(true, Routed::Passed);
    let (_, _, panels) = harness.parts();
    assert!(panels.over_target(&Event::Pointer(PointerEvent::Moved), Point::new(5.0, 5.0)));
    assert!(
        !panels.over_target(&Event::Pointer(PointerEvent::Moved), Point::new(45.0, 5.0)),
        "the bar is below"
    );
    assert!(!panels.over_target(&Event::Pointer(PointerEvent::Moved), Point::new(300.0, 100.0)));
}

/// The press holds while the pointer leaves the element and comes back,
/// so letting go on it still clicks, as on macOS and Windows (Brylie's
/// decision, 2026-11-17), over nothing of ours or over a widget of the
/// target's that takes the move. Leaving the window ends the press.
#[test]
fn a_press_holds_while_the_pointer_leaves_and_comes_back() {
    let mut harness = harness(false, Routed::Passed);
    harness.step(at(5.0, 5.0, PointerEvent::Pressed(PointerButton::Left)));
    harness.step(at(300.0, 100.0, PointerEvent::Moved));
    harness.step(at(45.0, 5.0, PointerEvent::Moved));
    harness.step(at(5.0, 5.0, PointerEvent::Moved));
    harness.step(at(5.0, 5.0, PointerEvent::Released(PointerButton::Left)));
    assert_eq!(clicks(&harness), [UiId::panel("bar").with("a")]);

    // Across a widget of the target's that takes the move, and back.
    harness.step(at(5.0, 5.0, PointerEvent::Pressed(PointerButton::Left)));
    harness.step(at(380.0, 100.0, PointerEvent::Moved));
    assert_eq!(harness.runtime().hovered(), None, "the target holds the hover");
    harness.step(at(5.0, 5.0, PointerEvent::Moved));
    harness.step(at(5.0, 5.0, PointerEvent::Released(PointerButton::Left)));
    assert_eq!(clicks(&harness).len(), 2, "still a click, once");

    harness.step(at(5.0, 5.0, PointerEvent::Pressed(PointerButton::Left)));
    harness.step(DriverEvent::Input {
        event: Event::Pointer(PointerEvent::LeftWindow),
        pointer: None,
    });
    harness.step(at(5.0, 5.0, PointerEvent::Released(PointerButton::Left)));
    assert_eq!(clicks(&harness).len(), 2, "leaving the window ended the press");
}

/// A release on another element than the press clicks nothing: letting go
/// anywhere else cancels a button's press (Brylie's decision, 2026-11-17).
#[test]
fn a_release_elsewhere_clicks_nothing() {
    let mut harness = harness(false, Routed::Passed);
    harness.step(at(5.0, 5.0, PointerEvent::Pressed(PointerButton::Left)));
    harness.step(at(45.0, 5.0, PointerEvent::Released(PointerButton::Left)));
    harness.step(at(300.0, 100.0, PointerEvent::Pressed(PointerButton::Left)));
    harness.step(at(5.0, 5.0, PointerEvent::Released(PointerButton::Left)));
    assert!(clicks(&harness).is_empty());
}

/// A release the target takes cancels the press too, as a release over
/// nothing does: a press on our button, a release on the target's widget,
/// then a press there and a release on our button click nothing. The same
/// for a late answer the bridge hands back as taken (`pointer_taken`).
#[test]
fn a_release_the_target_takes_cancels_the_press() {
    let mut passed = harness(false, Routed::Passed);
    passed.step(at(5.0, 5.0, PointerEvent::Pressed(PointerButton::Left)));
    passed.step(at(380.0, 100.0, PointerEvent::Released(PointerButton::Left)));
    passed.step(at(380.0, 100.0, PointerEvent::Pressed(PointerButton::Left)));
    passed.step(at(5.0, 5.0, PointerEvent::Released(PointerButton::Left)));
    assert!(clicks(&passed).is_empty(), "{:?}", clicks(&passed));

    let mut late = harness(false, Routed::Later);
    let press = Event::Pointer(PointerEvent::Pressed(PointerButton::Left));
    let release = Event::Pointer(PointerEvent::Released(PointerButton::Left));
    let on_a = Point::new(5.0, 5.0);
    let (_, _, mut panels) = late.parts();
    assert!(panels.pointer_below(&press, on_a).taken);
    let _ = panels.pointer_taken(&release);
    let _ = panels.pointer_taken(&press);
    assert!(panels.pointer_below(&release, on_a).taken);
    late.step(key());
    assert!(clicks(&late).is_empty(), "{:?}", clicks(&late));
}

/// One layer holds the hover. A move onto a button hovers it, a move the
/// target takes clears it, a move over nothing of ours clears it, and the
/// pointer leaving the window clears it. Hover runs no update.
#[test]
fn one_layer_holds_the_hover() {
    let mut harness = harness(false, Routed::Passed);
    let next = harness.step(at(5.0, 5.0, PointerEvent::Moved));
    assert_eq!(harness.runtime().hovered(), Some(&UiId::panel("bar").with("a")));
    assert!(next.draw, "the hover redraws");
    assert!(harness.app().heard.is_empty(), "and runs no update");
    harness.step(at(300.0, 100.0, PointerEvent::Moved));
    assert_eq!(harness.runtime().hovered(), None, "over nothing of ours");

    harness.step(at(5.0, 5.0, PointerEvent::Moved));
    harness.step(DriverEvent::Input {
        event: Event::Pointer(PointerEvent::LeftWindow),
        pointer: None,
    });
    assert_eq!(harness.runtime().hovered(), None, "the pointer left");

    let mut taken = self::harness(false, Routed::Taken);
    taken.step(at(5.0, 5.0, PointerEvent::Moved));
    assert_eq!(taken.runtime().hovered(), None, "the target took the move");
}

/// A layer below the input layer takes nothing and keeps no hover: with
/// the input on the overlays, the bar's buttons are neither hovered nor
/// clicked, and the event goes to the target. A hover there clears when
/// the input layer rises above it.
#[test]
fn nothing_below_the_input_layer_takes_the_pointer() {
    let mut harness = harness(false, Routed::Passed);
    harness.step(at(5.0, 5.0, PointerEvent::Moved));
    assert!(harness.runtime().hovered().is_some());
    harness.send(Message::Input(Layer::Overlays));
    assert_eq!(harness.runtime().hovered(), None, "cleared as the input rose");
    click(&mut harness, 5.0, 5.0);
    harness.step(at(5.0, 5.0, PointerEvent::Moved));
    assert!(clicks(&harness).is_empty());
    assert_eq!(harness.runtime().hovered(), None);
    assert_eq!(harness.runtime().focused(), None);
    assert_eq!(harness.runtime().target().heard.len(), 4, "the target heard every one");
}

/// Tab enters the runtime's first stop with nothing focused, but not while
/// the input layer is above the panels.
#[test]
fn tab_enters_from_nothing_only_while_the_panels_take_input() {
    let tab = DriverEvent::Input {
        event: Event::Key(loom_ui_core::KeyEvent::Pressed {
            key: loom_ui_core::Key::Tab,
            modifiers: loom_ui_core::Modifiers::NONE,
            repeat: false,
            text: None,
            code: Code::Other,
        }),
        pointer: None,
    };
    let mut harness = harness(false, Routed::Passed);
    harness.send(Message::Input(Layer::Question));
    harness.step(tab.clone());
    assert_eq!(harness.runtime().focused(), None, "a question holds the window");
    harness.send(Message::Input(Layer::Canvas));
    harness.step(tab);
    assert_eq!(harness.runtime().focused(), Some(&UiId::panel("bar").with("a")));
    assert!(
        harness.runtime().focus_ring().is_some(),
        "from the keyboard, with a ring"
    );
}

/// Tab moves only among the stops of the layers that take input. With
/// focus on the bar and the input raised to the overlays, Tab goes to the
/// overlay's button, not to the bar's next one. With no stop of the input
/// layer, the focus below is left as it is and the key goes on.
#[test]
fn tab_skips_the_stops_below_the_input_layer() {
    let tab = DriverEvent::Input {
        event: Event::Key(loom_ui_core::KeyEvent::Pressed {
            key: loom_ui_core::Key::Tab,
            modifiers: loom_ui_core::Modifiers::NONE,
            repeat: false,
            text: None,
            code: Code::Other,
        }),
        pointer: None,
    };
    let mut menu = harness(true, Routed::Passed);
    menu.send(Message::Input(Layer::Canvas));
    menu.step(tab.clone());
    assert_eq!(menu.runtime().focused(), Some(&UiId::panel("menu").with("item")));
    menu.step(tab.clone());
    assert_eq!(menu.runtime().focused(), Some(&UiId::panel("bar").with("a")));
    menu.send(Message::Input(Layer::Overlays));
    menu.step(tab.clone());
    assert_eq!(
        menu.runtime().focused(),
        Some(&UiId::panel("menu").with("item")),
        "the overlay's button, not the bar's next"
    );

    let mut bar = harness(false, Routed::Passed);
    bar.send(Message::Input(Layer::Canvas));
    bar.step(tab.clone());
    assert_eq!(bar.runtime().focused(), Some(&UiId::panel("bar").with("a")));
    bar.send(Message::Input(Layer::Overlays));
    bar.step(tab);
    assert_eq!(
        bar.runtime().focused(),
        Some(&UiId::panel("bar").with("a")),
        "left as it was"
    );
    assert_eq!(bar.app().heard.last(), Some(&Message::Key), "the key went on");
}

/// One element has focus across the window (section 6, rule 1): when the
/// target's own field takes the keyboard, the runtime lets go of its
/// focus, so the context names the field. The target's focus going to
/// nothing changes nothing of the runtime's.
#[test]
fn the_targets_field_taking_the_keyboard_ends_our_focus() {
    let tab = DriverEvent::Input {
        event: Event::Key(loom_ui_core::KeyEvent::Pressed {
            key: loom_ui_core::Key::Tab,
            modifiers: loom_ui_core::Modifiers::NONE,
            repeat: false,
            text: None,
            code: Code::Other,
        }),
        pointer: None,
    };
    let a = UiId::panel("bar").with("a");
    let mut harness = harness(false, Routed::Passed);
    harness.step(tab.clone());
    assert_eq!(harness.runtime().focused(), Some(&a));
    let field = UiId::panel("toolkit").with("field");
    harness.parts().1.focus = Some(field);
    harness.step(DriverEvent::Woken);
    assert_eq!(harness.runtime().focused(), None, "the field has the keyboard now");
    assert_eq!(harness.runtime().focus_ring(), None);

    harness.parts().1.focus = None;
    harness.step(tab);
    assert_eq!(harness.runtime().focused(), Some(&a));
    harness.step(DriverEvent::Woken);
    assert_eq!(
        harness.runtime().focused(),
        Some(&a),
        "no new focus of the target's, so ours stays"
    );
}

/// A key event for the focused element: `key` down, or up when `up`.
fn press_key(key: loom_ui_core::Key, repeat: bool) -> DriverEvent {
    DriverEvent::Input {
        event: Event::Key(loom_ui_core::KeyEvent::Pressed {
            key,
            modifiers: loom_ui_core::Modifiers::NONE,
            repeat,
            text: None,
            code: Code::Other,
        }),
        pointer: None,
    }
}

/// `key` coming up.
fn release_key(key: loom_ui_core::Key) -> DriverEvent {
    DriverEvent::Input {
        event: Event::Key(loom_ui_core::KeyEvent::Released { key, code: Code::Other }),
        pointer: None,
    }
}

/// True when the target heard `key` go down.
fn target_heard_press(harness: &Harness<Layers, Fake>, key: loom_ui_core::Key) -> bool {
    harness
        .runtime()
        .target()
        .heard
        .iter()
        .any(|event| matches!(event, Event::Key(loom_ui_core::KeyEvent::Pressed { key: got, .. }) if *got == key))
}

/// The bar's first button, focused from the keyboard.
fn focused_on_a() -> Harness<Layers, Fake> {
    let mut harness = harness(false, Routed::Passed);
    harness.send(Message::Input(Layer::Canvas));
    harness.step(press_key(loom_ui_core::Key::Tab, false));
    assert_eq!(harness.runtime().focused(), Some(&UiId::panel("bar").with("a")));
    harness
}

/// Section 3, rule 4: Enter clicks a focused button when it goes down, once
/// however long it is held, and the target never hears it. The click comes
/// on the next turn, as a pointer's does.
#[test]
fn enter_clicks_a_focused_button_on_key_down() {
    use loom_ui_core::Key;
    let mut harness = focused_on_a();
    harness.step(press_key(Key::Enter, false));
    assert_eq!(clicks(&harness), [UiId::panel("bar").with("a")]);
    harness.step(press_key(Key::Enter, true));
    harness.step(release_key(Key::Enter));
    harness.step(DriverEvent::Woken);
    assert_eq!(clicks(&harness).len(), 1, "a repeat clicks nothing");
    assert!(
        !target_heard_press(&harness, Key::Enter),
        "the target never heard Enter go down"
    );
}

/// Space clicks when it comes up on the element it went down on, once.
/// Escape, a focus move, or a pointer press while it is held cancels it.
#[test]
fn space_clicks_on_release_and_can_be_cancelled() {
    use loom_ui_core::Key;
    let a = UiId::panel("bar").with("a");
    let mut harness = focused_on_a();
    harness.step(press_key(Key::Space, false));
    harness.step(press_key(Key::Space, true));
    harness.step(DriverEvent::Woken);
    assert!(clicks(&harness).is_empty(), "nothing on the way down");
    harness.step(release_key(Key::Space));
    harness.step(DriverEvent::Woken);
    assert_eq!(clicks(&harness), std::slice::from_ref(&a));

    // Escape while held.
    harness.step(press_key(Key::Space, false));
    harness.step(press_key(Key::Escape, false));
    harness.step(release_key(Key::Space));
    harness.step(DriverEvent::Woken);
    assert_eq!(clicks(&harness).len(), 1, "Escape cancelled it");
    assert!(
        !target_heard_press(&harness, Key::Space),
        "the target heard no Space go down"
    );
    assert!(
        !target_heard_press(&harness, Key::Escape),
        "nor the Escape that cancelled it"
    );

    // A focus move while held: the release on the next button clicks nothing.
    harness.step(press_key(Key::Space, false));
    harness.step(press_key(Key::Tab, false));
    assert_eq!(harness.runtime().focused(), Some(&UiId::panel("bar").with("b")));
    harness.step(release_key(Key::Space));
    harness.step(DriverEvent::Woken);
    assert_eq!(clicks(&harness).len(), 1, "a focus move cancelled it");

    // A pointer press while held, over nothing of ours.
    let mut harness = focused_on_a();
    harness.step(press_key(Key::Space, false));
    harness.step(at(300.0, 100.0, PointerEvent::Pressed(PointerButton::Right)));
    harness.step(release_key(Key::Space));
    harness.step(DriverEvent::Woken);
    assert!(clicks(&harness).is_empty(), "a pointer press cancelled it");
}

/// Space clicks only if what it went down on can still be clicked when it
/// comes up: an update that raises the input layer above the bar while
/// Space is held leaves the release clicking nothing.
#[test]
fn space_released_after_the_input_layer_rose_clicks_nothing() {
    use loom_ui_core::Key;
    let mut harness = focused_on_a();
    harness.step(press_key(Key::Space, false));
    harness.send(Message::Input(Layer::Overlays));
    harness.step(release_key(Key::Space));
    harness.step(DriverEvent::Woken);
    assert!(clicks(&harness).is_empty(), "{:?}", clicks(&harness));
}

/// Any move of the focus while Space is held cancels the click, whatever
/// moved it: one update that focuses B and then A again clicks nothing. An
/// update that focuses A while A has it moves nothing, so that one clicks.
#[test]
fn any_focus_move_while_space_is_held_cancels_the_click() {
    use loom_ui_core::Key;
    let a = UiId::panel("bar").with("a");
    let b = UiId::panel("bar").with("b");
    let mut harness = focused_on_a();
    harness.step(press_key(Key::Space, false));
    harness.send(Message::Refocus(vec![b, a.clone()]));
    assert_eq!(harness.runtime().focused(), Some(&a), "back on A");
    harness.step(release_key(Key::Space));
    harness.step(DriverEvent::Woken);
    assert!(clicks(&harness).is_empty(), "B and back cancelled it");

    harness.step(press_key(Key::Space, false));
    harness.send(Message::Refocus(vec![a.clone()]));
    harness.step(release_key(Key::Space));
    harness.step(DriverEvent::Woken);
    assert_eq!(clicks(&harness), std::slice::from_ref(&a), "A again moved nothing");
}

/// A pointer press cancels a held Space even when it comes with no
/// position to route by.
#[test]
fn a_press_with_no_position_cancels_a_held_space() {
    use loom_ui_core::Key;
    let mut harness = focused_on_a();
    harness.step(press_key(Key::Space, false));
    harness.step(DriverEvent::Input {
        event: Event::Pointer(PointerEvent::Pressed(PointerButton::Left)),
        pointer: None,
    });
    harness.step(release_key(Key::Space));
    harness.step(DriverEvent::Woken);
    assert!(clicks(&harness).is_empty(), "{:?}", clicks(&harness));
}

/// The pointer leaving the window keeps a held Space: it is the keyboard's,
/// and no focus moved, so the release still clicks.
#[test]
fn the_pointer_leaving_keeps_a_held_space() {
    use loom_ui_core::Key;
    let mut harness = focused_on_a();
    harness.step(press_key(Key::Space, false));
    harness.step(DriverEvent::Input {
        event: Event::Pointer(PointerEvent::LeftWindow),
        pointer: None,
    });
    harness.step(release_key(Key::Space));
    harness.step(DriverEvent::Woken);
    assert_eq!(clicks(&harness), [UiId::panel("bar").with("a")]);
}

/// In a menu, Escape closes the menu at once (section 5, rule 11): with a
/// Space held on an item, one Escape cancels that Space and goes on to
/// the target, which closes the menu, and the release clicks nothing.
#[test]
fn escape_with_space_held_on_a_menu_item_cancels_and_goes_on() {
    use loom_ui_core::Key;
    let item = UiId::panel("menu").with("item");
    let mut harness = harness(true, Routed::Passed);
    harness.send(Message::Input(Layer::Overlays));
    harness.send(Message::Refocus(vec![item.clone()]));
    assert_eq!(harness.runtime().focused(), Some(&item));
    harness.step(press_key(Key::Space, false));
    harness.step(press_key(Key::Escape, false));
    assert!(
        target_heard_press(&harness, Key::Escape),
        "Escape went on, to close the menu"
    );
    harness.step(release_key(Key::Space));
    harness.step(DriverEvent::Woken);
    assert!(clicks(&harness).is_empty(), "Escape cancelled the Space");
}

/// Escape with a Space that could no longer click (a layer rose above its
/// button) drops it without taking Escape, which goes on to the target.
#[test]
fn escape_with_a_stale_space_goes_on() {
    use loom_ui_core::Key;
    let mut harness = focused_on_a();
    harness.step(press_key(Key::Space, false));
    harness.send(Message::Input(Layer::Overlays));
    harness.step(press_key(Key::Escape, false));
    assert!(target_heard_press(&harness, Key::Escape), "the target heard Escape");
    harness.step(release_key(Key::Space));
    harness.step(DriverEvent::Woken);
    assert!(clicks(&harness).is_empty());
}

/// While the canvas has the keyboard, Space and Enter are its own: a
/// button of ours that still has the focus is not activated by them.
#[test]
fn keys_go_to_the_canvas_while_it_has_the_keyboard() {
    use loom_ui_core::Key;
    let mut harness = focused_on_a();
    harness.send(Message::Canvas(true));
    harness.step(press_key(Key::Enter, false));
    harness.step(press_key(Key::Space, false));
    harness.step(release_key(Key::Space));
    harness.step(DriverEvent::Woken);
    assert!(clicks(&harness).is_empty(), "{:?}", clicks(&harness));
    assert!(target_heard_press(&harness, Key::Enter) && target_heard_press(&harness, Key::Space));
}

/// A repeat of Enter or Space is taken and clicks nothing, and draws no
/// frame either.
#[test]
fn an_ignored_repeat_draws_no_frame() {
    use loom_ui_core::Key;
    let mut harness = focused_on_a();
    harness.step(press_key(Key::Enter, false));
    harness.step(DriverEvent::Woken);
    let next = harness.step(press_key(Key::Enter, true));
    assert!(!next.draw, "a repeat changed nothing that shows");
    assert!(
        !target_heard_press(&harness, Key::Enter),
        "and is still kept from the target"
    );
}

/// Steps `event` and says whether any turn of it drew a frame: a click
/// queues the app's message for a turn of its own, so the last turn's
/// `Next` is not the one to read.
fn drew(harness: &mut Harness<Layers, Fake>, event: DriverEvent) -> bool {
    let before = harness.driver().redraws();
    harness.step(event);
    harness.driver().redraws() > before
}

/// What shows as pressed now, and the panels laid out so far.
fn pressed_and_layouts(harness: &mut Harness<Layers, Fake>) -> (Option<UiId>, u64) {
    let laid_out = harness.runtime().counts().panels_laid_out;
    let (_, _, panels) = harness.parts();
    (panels.shown_pressed(), laid_out)
}

/// A press, a move off and back, and a release each change what shows as
/// pressed, and each asks for a frame without laying a panel out again:
/// the look is chosen when painting (visual-system.md, Role tokens).
#[test]
fn a_change_of_the_press_draws_without_a_rebuild() {
    let mut harness = harness(false, Routed::Passed);
    let a = UiId::panel("bar").with("a");
    harness.step(at(5.0, 5.0, PointerEvent::Moved));
    let (_, before) = pressed_and_layouts(&mut harness);
    let drawn = drew(&mut harness, at(5.0, 5.0, PointerEvent::Pressed(PointerButton::Left)));
    assert!(drawn, "the press asks for a frame");
    assert_eq!(
        pressed_and_layouts(&mut harness),
        (Some(a.clone()), before),
        "pressed, no layout"
    );
    let drawn = drew(&mut harness, at(5.0, 150.0, PointerEvent::Moved));
    assert!(drawn, "off the element: no longer shown pressed");
    assert_eq!(pressed_and_layouts(&mut harness).0, None);
    let drawn = drew(&mut harness, at(5.0, 5.0, PointerEvent::Moved));
    assert!(drawn, "back on it: pressed again");
    assert_eq!(pressed_and_layouts(&mut harness).0, Some(a));
    let drawn = drew(&mut harness, at(5.0, 5.0, PointerEvent::Released(PointerButton::Left)));
    assert!(drawn, "the release asks for a frame");
    assert_eq!(pressed_and_layouts(&mut harness), (None, before), "released, no layout");
}

/// A press cancelled by the pointer leaving the window asks for a frame,
/// so the pressed look does not stay.
#[test]
fn a_cancelled_press_draws() {
    let mut harness = harness(false, Routed::Passed);
    harness.step(at(5.0, 5.0, PointerEvent::Pressed(PointerButton::Left)));
    let drawn = drew(
        &mut harness,
        DriverEvent::Input {
            event: Event::Pointer(PointerEvent::LeftWindow),
            pointer: None,
        },
    );
    assert!(drawn);
    assert_eq!(pressed_and_layouts(&mut harness).0, None);
}

/// A held Space on a focused button shows it pressed until Space comes
/// up, and both ask for a frame; a repeat asks for none.
#[test]
fn a_held_space_shows_pressed() {
    let mut harness = harness(false, Routed::Passed);
    let a = UiId::panel("bar").with("a");
    harness.step(press_key(loom_ui_core::Key::Tab, false));
    assert_eq!(harness.runtime().focused(), Some(&a));
    let drawn = drew(&mut harness, press_key(loom_ui_core::Key::Space, false));
    assert!(drawn, "Space down asks for a frame");
    assert_eq!(pressed_and_layouts(&mut harness).0, Some(a.clone()));
    let drawn = drew(&mut harness, press_key(loom_ui_core::Key::Space, true));
    assert!(!drawn, "a repeat changes nothing");
    let drawn = drew(&mut harness, release_key(loom_ui_core::Key::Space));
    assert!(drawn, "Space up asks for a frame");
    assert_eq!(pressed_and_layouts(&mut harness).0, None);
    assert_eq!(clicks(&harness), [a], "and it clicked");
}

/// A key going down with Shift held.
fn press_shifted(key: loom_ui_core::Key) -> DriverEvent {
    DriverEvent::Input {
        event: Event::Key(loom_ui_core::KeyEvent::Pressed {
            key,
            modifiers: loom_ui_core::Modifiers {
                shift: true,
                ..loom_ui_core::Modifiers::NONE
            },
            repeat: false,
            text: None,
            code: Code::Other,
        }),
        pointer: None,
    }
}

/// The app decides where Tab goes at the ends of a run of the runtime's
/// stops (ui-layer.md, section 6, rule 3). Forward out of the first run,
/// its message runs on the next turn, the runtime's focus clears, and the
/// key goes no further. Past the last stop the order wraps to the first,
/// and Shift+Tab from the first wraps to the last.
#[test]
fn the_app_decides_tab_at_the_end_of_a_run() {
    let app = Layers {
        overlay: false,
        input: Layer::Canvas,
        canvas: false,
        runs: true,
        reports: false,
        stacked: false,
        padded: false,
        off_item: false,
        heard: Vec::new(),
    };
    let target = Fake {
        answer: Routed::Passed,
        taken_from: 350.0,
        heard: Vec::new(),
        focus: None,
    };
    let mut harness = Harness::new(app, target, WindowState::new(Size::new(400.0, 200.0)), Instant::now());
    harness.lay_out(&mut Mono::default());
    let (a, b) = (UiId::panel("bar").with("a"), UiId::panel("bar").with("b"));
    let tab = || press_key(loom_ui_core::Key::Tab, false);
    harness.step(tab());
    assert_eq!(
        harness.runtime().focused(),
        Some(&a),
        "Tab from nothing enters the first stop"
    );
    harness.step(tab());
    assert_eq!(harness.runtime().focused(), None, "out of the first run");
    assert!(
        harness.app().heard.contains(&Message::IntoTarget),
        "the app's message ran"
    );
    assert!(
        !target_heard_press(&harness, loom_ui_core::Key::Tab),
        "the key went no further"
    );
    harness.step(press_shifted(loom_ui_core::Key::Tab));
    assert_eq!(
        harness.runtime().focused(),
        Some(&b),
        "Shift+Tab from nothing enters the last stop"
    );
    harness.step(tab());
    assert_eq!(
        harness.runtime().focused(),
        Some(&a),
        "past the last stop, the order wraps"
    );
    harness.step(press_shifted(loom_ui_core::Key::Tab));
    assert_eq!(
        harness.runtime().focused(),
        Some(&b),
        "Shift+Tab from the first wraps to the last"
    );
}

/// A held Space shows pressed only while letting go would click: when an
/// update raises the input layer above the bar, as a dialog opening does,
/// the button no longer looks pressed, and Space coming up clicks nothing.
#[test]
fn a_held_space_stops_looking_pressed_when_a_layer_rises() {
    let mut harness = harness(false, Routed::Passed);
    let a = UiId::panel("bar").with("a");
    harness.step(press_key(loom_ui_core::Key::Tab, false));
    harness.step(press_key(loom_ui_core::Key::Space, false));
    assert_eq!(
        pressed_and_layouts(&mut harness).0,
        Some(a),
        "pressed while it would click"
    );
    let drawn = {
        let before = harness.driver().redraws();
        harness.send(Message::Input(Layer::Overlays));
        harness.driver().redraws() > before
    };
    assert!(drawn, "the update that raised the layer drew");
    assert_eq!(pressed_and_layouts(&mut harness).0, None, "no longer pressed");
    harness.step(release_key(loom_ui_core::Key::Space));
    assert!(clicks(&harness).is_empty(), "and letting go clicks nothing");
}

/// A press on a button leaves the focus where it was (section 6, rule 3;
/// Brylie's decision, 2026-11-19): a button that Tab focused keeps the
/// focus when another button is clicked, and the click still lands, with
/// the pressed look while it is held.
#[test]
fn a_press_on_a_button_leaves_the_focus_where_it_was() {
    let mut harness = harness(false, Routed::Passed);
    let (a, b) = (UiId::panel("bar").with("a"), UiId::panel("bar").with("b"));
    harness.step(press_key(loom_ui_core::Key::Tab, false));
    assert_eq!(harness.runtime().focused(), Some(&a));
    harness.step(at(45.0, 5.0, PointerEvent::Moved));
    harness.step(at(45.0, 5.0, PointerEvent::Pressed(PointerButton::Left)));
    assert_eq!(pressed_and_layouts(&mut harness).0, Some(b.clone()), "b looks pressed");
    assert_eq!(harness.runtime().focused(), Some(&a), "a keeps the focus");
    harness.step(at(45.0, 5.0, PointerEvent::Released(PointerButton::Left)));
    assert_eq!(clicks(&harness), [b], "the click landed on b");
    assert_eq!(harness.runtime().focused(), Some(&a), "and a still has it");
}

/// A late answer goes through `pointer_below`, outside a turn, so it
/// reports a change of what shows as pressed itself: a left press on a
/// button already hovered, which moves neither the hover nor the focus,
/// still asks for a frame, and so does its release. A move that changes
/// nothing asks for none.
#[test]
fn pointer_below_reports_a_change_of_the_press() {
    let mut harness = harness(false, Routed::Later);
    let at = Point::new(45.0, 5.0);
    let mut below = |event: PointerEvent| {
        let (_, _, mut panels) = harness.parts();
        panels.pointer_below(&Event::Pointer(event), at)
    };
    assert!(below(PointerEvent::Moved).redraw, "the hover moved");
    assert!(!below(PointerEvent::Moved).redraw, "nothing changed");
    assert!(below(PointerEvent::Pressed(PointerButton::Left)).redraw, "pressed");
    assert!(below(PointerEvent::Released(PointerButton::Left)).redraw, "released");
}

/// Above the target, as below it: a press on a menu item already hovered,
/// which moves neither the hover nor the focus, asks for a frame for its
/// pressed look, and so does its release.
#[test]
fn pointer_above_target_reports_a_change_of_the_press() {
    let mut harness = harness(true, Routed::Later);
    harness.send(Message::Input(Layer::Overlays));
    let at = Point::new(5.0, 5.0);
    let mut above = |event: PointerEvent| {
        let (_, _, mut panels) = harness.parts();
        panels.pointer_above_target(&Event::Pointer(event), at)
    };
    assert!(above(PointerEvent::Moved).redraw, "the hover moved");
    assert!(!above(PointerEvent::Moved).redraw, "nothing changed");
    assert!(above(PointerEvent::Pressed(PointerButton::Left)).redraw, "pressed");
    assert!(above(PointerEvent::Released(PointerButton::Left)).redraw, "released");
}

/// When the other toolkit takes a pointer press, it cancels a held Space,
/// so the button no longer looks pressed, and that asks for a frame, though
/// the hover, over the other toolkit, did not change.
#[test]
fn a_press_the_other_toolkit_takes_reports_the_lost_press() {
    let mut harness = harness(false, Routed::Later);
    harness.step(press_key(loom_ui_core::Key::Tab, false));
    harness.step(press_key(loom_ui_core::Key::Space, false));
    let (_, _, mut panels) = harness.parts();
    assert!(panels.shown_pressed().is_some(), "Space shows the button pressed");
    assert!(panels.pointer.hovered.is_none(), "the pointer is not over it");
    let heard = panels.pointer_taken(&Event::Pointer(PointerEvent::Pressed(PointerButton::Left)));
    assert!(panels.shown_pressed().is_none(), "the press cancelled the Space");
    assert!(heard.redraw, "the pressed look went");
}

/// The ring colour the ring tests paint with, unlike anything else.
const RING: loom_ui_core::Color = loom_ui_core::Color::rgb(0.0, 1.0, 0.0);

/// A panel style whose ring is [`RING`], 2 wide and 2 outside.
fn ring_style() -> crate::paint::PanelStyle {
    crate::paint::PanelStyle {
        text: loom_ui_core::Color::WHITE,
        caret: loom_ui_core::Color::WHITE,
        selection: loom_ui_core::Color::BLACK,
        quiet: loom_ui_core::Color::WHITE,
        mark: loom_ui_core::Color::WHITE,
        primary: loom_ui_core::Color::rgb(0.3, 0.6, 1.0),
        track: loom_ui_core::Color::rgb(0.4, 0.4, 0.4),
        check: crate::paint::CheckLook {
            edge: loom_ui_core::Color::rgb(0.6, 0.6, 0.6),
            fill: loom_ui_core::Color::rgb(0.3, 0.5, 0.9),
            mark: loom_ui_core::Color::rgb(0.1, 0.1, 0.1),
        },
        ring: crate::focus::RingStyle {
            color: RING,
            width: 2.0,
            offset: 2.0,
        },
    }
}

/// The rings painted in `layer`: each quad outlined in [`RING`].
fn rings(harness: &mut Harness<Layers, Fake>, layer: Layer) -> Vec<crate::paint::Quad> {
    harness.lay_out(&mut Mono::default());
    let mut recorder = crate::paint::testing::Recorder::default();
    let (_, _, mut panels) = harness.parts();
    panels.paint(layer, &mut recorder, &ring_style());
    recorder
        .painted
        .iter()
        .filter_map(|painted| match painted {
            crate::paint::testing::Painted::Quad(quad) if quad.border == RING => Some(*quad),
            _ => None,
        })
        .collect()
}

/// Tab, as a key press.
fn tab_key() -> DriverEvent {
    DriverEvent::Input {
        event: Event::Key(loom_ui_core::KeyEvent::Pressed {
            key: loom_ui_core::Key::Tab,
            modifiers: loom_ui_core::Modifiers::NONE,
            repeat: false,
            text: None,
            code: Code::Other,
        }),
        pointer: None,
    }
}

/// Section 6, rule 3: focus from the keyboard paints one ring around the
/// focused element, its offset and its width outside the bounds, in the
/// element's layer and in no other. Rows 1 and 2 kept the ring's state and
/// painted nothing, so keyboard focus could not be seen.
#[test]
fn keyboard_focus_paints_a_ring_in_its_layer() {
    let mut harness = harness(false, Routed::Passed);
    assert!(rings(&mut harness, Layer::Panels).is_empty(), "no focus, no ring");
    harness.step(tab_key());
    let a = UiId::panel("bar").with("a");
    assert_eq!(harness.runtime().focused(), Some(&a));
    let bounds = harness.runtime().bounds(&a).expect("laid out");
    let painted = rings(&mut harness, Layer::Panels);
    assert_eq!(painted.len(), 1, "one ring");
    let ring = painted[0];
    assert_eq!(ring.rect.x, bounds.x - 4.0);
    assert_eq!(ring.rect.y, bounds.y - 4.0);
    assert_eq!(ring.rect.width, bounds.width + 8.0);
    assert_eq!(ring.rect.height, bounds.height + 8.0);
    assert_eq!(ring.border_width, 2.0);
    assert_eq!(ring.fill, loom_ui_core::Color::TRANSPARENT);
    assert!(rings(&mut harness, Layer::Overlays).is_empty(), "only in its own layer");
}

/// Section 6, rule 3: an element below the input layer keeps its focus
/// but shows no ring, and `focus_ring` agrees with what is painted; the
/// ring comes back when the layer drops.
#[test]
fn no_ring_shows_below_the_input_layer() {
    let mut harness = harness(false, Routed::Passed);
    harness.step(tab_key());
    harness.send(Message::Input(Layer::Overlays));
    assert_eq!(harness.runtime().focused(), Some(&UiId::panel("bar").with("a")), "kept");
    assert_eq!(harness.runtime().focus_ring(), None);
    assert!(rings(&mut harness, Layer::Panels).is_empty(), "under a raised layer");
    harness.send(Message::Input(Layer::Canvas));
    assert!(harness.runtime().focus_ring().is_some());
    assert_eq!(rings(&mut harness, Layer::Panels).len(), 1, "back when it drops");
}

/// The ring of an element in an overlay is painted with the overlay's
/// layer, above Iced, not with the panels below it.
#[test]
fn an_overlays_focus_rings_in_the_overlays_layer() {
    let mut menu = harness(true, Routed::Passed);
    // Tab from nothing enters while the panels take input; then the
    // overlay takes it, and its focused item keeps its ring.
    menu.step(tab_key());
    assert_eq!(menu.runtime().focused(), Some(&UiId::panel("menu").with("item")));
    menu.send(Message::Input(Layer::Overlays));
    assert!(rings(&mut menu, Layer::Panels).is_empty());
    assert_eq!(rings(&mut menu, Layer::Overlays).len(), 1);
}

/// An update that raises the input layer over the focused element hides
/// its ring with no panel built again, so it asks for a frame: otherwise
/// the last frame would keep showing the ring (CodeRabbit, #861). Dropping
/// the layer shows it again, with a frame too.
#[test]
fn a_ring_shown_or_hidden_asks_for_a_frame() {
    let mut harness = harness(false, Routed::Passed);
    harness.step(tab_key());
    assert!(harness.runtime().focus_ring().is_some());
    let before = harness.driver().redraws();
    harness.send(Message::Input(Layer::Overlays));
    assert_eq!(harness.runtime().focus_ring(), None);
    assert!(harness.driver().redraws() > before, "hiding the ring draws a frame");
    let hidden = harness.driver().redraws();
    harness.send(Message::Input(Layer::Canvas));
    assert!(harness.driver().redraws() > hidden, "showing it again draws one");
}

/// Section 3, rule 4 (plan 3.2, row 3a): a focused menu item is activated
/// as a button is, Enter on key down and Space on key up, each once.
#[test]
fn enter_and_space_click_a_focused_menu_item() {
    use loom_ui_core::Key;
    let item = UiId::panel("menu").with("item");
    let mut menu = harness(true, Routed::Passed);
    menu.step(tab_key());
    assert_eq!(menu.runtime().focused(), Some(&item));
    menu.step(press_key(Key::Enter, false));
    assert_eq!(clicks(&menu), std::slice::from_ref(&item), "Enter on key down");
    menu.step(press_key(Key::Space, false));
    menu.step(release_key(Key::Space));
    menu.step(DriverEvent::Woken);
    assert_eq!(clicks(&menu), [item.clone(), item], "Space on key up");
}

/// Section 5, rule 14: with the menu's layer taking input, a press of any
/// button outside the menu is taken, so neither the target nor the bar
/// under it hears it, and the app hears it on the next turn; a press on the
/// menu is the menu's own. With the panels taking input, the menu does not
/// close on a press outside it.
#[test]
fn a_press_outside_a_closing_menu_is_taken_and_told() {
    let menu = UiId::panel("menu");
    let outside = |of: &Harness<Layers, Fake>| {
        of.app()
            .heard
            .iter()
            .filter(|heard| matches!(heard, Message::Outside(_)))
            .count()
    };
    let mut menu_open = harness(true, Routed::Passed);
    menu_open.send(Message::Input(Layer::Overlays));
    for button in [PointerButton::Left, PointerButton::Right] {
        let heard = menu_open.runtime().target().heard.len();
        menu_open.step(at(200.0, 100.0, PointerEvent::Pressed(button)));
        assert_eq!(
            menu_open.runtime().target().heard.len(),
            heard,
            "{button:?}: the target heard nothing"
        );
    }
    assert_eq!(outside(&menu_open), 2, "both presses were told");
    assert!(menu_open.app().heard.contains(&Message::Outside(menu.clone())));
    {
        let (_, _, panels) = menu_open.parts();
        let press = Event::Pointer(PointerEvent::Pressed(PointerButton::Left));
        assert!(
            panels.over_target(&press, Point::new(200.0, 100.0)),
            "a driver keeps it from the toolkit"
        );
    }
    menu_open.step(at(5.0, 5.0, PointerEvent::Pressed(PointerButton::Left)));
    assert_eq!(outside(&menu_open), 2, "a press on the menu is its own");

    let mut below = harness(true, Routed::Passed);
    below.send(Message::Input(Layer::Canvas));
    below.step(at(200.0, 100.0, PointerEvent::Pressed(PointerButton::Left)));
    assert_eq!(outside(&below), 0, "only while its layer takes input");
}

/// Section 6, rule 6, step 2 (plan 3.2, row 3a): while the Question layer
/// takes input, a key press nothing of the runtime's takes goes to
/// `App::question_key` and never to the target, so a toolkit's field or
/// shortcut under the question hears nothing; its release still goes on.
#[test]
fn a_question_keeps_every_key_from_the_target() {
    use loom_ui_core::Key;
    let mut harness = harness(false, Routed::Passed);
    harness.send(Message::Input(Layer::Question));
    for key in [Key::Backspace, Key::Delete, Key::Character('d'), Key::Enter] {
        harness.step(press_key(key, false));
        assert!(!target_heard_press(&harness, key), "{key:?} went no further");
    }
    let asked = harness
        .app()
        .heard
        .iter()
        .filter(|heard| matches!(heard, Message::QuestionKey))
        .count();
    assert_eq!(asked, 4, "each press was the app's question key");
    let heard = harness.runtime().target().heard.len();
    harness.step(release_key(Key::Enter));
    assert_eq!(harness.runtime().target().heard.len(), heard + 1, "the release goes on");
}

/// Section 6, rule 4 (plan 3.2, row 3a): an overlay saves the focus it
/// opens over and gives it back, with its ring, when it closes, one level
/// at a time: a question over the menu gives the menu its item back, then
/// the menu gives the bar its button back. With no focus before, there is
/// none to give back.
#[test]
fn an_overlay_gives_back_the_focus_it_opened_over() {
    let (a, item) = (UiId::panel("bar").with("a"), UiId::panel("menu").with("item"));
    let mut menu = harness(true, Routed::Passed);
    menu.step(tab_key());
    menu.step(tab_key());
    assert_eq!(
        menu.runtime().focused(),
        Some(&a),
        "the bar's button, from the keyboard"
    );
    menu.send(Message::Input(Layer::Overlays));
    menu.send(Message::Refocus(vec![item.clone()]));
    assert_eq!(menu.runtime().focused(), Some(&item), "the menu took it");
    menu.send(Message::Input(Layer::Question));
    menu.send(Message::Refocus(Vec::new()));
    menu.send(Message::Input(Layer::Overlays));
    assert_eq!(
        menu.runtime().focused(),
        Some(&item),
        "the question gave the menu its item"
    );
    menu.send(Message::Input(Layer::Canvas));
    assert_eq!(menu.runtime().focused(), Some(&a), "the menu gave the bar its button");
    assert!(menu.runtime().focus_ring().is_some(), "with its ring");

    let mut nothing = harness(true, Routed::Passed);
    nothing.send(Message::Input(Layer::Overlays));
    nothing.send(Message::Input(Layer::Canvas));
    assert_eq!(nothing.runtime().focused(), None, "nothing to give back");
}

/// Section 6, rule 8 (plan 3.2, row 3b): the pointer moving over a menu
/// item focuses it with no ring, so the keys go on from there; moving over
/// a button focuses nothing.
#[test]
fn hovering_a_menu_item_focuses_it_without_a_ring() {
    let item = UiId::panel("menu").with("item");
    let mut menu = harness(true, Routed::Passed);
    menu.send(Message::Input(Layer::Overlays));
    menu.step(at(5.0, 5.0, PointerEvent::Moved));
    assert_eq!(menu.runtime().focused(), Some(&item));
    assert_eq!(menu.runtime().focus_ring(), None, "no ring from the pointer");

    let mut bar = harness(false, Routed::Passed);
    bar.send(Message::Input(Layer::Canvas));
    bar.step(at(5.0, 5.0, PointerEvent::Moved));
    assert_eq!(bar.runtime().focused(), None, "a button takes no focus from hover");
}

/// An overlay that closes in the same update as a focus effect onto
/// another element below it leaves the focus there: the focus it saved
/// goes back only when the focus is still on the closing overlay (section
/// 6, rule 4). The toolbar is in the Panels layer, above the canvas's
/// input layer, and still counts as below the overlay.
#[test]
fn closing_an_overlay_keeps_a_focus_its_update_moved() {
    let (a, b, item) = (
        UiId::panel("bar").with("a"),
        UiId::panel("bar").with("b"),
        UiId::panel("menu").with("item"),
    );
    let mut menu = harness(true, Routed::Passed);
    menu.step(tab_key());
    menu.step(tab_key());
    assert_eq!(menu.runtime().focused(), Some(&a));
    menu.send(Message::Input(Layer::Overlays));
    menu.send(Message::Refocus(vec![item]));
    menu.send(Message::CloseAndFocus(b.clone()));
    assert_eq!(menu.runtime().focused(), Some(&b), "the effect's focus is kept");
}

/// A turned-off menu item that stays a stop (section 6, rule 2, as the
/// APG keeps a disabled menu item focusable): the pointer moving over it
/// focuses it, as over any menu item, and a click on it picks nothing.
#[test]
fn hovering_a_turned_off_menu_item_focuses_it_and_a_click_picks_nothing() {
    let item = UiId::panel("menu").with("item");
    let mut menu = harness(true, Routed::Passed);
    menu.send(Message::OffItem);
    menu.send(Message::Input(Layer::Overlays));
    menu.lay_out(&mut Mono::default());
    menu.step(at(5.0, 5.0, PointerEvent::Moved));
    assert_eq!(menu.runtime().focused(), Some(&item), "it takes the focus");
    assert_eq!(menu.runtime().focus_ring(), None, "no ring from the pointer");
    click(&mut menu, 5.0, 5.0);
    assert!(
        !menu.app().heard.contains(&Message::Clicked(item)),
        "a click on it picks nothing"
    );
}

/// An app that shows one paragraph in the panels layer, "See Saw or
/// Sine", with links on "Saw" and "Sine".
struct Paragraph {
    heard: Vec<Message>,
}

impl App for Paragraph {
    type Message = Message;

    fn update(&mut self, message: Message, _cx: &Context<'_, Message>) -> Effects<Message> {
        self.heard.push(message);
        Effects::none()
    }

    fn view(&self, view: &mut View) {
        use crate::element::Inline;
        view.panel_with(UiId::panel("help"), Layer::Panels, &(), |_| {
            Element::rich(
                [
                    Inline::new("See "),
                    link("Saw", "saw"),
                    Inline::new(" or "),
                    link("Sine", "sine"),
                ],
                STYLE,
            )
            .key("notes")
        });
    }

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }

    fn input(&self) -> InputState {
        InputState {
            layer: Layer::Canvas,
            canvas_focused: false,
            target_first: false,
        }
    }

    fn clicked(&self, id: &UiId) -> Option<Message> {
        Some(Message::Clicked(id.clone()))
    }
}

/// An underlined link to `key`, named by `words`.
fn link(words: &str, key: &str) -> crate::element::Inline {
    crate::element::Inline::new(words)
        .underline()
        .link(key, Access::new(Role::Link, words).action(Action::Click))
}

/// The paragraph app, laid out in a 400 by 200 window.
fn paragraph() -> Harness<Paragraph, Fake> {
    let target = Fake {
        answer: Routed::Passed,
        taken_from: 350.0,
        heard: Vec::new(),
        focus: None,
    };
    let window = WindowState::new(Size::new(400.0, 200.0));
    let mut harness = Harness::new(Paragraph { heard: Vec::new() }, target, window, Instant::now());
    harness.lay_out(&mut Mono::default());
    harness
}

/// The clicks the paragraph app heard.
fn link_clicks(harness: &Harness<Paragraph, Fake>) -> Vec<UiId> {
    harness
        .app()
        .heard
        .iter()
        .filter_map(|message| match message {
            Message::Clicked(id) => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// Each link is a focus stop, in the order of the text, and Enter follows
/// the focused one, as WAI-ARIA's link pattern says. Space does not: it is
/// not the link's, so it goes on to the target, as a page's Space scrolls.
#[test]
fn enter_follows_a_focused_link_and_space_does_not() {
    use loom_ui_core::Key;
    let notes = UiId::panel("help").with("notes");
    let mut harness = paragraph();
    harness.step(press_key(Key::Tab, false));
    assert_eq!(harness.runtime().focused(), Some(&notes.clone().with("saw")));
    harness.step(press_key(Key::Tab, false));
    assert_eq!(harness.runtime().focused(), Some(&notes.clone().with("sine")));
    harness.step(press_key(Key::Space, false));
    harness.step(release_key(Key::Space));
    assert!(link_clicks(&harness).is_empty(), "Space follows no link");
    assert!(
        harness.runtime().target().heard.iter().any(|event| matches!(
            event,
            Event::Key(loom_ui_core::KeyEvent::Pressed { key: Key::Space, .. })
        )),
        "Space went on"
    );
    harness.step(press_key(Key::Enter, false));
    assert_eq!(link_clicks(&harness), [notes.with("sine")]);
}

/// A press on a link follows it on release and takes no focus, as a
/// button's does: the keys stay where they were.
#[test]
fn a_press_on_a_link_follows_it_without_focus() {
    let mut harness = paragraph();
    // "See " is 32 pixels at the fake's 8 a character, so "Saw" is 32 to 56.
    harness.step(at(40.0, 8.0, PointerEvent::Moved));
    click_paragraph(&mut harness, 40.0, 8.0);
    assert_eq!(link_clicks(&harness), [UiId::panel("help").with("notes").with("saw")]);
    assert_eq!(harness.runtime().focused(), None, "no focus from the press");
}

/// A left click on the paragraph app at `(x, y)`.
fn click_paragraph(harness: &mut Harness<Paragraph, Fake>, x: f32, y: f32) {
    harness.step(at(x, y, PointerEvent::Pressed(PointerButton::Left)));
    harness.step(at(x, y, PointerEvent::Released(PointerButton::Left)));
}

/// Keyboard focus on a link rings its words, not the whole paragraph.
#[test]
fn a_focused_link_rings_its_words() {
    let mut harness = paragraph();
    harness.step(tab_key());
    harness.lay_out(&mut Mono::default());
    let mut recorder = crate::paint::testing::Recorder::default();
    let (_, _, mut panels) = harness.parts();
    panels.paint(Layer::Panels, &mut recorder, &ring_style());
    let painted: Vec<crate::paint::Quad> = recorder
        .painted
        .iter()
        .filter_map(|painted| match painted {
            crate::paint::testing::Painted::Quad(quad) if quad.border == RING => Some(*quad),
            _ => None,
        })
        .collect();
    assert_eq!(painted.len(), 1);
    // "Saw" is 32 to 56 and 16 high; the ring is 4 outside.
    assert_eq!(
        painted[0].rect,
        loom_ui_core::Rect::new(Point::new(28.0, -4.0), Size::new(32.0, 24.0))
    );
}

/// The paragraph paints its text once, with its spans, and a link paints
/// nothing of its own.
#[test]
fn a_paragraph_paints_its_links_once() {
    let mut harness = paragraph();
    let mut recorder = crate::paint::testing::Recorder::default();
    let (_, _, mut panels) = harness.parts();
    panels.paint(Layer::Panels, &mut recorder, &ring_style());
    assert_eq!(recorder.texts(), ["See Saw or Sine"]);
    let spans = recorder
        .painted
        .iter()
        .find_map(|painted| match painted {
            crate::paint::testing::Painted::Spans { spans, .. } => Some(spans.clone()),
            _ => None,
        })
        .expect("painted with its spans");
    assert_eq!(spans.len(), 4);
    assert!(spans[1].underline && spans[3].underline && !spans[0].underline);
}

/// The bar with its second button reporting hover and holding a context
/// menu, and nothing over it.
fn reporting() -> Harness<Layers, Fake> {
    let app = Layers {
        overlay: false,
        input: Layer::Panels,
        canvas: false,
        runs: false,
        reports: true,
        stacked: false,
        padded: false,
        off_item: false,
        heard: Vec::new(),
    };
    let target = Fake {
        answer: Routed::Passed,
        taken_from: 350.0,
        heard: Vec::new(),
        focus: None,
    };
    let window = WindowState::new(Size::new(400.0, 200.0));
    let mut harness = Harness::new(app, target, window, Instant::now());
    harness.lay_out(&mut Mono::default());
    harness
}

/// What the app heard of hovers and context menus, in order.
fn reports(harness: &Harness<Layers, Fake>) -> Vec<Message> {
    harness
        .app()
        .heard
        .iter()
        .filter(|message| matches!(message, Message::Hovered(_) | Message::Unhovered(_) | Message::Menu(..)))
        .cloned()
        .collect()
}

/// Section 3, rule 4: an element that reports its hover tells the app once
/// each time the pointer comes onto it, not for each move over it, and an
/// element that does not ask tells it nothing.
#[test]
fn a_reporting_element_tells_the_app_its_hover_once_per_entry() {
    let mut harness = reporting();
    let b = UiId::panel("bar").with("b");
    harness.step(at(60.0, 8.0, PointerEvent::Moved));
    harness.step(at(62.0, 8.0, PointerEvent::Moved));
    assert_eq!(reports(&harness), [Message::Hovered(b.clone())]);
    harness.step(at(20.0, 8.0, PointerEvent::Moved));
    assert_eq!(
        reports(&harness),
        [Message::Hovered(b.clone()), Message::Unhovered(b.clone())],
        "the first button does not ask to be heard, and the second's hover ended"
    );
    harness.step(at(60.0, 8.0, PointerEvent::Moved));
    assert_eq!(
        reports(&harness),
        [
            Message::Hovered(b.clone()),
            Message::Unhovered(b.clone()),
            Message::Hovered(b)
        ]
    );
}

/// Section 5, rule 11: a hover the app heard of ends once, whichever way
/// it ends: the pointer moving off every element, leaving the window, or
/// the element leaving the layout.
#[test]
fn a_reported_hover_ends_once_each_way() {
    let b = UiId::panel("bar").with("b");
    let heard = |harness: &Harness<Layers, Fake>| reports(harness);
    let start_and_end = [Message::Hovered(b.clone()), Message::Unhovered(b.clone())];

    // Off every element, into the room below the bar.
    let mut off = reporting();
    off.step(at(60.0, 8.0, PointerEvent::Moved));
    off.step(at(60.0, 150.0, PointerEvent::Moved));
    off.step(at(70.0, 150.0, PointerEvent::Moved));
    assert_eq!(heard(&off), start_and_end, "off every element");

    // Out of the window.
    let mut out = reporting();
    out.step(at(60.0, 8.0, PointerEvent::Moved));
    out.step(DriverEvent::Input {
        event: Event::Pointer(PointerEvent::LeftWindow),
        pointer: None,
    });
    out.step(DriverEvent::Woken);
    assert_eq!(heard(&out), start_and_end, "out of the window");

    // The element no longer reports its hover, as one that left the
    // layout no longer can.
    let mut gone = reporting();
    gone.step(at(60.0, 8.0, PointerEvent::Moved));
    gone.send(Message::Reports(false));
    gone.lay_out(&mut Mono::default());
    gone.step(DriverEvent::Woken);
    gone.step(DriverEvent::Woken);
    assert_eq!(heard(&gone), start_and_end, "out of the layout");
}

/// Outside a turn, a hover that ends, because the other toolkit took the
/// pointer or the pointer moved off every element, is heard ending before
/// the pointer comes back onto the same element: the app hears the hover
/// start, end, and start again, never start twice.
#[test]
fn a_hover_cleared_outside_a_turn_ends_before_it_starts_again() {
    let b = UiId::panel("bar").with("b");
    let back_and_forth = [
        Message::Hovered(b.clone()),
        Message::Unhovered(b.clone()),
        Message::Hovered(b.clone()),
    ];
    let on_b = Point::new(60.0, 8.0);
    let moved = Event::Pointer(PointerEvent::Moved);

    // The other toolkit takes a move, then the pointer comes back.
    let mut taken = reporting();
    taken.step(at(on_b.x, on_b.y, PointerEvent::Moved));
    {
        let (_, _, mut panels) = taken.parts();
        let _ = panels.pointer_taken(&moved);
        let _ = panels.pointer_below(&moved, on_b);
    }
    taken.step(DriverEvent::Woken);
    assert_eq!(reports(&taken), back_and_forth, "taken by the other toolkit");

    // A late move off every element, then back.
    let mut off = reporting();
    off.step(at(on_b.x, on_b.y, PointerEvent::Moved));
    {
        let (_, _, mut panels) = off.parts();
        let _ = panels.pointer_below(&moved, Point::new(60.0, 150.0));
        let _ = panels.pointer_below(&moved, on_b);
    }
    off.step(DriverEvent::Woken);
    assert_eq!(reports(&off), back_and_forth, "off every element");
}

/// Only the pointer's own move reports: a layout that leaves the hover on
/// the same element tells the app nothing.
#[test]
fn a_layout_under_a_still_pointer_reports_no_hover() {
    let mut harness = reporting();
    harness.step(at(60.0, 8.0, PointerEvent::Moved));
    harness.lay_out(&mut Mono::default());
    harness.step(DriverEvent::RedrawRequested);
    assert_eq!(reports(&harness).len(), 1);
}

/// Section 5, rule 19: a right press on an element with a context menu
/// opens it, as not opened by a key; on one without, it opens nothing, and
/// a left press never does.
#[test]
fn a_right_press_opens_the_context_menu() {
    let mut harness = reporting();
    let b = UiId::panel("bar").with("b");
    harness.step(at(20.0, 8.0, PointerEvent::Pressed(PointerButton::Right)));
    harness.step(at(60.0, 8.0, PointerEvent::Pressed(PointerButton::Left)));
    harness.step(at(60.0, 8.0, PointerEvent::Released(PointerButton::Left)));
    assert!(
        !reports(&harness)
            .iter()
            .any(|message| matches!(message, Message::Menu(..)))
    );
    harness.step(at(60.0, 8.0, PointerEvent::Pressed(PointerButton::Right)));
    assert_eq!(reports(&harness).last(), Some(&Message::Menu(b, false)));
}

/// Section 5, rule 19: a screen reader's ShowContextMenu on the node of an
/// element with a context menu opens it as by a key; on any other node it
/// goes on to the app's own handling, which here hears nothing.
#[test]
fn show_context_menu_opens_it_as_a_key_does() {
    let mut harness = reporting();
    let show = |node: u64| {
        DriverEvent::Accessibility(crate::AccessEvent::Action(accesskit::ActionRequest {
            action: accesskit::Action::ShowContextMenu,
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(node),
            data: None,
        }))
    };
    harness.step(show(3));
    assert!(reports(&harness).is_empty(), "the first button has no context menu");
    harness.step(show(2));
    assert_eq!(reports(&harness), [Message::Menu(UiId::panel("bar").with("b"), true)]);
}

/// Section 7, rule 24: under a layer that takes the input, an element with
/// a context menu stays inert to a screen reader, as to the pointer.
#[test]
fn show_context_menu_under_a_raised_layer_opens_nothing() {
    let mut harness = reporting();
    harness.send(Message::Input(Layer::Overlays));
    harness.step(DriverEvent::Accessibility(crate::AccessEvent::Action(
        accesskit::ActionRequest {
            action: accesskit::Action::ShowContextMenu,
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(2),
            data: None,
        },
    )));
    assert!(reports(&harness).is_empty());
}

/// Section 5, rule 14: with two closing roots open, one over the other, a
/// press inside the lower one closes only the upper one, a press inside the
/// upper one closes neither, and a press outside both closes both.
#[test]
fn a_press_inside_the_lower_of_two_menus_closes_only_the_upper() {
    let app = Layers {
        overlay: true,
        input: Layer::Overlays,
        canvas: false,
        runs: false,
        reports: false,
        stacked: true,
        padded: false,
        off_item: false,
        heard: Vec::new(),
    };
    let target = Fake {
        answer: Routed::Passed,
        taken_from: 350.0,
        heard: Vec::new(),
        focus: None,
    };
    let mut harness = Harness::new(app, target, WindowState::new(Size::new(400.0, 200.0)), Instant::now());
    harness.lay_out(&mut Mono::default());
    let told = |harness: &Harness<Layers, Fake>| -> Vec<UiId> {
        harness
            .app()
            .heard
            .iter()
            .filter_map(|heard| match heard {
                Message::Outside(root) => Some(root.clone()),
                _ => None,
            })
            .collect()
    };
    harness.step(at(5.0, 5.0, PointerEvent::Pressed(PointerButton::Left)));
    assert_eq!(told(&harness), [UiId::panel("sub")], "inside the lower menu");
    harness.step(at(105.0, 55.0, PointerEvent::Pressed(PointerButton::Left)));
    assert_eq!(told(&harness).len(), 1, "inside the upper menu: neither");
    harness.step(at(300.0, 150.0, PointerEvent::Pressed(PointerButton::Left)));
    assert_eq!(
        told(&harness)[1..],
        [UiId::panel("menu"), UiId::panel("sub")],
        "outside both: both, lower first"
    );
}

/// While an overlay owns the input, a press on no stop keeps the focus
/// where it was, so the user keeps their place (section 6, rule 3).
#[test]
fn a_press_on_no_stop_over_an_overlay_keeps_the_focus() {
    let app = Layers {
        overlay: true,
        input: Layer::Canvas,
        canvas: false,
        runs: false,
        reports: false,
        stacked: false,
        padded: true,
        off_item: false,
        heard: Vec::new(),
    };
    let target = Fake {
        answer: Routed::Passed,
        taken_from: 350.0,
        heard: Vec::new(),
        focus: None,
    };
    let mut harness = Harness::new(app, target, WindowState::new(Size::new(400.0, 200.0)), Instant::now());
    harness.lay_out(&mut Mono::default());
    harness.send(Message::Input(Layer::Overlays));
    let item = UiId::panel("dialog").with("ok");
    harness.send(Message::Refocus(vec![item.clone()]));
    assert_eq!(harness.runtime().focused(), Some(&item));
    // Inside the dialog's room, above its button: the overlay's own, no stop.
    click(&mut harness, 10.0, 10.0);
    assert_eq!(harness.runtime().focused(), Some(&item), "kept over the overlay");
}
