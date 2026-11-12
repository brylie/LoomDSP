use super::*;
use crate::event::DriverEvent;
use crate::headless::Harness;
use crate::routing::NoTarget;
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// The minimal app's messages.
#[derive(Debug, Clone, PartialEq)]
enum Msg {
    /// Starts a watcher thread with the context's waker.
    Watch,
    /// What the watcher sends.
    Heard(u32),
    /// Records the window state from the context.
    Look,
}

/// An app that keeps every default method.
#[derive(Default)]
struct Minimal {
    seen: Vec<Msg>,
    windows: Vec<WindowState>,
    watcher: Option<std::thread::JoinHandle<()>>,
}

impl App for Minimal {
    type Message = Msg;

    fn update(&mut self, message: Msg, cx: &Context<'_, Msg>) -> Effects<Msg> {
        self.seen.push(message.clone());
        match message {
            Msg::Watch => {
                let waker = cx.waker();
                self.watcher = Some(std::thread::spawn(move || {
                    waker.send(Msg::Heard(7));
                }));
            }
            Msg::Look => self.windows.push(cx.window()),
            Msg::Heard(_) => {}
        }
        Effects::none()
    }

    fn view(&self, _view: &mut View) {}

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }
}

/// The defaults: no deadline, no message for a window event, no frames.
#[test]
fn an_app_with_the_defaults_sleeps_and_ignores_window_events() {
    let mut harness = Harness::new(
        Minimal::default(),
        NoTarget,
        WindowState::new(Size::new(10.0, 10.0)),
        Instant::now(),
    );
    assert!(harness.next().sleeps());
    let next = harness.step(DriverEvent::Window(WindowEvent::CloseRequested));
    assert!(next.sleeps());
    assert!(harness.app().seen.is_empty());
}

/// A watcher started with the context's waker reaches the app, and the
/// context shows the window as the runtime knows it.
#[test]
fn the_context_gives_a_waker_and_the_window() {
    let mut harness = Harness::new(
        Minimal::default(),
        NoTarget,
        WindowState::new(Size::new(320.0, 200.0)),
        Instant::now(),
    );
    harness.send(Msg::Watch);
    let mut heard = false;
    for _ in 0..1000 {
        harness.step(DriverEvent::Woken);
        if harness.app().seen.contains(&Msg::Heard(7)) {
            heard = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(heard, "the watcher's message arrived");
    harness.send(Msg::Look);
    assert_eq!(harness.app().windows, [WindowState::new(Size::new(320.0, 200.0))]);
}

/// A context made outside a runtime, as the bridge to Iced and a test
/// stepping one app make them, gives back what it was made with.
#[test]
fn a_context_made_by_hand_reads_back_what_it_was_given() {
    let (sender, _queue) = mpsc::channel::<Msg>();
    let waker = Waker::new(sender, Arc::new(|| {}));
    let now = Instant::now();
    let field = UiId::panel("inspector").with("field");
    let window = WindowState::new(Size::new(640.0, 480.0));
    let cx = Context::new(now, &waker, Some(field.clone()), window);
    assert_eq!(cx.now(), now);
    assert_eq!(cx.focused(), Some(&field));
    assert_eq!(cx.window(), window);
    assert!(
        cx.waker().send(Msg::Look),
        "the waker sends into the queue it was made with"
    );
}
