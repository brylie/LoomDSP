//! What the app plays, checked on a stepped output (`docs/design/audio-qa.md`,
//! Part 1, and the app test of Part 2). The fake devices play only when the
//! test steps the tape, so every check sees every sample, and the same test
//! gives the same tape.
//!
//! The app is driven through its own code paths: `App::boot_with` with real
//! `Args` and a preferences file, and the messages the window sends.

use super::*;
use crate::devices::fake::{self, Fake};
use crate::preferences::DeviceChoice;
use crate::settings::{Change, Message as Prefs};
use loom_core::safety::{CEILING, FADE_SECONDS};
use loom_host_desktop::{OutputInfo, Tape};
use loom_test::analysis::{max_step, peak};

/// The fake devices' sample rate and buffer size.
const RATE: usize = 48_000;

/// The length of the windows rule 4 compares with: 100 ms.
const WINDOW: usize = RATE / 10;

/// Frames in a fade: 10 ms.
fn fade_frames() -> usize {
    loom_core::safety::samples(FADE_SECONDS, RATE as f32)
}

/// A folder for one test, empty at the start.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("loom-qa-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An example patch from `examples/patches`.
fn example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/patches")
        .join(name)
}

/// Full-scale noise times 4 into both outputs.
const NOISE_GAIN_4: &str = r#"Patch(version: 1, name: "Noise gain 4", modules: [
    PatchModule(id: 1, kind: "loom.osc.noise@1"),
    PatchModule(id: 2, kind: "loom.math.mul@1", params: {"gain": 4.0}),
    PatchModule(id: 3, kind: "loom.io.audio-out@1"),
], cables: [
    PatchCable(from: (1, "out"), to: (2, "a")),
    PatchCable(from: (2, "out"), to: (3, "left")),
    PatchCable(from: (2, "out"), to: (3, "right")),
])"#;

/// A Constant of 1.0 into both outputs: DC.
const CONSTANT_ONE: &str = r#"Patch(version: 1, name: "Constant", modules: [
    PatchModule(id: 1, kind: "loom.math.constant@1", params: {"value": 1.0}),
    PatchModule(id: 2, kind: "loom.io.audio-out@1"),
], cables: [
    PatchCable(from: (1, "out"), to: (2, "left")),
    PatchCable(from: (1, "out"), to: (2, "right")),
])"#;

/// One test run: an app on fake devices that play on `tape`.
struct Run {
    app: App,
    tape: Tape,
    dir: PathBuf,
}

/// How a run starts.
struct Setup<'a> {
    name: &'a str,
    /// The patch text, written to a file in the test folder, or an example.
    patch: PatchFrom<'a>,
    /// `--output-safety`.
    flag: Option<OutputSafety>,
    /// The preferences file's text, if any.
    preferences: Option<&'a str>,
    /// `--record`, a file name in the test folder.
    record: Option<&'a str>,
    /// The devices; the first is the default.
    devices: Vec<OutputInfo>,
    /// Devices that open, then fail to start.
    wont_start: Vec<&'a str>,
}

/// Where a run's patch comes from.
enum PatchFrom<'a> {
    Text(&'a str),
    Example(&'a str),
}

impl<'a> Setup<'a> {
    fn new(name: &'a str, patch: PatchFrom<'a>) -> Self {
        Self {
            name,
            patch,
            flag: None,
            preferences: None,
            record: None,
            devices: vec![fake::device("Fake A"), fake::device("Fake B")],
            wont_start: Vec::new(),
        }
    }

    fn start(self) -> Run {
        let dir = folder(self.name);
        let patch = match self.patch {
            PatchFrom::Text(text) => {
                let path = dir.join("patch.ron");
                std::fs::write(&path, text).unwrap();
                path
            }
            PatchFrom::Example(name) => example(name),
        };
        let preferences = dir.join("preferences.ron");
        if let Some(text) = self.preferences {
            std::fs::write(&preferences, text).unwrap();
        }
        let tape = Tape::new();
        let host = Fake {
            tape: Some(tape.clone()),
            wont_start: self.wont_start.iter().map(|s| s.to_string()).collect(),
            ..Fake::new(&self.devices, &[])
        };
        let args = Args {
            patch: Some(patch),
            no_audio: false,
            no_recovery: true,
            output_safety: self.flag,
            record: self.record.map(|name| dir.join(name)),
            devices: devices::Options {
                preferences: Some(preferences),
                ..devices::Options::default()
            },
            ..tests::app_args()
        };
        let app = App::boot_with(args, &mut recovery::Dialogs, Rc::new(fake::Shared(Rc::new(host))));
        assert!(tape.is_playing(), "the patch plays: {}", app.status);
        Run { app, tape, dir }
    }
}

impl Run {
    /// Plays `seconds` more.
    fn play(&self, seconds: f64) {
        self.tape.step((seconds * RATE as f64) as usize);
    }

    /// Presses Hold note.
    fn hold(&mut self) {
        let _ = self.app.update(Message::HoldNote);
        assert!(self.app.holding, "the note is held");
    }

    /// Opens an example patch, as File > Open does.
    fn open(&mut self, name: &str) {
        assert!(self.app.replace(Source::File(&example(name))), "{}", self.app.status);
    }

    fn left(&self) -> Vec<f32> {
        self.tape.left()
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The largest step `samples` may have under rule 4, given the steady
/// audio it is compared with: that audio's largest step, plus one fade step
/// times its peak, because the fade gain moves too.
fn step_limit(steady: &[f32]) -> f32 {
    // Broken audio would make the limit infinite and pass every check.
    assert!(steady.iter().all(|x| x.is_finite()), "the steady audio is finite");
    max_step(steady) + peak(steady) / fade_frames() as f32 + 1e-6
}

/// Checks rule 4 at a switch: the app acted when the tape held `switch`
/// frames, and the new stream started at `start`. The old stream ends at
/// 0, the new one starts at 0, the fade-out has no step larger than the
/// 100 ms before the switch allows, and the fade-in none larger than the
/// 100 ms after it allows.
fn assert_no_click(left: &[f32], switch: usize, start: usize, what: &str) {
    assert!(switch >= WINDOW && start > switch, "{what}: the switch is on the tape");
    assert_eq!(left[start - 1], 0.0, "{what}: the old stream ends at 0");
    assert_eq!(left[start], 0.0, "{what}: the new stream starts at 0");
    assert!(start - switch <= RATE / 10 + 1024, "{what}: the fade-out is quick");

    let before = step_limit(&left[switch - WINDOW..switch]);
    let fade_out = max_step(&left[switch - 1..start]);
    assert!(fade_out <= before, "{what}: fade-out step {fade_out} > {before}");

    let fade_in_end = start + fade_frames();
    let after = step_limit(&left[fade_in_end..fade_in_end + WINDOW]);
    let fade_in = max_step(&left[start - 1..=fade_in_end]);
    assert!(fade_in <= after, "{what}: fade-in step {fade_in} > {after}");
}

/// Noise at gain 4 stays under the ceiling in Low latency and Clean, and
/// clips in Off, with the mode from `--output-safety`.
#[test]
fn the_flag_sets_the_mode_for_noise_at_gain_4() {
    for mode in [OutputSafety::LowLatency, OutputSafety::Clean, OutputSafety::Off] {
        let run = Setup {
            flag: Some(mode),
            ..Setup::new("noise-flag", PatchFrom::Text(NOISE_GAIN_4))
        }
        .start();
        run.play(1.0);
        check_noise(&run.left(), mode);
    }
}

/// The same with the mode from the preferences file.
#[test]
fn the_preference_sets_the_mode_for_noise_at_gain_4() {
    for mode in [OutputSafety::LowLatency, OutputSafety::Clean, OutputSafety::Off] {
        let text = format!("(output_safety: \"{}\")", mode.name());
        let run = Setup {
            preferences: Some(&text),
            ..Setup::new("noise-prefs", PatchFrom::Text(NOISE_GAIN_4))
        }
        .start();
        run.play(1.0);
        check_noise(&run.left(), mode);
    }
}

/// Noise at gain 4: under the ceiling and steady, or clipped in Off.
fn check_noise(left: &[f32], mode: OutputSafety) {
    let level = peak(left);
    match mode {
        OutputSafety::Off => {
            assert!(left.iter().any(|x| x.abs() >= 1.0), "Off clips");
        }
        _ => {
            assert!(level <= CEILING, "{mode}: peak {level} over the ceiling");
            // Steady: every 100 ms after the fade-in comes close to the ceiling.
            for window in left[fade_frames()..].as_chunks::<WINDOW>().0 {
                assert!(peak(window) > 0.8 * CEILING, "{mode}: the level dropped");
            }
        }
    }
}

/// A Constant of 1.0 is removed in Low latency and Clean, and held in Off.
#[test]
fn dc_through_the_app() {
    for mode in [OutputSafety::LowLatency, OutputSafety::Clean, OutputSafety::Off] {
        let run = Setup {
            flag: Some(mode),
            ..Setup::new("dc", PatchFrom::Text(CONSTANT_ONE))
        }
        .start();
        run.play(1.0);
        let left = run.left();
        let tail = &left[RATE / 2..];
        match mode {
            OutputSafety::Off => assert!(tail.iter().all(|&x| x == 1.0), "Off holds 1.0"),
            _ => assert!(peak(tail) < 0.001, "{mode}: DC left: {}", peak(tail)),
        }
    }
}

/// The Open test: sine, then fm-feedback, then sine, each with a held note.
/// Returns the tape.
fn open_sine_fm_sine(mode: OutputSafety) -> Vec<f32> {
    let mut run = Setup {
        flag: Some(mode),
        ..Setup::new("open", PatchFrom::Example("sine.ron"))
    }
    .start();
    run.hold();
    run.play(0.5);
    for (i, name) in ["fm-feedback.ron", "sine.ron"].into_iter().enumerate() {
        let switch = run.tape.len();
        run.open(name);
        run.hold();
        run.play(0.5);
        let start = run.tape.stream_starts()[i + 1];
        assert_no_click(&run.left(), switch, start, &format!("{mode}: open {name}"));
    }
    run.left()
}

/// Opening a patch gives no click, in every mode.
#[test]
fn no_click_on_open() {
    for mode in [OutputSafety::LowLatency, OutputSafety::Clean, OutputSafety::Off] {
        open_sine_fm_sine(mode);
    }
}

/// The same Open test twice gives the same tape.
#[test]
fn the_open_test_is_repeatable() {
    assert_eq!(
        open_sine_fm_sine(OutputSafety::LowLatency),
        open_sine_fm_sine(OutputSafety::LowLatency)
    );
}

/// New gives no click: the sine fades out to an empty patch, which starts
/// at 0 and stays silent.
#[test]
fn no_click_on_new() {
    let mut run = Setup::new("new", PatchFrom::Example("sine.ron")).start();
    run.hold();
    run.play(0.5);
    let switch = run.tape.len();
    assert!(run.app.replace(Source::Empty));
    run.play(0.2);
    let left = run.left();
    let start = run.tape.stream_starts()[1];
    assert_eq!(left[start - 1], 0.0, "the old stream ends at 0");
    let limit = step_limit(&left[switch - WINDOW..switch]);
    assert!(max_step(&left[switch - 1..start]) <= limit, "the fade-out has no click");
    assert!(left[start..].iter().all(|&x| x == 0.0), "the empty patch is silent");
}

/// Quitting gives no click: the sine fades out to 0 before the stream stops.
#[test]
fn no_click_on_quit() {
    let mut run = Setup::new("quit", PatchFrom::Example("sine.ron")).start();
    run.hold();
    run.play(0.5);
    let switch = run.tape.len();
    run.app.quit = true;
    let _ = run.app.update(Message::Tick);
    assert!(!run.tape.is_playing(), "the stream stopped");
    let left = run.left();
    assert!(left.len() > switch, "the fade-out was played");
    assert_eq!(*left.last().unwrap(), 0.0, "it ends at 0");
    let limit = step_limit(&left[switch - WINDOW..switch]);
    assert!(max_step(&left[switch - 1..]) <= limit, "the fade-out has no click");
}

/// Changing the device in the Preferences window gives no click.
#[test]
fn no_click_on_a_device_change() {
    let mut run = Setup::new("device", PatchFrom::Example("sine.ron")).start();
    run.hold();
    run.play(0.5);
    let switch = run.tape.len();
    run.app.preferences(Prefs::Open);
    run.app
        .preferences(Prefs::Audio(Change::Device(DeviceChoice::Named("Fake B".into()))));
    assert_eq!(run.tape.stream_starts().len(), 2, "{}", run.app.status);
    // The new engine starts the patch again; the note is held again.
    run.hold();
    run.play(0.5);
    let start = run.tape.stream_starts()[1];
    assert_no_click(&run.left(), switch, start, "device change");
}

/// A mode change from the Preferences window dips the output to 0 and back,
/// with no click.
#[test]
fn no_click_on_a_mode_change() {
    let mut run = Setup {
        flag: Some(OutputSafety::LowLatency),
        ..Setup::new("mode", PatchFrom::Example("sine.ron"))
    }
    .start();
    run.hold();
    run.play(0.5);
    let switch = run.tape.len();
    run.app.preferences(Prefs::Safety(OutputSafety::Clean));
    run.play(0.5);
    let left = run.left();
    let dip = &left[switch..switch + 3 * fade_frames()];
    assert!(dip.contains(&0.0), "the output dips to 0");
    let limit = step_limit(&left[switch - WINDOW..switch]).max(step_limit(&left[left.len() - WINDOW..]));
    let step = max_step(&left[switch - 1..switch + 3 * fade_frames()]);
    assert!(step <= limit, "mode change step {step} > {limit}");
    assert!(
        peak(&left[left.len() - WINDOW..]) > 0.5 * peak(&left[switch - WINDOW..switch]),
        "it plays again"
    );
}

/// `--record` keeps recording across two Opens until quit: the file holds
/// every stream, in order, with nothing between them (Part 2).
#[test]
fn record_covers_the_whole_run() {
    let mut run = Setup {
        record: Some("run.wav"),
        ..Setup::new("record", PatchFrom::Example("sine.ron"))
    }
    .start();
    run.hold();
    run.play(0.3);
    run.open("fm-feedback.ron");
    run.hold();
    run.play(0.3);
    run.open("sine.ron");
    run.play(0.3);
    run.app.quit = true;
    let _ = run.app.update(Message::Tick);
    assert_eq!(run.tape.stream_starts().len(), 3);
    let path = run.dir.join("run.wav");
    let tape = run.left();
    // Dropping the recorder closes the file, as closing the app does.
    let recorder = run.app.rig.recorder.take();
    assert!(recorder.as_ref().is_some_and(|r| r.dropped() == 0));
    drop(recorder);

    let audio = loom_test::analysis::read_wav(&path).unwrap();
    assert_eq!(audio.sample_rate, RATE as u32);
    assert_eq!(audio.channels[0], tape, "the file is what was played");
}

/// A device change to another sample rate ends the recording, with a
/// message; what was recorded stays readable.
#[test]
fn a_new_sample_rate_ends_the_recording() {
    let slow = OutputInfo {
        default_rate: 44_100,
        ..fake::device("Fake B")
    };
    let mut run = Setup {
        record: Some("run.wav"),
        devices: vec![fake::device("Fake A"), slow],
        ..Setup::new("record-rate", PatchFrom::Example("sine.ron"))
    }
    .start();
    run.play(0.2);
    let played = run.tape.len();
    run.app.preferences(Prefs::Open);
    run.app
        .preferences(Prefs::Audio(Change::Device(DeviceChoice::Named("Fake B".into()))));
    assert!(run.app.status.contains("The recording stopped"), "{}", run.app.status);
    assert!(run.app.rig.recorder.is_none());
    assert!(run.tape.is_playing(), "the new device plays");
    let audio = loom_test::analysis::read_wav(&run.dir.join("run.wav")).unwrap();
    assert_eq!(audio.sample_rate, RATE as u32);
    assert!(audio.frames() >= played, "the first stream is in the file");
}

/// A device at another sample rate that fails to start does not end the
/// recording: the old device plays again, and its new stream is recorded
/// into the same file.
#[test]
fn a_failed_device_change_keeps_the_recording() {
    let slow = OutputInfo {
        default_rate: 44_100,
        ..fake::device("Fake B")
    };
    let mut run = Setup {
        record: Some("run.wav"),
        devices: vec![fake::device("Fake A"), slow],
        wont_start: vec!["Fake B"],
        ..Setup::new("record-failed", PatchFrom::Example("sine.ron"))
    }
    .start();
    run.play(0.2);
    run.app.preferences(Prefs::Open);
    run.app
        .preferences(Prefs::Audio(Change::Device(DeviceChoice::Named("Fake B".into()))));
    assert!(
        run.app.status.contains("Still playing on \"Fake A\""),
        "{}",
        run.app.status
    );
    assert!(run.app.rig.recorder.is_some(), "the recording goes on");
    run.play(0.2);
    assert_eq!(run.tape.stream_starts().len(), 2, "Fake A plays again");
    run.app.quit = true;
    let _ = run.app.update(Message::Tick);
    drop(run.app.rig.recorder.take());
    let audio = loom_test::analysis::read_wav(&run.dir.join("run.wav")).unwrap();
    assert_eq!(audio.channels[0], run.left(), "both streams are in the file");
}
