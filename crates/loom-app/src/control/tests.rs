use super::run::{crop_region, save_screenshot, search};
use super::transport::*;
use super::*;

fn run(app: &mut App, line: &str) -> Answer {
    let command: Command = serde_json::from_str(line).unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let call = Call {
        command,
        reply: Reply(Arc::new(Mutex::new(Some(sender)))),
    };
    let _ = app.control(call);
    // As the window would: answers wait for the change to be drawn, and
    // a `cc` for the engine to take it. Frames come about 60 a second.
    for _ in 0..u32::from(FRAMES_BEFORE_ANSWER) + u32::from(FRAMES_FOR_ENGINE) {
        if let Ok(answer) = receiver.try_recv() {
            return answer;
        }
        let _ = app.update(Message::ControlFrame(Instant::now()));
        if app.control_waiting() {
            std::thread::sleep(Duration::from_millis(16));
        }
    }
    receiver.try_recv().expect("answered after drawing")
}

#[test]
fn parses_commands_and_rejects_unknown_ones() {
    let parse = |s: &str| serde_json::from_str::<Command>(s);
    assert!(matches!(
        parse(r#"{"cmd": "set", "module": 4, "param": "cutoff", "value": 800}"#),
        Ok(Command::Set { module: 4, .. })
    ));
    assert!(parse(r#"{"cmd": "fly"}"#).is_err());
    assert!(parse(r#"{"cmd": "select", "modules": [1], "extra": 1}"#).is_err());
}

/// File shortcuts would open a dialog nobody can close (rule 9).
#[test]
fn file_shortcuts_are_refused() {
    let cmd = keyboard::Modifiers::COMMAND;
    for c in ["n", "o", "s"] {
        let key = keyboard::Key::Character(c.into());
        assert!(refuse_dialog_keys(&key, cmd).is_err(), "{c}");
        assert!(
            refuse_dialog_keys(&key, cmd | keyboard::Modifiers::SHIFT).is_err(),
            "{c}"
        );
        assert!(refuse_dialog_keys(&key, keyboard::Modifiers::default()).is_ok(), "{c}");
    }
    assert!(refuse_dialog_keys(&keyboard::Key::Character("z".into()), cmd).is_ok());
}

/// A mistaken `--control` path that names a regular file is never
/// deleted, when the app closes or otherwise.
#[test]
fn closing_leaves_other_files_alone() {
    let path = std::env::temp_dir().join(format!("loom-control-not-a-socket-{}", std::process::id()));
    std::fs::write(&path, "keep me").unwrap();
    close(&[Listener::Socket(path.clone())]);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep me");
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn targets_split_into_module_and_name() {
    assert_eq!(split_target("4:cutoff"), Some((4, Some("cutoff"))));
    assert_eq!(split_target("4"), Some((4, None)));
    assert_eq!(split_target("Cutoff"), None);
}

#[test]
fn keys_and_modifiers_by_name() {
    assert_eq!(key("z"), Ok(keyboard::Key::Character("z".into())));
    assert_eq!(key("Escape"), Ok(keyboard::Key::Named(keyboard::key::Named::Escape)));
    assert!(key("hyper").is_err());
    assert_eq!(
        modifiers(&["cmd".to_string(), "shift".to_string()]),
        Ok(keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT)
    );
}

/// Serves a script through a fake app thread that answers every call.
#[test]
fn serve_writes_one_reply_per_command_with_its_id() {
    let (mut tx, mut rx) = mpsc::channel::<Call>(1);
    let app = std::thread::spawn(move || {
        while let Some(call) = iced::futures::executor::block_on(iced::futures::StreamExt::next(&mut rx)) {
            call.answer(Ok(fields(json!({ "seen": matches!(call.command, Command::Snapshot) }))));
        }
    });
    let input =
        "{\"id\": 1, \"cmd\": \"snapshot\"}\n\n{\"cmd\": \"fly\"}\n{\"id\": \"w\", \"cmd\": \"wait\", \"ms\": 1}\n";
    let mut output = Vec::new();
    serve(input.as_bytes(), &mut output, &mut tx, None);
    drop(tx);
    app.join().unwrap();
    let lines: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], json!({ "id": 1, "ok": true, "seen": true }));
    assert_eq!(lines[1]["ok"], json!(false));
    assert_eq!(lines[2], json!({ "id": "w", "ok": true }));
}

/// Serves `script` with the variables in `vars` to a real app, as
/// `--control-script` does, and returns the replies.
fn serve_script(app: &mut App, script: &'static str, vars: &'static [(&'static str, &'static str)]) -> Vec<Value> {
    let (mut tx, mut rx) = mpsc::channel::<Call>(1);
    let client = std::thread::spawn(move || {
        let env = |name: &str| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string());
        let mut output = Vec::new();
        serve(script.as_bytes(), &mut output, &mut tx, Some(&env));
        output
    });
    // The app's side: take each call and draw frames until it is answered.
    while !client.is_finished() {
        if let Ok(call) = rx.try_recv() {
            let _ = app.control(call);
        }
        let _ = app.update(Message::ControlFrame(Instant::now()));
        std::thread::sleep(Duration::from_millis(1));
    }
    String::from_utf8(client.join().unwrap())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// A script's `${NAME}` takes its value from the environment, as a
/// number when the whole string is one variable, and the reply names
/// the variables used (Script variables, V3 and V6).
#[test]
fn a_script_takes_values_from_the_environment() {
    let mut app = App::new(crate::Args {
        patch: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron")),
        ..crate::tests::app_args()
    });
    let script = r#"{"cmd": "set", "module": 4, "param": "cutoff", "value": "${LOOM_TEST_VALUE}"}"#;
    let replies = serve_script(&mut app, script, &[("LOOM_TEST_VALUE", "800")]);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["ok"], json!(true), "{}", replies[0]);
    assert_eq!(replies[0]["vars"], json!({"LOOM_TEST_VALUE": "800"}));
    let cutoff = app.controller.graph().node(ModuleId(4)).unwrap().params[0];
    assert!((cutoff - 800.0).abs() < 0.01, "cutoff is {cutoff}");
}

/// An unset variable ends the script with an error naming it, asks the
/// app to quit, and sets exit status 2; later lines do not run (V4).
#[test]
fn an_unset_variable_stops_the_script() {
    let mut app = App::new(crate::tests::app_args());
    let script = "{\"cmd\": \"audio\", \"device\": \"${LOOM_TEST_UNSET}\"}\n{\"cmd\": \"snapshot\"}\n";
    let replies = serve_script(&mut app, script, &[]);
    assert_eq!(replies.len(), 1, "the snapshot never ran");
    assert_eq!(replies[0]["ok"], json!(false));
    assert!(
        replies[0]["error"].as_str().unwrap().contains("LOOM_TEST_UNSET"),
        "{}",
        replies[0]
    );
    assert!(app.quit, "the app was asked to quit");
    assert_eq!(exit_status(), COULD_NOT_RUN);
}

/// Output that refuses every write, like a closed stdout.
struct Closed;

impl Write for Closed {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }
}

/// An unset variable still quits the app with status 2 when its reply
/// cannot be written (V4).
#[test]
fn an_unset_variable_quits_even_when_stdout_is_closed() {
    let (mut tx, mut rx) = mpsc::channel::<Call>(1);
    let app = std::thread::spawn(move || {
        let mut quit = false;
        while let Some(call) = iced::futures::executor::block_on(iced::futures::StreamExt::next(&mut rx)) {
            quit |= matches!(call.command, Command::Quit);
            call.answer(Ok(Map::new()));
        }
        quit
    });
    let env = |_: &str| None;
    serve(
        "{\"cmd\": \"audio\", \"device\": \"${LOOM_TEST_UNSET}\"}\n".as_bytes(),
        Closed,
        &mut tx,
        Some(&env),
    );
    drop(tx);
    assert!(app.join().unwrap(), "the app was asked to quit");
    assert_eq!(exit_status(), COULD_NOT_RUN);
}

#[test]
fn commands_change_the_app_as_the_ui_would() {
    let mut app = App::new(crate::Args {
        patch: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron")),
        fake_audio: true,
        no_audio: false,
        ..crate::tests::app_args()
    });

    assert_eq!(run(&mut app, r#"{"cmd": "select", "modules": [4]}"#), Ok(Map::new()));
    assert_eq!(app.selection, [4].into());
    assert!(run(&mut app, r#"{"cmd": "select", "modules": [99]}"#).is_err());

    let set = run(
        &mut app,
        r#"{"cmd": "set", "module": 4, "param": "cutoff", "value": 800}"#,
    )
    .unwrap();
    assert_eq!(set["value"], json!(800.0));
    assert_eq!(app.controller.graph().node(ModuleId(4)).unwrap().params[0], 800.0);
    let set = run(
        &mut app,
        r#"{"cmd": "set", "module": 4, "param": "cutoff", "position": 0}"#,
    )
    .unwrap();
    assert_eq!(set["value"], json!(20.0));
    let error = run(
        &mut app,
        r#"{"cmd": "set", "module": 4, "param": "nonsense", "value": 1}"#,
    )
    .unwrap_err();
    assert!(error.contains("no parameter `nonsense`"), "{error}");

    // Each set is one undo step, as a knob turn.
    let undone = run(&mut app, r#"{"cmd": "undo"}"#).unwrap();
    assert!(undone["redo"].is_string(), "{undone:?}");
    let cutoff = app.controller.graph().node(ModuleId(4)).unwrap().params[0];
    assert_eq!(cutoff, 800.0);

    let snapshot = run(&mut app, r#"{"cmd": "snapshot"}"#).unwrap();
    assert_eq!(snapshot["snapshot"]["selection"], json!([4]));

    assert_eq!(
        run(&mut app, r#"{"cmd": "note", "note": 60, "on": true}"#),
        Ok(Map::new())
    );
    assert_eq!(
        run(&mut app, r#"{"cmd": "cc", "controller": 1, "value": 64}"#),
        Ok(Map::new())
    );
    // Another control change queued just before, as from a MIDI port:
    // `cc` still waits for its own.
    assert_eq!(app.send_midi(MidiEvent::control_change(0, 0, 2, 10)), Ok(Map::new()));
    assert_eq!(
        run(&mut app, r#"{"cmd": "cc", "controller": 3, "value": 99}"#),
        Ok(Map::new())
    );
    let seen = app.controller.last_control_change().unwrap();
    assert_eq!((seen.control.controller, seen.value), (3, 99));

    let added = run(&mut app, r#"{"cmd": "add", "kind": "loom.filter.svf@1"}"#).unwrap();
    let module = added["module"].as_u64().unwrap();
    let from = run(
        &mut app,
        &format!(r#"{{"cmd": "connect", "from": "3:out", "to": "{module}:in"}}"#),
    );
    assert_eq!(from, Ok(Map::new()));
    assert!(
        run(&mut app, r#"{"cmd": "connect", "from": "3:nope", "to": "4:in"}"#)
            .unwrap_err()
            .contains("no output `nope`")
    );
    assert_eq!(
        run(&mut app, &format!(r#"{{"cmd": "remove", "modules": [{module}]}}"#)),
        Ok(Map::new())
    );
    assert!(app.controller.graph().node(ModuleId(module as u32)).is_none());

    // `probe` adds a scope fed by the output, as the probe menu does.
    let scope = run(&mut app, r#"{"cmd": "probe", "from": "3:out", "kind": "scope"}"#).unwrap();
    let scope = ModuleId(scope["module"].as_u64().unwrap() as u32);
    assert_eq!(
        app.controller.graph().node(scope).unwrap().descriptor.id,
        "loom.display.scope"
    );
    assert!(
        app.controller
            .graph()
            .cables()
            .any(|(from, to)| from.module == ModuleId(3) && to.module == scope)
    );
    assert!(
        run(&mut app, r#"{"cmd": "probe", "from": "3:out", "kind": "spectrum"}"#)
            .unwrap_err()
            .contains("not scope or meter")
    );
    assert_eq!(run(&mut app, r#"{"cmd": "undo"}"#).map(|_| ()), Ok(()));
    assert!(app.controller.graph().node(scope).is_none());

    assert_eq!(run(&mut app, r#"{"cmd": "quit"}"#), Ok(Map::new()));
    assert!(app.quit);
}

/// The MIDI learn check from #256: turn learn mode on, arm a control,
/// send a control change, turn learn mode off, and read the mapping.
#[test]
fn a_script_maps_a_control_in_learn_mode() {
    let mut app = App::new(crate::Args {
        patch: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron")),
        fake_audio: true,
        no_audio: false,
        ..crate::tests::app_args()
    });
    let ok = Ok(Map::new());
    assert!(
        run(&mut app, r#"{"cmd": "arm", "target": "4:cutoff"}"#)
            .unwrap_err()
            .contains("press `learn` first")
    );
    assert_eq!(run(&mut app, r#"{"cmd": "toolbar", "button": "learn"}"#), ok);
    assert!(app.learn.is_on());
    assert_eq!(run(&mut app, r#"{"cmd": "select", "modules": [4]}"#), ok);
    assert_eq!(run(&mut app, r#"{"cmd": "arm", "target": "4:cutoff"}"#), ok);
    let row = &run(&mut app, r#"{"cmd": "snapshot"}"#).unwrap()["snapshot"]["inspector"]["module"]["params"][0];
    assert_eq!(row["mode"]["learn"]["armed"], json!(true), "{row}");

    // `cc` answers once the engine has taken the change and the armed
    // parameter has learned it, so turning learn mode off next cannot
    // lose the mapping.
    assert_eq!(run(&mut app, r#"{"cmd": "cc", "controller": 21, "value": 64}"#), ok);
    assert!(app.learn.armed().is_none(), "the armed parameter learned the control");
    assert_eq!(run(&mut app, r#"{"cmd": "toolbar", "button": "learn"}"#), ok);
    assert!(!app.learn.is_on());
    assert_eq!(run(&mut app, r#"{"cmd": "toolbar", "button": "hold"}"#), ok);
    assert!(app.holding);

    let row = &run(&mut app, r#"{"cmd": "snapshot"}"#).unwrap()["snapshot"]["inspector"]["module"]["params"][0];
    let mapping = row["mapping"].as_str().unwrap_or_default();
    assert!(mapping.contains("21"), "{row}");
}

/// `sample` records a knob's live value once per frame (#255): with a
/// held envelope modulating a filter's Resonance, the live dot moves.
/// Then the app asks for no more frames (rule 7).
#[test]
fn sample_follows_a_live_value_frame_by_frame() {
    // The idle engine, run by `run_playing` between frames, so the
    // frame count does not depend on how fast this machine draws. A
    // held envelope stands in for a played note, which needs audio.
    let (mut app, env) = held_envelope();
    let filter = app.controller.add_module("loom.filter.svf").unwrap();
    app.commit();
    let resonance = app
        .controller
        .graph()
        .node(filter)
        .unwrap()
        .descriptor
        .param_index("resonance")
        .unwrap();
    let _ = app.update(Message::Assign {
        source: loom_core::OutputRef {
            module: ModuleId(env),
            port: 0,
        },
        module: filter.0,
        index: resonance,
    });

    let line = format!(r#"{{"cmd": "sample", "target": "{}:resonance", "ms": 300}}"#, filter.0);
    let sample = run_playing(&mut app, &line).unwrap();
    assert!(!app.control_waiting());
    let frames = sample["frames"].as_array().unwrap();
    // One entry per test-clock frame inside the 300 ms: at 0, 16.667,
    // and so on up to 283.3 ms. The count does not depend on the machine.
    assert_eq!(frames.len(), 18, "{sample:?}");
    let times: Vec<f64> = frames.iter().map(|f| f["t"].as_f64().unwrap()).collect();
    assert_eq!(times[0], 0.0);
    assert!(times.windows(2).all(|t| t[0] < t[1]), "{times:?}");
    assert!(times.iter().all(|&t| t < 300.0), "{times:?}");
    let live: Vec<f64> = frames.iter().map(|f| f["live"].as_f64().unwrap()).collect();
    assert!(
        live.windows(2).any(|l| l[0] != l[1]),
        "the live value did not move: {live:?}"
    );
    assert!(
        frames.iter().all(|f| f.get("peak").is_none()),
        "only meters have a peak"
    );

    let line = format!(r#"{{"cmd": "sample", "target": "{}:nonsense", "ms": 100}}"#, filter.0);
    let error = run(&mut app, &line).unwrap_err();
    assert!(error.contains("no control, light, or meter `nonsense`"), "{error}");
    let line = format!(r#"{{"cmd": "sample", "target": "{}:resonance", "ms": 0}}"#, filter.0);
    let error = run(&mut app, &line).unwrap_err();
    assert!(error.contains("from 1 to"), "{error}");
    assert!(!app.control_waiting());
}

/// Removing the sampled module ends the sample with an error, not with
/// the last values it drew, which the value table still holds.
#[test]
fn removing_the_sampled_module_ends_the_sample() {
    let mut app = App::new(crate::Args {
        patch: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron")),
        ..crate::tests::app_args()
    });
    let (sender, receiver) = std::sync::mpsc::channel();
    let call = Call {
        command: Command::Sample {
            target: "4:resonance".to_string(),
            ms: 10_000,
        },
        reply: Reply(Arc::new(Mutex::new(Some(sender)))),
    };
    let _ = app.control(call);
    for _ in 0..=FRAMES_BEFORE_ANSWER {
        let _ = app.update(Message::ControlFrame(Instant::now()));
    }
    assert!(receiver.try_recv().is_err(), "still sampling");
    let _ = app.update(Message::Canvas(CanvasEvent::Delete(vec![4])));
    let _ = app.update(Message::ControlFrame(Instant::now()));
    let error = receiver.try_recv().unwrap().unwrap_err();
    assert!(error.contains("removed while sampling"), "{error}");
    assert!(!app.control_waiting());
}

/// An idle-engine app with an envelope whose gate is held, so its
/// light and output move. Returns the app and the envelope's ID.
fn held_envelope() -> (App, u32) {
    let mut app = App::new(crate::tests::app_args());
    let gate = app.controller.add_module("loom.math.constant").unwrap();
    app.controller.set_param_named(gate, "value", 1.0).unwrap();
    let env = app.controller.add_module("loom.env.envelope").unwrap();
    app.controller.set_param_named(env, "decay", 0.3).unwrap();
    app.controller.set_param_named(env, "sustain", 0.2).unwrap();
    app.controller.connect_named(gate, "out", env, "gate").unwrap();
    app.commit();
    (app, env.0)
}

/// The time between two frames of [`run_playing`]: 60 frames a second,
/// as a window draws.
const TEST_FRAME: Duration = Duration::from_micros(16_667);

/// Runs a command like `run`, and between frames runs the idle engine
/// and hands the app its lights and meter levels, as a playing stream
/// and the window's subscriptions would. A `sample` of a live value, a
/// light, or a meter needs both; the tests have no subscriptions.
///
/// The frames come from a test clock, not the wall clock: each one is
/// [`TEST_FRAME`] after the last, with that much sound run before it.
/// So a `sample` of `ms` milliseconds always records the same number
/// of frames, however busy the machine is (#328).
fn run_playing(app: &mut App, line: &str) -> Answer {
    let command: Command = serde_json::from_str(line).unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let call = Call {
        command,
        reply: Reply(Arc::new(Mutex::new(Some(sender)))),
    };
    let _ = app.control(call);
    let sound = (app.controller.config().sample_rate * TEST_FRAME.as_secs_f32()).round() as usize;
    let mut now = Instant::now();
    // Enough frames for the longest `sample` in these tests.
    for _ in 0..500 {
        if let Ok(answer) = receiver.try_recv() {
            return answer;
        }
        app.run_engine(sound);
        let _ = app.update(Message::ControlFrame(now));
        now += TEST_FRAME;
    }
    receiver.try_recv().expect("answered")
}

/// `sample` records a light: its brightness (`position`) changes while
/// the envelope behind it falls from its peak to its sustain.
#[test]
fn sample_records_a_light() {
    let (mut app, env) = held_envelope();
    let line = format!(r#"{{"cmd": "sample", "target": "{env}:level", "ms": 400}}"#);
    let sample = run_playing(&mut app, &line).unwrap();
    let frames = sample["frames"].as_array().unwrap();
    assert!(frames.len() >= 2, "{sample:?}");
    let light: Vec<f64> = frames.iter().map(|f| f["position"].as_f64().unwrap()).collect();
    assert!(
        light.windows(2).any(|l| l[0] != l[1]),
        "the light did not change: {light:?}"
    );
    assert!(
        frames.iter().all(|f| f.get("peak").is_none()),
        "only meters have a peak"
    );
}

/// `sample` records a meter on a patch with sound: every entry has a
/// `peak` at or above its `position` (the RMS level).
#[test]
fn sample_records_a_meter() {
    let (mut app, env) = held_envelope();
    let added = run(
        &mut app,
        &format!(r#"{{"cmd": "probe", "from": "{env}:out", "kind": "meter"}}"#),
    )
    .unwrap();
    let meter = added["module"].as_u64().unwrap();
    let line = format!(r#"{{"cmd": "sample", "target": "{meter}:out", "ms": 400}}"#);
    let sample = run_playing(&mut app, &line).unwrap();
    let frames = sample["frames"].as_array().unwrap();
    assert!(frames.len() >= 2, "{sample:?}");
    for frame in frames {
        let position = frame["position"].as_f64().unwrap();
        let peak = frame["peak"].as_f64().unwrap_or_else(|| panic!("no peak: {frame}"));
        assert!(peak >= position, "{frame}");
    }
    assert!(
        frames.iter().any(|f| f["peak"].as_f64().unwrap() > 0.0),
        "the meter saw no sound: {sample:?}"
    );
}

/// Opening a patch ends a running `sample`, even when the new patch has
/// a module with the same ID and widget name (#270).
#[test]
fn opening_a_patch_ends_a_running_sample() {
    let mut app = App::new(crate::Args {
        patch: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron")),
        ..crate::tests::app_args()
    });
    let (sender, receiver) = std::sync::mpsc::channel();
    let call = Call {
        command: Command::Sample {
            target: "4:resonance".to_string(),
            ms: 10_000,
        },
        reply: Reply(Arc::new(Mutex::new(Some(sender)))),
    };
    let _ = app.control(call);
    for _ in 0..=FRAMES_BEFORE_ANSWER {
        let _ = app.update(Message::ControlFrame(Instant::now()));
    }
    assert!(receiver.try_recv().is_err(), "still sampling");
    // Module 4 of this patch also has a `resonance` control.
    let patch = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron");
    assert!(app.replace(crate::Source::File(&patch)));
    assert_eq!(
        receiver.try_recv().unwrap().unwrap_err(),
        "another patch was opened while sampling"
    );
    assert!(!app.control_waiting());
    // The new patch can be sampled again.
    let sample = run(&mut app, r#"{"cmd": "sample", "target": "4:resonance", "ms": 50}"#).unwrap();
    assert!(!sample["frames"].as_array().unwrap().is_empty());
}

/// The `open` command on a file in the bundled examples folder gives an
/// untitled copy, as File > Open does (`docs/design/files.md`).
#[test]
fn the_open_command_opens_an_example_as_a_copy() {
    let mut app = App::new(crate::tests::app_args());
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches");
    app.document.set_examples(Some(examples.clone()));
    let path = examples.join("subtractive.ron");
    let line = serde_json::json!({ "cmd": "open", "path": path }).to_string();
    run(&mut app, &line).unwrap();
    assert!(app.document.file().is_none());
    assert_eq!(app.ui_snapshot().title, "subtractive (example) - LoomDSP");
}

/// `panic` and `toolbar` with `button: "panic"` run panic, also with no
/// audio device (panic.md, rule 4).
#[test]
fn the_panic_command_and_button_run_panic() {
    let mut app = App::new(crate::tests::app_args());
    let ok = Ok(Map::new());
    assert_eq!(run(&mut app, r#"{"cmd": "panic"}"#), ok);
    assert!(app.alerts.status().starts_with("Panic"), "{}", app.alerts.status());
    app.alerts.say(String::new());
    assert_eq!(run(&mut app, r#"{"cmd": "toolbar", "button": "panic"}"#), ok);
    assert!(app.alerts.status().starts_with("Panic"), "{}", app.alerts.status());
}

/// `toolbar` refuses what a click could not do: dialogs (rule 9),
/// disabled buttons, and unknown names.
#[test]
fn toolbar_refuses_dialogs_disabled_buttons_and_unknown_names() {
    // No audio: Hold note is disabled. Nothing done yet: nor are Undo
    // and Redo.
    let mut app = App::new(crate::tests::app_args());
    for name in ["new", "open", "save", "save-as"] {
        let error = run(&mut app, &format!(r#"{{"cmd": "toolbar", "button": "{name}"}}"#)).unwrap_err();
        assert!(error.contains("opens a dialog"), "{name}: {error}");
    }
    for (name, reason) in [
        ("hold", "Hold note is disabled"),
        ("undo", "nothing to undo"),
        ("redo", "nothing to redo"),
    ] {
        let error = run(&mut app, &format!(r#"{{"cmd": "toolbar", "button": "{name}"}}"#)).unwrap_err();
        assert!(error.contains(reason), "{name}: {error}");
    }
    let error = run(&mut app, r#"{"cmd": "toolbar", "button": "MIDI learn"}"#).unwrap_err();
    assert!(error.contains("buttons: learn, hold, panic, undo, redo"), "{error}");
    let error = run(&mut app, r#"{"cmd": "toolbar", "button": "add"}"#).unwrap_err();
    assert!(error.contains("`add` command"), "{error}");
}

/// A command waiting to be drawn keeps its answer when another patch
/// is opened before the frame comes.
#[test]
fn opening_a_patch_keeps_waiting_answers() {
    let mut app = App::new(crate::tests::app_args());
    let (sender, receiver) = std::sync::mpsc::channel();
    let call = Call {
        command: Command::Select { modules: Vec::new() },
        reply: Reply(Arc::new(Mutex::new(Some(sender)))),
    };
    let _ = app.control(call);
    assert!(app.control_waiting());
    let patch = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/sine.ron");
    assert!(app.replace(crate::Source::File(&patch)));
    for _ in 0..FRAMES_BEFORE_ANSWER {
        let _ = app.update(Message::ControlFrame(Instant::now()));
    }
    assert_eq!(receiver.try_recv(), Ok(Ok(Map::new())));
}

/// `locate` finds a control, a port, and a text where the headless UI
/// harness finds them.
#[test]
fn locate_agrees_with_the_ui_harness() {
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    let found = Found {
        canvas: Some(ui.canvas()),
        text: None,
    };
    // The two sides round differently, so allow a tiny difference.
    let near = |a: Point, b: Point| (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01;
    let knob = locate(&ui.app, "4:cutoff", &found).unwrap();
    assert!(near(knob.center(), ui.control(4, "cutoff")));
    let out = locate(&ui.app, "8:out", &found).unwrap();
    assert!(near(out.center(), ui.output(8, "out")));
    let input = locate(&ui.app, "9:in", &found).unwrap();
    assert!(near(input.center(), ui.input(9, "in")));
    let tile = locate(&ui.app, "4", &found).unwrap();
    assert!(tile.contains(knob.center()));
    assert!(locate(&ui.app, "4:nonsense", &found).is_err());
    let text = Found {
        text: Some(Rectangle::new(Point::new(1.0, 2.0), iced::Size::new(3.0, 4.0))),
        ..found
    };
    assert_eq!(locate(&ui.app, "Cutoff", &text).unwrap().x, 1.0);
    assert!(locate(&ui.app, "Cutoff", &Found::default()).is_err());
}

/// The subtractive patch in the headless UI harness, with the control
/// channel's pointer on, and the filter envelope (5) assigned to the
/// filter's Resonance, so its output has a badge and the knob a range
/// arc.
fn pointer_ui() -> crate::ui_test::Ui {
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    ui.app.pointer = crate::pointer::Pointer::new(true);
    let resonance = ui
        .app
        .controller
        .graph()
        .node(ModuleId(4))
        .unwrap()
        .descriptor
        .param_index("resonance")
        .unwrap();
    let _ = ui.app.update(Message::Assign {
        source: loom_core::OutputRef {
            module: ModuleId(5),
            port: 0,
        },
        module: 4,
        index: resonance,
    });
    ui.redraw();
    ui
}

/// Runs a command on the harness's app as the window would: each frame
/// redraws the view, where pointer and key events go in, then counts a
/// control frame.
fn run_ui(ui: &mut crate::ui_test::Ui, line: &str) -> Answer {
    let command: Command = serde_json::from_str(line).unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let call = Call {
        command,
        reply: Reply(Arc::new(Mutex::new(Some(sender)))),
    };
    let _ = ui.app.control(call);
    for _ in 0..200 {
        if let Ok(answer) = receiver.try_recv() {
            return answer;
        }
        ui.redraw();
        let _ = ui.app.update(Message::ControlFrame(Instant::now()));
    }
    receiver.try_recv().expect("answered after drawing")
}

/// The snapshot's `pointer` field.
fn pointer_of(ui: &mut crate::ui_test::Ui) -> Value {
    run_ui(ui, r#"{"cmd": "snapshot"}"#).unwrap()["snapshot"]["pointer"].clone()
}

/// #251: hover an output, pin its badge, press Escape. The snapshot
/// shows each step, and the reply gives the pointer position.
#[test]
fn a_script_hovers_pins_a_badge_and_unpins_it_with_escape() {
    let mut ui = pointer_ui();
    let reply = run_ui(&mut ui, r#"{"cmd": "hover", "target": "5:out"}"#).unwrap();
    let out = ui.output(5, "out");
    assert!((reply["x"].as_f64().unwrap() as f32 - out.x).abs() < 0.01, "{reply:?}");
    let pointer = pointer_of(&mut ui);
    assert_eq!(
        pointer["hover"],
        json!({"kind": "output", "target": "5:out"}),
        "{pointer}"
    );
    assert_eq!(pointer["pinned"], Value::Null);
    assert_eq!(pointer["buttons"], json!([]));

    run_ui(&mut ui, r#"{"cmd": "hover", "target": "5:badge:out"}"#).unwrap();
    assert_eq!(
        pointer_of(&mut ui)["hover"],
        json!({"kind": "badge", "target": "5:badge:out"})
    );
    run_ui(&mut ui, r#"{"cmd": "press"}"#).unwrap();
    assert_eq!(pointer_of(&mut ui)["buttons"], json!(["left"]));
    run_ui(&mut ui, r#"{"cmd": "release"}"#).unwrap();
    let pointer = pointer_of(&mut ui);
    assert_eq!(pointer["pinned"], json!("5:out"), "{pointer}");
    assert_eq!(pointer["buttons"], json!([]));

    assert_eq!(run_ui(&mut ui, r#"{"cmd": "key", "key": "escape"}"#), Ok(Map::new()));
    assert_eq!(pointer_of(&mut ui)["pinned"], Value::Null, "Escape unpinned the badge");
    assert!(!ui.app.control_waiting(), "no frames asked for once done (rule 7)");
    assert!(!ui.app.pointer.waiting());
}

/// #251, as in #217: a drag on a knob's range arc, with Shift, changes
/// the assignment's depth, not the knob, as one undo step.
#[test]
fn a_script_drags_a_range_arc() {
    let mut ui = pointer_ui();
    let depth = |ui: &crate::ui_test::Ui| ui.app.controller.graph().assignments().next().unwrap().1.depth;
    let resonance = |ui: &crate::ui_test::Ui| ui.app.controller.graph().node(ModuleId(4)).unwrap().params[1];
    let (before, value) = (depth(&ui), resonance(&ui));
    // On the ring beside the knob: its centre is a grid step below the
    // top of its cell, and the ring is about 12 units out.
    let index = ui
        .app
        .controller
        .graph()
        .node(ModuleId(4))
        .unwrap()
        .descriptor
        .param_index("resonance")
        .unwrap();
    let widget = ui
        .app
        .controller
        .graph()
        .node(ModuleId(4))
        .unwrap()
        .descriptor
        .panel
        .widgets
        .iter()
        .position(|w| w.bind == "resonance")
        .unwrap();
    assert_eq!(index, 1);
    let rect = ui.app.scene.module(4).unwrap().widget_bounds(widget).unwrap();
    let ring = Point::new(rect.center_x() + 12.0, rect.y + loom_canvas::GRID - 1.0);
    let from = ui.canvas().to_window(ring);
    let line = format!(
        r#"{{"cmd": "drag", "from": [{}, {}], "to": [{}, {}], "steps": 6, "modifiers": ["shift"]}}"#,
        from.x,
        from.y,
        from.x,
        from.y - 60.0
    );
    let reply = run_ui(&mut ui, &line).unwrap();
    assert!((reply["y"].as_f64().unwrap() as f32 - (from.y - 60.0)).abs() < 0.01);
    assert_ne!(depth(&ui), before, "the drag changed the depth");
    assert_eq!(resonance(&ui), value, "and not the knob");
    run_ui(&mut ui, r#"{"cmd": "undo"}"#).unwrap();
    assert_eq!(depth(&ui), before, "one undo step");
}

/// A drag from an output to a tile control makes an assignment, and one
/// undo takes it back. During the drag the snapshot shows the wire.
#[test]
fn a_script_drags_an_output_onto_a_control() {
    let mut ui = pointer_ui();
    let count = |ui: &crate::ui_test::Ui| ui.app.controller.graph().assignments().count();
    let before = count(&ui);
    run_ui(&mut ui, r#"{"cmd": "press", "target": "7:out"}"#).unwrap();
    run_ui(&mut ui, r#"{"cmd": "hover", "target": "4:cutoff"}"#).unwrap();
    let pointer = pointer_of(&mut ui);
    assert_eq!(
        pointer["drag"],
        json!({"kind": "wire", "from": "7:out", "pulled": false}),
        "{pointer}"
    );
    assert_eq!(pointer["hover"], json!({"kind": "control", "target": "4:cutoff"}));
    run_ui(&mut ui, r#"{"cmd": "release"}"#).unwrap();
    assert_eq!(count(&ui), before + 1, "the drop made an assignment");
    assert_eq!(pointer_of(&mut ui)["drag"], Value::Null);

    run_ui(&mut ui, r#"{"cmd": "undo"}"#).unwrap();
    assert_eq!(count(&ui), before);
    // The same in one command.
    run_ui(&mut ui, r#"{"cmd": "drag", "from": "7:out", "to": "4:cutoff"}"#).unwrap();
    assert_eq!(count(&ui), before + 1);
}

/// A script can target an inspector parameter by its window coordinates
/// and reaches the same assignment and undo path as a real pointer.
#[test]
fn a_script_drags_an_output_onto_an_inspector_row() {
    let mut ui = pointer_ui();
    let _ = ui.app.update(Message::SelectModule(4));
    ui.redraw();
    let row = ui.text("Cutoff").expect("inspector Cutoff row").center();
    let before = ui.app.controller.graph().assignments().count();
    let command = serde_json::json!({"cmd": "drag", "from": "7:out", "to": [row.x, row.y]}).to_string();
    run_ui(&mut ui, &command).unwrap();
    assert_eq!(ui.app.controller.graph().assignments().count(), before + 1);
    run_ui(&mut ui, r#"{"cmd": "undo"}"#).unwrap();
    assert_eq!(ui.app.controller.graph().assignments().count(), before);
}

/// A real button let go while a script drag runs: the drag
/// goes on undisturbed, the release is not lost, and nothing reads as
/// held afterwards. Then a real pointer move takes over from the
/// script's pointer.
#[test]
fn a_real_release_during_a_script_drag_is_not_lost() {
    let mut ui = pointer_ui();
    // The real button goes down before the script starts.
    ui.press();
    let (sender, receiver) = std::sync::mpsc::channel();
    let call = Call {
        command: serde_json::from_str(r#"{"cmd": "drag", "from": "4:cutoff", "to": "4:resonance", "steps": 8}"#)
            .unwrap(),
        reply: Reply(Arc::new(Mutex::new(Some(sender)))),
    };
    let _ = ui.app.control(call);
    // The press, then two moves.
    for _ in 0..3 {
        ui.redraw();
    }
    assert!(ui.app.pointer.waiting(), "the drag is still going in");
    ui.release();
    let mut answer = None;
    for _ in 0..50 {
        ui.redraw();
        let _ = ui.app.update(Message::ControlFrame(Instant::now()));
        if let Ok(a) = receiver.try_recv() {
            answer = Some(a);
            break;
        }
    }
    assert!(answer.expect("answered").is_ok());
    let pointer = pointer_of(&mut ui);
    assert_eq!(pointer["buttons"], json!([]), "{pointer}");
    assert_eq!(pointer["drag"], Value::Null, "{pointer}");

    // A real move takes over: the snapshot follows the real pointer.
    let canvas = ui.canvas();
    let real = Point::new(canvas.bounds.x + 5.0, canvas.bounds.y + 5.0);
    ui.move_to(real);
    ui.redraw();
    let pointer = pointer_of(&mut ui);
    assert_eq!(
        (pointer["x"].as_f64(), pointer["y"].as_f64()),
        (Some(real.x as f64), Some(real.y as f64))
    );
}

/// A capture is saved whole, or cut to an area that is reported in
/// logical pixels. An area outside the window, or no window, is an
/// error.
#[test]
fn a_capture_is_saved_whole_or_cut_to_an_area() {
    let dir = std::env::temp_dir().join(format!("loom-control-shot-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("shot.png");
    let shot = || {
        Some(iced::window::Screenshot::new(
            vec![255u8; 40 * 20 * 4],
            iced::Size::new(40, 20),
            2.0,
        ))
    };
    let whole = save_screenshot(shot(), &path, None).unwrap();
    assert_eq!(
        (whole["width"].clone(), whole["height"].clone()),
        (json!(40), json!(20))
    );
    assert!(whole.get("area").is_none());
    let area = Rectangle::new(Point::new(2.0, 1.0), iced::Size::new(5.0, 4.0));
    let cut = save_screenshot(shot(), &path, Some(area)).unwrap();
    assert_eq!((cut["width"].clone(), cut["height"].clone()), (json!(10), json!(8)));
    assert_eq!(cut["area"], json!({"x": 2.0, "y": 1.0, "width": 5.0, "height": 4.0}));
    let outside = Rectangle::new(Point::new(100.0, 0.0), iced::Size::new(5.0, 4.0));
    assert!(
        save_screenshot(shot(), &path, Some(outside))
            .unwrap_err()
            .contains("outside the window")
    );
    assert!(save_screenshot(None, &path, None).unwrap_err().contains("no window"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// After the window is searched: `locate` answers, and a `screenshot`
/// of a target that is not there answers with the error at once.
#[test]
fn a_search_answers_locate_and_refuses_a_missing_screenshot_target() {
    let mut ui = pointer_ui();
    let found = Found {
        canvas: Some(ui.canvas()),
        text: None,
    };
    let ask = |ui: &mut crate::ui_test::Ui, target: &str, then: AfterSearch| {
        let (sender, receiver) = std::sync::mpsc::channel();
        let call = Call {
            command: Command::Snapshot,
            reply: Reply(Arc::new(Mutex::new(Some(sender)))),
        };
        let _ = ui.app.located(call, target, &found, then);
        receiver.try_recv()
    };
    let answer = ask(&mut ui, "4:cutoff", AfterSearch::Locate).unwrap().unwrap();
    assert!(answer["width"].as_f64().unwrap() > 0.0);
    let then = AfterSearch::Screenshot {
        path: PathBuf::from("unused.png"),
        margin: 4.0,
    };
    let error = ask(&mut ui, "4:nope", then.clone()).unwrap().unwrap_err();
    assert!(error.contains("`nope`"), "{error}");
    // A target that is there waits for the window capture.
    assert!(ask(&mut ui, "4:cutoff", then).is_err(), "not answered yet");
    let _ = search(
        Call {
            command: Command::Snapshot,
            reply: Reply(Arc::new(Mutex::new(None))),
        },
        "4".to_string(),
        AfterSearch::Locate,
    );
}

/// The snapshot names a tile under a light, a knob turn, a tile move,
/// and a pan.
#[test]
fn the_snapshot_names_what_the_pointer_is_on_and_does() {
    let mut ui = pointer_ui();
    run_ui(&mut ui, r#"{"cmd": "hover", "target": "5:level"}"#).unwrap();
    // The canvas hits a light as part of its tile.
    assert_eq!(pointer_of(&mut ui)["hover"], json!({"kind": "tile", "target": "5"}));
    run_ui(&mut ui, r#"{"cmd": "press", "target": "4:cutoff"}"#).unwrap();
    assert_eq!(
        pointer_of(&mut ui)["drag"],
        json!({"kind": "knob", "target": "4:cutoff"})
    );
    run_ui(&mut ui, r#"{"cmd": "release"}"#).unwrap();
    let tile = ui.app.scene.module(4).unwrap().bounds();
    let title = ui
        .canvas()
        .to_window(Point::new(tile.x + tile.width * 0.5, tile.y + 6.0));
    run_ui(
        &mut ui,
        &format!(r#"{{"cmd": "hover", "x": {}, "y": {}}}"#, title.x, title.y),
    )
    .unwrap();
    assert_eq!(pointer_of(&mut ui)["hover"], json!({"kind": "tile", "target": "4"}));
    run_ui(&mut ui, r#"{"cmd": "press"}"#).unwrap();
    assert_eq!(pointer_of(&mut ui)["drag"], json!({"kind": "move", "modules": [4]}));
    run_ui(&mut ui, r#"{"cmd": "release"}"#).unwrap();
    run_ui(&mut ui, r#"{"cmd": "press", "button": "middle"}"#).unwrap();
    assert_eq!(pointer_of(&mut ui)["drag"], json!({"kind": "pan"}));
    run_ui(&mut ui, r#"{"cmd": "release", "button": "middle"}"#).unwrap();
}

/// Pointer commands that cannot run say why.
#[test]
fn more_pointer_commands_are_refused() {
    let mut ui = pointer_ui();
    for (line, why) in [
        (r#"{"cmd": "release"}"#, "not in the window yet"),
        (r#"{"cmd": "scroll", "dy": 1}"#, "not in the window yet"),
        (r#"{"cmd": "hover"}"#, "give `target`"),
        (
            r#"{"cmd": "drag", "from": "4:cutoff", "to": "4:resonance", "steps": 0}"#,
            "from 1 to",
        ),
        (
            r#"{"cmd": "drag", "from": [1], "to": "4:resonance"}"#,
            "`from` is a target",
        ),
        (
            r#"{"cmd": "drag", "from": ["a", "b"], "to": "4:resonance"}"#,
            "`from` is a target",
        ),
        (
            r#"{"cmd": "scroll", "target": "4", "dy": 1, "modifiers": ["hyper"]}"#,
            "unknown modifier",
        ),
        (r#"{"cmd": "press", "button": "learn"}"#, "unknown mouse button"),
        (r#"{"cmd": "key", "key": "hyper"}"#, "unknown key"),
    ] {
        let error = run_ui(&mut ui, line).unwrap_err();
        assert!(error.contains(why), "{line}: {error}");
    }
    // With Shift, a press and release on a toggle-free spot work, and
    // the modifiers are let go after.
    run_ui(&mut ui, r#"{"cmd": "press", "x": 5, "y": 300, "modifiers": ["shift"]}"#).unwrap();
    run_ui(&mut ui, r#"{"cmd": "release", "button": "left", "x": 5, "y": 300}"#).unwrap();
    run_ui(&mut ui, r#"{"cmd": "press", "button": "right", "target": "4"}"#).unwrap();
    assert_eq!(pointer_of(&mut ui)["buttons"], json!(["right"]));
    run_ui(&mut ui, r#"{"cmd": "release", "button": "right"}"#).unwrap();
    run_ui(&mut ui, r#"{"cmd": "scroll", "dy": -1, "dx": 1}"#).unwrap();
}

/// With the browser open, a key goes to it as its key subscription
/// sends it, and the stats bar draws the clock with `--fake-audio`.
#[test]
fn keys_reach_the_open_browser_and_the_bar_draws_the_clock() {
    let app = App::new(crate::Args {
        patch: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron")),
        fake_audio: true,
        no_audio: false,
        ..crate::tests::app_args()
    });
    let mut ui = crate::ui_test::Ui::with_app(app);
    ui.app.pointer = crate::pointer::Pointer::new(true);
    assert!(
        ui.text("no device (clock) at 48000 Hz").is_some(),
        "the bar shows the clock"
    );
    let _ = ui.app.update(Message::OpenBrowser);
    ui.redraw();
    assert!(ui.app.browser.is_some());
    run_ui(&mut ui, r#"{"cmd": "key", "key": "escape"}"#).unwrap();
    assert!(ui.app.browser.is_none(), "Escape closed the browser");
}

/// Keys go through the view: Cmd+Z, which no widget takes, reaches the
/// app and undoes, as from the keyboard.
#[test]
fn a_key_no_widget_takes_reaches_the_app() {
    let mut ui = pointer_ui();
    assert!(ui.app.edits.history.undo_step().is_some());
    let before = ui.app.controller.graph().assignments().count();
    assert_eq!(
        run_ui(&mut ui, r#"{"cmd": "key", "key": "z", "modifiers": ["cmd"]}"#),
        Ok(Map::new())
    );
    assert_eq!(ui.app.controller.graph().assignments().count(), before - 1);
}

/// Bad pointer commands get an error, and nothing stays held.
#[test]
fn pointer_commands_refuse_what_they_cannot_aim_at() {
    let mut ui = pointer_ui();
    let error = run_ui(&mut ui, r#"{"cmd": "press"}"#).unwrap_err();
    assert!(error.contains("not in the window yet"), "{error}");
    let error = run_ui(&mut ui, r#"{"cmd": "hover", "target": "4:nonsense"}"#).unwrap_err();
    assert!(
        error.contains("no control, port, light, or meter `nonsense`"),
        "{error}"
    );
    let error = run_ui(&mut ui, r#"{"cmd": "hover", "target": "3:badge:out"}"#).unwrap_err();
    assert!(error.contains("no badge"), "{error}");
    let error = run_ui(&mut ui, r#"{"cmd": "hover", "x": 3}"#).unwrap_err();
    assert!(error.contains("give `target`, or `x` and `y`"), "{error}");
    let error = run_ui(&mut ui, r#"{"cmd": "press", "button": "thumb", "x": 1, "y": 1}"#).unwrap_err();
    assert!(error.contains("unknown mouse button"), "{error}");
    let error = run_ui(&mut ui, r#"{"cmd": "drag", "from": "4:cutoff", "to": 5}"#).unwrap_err();
    assert!(error.contains("`to` is a target or [x, y]"), "{error}");
    // The drag's end is not found: the press is let go again.
    let error = run_ui(&mut ui, r#"{"cmd": "drag", "from": "4:cutoff", "to": "4:nope"}"#).unwrap_err();
    assert!(error.contains("`nope`"), "{error}");
    assert_eq!(pointer_of(&mut ui)["buttons"], json!([]));
    assert_eq!(pointer_of(&mut ui)["drag"], Value::Null);
    // A wheel step over the canvas zooms it.
    let zoom = ui.canvas().camera.zoom;
    run_ui(&mut ui, r#"{"cmd": "scroll", "target": "4", "dy": 1}"#).unwrap();
    assert!(ui.canvas().camera.zoom > zoom);
}

/// #254: `locate`, and so the pointer commands, find a light by its
/// display value and a meter that shares its output's name by
/// `display:`.
#[test]
fn locate_finds_lights_and_meters() {
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    let env = ui.app.controller.add_module("loom.env.envelope").unwrap().0;
    let meter = ui.app.controller.add_module("loom.display.meter").unwrap().0;
    ui.app.commit();
    ui.redraw();
    let found = Found {
        canvas: Some(ui.canvas()),
        text: None,
    };
    let view = found.canvas.unwrap();
    let centre = |module: u32, widget: usize| {
        view.to_window(
            ui.app
                .scene
                .module(module)
                .unwrap()
                .widget_bounds(widget)
                .unwrap()
                .center(),
        )
    };
    let near = |a: Point, b: Point| (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01;
    let light = ui
        .app
        .controller
        .graph()
        .node(ModuleId(env))
        .unwrap()
        .descriptor
        .panel
        .widgets
        .iter()
        .position(|w| w.bind == "level")
        .unwrap();
    let rect = locate(&ui.app, &format!("{env}:level"), &found).unwrap();
    assert!(near(rect.center(), centre(env, light)));
    let rect = locate(&ui.app, &format!("{meter}:display:out"), &found).unwrap();
    assert!(near(rect.center(), centre(meter, 0)));
    // Without the prefix the name is the output (rule 10).
    let rect = locate(&ui.app, &format!("{meter}:out"), &found).unwrap();
    let port = ui.output(meter, "out");
    assert!(near(rect.center(), port));
}

/// The area a `screenshot` saves, in physical pixels: scaled, and cut
/// to the image.
#[test]
fn a_screenshot_area_is_scaled_and_cut_to_the_window() {
    let size = iced::Size::new(200, 100);
    let rect = |x, y, w, h| Rectangle::new(Point::new(x, y), iced::Size::new(w, h));
    let region = crop_region(size, 2.0, rect(10.0, 5.0, 20.0, 10.0)).unwrap();
    assert_eq!((region.x, region.y, region.width, region.height), (20, 10, 40, 20));
    let region = crop_region(size, 1.0, rect(-10.0, 90.0, 30.0, 30.0)).unwrap();
    assert_eq!((region.x, region.y, region.width, region.height), (0, 90, 20, 10));
    assert!(crop_region(size, 1.0, rect(300.0, 0.0, 10.0, 10.0)).is_none());
}

/// `screenshot` takes `target` with `margin`, or `rect`, and refuses
/// what does not fit together before anything is drawn.
#[test]
fn screenshot_fields_are_checked() {
    let mut app = App::new(crate::tests::app_args());
    let bad = [
        (
            r#"{"cmd": "screenshot", "path": "x.png", "margin": 4}"#,
            "goes with `target`",
        ),
        (
            r#"{"cmd": "screenshot", "path": "x.png", "target": "4", "rect": [0, 0, 1, 1]}"#,
            "not both",
        ),
        (
            r#"{"cmd": "screenshot", "path": "x.png", "rect": [0, 0, 0, 1]}"#,
            "above 0",
        ),
        (
            r#"{"cmd": "screenshot", "path": "x.png", "target": "4", "margin": -1}"#,
            "0 or more",
        ),
    ];
    for (line, why) in bad {
        let error = run(&mut app, line).unwrap_err();
        assert!(error.contains(why), "{line}: {error}");
    }
}

/// An app on the fake host, playing the subtractive patch, with its
/// preferences file in a folder of its own.
fn on_fake_devices(name: &str, fake: &std::rc::Rc<crate::devices::fake::Fake>) -> App {
    let dir = std::env::temp_dir().join(format!("loom-control-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let args = crate::Args {
        patch: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron")),
        no_audio: false,
        no_recovery: true,
        devices: crate::devices::Options {
            preferences: Some(dir.join(crate::preferences::FILE_NAME)),
            ..crate::devices::Options::default()
        },
        ..crate::tests::app_args()
    };
    App::boot_with(
        args,
        &mut crate::recovery::Dialogs,
        std::rc::Rc::new(crate::devices::fake::Shared(fake.clone())),
    )
}

/// The Preferences commands go through the window's messages: the
/// device, rate, buffer, output safety, and MIDI inputs change as a
/// click would change them, and each reply says what plays now
/// (control-channel.md, Devices and stream health).
#[test]
fn preferences_commands_change_what_the_window_shows() {
    use crate::devices::fake::{self, Fake};
    use crate::preferences::{BufferChoice, DeviceChoice, RateChoice};
    let fake = std::rc::Rc::new(Fake::new(
        &[fake::device("Fake A"), fake::device("Fake B")],
        &["Keys", "Pad"],
    ));
    let mut app = on_fake_devices("preferences", &fake);

    let reply = run(&mut app, r#"{"cmd": "audio", "device": "fake b"}"#).unwrap();
    assert_eq!(app.rig.shown_audio().device, DeviceChoice::Named("Fake B".into()));
    assert_eq!(fake.opened(), ["Fake A", "Fake B"]);
    assert!(
        reply["playing"]
            .as_str()
            .unwrap()
            .starts_with("Playing on \"Fake B\" at 48000 Hz"),
        "{reply:?}"
    );
    assert!(reply["status"].is_string());
    assert_eq!(reply["audio"]["device"], json!("Fake B"));

    let reply = run(&mut app, r#"{"cmd": "audio", "rate": 44100}"#).unwrap();
    assert_eq!(app.rig.shown_audio().sample_rate, RateChoice::Hz(44_100));
    assert!(reply["playing"].as_str().unwrap().contains("44100 Hz"), "{reply:?}");
    let reply = run(&mut app, r#"{"cmd": "audio", "buffer": 64}"#).unwrap();
    assert_eq!(app.rig.shown_audio().buffer, BufferChoice::Frames(64));
    assert!(reply["playing"].as_str().unwrap().contains("64 frames"), "{reply:?}");

    // What the window does not offer is refused, with what it offers.
    let error = run(&mut app, r#"{"cmd": "audio", "rate": 96000}"#).unwrap_err();
    assert!(error.contains("44100, 48000"), "{error}");
    let error = run(&mut app, r#"{"cmd": "audio", "buffer": 100}"#).unwrap_err();
    assert!(error.contains("32, 64"), "{error}");
    let error = run(&mut app, r#"{"cmd": "audio", "device": "Fake"}"#).unwrap_err();
    assert!(error.contains("several"), "{error}");
    let error = run(&mut app, r#"{"cmd": "audio", "device": "Nowhere"}"#).unwrap_err();
    assert!(error.contains("\"Fake A\", \"Fake B\""), "{error}");
    assert!(run(&mut app, r#"{"cmd": "audio", "rate": 48000, "buffer": 64}"#).is_err());
    assert!(run(&mut app, r#"{"cmd": "audio", "rate": "fast"}"#).is_err());
    assert_eq!(fake.opened().len(), 4, "a refused change opens nothing");

    run(&mut app, r#"{"cmd": "audio", "buffer": "default"}"#).unwrap();
    assert_eq!(app.rig.shown_audio().buffer, BufferChoice::DeviceDefault);
    run(&mut app, r#"{"cmd": "audio", "rate": "default"}"#).unwrap();
    assert_eq!(app.rig.shown_audio().sample_rate, RateChoice::DeviceDefault);
    run(&mut app, r#"{"cmd": "audio", "device": "default"}"#).unwrap();
    assert_eq!(app.rig.shown_audio().device, DeviceChoice::SystemDefault);
    assert_eq!(fake.opened().last().map(String::as_str), Some("Fake A"));
    assert!(app.rig.window.is_none(), "the commands leave the window closed");

    let reply = run(&mut app, r#"{"cmd": "output_safety", "mode": "clean"}"#).unwrap();
    assert_eq!(app.stream.output_safety(), OutputSafety::Clean);
    assert!(
        reply["playing"].as_str().unwrap().starts_with("Playing on"),
        "{reply:?}"
    );
    assert!(run(&mut app, r#"{"cmd": "output_safety", "mode": "loud"}"#).is_err());

    let reply = run(&mut app, r#"{"cmd": "midi", "input": "pad", "on": false}"#).unwrap();
    assert_eq!(reply["inputs"], json!(["Keys"]));
    assert!(!fake.is_open("Pad"));
    let reply = run(&mut app, r#"{"cmd": "midi", "input": "Pad", "on": true}"#).unwrap();
    assert_eq!(reply["inputs"], json!(["Keys", "Pad"]));
    assert!(run(&mut app, r#"{"cmd": "midi", "input": "Drums", "on": true}"#).is_err());

    assert!(!app.controller.graph().is_empty());
    assert_eq!(run(&mut app, r#"{"cmd": "new"}"#), Ok(Map::new()));
    assert_eq!(app.controller.graph().len(), 1, "only the new patch's Transport");
    assert!(fake.is_open("Keys"), "New keeps the MIDI inputs");
}

/// With `--fake-audio` there are no device lists, so `audio` is refused
/// (rule S6). Output safety still changes.
#[test]
fn audio_is_refused_with_no_device() {
    let mut app = App::new(crate::Args {
        fake_audio: true,
        no_audio: false,
        ..crate::tests::app_args()
    });
    let error = run(&mut app, r#"{"cmd": "audio", "rate": 48000}"#).unwrap_err();
    assert!(error.contains("--fake-audio"), "{error}");
    assert!(run(&mut app, r#"{"cmd": "output_safety", "mode": "off"}"#).is_ok());
    assert_eq!(app.stream.output_safety(), OutputSafety::Off);
}

/// A device can start, call back a few times, and then stop with no
/// error, while the app still says it plays. The snapshot's `audio`
/// field shows it: `advanced` turns false and `callbacks` stops.
#[test]
fn the_snapshot_sees_a_device_stop_calling_back() {
    use crate::devices::fake::{self, Fake, STALL_AFTER};
    let mut fake = Fake::new(&[fake::device("Fake A"), fake::device("Stuck")], &[]);
    fake.stalls = vec!["Stuck".to_string()];
    let fake = std::rc::Rc::new(fake);
    let mut app = on_fake_devices("stall", &fake);

    let audio = |answer: Answer| answer.unwrap()["audio"].clone();
    let first = audio(run(&mut app, r#"{"cmd": "snapshot"}"#));
    assert_eq!(first["device"], json!("Fake A"));
    assert_eq!(first["rate"], json!(48_000));

    run(&mut app, r#"{"cmd": "audio", "device": "Stuck"}"#).unwrap();
    let started = audio(run(&mut app, r#"{"cmd": "snapshot"}"#));
    assert_eq!(started["device"], json!("Stuck"));
    assert_eq!(started["callbacks"], json!(STALL_AFTER));
    assert_eq!(started["advanced"], json!(true), "a new stream counts from its start");
    assert_eq!(started["buffer"], json!(256));
    assert_eq!(started["stream_errors"], json!(0));
    assert_eq!(started["start_overloads"], json!(0));

    let stalled = audio(run(&mut app, r#"{"cmd": "snapshot"}"#));
    assert_eq!(stalled["callbacks"], json!(STALL_AFTER));
    assert_eq!(stalled["advanced"], json!(false));
    // The line still says it plays: only the counters show the stall.
    assert!(
        app.playing_line().starts_with("Playing on \"Stuck\""),
        "{}",
        app.playing_line()
    );
}

/// A short folder for a socket: the path limit is the operating
/// system's (rule 13).
#[cfg(unix)]
fn socket_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("loom-sock-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The socket replaces a stale one, answers each client one line per
/// command, accepts the next client, and is removed when the app closes.
#[cfg(unix)]
#[test]
fn socket_serves_clients_and_cleans_up() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    let dir = socket_dir("serve");
    let path = dir.join("s");
    // A socket file left by an earlier run.
    drop(UnixListener::bind(&path).unwrap());
    assert!(path.exists());

    let (tx, mut rx) = mpsc::channel::<Call>(1);
    let app = std::thread::spawn(move || {
        // Answers three commands, then closes as the app does.
        for _ in 0..3 {
            let call = iced::futures::executor::block_on(iced::futures::StreamExt::next(&mut rx)).unwrap();
            call.answer(Ok(fields(json!({ "seen": matches!(call.command, Command::Snapshot) }))));
        }
    });
    let listener = {
        let path = path.clone();
        std::thread::spawn(move || listen(&path, tx))
    };
    let ask = |lines: &str, replies: usize| -> Vec<Value> {
        let mut stream = loop {
            if let Ok(stream) = UnixStream::connect(&path) {
                break stream;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        stream.write_all(lines.as_bytes()).unwrap();
        let mut reader = BufReader::new(stream);
        (0..replies)
            .map(|_| {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                serde_json::from_str(&line).unwrap()
            })
            .collect()
    };
    let first = ask(
        "{\"id\": 1, \"cmd\": \"snapshot\"}\n{\"id\": 2, \"cmd\": \"snapshot\"}\n",
        2,
    );
    assert_eq!(first[0], json!({ "id": 1, "ok": true, "seen": true }));
    assert_eq!(first[1], json!({ "id": 2, "ok": true, "seen": true }));
    // The next client is accepted after the first one left.
    let second = ask("{\"id\": 3, \"cmd\": \"snapshot\"}\n", 1);
    assert_eq!(second[0], json!({ "id": 3, "ok": true, "seen": true }));

    app.join().unwrap();
    listener.join().unwrap();
    assert!(!path.exists(), "the socket is removed when the app closes");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A regular file at the socket path is never touched (rule 13).
#[cfg(unix)]
#[test]
fn socket_leaves_a_regular_file_alone() {
    let dir = socket_dir("file");
    let path = dir.join("s");
    std::fs::write(&path, "precious").unwrap();
    let (tx, _rx) = mpsc::channel::<Call>(1);
    listen(&path, tx);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "precious");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A path longer than the operating system allows is refused, and
/// `listen` returns instead of waiting.
#[cfg(unix)]
#[test]
fn socket_refuses_a_path_that_is_too_long() {
    let dir = socket_dir("long");
    let path = dir.join("s".repeat(200));
    let (tx, _rx) = mpsc::channel::<Call>(1);
    listen(&path, tx);
    assert!(!path.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// `open` on a file that does not load, and `save` where nothing can be
/// written, answer with the status line that says why, change nothing,
/// and open no dialog.
#[test]
fn open_and_save_failures_answer_with_the_reason() {
    let mut app = App::new(crate::tests::app_args());
    // A listener turns the channel on: errors then go to the reply and
    // the status line, never to a dialog nobody can close (rule 9).
    app.control.push(Listener::Script(PathBuf::from("unused")));
    let _ = crate::files::fake::asked();
    let dir = std::env::temp_dir().join(format!("loom-control-fail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let missing = dir.join("missing.ron");
    let line = serde_json::json!({ "cmd": "open", "path": missing }).to_string();
    let error = run(&mut app, &line).unwrap_err();
    assert!(error.starts_with("Could not open the patch"), "{error}");
    assert_eq!(error, app.alerts.status());

    // A file where the folder should be: the save cannot be written.
    std::fs::write(dir.join("blocked"), "").unwrap();
    let blocked = dir.join("blocked").join("patch.ron");
    let line = serde_json::json!({ "cmd": "save", "path": blocked }).to_string();
    let error = run(&mut app, &line).unwrap_err();
    assert!(error.starts_with("Could not save"), "{error}");
    assert!(error.ends_with("Your changes are not saved."), "{error}");
    assert_eq!(
        crate::files::fake::asked(),
        Vec::new(),
        "no dialog while the channel is on"
    );
}
