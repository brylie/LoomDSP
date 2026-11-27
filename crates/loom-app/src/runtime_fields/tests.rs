//! The wiring between the app and the runtime's text fields, on a test
//! form, not on any view's field: each field's edits and Enter become its
//! own messages, a focus question sees a focused runtime field, a screen
//! reader edits one through its node, and none of it outlives the field.

use crate::Message;
use crate::iced_bridge::TestPanels;
use crate::ui_test::Ui;
use crate::window_question::WindowQuestion;
use loom_ui::access::{Access, Role};
use loom_ui::{Element, FieldEvent, Layer, Length, View};
use loom_ui_core::{Key, Modifiers, UiId};
use std::cell::Cell;

thread_local! {
    /// Whether the test form is shown: a test hides it to see what is left
    /// once its field is gone.
    static SHOWN: Cell<bool> = const { Cell::new(true) };
    /// The form's text, as a view keeps its fields' text: the name field's,
    /// then the search field's.
    static TEXT: std::cell::RefCell<(String, String)> = const { std::cell::RefCell::new((String::new(), String::new())) };
}

/// The form's field whose view maps both an edit and Enter.
fn name() -> UiId {
    UiId::panel("test-form").with("name")
}

/// The form's field whose view maps only an edit, as a search field does.
fn search() -> UiId {
    UiId::panel("test-form").with("search")
}

/// The form: two fields at the window's top left, while it is shown.
fn form(view: &mut View) {
    if !SHOWN.with(Cell::get) {
        return;
    }
    let text = TEXT.with(|text| text.borrow().clone());
    view.panel_with(UiId::panel("test-form"), Layer::Overlays, &text, |text| {
        let field = |key: &str, label: &str, value: &str| {
            Element::field(value.to_string(), crate::style::BODY_TEXT)
                .key(key)
                .width(Length::Fixed(160.0))
                .access(Access::new(Role::TextInput, label))
        };
        Element::column([field("name", "Name", &text.0), field("search", "Search", &text.1)])
    });
}

/// The message a test looks for: a help link, which says only that it is
/// not bundled, carrying what happened as its text.
fn said(text: &str) -> Message {
    Message::HelpLink(text.to_string())
}

/// The form's mapping, as a view's own function would give it: `name`
/// maps an edit and Enter, `search` an edit only.
fn fields(id: &UiId, event: &FieldEvent) -> Option<Message> {
    if let FieldEvent::Edited(text) = event {
        TEXT.with(|kept| {
            let mut kept = kept.borrow_mut();
            if *id == name() {
                kept.0.clone_from(text);
            } else if *id == search() {
                kept.1.clone_from(text);
            }
        });
    }
    match event {
        FieldEvent::Edited(text) if *id == name() => Some(said(&format!("name edited {text}"))),
        FieldEvent::Committed(text) if *id == name() => Some(said(&format!("name committed {text}"))),
        FieldEvent::Edited(text) if *id == search() => Some(said(&format!("search edited {text}"))),
        _ => None,
    }
}

/// `MidiGhosts` focuses the name field, and `MidiPoll` the search field,
/// as the form's view would ask with a focus effect.
fn focuses(message: &Message) -> Option<UiId> {
    match message {
        Message::MidiGhosts => Some(name()),
        Message::MidiPoll => Some(search()),
        _ => None,
    }
}

/// The subtractive patch with the form shown.
fn with_form() -> Ui {
    SHOWN.with(|shown| shown.set(true));
    TEXT.with(|text| *text.borrow_mut() = (String::new(), String::new()));
    let mut ui = Ui::open("subtractive.ron");
    ui.with_test_panels(TestPanels {
        view: form,
        clicked: |_| None,
        focuses,
        fields,
    });
    ui
}

/// True when `messages` holds `message`.
fn heard(messages: &[Message], message: &Message) -> bool {
    let wanted = format!("{message:?}");
    messages.iter().any(|message| format!("{message:?}") == wanted)
}

/// Asks the window where focus is, as Tab from a field, the clipboard keys,
/// and a script's typing do, and gives the answer the app heard.
fn focus_answer(ui: &mut Ui) -> Option<UiId> {
    ui.app().ask_window(WindowQuestion::focus(|part| {
        said(&format!(
            "focus {}",
            part.map_or_else(|| "none".to_string(), |part| part.to_string())
        ))
    }));
    let log = ui.deliver(Message::MidiGhosts);
    log.iter().rev().find_map(|message| match message {
        Message::HelpLink(text) => text
            .strip_prefix("focus ")
            .map(|part| (part != "none").then(|| part.parse().expect("a UI ID"))),
        _ => None,
    })?
}

/// Typing in a runtime field gives its view's edit message, and Enter its
/// view's Enter message; a field whose view maps no Enter ignores it.
#[test]
fn each_field_gives_its_own_messages_and_enter_only_where_mapped() {
    let mut ui = with_form();
    ui.deliver(Message::MidiGhosts);
    ui.redraw();
    assert_eq!(ui.runtime().focused(), Some(&name()));
    let typed = ui.typewrite("ab");
    assert!(heard(&typed, &said("name edited ab")), "{typed:?}");
    let entered = ui.tap(Key::Enter, Modifiers::NONE);
    assert!(heard(&entered, &said("name committed ab")), "{entered:?}");

    ui.deliver(Message::MidiPoll);
    ui.redraw();
    assert_eq!(ui.runtime().focused(), Some(&search()));
    let typed = ui.typewrite("saw");
    assert!(heard(&typed, &said("search edited saw")), "{typed:?}");
    let entered = ui.tap(Key::Enter, Modifiers::NONE);
    assert!(
        !entered
            .iter()
            .any(|message| matches!(message, Message::HelpLink(text) if text.contains("committed"))),
        "the search field maps no Enter: {entered:?}"
    );
}

/// While a runtime field has focus, the app's copy of the focus names it,
/// and a focus question is answered with it, not with Iced's empty answer:
/// so Cmd+D, which the field leaves, asks first and leaves the patch alone.
/// Once the field's panel is gone, the question goes to Iced again, and
/// Cmd+D duplicates the selected tile, so nothing still takes the field
/// for focused.
#[test]
fn a_focus_question_sees_the_field_until_it_goes() {
    let duplicate = |ui: &mut Ui| {
        let before = ui.app().session.controller.graph().nodes().count();
        ui.tap(Key::Character('d'), Modifiers::platform_shortcut());
        ui.redraw();
        ui.app().session.controller.graph().nodes().count() - before
    };
    let mut ui = with_form();
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::Select(vec![4])));
    ui.deliver(Message::MidiGhosts);
    ui.redraw();
    assert_eq!(ui.app().focus, Some(name()), "the app's copy names the field");
    assert_eq!(focus_answer(&mut ui), Some(name()), "answered with the runtime's field");
    assert_eq!(duplicate(&mut ui), 0, "Cmd+D in the field leaves the patch alone");

    SHOWN.with(|shown| shown.set(false));
    ui.deliver(Message::MidiPoll);
    ui.redraw();
    assert_eq!(ui.runtime().focused(), None, "the field went with its panel");
    assert_eq!(focus_answer(&mut ui), None, "Iced answers again: no field");
    assert_eq!(duplicate(&mut ui), 1, "Cmd+D duplicates the selected tile again");
}

/// A screen reader's SetValue on a runtime field's node reaches the field
/// through the app's node table (`App::access_target`), and the field's
/// view hears the edit.
#[test]
fn a_screen_reader_sets_a_fields_value_through_its_node() {
    let mut ui = with_form();
    ui.deliver(Message::A11y(crate::a11y::Event::Activated));
    ui.redraw();
    let node = ui.app_mut().a11y_node_of(&name());
    let heard_now = ui.accessibility_turn(loom_ui::AccessEvent::Action(accesskit::ActionRequest {
        action: accesskit::Action::SetValue,
        target_tree: accesskit::TreeId::ROOT,
        target_node: node,
        data: Some(accesskit::ActionData::Value("Ada".into())),
    }));
    assert!(heard(&heard_now, &said("name edited Ada")), "{heard_now:?}");
}

/// A change to what a field reports rebuilds the tree only while a screen
/// reader listens, so it costs nothing without one.
#[test]
fn a_field_change_rebuilds_the_tree_only_while_a_reader_listens() {
    let mut ui = with_form();
    ui.deliver(Message::MidiGhosts);
    ui.redraw();
    let typed = ui.typewrite("a");
    assert!(
        !typed
            .iter()
            .any(|message| matches!(message, Message::A11y(crate::a11y::Event::FieldChanged))),
        "no reader, no rebuild: {typed:?}"
    );
    ui.deliver(Message::A11y(crate::a11y::Event::Activated));
    ui.redraw();
    let typed = ui.typewrite("b");
    assert!(
        typed
            .iter()
            .any(|message| matches!(message, Message::A11y(crate::a11y::Event::FieldChanged))),
        "a reader hears the change: {typed:?}"
    );
}

/// A runtime field's paste that could not read the clipboard is said in
/// the status line, for any field, whatever its view maps (ui-layer.md,
/// section 8, rule 2): never shown as an empty clipboard.
#[test]
fn a_refused_paste_is_said_in_the_status_line_for_any_field() {
    let mut app = crate::ui_test::open_app("subtractive.ron");
    let message = app
        .field_message(&UiId::panel("any").with("field"), &FieldEvent::PasteRefused)
        .expect("a refused paste has a message");
    assert!(matches!(message, Message::FieldPasteRefused), "{message:?}");
    let _ = app.send(message);
    assert_eq!(app.alerts.status(), crate::clipboard::COULD_NOT_READ);
}
