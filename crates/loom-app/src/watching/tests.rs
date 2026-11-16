//! Tests for which watchers the app runs.

use crate::Message;
use crate::tests::app;
use crate::watcher::Watcher;
use loom_ui::Waker;
use std::sync::{Arc, mpsc};

/// No watcher runs before the window opens, so a test's app never has a
/// thread sending it messages it did not ask for. Once started, the
/// engine's watchers run, and a new engine starts with none until the
/// next update starts its own.
#[test]
fn watchers_start_with_the_window_and_follow_the_engine() {
    let (sender, _queue) = mpsc::channel();
    let waker = Waker::new(sender, Arc::new(|| {}));
    let mut app = app();
    app.sync_watchers(&waker);
    assert!(
        app.session.engine.watchers.none_running(),
        "not before the window opens"
    );
    let _ = app.send(Message::StartWatching);
    app.sync_watchers(&waker);
    assert!(
        !app.session.engine.watchers.none_running(),
        "the telemetry monitor runs once watching"
    );
    assert!(app.replace(crate::Source::Empty));
    assert!(
        app.session.engine.watchers.none_running(),
        "the old engine's went with it"
    );
    app.sync_watchers(&waker);
    assert!(!app.session.engine.watchers.none_running(), "the new engine's start");
}

/// A message a watcher sent is taken first in `apply`, before anything
/// could return early (every watcher's message also runs while a question
/// is open), so the watcher gets room for its next one.
#[test]
fn applying_a_watched_message_makes_room_for_the_next() {
    let (sender, queue) = mpsc::channel();
    let waker = Waker::new(sender, Arc::new(|| {}));
    let mut app = app();
    // A watcher that sends two readings, the second only once the first
    // has been taken.
    let (sent, results) = mpsc::channel();
    app.session.engine.watchers.lights = Some(Watcher::start(
        &app.watchers.ledgers.lights,
        1,
        1,
        waker,
        Message::Lights,
        move |outlet| {
            let _ = sent.send(outlet.try_send(Vec::new()));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while std::time::Instant::now() < deadline {
                if outlet.try_send(Vec::new()).is_ok() {
                    let _ = sent.send(Ok(()));
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let _ = sent.send(Err(crate::watcher::SendError::Full));
        },
    ));
    assert_eq!(results.recv().unwrap(), Ok(()));
    let first = queue.recv().unwrap();
    assert!(
        results.recv_timeout(std::time::Duration::from_millis(100)).is_err(),
        "the second waits while the first is in flight"
    );
    let _ = app.send(first);
    assert_eq!(results.recv().unwrap(), Ok(()), "taken, so the next one went");
}

/// A message the old engine's watcher sent, taken after the engine was
/// replaced, frees nothing on the new engine's watcher: its count and its
/// room stay as they were.
#[test]
fn a_stale_message_leaves_the_new_watcher_alone() {
    let (sender, queue) = mpsc::channel();
    let waker = Waker::new(sender, Arc::new(|| {}));
    let mut app = app();
    let lights = |sent: Vec<((u32, usize), f32)>| {
        move |outlet: crate::watcher::Outlet<Vec<((u32, usize), f32)>>| {
            let _ = outlet.try_send(sent);
        }
    };
    app.session.engine.watchers.lights = Some(Watcher::start(
        &app.watchers.ledgers.lights,
        1,
        1,
        waker.clone(),
        Message::Lights,
        lights(vec![((1, 0), 0.5)]),
    ));
    let stale = queue
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("the old watcher sent");
    // New, as a player opens another patch: the old engine's watcher goes.
    assert!(app.replace(crate::Source::Empty));
    let new = Watcher::start(
        &app.watchers.ledgers.lights,
        2,
        1,
        waker,
        Message::Lights,
        lights(vec![((2, 0), 0.5)]),
    );
    let fresh = queue
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("the new watcher sent");
    assert_eq!(new.in_flight(), 1);
    app.watched_message_taken(&stale);
    assert_eq!(
        new.in_flight(),
        1,
        "the stale message freed the old sender, not the new one"
    );
    app.watched_message_taken(&fresh);
    assert_eq!(new.in_flight(), 0, "its own message frees it");
}

/// With something for each to read, every watcher starts: the audio stats
/// and the input on a fake device, the lights on screen, the monitor, MIDI
/// learn, the MIDI port list, the bench report, and the control channel.
/// Each kind of message frees its own sender.
#[test]
fn every_watcher_starts_from_what_it_reads() {
    use crate::devices::fake::{self, Fake, Shared};
    let dir = crate::recovery::tests::TempDir::new("watching-all");
    let script = dir.0.join("empty.jsonl");
    std::fs::write(&script, "").unwrap();
    let fake = std::rc::Rc::new(Fake {
        midi_lister: Some(|| Ok(Vec::new())),
        ..Fake::new(&[fake::device("Fake A")], &["Pad"])
    });
    *fake.audio_inputs.borrow_mut() = crate::devices::Inputs {
        default: Some("Mic".to_string()),
        devices: vec!["Mic".to_string()],
    };
    let args = crate::Args {
        no_audio: false,
        no_recovery: true,
        ..crate::tests::app_args()
    };
    let mut app = crate::App::boot_with(args, std::rc::Rc::new(Shared(fake.clone())));
    // An audio input in the patch opens the input; learn mode reads MIDI.
    let audio_in = loom_modules::registry().resolve("loom.io.audio-in").unwrap();
    app.add_module(audio_in, loom_ui_core::Point::new(100.0, 100.0));
    app.sync_audio_input();
    app.session.learn.start();
    app.reporter = crate::bench::Options {
        measure_idle: true,
        ..crate::bench::Options::default()
    }
    .reporter();
    app.control = vec![crate::control::Listener::Script(script)];

    let (sender, _queue) = mpsc::channel();
    let waker = Waker::new(sender, Arc::new(|| {}));
    app.watching = true;
    app.sync_watchers(&waker);
    let engine = &app.session.engine.watchers;
    assert!(
        engine.audio.is_some(),
        "the fake device plays, so its stats are watched"
    );
    assert!(engine.input.is_some(), "the input is open");
    assert!(engine.monitor.is_some());
    assert!(engine.learn.is_some(), "learn mode waits for MIDI");
    assert!(app.watchers.midi_ports.is_some(), "the host lists MIDI from any thread");
    assert!(app.watchers.bench.is_some());
    assert_eq!(app.watchers.control.len(), 1);
    // A second sync with nothing changed restarts none.
    let keys = |app: &crate::App| {
        let engine = &app.session.engine.watchers;
        [&engine.audio, &engine.input, &engine.monitor, &engine.learn].map(|watcher| watcher.as_ref().map(Watcher::key))
    };
    let before = keys(&app);
    app.sync_watchers(&waker);
    assert_eq!(keys(&app), before);
    // Every kind of watched message goes to its own ledger, and a message
    // no watcher sent frees nothing.
    for message in [
        Message::Audio(crate::audio_watch::Event::Stalled {
            callbacks: 0,
            advanced: std::time::Instant::now(),
        }),
        Message::Midi,
        Message::MidiPoll,
        Message::BenchDone,
        Message::Lights(Vec::new()),
    ] {
        app.watched_message_taken(&message);
    }
}

/// How long a test waits for a watcher thread: far longer than it needs,
/// so a busy machine does not fail it.
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

/// A waker into a plain queue, for a test to read what the threads send.
fn queue_waker() -> (Waker<Message>, mpsc::Receiver<Message>) {
    let (sender, queue) = mpsc::channel();
    (Waker::new(sender, Arc::new(|| {})), queue)
}

/// Adds a Constant at 1 feeding an envelope's gate, so the envelope's level
/// light reads well above 0.5 once the engine runs. Gives the light.
fn held_envelope(app: &mut crate::App) -> crate::lights::Light {
    let gate = app.session.controller.add_module("loom.math.constant").unwrap();
    app.session.controller.set_param_named(gate, "value", 1.0).unwrap();
    let env = app.session.controller.add_module("loom.env.envelope").unwrap();
    app.session.controller.connect_named(gate, "out", env, "gate").unwrap();
    app.commit();
    level_light(app, env.0)
}

/// The envelope's level light.
fn level_light(app: &crate::App, env: u32) -> crate::lights::Light {
    let d = app
        .session
        .controller
        .graph()
        .node(loom_core::ModuleId(env))
        .unwrap()
        .descriptor;
    (env, d.panel.widgets.iter().position(|w| w.binds("level")).unwrap())
}

/// Takes and applies what the threads send, as the app does, until `light`
/// reads above 0.5. Panics after [`DEADLINE`].
fn wait_until_lit(app: &mut crate::App, queue: &mpsc::Receiver<Message>, light: crate::lights::Light) {
    let end = std::time::Instant::now() + DEADLINE;
    while app.session.lights.get(&light).is_none_or(|&value| value <= 0.5) {
        let left = end.saturating_duration_since(std::time::Instant::now());
        let message = queue.recv_timeout(left).expect("the light thread sends the light lit");
        app.watched_message_taken(&message);
        if matches!(message, Message::Lights(_)) {
            app.apply(message);
        }
    }
}

/// Takes every message waiting now, so each sender has room again.
fn take_waiting(app: &crate::App, queue: &mpsc::Receiver<Message>) {
    for message in queue.try_iter() {
        app.watched_message_taken(&message);
    }
}

/// Polls `done` until it holds, or panics after [`DEADLINE`].
fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let end = std::time::Instant::now() + DEADLINE;
    while !done() {
        assert!(std::time::Instant::now() < end, "{what}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// An app with a Clock Divide and an envelope, each with lights, watching.
fn two_light_modules() -> (crate::App, u32, u32) {
    let mut app = app();
    let divide = app.session.controller.add_module("loom.logic.divide").unwrap().0;
    let env = app.session.controller.add_module("loom.env.envelope").unwrap().0;
    app.commit();
    app.watching = true;
    (app, divide, env)
}

/// A zoom that changes which lights are on screen keeps the light thread:
/// its handle's key covers only the engine, so `keep` keeps it. Hiding every
/// light stops it, and showing one starts it again (panels.md, Redraw, At
/// most one thread per engine).
#[test]
fn a_zoom_keeps_the_light_thread() {
    let (waker, _queue) = queue_waker();
    let (mut app, divide, env) = two_light_modules();
    app.set_visible_displays(vec![divide]);
    app.sync_watchers(&waker);
    let key = app.session.engine.watchers.lights.as_ref().map(Watcher::key);
    assert!(key.is_some(), "a light on screen is watched");
    app.set_visible_displays(vec![divide, env]);
    app.sync_watchers(&waker);
    assert_eq!(
        app.session.engine.watchers.lights.as_ref().map(Watcher::key),
        key,
        "the same thread"
    );
    app.set_visible_displays(Vec::new());
    app.sync_watchers(&waker);
    assert!(app.session.engine.watchers.lights.is_none(), "no light, no thread");
    app.set_visible_displays(vec![env]);
    app.sync_watchers(&waker);
    assert!(app.session.engine.watchers.lights.is_some(), "a light again starts one");
}

/// A light brought into view is read by the thread already running, at
/// once: the same handle sends a reading for it, and no thread starts.
#[test]
fn a_light_brought_into_view_is_read_by_the_same_thread() {
    let (waker, queue) = queue_waker();
    let (mut app, divide, env) = two_light_modules();
    app.set_visible_displays(vec![env]);
    app.sync_watchers(&waker);
    let handles = app.session.engine.watchers.light_list().unwrap().handles();
    let first = loop {
        match queue.recv_timeout(DEADLINE).expect("the first reading") {
            Message::Lights(readings) => break readings,
            other => app.watched_message_taken(&other),
        }
    };
    assert!(first.iter().all(|(light, _)| light.0 == env));
    app.watched_message_taken(&Message::Lights(first));
    take_waiting(&app, &queue);
    app.set_visible_displays(vec![env, divide]);
    app.sync_watchers(&waker);
    let next = loop {
        match queue.recv_timeout(DEADLINE).expect("a reading with the new light") {
            Message::Lights(readings) if readings.iter().any(|(light, _)| light.0 == divide) => break readings,
            other => app.watched_message_taken(&other),
        }
    };
    let watcher = app.session.engine.watchers.lights.as_ref().unwrap();
    assert_eq!(watcher.in_flight(), 1, "the running handle sent it");
    assert_eq!(
        app.session.engine.watchers.light_list().unwrap().handles(),
        handles,
        "no thread started"
    );
    app.watched_message_taken(&Message::Lights(next));
}

/// Hiding every light and showing one again ends the old thread and starts
/// a new one that sends a reading. The old thread let go of the shared
/// list, so its count falls back to the new thread's.
#[test]
fn hiding_and_showing_the_lights_ends_the_old_thread() {
    let (waker, queue) = queue_waker();
    let (mut app, _divide, env) = two_light_modules();
    app.sync_watchers(&waker);
    let before = app.session.engine.watchers.light_list().unwrap().handles();
    app.set_visible_displays(vec![env]);
    app.sync_watchers(&waker);
    let running = app.session.engine.watchers.light_list().unwrap().handles();
    assert!(running > before, "the thread holds the list");
    eventually("the first thread sends", || {
        app.session.engine.watchers.lights.as_ref().unwrap().in_flight() == 1
    });
    take_waiting(&app, &queue);
    app.set_visible_displays(Vec::new());
    app.sync_watchers(&waker);
    app.set_visible_displays(vec![env]);
    app.sync_watchers(&waker);
    let list = app.session.engine.watchers.light_list().unwrap().clone();
    // One more for this test's own clone.
    eventually("the old thread ends", || list.handles() == running + 1);
    eventually("the new thread sends", || {
        app.session.engine.watchers.lights.as_ref().unwrap().in_flight() == 1
    });
}

/// After New, Open, and a device change, the new engine's light thread
/// sends the light's value, read from the new engine's table.
#[test]
fn lights_update_after_new_open_and_a_device_change() {
    let (waker, queue) = queue_waker();
    let mut app = app();
    app.watching = true;

    // New.
    assert!(app.replace(crate::Source::Empty));
    let light = held_envelope(&mut app);
    app.set_visible_displays(vec![light.0]);
    app.process(4_800);
    app.sync_watchers(&waker);
    wait_until_lit(&mut app, &queue, light);

    // Open.
    take_waiting(&app, &queue);
    let dir = crate::recovery::tests::TempDir::new("watching-open");
    let patch = dir.0.join("held.ron");
    std::fs::write(
        &patch,
        r#"Patch(version: 1, modules: [
            PatchModule(id: 1, kind: "loom.math.constant@1", params: {"value": 1.0}),
            PatchModule(id: 2, kind: "loom.env.envelope@1"),
        ], cables: [PatchCable(from: (1, "out"), to: (2, "gate"))])"#,
    )
    .unwrap();
    assert!(app.replace(crate::Source::File(&patch)));
    let light = level_light(&app, 2);
    assert!(!app.session.lights.contains_key(&light), "a new session");
    app.set_visible_displays(vec![light.0]);
    app.process(4_800);
    app.sync_watchers(&waker);
    wait_until_lit(&mut app, &queue, light);

    // A device change, on fake devices that play from a timer.
    use crate::devices::fake::{self, Fake, Shared};
    use crate::settings::{Change, Message as Prefs};
    let fake = std::rc::Rc::new(Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &[]));
    let args = crate::Args {
        no_audio: false,
        no_recovery: true,
        ..crate::tests::app_args()
    };
    let mut app = crate::App::boot_with(args, std::rc::Rc::new(Shared(fake)));
    let (waker, queue) = queue_waker();
    app.watching = true;
    let light = held_envelope(&mut app);
    app.set_visible_displays(vec![light.0]);
    app.sync_watchers(&waker);
    wait_until_lit(&mut app, &queue, light);
    take_waiting(&app, &queue);
    let old = app.session.engine.watchers.light_list().unwrap().clone();
    let generation = app.generation;
    app.preferences(Prefs::Open);
    app.preferences(Prefs::Audio(Change::Device(crate::preferences::DeviceChoice::Named(
        "Fake B".into(),
    ))));
    assert_ne!(app.generation, generation, "a new engine");
    assert!(!app.session.lights.contains_key(&light), "its values start again");
    app.sync_watchers(&waker);
    // Only this test's clone is left once the old thread ends, so no
    // message of the old engine's can follow.
    eventually("the old engine's thread ends", || old.handles() == 1);
    wait_until_lit(&mut app, &queue, light);
}

/// A scope's drawn size in the watch list follows a zoom, a change of the
/// window's scale, and a tile resize: its widget's bounds, times the zoom,
/// times the scale. Each republishes the list in place, so the monitor
/// thread keeps running (panels.md, Drawing the scope trace, The drawn
/// size).
#[test]
fn a_scope_drawn_size_follows_the_zoom_the_scale_and_a_resize() {
    use loom_canvas::{Camera, CanvasEvent};
    let (waker, _queue) = queue_waker();
    let mut app = app();
    let scope = app.session.controller.add_module("loom.display.scope").unwrap().0;
    app.commit();
    app.watching = true;
    app.sync_watchers(&waker);
    let key = app.session.engine.watchers.monitor.as_ref().map(Watcher::key);
    assert!(key.is_some(), "the monitor runs");
    let widget = crate::scene::scope_outputs(&app.session.controller, &[scope])[0]
        .widget
        .1;
    let drawn = |app: &crate::App| {
        let scopes = app.session.engine.monitor.watched().scopes;
        assert_eq!(scopes.len(), 1);
        scopes.values().next().unwrap().drawn
    };
    let expected = |app: &crate::App, times: f32| {
        let bounds = app.session.scene.module(scope).unwrap().widget_bounds(widget).unwrap();
        Some(crate::monitor::DrawnSize::new(
            bounds.width * times,
            bounds.height * times,
        ))
    };
    let _ = app.send(Message::Canvas(CanvasEvent::VisibleDisplays(vec![scope])));
    assert_eq!(drawn(&app), None, "no camera yet, so no size");

    let camera = Camera {
        zoom: 2.0,
        ..Camera::default()
    };
    let _ = app.send(Message::Canvas(CanvasEvent::Camera(camera)));
    assert_eq!(drawn(&app), expected(&app, 2.0), "the zoom");

    let _ = app.send_scaled(Message::Tick, 2.0);
    assert_eq!(drawn(&app), expected(&app, 4.0), "the window's scale");

    let before = drawn(&app).unwrap();
    let tile = app.session.scene.module(scope).unwrap();
    let size = (
        (tile.width() / loom_canvas::GRID) as u8 + 6,
        (tile.height() / loom_canvas::GRID) as u8 + 4,
    );
    let _ = app.send_scaled(Message::Canvas(CanvasEvent::Resize { module: scope, size }), 2.0);
    let after = drawn(&app).unwrap();
    assert_eq!(Some(after), expected(&app, 4.0), "the tile's new size");
    assert!(after.width > before.width && after.height > before.height);

    app.sync_watchers(&waker);
    assert_eq!(
        app.session.engine.watchers.monitor.as_ref().map(Watcher::key),
        key,
        "the same monitor thread"
    );
}
