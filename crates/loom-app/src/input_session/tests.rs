use super::*;
use crate::devices::fake::{Fake, Shared};
use loom_core::{EngineConfig, EngineParts, OutputSafety, create_engine};
use std::rc::Rc;

/// A session with a fake host and a real engine that nothing plays, so a
/// test decides when its command queue drains.
struct Bench {
    session: InputSession,
    fake: Rc<Fake>,
    host: Shared,
    parts: EngineParts,
    alerts: Alerts,
    choice: InputChoice,
    flag: Option<&'static str>,
    wanted: bool,
    output_playing: bool,
}

impl Bench {
    /// A machine with these input devices, the first the system default.
    fn new(devices: &[&str]) -> Self {
        let fake = Rc::new(Fake::new(&[], &[]));
        let bench = Self {
            session: InputSession::new(),
            host: Shared(fake.clone()),
            fake,
            parts: create_engine(loom_modules::registry(), EngineConfig::default()),
            alerts: Alerts::default(),
            choice: InputChoice::SystemDefault,
            flag: None,
            wanted: true,
            output_playing: true,
        };
        bench.plug(devices);
        bench
    }

    /// Replaces the machine's input devices.
    fn plug(&self, devices: &[&str]) {
        *self.fake.audio_inputs.borrow_mut() = crate::devices::Inputs {
            default: devices.first().map(|name| (*name).to_string()),
            devices: devices.iter().map(|name| (*name).to_string()).collect(),
        };
    }

    /// One sync, with no retry wait: tests do not sleep.
    fn sync(&mut self) {
        self.session.forget_attempt();
        self.sync_now();
    }

    /// One sync that honours the retry wait.
    fn sync_now(&mut self) {
        let demand = Demand {
            wanted: self.wanted,
            needed: self.wanted && self.output_playing,
            flag: self.flag,
            choice: &self.choice,
            buffer: Some(256),
            sample_rate: 48_000,
        };
        self.session
            .sync(demand, &self.host, &mut self.parts.controller, &mut self.alerts);
    }

    /// The open stream's device, if any.
    fn device(&self) -> Option<String> {
        self.session.port().map(|port| port.name().to_string())
    }

    /// Fails the open stream, as its error callback or the watcher would.
    fn fail_stream(&self) {
        self.session.port().expect("a stream is open").status().fail();
    }

    /// Fills the engine's command queue.
    fn fill_queue(&mut self) {
        while self.parts.controller.set_audio_input(None).is_ok() {}
    }

    /// Lets the engine read its command queue.
    fn drain_queue(&mut self) {
        let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
        self.parts.engine.process(&mut left, &mut right);
    }

    /// The status line.
    fn status(&self) -> &str {
        self.alerts.status()
    }
}

/// The stream opens on demand and closes when the demand goes.
#[test]
fn the_stream_follows_the_demand() {
    let mut bench = Bench::new(&["Mic"]);
    bench.wanted = false;
    bench.sync();
    assert_eq!(bench.fake.input_opens.get(), 0, "no permission prompt until needed");
    bench.wanted = true;
    bench.sync();
    assert_eq!(bench.device().as_deref(), Some("Mic"));
    assert_eq!(bench.status(), "", "a first open says nothing");
    bench.wanted = false;
    bench.sync();
    assert_eq!(bench.device(), None);
    assert_eq!(bench.fake.input_closes.get(), 1);
}

/// With no output buffer known yet, nothing opens and no attempt is used up.
#[test]
fn opening_waits_for_the_output_buffer() {
    let mut bench = Bench::new(&["Mic"]);
    let demand = Demand {
        wanted: true,
        needed: true,
        flag: None,
        choice: &bench.choice,
        buffer: None,
        sample_rate: 48_000,
    };
    bench
        .session
        .sync(demand, &bench.host, &mut bench.parts.controller, &mut bench.alerts);
    assert_eq!(bench.device(), None);
    bench.sync_now();
    assert_eq!(
        bench.device().as_deref(),
        Some("Mic"),
        "the buffer arrived: open at once"
    );
}

/// A failed open is reported once, is not retried within a second, and the
/// retry opens the stream with a note that it is back.
#[test]
fn a_failed_open_is_reported_once_and_retried_after_a_wait() {
    let mut bench = Bench::new(&["Mic"]);
    *bench.fake.input_error.borrow_mut() = Some(InputError::Other("device busy".into()));
    bench.sync();
    assert_eq!(bench.status(), "Audio input: audio error: device busy.");

    *bench.fake.input_error.borrow_mut() = None;
    bench.sync_now();
    assert_eq!(bench.device(), None, "the second attempt waits");

    bench.sync();
    assert_eq!(bench.device().as_deref(), Some("Mic"));
    assert_eq!(bench.status(), "Audio input: \"Mic\" is back.");
}

/// Every failed attempt after the first is quiet, so newer messages stay.
#[test]
fn repeated_failures_keep_newer_messages() {
    let mut bench = Bench::new(&["Mic"]);
    *bench.fake.input_error.borrow_mut() = Some(InputError::Other("device busy".into()));
    bench.sync();
    bench.alerts.say("Mapped CC 74");
    bench.sync();
    assert_eq!(bench.status(), "Mapped CC 74");
    *bench.fake.input_error.borrow_mut() = None;
    bench.sync();
    assert!(bench.device().is_some());
    assert_eq!(bench.status(), "Mapped CC 74", "recovery replaces only its own error");
}

/// Each loss in a run is reported, and each recovery replaces only its error.
#[test]
fn repeated_loss_and_recovery_are_each_reported() {
    let mut bench = Bench::new(&["Mic"]);
    bench.sync();
    for round in 1..=3 {
        *bench.fake.input_error.borrow_mut() = Some(InputError::Other("device busy".into()));
        bench.fail_stream();
        bench.sync();
        assert_eq!(bench.device(), None);
        assert_eq!(bench.status(), "Audio input: \"Mic\" was lost. Trying to reopen it.");
        *bench.fake.input_error.borrow_mut() = None;
        bench.sync();
        assert_eq!(bench.status(), "Audio input: \"Mic\" is back.");
        assert_eq!(bench.fake.input_closes.get(), round);
    }
}

/// A lost named input is not replaced by the default: it waits for that
/// device, and the saved choice is what it reopens.
#[test]
fn a_lost_named_input_waits_for_that_device() {
    let mut bench = Bench::new(&["Other", "Mic"]);
    bench.choice = InputChoice::Named("Mic".into());
    bench.sync();
    assert_eq!(bench.device().as_deref(), Some("Mic"));

    bench.plug(&["Other"]);
    bench.fail_stream();
    bench.sync();
    assert_eq!(bench.device(), None);
    assert!(bench.session.named_lost());
    bench.sync();
    assert_eq!(bench.device(), None, "the default does not stand in");

    bench.plug(&["Other", "Mic"]);
    bench.sync();
    assert_eq!(bench.device().as_deref(), Some("Mic"));
    assert!(!bench.session.named_lost());
    assert_eq!(bench.status(), "Audio input: \"Mic\" is back.");
}

/// A saved device missing at start falls back for this run, with a note, and
/// the choice given to the session is left as it was.
#[test]
fn a_named_input_missing_at_start_falls_back() {
    let mut bench = Bench::new(&["Other"]);
    bench.choice = InputChoice::Named("Mic".into());
    bench.sync();
    assert_eq!(bench.device().as_deref(), Some("Other"));
    assert_eq!(
        bench.status(),
        "Audio input: \"Mic\" is not available. Using the system default."
    );
    assert!(!bench.session.named_lost());
}

/// The system default has no device to wait for: a loss reopens on whichever
/// device is the default now.
#[test]
fn a_lost_default_input_moves_to_the_new_default() {
    let mut bench = Bench::new(&["Mic", "Other"]);
    bench.sync();
    bench.plug(&["Other"]);
    bench.fail_stream();
    bench.sync();
    assert_eq!(bench.device().as_deref(), Some("Other"));
    assert!(!bench.session.named_lost());
    assert_eq!(bench.status(), "Audio input: \"Other\" is back.");
}

/// A lost `--input` device is waited for by part of its name, not replaced.
#[test]
fn a_lost_flag_input_is_opened_again_by_its_name() {
    let mut bench = Bench::new(&["Other", "My Mic"]);
    bench.flag = Some("mic");
    bench.sync();
    assert_eq!(bench.device().as_deref(), Some("My Mic"));
    bench.plug(&["Other"]);
    bench.fail_stream();
    bench.sync();
    assert_eq!(bench.device(), None);
    assert!(bench.status().contains("was lost"));
    bench.plug(&["Other", "My Mic"]);
    bench.sync();
    assert_eq!(bench.device().as_deref(), Some("My Mic"));
}

/// A refusal closes the stream and shows once per run, across a return of
/// access and a second refusal. Choosing an input lets it show again.
#[test]
fn a_refusal_shows_once_until_an_input_is_chosen() {
    let mut bench = Bench::new(&["Mic"]);
    bench.sync();
    bench.fake.input_permission_denied.set(true);
    bench.sync();
    assert_eq!(bench.device(), None);
    assert!(bench.status().contains("microphone was denied"));

    bench.alerts.say("newer");
    bench.fake.input_permission_denied.set(false);
    bench.sync();
    assert!(bench.device().is_some(), "access came back");
    assert_eq!(bench.status(), "newer");

    bench.fake.input_permission_denied.set(true);
    bench.sync();
    assert_eq!(bench.device(), None);
    assert_eq!(bench.status(), "newer", "once per run");

    bench.session.choose(&mut bench.parts.controller);
    bench.sync();
    assert!(bench.status().contains("microphone was denied"));
}

/// A refusal is reported after a device error, because each has its own
/// latch, and a refusal from opening counts as the same refusal.
#[test]
fn a_refusal_after_a_device_error_is_still_reported() {
    let mut bench = Bench::new(&["Mic"]);
    *bench.fake.input_error.borrow_mut() = Some(InputError::Other("device busy".into()));
    bench.sync();
    *bench.fake.input_error.borrow_mut() = Some(InputError::PermissionDenied);
    bench.sync();
    assert!(bench.status().contains("microphone was denied"));
    bench.alerts.say("newer");
    bench.fake.input_permission_denied.set(true);
    bench.sync();
    assert_eq!(bench.status(), "newer");
}

/// A stopped output drops the stream, and a lost named input is still waited
/// for on the next one. Choosing an input ends the wait.
#[test]
fn a_stopped_output_keeps_the_wait_for_a_named_input() {
    let mut bench = Bench::new(&["Other", "Mic"]);
    bench.choice = InputChoice::Named("Mic".into());
    bench.sync();
    bench.plug(&["Other"]);
    bench.fail_stream();
    bench.sync();
    assert!(bench.session.named_lost());
    bench.session.stream_stopped();
    assert!(bench.session.named_lost());

    // The audio poll syncs while the output is stopped.
    bench.output_playing = false;
    bench.sync();
    assert!(bench.session.named_lost());

    bench.output_playing = true;
    bench.sync();
    assert_eq!(bench.device(), None, "the default does not stand in");

    bench.session.choose(&mut bench.parts.controller);
    assert!(!bench.session.named_lost());
    bench.sync();
    assert_eq!(bench.device().as_deref(), Some("Other"), "the fallback applies again");
}

/// Removing the last Audio In ends the wait, whether or not the output plays:
/// the player took the input away, so a later one starts fresh and may fall
/// back to the system default.
#[test]
fn removing_the_last_audio_in_ends_the_wait() {
    for output_playing in [true, false] {
        let mut bench = Bench::new(&["Other", "Mic"]);
        bench.choice = InputChoice::Named("Mic".into());
        bench.sync();
        bench.plug(&["Other"]);
        bench.fail_stream();
        bench.sync();
        assert!(bench.session.named_lost());

        bench.output_playing = output_playing;
        bench.wanted = false;
        bench.sync();
        assert!(!bench.session.named_lost(), "output playing: {output_playing}");

        bench.wanted = true;
        bench.output_playing = true;
        bench.sync();
        assert_eq!(bench.device().as_deref(), Some("Other"));
    }
}

/// A stopped output closes an open stream and the next one reopens it, with
/// no message, because nothing went wrong.
#[test]
fn a_stopped_output_reopens_quietly() {
    let mut bench = Bench::new(&["Mic"]);
    bench.sync();
    bench.session.stream_stopped();
    assert_eq!(bench.device(), None);
    assert_eq!(bench.fake.input_closes.get(), 1);
    bench.sync();
    assert_eq!(bench.device().as_deref(), Some("Mic"));
    assert_eq!(bench.status(), "");
}

/// With the command queue full the stream cannot reach the engine, so it is
/// closed and tried again. There is no latch on this message: it repeats on
/// each retry until the queue drains, and then the input opens.
#[test]
fn a_full_queue_closes_the_new_stream_and_retries() {
    let mut bench = Bench::new(&["Mic"]);
    bench.fill_queue();
    bench.sync();
    assert_eq!(bench.device(), None);
    assert_eq!(bench.fake.input_opens.get(), 1);
    assert_eq!(bench.fake.input_closes.get(), 1, "the stream did not leak");
    assert!(bench.status().contains("queue is full"), "{}", bench.status());

    bench.alerts.say("newer");
    bench.sync();
    assert!(bench.status().contains("queue is full"), "the message repeats");

    bench.drain_queue();
    bench.alerts.say("newer");
    bench.sync();
    assert_eq!(bench.device().as_deref(), Some("Mic"));
    assert_eq!(bench.status(), "newer");
}

/// Closing with a full queue still closes the device. The engine never gets
/// the command, so it keeps its reader on the closed ring: Audio In then
/// reads silence, even with frames left in the ring, and no drift is counted.
#[test]
fn a_full_queue_does_not_stop_a_close() {
    let mut bench = Bench::new(&["Mic"]);
    let controller = &mut bench.parts.controller;
    let input = controller.add_module("loom.io.audio-in").unwrap();
    let output = controller.add_module("loom.io.audio-out").unwrap();
    controller.connect_named(input, "out", output, "in").unwrap();
    controller.commit().unwrap();
    bench.sync();
    bench.drain_queue();
    {
        let mut writer = bench.fake.input_writer.borrow_mut();
        let writer = writer.as_mut().expect("the stream is open");
        for _ in 0..256 {
            let _ = writer.push([0.5; loom_core::audio_input::MAX_INPUT_CHANNELS]);
        }
    }

    // Other commands fill the queue, so the engine's reader stays in place
    // when it drains.
    while bench.parts.controller.set_output_safety(OutputSafety::Off).is_ok() {}
    bench.wanted = false;
    bench.sync();
    assert_eq!(bench.device(), None);
    assert_eq!(bench.fake.input_closes.get(), 1);

    let (mut left, mut right) = ([1.0; 64], [1.0; 64]);
    for _ in 0..4 {
        bench.parts.engine.process(&mut left, &mut right);
        assert_eq!(left, [0.0; 64], "the input is silent");
    }
    let stats = bench.parts.controller.stats();
    assert_eq!(stats.input_underruns.load(Ordering::Relaxed), 0);
    assert_eq!(stats.input_overruns.load(Ordering::Relaxed), 0);
}

/// Each error category has its own status, and only a real refusal is called
/// one, whatever the backend's words say.
#[test]
fn input_error_categories_have_their_own_status() {
    let cases = [
        (InputError::PermissionDenied, "microphone was denied"),
        (
            InputError::Unavailable {
                device: Some("Mic".into()),
                detail: None,
            },
            "Audio input: no audio input device found matching `Mic`.",
        ),
        (
            InputError::UnsupportedRate {
                device: "Mic".into(),
                rate: 96_000,
            },
            "Audio input: \"Mic\" does not offer 96000 Hz.",
        ),
        (
            InputError::UnsupportedFormat("Mic".into()),
            "Audio input: audio device `Mic` does not support 32-bit float input.",
        ),
        (
            InputError::Other("permission denied by a plug-in".into()),
            "Audio input: audio error: permission denied by a plug-in.",
        ),
    ];
    for (error, text) in cases {
        assert!(input_error_message(&error).contains(text), "{error:?}");
    }
    assert!(!input_error_message(&InputError::Other("permission denied".into())).contains("was denied"));
}
