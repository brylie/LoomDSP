use super::*;
use std::sync::mpsc;
use std::time::Duration;

/// A waker whose messages land in the returned receiver.
fn waker() -> (Waker<u32>, mpsc::Receiver<u32>) {
    let (sender, inbox) = mpsc::channel();
    (Waker::new(sender, Arc::new(|| {})), inbox)
}

/// An open dialog with nothing set.
fn open_request() -> FileRequest {
    FileRequest {
        action: crate::effect::FileAction::Open,
        title: "Open".into(),
        filters: Vec::new(),
        folder: None,
        file_name: None,
    }
}

/// A driver with no window over `NoServices`.
fn driver() -> ShellDriver<u32, NoServices> {
    ShellDriver {
        services: NoServices,
        window: None,
        captures: Vec::new(),
        exit: false,
    }
}

/// With no services, the clipboard reads empty and each dialog is
/// dismissed, each answer through the waker.
#[test]
fn no_services_answers_empty_and_dismissed() {
    let (waker, inbox) = waker();
    let mut services = NoServices;
    Services::<u32>::clipboard_write(&mut services, "kept nowhere".into());
    services.clipboard_read(Reply::new(|text: Option<String>| u32::from(text.is_some())), &waker);
    services.file_dialog(
        open_request(),
        Reply::new(|path: Option<PathBuf>| 10 + u32::from(path.is_some())),
        &waker,
    );
    services.ask(
        Question {
            level: crate::effect::Level::Info,
            title: "Title".into(),
            text: "Text".into(),
            buttons: vec!["OK".into()],
        },
        Reply::new(|choice: Choice| 20 + u32::from(choice.is_some())),
        &waker,
    );
    assert_eq!(inbox.try_iter().collect::<Vec<_>>(), [0, 10, 20]);
}

/// A capture waits for the end of the turn, when the shell has its layers
/// to draw with; nothing is answered before.
#[test]
fn a_capture_waits_for_the_end_of_the_turn() {
    let (waker, inbox) = waker();
    let mut driver = driver();
    driver.capture(Reply::new(|_image: Result<Image, NoGpu>| 1), &waker);
    assert_eq!(driver.captures.len(), 1);
    assert!(inbox.try_recv().is_err());
}

/// Quitting sets a flag the loop acts on after the turn, since a driver
/// call has no event loop in hand.
#[test]
fn quitting_sets_a_flag() {
    let mut driver = driver();
    Driver::<u32>::exit(&mut driver);
    assert!(driver.exit);
}

/// Work runs on another thread and answers through the waker.
#[test]
fn work_runs_on_another_thread() {
    let (waker, inbox) = waker();
    let mut driver = driver();
    let here = thread::current().id();
    driver.work(
        Job::new(move |waker: &Waker<u32>| {
            waker.send(u32::from(thread::current().id() != here));
        }),
        &waker,
    );
    assert_eq!(inbox.recv_timeout(Duration::from_secs(5)), Ok(1));
}

/// The clipboard and dialogs go to the services; the title and limits go
/// nowhere while there is no window.
#[test]
fn the_rest_goes_to_the_services() {
    let (waker, inbox) = waker();
    let mut driver = driver();
    driver.window(&WindowSpec {
        title: "No window yet".into(),
        min_size: None,
    });
    driver.clipboard_write("text".into());
    driver.clipboard_read(Reply::new(|_: Option<String>| 3), &waker);
    driver.file_dialog(open_request(), Reply::new(|_: Option<PathBuf>| 4), &waker);
    driver.ask(
        Question {
            level: crate::effect::Level::Info,
            title: String::new(),
            text: String::new(),
            buttons: Vec::new(),
        },
        Reply::new(|_: Choice| 5),
        &waker,
    );
    assert_eq!(inbox.try_iter().collect::<Vec<_>>(), [3, 4, 5]);
}

/// An app that keeps each message with the focus it ran under.
#[derive(Default)]
struct Log {
    /// Each message, and the target's focus when it ran.
    seen: Vec<(u32, Option<loom_ui_core::UiId>)>,
}

impl App for Log {
    type Message = u32;

    fn update(&mut self, message: u32, cx: &crate::app::Context<'_, u32>) -> crate::effect::Effects<u32> {
        self.seen.push((message, cx.focused().cloned()));
        crate::effect::Effects::none()
    }

    fn view(&self, _view: &mut crate::view::View) {}

    fn window(&self) -> WindowSpec {
        WindowSpec {
            title: "Log".into(),
            min_size: None,
        }
    }
}

/// A target that only keeps what has focus.
#[derive(Default)]
struct Focus(Option<loom_ui_core::UiId>);

impl Target<u32> for Focus {
    fn input(&mut self, _: &loom_ui_core::Event, _: Option<loom_ui_core::Point>, _: Instant, _: &mut Vec<u32>) {}

    fn focus(&mut self, _: &crate::effect::FocusTarget, _: crate::effect::Selection) {}

    fn scroll_into_view(&mut self, _: &loom_ui_core::UiId) {}

    fn focused(&self) -> Option<loom_ui_core::UiId> {
        self.0.clone()
    }

    fn needs_redraw(&mut self, _: usize) -> bool {
        false
    }
}

/// Layers that hold a focus answer: they move the target's focus, then
/// hand over its message.
#[derive(Default)]
struct Answer(Option<(u32, loom_ui_core::UiId)>);

impl Layers<Log, Focus> for Answer {
    fn draw(&mut self, _: Parts<'_, Log, Focus>, _: &Frame<'_>) {}

    fn alone(&mut self, parts: Parts<'_, Log, Focus>) -> Option<u32> {
        let (message, part) = self.0.take()?;
        parts.target.0 = Some(part);
        Some(message)
    }
}

/// A message queued in the same instant as a focus answer runs first, in
/// its own turn, under the focus it was sent under; the answer's message
/// runs after, alone, under the new focus. So the app's copy of focus
/// never drifts from the target's within a turn.
#[test]
fn a_queued_message_runs_before_a_focus_answer_alone() {
    let field = loom_ui_core::UiId::panel("inspector").with("field");
    let mut runtime = Runtime::new(
        Log::default(),
        Focus::default(),
        WindowState::new(Size::new(100.0, 100.0)),
        Arc::new(|| {}),
    );
    let mut layers = Answer(Some((2, field.clone())));
    runtime.waker().send(1);
    let nexts = alone_turns(&mut runtime, &mut layers, &mut driver());
    assert_eq!(nexts.len(), 2);
    assert_eq!(runtime.counts().turns, 2);
    assert_eq!(runtime.app().seen, [(1, None), (2, Some(field))]);
}

/// With no message from the layers, only the queued turn runs.
#[test]
fn an_empty_answer_runs_only_the_queue() {
    let mut runtime = Runtime::new(
        Log::default(),
        Focus::default(),
        WindowState::new(Size::new(100.0, 100.0)),
        Arc::new(|| {}),
    );
    runtime.waker().send(1);
    let nexts = alone_turns(&mut runtime, &mut Answer::default(), &mut driver());
    assert_eq!(nexts.len(), 1);
    assert_eq!(runtime.app().seen, [(1, None)]);
}

/// The pointer is hidden and shown as `iced_winit` does it: hiding sets no
/// cursor, a second hide does nothing, and the next cursor shows the
/// pointer again; a cursor while shown only sets the cursor.
#[test]
fn the_pointer_hides_and_shows_again() {
    let mut hidden = false;
    let arrow = Pointer::Icon(CursorIcon::Default);
    assert_eq!(pointer_calls(&mut hidden, arrow), (Some(CursorIcon::Default), None));
    assert_eq!(pointer_calls(&mut hidden, Pointer::Hidden), (None, Some(false)));
    assert_eq!(pointer_calls(&mut hidden, Pointer::Hidden), (None, None));
    assert_eq!(
        pointer_calls(&mut hidden, Pointer::Icon(CursorIcon::Pointer)),
        (Some(CursorIcon::Pointer), Some(true))
    );
    assert!(!hidden);
}

/// A capture asked for before the first frame waits for it, so it never
/// gives a blank image; once a frame has been shown, every capture waiting
/// is drawn.
#[test]
fn a_capture_waits_for_the_first_frame() {
    let mut captures = vec![1, 2];
    assert!(due_captures(false, &mut captures).is_empty());
    assert_eq!(captures, [1, 2], "still waiting");
    assert_eq!(due_captures(true, &mut captures), [1, 2]);
    assert!(captures.is_empty());
}
