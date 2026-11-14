use super::*;
use iced::keyboard::{self, Key, Modifiers, key::Named};
use iced_runtime::futures::futures::{StreamExt, executor::block_on, stream};
use iced_runtime::futures::subscription::{Event as Runtime, into_recipes};

/// What Iced's subscriptions send for `event`, sorted, since Iced runs
/// them in no set order. The adapter's own event stream is left out: it
/// waits on the adapter, not on window events.
fn through_iced(app: &App, event: &iced::Event, status: Status) -> Vec<String> {
    let close = iced::window::close_requests().map(|_| Message::File(FileAction::Close));
    let scale = if app.a11y.is_active() {
        iced::event::listen_with(crate::a11y::rescaled).map(Message::A11y)
    } else {
        Subscription::none()
    };
    let subscriptions = [close, app.input_subscription(), always_subscription(), scale];
    let window = iced::window::Id::unique();
    let mut heard: Vec<String> = subscriptions
        .into_iter()
        .flat_map(into_recipes)
        .flat_map(|recipe| {
            let input = Runtime::Interaction {
                window,
                event: event.clone(),
                status,
            };
            block_on(recipe.stream(stream::iter(vec![input]).boxed()).collect::<Vec<_>>())
        })
        .map(|message| format!("{message:?}"))
        .collect();
    heard.sort();
    heard
}

/// What [`Listeners::messages`] sends for `event`, sorted the same way.
fn through_listeners(app: &App, event: &iced::Event, status: Status) -> Vec<String> {
    let mut heard: Vec<String> = app
        .listeners()
        .messages(event, status)
        .iter()
        .map(|message| format!("{message:?}"))
        .collect();
    heard.sort();
    heard
}

/// A key pressed with no modifiers.
fn press(key: Key) -> iced::Event {
    iced::Event::Keyboard(keyboard::Event::KeyPressed {
        modified_key: key.clone(),
        key,
        physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    })
}

/// The events each listener hears, and some none hears.
fn events() -> Vec<iced::Event> {
    use iced::window::Event as Window;
    vec![
        press(Key::Named(Named::Escape)),
        press(Key::Named(Named::F1)),
        press(Key::Character("a".into())),
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
        iced::Event::Mouse(iced::mouse::Event::CursorMoved {
            position: iced::Point::ORIGIN,
        }),
        iced::Event::Window(Window::CloseRequested),
        iced::Event::Window(Window::Rescaled(2.0)),
        iced::Event::Window(Window::FileDropped("/tmp/a.ron".into())),
        iced::Event::Window(Window::Focused),
    ]
}

/// The plain calls send what Iced's subscriptions send, for every event,
/// used by a widget or not, with and without a screen reader. Only the
/// order may differ, and Iced sets none.
#[test]
fn the_listeners_send_what_the_subscriptions_send() {
    for screen_reader in [false, true] {
        let mut app = crate::tests::app();
        if screen_reader {
            app.a11y_event(crate::a11y::Event::Activated);
        }
        for event in events() {
            for status in [Status::Ignored, Status::Captured] {
                assert_eq!(
                    through_listeners(&app, &event, status),
                    through_iced(&app, &event, status),
                    "{event:?}, {status:?}, screen reader {screen_reader}"
                );
            }
        }
    }
}

/// The close request becomes the app's close, so the window stays open
/// until the app has asked about unsaved work (files.md).
#[test]
fn the_close_request_is_the_apps_close() {
    let app = crate::tests::app();
    let close = iced::Event::Window(iced::window::Event::CloseRequested);
    let messages = app.listeners().messages(&close, Status::Ignored);
    assert!(
        matches!(messages.as_slice(), [Message::File(FileAction::Close)]),
        "{messages:?}"
    );
}

/// The listeners are a snapshot: taken before a change of who listens,
/// they still answer as the app stood, as Iced's subscriptions do for a
/// batch of events.
#[test]
fn the_listeners_are_a_snapshot() {
    let mut app = crate::tests::app();
    let before = app.listeners();
    app.a11y_event(crate::a11y::Event::Activated);
    assert_ne!(app.listeners(), before);
    let rescaled = iced::Event::Window(iced::window::Event::Rescaled(2.0));
    assert!(before.messages(&rescaled, Status::Ignored).is_empty());
    assert_eq!(app.listeners().messages(&rescaled, Status::Ignored).len(), 1);
}
