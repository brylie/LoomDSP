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

/// A clipboard in memory, for the driver's tests: no display needed.
#[derive(Default)]
struct Memory(Option<String>);

impl crate::clipboard::Backend for Memory {
    fn get(&mut self) -> ClipboardText {
        ClipboardText::from_text(self.0.clone())
    }

    fn set(&mut self, text: String) {
        self.0 = Some(text);
    }
}

/// A driver with no window over `NoServices`, with a clipboard in memory.
fn driver() -> ShellDriver<u32, NoServices> {
    ShellDriver {
        services: NoServices,
        clipboard: crate::clipboard::Clipboard::new(Memory::default()),
        access: None,
        window: None,
        captures: Vec::new(),
        exit: false,
    }
}

/// With no services, each dialog is dismissed, its answer through the
/// waker.
#[test]
fn no_services_answers_dismissed() {
    let (waker, inbox) = waker();
    let mut services = NoServices;
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
    assert_eq!(inbox.try_iter().collect::<Vec<_>>(), [10, 20]);
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
    assert_eq!(inbox.try_iter().collect::<Vec<_>>(), [4, 5]);
}

/// The clipboard is the shell's own, on its thread: a write, then a read
/// that answers through the waker, and a field's paste that answers its
/// `Paste`. Neither answers in the call itself.
#[test]
fn the_clipboard_is_the_shells_own() {
    let (waker, inbox) = waker();
    let mut driver = driver();
    driver.clipboard_write("osc".into());
    driver.clipboard_read(
        Reply::new(|text: ClipboardText| u32::from(text == ClipboardText::Text("osc".into()))),
        &waker,
    );
    assert_eq!(inbox.recv_timeout(std::time::Duration::from_secs(5)), Ok(1));
    let (answers, pasted) = mpsc::channel();
    let wake: crate::Wake = std::sync::Arc::new(|| {});
    driver.clipboard_paste(crate::Paste::new(
        loom_ui_core::UiId::panel("form").with("name"),
        0,
        answers,
        wake,
    ));
    let (_, _, text) = pasted
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("a paste answer");
    assert_eq!(text, ClipboardText::Text("osc".into()));
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
    fn input(
        &mut self,
        _: &loom_ui_core::Event,
        _: Option<loom_ui_core::Point>,
        _: Instant,
        _: &mut Vec<u32>,
    ) -> crate::Routed {
        crate::Routed::Passed
    }

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
    let arrow = Pointer::Icon(Cursor::Default);
    assert_eq!(pointer_calls(&mut hidden, arrow), (Some(CursorIcon::Default), None));
    assert_eq!(pointer_calls(&mut hidden, Pointer::Hidden), (None, Some(false)));
    assert_eq!(pointer_calls(&mut hidden, Pointer::Hidden), (None, None));
    assert_eq!(
        pointer_calls(&mut hidden, Pointer::Icon(Cursor::Pointer)),
        (Some(CursorIcon::Pointer), Some(true))
    );
    assert!(!hidden);
}

/// Each of our shapes becomes its own winit cursor, and the resize shapes
/// the two-way arrows the canvas asks for.
#[test]
fn each_cursor_becomes_its_own_winit_cursor() {
    let icons: Vec<CursorIcon> = Cursor::ALL.into_iter().map(icon).collect();
    for (i, a) in icons.iter().enumerate() {
        for b in &icons[i + 1..] {
            assert_ne!(a, b);
        }
    }
    assert_eq!(icon(Cursor::Default), CursorIcon::Default);
    assert_eq!(icon(Cursor::ResizeHorizontal), CursorIcon::EwResize);
    assert_eq!(icon(Cursor::ResizeVertical), CursorIcon::NsResize);
    assert_eq!(icon(Cursor::ResizeDiagonal), CursorIcon::NwseResize);
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

/// An area for the input method, for the tests below.
fn area(x: f32) -> Ime {
    Ime::On {
        area: loom_ui_core::Rect::new(loom_ui_core::Point::new(x, 0.0), loom_ui_core::Size::new(10.0, 18.0)),
    }
}

/// The runtime's field comes first: while it has focus, its area is set
/// and a report from the layers (Iced's field) is only remembered.
#[test]
fn the_runtimes_field_sets_the_input_method_first() {
    let mut memory = ImeMemory::default();
    assert_eq!(ime_to_apply(&mut memory, Some(area(5.0)), None), Some(area(5.0)));
    assert_eq!(
        ime_to_apply(&mut memory, Some(area(5.0)), Some(area(90.0))),
        None,
        "the layers' is ignored"
    );
    assert_eq!(memory.layers, Some(area(90.0)), "but remembered");
}

/// When the runtime's field lets go, the layers' last report comes back;
/// with none, the input method turns off, so the window never keeps a
/// field's area. Nothing is set again while nothing changed.
#[test]
fn the_layers_state_comes_back_when_our_field_lets_go() {
    let mut memory = ImeMemory::default();
    ime_to_apply(&mut memory, Some(area(5.0)), None);
    ime_to_apply(&mut memory, Some(area(5.0)), Some(area(90.0)));
    assert_eq!(
        ime_to_apply(&mut memory, None, None),
        Some(area(90.0)),
        "a report while ours had it comes back"
    );
    assert_eq!(ime_to_apply(&mut memory, None, None), None, "nothing changed");

    let mut memory = ImeMemory::default();
    ime_to_apply(&mut memory, Some(area(5.0)), None);
    assert_eq!(ime_to_apply(&mut memory, None, None), Some(Ime::Off));
    assert_eq!(ime_to_apply(&mut memory, None, None), None);
}

/// The caret moving in our field sets the new area; a report from the
/// layers that changes nothing is not set again.
#[test]
fn a_moved_caret_sets_the_area_again() {
    let mut memory = ImeMemory::default();
    ime_to_apply(&mut memory, Some(area(5.0)), None);
    assert_eq!(ime_to_apply(&mut memory, Some(area(12.0)), None), Some(area(12.0)));
    let mut memory = ImeMemory::default();
    ime_to_apply(&mut memory, None, Some(Ime::Off));
    assert_eq!(ime_to_apply(&mut memory, None, Some(Ime::Off)), None);
}

/// The layers' field had the input method, then ours took the focus,
/// which released theirs; the layers may never report Off for that. When
/// ours lets go, the input method turns off rather than going back to the
/// layers' old area, for a field that no longer has focus.
#[test]
fn our_field_taking_over_forgets_the_layers_area() {
    let mut memory = ImeMemory::default();
    assert_eq!(ime_to_apply(&mut memory, None, Some(area(90.0))), Some(area(90.0)));
    assert_eq!(ime_to_apply(&mut memory, Some(area(5.0)), None), Some(area(5.0)));
    assert_eq!(ime_to_apply(&mut memory, None, None), Some(Ime::Off));
}
