//! Tests for the timers as deadlines: each is due a period after it
//! starts, moves on once taken, and runs only while its condition holds.

use super::*;
use crate::Args;
use crate::devices::fake::{self, Fake};
use loom_host_desktop::Tape;
use std::path::PathBuf;
use std::rc::Rc;

/// The app with its window open, so timers may run.
fn open_app() -> App {
    let mut app = crate::tests::app();
    let _ = app.send(Message::StartWatching);
    app
}

/// An example patch, playing on a fake device, with the window open.
fn playing(name: &str) -> App {
    let dir = std::env::temp_dir().join(format!("loom-timers-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let host = Rc::new(Fake {
        tape: Some(Tape::new()),
        ..Fake::new(&[fake::device("Fake A")], &[])
    });
    let args = Args {
        patch: Some(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/patches")
                .join(name),
        ),
        no_audio: false,
        no_recovery: true,
        devices: crate::devices::Options {
            preferences: Some(dir.join("preferences.ron")),
            ..crate::devices::Options::default()
        },
        ..crate::tests::app_args()
    };
    let mut app = App::boot_with(args, Rc::new(fake::Shared(host)));
    assert!(app.session.engine.stream.is_playing(), "{}", app.alerts.status());
    let _ = app.send(Message::StartWatching);
    app
}

/// A timer that starts is due one period later, keeps that time while it
/// stays wanted, and stops once it is not.
#[test]
fn a_timer_is_due_a_period_after_it_starts_and_stops_when_unwanted() {
    let start = Instant::now();
    let mut timers = Timers::default();
    timers.keep(Timer::DropsDone, true, start);
    assert_eq!(timers.due(Timer::DropsDone), Some(start + Timer::DropsDone.period()));
    timers.keep(Timer::DropsDone, true, start + Duration::from_millis(40));
    assert_eq!(
        timers.due(Timer::DropsDone),
        Some(start + Timer::DropsDone.period()),
        "a running timer keeps its time"
    );
    timers.keep(Timer::DropsDone, false, start);
    assert_eq!(timers.due(Timer::DropsDone), None);
}

/// Once its message is taken, a timer is due again one period after the
/// time it was due, however late the turn ran, as `time::every` ticked; a
/// time already past makes it due at once, so a slow frame is followed by
/// the next straight away, never by a growing delay.
#[test]
fn a_taken_timer_moves_on_by_one_period_at_a_fixed_rate() {
    let start = Instant::now();
    let period = Timer::Frame.period();
    let mut timers = Timers::default();
    timers.keep(Timer::Frame, true, start);
    let first = start + period;
    timers.taken(Timer::Frame, first + Duration::from_millis(5));
    timers.keep(Timer::Frame, true, first + Duration::from_millis(5));
    assert_eq!(
        timers.due(Timer::Frame),
        Some(first + period),
        "the turn's 5 ms do not add up"
    );
    let late = first + period * 3;
    timers.taken(Timer::Frame, late);
    assert_eq!(timers.due(Timer::Frame), Some(late), "past already, so due at once");
    timers.taken(Timer::DropsDone, late);
    assert_eq!(timers.due(Timer::DropsDone), None, "one not running stays stopped");
}

/// The deadline is the earliest timer, and its message is that timer's.
#[test]
fn next_gives_the_earliest_timer() {
    let start = Instant::now();
    let mut timers = Timers::default();
    assert_eq!(timers.next(), None);
    timers.keep(Timer::Tick, true, start);
    timers.keep(Timer::DropsDone, true, start);
    timers.keep(Timer::Autosave, true, start);
    assert_eq!(
        timers.next(),
        Some((start + Duration::from_millis(100), Timer::DropsDone))
    );
    for timer in Timer::ALL {
        assert_eq!(Timer::of(&timer.message(start)), Some(timer), "{timer:?} round trips");
    }
}

/// No timer runs before the window opens, so a test gets no message it
/// did not ask for; once it opens, a drop being collected runs its timer,
/// which is the app's deadline, and stops when the drop is done.
#[test]
fn timers_run_once_the_window_opens_and_follow_the_state() {
    let mut app = crate::tests::app();
    app.drops.push(PathBuf::from("dropped.ron"));
    let _ = app.send(Message::Tick);
    assert_eq!(app.timers, Timers::default(), "nothing before the window opens");
    assert!(app.next_deadline().is_none());

    let _ = app.send(Message::StartWatching);
    let due = app.timers.due(Timer::DropsDone).expect("a drop is being collected");
    let deadline = app.next_deadline().expect("the timer is the deadline");
    assert_eq!(deadline.at, due);
    assert!(matches!(deadline.message, Message::DropsDone));

    let _ = app.send(deadline.message);
    assert!(app.drops.is_empty());
    assert_eq!(app.timers.due(Timer::DropsDone), None, "nothing left to collect");
}

/// While measuring, a frame runs at display rate and the live frame does
/// not; its message carries the time it is due.
#[test]
fn measuring_runs_frames_at_display_rate() {
    let mut app = open_app();
    app.measuring = true;
    let before = Instant::now();
    let _ = app.send(Message::Tick);
    let after = Instant::now();
    let due = app.timers.due(Timer::Frame).expect("a frame while measuring");
    assert_eq!(Timer::Frame.period(), FRAME);
    assert!(
        before + FRAME <= due && due <= after + FRAME,
        "a frame after the turn that started it"
    );
    assert_eq!(app.timers.due(Timer::LiveFrame), None);
    let deadline = app.next_deadline().expect("the frame is the deadline");
    assert!(matches!(deadline.message, Message::Frame(at) if at == due));
}

/// The live frame runs only while modulation moves: audio plays and an
/// assignment is on. Turning the assignments off stops it.
#[test]
fn the_live_frame_runs_only_while_modulation_moves() {
    let mut app = playing("modulation.ron");
    assert!(
        app.timers.due(Timer::LiveFrame).is_some(),
        "assignments play, so live values move"
    );
    let assignments: Vec<_> = app.session.controller.graph().assignments().map(|(id, _)| id).collect();
    for id in assignments {
        app.session.controller.set_assignment_enabled(id, false).unwrap();
    }
    let _ = app.send(Message::LiveFrame);
    assert_eq!(app.timers.due(Timer::LiveFrame), None, "nothing moves");

    let mut idle = open_app();
    let _ = idle.send(Message::Tick);
    assert_eq!(idle.timers.due(Timer::LiveFrame), None, "no audio, nothing moves");
}

/// A slot swap with no stream wants no tick: the engine is idle, so its
/// fade cannot end, and an idle app stays idle (#90). Its fade still
/// finishes on a tick once the engine has run, and the leaving part goes.
#[test]
fn a_swap_fade_with_no_stream_does_not_tick() {
    let mut app = crate::tests::app_with_text(
        r#"Patch(
    version: 1,
    modules: [
        PatchModule(id: 1, kind: "loom.osc.sine@1"),
        PatchModule(id: 2, kind: "loom.filter.svf@1"),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
        PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.filter@1", part: Some(2)))),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (9, "in")),
        PatchCable(from: (9, "out"), to: (3, "in")),
    ],
)"#,
    );
    let _ = app.send(Message::StartWatching);
    assert!(!app.session.engine.stream.is_playing());
    let modules = app.session.controller.graph().nodes().count();
    let _ = app.send(Message::SwapSlot {
        slot: 9,
        to: crate::slots::SwapTarget::Empty,
    });
    assert!(app.session.hierarchy.is_fading());
    assert_eq!(app.timers.due(Timer::Tick), None, "no stream, no tick");
    app.process(600);
    let _ = app.send(Timer::Tick.message(Instant::now()));
    assert!(!app.session.hierarchy.is_fading(), "the fade finished on the tick");
    assert!(
        app.session.controller.graph().nodes().count() < modules,
        "the part is gone"
    );
    assert_eq!(app.timers.due(Timer::Tick), None);
}

/// While audio plays, a slot swap ticks until its fade is done, so the
/// leaving part goes without a recording running (sub-patches.md, How a
/// swap crossfades).
#[test]
fn a_swap_fade_ticks_while_audio_plays() {
    let mut app = playing("effect-chain.ron");
    assert_eq!(app.timers.due(Timer::Tick), None, "an idle patch does not tick");
    let slot = app
        .session
        .hierarchy
        .id_at(&loom_patch::ModulePath::top(5), 11)
        .expect("Effect 2")
        .0;
    let _ = app.send(Message::SwapSlot {
        slot,
        to: crate::slots::SwapTarget::Empty,
    });
    assert!(app.session.hierarchy.is_fading());
    assert!(app.timers.due(Timer::Tick).is_some(), "the fade wants the tick");
}
