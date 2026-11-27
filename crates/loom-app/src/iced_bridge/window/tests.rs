use super::*;
use crate::tests::app;
use loom_ui::{Runtime, WindowState};
use loom_ui_core::{Key, Modifiers, UiId};
use std::sync::Arc;

/// A runtime around the app, with no window, for the layers to be lent.
fn runtime() -> Runtime<Hosted, IcedTarget> {
    let size = loom_ui_core::Size::new(crate::WINDOW.width, crate::WINDOW.height);
    Runtime::new(
        Hosted(app(), None, crate::iced_bridge::Extra::default()),
        IcedTarget::default(),
        WindowState::new(size),
        Arc::new(|| {}),
    )
}

/// A report after a turn with no events queues nothing and wakes nothing.
/// The shell asks for a report after every turn, so a report that woke the
/// loop would start the next turn itself and spin with no frame drawn.
#[test]
fn a_report_with_no_events_wakes_nothing() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let size = loom_ui_core::Size::new(crate::WINDOW.width, crate::WINDOW.height);
    let mut runtime = Runtime::new(
        Hosted(app(), None, crate::iced_bridge::Extra::default()),
        IcedTarget::default(),
        WindowState::new(size),
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let mut layers = WindowLayers::new();
    for _ in 0..2 {
        let report = report(&mut layers, &mut runtime);
        assert!(!report.alone);
        assert_eq!(report.redraw, Redraw::No);
    }
    assert_eq!(wakes.load(Ordering::SeqCst), 0, "no report woke the loop");
}

/// The layers' report, lent `runtime`'s parts.
fn report(layers: &mut WindowLayers, runtime: &mut Runtime<Hosted, IcedTarget>) -> Report {
    let waker = runtime.waker();
    let window = runtime.window();
    let (app, target, panels) = runtime.parts();
    layers.report(Parts {
        app,
        target,
        waker: &waker,
        window,
        panels,
    })
}

/// One event's messages go in Iced's order: the widgets' first, then the
/// listeners', here what an Escape no widget used becomes.
#[test]
fn the_widgets_messages_come_before_the_listeners() {
    let app = app();
    let escape = crate::ui_test::key_press(Key::Escape, Modifiers::NONE, None, false);
    let routed = batch(
        vec![Message::ShowHelp],
        &[escape],
        &[iced::event::Status::Ignored],
        app.listeners(),
    );
    assert!(
        matches!(
            routed.as_slice(),
            [
                Message::ShowHelp,
                Message::Key(_),
                Message::CloseHelp,
                Message::TextFocusCheck
            ]
        ),
        "{routed:?}"
    );
}

/// A task the window cannot finish is logged once and dropped, and the
/// report still says what the window should do: here the pointer and the
/// frame an earlier update asked for.
#[test]
fn a_stuck_task_leaves_the_report_whole() {
    let mut runtime = runtime();
    runtime
        .target_mut()
        .tasks
        .push(Task::future(std::future::pending::<Bridged>()));
    let mut layers = WindowLayers::new();
    layers.pointer = (mouse::Interaction::Pointer, false);
    layers.redraw = Redraw::NextFrame;
    let report = report(&mut layers, &mut runtime);
    assert_eq!(report.cursor, Some(Pointer::Icon(loom_ui_core::Cursor::Pointer)));
    assert_eq!(report.redraw, Redraw::NextFrame);
    assert!(!report.alone);
    assert_eq!(layers.logged, [true, false, false], "logged as waiting, once");
    // What was reported is not reported again.
    let again = super::tests::report(&mut layers, &mut runtime);
    assert_eq!(again.cursor, None);
    assert_eq!(again.redraw, Redraw::No);
}

/// A focus answer runs alone: the target takes it, then the app hears it
/// as `FocusMoved`, and the report says no more wait.
#[test]
fn a_focus_answer_moves_the_target_then_tells_the_app() {
    let mut runtime = runtime();
    let field = UiId::panel("inspector").with("field");
    let mut layers = WindowLayers::new();
    layers.answers.push_back(FocusAnswer {
        any: true,
        widget: Some(crate::places::field_widget_id(&field)),
    });
    let waker = runtime.waker();
    let window = runtime.window();
    let (app, target, panels) = runtime.parts();
    let message = layers.alone(Parts {
        app,
        target,
        waker: &waker,
        window,
        panels,
    });
    assert!(
        matches!(&message, Some(Message::FocusMoved(Some(part))) if *part == field),
        "{message:?}"
    );
    assert_eq!(runtime.target().focused, Some(field));
    assert!(!report(&mut layers, &mut runtime).alone);
}

/// Two frame requests give the sooner, and the redraw an update asks for
/// maps onto the shell's.
#[test]
fn the_sooner_frame_wins() {
    let now = Instant::now();
    let later = now + std::time::Duration::from_millis(5);
    assert_eq!(sooner(Redraw::No, Redraw::At(later)), Redraw::At(later));
    assert_eq!(sooner(Redraw::At(now), Redraw::At(later)), Redraw::At(now));
    assert_eq!(sooner(Redraw::At(now), Redraw::NextFrame), Redraw::NextFrame);
    assert_eq!(sooner(Redraw::No, Redraw::No), Redraw::No);
    assert_eq!(redraw(iced::window::RedrawRequest::Wait), Redraw::No);
    assert_eq!(pointer(mouse::Interaction::Hidden), Pointer::Hidden);
}

/// Every shape the canvas asks for comes back as itself through Iced's
/// pointer, so the canvas's cursor reaches the window unchanged. Iced's
/// plain and unused shapes are the arrow.
#[test]
fn the_canvas_cursor_comes_through_iced_unchanged() {
    use loom_canvas::iced_convert::ToIced;
    use loom_ui_core::Cursor;
    for cursor in Cursor::ALL {
        assert_eq!(pointer(cursor.to_iced()), Pointer::Icon(cursor), "{cursor:?}");
    }
    assert_eq!(pointer(mouse::Interaction::None), Pointer::Icon(Cursor::Default));
    assert_eq!(pointer(mouse::Interaction::Wait), Pointer::Icon(Cursor::Default));
}

/// A touch moves the cursor to the finger, in logical pixels, as a mouse
/// move does, so a tap reaches the widget under it.
#[test]
fn a_touch_moves_the_cursor() {
    let mut runtime = runtime();
    let mut layers = WindowLayers::new();
    layers.scale = 2.0;
    let touch = WinitEvent::Touch(winit::event::Touch {
        device_id: winit::event::DeviceId::dummy(),
        phase: winit::event::TouchPhase::Started,
        location: winit::dpi::PhysicalPosition::new(200.0, 100.0),
        force: None,
        id: 0,
    });
    let waker = runtime.waker();
    let window = runtime.window();
    let (app, target, panels) = runtime.parts();
    layers.window_event(
        Parts {
            app,
            target,
            waker: &waker,
            window,
            panels,
        },
        &touch,
    );
    assert_eq!(layers.cursor, mouse::Cursor::Available(iced::Point::new(100.0, 50.0)));
    assert_eq!(layers.events.len(), 1, "the touch is kept for Iced");
}

/// A clipboard in memory, for the Iced adapter's test: no display needed.
/// Clones are handles onto the same text, as two handles onto the system's
/// clipboard are.
#[derive(Clone, Default)]
struct Memory(std::sync::Arc<std::sync::Mutex<Option<String>>>);

impl loom_ui::clipboard::Backend for Memory {
    fn get(&mut self) -> loom_ui::ClipboardText {
        loom_ui::ClipboardText::from_text(self.0.lock().unwrap().clone())
    }

    fn set(&mut self, text: String) {
        *self.0.lock().unwrap() = Some(text);
    }
}

/// Iced's widgets reach the shell's clipboard through the adapter: what
/// they write, a read finds, on the same thread the app's effects use. The
/// primary selection is out of scope, so it reads nothing and keeps nothing.
#[test]
fn iceds_widgets_use_the_shells_clipboard() {
    use iced::advanced::Clipboard as _;
    use iced::advanced::clipboard::Kind;
    let mut clipboard = super::IcedClipboard(loom_ui::clipboard::Clipboard::new({
        let memory = Memory::default();
        move || memory.clone()
    }));
    clipboard.write(Kind::Standard, "osc".to_owned());
    assert_eq!(clipboard.read(Kind::Standard), Some("osc".to_owned()));
    clipboard.write(Kind::Primary, "ignored".to_owned());
    assert_eq!(clipboard.read(Kind::Primary), None);
    assert_eq!(clipboard.read(Kind::Standard), Some("osc".to_owned()));
}

/// A scripted key goes to the window as a real one: each report hands the
/// shell one key, as the runtime's input, and keeps Iced's copy for the
/// next report, as `window_event` keeps a real key's; one key a report, so
/// each has a turn of its own. With none left, the report hands none.
#[test]
fn a_scripted_key_is_handed_to_the_shell_one_a_report() {
    use iced::keyboard::{Event as Keyboard, Key, Location, Modifiers, key::Named};
    let mut runtime = runtime();
    let mut layers = WindowLayers::new();
    let down = |pressed: bool| {
        let key = Key::Named(Named::ArrowDown);
        iced::Event::Keyboard(if pressed {
            Keyboard::KeyPressed {
                key: key.clone(),
                modified_key: key.clone(),
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                location: Location::Standard,
                modifiers: Modifiers::empty(),
                text: None,
                repeat: false,
            }
        } else {
            Keyboard::KeyReleased {
                key: key.clone(),
                modified_key: key,
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                location: Location::Standard,
                modifiers: Modifiers::empty(),
            }
        })
    };
    runtime.app().0.pointer.script_key(down(true));
    runtime.app().0.pointer.script_key(down(false));
    let first = report(&mut layers, &mut runtime);
    assert!(
        matches!(
            first.input.as_slice(),
            [loom_ui_core::Event::Key(loom_ui_core::KeyEvent::Pressed {
                key: loom_ui_core::Key::Down,
                ..
            })]
        ),
        "{:?}",
        first.input
    );
    assert_eq!(layers.events.len(), 1, "Iced keeps its copy");
    let second = report(&mut layers, &mut runtime);
    assert!(
        matches!(
            second.input.as_slice(),
            [loom_ui_core::Event::Key(loom_ui_core::KeyEvent::Released { .. })]
        ),
        "{:?}",
        second.input
    );
    assert!(report(&mut layers, &mut runtime).input.is_empty());
}

/// What one step of the shell's `after` loop did, as
/// [`after`] copies it.
#[derive(Debug, PartialEq)]
enum Step {
    /// The layers reported: whether they handed the shell a key, and
    /// whether a message waits to run alone.
    Report {
        /// The report handed the shell a scripted key.
        key: bool,
        /// A message waits to run alone.
        alone: bool,
    },
    /// The queued messages ran, then the layers' message, alone.
    Alone,
    /// The runtime's turn for a key the layers handed it.
    Turn,
}

/// The shell's `after` loop (`crates/loom-ui/src/shell.rs`, `Shell::after`,
/// line 533) over the layers and `runtime`, in its order: while a report
/// says a message waits to run alone, run it and report again; then one
/// turn for each input the reports handed over, reporting after each. The
/// shell itself needs a window and its event loop, so this copies the order
/// it keeps. If that order changes, change this too, or the test checks an
/// order the window no longer has. It goes with this bridge in phase 4,
/// when Iced does.
fn after(layers: &mut WindowLayers, runtime: &mut Runtime<Hosted, IcedTarget>) -> Vec<Step> {
    let mut driver = loom_ui::headless::Headless::new();
    let mut queued = std::collections::VecDeque::new();
    let mut steps = Vec::new();
    loop {
        loop {
            let report = report(layers, runtime);
            steps.push(Step::Report {
                key: !report.input.is_empty(),
                alone: report.alone,
            });
            queued.extend(report.input);
            if !report.alone {
                break;
            }
            steps.push(Step::Alone);
            runtime.handle(loom_ui::DriverEvent::Woken, Instant::now(), &mut driver);
            let waker = runtime.waker();
            let window = runtime.window();
            let (app, target, panels) = runtime.parts();
            let message = layers.alone(Parts {
                app,
                target,
                waker: &waker,
                window,
                panels,
            });
            if let Some(message) = message {
                runtime.deliver(message, Instant::now(), &mut driver);
            }
        }
        let Some(event) = queued.pop_front() else {
            return steps;
        };
        steps.push(Step::Turn);
        runtime.handle(
            loom_ui::DriverEvent::Input { event, pointer: None },
            Instant::now(),
            &mut driver,
        );
    }
}

/// A scripted key waits while a focus answer runs alone, so Iced never
/// hears it before the runtime's turn for it. The report that hands the
/// key over keeps Iced's copy for the next report, and the shell reports
/// again after a message that runs alone: had that report come between,
/// Iced would hear the key with nothing yet saying the runtime took it (a
/// Tab from a field a script's click focused selected a tile; UX review of
/// #900).
#[test]
fn a_scripted_key_waits_for_the_answers_that_run_alone() {
    use iced::keyboard::{Event as Keyboard, Key, Location, Modifiers, key::Named};
    let mut runtime = runtime();
    let mut layers = WindowLayers::new();
    layers.answers.push_back(FocusAnswer {
        any: false,
        widget: None,
    });
    let tab = Key::Named(Named::Tab);
    runtime
        .app()
        .0
        .pointer
        .script_key(iced::Event::Keyboard(Keyboard::KeyPressed {
            key: tab.clone(),
            modified_key: tab,
            physical_key: iced::keyboard::key::Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            text: None,
            repeat: false,
        }));
    let steps = after(&mut layers, &mut runtime);
    for pair in steps.windows(2) {
        if matches!(pair[0], Step::Report { key: true, .. }) {
            assert_eq!(pair[1], Step::Turn, "the key's turn comes next: {steps:?}");
        }
    }
    assert_eq!(
        steps,
        [
            Step::Report {
                key: false,
                alone: true
            },
            Step::Alone,
            Step::Report {
                key: true,
                alone: false
            },
            Step::Turn,
            Step::Report {
                key: false,
                alone: false
            },
        ]
    );
}
