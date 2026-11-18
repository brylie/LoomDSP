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
use loom_ui_core::{Event, Point, PointerButton, PointerEvent, Size, UiId};
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
}

/// An app that shows a bar of two buttons in the panels layer, across the
/// top, and, when `overlay` is set, a button in the overlays layer over
/// the first.
struct Layers {
    overlay: bool,
    input: Layer,
    heard: Vec<Message>,
}

impl App for Layers {
    type Message = Message;

    fn update(&mut self, message: Message, _cx: &Context<'_, Message>) -> Effects<Message> {
        let mut effects = Effects::none();
        match &message {
            Message::Input(layer) => self.input = *layer,
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
        view.panel_with(UiId::panel("bar"), Layer::Panels, &(), |_| {
            Element::row([button("a", 40.0), button("b", 40.0)]).width(Length::FILL)
        });
        if self.overlay {
            view.panel_with(UiId::panel("menu"), Layer::Overlays, &(), |_| {
                Element::row([button("item", 20.0)])
            });
        }
    }

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }

    fn input(&self) -> InputState {
        InputState {
            layer: self.input,
            canvas_focused: false,
            target_first: false,
        }
    }

    fn clicked(&self, id: &UiId) -> Option<Message> {
        Some(Message::Clicked(id.clone()))
    }
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
    assert_eq!(harness.runtime().focused(), Some(&UiId::panel("menu").with("item")));
    assert_eq!(harness.runtime().focus_ring(), None, "a press shows no ring");
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
    assert_eq!(passed.runtime().focused(), Some(&UiId::panel("bar").with("a")));

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
    assert_eq!(harness.runtime().focused(), Some(&UiId::panel("bar").with("b")));
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
        }),
        pointer: None,
    }
}

/// `key` coming up.
fn release_key(key: loom_ui_core::Key) -> DriverEvent {
    DriverEvent::Input {
        event: Event::Key(loom_ui_core::KeyEvent::Released { key }),
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
