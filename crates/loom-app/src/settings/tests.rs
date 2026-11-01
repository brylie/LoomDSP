use super::*;
use crate::devices::fake::{self, Event, Fake};
use crate::preferences::FILE_NAME;
use crate::{Args, Source, recovery};
use std::path::PathBuf;
use std::time::Duration;

/// A folder for one test's preferences file.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("loom-settings-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Args for a run that plays on the fake devices, with the preferences
/// file in `dir`.
fn args(dir: &std::path::Path) -> Args {
    Args {
        no_audio: false,
        no_recovery: true,
        devices: devices::Options {
            preferences: Some(dir.join(FILE_NAME)),
            ..devices::Options::default()
        },
        ..crate::tests::app_args()
    }
}

fn start(args: Args, fake: &Rc<Fake>) -> App {
    App::boot_with(args, &mut recovery::Dialogs, Rc::new(fake::Shared(fake.clone())))
}

fn write(dir: &std::path::Path, text: &str) -> PathBuf {
    let path = dir.join(FILE_NAME);
    std::fs::write(&path, text).unwrap();
    path
}

fn two_devices() -> Rc<Fake> {
    Rc::new(Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &[]))
}

fn change(app: &mut App, change: Change) {
    app.preferences(Message::Open);
    app.preferences(Message::Audio(change));
}

/// Input opens only when the patch needs it and closes with the last module.
#[test]
fn audio_input_follows_module_lifecycle() {
    let dir = folder("audio-input-lifecycle");
    let fake = two_devices();
    *fake.audio_inputs.borrow_mut() = devices::Inputs {
        default: Some("Microphone".into()),
        devices: vec!["Microphone".into()],
    };
    let mut app = start(args(&dir), &fake);
    assert_eq!(fake.input_opens.get(), 0);
    let first = app.controller.add_module("loom.io.audio-in").unwrap();
    assert!(app.commit());
    assert_eq!(fake.input_opens.get(), 1);
    // The running input must also be included when subscriptions are built.
    let _ = app.subscription();
    let tracker = app.rig.input_tracker.as_ref().unwrap().clone();
    let second = app.controller.add_module("loom.io.audio-in").unwrap();
    assert!(app.commit());
    assert_eq!(fake.input_opens.get(), 1);
    assert!(std::sync::Arc::ptr_eq(
        &tracker,
        app.rig.input_tracker.as_ref().unwrap()
    ));
    app.controller.remove_module(first).unwrap();
    assert!(app.commit());
    assert_eq!(fake.input_closes.get(), 0);
    app.controller.remove_module(second).unwrap();
    assert!(app.commit());
    assert_eq!(fake.input_closes.get(), 1);
    assert!(fake.input_writer.borrow().is_none());
    assert!(app.rig.input_tracker.is_none());
}

/// A refused input leaves output playing and reports a useful error once.
#[test]
fn denied_audio_input_does_not_stop_the_patch() {
    let dir = folder("audio-input-denied");
    let fake = two_devices();
    *fake.input_error.borrow_mut() = Some("permission denied".into());
    let mut app = start(args(&dir), &fake);
    app.controller.add_module("loom.io.audio-in").unwrap();
    assert!(app.commit());
    assert!(app.stream.is_playing());
    assert!(app.alerts.status().contains("microphone was denied"));
    app.alerts.say("other status");
    app.rig.input_attempted = None;
    app.sync_audio_input();
    assert_eq!(app.alerts.status(), "other status");
}

/// A permission denied after opening silences input and reports the cause.
#[test]
fn permission_denied_after_input_opens_is_reported() {
    let dir = folder("audio-input-late-denial");
    let fake = two_devices();
    *fake.audio_inputs.borrow_mut() = devices::Inputs {
        default: Some("Mic".into()),
        devices: vec!["Mic".into()],
    };
    let mut app = start(args(&dir), &fake);
    app.controller.add_module("loom.io.audio-in").unwrap();
    assert!(app.commit());
    assert!(app.rig.input.is_some());
    fake.input_permission_denied.set(true);
    app.sync_audio_input();
    assert!(app.rig.input.is_none());
    assert!(app.stream.is_playing());
    assert!(app.alerts.status().contains("microphone was denied"));
    app.alerts.say("other status");
    app.sync_audio_input();
    assert_eq!(app.alerts.status(), "other status");
    assert_eq!(fake.input_closes.get(), 1);
}

/// Missing or denied input stays silent while the rest of the patch plays.
#[test]
fn unavailable_input_keeps_other_audio_playing() {
    for denied in [false, true] {
        let dir = folder(if denied {
            "audio-input-denied-sound"
        } else {
            "audio-input-missing-sound"
        });
        let tape = loom_host_desktop::Tape::new();
        let fake = Rc::new(Fake {
            tape: Some(tape.clone()),
            ..Fake::new(&[fake::device("Fake A")], &[])
        });
        if denied {
            *fake.input_error.borrow_mut() = Some("permission denied".into());
        }
        let mut options = args(&dir);
        options.output_safety = Some(OutputSafety::Off);
        let mut app = start(options, &fake);
        let input = app.controller.add_module("loom.io.audio-in").unwrap();
        let output = app.controller.add_module("loom.io.audio-out").unwrap();
        let constant = app.controller.add_module("loom.math.constant").unwrap();
        let other_output = app.controller.add_module("loom.io.audio-out").unwrap();
        app.controller.set_param_named(constant, "value", 0.2).unwrap();
        app.controller.connect_named(input, "out", output, "in").unwrap();
        app.controller
            .connect_named(constant, "out", other_output, "in")
            .unwrap();
        assert!(app.commit());
        assert!(app.stream.is_playing());
        assert!(app.rig.input.is_none());
        assert!(app.alerts.status().contains(if denied {
            "microphone was denied"
        } else {
            "no audio input"
        }));
        tape.step(1024);
        assert_eq!(app.controller.display(input, 0), Some(0.0));
        assert_eq!(tape.left()[1023], 0.2);
        assert_eq!(tape.right()[1023], 0.2);
        app.alerts.say("other status");
        app.rig.input_attempted = None;
        app.sync_audio_input();
        assert_eq!(app.alerts.status(), "other status");
    }
}

/// A failed input is replaced after the device can be opened again.
#[test]
fn failed_audio_input_reopens() {
    let dir = folder("audio-input-reopen");
    let fake = two_devices();
    *fake.audio_inputs.borrow_mut() = devices::Inputs {
        default: Some("Microphone".into()),
        devices: vec!["Microphone".into()],
    };
    let mut app = start(args(&dir), &fake);
    app.controller.add_module("loom.io.audio-in").unwrap();
    assert!(app.commit());
    fake.input_writer.borrow().as_ref().unwrap().status().fail();
    let _ = app.update(crate::Message::InputAudio(crate::input_watch::Event {
        underruns: 0,
        overruns: 0,
        show: false,
        failed: true,
        ready: false,
    }));
    assert_eq!(fake.input_closes.get(), 1);
    assert_eq!(fake.input_opens.get(), 2);
    assert!(app.rig.input.as_ref().unwrap().is_live());
    assert_eq!(app.alerts.status(), "Audio input: \"Microphone\" is back.");

    // A later loss must be reported again, even after a successful reopen.
    *fake.input_error.borrow_mut() = Some("device busy".into());
    fake.input_writer.borrow().as_ref().unwrap().status().fail();
    app.sync_audio_input();
    assert!(app.rig.input.is_none());
    assert!(app.alerts.status().contains("was lost"));

    // A permission refusal is distinct from the earlier device error.
    fake.input_permission_denied.set(true);
    app.sync_audio_input();
    assert!(app.alerts.status().contains("microphone was denied"));
}

/// Recovery preserves newer messages, and permission denial stays once per run.
#[test]
fn input_recovery_preserves_other_status_and_permission_latch() {
    let dir = folder("audio-input-recovery-status");
    let fake = two_devices();
    *fake.audio_inputs.borrow_mut() = devices::Inputs {
        default: Some("Microphone".into()),
        devices: vec!["Microphone".into()],
    };
    fake.input_permission_denied.set(true);
    let mut app = start(args(&dir), &fake);
    app.controller.add_module("loom.io.audio-in").unwrap();
    assert!(app.commit());
    assert!(app.alerts.status().contains("microphone was denied"));
    app.alerts.say("newer message");
    fake.input_permission_denied.set(false);
    app.sync_audio_input();
    assert!(app.rig.input.is_some());
    assert_eq!(app.alerts.status(), "newer message");
    fake.input_permission_denied.set(true);
    app.sync_audio_input();
    assert!(app.rig.input.is_none());
    assert_eq!(
        app.alerts.status(),
        "newer message",
        "permission denial is shown once per run"
    );

    // Choosing an input makes the permission warning eligible again.
    app.choose_audio_input(InputChoice::SystemDefault);
    assert!(app.alerts.status().contains("microphone was denied"));
}

/// A lost named input stays silent until that same device returns.
#[test]
fn named_audio_input_waits_for_its_return() {
    let dir = folder("audio-input-named-loss");
    write(&dir, "(audio: (input: Named(\"Microphone\")))");
    let fake = two_devices();
    *fake.audio_inputs.borrow_mut() = devices::Inputs {
        default: Some("Microphone".into()),
        devices: vec!["Microphone".into(), "Other".into()],
    };
    let mut app = start(args(&dir), &fake);
    app.controller.add_module("loom.io.audio-in").unwrap();
    assert!(app.commit());
    *fake.audio_inputs.borrow_mut() = devices::Inputs {
        default: Some("Other".into()),
        devices: vec!["Other".into()],
    };
    fake.input_writer.borrow().as_ref().unwrap().status().fail();
    app.sync_audio_input();
    assert!(app.rig.input.is_none());
    assert_eq!(fake.input_opens.get(), 1);
    fake.audio_inputs.borrow_mut().devices.push("Microphone".into());
    app.rig.input_attempted = None;
    app.sync_audio_input();
    assert_eq!(app.rig.input.as_ref().unwrap().name(), "Microphone");
    assert_eq!(fake.input_opens.get(), 2);
}

/// A missing saved input falls back without changing the saved choice.
#[test]
fn missing_saved_input_falls_back_without_saving() {
    let dir = folder("audio-input-fallback");
    write(&dir, "(audio: (input: Named(\"Missing\")))");
    let fake = two_devices();
    *fake.audio_inputs.borrow_mut() = devices::Inputs {
        default: Some("Microphone".into()),
        devices: vec!["Microphone".into()],
    };
    let mut app = start(args(&dir), &fake);
    app.controller.add_module("loom.io.audio-in").unwrap();
    assert!(app.commit());
    assert_eq!(app.rig.input.as_ref().unwrap().name(), "Microphone");
    assert_eq!(app.rig.store.saved().audio.input, InputChoice::Named("Missing".into()));
    assert!(app.alerts.status().contains("system default"));
}

/// Input flags apply for one run and do not replace the saved choice.
#[test]
fn audio_input_flags_do_not_change_saved_choice() {
    for disabled in [false, true] {
        let dir = folder(if disabled {
            "audio-input-flag-off"
        } else {
            "audio-input-flag-name"
        });
        let path = write(&dir, "(audio: (input: Named(\"Saved\")))");
        let fake = two_devices();
        *fake.audio_inputs.borrow_mut() = devices::Inputs {
            default: Some("Saved".into()),
            devices: vec!["Saved".into(), "Chosen input".into()],
        };
        let mut options = args(&dir);
        if disabled {
            options.devices.no_input = true;
        } else {
            options.devices.input = Some("chosen".into());
        }
        let mut app = start(options, &fake);
        app.controller.add_module("loom.io.audio-in").unwrap();
        assert!(app.commit());
        if disabled {
            assert!(app.rig.input.is_none());
            assert_eq!(fake.input_opens.get(), 0);
        } else {
            assert_eq!(app.rig.input.as_ref().unwrap().name(), "Chosen input");
        }
        assert_eq!(app.rig.store.saved().audio.input, InputChoice::Named("Saved".into()));
        app.preferences(Message::Safety(OutputSafety::Off));
        let saved = std::fs::read_to_string(path).unwrap();
        assert!(saved.contains("Saved"));
        assert!(!saved.contains("Chosen input"));
    }
}

/// A fake input uses the same ring as a real stream and reaches the output.
#[test]
fn audio_input_plays_through_the_fake_host() {
    let dir = folder("audio-input-tape");
    let tape = loom_host_desktop::Tape::new();
    let fake = Rc::new(Fake {
        tape: Some(tape.clone()),
        ..Fake::new(&[fake::device("Fake A")], &[])
    });
    *fake.audio_inputs.borrow_mut() = devices::Inputs {
        default: Some("Microphone".into()),
        devices: vec!["Microphone".into()],
    };
    let mut options = args(&dir);
    options.output_safety = Some(OutputSafety::Off);
    let mut app = start(options, &fake);
    let input = app.controller.add_module("loom.io.audio-in").unwrap();
    let output = app.controller.add_module("loom.io.audio-out").unwrap();
    app.controller.connect_named(input, "out", output, "in").unwrap();
    assert!(app.commit());
    {
        let mut writer = fake.input_writer.borrow_mut();
        for _ in 0..256 {
            assert!(writer.as_mut().unwrap().push([0.0; 8]));
        }
    }
    tape.step(256);
    let _ = app.update(crate::Message::InputAudio(crate::input_watch::Event {
        underruns: 0,
        overruns: 0,
        show: false,
        failed: false,
        ready: true,
    }));
    assert!(!app.rig.input_warned, "silent input is not live sound");
    for _ in 0..4 {
        {
            let mut writer = fake.input_writer.borrow_mut();
            let writer = writer.as_mut().unwrap();
            for _ in 0..256 {
                let mut frame = [0.0; 8];
                frame[0] = 0.25;
                frame[1] = -0.25;
                assert!(writer.push(frame));
            }
        }
        tape.step(256);
    }
    assert_eq!(tape.left()[1023], 0.25);
    assert_eq!(tape.right()[1023], -0.25);
    let _ = app.update(crate::Message::InputAudio(crate::input_watch::Event {
        underruns: 0,
        overruns: 0,
        show: false,
        failed: false,
        ready: true,
    }));
    assert!(app.alerts.status().contains("avoid feedback"));
    app.alerts.say("other status");
    let _ = app.update(crate::Message::InputAudio(crate::input_watch::Event {
        underruns: 0,
        overruns: 0,
        show: false,
        failed: false,
        ready: true,
    }));
    assert_eq!(app.alerts.status(), "other status", "the warning is shown once per run");
}

/// A fake host supplies a recorded stereo input identically at each block size.
#[test]
fn fake_host_recorded_input_is_block_invariant() {
    fn render(block: usize) -> [Vec<f32>; 2] {
        let fake = two_devices();
        *fake.audio_inputs.borrow_mut() = devices::Inputs {
            default: Some("Microphone".into()),
            devices: vec!["Microphone".into()],
        };
        let host = fake::Shared(fake.clone());
        let (_port, reader) = host
            .open_audio_input(&loom_host_desktop::AudioOptions::default(), 1024)
            .unwrap();
        {
            let mut writer = fake.input_writer.borrow_mut();
            for n in 0..1024 {
                let mut frame = [0.0; 8];
                frame[0] = 0.1 + (n % 11) as f32 * 0.01;
                frame[1] = -0.2 + (n % 7) as f32 * 0.01;
                assert!(writer.as_mut().unwrap().push(frame));
            }
        }
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
        let controller = &mut parts.controller;
        controller.set_audio_input(Some(reader)).unwrap();
        let input = controller.add_module("loom.io.audio-in").unwrap();
        let split = controller.add_module("loom.mix.split").unwrap();
        controller.set_param_named(input, "gain", 2.0).unwrap();
        controller.connect_named(input, "out", split, "in").unwrap();
        let probes = ["left", "right"].map(|name| {
            let output = controller.output_ref(split, name).unwrap();
            controller.add_probe(output, loom_core::ProbeKind::Scope).unwrap()
        });
        controller.commit().unwrap();
        let mut result = [Vec::new(), Vec::new()];
        for (start, end, mute) in [(0, 320, 0.0), (320, 640, 1.0), (640, 960, 0.0)] {
            parts.controller.set_param_named(input, "mute", mute).unwrap();
            let mut position = start;
            while position < end {
                let count = block.min(end - position);
                parts.engine.process(&mut vec![0.0; count], &mut vec![0.0; count]);
                for chunk in parts.telemetry.scope() {
                    let channel = probes.iter().position(|&probe| probe == chunk.probe).unwrap();
                    result[channel].extend_from_slice(chunk.samples());
                }
                position += count;
            }
        }
        result
    }
    let one = render(1);
    assert_eq!(one[0].len(), 960);
    assert_eq!(one[1].len(), 960);
    assert_eq!(one, render(77));
    assert_eq!(one, render(512));
    assert_eq!(one[0][0], 0.2);
    assert_eq!(one[1][0], -0.4);
    assert!(one[0][639].abs() < 0.005 && one[1][639].abs() < 0.005);
    assert!(one[0][959] > 0.0 && one[1][959] < 0.0);
}

/// Preferences show the run-only input and feedback warning before opening it.
#[test]
fn preferences_show_input_flag_before_the_stream_opens() {
    let dir = folder("audio-input-flag-preferences");
    let fake = two_devices();
    *fake.audio_inputs.borrow_mut() = devices::Inputs {
        default: Some("Other input".into()),
        devices: vec!["Other input".into(), "Fake A".into()],
    };
    let mut options = args(&dir);
    options.devices.input = Some("fAkE a".into());
    let mut app = start(options, &fake);
    assert_eq!(fake.input_opens.get(), 0);
    app.preferences(Message::Open);
    let mut ui = crate::ui_test::Ui::with_app(app);
    assert_eq!(ui.app.rig.effective_input(&fake.audio_inputs.borrow()), Some("Fake A"));
    assert!(ui.text("Set by an input flag for this run.").is_some());
    assert!(
        ui.text("Input and output use the same device. Speakers can feed the microphone.")
            .is_some()
    );
    assert!(
        ui.text("Input latency about 10.7 ms. The patch adds no delay of its own.")
            .is_some()
    );
    assert_eq!(
        fake.input_opens.get(),
        0,
        "showing preferences never opens the microphone"
    );
    ui.app.preferences(Message::Input(InputChoice::Off));
    assert!(
        ui.text("Input and output use the same device. Speakers can feed the microphone.")
            .is_none()
    );
    assert!(
        ui.text("Input latency about 10.7 ms. The patch adds no delay of its own.")
            .is_none()
    );
    assert_eq!(fake.input_opens.get(), 0);
}

/// The system-default input can show the warning before Audio In exists.
#[test]
fn preferences_show_default_input_feedback_warning_before_use() {
    let dir = folder("audio-input-default-preferences");
    let fake = two_devices();
    *fake.audio_inputs.borrow_mut() = devices::Inputs {
        default: Some("Fake A".into()),
        devices: vec!["Fake A".into()],
    };
    let mut app = start(args(&dir), &fake);
    app.preferences(Message::Open);
    let mut ui = crate::ui_test::Ui::with_app(app);
    assert!(
        ui.text("Input and output use the same device. Speakers can feed the microphone.")
            .is_some()
    );
    assert_eq!(fake.input_opens.get(), 0);
}

/// Preferences warn when input and output use the same device.
#[test]
fn same_audio_input_and_output_show_feedback_note() {
    let dir = folder("audio-input-same-device");
    let fake = two_devices();
    *fake.audio_inputs.borrow_mut() = devices::Inputs {
        default: Some("Fake A".into()),
        devices: vec!["Fake A".into()],
    };
    let mut app = start(args(&dir), &fake);
    app.controller.add_module("loom.io.audio-in").unwrap();
    assert!(app.commit());
    app.preferences(Message::Open);
    let mut ui = crate::ui_test::Ui::with_app(app);
    assert!(
        ui.text("Input and output use the same device. Speakers can feed the microphone.")
            .is_some()
    );
    assert!(
        ui.text("Input latency about 10.7 ms. The patch adds no delay of its own.")
            .is_some()
    );
}

/// Disabling input persists and leaves the output running.
#[test]
fn audio_input_choice_is_saved_without_restarting_output() {
    let dir = folder("audio-input-choice");
    let fake = two_devices();
    *fake.audio_inputs.borrow_mut() = devices::Inputs {
        default: Some("Microphone".into()),
        devices: vec!["Microphone".into()],
    };
    let mut app = start(args(&dir), &fake);
    let module = app.controller.add_module("loom.io.audio-in").unwrap();
    assert!(app.commit());
    app.preferences(Message::Input(InputChoice::Off));
    assert_eq!(fake.input_closes.get(), 1);
    assert_eq!(app.rig.store.saved().audio.input, InputChoice::Off);
    assert!(app.stream.is_playing());
    app.controller.remove_module(module).unwrap();
    assert!(app.commit());
    assert!(
        std::fs::read_to_string(dir.join(FILE_NAME))
            .unwrap()
            .contains("input: Off")
    );
}

#[test]
fn no_saved_devices_play_on_the_defaults_with_every_midi_input() {
    let dir = folder("defaults");
    let fake = Rc::new(Fake::new(
        &[fake::device("Fake A"), fake::device("Fake B")],
        &["Keys", "Pad"],
    ));
    let app = start(args(&dir), &fake);
    assert_eq!(fake.opened(), ["Fake A"]);
    assert_eq!(app.stream.sample_rate(), 48_000.0);
    assert!(app.stream.is_playing());
    assert!(fake.is_open("Keys") && fake.is_open("Pad"));
    assert_eq!(app.controller.config().output_safety, OutputSafety::LowLatency);
    assert!(!dir.join(FILE_NAME).exists(), "starting writes nothing");
    assert_eq!(
        app.playing_line(),
        "Playing on \"Fake A\" at 48000 Hz, 256 frames (5.3 ms)."
    );
}

#[test]
fn a_saved_device_plays_when_it_is_there() {
    let dir = folder("saved");
    write(&dir, "(audio: (device: Named(\"Fake B\")))");
    let fake = two_devices();
    let _app = start(args(&dir), &fake);
    assert_eq!(fake.opened(), ["Fake B"]);
}

#[test]
fn a_missing_device_falls_back_and_the_file_is_unchanged() {
    let dir = folder("missing");
    let text = "(audio: (device: Named(\"Fake B\")))";
    let path = write(&dir, text);
    let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &[]));
    let app = start(args(&dir), &fake);
    assert_eq!(fake.opened(), ["Fake A"]);
    assert!(
        app.alerts.status().contains("\"Fake B\" is not connected"),
        "{}",
        app.alerts.status()
    );
    assert!(
        app.alerts.status().contains("Playing on \"Fake A\""),
        "{}",
        app.alerts.status()
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    assert_eq!(
        app.rig.shown_audio().device,
        DeviceChoice::Named("Fake B".into()),
        "the window still shows the saved choice"
    );
}

#[test]
fn a_rate_the_device_does_not_offer_plays_at_its_default() {
    let dir = folder("rate");
    let text = "(audio: (sample_rate: Hz(96000)))";
    let path = write(&dir, text);
    let app = start(args(&dir), &two_devices());
    assert_eq!(app.stream.sample_rate(), 48_000.0);
    assert!(
        app.alerts.status().contains("does not offer 96000 Hz. Using 48000 Hz."),
        "{}",
        app.alerts.status()
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), text);
}

#[test]
fn a_rate_change_keeps_the_patch_and_its_history() {
    let dir = folder("restart");
    let mut app = start(args(&dir), &two_devices());
    let constant = loom_modules::registry().resolve("loom.math.constant").unwrap();
    for x in [100.0, 200.0, 300.0] {
        app.add_module(constant, iced::Point::new(x, 100.0));
    }
    app.controller.set_tempo(90.0).unwrap();
    let before = undo::Snapshot::take(&app.controller, &app.positions);
    let ids: Vec<_> = app.controller.graph().nodes().map(|(id, _)| id).collect();
    assert_eq!(app.edits.history.len(), 3);
    assert!(app.unsaved());

    change(&mut app, Change::Rate(RateChoice::Hz(44_100)));
    assert_eq!(app.controller.config().sample_rate, 44_100.0);
    assert_eq!(app.stream.sample_rate(), 44_100.0);
    assert_eq!(
        app.rig.last_stop,
        Some(true),
        "the old engine faded out before it stopped"
    );
    assert_eq!(
        app.controller.graph().nodes().map(|(id, _)| id).collect::<Vec<_>>(),
        ids
    );
    assert!(
        undo::diff(
            &before,
            &undo::Snapshot::take(&app.controller, &app.positions),
            &BTreeSet::new()
        )
        .is_none(),
        "the same patch"
    );
    assert_eq!(app.edits.history.len(), 3);
    assert!(app.unsaved());
    assert_eq!(app.controller.transport().tempo, 90.0);
    assert_eq!(
        crate::preferences::load(&dir.join(FILE_NAME))
            .preferences
            .audio
            .sample_rate,
        RateChoice::Hz(44_100)
    );
    // The undo history still applies to the new engine.
    app.undo();
    // Two constants and the patch's Transport.
    assert_eq!(app.controller.graph().len(), 3);

    // Picking it again does nothing.
    let generation = app.generation;
    change(&mut app, Change::Rate(RateChoice::Hz(44_100)));
    assert_eq!(app.generation, generation);
}

#[test]
fn a_device_that_fails_to_open_leaves_the_old_one_playing() {
    let dir = folder("broken-device");
    let fake = Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &[]);
    fake.broken.borrow_mut().push("Fake B".into());
    let fake = Rc::new(fake);
    let mut app = start(args(&dir), &fake);
    let generation = app.generation;
    change(&mut app, Change::Device(DeviceChoice::Named("Fake B".into())));
    assert_eq!(app.generation, generation);
    assert!(app.stream.is_playing());
    assert_eq!(app.rig.playing.as_ref().unwrap().device, "Fake A");
    assert!(!dir.join(FILE_NAME).exists(), "nothing saved");
    assert_eq!(
        app.alerts.status(),
        "Could not use \"Fake B\" at 48000 Hz: the device is busy. Still playing on \"Fake A\"."
    );
}

#[test]
fn a_device_that_fails_to_start_hands_back_to_the_old_one() {
    let dir = folder("wont-start");
    let fake = Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &["Keys"]);
    fake.wont_start.borrow_mut().push("Fake B".into());
    let fake = Rc::new(fake);
    let mut app = start(args(&dir), &fake);
    let constant = loom_modules::registry().resolve("loom.math.constant").unwrap();
    app.add_module(constant, iced::Point::new(100.0, 100.0));
    let modules = app.controller.graph().len();

    change(&mut app, Change::Device(DeviceChoice::Named("Fake B".into())));
    assert_eq!(fake.opened(), ["Fake A", "Fake B", "Fake A"], "back to the old device");
    assert!(app.stream.is_playing(), "sound is still playing");
    assert_eq!(app.rig.playing.as_ref().unwrap().device, "Fake A");
    assert_eq!(app.controller.graph().len(), modules, "with the same patch");
    assert_eq!(app.edits.history.len(), 1);
    assert!(fake.is_open("Keys"));
    assert!(!dir.join(FILE_NAME).exists(), "nothing saved");
    assert_eq!(
        app.alerts.status(),
        "Could not use \"Fake B\": the stream did not start. Still playing on \"Fake A\"."
    );
}

#[test]
fn when_neither_device_starts_audio_is_off_and_nothing_is_saved() {
    let dir = folder("neither-starts");
    let fake = Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &[]);
    // Fake A started at boot, which does not go through the host's
    // start; it refuses only when it is started again.
    fake.wont_start.borrow_mut().extend(["Fake A".into(), "Fake B".into()]);
    let fake = Rc::new(fake);
    let mut app = start(args(&dir), &fake);
    assert!(app.stream.is_playing());

    change(&mut app, Change::Device(DeviceChoice::Named("Fake B".into())));
    assert_eq!(
        fake.opened(),
        ["Fake A", "Fake B", "Fake A"],
        "it tried the old device again"
    );
    assert!(!app.stream.is_playing(), "audio is off");
    assert!(app.rig.playing.is_none());
    assert!(!dir.join(FILE_NAME).exists(), "nothing saved");
    assert_eq!(
        app.alerts.status(),
        "Could not use \"Fake B\": the stream did not start. Audio is off."
    );

    // The failed stream took its engine; an idle one takes its place,
    // so edits still work, well past the queue's size (#220).
    assert!(app.stream.is_idle());
    let constant = loom_modules::registry().resolve("loom.math.constant").unwrap();
    app.add_module(constant, iced::Point::new(100.0, 100.0));
    let id = *app.selection.first().unwrap();
    for i in 0..2_000 {
        let _ = app.update(crate::Message::Param {
            module: id,
            index: 0,
            position: (i % 100) as f32 / 100.0,
        });
        assert!(
            !app.alerts.status().contains("queue is full"),
            "move {i}: {}",
            app.alerts.status()
        );
    }
}

#[test]
fn a_device_change_opens_the_new_device_and_fades_the_old_one() {
    let dir = folder("device");
    let fake = two_devices();
    let mut app = start(args(&dir), &fake);
    change(&mut app, Change::Device(DeviceChoice::Named("Fake B".into())));
    assert_eq!(fake.opened(), ["Fake A", "Fake B"]);
    assert_eq!(app.rig.last_stop, Some(true));
    assert_eq!(app.rig.playing.as_ref().unwrap().device, "Fake B");
    assert_eq!(
        crate::preferences::load(&dir.join(FILE_NAME)).preferences.audio.device,
        DeviceChoice::Named("Fake B".into())
    );
}

#[test]
fn open_new_and_quit_fade_out_first() {
    let dir = folder("fade");
    let mut app = start(args(&dir), &two_devices());
    assert!(app.replace(Source::Empty));
    assert_eq!(app.rig.last_stop, Some(true), "New faded out");
    app.rig.last_stop = None;
    app.quit = true;
    let _ = app.update(crate::Message::Tick);
    assert_eq!(app.rig.last_stop, Some(true), "quit faded out");
}

#[test]
fn turning_an_input_off_releases_its_notes_and_is_saved() {
    let dir = folder("midi");
    let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Pad", "Keys"]));
    let mut app = start(args(&dir), &fake);
    let note = loom_modules::registry().resolve("loom.midi.note").unwrap();
    app.add_module(note, iced::Point::new(100.0, 100.0));
    let id = *app.selection.iter().next().unwrap();
    let gate = app
        .controller
        .graph()
        .node(loom_core::ModuleId(id))
        .unwrap()
        .descriptor
        .display_index("gate")
        .unwrap();
    let wait_for = |app: &App, value: f32| {
        for _ in 0..200 {
            if app.controller.display(loom_core::ModuleId(id), gate) == Some(value) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    };
    fake.play("Pad", &[0x90, 60, 100]);
    assert!(wait_for(&app, 1.0), "the note plays");

    app.preferences(Message::Midi("Pad".into(), false));
    assert!(wait_for(&app, 0.0), "and is released");
    assert!(!fake.is_open("Pad"));
    assert!(fake.is_open("Keys"));
    assert!(fake.log.borrow().contains(&Event::MidiClosed("Pad".into())));
    assert_eq!(
        crate::preferences::load(&dir.join(FILE_NAME)).preferences.midi.off,
        ["Pad"]
    );
    fake.play("Keys", &[0x90, 62, 100]);
    assert!(wait_for(&app, 1.0), "the other input still plays");
    assert!(app.stream.is_playing(), "audio did not stop");
}

/// The panic trigger (panic.md, MIDI trigger): Learn takes the next
/// control change, the mapping goes in `preferences.ron` and not in
/// the patch, a value of 0 does nothing, and 127 runs panic, which
/// releases a held note. Clear removes it.
#[test]
fn a_learned_panic_trigger_is_saved_and_runs_panic() {
    let dir = folder("panic");
    let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Pad"]));
    let mut app = start(args(&dir), &fake);
    let note = loom_modules::registry().resolve("loom.midi.note").unwrap();
    app.add_module(note, iced::Point::new(100.0, 100.0));
    let id = loom_core::ModuleId(*app.selection.iter().next().unwrap());
    let gate = app
        .controller
        .graph()
        .node(id)
        .unwrap()
        .descriptor
        .display_index("gate")
        .unwrap();
    let wait_for = |app: &App, value: f32| {
        (0..400).any(|_| {
            std::thread::sleep(Duration::from_millis(5));
            app.controller.display(id, gate) == Some(value)
        })
    };

    app.preferences(Message::Open);
    app.preferences(Message::PanicLearn);
    assert_eq!(app.panic_line(), "Waiting for MIDI...");
    fake.play("Pad", &[0xB0, 123, 127]);
    assert!(
        (0..400).any(|_| {
            std::thread::sleep(Duration::from_millis(5));
            app.controller.last_control_change().is_some()
        }),
        "the control change reached the engine"
    );
    let _ = app.update(crate::Message::Midi);
    assert_eq!(app.panic_line(), "Channel 1, controller 123");
    let saved = crate::preferences::load(&dir.join(FILE_NAME)).preferences.panic;
    assert_eq!(saved, Some(crate::preferences::PanicPref::Cc(0, 123)));
    let patch = app.current_patch().to_ron().unwrap();
    assert!(!patch.contains("123"), "the trigger is not in the patch: {patch}");

    fake.play("Pad", &[0x90, 60, 100]);
    assert!(wait_for(&app, 1.0), "the note plays");
    fake.play("Pad", &[0xB0, 123, 0]);
    assert!(!wait_for(&app, 0.0), "a value of 0 does nothing");
    fake.play("Pad", &[0xB0, 123, 127]);
    assert!(wait_for(&app, 0.0), "127 runs panic and the note is released");

    app.preferences(Message::PanicClear);
    assert_eq!(app.panic_line(), "None");
    assert_eq!(crate::preferences::load(&dir.join(FILE_NAME)).preferences.panic, None);
}

/// An app on the fake host with a MIDI Note module, and a check that
/// waits for its gate light to read a value.
fn note_app(dir: &std::path::Path, fake: &Rc<Fake>) -> (App, impl Fn(&App, f32) -> bool + use<>) {
    let mut app = start(args(dir), fake);
    let note = loom_modules::registry().resolve("loom.midi.note").unwrap();
    app.add_module(note, iced::Point::new(100.0, 100.0));
    let id = loom_core::ModuleId(*app.selection.iter().next().unwrap());
    let gate = note.display_index("gate").unwrap();
    let wait_for = move |app: &App, value: f32| {
        (0..400).any(|_| {
            std::thread::sleep(Duration::from_millis(5));
            app.controller.display(id, gate) == Some(value)
        })
    };
    (app, wait_for)
}

/// An input plugged in while the app runs is opened without opening
/// Preferences, and plays; one in `midi.off` is not opened.
#[test]
fn an_input_plugged_in_is_opened_unless_it_is_off() {
    let dir = folder("plug");
    write(&dir, "(midi: (off: [\"Drums\"]))");
    let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &[]));
    let (mut app, wait_for) = note_app(&dir, &fake);
    assert!(app.rig.ports.is_empty());
    app.preferences(Message::Open);
    fake.plug("Keys");
    fake.plug("Drums");
    app.poll_midi();
    assert_eq!(app.open_midi_inputs(), ["Keys"]);
    let listed = &app.rig.window.as_ref().unwrap().midi;
    assert_eq!(
        listed.as_deref(),
        Ok(&["Keys".to_string(), "Drums".to_string()][..]),
        "the window follows"
    );
    assert_eq!(app.alerts.status(), "MIDI: \"Keys\" connected.");
    fake.play("Keys", &[0x90, 60, 100]);
    assert!(wait_for(&app, 1.0), "a note on it plays");
}

/// Unplugging an input with a held note releases it in the engine, and
/// the app keeps running. The input comes back as a new connection,
/// and notes play once.
#[test]
fn an_input_unplugged_releases_its_notes_and_comes_back() {
    let dir = folder("unplug");
    let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Keys"]));
    let (mut app, wait_for) = note_app(&dir, &fake);
    fake.play("Keys", &[0x90, 60, 100]);
    assert!(wait_for(&app, 1.0));
    fake.unplug("Keys");
    app.poll_midi();
    assert!(wait_for(&app, 0.0), "the held note is released");
    assert_eq!(app.alerts.status(), "MIDI: \"Keys\" disconnected.");
    assert!(!fake.is_open("Keys"));
    assert!(app.stream.is_playing(), "the app keeps playing");

    fake.plug("Keys");
    app.poll_midi();
    assert_eq!(fake.open_count(), 1, "one connection, not two");
    fake.play("Keys", &[0x90, 60, 100]);
    assert!(wait_for(&app, 1.0));
    fake.play("Keys", &[0x80, 60, 0]);
    assert!(wait_for(&app, 0.0), "one note off ends it: it was not doubled");
}

/// Two inputs hold the same note. Removing one keeps the note until the
/// other lets go.
#[test]
fn a_shared_note_ends_with_its_last_input() {
    let dir = folder("shared-note");
    let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Keys", "Pad"]));
    let (mut app, wait_for) = note_app(&dir, &fake);
    fake.play("Keys", &[0x90, 60, 100]);
    fake.play("Pad", &[0x90, 60, 100]);
    assert!(wait_for(&app, 1.0));
    fake.unplug("Keys");
    app.poll_midi();
    assert!(!wait_for(&app, 0.0), "the pad still holds the note");
    fake.play("Pad", &[0x80, 60, 0]);
    assert!(wait_for(&app, 0.0));
}

/// A quick replug between two polls, with a new port identity, counts as
/// a removal: the old connection closes, its note is released, and the
/// new one plays. Ten quick cycles leak nothing and leave no stuck note.
#[test]
fn a_quick_replug_is_a_new_connection() {
    let dir = folder("replug");
    let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Keys"]));
    let (mut app, wait_for) = note_app(&dir, &fake);
    fake.play("Keys", &[0x90, 60, 100]);
    assert!(wait_for(&app, 1.0));
    fake.unplug("Keys");
    fake.plug("Keys");
    app.poll_midi();
    assert!(wait_for(&app, 0.0), "the old connection's note is released");
    assert_eq!(fake.open_count(), 1);
    for _ in 0..10 {
        fake.play("Keys", &[0x90, 62, 100]);
        fake.unplug("Keys");
        app.poll_midi();
        fake.plug("Keys");
        app.poll_midi();
    }
    assert!(wait_for(&app, 0.0), "no stuck note");
    assert_eq!(fake.open_count(), 1, "no leaked connection");
    assert_eq!(app.open_midi_inputs(), ["Keys"]);
    fake.play("Keys", &[0x90, 64, 100]);
    assert!(wait_for(&app, 1.0), "and it still plays");
}

/// Two inputs with the same name are told apart by order.
#[test]
fn same_named_inputs_are_numbered() {
    let dir = folder("same-names");
    let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Pad", "Pad"]));
    let app = start(args(&dir), &fake);
    assert_eq!(app.open_midi_inputs(), ["Pad", "Pad (2)"]);
}

/// An input that fails to open is reported, at start, when it is
/// plugged in, and when it is turned on, and the others still play. A
/// poll that finds nothing new says nothing, and a list that cannot be
/// read changes nothing.
#[test]
fn inputs_that_fail_are_reported_and_the_others_play() {
    let dir = folder("broken-midi");
    let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Keys", "Busy"]));
    fake.broken_midi.borrow_mut().push("Busy".into());
    fake.broken_midi.borrow_mut().push("Late".into());
    let mut app = start(args(&dir), &fake);
    assert_eq!(app.open_midi_inputs(), ["Keys"]);
    assert!(
        app.alerts.status().contains("Busy: the port is busy"),
        "{}",
        app.alerts.status()
    );

    app.alerts.say(String::new());
    app.poll_midi();
    assert!(
        app.alerts.status().is_empty(),
        "nothing changed: {}",
        app.alerts.status()
    );
    fake.plug("Late");
    app.poll_midi();
    assert!(
        app.alerts.status().contains("could not open \"Late\""),
        "{}",
        app.alerts.status()
    );
    assert_eq!(app.open_midi_inputs(), ["Keys"]);

    app.preferences(Message::Midi("Busy".into(), true));
    assert!(app.alerts.status().starts_with("MIDI: Busy"), "{}", app.alerts.status());
    assert_eq!(app.open_midi_inputs(), ["Keys"]);

    fake.midi_list_fails.set(true);
    fake.unplug("Keys");
    app.poll_midi();
    assert_eq!(app.open_midi_inputs(), ["Keys"], "an unreadable list closes nothing");
}

/// When the MIDI inputs cannot be listed at start, the status says so.
#[test]
fn an_unreadable_input_list_at_start_is_reported() {
    let dir = folder("no-midi-service");
    let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Keys"]));
    fake.midi_list_fails.set(true);
    let app = start(args(&dir), &fake);
    assert!(app.rig.ports.is_empty());
    assert!(
        app.alerts.status().contains("the MIDI service is not running"),
        "{}",
        app.alerts.status()
    );
}

/// The Panic section of the window: Learn shows it is waiting and turns
/// into Cancel, a second press cancels, and nothing arrived means no
/// trigger. The window draws in each state.
#[test]
fn the_panic_section_learns_and_cancels() {
    let dir = folder("panic-window");
    let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Pad"]));
    let mut app = start(args(&dir), &fake);
    app.preferences(Message::Open);
    assert!(app.preferences_view().is_some());
    assert_eq!(app.panic_line(), "None");
    app.preferences(Message::PanicLearn);
    assert!(app.preferences_view().is_some());
    let _ = app.update(crate::Message::Midi);
    assert_eq!(app.panic_line(), "Waiting for MIDI...", "no MIDI came yet");
    app.preferences(Message::PanicLearn);
    assert!(app.alerts.status().contains("cancelled"), "{}", app.alerts.status());
    assert_eq!(app.panic_line(), "None");

    // A note on is learned too.
    app.preferences(Message::PanicLearn);
    fake.play("Pad", &[0x99, 36, 100]);
    assert!(
        (0..400).any(|_| {
            std::thread::sleep(Duration::from_millis(5));
            app.controller.last_note_on().is_some()
        }),
        "the note reached the engine"
    );
    let _ = app.update(crate::Message::Midi);
    assert_eq!(app.panic_line(), "Channel 10, note 36");
    assert!(app.preferences_view().is_some());

    // Closing the window cancels a learn, so a later note on is not
    // saved as the trigger where the player cannot see it.
    app.preferences(Message::PanicLearn);
    app.preferences(Message::Close);
    assert_eq!(app.rig.panic_learn, None, "closing cancels the learn");
}

/// The same check through real MIDI inputs: two virtual controllers,
/// opened by `midir` (audio-qa.md, Virtual MIDI controllers). It needs the operating
/// system's MIDI service, so it is ignored by default: run it with
/// `cargo test -p loom-app -- --ignored` on macOS, or on Linux with the
/// ALSA sequencer.
#[test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[ignore = "needs virtual MIDI ports (macOS, or Linux with ALSA)"]
fn turning_a_real_input_off_releases_its_notes_and_is_saved() {
    use loom_host_desktop::VirtualController;
    let dir = folder("real-midi");
    let run = format!("Loom test {}", std::process::id());
    // Without a MIDI service (a container with no ALSA sequencer), skip.
    let create = |name: String| match VirtualController::create(&name) {
        Ok(port) => Some(port),
        Err(e) if e.is_missing_hardware() => {
            eprintln!("skipped: {e} (docs/dev/test-devices.md)");
            None
        }
        Err(e) => panic!("{e}"),
    };
    let (Some(mut pad), Some(mut keys)) = (create(format!("{run} Pad")), create(format!("{run} Keys"))) else {
        return;
    };
    let (pad_name, keys_name) = (pad.input_name().unwrap(), keys.input_name().unwrap());
    let fake = Rc::new(Fake {
        real_midi: Some(run),
        ..Fake::new(&[fake::device("Fake A")], &[])
    });
    let mut app = start(args(&dir), &fake);
    let open = |app: &App| app.rig.ports.iter().map(|p| p.name().to_string()).collect::<Vec<_>>();
    assert_eq!(open(&app).len(), 2, "both ports open: {:?}", open(&app));

    let note = loom_modules::registry().resolve("loom.midi.note").unwrap();
    app.add_module(note, iced::Point::new(100.0, 100.0));
    let id = loom_core::ModuleId(*app.selection.iter().next().unwrap());
    let gate = app
        .controller
        .graph()
        .node(id)
        .unwrap()
        .descriptor
        .display_index("gate")
        .unwrap();
    let wait_for = |app: &App, value: f32| {
        (0..400).any(|_| {
            std::thread::sleep(Duration::from_millis(5));
            app.controller.display(id, gate) == Some(value)
        })
    };
    pad.note_on(0, 60, 100).unwrap();
    assert!(wait_for(&app, 1.0), "the note plays");

    app.preferences(Message::Midi(pad_name.clone(), false));
    assert!(wait_for(&app, 0.0), "and is released, though the key is held");
    assert!(pad.holds_notes());
    assert_eq!(open(&app), std::slice::from_ref(&keys_name));
    assert_eq!(
        crate::preferences::load(&dir.join(FILE_NAME)).preferences.midi.off,
        [pad_name]
    );
    pad.note_on(0, 64, 100).unwrap();
    assert!(!wait_for(&app, 1.0), "the input that is off does not play");
    keys.note_on(0, 62, 100).unwrap();
    assert!(wait_for(&app, 1.0), "the other input still plays");
    assert!(app.stream.is_playing(), "audio did not stop");
}

#[test]
fn inputs_that_are_off_are_not_opened() {
    let dir = folder("midi-off");
    write(&dir, "(midi: (off: [\"Pad\"]))");
    let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Pad", "Keys"]));
    let _app = start(args(&dir), &fake);
    assert!(!fake.is_open("Pad"));
    assert!(fake.is_open("Keys"));
}

#[test]
fn midi_inputs_stay_open_across_a_new_patch() {
    let dir = folder("midi-new");
    let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Keys"]));
    let mut app = start(args(&dir), &fake);
    assert!(app.replace(Source::Empty));
    assert!(fake.is_open("Keys"));
    assert_eq!(fake.log.borrow().len(), 3, "opened once: {:?}", fake.log.borrow());
}

#[test]
fn the_device_flag_wins_for_this_run_only() {
    let dir = folder("flag");
    let path = write(&dir, "(audio: (device: Named(\"Fake B\")))");
    let fake = two_devices();
    let mut args = args(&dir);
    args.devices.device = Some("Fake A".into());
    let mut app = start(args, &fake);
    assert_eq!(fake.opened(), ["Fake A"]);
    assert_eq!(app.rig.shown_audio().device, DeviceChoice::Named("Fake A".into()));
    assert!(app.rig.flags.device.is_some(), "the window shows the flag note");
    app.quit = true;
    let _ = app.update(crate::Message::Tick);
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "(audio: (device: Named(\"Fake B\")))"
    );
}

#[test]
fn the_midi_flag_opens_matching_inputs_even_when_off() {
    let dir = folder("midi-flag");
    write(&dir, "(midi: (off: [\"Keys 2\"]))");
    let fake = Rc::new(Fake::new(&[fake::device("Fake A")], &["Pad", "Keys", "Keys 2"]));
    let mut args = args(&dir);
    args.devices.midi = Some("Keys".into());
    let _app = start(args, &fake);
    assert!(!fake.is_open("Pad"));
    assert!(fake.is_open("Keys") && fake.is_open("Keys 2"));
}

#[test]
fn the_saved_output_safety_applies_unless_a_flag_sets_it() {
    let dir = folder("safety");
    write(&dir, "(output_safety: \"clean\")");
    let fake = two_devices();
    let app = start(args(&dir), &fake);
    assert_eq!(app.controller.config().output_safety, OutputSafety::Clean);

    let mut flagged = args(&dir);
    flagged.output_safety = Some(OutputSafety::LowLatency);
    let app = start(flagged, &fake);
    assert_eq!(app.controller.config().output_safety, OutputSafety::LowLatency);
    assert!(app.rig.safety_flag);
}

/// The activity choice saves and follows newly built engines.
#[test]
fn activity_preference_survives_engine_change() {
    let dir = folder("activity");
    let path = write(&dir, "(show_activity: false)");
    let mut app = start(args(&dir), &two_devices());
    assert!(!app.rig.store.saved().show_activity);
    app.preferences(Message::Activity(true));
    assert!(app.rig.store.saved().show_activity);
    assert!(crate::preferences::load(&path).preferences.show_activity);
    app.preferences(Message::Activity(false));
    assert!(!crate::preferences::load(&path).preferences.show_activity);
    app.keep_engine_idle();
    assert!(!app.rig.store.saved().show_activity);
}

#[test]
fn the_safety_flag_is_shown_and_never_saved() {
    let dir = folder("safety-flag");
    let path = write(&dir, "(output_safety: \"clean\")");
    let mut flagged = args(&dir);
    flagged.output_safety = Some(OutputSafety::Off);
    let mut app = start(flagged, &two_devices());
    assert_eq!(app.controller.config().output_safety, OutputSafety::Off);
    assert_eq!(app.stream.output_safety(), OutputSafety::Off, "the window shows Off");
    assert!(app.rig.safety_flag, "with the flag note");
    app.quit = true;
    let _ = app.update(crate::Message::Tick);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "(output_safety: \"clean\")");
}

#[test]
fn a_failed_save_still_changes_the_mode() {
    let dir = folder("unwritable");
    // A file where the settings folder should be: nothing can be saved.
    std::fs::write(dir.join("blocked"), "").unwrap();
    let mut args = args(&dir);
    args.devices.preferences = Some(dir.join("blocked").join(FILE_NAME));
    let mut app = start(args, &two_devices());
    app.preferences(Message::Safety(OutputSafety::Clean));
    assert_eq!(app.rig.sent, [OutputSafety::Clean], "the engine runs Clean");
    assert!(
        app.alerts.status().ends_with("The change applies until you quit."),
        "{}",
        app.alerts.status()
    );
}

#[test]
fn picking_a_mode_tells_the_engine_and_saves_it() {
    let dir = folder("pick");
    let path = dir.join(FILE_NAME);
    let mut app = start(args(&dir), &two_devices());
    app.preferences(Message::Safety(OutputSafety::Off));
    assert_eq!(app.rig.sent, [OutputSafety::Off]);
    assert_eq!(
        crate::preferences::load(&path).preferences.output_safety,
        OutputSafety::Off
    );
    let written = std::fs::metadata(&path).unwrap().modified().unwrap();

    // The mode in effect: nothing sent, nothing written.
    std::thread::sleep(Duration::from_millis(20));
    app.preferences(Message::Safety(OutputSafety::Off));
    assert_eq!(app.rig.sent, [OutputSafety::Off]);
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), written);

    assert!(app.replace(Source::Empty));
    assert_eq!(
        app.controller.config().output_safety,
        OutputSafety::Off,
        "the next engine too"
    );
}

#[test]
fn a_broken_file_still_starts_with_safety_on() {
    let dir = folder("broken-file");
    let path = write(&dir, "(output_safety: \"off\"");
    let mut app = start(args(&dir), &two_devices());
    assert_eq!(app.controller.config().output_safety, OutputSafety::LowLatency);
    assert!(
        app.alerts.status().contains("Could not read preferences"),
        "{}",
        app.alerts.status()
    );
    app.preferences(Message::Safety(OutputSafety::Clean));
    assert_eq!(
        std::fs::read_to_string(dir.join("preferences.broken.ron")).unwrap(),
        "(output_safety: \"off\""
    );
    assert_eq!(
        crate::preferences::load(&path).preferences.output_safety,
        OutputSafety::Clean
    );
}

#[test]
fn with_no_audio_a_choice_is_only_saved() {
    let dir = folder("no-audio");
    let fake = two_devices();
    let mut app = start(
        Args {
            no_audio: true,
            ..args(&dir)
        },
        &fake,
    );
    assert_eq!(app.playing_line(), "Audio is off for this run (--no-audio).");
    change(&mut app, Change::Device(DeviceChoice::Named("Fake B".into())));
    assert!(fake.opened().is_empty());
    assert_eq!(
        crate::preferences::load(&dir.join(FILE_NAME)).preferences.audio.device,
        DeviceChoice::Named("Fake B".into())
    );
}

#[test]
fn the_window_opens_and_closes_with_its_shortcut_and_escape() {
    use iced::keyboard::{Event, Key, Modifiers, key::Named};
    let mut app = start(args(&folder("keys")), &two_devices());
    let press = |key: Key, modifiers: Modifiers| {
        crate::Message::Key(Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: iced::keyboard::key::Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
            location: iced::keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    };
    let _ = app.update(press(Key::Character(",".into()), Modifiers::COMMAND));
    assert!(app.rig.window.is_some());
    let _ = app.update(press(Key::Named(Named::Escape), Modifiers::empty()));
    assert!(app.rig.window.is_none());
}

/// unwrap_reopen_error strips the restart wrapper while preserving raw reasons.
#[test]
fn unwrap_reopen_error_normalizes_various_formats() {
    assert_eq!(
        App::unwrap_reopen_error("Could not use \"Fake B\": the stream did not start. Audio is off."),
        "the stream did not start"
    );
    assert_eq!(
        App::unwrap_reopen_error("Could not use \"Fake A\": the device is busy. Audio is off."),
        "the device is busy"
    );
    assert_eq!(
        App::unwrap_reopen_error("the stream did not start."),
        "the stream did not start"
    );
    assert_eq!(
        App::unwrap_reopen_error("the stream did not start"),
        "the stream did not start"
    );
    assert_eq!(
        App::unwrap_reopen_error("no audio output device found"),
        "no audio output device found"
    );
}

/// When the initial device fails to open at boot, the app reports the error and preserves the idle engine.
#[test]
fn boot_open_failure_leaves_engine_idle_and_reports_error() {
    let dir = folder("boot-open-fail");
    let fake = Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &[]);
    fake.broken.borrow_mut().push("Fake A".into());
    let fake = Rc::new(fake);
    let app = start(args(&dir), &fake);
    assert!(!app.stream.is_playing());
    assert!(app.stream.is_idle());
    assert!(
        app.stream.label().contains("the device is busy"),
        "audio_name: {:?}",
        app.stream.label()
    );
}

/// AudioPoll subscription is active when audio is enabled, and Message::AudioPoll triggers poll_audio.
#[test]
fn audio_poll_subscription_and_message_handling() {
    let dir = folder("audio-poll-sub");
    let fake = two_devices();
    let mut app = start(args(&dir), &fake);
    let _ = app.subscription();
    let _ = app.update(crate::Message::AudioPoll);
}

/// check_audio returns early when no_audio or fake_audio is active.
#[test]
fn check_audio_returns_early_when_audio_disabled() {
    let dir = folder("check-audio-disabled");
    let fake = two_devices();
    let mut app = start(args(&dir), &fake);
    app.stream.set_playback(crate::stream::Playback::Off);
    app.check_audio(Instant::now());
    app.stream.set_playback(crate::stream::Playback::Device);
    app.stream.set_playback(crate::stream::Playback::Fake);
    app.check_audio(Instant::now());
}

/// handle_audio_loss guards against redundant rebuilds and inactive audio.
#[test]
fn handle_audio_loss_guards_and_named_fallback() {
    let dir = folder("handle-audio-loss-guards");
    write(&dir, "(audio: (device: Named(\"Fake A\")))");
    let fake = two_devices();
    let mut app = start(args(&dir), &fake);

    // When audio is running on a named device, loss puts it in lost_audio
    app.handle_audio_loss("initial");
    assert!(app.rig.lost_audio.is_some());

    // Second call while lost_audio is Some returns early without duplicate handling (Rule 3)
    app.handle_audio_loss("duplicate");
    assert!(app.rig.lost_audio.is_some());

    // When audio was never playing and not lost, return early
    app.rig.lost_audio = None;
    app.rig.playing = None;
    assert!(!app.stream.is_playing());
    app.handle_audio_loss("ignored");
    assert!(app.rig.lost_audio.is_none());

    // When _audio is present but playing is None, falls back to saved device name
    let output = loom_host_desktop::AudioOutput::clock(48_000, 256);
    let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    let running = output.start(parts.engine).unwrap();
    app.stream.set_running(Some(running));
    let mut saved = app.rig.store.saved().clone();
    saved.audio.device = DeviceChoice::Named("Saved Device".into());
    app.rig.save(saved);
    app.handle_audio_loss("unplugged");
    assert_eq!(app.rig.lost_audio.as_ref().unwrap().device, "Saved Device");

    // When _audio is present, playing is None, and saved is SystemDefault, falls back to "audio device"
    let output2 = loom_host_desktop::AudioOutput::clock(48_000, 256);
    let parts2 = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    app.stream.set_running(Some(output2.start(parts2.engine).unwrap()));
    app.rig.lost_audio = None;
    let mut saved2 = app.rig.store.saved().clone();
    saved2.audio.device = DeviceChoice::SystemDefault;
    app.rig.save(saved2);
    fake.outputs.borrow_mut().default = None;
    app.handle_audio_loss("unplugged");
    assert_eq!(app.rig.lost_audio.as_ref().unwrap().device, "audio device");

    // When check_audio runs sooner than 1s after attempt, it waits without retrying
    let recent = Instant::now() + Duration::from_millis(100);
    app.check_audio(recent);
    assert_eq!(app.rig.lost_audio.as_ref().unwrap().attempts, 0);
}

/// The status bar shows what a performer needs: the developer tools
/// only with `--dev`, and MIDI inputs as a count, so four inputs fit
/// in a 1280 px window (visual-system.md, Status bar).
#[test]
fn the_status_bar_counts_midi_and_hides_developer_tools() {
    for dev in [false, true] {
        let dir = folder(if dev { "status-dev" } else { "status-plain" });
        let fake = Rc::new(Fake::new(
            &[fake::device("Fake A")],
            &[
                "Long Keyboard Name MIDI 1",
                "Long Keyboard Name DAW",
                "Pad Controller Port",
                "Pedal Board",
            ],
        ));
        let app = start(Args { dev, ..args(&dir) }, &fake);
        assert_eq!(app.rig.ports.len(), 4);
        let mut ui = crate::ui_test::Ui::with_app(app);
        let midi = ui.text("MIDI: 4 inputs").expect("the MIDI count");
        assert!(
            midi.x + midi.width <= 1280.0,
            "the MIDI count ends at {}",
            midi.x + midi.width
        );
        assert!(ui.text("Long Keyboard Name DAW").is_none(), "names only in hover text");
        // Switch labels are not text widgets, so look for the frame
        // summary, which shows with the Measure and Move camera switches.
        assert_eq!(
            ui.text("redraws on change").is_some(),
            dev,
            "developer tools with --dev only"
        );
    }
}

/// When the preferences file cannot be written, each choice (activity,
/// device, panic trigger, audio input, MIDI input) still applies to this
/// run, and the status line says why it was not saved.
#[test]
fn failed_saves_say_why_and_keep_the_choice() {
    let dir = folder("unwritable-choices");
    std::fs::write(dir.join("blocked"), "").unwrap();
    let mut args = args(&dir);
    args.devices.preferences = Some(dir.join("blocked").join(FILE_NAME));
    let fake = Rc::new(Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &["Pad"]));
    *fake.audio_inputs.borrow_mut() = devices::Inputs {
        default: Some("Microphone".into()),
        devices: vec!["Microphone".into()],
    };
    let mut app = start(args, &fake);
    let unsaved = |app: &App| app.alerts.status().ends_with("The change applies until you quit.");

    app.alerts.say(String::new());
    let shown = app.rig.store.saved().show_activity;
    app.preferences(Message::Activity(!shown));
    assert!(unsaved(&app), "{}", app.alerts.status());

    app.alerts.say(String::new());
    change(&mut app, Change::Device(DeviceChoice::Named("Fake B".into())));
    assert!(unsaved(&app), "{}", app.alerts.status());

    app.alerts.say(String::new());
    app.set_panic_pref(Some(crate::preferences::PanicPref::Cc(0, 123)));
    assert!(unsaved(&app), "{}", app.alerts.status());
    app.alerts.say(String::new());
    app.preferences(Message::PanicClear);
    assert!(unsaved(&app), "{}", app.alerts.status());
    assert_eq!(app.rig.store.saved().panic, None, "cleared for this run");

    // A patch that uses the input opens it, so turning it off has
    // something to close.
    app.controller.add_module("loom.io.audio-in").unwrap();
    assert!(app.commit());
    assert_eq!(fake.input_opens.get(), 1);
    app.alerts.say(String::new());
    app.preferences(Message::Input(InputChoice::Off));
    assert!(unsaved(&app), "{}", app.alerts.status());
    assert_eq!(app.rig.store.saved().audio.input, InputChoice::Off);
    assert_eq!(fake.input_closes.get(), 1, "closed for this run");
    assert!(app.rig.input.is_none());

    app.alerts.say(String::new());
    assert!(fake.is_open("Pad"), "every MIDI input opens when none is saved");
    app.preferences(Message::Midi("Pad".to_string(), false));
    assert!(unsaved(&app), "{}", app.alerts.status());
    assert!(!fake.is_open("Pad"), "closed for this run");
}
