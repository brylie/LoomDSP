//! A single-line text field, with no window.
//!
//! The app holds a name and shows it in a field. The field edits in place
//! and tells the app each change with `App::field_event`; the app takes
//! the change into its model, so the next view shows it (docs/design/
//! ui-layer.md, section 8, rule 2). Undo inside the field is the field's
//! own (rule 3). The field moves by word with the system's fonts, so the
//! example uses them too.
//!
//! Run it with `cargo run -p loom-ui --example field`.

use loom_ui::access::{Access, Role};
use loom_ui::headless::Harness;
use loom_ui::paint::Typesetter;
use loom_ui::{
    App, Context, DriverEvent, Effects, Element, FieldEvent, Layer, Length, NoTarget, View, WindowSpec, WindowState,
};
use loom_ui_core::{Event, Key, KeyEvent, Modifiers, Size, TextStyle, UiId, Weight};
use std::time::Instant;

/// How text is set in this example.
const STYLE: TextStyle = TextStyle {
    size: 13.0,
    line_height: 1.25,
    weight: Weight::Regular,
};

/// What the example app does.
#[derive(Debug)]
enum Message {
    /// Focus the field.
    Focus,
    /// What the field said.
    Field(FieldEvent),
}

/// An app with one field for a name.
#[derive(Default)]
struct Example {
    name: String,
    committed: Option<String>,
}

/// The field's UI ID.
fn field_id() -> UiId {
    UiId::panel("form").with("name")
}

/// The form: one field, showing `name`.
#[expect(
    clippy::ptr_arg,
    reason = "`View::panel_with` hands the builder its model, a `String`, by reference"
)]
fn form(name: &String) -> Element {
    Element::column([Element::field(name.clone(), STYLE)
        .key("name")
        .width(Length::Fixed(200.0))
        .access(Access::new(Role::TextInput, "Name"))])
}

impl App for Example {
    type Message = Message;

    fn update(&mut self, message: Message, _cx: &Context<'_, Message>) -> Effects<Message> {
        match message {
            Message::Focus => return Effects::focus(field_id()),
            Message::Field(FieldEvent::Edited(text)) => self.name = text,
            Message::Field(FieldEvent::Committed(text)) => self.committed = Some(text),
            Message::Field(FieldEvent::Left) => {}
            // A real app shows a status message, which a screen reader
            // announces (ui-layer.md, section 8, rule 2).
            Message::Field(FieldEvent::PasteRefused) => println!("Couldn't read the clipboard"),
        }
        Effects::none()
    }

    fn view(&self, view: &mut View) {
        view.panel_with(UiId::panel("form"), Layer::Panels, &self.name, form);
    }

    fn window(&self) -> WindowSpec {
        WindowSpec {
            title: "Field".to_owned(),
            min_size: None,
        }
    }

    fn field_event(&self, _id: &UiId, event: &FieldEvent) -> Option<Message> {
        Some(Message::Field(event.clone()))
    }
}

/// A key press, typing `text`.
fn press(key: Key, modifiers: Modifiers, text: Option<&str>) -> DriverEvent {
    DriverEvent::Input {
        event: Event::Key(KeyEvent::Pressed {
            key,
            modifiers,
            repeat: false,
            text: text.map(str::to_owned),
        }),
        pointer: None,
    }
}

/// The shortcut key for undo on the platform this was built for.
fn shortcut() -> Modifiers {
    if cfg!(target_os = "macos") {
        Modifiers {
            meta: true,
            ..Modifiers::NONE
        }
    } else {
        Modifiers {
            control: true,
            ..Modifiers::NONE
        }
    }
}

fn main() {
    let mut text = Typesetter::new();
    let window = WindowState::new(Size::new(320.0, 80.0));
    let mut harness = Harness::new(Example::default(), NoTarget, window, Instant::now());
    harness.lay_out(&mut text);
    harness.send(Message::Focus);
    harness.lay_out(&mut text);

    for letter in "Loom".chars() {
        let typed = letter.to_string();
        harness.step(press(Key::Character(letter), Modifiers::NONE, Some(&typed)));
    }
    harness.lay_out(&mut text);
    println!("Typed: the app holds {:?}", harness.app().name);

    harness.step(press(Key::Character('z'), shortcut(), None));
    harness.lay_out(&mut text);
    println!("Undo, inside the field: the app holds {:?}", harness.app().name);

    for letter in "Weave".chars() {
        let typed = letter.to_string();
        harness.step(press(Key::Character(letter), Modifiers::NONE, Some(&typed)));
    }
    harness.step(press(Key::Enter, Modifiers::NONE, None));
    println!("Enter: the app committed {:?}", harness.app().committed);
}
