use super::*;
use crate::app::Context;
use crate::effect::{Effects, FileAction};
use crate::routing::NoTarget;
use crate::view::View;
use loom_ui_core::Size;

/// What the probe app is asked to do.
#[derive(Debug, Clone, PartialEq)]
enum Ask {
    Open,
    Opened(Option<PathBuf>),
    Question,
    Shot,
    Shot2(bool),
    Copy,
}

/// An app that turns each message into one effect with an answer.
#[derive(Default)]
struct Probe {
    seen: Vec<Ask>,
}

impl App for Probe {
    type Message = Ask;

    fn update(&mut self, message: Ask, _cx: &Context<'_, Ask>) -> Effects<Ask> {
        self.seen.push(message.clone());
        match message {
            Ask::Open => Effects::file_dialog(
                FileRequest {
                    action: FileAction::Open,
                    title: "Open patch".into(),
                    filters: Vec::new(),
                    folder: None,
                    file_name: None,
                },
                Ask::Opened,
            ),
            Ask::Question => Effects::ask(
                Question {
                    level: crate::effect::Level::Info,
                    title: "Name".into(),
                    text: "Add your name?".into(),
                    buttons: vec!["Add".into()],
                },
                |_| Ask::Copy,
            ),
            Ask::Shot => Effects::capture(|image| Ask::Shot2(image.is_ok())),
            Ask::Copy => Effects::clipboard_write("copied"),
            Ask::Opened(_) | Ask::Shot2(_) => Effects::none(),
        }
    }

    fn view(&self, _view: &mut View) {}

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }
}

/// A harness for the probe app.
fn harness() -> Harness<Probe, NoTarget> {
    Harness::new(
        Probe::default(),
        NoTarget,
        WindowState::new(Size::new(10.0, 10.0)),
        Instant::now(),
    )
}

/// Section 2, rule 3: a file dialog with no answer panics, naming it.
#[test]
#[should_panic(expected = "the headless driver has no answer for file dialog")]
fn a_file_dialog_with_no_answer_panics_naming_it() {
    harness().send(Ask::Open);
}

/// Section 2, rule 3: a question with no answer panics, naming it.
#[test]
#[should_panic(expected = "the headless driver has no answer for question")]
fn a_question_with_no_answer_panics_naming_it() {
    harness().send(Ask::Question);
}

/// Section 2, rule 3: a capture with no answer panics, naming it.
#[test]
#[should_panic(expected = "the headless driver has no answer for window capture")]
fn a_capture_with_no_answer_panics_naming_it() {
    harness().send(Ask::Shot);
}

/// Answers given first are used in order, and the driver records what
/// it showed. The clipboard is in memory.
#[test]
fn answers_given_first_are_used_in_order() {
    let mut harness = harness();
    harness.driver_mut().answer_file(Some("a.ron".into()));
    harness.driver_mut().answer_file(None);
    harness.driver_mut().answer_question(Some(0));
    harness.driver_mut().answer_capture(Err(NoGpu));
    harness.send(Ask::Open);
    harness.send(Ask::Open);
    harness.send(Ask::Question);
    harness.send(Ask::Shot);
    assert_eq!(
        harness.app().seen,
        [
            Ask::Open,
            Ask::Opened(Some("a.ron".into())),
            Ask::Open,
            Ask::Opened(None),
            Ask::Question,
            Ask::Copy,
            Ask::Shot,
            Ask::Shot2(false),
        ]
    );
    assert_eq!(harness.driver().file_requests().len(), 2);
    assert_eq!(harness.driver().file_requests()[0].title, "Open patch");
    assert_eq!(harness.driver().questions().len(), 1);
    assert_eq!(harness.driver().clipboard(), Some("copied"));
    assert!(!harness.driver().exited());
}

/// The clock moves only when the test moves it, and a turn with nothing
/// due leaves the harness asleep.
#[test]
fn the_clock_moves_only_when_the_test_moves_it() {
    let start = Instant::now();
    let mut harness = Harness::new(
        Probe::default(),
        NoTarget,
        WindowState::new(Size::new(10.0, 10.0)),
        start,
    );
    assert_eq!(harness.now(), start);
    assert!(harness.advance(Duration::from_secs(2)).sleeps());
    assert_eq!(harness.now(), start + Duration::from_secs(2));
    assert!(Headless::default().clipboard().is_none());
}

/// An app whose deadline never moves on.
struct Stuck(Instant);

impl App for Stuck {
    type Message = ();

    fn update(&mut self, _message: (), _cx: &Context<'_, ()>) -> Effects<()> {
        Effects::none()
    }

    fn view(&self, _view: &mut View) {}

    fn window(&self) -> WindowSpec {
        WindowSpec::default()
    }

    fn next_deadline(&self) -> Option<crate::Deadline<()>> {
        Some(crate::Deadline {
            at: self.0,
            message: (),
        })
    }
}

/// A deadline the app never moves on is a mistake the harness names,
/// rather than a test that hangs.
#[test]
#[should_panic(expected = "the deadline never moves on")]
fn a_deadline_that_never_moves_on_panics() {
    let start = Instant::now();
    let mut harness = Harness::new(Stuck(start), NoTarget, WindowState::new(Size::new(10.0, 10.0)), start);
    harness.advance(Duration::from_millis(1));
}
