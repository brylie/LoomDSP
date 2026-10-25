//! Checks on a real audio device. They need hardware, so they are ignored
//! by default: run them with
//! `cargo test -p loom-host-desktop -- --ignored --test-threads=1`, so no
//! two of them use a device at once. `LOOM_DEVICE` names an output
//! device by part of its name, and `LOOM_LOOPBACK` a loopback device. A
//! test whose variable is not set, or whose device is not on this
//! machine, prints `skipped:` and passes, so the run works on any
//! machine (docs/dev/test-devices.md). Each stream is silent, except in
//! the capture tests, which play a DC offset into the loopback device.
//!
//! In a file of their own, so coverage, measured on machines with no
//! audio device, leaves them out.

use super::*;
use loom_core::{EngineConfig, OutputSafety, create_engine};

/// The value of the environment variable `var`, or `None` after printing
/// why the test is skipped. An empty value counts as unset.
fn env_or_skip(var: &str, what: &str) -> Option<String> {
    let value = std::env::var(var).ok().filter(|v| !v.is_empty());
    if value.is_none() {
        eprintln!("skipped: set {var} to {what} (docs/dev/test-devices.md)");
    }
    value
}

/// `result`'s value, or `None` after printing why the test is skipped
/// when the hardware is not on this machine. Any other error fails the
/// test.
fn or_skip<T>(result: Result<T, HostError>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(e) if e.is_missing_hardware() => {
            eprintln!("skipped: {e} (docs/dev/test-devices.md)");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

/// Plays a silent engine on `options` for 0.7 s and returns the
/// callbacks the device made, or `None` when the device is not here.
fn callbacks(options: &AudioOptions) -> Option<u64> {
    let output = or_skip(AudioOutput::open(options))?;
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(output.sample_rate() as f32)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    let audio = output.start(parts.engine).unwrap();
    std::thread::sleep(Duration::from_millis(700));
    let made = audio.stats().snapshot().callbacks;
    parts.controller.fade_out().unwrap();
    audio.stop(STOP_LIMIT);
    Some(made)
}

/// Plays `examples/dev/check-dc.ron` (a Constant of 0.5, output safety
/// Off) into the loopback device named by `LOOM_LOOPBACK` with buffers
/// of [`LOOPBACK_BUFFER`] frames at [`LOOPBACK_RATE`], captures the loopback's input side, and stops
/// the stream with `stop` after a fade-out request. `stop` returns true if
/// the stream stopped on the engine's silence, false if it was cut at the
/// limit; the answer is printed, so a failed check says which it was.
/// Returns the captured left channel and the sample rate, or `None` when
/// `LOOM_LOOPBACK` is not set.
fn capture_a_stop(stop: impl FnOnce(RunningAudio) -> bool) -> Option<(Vec<f32>, u32)> {
    // Two tests playing into one loopback at once would hear each
    // other, so one capture runs at a time. A failed test poisons the
    // lock; the next one still runs.
    static LOOPBACK: Mutex<()> = Mutex::new(());
    let _one_at_a_time = LOOPBACK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let loopback = env_or_skip("LOOM_LOOPBACK", "part of a loopback device's name")?;
    let options = AudioOptions {
        device: Some(loopback),
        exact: false,
        sample_rate: Some(LOOPBACK_RATE),
        buffer_size: Some(LOOPBACK_BUFFER),
    };
    let output = or_skip(AudioOutput::open(&options))?;
    let rate = output.sample_rate();
    let dir = record::TestDir::new(&format!("capture-{:?}", std::thread::current().id()));
    let path = dir.0.join("capture.wav");
    let input = AudioOptions {
        sample_rate: Some(rate),
        buffer_size: None,
        ..options
    };
    let capture = or_skip(Capture::start(&input, &path))?;
    // Some inputs take seconds to deliver their first frames (the ALSA
    // plugin for PulseAudio takes about 2 s), and the capture would
    // miss the stream's start or all of it.
    let waited = Instant::now();
    while capture.frames() == 0 {
        assert!(
            waited.elapsed() < Duration::from_secs(10),
            "the loopback's input delivered frames"
        );
        std::thread::sleep(Duration::from_millis(1));
    }

    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(rate as f32)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    let patch = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/dev/check-dc.ron");
    loom_patch::Patch::load(patch)
        .unwrap()
        .load_into(&mut parts.controller)
        .unwrap();
    let audio = output.start(parts.engine).unwrap();
    let stats = audio.stats().clone();
    std::thread::sleep(Duration::from_millis(600));
    parts.controller.fade_out().unwrap();
    let silent = stop(audio);
    eprintln!("stopped on the engine's silence: {silent}");
    // Long enough for the device to play out what it was handed.
    std::thread::sleep(Duration::from_millis(400));
    let report = capture.stop().unwrap();
    // A dropout with no error on either side happened in the device
    // (for example the PipeWire graph), not in LoomDSP's callbacks.
    let s = stats.snapshot();
    eprintln!(
        "output: {} callbacks, {} overruns, {} stream errors, {} start overloads; capture: {} frames, {} stream errors",
        s.callbacks, s.overruns, s.stream_errors, s.start_overloads, report.frames, report.stream_errors
    );
    assert_eq!(report.dropped, 0, "no frame was dropped");

    let mut reader = hound::WavReader::open(&path).unwrap();
    let left = reader.samples::<f32>().step_by(2).map(Result::unwrap).collect();
    Some((left, rate))
}

/// The level the capture holds, and the largest step of the stop: from
/// the last sample at that level on, where the fade must be. The
/// loopback's volume scales the level, so checks are relative to it.
///
/// A step inside the stream before that is a dropout, not the stop: on
/// a busy CI runner, PipeWire sometimes delivers one 512-frame quantum
/// of silence and then the level again (#361). It is printed, with
/// where it is, but does not count as the stop's step.
fn level_and_step(samples: &[f32]) -> (f32, f32) {
    let level = samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let held = samples.iter().position(|x| x.abs() >= level).unwrap_or(0);
    let last = samples.iter().rposition(|x| x.abs() >= level).unwrap_or(0);
    let (dropout_at, dropout) = largest_step(samples, held, last);
    if dropout > 0.0 {
        let back = samples[dropout_at + 1..=last]
            .iter()
            .position(|x| x.abs() >= level)
            .map_or(last, |i| dropout_at + 1 + i);
        eprintln!(
            "dropout inside the stream: {} -> {} at frame {dropout_at}, back at the level at frame {back}",
            samples[dropout_at],
            samples[dropout_at + 1]
        );
    }
    let (at, step) = largest_step(samples, last, samples.len().saturating_sub(1));
    eprintln!(
        "captured level {level}, held from frame {held} to {last}; the stop's largest step {step}: {} -> {} at frame {at} of {}",
        samples.get(at).copied().unwrap_or(0.0),
        samples.get(at + 1).copied().unwrap_or(0.0),
        samples.len()
    );
    assert!(
        level > 0.01,
        "the loopback delivered sound (level {level}). On macOS the terminal needs microphone access."
    );
    (level, step)
}

/// The largest step between two neighbouring samples from frame `from`
/// to frame `to`, and the frame it starts at. Zero when the range has
/// fewer than two samples.
fn largest_step(samples: &[f32], from: usize, to: usize) -> (usize, f32) {
    samples
        .get(from..=to)
        .unwrap_or(&[])
        .windows(2)
        .enumerate()
        .fold((from, 0.0f32), |(at, m), (i, w)| {
            let d = (w[1] - w[0]).abs();
            if d > m { (from + i, d) } else { (at, m) }
        })
}

/// The stop's step ignores a dropout inside the stream but still sees a
/// cut stop. The shapes are those of the CI capture in #361: a 512-frame
/// gap of silence, the level again, then a 480-frame fade.
#[test]
fn the_stop_step_ignores_a_dropout_inside_the_stream() {
    let fade: Vec<f32> = (0..480).map(|i| 0.5 * (1.0 - i as f32 / 480.0)).collect();
    let stream = |gap: bool, end: &[f32]| {
        let mut s = vec![0.0f32; 100];
        s.extend(std::iter::repeat_n(0.5, 1000));
        if gap {
            s.extend(std::iter::repeat_n(0.0, 512));
        }
        s.extend(std::iter::repeat_n(0.5, 1000));
        s.extend_from_slice(end);
        s.extend(std::iter::repeat_n(0.0, 200));
        s
    };
    let (level, step) = level_and_step(&stream(true, &fade));
    assert_eq!(level, 0.5);
    assert!(step <= fade_step(level, 48_000), "{step}");

    let (_, step) = level_and_step(&stream(true, &[]));
    assert_eq!(step, 0.5, "a cut stop is still a step of the whole level");
}

/// The largest step a 10 ms fade from `level` may take at `rate`, with
/// a margin of 2.
fn fade_step(level: f32, rate: u32) -> f32 {
    2.0 * level / (loom_core::safety::FADE_SECONDS * rate as f32)
}

/// The sample rate of the loopback tests. With [`LOOPBACK_BUFFER`],
/// a buffer is 85 ms long, as when Open clicked in #26. A fixed 100 ms
/// stop limit cut the fade only when buffers were that long in time:
/// at 96 kHz, 4096 frames are short enough for the fade to get through,
/// and devices often offer no larger buffer. The tests leave the
/// loopback at this rate.
const LOOPBACK_RATE: u32 = 48_000;

/// The buffer size of the loopback tests, in frames.
const LOOPBACK_BUFFER: u32 = 4096;

/// Stops tried by each loopback test. Where a stop lands between two
/// device callbacks changes from run to run, so one stop proves little.
const STOPS: usize = 5;

/// A stop with long buffers fades out on the device: in every
/// one of [`STOPS`] tries, the capture of a loopback has no step larger
/// than the 10 ms fade. With a fixed 100 ms stop limit the stream was
/// cut before the fade reached the device (#26), which clicked. Needs
/// `LOOM_LOOPBACK`.
#[test]
#[ignore = "needs a loopback audio device"]
fn a_stop_with_large_buffers_fades_out_on_the_device() {
    for _ in 0..STOPS {
        let Some((samples, rate)) = capture_a_stop(|audio| audio.stop(STOP_LIMIT)) else {
            return;
        };
        let (level, step) = level_and_step(&samples);
        let limit = fade_step(level, rate);
        assert!(
            step <= limit,
            "largest step {step} is over the fade's {limit} (level {level})"
        );
    }
}

/// The check above can fail: stopped at a fixed 100 ms limit, as
/// before the #26 fix, the capture shows the cut as a step of about
/// the whole level. The cut depends on where the stop lands between
/// callbacks, so it is not in every stop: at least one of [`STOPS`]
/// tries shows it. Needs `LOOM_LOOPBACK`.
#[test]
#[ignore = "needs a loopback audio device"]
fn a_fixed_100_ms_stop_limit_shows_as_a_cut() {
    let mut steps = Vec::new();
    for _ in 0..STOPS {
        let Some((samples, rate)) = capture_a_stop(|audio| audio.stop_within(|_| STOP_LIMIT)) else {
            return;
        };
        let (level, step) = level_and_step(&samples);
        if step > fade_step(level, rate) {
            return;
        }
        steps.push(step);
    }
    panic!("no stop showed the cut: largest steps {steps:?}");
}

/// A stream at a new sample rate keeps playing. On macOS the notice of
/// the rate switch used to pause the new stream, and the app went
/// silent after a rate change in Preferences.
#[test]
#[ignore = "needs an audio device"]
fn a_new_sample_rate_keeps_playing() {
    let Some(device) = env_or_skip("LOOM_DEVICE", "part of an output device's name") else {
        return;
    };
    for rate in [44_100, 48_000, 96_000, 44_100] {
        let options = AudioOptions {
            device: Some(device.clone()),
            exact: false,
            sample_rate: Some(rate),
            buffer_size: Some(4096),
        };
        let Some(made) = callbacks(&options) else { return };
        assert!(made > 0, "the stream at {rate} Hz plays");
    }
}

/// Only errors that mean the hardware is not on this machine skip a test.
/// A device that is here but refuses what is asked fails it.
#[test]
fn only_missing_hardware_skips_a_test() {
    assert!(HostError::NoOutputDevice(Some("MOTU".into())).is_missing_hardware());
    assert!(HostError::NoInputDevice(None).is_missing_hardware());
    assert!(HostError::NoMidiPort(None).is_missing_hardware());
    assert!(HostError::NoMidiService("no ALSA sequencer".into()).is_missing_hardware());
    let rate = HostError::UnsupportedSampleRate {
        device: "MOTU".into(),
        rate: 96_000,
    };
    assert!(!rate.is_missing_hardware());
    assert!(!HostError::Audio("stream failed".into()).is_missing_hardware());
    assert!(!HostError::Midi("send failed".into()).is_missing_hardware());
}
