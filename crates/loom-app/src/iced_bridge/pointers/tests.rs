//! Routing between Iced and the runtime's own layers, in the app
//! (ui-layer.md, section 5, rule 10; section 6, rules 1 and 2). The app
//! shows no `loom-ui` panel yet, so these tests add two through `Hosted`'s
//! test hook: a bar of one button at the bottom left, over the stats bar,
//! below Iced; and a pop-up over the toolbar's left end, above Iced.

use crate::Message;
use crate::iced_bridge::TestPanels;
use crate::ui_test::Ui;
use iced::keyboard;
use loom_ui::access::{Access, Action, Role};
use loom_ui::{Element, Layer, Length, View};
use loom_ui_core::{Point, UiId};

/// The pop-up's size, from the window's top-left corner. Wide enough to
/// cover Open whatever the system font's widths (its centre is near 200 on
/// macOS and 226 on Linux), and far short of Undo, near 1150.
const POP: (f32, f32) = (400.0, 48.0);

/// The bar's button: its key, and its size, at the window's bottom left.
const GO: (f32, f32) = (60.0, 16.0);

/// A button of the runtime's: keyed, a focus stop, and a node.
fn button(key: &str, width: f32, height: f32) -> Element {
    Element::space(Length::Fixed(width), Length::Fixed(height))
        .min_width(width)
        .key(key)
        .focusable()
        .access(Access::new(Role::Button, key).action(Action::Click))
}

/// The bar alone, in the panels layer, below Iced.
fn bar(view: &mut View) {
    view.panel_with(UiId::panel("test-bar"), Layer::Panels, &(), |_| {
        Element::column([
            Element::space(Length::Shrink, Length::FILL),
            Element::row([button("go", GO.0, GO.1)]),
        ])
        .width(Length::FILL)
        .height(Length::FILL)
    });
}

/// The bar, and the pop-up in the overlays layer, above Iced.
fn bar_and_pop(view: &mut View) {
    bar(view);
    view.panel_with(UiId::panel("test-pop"), Layer::Overlays, &(), |_| {
        Element::row([button("pop", POP.0, POP.1)])
    });
}

/// The bar's button.
fn go() -> UiId {
    UiId::panel("test-bar").with("go")
}

/// A click on the bar opens help, and one on the pop-up the shortcuts.
fn clicked(id: &UiId) -> Option<Message> {
    if *id == go() {
        Some(Message::ShowHelp)
    } else {
        Some(Message::ShowShortcuts)
    }
}

/// `MidiGhosts`, harmless with no MIDI, moves the focus to the bar's
/// button, as one of the app's own panels would ask with a focus effect.
fn focuses(message: &Message) -> Option<UiId> {
    matches!(message, Message::MidiGhosts).then(go)
}

/// The subtractive patch with `view`'s panels added.
fn ui_with(view: fn(&mut View)) -> Ui {
    let mut ui = Ui::open("subtractive.ron");
    ui.with_test_panels(TestPanels { view, clicked, focuses });
    ui
}

/// The middle of the bar's button.
fn on_go() -> Point {
    Point::new(GO.0 / 2.0, crate::WINDOW.height - GO.1 / 2.0)
}

/// A left click at `at`: move there, press, release.
fn click(ui: &mut Ui, at: Point) -> Vec<Message> {
    let mut heard = ui.move_to(at);
    heard.extend(ui.press());
    heard.extend(ui.release());
    heard
}

/// The plan's test: with a question open, a click on the toolbar and one
/// on the bar each reach only the question, and the bar is neither
/// hovered nor focused. With the question gone, the bar's click is the
/// bar's: its message runs, and its button has the focus, with no ring.
#[test]
fn a_question_holds_presses_on_both_toolkits() {
    let mut ui = ui_with(bar);
    ui.app_mut().overlays.question = Some(crate::question::Question::error("No".to_string()));
    ui.redraw();
    let open = ui.text("Open").expect("the toolbar shows Open").center();
    let heard = click(&mut ui, open);
    assert!(!heard.iter().any(|m| matches!(m, Message::File(_))), "{heard:?}");
    let heard = click(&mut ui, on_go());
    assert!(!heard.iter().any(|m| matches!(m, Message::ShowHelp)), "{heard:?}");
    let runtime = ui.runtime();
    assert_eq!(runtime.hovered(), None);
    assert_eq!(runtime.focused(), None);

    ui.app_mut().overlays.question = None;
    ui.redraw();
    let heard = click(&mut ui, on_go());
    assert!(heard.iter().any(|m| matches!(m, Message::ShowHelp)), "{heard:?}");
    assert_eq!(ui.runtime().focused(), Some(&go()));
    assert_eq!(ui.runtime().focus_ring(), None, "a press shows no ring");
}

/// A press Iced captures never reaches the runtime's layers, so the focus
/// they had stays (section 6, rule 1 is the target's to keep there).
#[test]
fn a_press_iced_captures_leaves_our_focus() {
    let mut ui = ui_with(bar);
    click(&mut ui, on_go());
    assert_eq!(ui.runtime().focused(), Some(&go()));
    let undo = ui.text("Undo").expect("the toolbar shows Undo").center();
    click(&mut ui, undo);
    assert_eq!(ui.runtime().focused(), Some(&go()), "Iced took that press");
}

/// A layer above Iced takes its point before Iced hears it: a click on the
/// pop-up over the toolbar is the pop-up's, and the toolbar under it hears
/// nothing. A move onto it hovers it; a move off, onto the bar below Iced,
/// moves the hover there; and a move onto the toolbar clears it.
#[test]
fn a_layer_above_iced_keeps_its_points_from_iced() {
    let mut ui = ui_with(bar_and_pop);
    let open = ui.text("Open").expect("the toolbar shows Open").center();
    assert!(open.x < POP.0 && open.y < POP.1, "the pop-up covers Open at {open:?}");
    // The other half of what the pop-up's size assumes: Undo, where the
    // hover goes back to Iced below, starts past its right edge.
    let undo_box = ui.text("Undo").expect("the toolbar shows Undo");
    assert!(undo_box.x >= POP.0, "the pop-up leaves Undo out, at {undo_box:?}");
    let heard = click(&mut ui, open);
    assert!(heard.iter().any(|m| matches!(m, Message::ShowShortcuts)), "{heard:?}");
    assert!(!heard.iter().any(|m| matches!(m, Message::File(_))), "{heard:?}");
    let _ = ui.deliver(Message::CloseHelp);

    ui.move_to(Point::new(10.0, 10.0));
    assert_eq!(ui.runtime().hovered(), Some(&UiId::panel("test-pop").with("pop")));
    ui.move_to(on_go());
    assert_eq!(ui.runtime().hovered(), Some(&go()));
    let undo = ui.text("Undo").expect("the toolbar shows Undo").center();
    ui.move_to(undo);
    assert_eq!(ui.runtime().hovered(), None, "Iced holds the hover now");
}

/// Section 6, rule 2, between the toolkits: with nothing focused in
/// either, Tab enters the runtime's first stop, and Iced never hears it,
/// so no field of Iced's takes it too.
#[test]
fn tab_from_nothing_enters_our_first_stop_and_not_iced() {
    let mut ui = ui_with(bar);
    assert_eq!(ui.focused_field(), None);
    ui.tap(
        keyboard::Key::Named(keyboard::key::Named::Tab),
        keyboard::Modifiers::default(),
    );
    assert_eq!(ui.runtime().focused(), Some(&go()));
    assert!(ui.runtime().focus_ring().is_some(), "from the keyboard, with a ring");
    assert_eq!(ui.focused_field(), None, "no Iced field took the Tab");
}

/// The bug this pull request fixes on main: when the runtime's element
/// takes the focus from Iced's field, the field lets go before Iced hears
/// the next key, so the key never lands in the field (section 6, rule 1).
#[test]
fn a_key_after_our_element_takes_the_focus_misses_the_old_field() {
    let mut ui = ui_with(bar);
    ui.app_mut().session.selection = [4].into_iter().collect();
    ui.redraw();
    let field = ui.value_field(4, 0);
    click(&mut ui, field);
    assert!(ui.focused_field().is_some(), "the value field has the focus");
    ui.send(Message::MidiGhosts);
    assert_eq!(ui.runtime().focused(), Some(&go()));
    let typed = ui.typewrite("7");
    // Iced's field would send its new text if it still had the key.
    assert!(
        !typed
            .iter()
            .any(|m| matches!(m, Message::Inspector(crate::inspector::Message::ValueText { .. }))),
        "the key missed the field: {typed:?}"
    );
    assert!(ui.app().session.value_draft.is_none());
    assert_eq!(ui.focused_field(), None);
}

/// The keys Iced heard, as the app's key listener reports them.
fn iced_heard(messages: &[Message]) -> Vec<keyboard::Event> {
    messages
        .iter()
        .filter_map(|m| match m {
            Message::Key(event) => Some(event.clone()),
            _ => None,
        })
        .collect()
}

/// True for a press of `key` among what Iced heard.
fn pressed(heard: &[keyboard::Event], key: &keyboard::Key) -> bool {
    heard
        .iter()
        .any(|event| matches!(event, keyboard::Event::KeyPressed { key: got, .. } if got == key))
}

/// Only the key the runtime took is kept from Iced. A Tab the runtime
/// takes, then a modifier change and a key in the same batch: Iced hears
/// the last two. A taken Tab between two kept keys: Iced hears both keys.
#[test]
fn only_the_key_the_runtime_took_is_kept_from_iced() {
    use crate::ui_test::key_press;
    let none = keyboard::Modifiers::default();
    let tab = keyboard::Key::Named(keyboard::key::Named::Tab);
    let x = keyboard::Key::Character("x".into());
    let q = keyboard::Key::Character("q".into());

    let mut ui = ui_with(bar);
    let heard = iced_heard(&ui.step(&[
        key_press(tab.clone(), none, None, false),
        iced::Event::Keyboard(keyboard::Event::ModifiersChanged(keyboard::Modifiers::SHIFT)),
        key_press(x.clone(), none, Some("x"), false),
    ]));
    assert_eq!(ui.runtime().focused(), Some(&go()), "the runtime took the Tab");
    assert!(!pressed(&heard, &tab), "{heard:?}");
    assert!(
        heard
            .iter()
            .any(|event| matches!(event, keyboard::Event::ModifiersChanged(_))),
        "{heard:?}"
    );
    assert!(pressed(&heard, &x), "{heard:?}");

    let mut ui = ui_with(bar);
    let heard = iced_heard(&ui.step(&[
        key_press(x.clone(), none, Some("x"), false),
        key_press(tab.clone(), none, None, false),
        key_press(q.clone(), none, Some("q"), false),
    ]));
    assert_eq!(ui.runtime().focused(), Some(&go()));
    assert!(pressed(&heard, &x) && pressed(&heard, &q), "{heard:?}");
    assert!(!pressed(&heard, &tab), "{heard:?}");
}
