//! Routing between Iced and the runtime's own layers, in the app
//! (ui-layer.md, section 5, rule 10; section 6, rules 1 and 2). The app
//! shows no `loom-ui` panel yet, so these tests add two through `Hosted`'s
//! test hook: a bar of one button at the bottom left, over the stats bar,
//! below Iced; and a pop-up over the toolbar's left end, above Iced.

use crate::Message;
use crate::iced_bridge::TestPanels;
use crate::ui_test::Ui;
use iced::keyboard;
use loom_canvas::iced_convert::ToIced as _;
use loom_ui::access::{Access, Action, Role};
use loom_ui::{Element, Layer, Length, View};
use loom_ui_core::{Key, Modifiers, Point, UiId};

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
    ui.with_test_panels(TestPanels {
        view,
        clicked,
        focuses,
        fields: |_, _| None,
    });
    ui
}

/// The centre of the toolbar's button `key`, by its UI ID: text such as
/// Open is shown by the inspector too.
fn toolbar_centre(ui: &mut Ui, key: &'static str) -> Point {
    ui.lay_out_panels();
    ui.runtime()
        .bounds(&crate::toolbar::toolbar_id().with(key))
        .expect("the toolbar's button")
        .center()
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
/// bar's: its message runs, and its button takes no focus from the press
/// (section 6, rule 3; Brylie's decision, 2026-11-19). That message opens
/// help, which takes the focus to its Close button, with no ring, since
/// the pointer opened it (help-pages.md, Keys, links, and focus).
#[test]
fn a_question_holds_presses_on_both_toolkits() {
    let mut ui = ui_with(bar);
    ui.app_mut().overlays.question = Some(crate::question::Question::error("No".to_string()));
    ui.redraw();
    let open = toolbar_centre(&mut ui, "open");
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
    let _ = ui.redraw();
    assert_eq!(
        ui.runtime().focused(),
        Some(&crate::overlays::access::help_id().with("close")),
        "the button took no focus; help, which it opened, did"
    );
    assert_eq!(ui.runtime().focus_ring(), None, "opened by the pointer: no ring");
}

/// A press Iced captures never reaches the runtime's layers, so the focus
/// they had stays (section 6, rule 1 is the target's to keep there): a
/// press on a tile of the canvas, the one part Iced still draws since the
/// matrix moved (plan 3.2, row 4e).
#[test]
fn a_press_iced_captures_leaves_our_focus() {
    let mut ui = ui_with(bar);
    // Focused as one of the app's panels would ask, with no click.
    ui.send(Message::MidiGhosts);
    assert_eq!(ui.runtime().focused(), Some(&go()));
    let tile = ui.app().session.scene.module(4).expect("the filter's tile").bounds();
    let at = ui
        .canvas()
        .to_window(loom_ui_core::Point::new(tile.center().x, tile.y + 10.0));
    click(&mut ui, at);
    assert_eq!(ui.runtime().focused(), Some(&go()), "Iced took that press");
}

/// A layer above Iced takes its point before Iced hears it, and before
/// the toolbar below Iced: a click on the pop-up over the toolbar is the
/// pop-up's, and the toolbar under it hears nothing. A move onto it hovers
/// it; a move off, onto the bar below Iced, moves the hover there; a move
/// onto the toolbar's Undo, past the pop-up, hovers Undo; and a move onto
/// the canvas, Iced's, clears it.
#[test]
fn a_layer_above_iced_keeps_its_points_from_iced() {
    let mut ui = ui_with(bar_and_pop);
    let open = toolbar_centre(&mut ui, "open");
    assert!(open.x < POP.0 && open.y < POP.1, "the pop-up covers Open at {open:?}");
    // The other half of what the pop-up's size assumes: Undo starts past
    // its right edge.
    ui.lay_out_panels();
    let undo_box = ui
        .runtime()
        .bounds(&crate::toolbar::toolbar_id().with("undo"))
        .expect("the toolbar shows Undo");
    assert!(undo_box.x >= POP.0, "the pop-up leaves Undo out, at {undo_box:?}");
    let heard = click(&mut ui, open);
    assert!(heard.iter().any(|m| matches!(m, Message::ShowShortcuts)), "{heard:?}");
    assert!(!heard.iter().any(|m| matches!(m, Message::File(_))), "{heard:?}");
    let _ = ui.deliver(Message::CloseHelp);

    ui.move_to(Point::new(10.0, 10.0));
    assert_eq!(ui.runtime().hovered(), Some(&UiId::panel("test-pop").with("pop")));
    ui.move_to(on_go());
    assert_eq!(ui.runtime().hovered(), Some(&go()));
    ui.move_to(undo_box.center());
    assert_eq!(ui.runtime().hovered(), Some(&UiId::panel("toolbar").with("undo")));
    // The canvas's middle: Iced's, and clear of Undo's hover text, which
    // keeps the hover while the pointer is on it.
    ui.move_to(Point::new(500.0, 400.0));
    assert_eq!(ui.runtime().hovered(), None, "Iced holds the hover now");
}

/// Section 6, rule 2, between the toolkits: with nothing focused in
/// either, Tab is the canvas's, which reads first, and Shift+Tab enters
/// the runtime's last stop. Iced never hears a Tab the runtime took, so no
/// field of Iced's takes it too.
#[test]
fn shift_tab_from_nothing_enters_our_last_stop_and_not_iced() {
    let mut ui = ui_with(bar);
    assert_eq!(ui.focused_field(), None);
    ui.tap(Key::Tab, Modifiers::NONE);
    assert_eq!(ui.runtime().focused(), None, "Tab from nothing is the canvas's");
    ui.app_mut().session.selection.clear();
    ui.redraw();
    ui.tap(Key::Tab, Modifiers::SHIFT);
    assert_eq!(ui.runtime().focused(), Some(&go()));
    assert!(ui.runtime().focus_ring().is_some(), "from the keyboard, with a ring");
    assert_eq!(ui.focused_field(), None, "no Iced field took the Tab");
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
fn pressed(heard: &[keyboard::Event], key: &Key) -> bool {
    let key = key.to_iced();
    heard
        .iter()
        .any(|event| matches!(event, keyboard::Event::KeyPressed { key: got, .. } if *got == key))
}

/// Only the key the runtime took is kept from Iced. A Shift+Tab the runtime
/// takes, then a modifier change and a key in the same batch: Iced hears
/// the last two. A taken Shift+Tab between two kept keys: Iced hears both.
#[test]
fn only_the_key_the_runtime_took_is_kept_from_iced() {
    use crate::ui_test::key_press;
    let none = Modifiers::NONE;
    let tab = Key::Tab;
    let x = Key::Character('x');
    let q = Key::Character('q');

    let shift = Modifiers::SHIFT;
    let mut ui = ui_with(bar);
    let heard = iced_heard(&ui.step(&[
        key_press(tab, shift, None, false),
        iced::Event::Keyboard(keyboard::Event::ModifiersChanged(keyboard::Modifiers::SHIFT)),
        key_press(x, none, Some("x"), false),
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
        key_press(x, none, Some("x"), false),
        key_press(tab, shift, None, false),
        key_press(q, none, Some("q"), false),
    ]));
    assert_eq!(ui.runtime().focused(), Some(&go()));
    assert!(pressed(&heard, &x) && pressed(&heard, &q), "{heard:?}");
    assert!(!pressed(&heard, &tab), "{heard:?}");
}

/// A field of the runtime's, in the panels layer, below Iced.
fn bar_with_field(view: &mut View) {
    use loom_ui::paint::{TextStyle, Weight};
    const STYLE: TextStyle = TextStyle {
        size: 13.0,
        line_height: 1.25,
        weight: Weight::Regular,
    };
    view.panel_with(UiId::panel("test-bar"), Layer::Panels, &(), |_| {
        Element::row([Element::field(String::new(), STYLE)
            .key("go")
            .width(Length::Fixed(120.0))
            .access(Access::new(Role::TextInput, "Name"))])
    });
}

/// The meeting point of the field and the layers (ui-layer.md, section 6,
/// rule 1): a key the runtime's field takes is kept from Iced, with Iced's
/// toolbar and canvas below it. Typing, an arrow, and Backspace are the
/// field's and never reach Iced's key listener, so Backspace never deletes
/// a selected module, and the shortcut for undo never reaches the patch.
/// (The panel has no model, so the field's text goes back to empty at each
/// layout; loom-ui's own tests check the editing.) Their releases go on
/// to Iced by design. A field takes no Tab:
/// with the field the only stop, Tab steps past the end and focus leaves
/// the runtime's elements for Iced's (section 6, rule 2). Escape, which the field passes on,
/// still reaches Iced. The field is focused by a focus effect, so the test
/// needs nothing of Iced's focus.
#[test]
fn a_key_our_field_takes_is_kept_from_iced() {
    use crate::ui_test::key_press;
    let none = Modifiers::NONE;
    let a = Key::Character('a');
    let b = Key::Character('b');
    let z = Key::Character('z');
    let left = Key::Left;
    let backspace = Key::Backspace;
    let tab = Key::Tab;
    let escape = Key::Escape;

    let mut ui = ui_with(bar_with_field);
    ui.app_mut().session.selection = [4].into_iter().collect();
    ui.send(Message::MidiGhosts);
    assert_eq!(ui.runtime().focused(), Some(&go()), "our field has the focus");
    let modules = ui.app().session.controller.graph().len();

    let heard = iced_heard(&ui.step(&[
        key_press(a, none, Some("a"), false),
        key_press(b, none, Some("b"), false),
        key_press(left, none, None, false),
        key_press(backspace, none, None, false),
    ]));
    for key in [&a, &b, &left, &backspace] {
        assert!(!pressed(&heard, key), "{key:?} reached Iced: {heard:?}");
    }
    assert_eq!(
        ui.app().session.controller.graph().len(),
        modules,
        "Backspace deleted no module"
    );

    // Only presses are kept (`Target::kept`): a release goes on to Iced by
    // design, so nothing in Iced holds a key down.
    let heard = iced_heard(&ui.step(&[
        crate::ui_test::key_released(a, none),
        crate::ui_test::key_released(backspace, none),
    ]));
    let released = |key: &Key| {
        let key = key.to_iced();
        heard
            .iter()
            .any(|event| matches!(event, keyboard::Event::KeyReleased { key: got, .. } if *got == key))
    };
    assert!(released(&a) && released(&backspace), "{heard:?}");
    assert_eq!(ui.runtime().focused(), Some(&go()), "a release moves no focus");

    let heard = iced_heard(&ui.step(&[key_press(z, Modifiers::platform_shortcut(), None, false)]));
    assert!(!pressed(&heard, &z), "{heard:?}");

    let heard = iced_heard(&ui.step(&[key_press(escape, none, None, false)]));
    assert!(pressed(&heard, &escape), "{heard:?}");
    assert_eq!(
        ui.runtime().focused(),
        Some(&go()),
        "Escape left the focus on our field"
    );

    ui.step(&[key_press(tab, none, None, false)]);
    assert_ne!(ui.runtime().focused(), Some(&go()), "Tab left our field");
}

/// An input method event, as Iced reports one.
fn ime(event: iced::advanced::input_method::Event) -> iced::Event {
    iced::Event::InputMethod(event)
}

/// The input method reaches our field through the bridge, and is kept
/// from Iced as a taken key is (section 6, rule 1). While it composes, the
/// keys are the input method's: Tab moves no focus off our field, and
/// neither Tab nor Backspace reaches Iced.
#[test]
fn the_input_method_reaches_our_field_and_not_iced() {
    use crate::ui_test::key_press;
    use iced::advanced::input_method::Event as Method;
    let none = Modifiers::NONE;
    let tab = Key::Tab;
    let backspace = Key::Backspace;

    let mut ui = ui_with(bar_with_field);
    ui.send(Message::MidiGhosts);
    assert_eq!(ui.runtime().focused(), Some(&go()), "our field has the focus");
    assert!(
        matches!(ui.runtime().ime(), Some(loom_ui::Ime::On { .. })),
        "the window's input method follows our field"
    );
    ui.step(&[ime(Method::Opened), ime(Method::Preedit("に".to_owned(), Some(3..3)))]);
    let heard = iced_heard(&ui.step(&[
        key_press(tab, none, None, false),
        key_press(backspace, none, None, false),
    ]));
    assert_eq!(
        ui.runtime().focused(),
        Some(&go()),
        "Tab is the input method's while it composes"
    );
    assert!(!pressed(&heard, &tab) && !pressed(&heard, &backspace), "{heard:?}");
}
