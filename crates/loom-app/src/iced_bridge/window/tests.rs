use super::*;
use crate::tests::app;
use loom_ui::{Runtime, WindowState};
use loom_ui_core::UiId;
use std::sync::Arc;

/// A runtime around the app, with no window, for the layers to be lent.
fn runtime() -> Runtime<Hosted, IcedTarget> {
    let size = loom_ui_core::Size::new(crate::WINDOW.width, crate::WINDOW.height);
    Runtime::new(
        Hosted(app(), None),
        IcedTarget::default(),
        WindowState::new(size),
        Arc::new(|| {}),
    )
}

/// The layers' report, lent `runtime`'s parts.
fn report(layers: &mut WindowLayers, runtime: &mut Runtime<Hosted, IcedTarget>) -> Report {
    let waker = runtime.waker();
    let window = runtime.window();
    let (app, target) = runtime.parts();
    layers.report(Parts {
        app,
        target,
        waker: &waker,
        window,
    })
}

/// One event's messages go in Iced's order: the widgets' first, then the
/// listeners', here the close request the app hears as its close.
#[test]
fn the_widgets_messages_come_before_the_listeners() {
    let app = app();
    let close = iced::Event::Window(iced::window::Event::CloseRequested);
    let routed = batch(
        vec![Message::ShowHelp],
        &[close],
        &[iced::event::Status::Ignored],
        app.listeners(),
    );
    assert!(
        matches!(
            routed.as_slice(),
            [Message::ShowHelp, Message::File(crate::FileAction::Close)]
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
    let mut layers = WindowLayers::new(SharedClipboard::default());
    layers.pointer = (mouse::Interaction::Pointer, false);
    layers.redraw = Redraw::NextFrame;
    let report = report(&mut layers, &mut runtime);
    assert_eq!(report.cursor, Some(Pointer::Icon(winit::window::CursorIcon::Pointer)));
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
    let mut layers = WindowLayers::new(SharedClipboard::default());
    layers.answers.push_back(FocusAnswer {
        any: true,
        widget: Some(crate::places::field_widget_id(&field)),
    });
    let waker = runtime.waker();
    let window = runtime.window();
    let (app, target) = runtime.parts();
    let message = layers.alone(Parts {
        app,
        target,
        waker: &waker,
        window,
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

/// A touch moves the cursor to the finger, in logical pixels, as a mouse
/// move does, so a tap reaches the widget under it.
#[test]
fn a_touch_moves_the_cursor() {
    let mut runtime = runtime();
    let mut layers = WindowLayers::new(SharedClipboard::default());
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
    let (app, target) = runtime.parts();
    layers.window_event(
        Parts {
            app,
            target,
            waker: &waker,
            window,
        },
        &touch,
    );
    assert_eq!(layers.cursor, mouse::Cursor::Available(iced::Point::new(100.0, 50.0)));
    assert_eq!(layers.events.len(), 1, "the touch is kept for Iced");
}
