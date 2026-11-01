use super::fake::{NoMidi, device};
use super::*;

/// The system adapter opens, watches, and closes a real loopback input.
#[test]
#[ignore = "needs a loopback audio device"]
fn system_input_adapter_uses_loopback_device() {
    let Ok(wanted) = std::env::var("LOOM_LOOPBACK") else {
        eprintln!("skipped: set LOOM_LOOPBACK to the loopback device's name");
        return;
    };
    let inputs = System.audio_inputs().expect("list input devices");
    assert!(
        inputs
            .devices
            .iter()
            .any(|name| name.to_lowercase().contains(&wanted.to_lowercase()))
    );
    let listed = System.audio_input_lister().unwrap()().expect("list from watcher");
    assert_eq!(listed, inputs);
    assert_eq!(
        System.audio_input_permission_checker().unwrap()(),
        System.audio_input_permission_denied()
    );
    let missing = AudioOptions {
        device: Some("loom-input-device-that-does-not-exist".into()),
        exact: true,
        ..AudioOptions::default()
    };
    assert!(System.open_audio_input(&missing, 256).is_err());
    let options = AudioOptions {
        device: Some(wanted),
        exact: false,
        sample_rate: Some(48_000),
        buffer_size: Some(256),
    };
    let (input, _reader) = System.open_audio_input(&options, 256).expect("open loopback input");
    assert!(inputs.devices.contains(&input.name().to_string()));
    let status = input.status();
    let started = std::time::Instant::now();
    while status.callbacks() == 0 && started.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(input.is_live());
    assert!(status.callbacks() > 0, "the input device must call back");
    drop(input);
    assert!(!status.is_live(), "closing the stream makes the reader silent");
}

fn outputs(names: &[&str]) -> Outputs {
    Outputs {
        default: names.first().map(|n| n.to_string()),
        devices: names.iter().map(|n| device(n)).collect(),
    }
}

fn wanted(device: Pick) -> Wanted {
    Wanted {
        device,
        rate: None,
        buffer: None,
    }
}

#[test]
fn only_common_rates_in_the_range_are_offered() {
    let wide = OutputInfo {
        rates: vec![(8_000, 384_000)],
        ..device("Wide")
    };
    assert_eq!(offered_rates(&wide), COMMON_RATES);
    assert_eq!(
        rate_choices(&wide).len(),
        7,
        "Device default and the six common rates, and nothing else"
    );
    assert_eq!(offered_rates(&device("Small")), [44_100, 48_000]);
}

#[test]
fn buffers_are_powers_of_two_in_the_range() {
    assert_eq!(offered_buffers(&device("A")), [32, 64, 128, 256, 512, 1024]);
    let unreported = OutputInfo {
        buffer: None,
        ..device("B")
    };
    assert_eq!(offered_buffers(&unreported), [64, 128, 256, 512, 1024, 2048]);
    let huge = OutputInfo {
        buffer: Some((1, 1 << 20)),
        ..device("C")
    };
    assert_eq!(offered_buffers(&huge).first(), Some(&16));
    assert_eq!(offered_buffers(&huge).last(), Some(&4096));
    assert_eq!(buffer_time(128, 96_000), "1.3 ms");
}

#[test]
fn a_saved_device_plays_when_plugged_in() {
    let plan = plan(&outputs(&["Fake A", "Fake B"]), &wanted(Pick::Named("Fake B".into()))).unwrap();
    assert_eq!(plan.options.device.as_deref(), Some("Fake B"));
    assert!(plan.options.exact);
    assert!(plan.notes.is_empty());
}

#[test]
fn a_missing_saved_device_falls_back_to_the_default() {
    let plan = plan(&outputs(&["Fake A"]), &wanted(Pick::Named("Fake B".into()))).unwrap();
    assert_eq!(plan.options.device.as_deref(), Some("Fake A"));
    assert_eq!(
        plan.notes,
        ["Audio: \"Fake B\" is not connected. Playing on \"Fake A\". Choose a device in Preferences."]
    );
}

#[test]
fn a_rate_the_device_does_not_offer_falls_back() {
    let plan = plan(
        &outputs(&["Fake A"]),
        &Wanted {
            rate: Some(96_000),
            buffer: Some(100),
            ..wanted(Pick::SystemDefault)
        },
    )
    .unwrap();
    assert_eq!(plan.options.sample_rate, None);
    assert_eq!(plan.options.buffer_size, None);
    assert_eq!(
        plan.notes,
        [
            "Audio: \"Fake A\" does not offer 96000 Hz. Using 48000 Hz.",
            "Audio: \"Fake A\" does not offer 100 frames. Using the device default.",
        ]
    );
}

#[test]
fn the_device_flag_matches_part_of_a_name() {
    let list = outputs(&["Fake A", "MOTU M4"]);
    let plan = plan(&list, &wanted(Pick::Matching("motu".into()))).unwrap();
    assert_eq!(plan.options.device.as_deref(), Some("MOTU M4"));
    let error = super::plan(&list, &wanted(Pick::Matching("RME".into()))).unwrap_err();
    assert_eq!(error, "no audio output device matches \"RME\"");
}

#[test]
fn flags_win_over_the_saved_choice() {
    let saved = AudioPrefs {
        device: DeviceChoice::Named("Fake B".into()),
        input: crate::preferences::InputChoice::SystemDefault,
        sample_rate: RateChoice::Hz(44_100),
        buffer: BufferChoice::Frames(64),
    };
    let flags = Options {
        device: Some("Fake A".into()),
        buffer: Some(128),
        ..Options::default()
    };
    assert_eq!(
        Wanted::new(&saved, &flags),
        Wanted {
            device: Pick::Matching("Fake A".into()),
            rate: Some(44_100),
            buffer: Some(128),
        }
    );
}

#[test]
fn midi_inputs_at_start() {
    let inputs = ["Pad".to_string(), "Keys".to_string(), "Keys 2".to_string()];
    let off = ["Pad".to_string(), "Keys 2".to_string()];
    assert_eq!(midi_at_start(&inputs, &off, &Options::default()), ["Keys"]);
    let only_keys = Options {
        midi: Some("KEYS".into()),
        ..Options::default()
    };
    assert_eq!(midi_at_start(&inputs, &off, &only_keys), ["Keys", "Keys 2"]);
    let none = Options {
        no_midi: true,
        ..Options::default()
    };
    assert!(midi_at_start(&inputs, &[], &none).is_empty());
}

#[test]
fn the_playing_line() {
    let playing = Playing {
        device: "MOTU M4".into(),
        rate: 96_000,
        buffer: Some(128),
    };
    assert_eq!(
        playing.describe(None),
        "Playing on \"MOTU M4\" at 96000 Hz, 128 frames (1.3 ms)."
    );
    let chosen_by_device = Playing {
        buffer: None,
        ..playing
    };
    assert_eq!(
        chosen_by_device.describe(Some(512)),
        "Playing on \"MOTU M4\" at 96000 Hz, 512 frames (5.3 ms)."
    );
    assert_eq!(chosen_by_device.describe(None), "Playing on \"MOTU M4\" at 96000 Hz.");
}

/// Verifies that NoMidi host implements start by delegating to output.start.
#[test]
fn no_midi_host_start() {
    let host = NoMidi;
    let output = AudioOutput::clock(48_000, 256);
    let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    let started = host.start(output, parts.engine);
    assert!(started.is_ok());
}

/// Verifies that the default Host::start implementation handles outputs.
#[test]
fn host_start_default_implementation() {
    let output = AudioOutput::clock(48_000, 256);
    let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    let started = System.start(output, parts.engine);
    assert!(started.is_ok());
}

/// When outputs.default is None, add_device promotes the first device to default.
#[test]
fn fake_add_device_sets_default_when_none() {
    let fake = fake::Fake {
        outputs: std::cell::RefCell::new(Outputs {
            default: None,
            devices: Vec::new(),
        }),
        ..fake::Fake::new(&[], &[])
    };
    assert!(fake.outputs.borrow().default.is_none());
    fake.add_device(fake::device("USB Audio"));
    assert_eq!(fake.outputs.borrow().default.as_deref(), Some("USB Audio"));
}

/// NoMidi delegates outputs and open to System.
#[test]
fn no_midi_delegates_to_system() {
    let host = NoMidi;
    let _ = host.outputs();
    let options = AudioOptions {
        device: Some("nonexistent device that will fail to open".into()),
        exact: true,
        sample_rate: None,
        buffer_size: None,
    };
    let _ = host.open(&options);
}

/// The list the MIDI watch thread reads: one input, or two once the
/// test plugs one in.
static PLUGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn watched_list() -> Result<Vec<MidiInputInfo>, String> {
    let input = |name: &str| MidiInputInfo {
        name: name.into(),
        id: name.into(),
    };
    let mut list = vec![input("Pad")];
    if PLUGGED.load(std::sync::atomic::Ordering::Relaxed) {
        list.push(input("Keys"));
    }
    Ok(list)
}

/// The MIDI watch wakes the UI once when the list changes, and not
/// while it stays as the UI saw it (#90). A real host offers a lister;
/// the fake does not, so tests poll from the UI as before.
#[test]
fn the_midi_watch_wakes_the_ui_only_on_a_change() {
    use iced::futures::StreamExt;
    assert!(System.midi_lister().is_some());
    assert!(fake::NoMidi.midi_lister().is_none());
    let watch = MidiWatch {
        list: watched_list,
        seen: watched_list().unwrap(),
        generation: 0,
    };
    let mut rx = midi_stream(&watch);
    std::thread::sleep(MIDI_POLL * 3);
    assert!(rx.try_recv().is_err(), "no change, so nothing was sent");
    PLUGGED.store(true, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(iced::futures::executor::block_on(rx.next()), Some(()));
    std::thread::sleep(MIDI_POLL * 3);
    assert!(rx.try_recv().is_err(), "sent once per change");
}
