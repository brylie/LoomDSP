//! The runtime's turn, one test per rule of ui-layer.md, section 2, and
//! the redraw rules of section 3. Each test names its rule.

use super::*;
use crate::effect::{Choice, Effects, FocusTarget, Selection};
use crate::headless::{Harness, Headless};
use crate::routing::NoTarget;
use crate::view::Layer;
use loom_ui_core::{Key, KeyEvent, Modifiers, Point, Size, UiId};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// The test app's messages.
#[derive(Debug, Clone, PartialEq)]
enum Msg {
    /// Adds to the count, which its panel shows.
    Add(u32),
    /// Starts two pieces of work that each add.
    Chain,
    /// Changes nothing.
    Noop,
    /// A periodic check that changes nothing shown.
    Poll,
    /// Polls every this many milliseconds from now on.
    PollEvery(u64),
    /// The window's close button.
    Close,
    /// A file dropped on the window.
    Dropped(PathBuf),
    /// Work that sends three results as it goes.
    Load,
    /// One of those results.
    Loaded(u32),
    /// Asks a question.
    Ask,
    /// The question's answer.
    Answered(Choice),
    /// Reads the clipboard.
    Paste,
    /// The clipboard's text.
    Pasted(Option<String>),
    /// Focuses the search field, selecting all of it.
    FocusSearch,
    /// Scrolls the search field into view.
    ShowSearch,
    /// Records what has focus, from the context.
    ReadFocus,
    /// Asks for a message after the next frame is shown.
    Note,
    /// That message.
    Drawn,
    /// Hands a request to something that applies it when it draws.
    Touch,
    /// Starts or stops animating.
    Spin(bool),
    /// Sets the title.
    Title(&'static str),
    /// Quits.
    Quit,
}

/// An app that does what its messages say and records each one.
#[derive(Default)]
struct TestApp {
    count: u32,
    seen: Vec<Msg>,
    period: Option<Duration>,
    next_poll: Option<Instant>,
    polls: u32,
    closed: bool,
    animating: bool,
    title: &'static str,
    focus_seen: Vec<Option<UiId>>,
    times: Vec<Instant>,
}

/// The search field's ID.
fn search() -> UiId {
    UiId::panel("browser").with("search")
}

impl App for TestApp {
    type Message = Msg;

    fn update(&mut self, message: Msg, cx: &Context<'_, Msg>) -> Effects<Msg> {
        self.seen.push(message.clone());
        self.times.push(cx.now());
        match message {
            Msg::Add(n) => self.count += n,
            Msg::Chain => return Effects::work(|| Msg::Add(1)).and(Effects::work(|| Msg::Add(2))),
            Msg::Noop | Msg::Dropped(_) | Msg::Loaded(_) | Msg::Answered(_) | Msg::Pasted(_) | Msg::Drawn => {}
            Msg::Poll => {
                self.polls += 1;
                self.next_poll = self.period.map(|period| cx.now() + period);
            }
            Msg::PollEvery(ms) => {
                self.period = Some(Duration::from_millis(ms));
                self.next_poll = self.period.map(|period| cx.now() + period);
            }
            Msg::Close => self.closed = true,
            Msg::Load => {
                return Effects::job(|waker| {
                    for n in 1..=3 {
                        waker.send(Msg::Loaded(n));
                    }
                });
            }
            Msg::Ask => {
                return Effects::ask(
                    crate::effect::Question {
                        level: crate::effect::Level::Warning,
                        title: "Unsaved".into(),
                        text: "Save first?".into(),
                        buttons: vec!["Save".into(), "Cancel".into()],
                    },
                    Msg::Answered,
                );
            }
            Msg::Paste => return Effects::clipboard_read(Msg::Pasted),
            Msg::FocusSearch => return Effects::focus_and_select_all(search()),
            Msg::ShowSearch => return Effects::scroll_into_view(search()),
            Msg::ReadFocus => self.focus_seen.push(cx.focused().cloned()),
            Msg::Note => return Effects::after_draw(Msg::Drawn),
            Msg::Touch => return Effects::redraw(),
            Msg::Spin(on) => self.animating = on,
            Msg::Title(title) => self.title = title,
            Msg::Quit => return Effects::quit(),
        }
        Effects::none()
    }

    fn view(&self, view: &mut View) {
        view.panel(UiId::panel("counter"), Layer::Panels, &self.count);
        view.panel(UiId::panel("closed"), Layer::Panels, &self.closed);
    }

    fn window(&self) -> WindowSpec {
        WindowSpec {
            title: self.title.to_string(),
            min_size: None,
        }
    }

    fn next_deadline(&self) -> Option<Deadline<Msg>> {
        self.next_poll.map(|at| Deadline { at, message: Msg::Poll })
    }

    fn window_event(&self, event: &WindowEvent) -> Option<Msg> {
        match event {
            WindowEvent::CloseRequested => Some(Msg::Close),
            WindowEvent::FileDropped(path) => Some(Msg::Dropped(path.clone())),
            _ => None,
        }
    }

    fn animating(&self) -> bool {
        self.animating
    }
}

/// A target that records what reaches it, turns a key press into
/// `Msg::Add(1)`, and keeps the focus it is given.
#[derive(Default)]
struct Recorder {
    events: Vec<Event>,
    focus: Option<UiId>,
    selections: Vec<Selection>,
    scrolled: Vec<UiId>,
    /// When true, the target asks for a draw after any turn that ran an
    /// update, as the Iced bridge will in phase 2.
    draws_after_updates: bool,
}

impl Target<Msg> for Recorder {
    fn input(&mut self, event: &Event, _pointer: Option<Point>, _now: Instant, out: &mut Vec<Msg>) {
        self.events.push(event.clone());
        if matches!(event, Event::Key(KeyEvent::Pressed { .. })) {
            out.push(Msg::Add(1));
        }
    }

    fn focus(&mut self, target: &FocusTarget, select: Selection) {
        if let FocusTarget::Element(id) = target {
            self.focus = Some(id.clone());
        }
        self.selections.push(select);
    }

    fn scroll_into_view(&mut self, id: &UiId) {
        self.scrolled.push(id.clone());
    }

    fn focused(&self) -> Option<UiId> {
        self.focus.clone()
    }

    fn needs_redraw(&mut self, updates: usize) -> bool {
        self.draws_after_updates && updates > 0
    }
}

/// A harness for the test app with a recording target.
fn harness() -> Harness<TestApp, Recorder> {
    Harness::new(
        TestApp::default(),
        Recorder::default(),
        WindowState::new(Size::new(800.0, 600.0)),
        Instant::now(),
    )
}

/// A key press event, as a driver would send it.
fn key_press() -> DriverEvent {
    DriverEvent::Input {
        event: Event::Key(KeyEvent::Pressed {
            key: Key::Character('a'),
            modifiers: Modifiers::default(),
            text: Some("a".into()),
            repeat: false,
        }),
        pointer: None,
    }
}

/// Section 2, rule 5: one message gives one update, and the messages its
/// effects produce wait for the next turn.
#[test]
fn one_message_gives_one_update_and_effect_messages_wait() {
    let mut runtime = Runtime::new(
        TestApp::default(),
        Recorder::default(),
        WindowState::new(Size::new(100.0, 100.0)),
        Arc::new(|| {}),
    );
    let mut driver = Headless::new();
    let now = Instant::now();
    runtime.waker().send(Msg::Chain);

    let next = runtime.handle(DriverEvent::Woken, now, &mut driver);
    assert_eq!(runtime.counts().updates, 1, "only Chain ran in the first turn");
    assert_eq!(runtime.app().count, 0);
    assert!(next.again, "the work's two messages wait");

    let next = runtime.handle(DriverEvent::Woken, now, &mut driver);
    assert_eq!(runtime.counts().updates, 3);
    assert_eq!(runtime.app().seen, [Msg::Chain, Msg::Add(1), Msg::Add(2)]);
    assert!(!next.again);
}

/// Section 2, rule 4: work runs inline under the headless driver, and its
/// messages arrive in order on the next turn.
#[test]
fn work_runs_inline_and_its_messages_arrive_in_order() {
    let mut harness = harness();
    let before = harness.runtime().counts().turns;
    harness.send(Msg::Load);
    assert_eq!(
        harness.app().seen,
        [Msg::Load, Msg::Loaded(1), Msg::Loaded(2), Msg::Loaded(3)]
    );
    assert_eq!(
        harness.runtime().counts().turns - before,
        2,
        "one turn for Load, one for its results"
    );
}

/// Section 2, rule 7: a message from another thread is queued, wakes the
/// loop once, and runs on the next turn.
#[test]
fn a_message_from_another_thread_wakes_the_loop_and_runs_next_turn() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let mut runtime = Runtime::new(
        TestApp::default(),
        NoTarget,
        WindowState::new(Size::new(100.0, 100.0)),
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let waker = runtime.waker();
    std::thread::spawn(move || {
        waker.send(Msg::Add(5));
        waker.send(Msg::Add(6));
    })
    .join()
    .expect("the watcher ends");
    assert_eq!(wakes.load(Ordering::SeqCst), 2);

    runtime.handle(DriverEvent::Woken, Instant::now(), &mut Headless::new());
    assert_eq!(runtime.app().seen, [Msg::Add(5), Msg::Add(6)]);
}

/// Section 2, rule 8: a deadline wakes the loop and runs update, and when
/// that update changes nothing shown it costs no draw. The deadline moves
/// on from state.
#[test]
fn a_deadline_runs_update_and_costs_no_draw_when_nothing_changes() {
    let mut harness = harness();
    let start = harness.now();
    harness.send(Msg::PollEvery(250));
    assert_eq!(harness.next().deadline, Some(start + Duration::from_millis(250)));
    let draws = harness.driver().redraws();

    harness.advance(Duration::from_secs(1));
    assert_eq!(harness.app().polls, 4);
    assert_eq!(harness.driver().redraws(), draws, "polls changed nothing shown");
    assert_eq!(harness.next().deadline, Some(start + Duration::from_millis(1250)));
    let poll_times: Vec<_> = harness
        .app()
        .seen
        .iter()
        .zip(&harness.app().times)
        .filter(|(message, _)| **message == Msg::Poll)
        .map(|(_, at)| at.duration_since(start).as_millis())
        .collect();
    assert_eq!(poll_times, [250, 500, 750, 1000], "each poll ran at its own time");
}

/// Section 2, Behaviour 7: with nothing queued, no deadline, and nothing
/// animating, the loop sleeps. Animating asks for frames, and a frame
/// draws with no update.
#[test]
fn the_loop_sleeps_when_nothing_is_due_and_frames_draw_without_update() {
    let mut harness = harness();
    assert!(harness.next().sleeps());

    let next = harness.send(Msg::Spin(true));
    assert!(next.frames && !next.sleeps());
    let (updates, draws) = (harness.runtime().counts().updates, harness.driver().redraws());
    harness.step(DriverEvent::Frame);
    harness.step(DriverEvent::Frame);
    assert_eq!(harness.runtime().counts().updates, updates, "frames run no update");
    assert_eq!(harness.driver().redraws(), draws + 2);
    assert_eq!(harness.runtime().counts().frames, 2);

    assert!(harness.send(Msg::Spin(false)).sleeps());
}

/// Section 2, Behaviour 2: a close request and a dropped file become
/// messages through the app, and size, scale, and focus change the
/// runtime's window state. Losing focus also reaches the target.
#[test]
fn window_events_become_messages_or_window_state() {
    let mut harness = harness();
    harness.step(DriverEvent::Window(WindowEvent::CloseRequested));
    harness.step(DriverEvent::Window(WindowEvent::FileDropped("a.ron".into())));
    assert!(harness.app().closed);
    assert_eq!(harness.app().seen, [Msg::Close, Msg::Dropped("a.ron".into())]);

    harness.step(DriverEvent::Window(WindowEvent::Resized(Size::new(1024.0, 768.0))));
    harness.step(DriverEvent::Window(WindowEvent::ScaleChanged(2.0)));
    harness.step(DriverEvent::Window(WindowEvent::Focused(false)));
    let window = harness.runtime().window();
    assert_eq!(
        (window.size, window.scale, window.focused),
        (Size::new(1024.0, 768.0), 2.0, false)
    );
    assert_eq!(harness.runtime().target().events, [Event::WindowUnfocused]);
    assert_eq!(
        harness.app().seen.len(),
        2,
        "resize, scale, and focus send no message here"
    );
}

/// Section 2, Behaviour 2: an element event goes to the target, and the
/// message it gives runs in the same turn.
#[test]
fn an_element_event_reaches_the_target_and_its_message_runs() {
    let mut harness = harness();
    harness.step(key_press());
    assert_eq!(harness.app().count, 1);
    assert_eq!(harness.runtime().target().events.len(), 1);
}

/// Section 3, rule 6, and the redraw guarantee of plan step 2.1: a changed
/// panel draws, a message that changes nothing does not, an explicit
/// Redraw draws once, and a system redraw request draws with no update.
#[test]
fn only_a_visible_change_or_a_request_draws() {
    let mut harness = harness();
    let draws = |harness: &Harness<TestApp, Recorder>| harness.driver().redraws();
    let start = draws(&harness);
    assert_eq!(start, 1, "the first turn draws the first view");

    harness.send(Msg::Noop);
    assert_eq!(draws(&harness), start, "nothing changed");
    harness.send(Msg::Add(1));
    assert_eq!(draws(&harness), start + 1, "the counter panel changed");
    harness.send(Msg::Touch);
    assert_eq!(draws(&harness), start + 2, "a request with no model change still draws");

    let updates = harness.runtime().counts().updates;
    harness.step(DriverEvent::RedrawRequested);
    assert_eq!(draws(&harness), start + 3);
    assert_eq!(
        harness.runtime().counts().updates,
        updates,
        "a system request runs no update"
    );
}

/// Section 3, rule 2: the runtime compares each panel each turn that ran
/// an update, and counts only those that changed.
#[test]
fn panels_are_compared_each_turn_and_only_changes_count() {
    let mut harness = harness();
    let first = harness.runtime().counts();
    assert_eq!(
        (first.panels_compared, first.panels_changed),
        (2, 2),
        "the first view is all new"
    );
    harness.send(Msg::Add(1));
    harness.send(Msg::Noop);
    let counts = harness.runtime().counts();
    assert_eq!((counts.panels_compared, counts.panels_changed), (6, 3));
    assert_eq!(harness.runtime().view().map(View::len), Some(2));
}

/// Phase 2's bridge: a target that asks for a draw after any update makes
/// even a message that changes no model draw, and a turn with no update
/// still draws nothing.
#[test]
fn a_target_can_ask_for_a_draw_after_updates() {
    let mut harness = harness();
    let start = harness.driver().redraws();
    harness.send(Msg::Noop);
    assert_eq!(harness.driver().redraws(), start);
    // The bridge's answer, switched on.
    let mut runtime = Runtime::new(
        TestApp::default(),
        Recorder {
            draws_after_updates: true,
            ..Recorder::default()
        },
        WindowState::new(Size::new(10.0, 10.0)),
        Arc::new(|| {}),
    );
    let mut driver = Headless::new();
    let now = Instant::now();
    assert!(runtime.handle(DriverEvent::Woken, now, &mut driver).draw);
    runtime.waker().send(Msg::Noop);
    assert!(runtime.handle(DriverEvent::Woken, now, &mut driver).draw);
    assert!(
        !runtime.handle(DriverEvent::DeadlineReached, now, &mut driver).draw,
        "no update ran, so no draw"
    );
    assert_eq!(runtime.counts().redraws, 2);
}

/// `AfterDraw`: the message waits until the next frame has been shown, and
/// asking for it draws even when nothing else changed.
#[test]
fn after_draw_waits_for_the_frame_to_be_shown() {
    let mut harness = harness();
    let draws = harness.driver().redraws();
    harness.send(Msg::Note);
    assert_eq!(harness.driver().redraws(), draws + 1);
    assert!(!harness.app().seen.contains(&Msg::Drawn));
    harness.step(DriverEvent::Presented);
    assert_eq!(harness.app().seen.last(), Some(&Msg::Drawn));
    harness.step(DriverEvent::Presented);
    assert_eq!(
        harness
            .app()
            .seen
            .iter()
            .filter(|message| **message == Msg::Drawn)
            .count(),
        1,
        "it comes once"
    );
}

/// The context: `update` reads the focus the target holds, including
/// focus set by an earlier message's effect in the same batch, and the
/// moment from the driver.
#[test]
fn update_reads_focus_and_time_from_the_context() {
    let mut harness = harness();
    let waker = harness.runtime().waker();
    waker.send(Msg::ReadFocus);
    waker.send(Msg::FocusSearch);
    waker.send(Msg::ReadFocus);
    harness.step(DriverEvent::Woken);
    assert_eq!(harness.app().focus_seen, [None, Some(search())]);
    assert_eq!(harness.runtime().target().selections, [Selection::All]);
    assert!(harness.app().times.iter().all(|at| *at == harness.now()));
}

/// Effects with answers: a question's button and the clipboard's text
/// arrive on the next turn, and the driver records the question.
#[test]
fn answers_arrive_on_the_next_turn() {
    let mut harness = harness();
    harness.driver_mut().answer_question(Some(1));
    harness.driver_mut().set_clipboard(Some("osc".into()));
    harness.send(Msg::Ask);
    harness.send(Msg::Paste);
    assert_eq!(
        harness.app().seen,
        [
            Msg::Ask,
            Msg::Answered(Some(1)),
            Msg::Paste,
            Msg::Pasted(Some("osc".into()))
        ]
    );
    assert_eq!(harness.driver().questions()[0].title, "Unsaved");
}

/// The window spec reaches the driver on the first turn and then only
/// when it changes.
#[test]
fn the_window_spec_reaches_the_driver_only_when_it_changes() {
    let mut harness = harness();
    assert_eq!(harness.driver().window_changes(), 1);
    harness.send(Msg::Title("Patch"));
    harness.send(Msg::Title("Patch"));
    harness.send(Msg::Noop);
    assert_eq!(harness.driver().window_changes(), 2);
    assert_eq!(
        harness.driver().window_spec().map(|spec| spec.title.as_str()),
        Some("Patch")
    );
}

/// Quit ends the loop: the driver is told, later messages in the batch do
/// not run, and later events change nothing.
#[test]
fn quit_ends_the_loop() {
    let mut harness = harness();
    let waker = harness.runtime().waker();
    waker.send(Msg::Quit);
    waker.send(Msg::Add(1));
    let next = harness.step(DriverEvent::Woken);
    assert!(harness.runtime().exited() && harness.driver().exited());
    assert!(next.sleeps());
    assert_eq!(harness.app().seen, [Msg::Quit]);
    let turns = harness.runtime().counts().turns;
    harness.step(key_press());
    assert_eq!(harness.runtime().counts().turns, turns);
    assert_eq!(harness.app().count, 0);
}

/// A scroll effect reaches the target that holds the element, and the
/// target can be changed through the runtime, as a driver that draws it
/// would.
#[test]
fn a_scroll_effect_reaches_the_target() {
    let mut runtime = Runtime::new(
        TestApp::default(),
        Recorder::default(),
        WindowState::new(Size::new(10.0, 10.0)),
        Arc::new(|| {}),
    );
    runtime.waker().send(Msg::ShowSearch);
    runtime.handle(DriverEvent::Woken, Instant::now(), &mut Headless::new());
    assert_eq!(runtime.target().scrolled, [search()]);
    runtime.target_mut().scrolled.clear();
    assert!(runtime.target().scrolled.is_empty());
}

/// `Presented`: a frame shown before the turn that asks for the after-draw
/// message does not deliver it; the next one shown after that turn does.
#[test]
fn a_frame_shown_before_the_request_does_not_deliver_the_message() {
    let mut harness = harness();
    harness.step(DriverEvent::Presented);
    assert!(harness.app().seen.is_empty(), "nothing waits yet");

    let next = harness.send(Msg::Note);
    assert!(next.draw, "asking for the message draws a frame");
    assert!(!harness.app().seen.contains(&Msg::Drawn), "that frame is not shown yet");
    harness.step(DriverEvent::Presented);
    assert_eq!(harness.app().seen, [Msg::Note, Msg::Drawn]);
}

/// A system redraw request and a frame each say to draw at once, and run
/// no update; a turn that changes nothing says not to draw.
#[test]
fn next_says_when_to_draw() {
    let mut harness = harness();
    assert!(!harness.send(Msg::Noop).draw);
    assert!(harness.send(Msg::Add(1)).draw);
    assert!(harness.step(DriverEvent::RedrawRequested).draw);
    assert!(harness.step(DriverEvent::Frame).draw);
    assert!(
        !harness
            .step(DriverEvent::Window(WindowEvent::Resized(Size::new(5.0, 5.0))))
            .draw,
        "a resize draws through the system's redraw request"
    );
}

/// `deliver` runs its one message alone: what a waker queued before it
/// waits for the next turn, and so does a deadline that is due. The bridge
/// to Iced hands over the window's word on focus this way.
#[test]
fn deliver_runs_one_message_alone() {
    let start = Instant::now();
    let mut runtime = Runtime::new(
        TestApp::default(),
        NoTarget,
        WindowState::new(Size::new(10.0, 10.0)),
        Arc::new(|| {}),
    );
    let mut driver = Headless::new();
    runtime.waker().send(Msg::Add(1));
    let next = runtime.deliver(Msg::Add(10), start, &mut driver);
    assert_eq!(runtime.app().seen, [Msg::Add(10)], "only the delivered message");
    assert!(next.again, "the queued one is due next");
    runtime.handle(DriverEvent::Woken, start, &mut driver);
    assert_eq!(runtime.app().seen, [Msg::Add(10), Msg::Add(1)]);
    assert_eq!(runtime.counts().turns, 2);
}

/// `deliver` keeps the runtime's rules (section 2, rules 5 and 10): one
/// update, that message's effects right after it, the turn drawing because
/// a panel changed, and the messages its effects send left for the next
/// turn.
#[test]
fn deliver_keeps_the_rules_of_a_turn() {
    let start = Instant::now();
    let mut runtime = Runtime::new(
        TestApp::default(),
        NoTarget,
        WindowState::new(Size::new(10.0, 10.0)),
        Arc::new(|| {}),
    );
    let mut driver = Headless::new();
    runtime.handle(DriverEvent::Woken, start, &mut driver);

    // Chain's two pieces of work run inline, and each sends a message.
    let next = runtime.deliver(Msg::Chain, start, &mut driver);
    assert_eq!(runtime.app().seen, [Msg::Chain], "one update");
    assert_eq!(runtime.counts().updates, 1);
    assert!(next.again, "what its effects sent waits for the next turn");
    assert!(!next.draw, "nothing shown changed yet");

    let next = runtime.handle(DriverEvent::Woken, start, &mut driver);
    assert_eq!(runtime.app().seen, [Msg::Chain, Msg::Add(1), Msg::Add(2)]);
    assert!(next.draw, "the count's panel changed");

    let next = runtime.deliver(Msg::Add(5), start, &mut driver);
    assert!(next.draw, "a delivered change draws too");
    assert_eq!(runtime.app().count, 8);
}
